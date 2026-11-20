//! Prompt-cache-stable conversation history assembly, quantized 16-step stale tool compaction,
//! late-note keyword lore activation, and deterministic `ReplayToolCallIds`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use parchment_model::{
    ChatSession, ContextFragment, FragmentActivationMode, GoalTrackerState, MessageNode,
    MessageRole, MessageVariant,
};

pub const OUTPUT_LIMIT_NOTICE: &str = "\n\n*[Output truncated: max output tokens limit reached]*";
pub const COMPACTION_BATCH_STEPS: usize = 16;

const INLINE_ERROR_PREFIXES: [&str; 7] = [
    "**[Provider Error:",
    "**[Stream Error:",
    "**Error:**",
    "_Stopped before any text arrived._",
    "*[Stream paused/interrupted",
    "*[Stream interrupted",
    "*[Interrupted:",
];

const INLINE_ERROR_SUFFIXES: [&str; 4] = [
    OUTPUT_LIMIT_NOTICE,
    "\n\n*[Stream paused/interrupted:",
    "\n\n*[Stream interrupted",
    "\n\n*[Interrupted:",
];

/// Strips UI-only inline error banners and interruption footers before sending history back to LLMs.
pub fn strip_inline_error_notices(raw: &str) -> &str {
    let trimmed_start = raw.trim_start();
    if trimmed_start.is_empty()
        || INLINE_ERROR_PREFIXES
            .iter()
            .any(|p| trimmed_start.starts_with(p))
    {
        return "";
    }
    let mut end_idx = raw.len();
    for marker in INLINE_ERROR_SUFFIXES {
        if let Some(pos) = raw[..end_idx].find(marker) {
            end_idx = pos;
        }
    }
    raw[..end_idx].trim_end()
}

pub fn has_meaningful_assistant_context(v: &MessageVariant) -> bool {
    !strip_inline_error_notices(&v.content).trim().is_empty()
        || !v.thinking_summary.trim().is_empty()
        || !v.thinking_signature.trim().is_empty()
        || !v.tool_calls.is_empty()
        || !v.media_items.is_empty()
        || !v.attachments.is_empty()
}

pub fn sanitize_context_nodes(history: &[MessageNode]) -> Vec<&MessageNode> {
    history
        .iter()
        .filter(|node| {
            if !node.included_in_context {
                return false;
            }
            if node.role == MessageRole::Assistant {
                return node
                    .active_variant()
                    .is_some_and(has_meaningful_assistant_context);
            }
            true
        })
        .collect()
}

/// Quantized stale tool-output compaction (`COMPACTION_BATCH_STEPS = 16`) so prompt-cache
/// prefixes remain byte-identical across 16 tool steps instead of sliding every step.
pub fn compact_stale_tool_output_text<'a>(
    step_index: usize,
    total_steps: usize,
    tool_name: &str,
    raw_output: &'a str,
    keep_recent_count: usize,
    min_chars_to_compact: usize,
) -> Cow<'a, str> {
    if raw_output.len() < min_chars_to_compact {
        return Cow::Borrowed(raw_output);
    }
    let compactable_frontier = if keep_recent_count == 0 {
        total_steps
    } else {
        let raw_frontier = total_steps.saturating_sub(keep_recent_count);
        (raw_frontier / COMPACTION_BATCH_STEPS) * COMPACTION_BATCH_STEPS
    };
    if step_index >= compactable_frontier {
        return Cow::Borrowed(raw_output);
    }

    let head_len = floor_boundary(raw_output, 400);
    let tail_start = ceil_boundary(raw_output, raw_output.len().saturating_sub(400));
    let head = raw_output[..head_len].trim_end();
    let tail = raw_output[tail_start..].trim_start();
    Cow::Owned(format!(
        "[Compacted earlier `{tool_name}` output ({} chars total) — head/tail receipt]\n{head}\n...\n{tail}",
        raw_output.len()
    ))
}

pub fn compact_historical_tool_result_for_wire<'a>(
    raw_result: &'a str,
    tool_name: &str,
    is_older_assistant_turn: bool,
    call_index_in_turn: usize,
    total_calls_in_turn: usize,
) -> Cow<'a, str> {
    if is_older_assistant_turn {
        if raw_result.len() <= 2_000 {
            return Cow::Borrowed(raw_result);
        }
        return compact_stale_tool_output_text(
            call_index_in_turn,
            total_calls_in_turn.max(call_index_in_turn + 1),
            tool_name,
            raw_result,
            0,
            2_000,
        );
    }
    compact_stale_tool_output_text(
        call_index_in_turn,
        total_calls_in_turn,
        tool_name,
        raw_result,
        8,
        2_400,
    )
}

