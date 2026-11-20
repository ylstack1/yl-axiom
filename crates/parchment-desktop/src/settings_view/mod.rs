//! Full 8-category Settings screen matching `ParchmentSettingsPages.kt` and all subpages 1:1:
//! 1. Preferences (`desktop-settings.png`): profile picture/name, spend limit, 9 visual
//!    `ThemePreviewCard` tiles, 8 circular accent swatches + 4-field hex editor, fonts + `.ttf/.otf`,
//!    typography steppers, ALL 16 chat display toggles, ALL 9 rendering toggles, ALL 18 behavior toggles.
//! 2. Providers (`desktop-page-provi.png`): provider cards with `ModelLabSvgIcon` & `On`/`Off`,
//!    live connection test, `Advanced` protocol subpage, `ModelConfig` subpage with catalog sync.
//! 3. Assistant (`desktop-page-assis.png`): list + dedicated Editor subpage with 18 vector icons,
//!    model/thinking overrides, and embedded default permissions.
//! 4. Default Model: 4 numbered cards (Global, Title + template, Summary, Translation + 8 languages).
//! 5. Workspace (`desktop-page-works.png`): 48px `AgentPermissionEditor`, Global & Project memory,
//!    MCP servers + JSON import, Agent Skills, and Agent Tool Registry (`X of Y available`).
//! 6. World Book & Cards (`desktop-page-world.png`): PNG/JSON cards, Prompt presets, chat/global lore.
//! 7. Shortcuts (`SettingsShortcutsPage.kt`): live key recorder, conflict swap, reset, send mode.
//! 8. Storage & Updates (`desktop-page-stora.png`): real disk file manager with proportional bar,
//!    orphan cleanup, preview/open/delete, OTA updates, `.parchment` & `.json` backup, wipe.
//!
//! Plus live cross-page Settings Search with per-category match counts.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::harness::{McpServerState, McpServerStatus};
use parchment_core::model::{
    AgentPermissionPolicy, AppStateSnapshot, AuthHeaderMode, CUSTOM_DIALECT_ANTHROPIC,
    CUSTOM_DIALECT_GEMINI, CUSTOM_DIALECT_OPENAI_CHAT, CUSTOM_DIALECT_OPENAI_RESPONSES,
    ChatSession, FragmentActivationMode, MessageNavVisibility, ProviderProfile, ProviderProtocol,
    SendShortcutMode, ShellAccess, ToolApprovalMode, ViewMode, WorkspaceMode,
};

use crate::icons::{ASSISTANT_ICON_CHOICES, KissIcon, kiss_icon, model_lab_icon, tool_kiss_icon};
use crate::ota::UpdateReleaseInfo;
use crate::shortcuts::{
    AppShortcutAction, ShortcutBinding, is_shortcut_customized, resolve_shortcut,
};
use crate::strings;
use crate::theme::{
    ACCENT_SWATCHES, CAT_BRASS, CAT_OCHRE, CAT_SAGE, CAT_SLATE, CODE_FONTS, ParchmentPalette,
    TEXT_FONTS, parse_hex_color, theme_presets,
};
use crate::ui_kit::{
    SepiaButtonStyle, adaptive_setting_row, field_label, format_bytes_compact, format_tokens_short,
    h_divider, icon_button, kiss_bounce, kiss_bounce_subtle, model_privacy_badge_chip,
    permission_option_row, primary_model_privacy_badge, resolve_model_privacy_badges,
    rubric_mark_overlay, section_card, section_card_with_action, sepia_avatar_circle, sepia_button,
    sepia_chip, sepia_toggle_switch, setting_toggle_row, shortcut_keycap_row, status_pill,
    step_slider, v_divider,
};

mod assistants;
mod preferences;
mod provider_subpages;
mod providers;
mod workspace;
mod worldbook_storage;

use assistants::*;
use preferences::*;
use provider_subpages::*;
use providers::*;
use workspace::*;
use worldbook_storage::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Preferences,
    Providers,
    Assistants,
    DefaultModel,
    Workspace,
    WorldBook,
    Shortcuts,
    BackupAndUpdates,
}

