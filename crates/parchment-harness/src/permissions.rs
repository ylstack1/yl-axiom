//! Built-in 21-tool catalog, deferred `mcp_schema` support, and `AgentPermissionGate`.
//!
//! The exact same catalog and permission policy govern tool advertisement AND tool dispatch,
//! including indirect writes (`agent_memory` write vs read, `computer_use`, `shell_exec`, `web_*`).

use std::collections::HashSet;

use parchment_model::{AgentPermissionPolicy, ToolApprovalMode, WorkspaceMode};
use parchment_protocol::ToolDefinition;
use serde_json::{Value, json};

pub const MCP_TOOL_PREFIX: &str = "mcp__";
pub const MCP_SCHEMA_TOOL_NAME: &str = "mcp_schema";
pub const COMPUTER_USE_TOOL_NAME: &str = "computer_use";
pub const DEFERRED_MCP_SCHEMA_THRESHOLD: usize = 6;

pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_TOOL_PREFIX)
}

#[derive(Debug, Clone)]
pub struct HarnessToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters_schema: Value,
    pub requires_write_permission: bool,
    pub requires_workspace_folder: bool,
    pub requires_shell_permission: bool,
}

impl HarnessToolDefinition {
    pub fn to_wire_tool(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters_schema: self.parameters_schema.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolAccessScope {
    pub policy: AgentPermissionPolicy,
    pub goal_mode_active: bool,
    pub include_local_web_search_tool: bool,
    pub expanded_mcp_tools: Option<HashSet<String>>,
}

impl ToolAccessScope {
    pub fn new(policy: AgentPermissionPolicy) -> Self {
        let web = policy.web_access;
        Self {
            policy,
            goal_mode_active: false,
            include_local_web_search_tool: web,
            expanded_mcp_tools: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolEffect {
    pub requires_approval: bool,
    pub changes_files: bool,
}

pub struct ToolCatalog;

impl ToolCatalog {
    pub fn mcp_schema_tool() -> HarnessToolDefinition {
        HarnessToolDefinition {
            name: MCP_SCHEMA_TOOL_NAME.into(),
            description: "Load full JSON parameter schemas and enable deferred MCP tools listed under Deferred MCP Tools.".into(),
            parameters_schema: json!({
                "type": "object",
                "properties": {
                    "names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Exact mcp__... tool names or search keywords to inspect and enable."
                    }
                },
                "required": ["names"]
            }),
            requires_write_permission: false,
            requires_workspace_folder: false,
            requires_shell_permission: false,
        }
    }

    pub fn builtin_tools() -> Vec<HarnessToolDefinition> {
        vec![
            tool(
                "fs_list",
                "List directory entries in workspace.",
                false,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative directory path ('' for root)." },
                        "recursive": { "type": "boolean", "description": "List subdirectories recursively." }
                    }
                }),
            ),
            tool(
                "fs_glob",
                "Find workspace paths matching a glob (e.g. '**/*.rs', 'docs/*').",
                false,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Glob pattern." },
                        "max_results": { "type": "integer", "description": "Max results (default 200)." }
                    },
                    "required": ["pattern"]
                }),
            ),
            tool(
                "fs_read",
                "Read UTF-8 file lines in workspace with line numbers.",
                false,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative file path." },
                        "start_line": { "type": "integer", "description": "1-based start line (default 1)." },
                        "max_lines": { "type": "integer", "description": "Max lines to read (default 250)." }
                    },
                    "required": ["path"]
                }),
            ),
            tool(
                "fs_search",
                "Search text or regex across workspace files.",
                false,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Literal text or regex pattern." },
                        "path": { "type": "string", "description": "Optional relative directory or single file." },
                        "file_glob": { "type": "string", "description": "Optional filename/extension filter." },
                        "max_results": { "type": "integer", "description": "Max matches (default 150)." }
                    },
                    "required": ["query"]
                }),
            ),
            tool(
                "fs_write",
                "Create or overwrite a UTF-8 file in workspace.",
                true,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Relative file path." },
                        "content": { "type": "string", "description": "Full file text." }
                    },
                    "required": ["path", "content"]
                }),
            ),
            tool(
                "fs_edit",
                "Surgical search-and-replace on an existing workspace file.",
                true,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_text": { "type": "string" },
                        "new_text": { "type": "string" },
                        "replace_all": { "type": "boolean" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }),
            ),
            tool(
                "fs_multi_edit",
                "Apply multiple search-and-replace hunks to one file atomically.",
                true,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "edits": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_text": { "type": "string" },
                                    "new_text": { "type": "string" },
                                    "replace_all": { "type": "boolean" }
                                },
                                "required": ["old_text", "new_text"]
                            }
                        }
                    },
                    "required": ["path", "edits"]
                }),
            ),
            tool(
                "fs_move",
                "Move or rename a file or directory in workspace.",
                true,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "source_path": { "type": "string" },
                        "dest_path": { "type": "string" }
                    },
                    "required": ["source_path", "dest_path"]
                }),
            ),
            tool(
                "fs_delete",
                "Delete a file or directory in workspace.",
                true,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }),
            ),
            tool(
                "web_search",
                "Search the web for current docs and facts.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "max_results": { "type": "integer" }
                    },
                    "required": ["query"]
                }),
            ),
            tool(
                "web_fetch",
                "Fetch an HTTP/HTTPS URL as clean Markdown/text or JSON.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" },
                        "max_chars": { "type": "integer" }
                    },
                    "required": ["url"]
                }),
            ),
            tool(
                "calc_eval",
                "Evaluate a math/scientific expression.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" }
                    },
                    "required": ["expression"]
                }),
            ),
            tool(
                "dice_roll",
                "Roll unbiased dice (e.g. '1d20', '2d6+3', '4d6kh3').",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" },
                        "reason": { "type": "string" }
                    },
                    "required": ["expression"]
                }),
            ),
            tool(
                "todo_write",
                "Update chat task checklist (never project files).",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string" },
                        "todos": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "content": { "type": "string" },
                                    "status": { "type": "string" }
                                },
                                "required": ["id", "content", "status"]
                            }
                        }
                    },
                    "required": ["todos"]
                }),
            ),
            tool(
                COMPUTER_USE_TOOL_NAME,
                "Control this device (each action requires approval).",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["screenshot", "click", "double_click", "right_click", "move", "type", "key", "scroll"] },
                        "x": { "type": "integer" },
                        "y": { "type": "integer" },
                        "text": { "type": "string" },
                        "keys": { "type": "string" },
                        "amount": { "type": "integer" }
                    },
                    "required": ["action"]
                }),
            ),
            tool(
                "agent_delegate",
                "Delegate a self-contained read/search task to an isolated helper.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "task": { "type": "string" }
                    },
                    "required": ["task"]
                }),
            ),
            tool(
                "skill_load",
                "Load full instructions of a skill listed under Available skills.",
                false,
                true,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }),
            ),
            tool(
                "ask_user",
                "Pause to ask the user a genuine decision or missing input.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "options": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["question"]
                }),
            ),
            tool(
                "goal_complete",
                "Mark the active goal complete once all acceptance criteria are verified.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string" },
                        "verification": { "type": "string" }
                    },
                    "required": ["summary"]
                }),
            ),
            tool(
                "agent_memory",
                "Read or update global memory/AGENT.md across sessions.",
                false,
                false,
                false,
                json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            ),
            tool(
                "shell_exec",
                "Run a shell command in workspace and return stdout/stderr.",
                true,
                true,
                true,
                json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" },
                        "timeout_seconds": { "type": "integer" }
                    },
                    "required": ["command"]
                }),
            ),
        ]
    }

    pub fn is_parallel_safe_read_only(name: &str) -> bool {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "fs_read"
                | "fs_list"
                | "fs_glob"
                | "fs_search"
                | "skill_load"
                | MCP_SCHEMA_TOOL_NAME
                | "web_search"
                | "web_fetch"
                | "calc_eval"
                | "dice_roll"
        )
    }

    pub fn should_defer_mcp_schemas(
        policy: &AgentPermissionPolicy,
        external_tools: &[HarnessToolDefinition],
    ) -> bool {
        if !policy.tools_enabled {
            return false;
        }
        external_tools
            .iter()
            .filter(|t| policy.allows_tool(&t.name))
            .count()
            > DEFERRED_MCP_SCHEMA_THRESHOLD
    }

    pub fn active_tools(
        scope: &ToolAccessScope,
        external_tools: &[HarnessToolDefinition],
    ) -> Vec<HarnessToolDefinition> {
        let policy = &scope.policy;
        let has_ws = policy.has_workspace();
        if !has_ws
            && !scope.goal_mode_active
            && !policy.web_access
            && !scope.include_local_web_search_tool
            && external_tools.is_empty()
            && !policy.computer_access
        {
            return Vec::new();
        }
        let defer_mcp = scope.expanded_mcp_tools.is_some()
            && Self::should_defer_mcp_schemas(policy, external_tools);

        let mut all = Self::builtin_tools();
        all.extend_from_slice(external_tools);

        let mut active: Vec<HarnessToolDefinition> = all
            .into_iter()
            .filter(|t| {
                if !policy.allows_tool(&t.name) {
                    return false;
                }
                if is_mcp_tool_name(&t.name)
                    && defer_mcp
                    && !scope
                        .expanded_mcp_tools
                        .as_ref()
                        .is_some_and(|set| set.contains(&t.name))
                {
                    return false;
                }
                if t.name == COMPUTER_USE_TOOL_NAME && !policy.computer_access {
                    return false;
                }
                if matches!(t.name.as_str(), "web_search" | "web_fetch") && !policy.web_access {
                    return false;
                }
                if t.name == "web_search" && !scope.include_local_web_search_tool {
                    return false;
                }
                if !has_ws {
                    !t.requires_workspace_folder
                } else {
                    let write_ok = policy.workspace_mode == WorkspaceMode::ReadWrite
                        || !t.requires_write_permission;
                    let shell_ok = !t.requires_shell_permission
                        || (policy.workspace_mode == WorkspaceMode::ReadWrite
                            && policy.allow_shell());
                    write_ok && shell_ok
                }
            })
            .collect();

        if defer_mcp && policy.allows_tool(MCP_SCHEMA_TOOL_NAME) {
            active.push(Self::mcp_schema_tool());
        }
        active
    }
}

