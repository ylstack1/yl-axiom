use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::chat::{ChatSession, WorkspaceMode};
use crate::provider::ProviderProfile;
use crate::settings::AssistantAgent;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolApprovalMode {
    #[default]
    #[serde(rename = "ASK_FOR_CHANGES")]
    AskForChanges,
    #[serde(rename = "ASK_EVERY_TOOL")]
    AskEveryTool,
    #[serde(rename = "NEVER")]
    Never,
}

/// Shell commands are not confined by a working directory or by file-tool permissions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellAccess {
    #[default]
    #[serde(rename = "DISABLED")]
    Disabled,
    #[serde(rename = "FULL_ACCESS")]
    FullAccess,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPermissionPolicy {
    #[serde(default)]
    pub approval_mode: ToolApprovalMode,
    #[serde(default)]
    pub workspace_path: String,
    #[serde(default)]
    pub extra_workspace_paths: Vec<String>,
    #[serde(default)]
    pub workspace_mode: WorkspaceMode,
    #[serde(default = "default_true")]
    pub tools_enabled: bool,
    #[serde(default)]
    pub tool_overrides: BTreeMap<String, bool>,
    #[serde(default)]
    pub shell_access: ShellAccess,
    #[serde(default)]
    pub web_access: bool,
    /// Lets the agent see the screen and control this device, one approved action at a time.
    #[serde(default)]
    pub computer_access: bool,
}

impl Default for AgentPermissionPolicy {
    fn default() -> Self {
        Self {
            approval_mode: ToolApprovalMode::AskForChanges,
            workspace_path: String::new(),
            extra_workspace_paths: Vec::new(),
            workspace_mode: WorkspaceMode::Off,
            tools_enabled: true,
            tool_overrides: BTreeMap::new(),
            shell_access: ShellAccess::Disabled,
            web_access: false,
            computer_access: false,
        }
    }
}

impl AgentPermissionPolicy {
    pub fn has_workspace(&self) -> bool {
        self.workspace_mode != WorkspaceMode::Off
            && (!self.workspace_path.trim().is_empty()
                || self
                    .extra_workspace_paths
                    .iter()
                    .any(|p| !p.trim().is_empty()))
    }

    /// Returns all unique non-empty mounted workspace folders (primary first, then extras).
    pub fn all_workspace_paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        let primary = self.workspace_path.trim();
        if !primary.is_empty() {
            out.push(primary.to_string());
        }
        for extra in &self.extra_workspace_paths {
            let t = extra.trim();
            if !t.is_empty() && !out.iter().any(|p| p.eq_ignore_ascii_case(t)) {
                out.push(t.to_string());
            }
        }
        out
    }

    pub fn add_workspace_path(&mut self, path: String) {
        let t = path.trim();
        if t.is_empty() {
            return;
        }
        if self.workspace_path.trim().is_empty() {
            self.workspace_path = t.to_string();
        } else if !self.workspace_path.trim().eq_ignore_ascii_case(t)
            && !self
                .extra_workspace_paths
                .iter()
                .any(|p| p.trim().eq_ignore_ascii_case(t))
        {
            self.extra_workspace_paths.push(t.to_string());
        }
        if self.workspace_mode == WorkspaceMode::Off {
            self.workspace_mode = WorkspaceMode::ReadOnly;
        }
    }

    pub fn remove_workspace_path(&mut self, target: &str) {
        let t = target.trim();
        if self.workspace_path.trim().eq_ignore_ascii_case(t) {
            self.workspace_path = if !self.extra_workspace_paths.is_empty() {
                self.extra_workspace_paths.remove(0)
            } else {
                String::new()
            };
        } else {
            self.extra_workspace_paths
                .retain(|p| !p.trim().eq_ignore_ascii_case(t));
        }
        if self.all_workspace_paths().is_empty() {
            self.workspace_mode = WorkspaceMode::Off;
            self.shell_access = ShellAccess::Disabled;
        }
    }

    pub fn clear_workspace_paths(&mut self) {
        self.workspace_path.clear();
        self.extra_workspace_paths.clear();
        self.workspace_mode = WorkspaceMode::Off;
        self.shell_access = ShellAccess::Disabled;
    }

    pub fn allow_shell(&self) -> bool {
        self.shell_access == ShellAccess::FullAccess
    }

    pub fn allows_tool(&self, name: &str) -> bool {
        self.tools_enabled && self.tool_overrides.get(name).copied() != Some(false)
    }

    pub fn as_overrides(&self) -> AgentPermissionOverrides {
        AgentPermissionOverrides {
            approval_mode: Some(self.approval_mode),
            workspace_path: Some(self.workspace_path.clone()),
            extra_workspace_paths: Some(self.extra_workspace_paths.clone()),
            workspace_mode: Some(self.workspace_mode),
            tools_enabled: Some(self.tools_enabled),
            tool_overrides: Some(self.tool_overrides.clone()),
            shell_access: Some(self.shell_access),
            web_access: Some(self.web_access),
            computer_access: Some(self.computer_access),
        }
    }
}

