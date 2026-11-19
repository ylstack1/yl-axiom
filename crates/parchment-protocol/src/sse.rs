//! Zero-copy SSE byte framer and adaptive idle-timeout stream reader with plain-JSON fallback.

use std::time::Duration;

use bytes::{Buf as _, Bytes, BytesMut};
use futures_util::{Stream, StreamExt};

use crate::error_policy::ProtocolError;

pub const DEFAULT_MAX_SSE_BUFFER_BYTES: usize = 64 * 1024 * 1024;
pub const INITIAL_LINE_TIMEOUT: Duration = Duration::from_secs(300);
pub const STREAMING_LINE_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub event_type: String,
    pub data: String,
}

/// Incremental SSE byte framer with single-line complete-JSON immediate dispatch
/// (matching v2 `SseStreamReader.kt` for proxies that hold back blank separator lines).
pub struct SseFramer {
    buf: BytesMut,
    event_type: String,
    data_buf: String,
    raw_fallback: Vec<u8>,
    saw_sse_line: bool,
    saw_sse_comment: bool,
    saw_event: bool,
    saw_done: bool,
    max_buffer_bytes: usize,
}

impl Default for SseFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl SseFramer {
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_MAX_SSE_BUFFER_BYTES)
    }

    pub fn with_limit(max_buffer_bytes: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(8192),
            event_type: String::new(),
            data_buf: String::with_capacity(1024),
            raw_fallback: Vec::new(),
            saw_sse_line: false,
            saw_sse_comment: false,
            saw_event: false,
            saw_done: false,
            max_buffer_bytes,
        }
    }

    pub fn saw_sse_line(&self) -> bool {
        self.saw_sse_line
    }

    pub fn saw_done(&self) -> bool {
        self.saw_done
    }

    pub fn saw_event(&self) -> bool {
        self.saw_event
    }

    pub fn saw_sse_comment(&self) -> bool {
        self.saw_sse_comment
    }

    /// Feeds raw bytes from the network socket, pushing any complete SSE frames into `out`.
    pub fn push_bytes(
        &mut self,
        chunk: &[u8],
        out: &mut Vec<SseFrame>,
    ) -> Result<(), ProtocolError> {
        if self.buf.len().saturating_add(chunk.len()) > self.max_buffer_bytes {
            return Err(ProtocolError::NoRetry(format!(
                "SSE line exceeded the {} byte limit.",
                self.max_buffer_bytes
            )));
        }
        let mut buf = std::mem::take(&mut self.buf);
        buf.extend_from_slice(chunk);

        let mut start = 0usize;
        let mut res = Ok(());
        while let Some(rel) = buf[start..].iter().position(|&b| b == b'\n') {
            let nl = start + rel;
            let end = if nl > start && buf[nl - 1] == b'\r' {
                nl - 1
            } else {
                nl
            };
            let line = &buf[start..end];
            start = nl + 1;
            if let Err(err) = self.process_line(line, out) {
                res = Err(err);
                break;
            }
        }
        if start > 0 {
            buf.advance(start);
        }
        self.buf = buf;
        res
    }

    /// Flushes any trailing partial line at EOF and returns an optional plain-JSON fallback body
    /// if the upstream responded with a single JSON document instead of SSE.
    pub fn finish(&mut self, out: &mut Vec<SseFrame>) -> Result<Option<String>, ProtocolError> {
        if !self.buf.is_empty() {
            let tail = self.buf.split();
            let slice = tail.strip_suffix(b"\r").unwrap_or(&tail);
            self.process_line(slice, out)?;
        }
        self.flush_pending(out);
        if !self.saw_sse_line && !self.raw_fallback.is_empty() {
            let text = String::from_utf8_lossy(&self.raw_fallback)
                .trim()
                .to_string();
            self.raw_fallback.clear();
            if !text.is_empty() {
                return Ok(Some(text));
            }
        }
        Ok(None)
    }

    fn flush_pending(&mut self, out: &mut Vec<SseFrame>) {
        if !self.data_buf.is_empty() {
            let data = std::mem::take(&mut self.data_buf);
            let event_type = std::mem::take(&mut self.event_type);
            self.emit_frame(event_type, data, out);
        }
    }

    fn process_line(
        &mut self,
        line_bytes: &[u8],
        out: &mut Vec<SseFrame>,
    ) -> Result<(), ProtocolError> {
        // Strip UTF-8 BOM only at the very start of the stream.
        let line_bytes = if !self.saw_sse_line && self.raw_fallback.is_empty() {
            line_bytes
                .strip_prefix(&[0xEF, 0xBB, 0xBF])
                .unwrap_or(line_bytes)
        } else {
            line_bytes
        };
        let line = std::str::from_utf8(line_bytes).unwrap_or("");

        if line.starts_with(':') {
            self.saw_sse_comment = true;
            return Ok(());
        }
        if line.starts_with("id:") || line.starts_with("retry:") {
            self.saw_sse_line = true;
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix("event:") {
            self.saw_sse_line = true;
            self.flush_pending(out);
            self.event_type.clear();
            self.event_type.push_str(strip_sse_field_space(rest));
            return Ok(());
        }
        if let Some(rest) = line.strip_prefix("data:") {
            self.saw_sse_line = true;
            let val = strip_sse_field_space(rest);
            if val == "[DONE]" {
                self.flush_pending(out);
                let ev = std::mem::take(&mut self.event_type);
                self.emit_frame(ev, "[DONE]".to_string(), out);
            } else if self.data_buf.is_empty() && is_complete_json_object(val) {
                let ev = std::mem::take(&mut self.event_type);
                self.emit_frame(ev, val.to_string(), out);
            } else {
                if self
                    .data_buf
                    .len()
                    .saturating_add(val.len())
                    .saturating_add(1)
                    > self.max_buffer_bytes
                {
                    return Err(ProtocolError::NoRetry(format!(
                        "SSE event exceeded the {} byte limit.",
                        self.max_buffer_bytes
                    )));
                }
                if !self.data_buf.is_empty() {
                    self.data_buf.push('\n');
                }
                self.data_buf.push_str(val);
            }
            return Ok(());
        }
        if line.trim().is_empty() {
            self.flush_pending(out);
            return Ok(());
        }

        if !self.saw_sse_line {
            if self
                .raw_fallback
                .len()
                .saturating_add(line_bytes.len())
                .saturating_add(1)
                > self.max_buffer_bytes
            {
                return Err(ProtocolError::NoRetry(format!(
                    "Response body exceeded the {} byte limit.",
                    self.max_buffer_bytes
                )));
            }
            if !self.raw_fallback.is_empty() {
                self.raw_fallback.push(b'\n');
            }
            self.raw_fallback.extend_from_slice(line_bytes);
        }
        Ok(())
    }

    fn emit_frame(&mut self, event_type: String, data: String, out: &mut Vec<SseFrame>) {
        if data == "[DONE]" {
            self.saw_done = true;
        } else {
            self.saw_event = true;
        }
        out.push(SseFrame { event_type, data });
    }
}

