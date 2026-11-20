#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]
//! Security & end-to-end integration tests for `parchment-harness`.
//! Uses isolated temp dirs and local `127.0.0.1:0` HTTP fixtures — never live endpoints.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use base64::Engine;
use parchment_harness::codecs::{CardImportResult, extract_docx_text, import_card_or_prompt_bytes};
use parchment_harness::math_dice::{eval_math_expression, roll_dice_expression};
use parchment_harness::mcp::{MCP_FALLBACK_VERSION, McpClient, McpRegistry};
use parchment_harness::path_guard::WorkspacePathGuard;
use parchment_harness::permissions::{AgentPermissionGate, ToolAccessScope};
use parchment_harness::runner::{AgentTurnRunner, format_elapsed_whole_seconds};
use parchment_harness::ssrf::{PublicWebTransport, is_public_ip, validate_public_url};
use parchment_harness::tools::{
    ComputerUseAction, ComputerUseBackend, HarnessContext, HarnessExecutor,
};
use parchment_harness::{ToolApprovalBroker, ToolApprovalDecision, ToolAuthorization};
use parchment_model::{
    AgentPermissionPolicy, ChatSession, GoalTrackerState, McpServerConfig, MessageNode,
    MessageRole, MessageVariant, ProviderProfile, ProviderProtocol, ShellAccess, ToolApprovalMode,
    WorkspaceMode,
};
use parchment_protocol::ProtocolClient;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!("parchment_harness_{label}_{nanos}"));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn workspace_path_guard_enforces_sandbox_and_ripgrep_search_works() {
    let ws = temp_dir("ws_guard");
    let ws_str = ws.to_string_lossy().into_owned();

    // Write, edit, multi-edit, read, glob, ripgrep search inside workspace.
    WorkspacePathGuard::write_file_atomic(
        &ws_str,
        "src/main.rs",
        "fn main() {\n    println!(\"v2\");\n}\n",
    )
    .unwrap();
    WorkspacePathGuard::edit_file(&ws_str, "src/main.rs", &[("v2", "v3_native", false)]).unwrap();
    let read = WorkspacePathGuard::read_file_lines(&ws_str, "src/main.rs", 1, 50).unwrap();
    assert!(read.contains("v3_native"));

    let glob = WorkspacePathGuard::glob_files(&ws_str, "**/*.rs", 20).unwrap();
    assert!(glob.contains("src/main.rs"));

    let hits =
        WorkspacePathGuard::search_files(&ws_str, "v3_native", None, Some(".rs"), 20).unwrap();
    assert!(hits.contains("src/main.rs:2:"));

    // Traversal, absolute paths, drive letters, and NUL bytes MUST be rejected.
    assert!(WorkspacePathGuard::resolve(&ws_str, "../outside.txt").is_err());
    assert!(WorkspacePathGuard::resolve(&ws_str, "/etc/passwd").is_err());
    assert!(WorkspacePathGuard::resolve(&ws_str, "C:\\Windows\\win.ini").is_err());
    assert!(WorkspacePathGuard::resolve(&ws_str, "a\0b.txt").is_err());
    assert!(WorkspacePathGuard::delete_path(&ws_str, ".").is_err());

    let _ = fs::remove_dir_all(&ws);
}

