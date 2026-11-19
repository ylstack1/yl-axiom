//! SSRF-hardened `PublicWebTransport` (independent of LLM provider HTTP transport/proxy).
//!
//! Security contracts:
//! - Rejects URLs with embedded credentials, non-http(s) schemes, scoped IPv6 `%`, and any IANA
//!   special-purpose / loopback / RFC1918 / link-local / CGNAT / ULA / multicast / IPv4-mapped IPv6.
//! - Uses a custom `reqwest::dns::Resolve` so TCP sockets bind ONLY to the validated public IPs
//!   (eliminating DNS rebinding TOCTOU).
//! - Disables automatic redirects (`Policy::none()`) and re-validates every redirect hop (max 6).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use thiserror::Error;
use url::Url;

const MAX_REDIRECT_HOPS: usize = 6;

#[derive(Debug, Error)]
pub enum WebTransportError {
    #[error("{0}")]
    Blocked(String),
    #[error("HTTP {status}: {snippet}")]
    HttpStatus { status: u16, snippet: String },
    #[error("Web transport error: {0}")]
    Transport(String),
}

/// Checks whether `ip` is a routable public internet address (exact IANA special-purpose exclusions
/// matching v2 `PublicWebTransport.kt`, plus IPv4-mapped IPv6 unwrapping).
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_v4(mapped);
            }
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            let segs = v6.segments();
            // Require global unicast `2000::/3` (high 3 bits == 001 -> 0x2000..=0x3fff)
            if (segs[0] & 0xe000) != 0x2000 {
                return false;
            }
            // Exclude IANA special-purpose inside 2000::/3:
            // 2001::/23 (0x2001, 0x0000..=0x01ff), 2001:db8::/32, 2002::/16 (6to4), 3fff::/20 (docs)
            if segs[0] == 0x2001 && (segs[1] & 0xfe00) == 0x0000 {
                return false;
            }
            if segs[0] == 0x2001 && segs[1] == 0x0db8 {
                return false;
            }
            if segs[0] == 0x2002 {
                return false;
            }
            if segs[0] == 0x3fff && (segs[1] & 0xf000) == 0x0000 {
                return false;
            }
            true
        }
    }
}

fn is_public_v4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _d] = v4.octets();
    if a == 0 // 0.0.0.0/8
        || a == 10 // 10.0.0.0/8
        || (a == 100 && (64..=127).contains(&b)) // 100.64.0.0/10 CGNAT
        || a == 127 // 127.0.0.0/8 loopback
        || (a == 169 && b == 254) // 169.254.0.0/16 link-local / cloud metadata
        || (a == 172 && (16..=31).contains(&b)) // 172.16.0.0/12
        || (a == 192 && b == 0 && c == 0) // 192.0.0.0/24
        || (a == 192 && b == 0 && c == 2) // 192.0.2.0/24
        || (a == 192 && b == 88 && c == 99) // 192.88.99.0/24
        || (a == 192 && b == 168) // 192.168.0.0/16
        || (a == 198 && (18..=19).contains(&b)) // 198.18.0.0/15
        || (a == 198 && b == 51 && c == 100) // 198.51.100.0/24
        || (a == 203 && b == 0 && c == 113) // 203.0.113.0/24
        || a >= 224
    // 224.0.0.0/4 multicast + 240.0.0.0/4 reserved
    {
        return false;
    }
    true
}

