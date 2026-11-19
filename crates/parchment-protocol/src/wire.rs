//! All 5 LLM wire dialects (request builders + SSE/JSON event parsers) with strict `CUSTOM`
//! routing, prompt-cache-stable field ordering (`tools` before `messages`/`input`/`contents`),
//! and verbatim opaque reasoning signature preservation.

use std::borrow::Cow;
use std::collections::BTreeMap;

use parchment_model::{
    AuthHeaderMode, CUSTOM_DIALECT_ANTHROPIC, CUSTOM_DIALECT_GEMINI, CUSTOM_DIALECT_OPENAI_CHAT,
    CUSTOM_DIALECT_OPENAI_RESPONSES, ChatSession, GoalTrackerState, MessageNode, MessageRole,
    ProviderProfile, ProviderProtocol, ProviderTokenUsage,
};
use serde_json::{Map, Value, json};

use crate::error_policy::ProtocolError;
use crate::history::{
    ReplayToolCallIds, build_compiled_system_prompt, build_late_context_note,
    compact_historical_tool_result_for_wire, sanitize_context_nodes, strip_inline_error_notices,
};
use crate::signatures::{
    apply_cross_provider_reasoning_handoff, effective_wire_dialect, is_native_reasoning_compatible,
    is_valid_anthropic_signature, is_valid_openai_encrypted_reasoning, reusable_provider_chain_id,
    sanitize_gemini_thought_signature,
};
use crate::usage::{UsageDialect, parse_provider_usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WireDialect {
    OpenAiResponses,
    OpenAiChat,
    AnthropicMessages,
    GeminiInteractions,
    GeminiGenerateContent,
}

impl WireDialect {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::OpenAiResponses => CUSTOM_DIALECT_OPENAI_RESPONSES,
            Self::OpenAiChat => CUSTOM_DIALECT_OPENAI_CHAT,
            Self::AnthropicMessages => CUSTOM_DIALECT_ANTHROPIC,
            Self::GeminiInteractions => CUSTOM_DIALECT_GEMINI,
            Self::GeminiGenerateContent => "gemini_generate_content",
        }
    }

    pub fn usage_dialect(self) -> UsageDialect {
        match self {
            Self::OpenAiResponses => UsageDialect::OpenAiResponses,
            Self::OpenAiChat => UsageDialect::OpenAiChat,
            Self::AnthropicMessages => UsageDialect::Anthropic,
            Self::GeminiInteractions => UsageDialect::GeminiInteractions,
            Self::GeminiGenerateContent => UsageDialect::GeminiContent,
        }
    }

    pub fn protocol_family(self) -> ProviderProtocol {
        match self {
            Self::OpenAiResponses | Self::OpenAiChat => ProviderProtocol::OpenAiResponses,
            Self::AnthropicMessages => ProviderProtocol::AnthropicMessages,
            Self::GeminiInteractions | Self::GeminiGenerateContent => {
                ProviderProtocol::GeminiInteractions
            }
        }
    }

    pub fn legacy_fallback(self) -> Option<Self> {
        match self {
            Self::OpenAiResponses => Some(Self::OpenAiChat),
            Self::GeminiInteractions => Some(Self::GeminiGenerateContent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
}

#[derive(Debug, Clone, Default)]
pub struct WireAdaptOptions {
    pub reasoning_effort_override: Option<String>,
    pub omit_reasoning: bool,
    pub omit_anthropic_adaptive_display: bool,
    pub strip_sampling: bool,
    pub omit_stream_options: bool,
    pub use_max_tokens_key: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct WireRequest {
    pub dialect: WireDialect,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body_json: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawWebCitation {
    pub url: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Status(String),
    InteractionId(String),
    ThinkingDelta(String),
    ThinkingSignature(String),
    TextDelta(String),
    ToolCall {
        id: String,
        name: String,
        arguments_json: String,
        thought_signature: String,
    },
    Citation(RawWebCitation),
    Usage {
        response_id: String,
        usage: ProviderTokenUsage,
    },
    OutputLimit,
    Done,
}

/// Resolves the primary wire dialect for `profile`.
/// `CUSTOM` requires an explicit valid `custom_dialect` and non-blank `base_url`; unresolved `CUSTOM` blocks Send.
pub fn resolve_primary_dialect(profile: &ProviderProfile) -> Result<WireDialect, ProtocolError> {
    match profile.protocol {
        ProviderProtocol::GeminiInteractions => Ok(WireDialect::GeminiInteractions),
        ProviderProtocol::OpenAiResponses => Ok(WireDialect::OpenAiResponses),
        ProviderProtocol::AnthropicMessages => Ok(WireDialect::AnthropicMessages),
        ProviderProtocol::Custom => {
            if profile.base_url.trim().is_empty() {
                return Err(ProtocolError::NoRetry(
                    "Custom provider requires a Server URL before sending.".into(),
                ));
            }
            match profile.custom_dialect.trim().to_ascii_lowercase().as_str() {
                CUSTOM_DIALECT_OPENAI_CHAT => Ok(WireDialect::OpenAiChat),
                CUSTOM_DIALECT_OPENAI_RESPONSES => Ok(WireDialect::OpenAiResponses),
                CUSTOM_DIALECT_ANTHROPIC => Ok(WireDialect::AnthropicMessages),
                CUSTOM_DIALECT_GEMINI => Ok(WireDialect::GeminiInteractions),
                _ => Err(ProtocolError::NoRetry(
                    "Custom provider requires an explicit API format before sending.".into(),
                )),
            }
        }
    }
}

pub fn build_endpoint_url(
    profile: &ProviderProfile,
    dialect: WireDialect,
    model: &str,
) -> Result<String, ProtocolError> {
    let base = profile.effective_base_url().trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(ProtocolError::NoRetry(
            "Provider Server URL is empty.".into(),
        ));
    }

    let mut url = if profile.protocol.is_custom_wire() {
        // CUSTOM uses verbatim URL + optional path override, never appending silent guessed paths.
        let path = profile.endpoint_path_override.trim();
        if path.is_empty() {
            base.to_string()
        } else if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        }
    } else {
        let override_path = profile.endpoint_path_override.trim();
        let path = match dialect {
            WireDialect::GeminiGenerateContent => {
                let clean = model.trim().strip_prefix("models/").unwrap_or(model.trim());
                format!("/v1beta/models/{clean}:streamGenerateContent?alt=sse")
            }
            WireDialect::OpenAiChat => "/v1/chat/completions".to_string(),
            WireDialect::OpenAiResponses => {
                if !override_path.is_empty() {
                    override_path.to_string()
                } else {
                    "/v1/responses".to_string()
                }
            }
            WireDialect::AnthropicMessages => {
                if !override_path.is_empty() {
                    override_path.to_string()
                } else {
                    "/v1/messages".to_string()
                }
            }
            WireDialect::GeminiInteractions => {
                if !override_path.is_empty() {
                    override_path.to_string()
                } else {
                    "/v1beta/interactions".to_string()
                }
            }
        };
        if path.starts_with("http://") || path.starts_with("https://") {
            path
        } else if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        }
    };

    if profile.auth_header_mode == AuthHeaderMode::QueryParam && !profile.api_key.trim().is_empty()
    {
        let sep = if url.contains('?') { '&' } else { '?' };
        url.push(sep);
        url.push_str("key=");
        url.push_str(profile.api_key.trim());
    }

    Ok(url)
}

pub fn build_auth_headers(
    profile: &ProviderProfile,
    dialect: WireDialect,
) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    headers.push(("Content-Type".to_string(), "application/json".to_string()));
    headers.push(("Accept".to_string(), "text/event-stream".to_string()));

    let key = profile.api_key.trim();
    if !key.is_empty() {
        match profile.auth_header_mode {
            AuthHeaderMode::Native => match dialect {
                WireDialect::GeminiInteractions | WireDialect::GeminiGenerateContent => {
                    headers.push(("x-goog-api-key".to_string(), key.to_string()));
                }
                WireDialect::AnthropicMessages => {
                    headers.push(("x-api-key".to_string(), key.to_string()));
                }
                WireDialect::OpenAiResponses | WireDialect::OpenAiChat => {
                    headers.push(("Authorization".to_string(), format!("Bearer {key}")));
                }
            },
            AuthHeaderMode::BearerOnly => {
                headers.push(("Authorization".to_string(), format!("Bearer {key}")));
            }
            AuthHeaderMode::QueryParam => {}
        }
    }

    if dialect == WireDialect::AnthropicMessages {
        headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
        if !profile.anthropic_beta_headers.trim().is_empty() {
            headers.push((
                "anthropic-beta".to_string(),
                profile.anthropic_beta_headers.trim().to_string(),
            ));
        }
    }

    for line in profile.custom_headers_raw.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let kt = k.trim();
            let vt = v.trim();
            if !kt.is_empty() && !vt.is_empty() {
                headers.push((kt.to_string(), vt.to_string()));
            }
        }
    }
    headers
}