#[tokio::test]
async fn ssrf_guard_blocks_private_ips_credentials_and_redirect_to_metadata() {
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
    assert!(!is_public_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    assert!(!is_public_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
    assert!(!is_public_ip(IpAddr::V6(
        "::ffff:127.0.0.1".parse().unwrap()
    )));
    assert!(!is_public_ip(IpAddr::V6("2001:db8::1".parse().unwrap())));
    assert!(is_public_ip(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
    assert!(is_public_ip(IpAddr::V6(
        "2606:4700:4700::1111".parse().unwrap()
    )));

    assert!(validate_public_url("http://127.0.0.1/secret").is_err());
    assert!(validate_public_url("http://169.254.169.254/latest/meta-data").is_err());
    assert!(validate_public_url("https://user:pass@example.com/a").is_err());
    assert!(validate_public_url("file:///etc/passwd").is_err());

    // Local redirect server trying to redirect to cloud metadata IP 169.254.169.254 -> BLOCKED!
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let resp = "HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/latest/meta-data\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        }
    });

    let transport = PublicWebTransport::new_for_local_tests().unwrap();
    let err = transport
        .fetch_text(&format!("http://127.0.0.1:{port}/bounce"), 4000)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("private or reserved"),
        "expected SSRF redirect block, got: {err}"
    );
}

struct MockComputerUse;

#[async_trait]
impl ComputerUseBackend for MockComputerUse {
    async fn execute(&self, action: &ComputerUseAction) -> Result<(String, String), String> {
        Ok((
            format!("Executed {}", action.action),
            "FAKE_JPEG_B64".into(),
        ))
    }
}

#[tokio::test]
async fn approval_broker_enforces_one_use_permits_invalidation_and_computer_use_never_run_wide() {
    let ws = temp_dir("approval");
    let storage = temp_dir("approval_store");
    let policy = AgentPermissionPolicy {
        approval_mode: ToolApprovalMode::AskForChanges,
        workspace_path: ws.to_string_lossy().into_owned(),
        workspace_mode: WorkspaceMode::ReadWrite,
        tools_enabled: true,
        shell_access: ShellAccess::Disabled,
        web_access: false,
        computer_access: true,
        ..Default::default()
    };
    let scope = ToolAccessScope::new(policy.clone());

    // Shell requires explicit FULL_ACCESS even in READ_WRITE.
    assert!(
        AgentPermissionGate::denial_or_unknown(
            &scope,
            &[],
            "shell_exec",
            r#"{"command":"echo hi"}"#
        )
        .is_some()
    );

    let broker = ToolApprovalBroker::new("run-1", "chat-1", scope.clone());

    // 1. computer_use: even when answered AllowExactActionForRun, MUST ask again next time!
    let broker_bg = broker.clone();
    let approve_once = tokio::spawn(async move {
        loop {
            let pending = broker_bg.pending_requests().await;
            if let Some(req) = pending.first() {
                broker_bg
                    .respond(
                        &req.request_id,
                        ToolApprovalDecision::AllowExactActionForRun,
                    )
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    let auth1 = broker
        .authorize(&[], "c1", "computer_use", r#"{"action":"screenshot"}"#)
        .await;
    approve_once.await.unwrap();
    let permit1 = match auth1 {
        ToolAuthorization::Allowed(p) => p,
        _ => panic!("expected computer_use allowed once"),
    };
    // Claim once succeeds; second claim MUST fail (one-use!).
    assert!(
        permit1
            .claim(&scope, &[], "computer_use", r#"{"action":"screenshot"}"#)
            .is_ok()
    );
    assert!(
        permit1
            .claim(&scope, &[], "computer_use", r#"{"action":"screenshot"}"#)
            .is_err()
    );

    // Calling computer_use a second time MUST still block waiting for approval (never run-wide granted!).
    let broker_bg2 = broker.clone();
    let deny_second = tokio::spawn(async move {
        loop {
            let pending = broker_bg2.pending_requests().await;
            if let Some(req) = pending.first() {
                broker_bg2
                    .respond(&req.request_id, ToolApprovalDecision::Deny)
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    let auth2 = broker
        .authorize(&[], "c2", "computer_use", r#"{"action":"screenshot"}"#)
        .await;
    deny_second.await.unwrap();
    assert!(matches!(auth2, ToolAuthorization::Denied(_)));

    // 2. Synchronous invalidate() immediately revokes an unconsumed permit.
    let broker2 = ToolApprovalBroker::new("run-2", "chat-1", scope.clone());
    let auth_read = broker2
        .authorize(&[], "c3", "fs_list", r#"{"path":""}"#)
        .await;
    let permit_read = match auth_read {
        ToolAuthorization::Allowed(p) => p,
        _ => panic!("expected read allowed without approval"),
    };
    broker2.invalidate();
    assert!(permit_read.is_revoked());
    assert!(
        permit_read
            .claim(&scope, &[], "fs_list", r#"{"path":""}"#)
            .is_err()
    );

    // 3. todo_write -> chats/<id>/TODO.md and agent_memory -> memory/AGENT.md (never workspace!).
    let mut never_policy = policy.clone();
    never_policy.approval_mode = ToolApprovalMode::Never;
    let ctx = HarnessContext {
        chat_id: "chat-42".into(),
        storage_root: storage.clone(),
        broker: ToolApprovalBroker::new("run-3", "chat-42", ToolAccessScope::new(never_policy)),
        web: Arc::new(PublicWebTransport::new_for_local_tests().unwrap()),
        mcp: None,
        computer_use: Some(Arc::new(MockComputerUse)),
        subagent_delegate: None,
        goal_state: Arc::new(Mutex::new(GoalTrackerState::default())),
        expanded_mcp_tools: Arc::new(Mutex::new(HashSet::new())),
        computer_use_settle_ms: 0,
    };
    let todo_out = HarnessExecutor::execute_tool(
        &ctx,
        "t1",
        "todo_write",
        r#"{"goal":"Ship v3","todos":[{"id":"1","content":"Run tests","status":"completed"}]}"#,
    )
    .await;
    assert!(!todo_out.is_error, "{}", todo_out.text);
    assert!(
        storage
            .join("chats")
            .join("chat-42")
            .join("TODO.md")
            .exists()
    );
    assert!(!ws.join("TODO.md").exists());

    let mem_w = HarnessExecutor::execute_tool(
        &ctx,
        "t2",
        "agent_memory",
        r#"{"action":"append","content":"Remember: use WAL."}"#,
    )
    .await;
    assert!(!mem_w.is_error, "{}", mem_w.text);
    assert!(storage.join("memory").join("AGENT.md").exists());
    assert!(!ws.join("AGENT.md").exists());
    let mem_r =
        HarnessExecutor::execute_tool(&ctx, "t3", "agent_memory", r#"{"action":"read"}"#).await;
    assert!(mem_r.text.contains("use WAL"));

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&storage);
}

#[test]
fn math_dice_docx_and_png_card_codecs_work() {
    assert_eq!(
        eval_math_expression("2 + 3 * (4 ^ 2) - sqrt(16)").unwrap(),
        "46"
    );
    assert!(eval_math_expression("1 / 0").is_err());

    let roll = roll_dice_expression("4d6kh3+2", Some("Strength")).unwrap();
    assert!(roll.contains("Strength") && roll.contains("total="));
    assert_eq!(
        format_elapsed_whole_seconds(Duration::from_secs(1221)),
        "20m21s"
    );

    // Minimal .docx zip with word/document.xml
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zw = ZipWriter::new(&mut cursor);
        zw.start_file("word/document.xml", SimpleFileOptions::default())
            .unwrap();
        zw.write_all(
            br#"<w:document><w:body><w:p><w:r><w:t>Hello &amp; welcome</w:t></w:r></w:p><w:p><w:r><w:t>Second line</w:t></w:r></w:p></w:body></w:document>"#,
        )
        .unwrap();
        zw.finish().unwrap();
    }
    let extracted = extract_docx_text(cursor.get_ref()).unwrap();
    assert_eq!(extracted, "Hello & welcome\nSecond line");

    // Minimal PNG with SillyTavern `chara` base64 tEXt chunk.
    let card_json = r#"{"spec":"chara_card_v2","data":{"name":"Archivist","description":"Keeper of scrolls","first_mes":"Greetings."}}"#;
    let b64 = base64::engine::general_purpose::STANDARD.encode(card_json.as_bytes());
    let mut payload = b"chara\0".to_vec();
    payload.extend_from_slice(b64.as_bytes());
    let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    png.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    png.extend_from_slice(b"tEXt");
    png.extend_from_slice(&payload);
    png.extend_from_slice(&0u32.to_be_bytes()); // dummy CRC
    match import_card_or_prompt_bytes(&png).unwrap() {
        CardImportResult::Card { card, .. } => {
            assert_eq!(card.name, "Archivist");
            assert_eq!(card.first_message, "Greetings.");
        }
        other => panic!("expected Card, got {other:?}"),
    }
}

#[tokio::test]
async fn mcp_http_negotiates_fallback_version_and_renews_lost_session_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let call_hits = Arc::new(AtomicUsize::new(0));
    let call_hits_bg = call_hits.clone();

    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let hits = call_hits_bg.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut tmp = [0u8; 4096];
                let header_end;
                loop {
                    let n = match sock.read(&mut tmp).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    raw.extend_from_slice(&tmp[..n]);
                    if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = p + 4;
                        break;
                    }
                }
                let hdr = String::from_utf8_lossy(&raw[..header_end]).to_string();
                let mut content_len = 0usize;
                let mut proto_ver = String::new();
                let mut sid = String::new();
                for line in hdr.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_len = v.trim().parse().unwrap_or(0);
                        } else if k.trim().eq_ignore_ascii_case("mcp-protocol-version") {
                            proto_ver = v.trim().to_string();
                        } else if k.trim().eq_ignore_ascii_case("mcp-session-id") {
                            sid = v.trim().to_string();
                        }
                    }
                }
                while raw.len() < header_end + content_len {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&tmp[..n]);
                }
                let body = String::from_utf8_lossy(&raw[header_end..header_end + content_len]);
                let msg: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

                let (status, out_sid, resp_body) = match method {
                    "initialize" => {
                        if proto_ver == "2025-11-25" {
                            (400, "", r#"{"error":"use fallback"}"#.to_string())
                        } else {
                            (
                                200,
                                "sess-renewed",
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": { "protocolVersion": MCP_FALLBACK_VERSION, "capabilities": {} }
                                })
                                .to_string(),
                            )
                        }
                    }
                    "notifications/initialized" => (202, "", String::new()),
                    "tools/list" => (
                        200,
                        "",
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "tools": [{ "name": "ping", "description": "Ping test", "inputSchema": {"type":"object"} }] }
                        })
                        .to_string(),
                    ),
                    "tools/call" => {
                        let attempt = hits.fetch_add(1, Ordering::SeqCst);
                        if attempt == 0 && sid == "sess-renewed" {
                            // Simulate expired HTTP session (404) -> registry renews ONCE!
                            (404, "", r#"{"error":"session expired"}"#.to_string())
                        } else {
                            (
                                200,
                                "",
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": { "content": [{ "type": "text", "text": "pong!" }], "isError": false }
                                })
                                .to_string(),
                            )
                        }
                    }
                    _ => (200, "", "{}".to_string()),
                };

                let mut wire = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    resp_body.len()
                );
                if !out_sid.is_empty() {
                    wire.push_str(&format!("Mcp-Session-Id: {out_sid}\r\n"));
                }
                wire.push_str("\r\n");
                wire.push_str(&resp_body);
                let _ = sock.write_all(wire.as_bytes()).await;
            });
        }
    });

    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    // Reject non-HTTPS non-loopback bearer tokens.
    assert!(McpClient::connect_http(http.clone(), "http://example.com/mcp", "secret").is_err());

    let reg = McpRegistry::new(http);
    reg.sync(&[McpServerConfig {
        id: "srv1".into(),
        name: "Demo".into(),
        url: format!("http://127.0.0.1:{port}/mcp"),
        enabled: true,
        ..Default::default()
    }])
    .await;
    let defs = reg.tool_definitions().await;
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "mcp__Demo__ping");

    let res = reg
        .call_tool("mcp__Demo__ping", serde_json::Map::new())
        .await
        .unwrap();
    assert_eq!(res.text, "pong!");
    assert_eq!(
        call_hits.load(Ordering::SeqCst),
        2,
        "expired HTTP session must renew once"
    );
}

