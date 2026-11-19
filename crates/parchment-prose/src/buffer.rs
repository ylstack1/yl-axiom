//! Append-only UTF-8 streaming text buffer with monotonic CRLF->LF normalization.
//!
//! Eliminates O(N^2) UTF-16 string copy churn during live SSE streaming: incoming deltas are
//! normalized and appended directly into a single growable UTF-8 buffer (with pending trailing
//! `\r` held back until the next chunk arrives so committed bytes never shrink or mutate).

/// Monotonically normalizes CRLF and lone CR to LF.
/// A trailing `\r` is held back until `\n` arrives so streaming text grows strictly by append.
pub fn normalize_line_endings(raw: &str) -> String {
    if !raw.as_bytes().contains(&b'\r') {
        return raw.to_string();
    }
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut last = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i > last {
                out.push_str(&raw[last..i]);
            }
            if i + 1 < bytes.len() && bytes[i + 1] != b'\n' {
                out.push('\n');
            }
            // Trailing `\r` at the very end is intentionally omitted until its `\n` arrives.
            last = i + 1;
        }
        i += 1;
    }
    if last < raw.len() {
        out.push_str(&raw[last..]);
    }
    out
}

/// Append-only normalized UTF-8 text buffer designed for 120Hz/240Hz streaming turns.
#[derive(Debug, Clone, Default)]
pub struct StreamingTextBuffer {
    buf: String,
    pending_cr: bool,
    line_starts: Vec<usize>,
}

impl StreamingTextBuffer {
    pub fn new() -> Self {
        Self::with_capacity(4096)
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            buf: String::with_capacity(cap),
            pending_cr: false,
            line_starts: vec![0],
        }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.pending_cr = false;
        self.line_starts.clear();
        self.line_starts.push(0);
    }

    pub fn as_str(&self) -> &str {
        &self.buf
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_slice(&self, line_idx: usize) -> Option<&str> {
        let start = *self.line_starts.get(line_idx)?;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .map(|&next| next.saturating_sub(1))
            .unwrap_or(self.buf.len());
        self.buf.get(start..end)
    }

    /// Appends a streaming UTF-8 delta, normalizing CRLF/CR->LF in-place and returning
    /// the byte range of newly committed UTF-8 content.
    pub fn push_delta(&mut self, delta: &str) -> std::ops::Range<usize> {
        let start_len = self.buf.len();
        if delta.is_empty() {
            return start_len..start_len;
        }

        // Fast path: no pending CR and no CR inside delta (99.9% of SSE deltas).
        if !self.pending_cr && !delta.as_bytes().contains(&b'\r') {
            self.buf.push_str(delta);
            self.index_newlines_from(start_len);
            return start_len..self.buf.len();
        }

        self.buf.reserve(delta.len() + 1);
        let bytes = delta.as_bytes();
        let mut i = 0usize;

        if self.pending_cr {
            self.pending_cr = false;
            if bytes[0] != b'\n' {
                self.buf.push('\n');
            }
        }

        let mut copy_from = 0usize;
        while i < bytes.len() {
            if bytes[i] == b'\r' {
                if i > copy_from {
                    self.buf.push_str(&delta[copy_from..i]);
                }
                if i + 1 < bytes.len() {
                    if bytes[i + 1] != b'\n' {
                        self.buf.push('\n');
                    }
                } else {
                    self.pending_cr = true;
                }
                copy_from = i + 1;
            }
            i += 1;
        }
        if copy_from < delta.len() {
            self.buf.push_str(&delta[copy_from..]);
        }

        self.index_newlines_from(start_len);
        start_len..self.buf.len()
    }

    /// Commits any pending lone trailing `\r` as `\n` once the stream finishes.
    pub fn finish(&mut self) {
        if self.pending_cr {
            self.pending_cr = false;
            let prev = self.buf.len();
            self.buf.push('\n');
            self.index_newlines_from(prev);
        }
    }

    fn index_newlines_from(&mut self, from: usize) {
        for (rel, &b) in self.buf.as_bytes()[from..].iter().enumerate() {
            if b == b'\n' {
                self.line_starts.push(from + rel + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_buffer_normalizes_split_crlf_monotonically() {
        let mut buf = StreamingTextBuffer::new();
        buf.push_delta("line1\r");
        assert_eq!(buf.as_str(), "line1");
        buf.push_delta("\nline2\rline3\r\n");
        assert_eq!(buf.as_str(), "line1\nline2\nline3\n");
        assert_eq!(buf.line_slice(0), Some("line1"));
        assert_eq!(buf.line_slice(1), Some("line2"));
        assert_eq!(buf.line_slice(2), Some("line3"));
    }
}