/// Builds a complete wire request for any of the 5 supported dialects.
/// Guarantees `tools` is serialized BEFORE `messages`/`input`/`contents` for prompt-cache stability.
#[allow(clippy::too_many_arguments)]
pub fn build_wire_request(
    profile: &ProviderProfile,
    dialect: WireDialect,
    model: &str,
    session: &ChatSession,
    history_nodes: &[MessageNode],
    tools: &[ToolDefinition],
    goal_state: Option<&GoalTrackerState>,
    continue_prefix: &str,
    adapt: &WireAdaptOptions,
) -> Result<WireRequest, ProtocolError> {
    let url = build_endpoint_url(profile, dialect, model)?;
    let headers = build_auth_headers(profile, dialect);
    let clean_nodes = sanitize_context_nodes(history_nodes);
    let system_prompt = build_compiled_system_prompt(session, goal_state);
    let late_note = build_late_context_note(session, &clean_nodes);

    let prev_assistant_idx = clean_nodes
        .iter()
        .rposition(|n| n.role == MessageRole::Assistant);
    let prev_chain_id = if profile.use_stateful_chaining
        && continue_prefix.trim().is_empty()
        && matches!(
            dialect,
            WireDialect::OpenAiResponses | WireDialect::GeminiInteractions
        )
        && let Some(idx) = prev_assistant_idx
        && let Some(var) = clean_nodes[idx].active_variant()
    {
        reusable_provider_chain_id(var, dialect.protocol_family(), model, dialect.wire_name())
    } else {
        String::new()
    };
    let chaining_active = !prev_chain_id.is_empty();

    let handoff_nodes = apply_cross_provider_reasoning_handoff(
        &clean_nodes,
        dialect.protocol_family(),
        chaining_active,
        dialect.wire_name(),
        model,
        strip_inline_error_notices,
    );
    let source_nodes: &[Cow<'_, MessageNode>] = if chaining_active
        && let Some(idx) = prev_assistant_idx
        && idx + 1 < handoff_nodes.len()
    {
        &handoff_nodes[idx + 1..]
    } else {
        &handoff_nodes
    };

    let body_obj = match dialect {
        WireDialect::OpenAiResponses => build_openai_responses_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            &prev_chain_id,
            adapt,
        ),
        WireDialect::OpenAiChat => build_openai_chat_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            continue_prefix,
            adapt,
        ),
        WireDialect::AnthropicMessages => build_anthropic_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            continue_prefix,
            adapt,
        ),
        WireDialect::GeminiInteractions => build_gemini_interactions_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            &prev_chain_id,
            adapt,
        ),
        WireDialect::GeminiGenerateContent => build_gemini_generate_content_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            adapt,
        ),
    };

    let body_json = serde_json::to_string(&Value::Object(body_obj))
        .map_err(|e| ProtocolError::NoRetry(e.to_string()))?;

    Ok(WireRequest {
        dialect,
        url,
        headers,
        body_json,
    })
}

