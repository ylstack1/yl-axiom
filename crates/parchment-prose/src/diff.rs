//! Unified line diff engine (LCS with prefix/suffix trimming and collapsed unchanged context runs)
//! plus parser for ```` ```diff ```` Markdown blocks.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLineKind {
    Context,
    Removed,
    Added,
    Collapsed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub text: String,
}

/// Computes a unified line diff with unchanged runs > 6 lines collapsed into a summary row.
pub fn compute_unified_line_diff(old_text: &str, new_text: &str) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = if old_text.is_empty() {
        Vec::new()
    } else {
        old_text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect()
    };
    let new_lines: Vec<&str> = if new_text.is_empty() {
        Vec::new()
    } else {
        new_text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect()
    };

    if old_lines.is_empty() && new_lines.is_empty() {
        return Vec::new();
    }
    if old_lines.is_empty() {
        return new_lines
            .iter()
            .take(200)
            .enumerate()
            .map(|(idx, line)| DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(idx + 1),
                text: (*line).to_string(),
            })
            .collect();
    }
    if new_lines.is_empty() {
        return old_lines
            .iter()
            .take(200)
            .enumerate()
            .map(|(idx, line)| DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(idx + 1),
                new_line: None,
                text: (*line).to_string(),
            })
            .collect();
    }

    let min_total = old_lines.len().min(new_lines.len());
    let mut prefix_len = 0usize;
    while prefix_len < min_total && old_lines[prefix_len] == new_lines[prefix_len] {
        prefix_len += 1;
    }
    let mut suffix_len = 0usize;
    while suffix_len < (min_total - prefix_len)
        && old_lines[old_lines.len() - 1 - suffix_len]
            == new_lines[new_lines.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let mid_old = &old_lines[prefix_len..old_lines.len() - suffix_len];
    let mid_new = &new_lines[prefix_len..new_lines.len() - suffix_len];

    let mut raw = Vec::with_capacity(old_lines.len().max(new_lines.len()) + 8);
    for (idx, line) in old_lines[..prefix_len].iter().enumerate() {
        raw.push(DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(idx + 1),
            new_line: Some(idx + 1),
            text: (*line).to_string(),
        });
    }

    append_middle_lcs(mid_old, mid_new, prefix_len, &mut raw);

    let old_suf_start = old_lines.len() - suffix_len;
    let new_suf_start = new_lines.len() - suffix_len;
    for k in 0..suffix_len {
        raw.push(DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(old_suf_start + k + 1),
            new_line: Some(new_suf_start + k + 1),
            text: old_lines[old_suf_start + k].to_string(),
        });
    }

    collapse_context_runs(raw)
}

