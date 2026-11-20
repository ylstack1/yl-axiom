//! Main GPUI desktop shell (`ParchmentDesktopApp`) with full Kotlin parity (`docs/design.md`).
//!
//! Coordinates:
//! - Persistent `ChatSession`, MRU-12 tabs, drafts, `DesktopWindowConfig`, and `AppStateSnapshot`
//! - Multi-tab async streaming (`HashMap<String, ActiveRunState>` with abort, `Retry now`,
//!   `Queue`, `Send now` steer, 3.5 s SQLite checkpoints, and one-use `ToolApprovalBroker`)
//! - Dedicated background channel for model fetching, OTA checks, AI titles, summaries, and
//!   translations (never calling `block_on` on the UI thread)
//! - Full wiring of `sidebar`, `prose_view`, `composer`, `vault`, `overlays`, and 8-page `settings_view`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, ExternalPaths, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, Keystroke, KeystrokeEvent, MouseButton,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState, TextareaState};
use gpui_component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::harness::{
    AgentTurnRunner, CardImportResult, HarnessContext, McpRegistry, McpServerStatus,
    PublicWebTransport, RunProgressEvent, ToolAccessScope, ToolApprovalBroker,
    ToolApprovalDecision, ToolApprovalRequest, UserQuestion, WorkspacePathGuard,
    export_card_to_png_bytes, extract_md_skills_from_zip, format_elapsed_whole_seconds,
    import_card_or_prompt_bytes,
};
use parchment_core::model::{
    AgentPermissionPolicy, AgentPermissionResolver, ApiUsageRecord, AppStateSnapshot,
    AssistantAgent, CharacterOrScenarioCard, ChatSession, ChatStub, ContextFragment,
    DesktopWindowConfig, FragmentActivationMode, GoalTrackerState, McpServerConfig,
    MessageNavVisibility, MessageNode, MessageRole, MessageVariant, ModelPricingSpec,
    PinnedMessageStub, PromptAttachment, ProviderProfile, ProviderProtocol, SendShortcutMode,
    ShellAccess, SystemPromptPreset, TokenPricing, ViewMode, WorkspaceMode,
};
use parchment_core::protocol::usage::{estimate_request_cost, spend_toward_limit};
use parchment_core::storage::SearchHit;
use parchment_core::{ParchmentCore, build_streaming_code_fence_payload};
use tokio::sync::{Mutex as AsyncMutex, Notify, mpsc};

use crate::composer::{
    ComposerAction, ComposerActionHandler, ComposerRenderProps, ContextBreakdownInfo, QueuedPrompt,
    RetryBannerInfo, render_composer_deck,
};
use crate::icons::{KissIcon, kiss_icon};
use crate::ota::{OtaStagedFile, UpdateReleaseInfo, check_latest_github_release};
use crate::overlays::{
    ActiveMediaViewerItem, ConfirmDialogKind, OverlayAction, OverlayActionHandler, PaletteEntry,
    command_palette_entries, prompt_jump_targets, render_attachment_viewer_modal,
    render_command_palette, render_confirm_dialog, render_prompt_jump, render_quick_model_picker,
    render_tab_switcher_modal,
};
use crate::prose_view::{
    DisplayMessageRow, MessageAction, MessageActionHandler, TurnRenderContext, TurnUiState,
    render_message_row,
};
use crate::settings_view::{
    AssistantSubpage, InstalledSkillInfo, McpDirectoryItem, ProvidersSubpage, SettingsAction,
    SettingsActionHandler, SettingsInputs, SettingsRenderProps, SettingsSection,
    ShortcutConflictPrompt, StorageFileCategory, StoredFileEntry, WorldBookScope,
    make_default_profile, render_settings_screen,
};
use crate::shortcuts::{
    AppShortcutAction, ShortcutCaptureResult, capture_shortcut_event, find_shortcut_conflict,
    match_shortcut_action, resolve_shortcut, swap_custom_shortcuts, update_custom_shortcut,
};
use crate::sidebar::{
    SidebarAction, SidebarActionHandler, SidebarRenderProps, SidebarTab, render_sidebar,
};
use crate::strings;
use crate::theme::{
    ParchmentPalette, register_custom_font_file, sync_gpui_component_theme, theme_presets,
};
use crate::ui_kit::{
    SepiaButtonStyle, h_divider, icon_button, kiss_bounce, kiss_bounce_subtle, rubric_mark_overlay,
    sepia_avatar_circle, sepia_button, spring_pop_enter, spring_slide_drawer, streaming_pulse_dot,
    strip_markdown_to_plain_text, v_divider, with_tooltip,
};
use crate::vault::{
    NotesAction, NotesActionHandler, VaultAction, VaultActionHandler, VaultFilter,
    count_vault_items, render_chat_notes_panel, render_right_vault_sidebar,
};
use crate::win32::{
    PowerLeaseStatus, Win32ComputerUse, WindowsPowerLease, apply_dwm_chrome_to_active_window,
    open_local_file_or_folder, open_web_link, pick_folder_dialog, pick_open_file_dialog,
    pick_save_file_dialog, speak_text_aloud, start_voice_input, stop_speaking_aloud,
    stop_voice_input,
};