fn tool(
    name: &str,
    description: &str,
    requires_write_permission: bool,
    requires_workspace_folder: bool,
    requires_shell_permission: bool,
    parameters_schema: Value,
) -> HarnessToolDefinition {
    HarnessToolDefinition {
        name: name.into(),
        description: description.into(),
        parameters_schema,
        requires_write_permission,
        requires_workspace_folder,
        requires_shell_permission,
    }
}

pub struct AgentPermissionGate;

impl AgentPermissionGate {
    /// Checks whether a tool call is allowed under `scope`. Returns `Some(reason)` if denied.
    pub fn denial(
        scope: &ToolAccessScope,
        external_tools: &[HarnessToolDefinition],
        name: &str,
        arguments_json: &str,
    ) -> Option<String> {
        let policy = &scope.policy;
        let def = if name == MCP_SCHEMA_TOOL_NAME {
            Some(ToolCatalog::mcp_schema_tool())
        } else {
            ToolCatalog::builtin_tools()
                .into_iter()
                .chain(external_tools.iter().cloned())
                .find(|t| t.name == name)
        }?;

        if !policy.allows_tool(name) {
            return Some(format!("Tool '{name}' is disabled by the current policy."));
        }
        if def.requires_workspace_folder && !policy.has_workspace() {
            return Some(
                "Workspace harness is disabled or no workspace directory is mounted.".into(),
            );
        }
        if def.requires_shell_permission && !policy.allow_shell() {
            return Some(
                "Shell execution is disabled. Enable full process access explicitly before running commands."
                    .into(),
            );
        }
        if def.requires_write_permission && policy.workspace_mode != WorkspaceMode::ReadWrite {
            return Some("Workspace is in Read-Only mode.".into());
        }

        let trimmed_args = if arguments_json.trim().is_empty() {
            "{}"
        } else {
            arguments_json
        };
        let parsed = serde_json::from_str::<Value>(trimmed_args).ok();
        let Some(obj) = parsed.as_ref().and_then(Value::as_object) else {
            return Some("Tool arguments must be a valid JSON object.".into());
        };

        if name == "agent_memory" {
            let action = obj
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("read")
                .to_ascii_lowercase();
            if !matches!(action.as_str(), "read" | "append" | "overwrite") {
                return Some(format!("Unknown agent_memory action '{action}'."));
            }
            if action != "read" && policy.workspace_mode == WorkspaceMode::ReadOnly {
                return Some("Workspace is in Read-Only mode. Memory writes are disabled.".into());
            }
        }
        if matches!(name, "web_search" | "web_fetch") && !policy.web_access {
            return Some("Web access is disabled by the current policy.".into());
        }
        if name == COMPUTER_USE_TOOL_NAME && !policy.computer_access {
            return Some("Computer control is turned off for this chat.".into());
        }
        if name == MCP_SCHEMA_TOOL_NAME {
            return if ToolCatalog::should_defer_mcp_schemas(policy, external_tools) {
                None
            } else {
                Some(format!("Tool '{name}' is not available in this run."))
            };
        }

        let active = ToolCatalog::active_tools(scope, external_tools);
        if !active.iter().any(|t| t.name == name) {
            if is_mcp_tool_name(name)
                && scope.expanded_mcp_tools.is_some()
                && ToolCatalog::should_defer_mcp_schemas(policy, external_tools)
            {
                return Some(format!(
                    "MCP tool '{name}' schema is deferred. Call {MCP_SCHEMA_TOOL_NAME}(names=[\"{name}\"]) first."
                ));
            }
            return Some(format!("Tool '{name}' is not available in this run."));
        }
        None
    }

