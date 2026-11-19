//! Typed provider HTTP errors, server retry-hint parsing (`Retry-After`, `retry-after-ms`,
//! Gemini `google.rpc.RetryInfo`), backoff scheduling, and non-retryable stop classification.

use serde_json::Value;
use thiserror::Error;

const MAX_HINT_SCAN_BYTES: usize = 64 * 1024;
const MIN_SERVER_HINT_MS: u64 = 1_000;
const MAX_SERVER_HINT_MS: u64 = 300_000;
pub const MAX_RETRY_WAIT_MS: u64 = 60_000;

const CONNECTION_BACKOFF_MS: [u64; 6] = [2_000, 4_000, 8_000, 16_000, 30_000, 60_000];
const SERVER_BACKOFF_MS: [u64; 5] = [5_000, 10_000, 20_000, 40_000, 60_000];

const NON_RETRYABLE_STATUSES: [u16; 16] = [
    401, 402, 403, 404, 405, 406, 410, 411, 413, 414, 415, 416, 417, 422, 451, 501,
];

const AUTH_STOP_MARKERS: [&str; 4] = [
    "invalid_api_key",
    "invalid api key",
    "authentication_error",
    "permission_denied",
];

const BILLING_STOP_MARKERS: [&str; 4] = [
    "billing_not_active",
    "insufficient_quota",
    "billing_hard_limit_reached",
    "enforced_spend_limit_reached",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunErrorKind {
    ApiKey,
    Quota,
    SpendLimit,
    ConversationLimit,
    Model,
}

#[derive(Debug, Clone)]
pub struct ProviderHttpError {
    pub label: String,
    pub status: u16,
    pub body: String,
    pub retry_after_ms: Option<u64>,
}

impl std::fmt::Display for ProviderHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.label.trim().is_empty() {
            write!(f, "HTTP {}: {}", self.status, self.body)
        } else {
            write!(f, "{} HTTP {}: {}", self.label, self.status, self.body)
        }
    }
}

impl std::error::Error for ProviderHttpError {}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("{0}")]
    Http(#[from] ProviderHttpError),
    #[error("{0}")]
    NoRetry(String),
    #[error("Background job accepted (interaction={interaction_id}): {message}")]
    AcceptedJob {
        interaction_id: String,
        message: String,
    },
    #[error("Chat spend limit reached: {0}")]
    SpendLimit(String),
    #[error("{0}")]
    StreamInterrupted(String),
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Request cancelled")]
    Cancelled,
}

impl ProtocolError {
    pub fn provider_http(&self) -> Option<&ProviderHttpError> {
        match self {
            Self::Http(err) => Some(err),
            _ => None,
        }
    }

    pub fn http_status(&self) -> Option<u16> {
        if let Some(http) = self.provider_http() {
            return Some(http.status);
        }
        parse_http_status_from_text(&self.to_string())
    }
}

fn parse_http_status_from_text(text: &str) -> Option<u16> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i + 8 <= bytes.len() {
        if &bytes[i..i + 5] == b"http "
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric())
            && matches!(bytes[i + 5], b'4' | b'5')
            && bytes[i + 6].is_ascii_digit()
            && bytes[i + 7].is_ascii_digit()
            && (i + 8 == bytes.len() || !bytes[i + 8].is_ascii_digit())
            && let Ok(code) = lower[i + 5..i + 8].parse::<u16>()
        {
            return Some(code);
        }
        i += 1;
    }
    None
}

/// Parses server retry hints in priority order:
/// 1. `retry-after-ms`
/// 2. `retry-after` (numeric seconds OR RFC 7231 IMF-fixdate)
/// 3. Gemini `google.rpc.RetryInfo.retryDelay` (`"12.5s"`) in the response JSON body.
pub fn parse_retry_after_ms<'a>(
    header: impl Fn(&str) -> Option<&'a str>,
    body: &str,
    now_ms: i64,
) -> Option<u64> {
    if let Some(raw_ms) = header("retry-after-ms")
        && let Ok(ms) = raw_ms.trim().parse::<f64>()
        && ms.is_finite()
        && ms > 0.0
    {
        return Some(ms.ceil() as u64);
    }

    if let Some(raw) = header("retry-after")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Ok(secs) = raw.parse::<f64>() {
            return (secs.is_finite() && secs > 0.0).then_some((secs * 1000.0).ceil() as u64);
        }
        if let Some(target_ms) = parse_imf_fixdate_ms(raw) {
            let delta = target_ms - now_ms;
            return (delta > 0).then_some(delta as u64);
        }
    }

    for detail in gemini_error_details(body) {
        let typ = detail.get("@type").and_then(Value::as_str).unwrap_or("");
        if typ.ends_with("google.rpc.RetryInfo")
            && let Some(delay_str) = detail.get("retryDelay").and_then(Value::as_str)
            && let Some(secs_str) = delay_str.trim().strip_suffix('s')
            && let Ok(secs) = secs_str.parse::<f64>()
            && secs.is_finite()
            && secs > 0.0
        {
            return Some((secs * 1000.0).ceil() as u64);
        }
    }
    None
}

