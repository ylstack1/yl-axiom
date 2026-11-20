//! Shared dispatcher for all 21 built-in tools + `mcp_schema` + `mcp__*` external tools.
//!
//! Enforces `ToolApprovalBroker` + `AgentPermissionGate` + `ToolExecutionPermit::claim` across
//! single, parallel read-only, and parallel `agent_delegate` (max 4) execution paths.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parchment_model::{GoalTrackerState, TodoItem};
use serde_json::{Map, Value};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::approval::{ToolApprovalBroker, ToolAuthorization, ToolExecutionPermit};
use crate::math_dice::{eval_math_expression, roll_dice_expression};
use crate::mcp::McpRegistry;
use crate::path_guard::WorkspacePathGuard;
use crate::permissions::{
    COMPUTER_USE_TOOL_NAME, HarnessToolDefinition, MCP_SCHEMA_TOOL_NAME, ToolAccessScope,
    is_mcp_tool_name,
};
use crate::ssrf::PublicWebTransport;

pub const MAX_SHELL_OUTPUT_HALF_BYTES: usize = 16 * 1024;
pub const MAX_PARALLEL_SUBAGENTS: usize = 4;

#[derive(Debug, Clone)]
pub struct ComputerUseAction {
    pub action: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub text: Option<String>,
    pub keys: Option<String>,
    pub amount: Option<i32>,
}

#[derive(Debug, Clone, Default)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
    /// Transient screenshot JPEG base64 sent ONLY on the immediately following model turn; never saved.
    pub screenshot_jpeg_base64: String,
    pub stopped_by_user: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            screenshot_jpeg_base64: String::new(),
            stopped_by_user: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
            screenshot_jpeg_base64: String::new(),
            stopped_by_user: false,
        }
    }
}

/// Platform backend for `computer_use` (Win32 on Windows desktop; AccessibilityService on Android).
#[async_trait]
pub trait ComputerUseBackend: Send + Sync {
    async fn execute(&self, action: &ComputerUseAction) -> Result<(String, String), String>;
}

/// Helper delegate callback supplied by `AgentTurnRunner` so only the owning run pays for helpers.
#[async_trait]
pub trait SubagentDelegate: Send + Sync {
    async fn run_helper(&self, task: &str) -> Result<String, String>;
}

pub struct HarnessContext {
    pub chat_id: String,
    pub storage_root: PathBuf,
    pub broker: Arc<ToolApprovalBroker>,
    pub web: Arc<PublicWebTransport>,
    pub mcp: Option<Arc<McpRegistry>>,
    pub computer_use: Option<Arc<dyn ComputerUseBackend>>,
    pub subagent_delegate: Option<Arc<dyn SubagentDelegate>>,
    pub goal_state: Arc<Mutex<GoalTrackerState>>,
    pub expanded_mcp_tools: Arc<Mutex<HashSet<String>>>,
    pub computer_use_settle_ms: u64,
}

pub struct HarnessExecutor;

impl HarnessExecutor {
    /// Authorizes, claims a one-use permit bound to exact raw arguments, and executes a single tool call.
    pub async fn execute_tool(
        ctx: &HarnessContext,
        call_id: &str,
        name: &str,
        arguments_json: &str,
    ) -> ToolOutput {
        let external_tools = match &ctx.mcp {
            Some(reg) => reg.tool_definitions().await,
            None => Vec::new(),
        };

        let permit = match ctx
            .broker
            .authorize(&external_tools, call_id, name, arguments_json)
            .await
        {
            ToolAuthorization::Allowed(p) => p,
            ToolAuthorization::Denied(reason) => return ToolOutput::err(reason),
            ToolAuthorization::Stopped => {
                return ToolOutput {
                    text: "Run stopped while awaiting tool approval.".into(),
                    is_error: true,
                    screenshot_jpeg_base64: String::new(),
                    stopped_by_user: true,
                };
            }
        };

        Self::execute_with_permit(ctx, &external_tools, call_id, name, arguments_json, permit).await
    }

