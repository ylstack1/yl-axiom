//! Context engineering for Axiom Coding turns.
//!
//! Three rules, in the order they are applied when a coding turn is assembled:
//!
//! 1. Stable cached prefix first — tools, then coding instructions, then late
//!    notes. Nothing that changes per turn is placed before this block, so the
//!    provider prompt cache keeps hitting.
//! 2. Quantized compaction instead of truncation — history is only compacted on
//!    a fixed step boundary, so the prefix stays byte-identical between
//!    boundaries and the cache is not invalidated by a partial rewrite.
//! 3. Tool-result clearing with an exclusion list — stale large tool output is
//!    dropped, but build and test evidence is never cleared, because that output
//!    is the verification record the coordinator depends on.

/// Compaction only happens on this step boundary, so the cached prefix stays
/// identical between boundaries.
pub const COMPACTION_STEP: usize = 16;

/// Tool output above this size is a candidate for clearing once it goes stale.
pub const CLEARABLE_OUTPUT_CHARS: usize = 4_096;

/// Tools whose output is verification evidence and must never be cleared.
const EVIDENCE_TOOLS: [&str; 4] = ["shell_exec", "cargo_test", "cargo_build", "cargo_clippy"];

/// A single assembled context section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSection {
    pub kind: SectionKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// Cached prefix: tool schemas and coding instructions. Never compacted.
    StablePrefix,
    /// Conversation history, compacted only on a step boundary.
    History,
    /// Tool results; stale ones are cleared unless they are evidence.
    ToolResults,
    /// Late notes and lore, appended last so the prefix stays stable.
    LateNotes,
}

/// Ordered context plan for one coding turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingContext {
    pub sections: Vec<ContextSection>,
    pub compacted: bool,
    pub cleared_results: usize,
}

/// True when `step` sits on a compaction boundary.
pub fn is_compaction_boundary(step: usize) -> bool {
    step > 0 && step % COMPACTION_STEP == 0
}

/// True when a tool's output is verification evidence and must be preserved.
pub fn is_evidence_tool(tool_name: &str) -> bool {
    EVIDENCE_TOOLS.contains(&tool_name)
}

/// Decides whether a tool result should be cleared from context.
///
/// A result is cleared only when it is stale, large, and not evidence. Small
/// results are kept because clearing them costs more cache than it saves.
pub fn should_clear_result(tool_name: &str, is_stale: bool, chars: usize) -> bool {
    is_stale && chars >= CLEARABLE_OUTPUT_CHARS && !is_evidence_tool(tool_name)
}

/// Assembles a turn's context in cache-friendly order.
///
/// `history` is compacted only when `step` is on a boundary; otherwise it is
/// passed through unchanged so the cached prefix remains byte-identical.
pub fn assemble(
    stable_prefix: &str,
    history: &str,
    step: usize,
    results: &[ToolResultContext],
    late_notes: &str,
) -> CodingContext {
    let compacted = is_compaction_boundary(step);
    let mut cleared = 0usize;
    let mut kept: Vec<&ToolResultContext> = Vec::with_capacity(results.len());

    for result in results {
        if should_clear_result(
            &result.tool_name,
            result.is_stale,
            result.text.chars().count(),
        ) {
            cleared += 1;
        } else {
            kept.push(result);
        }
    }

    let mut sections = Vec::with_capacity(4);
    if !stable_prefix.is_empty() {
        sections.push(ContextSection {
            kind: SectionKind::StablePrefix,
            text: stable_prefix.to_string(),
        });
    }
    sections.push(ContextSection {
        kind: SectionKind::History,
        text: if compacted {
            compact_history(history)
        } else {
            history.to_string()
        },
    });
    if !kept.is_empty() {
        let joined = kept
            .iter()
            .map(|r| format!("[{}] {}", r.tool_name, r.text))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(ContextSection {
            kind: SectionKind::ToolResults,
            text: joined,
        });
    }
    if !late_notes.is_empty() {
        sections.push(ContextSection {
            kind: SectionKind::LateNotes,
            text: late_notes.to_string(),
        });
    }

    CodingContext {
        sections,
        compacted,
        cleared_results: cleared,
    }
}

