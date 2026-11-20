use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_openai_responses_body(
    profile: &ProviderProfile,
    model: &str,
    system_prompt: &str,
    late_note: &str,
    nodes: &[Cow<'_, MessageNode>],
    tools: &[ToolDefinition],
    prev_response_id: &str,
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
    if !tools.is_empty() {
        let arr: Vec<Value> = tools
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
        body.insert("tools".into(), Value::Array(arr));
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
                input.push(json!({ "role": role, "content": clean }));
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
            MessageRole::User => messages.push(json!({ "role": "user", "content": clean })),
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
        if !adapt.omit_reasoning {
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
    if !tools.is_empty() {
        let last_idx = tools.len() - 1;
        let arr: Vec<Value> = tools
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut obj = Map::new();
                obj.insert("name".into(), json!(t.name));
                obj.insert("description".into(), json!(t.description));
                obj.insert("input_schema".into(), t.parameters_schema.clone());
                if i == last_idx {
                    obj.insert("cache_control".into(), json!({ "type": "ephemeral" }));
                }
                Value::Object(obj)
            })
            .collect();
        body.insert("tools".into(), Value::Array(arr));
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
                messages.push(json!({ "role": "user", "content": clean }));
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let mut blocks = Vec::new();
                if !v.thinking_summary.is_empty()
                    && is_valid_anthropic_signature(
                        &v.thinking_signature,
                        v.provider_protocol,
                        &v.model_used,
                        model,
                    )
                {
                    blocks.push(json!({
                        "type": "thinking",
                        "thinking": v.thinking_summary,
                        "signature": v.thinking_signature,
                    }));
                }
                if v.tool_calls.is_empty() {
                    blocks.push(json!({ "type": "text", "text": clean }));
                    messages.push(json!({ "role": "assistant", "content": blocks }));
                } else {
                    let total_calls = v.tool_calls.len();
                    let mut tool_results = Vec::new();
                    for (tc_idx, tc) in v.tool_calls.iter().enumerate() {
                        if !tc.preceding_text.trim().is_empty() {
                            blocks.push(json!({ "type": "text", "text": tc.preceding_text }));
                        }
                        let cid = ids.resolve(&tc.id, "toolu");
                        let input_val: Value =
                            serde_json::from_str(&tc.arguments_json).unwrap_or_else(|_| json!({}));
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
                            tc_idx,
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
    if !tools.is_empty() {
        let arr: Vec<Value> = tools
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
        body.insert("tools".into(), Value::Array(arr));
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
                input.push(json!({
                    "role": "user",
                    "content": [{ "type": "text", "text": clean }]
                }));
            }
            MessageRole::Assistant => {
                let is_older = Some(node_idx) != last_asst_idx;
                let mut content_blocks = Vec::new();
                let sig = sanitize_gemini_thought_signature(&v.thinking_signature, false);
                if !sig.is_empty() {
                    content_blocks.push(json!({ "type": "thought", "signature": sig }));
                }
                if v.tool_calls.is_empty() {
                    content_blocks.push(json!({ "type": "text", "text": clean }));
                    input.push(json!({ "role": "model", "content": content_blocks }));
                } else {
                    let total_calls = v.tool_calls.len();
                    let mut result_blocks = Vec::new();
                    for (tc_idx, tc) in v.tool_calls.iter().enumerate() {
                        let cid = ids.resolve(&tc.id, "call");
                        let fn_sig = sanitize_gemini_thought_signature(&tc.thought_signature, true);
                        if !fn_sig.is_empty() {
                            content_blocks.push(json!({ "type": "thought", "signature": fn_sig }));
                        }
                        let args: Value =
                            serde_json::from_str(&tc.arguments_json).unwrap_or_else(|_| json!({}));
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
                            tc_idx,
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
        body.insert("tools".into(), json!([{ "function_declarations": decls }]));
    }

    let mut gen_cfg = Map::new();
    if let Some(max_out) = profile.max_output_tokens
        && max_out > 0
    {
        gen_cfg.insert("maxOutputTokens".into(), json!(max_out));
    }
    if let Some(eff) = effective_reasoning_effort(profile, adapt) {
        let level = eff.to_ascii_uppercase();
        gen_cfg.insert(
            "thinkingConfig".into(),
            json!({ "includeThoughts": true, "thinkingLevel": level }),
        );
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
                contents.push(json!({ "role": "user", "parts": [{ "text": clean }] }));
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
                    let mut model_parts = Vec::new();
                    let mut resp_parts = Vec::new();
                    for (tc_idx, tc) in v.tool_calls.iter().enumerate() {
                        let args: Value =
                            serde_json::from_str(&tc.arguments_json).unwrap_or_else(|_| json!({}));
                        let mut call_part = Map::new();
                        call_part.insert(
                            "functionCall".into(),
                            json!({ "name": tc.name, "args": args }),
                        );
                        let fn_sig = sanitize_gemini_thought_signature(&tc.thought_signature, true);
                        if !fn_sig.is_empty() {
                            call_part.insert("thoughtSignature".into(), json!(fn_sig));
                        }
                        model_parts.push(Value::Object(call_part));

                        let out = compact_historical_tool_result_for_wire(
                            &tc.result_text,
                            &tc.name,
                            is_older,
                            tc_idx,
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
    body.insert("contents".into(), Value::Array(contents));
    body
}
