//! Opaque reasoning signatures, Franken-signature guards, effort downgrade ladder,
//! and cross-provider reasoning handoff (8,000-char deterministic 25% head + 75% tail).

use std::borrow::Cow;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use parchment_model::{
    CUSTOM_DIALECT_ANTHROPIC, CUSTOM_DIALECT_GEMINI, CUSTOM_DIALECT_OPENAI_CHAT,
    CUSTOM_DIALECT_OPENAI_RESPONSES, MessageNode, MessageRole, MessageVariant, ProviderProtocol,
};

pub const REASONING_HANDOFF_MARKER: &str = "[Prior model reasoning summary:]";
pub const MAX_REASONING_HANDOFF_CHARS: usize = 8_000;

const GEMINI_SIG_MAGIC: [u8; 5] = [0x01, 0x69, 0x14, 0x7d, 0x13];
const GEMINI_SERVER_TOOL_TAG: [u8; 3] = [0x08, 0x02, 0x12];

fn find_subarray(haystack: &[u8], needle: &[u8], end_exclusive: usize) -> Option<usize> {
    let limit_end = haystack.len().min(end_exclusive);
    if needle.is_empty() || limit_end < needle.len() {
        return None;
    }
    haystack[..limit_end]
        .windows(needle.len())
        .position(|w| w == needle)
}

pub fn is_gemini_server_side_tool_signature(seg: &str) -> bool {
    if seg.len() < 16 || seg.starts_with("gAAAAAB") {
        return false;
    }
    let Ok(decoded) = BASE64_STANDARD.decode(seg.trim()) else {
        return false;
    };
    let Some(magic_pos) = find_subarray(&decoded, &GEMINI_SIG_MAGIC, decoded.len()) else {
        return false;
    };
    if magic_pos == 0 {
        return false;
    }
    find_subarray(&decoded, &GEMINI_SERVER_TOOL_TAG, magic_pos).is_some()
}

pub fn is_complete_gemini_signature_protobuf(seg: &str) -> bool {
    if seg.len() < 16 || seg.starts_with("gAAAAAB") {
        return false;
    }
    let Ok(decoded) = BASE64_STANDARD.decode(seg.trim()) else {
        return false;
    };
    find_subarray(&decoded, &GEMINI_SIG_MAGIC, decoded.len()).is_some()
}

fn has_mid_base64_padding(trimmed: &str) -> bool {
    let without_trailing_eq = trimmed.trim_end_matches('=');
    without_trailing_eq.contains('=')
}