/// SSE spec strips at most one leading ASCII space after the colon (and trims trailing whitespace).
fn strip_sse_field_space(rest: &str) -> &str {
    rest.strip_prefix(' ').unwrap_or(rest).trim_end()
}

/// Fast structural scan verifying whether `s` is a single complete JSON object `{...}`
/// without building a DOM tree.
pub fn is_complete_json_object(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return false;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, b) in trimmed.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
                if depth == 0 && idx + 1 < trimmed.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_string
}

/// Consumer receiving parsed SSE frames or plain-JSON fallback bodies from `drive_sse_or_json_stream`.
pub trait SseStreamConsumer {
    fn is_finished(&self) -> bool;
    fn on_frame(&mut self, frame: &SseFrame) -> Result<(), ProtocolError>;
    fn on_plain_json(&mut self, body: &str) -> Result<(), ProtocolError>;
}

/// Drives an async byte stream through `SseFramer` with adaptive per-chunk idle timeouts.
/// Closing an SSE stream without a finish signal or `[DONE]` returns `ProtocolError::StreamInterrupted`.
pub async fn drive_sse_or_json_stream<S, E, C>(
    mut stream: S,
    initial_timeout: Duration,
    streaming_timeout: Duration,
    consumer: &mut C,
) -> Result<(), ProtocolError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
    C: SseStreamConsumer,
{
    let mut framer = SseFramer::new();
    let mut batch: Vec<SseFrame> = Vec::with_capacity(16);

    loop {
        let wait = if framer.saw_sse_line() {
            streaming_timeout
        } else {
            initial_timeout
        };
        let next_item = tokio::time::timeout(wait, stream.next())
            .await
            .map_err(|_| {
                ProtocolError::StreamInterrupted(format!(
                    "SSE stream timed out waiting for next event after {}s (connection stalled).",
                    wait.as_secs()
                ))
            })?;

        match next_item {
            None => break,
            Some(Err(e)) => {
                return Err(ProtocolError::StreamInterrupted(format!(
                    "Stream transport interrupted: {e}"
                )));
            }
            Some(Ok(chunk)) => {
                batch.clear();
                framer.push_bytes(&chunk, &mut batch)?;
                for frame in &batch {
                    consumer.on_frame(frame)?;
                }
            }
        }
    }

    batch.clear();
    let fallback = framer.finish(&mut batch)?;
    for frame in &batch {
        consumer.on_frame(frame)?;
    }

    if let Some(json_body) = fallback {
        consumer.on_plain_json(&json_body)?;
        return Ok(());
    }

    if !consumer.is_finished() && !framer.saw_done() {
        if framer.saw_event() || framer.saw_sse_line() || framer.saw_sse_comment() {
            return Err(ProtocolError::StreamInterrupted(
                "SSE stream closed before completion (missing finish event or [DONE]).".into(),
            ));
        }
        return Err(ProtocolError::StreamInterrupted(
            "Provider returned an empty stream body.".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_framer_dispatches_single_line_json_immediately_and_handles_multiline_and_fallback() {
        let mut framer = SseFramer::new();
        let mut out = Vec::new();
        // Proxy sends a complete JSON data line without a trailing blank line yet.
        framer
            .push_bytes(b"event: delta\ndata: {\"text\":\"fast\"}\n", &mut out)
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event_type, "delta");
        assert_eq!(out[0].data, "{\"text\":\"fast\"}");

        // Multi-line data waits for blank line.
        out.clear();
        framer
            .push_bytes(
                b"data: line1\r\ndata: line2\r\n\r\ndata: [DONE]\n\n",
                &mut out,
            )
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].data, "line1\nline2");
        assert_eq!(out[1].data, "[DONE]");
        assert!(framer.saw_done());

        // Plain-JSON fallback when no SSE markers exist.
        let mut json_framer = SseFramer::new();
        let mut empty = Vec::new();
        json_framer
            .push_bytes(b"{\"id\":\"resp_1\",\"output_text\":\"hello\"}", &mut empty)
            .unwrap();
        assert!(empty.is_empty());
        let fb = json_framer.finish(&mut empty).unwrap();
        assert_eq!(
            fb.as_deref(),
            Some("{\"id\":\"resp_1\",\"output_text\":\"hello\"}")
        );
    }
}