/// Evaluates keyword-triggered World Book / lorebook fragments against the last 8 turns.
/// Returned separately so callers inject them as a late note rather than mutating the static system prefix.
pub fn triggered_keyword_fragments<'a>(
    session: &'a ChatSession,
    recent_turns: &[&MessageNode],
) -> Vec<&'a ContextFragment> {
    let has_any_kw = session
        .fragments
        .iter()
        .chain(
            session
                .active_card
                .iter()
                .flat_map(|c| c.embedded_fragments.iter()),
        )
        .any(|f| f.mode == FragmentActivationMode::Keyword);
    if !has_any_kw {
        return Vec::new();
    }

    let window_start = recent_turns.len().saturating_sub(8);
    let slice = &recent_turns[window_start..];
    let total_len: usize = slice.iter().map(|n| n.active_content().len() + 1).sum();
    let mut joined_lower = String::with_capacity(total_len);
    for n in slice {
        if !joined_lower.is_empty() {
            joined_lower.push('\n');
        }
        for b in n.active_content().chars() {
            joined_lower.push(b.to_ascii_lowercase());
        }
    }

    let mut out = Vec::new();
    let mut kw_buf = String::new();
    let mut check_list = |list: &'a [ContextFragment]| {
        for frag in list {
            if frag.mode != FragmentActivationMode::Keyword {
                continue;
            }
            let matched = frag.keywords.iter().any(|raw_kw| {
                let trimmed = raw_kw.trim();
                if trimmed.is_empty() {
                    return false;
                }
                kw_buf.clear();
                for ch in trimmed.chars() {
                    kw_buf.push(ch.to_ascii_lowercase());
                }
                joined_lower.contains(&kw_buf)
            });
            if matched {
                out.push(frag);
            }
        }
    };
    check_list(&session.fragments);
    if let Some(card) = &session.active_card {
        check_list(&card.embedded_fragments);
    }
    out
}

/// Builds the static cacheable system instruction (system prompt + persona card + pinned fragments
/// + continuity summary + goal state). Keyword lore is intentionally omitted here and injected late.
pub fn build_compiled_system_prompt(
    session: &ChatSession,
    goal_state: Option<&GoalTrackerState>,
) -> String {
    let mut out = String::new();
    let push_section = |out: &mut String, text: &str| {
        if text.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(text);
    };

    push_section(&mut out, session.system_prompt.trim());
    if let Some(card) = &session.active_card {
        let compiled = card.build_compiled_instructions(&session.user_name);
        push_section(&mut out, &compiled);
    }

    let mut wrote_pinned_header = false;
    for f in session.fragments.iter().chain(
        session
            .active_card
            .iter()
            .flat_map(|c| c.embedded_fragments.iter()),
    ) {
        if f.mode != FragmentActivationMode::Pinned || f.content.trim().is_empty() {
            continue;
        }
        if !wrote_pinned_header {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str("### Pinned World Book / Context");
            wrote_pinned_header = true;
        }
        out.push('\n');
        let title = f.title.trim();
        if !title.is_empty() {
            out.push_str("#### ");
            out.push_str(title);
            out.push('\n');
        }
        out.push_str(f.content.trim());
        out.push('\n');
    }
    while out.ends_with('\n') {
        out.pop();
    }

    let summary = session.story_summary.trim();
    if !summary.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("### Continuity Summary\n");
        out.push_str(summary);
    }

    if let Some(goal) = goal_state {
        let obj = goal.goal_objective.trim();
        if !obj.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str("### Active Goal\n");
            out.push_str(obj);
        }
    }

    out
}

/// Formats triggered keyword lore + author's note as a late prompt note so the system prefix stays cached.
pub fn build_late_context_note(session: &ChatSession, history: &[&MessageNode]) -> String {
    let mut out = String::new();
    let kw_frags = triggered_keyword_fragments(session, history);
    if !kw_frags.is_empty() {
        out.push_str("[Triggered World Book Notes:\n");
        for f in kw_frags {
            let title = f.title.trim();
            if !title.is_empty() {
                out.push_str("- ");
                out.push_str(title);
                out.push_str(": ");
            }
            out.push_str(f.content.trim());
            out.push('\n');
        }
        out.push(']');
    }
    let note = session.authors_note.trim();
    if !note.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("[Author's Note: ");
        out.push_str(note);
        out.push(']');
    }
    if let Some(card) = &session.active_card {
        let phi = card.post_history_instructions.trim();
        if !phi.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(phi);
        }
    }
    out
}