mod actions;
mod chrome;
mod init;
mod init_subs;
mod overlays_keys;
mod settings_core_actions;
mod settings_ext_actions;
mod state;
mod streaming;

use init_subs::*;

const MAX_MRU_TABS: usize = 12;
const MAX_ATTACHMENT_BYTES: usize = 50 * 1024 * 1024;
const MAX_INLINE_TEXT_BYTES: usize = 8 * 1024 * 1024;
const STREAM_CHECKPOINT_INTERVAL: Duration = Duration::from_millis(3500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveOverlay {
    None,
    Settings,
    CommandPalette,
    PromptJump,
    QuickModelPicker,
    TabSwitcher,
    AttachmentViewer,
    ConfirmDialog,
}

enum LiveRunUpdate {
    Progress(RunProgressEvent),
    Finished(Box<Result<(MessageVariant, i64), String>>),
}

enum BgTaskEvent {
    ModelsFetched {
        profile_id: String,
        result: Result<Vec<String>, String>,
    },
    ProviderTested {
        profile_id: String,
        result: Result<String, String>,
    },
    VoiceRecognized(Result<String, String>),
    SpeechFinished(String),
    UpdateChecked(Result<Option<UpdateReleaseInfo>, String>),
    TitleGenerated {
        chat_id: String,
        result: Result<(String, Vec<ApiUsageRecord>), String>,
    },
    SummaryGenerated {
        chat_id: String,
        compact: bool,
        result: Result<(String, Vec<ApiUsageRecord>), String>,
    },
    TranslationGenerated {
        chat_id: String,
        node_id: String,
        result: Result<(String, Vec<ApiUsageRecord>), String>,
    },
    DraftImpersonated(Result<String, String>),
    ModelCatalogSynced(bool),
    McpStatusesUpdated(HashMap<String, McpServerStatus>),
    McpRegistrySearched(Result<Vec<McpDirectoryItem>, String>),
    SkillInstalledFromUrl(Result<String, String>),
}
struct ActiveRunState {
    chat_id: String,
    node_id: String,
    variant_index: usize,
    is_continue: bool,
    continue_prefix: String,
    model_used: String,
    started_at: Instant,
    abort_handle: tokio::task::AbortHandle,
    retry_now: Arc<Notify>,
    broker: Arc<ToolApprovalBroker>,
    rx: mpsc::UnboundedReceiver<LiveRunUpdate>,
    status_line: String,
    retry_banner: Option<String>,
    pending_approvals: Vec<ToolApprovalRequest>,
    pending_questions: Vec<UserQuestion>,
    accumulated_text: String,
    accumulated_thinking: String,
    last_checkpoint_at: Instant,
}

enum NoticeAction {
    None,
    UndoDelete(Box<ChatSession>),
    OpenProviders,
    OpenPreferences,
    OpenModelPicker,
    ShortenConversation,
}

struct FloatingNotice {
    id: u64,
    message: String,
    action_label: Option<&'static str>,
    action: NoticeAction,
}

struct StreamBenchState {
    payload: String,
    cursor: usize,
    active: bool,
    exit_when_done: bool,
    bench_out: Option<PathBuf>,
    started_at: Option<Instant>,
    last_frame_at: Option<Instant>,
    frame_deltas_ms: Vec<f64>,
    cpu_update_ms: Vec<f64>,
    cpu_render_ms: Vec<f64>,
    startup_core_ms: f64,
    last_summary: SharedString,
}

impl StreamBenchState {
    fn new(bench_out: Option<PathBuf>, startup_core_ms: f64) -> Self {
        Self {
            payload: build_streaming_code_fence_payload(),
            cursor: 0,
            active: false,
            exit_when_done: false,
            bench_out,
            started_at: None,
            last_frame_at: None,
            frame_deltas_ms: Vec::with_capacity(512),
            cpu_update_ms: Vec::with_capacity(512),
            cpu_render_ms: Vec::with_capacity(512),
            startup_core_ms,
            last_summary: SharedString::new_static(""),
        }
    }
}

pub struct ParchmentDesktopApp {
    core: Arc<ParchmentCore>,
    data_dir: PathBuf,
    focus_handle: FocusHandle,
    snapshot: AppStateSnapshot,
    palette: ParchmentPalette,
    power_lease: Arc<WindowsPowerLease>,
    mcp_registry: Arc<McpRegistry>,
    _subscriptions: Vec<Subscription>,

    // Navigation & sidebars
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    sidebar_search_input: Entity<InputState>,
    sidebar_chats_scroll: gpui::UniformListScrollHandle,
    stubs: Vec<ChatStub>,
    open_tabs: Vec<String>,
    search_hits: Vec<SearchHit>,
    pinned_stubs: Vec<PinnedMessageStub>,
    pinned_all_chats: bool,
    workspace_entries: Vec<String>,
    custom_slash_commands: Vec<(String, String)>,

    // Right vault & Chat notes
    vault_open: bool,
    vault_filter: VaultFilter,
    notes_open: bool,
    notes_system_input: Entity<TextareaState>,
    notes_authors_input: Entity<TextareaState>,
    notes_summary_input: Entity<TextareaState>,

    // Active chat state & minimap
    active_session: ChatSession,
    is_editing_title: bool,
    title_edit_input: Entity<InputState>,
    messages: Rc<RefCell<Vec<DisplayMessageRow>>>,
    turn_ui_states: HashMap<String, TurnUiState>,
    editing_message_index: Option<usize>,
    edit_msg_input: Entity<TextareaState>,
    scroller: Entity<MessageScrollerState>,
    last_scroll_top: Option<(usize, i32)>,
    minimap_scroll_linger_until: Option<Instant>,
    minimap_open: bool,
    minimap_prompts_only: bool,
    minimap_filter_input: Entity<InputState>,
    question_answer_input: Entity<InputState>,
    speaking_node_id: Option<String>,

    // Composer state
    composer: Entity<TextareaState>,
    prefill_input: Entity<TextareaState>,
    custom_thinking_input: Entity<InputState>,
    pending_attachments: Vec<PromptAttachment>,
    queued_prompts: HashMap<String, Vec<QueuedPrompt>>,
    show_tools_drawer: bool,
    show_inline_tool_registry: bool,
    show_thinking_popover: bool,
    show_modality_popover: bool,
    show_context_popover: bool,
    show_background_usage: bool,
    show_prefill_box: bool,
    goal_mode_enabled: bool,
    pending_guided_direction: Option<String>,
    is_drafting_reply: bool,
    retry_banner_collapsed: bool,
    is_voice_listening: bool,
    overlay: ActiveOverlay,
    overlay_filter_input: Entity<InputState>,
    overlay_selected_index: usize,
    quick_model_provider_filter: Option<String>,
    active_viewer: Option<ActiveMediaViewerItem>,
    viewer_zoom: f32,
    viewer_show_preview: bool,
    viewer_wrap: bool,
    viewer_copied: bool,
    pending_confirm: Option<ConfirmDialogKind>,
    settings_section: SettingsSection,
    providers_subpage: ProvidersSubpage,
    assistant_subpage: AssistantSubpage,
    show_api_key: bool,
    show_add_api_key_box: bool,
    show_add_model_box: bool,
    show_hex_editor: bool,
    world_book_scope: WorldBookScope,
    recording_shortcut: Option<AppShortcutAction>,
    shortcut_feedback: String,
    shortcut_conflict: Option<ShortcutConflictPrompt>,
    storage_files: Vec<StoredFileEntry>,
    storage_category_filter: Option<StorageFileCategory>,
    global_memory_bytes: Option<u64>,
    project_memory_files: Vec<(String, String)>,
    installed_skills: Vec<InstalledSkillInfo>,
    mcp_statuses: HashMap<String, McpServerStatus>,
    mcp_registry_results: Vec<McpDirectoryItem>,
    is_searching_mcp_registry: bool,
    mcp_registry_no_token_only: bool,
    provider_test_status: String,
    settings_inputs: SettingsInputs,
    confirm_wipe_pending: bool,
    // Async multi-tab runs & background utility tasks
    active_runs: HashMap<String, ActiveRunState>,
    bg_tx: mpsc::UnboundedSender<BgTaskEvent>,
    bg_rx: mpsc::UnboundedReceiver<BgTaskEvent>,
    pending_bg_tasks: usize,
    pump_scheduled: bool,

    // Live status, model discovery, OTA & benchmark
    is_fetching_models: bool,
    models_fetch_status: String,
    is_checking_updates: bool,
    latest_release: Option<UpdateReleaseInfo>,
    update_status: String,
    notice: Option<FloatingNotice>,
    cached_context_breakdown: RefCell<Option<(u64, ContextBreakdownInfo, f64)>>,
    stream: StreamBenchState,
}

impl Focusable for ParchmentDesktopApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ParchmentDesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let p = &self.palette;
        let canvas = p.canvas_bg.to_hsla();
        let elev = p.elevated_bg.to_hsla();
        let border = p.subtle_border();
        let ink = p.primary_ink.to_hsla();
        let muted = p.muted_ink.to_hsla();

        // Full-screen Settings replaces top chrome & chat canvas cleanly (matching Kotlin).
        if self.overlay == ActiveOverlay::Settings {
            let entity = cx.entity().downgrade();
            let on_settings: SettingsActionHandler = Rc::new(move |act, w, cx| {
                let _ = entity.update(cx, |this, cx| this.handle_settings_action(act, w, cx));
            });
            let search_q = self.settings_inputs.search.read(cx).value().to_string();
            let model_q = self
                .settings_inputs
                .model_filter
                .read(cx)
                .value()
                .to_string();
            let dm_q = self
                .settings_inputs
                .default_model_filter
                .read(cx)
                .value()
                .to_string();
            let storage_q = self
                .settings_inputs
                .storage_search
                .read(cx)
                .value()
                .to_string();
            let tool_q = self
                .settings_inputs
                .tool_registry_search
                .read(cx)
                .value()
                .to_string();
            let resolved_pol = self.resolved_policy();
            let data_dir_str = self.data_dir.to_string_lossy().to_string();

            let settings_el = render_settings_screen(SettingsRenderProps {
                palette: p,
                section: self.settings_section,
                providers_subpage: &self.providers_subpage,
                assistant_subpage: &self.assistant_subpage,
                show_api_key: self.show_api_key,
                show_add_api_key_box: self.show_add_api_key_box,
                show_add_model_box: self.show_add_model_box,
                show_hex_editor: self.show_hex_editor,
                world_book_scope: self.world_book_scope,
                recording_shortcut: self.recording_shortcut,
                shortcut_feedback: &self.shortcut_feedback,
                shortcut_conflict: self.shortcut_conflict.as_ref(),
                storage_files: &self.storage_files,
                storage_category_filter: self.storage_category_filter,
                storage_search_query: &storage_q,
                global_memory_bytes: self.global_memory_bytes,
                project_memory_files: &self.project_memory_files,
                installed_skills: &self.installed_skills,
                mcp_statuses: &self.mcp_statuses,
                mcp_registry_results: &self.mcp_registry_results,
                is_searching_mcp_registry: self.is_searching_mcp_registry,
                mcp_registry_no_token_only: self.mcp_registry_no_token_only,
                tool_registry_query: &tool_q,
                provider_test_status: &self.provider_test_status,
                snapshot: &self.snapshot,
                active_session: &self.active_session,
                resolved_policy: &resolved_pol,
                inputs: &self.settings_inputs,
                search_query: &search_q,
                model_filter_query: &model_q,
                default_model_filter_query: &dm_q,
                is_fetching_models: self.is_fetching_models,
                models_fetch_status: &self.models_fetch_status,
                is_checking_updates: self.is_checking_updates,
                latest_release: self.latest_release.as_ref(),
                update_status: &self.update_status,
                chat_count: self.stubs.len(),
                data_dir_display: &data_dir_str,
                confirm_wipe_pending: self.confirm_wipe_pending,
                on_action: &on_settings,
            });
            let notice_bar = self.render_notice_bar(cx);
            return div()
                .relative()
                .track_focus(&self.focus_handle)
                .size_full()
                .bg(canvas)
                .text_color(ink)
                .font_family(p.prose_font_family.clone())
                .child(div().size_full().overflow_hidden().child(settings_el))
                .children(notice_bar)
                .into_any_element();
        }

        let profile = self.active_profile();
        let active_model_id = self.effective_model_id(&profile);
        let policy = self.resolved_policy();
        let streaming_ids: Vec<String> = self.active_runs.keys().cloned().collect();
        let is_streaming_active = self.active_runs.contains_key(&self.active_session.id);

        // 1. Left Sidebar (smooth spring width + opacity slide-in/out!)
        let sidebar_el = {
            let entity = cx.entity().downgrade();
            let on_sidebar: SidebarActionHandler = Rc::new(move |act, w, cx| {
                let _ = entity.update(cx, |this, cx| this.handle_sidebar_action(act, w, cx));
            });
            let search_q = self.sidebar_search_input.read(cx).value().to_string();
            spring_slide_drawer(
                "sidebar-slide-anim",
                self.sidebar_open,
                286.0,
                h_flex()
                    .size_full()
                    .child(render_sidebar(SidebarRenderProps {
                        palette: p,
                        selected_tab: self.sidebar_tab,
                        search_input: &self.sidebar_search_input,
                        search_query: &search_q,
                        assistants: &self.snapshot.assistants,
                        active_assistant_id: &self.snapshot.active_assistant_id,
                        stubs: &self.stubs,
                        active_chat_id: &self.active_session.id,
                        streaming_chat_ids: &streaming_ids,
                        search_hits: &self.search_hits,
                        pinned_stubs: &self.pinned_stubs,
                        pinned_all_chats: self.pinned_all_chats,
                        show_dates: self.snapshot.preferences.show_chat_list_dates,
                        chats_scroll: &self.sidebar_chats_scroll,
                        on_action: &on_sidebar,
                    })),
            )
        };
        // 2. Top Editorial Bar + Optional Desktop Browser Tab Bar (full-width above sidebar, 1:1 Kotlin)
        let top_bar = self.render_top_editorial_bar(&profile, cx);
        let tab_bar = self.render_desktop_tab_bar(cx);

        // 3. Optional Chat Notes collapsible drawer
        let notes_panel = self.notes_open.then(|| {
            let entity = cx.entity().downgrade();
            let on_notes: NotesActionHandler = Rc::new(move |act, w, cx| {
                let _ = entity.update(cx, |this, cx| this.handle_notes_action(act, w, cx));
            });
            spring_pop_enter(
                "chat-notes-anim",
                render_chat_notes_panel(
                    &self.notes_system_input,
                    &self.notes_authors_input,
                    &self.notes_summary_input,
                    is_streaming_active,
                    !self.active_session.messages.is_empty(),
                    self.active_session.messages.len() >= 4,
                    p,
                    &on_notes,
                ),
            )
        });

        // 4. Empty chat intro OR Virtualized MessageScroller + viewport overlays
        let is_empty_chat = self.messages.borrow().is_empty();
        let viewport_el: AnyElement = if is_empty_chat {
            self.render_empty_chat_intro(&profile, cx)
        } else {
            let messages_rc = self.messages.clone();
            let view_mode = self.active_session.view_mode;
            let palette_clone = self.palette.clone();
            let prefs_clone = self.snapshot.preferences.clone();
            let user_name = if !self.snapshot.preferences.profile_name.trim().is_empty() {
                self.snapshot.preferences.profile_name.clone()
            } else {
                self.active_session.user_name.clone()
            };
            let user_picture_path = self.snapshot.preferences.profile_picture_path.clone();
            let asst_name = self
                .active_assistant()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| strings::DEFAULT_ASSISTANT.into());
            let provider_name = profile.name.clone();
            let speaking_nid = self.speaking_node_id.clone();
            let highlight_dn = self.snapshot.theme.highlight_dialogue_and_narration;
            let turn_states_clone = self.turn_ui_states.clone();
            let editing_idx = self.editing_message_index;
            let edit_input_clone = self.edit_msg_input.clone();
            let entity_msg = cx.entity().downgrade();
            let on_msg_action: MessageActionHandler = Rc::new(move |act, w, cx| {
                let _ = entity_msg.update(cx, |this, cx| this.handle_message_action(act, w, cx));
            });
            let nav_overlays = self.render_viewport_nav_overlays(cx);

            div()
                .relative()
                .flex_1()
                .w_full()
                .overflow_hidden()
                .child(
                    MessageScroller::new(
                        "manuscript-scroller",
                        self.scroller.clone(),
                        move |ix, _window, _cx| {
                            let guard = messages_rc.borrow();
                            let Some(msg) = guard.get(ix) else {
                                return div().into_any_element();
                            };
                            let default_ui = TurnUiState::default();
                            let ui_state =
                                turn_states_clone.get(&msg.node_id).unwrap_or(&default_ui);
                            let is_editing = editing_idx == Some(ix);
                            let is_speaking_this_turn =
                                speaking_nid.as_deref() == Some(msg.node_id.as_str());
                            let ctx = TurnRenderContext {
                                view_mode,
                                palette: &palette_clone,
                                prefs: &prefs_clone,
                                user_name: &user_name,
                                user_picture_path: &user_picture_path,
                                assistant_name: &asst_name,
                                provider_name: &provider_name,
                                is_speaking_this_turn,
                                highlight_dialogue_and_narration: highlight_dn,
                                ui_state,
                                is_editing,
                                edit_input: is_editing.then_some(&edit_input_clone),
                                on_action: &on_msg_action,
                            };
                            render_message_row(ix, msg, &ctx)
                        },
                    )
                    .with_row_style({
                        let mut s = StyleRefinement::default();
                        s.padding.bottom = Some(px(0.0).into());
                        s.padding.left = Some(px(0.0).into());
                        s.padding.right = Some(px(0.0).into());
                        s
                    })
                    .jump_button(false)
                    .size_full(),
                )
                .children(nav_overlays)
                .into_any_element()
        };

        // 5. Approval/question cards, status strip, and composer deck (with in-card retry banner)
        let banners = self.render_approval_and_retry_banners(cx);
        let draft_text = self.composer.read(cx).value().to_string();
        let (breakdown, session_cost) =
            self.cached_context_breakdown(&profile, &policy, &draft_text);
        let retry_info = self
            .active_runs
            .get(&self.active_session.id)
            .and_then(|r| r.retry_banner.as_ref())
            .map(|raw| {
                let (headline, detail) = match raw.split_once(": ") {
                    Some((h, d)) => (h.to_string(), d.to_string()),
                    None => (raw.clone(), String::new()),
                };
                RetryBannerInfo {
                    headline,
                    detail,
                    collapsed: self.retry_banner_collapsed,
                }
            });
        let empty_q: Vec<QueuedPrompt> = Vec::new();
        let queued_ref = self
            .queued_prompts
            .get(&self.active_session.id)
            .unwrap_or(&empty_q);
        let entity_comp = cx.entity().downgrade();
        let on_comp: ComposerActionHandler = Rc::new(move |act, w, cx| {
            let _ = entity_comp.update(cx, |this, cx| this.handle_composer_action(act, w, cx));
        });
        let live_status_owned: Option<String> = if self.is_drafting_reply {
            Some("Drafting User's turn…".into())
        } else if let Some(run) = self.active_runs.get(&self.active_session.id)
            && run.retry_banner.is_none()
        {
            let elapsed = format_elapsed_whole_seconds(run.started_at.elapsed());
            let awake_note = if self.power_lease.status() == PowerLeaseStatus::Unavailable {
                " · keep-awake unavailable"
            } else {
                ""
            };
            let think_words = run.accumulated_thinking.split_whitespace().count();
            if think_words > 0 && run.accumulated_text.is_empty() {
                Some(format!(
                    "Thinking ({} · {think_words} words · {elapsed}){awake_note}…",
                    run.model_used
                ))
            } else if !run.status_line.is_empty() {
                Some(format!("{} · {elapsed}{awake_note}", run.status_line))
            } else {
                Some(format!("Working ({} · {elapsed})…", run.model_used))
            }
        } else {
            None
        };
        let ws_missing = policy.workspace_mode != WorkspaceMode::Off
            && policy
                .all_workspace_paths()
                .iter()
                .any(|dir| !std::path::Path::new(dir).is_dir());
        let composer_el = render_composer_deck(ComposerRenderProps {
            palette: p,
            composer_input: &self.composer,
            prefill_input: &self.prefill_input,
            custom_thinking_input: &self.custom_thinking_input,
            draft_text: &draft_text,
            active_model_id: &active_model_id,
            profile: &profile,
            policy: &policy,
            has_chat_overrides: self.active_session.permission_overrides.is_some(),
            is_streaming: is_streaming_active,
            is_drafting_reply: self.is_drafting_reply,
            is_voice_listening: self.is_voice_listening,
            live_status_banner: live_status_owned.as_deref(),
            retry_banner: retry_info.as_ref(),
            show_tools_drawer: self.show_tools_drawer,
            show_inline_tool_registry: self.show_inline_tool_registry,
            show_thinking_popover: self.show_thinking_popover,
            show_modality_popover: self.show_modality_popover,
            show_context_popover: self.show_context_popover,
            show_background_usage: self.show_background_usage,
            show_prefill_box: self.show_prefill_box,
            goal_mode_enabled: self.goal_mode_enabled,
            workspace_folder_missing: ws_missing,
            show_token_stats: self.snapshot.preferences.show_token_and_context_stats,
            show_reasoning_on_button: self.snapshot.preferences.show_reasoning_level_on_button,
            pending_attachments: &self.pending_attachments,
            queued_prompts: queued_ref,
            workspace_files: &self.workspace_entries,
            custom_slash_commands: &self.custom_slash_commands,
            breakdown: &breakdown,
            utility_records: &self.active_session.utility_usage_records,
            session_cost_usd: session_cost,
            has_messages: !self.active_session.messages.is_empty(),
            on_action: &on_comp,
        });

        // 6. Optional Right Chat Vault sidebar (smooth spring slide-in/out!)
        let vault_el = {
            let entity = cx.entity().downgrade();
            let on_vault: VaultActionHandler = Rc::new(move |act, w, cx| {
                let _ = entity.update(cx, |this, cx| this.handle_vault_action(act, w, cx));
            });
            spring_slide_drawer(
                "vault-slide-anim",
                self.vault_open,
                301.0,
                h_flex()
                    .size_full()
                    .child(v_divider(p))
                    .child(render_right_vault_sidebar(
                        &self.active_session,
                        &self.pending_attachments,
                        self.vault_filter,
                        p,
                        &on_vault,
                    )),
            )
        };
        // 7. Optional Floating Overlay Modal
        let overlay_el: Option<AnyElement> = match self.overlay {
            ActiveOverlay::None | ActiveOverlay::Settings => None,
            ActiveOverlay::CommandPalette => {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                let q = self.overlay_filter_input.read(cx).value().to_string();
                let can_retry = retry_info.is_some();
                let entries = command_palette_entries(&q, &self.stubs, can_retry, &|cid| {
                    self.shortcut_hints_for_command(cid)
                });
                let sel = self
                    .overlay_selected_index
                    .min(entries.len().saturating_sub(1));
                Some(render_command_palette(
                    &self.overlay_filter_input,
                    &entries,
                    sel,
                    p,
                    &on_ov,
                ))
            }
            ActiveOverlay::PromptJump => {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                let q = self.overlay_filter_input.read(cx).value().to_string();
                let rows = self.messages.borrow();
                let targets = prompt_jump_targets(&q, &rows);
                let sel = self
                    .overlay_selected_index
                    .min(targets.len().saturating_sub(1));
                Some(render_prompt_jump(
                    &self.overlay_filter_input,
                    &targets,
                    sel,
                    p,
                    &on_ov,
                ))
            }
            ActiveOverlay::QuickModelPicker => {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                let q = self.overlay_filter_input.read(cx).value().to_string();
                Some(render_quick_model_picker(
                    &self.overlay_filter_input,
                    &q,
                    &self.snapshot.profiles,
                    &profile.id,
                    &active_model_id,
                    self.quick_model_provider_filter.as_deref(),
                    p,
                    &on_ov,
                ))
            }
            ActiveOverlay::TabSwitcher => {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                let q = self.overlay_filter_input.read(cx).value().to_string();
                Some(render_tab_switcher_modal(
                    &self.overlay_filter_input,
                    &q,
                    &self.open_tabs,
                    &self.active_session.id,
                    &self.stubs,
                    &streaming_ids,
                    p,
                    &on_ov,
                ))
            }
            ActiveOverlay::AttachmentViewer => self.active_viewer.as_ref().map(|item| {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                render_attachment_viewer_modal(
                    item,
                    self.viewer_zoom,
                    self.viewer_show_preview,
                    self.viewer_wrap,
                    self.viewer_copied,
                    p,
                    &on_ov,
                )
            }),
            ActiveOverlay::ConfirmDialog => self.pending_confirm.as_ref().map(|kind| {
                let entity = cx.entity().downgrade();
                let on_ov: OverlayActionHandler = Rc::new(move |act, w, cx| {
                    let _ = entity.update(cx, |this, cx| this.handle_overlay_action(act, w, cx));
                });
                render_confirm_dialog(
                    kind,
                    self.snapshot.preferences.confirm_before_deleting_chat,
                    self.snapshot.preferences.confirm_before_regenerating,
                    p,
                    &on_ov,
                )
            }),
        };

        let notice_bar = self.render_notice_bar(cx);
        let bench_banner = (!self.stream.last_summary.is_empty()).then(|| {
            div()
                .w_full()
                .px(px(16.0))
                .py(px(4.0))
                .bg(elev)
                .border_b_1()
                .border_color(border)
                .font_family(p.code_font_family.clone())
                .text_size(px(11.0))
                .text_color(muted)
                .child(self.stream.last_summary.clone())
        });

        let center_column = v_flex()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .bg(canvas)
            .children(notes_panel)
            .child(viewport_el)
            .children(banners)
            .child(composer_el);

        let any_composer_popover = self.show_thinking_popover
            || self.show_modality_popover
            || self.show_context_popover
            || self.show_tools_drawer;
        let mut main_body = div()
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .child(
                h_flex()
                    .size_full()
                    .child(sidebar_el)
                    .child(center_column)
                    .child(vault_el),
            )
            .children(notice_bar);
        if any_composer_popover {
            main_body = main_body.on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.show_thinking_popover = false;
                    this.show_modality_popover = false;
                    this.show_context_popover = false;
                    this.show_tools_drawer = false;
                    cx.notify();
                }),
            );
        }
        let mut shell = v_flex()
            .size_full()
            .bg(canvas)
            .child(top_bar)
            .child(h_divider(p));
        if let Some(tb) = tab_bar {
            shell = shell.child(tb).child(h_divider(p));
        }
        if let Some(bb) = bench_banner {
            shell = shell.child(bb);
        }
        shell = shell.child(main_body);

        let root_el = div()
            .relative()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(canvas)
            .text_color(ink)
            .font_family(p.prose_font_family.clone())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                for p in paths.paths() {
                    this.attach_file_from_path(p, cx);
                }
            }))
            .child(shell)
            .children(overlay_el)
            .into_any_element();
        if self.stream.active {
            self.stream
                .cpu_render_ms
                .push(render_started.elapsed().as_secs_f64() * 1000.0);
        }
        root_el
    }
}

