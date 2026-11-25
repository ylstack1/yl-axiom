#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]
//! End-to-end local HTTP/SSE integration tests for `parchment-protocol`.
//!
//! Uses ephemeral `127.0.0.1:0` `tokio::net::TcpListener` fixtures (never live endpoints) to
//! verify all 5 wire dialects, prompt-cache prefix key ordering (`tools` before messages),
//! `CUSTOM` strict routing + no fallback, primary->legacy fallback, HTTP 400 reasoning self-healing
//! (including Anthropic adaptive `display` stripped before dropping reasoning), `Retry-After` /
//! Gemini `RetryInfo` retries vs immediate stop on daily quota / 401, interrupted-EOF handling,
//! single-flight `RegistryCatalogClient` (`2.5 s` bounded non-cancelling wait + `models.dev` gap
//! fill + `60 s` failure cooldown), and 10,000-event SSE framing throughput.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use parchment_model::{
    CUSTOM_DIALECT_OPENAI_CHAT, CUSTOM_DIALECT_OPENAI_RESPONSES, ChatSession, MessageNode,
    MessageRole, MessageVariant, ProviderProfile, ProviderProtocol,
};
use parchment_protocol::{
    ProtocolClient, ProtocolError, RegistryCatalogClient, RegistryEndpoints, SseFramer,
    StreamCallbacks, ToolDefinition, WireDialect,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    body: String,
}

#[derive(Debug, Clone)]
struct MockResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    delay: Duration,
}

impl MockResponse {
    fn sse(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "text/event-stream".into())],
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn with_header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

