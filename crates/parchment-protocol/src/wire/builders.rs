use base64::Engine as _;
use parchment_model::PromptAttachment;

use super::*;

const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

fn normalized_mime(att: &PromptAttachment) -> &str {
    let t = att.mime_type.trim();
    if t.is_empty() {
        "application/octet-stream"
    } else {
        t
    }
}

fn format_inline_text_attachment(att: &PromptAttachment) -> Option<String> {
    if att.text_content.trim().is_empty() {
        return None;
    }
    let name = if att.file_name.trim().is_empty() {
        "attachment.txt"
    } else {
        att.file_name.trim()
    };
    Some(format!("[Attached file: {name}]\n{}", att.text_content))
}

fn load_attachment_base64(att: &PromptAttachment) -> Option<Cow<'_, str>> {
    let inline = att.base64_data.trim();
    if !inline.is_empty() {
        return Some(Cow::Borrowed(inline));
    }
    let path = att.local_file_path.trim();
    if path.is_empty() {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() == 0 || meta.len() > MAX_ATTACHMENT_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Cow::Owned(
        base64::engine::general_purpose::STANDARD.encode(bytes),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_openai_responses_body(
    profile: &ProviderProfile,
    model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    prev_response_id: &str,
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(profile.keeps_provider_state()));

    if !system_prompt.is_empty() {
        body.insert("instructions".into(), json!(system_prompt));
    }
    if !prev_response_id.is_empty() {
        body.insert("previous_response_id".into(), json!(prev_response_id));
    }
    if let Some(max_out) = profile.max_output_tokens
        && max_out > 0
    {
        body.insert("max_output_tokens".into(), json!(max_out));
    }

    if let Some(effort) = effective_reasoning_effort(profile, adapt) {
        body.insert(
            "reasoning".into(),
            json!({ "effort": effort, "summary": "auto" }),
        );
        if prev_response_id.is_empty() {
            body.insert("include".into(), json!(["reasoning.encrypted_content"]));
        }
    } else if !adapt.strip_sampling {
        if let Some(t) = profile.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(p) = profile.top_p {
            body.insert("top_p".into(), json!(p));
        }
    }

    // Prompt-cache invariant: `tools` MUST appear before `input`.
    let mut tools_arr: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters_schema,
            })
        })
        .collect();
    if !adapt.omit_builtin_tools {
        if profile.enable_web_search {
            tools_arr.push(json!({ "type": "web_search_preview" }));
        }
        if profile.enable_code_execution {
            tools_arr.push(json!({
                "type": "code_interpreter",
                "container": { "type": "auto" }
            }));
        }
    }
    if !tools_arr.is_empty() {
        body.insert("tools".into(), Value::Array(tools_arr));
    }
    let mut ids = ReplayToolCallIds::new();
    let mut input = Vec::new();
    let last_asst_idx = nodes.iter().rposition(|n| n.role == MessageRole::Assistant);

    for (node_idx, node) in nodes.iter().enumerate() {
        let Some(v) = node.active_variant() else {
            continue;
        };
        let clean = strip_inline_error_notices(&v.content);
        match node.role {
            MessageRole::User | MessageRole::System => {
                let role = if node.role == MessageRole::System {
                    "developer"
                } else {
                    "user"
                };
                if node.role == MessageRole::User && !v.attachments.is_empty() {
                    let mut parts = Vec::new();
                    if !clean.is_empty() {
                        parts.push(json!({ "type": "input_text", "text": clean }));
                    }
                    for att in &v.attachments {
                        if let Some(inline) = format_inline_text_attachment(att) {
                            parts.push(json!({ "type": "input_text", "text": inline }));
                        } else if let Some(b64) = load_attachment_base64(att) {
                            let mime = normalized_mime(att);
                            if mime.starts_with("image/") {
                                parts.push(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{mime};base64,{b64}")
                                }));
                            } else {
                                parts.push(json!({
                                    "type": "input_file",
                                    "filename": att.file_name,
                                    "file_data": format!("data:{mime};base64,{b64}")
                                }));
                            }
                        }
                    }
                    if parts.is_empty() {
                        parts.push(json!({ "type": "input_text", "text": "" }));
                    }
                    input.push(json!({ "role": role, "content": parts }));
                } else {
                    input.push(json!({ "role": role, "content": clean }));
                }
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let born_dialect = effective_wire_dialect(v);
                if is_valid_openai_encrypted_reasoning(
                    &v.thinking_signature,
                    v.provider_protocol,
                    &v.model_used,
                    model,
                    &born_dialect,
                ) {
                    input.push(json!({
                        "type": "reasoning",
                        "summary": if v.thinking_summary.is_empty() { json!([]) } else { json!([{ "type": "summary_text", "text": v.thinking_summary }]) },
                        "encrypted_content": v.thinking_signature,
                    }));
                }
                let total_calls = v.tool_calls.len();
                for (tc_idx, tc) in v.tool_calls.iter().enumerate() {
                    if !tc.preceding_text.trim().is_empty() {
                        input.push(json!({ "role": "assistant", "content": tc.preceding_text }));
                    }
                    let cid = ids.resolve(&tc.id, "call");
                    input.push(json!({
                        "type": "function_call",
                        "call_id": cid,
                        "name": tc.name,
                        "arguments": if tc.arguments_json.trim().is_empty() { "{}" } else { tc.arguments_json.as_str() },
                    }));
                    let out = compact_historical_tool_result_for_wire(
                        &tc.result_text,
                        &tc.name,
                        is_older,
                        tc_idx,
                        total_calls,
                    );
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": cid,
                        "output": out,
                    }));
                }
                if !clean.is_empty() || v.tool_calls.is_empty() {
                    input.push(json!({ "role": "assistant", "content": clean }));
                }
            }
        }
    }
    if !late_note.is_empty() {
        input.push(json!({ "role": "developer", "content": late_note }));
    }
    if !continue_prefix.is_empty() && prev_response_id.is_empty() {
        input.push(json!({ "role": "assistant", "content": continue_prefix }));
    }
    body.insert("input".into(), Value::Array(input));
    body
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_openai_chat_body(
    profile: &ProviderProfile,
    model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("stream".into(), json!(true));
    if !adapt.omit_stream_options {
        body.insert("stream_options".into(), json!({ "include_usage": true }));
    }

    let effort = effective_reasoning_effort(profile, adapt);
    if !adapt.strip_sampling {
        if let Some(t) = profile.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(p) = profile.top_p {
            body.insert("top_p".into(), json!(p));
        }
    }
    if let Some(max_out) = profile.max_output_tokens
        && max_out > 0
    {
        let use_legacy_max_tokens = adapt.use_max_tokens_key.unwrap_or(effort.is_none());
        let key = if use_legacy_max_tokens {
            "max_tokens"
        } else {
            "max_completion_tokens"
        };
        body.insert(key.into(), json!(max_out));
    }
    if let Some(eff) = effort {
        body.insert("reasoning_effort".into(), json!(eff));
    }

    // Prompt-cache invariant: `tools` MUST appear before `messages`.
    if !tools.is_empty() {
        let arr: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters_schema,
                    }
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(arr));
    }

    let mut ids = ReplayToolCallIds::new();
    let mut messages = Vec::new();
    if !system_prompt.is_empty() {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    let last_asst_idx = nodes.iter().rposition(|n| n.role == MessageRole::Assistant);
    for (node_idx, node) in nodes.iter().enumerate() {
        let Some(v) = node.active_variant() else {
            continue;
        };
        let clean = strip_inline_error_notices(&v.content);
        match node.role {
            MessageRole::User => {
                if !v.attachments.is_empty() {
                    let mut parts = Vec::new();
                    if !clean.is_empty() {
                        parts.push(json!({ "type": "text", "text": clean }));
                    }
                    for att in &v.attachments {
                        if let Some(inline) = format_inline_text_attachment(att) {
                            parts.push(json!({ "type": "text", "text": inline }));
                        } else if let Some(b64) = load_attachment_base64(att) {
                            let mime = normalized_mime(att);
                            if mime.starts_with("image/") {
                                parts.push(json!({
                                    "type": "image_url",
                                    "image_url": { "url": format!("data:{mime};base64,{b64}") }
                                }));
                            } else {
                                parts.push(json!({
                                    "type": "file",
                                    "file": {
                                        "filename": att.file_name,
                                        "file_data": format!("data:{mime};base64,{b64}")
                                    }
                                }));
                            }
                        }
                    }
                    if parts.is_empty() {
                        parts.push(json!({ "type": "text", "text": "" }));
                    }
                    messages.push(json!({ "role": "user", "content": parts }));
                } else {
                    messages.push(json!({ "role": "user", "content": clean }));
                }
            }
            MessageRole::System => messages.push(json!({ "role": "system", "content": clean })),
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let replay_reasoning = !v.thinking_summary.trim().is_empty()
                    && is_native_reasoning_compatible(
                        v,
                        ProviderProtocol::OpenAiResponses,
                        CUSTOM_DIALECT_OPENAI_CHAT,
                        model,
                    );
                if v.tool_calls.is_empty() {
                    let mut msg = Map::new();
                    msg.insert("role".into(), json!("assistant"));
                    msg.insert("content".into(), json!(clean));
                    if replay_reasoning {
                        msg.insert("reasoning_content".into(), json!(v.thinking_summary));
                    }
                    messages.push(Value::Object(msg));
                } else {
                    // Group sequential tool steps via `starts_tool_step`.
                    let total_calls = v.tool_calls.len();
                    let mut step_start = 0usize;
                    while step_start < total_calls {
                        let mut step_end = step_start + 1;
                        while step_end < total_calls && !v.tool_calls[step_end].starts_tool_step {
                            step_end += 1;
                        }
                        let mut calls_json = Vec::new();
                        let mut results_json = Vec::new();
                        for (tc_idx, tc) in v.tool_calls[step_start..step_end].iter().enumerate() {
                            let abs_idx = step_start + tc_idx;
                            let cid = ids.resolve(&tc.id, "call");
                            calls_json.push(json!({
                                "id": cid,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": if tc.arguments_json.trim().is_empty() { "{}" } else { tc.arguments_json.as_str() },
                                }
                            }));
                            let out = compact_historical_tool_result_for_wire(
                                &tc.result_text,
                                &tc.name,
                                is_older,
                                abs_idx,
                                total_calls,
                            );
                            results_json.push(json!({
                                "role": "tool",
                                "tool_call_id": cid,
                                "content": out,
                            }));
                        }
                        let mut asst_msg = Map::new();
                        asst_msg.insert("role".into(), json!("assistant"));
                        let lead = v.tool_calls[step_start].preceding_text.trim();
                        asst_msg.insert(
                            "content".into(),
                            if lead.is_empty() {
                                Value::Null
                            } else {
                                json!(lead)
                            },
                        );
                        if step_start == 0 && replay_reasoning {
                            asst_msg.insert("reasoning_content".into(), json!(v.thinking_summary));
                        }
                        asst_msg.insert("tool_calls".into(), Value::Array(calls_json));
                        messages.push(Value::Object(asst_msg));
                        messages.extend(results_json);
                        step_start = step_end;
                    }
                    if !clean.is_empty() {
                        messages.push(json!({ "role": "assistant", "content": clean }));
                    }
                }
            }
        }
    }
    if !late_note.is_empty() {
        messages.push(json!({ "role": "system", "content": late_note }));
    }
    if !continue_prefix.is_empty() {
        messages.push(json!({ "role": "assistant", "content": continue_prefix }));
    }
    body.insert("messages".into(), Value::Array(messages));
    body
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_anthropic_body(
    profile: &ProviderProfile,
    model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("stream".into(), json!(true));

    let think_budget = profile.thinking_budget_tokens.unwrap_or(0);
    let effort = effective_reasoning_effort(profile, adapt);
    let use_budget = effort.is_some() && think_budget >= 1024;
    let use_adaptive = effort.is_some() && !use_budget;

    let resolved_max = profile
        .max_output_tokens
        .filter(|&v| v > 0)
        .unwrap_or(64_000);
    let max_tokens = if use_budget {
        resolved_max.max(think_budget + 4096)
    } else {
        resolved_max
    };
    body.insert("max_tokens".into(), json!(max_tokens));

    if use_adaptive {
        let mut thinking = Map::new();
        thinking.insert("type".into(), json!("adaptive"));
        if !adapt.omit_anthropic_adaptive_display {
            thinking.insert("display".into(), json!("summarized"));
        }
        body.insert("thinking".into(), Value::Object(thinking));
        if let Some(eff) = effort {
            let mapped = if eff == "minimal" {
                "low"
            } else {
                eff.as_str()
            };
            if mapped != "auto" && mapped != "default" {
                body.insert("output_config".into(), json!({ "effort": mapped }));
            }
        }
    } else if use_budget {
        body.insert(
            "thinking".into(),
            json!({ "type": "enabled", "budget_tokens": think_budget }),
        );
    } else {
        let raw_eff = profile.thinking_effort.trim().to_ascii_lowercase();
        if !adapt.omit_reasoning && matches!(raw_eff.as_str(), "none" | "off") {
            body.insert("thinking".into(), json!({ "type": "disabled" }));
        }
        if !adapt.strip_sampling {
            if let Some(t) = profile.temperature {
                body.insert("temperature".into(), json!(t));
            } else if let Some(p) = profile.top_p {
                body.insert("top_p".into(), json!(p));
            }
        }
    }

    if !system_prompt.is_empty() {
        body.insert(
            "system".into(),
            json!([{
                "type": "text",
                "text": system_prompt,
                "cache_control": { "type": "ephemeral" }
            }]),
        );
    }

    // Prompt-cache invariant: `tools` MUST appear before `messages`, with cache_control on the last tool.
    let mut tools_arr: Vec<Value> = tools
        .iter()
        .map(|t| {
            let mut obj = Map::new();
            obj.insert("name".into(), json!(t.name));
            obj.insert("description".into(), json!(t.description));
            obj.insert("input_schema".into(), t.parameters_schema.clone());
            Value::Object(obj)
        })
        .collect();
    if !adapt.omit_builtin_tools && profile.enable_web_search {
        tools_arr.push(json!({
            "type": "web_search_20250305",
            "name": "web_search"
        }));
    }
    if let Some(Value::Object(last_tool)) = tools_arr.last_mut() {
        last_tool.insert("cache_control".into(), json!({ "type": "ephemeral" }));
    }
    if !tools_arr.is_empty() {
        body.insert("tools".into(), Value::Array(tools_arr));
    }
    let mut ids = ReplayToolCallIds::new();
    let mut messages = Vec::new();
    let last_asst_idx = nodes.iter().rposition(|n| n.role == MessageRole::Assistant);

    for (node_idx, node) in nodes.iter().enumerate() {
        let Some(v) = node.active_variant() else {
            continue;
        };
        let clean = strip_inline_error_notices(&v.content);
        match node.role {
            MessageRole::User | MessageRole::System => {
                if node.role == MessageRole::User && !v.attachments.is_empty() {
                    let mut blocks = Vec::new();
                    if !clean.is_empty() {
                        blocks.push(json!({ "type": "text", "text": clean }));
                    }
                    for att in &v.attachments {
                        if let Some(inline) = format_inline_text_attachment(att) {
                            blocks.push(json!({ "type": "text", "text": inline }));
                        } else if let Some(b64) = load_attachment_base64(att) {
                            let mime = normalized_mime(att);
                            if mime.starts_with("image/") {
                                blocks.push(json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": mime,
                                        "data": b64
                                    }
                                }));
                            } else if mime.eq_ignore_ascii_case("application/pdf") {
                                blocks.push(json!({
                                    "type": "document",
                                    "source": {
                                        "type": "base64",
                                        "media_type": "application/pdf",
                                        "data": b64
                                    }
                                }));
                            }
                        }
                    }
                    if blocks.is_empty() {
                        blocks.push(json!({ "type": "text", "text": "" }));
                    }
                    messages.push(json!({ "role": "user", "content": blocks }));
                } else {
                    messages.push(json!({ "role": "user", "content": clean }));
                }
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let valid_thinking = !v.thinking_summary.is_empty()
                    && is_valid_anthropic_signature(
                        &v.thinking_signature,
                        v.provider_protocol,
                        &v.model_used,
                        model,
                    );
                if v.tool_calls.is_empty() {
                    let mut blocks = Vec::new();
                    if valid_thinking {
                        blocks.push(json!({
                            "type": "thinking",
                            "thinking": v.thinking_summary,
                            "signature": v.thinking_signature,
                        }));
                    }
                    blocks.push(json!({ "type": "text", "text": clean }));
                    messages.push(json!({ "role": "assistant", "content": blocks }));
                } else {
                    let total_calls = v.tool_calls.len();
                    let mut step_start = 0usize;
                    while step_start < total_calls {
                        let mut step_end = step_start + 1;
                        while step_end < total_calls && !v.tool_calls[step_end].starts_tool_step {
                            step_end += 1;
                        }
                        let mut blocks = Vec::new();
                        if step_start == 0 && valid_thinking {
                            blocks.push(json!({
                                "type": "thinking",
                                "thinking": v.thinking_summary,
                                "signature": v.thinking_signature,
                            }));
                        }
                        let mut tool_results = Vec::new();
                        for (rel_idx, tc) in v.tool_calls[step_start..step_end].iter().enumerate() {
                            let abs_idx = step_start + rel_idx;
                            if !tc.preceding_text.trim().is_empty() {
                                blocks.push(json!({ "type": "text", "text": tc.preceding_text }));
                            }
                            let cid = ids.resolve(&tc.id, "toolu");
                            let input_val: Value = serde_json::from_str(&tc.arguments_json)
                                .unwrap_or_else(|_| json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": cid,
                                "name": tc.name,
                                "input": input_val,
                            }));
                            let out = compact_historical_tool_result_for_wire(
                                &tc.result_text,
                                &tc.name,
                                is_older,
                                abs_idx,
                                total_calls,
                            );
                            tool_results.push(json!({
                                "type": "tool_result",
                                "tool_use_id": cid,
                                "content": out,
                                "is_error": tc.is_error,
                            }));
                        }
                        messages.push(json!({ "role": "assistant", "content": blocks }));
                        messages.push(json!({ "role": "user", "content": tool_results }));
                        step_start = step_end;
                    }
                    if !clean.is_empty() {
                        messages.push(json!({
                            "role": "assistant",
                            "content": [{ "type": "text", "text": clean }]
                        }));
                    }
                }
            }
        }
    }
    if !late_note.is_empty() {
        messages.push(json!({ "role": "user", "content": late_note }));
    }
    if !continue_prefix.is_empty() {
        messages.push(json!({ "role": "assistant", "content": continue_prefix }));
    }
    body.insert("messages".into(), Value::Array(messages));
    body
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_gemini_interactions_body(
    profile: &ProviderProfile,
    model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    prev_interaction_id: &str,
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    let clean_model = model.trim().trim_prefix("models/");
    body.insert("model".into(), json!(clean_model));
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(profile.keeps_provider_state()));
    if profile.keeps_provider_state() && profile.use_background_long_decode {
        body.insert("background".into(), json!(true));
    }
    if !prev_interaction_id.is_empty() {
        body.insert("previous_interaction_id".into(), json!(prev_interaction_id));
    }
    if !system_prompt.is_empty() {
        body.insert("system_instruction".into(), json!(system_prompt));
    }

    let mut gen_cfg = Map::new();
    if let Some(max_out) = profile.max_output_tokens
        && max_out > 0
    {
        gen_cfg.insert("max_output_tokens".into(), json!(max_out));
    }
    if let Some(eff) = effective_reasoning_effort(profile, adapt) {
        gen_cfg.insert("thinking_level".into(), json!(eff));
        gen_cfg.insert("thinking_summaries".into(), json!("auto"));
    } else if !adapt.strip_sampling {
        if let Some(t) = profile.temperature {
            gen_cfg.insert("temperature".into(), json!(t));
        }
        if let Some(p) = profile.top_p {
            gen_cfg.insert("top_p".into(), json!(p));
        }
    }
    if !gen_cfg.is_empty() {
        body.insert("generation_config".into(), Value::Object(gen_cfg));
    }

    // Prompt-cache invariant: `tools` MUST appear before `input`.
    let mut tools_arr: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "name": t.name,
                "description": t.description,
                "parameters": t.parameters_schema,
            })
        })
        .collect();
    if !adapt.omit_builtin_tools {
        if profile.enable_web_search {
            tools_arr.push(json!({ "type": "google_search" }));
        }
        if profile.enable_code_execution {
            tools_arr.push(json!({ "type": "code_execution" }));
        }
        if profile.enable_url_context {
            tools_arr.push(json!({ "type": "url_context" }));
        }
    }
    if !tools_arr.is_empty() {
        body.insert("tools".into(), Value::Array(tools_arr));
    }
    let mut ids = ReplayToolCallIds::new();
    let mut input = Vec::new();
    let last_asst_idx = nodes.iter().rposition(|n| n.role == MessageRole::Assistant);
    for (node_idx, node) in nodes.iter().enumerate() {
        let Some(v) = node.active_variant() else {
            continue;
        };
        let clean = strip_inline_error_notices(&v.content);
        match node.role {
            MessageRole::User | MessageRole::System => {
                let mut parts = Vec::new();
                if !clean.is_empty() || v.attachments.is_empty() {
                    parts.push(json!({ "type": "text", "text": clean }));
                }
                for att in &v.attachments {
                    if let Some(inline) = format_inline_text_attachment(att) {
                        parts.push(json!({ "type": "text", "text": inline }));
                    } else if let Some(b64) = load_attachment_base64(att) {
                        let mime = normalized_mime(att);
                        let kind = if mime.starts_with("image/") {
                            "image"
                        } else if mime.starts_with("audio/") {
                            "audio"
                        } else if mime.starts_with("video/") {
                            "video"
                        } else {
                            "document"
                        };
                        parts.push(json!({
                            "type": kind,
                            "data": b64,
                            "mime_type": mime
                        }));
                    }
                }
                input.push(json!({ "role": "user", "content": parts }));
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let turn_sig = sanitize_gemini_thought_signature(&v.thinking_signature, false);
                if v.tool_calls.is_empty() {
                    let mut content_blocks = Vec::new();
                    if !turn_sig.is_empty() {
                        content_blocks.push(json!({ "type": "thought", "signature": turn_sig }));
                    }
                    content_blocks.push(json!({ "type": "text", "text": clean }));
                    input.push(json!({ "role": "model", "content": content_blocks }));
                } else {
                    let total_calls = v.tool_calls.len();
                    let mut step_start = 0usize;
                    while step_start < total_calls {
                        let mut step_end = step_start + 1;
                        while step_end < total_calls && !v.tool_calls[step_end].starts_tool_step {
                            step_end += 1;
                        }
                        let mut content_blocks = Vec::new();
                        if step_start == 0 && !turn_sig.is_empty() {
                            content_blocks
                                .push(json!({ "type": "thought", "signature": turn_sig }));
                        }
                        let mut result_blocks = Vec::new();
                        for (rel_idx, tc) in v.tool_calls[step_start..step_end].iter().enumerate() {
                            let abs_idx = step_start + rel_idx;
                            let cid = ids.resolve(&tc.id, "call");
                            let fn_sig =
                                sanitize_gemini_thought_signature(&tc.thought_signature, true);
                            if !fn_sig.is_empty() {
                                content_blocks
                                    .push(json!({ "type": "thought", "signature": fn_sig }));
                            }
                            if !tc.preceding_text.trim().is_empty() {
                                content_blocks
                                    .push(json!({ "type": "text", "text": tc.preceding_text }));
                            }
                            let args: Value = serde_json::from_str(&tc.arguments_json)
                                .unwrap_or_else(|_| json!({}));
                            content_blocks.push(json!({
                                "type": "function_call",
                                "id": cid,
                                "name": tc.name,
                                "arguments": args,
                            }));
                            let out = compact_historical_tool_result_for_wire(
                                &tc.result_text,
                                &tc.name,
                                is_older,
                                abs_idx,
                                total_calls,
                            );
                            result_blocks.push(json!({
                                "type": "function_result",
                                "call_id": cid,
                                "name": tc.name,
                                "result": out,
                            }));
                        }
                        input.push(json!({ "role": "model", "content": content_blocks }));
                        input.push(json!({ "role": "user", "content": result_blocks }));
                        step_start = step_end;
                    }
                    if !clean.is_empty() {
                        input.push(json!({
                            "role": "model",
                            "content": [{ "type": "text", "text": clean }]
                        }));
                    }
                }
            }
        }
    }
    if !late_note.is_empty() {
        input.push(json!({
            "role": "user",
            "content": [{ "type": "text", "text": late_note }]
        }));
    }
    if !continue_prefix.is_empty() && prev_interaction_id.is_empty() {
        input.push(json!({
            "role": "model",
            "content": [{ "type": "text", "text": continue_prefix }]
        }));
    }
    body.insert("input".into(), Value::Array(input));
    body
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_gemini_generate_content_body(
    profile: &ProviderProfile,
    _model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    if !system_prompt.is_empty() {
        body.insert(
            "systemInstruction".into(),
            json!({ "parts": [{ "text": system_prompt }] }),
        );
    }

    // Prompt-cache invariant: `tools` MUST appear before `contents`.
    let mut tools_arr = Vec::new();
    if !tools.is_empty() {
        let decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters_schema,
                })
            })
            .collect();
        tools_arr.push(json!({ "function_declarations": decls }));
    }
    if !adapt.omit_builtin_tools {
        if profile.enable_web_search {
            tools_arr.push(json!({ "google_search": {} }));
        }
        if profile.enable_code_execution {
            tools_arr.push(json!({ "code_execution": {} }));
        }
        if profile.enable_url_context {
            tools_arr.push(json!({ "url_context": {} }));
        }
    }
    if !tools_arr.is_empty() {
        body.insert("tools".into(), Value::Array(tools_arr));
    }

    let mut gen_cfg = Map::new();
    if let Some(max_out) = profile.max_output_tokens
        && max_out > 0
    {
        gen_cfg.insert("maxOutputTokens".into(), json!(max_out));
    }
    if let Some(eff) = effective_reasoning_effort(profile, adapt) {
        let explicit_budget = profile.thinking_budget_tokens.filter(|&b| b > 0);
        if explicit_budget.is_some() || adapt.use_gemini_thinking_budget {
            let budget = explicit_budget.unwrap_or(match eff.as_str() {
                "minimal" | "low" => 2048,
                "medium" => 8192,
                _ => 16384,
            });
            gen_cfg.insert(
                "thinkingConfig".into(),
                json!({ "includeThoughts": true, "thinkingBudget": budget }),
            );
        } else {
            let level = eff.to_ascii_uppercase();
            gen_cfg.insert(
                "thinkingConfig".into(),
                json!({ "includeThoughts": true, "thinkingLevel": level }),
            );
        }
    } else if !adapt.strip_sampling {
        if let Some(t) = profile.temperature {
            gen_cfg.insert("temperature".into(), json!(t));
        }
        if let Some(p) = profile.top_p {
            gen_cfg.insert("topP".into(), json!(p));
        }
    }
    if !gen_cfg.is_empty() {
        body.insert("generationConfig".into(), Value::Object(gen_cfg));
    }

    let mut contents = Vec::new();
    let last_asst_idx = nodes.iter().rposition(|n| n.role == MessageRole::Assistant);
    for (node_idx, node) in nodes.iter().enumerate() {
        let Some(v) = node.active_variant() else {
            continue;
        };
        let clean = strip_inline_error_notices(&v.content);
        match node.role {
            MessageRole::User | MessageRole::System => {
                let mut parts = Vec::new();
                if !clean.is_empty() || v.attachments.is_empty() {
                    parts.push(json!({ "text": clean }));
                }
                for att in &v.attachments {
                    if let Some(inline) = format_inline_text_attachment(att) {
                        parts.push(json!({ "text": inline }));
                    } else if let Some(b64) = load_attachment_base64(att) {
                        let mime = normalized_mime(att);
                        parts.push(json!({
                            "inlineData": { "mimeType": mime, "data": b64 }
                        }));
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                if v.tool_calls.is_empty() {
                    let mut part = Map::new();
                    part.insert("text".into(), json!(clean));
                    let sig = sanitize_gemini_thought_signature(&v.thinking_signature, false);
                    if !sig.is_empty() {
                        part.insert("thoughtSignature".into(), json!(sig));
                    }
                    contents.push(json!({ "role": "model", "parts": [Value::Object(part)] }));
                } else {
                    let total_calls = v.tool_calls.len();
                    let mut step_start = 0usize;
                    while step_start < total_calls {
                        let mut step_end = step_start + 1;
                        while step_end < total_calls && !v.tool_calls[step_end].starts_tool_step {
                            step_end += 1;
                        }
                        let mut model_parts = Vec::new();
                        let mut resp_parts = Vec::new();
                        for (rel_idx, tc) in v.tool_calls[step_start..step_end].iter().enumerate() {
                            let abs_idx = step_start + rel_idx;
                            if !tc.preceding_text.trim().is_empty() {
                                model_parts.push(json!({ "text": tc.preceding_text }));
                            }
                            let args: Value = serde_json::from_str(&tc.arguments_json)
                                .unwrap_or_else(|_| json!({}));
                            let mut call_part = Map::new();
                            call_part.insert(
                                "functionCall".into(),
                                json!({ "name": tc.name, "args": args }),
                            );
                            let fn_sig =
                                sanitize_gemini_thought_signature(&tc.thought_signature, true);
                            if !fn_sig.is_empty() {
                                call_part.insert("thoughtSignature".into(), json!(fn_sig));
                            }
                            model_parts.push(Value::Object(call_part));

                            let out = compact_historical_tool_result_for_wire(
                                &tc.result_text,
                                &tc.name,
                                is_older,
                                abs_idx,
                                total_calls,
                            );
                            resp_parts.push(json!({
                                "functionResponse": {
                                    "name": tc.name,
                                    "response": { "result": out }
                                }
                            }));
                        }
                        contents.push(json!({ "role": "model", "parts": model_parts }));
                        contents.push(json!({ "role": "user", "parts": resp_parts }));
                        step_start = step_end;
                    }
                    if !clean.is_empty() {
                        contents.push(json!({ "role": "model", "parts": [{ "text": clean }] }));
                    }
                }
            }
        }
    }
    if !late_note.is_empty() {
        contents.push(json!({ "role": "user", "parts": [{ "text": late_note }] }));
    }
    if !continue_prefix.is_empty() {
        contents.push(json!({ "role": "model", "parts": [{ "text": continue_prefix }] }));
    }
    body.insert("contents".into(), Value::Array(contents));
    body
}