/// Validates syntactic URL safety and rejects literal private/reserved IP hosts immediately.
pub fn validate_public_url(raw: &str) -> Result<Url, WebTransportError> {
    let url = Url::parse(raw.trim())
        .map_err(|e| WebTransportError::Blocked(format!("Invalid URL: {e}")))?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(WebTransportError::Blocked(
            "Agent web URLs must use http or https.".into(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(WebTransportError::Blocked(
            "Agent web URLs cannot include credentials.".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| WebTransportError::Blocked("URL must include a host.".into()))?;
    if host.contains('%') {
        return Err(WebTransportError::Blocked(
            "Scoped IP addresses are not allowed.".into(),
        ));
    }
    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost") {
        return Err(WebTransportError::Blocked(
            "Agent web tools cannot access private or reserved destinations.".into(),
        ));
    }
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>()
        && !is_public_ip(ip)
    {
        return Err(WebTransportError::Blocked(
            "Agent web tools cannot access private or reserved destinations.".into(),
        ));
    }
    Ok(url)
}

/// Custom DNS resolver that enforces public-only IPs right at socket connect time.
struct PublicOnlyDnsResolver {
    allow_loopback_for_tests: bool,
}

impl Resolve for PublicOnlyDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let allow_loopback = self.allow_loopback_for_tests;
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No DNS addresses resolved.",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            for addr in &addrs {
                let ip = addr.ip();
                let allowed = is_public_ip(ip) || (allow_loopback && ip.is_loopback());
                if !allowed {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "This request can connect only to public internet addresses.",
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
            }
            let iter: Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

pub struct PublicWebTransport {
    client: reqwest::Client,
    allow_loopback_for_tests: bool,
}

impl PublicWebTransport {
    pub fn new() -> Result<Self, WebTransportError> {
        Self::build(false)
    }

    /// Only for localhost redirect/SSRF tests — still rejects non-loopback private ranges (e.g. `10.0.0.1`, `169.254.169.254`).
    pub fn new_for_local_tests() -> Result<Self, WebTransportError> {
        Self::build(true)
    }

    fn build(allow_loopback_for_tests: bool) -> Result<Self, WebTransportError> {
        let resolver = Arc::new(PublicOnlyDnsResolver {
            allow_loopback_for_tests,
        });
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .no_proxy()
            .redirect(Policy::none())
            .dns_resolver(resolver)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| WebTransportError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            allow_loopback_for_tests,
        })
    }

    fn validate_hop(&self, raw: &str) -> Result<Url, WebTransportError> {
        if !self.allow_loopback_for_tests {
            return validate_public_url(raw);
        }
        let url = Url::parse(raw.trim())
            .map_err(|e| WebTransportError::Blocked(format!("Invalid URL: {e}")))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(WebTransportError::Blocked(
                "Agent web URLs must use http or https.".into(),
            ));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(WebTransportError::Blocked(
                "Agent web URLs cannot include credentials.".into(),
            ));
        }
        if let Some(host) = url.host_str() {
            let bare = host.trim_start_matches('[').trim_end_matches(']');
            if let Ok(ip) = bare.parse::<IpAddr>()
                && !is_public_ip(ip)
                && !ip.is_loopback()
            {
                return Err(WebTransportError::Blocked(
                    "Agent web tools cannot access private or reserved destinations.".into(),
                ));
            }
        }
        Ok(url)
    }

    /// Fetches a URL across up to 6 validated redirect hops and converts HTML to clean Markdown/text.
    pub async fn fetch_text(
        &self,
        url: &str,
        max_chars: usize,
    ) -> Result<String, WebTransportError> {
        let limit = max_chars.clamp(500, 100_000);
        let raw_byte_limit = (limit * 4).clamp(32_000, 400_000);
        let (bytes, content_type) = self
            .request_with_redirects(
                url,
                "text/markdown, text/html, application/xhtml+xml, application/json, text/plain;q=0.9, */*;q=0.8",
                raw_byte_limit,
            )
            .await?;
        let raw = String::from_utf8_lossy(&bytes);
        let readable = convert_html_to_readable_text(&raw, &content_type);
        Ok(readable.chars().take(limit).collect())
    }

    /// Fetches bounded raw bytes across validated redirect hops.
    pub async fn fetch_bytes(
        &self,
        url: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, WebTransportError> {
        let (bytes, _) = self.request_with_redirects(url, "*/*", max_bytes).await?;
        Ok(bytes)
    }

    async fn request_with_redirects(
        &self,
        url: &str,
        accept: &str,
        max_bytes: usize,
    ) -> Result<(Vec<u8>, String), WebTransportError> {
        let mut target = self.validate_hop(url)?;
        for _ in 0..MAX_REDIRECT_HOPS {
            let resp = self
                .client
                .get(target.as_str())
                .header("User-Agent", "ParchmentAI/3.0")
                .header("Accept", accept)
                .send()
                .await
                .map_err(|e| WebTransportError::Transport(e.to_string()))?;

            let status = resp.status().as_u16();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        WebTransportError::Blocked(
                            "Redirect response missing Location header.".into(),
                        )
                    })?;
                let next = target.join(loc).map_err(|e| {
                    WebTransportError::Blocked(format!("Invalid redirect URL: {e}"))
                })?;
                target = self.validate_hop(next.as_str())?;
                continue;
            }

            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            let mut out = Vec::with_capacity(max_bytes.min(32 * 1024));
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| WebTransportError::Transport(e.to_string()))?;
                let take = chunk.len().min(max_bytes.saturating_sub(out.len()));
                out.extend_from_slice(&chunk[..take]);
                if out.len() >= max_bytes {
                    break;
                }
            }
            if !(200..300).contains(&status) {
                let snippet: String = String::from_utf8_lossy(&out).chars().take(400).collect();
                return Err(WebTransportError::HttpStatus { status, snippet });
            }
            return Ok((out, content_type));
        }
        Err(WebTransportError::Blocked(
            "Web request exceeded the redirect limit.".into(),
        ))
    }
}