#[tokio::test]
async fn agent_turn_runner_completes_multi_step_tool_loop_against_local_sse() {
    let ws = temp_dir("runner_ws");
    let storage = temp_dir("runner_store");
    let round_counter = Arc::new(AtomicUsize::new(0));
    let rc_bg = round_counter.clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let n_round = rc_bg.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let sse = if n_round == 0 {
                    concat!(
                        "event: response.output_text.delta\n",
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Calculating...\"}\n\n",
                        "event: response.output_item.done\n",
                        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c1\",\"name\":\"calc_eval\",\"arguments\":\"{\\\"expression\\\":\\\"6 * 7\\\"}\"}}\n\n",
                        "event: response.completed\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r1\",\"usage\":{\"input_tokens\":50,\"output_tokens\":10}}}\n\n",
                    )
                } else {
                    concat!(
                        "event: response.output_text.delta\n",
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"The answer is 42.\"}\n\n",
                        "event: response.completed\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"r2\",\"usage\":{\"input_tokens\":80,\"output_tokens\":8}}}\n\n",
                    )
                };
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    sse.len(),
                    sse
                );
                let _ = sock.write_all(wire.as_bytes()).await;
            });
        }
    });

    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let protocol = ProtocolClient::with_test_timings(http, Duration::from_secs(5), 20);
    let mut profile: ProviderProfile = serde_json::from_value(json!({
        "id": "p1",
        "name": "LocalMock",
        "protocol": ProviderProtocol::OpenAiResponses,
    }))
    .unwrap();
    profile.base_url = format!("http://127.0.0.1:{port}");
    let session: ChatSession = serde_json::from_value(json!({
        "id": "chat-run-1",
        "title": "Runner Test",
    }))
    .unwrap();
    let history = vec![MessageNode {
        id: "u1".into(),
        role: MessageRole::User,
        variants: vec![MessageVariant {
            id: "uv1".into(),
            content: "Compute 6*7".into(),
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    }];
    let policy = AgentPermissionPolicy {
        approval_mode: ToolApprovalMode::Never,
        workspace_path: ws.to_string_lossy().into_owned(),
        workspace_mode: WorkspaceMode::ReadOnly,
        tools_enabled: true,
        ..Default::default()
    };
    let ctx = HarnessContext {
        chat_id: session.id.clone(),
        storage_root: storage.clone(),
        broker: ToolApprovalBroker::new("run-loop", &session.id, ToolAccessScope::new(policy)),
        web: Arc::new(PublicWebTransport::new_for_local_tests().unwrap()),
        mcp: None,
        computer_use: None,
        subagent_delegate: None,
        goal_state: Arc::new(Mutex::new(GoalTrackerState::default())),
        expanded_mcp_tools: Arc::new(Mutex::new(HashSet::new())),
        computer_use_settle_ms: 0,
    };

    let outcome = AgentTurnRunner::run_turn(
        &protocol,
        &profile,
        "gpt-5.4",
        &session,
        &history,
        "",
        &ctx,
        "run-loop",
        0.0,
        0.0,
        None,
        |_| {},
    )
    .await
    .unwrap();

    assert_eq!(outcome.variant.content, "The answer is 42.");
    assert_eq!(outcome.variant.tool_calls.len(), 1);
    assert_eq!(outcome.variant.tool_calls[0].name, "calc_eval");
    assert_eq!(outcome.variant.tool_calls[0].result_text, "42");
    assert!(outcome.variant.tool_calls[0].starts_tool_step);
    assert_eq!(round_counter.load(Ordering::SeqCst), 2);

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&storage);
}