async fn spawn_mock_server<F>(handler: F) -> (String, Arc<Mutex<Vec<CapturedRequest>>>)
where
    F: Fn(usize, &CapturedRequest) -> MockResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(Mutex::new(Vec::<CapturedRequest>::new()));
    let captured_bg = captured.clone();
    let handler = Arc::new(handler);

    tokio::spawn(async move {
        let mut seq = 0usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            let captured_conn = captured_bg.clone();
            let handler_conn = handler.clone();
            let idx = seq;
            seq += 1;
            tokio::spawn(async move {
                let mut raw = Vec::<u8>::with_capacity(4096);
                let mut tmp = [0u8; 4096];
                let header_end;
                loop {
                    let n = match socket.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = pos + 4;
                        break;
                    }
                }
                let header_str = String::from_utf8_lossy(&raw[..header_end]).to_string();
                let mut lines = header_str.lines();
                let req_line = lines.next().unwrap_or("");
                let mut parts = req_line.split_whitespace();
                let _method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("").to_string();
                let mut content_len = 0usize;
                for line in lines {
                    if let Some((k, v)) = line.split_once(':')
                        && k.trim().eq_ignore_ascii_case("content-length")
                    {
                        content_len = v.trim().parse().unwrap_or(0);
                    }
                }
                while raw.len() < header_end + content_len {
                    let n = match socket.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&tmp[..n]);
                }
                let body = String::from_utf8_lossy(
                    &raw[header_end..raw.len().min(header_end + content_len)],
                )
                .to_string();
                let req = CapturedRequest { path, body };
                captured_conn.lock().unwrap().push(req.clone());

                let resp = handler_conn(idx, &req);
                if !resp.delay.is_zero() {
                    tokio::time::sleep(resp.delay).await;
                }
                let reason = match resp.status {
                    200 => "OK",
                    400 => "Bad Request",
                    401 => "Unauthorized",
                    404 => "Not Found",
                    429 => "Too Many Requests",
                    500 => "Internal Server Error",
                    _ => "Status",
                };
                let mut wire = format!(
                    "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    resp.status,
                    reason,
                    resp.body.len()
                );
                for (k, v) in &resp.headers {
                    wire.push_str(&format!("{k}: {v}\r\n"));
                }
                wire.push_str("\r\n");
                wire.push_str(&resp.body);
                let _ = socket.write_all(wire.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    (format!("http://127.0.0.1:{}", addr.port()), captured)
}

fn test_http_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn make_profile(protocol: ProviderProtocol, base_url: impl Into<String>) -> ProviderProfile {
    let mut p: ProviderProfile = serde_json::from_value(json!({
        "id": "prof-test",
        "name": "Test Provider",
        "protocol": protocol,
    }))
    .unwrap();
    p.base_url = base_url.into();
    p
}

fn sample_session_and_history() -> (ChatSession, Vec<MessageNode>) {
    let mut session: ChatSession = serde_json::from_value(json!({
        "id": "chat-proto-1",
        "title": "Protocol Test",
    }))
    .unwrap();
    session.system_prompt = "You are YL Axiom test assistant.".into();
    let user = MessageNode {
        id: "m1".into(),
        role: MessageRole::User,
        variants: vec![MessageVariant {
            id: "v1".into(),
            content: "Check version and summarize.".into(),
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    };
    (session, vec![user])
}

fn sample_tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition {
        name: "fs_read".into(),
        description: "Read a workspace file".into(),
        parameters_schema: json!({
            "type": "object",
            "properties": { "path": { "type": "string" } },
            "required": ["path"]
        }),
    }]
}

#[derive(Default)]
struct RecordingCallbacks {
    text: String,
    thinking: String,
    retries: Vec<(usize, u64)>,
}

impl StreamCallbacks for RecordingCallbacks {
    fn on_text_delta(&mut self, delta: &str) {
        self.text.push_str(delta);
    }

    fn on_thinking_delta(&mut self, delta: &str) {
        self.thinking.push_str(delta);
    }

    fn on_retry_wait(&mut self, attempt: usize, wait_ms: u64, _reason: &str) {
        self.retries.push((attempt, wait_ms));
    }
}

fn assert_key_precedes(raw_json: &str, earlier_key: &str, later_key: &str) {
    let a = raw_json
        .find(&format!("\"{earlier_key}\""))
        .unwrap_or_else(|| panic!("missing key {earlier_key} in {raw_json}"));
    let b = raw_json
        .find(&format!("\"{later_key}\""))
        .unwrap_or_else(|| panic!("missing key {later_key} in {raw_json}"));
    assert!(
        a < b,
        "expected \"{earlier_key}\" (pos {a}) before \"{later_key}\" (pos {b}) for prompt-cache stability"
    );
}

#[tokio::test]
async fn all_five_dialects_stream_preserve_tool_prefix_order_and_opaque_signatures() {
    let (session, history) = sample_session_and_history();
    let tools = sample_tools();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    // 1. OpenAI Responses
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::sse(concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
            "event: response.reasoning_summary_text.delta\n",
            "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Planning \"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"reasoning\",\"encrypted_content\":\"gAAAAAB-opaque-openai-enc-blob-12345\"}}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello Responses!\"}\n\n",
            "event: response.output_item.added\n",
            "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"fs_read\",\"arguments\":\"\"}}\n\n",
            "event: response.function_call_arguments.delta\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"call_id\":\"call_1\",\"delta\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"fs_read\",\"arguments\":\"{\\\"path\\\":\\\"Cargo.toml\\\"}\"}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\",\"usage\":{\"input_tokens\":120,\"output_tokens\":30,\"input_tokens_details\":{\"cached_tokens\":80}}}}\n\n",
        ))
    })
    .await;
    let mut profile = make_profile(ProviderProtocol::OpenAiResponses, base);
    profile.api_key = "sk-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile, "gpt-5.4", &session, &history, &tools, None, "", "r1", None, &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::OpenAiResponses);
    assert_eq!(res.interaction_id, "resp_123");
    assert_eq!(res.content, "Hello Responses!");
    assert_eq!(res.thinking_summary, "Planning ");
    assert_eq!(
        res.thinking_signature,
        "gAAAAAB-opaque-openai-enc-blob-12345"
    );
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.tool_calls[0].name, "fs_read");
    assert_eq!(res.token_metrics.input_tokens, 120);
    assert_eq!(res.token_metrics.cached_input_tokens, 80);
    {
        let reqs = captured.lock().unwrap();
        assert_eq!(reqs[0].path, "/v1/responses");
        assert_key_precedes(&reqs[0].body, "tools", "input");
    }

    // 2. OpenAI Chat (via CUSTOM openai_chat)
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::sse(concat!(
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"reasoning_content\":\"Deep thought\",\"content\":\"Hello Chat!\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_chat_1\",\"function\":{\"name\":\"fs_read\",\"arguments\":\"{\\\"path\\\":\\\"README.md\\\"}\"}}]}}]}\n\n",
            "data: {\"id\":\"chatcmpl_1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":90,\"completion_tokens\":20}}\n\n",
            "data: [DONE]\n\n",
        ))
    })
    .await;
    profile = make_profile(
        ProviderProtocol::Custom,
        format!("{base}/v1/chat/completions"),
    );
    profile.custom_dialect = CUSTOM_DIALECT_OPENAI_CHAT.into();
    profile.api_key = "sk-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "deepseek-reasoner",
            &session,
            &history,
            &tools,
            None,
            "",
            "r2",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::OpenAiChat);
    assert_eq!(res.content, "Hello Chat!");
    assert_eq!(res.thinking_summary, "Deep thought");
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.token_metrics.input_tokens, 90);
    {
        let reqs = captured.lock().unwrap();
        assert_key_precedes(&reqs[0].body, "tools", "messages");
    }

    // 3. Anthropic Messages
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::sse(concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"usage\":{\"input_tokens\":200,\"cache_read_input_tokens\":100,\"cache_creation_input_tokens\":50}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Anthropic thinking\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"EqoBCkYIAxgCKkD-opaque-anthropic-sig-verbatim\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello Claude!\"}}\n\n",
            "event: content_block_stop\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":45}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        ))
    })
    .await;
    profile = make_profile(ProviderProtocol::AnthropicMessages, base);
    profile.api_key = concat!("sk-", "ant-test").into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "claude-opus-4-6",
            &session,
            &history,
            &tools,
            None,
            "",
            "r3",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::AnthropicMessages);
    assert_eq!(res.content, "Hello Claude!");
    assert_eq!(
        res.thinking_signature,
        "EqoBCkYIAxgCKkD-opaque-anthropic-sig-verbatim"
    );
    assert_eq!(res.token_metrics.cached_input_tokens, 100);
    assert_eq!(res.token_metrics.cache_creation_input_tokens, 50);
    assert_eq!(res.token_metrics.output_tokens, 45);
    {
        let reqs = captured.lock().unwrap();
        assert_eq!(reqs[0].path, "/v1/messages");
        assert_key_precedes(&reqs[0].body, "tools", "messages");
    }

    // 4. Gemini Interactions
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::sse(concat!(
            "data: {\"event_type\":\"content.delta\",\"interaction\":{\"id\":\"inter_99\"},\"index\":0,\"delta\":{\"type\":\"thought\",\"text\":\"Gemini reasoning\",\"signature\":\"ChQKEmdlbWluaS1vcGFxdWUtc2ln\"}}\n\n",
            "data: {\"event_type\":\"content.delta\",\"index\":1,\"delta\":{\"type\":\"text\",\"text\":\"Hello Gemini Interactions!\"}}\n\n",
            "data: {\"event_type\":\"interaction.complete\",\"interaction\":{\"id\":\"inter_99\",\"usage\":{\"total_input_tokens\":150,\"total_output_tokens\":40,\"total_cached_tokens\":30}}}\n\n",
        ))
    })
    .await;
    profile = make_profile(ProviderProtocol::GeminiInteractions, base);
    profile.api_key = "AIza-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gemini-3.1-pro",
            &session,
            &history,
            &tools,
            None,
            "",
            "r4",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::GeminiInteractions);
    assert_eq!(res.interaction_id, "inter_99");
    assert_eq!(res.content, "Hello Gemini Interactions!");
    assert_eq!(res.thinking_signature, "ChQKEmdlbWluaS1vcGFxdWUtc2ln");
    assert_eq!(res.token_metrics.input_tokens, 150);
    {
        let reqs = captured.lock().unwrap();
        assert_eq!(reqs[0].path, "/v1beta/interactions");
        assert_key_precedes(&reqs[0].body, "tools", "input");
    }

    // 5. Gemini generateContent (via automatic fallback when Interactions returns 404)
    let (base, captured) = spawn_mock_server(|_, req| {
        if req.path == "/v1beta/interactions" {
            MockResponse::json(404, r#"{"error":{"message":"unknown endpoint"}}"#)
        } else {
            MockResponse::sse(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"thought\":true,\"text\":\"Legacy thought\",\"thoughtSignature\":\"ChQKEmdlbWluaS1sZWdhY3ktc2ln\"},{\"text\":\"Hello generateContent!\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":77,\"candidatesTokenCount\":19}}\n\n",
            )
        }
    })
    .await;
    profile = make_profile(ProviderProtocol::GeminiInteractions, base);
    profile.api_key = "AIza-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gemini-3.1-flash",
            &session,
            &history,
            &tools,
            None,
            "",
            "r5",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::GeminiGenerateContent);
    assert_eq!(res.content, "Hello generateContent!");
    assert_eq!(res.thinking_signature, "ChQKEmdlbWluaS1sZWdhY3ktc2ln");
    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].path, "/v1beta/interactions");
    assert_eq!(
        reqs[1].path,
        "/v1beta/models/gemini-3.1-flash:streamGenerateContent?alt=sse"
    );
    assert_key_precedes(&reqs[1].body, "tools", "contents");
}