    async fn execute_with_permit(
        ctx: &HarnessContext,
        external_tools: &[HarnessToolDefinition],
        call_id: &str,
        name: &str,
        arguments_json: &str,
        permit: ToolExecutionPermit,
    ) -> ToolOutput {
        // Stable contract: claim on `permit.scope` (exact scope + raw args + unrevoked revision, one-use).
        let claim_scope = permit.scope.clone();
        if let Err(reason) = permit.claim(&claim_scope, external_tools, name, arguments_json) {
            return ToolOutput::err(reason);
        }

        let trimmed = if arguments_json.trim().is_empty() {
            "{}"
        } else {
            arguments_json
        };
        let Ok(Value::Object(args)) = serde_json::from_str::<Value>(trimmed) else {
            return ToolOutput::err("Tool arguments must be a valid JSON object.");
        };

        let ws_roots = claim_scope.policy.all_workspace_paths();
        let ws_root = ws_roots.first().map(String::as_str).unwrap_or("");
        match name {
            "fs_list" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let recursive = args
                    .get("recursive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                match WorkspacePathGuard::list_dir_multi(&ws_roots, rel, recursive, 500) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_glob" => {
                let pat = str_arg(&args, "pattern").unwrap_or("");
                let max = int_arg(&args, "max_results", 200) as usize;
                let (root, rel_pat) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, pat) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::glob_files(&root, &rel_pat, max) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_read" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let start = int_arg(&args, "start_line", 1).max(1) as usize;
                let max_lines = int_arg(&args, "max_lines", 250).max(1) as usize;
                let (root, sub) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, rel) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::read_file_lines(&root, &sub, start, max_lines) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_search" => {
                let query = str_arg(&args, "query").unwrap_or("");
                let path = str_arg(&args, "path");
                let glob = str_arg(&args, "file_glob");
                let max = int_arg(&args, "max_results", 150) as usize;
                let (root, sub_opt) = if let Some(p) = path {
                    match WorkspacePathGuard::resolve_in_roots(&ws_roots, p) {
                        Ok((r, s)) => (r, (!s.is_empty()).then_some(s)),
                        Err(e) => return ToolOutput::err(e.to_string()),
                    }
                } else {
                    (ws_root.to_string(), None)
                };
                match WorkspacePathGuard::search_files(&root, query, sub_opt.as_deref(), glob, max)
                {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_write" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let content = str_arg(&args, "content").unwrap_or("");
                let (root, sub) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, rel) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::write_file_atomic(&root, &sub, content) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_edit" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let old_text = str_arg(&args, "old_text").unwrap_or("");
                let new_text = str_arg(&args, "new_text").unwrap_or("");
                let replace_all = args
                    .get("replace_all")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let (root, sub) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, rel) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::edit_file(
                    &root,
                    &sub,
                    &[(old_text, new_text, replace_all)],
                ) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_multi_edit" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let Some(arr) = args.get("edits").and_then(Value::as_array) else {
                    return ToolOutput::err("fs_multi_edit requires an 'edits' array.");
                };
                let mut parsed_edits: Vec<(String, String, bool)> = Vec::with_capacity(arr.len());
                for item in arr {
                    let Some(obj) = item.as_object() else {
                        return ToolOutput::err("Each edit entry must be an object.");
                    };
                    let old_t = obj
                        .get("old_text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let new_t = obj
                        .get("new_text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let all = obj
                        .get("replace_all")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    parsed_edits.push((old_t, new_t, all));
                }
                let refs: Vec<(&str, &str, bool)> = parsed_edits
                    .iter()
                    .map(|(o, n, a)| (o.as_str(), n.as_str(), *a))
                    .collect();
                let (root, sub) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, rel) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::edit_file(&root, &sub, &refs) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_move" => {
                let src = str_arg(&args, "source_path").unwrap_or("");
                let dst = str_arg(&args, "dest_path").unwrap_or("");
                let (root_s, sub_s) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, src) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                let (root_d, sub_d) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, dst) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                if root_s != root_d {
                    return ToolOutput::err(
                        "fs_move requires source and destination in the same mounted workspace folder.",
                    );
                }
                match WorkspacePathGuard::move_path(&root_s, &sub_s, &sub_d) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "fs_delete" => {
                let rel = str_arg(&args, "path").unwrap_or("");
                let (root, sub) = match WorkspacePathGuard::resolve_in_roots(&ws_roots, rel) {
                    Ok(v) => v,
                    Err(e) => return ToolOutput::err(e.to_string()),
                };
                match WorkspacePathGuard::delete_path(&root, &sub) {
                    Ok(s) => ToolOutput::ok(s),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "calc_eval" => {
                let expr = str_arg(&args, "expression").unwrap_or("");
                match eval_math_expression(expr) {
                    Ok(v) => ToolOutput::ok(v),
                    Err(e) => ToolOutput::err(e),
                }
            }
            "dice_roll" => {
                let expr = str_arg(&args, "expression").unwrap_or("");
                let reason = str_arg(&args, "reason");
                match roll_dice_expression(expr, reason) {
                    Ok(v) => ToolOutput::ok(v),
                    Err(e) => ToolOutput::err(e),
                }
            }
            "web_fetch" => {
                let url = str_arg(&args, "url").unwrap_or("");
                let max_chars = int_arg(&args, "max_chars", 24_000) as usize;
                match ctx.web.fetch_text(url, max_chars).await {
                    Ok(body) => ToolOutput::ok(body),
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "web_search" => {
                let query = str_arg(&args, "query").unwrap_or("").trim();
                let max_res = int_arg(&args, "max_results", 6).clamp(1, 20) as usize;
                if query.is_empty() {
                    return ToolOutput::err("web_search requires a non-empty query.");
                }
                let encoded = url_encode(query);
                let url = format!("https://html.duckduckgo.com/html/?q={encoded}");
                match ctx.web.fetch_text(&url, 18_000).await {
                    Ok(body) => {
                        let snippet: String = body
                            .lines()
                            .take(max_res * 6)
                            .collect::<Vec<_>>()
                            .join("\n");
                        ToolOutput::ok(snippet)
                    }
                    Err(e) => ToolOutput::err(e.to_string()),
                }
            }
            "todo_write" => execute_todo_write(ctx, &args).await,
            "goal_complete" => execute_goal_complete(ctx, &args).await,
            "agent_memory" => execute_agent_memory(ctx, &args),
            "skill_load" => execute_skill_load(ws_root, &args),
            "ask_user" => {
                let q = str_arg(&args, "question").unwrap_or("");
                let opts: Vec<String> = args
                    .get("options")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                match ctx.broker.ask_user(call_id, q, opts).await {
                    Some(answer) => ToolOutput::ok(format!("User answered: {answer}")),
                    None => ToolOutput::err(
                        "Question cancelled because the run ended or permissions changed.",
                    ),
                }
            }
            "agent_delegate" => {
                let task = str_arg(&args, "task").unwrap_or("").trim();
                if task.is_empty() {
                    return ToolOutput::err("agent_delegate requires a non-empty task.");
                }
                let Some(delegate) = &ctx.subagent_delegate else {
                    return ToolOutput::err("Subagent helper runner is not attached to this run.");
                };
                match delegate.run_helper(task).await {
                    Ok(findings) => ToolOutput::ok(findings),
                    Err(e) => ToolOutput::err(e),
                }
            }
            COMPUTER_USE_TOOL_NAME => execute_computer_use(ctx, &args).await,
            "shell_exec" => execute_shell(ws_root, &args).await,
            MCP_SCHEMA_TOOL_NAME => execute_mcp_schema(ctx, external_tools, &args).await,
            other if is_mcp_tool_name(other) => {
                let Some(reg) = &ctx.mcp else {
                    return ToolOutput::err("No MCP registry is attached.");
                };
                match reg.call_tool(other, args).await {
                    Ok(res) => ToolOutput {
                        text: res.text,
                        is_error: res.is_error,
                        screenshot_jpeg_base64: String::new(),
                        stopped_by_user: false,
                    },
                    Err(e) => ToolOutput::err(e),
                }
            }
            unknown => ToolOutput::err(format!("Unknown workspace tool '{unknown}'.")),
        }
    }

    /// Executes a batch of `agent_delegate` tasks in parallel (capped at `MAX_PARALLEL_SUBAGENTS = 4`).
    pub async fn execute_parallel_delegates(
        ctx: &HarnessContext,
        calls: &[(String, String)], // (call_id, arguments_json)
    ) -> Vec<ToolOutput> {
        let mut results = Vec::with_capacity(calls.len());
        for chunk in calls.chunks(MAX_PARALLEL_SUBAGENTS) {
            let futs = chunk
                .iter()
                .map(|(cid, args)| Self::execute_tool(ctx, cid, "agent_delegate", args));
            let batch_res = futures_util::future::join_all(futs).await;
            results.extend(batch_res);
        }
        results
    }
}

async fn execute_todo_write(ctx: &HarnessContext, args: &Map<String, Value>) -> ToolOutput {
    let Some(arr) = args.get("todos").and_then(Value::as_array) else {
        return ToolOutput::err("todo_write requires a 'todos' array.");
    };
    let mut todos = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(obj) = item.as_object() else {
            continue;
        };
        todos.push(TodoItem {
            id: obj
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            content: obj
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            status: obj
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
                .to_string(),
        });
    }
    let goal_opt = str_arg(args, "goal").map(str::to_string);
    let md = {
        let mut st = ctx.goal_state.lock().await;
        if let Some(g) = goal_opt
            && !g.trim().is_empty()
        {
            st.goal_objective = g;
        }
        st.todos = todos;
        let mut buf = String::new();
        if !st.goal_objective.is_empty() {
            buf.push_str(&format!("# Goal\n{}\n\n", st.goal_objective));
        }
        buf.push_str("## Tasks\n");
        for t in &st.todos {
            let mark = match t.status.to_ascii_lowercase().as_str() {
                "completed" => "x",
                "in_progress" => "~",
                "cancelled" => "-",
                _ => " ",
            };
            buf.push_str(&format!("- [{mark}] ({}) {}\n", t.id, t.content));
        }
        buf
    };

    // Stable contract: `todo_write` writes `chats/<id>/TODO.md`, NEVER project files.
    let safe_id: String = ctx
        .chat_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = ctx.storage_root.join("chats").join(&safe_id);
    if let Err(e) = fs::create_dir_all(&dir) {
        return ToolOutput::err(format!("Failed creating chat TODO folder: {e}"));
    }
    let todo_path = dir.join("TODO.md");
    if let Err(e) = fs::write(&todo_path, md.as_bytes()) {
        return ToolOutput::err(format!("Failed writing chat TODO.md: {e}"));
    }
    ToolOutput::ok(format!("Updated chat checklist ({})", todo_path.display()))
}