fn split_shell_compound_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut cur = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                cur.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                cur.push(c);
            }
            ';' if !in_single && !in_double => {
                let t = cur.trim().to_string();
                if !t.is_empty() {
                    segments.push(t);
                }
                cur.clear();
            }
            '&' | '|' if !in_single && !in_double => {
                if chars.peek() == Some(&c) {
                    chars.next();
                }
                let t = cur.trim().to_string();
                if !t.is_empty() {
                    segments.push(t);
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let tail = cur.trim().to_string();
    if !tail.is_empty() {
        segments.push(tail);
    }
    if segments.is_empty() {
        segments.push(command.trim().to_string());
    }
    segments
}

fn is_risky_shell_segment(seg: &str) -> bool {
    let low = seg.trim().to_ascii_lowercase();
    low.starts_with("rm ")
        || low.starts_with("del ")
        || low.starts_with("rmdir ")
        || low.starts_with("remove-item ")
        || low.starts_with("format ")
        || low.contains("git reset --hard")
        || low.contains("git push --force")
        || low.contains("git push -f")
        || low.contains("git clean -f")
        || low.contains("chmod 777")
        || low.contains("dd if=")
}

impl ParchmentDesktopApp {
    /// Floating pill notification matching `FloatingNotification.kt` (`desktop-key-error.png`).
    fn render_notice_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let n = self.notice.as_ref()?;
        let p = &self.palette;
        let hover_bg = p.hover_tint();
        let pill = h_flex()
            .id("floating-notice-pill")
            .max_w(px(560.0))
            .pl(px(16.0))
            .pr(px(4.0))
            .py(px(4.0))
            .rounded(px(12.0))
            .bg(p.elevated_bg.to_hsla())
            .border_1()
            .border_color(p.border.with_alpha(0.75))
            .shadow_lg()
            .items_center()
            .gap(px(10.0))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(kiss_icon(KissIcon::CircleDot, 14.0, p.accent.to_hsla()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.5))
                    .font_medium()
                    .text_color(p.primary_ink.to_hsla())
                    .child(n.message.clone()),
            )
            .children(n.action_label.map(|label| {
                sepia_button(
                    "notice-action-btn",
                    label,
                    None,
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    cx.listener(|this, _, window, cx| this.trigger_notice_action(window, cx)),
                )
            }))
            .child(
                div()
                    .id("dismiss-notice-btn")
                    .w(px(48.0))
                    .h(px(48.0))
                    .rounded(px(999.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    }))
                    .child(kiss_icon(KissIcon::Close, 18.0, p.muted_ink.to_hsla())),
            );

        Some(
            div()
                .absolute()
                .top(px(12.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .px(px(12.0))
                .child(spring_pop_enter("floating-notice-anim", pill))
                .into_any_element(),
        )
    }
}