#[tokio::test]
async fn custom_protocol_blocks_unresolved_and_never_falls_back_on_404() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);
    let mut cb = RecordingCallbacks::default();

    // Unresolved custom_dialect blocks Send before any HTTP request.
    let unresolved = make_profile(ProviderProtocol::Custom, "http://127.0.0.1:9/exact");
    let err = client
        .execute_round(
            &unresolved,
            "m",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ProtocolError::NoRetry(_)));

    // Valid CUSTOM sends verbatim URL and NEVER falls back on 404.
    let (base, captured) =
        spawn_mock_server(|_, _| MockResponse::json(404, r#"{"error":{"message":"not found"}}"#))
            .await;
    let mut custom = make_profile(
        ProviderProtocol::Custom,
        format!("{base}/my/custom/exact-route"),
    );
    custom.custom_dialect = CUSTOM_DIALECT_OPENAI_RESPONSES.into();
    custom.use_legacy_fallback_endpoint = true;
    let err = client
        .execute_round(
            &custom,
            "m",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap_err();
    assert_eq!(err.http_status(), Some(404));
    let reqs = captured.lock().unwrap();
    assert_eq!(
        reqs.len(),
        1,
        "CUSTOM must never fall back to another dialect"
    );
    assert_eq!(reqs[0].path, "/my/custom/exact-route");
}

#[tokio::test]
async fn automatic_openai_responses_to_chat_fallback_on_404() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);
    let (base, captured) = spawn_mock_server(|_, req| {
        if req.path == "/v1/responses" {
            MockResponse::json(404, r#"{"error":{"message":"unrecognized request url"}}"#)
        } else {
            MockResponse::sse(
                "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"content\":\"Fallback OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
        }
    })
    .await;

    let mut profile = make_profile(ProviderProtocol::OpenAiResponses, base);
    profile.api_key = "sk-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::OpenAiChat);
    assert_eq!(res.content, "Fallback OK");
    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].path, "/v1/responses");
    assert_eq!(reqs[1].path, "/v1/chat/completions");
}