async fn execute_goal_complete(ctx: &HarnessContext, args: &Map<String, Value>) -> ToolOutput {
    let summary = str_arg(args, "summary").unwrap_or("").trim();
    if summary.is_empty() {
        return ToolOutput::err("goal_complete requires a non-empty summary.");
    }
    let verification = str_arg(args, "verification").unwrap_or("").trim();
    let mut st = ctx.goal_state.lock().await;
    st.is_completed = true;
    st.verification_summary = if verification.is_empty() {
        summary.to_string()
    } else {
        format!("{summary}\nVerification: {verification}")
    };
    ToolOutput::ok(format!("Goal marked complete: {summary}"))
}

fn execute_agent_memory(ctx: &HarnessContext, args: &Map<String, Value>) -> ToolOutput {
    // Stable contract: `agent_memory` uses global `memory/AGENT.md`, NEVER project files.
    let mem_dir = ctx.storage_root.join("memory");
    let mem_path = mem_dir.join("AGENT.md");
    let action = str_arg(args, "action")
        .unwrap_or("read")
        .to_ascii_lowercase();
    match action.as_str() {
        "read" => match fs::read_to_string(&mem_path) {
            Ok(text) => ToolOutput::ok(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                ToolOutput::ok("(No global agent memory recorded yet.)")
            }
            Err(e) => ToolOutput::err(format!("Failed reading memory/AGENT.md: {e}")),
        },
        "append" => {
            let content = str_arg(args, "content").unwrap_or("");
            if let Err(e) = fs::create_dir_all(&mem_dir) {
                return ToolOutput::err(format!("Failed creating memory dir: {e}"));
            }
            let mut existing = fs::read_to_string(&mem_path).unwrap_or_default();
            if !existing.is_empty() && !existing.ends_with('\n') {
                existing.push('\n');
            }
            existing.push_str(content);
            if !existing.ends_with('\n') {
                existing.push('\n');
            }
            match fs::write(&mem_path, existing.as_bytes()) {
                Ok(()) => ToolOutput::ok("Appended to global memory/AGENT.md."),
                Err(e) => ToolOutput::err(format!("Failed writing memory/AGENT.md: {e}")),
            }
        }
        "overwrite" => {
            let content = str_arg(args, "content").unwrap_or("");
            if let Err(e) = fs::create_dir_all(&mem_dir) {
                return ToolOutput::err(format!("Failed creating memory dir: {e}"));
            }
            match fs::write(&mem_path, content.as_bytes()) {
                Ok(()) => ToolOutput::ok("Updated global memory/AGENT.md."),
                Err(e) => ToolOutput::err(format!("Failed writing memory/AGENT.md: {e}")),
            }
        }
        other => ToolOutput::err(format!("Unknown agent_memory action '{other}'.")),
    }
}

