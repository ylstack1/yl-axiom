//! Model Context Protocol (MCP) JSON-RPC client & live server registry.
//!
//! Contracts:
//! - Handshake spec `2025-11-25` with automatic fallback to `2026-07-28`.
//! - Streamable HTTP (JSON or SSE responses, bearer tokens restricted to HTTPS or loopback,
//!   `Mcp-Session-Id` tracking with single session renewal on 404).
//! - Stdio child process transport (newline-delimited JSON-RPC over child stdin/stdout).
//! - `McpRegistry` names tools `mcp__<server>__<tool>` (<= 64 chars), restarts stopped servers
//!   at most `3` times in `10` minutes under identical tool names, NEVER resending the call
//!   that discovered the server had stopped.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parchment_model::McpServerConfig;
use parchment_protocol::SseFramer;
use serde_json::{Map, Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use url::Url;

use crate::permissions::{HarnessToolDefinition, MCP_TOOL_PREFIX};

pub const MCP_HANDSHAKE_VERSION: &str = "2025-11-25";
pub const MCP_FALLBACK_VERSION: &str = "2026-07-28";
const MAX_MCP_TOOL_NAME_CHARS: usize = 64;
const MAX_STARTS_PER_WINDOW: usize = 3;
const START_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Error)]
pub enum McpError {
    #[error("{0}")]
    Protocol(String),
    #[error("MCP HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("MCP transport error: {0}")]
    Transport(String),
}

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct McpToolResult {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerState {
    Connecting,
    Ready,
    Failed,
}

#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub state: McpServerState,
    pub tool_count: usize,
    pub message: String,
}

enum McpTransport {
    Http(HttpMcpTransport),
    Stdio(Box<StdioMcpTransport>),
}

struct HttpMcpTransport {
    http: reqwest::Client,
    url: String,
    bearer_token: String,
    protocol_version: Mutex<String>,
    session_id: Mutex<String>,
    session_lost: AtomicBool,
}

impl HttpMcpTransport {
    fn new(http: reqwest::Client, url: &str, bearer_token: &str) -> Result<Self, McpError> {
        let clean_url = url.trim().to_string();
        let clean_token = sanitize_header(bearer_token);
        let parsed = Url::parse(&clean_url)
            .map_err(|_| McpError::Protocol("Invalid MCP server URL.".into()))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" {
            return Err(McpError::Protocol(
                "MCP server URL must use http or https.".into(),
            ));
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| McpError::Protocol("MCP server URL must include a host.".into()))?;
        let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
        if parsed.scheme() != "https" && !loopback && !clean_token.is_empty() {
            return Err(McpError::Protocol(
                "A token can be sent only over https or to this device.".into(),
            ));
        }
        Ok(Self {
            http,
            url: clean_url,
            bearer_token: clean_token,
            protocol_version: Mutex::new(MCP_HANDSHAKE_VERSION.to_string()),
            session_id: Mutex::new(String::new()),
            session_lost: AtomicBool::new(false),
        })
    }

    async fn send_rpc(
        &self,
        msg: &Value,
        expect_response: bool,
    ) -> Result<Option<Value>, McpError> {
        let proto = self.protocol_version.lock().await.clone();
        let sess = self.session_id.lock().await.clone();
        let mut req = self
            .http
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", sanitize_header(&proto));
        if let Some(method) = msg.get("method").and_then(Value::as_str) {
            req = req.header("Mcp-Method", sanitize_header(method));
            if method == "tools/call"
                && let Some(name) = msg
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str)
            {
                req = req.header("Mcp-Name", sanitize_header(name));
            }
        }
        if !self.bearer_token.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.bearer_token));
        }
        if !sess.is_empty() {
            req = req.header("Mcp-Session-Id", sanitize_header(&sess));
        }
        let resp = req
            .body(msg.to_string())
            .send()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;

        if let Some(new_sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(sanitize_header)
            .filter(|s| !s.is_empty())
        {
            *self.session_id.lock().await = new_sid;
        }

        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if status == 404 && !sess.is_empty() {
            self.session_id.lock().await.clear();
            self.session_lost.store(true, Ordering::SeqCst);
        }
        let body = resp
            .text()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;

        if !expect_response {
            if !(200..300).contains(&status) {
                return Err(McpError::Http {
                    status,
                    message: format!("Notification rejected: {body}"),
                });
            }
            return Ok(None);
        }
        if !(200..300).contains(&status) {
            return Err(McpError::Http {
                status,
                message: body.chars().take(400).collect(),
            });
        }

        let expected_id = msg.get("id");
        if content_type.contains("text/event-stream") {
            let mut framer = SseFramer::new();
            let mut frames = Vec::new();
            let _ = framer.push_bytes(body.as_bytes(), &mut frames);
            let _ = framer.finish(&mut frames);
            for frame in frames {
                if let Ok(v) = serde_json::from_str::<Value>(&frame.data)
                    && v.get("id") == expected_id
                    && (v.get("result").is_some() || v.get("error").is_some())
                {
                    return Ok(Some(v));
                }
            }
            return Err(McpError::Protocol(
                "MCP SSE stream closed without matching JSON-RPC response.".into(),
            ));
        }

        let parsed: Value = serde_json::from_str(body.trim())
            .map_err(|e| McpError::Protocol(format!("Invalid MCP JSON response: {e}")))?;
        Ok(Some(parsed))
    }
}