fn gemini_error_details(body: &str) -> Vec<serde_json::Map<String, Value>> {
    if body.len() > MAX_HINT_SCAN_BYTES || !body.contains("\"details\"") {
        return Vec::new();
    }
    let Ok(root) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let top = match &root {
        Value::Object(map) => Some(map),
        Value::Array(arr) => arr.first().and_then(Value::as_object),
        _ => None,
    };
    let Some(top) = top else {
        return Vec::new();
    };
    let holder = top.get("error").and_then(Value::as_object).unwrap_or(top);
    let Some(details) = holder.get("details").and_then(Value::as_array) else {
        return Vec::new();
    };
    details
        .iter()
        .filter_map(|v| v.as_object().cloned())
        .collect()
}

/// Gemini daily quota (`quotaId` containing `PerDay`): retrying within minutes cannot succeed.
pub fn is_gemini_daily_quota(body: &str) -> bool {
    gemini_error_details(body).iter().any(|detail| {
        let typ = detail.get("@type").and_then(Value::as_str).unwrap_or("");
        if !typ.ends_with("google.rpc.QuotaFailure") {
            return false;
        }
        detail
            .get("violations")
            .and_then(Value::as_array)
            .is_some_and(|violations| {
                violations.iter().any(|v| {
                    v.get("quotaId")
                        .and_then(Value::as_str)
                        .is_some_and(|qid| qid.to_ascii_lowercase().contains("perday"))
                })
            })
    })
}

pub fn is_hard_auth_or_quota_error(err: &ProtocolError) -> bool {
    if matches!(err, ProtocolError::SpendLimit(_)) {
        return true;
    }
    let status = err.http_status();
    if matches!(status, Some(401 | 403)) {
        return true;
    }
    let msg = err.to_string().to_ascii_lowercase();
    if AUTH_STOP_MARKERS.iter().any(|m| msg.contains(m))
        || BILLING_STOP_MARKERS.iter().any(|m| msg.contains(m))
    {
        return true;
    }
    status == Some(429)
        && is_gemini_daily_quota(err.provider_http().map(|h| h.body.as_str()).unwrap_or(""))
}

pub fn is_context_length_exceeded_error(err: &ProtocolError) -> bool {
    if err.http_status() == Some(413) {
        return true;
    }
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("context_length_exceeded")
        || msg.contains("maximum context length")
        || msg.contains("prompt is too long")
        || (msg.contains("input token count") && msg.contains("exceed"))
        || msg.contains("too many tokens")
        || msg.contains("exceeds the maximum number of tokens")
        || msg.contains("request too large")
}

pub fn run_error_kind_of(err: &ProtocolError) -> Option<RunErrorKind> {
    if matches!(err, ProtocolError::SpendLimit(_)) {
        return Some(RunErrorKind::SpendLimit);
    }
    if is_context_length_exceeded_error(err) {
        return Some(RunErrorKind::ConversationLimit);
    }
    let status = err.http_status();
    let msg = err.to_string().to_ascii_lowercase();
    if matches!(status, Some(401 | 403)) || AUTH_STOP_MARKERS.iter().any(|m| msg.contains(m)) {
        return Some(RunErrorKind::ApiKey);
    }
    if status == Some(402) || is_hard_auth_or_quota_error(err) {
        return Some(RunErrorKind::Quota);
    }
    if status == Some(404) && msg.contains("model") {
        return Some(RunErrorKind::Model);
    }
    None
}

pub fn is_endpoint_unsupported_for_auto_fallback(err: &ProtocolError) -> bool {
    let status = err.http_status();
    if matches!(status, Some(404 | 405 | 501)) {
        return true;
    }
    let msg = err.to_string().to_ascii_lowercase();
    let endpoint_wording = msg.contains("unknown endpoint")
        || msg.contains("unrecognized request url")
        || msg.contains("unsupported path");
    match err.provider_http() {
        Some(typed) => endpoint_wording && (400..500).contains(&typed.status),
        None => endpoint_wording || msg.contains("not found"),
    }
}