/// Lightweight streaming HTML -> clean readable text/Markdown converter (strips `<script>`/`<style>`).
pub fn convert_html_to_readable_text(raw: &str, content_type: &str) -> String {
    let ct = content_type.to_ascii_lowercase();
    let looks_html = ct.contains("html")
        || (!ct.contains("json")
            && !ct.contains("plain")
            && !ct.contains("markdown")
            && (raw.contains("<html") || raw.contains("<body") || raw.contains("<div")));
    if !looks_html {
        return raw.to_string();
    }

    let mut out = String::with_capacity(raw.len() / 2);
    let mut i = 0usize;
    let bytes = raw.as_bytes();
    let mut skip_tag: Option<&'static str> = None;

    while i < bytes.len() {
        if bytes[i] == b'<' {
            let Some(rel_gt) = raw[i..].find('>') else {
                break;
            };
            let tag = raw[i + 1..i + rel_gt].trim();
            i += rel_gt + 1;
            let low = tag.to_ascii_lowercase();
            if let Some(active) = skip_tag {
                if low.starts_with('/') && low[1..].trim_start().starts_with(active) {
                    skip_tag = None;
                }
                continue;
            }
            if low.starts_with("script") {
                if !low.ends_with('/') {
                    skip_tag = Some("script");
                }
                continue;
            }
            if low.starts_with("style") {
                if !low.ends_with('/') {
                    skip_tag = Some("style");
                }
                continue;
            }
            if low.starts_with("br")
                || low.starts_with("/p")
                || low.starts_with("/div")
                || low.starts_with("/li")
                || low.starts_with("/tr")
                || low.starts_with("/h1")
                || low.starts_with("/h2")
                || low.starts_with("/h3")
            {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            } else if low.starts_with("li") {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
                out.push_str("- ");
            }
            continue;
        }
        if skip_tag.is_some() {
            i += 1;
            continue;
        }
        let next_lt = raw[i..].find('<').map(|r| i + r).unwrap_or(bytes.len());
        let decoded = decode_basic_entities(&raw[i..next_lt]);
        for part in decoded.split_whitespace() {
            if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(part);
        }
        i = next_lt;
    }

    // Collapse excess blank lines.
    let mut cleaned = String::with_capacity(out.len());
    let mut prev_blank = false;
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !prev_blank && !cleaned.is_empty() {
                cleaned.push('\n');
            }
            prev_blank = true;
        } else {
            cleaned.push_str(t);
            cleaned.push('\n');
            prev_blank = false;
        }
    }
    cleaned.trim().to_string()
}

fn decode_basic_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[allow(dead_code)]
fn _assert_ipv6_type(_: Ipv6Addr) {}