#[tokio::test]
async fn agent_turn_runner_accumulates_metrics_passes_prefix_and_excludes_approval_wait() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let ws = temp_dir("runner-metrics-ws");
    let storage = temp_dir("runner-metrics-store");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captured = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let captured_bg = captured.clone();

    tokio::spawn(async move {
        let mut round = 0usize;
        while let Ok((mut socket, _)) = listener.accept().await {
            round += 1;
            let mut buf = vec![0u8; 16384];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let req_str = String::from_utf8_lossy(&buf[..n]).to_string();
            captured_bg.lock().unwrap().push(req_str);
            let body = if round == 1 {
                concat!(
                    "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"reasoning_content\":\"think-1\"},\"index\":0}]}\n\n",
                    "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_w\",\"type\":\"function\",\"function\":{\"name\":\"fs_write\",\"arguments\":\"{\\\"path\\\":\\\"note.txt\\\",\\\"content\\\":\\\"hi\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20}}\n\n",
                    "data: [DONE]\n\n"
                )
            } else {
                concat!(
                    "data: {\"id\":\"c2\",\"choices\":[{\"delta\":{\"content\":\"Wrote file.\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":150,\"completion_tokens\":30}}\n\n",
                    "data: [DONE]\n\n"
                )
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });

    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let protocol = ProtocolClient::with_test_timings(http, Duration::from_secs(5), 20);
    let mut profile: ProviderProfile = serde_json::from_value(json!({
        "id": "p-runner-2",
        "name": "Local Chat",
        "protocol": "CUSTOM",
        "customDialect": "openai_chat",
    }))
    .unwrap();
    profile.base_url = format!("http://127.0.0.1:{}/v1/chat/completions", addr.port());

    let session: ChatSession = serde_json::from_value(json!({
        "id": "chat-runner-2",
        "title": "Runner Metrics Test",
    }))
    .unwrap();
    let history = vec![MessageNode {
        id: "u1".into(),
        role: MessageRole::User,
        variants: vec![MessageVariant {
            id: "uv1".into(),
            content: "Write note.txt".into(),
            ..Default::default()
        }],
        active_variant_index: 0,
        included_in_context: true,
        pinned: false,
    }];
    let policy = AgentPermissionPolicy {
        approval_mode: ToolApprovalMode::AskForChanges,
        workspace_path: ws.to_string_lossy().into_owned(),
        workspace_mode: WorkspaceMode::ReadWrite,
        tools_enabled: true,
        ..Default::default()
    };
    let broker = ToolApprovalBroker::new("run-metrics", &session.id, ToolAccessScope::new(policy));
    let broker_approver = broker.clone();
    tokio::spawn(async move {
        loop {
            let reqs = broker_approver.pending_requests().await;
            if let Some(first) = reqs.first() {
                tokio::time::sleep(Duration::from_millis(120)).await;
                broker_approver
                    .respond(&first.request_id, ToolApprovalDecision::AllowOnce)
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });

    let ctx = HarnessContext {
        chat_id: session.id.clone(),
        storage_root: storage.clone(),
        broker: broker.clone(),
        web: Arc::new(PublicWebTransport::new_for_local_tests().unwrap()),
        mcp: None,
        computer_use: None,
        subagent_delegate: None,
        goal_state: Arc::new(Mutex::new(GoalTrackerState::default())),
        expanded_mcp_tools: Arc::new(Mutex::new(HashSet::new())),
        computer_use_settle_ms: 0,
    };

    let outcome = AgentTurnRunner::run_turn(
        &protocol,
        &profile,
        "test-chat-model",
        &session,
        &history,
        "Sure, I will ",
        &ctx,
        "run-metrics",
        0.0,
        0.0,
        None,
        |_| {},
    )
    .await
    .unwrap();

    assert_eq!(outcome.variant.content, "Wrote file.");
    assert_eq!(outcome.variant.token_metrics.output_tokens, 50);
    assert_eq!(outcome.variant.token_metrics.input_tokens, 150);
    assert!(!outcome.variant.raw_request_preview.is_empty());
    assert!(!outcome.variant.raw_response_preview.is_empty());
    assert!(broker.total_wait_ms() >= 100);
    assert!(
        outcome.elapsed_ms < 110,
        "approval wait should be excluded from elapsed_ms (got {}ms)",
        outcome.elapsed_ms
    );
    let reqs = captured.lock().unwrap();
    assert!(reqs[0].contains("Sure, I will "));
    assert!(!reqs[1].contains("Sure, I will "));

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&storage);
}