fn apply_design_defaults_once(snapshot: &mut AppStateSnapshot) {
    if snapshot.design_defaults_applied {
        return;
    }
    snapshot.design_defaults_applied = true;
    for prof in &mut snapshot.profiles {
        if matches!(
            prof.id.as_str(),
            "prof-gemini" | "prof-openai" | "prof-anthropic"
        ) && prof.thinking_effort == "high"
        {
            prof.thinking_effort = "none".into();
        }
    }
    snapshot.preferences.show_user_avatar = false;
    snapshot.preferences.show_user_name = false;
    snapshot.preferences.show_user_timestamp = false;
    snapshot.preferences.show_provider_after_model_name = false;
    snapshot.preferences.show_assistant_avatar_in_title_bar = false;
}

fn default_assistant_agent() -> AssistantAgent {
    AssistantAgent {
        id: "asst-default".into(),
        name: strings::DEFAULT_ASSISTANT.into(),
        icon_emoji: String::new(),
        system_prompt: String::new(),
        authors_note: String::new(),
        authors_note_depth: 2,
        default_profile_id: String::new(),
        default_model_id: String::new(),
        thinking_effort: String::new(),
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        context_message_limit: 0,
        active_card: None,
        fragments: Vec::new(),
        workspace_path: String::new(),
        workspace_mode: WorkspaceMode::Off,
        permission_defaults: None,
        enable_web_search: false,
    }
}

fn sanitize_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        "parchment-chat".into()
    } else {
        trimmed.chars().take(60).collect()
    }
}

fn guess_mime_type(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "txt" | "log" | "csv" | "toml" | "yaml" | "yml" | "xml" | "html" | "css" | "js" | "ts"
        | "rs" | "kt" | "java" | "py" | "c" | "cpp" | "h" | "go" | "sh" | "ps1" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}

fn is_inline_text_mime(mime: &str, ext: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || matches!(
            ext,
            "rs" | "kt"
                | "kts"
                | "java"
                | "py"
                | "js"
                | "ts"
                | "tsx"
                | "jsx"
                | "toml"
                | "yaml"
                | "yml"
                | "md"
                | "txt"
                | "csv"
                | "xml"
                | "html"
                | "css"
                | "sql"
                | "sh"
                | "ps1"
        )
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