fn execute_skill_load(ws_root: &str, args: &Map<String, Value>) -> ToolOutput {
    let name = str_arg(args, "name").unwrap_or("").trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return ToolOutput::err("Invalid skill name.");
    }
    let candidates = [
        format!("skills/{name}/SKILL.md"),
        format!(".parchment/skills/{name}/SKILL.md"),
        format!(".agents/skills/{name}/SKILL.md"),
    ];
    for rel in candidates {
        if let Ok(text) = WorkspacePathGuard::read_file_lines(ws_root, &rel, 1, 2_000) {
            return ToolOutput::ok(text);
        }
    }
    ToolOutput::err(format!("Skill '{name}' not found in workspace."))
}

async fn execute_computer_use(ctx: &HarnessContext, args: &Map<String, Value>) -> ToolOutput {
    let Some(backend) = &ctx.computer_use else {
        return ToolOutput::err("Computer control backend is unavailable on this platform.");
    };
    let action = ComputerUseAction {
        action: str_arg(args, "action").unwrap_or("").to_string(),
        x: args.get("x").and_then(Value::as_i64).map(|v| v as i32),
        y: args.get("y").and_then(Value::as_i64).map(|v| v as i32),
        text: str_arg(args, "text").map(str::to_string),
        keys: str_arg(args, "keys").map(str::to_string),
        amount: args.get("amount").and_then(Value::as_i64).map(|v| v as i32),
    };
    let settle = Duration::from_millis(ctx.computer_use_settle_ms);
    if !settle.is_zero() {
        tokio::time::sleep(settle).await;
    }
    let res = backend.execute(&action).await;
    if !settle.is_zero() {
        tokio::time::sleep(settle).await;
    }
    match res {
        Ok((summary, jpeg_b64)) => ToolOutput {
            text: summary,
            is_error: false,
            screenshot_jpeg_base64: jpeg_b64,
            stopped_by_user: false,
        },
        Err(e) => ToolOutput::err(e),
    }
}

