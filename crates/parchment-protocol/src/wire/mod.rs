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
    pub use_gemini_thinking_budget: bool,
    pub omit_builtin_tools: bool,
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

const CONCRETE_ENDPOINT_TAILS: &[&str] = &[
    "/responses",
    "/chat/completions",
    "/completions",
    "/messages",
    "/interactions",
    ":generatecontent",
    ":streamgeneratecontent",
    ":predict",
    ":predictlongrunning",
    "/images/generations",
    "/images/edits",
    "/videos/generations",
    "/video/generations",
    "/contents/generations/tasks",
    "/audio/speech",
    "/audio/transcriptions",
    "/generate",
    "/invoke",
    "/chat",
];

/// Matches Kotlin `VERSION_SUFFIX_REGEX` (`/(?:api/)?(?:paas/)?v\d+(?:beta\d*|alpha\d*)?$`
/// case-insensitive) strictly within the URL path (never a hostname).
pub(crate) fn strip_api_version_suffix(base: &str) -> Option<&str> {
    let trimmed = base.trim().trim_end_matches('#').trim_end_matches('/');
    let path_start = if let Some(scheme_end) = trimmed.find("://") {
        scheme_end + 3 + trimmed[scheme_end + 3..].find('/')?
    } else {
        0
    };
    let path = &trimmed[path_start..];
    let last_slash = path.rfind('/')?;
    let seg = &path[last_slash + 1..];
    if !is_version_segment(seg) {
        return None;
    }
    let mut cut = path_start + last_slash;
    if let Some(before) = trimmed[path_start..cut].strip_suffix_ascii_icase("/paas") {
        cut = path_start + before.len();
    }
    if let Some(before) = trimmed[path_start..cut].strip_suffix_ascii_icase("/api") {
        cut = path_start + before.len();
    }
    Some(&trimmed[..cut])
}

pub(crate) fn has_api_version_suffix(base: &str) -> bool {
    strip_api_version_suffix(base).is_some()
}

fn is_version_segment(seg: &str) -> bool {
    let bytes = seg.as_bytes();
    if bytes.len() < 2 || !matches!(bytes[0], b'v' | b'V') {
        return false;
    }
    let mut i = 1;
    let digit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return false;
    }
    let rest = &seg[i..];
    if rest.is_empty() {
        return true;
    }
    let tail = if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case("beta") {
        &rest[4..]
    } else if rest.len() >= 5 && rest[..5].eq_ignore_ascii_case("alpha") {
        &rest[5..]
    } else {
        return false;
    };
    tail.bytes().all(|b| b.is_ascii_digit())
}

trait StrAsciiIcaseExt {
    fn ends_with_ascii_icase(&self, suffix: &str) -> bool;
    fn strip_suffix_ascii_icase(&self, suffix: &str) -> Option<&str>;
    fn starts_with_http_scheme(&self) -> bool;
    fn contains_ascii_icase(&self, needle: &str) -> bool;
}

