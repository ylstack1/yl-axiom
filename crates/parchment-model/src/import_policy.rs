use std::collections::BTreeMap;

use crate::chat::{ChatSession, WorkspaceMode};
use crate::permissions::{
    AgentPermissionOverrides, AgentPermissionPolicy, ShellAccess, ToolApprovalMode,
};
use crate::settings::{AppStateSnapshot, AssistantAgent, ThemeConfig};

/// True only for links a browser should open: never a local path or another scheme.
pub fn is_web_link(target: &str) -> bool {
    let trimmed = target.trim();
    trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || trimmed
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

pub fn sanitize_media_segment(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Imported chats are untrusted, so media references may only name a portable file inside app storage:
/// `chats/<id>/media/<name>` or `media/<name>`. Absolute paths, UNC paths, and `..` escapes are dropped.
pub fn portable_imported_media_path(raw_path: &str) -> String {
    let normalized = raw_path.trim().replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() || segments.iter().any(|&s| s == "." || s == "..") {
        return String::new();
    }
    let last_chats = segments.iter().rposition(|&s| s == "chats");
    let portable: Vec<&str> = match last_chats {
        Some(idx) if idx + 4 == segments.len() && segments[idx + 2] == "media" => {
            vec!["chats", segments[idx + 1], "media", segments[idx + 3]]
        }
        _ if segments.len() >= 2 && segments[segments.len() - 2] == "media" => {
            vec!["media", segments[segments.len() - 1]]
        }
        _ => return String::new(),
    };
    portable
        .into_iter()
        .map(sanitize_media_segment)
        .collect::<Vec<_>>()
        .join("/")
}

impl ThemeConfig {
    /// Windows contacts network paths (`\\server\share`) as soon as they are probed, so imports drop them.
    pub fn without_network_font_paths(&self) -> Self {
        let keep_local = |path: &str| -> String {
            let norm = path.trim().replace('\\', "/");
            if norm.starts_with("//") {
                String::new()
            } else {
                path.to_string()
            }
        };
        let mut out = self.clone();
        out.custom_prose_font_path = keep_local(&self.custom_prose_font_path);
        out.custom_code_font_path = keep_local(&self.custom_code_font_path);
        out
    }
}

impl ChatSession {
    pub fn with_portable_imported_media(&self) -> Self {
        let mut out = self.clone();
        for node in &mut out.messages {
            for variant in &mut node.variants {
                for att in &mut variant.attachments {
                    att.local_file_path = portable_imported_media_path(&att.local_file_path);
                }
                for media in &mut variant.media_items {
                    media.local_file_path = portable_imported_media_path(&media.local_file_path);
                    if !is_web_link(&media.remote_url) {
                        media.remote_url.clear();
                    }
                }
            }
        }
        out
    }

    /// Imports carry content and configuration, never local execution authority or machine paths.
    pub fn with_imported_permissions(&self) -> Self {
        let mut out = self.with_portable_imported_media();
        out.workspace_path.clear();
        out.workspace_mode = WorkspaceMode::Off;
        out.permission_overrides = Some(AgentPermissionOverrides {
            approval_mode: Some(ToolApprovalMode::AskEveryTool),
            workspace_path: Some(String::new()),
            extra_workspace_paths: Some(Vec::new()),
            workspace_mode: Some(WorkspaceMode::Off),
            tools_enabled: Some(false),
            tool_overrides: Some(BTreeMap::new()),
            shell_access: Some(ShellAccess::Disabled),
            web_access: Some(false),
            computer_access: Some(false),
        });
        out
    }
}

impl AssistantAgent {
    pub fn with_imported_permissions(&self) -> Self {
        let mut out = self.clone();
        out.workspace_path.clear();
        out.workspace_mode = WorkspaceMode::Off;
        out.enable_web_search = false;
        out.permission_defaults = Some(AgentPermissionPolicy {
            approval_mode: ToolApprovalMode::AskEveryTool,
            tools_enabled: false,
            ..AgentPermissionPolicy::default()
        });
        out
    }
}

impl AppStateSnapshot {
    /// Sanitizes an untrusted imported settings/state snapshot, preserving the local OTA repo.
    pub fn with_imported_permissions(&self, current_update_repo: &str) -> Self {
        let mut out = self.clone();
        out.ota_github_repo = current_update_repo.to_string();
        out.preferences.profile_picture_path.clear();
        out.preferences.mcp_servers = self
            .preferences
            .mcp_servers
            .iter()
            .filter(|s| s.command.trim().is_empty())
            .map(|s| {
                let mut cloned = s.clone();
                cloned.enabled = false;
                cloned
            })
            .collect();
        out.assistants = self
            .assistants
            .iter()
            .map(AssistantAgent::with_imported_permissions)
            .collect();
        out.sessions = self
            .sessions
            .iter()
            .map(ChatSession::with_imported_permissions)
            .collect();
        for profile in &mut out.profiles {
            profile.allow_shell_execution = false;
        }
        out.theme = self.theme.without_network_font_paths();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        GeneratedMediaItem, MediaKind, MessageNode, MessageRole, MessageVariant, PromptAttachment,
    };
    use crate::permissions::AgentPermissionResolver;
    use crate::provider::{ProviderProfile, ProviderProtocol};
    use crate::settings::McpServerConfig;

    #[test]
    fn portable_media_path_blocks_traversal_and_absolute_paths() {
        assert_eq!(
            portable_imported_media_path("chats/abc-1/media/photo.png"),
            "chats/abc-1/media/photo.png"
        );
        assert_eq!(
            portable_imported_media_path("C:\\Users\\x\\chats\\id1\\media\\a b.png"),
            "chats/id1/media/a_b.png"
        );
        assert_eq!(
            portable_imported_media_path("/tmp/media/pic.jpg"),
            "media/pic.jpg"
        );
        assert_eq!(
            portable_imported_media_path("chats/../etc/media/passwd"),
            ""
        );
        assert_eq!(portable_imported_media_path("C:\\Windows\\system.ini"), "");
        assert_eq!(portable_imported_media_path("\\\\evil\\share\\a.png"), "");
    }

    #[test]
    fn imported_permissions_strip_local_authority_and_network_fonts() {
        let mut snap = AppStateSnapshot {
            ota_github_repo: "attacker/evil".into(),
            ..AppStateSnapshot::default()
        };
        snap.preferences.profile_picture_path = "C:\\secret.png".into();
        snap.preferences.mcp_servers = vec![
            McpServerConfig {
                id: "local".into(),
                name: "Local".into(),
                command: "cmd /c calc".into(),
                enabled: true,
                ..Default::default()
            },
            McpServerConfig {
                id: "http".into(),
                name: "Remote".into(),
                url: "https://example.com/mcp".into(),
                enabled: true,
                ..Default::default()
            },
        ];
        snap.theme.custom_prose_font_path = "\\\\evil-host\\share\\font.ttf".into();
        snap.theme.custom_code_font_path = "C:\\Fonts\\Mono.ttf".into();
        snap.sessions.push(ChatSession {
            id: "c1".into(),
            title: "Imported".into(),
            created_at: 1,
            updated_at: 2,
            profile_id: "p1".into(),
            model_override: String::new(),
            assistant_id: String::new(),
            is_pinned: false,
            is_title_manually_set: false,
            draft_input: String::new(),
            draft_prefill: String::new(),
            user_name: "User".into(),
            system_prompt: String::new(),
            active_card: None,
            story_summary: String::new(),
            fragments: vec![],
            authors_note: String::new(),
            authors_note_depth: 2,
            view_mode: Default::default(),
            workspace_path: "C:\\Users\\victim".into(),
            workspace_mode: WorkspaceMode::ReadWrite,
            permission_overrides: None,
            utility_usage_records: vec![],
            messages: vec![MessageNode {
                id: "m1".into(),
                role: MessageRole::Assistant,
                variants: vec![MessageVariant {
                    id: "v1".into(),
                    content: "hi".into(),
                    attachments: vec![PromptAttachment {
                        id: "a1".into(),
                        file_name: "f.txt".into(),
                        mime_type: "text/plain".into(),
                        base64_data: String::new(),
                        text_content: String::new(),
                        local_file_path: "C:\\Windows\\win.ini".into(),
                    }],
                    media_items: vec![GeneratedMediaItem {
                        id: "g1".into(),
                        kind: MediaKind::Image,
                        mime_type: "image/png".into(),
                        local_file_path: "chats/c1/media/img.png".into(),
                        remote_url: "file:///etc/passwd".into(),
                        revised_prompt: String::new(),
                        aspect_ratio: String::new(),
                    }],
                    ..Default::default()
                }],
                active_variant_index: 0,
                included_in_context: true,
                pinned: false,
            }],
        });

        let clean = snap.with_imported_permissions("baros4294/parchment");
        assert_eq!(clean.ota_github_repo, "baros4294/parchment");
        assert!(clean.preferences.profile_picture_path.is_empty());
        assert_eq!(clean.preferences.mcp_servers.len(), 1);
        assert_eq!(clean.preferences.mcp_servers[0].id, "http");
        assert!(!clean.preferences.mcp_servers[0].enabled);
        assert!(clean.theme.custom_prose_font_path.is_empty());
        assert_eq!(clean.theme.custom_code_font_path, "C:\\Fonts\\Mono.ttf");

        let chat = &clean.sessions[0];
        let resolved = AgentPermissionResolver::resolve(chat, None, None);
        assert!(!resolved.tools_enabled);
        assert!(!resolved.has_workspace());
        assert!(!resolved.allow_shell());
        assert!(!resolved.web_access);
        assert!(!resolved.computer_access);
        assert!(
            chat.messages[0].variants[0].attachments[0]
                .local_file_path
                .is_empty()
        );
        assert_eq!(
            chat.messages[0].variants[0].media_items[0].local_file_path,
            "chats/c1/media/img.png"
        );
        assert!(
            chat.messages[0].variants[0].media_items[0]
                .remote_url
                .is_empty()
        );
    }

    #[test]
    fn v2_kotlin_json_decodes_cleanly_including_legacy_bytedance_and_opaque_signatures() {
        let v2_profile_json = r#"{
            "id": "p-legacy",
            "name": "Legacy",
            "protocol": "BYTEDANCE_ARK",
            "storeServerSide": false,
            "useStatefulChaining": false
        }"#;
        let profile: ProviderProfile = serde_json::from_str(v2_profile_json).unwrap();
        assert_eq!(profile.protocol, ProviderProtocol::OpenAiResponses);
        assert!(!profile.keeps_provider_state());

        let v2_variant_json = r#"{
            "id": "v1",
            "content": "Answer",
            "thinkingSignature": "opaque_sig_==+/99",
            "toolCalls": [{
                "id": "tc1",
                "name": "fs_read",
                "argumentsJson": "{\"path\":\"a.rs\"}",
                "thoughtSignature": "tool_opaque_sig_xyz",
                "startsToolStep": true
            }]
        }"#;
        let mut variant: MessageVariant = serde_json::from_str(v2_variant_json).unwrap();
        assert_eq!(variant.thinking_signature, "opaque_sig_==+/99");
        assert_eq!(
            variant.tool_calls[0].thought_signature,
            "tool_opaque_sig_xyz"
        );
        assert!(variant.tool_calls[0].starts_tool_step);
        // Transient screenshot must NEVER serialize to JSON
        variant.tool_calls[0].screenshot_jpeg_base64 = "SECRET_SCREEN".into();
        let encoded = serde_json::to_string(&variant).unwrap();
        assert!(!encoded.contains("SECRET_SCREEN"));
    }
}