fn effective_reasoning_effort<'a>(
    profile: &'a ProviderProfile,
    adapt: &'a WireAdaptOptions,
) -> Option<String> {
    if adapt.omit_reasoning {
        return None;
    }
    if let Some(ov) = &adapt.reasoning_effort_override {
        let t = ov.trim().to_ascii_lowercase();
        if !t.is_empty() && !matches!(t.as_str(), "none" | "off" | "default") {
            return Some(t);
        }
        return None;
    }
    let t = profile.thinking_effort.trim().to_ascii_lowercase();
    if t.is_empty() || matches!(t.as_str(), "none" | "off" | "default") {
        None
    } else {
        Some(t)
    }
}

#[allow(clippy::too_many_arguments)]
fn build_openai_responses_body(
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
fn build_openai_chat_body(
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
fn build_anthropic_body(
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
fn build_gemini_interactions_body(
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
    let clean_model = model.trim().strip_prefix("models/").unwrap_or(model.trim());
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
fn build_gemini_generate_content_body(
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

/// Stateful per-round SSE parser translating all 5 wire dialects into unified `StreamEvent`s.
pub struct WireStreamParser {
    dialect: WireDialect,
    finished: bool,
    // Accumulators for chunked function/tool calls across SSE events.
    chat_tools: BTreeMap<usize, (String, String, String)>,
    anthropic_blocks: BTreeMap<usize, AnthropicBlockState>,
    responses_calls: BTreeMap<String, (String, String)>,
    gemini_fn_calls: BTreeMap<usize, (String, String, String, String)>,
    last_thought_sig: String,
}

enum AnthropicBlockState {
    Text,
    Thinking,
    ToolUse {
        id: String,
        name: String,
        args: String,
    },
}

impl WireStreamParser {
    pub fn new(dialect: WireDialect) -> Self {
        Self {
            dialect,
            finished: false,
            chat_tools: BTreeMap::new(),
            anthropic_blocks: BTreeMap::new(),
            responses_calls: BTreeMap::new(),
            gemini_fn_calls: BTreeMap::new(),
            last_thought_sig: String::new(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn consume_sse(
        &mut self,
        event_type: &str,
        data: &str,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if data == "[DONE]" {
            self.flush_pending_tools(out);
            self.finished = true;
            out.push(StreamEvent::Done);
            return Ok(());
        }
        let Ok(val) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        let Some(obj) = val.as_object() else {
            return Ok(());
        };

        match self.dialect {
            WireDialect::OpenAiResponses => self.parse_openai_responses(event_type, obj, out),
            WireDialect::OpenAiChat => self.parse_openai_chat(obj, out),
            WireDialect::AnthropicMessages => self.parse_anthropic(event_type, obj, out),
            WireDialect::GeminiInteractions => self.parse_gemini_interactions(event_type, obj, out),
            WireDialect::GeminiGenerateContent => self.parse_gemini_generate_content(obj, out),
        }
    }

    pub fn flush_pending_tools(&mut self, out: &mut Vec<StreamEvent>) {
        let chat = std::mem::take(&mut self.chat_tools);
        for (_, (id, name, args)) in chat {
            if !name.is_empty() {
                out.push(StreamEvent::ToolCall {
                    id,
                    name,
                    arguments_json: if args.trim().is_empty() {
                        "{}".into()
                    } else {
                        args
                    },
                    thought_signature: String::new(),
                });
            }
        }
        let gem = std::mem::take(&mut self.gemini_fn_calls);
        for (_, (id, name, args, sig)) in gem {
            if !name.is_empty() {
                out.push(StreamEvent::ToolCall {
                    id,
                    name,
                    arguments_json: if args.trim().is_empty() {
                        "{}".into()
                    } else {
                        args
                    },
                    thought_signature: sig,
                });
            }
        }
    }

    fn parse_openai_responses(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        match ev {
            "response.created" | "response.in_progress" => {
                if let Some(id) = obj
                    .get("response")
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                {
                    out.push(StreamEvent::InteractionId(id.to_string()));
                }
            }
            "response.output_text.delta" => {
                if let Some(d) = obj.get("delta").and_then(Value::as_str) {
                    out.push(StreamEvent::TextDelta(d.to_string()));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning.delta" => {
                if let Some(d) = obj.get("delta").and_then(Value::as_str) {
                    out.push(StreamEvent::ThinkingDelta(d.to_string()));
                }
            }
            "response.output_item.added" => {
                if let Some(item) = obj.get("item").and_then(Value::as_object)
                    && item.get("type").and_then(Value::as_str) == Some("function_call")
                {
                    let call_id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if !call_id.is_empty() {
                        self.responses_calls.insert(call_id, (name, args));
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let Some(item_id) = obj
                    .get("call_id")
                    .or_else(|| obj.get("item_id"))
                    .and_then(Value::as_str)
                    && let Some(delta) = obj.get("delta").and_then(Value::as_str)
                    && let Some(entry) = self.responses_calls.get_mut(item_id)
                {
                    entry.1.push_str(delta);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = obj.get("item").and_then(Value::as_object) {
                    match item.get("type").and_then(Value::as_str) {
                        Some("reasoning") => {
                            if let Some(enc) = item.get("encrypted_content").and_then(Value::as_str)
                            {
                                out.push(StreamEvent::ThinkingSignature(enc.to_string()));
                            }
                        }
                        Some("function_call") => {
                            let call_id = item
                                .get("call_id")
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = item
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = item
                                .get("arguments")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .or_else(|| self.responses_calls.remove(&call_id).map(|e| e.1))
                                .unwrap_or_else(|| "{}".to_string());
                            if !name.is_empty() {
                                out.push(StreamEvent::ToolCall {
                                    id: call_id,
                                    name,
                                    arguments_json: args,
                                    thought_signature: String::new(),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            "response.completed" | "response.incomplete" => {
                let resp = obj.get("response").and_then(Value::as_object);
                let resp_id = resp
                    .and_then(|r| r.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(usage_obj) =
                    resp.and_then(|r| r.get("usage")).and_then(Value::as_object)
                {
                    out.push(StreamEvent::Usage {
                        response_id: resp_id,
                        usage: parse_provider_usage(
                            UsageDialect::OpenAiResponses,
                            Some(usage_obj),
                            true,
                            resp,
                        ),
                    });
                }
                if ev == "response.incomplete" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "response.failed" | "error" => {
                let msg = obj
                    .get("error")
                    .or_else(|| obj.get("response").and_then(|r| r.get("error")))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("OpenAI Responses stream error");
                return Err(ProtocolError::StreamInterrupted(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_openai_chat(
        &mut self,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if let Some(err) = obj.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI Chat stream error");
            return Err(ProtocolError::StreamInterrupted(msg.to_string()));
        }
        let resp_id = obj
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(usage_obj) = obj.get("usage").and_then(Value::as_object) {
            out.push(StreamEvent::Usage {
                response_id: resp_id,
                usage: parse_provider_usage(
                    UsageDialect::OpenAiChat,
                    Some(usage_obj),
                    true,
                    Some(obj),
                ),
            });
        }
        if let Some(choice) = obj
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_object)
        {
            if let Some(delta) = choice.get("delta").and_then(Value::as_object) {
                if let Some(r) = delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
                    .and_then(Value::as_str)
                    && !r.is_empty()
                {
                    out.push(StreamEvent::ThinkingDelta(r.to_string()));
                }
                if let Some(c) = delta.get("content").and_then(Value::as_str)
                    && !c.is_empty()
                {
                    out.push(StreamEvent::TextDelta(c.to_string()));
                }
                if let Some(tcs) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = self
                            .chat_tools
                            .entry(idx)
                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc.get("id").and_then(Value::as_str)
                            && !id.is_empty()
                        {
                            entry.0 = id.to_string();
                        }
                        if let Some(func) = tc.get("function").and_then(Value::as_object) {
                            if let Some(n) = func.get("name").and_then(Value::as_str)
                                && !n.is_empty()
                            {
                                entry.1.push_str(n);
                            }
                            if let Some(a) = func.get("arguments").and_then(Value::as_str) {
                                entry.2.push_str(a);
                            }
                        }
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.flush_pending_tools(out);
                if reason == "length" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
            }
        }
        Ok(())
    }

    fn parse_anthropic(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        match ev {
            "message_start" => {
                if let Some(msg) = obj.get("message").and_then(Value::as_object) {
                    let resp_id = msg
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Some(u) = msg.get("usage").and_then(Value::as_object) {
                        out.push(StreamEvent::Usage {
                            response_id: resp_id,
                            usage: parse_provider_usage(
                                UsageDialect::Anthropic,
                                Some(u),
                                false,
                                Some(msg),
                            ),
                        });
                    }
                }
            }
            "content_block_start" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(blk) = obj.get("content_block").and_then(Value::as_object) {
                    match blk.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            self.anthropic_blocks.insert(idx, AnthropicBlockState::Text);
                        }
                        Some("thinking") => {
                            self.anthropic_blocks
                                .insert(idx, AnthropicBlockState::Thinking);
                        }
                        Some("tool_use") => {
                            let id = blk
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = blk
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            self.anthropic_blocks.insert(
                                idx,
                                AnthropicBlockState::ToolUse {
                                    id,
                                    name,
                                    args: String::new(),
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
            "content_block_delta" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(delta) = obj.get("delta").and_then(Value::as_object) {
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                out.push(StreamEvent::TextDelta(t.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(t) = delta.get("thinking").and_then(Value::as_str) {
                                out.push(StreamEvent::ThinkingDelta(t.to_string()));
                            }
                        }
                        Some("signature_delta") => {
                            if let Some(s) = delta.get("signature").and_then(Value::as_str) {
                                out.push(StreamEvent::ThinkingSignature(s.to_string()));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(j) = delta.get("partial_json").and_then(Value::as_str)
                                && let Some(AnthropicBlockState::ToolUse { args, .. }) =
                                    self.anthropic_blocks.get_mut(&idx)
                            {
                                args.push_str(j);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(AnthropicBlockState::ToolUse { id, name, args }) =
                    self.anthropic_blocks.remove(&idx)
                    && !name.is_empty()
                {
                    out.push(StreamEvent::ToolCall {
                        id,
                        name,
                        arguments_json: if args.trim().is_empty() {
                            "{}".into()
                        } else {
                            args
                        },
                        thought_signature: String::new(),
                    });
                }
            }
            "message_delta" => {
                if let Some(u) = obj.get("usage").and_then(Value::as_object) {
                    out.push(StreamEvent::Usage {
                        response_id: String::new(),
                        usage: parse_provider_usage(
                            UsageDialect::Anthropic,
                            Some(u),
                            true,
                            Some(obj),
                        ),
                    });
                }
                if obj
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(Value::as_str)
                    == Some("max_tokens")
                {
                    out.push(StreamEvent::OutputLimit);
                }
            }
            "message_stop" => {
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "error" => {
                let msg = obj
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Anthropic stream error");
                return Err(ProtocolError::StreamInterrupted(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_gemini_interactions(
        &mut self,
        event_type: &str,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        let ev = obj
            .get("event_type")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .unwrap_or(event_type);
        let inter = obj.get("interaction").and_then(Value::as_object);
        if let Some(id) = inter
            .and_then(|i| i.get("id"))
            .or_else(|| obj.get("interaction_id"))
            .and_then(Value::as_str)
            && !id.is_empty()
        {
            out.push(StreamEvent::InteractionId(id.to_string()));
        }
        if let Some(u) = obj
            .get("usage")
            .or_else(|| inter.and_then(|i| i.get("usage")))
            .and_then(Value::as_object)
        {
            let is_term = matches!(
                ev,
                "interaction.complete" | "interaction.completed" | "interaction.incomplete"
            );
            out.push(StreamEvent::Usage {
                response_id: String::new(),
                usage: parse_provider_usage(
                    UsageDialect::GeminiInteractions,
                    Some(u),
                    is_term,
                    inter.or(Some(obj)),
                ),
            });
        }
        match ev {
            "content.delta" | "step.delta" => {
                let idx = obj.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                if let Some(delta) = obj.get("delta").and_then(Value::as_object) {
                    match delta.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                out.push(StreamEvent::TextDelta(t.to_string()));
                            }
                        }
                        Some("thought") | Some("thought_summary") => {
                            if let Some(t) = delta
                                .get("content")
                                .and_then(|c| c.get("text"))
                                .or_else(|| delta.get("text"))
                                .and_then(Value::as_str)
                            {
                                out.push(StreamEvent::ThinkingDelta(t.to_string()));
                            }
                            if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                                self.last_thought_sig = sig.to_string();
                                out.push(StreamEvent::ThinkingSignature(sig.to_string()));
                            }
                        }
                        Some("thought_signature") => {
                            if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                                self.last_thought_sig = sig.to_string();
                                out.push(StreamEvent::ThinkingSignature(sig.to_string()));
                            }
                        }
                        Some("function_call") => {
                            let id = delta
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = delta
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = delta
                                .get("arguments")
                                .map(|a| {
                                    a.as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| a.to_string())
                                })
                                .unwrap_or_default();
                            let entry = self.gemini_fn_calls.entry(idx).or_insert_with(|| {
                                (
                                    id.clone(),
                                    name.clone(),
                                    String::new(),
                                    self.last_thought_sig.clone(),
                                )
                            });
                            if !id.is_empty() {
                                entry.0 = id;
                            }
                            if !name.is_empty() {
                                entry.1 = name;
                            }
                            entry.2.push_str(&args);
                        }
                        _ => {}
                    }
                }
            }
            "interaction.complete"
            | "interaction.completed"
            | "interaction.requires_action"
            | "interaction.incomplete" => {
                self.flush_pending_tools(out);
                if ev == "interaction.incomplete" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
            "interaction.failed" | "error" => {
                let msg = obj
                    .get("error")
                    .or_else(|| inter.and_then(|i| i.get("error")))
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("Gemini Interactions error");
                return Err(ProtocolError::NoRetry(msg.to_string()));
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_gemini_generate_content(
        &mut self,
        obj: &Map<String, Value>,
        out: &mut Vec<StreamEvent>,
    ) -> Result<(), ProtocolError> {
        if let Some(err) = obj.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Gemini generateContent error");
            return Err(ProtocolError::StreamInterrupted(msg.to_string()));
        }
        if let Some(u) = obj.get("usageMetadata").and_then(Value::as_object) {
            let is_final = obj
                .get("candidates")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|c| c.get("finishReason"))
                .and_then(Value::as_str)
                .is_some();
            out.push(StreamEvent::Usage {
                response_id: String::new(),
                usage: parse_provider_usage(
                    UsageDialect::GeminiContent,
                    Some(u),
                    is_final,
                    Some(obj),
                ),
            });
        }
        if let Some(cand) = obj
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_object)
        {
            if let Some(parts) = cand
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(Value::as_array)
            {
                for (p_idx, part) in parts.iter().enumerate() {
                    let Some(pobj) = part.as_object() else {
                        continue;
                    };
                    if let Some(sig) = pobj.get("thoughtSignature").and_then(Value::as_str)
                        && !sig.is_empty()
                    {
                        self.last_thought_sig = sig.to_string();
                        out.push(StreamEvent::ThinkingSignature(sig.to_string()));
                    }
                    let is_thought = pobj.get("thought").and_then(Value::as_bool) == Some(true);
                    if let Some(text) = pobj.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        if is_thought {
                            out.push(StreamEvent::ThinkingDelta(text.to_string()));
                        } else {
                            out.push(StreamEvent::TextDelta(text.to_string()));
                        }
                    }
                    if let Some(fc) = pobj.get("functionCall").and_then(Value::as_object) {
                        let name = fc
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let args = fc
                            .get("args")
                            .map(|a| a.to_string())
                            .unwrap_or_else(|| "{}".to_string());
                        if !name.is_empty() {
                            out.push(StreamEvent::ToolCall {
                                id: format!("gem_call_{p_idx}"),
                                name,
                                arguments_json: args,
                                thought_signature: self.last_thought_sig.clone(),
                            });
                        }
                    }
                }
            }
            if let Some(reason) = cand.get("finishReason").and_then(Value::as_str) {
                if reason == "MAX_TOKENS" {
                    out.push(StreamEvent::OutputLimit);
                }
                self.finished = true;
                out.push(StreamEvent::Done);
            }
        }
        Ok(())
    }
}