struct StdioMcpTransport {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    alive: AtomicBool,
}

impl StdioMcpTransport {
    fn spawn(command_line: &str) -> Result<Self, McpError> {
        let parts = split_command_args(command_line);
        let Some((program, args)) = parts.split_first() else {
            return Err(McpError::Protocol("MCP command is empty.".into()));
        };
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| McpError::Transport(format!("Failed spawning MCP process: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("Missing child stdin.".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("Missing child stdout.".into()))?;
        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            alive: AtomicBool::new(true),
        })
    }

    async fn send_rpc(
        &self,
        msg: &Value,
        expect_response: bool,
    ) -> Result<Option<Value>, McpError> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err(McpError::Transport("MCP server process stopped.".into()));
        }
        {
            let mut stdin = self.stdin.lock().await;
            let mut line = msg.to_string();
            line.push('\n');
            if let Err(e) = stdin.write_all(line.as_bytes()).await {
                self.alive.store(false, Ordering::SeqCst);
                return Err(McpError::Transport(e.to_string()));
            }
            if let Err(e) = stdin.flush().await {
                self.alive.store(false, Ordering::SeqCst);
                return Err(McpError::Transport(e.to_string()));
            }
        }
        if !expect_response {
            return Ok(None);
        }
        let expected_id = msg.get("id");
        let mut stdout = self.stdout.lock().await;
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = stdout
                .read_line(&mut buf)
                .await
                .map_err(|e| McpError::Transport(e.to_string()))?;
            if n == 0 {
                self.alive.store(false, Ordering::SeqCst);
                return Err(McpError::Transport("MCP server closed stdout.".into()));
            }
            if let Ok(v) = serde_json::from_str::<Value>(buf.trim())
                && v.get("id") == expected_id
                && (v.get("result").is_some() || v.get("error").is_some())
            {
                return Ok(Some(v));
            }
        }
    }

    async fn close(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}

pub struct McpClient {
    transport: McpTransport,
    next_id: AtomicU64,
}