async fn execute_mcp_schema(
    ctx: &HarnessContext,
    external_tools: &[HarnessToolDefinition],
    args: &Map<String, Value>,
) -> ToolOutput {
    let Some(names) = args.get("names").and_then(Value::as_array) else {
        return ToolOutput::err("mcp_schema requires a 'names' array.");
    };
    let queries: Vec<String> = names
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.trim().to_ascii_lowercase()))
        .filter(|s| !s.is_empty())
        .collect();
    let mut expanded = ctx.expanded_mcp_tools.lock().await;
    let mut matched = Vec::new();
    for tool in external_tools {
        let low_name = tool.name.to_ascii_lowercase();
        let low_desc = tool.description.to_ascii_lowercase();
        if queries
            .iter()
            .any(|q| low_name == *q || low_name.contains(q) || low_desc.contains(q))
        {
            expanded.insert(tool.name.clone());
            matched.push(format!(
                "### {}\n{}\nSchema: {}",
                tool.name, tool.description, tool.parameters_schema
            ));
        }
    }
    if matched.is_empty() {
        ToolOutput::err("No deferred MCP tools matched the requested names/keywords.")
    } else {
        ToolOutput::ok(matched.join("\n\n"))
    }
}

async fn execute_shell(ws_root: &str, args: &Map<String, Value>) -> ToolOutput {
    let command = str_arg(args, "command").unwrap_or("").trim();
    if command.is_empty() {
        return ToolOutput::err("shell_exec requires a non-empty command.");
    }
    let timeout_secs = int_arg(args, "timeout_seconds", 30).clamp(1, 1800) as u64;
    let cwd = match WorkspacePathGuard::root(ws_root) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e.to_string()),
    };

    let mut cmd = build_platform_shell(command, &cwd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ToolOutput::err(format!("Failed spawning shell: {e}")),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let out_task = tokio::spawn(async move { capture_head_tail(stdout).await });
    let err_task = tokio::spawn(async move { capture_head_tail(stderr).await });

    let status = match tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait()).await {
        Ok(Ok(st)) => st,
        Ok(Err(e)) => {
            let _ = child.kill().await;
            return ToolOutput::err(format!("Shell wait error: {e}"));
        }
        Err(_) => {
            let _ = child.kill().await;
            return ToolOutput::err(format!(
                "Command timed out after {timeout_secs}s and was terminated."
            ));
        }
    };

    let out_str = out_task.await.unwrap_or_default();
    let err_str = err_task.await.unwrap_or_default();
    let code = status.code().unwrap_or(-1);
    let mut combined = format!("Exit code: {code}");
    if !out_str.is_empty() {
        combined.push_str("\n--- stdout ---\n");
        combined.push_str(&out_str);
    }
    if !err_str.is_empty() {
        combined.push_str("\n--- stderr ---\n");
        combined.push_str(&err_str);
    }
    ToolOutput {
        text: combined,
        is_error: !status.success(),
        screenshot_jpeg_base64: String::new(),
        stopped_by_user: false,
    }
}