pub fn is_transient_or_proxy_timeout_error(err: &ProtocolError) -> bool {
    if matches!(
        err,
        ProtocolError::Cancelled
            | ProtocolError::NoRetry(_)
            | ProtocolError::AcceptedJob { .. }
            | ProtocolError::SpendLimit(_)
    ) {
        return false;
    }
    if is_hard_auth_or_quota_error(err) || is_endpoint_unsupported_for_auto_fallback(err) {
        return false;
    }
    let msg = err.to_string().to_ascii_lowercase();
    if msg.contains("request_too_large")
        || msg.contains("payload too large")
        || msg.contains("entity too large")
    {
        return false;
    }
    match err.http_status() {
        Some(400) => {
            !(msg.contains("invalid_request")
                || msg.contains("invalid_argument")
                || msg.contains("bad_request"))
                && (msg.contains("rate limit")
                    || msg.contains("rate_limit")
                    || msg.contains("ratelimit")
                    || msg.contains("too many requests")
                    || msg.contains("resource_exhausted")
                    || msg.contains("overload")
                    || msg.contains("timeout")
                    || msg.contains("busy")
                    || msg.contains("capacity")
                    || msg.contains("try again"))
        }
        None => true,
        Some(code) => !NON_RETRYABLE_STATUSES.contains(&code),
    }
}

pub fn should_fallback_to_legacy_protocol(
    err: &ProtocolError,
    use_legacy_fallback_endpoint: bool,
) -> bool {
    if matches!(
        err,
        ProtocolError::Cancelled
            | ProtocolError::NoRetry(_)
            | ProtocolError::AcceptedJob { .. }
            | ProtocolError::SpendLimit(_)
    ) {
        return false;
    }
    if is_hard_auth_or_quota_error(err) {
        return false;
    }
    if is_endpoint_unsupported_for_auto_fallback(err) {
        return true;
    }
    use_legacy_fallback_endpoint
}

/// Computes retry wait in ms (`attempt` is 1-based; `jitter_unit` in `[0.0, 1.0)` for deterministic testing).
pub fn retry_delay_ms(
    attempt: usize,
    err: &ProtocolError,
    jitter_unit: f64,
    fixed_ms: Option<u64>,
) -> u64 {
    let j = jitter_unit.clamp(0.0, 0.999_999);
    let hint_ms = err
        .provider_http()
        .and_then(|h| h.retry_after_ms)
        .map(|hint| {
            let base = hint.clamp(MIN_SERVER_HINT_MS, MAX_SERVER_HINT_MS);
            base + ((base as f64) * (j * 0.1)) as u64
        });
    if let Some(fixed) = fixed_ms {
        return fixed.max(hint_ms.unwrap_or(0));
    }
    if let Some(hint) = hint_ms {
        return hint;
    }
    let schedule: &[u64] = if err.http_status().is_none() {
        &CONNECTION_BACKOFF_MS
    } else {
        &SERVER_BACKOFF_MS
    };
    let idx = attempt.saturating_sub(1).min(schedule.len() - 1);
    let base = schedule[idx];
    let factor = 0.8 + j * 0.4; // ±20% jitter
    ((base as f64) * factor)
        .round()
        .clamp(0.0, MAX_RETRY_WAIT_MS as f64) as u64
}