/// One tool result considered for clearing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultContext {
    pub tool_name: String,
    pub text: String,
    pub is_stale: bool,
}

/// Compacts history by keeping the most recent turns and replacing the rest
/// with a single marker line. Bounded: never returns more than the recent tail.
pub fn compact_history(history: &str) -> String {
    const KEEP_RECENT: usize = 8;
    let lines: Vec<&str> = history.lines().collect();
    if lines.len() <= KEEP_RECENT {
        return history.to_string();
    }
    let dropped = lines.len() - KEEP_RECENT;
    let mut out = String::with_capacity(history.len());
    out.push_str(&format!("[Compacted {dropped} earlier steps]\n"));
    for line in &lines[lines.len() - KEEP_RECENT..] {
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compaction_only_on_boundary() {
        assert!(!is_compaction_boundary(0));
        assert!(!is_compaction_boundary(15));
        assert!(is_compaction_boundary(16));
        assert!(is_compaction_boundary(32));
        assert!(!is_compaction_boundary(17));
    }

    #[test]
    fn evidence_tools_are_never_cleared() {
        assert!(is_evidence_tool("shell_exec"));
        assert!(is_evidence_tool("cargo_test"));
        assert!(!is_evidence_tool("fs_read"));
        // Even a large stale evidence result is preserved.
        assert!(!should_clear_result("shell_exec", true, 100_000));
    }

    #[test]
    fn only_large_stale_results_are_cleared() {
        assert!(should_clear_result("fs_read", true, 10_000));
        assert!(!should_clear_result("fs_read", true, 100));
        assert!(!should_clear_result("fs_read", false, 10_000));
    }

    #[test]
    fn stable_prefix_is_never_compacted() {
        let ctx = assemble("TOOLS+INSTRUCTIONS", "h1\nh2", 16, &[], "");
        assert_eq!(ctx.sections[0].kind, SectionKind::StablePrefix);
        assert_eq!(ctx.sections[0].text, "TOOLS+INSTRUCTIONS");
        assert!(ctx.compacted);
    }

    #[test]
    fn history_passthrough_off_boundary() {
        let ctx = assemble("P", "a\nb\nc", 17, &[], "");
        let history = ctx
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::History)
            .unwrap();
        assert_eq!(history.text, "a\nb\nc");
        assert!(!ctx.compacted);
    }

    #[test]
    fn compaction_keeps_recent_tail() {
        let long = (0..40)
            .map(|i| format!("step {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = compact_history(&long);
        assert!(out.starts_with("[Compacted 32 earlier steps]"));
        assert!(out.contains("step 39"));
        assert!(!out.contains("step 0\n"));
    }

    #[test]
    fn short_history_is_untouched() {
        let short = "a\nb";
        assert_eq!(compact_history(short), "a\nb");
    }

    #[test]
    fn cleared_results_are_counted_and_omitted() {
        let results = vec![
            ToolResultContext {
                tool_name: "fs_read".into(),
                text: "x".repeat(10_000),
                is_stale: true,
            },
            ToolResultContext {
                tool_name: "shell_exec".into(),
                text: "test result: ok".into(),
                is_stale: true,
            },
        ];
        let ctx = assemble("P", "h", 16, &results, "");
        assert_eq!(ctx.cleared_results, 1);
        let joined = ctx
            .sections
            .iter()
            .find(|s| s.kind == SectionKind::ToolResults)
            .unwrap();
        assert!(joined.text.contains("shell_exec"));
        assert!(!joined.text.contains("xxxxxxxx"));
    }

    #[test]
    fn late_notes_go_last() {
        let ctx = assemble("P", "h", 1, &[], "note");
        let last = ctx.sections.last().unwrap();
        assert_eq!(last.kind, SectionKind::LateNotes);
    }
}
