//! Plain-language UI copy table adhering to `docs/DESIGN.md` ("Words on screen").
//!
//! Sentence case, no internal developer jargon:
//! "API format" (not wire dialect), "Server URL" (not base URL), "Shorten the conversation"
//! (not compact), "Conversation limit" (not context window), "Send now" (not steer),
//! "Branch" (not fork), "Choose folder" (not mount), "Thinking" (not reasoning),
//! "Reused" (not cache read), "Update" (not OTA).

pub const APP_NAME: &str = "Parchment";
pub const NEW_CHAT: &str = "New chat";
pub const SIDEBAR_CHATS: &str = "Chats";
pub const SIDEBAR_SEARCH: &str = "Search";
pub const SIDEBAR_PINNED: &str = "Pinned";
pub const SIDEBAR_FILES: &str = "Files";
pub const VIEW_BUBBLES: &str = "Bubbles";
pub const VIEW_PAGE: &str = "Page";
pub const SETTINGS_TITLE: &str = "Settings";
pub const SEND: &str = "Send";
pub const STOP: &str = "Stop";
pub const RETRY_NOW: &str = "Retry now";
pub const UNDO: &str = "Undo";
pub const DISMISS: &str = "Dismiss";
pub const COMMAND_PALETTE_TITLE: &str = "Command palette";
pub const PROMPT_JUMP_TITLE: &str = "Jump to a prompt";
pub const EMPTY_NO_PROVIDER_TITLE: &str = "Connect a model provider to begin";
pub const EMPTY_NO_PROVIDER_BODY: &str =
    "Add an API key for OpenAI, Anthropic, Google Gemini, or a custom Server URL in Settings.";
pub const SETUP_PROVIDER_CTA: &str = "Set up a provider";
pub const CHOOSE_FOLDER: &str = "Choose folder";
pub const API_FORMAT_LABEL: &str = "API format";
pub const SERVER_URL_LABEL: &str = "Server URL";
pub const THINKING_LABEL: &str = "Thinking";
pub const CHECK_FOR_UPDATES: &str = "Check for updates";
pub const EXPORT_ARCHIVE: &str = "Export chats (.parchment)";
pub const IMPORT_ARCHIVE: &str = "Import chats or card";
pub const MIGRATE_V2: &str = "Import from Parchment v2";

pub fn window_title_for_chat(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        APP_NAME.to_string()
    } else {
        format!("{trimmed} — {APP_NAME}")
    }
}

pub fn deleted_chat_notice(title: &str) -> String {
    format!("Deleted \"{title}\".")
}