fn build_platform_shell(command: &str, cwd: &Path) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command])
            .current_dir(cwd);
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]).current_dir(cwd);
        c
    }
}

/// Bounded head + tail stream capture so huge command output never blows up RAM.
async fn capture_head_tail<R: AsyncReadExt + Unpin>(reader: Option<R>) -> String {
    let Some(mut r) = reader else {
        return String::new();
    };
    let mut head = Vec::<u8>::with_capacity(4096);
    let mut tail = Vec::<u8>::new();
    let mut total_bytes = 0usize;
    let mut buf = [0u8; 4096];

    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        total_bytes += n;
        let slice = &buf[..n];
        if head.len() < MAX_SHELL_OUTPUT_HALF_BYTES {
            let take = (MAX_SHELL_OUTPUT_HALF_BYTES - head.len()).min(slice.len());
            head.extend_from_slice(&slice[..take]);
            let rem = &slice[take..];
            if !rem.is_empty() {
                push_tail(&mut tail, rem);
            }
        } else {
            push_tail(&mut tail, slice);
        }
    }

    if total_bytes <= MAX_SHELL_OUTPUT_HALF_BYTES * 2 {
        head.extend_from_slice(&tail);
        return String::from_utf8_lossy_owned(head);
    }
    let omitted = total_bytes.saturating_sub(head.len() + tail.len());
    format!(
        "{}\n... [{} bytes omitted] ...\n{}",
        String::from_utf8_lossy(&head),
        omitted,
        String::from_utf8_lossy(&tail)
    )
}

fn push_tail(tail: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() >= MAX_SHELL_OUTPUT_HALF_BYTES {
        tail.clear();
        tail.extend_from_slice(&bytes[bytes.len() - MAX_SHELL_OUTPUT_HALF_BYTES..]);
        return;
    }
    tail.extend_from_slice(bytes);
    if tail.len() > MAX_SHELL_OUTPUT_HALF_BYTES {
        let excess = tail.len() - MAX_SHELL_OUTPUT_HALF_BYTES;
        tail.drain(..excess);
    }
}

fn str_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn int_arg(args: &Map<String, Value>, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn url_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[allow(dead_code)]
fn _assert_scope_used(_: &ToolAccessScope) {}
