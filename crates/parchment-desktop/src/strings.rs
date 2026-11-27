//! Plain-language UI copy table adhering to `docs/design.md` ("Words on screen") and `Strings.kt`.
//!
//! Sentence case, no internal developer jargon:
//! "API format" (not wire dialect), "Server URL" (not base URL), "Shorten the conversation"
//! (not compact), "Conversation limit" (not context window), "Send now" (not steer),
//! "Branch" (not fork), "Choose folder" (not mount), "Thinking" (not reasoning),
//! "Reused" (not cache read), "Update" (not OTA).

pub const APP_NAME: &str = "YL Axiom";
pub const NEW_CHAT: &str = "New chat";
pub const UNTITLED: &str = "Untitled";
pub const DEFAULT_ASSISTANT: &str = "Default Assistant";

// Sidebar
pub const SIDEBAR_ASSISTANTS: &str = "Assistants";
pub const SIDEBAR_CHATS: &str = "Chats";
pub const SIDEBAR_PINNED: &str = "Pinned";
pub const SEARCH_CHATS: &str = "Search chats…";
pub const SEARCH_ASSISTANTS: &str = "Search assistants…";
pub const SEARCH_PINNED: &str = "Search pinned…";
pub const CLEAR_ALL: &str = "Clear all";

// Top bar & tabs
pub const CHOOSE_MODEL: &str = "Choose a model";
pub const SELECT_MODEL: &str = "Select model";
pub const MANAGE_PROVIDERS: &str = "Manage providers";
pub const SETTINGS_TITLE: &str = "Settings";
pub const CHAT_BACK: &str = "Chat";
pub const VIEW_BUBBLES: &str = "Bubbles";
pub const VIEW_PAGE: &str = "Page";
pub const NEW_TAB: &str = "New tab";

// Composer
pub const SEND: &str = "Send";
pub const STOP: &str = "Stop";
pub const QUEUE: &str = "Queue";
pub const SEND_NOW: &str = "Send now";
pub const RETRY_NOW: &str = "Retry now";
pub const TOOLS: &str = "Tools";
pub const GOAL_MODE: &str = "Goal mode";
pub const PREFILL_LABEL: &str = "Start the reply with…";
pub const SHORTEN_CONVERSATION: &str = "Shorten the conversation";

// Message actions
pub const COPIED: &str = "Copied";
pub const COPY_MARKDOWN: &str = "Copy Markdown";
pub const EDIT: &str = "Edit";
pub const CONTINUE: &str = "Continue";
pub const TRANSLATE: &str = "Translate";
pub const BRANCH: &str = "Branch";
pub const REWIND: &str = "Rewind";
pub const EXCLUDE_CONTEXT: &str = "Exclude from context";
pub const INCLUDE_CONTEXT: &str = "Include in context";
pub const LEFT_OUT_OF_CONTEXT: &str = "Excluded from context";
pub const RAW_WIRE: &str = "Raw request & response";
pub const DELETE: &str = "Delete";
pub const SAVE: &str = "Save";
pub const SAVE_AND_REGENERATE: &str = "Save & regenerate";
pub const CANCEL: &str = "Cancel";

// Notices & modals
pub const UNDO: &str = "Undo";
pub const COMMAND_PALETTE_TITLE: &str = "Command palette";
pub const COMMAND_PALETTE_PLACEHOLDER: &str = "Type a command or chat title…";
pub const PROMPT_JUMP_TITLE: &str = "Jump to a prompt";
pub const PROMPT_JUMP_PLACEHOLDER: &str = "Filter prompts in this chat…";
pub const EMPTY_START_HEADING: &str = "Start with a question, a draft or a file.";
pub const CHOOSE_A_MODEL_IN_SETTINGS: &str = "Choose a model in Settings";
pub const START_OPENING_SCENE: &str = "Start opening scene";
pub const CHECK_API_KEY: &str = "Check API key";
pub const STREAMING_FOLLOW_UP_PLACEHOLDER: &str =
    "Type a follow-up to send now or queue for later…";

// Settings
pub const CHOOSE_FOLDER: &str = "Choose folder";
pub const CLEAR_FOLDER: &str = "Clear folder";
pub const SERVER_URL_LABEL: &str = "Server URL";
pub const CHECK_FOR_UPDATES: &str = "Check for updates";
pub const EXPORT_ARCHIVE: &str = "Export chats (.parchment)";
pub const IMPORT_ARCHIVE: &str = "Import chats or card";
pub const WIPE_ALL_DATA: &str = "Wipe all local data";

pub fn window_title_for_chat(title: &str) -> String {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        APP_NAME.to_string()
    } else {
        format!("{trimmed} — {APP_NAME}")
    }
}

pub fn window_title_for_settings() -> String {
    format!("{SETTINGS_TITLE} — {APP_NAME}")
}

pub fn window_title_for_stream(status: &str, elapsed: &str, chat_title: &str) -> String {
    let base = window_title_for_chat(chat_title);
    let st = status.trim();
    if st.is_empty() {
        format!("Working · {elapsed} — {base}")
    } else {
        format!("{st} · {elapsed} — {base}")
    }
}

pub fn message_assistant_placeholder(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        format!("Message {DEFAULT_ASSISTANT}…")
    } else {
        format!("Message {trimmed}…")
    }
}

pub fn add_api_key_to_start(provider_name: &str) -> String {
    format!("Add your {provider_name} API key to start chatting.")
}

pub fn set_up_provider(provider_name: &str) -> String {
    format!("Set up {provider_name}")
}

pub fn deleted_chat_notice(title: &str) -> String {
    format!("Deleted \"{title}\".")
}