impl StrAsciiIcaseExt for str {
    fn ends_with_ascii_icase(&self, suffix: &str) -> bool {
        self.len() >= suffix.len()
            && self.is_char_boundary(self.len() - suffix.len())
            && self[self.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    }

    fn strip_suffix_ascii_icase(&self, suffix: &str) -> Option<&str> {
        if self.ends_with_ascii_icase(suffix) {
            Some(&self[..self.len() - suffix.len()])
        } else {
            None
        }
    }

    fn starts_with_http_scheme(&self) -> bool {
        (self.len() >= 7 && self[..7].eq_ignore_ascii_case("http://"))
            || (self.len() >= 8 && self[..8].eq_ignore_ascii_case("https://"))
    }

    fn contains_ascii_icase(&self, needle: &str) -> bool {
        self.to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }
}

/// Removes invisible format/control characters (BOM, ZWSP, CR/LF) a paste can smuggle into a key/header.
fn strip_invisible_credential_chars(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control() && !matches!(*c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}'))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn build_endpoint_url(
    profile: &ProviderProfile,
    dialect: WireDialect,
    model: &str,
) -> Result<String, ProtocolError> {
    let raw_base = profile.effective_base_url().trim();
    if raw_base.is_empty() {
        return Err(ProtocolError::NoRetry(
            "Provider Server URL is empty.".into(),
        ));
    }
    let raw_override = profile.endpoint_path_override.trim();

    // 1. Custom providers: URL is sent verbatim with optional explicit override, never guessed.
    if profile.protocol.is_custom_wire() {
        if raw_override.starts_with_http_scheme() {
            return Ok(raw_override.trim_end_matches('#').to_string());
        }
        if !raw_override.is_empty() && raw_override != "/" && raw_override != "#" {
            let clean_override = format!(
                "/{}",
                raw_override.trim_end_matches('#').trim_start_matches('/')
            );
            let base_no_slash = raw_base.trim_end_matches('#').trim_end_matches('/');
            return Ok(if base_no_slash.ends_with_ascii_icase(&clean_override) {
                raw_base.trim_end_matches('#').to_string()
            } else {
                format!("{base_no_slash}{clean_override}")
            });
        }
        return Ok(raw_base
            .trim_end_matches('#')
            .trim_end_matches('/')
            .to_string());
    }

    // 1b. Trailing `#` opt-out on managed protocols.
    if raw_base.ends_with('#') || raw_override == "/" || raw_override == "#" {
        return Ok(raw_base
            .trim_end_matches('#')
            .trim_end_matches('/')
            .to_string());
    }

    let base = raw_base.trim_end_matches('/');
    let ignore_override_for_fallback = (dialect == WireDialect::GeminiGenerateContent
        && raw_override.contains_ascii_icase("interactions"))
        || (dialect == WireDialect::OpenAiChat && raw_override.contains_ascii_icase("responses"));

    // 2. Explicit endpoint_path_override (skipped when falling back from a primary-only override).
    if !raw_override.is_empty() && !ignore_override_for_fallback {
        if raw_override.starts_with_http_scheme() {
            return Ok(raw_override.trim_end_matches('#').to_string());
        }
        let clean_override = format!(
            "/{}",
            raw_override.trim_end_matches('#').trim_start_matches('/')
        );
        return Ok(if base.ends_with_ascii_icase(&clean_override) {
            base.to_string()
        } else {
            format!("{base}{clean_override}")
        });
    }

    // 3. Base URL that already ends in a concrete endpoint tail is used as-is,
    //    except when falling back from a primary endpoint tail (`/interactions` or `/responses`).
    let effective_base = match dialect {
        WireDialect::GeminiGenerateContent => base
            .strip_suffix_ascii_icase("/interactions")
            .unwrap_or(base),
        WireDialect::OpenAiChat => base.strip_suffix_ascii_icase("/responses").unwrap_or(base),
        _ => base,
    };
    if effective_base == base {
        let lower_base = base.to_ascii_lowercase();
        if CONCRETE_ENDPOINT_TAILS.iter().any(|tail| {
            lower_base.ends_with(tail)
                || lower_base
                    .find(tail)
                    .is_some_and(|idx| lower_base[idx + tail.len()..].starts_with('?'))
        }) {
            return Ok(base.to_string());
        }
    }

    // 4. Version-agnostic root path resolution (/v1, /v2, /v1beta, /api/v3, /api/paas/v4, ...).
    let clean_model = model.trim().trim_start_matches('/').trim_prefix("models/");
    let (with_ver, without_ver) = match dialect {
        WireDialect::GeminiGenerateContent => (
            format!("/v1beta/models/{clean_model}:streamGenerateContent?alt=sse"),
            format!("/models/{clean_model}:streamGenerateContent?alt=sse"),
        ),
        WireDialect::OpenAiChat => (
            "/v1/chat/completions".to_string(),
            "/chat/completions".to_string(),
        ),
        WireDialect::OpenAiResponses => ("/v1/responses".to_string(), "/responses".to_string()),
        WireDialect::AnthropicMessages => ("/v1/messages".to_string(), "/messages".to_string()),
        WireDialect::GeminiInteractions => (
            "/v1beta/interactions".to_string(),
            "/interactions".to_string(),
        ),
    };
    let suffix = if has_api_version_suffix(effective_base) {
        without_ver
    } else {
        with_ver
    };
    Ok(format!("{effective_base}{suffix}"))
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

    let key = strip_invisible_credential_chars(api_key);
    let base_lower = profile.effective_base_url().to_ascii_lowercase();
    if !key.is_empty() {
        match profile.auth_header_mode {
            AuthHeaderMode::Native => match dialect {
                WireDialect::GeminiInteractions | WireDialect::GeminiGenerateContent => {
                    headers.push(("x-goog-api-key".to_string(), key.clone()));
                    let send_bearer = if profile.protocol.is_custom_wire() {
                        key.starts_with("sk-")
                    } else {
                        !base_lower.contains("googleapis.com") || key.starts_with("sk-")
                    };
                    if send_bearer {
                        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
                    }
                }
                WireDialect::AnthropicMessages => {
                    headers.push(("x-api-key".to_string(), key.clone()));
                    if !profile.protocol.is_custom_wire() && !base_lower.contains("anthropic.com") {
                        headers.push(("Authorization".to_string(), format!("Bearer {key}")));
                    }
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

    let mut custom_has_anthropic_version = false;
    let mut custom_headers = Vec::new();
    for line in profile.custom_headers_raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let kt = strip_invisible_credential_chars(k);
            let vt = strip_invisible_credential_chars(v);
            if !kt.is_empty() && !kt.chars().any(char::is_whitespace) && !vt.is_empty() {
                if kt.eq_ignore_ascii_case("anthropic-version") {
                    custom_has_anthropic_version = true;
                }
                custom_headers.push((kt, vt));
            }
        }
    }

    if dialect == WireDialect::AnthropicMessages {
        if !custom_has_anthropic_version {
            headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
        }
        if !profile.anthropic_beta_headers.trim().is_empty() {
            headers.push((
                "anthropic-beta".to_string(),
                profile.anthropic_beta_headers.trim().to_string(),
            ));
        }
    }

    headers.extend(custom_headers);
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

    let mut body_obj = match dialect {
        WireDialect::OpenAiResponses => build_openai_responses_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            &prev_chain_id,
            continue_prefix,
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
            continue_prefix,
            adapt,
        ),
        WireDialect::GeminiGenerateContent => build_gemini_generate_content_body(
            profile,
            model,
            &system_prompt,
            &late_note,
            source_nodes,
            tools,
            continue_prefix,
            adapt,
        ),
    };

    merge_raw_json_extra_body(&mut body_obj, &profile.raw_json_extra_body);

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

const DISALLOWED_EXTRA_BODY_KEYS: &[&str] = &[
    "tools",
    "tool_choice",
    "toolchoice",
    "tool_config",
    "toolconfig",
    "built_in_tools",
    "builtintools",
    "mcp_servers",
    "mcpservers",
    "functions",
    "function_call",
    "functioncall",
];

fn is_disallowed_extra_body_key(key: &str) -> bool {
    DISALLOWED_EXTRA_BODY_KEYS
        .iter()
        .any(|disallowed| key.eq_ignore_ascii_case(disallowed))
}

fn deep_merge_extra_body(target: &mut Map<String, Value>, patch: Map<String, Value>, depth: usize) {
    if depth > 16 {
        return;
    }
    for (k, v) in patch {
        if is_disallowed_extra_body_key(&k) {
            continue;
        }
        match (target.get_mut(&k), v) {
            (Some(Value::Object(dst)), Value::Object(src)) => {
                deep_merge_extra_body(dst, src, depth + 1);
            }
            (_, Value::Object(src)) => {
                let mut clean = Map::new();
                deep_merge_extra_body(&mut clean, src, depth + 1);
                target.insert(k, Value::Object(clean));
            }
            (_, other) => {
                target.insert(k, other);
            }
        }
    }
}

/// Deep-merges user-configured `raw_json_extra_body` into a wire request object.
/// Malformed or non-object JSON is ignored cleanly; tool-granting keys are stripped so
/// raw options can never smuggle tools past `AgentPermissionResolver`.
pub(crate) fn merge_raw_json_extra_body(body: &mut Map<String, Value>, raw_extra: &str) {
    let trimmed = raw_extra.trim();
    if trimmed.is_empty() {
        return;
    }
    let Ok(Value::Object(extra_map)) = serde_json::from_str::<Value>(trimmed) else {
        return;
    };
    deep_merge_extra_body(body, extra_map, 0);
}
