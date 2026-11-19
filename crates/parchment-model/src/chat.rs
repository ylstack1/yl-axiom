use serde::{Deserialize, Serialize};

use crate::permissions::AgentPermissionOverrides;
use crate::provider::ProviderProtocol;
use crate::usage::{ApiUsageRecord, TokenMetrics};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    #[serde(rename = "IMAGE")]
    Image,
    #[serde(rename = "VIDEO")]
    Video,
    #[serde(rename = "AUDIO")]
    Audio,
}

fn default_mime_png() -> String {
    "image/png".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedMediaItem {
    pub id: String,
    pub kind: MediaKind,
    #[serde(default = "default_mime_png")]
    pub mime_type: String,
    #[serde(default)]
    pub local_file_path: String,
    #[serde(default)]
    pub remote_url: String,
    #[serde(default)]
    pub revised_prompt: String,
    #[serde(default)]
    pub aspect_ratio: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    #[serde(rename = "USER")]
    User,
    #[serde(rename = "ASSISTANT")]
    Assistant,
    #[serde(rename = "SYSTEM")]
    System,
}

impl MessageRole {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Assistant => "ASSISTANT",
            Self::System => "SYSTEM",
        }
    }

    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "USER" | "user" => Self::User,
            "SYSTEM" | "system" => Self::System,
            _ => Self::Assistant,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments_json: String,
    pub result_text: String,
    pub is_error: bool,
    pub is_running: bool,
    pub duration_ms: i64,
    /// Opaque reasoning/tool signature — MUST be preserved verbatim.
    pub thought_signature: String,
    pub preceding_thinking: String,
    /// Text the model wrote after the previous tool and before this one.
    pub preceding_text: String,
    /// Sent once with the next request and NEVER persisted to disk/SQLite.
    #[serde(skip)]
    pub screenshot_jpeg_base64: String,
    pub message_phases: bool,
    /// First call of a model step so history separates sequential steps from parallel calls.
    pub starts_tool_step: bool,
}

