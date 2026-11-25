//! Zero-regex, zero-allocation `o200k_base` token estimator & context-window breakdown.
//!
//! Mirrors v2 `O200kTokenizer.kt`'s `o200k_base` pre-tokenization boundaries and BPE subword
//! splitting rules via a hand-written linear-time UTF-8 scanner + bounded 2,048-entry cache.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use parchment_model::{MessageNode, PromptAttachment};

const MAX_CACHE_ENTRIES: usize = 2_048;
const LARGE_SAMPLE_THRESHOLD_BYTES: usize = 24_000;
const SAMPLE_SLICE_BYTES: usize = 6_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextWindowBreakdown {
    pub system_and_persona_tokens: usize,
    pub lorebook_and_memory_tokens: usize,
    pub workspace_and_tools_tokens: usize,
    pub uploaded_files_tokens: usize,
    pub uploaded_files_count: usize,
    pub chat_history_tokens: usize,
    pub active_turns_count: usize,
    pub muted_turns_count: usize,
    pub composer_draft_tokens: usize,
    pub context_limit_tokens: usize,
}

impl ContextWindowBreakdown {
    pub fn total_used_tokens(&self) -> usize {
        self.system_and_persona_tokens
            + self.lorebook_and_memory_tokens
            + self.workspace_and_tools_tokens
            + self.uploaded_files_tokens
            + self.chat_history_tokens
            + self.composer_draft_tokens
    }

    pub fn free_space_tokens(&self) -> usize {
        self.context_limit_tokens
            .saturating_sub(self.total_used_tokens())
    }
}

struct BoundedCache {
    map: HashMap<u64, usize>,
    order: VecDeque<u64>,
}

impl BoundedCache {
    fn new() -> Self {
        Self {
            map: HashMap::with_capacity(256),
            order: VecDeque::with_capacity(256),
        }
    }

    fn get(&self, key: u64) -> Option<usize> {
        self.map.get(&key).copied()
    }

    fn insert(&mut self, key: u64, value: usize) {
        if self.map.insert(key, value).is_none() {
            self.order.push_back(key);
            if self.order.len() > MAX_CACHE_ENTRIES
                && let Some(old) = self.order.pop_front()
            {
                self.map.remove(&old);
            }
        }
    }

    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

pub struct O200kTokenizer {
    cache: Mutex<BoundedCache>,
}

impl Default for O200kTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl O200kTokenizer {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(BoundedCache::new()),
        }
    }

    pub fn clear_caches(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
    }

    /// Constant-time token estimate for live streaming buffers without mutating the cache.
    pub fn estimate_tokens_fast(text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        (text.len().div_ceil(4)).max(1)
    }

    pub fn count_tokens(&self, text: &str, cache_result: bool) -> usize {
        if text.is_empty() {
            return 0;
        }
        let key = fingerprint64(text);
        if let Ok(c) = self.cache.lock()
            && let Some(hit) = c.get(key)
        {
            return hit;
        }

        let count = if text.len() > LARGE_SAMPLE_THRESHOLD_BYTES {
            let head_end = floor_char_boundary(text, SAMPLE_SLICE_BYTES);
            let tail_start =
                ceil_char_boundary(text, text.len().saturating_sub(SAMPLE_SLICE_BYTES));
            let sample_tokens =
                scan_o200k_tokens(&text[..head_end]) + scan_o200k_tokens(&text[tail_start..]);
            let sampled_bytes = (head_end + (text.len() - tail_start)).max(1);
            (((sample_tokens as f64) * (text.len() as f64)) / (sampled_bytes as f64))
                .round()
                .max(1.0) as usize
        } else {
            scan_o200k_tokens(text).max(1)
        };

        if cache_result && let Ok(mut c) = self.cache.lock() {
            c.insert(key, count);
        }
        count
    }

    pub fn count_attachment_tokens(&self, att: &PromptAttachment) -> usize {
        if !att.text_content.trim().is_empty() {
            return self.count_tokens(&att.text_content, true) + 8;
        }
        if att.mime_type.starts_with("image/") {
            // Standard high-detail multimodal vision tile budget
            return 765;
        }
        if att.mime_type == "application/pdf" {
            return 1_500;
        }
        64
    }

    pub fn count_history_tokens(&self, nodes: &[MessageNode]) -> (usize, usize, usize) {
        let mut tokens = 0usize;
        let mut active = 0usize;
        let mut muted = 0usize;
        for node in nodes {
            if !node.included_in_context {
                muted += 1;
                continue;
            }
            active += 1;
            if let Some(v) = node.active_variant() {
                tokens += 4; // role/framing overhead
                tokens += self.count_tokens(&v.content, true);
                for att in &v.attachments {
                    tokens += self.count_attachment_tokens(att);
                }
                for tc in &v.tool_calls {
                    tokens += self.count_tokens(&tc.name, true)
                        + self.count_tokens(&tc.arguments_json, true)
                        + self.count_tokens(&tc.result_text, true)
                        + 12;
                }
            }
        }
        (tokens, active, muted)
    }
}

