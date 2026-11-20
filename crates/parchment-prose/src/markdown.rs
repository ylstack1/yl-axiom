//! Incremental prefix-cached Markdown block & inline parser designed for 120Hz/240Hz streaming.
//!
//! Key invariants:
//! - Cached prefix splits require non-empty text AFTER the split boundary so an open code fence
//!   can never be mistakenly finalized before its closing fence arrives.
//! - Open streaming code fences store byte ranges into `StreamingTextBuffer` so extending a 70–90 KB
//!   code fence allocates 0 bytes per chunk (O(delta) scan, O(1) update).
//! - Depth and size limits fall back cleanly to plain `ProseBlock::Paragraph` / `InlineSpan::Text`.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::buffer::{StreamingTextBuffer, normalize_line_endings};

pub const MAX_MARKDOWN_INLINE_DEPTH: usize = 16;
pub const MAX_MARKDOWN_BLOCK_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MARKDOWN_BLOCKS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProseBlock {
    Heading {
        level: u8,
        text: String,
    },
    SceneBreak {
        raw: String,
    },
    BlockQuote {
        text: String,
    },
    CodeFence {
        language: String,
        /// Byte slice into the owning normalized buffer when parsed incrementally, or `0..code.len()`.
        body_range: Range<usize>,
        /// Populated for standalone `parse_prose_blocks` or closed blocks; empty while streaming via `body_range`.
        code: String,
        closed: bool,
    },
    MathBlock {
        raw_latex: String,
    },
    SvgBlock {
        raw_svg: String,
    },
    TableBlock {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    ListBlock {
        ordered: bool,
        items: Vec<String>,
    },
    Paragraph {
        text: String,
    },
}

impl ProseBlock {
    /// Returns the code body slice without copying, whether stored inline or via `body_range`.
    pub fn code_str<'a>(&'a self, full_buf: &'a str) -> &'a str {
        match self {
            Self::CodeFence {
                body_range, code, ..
            } => {
                if !code.is_empty() {
                    code.as_str()
                } else {
                    full_buf.get(body_range.clone()).unwrap_or("")
                }
            }
            _ => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlineSpan {
    Text(String),
    Bold(Vec<InlineSpan>),
    Italic(Vec<InlineSpan>),
    Strike(Vec<InlineSpan>),
    Code(String),
    InlineMath(String),
    Link { label: String, url: String },
}

/// Stateful incremental Markdown parser attached to an append-only `StreamingTextBuffer`.
#[derive(Debug, Clone)]
pub struct IncrementalProseParser {
    buffer: StreamingTextBuffer,
    enable_math: bool,
    stable_prefix_end: usize,
    stable_blocks: Vec<ProseBlock>,
    tail_blocks: Vec<ProseBlock>,
    open_fence: Option<OpenFenceState>,
}

#[derive(Debug, Clone)]
struct OpenFenceState {
    blocks_before_count: usize,
    fence_char: u8,
    fence_len: usize,
    language: String,
    body_start: usize,
    scan_from: usize,
}

impl Default for IncrementalProseParser {
    fn default() -> Self {
        Self::new(true)
    }
}

impl IncrementalProseParser {
    pub fn new(enable_math: bool) -> Self {
        Self {
            buffer: StreamingTextBuffer::new(),
            enable_math,
            stable_prefix_end: 0,
            stable_blocks: Vec::new(),
            tail_blocks: Vec::new(),
            open_fence: None,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.stable_prefix_end = 0;
        self.stable_blocks.clear();
        self.tail_blocks.clear();
        self.open_fence = None;
    }

    pub fn buffer_str(&self) -> &str {
        self.buffer.as_str()
    }

    pub fn stable_blocks(&self) -> &[ProseBlock] {
        &self.stable_blocks
    }

    pub fn tail_blocks(&self) -> &[ProseBlock] {
        &self.tail_blocks
    }

    pub fn block_count(&self) -> usize {
        self.stable_blocks.len() + self.tail_blocks.len()
    }

    pub fn iter_blocks(&self) -> impl Iterator<Item = &ProseBlock> {
        self.stable_blocks.iter().chain(self.tail_blocks.iter())
    }

    /// Appends a streaming UTF-8 delta and incrementally updates only the active tail or open fence.
    pub fn push_delta(&mut self, delta: &str) {
        let added = self.buffer.push_delta(delta);
        if added.is_empty() {
            return;
        }
        self.reparse_incremental();
    }

    /// Finalizes any pending trailing `\r` and materializes the final block list.
    pub fn finish(&mut self) {
        self.buffer.finish();
        self.reparse_incremental();
    }

    fn reparse_incremental(&mut self) {
        let full = self.buffer.as_str();
        if full.len() > MAX_MARKDOWN_BLOCK_BYTES {
            // Bounded fallback for gigantic unformatted payloads.
            self.stable_blocks.clear();
            self.tail_blocks.clear();
            self.open_fence = None;
            self.tail_blocks.push(ProseBlock::Paragraph {
                text: full[..MAX_MARKDOWN_BLOCK_BYTES].to_string(),
            });
            return;
        }

        // Fast path: active open code fence -> scan ONLY newly appended lines in O(delta) with ZERO copies.
        if let Some(mut fence) = self.open_fence.take() {
            let mut line_start = fence.scan_from;
            let bytes = full.as_bytes();
            let mut closed = false;
            while line_start < bytes.len() {
                let nl = bytes[line_start..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|rel| line_start + rel);
                let line_end = nl.unwrap_or(bytes.len());
                if is_closing_fence(
                    &full[line_start..line_end],
                    fence.fence_char,
                    fence.fence_len,
                ) {
                    closed = true;
                    break;
                }
                match nl {
                    Some(pos) => line_start = pos + 1,
                    None => break,
                }
            }

            if !closed {
                fence.scan_from = line_start;
                let mut body_start = fence.body_start;
                while body_start < bytes.len() && bytes[body_start] == b'\n' {
                    body_start += 1;
                }
                let range = body_start..bytes.len();
                let blocks_before = fence.blocks_before_count;
                self.tail_blocks.truncate(blocks_before);
                self.tail_blocks.push(ProseBlock::CodeFence {
                    language: fence.language.clone(),
                    body_range: range,
                    code: String::new(),
                    closed: false,
                });
                self.open_fence = Some(fence);
                return;
            }
            // Fence just closed! Fall through to clean tail reparse from `stable_prefix_end`.
        }

        // Parse only from `stable_prefix_end..full.len()` and promote completed prefix blocks when safe.
        let (blocks, maybe_open, safe_commit_end) =
            parse_range_incremental(full, self.stable_prefix_end, self.enable_math);

        if safe_commit_end > self.stable_prefix_end && safe_commit_end < full.len() {
            // Invariant: cached split MUST have text after it so an open fence cannot be split prematurely.
            let (committed, _, _) =
                parse_range_incremental(full, self.stable_prefix_end, self.enable_math);
            // Only commit blocks strictly preceding `safe_commit_end`.
            let prefix_blocks = parse_range_exact(
                full,
                self.stable_prefix_end..safe_commit_end,
                self.enable_math,
                false,
            );
            let _ = committed;
            if self.stable_blocks.len() + prefix_blocks.len() <= MAX_MARKDOWN_BLOCKS {
                self.stable_blocks.extend(prefix_blocks);
                self.stable_prefix_end = safe_commit_end;
                let (new_tail, new_open, _) =
                    parse_range_incremental(full, self.stable_prefix_end, self.enable_math);
                self.tail_blocks = new_tail;
                self.open_fence = new_open;
                return;
            }
        }

        self.tail_blocks = blocks;
        self.open_fence = maybe_open;
    }
}

/// Standalone non-streaming Markdown block parser (normalizes CRLF/CR->LF and returns owned blocks).
pub fn parse_prose_blocks(raw: &str, enable_math: bool) -> Vec<ProseBlock> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let normalized = normalize_line_endings(raw);
    if normalized.len() > MAX_MARKDOWN_BLOCK_BYTES {
        return vec![ProseBlock::Paragraph {
            text: normalized[..MAX_MARKDOWN_BLOCK_BYTES].to_string(),
        }];
    }
    parse_range_exact(&normalized, 0..normalized.len(), enable_math, true)
}

fn parse_range_incremental(
    full: &str,
    start_offset: usize,
    enable_math: bool,
) -> (Vec<ProseBlock>, Option<OpenFenceState>, usize) {
    let slice = &full[start_offset..];
    let mut blocks = Vec::new();
    let mut lines = LineCursor::new(full, start_offset);
    let mut safe_commit_end = start_offset;
    let mut open_fence = None;
    let _ = slice;

    while let Some((line_start, line_end, line)) = lines.next_line() {
        if blocks.len() >= MAX_MARKDOWN_BLOCKS {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line after at least one complete block and with trailing content remaining.
            if !blocks.is_empty() && lines.pos < full.len() {
                safe_commit_end = lines.pos;
            }
            continue;
        }

        // 1. Fenced code block (``` or ~~~).
        if let Some((fchar, flen, lang)) = parse_fence_opener(trimmed) {
            let body_start = lines.pos;
            let mut body_end = full.len();
            let mut closed = false;
            while let Some((ls, _le, l)) = lines.next_line() {
                if is_closing_fence(l, fchar, flen) {
                    body_end = ls;
                    closed = true;
                    break;
                }
            }
            let mut trimmed_body_start = body_start;
            while trimmed_body_start < body_end && full.as_bytes()[trimmed_body_start] == b'\n' {
                trimmed_body_start += 1;
            }
            let trimmed_body_end = trim_trailing_newline(full, trimmed_body_start, body_end);
            if !closed {
                let blocks_before_count = blocks.len();
                blocks.push(ProseBlock::CodeFence {
                    language: lang.to_string(),
                    body_range: trimmed_body_start..full.len(),
                    code: String::new(),
                    closed: false,
                });
                let last_nl = full[body_start..]
                    .rfind('\n')
                    .map(|r| body_start + r + 1)
                    .unwrap_or(body_start);
                open_fence = Some(OpenFenceState {
                    blocks_before_count,
                    fence_char: fchar,
                    fence_len: flen,
                    language: lang.to_string(),
                    body_start,
                    scan_from: last_nl,
                });
                break;
            } else if lang.eq_ignore_ascii_case("svg")
                && full[trimmed_body_start..trimmed_body_end]
                    .trim()
                    .starts_with("<svg")
            {
                blocks.push(ProseBlock::SvgBlock {
                    raw_svg: full[trimmed_body_start..trimmed_body_end].to_string(),
                });
            } else {
                blocks.push(ProseBlock::CodeFence {
                    language: lang.to_string(),
                    body_range: trimmed_body_start..trimmed_body_end,
                    code: full[trimmed_body_start..trimmed_body_end].to_string(),
                    closed: true,
                });
            }
            continue;
        }

        // 2. Display Math block (`$$...$$` or `\[...\]`).
        if enable_math && (trimmed.starts_with("$$") || trimmed.starts_with("\\[")) {
            let (close_delim, single_rest) = if let Some(r) = trimmed.strip_prefix("$$") {
                ("$$", r)
            } else {
                ("\\]", &trimmed[2..])
            };
            if let Some(end_idx) = single_rest.find(close_delim)
                && !single_rest[..end_idx].trim().is_empty()
            {
                blocks.push(ProseBlock::MathBlock {
                    raw_latex: single_rest[..end_idx].trim().to_string(),
                });
                continue;
            }
            let mut math_lines = Vec::new();
            if !single_rest.trim().is_empty() {
                math_lines.push(single_rest.trim());
            }
            while let Some((_ls, _le, l)) = lines.next_line() {
                let lt = l.trim();
                if let Some(before) = lt.strip_suffix(close_delim) {
                    if !before.trim().is_empty() {
                        math_lines.push(before.trim());
                    }
                    break;
                }
                math_lines.push(l);
            }
            blocks.push(ProseBlock::MathBlock {
                raw_latex: math_lines.join("\n").trim().to_string(),
            });
            continue;
        }

        // 3. Raw inline `<svg ...>...</svg>` block.
        if trimmed.starts_with("<svg") {
            let mut svg_buf = String::from(line);
            if !trimmed.contains("</svg>") {
                while let Some((_ls, _le, l)) = lines.next_line() {
                    svg_buf.push('\n');
                    svg_buf.push_str(l);
                    if l.contains("</svg>") {
                        break;
                    }
                }
            }
            blocks.push(ProseBlock::SvgBlock { raw_svg: svg_buf });
            continue;
        }

        // 4. ATX Heading (`#` .. `######`).
        if let Some((level, heading_text)) = parse_heading(trimmed) {
            blocks.push(ProseBlock::Heading {
                level,
                text: heading_text.to_string(),
            });
            continue;
        }

        // 5. Thematic scene break (`---`, `***`, `___`).
        if is_scene_break(trimmed) {
            blocks.push(ProseBlock::SceneBreak {
                raw: trimmed.to_string(),
            });
            continue;
        }

        // 6. Blockquote (`> ...`).
        if trimmed.starts_with('>') {
            let mut quote_lines = vec![strip_blockquote_prefix(trimmed)];
            while let Some((_ls, _le, next_l)) = lines.peek_line() {
                let nt = next_l.trim();
                if nt.starts_with('>') {
                    lines.next_line();
                    quote_lines.push(strip_blockquote_prefix(nt));
                } else {
                    break;
                }
            }
            blocks.push(ProseBlock::BlockQuote {
                text: quote_lines.join("\n"),
            });
            continue;
        }

        // 7. GFM Pipe Table (header row + separator row `|---|---|`).
        if line.contains('|')
            && let Some((_ns, _ne, next_l)) = lines.peek_line()
            && is_table_separator(next_l)
        {
            lines.next_line(); // consume separator
            let headers = split_table_cells(line);
            let mut rows = Vec::new();
            while let Some((_rs, _re, rl)) = lines.peek_line() {
                if rl.trim().is_empty() || !rl.contains('|') {
                    break;
                }
                lines.next_line();
                rows.push(split_table_cells(rl));
            }
            blocks.push(ProseBlock::TableBlock { headers, rows });
            continue;
        }

        // 8. Bullet or ordered list.
        if let Some((ordered, first_item)) = parse_list_item(trimmed) {
            let mut items = vec![first_item.to_string()];
            while let Some((_ns, _ne, nl)) = lines.peek_line() {
                let nt = nl.trim();
                if let Some((next_ord, item_txt)) = parse_list_item(nt)
                    && next_ord == ordered
                {
                    lines.next_line();
                    items.push(item_txt.to_string());
                } else {
                    break;
                }
            }
            blocks.push(ProseBlock::ListBlock { ordered, items });
            continue;
        }

        // 9. Paragraph (merge consecutive plain lines until next blank or block opener).
        let mut para = String::from(line);
        let _ = (line_start, line_end);
        while let Some((_ns, _ne, nl)) = lines.peek_line() {
            let nt = nl.trim();
            if nt.is_empty()
                || parse_fence_opener(nt).is_some()
                || parse_heading(nt).is_some()
                || is_scene_break(nt)
                || nt.starts_with('>')
                || parse_list_item(nt).is_some()
                || (enable_math && (nt.starts_with("$$") || nt.starts_with("\\[")))
            {
                break;
            }
            lines.next_line();
            para.push('\n');
            para.push_str(nl);
        }
        blocks.push(ProseBlock::Paragraph { text: para });
    }

    (blocks, open_fence, safe_commit_end)
}

fn parse_range_exact(
    full: &str,
    range: Range<usize>,
    enable_math: bool,
    materialize_open_code: bool,
) -> Vec<ProseBlock> {
    let sub = &full[range.clone()];
    let (mut blocks, _, _) = parse_range_incremental(sub, 0, enable_math);
    if materialize_open_code {
        for blk in &mut blocks {
            if let ProseBlock::CodeFence {
                body_range, code, ..
            } = blk
                && code.is_empty()
                && body_range.start < body_range.end
            {
                *code = sub.get(body_range.clone()).unwrap_or("").to_string();
            }
        }
    } else if range.start > 0 {
        for blk in &mut blocks {
            if let ProseBlock::CodeFence { body_range, .. } = blk {
                *body_range = (body_range.start + range.start)..(body_range.end + range.start);
            }
        }
    }
    blocks
}

struct LineCursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> LineCursor<'a> {
    fn new(text: &'a str, start: usize) -> Self {
        Self { text, pos: start }
    }

    fn peek_line(&self) -> Option<(usize, usize, &'a str)> {
        if self.pos >= self.text.len() {
            return None;
        }
        let rest = &self.text[self.pos..];
        let end = rest
            .as_bytes()
            .iter()
            .position(|&b| b == b'\n')
            .map(|r| self.pos + r)
            .unwrap_or(self.text.len());
        Some((self.pos, end, &self.text[self.pos..end]))
    }

    fn next_line(&mut self) -> Option<(usize, usize, &'a str)> {
        let (s, e, slice) = self.peek_line()?;
        self.pos = if e < self.text.len() { e + 1 } else { e };
        Some((s, e, slice))
    }
}

fn parse_fence_opener(trimmed: &str) -> Option<(u8, usize, &str)> {
    let bytes = trimmed.as_bytes();
    let ch = *bytes.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let count = bytes.iter().take_while(|&&b| b == ch).count();
    if count < 3 {
        return None;
    }
    let rest = trimmed[count..].trim();
    if ch == b'`' && rest.contains('`') {
        return None; // inline code span on one line, not a fence opener
    }
    let lang = rest.split_whitespace().next().unwrap_or("");
    Some((ch, count, lang))
}

fn is_closing_fence(line: &str, fence_char: u8, fence_len: usize) -> bool {
    let trimmed = line.trim();
    let bytes = trimmed.as_bytes();
    bytes.len() >= fence_len && bytes.iter().all(|&b| b == fence_char)
}

fn trim_trailing_newline(s: &str, start: usize, mut end: usize) -> usize {
    while end > start && s.as_bytes()[end - 1] == b'\n' {
        end -= 1;
    }
    end
}

fn parse_heading(trimmed: &str) -> Option<(u8, &str)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if rest.is_empty() || !rest.starts_with(' ') {
        return None;
    }
    Some((hashes as u8, rest.trim()))
}

fn is_scene_break(trimmed: &str) -> bool {
    if trimmed.len() < 3 {
        return false;
    }
    let mut marker: Option<u8> = None;
    let mut count = 0usize;
    for b in trimmed.bytes() {
        if b == b' ' || b == b'\t' {
            continue;
        }
        if matches!(b, b'-' | b'*' | b'_') {
            if let Some(m) = marker {
                if m != b {
                    return false;
                }
            } else {
                marker = Some(b);
            }
            count += 1;
        } else {
            return false;
        }
    }
    count >= 3
}

fn strip_blockquote_prefix(trimmed: &str) -> &str {
    trimmed.trim_prefix('>').trim_prefix(' ')
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('-') || !trimmed.contains('|') {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

fn split_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let stripped = trimmed.trim_prefix('|').trim_suffix('|');
    stripped.split('|').map(|c| c.trim().to_string()).collect()
}

fn parse_list_item(trimmed: &str) -> Option<(bool, &str)> {
    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return Some((false, rest.trim()));
    }
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0
        && digits <= 4
        && trimmed.as_bytes().get(digits) == Some(&b'.')
        && trimmed.as_bytes().get(digits + 1) == Some(&b' ')
    {
        return Some((true, trimmed[digits + 2..].trim()));
    }
    None
}

/// Bounded inline Markdown parser (`**bold**`, `*italic*`, `` `code` ``, `~~strike~~`, `[label](url)`, `$math$` / `\(...\)`).
pub fn parse_inline_markdown(text: &str, enable_math: bool) -> Vec<InlineSpan> {
    parse_inline_rec(text, enable_math, 0)
}

fn parse_inline_rec(text: &str, enable_math: bool, depth: usize) -> Vec<InlineSpan> {
    if text.is_empty() {
        return Vec::new();
    }
    if depth >= MAX_MARKDOWN_INLINE_DEPTH || text.len() > 131_072 {
        return vec![InlineSpan::Text(text.to_string())];
    }

    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut plain_start = 0usize;
    let mut i = 0usize;

    let flush_plain = |out: &mut Vec<InlineSpan>, start: usize, end: usize| {
        if end > start {
            out.push(InlineSpan::Text(text[start..end].to_string()));
        }
    };

    while i < bytes.len() {
        // Inline code: `...`
        if bytes[i] == b'`' {
            let ticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            let delim = &text[i..i + ticks];
            if let Some(rel_end) = text[i + ticks..].find(delim) {
                flush_plain(&mut out, plain_start, i);
                let code = &text[i + ticks..i + ticks + rel_end];
                out.push(InlineSpan::Code(code.to_string()));
                i += ticks + rel_end + ticks;
                plain_start = i;
                continue;
            }
        }

        // Inline math `\(...\)` or `$...$`
        if enable_math && bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'(') {
            if let Some(rel_end) = text[i + 2..].find("\\)") {
                flush_plain(&mut out, plain_start, i);
                out.push(InlineSpan::InlineMath(
                    text[i + 2..i + 2 + rel_end].trim().to_string(),
                ));
                i += 2 + rel_end + 2;
                plain_start = i;
                continue;
            }
        } else if enable_math
            && bytes[i] == b'$'
            && bytes.get(i + 1) != Some(&b'$')
            && (i == 0 || bytes[i - 1] != b'\\')
            && bytes.get(i + 1).is_some_and(|b| !b.is_ascii_whitespace())
            && let Some(rel_end) = find_closing_dollar(&text[i + 1..])
        {
            flush_plain(&mut out, plain_start, i);
            out.push(InlineSpan::InlineMath(
                text[i + 1..i + 1 + rel_end].trim().to_string(),
            ));
            i += 1 + rel_end + 1;
            plain_start = i;
            continue;
        }

        // Bold `**...**`
        if bytes[i..].starts_with(b"**")
            && let Some(rel_end) = text[i + 2..].find("**")
            && rel_end > 0
        {
            flush_plain(&mut out, plain_start, i);
            let inner = parse_inline_rec(&text[i + 2..i + 2 + rel_end], enable_math, depth + 1);
            out.push(InlineSpan::Bold(inner));
            i += 2 + rel_end + 2;
            plain_start = i;
            continue;
        }

        // Strikethrough `~~...~~`
        if bytes[i..].starts_with(b"~~")
            && let Some(rel_end) = text[i + 2..].find("~~")
            && rel_end > 0
        {
            flush_plain(&mut out, plain_start, i);
            let inner = parse_inline_rec(&text[i + 2..i + 2 + rel_end], enable_math, depth + 1);
            out.push(InlineSpan::Strike(inner));
            i += 2 + rel_end + 2;
            plain_start = i;
            continue;
        }

        // Italic `*...*`
        if bytes[i] == b'*'
            && bytes
                .get(i + 1)
                .is_some_and(|&b| b != b'*' && !b.is_ascii_whitespace())
            && let Some(rel_end) = text[i + 1..].find('*')
            && rel_end > 0
        {
            flush_plain(&mut out, plain_start, i);
            let inner = parse_inline_rec(&text[i + 1..i + 1 + rel_end], enable_math, depth + 1);
            out.push(InlineSpan::Italic(inner));
            i += 1 + rel_end + 1;
            plain_start = i;
            continue;
        }

        // Link `[label](url)`
        if bytes[i] == b'['
            && let Some(close_br) = text[i + 1..].find("](")
            && let Some(close_par) = text[i + 1 + close_br + 2..].find(')')
        {
            let label = &text[i + 1..i + 1 + close_br];
            let url = &text[i + 1 + close_br + 2..i + 1 + close_br + 2 + close_par];
            if !label.is_empty() && !url.is_empty() {
                flush_plain(&mut out, plain_start, i);
                out.push(InlineSpan::Link {
                    label: label.to_string(),
                    url: url.to_string(),
                });
                i += 1 + close_br + 2 + close_par + 1;
                plain_start = i;
                continue;
            }
        }

        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }

    flush_plain(&mut out, plain_start, text.len());
    out
}

fn find_closing_dollar(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            return None;
        }
        if bytes[i] == b'$' && (i == 0 || bytes[i - 1] != b'\\') {
            if i > 0 && !bytes[i - 1].is_ascii_whitespace() {
                return Some(i);
            }
            return None;
        }
        i += 1;
    }
    None
}