fn extract_base64_segments(raw: &str) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if is_b64_body(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_b64_body(bytes[i]) {
                i += 1;
            }
            let mut eqs = 0;
            while i < bytes.len() && bytes[i] == b'=' && eqs < 2 {
                i += 1;
                eqs += 1;
            }
            let seg = &raw[start..i];
            if seg.len() >= 16 {
                out.push(seg);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_b64_body(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/'
}

/// Sanitizes a raw or historically concatenated Gemini `thoughtSignature`:
/// - Splits glued Base64 segments (`...=...==...==`) without mutating valid opaque blobs.
/// - When `for_function_call` is true, filters out server-side `google_search` (`08 02 12`)
///   signatures so `/v1beta/interactions` and `:streamGenerateContent` never fail with HTTP 400.
pub fn sanitize_gemini_thought_signature(raw_sig: &str, for_function_call: bool) -> String {
    let trimmed = raw_sig.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed == "context_engineering_is_the_way_to_go"
        || trimmed == "skip_thought_signature_validator"
    {
        return if for_function_call {
            trimmed.to_string()
        } else {
            String::new()
        };
    }
    if trimmed.starts_with("gAAAAAB") {
        return String::new();
    }
    let mid_padding = has_mid_base64_padding(trimmed);
    let is_real_gemini_pb = is_complete_gemini_signature_protobuf(trimmed)
        || is_gemini_server_side_tool_signature(trimmed);
    if !mid_padding && !is_real_gemini_pb {
        return trimmed.to_string();
    }
    let segments = extract_base64_segments(trimmed);
    if segments.is_empty() {
        return String::new();
    }
    if for_function_call {
        if let Some(seg) = segments.iter().find(|&&s| {
            is_complete_gemini_signature_protobuf(s) && !is_gemini_server_side_tool_signature(s)
        }) {
            return (*seg).to_string();
        }
        if let Some(seg) = segments.iter().find(|&&s| {
            !is_gemini_server_side_tool_signature(s) && BASE64_STANDARD.decode(s).is_ok()
        }) {
            return (*seg).to_string();
        }
        String::new()
    } else {
        if let Some(seg) = segments.iter().rev().find(|&&s| {
            is_complete_gemini_signature_protobuf(s) && !is_gemini_server_side_tool_signature(s)
        }) {
            return (*seg).to_string();
        }
        segments
            .iter()
            .rev()
            .find(|&&s| BASE64_STANDARD.decode(s).is_ok())
            .map(|&s| s.to_string())
            .unwrap_or_default()
    }
}

pub fn normalize_reasoning_model_id(name: &str) -> &str {
    let trimmed = name.trim();
    trimmed.strip_prefix("models/").unwrap_or(trimmed)
}

pub fn is_same_reasoning_model(saved_model: &str, current_model: &str, allow_blank: bool) -> bool {
    let s = normalize_reasoning_model_id(saved_model);
    let c = normalize_reasoning_model_id(current_model);
    if s.is_empty() || c.is_empty() {
        return allow_blank;
    }
    s.eq_ignore_ascii_case(c)
}

/// Maps a rejected reasoning level along the downgrade/step-up ladder (`max -> xhigh -> high`,
/// or `minimal -> low` when minimal is explicitly rejected).
pub fn step_down_reasoning_effort(current_effort: &str, err_msg: &str) -> Option<&'static str> {
    match current_effort.trim().to_ascii_lowercase().as_str() {
        "max" => Some("xhigh"),
        "xhigh" => Some("high"),
        "minimal" => {
            let lower = err_msg.to_ascii_lowercase();
            if lower.contains("minimal")
                || lower.contains("effort")
                || lower.contains("reasoning")
                || lower.contains("unsupported_value")
                || lower.contains("invalid_value")
            {
                Some("low")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn effective_wire_dialect(variant: &MessageVariant) -> String {
    let stamped = variant.provider_wire_dialect.trim().to_ascii_lowercase();
    if !stamped.is_empty() {
        return stamped;
    }
    match variant.provider_protocol {
        Some(ProviderProtocol::GeminiInteractions) => CUSTOM_DIALECT_GEMINI.to_string(),
        Some(ProviderProtocol::OpenAiResponses) => CUSTOM_DIALECT_OPENAI_RESPONSES.to_string(),
        Some(ProviderProtocol::AnthropicMessages) => CUSTOM_DIALECT_ANTHROPIC.to_string(),
        _ => String::new(),
    }
}

pub fn is_valid_openai_encrypted_reasoning(
    raw_sig: &str,
    saved_protocol: Option<ProviderProtocol>,
    saved_model: &str,
    current_model: &str,
    saved_wire_dialect: &str,
) -> bool {
    if let Some(proto) = saved_protocol
        && proto != ProviderProtocol::OpenAiResponses
    {
        return false;
    }
    if saved_wire_dialect.eq_ignore_ascii_case(CUSTOM_DIALECT_OPENAI_CHAT) {
        return false;
    }
    if !is_same_reasoning_model(saved_model, current_model, true) {
        return false;
    }
    let trimmed = raw_sig.trim();
    if trimmed.is_empty() || trimmed.contains(' ') || has_mid_base64_padding(trimmed) {
        return false;
    }
    !is_complete_gemini_signature_protobuf(trimmed)
        && !is_gemini_server_side_tool_signature(trimmed)
}

pub fn is_valid_anthropic_signature(
    raw_sig: &str,
    saved_protocol: Option<ProviderProtocol>,
    saved_model: &str,
    current_model: &str,
) -> bool {
    if let Some(proto) = saved_protocol
        && proto != ProviderProtocol::AnthropicMessages
    {
        return false;
    }
    if !is_same_reasoning_model(saved_model, current_model, true) {
        return false;
    }
    let trimmed = raw_sig.trim();
    if trimmed.is_empty() || trimmed.starts_with("gAAAAAB") || has_mid_base64_padding(trimmed) {
        return false;
    }
    !is_complete_gemini_signature_protobuf(trimmed)
        && !is_gemini_server_side_tool_signature(trimmed)
}

/// Appends an incoming signature chunk while preventing distinct complete Base64 signatures
/// from concatenating into an invalid Franken-signature.
pub fn append_signature_chunk_safe(builder: &mut String, incoming: &str) -> String {
    if incoming.is_empty() {
        return String::new();
    }
    if builder == incoming || builder.ends_with(incoming) {
        return String::new();
    }
    if builder.is_empty() {
        builder.push_str(incoming);
        return incoming.to_string();
    }
    let cur_ends_padding = builder.ends_with('=');
    let both_complete_gemini = is_complete_gemini_signature_protobuf(builder)
        && is_complete_gemini_signature_protobuf(incoming);
    if cur_ends_padding || both_complete_gemini {
        if is_gemini_server_side_tool_signature(incoming)
            && !is_gemini_server_side_tool_signature(builder)
        {
            return String::new();
        }
        builder.clear();
        builder.push_str(incoming);
        return incoming.to_string();
    }
    builder.push_str(incoming);
    incoming.to_string()
}

/// A server-side chain is reusable ONLY by matching protocol, non-blank normalized model, and wire dialect.
pub fn reusable_provider_chain_id(
    previous: &MessageVariant,
    protocol: ProviderProtocol,
    model: &str,
    current_wire_dialect: &str,
) -> String {
    let target_dialect = current_wire_dialect.trim().to_ascii_lowercase();
    let prev_dialect = effective_wire_dialect(previous);
    if !target_dialect.is_empty() && !prev_dialect.is_empty() && target_dialect != prev_dialect {
        return String::new();
    }
    let prev_model = normalize_reasoning_model_id(&previous.model_used);
    let cur_model = normalize_reasoning_model_id(model);
    let same_model = !prev_model.is_empty()
        && !cur_model.is_empty()
        && prev_model.eq_ignore_ascii_case(cur_model);
    if previous.provider_protocol == Some(protocol) && same_model {
        previous.provider_interaction_id.clone()
    } else {
        String::new()
    }
}

pub fn is_native_reasoning_compatible(
    variant: &MessageVariant,
    target_protocol: ProviderProtocol,
    target_wire_dialect: &str,
    current_model: &str,
) -> bool {
    if variant.content.contains(REASONING_HANDOFF_MARKER) {
        return false;
    }
    if !is_same_reasoning_model(&variant.model_used, current_model, true) {
        return false;
    }
    let target_dialect = target_wire_dialect.trim().to_ascii_lowercase();
    let born_dialect = effective_wire_dialect(variant);
    if !target_dialect.is_empty() && !born_dialect.is_empty() {
        return born_dialect == target_dialect;
    }
    match variant.provider_protocol {
        Some(proto) => proto == target_protocol,
        None => true,
    }
}

fn has_missing_native_tool_signature(
    variant: &MessageVariant,
    target_protocol: ProviderProtocol,
    target_wire_dialect: &str,
) -> bool {
    if variant.tool_calls.is_empty() || variant.provider_protocol.is_none() {
        return false;
    }
    let resolved = {
        let d = target_wire_dialect.trim().to_ascii_lowercase();
        if !d.is_empty() {
            d
        } else {
            match target_protocol {
                ProviderProtocol::GeminiInteractions => CUSTOM_DIALECT_GEMINI.to_string(),
                ProviderProtocol::AnthropicMessages => CUSTOM_DIALECT_ANTHROPIC.to_string(),
                _ => String::new(),
            }
        }
    };
    match resolved.as_str() {
        CUSTOM_DIALECT_GEMINI => {
            sanitize_gemini_thought_signature(&variant.thinking_signature, false).is_empty()
                && variant.tool_calls.iter().all(|tc| {
                    sanitize_gemini_thought_signature(&tc.thought_signature, true).is_empty()
                })
        }
        CUSTOM_DIALECT_ANTHROPIC => {
            !is_valid_anthropic_signature(
                &variant.thinking_signature,
                variant.provider_protocol,
                &variant.model_used,
                "",
            ) && variant.tool_calls.iter().all(|tc| {
                !is_valid_anthropic_signature(
                    &tc.thought_signature,
                    variant.provider_protocol,
                    &variant.model_used,
                    "",
                )
            })
        }
        CUSTOM_DIALECT_OPENAI_RESPONSES => {
            let born = effective_wire_dialect(variant);
            !is_valid_openai_encrypted_reasoning(
                &variant.thinking_signature,
                variant.provider_protocol,
                &variant.model_used,
                "",
                &born,
            ) && variant.tool_calls.iter().all(|tc| {
                !is_valid_openai_encrypted_reasoning(
                    &tc.thought_signature,
                    variant.provider_protocol,
                    &variant.model_used,
                    "",
                    &born,
                )
            })
        }
        _ => false,
    }
}

/// Deterministic 25% head + 75% tail reasoning handoff clip (`MAX_REASONING_HANDOFF_CHARS = 8,000`).
pub fn clip_reasoning_summary_for_handoff(summary: &str, max_chars: usize) -> String {
    let char_count = summary.chars().count();
    if char_count <= max_chars {
        return summary.to_string();
    }
    const MARKER: &str = "\n\n[...middle reasoning omitted...]\n\n";
    let marker_chars = MARKER.chars().count();
    if max_chars <= marker_chars + 16 {
        let end = summary
            .char_indices()
            .nth(max_chars)
            .map_or(summary.len(), |(i, _)| i);
        return summary[..end].to_string();
    }
    let budget = max_chars - marker_chars;
    let head_chars = budget / 4;
    let tail_chars = budget - head_chars;
    let head_end = summary
        .char_indices()
        .nth(head_chars)
        .map_or(summary.len(), |(i, _)| i);
    let tail_start = summary
        .char_indices()
        .nth(char_count.saturating_sub(tail_chars))
        .map_or(summary.len(), |(i, _)| i);
    let head_trimmed = summary[..head_end].trim_end();
    let tail_trimmed = summary[tail_start..].trim_start();
    if head_trimmed.is_empty() {
        let end = tail_trimmed
            .char_indices()
            .nth(max_chars)
            .map_or(tail_trimmed.len(), |(i, _)| i);
        return tail_trimmed[..end].to_string();
    }
    format!("{head_trimmed}{MARKER}{tail_trimmed}")
}

/// Carries prior reasoning narrative across providers/models as plain text (capped at 8,000 chars),
/// shifting the first tool step's `preceding_text` so interleaved step ordering is preserved.
/// Borrows every native-compatible node with zero clones.
pub fn apply_cross_provider_reasoning_handoff<'a>(
    nodes: &[&'a MessageNode],
    current_protocol: ProviderProtocol,
    chaining_active: bool,
    current_wire_dialect: &str,
    current_model: &str,
    strip_inline_errors: impl Fn(&str) -> &str,
) -> Vec<Cow<'a, MessageNode>> {
    if chaining_active {
        return nodes.iter().map(|&n| Cow::Borrowed(n)).collect();
    }
    nodes
        .iter()
        .map(|&node| {
            if node.role != MessageRole::Assistant || !node.included_in_context {
                return Cow::Borrowed(node);
            }
            let Some(variant) = node.active_variant() else {
                return Cow::Borrowed(node);
            };
            let summary = variant.thinking_summary.trim();
            if summary.is_empty() || variant.content.contains(REASONING_HANDOFF_MARKER) {
                return Cow::Borrowed(node);
            }
            let native_compat = is_native_reasoning_compatible(
                variant,
                current_protocol,
                current_wire_dialect,
                current_model,
            );
            let missing_sig = native_compat
                && has_missing_native_tool_signature(
                    variant,
                    current_protocol,
                    current_wire_dialect,
                );
            if native_compat && !missing_sig {
                return Cow::Borrowed(node);
            }

            let clean_base = strip_inline_errors(&variant.content);
            let has_tools = !variant.tool_calls.is_empty();
            let handoff_header = format!(
                "{REASONING_HANDOFF_MARKER}\n{}",
                clip_reasoning_summary_for_handoff(summary, MAX_REASONING_HANDOFF_CHARS)
            );
            let handoff_prefix = format!("{handoff_header}\n\n");
            let handoff_content = if has_tools || !clean_base.trim().is_empty() {
                format!("{handoff_prefix}{clean_base}")
            } else {
                handoff_header
            };

            let mut updated_node = node.clone();
            let idx = updated_node
                .active_variant_index
                .min(updated_node.variants.len().saturating_sub(1));
            if let Some(v) = updated_node.variants.get_mut(idx) {
                v.content = handoff_content;
                if let Some(first_tool) = v.tool_calls.first_mut() {
                    first_tool.preceding_text =
                        format!("{handoff_prefix}{}", first_tool.preceding_text);
                }
            }
            Cow::Owned(updated_node)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parchment_model::ToolCallRecord;

    #[test]
    fn franken_signature_guard_splits_and_filters_server_tool_signatures() {
        // Real Gemini thought signature payload containing GEMINI_SIG_MAGIC (`01 69 14 7d 13`).
        let mut user_pb = vec![0x12, 0x10, 0x0a, 0x01, 0x01];
        user_pb.extend_from_slice(&GEMINI_SIG_MAGIC);
        user_pb.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        let user_sig = BASE64_STANDARD.encode(&user_pb);

        // Server-side google_search signature containing `08 02 12` before magic.
        let mut server_pb = vec![0x08, 0x02, 0x12, 0x08];
        server_pb.extend_from_slice(&GEMINI_SIG_MAGIC);
        server_pb.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55]);
        let server_sig = BASE64_STANDARD.encode(&server_pb);

        assert!(is_complete_gemini_signature_protobuf(&user_sig));
        assert!(!is_gemini_server_side_tool_signature(&user_sig));
        assert!(is_gemini_server_side_tool_signature(&server_sig));

        let glued = format!("{server_sig}{user_sig}");
        assert_eq!(sanitize_gemini_thought_signature(&glued, true), user_sig);

        // append_signature_chunk_safe refuses to overwrite a valid user sig with a server tool sig.
        let mut buf = user_sig.clone();
        assert_eq!(append_signature_chunk_safe(&mut buf, &server_sig), "");
        assert_eq!(buf, user_sig);
    }

    #[test]
    fn cross_provider_handoff_clips_8k_and_shifts_tool_preceding_text() {
        let long_summary = format!("HEAD_{}_TAIL", "x".repeat(12_000));
        let clipped =
            clip_reasoning_summary_for_handoff(&long_summary, MAX_REASONING_HANDOFF_CHARS);
        assert!(clipped.chars().count() <= MAX_REASONING_HANDOFF_CHARS);
        assert!(clipped.starts_with("HEAD_"));
        assert!(clipped.ends_with("_TAIL"));
        assert!(clipped.contains("[...middle reasoning omitted...]"));

        let nodes = [MessageNode {
            id: "a1".into(),
            role: MessageRole::Assistant,
            variants: vec![MessageVariant {
                id: "v1".into(),
                content: "Step 1 prose. Final answer.".into(),
                thinking_summary: "Cross-provider thought.".into(),
                provider_protocol: Some(ProviderProtocol::AnthropicMessages),
                provider_wire_dialect: CUSTOM_DIALECT_ANTHROPIC.into(),
                model_used: "claude-opus-4-6".into(),
                tool_calls: vec![ToolCallRecord {
                    id: "tc1".into(),
                    name: "fs_read".into(),
                    arguments_json: "{}".into(),
                    preceding_text: "Step 1 prose.".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        }];
        let node_refs: Vec<&MessageNode> = nodes.iter().collect();
        let out = apply_cross_provider_reasoning_handoff(
            &node_refs,
            ProviderProtocol::OpenAiResponses,
            false,
            CUSTOM_DIALECT_OPENAI_RESPONSES,
            "gpt-5.4",
            |s| s,
        );
        let v = &out[0].variants[0];
        assert!(v.content.starts_with(REASONING_HANDOFF_MARKER));
        assert!(
            v.tool_calls[0]
                .preceding_text
                .starts_with(REASONING_HANDOFF_MARKER)
        );
        assert!(v.tool_calls[0].preceding_text.ends_with("Step 1 prose."));
    }

    #[test]
    fn effort_ladder_downgrades_max_xhigh_and_steps_up_minimal() {
        assert_eq!(step_down_reasoning_effort("max", "bad"), Some("xhigh"));
        assert_eq!(step_down_reasoning_effort("xhigh", "bad"), Some("high"));
        assert_eq!(step_down_reasoning_effort("high", "bad"), None);
        assert_eq!(
            step_down_reasoning_effort("minimal", "unsupported_value for reasoning effort"),
            Some("low")
        );
        assert_eq!(
            step_down_reasoning_effort("minimal", "unrelated error"),
            None
        );
    }
}
