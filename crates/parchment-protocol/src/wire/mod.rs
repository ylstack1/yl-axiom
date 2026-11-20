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

mod builders;
mod parser;

use builders::*;
pub use parser::WireStreamParser;

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

    let url = if profile.protocol.is_custom_wire() {
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
                let clean = model.trim().trim_prefix("models/");
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

    Ok(url)
}

pub fn build_auth_headers(
    profile: &ProviderProfile,
    dialect: WireDialect,
) -> Vec<(String, String)> {
    build_auth_headers_for_key(profile, &profile.api_key, dialect)
}

pub fn build_auth_headers_for_key(
    profile: &ProviderProfile,
    api_key: &str,
    dialect: WireDialect,
) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    headers.push(("Content-Type".to_string(), "application/json".to_string()));
    headers.push(("Accept".to_string(), "text/event-stream".to_string()));

    let key = api_key.trim();
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