impl SettingsSection {
    pub const ALL: [Self; 8] = [
        Self::Preferences,
        Self::Providers,
        Self::Assistants,
        Self::DefaultModel,
        Self::Workspace,
        Self::WorldBook,
        Self::Shortcuts,
        Self::BackupAndUpdates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Preferences => "Preferences",
            Self::Providers => "Providers",
            Self::Assistants => "Assistant",
            Self::DefaultModel => "Default Model",
            Self::Workspace => "Workspace",
            Self::WorldBook => "World Book & Cards",
            Self::Shortcuts => "Shortcuts",
            Self::BackupAndUpdates => "Storage & Updates",
        }
    }

    pub fn icon(self) -> KissIcon {
        match self {
            Self::Preferences => KissIcon::Monitor,
            Self::Providers => KissIcon::ServerNetwork,
            Self::Assistants => KissIcon::Bot,
            Self::DefaultModel => KissIcon::Cpu,
            Self::Workspace => KissIcon::Folder,
            Self::WorldBook => KissIcon::BookOpen,
            Self::Shortcuts => KissIcon::Keyboard,
            Self::BackupAndUpdates => KissIcon::Cloud,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProvidersSubpage {
    #[default]
    Main,
    Advanced,
    ModelConfig(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AssistantSubpage {
    #[default]
    List,
    Edit(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldBookScope {
    ActiveChat,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFileCategory {
    Database,
    Uploads,
    GeneratedMedia,
    Backups,
    TempOrLogs,
}

impl StorageFileCategory {
    pub const ALL: [Self; 5] = [
        Self::Database,
        Self::Uploads,
        Self::GeneratedMedia,
        Self::Backups,
        Self::TempOrLogs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Database => "Database",
            Self::Uploads => "Uploads",
            Self::GeneratedMedia => "Media",
            Self::Backups => "Backups",
            Self::TempOrLogs => "Temp / Logs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredFileEntry {
    pub file_name: String,
    pub relative_path: String,
    pub full_path: String,
    pub size_bytes: u64,
    pub category: StorageFileCategory,
    pub is_orphan_or_temp: bool,
}

#[derive(Debug, Clone)]
pub struct ShortcutConflictPrompt {
    pub target: AppShortcutAction,
    pub conflict_with: AppShortcutAction,
    pub binding: ShortcutBinding,
}

#[derive(Debug, Clone)]
pub struct InstalledSkillInfo {
    pub name: String,
    pub path: String,
    pub source_label: String,
}

#[derive(Debug, Clone)]
pub struct McpDirectoryItem {
    pub name: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub needs_token: bool,
}

/// Builds a fresh default provider profile directly (zero hardcoded models).
pub fn make_default_profile(protocol: ProviderProtocol) -> ProviderProfile {
    let (id, name) = match protocol {
        ProviderProtocol::OpenAiResponses => ("prof-openai", "OpenAI"),
        ProviderProtocol::AnthropicMessages => ("prof-anthropic", "Anthropic"),
        ProviderProtocol::GeminiInteractions => ("prof-gemini", "Google Gemini"),
        ProviderProtocol::Custom => ("prof-custom", "Custom API"),
    };
    ProviderProfile::new(id, name, protocol)
}

pub fn auth_mode_label(mode: AuthHeaderMode) -> &'static str {
    match mode {
        AuthHeaderMode::Native => "Native",
        AuthHeaderMode::BearerOnly => "Authorization: Bearer",
        AuthHeaderMode::QueryParam => "Query param (?key=)",
    }
}

#[derive(Debug, Clone)]
pub enum SettingsAction {
    Close,
    SelectSection(SettingsSection),
    ClearSearch,
    // Preferences
    PickProfilePicture,
    RemoveProfilePicture,
    SetViewMode(ViewMode),
    ApplyThemePreset(usize),
    SetAccentHex(String),
    ToggleHexEditor,
    ApplyCustomHexPalette,
    SetProseFont(String),
    PickCustomProseFont,
    ClearCustomProseFont,
    SetCodeFont(String),
    PickCustomCodeFont,
    ClearCustomCodeFont,
    SetSendShortcut(SendShortcutMode),
    SetMessageNavVisibility(MessageNavVisibility),
    SetPasteThreshold(i32),
    TogglePref(&'static str, bool),
    // Providers
    SelectProfile(String),
    AddProfile(ProviderProtocol),
    DeleteProfile(String),
    OpenProviderSubpage(ProvidersSubpage),
    SetProfileProtocol(ProviderProtocol),
    TestProviderConnection,
    ResetProviderDefaultUrl,
    ClearProviderUrl,
    SetCustomDialect(String),
    SetAuthHeaderMode(AuthHeaderMode),
    SetThinkingEffort(String),
    ToggleProviderFlag(&'static str, bool),
    ToggleShowApiKey,
    ToggleAddApiKeyBox,
    AddExtraApiKey,
    RemoveExtraApiKey(usize),
    PromoteExtraApiKey(usize),
    ToggleAddModelBox,
    AddManualModel,
    RemoveSavedModel(String),
    ClearSavedModels,
    SaveProviderFields,
    FetchProviderModels,
    SelectSavedModel(String),
    SyncModelSpecFromCatalog(String),
    FetchModelCatalog,
    ResetModelSpec(String),
    OpenAssistantSubpage(AssistantSubpage),
    CreateAssistant,
    DuplicateAssistant(String),
    DeleteAssistant(String),
    SetAssistantIcon(String),
    SetAssistantThinkingEffort(String),
    ToggleAssistantFollowGlobalModel(bool),
    SetAssistantModel {
        profile_id: String,
        model_id: String,
    },
    ToggleAssistantToolsEnabled,
    ToggleAssistantBuiltinTool(String, bool),
    SetAssistantWorkspaceMode(WorkspaceMode),
    PickAssistantWorkspaceFolder,
    RemoveAssistantWorkspaceFolder(String),
    ClearAssistantWorkspaceFolder,
    ToggleAssistantShell,
    ToggleAssistantWeb,
    ToggleAssistantComputer,
    SetAssistantApprovalMode(ToolApprovalMode),
    // Default models
    SetGlobalDefaultModel {
        profile_id: String,
        model_id: String,
    },
    SetUtilityModel {
        slot: &'static str,
        profile_id: String,
        model_id: String,
    },
    ToggleDefaultModelFlag(&'static str, bool),
    SetTranslationLanguage(String),
    // Workspace, Memory, MCP, Skills & Tool Registry
    ToggleToolsEnabled,
    PickWorkspaceFolder,
    RemoveWorkspaceFolder(String),
    ClearWorkspaceFolder,
    ResetChatWorkspaceToAssistant,
    SetWorkspaceMode(WorkspaceMode),
    ToggleShellAccess,
    ToggleWebAccess,
    ToggleComputerAccess,
    SetApprovalMode(ToolApprovalMode),
    ToggleBuiltinTool(String, bool),
    OpenOrCreateGlobalMemory,
    OpenProjectMemoryFile(String),
    AddMcpServer,
    BrowseMcpCommand,
    ImportMcpJsonFile,
    ToggleMcpServer(String, bool),
    RestartMcpServers,
    DeleteMcpServer(String),
    SearchMcpRegistry,
    ToggleMcpRegistryNoTokenOnly,
    AddMcpRegistryServer {
        name: String,
        url: String,
    },
    InstallSkillFile,
    InstallSkillFromUrl,
    InstallSkillFolder,
    InspectSkillFile(String),
    DeleteSkillFile(String),
    SetWorldBookScope(WorldBookScope),
    AddWorldBookFragment,
    CycleFragmentMode(String),
    DeleteWorldBookFragment(String),
    SaveCurrentPromptAsPreset,
    ApplyPresetToChat(String),
    DeletePreset(String),
    ImportCharacterCard,
    CreateBlankActiveCard,
    SaveActiveCardToLibrary,
    ExportActiveCard,
    ExportActiveCardPng,
    ApplySavedCard(String),
    DeleteSavedCard(String),
    ClearActiveCard,
    // Shortcuts
    BeginRecordShortcut(AppShortcutAction),
    CancelRecordShortcut,
    AcceptShortcutSwap,
    CancelShortcutSwap,
    ResetSingleShortcut(AppShortcutAction),
    ResetAllShortcuts,
    // Storage & Updates
    RefreshStorageScan,
    SetStorageCategoryFilter(Option<StorageFileCategory>),
    CleanOrphanStorageFiles,
    PreviewStoredFile(String),
    OpenStoredFile(String),
    DeleteStoredFile(String),
    OpenDataFolder,
    ExportArchive,
    ExportCurrentChatArchive,
    ExportCurrentChatJson,
    ImportArchive,
    ExportSettingsJson,
    ImportSettingsJson,
    ToggleAutoCheckOta(bool),
    CheckForUpdates,
    OpenReleasePage(String),
    VerifyDownloadedInstaller,
    RequestWipeAllData,
    ConfirmWipeAllData,
    CancelWipeAllData,
}

pub type SettingsActionHandler = Rc<dyn Fn(SettingsAction, &mut Window, &mut App)>;

/// All editable `InputState` / `TextareaState` entities owned by `ParchmentDesktopApp` for Settings.
pub struct SettingsInputs {
    pub search: Entity<InputState>,
    // Preferences
    pub user_profile_name: Entity<InputState>,
    pub spend_limit: Entity<InputState>,
    pub paste_threshold: Entity<InputState>,
    pub prose_font_size: Entity<InputState>,
    pub code_font_size: Entity<InputState>,
    pub max_reading_width: Entity<InputState>,
    pub line_height: Entity<InputState>,
    pub paragraph_spacing: Entity<InputState>,
    pub hex_canvas: Entity<InputState>,
    pub hex_card: Entity<InputState>,
    pub hex_text: Entity<InputState>,
    pub hex_accent: Entity<InputState>,
    pub provider_name: Entity<InputState>,
    pub api_key: Entity<InputState>,
    pub extra_api_key_add: Entity<InputState>,
    pub base_url: Entity<InputState>,
    pub endpoint_override: Entity<InputState>,
    pub default_model: Entity<InputState>,
    pub manual_model_add: Entity<InputState>,
    pub model_filter: Entity<InputState>,
    pub temperature: Entity<InputState>,
    pub top_p: Entity<InputState>,
    pub max_output_tokens: Entity<InputState>,
    pub context_window: Entity<InputState>,
    pub thinking_budget_tokens: Entity<InputState>,
    pub input_price: Entity<InputState>,
    pub output_price: Entity<InputState>,
    pub custom_headers_raw: Entity<TextareaState>,
    pub raw_json_extra_body: Entity<TextareaState>,
    // Per-model config subpage
    pub model_cfg_context: Entity<InputState>,
    pub model_cfg_max_out: Entity<InputState>,
    pub model_cfg_in_price: Entity<InputState>,
    pub model_cfg_out_price: Entity<InputState>,
    pub model_cfg_cache_read: Entity<InputState>,
    pub model_cfg_cache_write: Entity<InputState>,
    pub model_cfg_long_cache_write: Entity<InputState>,
    pub model_cfg_reasoning_price: Entity<InputState>,
    // Assistant
    pub assistant_name: Entity<InputState>,
    pub assistant_system_prompt: Entity<TextareaState>,
    pub assistant_authors_note: Entity<TextareaState>,
    pub assistant_default_model: Entity<InputState>,
    pub assistant_temperature: Entity<InputState>,
    pub assistant_top_p: Entity<InputState>,
    pub assistant_max_output: Entity<InputState>,
    pub assistant_context_msg_limit: Entity<InputState>,
    // Default Models
    pub default_model_filter: Entity<InputState>,
    pub title_prompt: Entity<TextareaState>,
    pub translation_lang: Entity<InputState>,
    // MCP, Skills & Tool Registry
    pub mcp_name: Entity<InputState>,
    pub mcp_url: Entity<InputState>,
    pub mcp_command: Entity<InputState>,
    pub mcp_bearer: Entity<InputState>,
    pub mcp_registry_search: Entity<InputState>,
    pub skill_github_url: Entity<InputState>,
    pub tool_registry_search: Entity<InputState>,
    // World Book & Active Card Editor
    pub wb_card_name: Entity<InputState>,
    pub wb_card_desc: Entity<TextareaState>,
    pub wb_card_first_mes: Entity<TextareaState>,
    pub wb_title: Entity<InputState>,
    pub wb_keywords: Entity<InputState>,
    pub wb_content: Entity<TextareaState>,
    // Storage & OTA
    pub storage_search: Entity<InputState>,
    pub ota_repo: Entity<InputState>,
}

pub struct SettingsRenderProps<'a> {
    pub palette: &'a ParchmentPalette,
    pub section: SettingsSection,
    pub providers_subpage: &'a ProvidersSubpage,
    pub assistant_subpage: &'a AssistantSubpage,
    pub show_api_key: bool,
    pub show_add_api_key_box: bool,
    pub show_add_model_box: bool,
    pub show_hex_editor: bool,
    pub world_book_scope: WorldBookScope,
    pub recording_shortcut: Option<AppShortcutAction>,
    pub shortcut_feedback: &'a str,
    pub shortcut_conflict: Option<&'a ShortcutConflictPrompt>,
    pub storage_files: &'a [StoredFileEntry],
    pub storage_category_filter: Option<StorageFileCategory>,
    pub storage_search_query: &'a str,
    pub global_memory_bytes: Option<u64>,
    pub project_memory_files: &'a [(String, String)],
    pub installed_skills: &'a [InstalledSkillInfo],
    pub mcp_statuses: &'a HashMap<String, McpServerStatus>,
    pub mcp_registry_results: &'a [McpDirectoryItem],
    pub is_searching_mcp_registry: bool,
    pub mcp_registry_no_token_only: bool,
    pub tool_registry_query: &'a str,
    pub provider_test_status: &'a str,
    pub snapshot: &'a AppStateSnapshot,
    pub active_session: &'a ChatSession,
    pub resolved_policy: &'a AgentPermissionPolicy,
    pub inputs: &'a SettingsInputs,
    pub search_query: &'a str,
    pub model_filter_query: &'a str,
    pub default_model_filter_query: &'a str,
    pub is_fetching_models: bool,
    pub models_fetch_status: &'a str,
    pub is_checking_updates: bool,
    pub latest_release: Option<&'a UpdateReleaseInfo>,
    pub update_status: &'a str,
    pub chat_count: usize,
    pub data_dir_display: &'a str,
    pub confirm_wipe_pending: bool,
    pub on_action: &'a SettingsActionHandler,
}

const SEARCH_INDEX: &[(SettingsSection, &str, &str)] = &[
    (
        SettingsSection::Preferences,
        "Your profile & picture",
        "profile avatar name display picture",
    ),
    (
        SettingsSection::Preferences,
        "Spend limit",
        "spend budget limit usd cap",
    ),
    (
        SettingsSection::Preferences,
        "Theme presets & dark mode",
        "theme sepia dark light ink slate forest rose midnight",
    ),
    (
        SettingsSection::Preferences,
        "Accent color & custom hex",
        "accent color swatch hex custom",
    ),
    (
        SettingsSection::Preferences,
        "Typography & fonts",
        "font inter serif latex jetbrains mono size reading width line height",
    ),
    (
        SettingsSection::Preferences,
        "Bubbles / Page view mode",
        "bubbles page manuscript layout view mode",
    ),
    (
        SettingsSection::Preferences,
        "Chat item display toggles",
        "avatar timestamp model icon token stats thinking cards tool cards",
    ),
    (
        SettingsSection::Preferences,
        "Rendering (Markdown, math, code)",
        "markdown math latex dollar code wrap collapse dialogue narration",
    ),
    (
        SettingsSection::Preferences,
        "Behavior, startup & minimap",
        "minimap scroll paste confirm delete fork branch launch send shortcut",
    ),
    (
        SettingsSection::Providers,
        "Providers & API keys",
        "openai anthropic gemini custom api key server url endpoint",
    ),
    (
        SettingsSection::Providers,
        "Fetch models & pricing",
        "fetch models catalog pricing context window cost per million",
    ),
    (
        SettingsSection::Providers,
        "Thinking & server storage",
        "thinking reasoning effort server storage stateful chain temperature",
    ),
    (
        SettingsSection::Assistants,
        "Assistants & personas",
        "assistant agent persona system prompt author note icon",
    ),
    (
        SettingsSection::DefaultModel,
        "Global default model",
        "default model global provider",
    ),
    (
        SettingsSection::DefaultModel,
        "Title, summary & translation models",
        "title summary compact translation language prompt template",
    ),
    (
        SettingsSection::Workspace,
        "Workspace folder & permissions",
        "workspace folder read write shell terminal web computer approval",
    ),
    (
        SettingsSection::Workspace,
        "Global & project memory (AGENT.md)",
        "agent memory agents.md claude.md parchment.md",
    ),
    (
        SettingsSection::Workspace,
        "MCP servers",
        "mcp model context protocol server stdio http bearer json",
    ),
    (
        SettingsSection::Workspace,
        "Agent skills & tool registry",
        "skills tool registry fs_read fs_edit shell_exec web_fetch",
    ),
    (
        SettingsSection::WorldBook,
        "Character & scenario cards",
        "character card scenario png json import export",
    ),
    (
        SettingsSection::WorldBook,
        "System prompt presets",
        "prompt preset library",
    ),
    (
        SettingsSection::WorldBook,
        "World Book lore entries",
        "world book lore fragment keyword pinned",
    ),
    (
        SettingsSection::Shortcuts,
        "Keyboard shortcuts & bindings",
        "shortcut hotkey ctrl keybind swap reset enter",
    ),
    (
        SettingsSection::BackupAndUpdates,
        "Storage & file manager",
        "storage files disk bytes orphan clean database uploads media",
    ),
    (
        SettingsSection::BackupAndUpdates,
        "App updates (GitHub)",
        "update ota release version github installer",
    ),
    (
        SettingsSection::BackupAndUpdates,
        "Backup, export, import & wipe",
        "backup export import archive parchment json wipe",
    ),
];

pub fn render_settings_screen(props: SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let canvas = p.canvas_bg.to_hsla();
    let sheet = p.sheet_bg.to_hsla();
    let card = p.card_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let h_close = props.on_action.clone();
    let q_low = props.search_query.trim().to_ascii_lowercase();
    let q_tokens: Vec<&str> = q_low.split_whitespace().filter(|t| !t.is_empty()).collect();
    let matches_tokens = |haystack: &str| {
        if q_tokens.is_empty() {
            return false;
        }
        let h = haystack.to_ascii_lowercase();
        q_tokens.iter().all(|tok| h.contains(tok))
    };

    let mut match_counts: BTreeMap<usize, usize> = BTreeMap::new();
    if !q_tokens.is_empty() {
        for (sec, title, kw) in SEARCH_INDEX {
            if matches_tokens(&format!("{title} {kw}")) {
                let idx = SettingsSection::ALL
                    .iter()
                    .position(|s| s == sec)
                    .unwrap_or(0);
                *match_counts.entry(idx).or_insert(0) += 1;
            }
        }
        let model_hits: usize = props
            .snapshot
            .profiles
            .iter()
            .map(|pr| {
                pr.saved_models
                    .iter()
                    .filter(|m| matches_tokens(&format!("{} {m}", pr.name)))
                    .count()
            })
            .sum::<usize>()
            .min(10);
        if model_hits > 0 {
            *match_counts.entry(1).or_insert(0) += model_hits;
        }
        let asst_hits = props
            .snapshot
            .assistants
            .iter()
            .filter(|a| {
                matches_tokens(&format!(
                    "{} {} {} assistant",
                    a.name, a.system_prompt, a.authors_note
                ))
            })
            .count();
        if asst_hits > 0 {
            *match_counts.entry(2).or_insert(0) += asst_hits;
        }
        let lore_hits = props
            .active_session
            .fragments
            .iter()
            .chain(props.snapshot.global_world_book.iter())
            .filter(|f| {
                matches_tokens(&format!(
                    "{} {} {} world book lore",
                    f.title,
                    f.content,
                    f.keywords.join(" ")
                ))
            })
            .count();
        if lore_hits > 0 {
            *match_counts.entry(5).or_insert(0) += lore_hits;
        }
        let sc_hits = AppShortcutAction::ALL
            .iter()
            .filter(|a| {
                let chord =
                    resolve_shortcut(**a, &props.snapshot.preferences.custom_shortcuts).format();
                matches_tokens(&format!(
                    "{} {} {chord} shortcut",
                    a.title(),
                    a.description()
                ))
            })
            .count();
        if sc_hits > 0 {
            *match_counts.entry(6).or_insert(0) += sc_hits;
        }
    }
    // Left Category Rail (`desktop-settings.png`)
    let nav_rail = v_flex()
        .w(px(225.0))
        .h_full()
        .flex_shrink_0()
        .bg(sheet)
        .p(px(14.0))
        .gap(px(8.0))
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_semibold()
                        .text_color(ink)
                        .child(strings::SETTINGS_TITLE),
                )
                .child(sepia_button(
                    "set-back-chat",
                    strings::CHAT_BACK,
                    Some(KissIcon::ArrowLeft),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_close(SettingsAction::Close, w, cx),
                )),
        )
        .child(Input::new(&props.inputs.search).prefix(kiss_icon(KissIcon::Search, 13.0, muted)))
        .child(
            v_flex()
                .id("settings-nav-scroll")
                .flex_1()
                .w_full()
                .gap(px(3.0))
                .overflow_y_scrollbar()
                .children(
                    SettingsSection::ALL
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(ix, sec)| {
                            let active = sec == props.section && q_low.is_empty();
                            let count = match_counts.get(&ix).copied().unwrap_or(0);
                            let h = props.on_action.clone();
                            let mut item = h_flex()
                                .id(("set-nav-item", ix))
                                .relative()
                                .overflow_hidden()
                                .w_full()
                                .px(px(12.0))
                                .py(px(9.0))
                                .rounded(px(10.0))
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .bg(if active {
                                    card
                                } else {
                                    p.canvas_bg.with_alpha(0.0)
                                })
                                .hover(move |s| s.bg(if active { card } else { hover_bg }))
                                .on_click(move |_, w, cx| {
                                    h(SettingsAction::SelectSection(sec), w, cx)
                                });
                            if active {
                                item = item.child(rubric_mark_overlay(9.0, p));
                            }
                            if !q_low.is_empty() && count == 0 {
                                item = item.opacity(0.45);
                            }
                            kiss_bounce_subtle(
                                ("set-nav-kb", ix),
                                item.child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(kiss_icon(
                                            sec.icon(),
                                            14.0,
                                            if active { accent } else { narration },
                                        ))
                                        .child({
                                            let mut lbl = div()
                                                .truncate()
                                                .text_size(px(13.5))
                                                .text_color(if active { accent } else { ink })
                                                .child(sec.label());
                                            if active {
                                                lbl = lbl.font_semibold();
                                            } else {
                                                lbl = lbl.font_medium();
                                            }
                                            lbl
                                        }),
                                )
                                .children(
                                    (!q_low.is_empty() && count > 0).then(|| {
                                        div()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded(px(999.0))
                                            .bg(p.accent.with_alpha(0.16))
                                            .text_size(px(11.0))
                                            .font_semibold()
                                            .text_color(accent)
                                            .child(count.to_string())
                                    }),
                                ),
                            )
                        }),
                ),
        );

    let detail_body: AnyElement = if !q_low.is_empty() {
        render_settings_search_results(&props, &q_low)
    } else {
        match props.section {
            SettingsSection::Preferences => render_preferences_page(&props),
            SettingsSection::Providers => render_providers_page(&props),
            SettingsSection::Assistants => render_assistants_page(&props),
            SettingsSection::DefaultModel => render_default_model_page(&props),
            SettingsSection::Workspace => render_workspace_page(&props),
            SettingsSection::WorldBook => render_worldbook_page(&props),
            SettingsSection::Shortcuts => render_shortcuts_page(&props),
            SettingsSection::BackupAndUpdates => render_backup_ota_page(&props),
        }
    };

    let is_split_providers = q_low.is_empty() && props.section == SettingsSection::Providers;
    let right_pane: AnyElement = if is_split_providers {
        div()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .child(detail_body)
            .into_any_element()
    } else {
        let scroll_key: usize = (props.section as usize) * 8
            + usize::from(matches!(props.assistant_subpage, AssistantSubpage::Edit(_)))
            + usize::from(!q_low.is_empty()) * 2;
        div()
            .id(("settings-pane-wrap", scroll_key))
            .flex_1()
            .h_full()
            .overflow_hidden()
            .child(
                div()
                    .id(("settings-detail-scroll", scroll_key))
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .p(px(16.0))
                            .pb(px(36.0))
                            .gap(px(14.0))
                            .child(detail_body),
                    ),
            )
            .into_any_element()
    };

    h_flex()
        .size_full()
        .bg(canvas)
        .child(nav_rail)
        .child(v_divider(p))
        .child(right_pane)
        .into_any_element()
}

fn render_settings_search_results(props: &SettingsRenderProps<'_>, q_low: &str) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;
    let h_clr = on.clone();
    let hover_bg = p.hover_tint();
    let tokens: Vec<&str> = q_low.split_whitespace().filter(|t| !t.is_empty()).collect();
    let matches_tokens = |haystack: &str| {
        if tokens.is_empty() {
            return false;
        }
        let h = haystack.to_ascii_lowercase();
        tokens.iter().all(|tok| h.contains(tok))
    };

    let section_matches: Vec<(SettingsSection, &str)> = SEARCH_INDEX
        .iter()
        .filter(|(_, title, kw)| matches_tokens(&format!("{title} {kw}")))
        .map(|(s, t, _)| (*s, *t))
        .collect();

    let model_matches: Vec<(&ProviderProfile, &String)> = props
        .snapshot
        .profiles
        .iter()
        .flat_map(|prof| {
            prof.saved_models
                .iter()
                .filter(move |m| matches_tokens(&format!("{} {m}", prof.name)))
                .map(move |m| (prof, m))
        })
        .take(20)
        .collect();

    let asst_matches: Vec<&parchment_core::model::AssistantAgent> = props
        .snapshot
        .assistants
        .iter()
        .filter(|a| {
            matches_tokens(&format!(
                "{} {} {} assistant persona",
                a.name, a.system_prompt, a.authors_note
            ))
        })
        .collect();

    let mut seen_lore = std::collections::HashSet::new();
    let lore_matches: Vec<&parchment_core::model::ContextFragment> = props
        .active_session
        .fragments
        .iter()
        .chain(props.snapshot.global_world_book.iter())
        .filter(|f| seen_lore.insert(f.id.as_str()))
        .filter(|f| {
            matches_tokens(&format!(
                "{} {} {} world book lorebook fragment",
                f.title,
                f.content,
                f.keywords.join(" ")
            ))
        })
        .take(15)
        .collect();

    let shortcut_matches: Vec<AppShortcutAction> = AppShortcutAction::ALL
        .iter()
        .copied()
        .filter(|act| {
            let chord =
                resolve_shortcut(*act, &props.snapshot.preferences.custom_shortcuts).format();
            matches_tokens(&format!(
                "{} {} {chord} keyboard shortcut hotkey keybinding",
                act.title(),
                act.description()
            ))
        })
        .collect();

    let total_hits = section_matches.len()
        + model_matches.len()
        + asst_matches.len()
        + lore_matches.len()
        + shortcut_matches.len();

    let mut col = v_flex().w_full().gap(px(12.0)).child(
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                div()
                    .text_size(px(18.0))
                    .font_semibold()
                    .text_color(p.primary_ink.to_hsla())
                    .child(format!(
                        "Search results for \"{}\" ({total_hits})",
                        props.search_query.trim()
                    )),
            )
            .child(sepia_button(
                "set-search-clr",
                "Clear search",
                Some(KissIcon::Close),
                SepiaButtonStyle::Ghost,
                true,
                false,
                p,
                move |_, w, cx| h_clr(SettingsAction::ClearSearch, w, cx),
            )),
    );

    if total_hits == 0 {
        col = col.child(section_card(
            "No matching settings",
            Some("Try another keyword such as theme, font, api key, model name, mcp, shortcut, or storage."),
            p,
        ));
        return col.into_any_element();
    }

    if !model_matches.is_empty() {
        let mut m_card = section_card(&format!("Models ({})", model_matches.len()), None, p);
        for (ix, (prof, mid)) in model_matches.into_iter().enumerate() {
            let pid = prof.id.clone();
            let model_id = mid.clone();
            let h_sel = on.clone();
            m_card = m_card.child(
                h_flex()
                    .id(("set-sr-mod", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .border_1()
                    .border_color(p.subtle_border())
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_sel(SettingsAction::SelectProfile(pid.clone()), w, cx);
                        h_sel(SettingsAction::SelectSavedModel(model_id.clone()), w, cx);
                        h_sel(SettingsAction::ClearSearch, w, cx);
                        h_sel(
                            SettingsAction::SelectSection(SettingsSection::Providers),
                            w,
                            cx,
                        );
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(model_lab_icon(mid, 15.0, p.accent.to_hsla(), &prof.name))
                            .child(
                                v_flex()
                                    .child(
                                        div()
                                            .font_family(p.code_font_family.clone())
                                            .text_size(px(13.0))
                                            .font_medium()
                                            .text_color(p.primary_ink.to_hsla())
                                            .child(mid.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(p.muted_ink.to_hsla())
                                            .child(prof.name.clone()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_medium()
                            .text_color(p.accent.to_hsla())
                            .child("Select & open"),
                    ),
            );
        }
        col = col.child(m_card);
    }

    if !asst_matches.is_empty() {
        let mut a_card = section_card(&format!("Assistants ({})", asst_matches.len()), None, p);
        for (ix, asst) in asst_matches.into_iter().enumerate() {
            let aid = asst.id.clone();
            let h_open = on.clone();
            a_card = a_card.child(
                h_flex()
                    .id(("set-sr-asst", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .border_1()
                    .border_color(p.subtle_border())
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_open(SettingsAction::ClearSearch, w, cx);
                        h_open(
                            SettingsAction::SelectSection(SettingsSection::Assistants),
                            w,
                            cx,
                        );
                        h_open(
                            SettingsAction::OpenAssistantSubpage(AssistantSubpage::Edit(
                                aid.clone(),
                            )),
                            w,
                            cx,
                        );
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(sepia_avatar_circle(&asst.icon_emoji, false, 26.0, p))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_medium()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(asst.name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_medium()
                            .text_color(p.accent.to_hsla())
                            .child("Edit assistant"),
                    ),
            );
        }
        col = col.child(a_card);
    }

    if !shortcut_matches.is_empty() {
        let mut sc_card = section_card(
            &format!("Keyboard shortcuts ({})", shortcut_matches.len()),
            None,
            p,
        );
        for (ix, act) in shortcut_matches.into_iter().enumerate() {
            let tokens_row =
                resolve_shortcut(act, &props.snapshot.preferences.custom_shortcuts).keycap_tokens();
            let is_mod = is_shortcut_customized(act, &props.snapshot.preferences.custom_shortcuts);
            let h_jump = on.clone();
            sc_card = sc_card.child(
                h_flex()
                    .id(("set-sr-sc", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .border_1()
                    .border_color(p.subtle_border())
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_jump(SettingsAction::ClearSearch, w, cx);
                        h_jump(
                            SettingsAction::SelectSection(SettingsSection::Shortcuts),
                            w,
                            cx,
                        );
                    })
                    .child(
                        v_flex()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_medium()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(act.title()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(act.description()),
                            ),
                    )
                    .child(shortcut_keycap_row(&tokens_row, is_mod, p)),
            );
        }
        col = col.child(sc_card);
    }

    if !lore_matches.is_empty() {
        let mut l_card = section_card(
            &format!("World Book entries ({})", lore_matches.len()),
            None,
            p,
        );
        for (ix, frag) in lore_matches.into_iter().enumerate() {
            let h_jump = on.clone();
            l_card = l_card.child(
                h_flex()
                    .id(("set-sr-lore", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .border_1()
                    .border_color(p.subtle_border())
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_jump(SettingsAction::ClearSearch, w, cx);
                        h_jump(
                            SettingsAction::SelectSection(SettingsSection::WorldBook),
                            w,
                            cx,
                        );
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_medium()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(frag.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(frag.content.chars().take(100).collect::<String>()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_medium()
                            .text_color(p.accent.to_hsla())
                            .child("World Book"),
                    ),
            );
        }
        col = col.child(l_card);
    }

    if !section_matches.is_empty() {
        let mut results_card = section_card("Matching settings", None, p);
        for (ix, (sec, title)) in section_matches.into_iter().enumerate() {
            let h_jump = on.clone();
            results_card = results_card.child(
                h_flex()
                    .id(("set-sr", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .border_1()
                    .border_color(p.subtle_border())
                    .justify_between()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_jump(SettingsAction::ClearSearch, w, cx);
                        h_jump(SettingsAction::SelectSection(sec), w, cx);
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(kiss_icon(sec.icon(), 14.0, p.accent.to_hsla()))
                            .child(
                                v_flex()
                                    .child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_medium()
                                            .text_color(p.primary_ink.to_hsla())
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(p.muted_ink.to_hsla())
                                            .child(sec.label()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_medium()
                            .text_color(p.accent.to_hsla())
                            .child(format!("Open in {}", sec.label())),
                    ),
            );
        }
        col = col.child(results_card);
    }

    col.into_any_element()
}

// ============================================================================================
// Page 1: Preferences (`SettingsPreferencesPage.kt` + `ParchmentProfileSettings.kt` 1:1)