fn scan_o200k_tokens(text: &str) -> usize {
    let mut total = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let n = bytes.len();

    while i < n {
        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            let start = i;
            let mut newlines = 0usize;
            while let Some(c) = text[i..].chars().next() {
                if !c.is_whitespace() {
                    break;
                }
                if c == '\n' {
                    newlines += 1;
                }
                i += c.len_utf8();
            }
            let len = i - start;
            // In `o200k_base` BPE, a single ASCII space before a token is merged into that token.
            if !(len == 1 && ch == ' ' && i < n) {
                total += newlines.max(len.div_ceil(4)).max(1);
            }
        } else if ch.is_numeric() {
            // 1-to-3 digit chunks = 1 token in o200k_base.
            let mut digits = 0usize;
            while digits < 3
                && let Some(c) = text[i..].chars().next()
            {
                if !c.is_numeric() {
                    break;
                }
                i += c.len_utf8();
                digits += 1;
            }
            total += 1;
        } else if ch.is_alphabetic() {
            let mut non_ascii = 0usize;
            let mut char_count = 0usize;
            while let Some(c) = text[i..].chars().next() {
                if !c.is_alphabetic() {
                    break;
                }
                if !c.is_ascii() {
                    non_ascii += 1;
                }
                char_count += 1;
                i += c.len_utf8();
            }
            if i < n && bytes[i] == b'\'' {
                let rest = &text[i + 1..];
                let adv = if starts_with_ci(rest, "re")
                    || starts_with_ci(rest, "ve")
                    || starts_with_ci(rest, "ll")
                {
                    3
                } else if rest.starts_with(['s', 't', 'm', 'd', 'S', 'T', 'M', 'D']) {
                    2
                } else {
                    0
                };
                if adv > 0 {
                    i += adv;
                    char_count += adv;
                }
            }
            if non_ascii > 0 {
                let ascii_part = char_count - non_ascii;
                let ascii_toks = if ascii_part == 0 {
                    0
                } else {
                    ascii_part.div_ceil(5).max(1)
                };
                let unicode_toks = (non_ascii * 5).div_ceil(4);
                total += ascii_toks + unicode_toks.max(1);
            } else if char_count <= 7 {
                total += 1;
            } else if char_count <= 12 {
                total += 2;
            } else {
                total += ((char_count + 3) / 5).max(2);
            }
        } else {
            // Punctuation / operator cluster.
            let mut char_count = 0usize;
            while let Some(c) = text[i..].chars().next() {
                if c.is_whitespace() || c.is_alphanumeric() {
                    break;
                }
                char_count += 1;
                i += c.len_utf8();
            }
            total += char_count.div_ceil(3).max(1);
        }
    }
    total
}

fn starts_with_ci(s: &str, pat: &str) -> bool {
    s.len() >= pat.len() && s.as_bytes()[..pat.len()].eq_ignore_ascii_case(pat.as_bytes())
}

fn fingerprint64(text: &str) -> u64 {
    // Fast FNV-1a 64-bit over length + head/mid/tail + full bytes (capped for speed on huge strings).
    let bytes = text.as_bytes();
    let mut h: u64 = 0xcbf29ce484222325 ^ (bytes.len() as u64);
    let step = (bytes.len() / 2048).max(1);
    let mut i = 0;
    while i < bytes.len() {
        h ^= u64::from(bytes[i]);
        h = h.wrapping_mul(0x100000001b3);
        i += step;
    }
    h
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn ceil_char_boundary(s: &str, mut idx: usize) -> usize {
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
    fn tokenizer_counts_ascii_numbers_cjk_and_large_payloads_fast() {
        let tok = O200kTokenizer::new();
        assert_eq!(tok.count_tokens("", true), 0);
        let english = tok.count_tokens(
            "Hello world! It's 2026 and YL Axiom v3 runs natively.",
            true,
        );
        assert!(
            (8..=20).contains(&english),
            "unexpected english count: {english}"
        );
        let cjk = tok.count_tokens("日本語テスト漢字", true);
        assert!(cjk >= 8);
        let huge = "fn fast_rust_stream(x: usize) -> usize { x + 1 }\n".repeat(1_000);
        let t0 = std::time::Instant::now();
        let huge_count = tok.count_tokens(&huge, true);
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert!(huge_count > 5_000);
        assert!(elapsed_ms < 20.0, "tokenizer too slow: {elapsed_ms:.2} ms");
    }
}