pub fn extract_clean_api_error_summary(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let json_start = match (trimmed.find('{'), trimmed.find('[')) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    if let Some(start) = json_start {
        let prefix = trimmed[..start].trim().trim_end_matches(':').trim();
        let candidate = trimmed[start..].trim();
        if let Ok(root) = serde_json::from_str::<Value>(candidate) {
            let root_obj = match &root {
                Value::Object(m) => Some(m),
                Value::Array(a) => a.first().and_then(Value::as_object),
                _ => None,
            };
            if let Some(ro) = root_obj {
                let err_obj = ro.get("error").and_then(Value::as_object).unwrap_or(ro);
                let code = err_obj
                    .get("code")
                    .or_else(|| err_obj.get("status"))
                    .or_else(|| err_obj.get("type"))
                    .and_then(|v| match v {
                        Value::String(s) => Some(s.clone()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    });
                let msg = err_obj
                    .get("message")
                    .or_else(|| err_obj.get("detail"))
                    .or_else(|| ro.get("error"))
                    .and_then(Value::as_str);
                if let Some(m) = msg.filter(|s| !s.trim().is_empty()) {
                    let clean_msg = collapse_ws(m);
                    if !prefix.is_empty() {
                        let tag = match code.as_deref() {
                            Some(c)
                                if !c.is_empty()
                                    && !prefix
                                        .to_ascii_lowercase()
                                        .contains(&c.to_ascii_lowercase()) =>
                            {
                                format!(" ({c})")
                            }
                            _ => String::new(),
                        };
                        return format!("{prefix}{tag}: {clean_msg}");
                    }
                    let tag = match code.as_deref() {
                        Some(c) if !c.is_empty() => format!("[{c}] "),
                        _ => String::new(),
                    };
                    return format!("{tag}{clean_msg}");
                }
            }
        }
    }
    collapse_ws(trimmed)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses RFC 7231 IMF-fixdate (`Wed, 21 Oct 2026 07:28:00 GMT`) into Unix epoch ms (zero external deps).
fn parse_imf_fixdate_ms(raw: &str) -> Option<i64> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() != 6 || !parts[5].eq_ignore_ascii_case("GMT") {
        return None;
    }
    let day: i64 = parts[1].parse().ok()?;
    let month: i64 = match parts[2] {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts[3].parse().ok()?;
    let hms: Vec<&str> = parts[4].split(':').collect();
    if hms.len() != 3 {
        return None;
    }
    let hour: i64 = hms[0].parse().ok()?;
    let min: i64 = hms[1].parse().ok()?;
    let sec: i64 = hms[2].parse().ok()?;
    if !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&min)
        || !(0..=60).contains(&sec)
    {
        return None;
    }
    // Howard Hinnant civil-from-days inverse algorithm (exact proleptic Gregorian days since 1970-01-01).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3600 + min * 60 + sec;
    Some(secs * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_retry_after_ms_seconds_http_date_and_gemini_retry_info() {
        assert_eq!(
            parse_retry_after_ms(|k| (k == "retry-after-ms").then_some("1500.2"), "", 0),
            Some(1501)
        );
        assert_eq!(
            parse_retry_after_ms(|k| (k == "retry-after").then_some("12"), "", 0),
            Some(12_000)
        );
        let now_ms = parse_imf_fixdate_ms("Thu, 19 Nov 2026 10:00:00 GMT").unwrap();
        assert_eq!(
            parse_retry_after_ms(
                |k| (k == "retry-after").then_some("Thu, 19 Nov 2026 10:00:30 GMT"),
                "",
                now_ms
            ),
            Some(30_000)
        );
        let gemini_body = r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"42.5s"}]}}"#;
        assert_eq!(parse_retry_after_ms(|_| None, gemini_body, 0), Some(42_500));
    }

    #[test]
    fn daily_quota_and_billing_never_retry_while_transient_429_does() {
        let daily_body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProject"}]}]}}"#;
        let daily_err = ProtocolError::Http(ProviderHttpError {
            label: "Gemini".into(),
            status: 429,
            body: daily_body.into(),
            retry_after_ms: Some(10_000),
        });
        assert!(is_hard_auth_or_quota_error(&daily_err));
        assert!(!is_transient_or_proxy_timeout_error(&daily_err));
        assert_eq!(run_error_kind_of(&daily_err), Some(RunErrorKind::Quota));

        let transient_429 = ProtocolError::Http(ProviderHttpError {
            label: "OpenAI".into(),
            status: 429,
            body: r#"{"error":{"message":"rate limit exceeded"}}"#.into(),
            retry_after_ms: Some(15_000),
        });
        assert!(is_transient_or_proxy_timeout_error(&transient_429));
        assert_eq!(retry_delay_ms(1, &transient_429, 0.0, None), 15_000);
        // Server hint clamped to 300s max
        let huge_hint = ProtocolError::Http(ProviderHttpError {
            label: "OpenAI".into(),
            status: 429,
            body: "slow down".into(),
            retry_after_ms: Some(900_000),
        });
        assert_eq!(retry_delay_ms(1, &huge_hint, 0.0, None), 300_000);
    }

    #[test]
    fn clean_summary_unwraps_nested_json_errors() {
        let raw = r#"Anthropic HTTP 400: {"error":{"type":"invalid_request_error","message":"max_tokens: must be >= 1024"}}"#;
        assert_eq!(
            extract_clean_api_error_summary(raw),
            "Anthropic HTTP 400 (invalid_request_error): max_tokens: must be >= 1024"
        );
    }
}