    pub fn denial_or_unknown(
        scope: &ToolAccessScope,
        external_tools: &[HarnessToolDefinition],
        name: &str,
        arguments_json: &str,
    ) -> Option<String> {
        let known = name == MCP_SCHEMA_TOOL_NAME
            || ToolCatalog::builtin_tools().iter().any(|t| t.name == name)
            || external_tools.iter().any(|t| t.name == name);
        if !known {
            return Some(format!("Unknown workspace tool '{name}'."));
        }
        Self::denial(scope, external_tools, name, arguments_json)
    }

    pub fn effect(scope: &ToolAccessScope, name: &str, arguments_json: &str) -> ToolEffect {
        let action = serde_json::from_str::<Value>(arguments_json)
            .ok()
            .and_then(|v| v.get("action").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| "read".to_string());
        let changes_files = matches!(
            name,
            "fs_write" | "fs_edit" | "fs_multi_edit" | "fs_move" | "fs_delete" | "shell_exec"
        ) || (name == "agent_memory" && !action.eq_ignore_ascii_case("read"));
        let changes_state = matches!(name, "todo_write" | "goal_complete");
        let uses_web = matches!(name, "web_search" | "web_fetch");
        let external = is_mcp_tool_name(name);

        let requires_approval = name == COMPUTER_USE_TOOL_NAME
            || (name != "ask_user"
                && match scope.policy.approval_mode {
                    ToolApprovalMode::AskEveryTool => true,
                    ToolApprovalMode::AskForChanges => {
                        changes_files || changes_state || uses_web || external
                    }
                    ToolApprovalMode::Never => false,
                });
        ToolEffect {
            requires_approval,
            changes_files,
        }
    }
}