impl McpClient {
    pub fn connect_http(
        http: reqwest::Client,
        url: &str,
        bearer_token: &str,
    ) -> Result<Self, McpError> {
        Ok(Self {
            transport: McpTransport::Http(HttpMcpTransport::new(http, url, bearer_token)?),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn connect_stdio(command_line: &str) -> Result<Self, McpError> {
        Ok(Self {
            transport: McpTransport::Stdio(Box::new(StdioMcpTransport::spawn(command_line)?)),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn is_alive(&self) -> bool {
        match &self.transport {
            McpTransport::Http(_) => true,
            McpTransport::Stdio(s) => s.alive.load(Ordering::SeqCst),
        }
    }

    pub fn take_lost_session(&self) -> bool {
        match &self.transport {
            McpTransport::Http(h) => h.session_lost.swap(false, Ordering::SeqCst),
            McpTransport::Stdio(_) => false,
        }
    }

    /// Negotiates MCP `2025-11-25`, automatically falling back to `2026-07-28`.
    pub async fn initialize(&self) -> Result<String, McpError> {
        match self.try_handshake(MCP_HANDSHAKE_VERSION).await {
            Ok(v) => Ok(v),
            Err(_) => self.try_handshake(MCP_FALLBACK_VERSION).await,
        }
    }

    async fn try_handshake(&self, version: &str) -> Result<String, McpError> {
        if let McpTransport::Http(h) = &self.transport {
            *h.protocol_version.lock().await = version.to_string();
        }
        let resp = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": { "name": "Parchment", "version": parchment_model::APP_VERSION }
                }),
            )
            .await?;
        let negotiated = resp
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(version)
            .to_string();
        if let McpTransport::Http(h) = &self.transport {
            *h.protocol_version.lock().await = negotiated.clone();
        }
        let _ = self.notify("notifications/initialized", json!({})).await;
        Ok(negotiated)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        let resp = self.request("tools/list", json!({})).await?;
        let tools = resp
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .into_iter()
            .filter_map(|t| {
                let obj = t.as_object()?;
                let name = obj.get("name")?.as_str()?.to_string();
                let description = obj
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input_schema = obj
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                Some(McpToolInfo {
                    name,
                    description,
                    input_schema,
                })
            })
            .collect())
    }

    pub async fn call_tool(
        &self,
        remote_name: &str,
        arguments: Map<String, Value>,
    ) -> Result<McpToolResult, McpError> {
        let res = self
            .request(
                "tools/call",
                json!({ "name": remote_name, "arguments": Value::Object(arguments) }),
            )
            .await?;
        let is_error = res.get("isError").and_then(Value::as_bool).unwrap_or(false);
        let mut parts = Vec::new();
        if let Some(content) = res.get("content").and_then(Value::as_array) {
            for item in content {
                if let Some(t) = item.get("text").and_then(Value::as_str) {
                    parts.push(t.to_string());
                } else {
                    parts.push(item.to_string());
                }
            }
        }
        let text = if parts.is_empty() {
            res.to_string()
        } else {
            parts.join("\n")
        };
        Ok(McpToolResult { text, is_error })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        let raw = match &self.transport {
            McpTransport::Http(h) => h.send_rpc(&msg, true).await?,
            McpTransport::Stdio(s) => s.send_rpc(&msg, true).await?,
        }
        .ok_or_else(|| McpError::Protocol("Missing JSON-RPC response.".into()))?;

        if let Some(err) = raw.get("error") {
            let emsg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("MCP JSON-RPC error");
            return Err(McpError::Protocol(emsg.to_string()));
        }
        Ok(raw.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        match &self.transport {
            McpTransport::Http(h) => {
                h.send_rpc(&msg, false).await?;
            }
            McpTransport::Stdio(s) => {
                s.send_rpc(&msg, false).await?;
            }
        }
        Ok(())
    }

    pub async fn close(&self) {
        if let McpTransport::Stdio(s) = &self.transport {
            s.close().await;
        }
    }
}

struct LiveServer {
    config: McpServerConfig,
    client: Arc<McpClient>,
}

#[derive(Clone)]
struct RegisteredRemoteTool {
    definition: HarnessToolDefinition,
    server_id: String,
    remote_name: String,
}

struct RegistryInner {
    servers: HashMap<String, LiveServer>,
    configs: HashMap<String, McpServerConfig>,
    tools: Vec<RegisteredRemoteTool>,
    statuses: HashMap<String, McpServerStatus>,
    start_history: HashMap<String, VecDeque<Instant>>,
}

/// Manages live MCP connections, deterministic `mcp__<server>__<tool>` naming,
/// HTTP lost-session single renewal, and `3 starts / 10 min` restart budgeting.
pub struct McpRegistry {
    http: reqwest::Client,
    inner: Mutex<RegistryInner>,
}

impl McpRegistry {
    pub fn new(http: reqwest::Client) -> Arc<Self> {
        Arc::new(Self {
            http,
            inner: Mutex::new(RegistryInner {
                servers: HashMap::new(),
                configs: HashMap::new(),
                tools: Vec::new(),
                statuses: HashMap::new(),
                start_history: HashMap::new(),
            }),
        })
    }

    pub async fn tool_definitions(&self) -> Vec<HarnessToolDefinition> {
        let inner = self.inner.lock().await;
        inner.tools.iter().map(|t| t.definition.clone()).collect()
    }

    pub async fn statuses(&self) -> HashMap<String, McpServerStatus> {
        self.inner.lock().await.statuses.clone()
    }

    pub async fn sync(&self, configs: &[McpServerConfig]) {
        let wanted: HashMap<String, McpServerConfig> = configs
            .iter()
            .filter(|c| c.enabled && (!c.url.trim().is_empty() || !c.command.trim().is_empty()))
            .map(|c| (c.id.clone(), c.clone()))
            .collect();

        let to_close: Vec<Arc<McpClient>> = {
            let mut inner = self.inner.lock().await;
            inner.configs = wanted.clone();
            let mut stale_ids = Vec::new();
            for (id, live) in &inner.servers {
                if wanted.get(id) != Some(&live.config) {
                    stale_ids.push(id.clone());
                }
            }
            let mut closed = Vec::new();
            for id in stale_ids {
                if let Some(live) = inner.servers.remove(&id) {
                    closed.push(live.client);
                }
                inner.statuses.remove(&id);
                inner.tools.retain(|t| t.server_id != id);
            }
            closed
        };
        for c in to_close {
            c.close().await;
        }

        for cfg in wanted.values() {
            let already = self.inner.lock().await.servers.contains_key(&cfg.id);
            if !already {
                let _ = self.connect_server(cfg).await;
            }
        }
    }

    async fn connect_server(&self, cfg: &McpServerConfig) -> Result<(), String> {
        {
            let mut inner = self.inner.lock().await;
            let now = Instant::now();
            let hist = inner.start_history.entry(cfg.id.clone()).or_default();
            while hist
                .front()
                .is_some_and(|t| now.duration_since(*t) > START_WINDOW)
            {
                hist.pop_front();
            }
            if hist.len() >= MAX_STARTS_PER_WINDOW {
                let msg = "MCP server reached restart budget (3 starts in 10 minutes).".to_string();
                inner.statuses.insert(
                    cfg.id.clone(),
                    McpServerStatus {
                        state: McpServerState::Failed,
                        tool_count: 0,
                        message: msg.clone(),
                    },
                );
                return Err(msg);
            }
            hist.push_back(now);
            inner.statuses.insert(
                cfg.id.clone(),
                McpServerStatus {
                    state: McpServerState::Connecting,
                    tool_count: 0,
                    message: String::new(),
                },
            );
        }

        let client = if !cfg.command.trim().is_empty() {
            McpClient::connect_stdio(&cfg.command)
        } else {
            McpClient::connect_http(self.http.clone(), &cfg.url, &cfg.bearer_token)
        }
        .map_err(|e| e.to_string())?;
        let client = Arc::new(client);

        let connect_res = tokio::time::timeout(Duration::from_secs(30), async {
            client.initialize().await?;
            client.list_tools().await
        })
        .await;

        match connect_res {
            Ok(Ok(remote_tools)) => {
                let mut inner = self.inner.lock().await;
                let prev_names: HashMap<String, String> = inner
                    .tools
                    .iter()
                    .filter(|t| t.server_id == cfg.id)
                    .map(|t| (t.remote_name.clone(), t.definition.name.clone()))
                    .collect();
                inner.tools.retain(|t| t.server_id != cfg.id);
                let mut taken: HashSet<String> = inner
                    .tools
                    .iter()
                    .map(|t| t.definition.name.clone())
                    .collect();
                let count = remote_tools.len();
                let label = if cfg.name.trim().is_empty() {
                    &cfg.id
                } else {
                    &cfg.name
                };
                for rt in remote_tools {
                    let qname = prev_names
                        .get(&rt.name)
                        .filter(|n| !taken.contains(*n))
                        .cloned()
                        .unwrap_or_else(|| qualified_mcp_tool_name(cfg, &rt.name, &taken));
                    taken.insert(qname.clone());
                    let desc: String = format!("[From MCP server \"{label}\"] {}", rt.description)
                        .chars()
                        .take(1024)
                        .collect();
                    inner.tools.push(RegisteredRemoteTool {
                        definition: HarnessToolDefinition {
                            name: qname,
                            description: desc,
                            parameters_schema: rt.input_schema,
                            requires_write_permission: false,
                            requires_workspace_folder: false,
                            requires_shell_permission: false,
                        },
                        server_id: cfg.id.clone(),
                        remote_name: rt.name,
                    });
                }
                inner.servers.insert(
                    cfg.id.clone(),
                    LiveServer {
                        config: cfg.clone(),
                        client,
                    },
                );
                inner.statuses.insert(
                    cfg.id.clone(),
                    McpServerStatus {
                        state: McpServerState::Ready,
                        tool_count: count,
                        message: String::new(),
                    },
                );
                Ok(())
            }
            Ok(Err(e)) => {
                client.close().await;
                let msg = e.to_string();
                self.inner.lock().await.statuses.insert(
                    cfg.id.clone(),
                    McpServerStatus {
                        state: McpServerState::Failed,
                        tool_count: 0,
                        message: msg.clone(),
                    },
                );
                Err(msg)
            }
            Err(_) => {
                client.close().await;
                let msg = "Timed out connecting to MCP server.".to_string();
                self.inner.lock().await.statuses.insert(
                    cfg.id.clone(),
                    McpServerStatus {
                        state: McpServerState::Failed,
                        tool_count: 0,
                        message: msg.clone(),
                    },
                );
                Err(msg)
            }
        }
    }

    /// Invokes a registered `mcp__*` tool.
    /// - If an HTTP session returns 404 (session lost), renews the session ONCE (without resending
    ///   when a stdio server crashes — stopped servers restart under the same names, never resending).
    pub async fn call_tool(
        &self,
        qualified_name: &str,
        arguments: Map<String, Value>,
    ) -> Result<McpToolResult, String> {
        let (server_id, remote_name, client, cfg) = {
            let inner = self.inner.lock().await;
            let tool = inner
                .tools
                .iter()
                .find(|t| t.definition.name == qualified_name)
                .ok_or_else(|| format!("MCP tool '{qualified_name}' is not connected."))?;
            let live = inner
                .servers
                .get(&tool.server_id)
                .ok_or_else(|| format!("MCP server '{}' is offline.", tool.server_id))?;
            (
                tool.server_id.clone(),
                tool.remote_name.clone(),
                live.client.clone(),
                live.config.clone(),
            )
        };

        match client.call_tool(&remote_name, arguments.clone()).await {
            Ok(res) => Ok(res),
            Err(err) => {
                // HTTP lost session (404 with session id): renew session ONCE.
                if client.take_lost_session() && client.initialize().await.is_ok() {
                    return client
                        .call_tool(&remote_name, arguments)
                        .await
                        .map_err(|e| e.to_string());
                }
                // Stopped stdio server: restart under same names (within 3/10m budget), NEVER resend this call!
                if !client.is_alive() {
                    self.inner.lock().await.servers.remove(&server_id);
                    let _ = self.connect_server(&cfg).await;
                    return Err(format!(
                        "MCP server '{}' stopped during '{qualified_name}' ({err}). Restarted server; call was NOT automatically resent because its side effects are unknown.",
                        cfg.name
                    ));
                }
                Err(err.to_string())
            }
        }
    }
}

fn qualified_mcp_tool_name(
    server: &McpServerConfig,
    remote_tool: &str,
    taken: &HashSet<String>,
) -> String {
    let srv_raw = if server.name.trim().is_empty() {
        &server.id
    } else {
        &server.name
    };
    let srv: String = slug(srv_raw).chars().take(20).collect();
    let tl = slug(remote_tool);
    let base: String = format!("{MCP_TOOL_PREFIX}{srv}__{tl}")
        .chars()
        .take(MAX_MCP_TOOL_NAME_CHARS)
        .collect();
    if !taken.contains(&base) {
        return base;
    }
    for n in 2..1000 {
        let suffix = format!("_{n}");
        let prefix: String = base
            .chars()
            .take(MAX_MCP_TOOL_NAME_CHARS.saturating_sub(suffix.len()).max(1))
            .collect();
        let cand = format!("{prefix}{suffix}");
        if !taken.contains(&cand) {
            return cand;
        }
    }
    base
}

fn slug(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn sanitize_header(raw: &str) -> String {
    raw.trim().replace(['\r', '\n'], "")
}

fn split_command_args(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in cmd.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_ascii_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