/// `None` inherits; explicit `Off`, empty path, or `false` revokes the inherited setting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentPermissionOverrides {
    pub approval_mode: Option<ToolApprovalMode>,
    pub workspace_path: Option<String>,
    pub extra_workspace_paths: Option<Vec<String>>,
    pub workspace_mode: Option<WorkspaceMode>,
    pub tools_enabled: Option<bool>,
    pub tool_overrides: Option<BTreeMap<String, bool>>,
    pub shell_access: Option<ShellAccess>,
    pub web_access: Option<bool>,
    pub computer_access: Option<bool>,
}

impl AgentPermissionOverrides {
    pub fn apply_to(&self, defaults: &AgentPermissionPolicy) -> AgentPermissionPolicy {
        let mut tool_overrides = defaults.tool_overrides.clone();
        if let Some(extra) = &self.tool_overrides {
            for (k, &v) in extra {
                tool_overrides.insert(k.clone(), v);
            }
        }
        AgentPermissionPolicy {
            approval_mode: self.approval_mode.unwrap_or(defaults.approval_mode),
            workspace_path: self
                .workspace_path
                .clone()
                .unwrap_or_else(|| defaults.workspace_path.clone()),
            extra_workspace_paths: self
                .extra_workspace_paths
                .clone()
                .unwrap_or_else(|| defaults.extra_workspace_paths.clone()),
            workspace_mode: self.workspace_mode.unwrap_or(defaults.workspace_mode),
            tools_enabled: self.tools_enabled.unwrap_or(defaults.tools_enabled),
            tool_overrides,
            shell_access: self.shell_access.unwrap_or(defaults.shell_access),
            web_access: self.web_access.unwrap_or(defaults.web_access),
            computer_access: self.computer_access.unwrap_or(defaults.computer_access),
        }
    }
}

pub struct AgentPermissionResolver;

impl AgentPermissionResolver {
    pub fn defaults(assistant: Option<&AssistantAgent>) -> AgentPermissionPolicy {
        if let Some(policy) = assistant.and_then(|a| a.permission_defaults.as_ref()) {
            return policy.clone();
        }
        AgentPermissionPolicy {
            workspace_path: assistant
                .map(|a| a.workspace_path.clone())
                .unwrap_or_default(),
            workspace_mode: assistant
                .map(|a| a.workspace_mode)
                .unwrap_or(WorkspaceMode::Off),
            web_access: assistant.is_some_and(|a| a.enable_web_search),
            ..AgentPermissionPolicy::default()
        }
    }

    /// Old provider shell flags never constitute explicit consent to unrestricted shell access.
    pub fn legacy_overrides(
        session: &ChatSession,
        profile: Option<&ProviderProfile>,
    ) -> AgentPermissionOverrides {
        let has_legacy_ws = !session.workspace_path.trim().is_empty()
            || session.workspace_mode != WorkspaceMode::Off;
        AgentPermissionOverrides {
            workspace_path: has_legacy_ws.then(|| session.workspace_path.clone()),
            workspace_mode: has_legacy_ws.then_some(session.workspace_mode),
            web_access: profile
                .is_some_and(|p| p.enable_web_search || p.enable_url_context)
                .then_some(true),
            ..AgentPermissionOverrides::default()
        }
    }

    pub fn resolve(
        session: &ChatSession,
        assistant: Option<&AssistantAgent>,
        profile: Option<&ProviderProfile>,
    ) -> AgentPermissionPolicy {
        let base = Self::defaults(assistant);
        match &session.permission_overrides {
            Some(ov) => ov.apply_to(&base),
            None => Self::legacy_overrides(session, profile).apply_to(&base),
        }
    }
}