#[tokio::test]
async fn http_400_self_healing_anthropic_adaptive_display_and_effort_ladder() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    // 1. Anthropic adaptive `display` MUST be stripped BEFORE dropping reasoning!
    let (base, captured) = spawn_mock_server(|attempt, req| {
        let parsed: Value = serde_json::from_str(&req.body).unwrap();
        if attempt == 0 {
            assert!(
                parsed.get("thinking").and_then(|t| t.get("display")).is_some(),
                "first Anthropic adaptive request should include thinking.display"
            );
            MockResponse::json(
                400,
                r#"{"error":{"type":"invalid_request_error","message":"thinking.display: Extra inputs are not permitted"}}"#,
            )
        } else {
            assert!(
                parsed.get("thinking").is_some(),
                "second Anthropic request must KEEP thinking enabled after stripping display"
            );
            assert!(
                parsed.get("thinking").and_then(|t| t.get("display")).is_none(),
                "second Anthropic request must strip thinking.display"
            );
            MockResponse::sse(concat!(
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Healed Anthropic!\"}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            ))
        }
    })
    .await;
    let mut ant_profile = make_profile(ProviderProtocol::AnthropicMessages, base);
    ant_profile.api_key = "sk-ant".into();
    ant_profile.thinking_effort = "high".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &ant_profile,
            "claude-opus-4-6",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.content, "Healed Anthropic!");
    assert_eq!(captured.lock().unwrap().len(), 2);

    // 2. OpenAI Responses reasoning effort downgrade ladder: max -> xhigh -> high.
    let (base, captured) = spawn_mock_server(|_, req| {
        let parsed: Value = serde_json::from_str(&req.body).unwrap();
        let eff = parsed["reasoning"]["effort"].as_str().unwrap_or("");
        if eff == "max" || eff == "xhigh" {
            MockResponse::json(
                400,
                format!(
                    r#"{{"error":{{"message":"Invalid value: '{eff}' for reasoning.effort"}}}}"#
                ),
            )
        } else {
            MockResponse::sse(concat!(
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Healed effort!\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r_heal\"}}\n\n",
            ))
        }
    })
    .await;
    let mut oai_profile = make_profile(ProviderProtocol::OpenAiResponses, base);
    oai_profile.api_key = "sk-oai".into();
    oai_profile.thinking_effort = "max".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &oai_profile,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.content, "Healed effort!");
    let efforts: Vec<String> = captured
        .lock()
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_str::<Value>(&r.body).unwrap()["reasoning"]["effort"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(efforts, vec!["max", "xhigh", "high"]);

    // 3. `minimal -> low` step-up ladder.
    let (base, captured) = spawn_mock_server(|_, req| {
        let parsed: Value = serde_json::from_str(&req.body).unwrap();
        let eff = parsed["reasoning"]["effort"].as_str().unwrap_or("");
        if eff == "minimal" {
            MockResponse::json(
                400,
                r#"{"error":{"message":"Unsupported reasoning effort 'minimal'"}}"#,
            )
        } else {
            MockResponse::sse(concat!(
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Low OK\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r_low\"}}\n\n",
            ))
        }
    })
    .await;
    let mut oai_min = make_profile(ProviderProtocol::OpenAiResponses, base);
    oai_min.api_key = "sk-oai".into();
    oai_min.thinking_effort = "minimal".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &oai_min,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.content, "Low OK");
    let efforts: Vec<String> = captured
        .lock()
        .unwrap()
        .iter()
        .map(|r| {
            serde_json::from_str::<Value>(&r.body).unwrap()["reasoning"]["effort"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(efforts, vec!["minimal", "low"]);
}

#[tokio::test]
async fn transient_429_retries_while_daily_quota_and_401_stop_immediately() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 15);

    // 1. Transient 429 with Retry-After succeeds on attempt 2.
    let (base, captured) = spawn_mock_server(|attempt, _| {
        if attempt == 0 {
            MockResponse::json(429, r#"{"error":{"message":"rate limit exceeded"}}"#)
                .with_header("Retry-After", "2")
        } else {
            MockResponse::sse(concat!(
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Recovered!\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r_ok\"}}\n\n",
            ))
        }
    })
    .await;
    let mut profile = make_profile(ProviderProtocol::OpenAiResponses, base);
    profile.api_key = "sk-test".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.content, "Recovered!");
    assert_eq!(captured.lock().unwrap().len(), 2);
    assert_eq!(cb.retries.len(), 1);

    // 2. Gemini 429 daily quota (`PerDay`) NEVER retries.
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::json(
            429,
            r#"{"error":{"code":429,"details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaId":"GenerateRequestsPerDayPerProject"}]}]}}"#,
        )
        .with_header("Retry-After", "1")
    })
    .await;
    let mut gem_profile = make_profile(ProviderProtocol::GeminiInteractions, base);
    gem_profile.api_key = "AIza".into();
    let mut cb = RecordingCallbacks::default();
    let err = client
        .execute_round(
            &gem_profile,
            "gemini-3.1-pro",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap_err();
    assert_eq!(err.http_status(), Some(429));
    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "daily quota must never retry"
    );

    // 3. HTTP 401 invalid_api_key NEVER retries.
    let (base, captured) = spawn_mock_server(|_, _| {
        MockResponse::json(
            401,
            r#"{"error":{"code":"invalid_api_key","message":"Bad key"}}"#,
        )
    })
    .await;
    let mut bad_key = make_profile(ProviderProtocol::OpenAiResponses, base);
    bad_key.api_key = "sk-bad".into();
    let mut cb = RecordingCallbacks::default();
    let err = client
        .execute_round(
            &bad_key,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap_err();
    assert_eq!(err.http_status(), Some(401));
    assert_eq!(captured.lock().unwrap().len(), 1, "401 must never retry");
}