fn append_middle_lcs(
    mid_old: &[&str],
    mid_new: &[&str],
    prefix_offset: usize,
    out: &mut Vec<DiffLine>,
) {
    const MAX_DIM: usize = 360;
    if mid_old.is_empty() && mid_new.is_empty() {
        return;
    }
    if mid_old.is_empty() {
        for (idx, line) in mid_new.iter().take(200).enumerate() {
            out.push(DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(prefix_offset + idx + 1),
                text: (*line).to_string(),
            });
        }
        return;
    }
    if mid_new.is_empty() {
        for (idx, line) in mid_old.iter().take(200).enumerate() {
            out.push(DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(prefix_offset + idx + 1),
                new_line: None,
                text: (*line).to_string(),
            });
        }
        return;
    }
    if mid_old.len() > MAX_DIM || mid_new.len() > MAX_DIM {
        for (idx, line) in mid_old.iter().take(120).enumerate() {
            out.push(DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(prefix_offset + idx + 1),
                new_line: None,
                text: (*line).to_string(),
            });
        }
        for (idx, line) in mid_new.iter().take(120).enumerate() {
            out.push(DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(prefix_offset + idx + 1),
                text: (*line).to_string(),
            });
        }
        return;
    }

    let m = mid_old.len();
    let n = mid_new.len();
    let stride = n + 1;
    let mut dp = vec![0u32; (m + 1) * stride];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i * stride + j] = if mid_old[i] == mid_new[j] {
                1 + dp[(i + 1) * stride + (j + 1)]
            } else {
                dp[(i + 1) * stride + j].max(dp[i * stride + (j + 1)])
            };
        }
    }

    let mut i = 0usize;
    let mut j = 0usize;
    while i < m && j < n {
        if mid_old[i] == mid_new[j] {
            out.push(DiffLine {
                kind: DiffLineKind::Context,
                old_line: Some(prefix_offset + i + 1),
                new_line: Some(prefix_offset + j + 1),
                text: mid_old[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if dp[(i + 1) * stride + j] >= dp[i * stride + (j + 1)] {
            out.push(DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(prefix_offset + i + 1),
                new_line: None,
                text: mid_old[i].to_string(),
            });
            i += 1;
        } else {
            out.push(DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(prefix_offset + j + 1),
                text: mid_new[j].to_string(),
            });
            j += 1;
        }
    }
    while i < m {
        out.push(DiffLine {
            kind: DiffLineKind::Removed,
            old_line: Some(prefix_offset + i + 1),
            new_line: None,
            text: mid_old[i].to_string(),
        });
        i += 1;
    }
    while j < n {
        out.push(DiffLine {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(prefix_offset + j + 1),
            text: mid_new[j].to_string(),
        });
        j += 1;
    }
}

fn collapse_context_runs(lines: Vec<DiffLine>) -> Vec<DiffLine> {
    let total = lines.len();
    let mut out = Vec::with_capacity(total);
    let mut ctx_run: Vec<DiffLine> = Vec::new();
    let flush_ctx =
        |ctx_run: &mut Vec<DiffLine>, out: &mut Vec<DiffLine>, at_start: bool, at_end: bool| {
            let run_len = ctx_run.len();
            if run_len == 0 {
                return;
            }
            if run_len <= 6 {
                out.append(ctx_run);
                return;
            }
            let keep_top = if at_start { 0 } else { 3 };
            let keep_bottom = if at_end { 0 } else { 3 };
            let hidden = run_len.saturating_sub(keep_top + keep_bottom);
            if hidden == 0 {
                out.append(ctx_run);
                return;
            }
            let mut iter = ctx_run.drain(..);
            out.extend(iter.by_ref().take(keep_top));
            for _ in 0..hidden {
                let _ = iter.next();
            }
            out.push(DiffLine {
                kind: DiffLineKind::Collapsed,
                old_line: None,
                new_line: None,
                text: format!("… {hidden} unchanged lines …"),
            });
            out.extend(iter);
        };

    let mut seen_non_ctx = false;
    for line in lines {
        if line.kind == DiffLineKind::Context {
            ctx_run.push(line);
        } else {
            flush_ctx(&mut ctx_run, &mut out, !seen_non_ctx, false);
            seen_non_ctx = true;
            out.push(line);
        }
    }
    flush_ctx(&mut ctx_run, &mut out, !seen_non_ctx, true);
    out
}

/// Parses an existing unified diff fenced code block (`+`, `-`, `@@`, context).
pub fn parse_diff_fence(raw: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
            out.push(DiffLine {
                kind: DiffLineKind::Collapsed,
                old_line: None,
                new_line: None,
                text: line.to_string(),
            });
        } else if let Some(rest) = line.strip_prefix('+') {
            out.push(DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: None,
                text: rest.to_string(),
            });
        } else if let Some(rest) = line.strip_prefix('-') {
            out.push(DiffLine {
                kind: DiffLineKind::Removed,
                old_line: None,
                new_line: None,
                text: rest.to_string(),
            });
        } else {
            out.push(DiffLine {
                kind: DiffLineKind::Context,
                old_line: None,
                new_line: None,
                text: line.strip_prefix(' ').unwrap_or(line).to_string(),
            });
        }
    }
    out
}