fn default_todo_status() -> String {
    "pending".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    #[serde(default = "default_todo_status")]
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GoalTrackerState {
    pub goal_objective: String,
    pub todos: Vec<TodoItem>,
    pub is_completed: bool,
    pub verification_summary: String,
}

impl GoalTrackerState {
    pub fn completed_count(&self) -> usize {
        self.todos
            .iter()
            .filter(|t| t.status.eq_ignore_ascii_case("completed"))
            .count()
    }

    pub fn has_pending_or_in_progress(&self) -> bool {
        !self.is_completed
            && ((self.todos.is_empty() && !self.goal_objective.trim().is_empty())
                || self.todos.iter().any(|t| {
                    t.status.eq_ignore_ascii_case("pending")
                        || t.status.eq_ignore_ascii_case("in_progress")
                }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptAttachment {
    pub id: String,
    pub file_name: String,
    pub mime_type: String,
    #[serde(default)]
    pub base64_data: String,
    #[serde(default)]
    pub text_content: String,
    #[serde(default)]
    pub local_file_path: String,
}

fn default_citation_index() -> i32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSourceCitation {
    #[serde(default = "default_citation_index")]
    pub index: i32,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub grounded_line_numbers: Vec<i32>,
    #[serde(default)]
    pub grounded_lines: Vec<String>,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessageVariant {
    pub id: String,
    pub content: String,
    pub thinking_summary: String,
    pub trailing_thinking: String,
    /// Opaque thinking signature — preserved verbatim.
    pub thinking_signature: String,
    pub thinking_duration_ms: i64,
    pub tool_calls: Vec<ToolCallRecord>,
    pub goal_tracker: Option<GoalTrackerState>,
    pub media_items: Vec<GeneratedMediaItem>,
    pub attachments: Vec<PromptAttachment>,
    pub web_sources: Vec<WebSourceCitation>,
    pub provider_interaction_id: String,
    pub pending_background_interaction_id: String,
    pub is_streaming: bool,
    pub provider_protocol: Option<ProviderProtocol>,
    /// Concrete wire dialect that minted this turn (`openai_chat`, `openai_responses`, etc.).
    pub provider_wire_dialect: String,
    pub model_used: String,
    pub created_at: i64,
    pub token_metrics: TokenMetrics,
    pub api_usage_records: Vec<ApiUsageRecord>,
    pub legacy_usage_owner_session_id: String,
    pub raw_request_preview: String,
    pub raw_response_preview: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNode {
    pub id: String,
    pub role: MessageRole,
    #[serde(default)]
    pub variants: Vec<MessageVariant>,
    #[serde(default)]
    pub active_variant_index: usize,
    #[serde(default = "default_true")]
    pub included_in_context: bool,
    #[serde(default)]
    pub pinned: bool,
}

impl MessageNode {
    pub fn active_variant(&self) -> Option<&MessageVariant> {
        if self.variants.is_empty() {
            return None;
        }
        let idx = self.active_variant_index.min(self.variants.len() - 1);
        self.variants.get(idx)
    }

    pub fn active_content(&self) -> &str {
        self.active_variant()
            .map(|v| v.content.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[serde(rename = "MANUSCRIPT")]
    Manuscript,
    #[default]
    #[serde(rename = "EDITORIAL_TURNS")]
    EditorialTurns,
}

impl ViewMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Manuscript => "MANUSCRIPT",
            Self::EditorialTurns => "EDITORIAL_TURNS",
        }
    }

    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "MANUSCRIPT" => Self::Manuscript,
            _ => Self::EditorialTurns,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceMode {
    #[default]
    #[serde(rename = "OFF")]
    Off,
    #[serde(rename = "READ_ONLY")]
    ReadOnly,
    #[serde(rename = "READ_WRITE")]
    ReadWrite,
}

impl WorkspaceMode {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::ReadOnly => "READ_ONLY",
            Self::ReadWrite => "READ_WRITE",
        }
    }

    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "READ_ONLY" => Self::ReadOnly,
            "READ_WRITE" => Self::ReadWrite,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FragmentActivationMode {
    #[default]
    #[serde(rename = "PINNED")]
    Pinned,
    #[serde(rename = "KEYWORD")]
    Keyword,
    #[serde(rename = "MUTED")]
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextFragment {
    pub id: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub mode: FragmentActivationMode,
}

fn default_source_format() -> String {
    "parchment-v1".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CharacterOrScenarioCard {
    pub name: String,
    pub description: String,
    pub personality: String,
    pub scenario: String,
    pub first_message: String,
    pub alternate_greetings: Vec<String>,
    pub example_dialogue: String,
    pub system_prompt: String,
    pub post_history_instructions: String,
    pub creator_notes: String,
    pub tags: Vec<String>,
    pub embedded_fragments: Vec<ContextFragment>,
    #[serde(default = "default_source_format")]
    pub source_format: String,
    /// Original imported card `data` JSON object so unedited fields survive export.
    pub source_data: String,
}

impl CharacterOrScenarioCard {
    pub fn build_compiled_instructions(&self, user_name: &str) -> String {
        let char_name = if self.name.trim().is_empty() {
            "Character"
        } else {
            self.name.as_str()
        };
        let replace_macros = |input: &str| -> String {
            replace_ascii_case_insensitive(
                &replace_ascii_case_insensitive(input, "{{char}}", char_name),
                "{{user}}",
                user_name,
            )
        };

        let mut sb = String::new();
        if !self.system_prompt.trim().is_empty() {
            sb.push_str(&replace_macros(self.system_prompt.trim()));
            sb.push_str("\n\n");
        }
        if !self.name.trim().is_empty() {
            sb.push_str("### Character / Persona: ");
            sb.push_str(self.name.trim());
            sb.push('\n');
        }
        if !self.description.trim().is_empty() {
            sb.push_str(&replace_macros(self.description.trim()));
            sb.push_str("\n\n");
        }
        if !self.personality.trim().is_empty() {
            sb.push_str("### Personality & Voice\n");
            sb.push_str(&replace_macros(self.personality.trim()));
            sb.push_str("\n\n");
        }
        if !self.scenario.trim().is_empty() {
            sb.push_str("### Scenario & Setting\n");
            sb.push_str(&replace_macros(self.scenario.trim()));
            sb.push_str("\n\n");
        }
        if !self.example_dialogue.trim().is_empty() {
            sb.push_str("### Style & Dialogue Examples\n");
            sb.push_str(&replace_macros(self.example_dialogue.trim()));
            sb.push_str("\n\n");
        }
        sb.trim().to_string()
    }
}

fn replace_ascii_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let hay_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let nlen = needle_bytes.len();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i < hay_bytes.len() {
        if i + nlen <= hay_bytes.len() && hay_bytes[i..i + nlen].eq_ignore_ascii_case(needle_bytes)
        {
            out.push_str(replacement);
            i += nlen;
        } else {
            let ch = haystack[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn default_user_name() -> String {
    "User".to_string()
}
fn default_authors_note_depth() -> i32 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub profile_id: String,
    #[serde(default)]
    pub model_override: String,
    #[serde(default)]
    pub assistant_id: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_title_manually_set: bool,
    #[serde(default)]
    pub draft_input: String,
    #[serde(default)]
    pub draft_prefill: String,
    #[serde(default = "default_user_name")]
    pub user_name: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub active_card: Option<CharacterOrScenarioCard>,
    #[serde(default)]
    pub story_summary: String,
    #[serde(default)]
    pub fragments: Vec<ContextFragment>,
    #[serde(default)]
    pub authors_note: String,
    #[serde(default = "default_authors_note_depth")]
    pub authors_note_depth: i32,
    #[serde(default)]
    pub view_mode: ViewMode,
    #[serde(default)]
    pub workspace_path: String,
    #[serde(default)]
    pub workspace_mode: WorkspaceMode,
    #[serde(default)]
    pub permission_overrides: Option<AgentPermissionOverrides>,
    #[serde(default)]
    pub utility_usage_records: Vec<ApiUsageRecord>,
    #[serde(default)]
    pub messages: Vec<MessageNode>,
}

impl ChatSession {
    pub fn new(id: impl Into<String>, title: impl Into<String>, now_ms: i64) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            created_at: now_ms,
            updated_at: now_ms,
            profile_id: String::new(),
            model_override: String::new(),
            assistant_id: String::new(),
            is_pinned: false,
            is_title_manually_set: false,
            draft_input: String::new(),
            draft_prefill: String::new(),
            user_name: default_user_name(),
            system_prompt: String::new(),
            active_card: None,
            story_summary: String::new(),
            fragments: Vec::new(),
            authors_note: String::new(),
            authors_note_depth: 2,
            view_mode: ViewMode::EditorialTurns,
            workspace_path: String::new(),
            workspace_mode: WorkspaceMode::Off,
            permission_overrides: None,
            utility_usage_records: Vec::new(),
            messages: Vec::new(),
        }
    }
}

/// Lightweight chat summary for sidebar listing (zero message body hydration).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStub {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub profile_id: String,
    pub assistant_id: String,
    pub is_pinned: bool,
    pub message_count: u32,
    pub pinned_message_count: u32,
}

/// Lightweight pinned-reply entry for the sidebar Pinned tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedMessageStub {
    pub chat_id: String,
    pub chat_title: String,
    pub message_id: String,
    pub role: MessageRole,
    pub preview: String,
    pub created_at: i64,
}