#[tokio::test]
async fn sse_closing_without_finish_or_done_is_interrupted() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 10);

    // Partial delta followed by abrupt EOF (no finish_reason, no [DONE]).
    let (base, _captured) = spawn_mock_server(|_, _| {
        MockResponse::sse(
            "data: {\"id\":\"c_cut\",\"choices\":[{\"delta\":{\"content\":\"halfway\"}}]}\n\n",
        )
    })
    .await;
    let mut profile = make_profile(
        ProviderProtocol::Custom,
        format!("{base}/v1/chat/completions"),
    );
    profile.custom_dialect = CUSTOM_DIALECT_OPENAI_CHAT.into();
    let mut cb = RecordingCallbacks::default();
    let err = client
        .execute_round(
            &profile,
            "gpt-5.4",
            &session,
            &history,
            &[],
            None,
            "",
            "r",
            None,
            &mut cb,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProtocolError::StreamInterrupted(_)),
        "expected StreamInterrupted, got {err:?}"
    );
    assert_eq!(cb.text, "halfway");
}

#[tokio::test]
async fn registry_single_flight_bounded_wait_does_not_cancel_download_and_models_dev_fills_gaps() {
    let (base, captured) = spawn_mock_server(|_, req| match req.path.as_str() {
        "/openrouter" => MockResponse::json(
            200,
            r#"{"data":[{"id":"openai/gpt-5.4","name":"GPT 5.4","context_length":200000,"pricing":{"prompt":"0.0000025","completion":"0.00001"},"architecture":{"input_modalities":["text","image"],"output_modalities":["text"]}}]}"#,
        )
        .with_delay(Duration::from_millis(120)),
        "/modelsdev" => MockResponse::json(
            200,
            r#"{"anthropic":{"models":{"claude-opus-4-6":{"name":"Claude Opus 4.6","cost":{"input":15.0,"output":75.0},"limit":{"context":200000,"output":32000},"modalities":{"input":["text","image"],"output":["text"]}}}}}"#,
        ),
        _ => MockResponse::json(404, "{}"),
    })
    .await;

    let reg = RegistryCatalogClient::with_timings(
        test_http_client(),
        RegistryEndpoints {
            openrouter_models_url: format!("{base}/openrouter"),
            vercel_models_url: String::new(),
            litellm_prices_url: String::new(),
            models_dev_api_url: format!("{base}/modelsdev"),
        },
        30 * 60 * 1000,
        60 * 1000,
        Duration::from_millis(40),
    );

    // Caller bounded wait (40ms) times out BEFORE the 120ms OpenRouter download finishes,
    // yet MUST NOT cancel the detached background sync.
    let t0 = Instant::now();
    let mods = reg.resolve_output_modalities("openai/gpt-5.4").await;
    assert!(mods.is_empty());
    assert!(
        t0.elapsed() < Duration::from_millis(110),
        "bounded modality wait should return promptly"
    );

    // Second concurrent ensure_sync_started joins the same single-flight sync (no duplicate HTTP calls).
    assert!(reg.ensure_sync_started(false).await);

    // Wait for the detached background download to complete.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let oai = reg
        .get_cached_spec("gpt-5.4")
        .expect("OpenRouter download must complete even after modality wait timed out");
    assert_eq!(oai.output_modalities, vec!["text"]);
    assert!((oai.input_price_per_m - 2.5).abs() < 1e-6);

    let ant = reg
        .get_cached_spec("claude-opus-4-6")
        .expect("models.dev gap-filler must populate missing models");
    assert_eq!(ant.context_window_tokens, 200_000);
    assert!((ant.output_price_per_m - 75.0).abs() < 1e-6);

    {
        let reqs = captured.lock().unwrap();
        let or_hits = reqs.iter().filter(|r| r.path == "/openrouter").count();
        let md_hits = reqs.iter().filter(|r| r.path == "/modelsdev").count();
        assert_eq!(
            or_hits, 1,
            "single-flight must issue only 1 OpenRouter request"
        );
        assert_eq!(
            md_hits, 1,
            "single-flight must issue only 1 models.dev request"
        );
    }

    // Empty-failure cooldown test: all-500 registry enforces backoff unless bypassed.
    let (fail_base, fail_captured) =
        spawn_mock_server(|_, _| MockResponse::json(500, r#"{"error":"down"}"#)).await;
    let fail_reg = RegistryCatalogClient::with_timings(
        test_http_client(),
        RegistryEndpoints {
            openrouter_models_url: format!("{fail_base}/fail"),
            vercel_models_url: String::new(),
            litellm_prices_url: String::new(),
            models_dev_api_url: String::new(),
        },
        30 * 60 * 1000,
        60 * 1000,
        Duration::from_millis(500),
    );
    assert!(fail_reg.ensure_sync_started(false).await);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(fail_captured.lock().unwrap().len(), 1);
    assert!(
        !fail_reg.ensure_sync_started(false).await,
        "must enforce 60s failure cooldown after empty failure"
    );
    assert!(
        fail_reg.ensure_sync_started(true).await,
        "bypass_failure_cooldown=true must allow manual retry"
    );
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(fail_captured.lock().unwrap().len(), 2);
}

#[test]
fn sse_framer_10k_events_throughput_probe() {
    let mut payload = Vec::with_capacity(10_000 * 96);
    for i in 0..10_000 {
        payload.extend_from_slice(
            format!(
                "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"tok_{i} \"}}\n\n"
            )
            .as_bytes(),
        );
    }

    let t0 = Instant::now();
    let mut framer = SseFramer::new();
    let mut frames = Vec::with_capacity(16);
    let mut total = 0usize;
    for chunk in payload.chunks(257) {
        frames.clear();
        framer.push_bytes(chunk, &mut frames).unwrap();
        total += frames.len();
    }
    let elapsed = t0.elapsed();
    assert_eq!(total, 10_000);
    println!(
        "[parchment-protocol] SseFramer 10,000 events across 257-byte chunks: {:.2} ms ({:.1} ns/event)",
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_nanos() as f64 / 10_000.0
    );
}

#[test]
fn smart_endpoint_url_avoids_duplicate_version_suffix_and_respects_overrides() {
    use parchment_protocol::wire::{build_auth_headers_for_key, build_endpoint_url};

    // Versioned Gemini base URL (/v1beta, /v1, /api/v1) never duplicates `/v1beta`.
    for base in [
        "https://generativelanguage.googleapis.com/v1beta",
        "https://generativelanguage.googleapis.com/v1beta/",
        "https://proxy.example.com/v1",
        "https://proxy.example.com/api/v1",
        "https://proxy.example.com/api/paas/v4",
    ] {
        let clean_base = base.trim_end_matches('/');
        let prof = make_profile(ProviderProtocol::GeminiInteractions, base);
        assert_eq!(
            build_endpoint_url(&prof, WireDialect::GeminiInteractions, "gemini-2.5-pro").unwrap(),
            format!("{clean_base}/interactions")
        );
        assert_eq!(
            build_endpoint_url(
                &prof,
                WireDialect::GeminiGenerateContent,
                "models/gemini-2.5-pro"
            )
            .unwrap(),
            format!("{clean_base}/models/gemini-2.5-pro:streamGenerateContent?alt=sse")
        );
    }

    // Concrete `/v1beta/interactions` base URL stays verbatim for primary and falls back cleanly.
    let concrete = make_profile(
        ProviderProtocol::GeminiInteractions,
        "https://generativelanguage.googleapis.com/v1beta/interactions",
    );
    assert_eq!(
        build_endpoint_url(&concrete, WireDialect::GeminiInteractions, "gemini-2.5-pro").unwrap(),
        "https://generativelanguage.googleapis.com/v1beta/interactions"
    );
    assert_eq!(
        build_endpoint_url(
            &concrete,
            WireDialect::GeminiGenerateContent,
            "gemini-2.5-pro"
        )
        .unwrap(),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
    );

    // Explicit concrete `:streamGenerateContent` base URL stays verbatim.
    let concrete_gc = make_profile(
        ProviderProtocol::GeminiInteractions,
        "https://proxy.example.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
    );
    assert_eq!(
        build_endpoint_url(
            &concrete_gc,
            WireDialect::GeminiGenerateContent,
            "gemini-2.5-pro"
        )
        .unwrap(),
        "https://proxy.example.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
    );

    // Trailing `#` opts out of path appending verbatim.
    let pinned = make_profile(
        ProviderProtocol::GeminiInteractions,
        "https://proxy.example.com/custom-gemini#",
    );
    assert_eq!(
        build_endpoint_url(
            &pinned,
            WireDialect::GeminiGenerateContent,
            "gemini-2.5-pro"
        )
        .unwrap(),
        "https://proxy.example.com/custom-gemini"
    );

    // Interactions endpoint override is ignored when falling back to generateContent.
    let mut with_inter_ov = make_profile(
        ProviderProtocol::GeminiInteractions,
        "https://generativelanguage.googleapis.com/v1beta",
    );
    with_inter_ov.endpoint_path_override = "/v1beta/interactions".into();
    assert_eq!(
        build_endpoint_url(
            &with_inter_ov,
            WireDialect::GeminiGenerateContent,
            "gemini-2.5-pro"
        )
        .unwrap(),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
    );

    // Non-googleapis Gemini proxy receives both x-goog-api-key and Bearer auth headers.
    let proxy = make_profile(
        ProviderProtocol::GeminiInteractions,
        "https://proxy.example.com/v1beta",
    );
    let headers =
        build_auth_headers_for_key(&proxy, "sk-test-key", WireDialect::GeminiGenerateContent);
    assert!(
        headers
            .iter()
            .any(|(k, v)| k == "x-goog-api-key" && v == "sk-test-key")
    );
    assert!(
        headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer sk-test-key")
    );
}

#[tokio::test]
async fn gemini_versioned_base_url_falls_back_without_duplicate_v1beta() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    let (base, captured) = spawn_mock_server(|_, req| {
        if req.path == "/v1beta/interactions" {
            MockResponse::json(
                404,
                r#"{"error":{"code":404,"message":"Method not found.","status":"NOT_FOUND"}}"#,
            )
        } else if req.path == "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse" {
            MockResponse::sse(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Versioned OK\"}]},\"finishReason\":\"STOP\"}]}\n\n",
            )
        } else {
            MockResponse::json(
                404,
                r#"{"error":{"code":404,"message":"Method not found.","status":"NOT_FOUND"}}"#,
            )
        }
    })
    .await;

    let mut profile = make_profile(
        ProviderProtocol::GeminiInteractions,
        format!("{base}/v1beta"),
    );
    profile.api_key = "AIza-versioned".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gemini-2.5-pro",
            &session,
            &history,
            &[],
            None,
            "",
            "r-ver",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::GeminiGenerateContent);
    assert_eq!(res.content, "Versioned OK");
    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].path, "/v1beta/interactions");
    assert_eq!(
        reqs[1].path,
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"
    );
}