/// Condenses the oldest 50% of active context turns into a compact Continuity Digest on context overflow.
pub fn compact_history_nodes_on_overflow(nodes: &[MessageNode], now_ms: i64) -> Vec<MessageNode> {
    let active_indices: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.included_in_context.then_some(i))
        .collect();
    if active_indices.len() <= 4 {
        return nodes.to_vec();
    }
    let compact_count = (active_indices.len() / 2).max(2);
    let to_compact: HashSet<usize> = active_indices.into_iter().take(compact_count).collect();

    let mut digest_lines = Vec::new();
    for (idx, n) in nodes.iter().enumerate() {
        if !to_compact.contains(&idx) {
            continue;
        }
        let speaker = match n.role {
            MessageRole::Assistant => "Assistant",
            _ => "User",
        };
        let clean = strip_inline_error_notices(n.active_content());
        let collapsed: String = clean
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(260)
            .collect();
        if !collapsed.is_empty() {
            digest_lines.push(format!("- {speaker}: {collapsed}"));
        }
    }
    let digest_text = format!(
        "[Auto-Compacted Earlier Conversation Continuity Digest ({} turns condensed):\n{}]",
        to_compact.len(),
        digest_lines
            .into_iter()
            .take(24)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut out = Vec::with_capacity(nodes.len() + 1);
    out.push(MessageNode {
        id: format!("node-auto-digest-{now_ms}"),
        role: MessageRole::User,
        variants: vec![MessageVariant {
            id: "var-auto-digest".to_string(),
            content: digest_text,
            created_at: now_ms,
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    });
    for (idx, n) in nodes.iter().enumerate() {
        let mut cloned = n.clone();
        if to_compact.contains(&idx) {
            cloned.included_in_context = false;
        }
        out.push(cloned);
    }
    out
}

/// Deterministic, wire-safe tool call ID mapper (`ReplayToolCallIds` contract).
/// Sanitizes IDs to `[A-Za-z0-9_-]` (max 64 chars) and guarantees uniqueness across replayed history.
#[derive(Default)]
pub struct ReplayToolCallIds {
    assigned: HashMap<String, String>,
    used: HashSet<String>,
    seq: usize,
}

impl ReplayToolCallIds {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(&mut self, raw_id: &str, prefix: &str) -> String {
        if !raw_id.trim().is_empty()
            && let Some(existing) = self.assigned.get(raw_id)
        {
            return existing.clone();
        }
        let sanitized: String = raw_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect();
        let candidate = if !sanitized.is_empty() && !self.used.contains(&sanitized) {
            sanitized
        } else {
            loop {
                self.seq += 1;
                let next = format!("{prefix}_{}", self.seq);
                if !self.used.contains(&next) {
                    break next;
                }
            }
        };
        self.used.insert(candidate.clone());
        if !raw_id.trim().is_empty() {
            self.assigned.insert(raw_id.to_string(), candidate.clone());
        }
        candidate
    }
}

fn floor_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn ceil_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantized_16_step_compaction_keeps_prefix_stable_until_boundary() {
        let big = "a".repeat(3_000);
        // At 20 total steps with keep_recent=8, raw frontier=12 -> quantized to 0 (nothing compacted yet).
        assert_eq!(
            compact_stale_tool_output_text(0, 20, "fs_read", &big, 8, 2_400),
            big
        );
        // At 24 total steps with keep_recent=8, raw frontier=16 -> quantized to 16 (steps 0..16 compacted).
        let compacted = compact_stale_tool_output_text(0, 24, "fs_read", &big, 8, 2_400);
        assert!(compacted.contains("[Compacted earlier `fs_read` output"));
        assert_eq!(
            compact_stale_tool_output_text(16, 24, "fs_read", &big, 8, 2_400),
            big
        );
    }

    #[test]
    fn replay_tool_call_ids_sanitizes_and_deduplicates() {
        let mut ids = ReplayToolCallIds::new();
        assert_eq!(ids.resolve("call:abc/1", "call"), "call_abc_1");
        assert_eq!(ids.resolve("call:abc/1", "call"), "call_abc_1");
        let second = ids.resolve("call_abc_1", "call");
        assert_ne!(second, "call_abc_1");
    }
}