#[test]
fn strip_inline_error_notices_strips_desktop_error_and_stopped_banners() {
    use parchment_protocol::history::strip_inline_error_notices;
    assert_eq!(strip_inline_error_notices("**Error:** HTTP 500 boom"), "");
    assert_eq!(
        strip_inline_error_notices("_Stopped before any text arrived._"),
        ""
    );
    assert_eq!(
        strip_inline_error_notices("**[Provider Error: bad key]**"),
        ""
    );
    assert_eq!(
        strip_inline_error_notices("Normal assistant reply."),
        "Normal assistant reply."
    );
}

#[test]
fn attachments_extra_body_builtin_tools_and_multistep_serialize_across_all_5_dialects() {
    use parchment_model::{PromptAttachment, ToolCallRecord};
    use parchment_protocol::wire::{WireAdaptOptions, build_wire_request};

    let (session, mut history) = sample_session_and_history();
    history[0].variants[0].attachments = vec![
        PromptAttachment {
            id: "a1".into(),
            file_name: "photo.png".into(),
            mime_type: "image/png".into(),
            base64_data: "iVBORw0KGgo=".into(),
            text_content: String::new(),
            local_file_path: String::new(),
        },
        PromptAttachment {
            id: "a2".into(),
            file_name: "notes.txt".into(),
            mime_type: "text/plain".into(),
            base64_data: String::new(),
            text_content: "hello from attachment".into(),
            local_file_path: String::new(),
        },
        PromptAttachment {
            id: "a3".into(),
            file_name: "spec.pdf".into(),
            mime_type: "application/pdf".into(),
            base64_data: "JVBERi0xLjQ=".into(),
            text_content: String::new(),
            local_file_path: String::new(),
        },
    ];
    history.push(MessageNode {
        id: "m2".into(),
        role: MessageRole::Assistant,
        variants: vec![MessageVariant {
            id: "v2".into(),
            content: "Final summary.".into(),
            tool_calls: vec![
                ToolCallRecord {
                    id: "c1".into(),
                    name: "fs_read".into(),
                    arguments_json: r#"{"path":"a.txt"}"#.into(),
                    result_text: "alpha".into(),
                    starts_tool_step: true,
                    ..Default::default()
                },
                ToolCallRecord {
                    id: "c2".into(),
                    name: "fs_read".into(),
                    arguments_json: r#"{"path":"b.txt"}"#.into(),
                    result_text: "beta".into(),
                    starts_tool_step: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    });

    let cases = [
        (
            ProviderProtocol::OpenAiResponses,
            WireDialect::OpenAiResponses,
        ),
        (ProviderProtocol::OpenAiResponses, WireDialect::OpenAiChat),
        (
            ProviderProtocol::AnthropicMessages,
            WireDialect::AnthropicMessages,
        ),
        (
            ProviderProtocol::GeminiInteractions,
            WireDialect::GeminiInteractions,
        ),
        (
            ProviderProtocol::GeminiInteractions,
            WireDialect::GeminiGenerateContent,
        ),
    ];
    for (proto, dialect) in cases {
        let mut profile = make_profile(proto, "https://example.invalid");
        profile.thinking_effort.clear();
        profile.enable_web_search = true;
        profile.enable_code_execution = true;
        profile.enable_url_context = true;
        profile.raw_json_extra_body =
            r#"{"custom_flag": true, "tools": ["smuggled"], "tool_choice": "auto"}"#.into();

        let req = build_wire_request(
            &profile,
            dialect,
            "test-model",
            &session,
            &history,
            &sample_tools(),
            None,
            "Prefill:",
            &WireAdaptOptions::default(),
        )
        .unwrap();
        let body: Value = serde_json::from_str(&req.body_json).unwrap();
        assert_eq!(body["custom_flag"], json!(true), "dialect={dialect:?}");
        assert!(body.get("tool_choice").is_none(), "disallowed key leaked");
        assert!(
            !req.body_json.contains("smuggled"),
            "smuggled tools leaked on {dialect:?}"
        );
        assert!(
            req.body_json.contains("iVBORw0KGgo="),
            "image missing on {dialect:?}"
        );
        assert!(
            req.body_json.contains("hello from attachment"),
            "inline text missing on {dialect:?}"
        );
        assert!(
            req.body_json.contains("JVBERi0xLjQ="),
            "PDF missing on {dialect:?}"
        );
        assert!(
            req.body_json.contains("Prefill:"),
            "continue_prefix missing on {dialect:?}"
        );
        match dialect {
            WireDialect::OpenAiResponses => {
                assert!(req.body_json.contains("web_search_preview"));
                assert!(req.body_json.contains("code_interpreter"));
            }
            WireDialect::AnthropicMessages => {
                assert!(req.body_json.contains("web_search_20250305"));
                assert!(body.get("thinking").is_none());
                // Two separate starts_tool_step rounds => 2 assistant tool_use turns + 2 user tool_result turns + closing assistant + continue_prefix
                let msgs = body["messages"].as_array().unwrap();
                assert!(
                    msgs.len() >= 6,
                    "expected multi-step Anthropic replay: {msgs:?}"
                );
            }
            WireDialect::GeminiInteractions => {
                assert!(req.body_json.contains("google_search"));
                assert!(req.body_json.contains("code_execution"));
                assert!(req.body_json.contains("url_context"));
                let inp = body["input"].as_array().unwrap();
                assert!(
                    inp.len() >= 6,
                    "expected multi-step Gemini Interactions replay: {inp:?}"
                );
            }
            WireDialect::GeminiGenerateContent => {
                assert!(req.body_json.contains("google_search"));
                assert!(req.body_json.contains("code_execution"));
                assert!(req.body_json.contains("url_context"));
                let contents = body["contents"].as_array().unwrap();
                assert!(
                    contents.len() >= 6,
                    "expected multi-step Gemini GenerateContent replay: {contents:?}"
                );
            }
            WireDialect::OpenAiChat => {}
        }
    }
}

#[tokio::test]
async fn openai_responses_item_id_tool_deltas_and_url_citations_parse_cleanly() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    let sse = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_42\"}}\n\n",
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_item_1\",\"call_id\":\"call_real_99\",\"type\":\"function_call\",\"name\":\"fs_read\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_item_1\",\"delta\":\"{\\\"path\\\":\\\"a\"}\n\n",
        "event: response.function_call_arguments.delta\ndata: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_item_1\",\"delta\":\".rs\\\"}\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_item_1\",\"call_id\":\"call_real_99\",\"type\":\"function_call\",\"name\":\"fs_read\",\"arguments\":\"\"}}\n\n",
        "event: response.output_text.annotation.added\ndata: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"url\":\"https://docs.rs/yl-axiom\",\"title\":\"YL Axiom Docs\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_42\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}}\n\n",
    );
    let (base, _) = spawn_mock_server(move |_, _| MockResponse::sse(sse)).await;
    let profile = make_profile(ProviderProtocol::OpenAiResponses, base);
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "test-model",
            &session,
            &history,
            &sample_tools(),
            None,
            "",
            "r-resp-deltas",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.tool_calls.len(), 1);
    assert_eq!(res.tool_calls[0].id, "call_real_99");
    assert_eq!(res.tool_calls[0].arguments_json, r#"{"path":"a.rs"}"#);
    assert_eq!(res.citations.len(), 1);
    assert_eq!(res.citations[0].url, "https://docs.rs/parchment");
    assert!(!res.raw_request_preview.is_empty());
    assert!(!res.raw_response_preview.is_empty());
}

#[tokio::test]
async fn gemini_plain_json_array_grounding_citations_and_thinking_budget_heal() {
    let (session, history) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    let (base, captured) = spawn_mock_server(|_, req| {
        if req.path.contains("/interactions") {
            return MockResponse::json(404, r#"{"error":{"message":"not found"}}"#);
        }
        if req.body.contains("thinkingLevel") {
            return MockResponse::json(
                400,
                r#"{"error":{"message":"Invalid JSON payload: unknown field thinkingLevel"}}"#,
            );
        }
        MockResponse::json(
            200,
            r#"[{"candidates":[{"content":{"parts":[{"thought":true,"text":"plan","thought_signature":"sig_snake_ok"},{"text":"Grounded answer."}]},"groundingMetadata":{"groundingChunks":[{"web":{"uri":"https://example.org/fact","title":"Fact Source"}}]},"finishReason":"STOP"}]}]"#,
        )
    })
    .await;

    let mut profile = make_profile(ProviderProtocol::GeminiInteractions, base);
    profile.thinking_effort = "medium".into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "gemini-test",
            &session,
            &history,
            &[],
            None,
            "",
            "r-gem-arr",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.dialect_used, WireDialect::GeminiGenerateContent);
    assert_eq!(res.content, "Grounded answer.");
    assert_eq!(res.thinking_signature, "sig_snake_ok");
    assert_eq!(res.citations.len(), 1);
    assert_eq!(res.citations[0].url, "https://example.org/fact");
    let reqs = captured.lock().unwrap();
    assert!(
        reqs.last()
            .unwrap()
            .body
            .contains("\"thinkingBudget\":8192")
    );
}

#[tokio::test]
async fn openai_chat_stream_options_heal_and_context_overflow_auto_compaction() {
    let (session, _) = sample_session_and_history();
    let client = ProtocolClient::with_test_timings(test_http_client(), Duration::from_secs(5), 20);

    let mut history = Vec::new();
    for i in 0..8 {
        history.push(MessageNode {
            id: format!("m-{i}"),
            role: if i % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            },
            variants: vec![MessageVariant {
                id: format!("v-{i}"),
                content: format!("Turn number {i} content"),
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        });
    }

    let (base, captured) = spawn_mock_server(|_, req| {
        if req.body.contains("stream_options") {
            return MockResponse::json(
                400,
                r#"{"error":{"message":"Unrecognized request argument supplied: stream_options"}}"#,
            );
        }
        if !req.body.contains("Auto-Compacted Earlier Conversation Continuity Digest") {
            return MockResponse::json(
                400,
                r#"{"error":{"message":"This model's maximum context length is 8192 tokens (context_length_exceeded)"}}"#,
            );
        }
        MockResponse::sse(
            "data: {\"id\":\"chatcmpl_ok\",\"choices\":[{\"delta\":{\"content\":\"Compacted OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        )
    })
    .await;

    let mut profile = make_profile(
        ProviderProtocol::Custom,
        format!("{base}/v1/chat/completions"),
    );
    profile.custom_dialect = CUSTOM_DIALECT_OPENAI_CHAT.into();
    let mut cb = RecordingCallbacks::default();
    let res = client
        .execute_round(
            &profile,
            "proxy-model",
            &session,
            &history,
            &[],
            None,
            "",
            "r-overflow",
            None,
            &mut cb,
        )
        .await
        .unwrap();
    assert_eq!(res.content, "Compacted OK");
    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 3);
    assert!(reqs[0].body.contains("stream_options"));
    assert!(!reqs[1].body.contains("stream_options"));
    assert!(
        reqs[2]
            .body
            .contains("Auto-Compacted Earlier Conversation Continuity Digest")
    );
}
