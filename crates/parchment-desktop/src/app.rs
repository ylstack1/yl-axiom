//! Main GPUI desktop shell (`ParchmentDesktopApp`) implementing `docs/DESIGN.md`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::harness::{
    AgentTurnRunner, HarnessContext, PublicWebTransport, RunProgressEvent, ToolAccessScope,
    ToolApprovalBroker, ToolApprovalDecision, ToolApprovalRequest, WorkspacePathGuard,
    format_elapsed_whole_seconds,
};
use parchment_core::model::{
    AgentPermissionPolicy, AppStateSnapshot, ChatSession, ChatStub, GoalTrackerState, MessageRole,
    MessageVariant, PinnedMessageStub, ProviderProtocol, ShellAccess, ToolApprovalMode, ViewMode,
    WorkspaceMode,
};
use parchment_core::storage::SearchHit;
use parchment_core::{ParchmentCore, build_streaming_code_fence_payload};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::ota::{OtaStagedFile, UpdateReleaseInfo, check_latest_github_release};
use crate::prose_view::{DisplayMessageRow, render_message_row};
use crate::settings_view::{
    SettingsSection, cycle_custom_dialect, cycle_thinking_effort, make_default_profile,
    protocol_plain_label,
};
use crate::strings;
use crate::theme::{ParchmentPalette, theme_presets};
use crate::win32::{
    Win32ComputerUse, WindowsPowerLease, apply_dwm_chrome_to_active_window, open_web_link,
    pick_folder_dialog, pick_open_file_dialog, pick_save_file_dialog,
};

const MAX_MRU_TABS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Chats,
    Search,
    Pinned,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveOverlay {
    None,
    Settings,
    CommandPalette,
    PromptJump,
}

enum LiveRunUpdate {
    Progress(RunProgressEvent),
    Finished(Box<Result<(MessageVariant, i64), String>>),
}

struct ActiveRunState {
    chat_id: String,
    started_at: Instant,
    broker: Arc<ToolApprovalBroker>,
    rx: mpsc::UnboundedReceiver<LiveRunUpdate>,
    status_line: String,
    retry_banner: Option<String>,
    pending_approvals: Vec<ToolApprovalRequest>,
    accumulated_text: String,
    accumulated_thinking: String,
}

struct UndoNotice {
    message: String,
    deleted_session: Option<ChatSession>,
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
    web_transport: Arc<PublicWebTransport>,

    // Sidebar & navigation
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    stubs: Vec<ChatStub>,
    open_tabs: Vec<String>,
    active_chat_id: String,
    active_title: String,
    view_mode: ViewMode,
    search_input: Entity<TextareaState>,
    search_hits: Vec<SearchHit>,
    pinned_stubs: Vec<PinnedMessageStub>,
    workspace_entries: Vec<String>,

    // Active viewport & composer
    messages: Rc<RefCell<Vec<DisplayMessageRow>>>,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<TextareaState>,

    // Settings & overlays
    overlay: ActiveOverlay,
    settings_section: SettingsSection,
    api_key_input: Entity<TextareaState>,
    base_url_input: Entity<TextareaState>,
    model_input: Entity<TextareaState>,
    workspace_path: String,
    workspace_mode: WorkspaceMode,
    shell_access: ShellAccess,
    web_access: bool,
    computer_access: bool,
    approval_mode: ToolApprovalMode,

    // Active generation, toast notice, and perf benchmark state
    active_run: Option<ActiveRunState>,
    notice: Option<UndoNotice>,
    latest_release: Option<UpdateReleaseInfo>,
    update_status: String,
    stream: StreamBenchState,
}

impl Focusable for ParchmentDesktopApp {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ParchmentDesktopApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Arc<ParchmentCore>,
        data_dir: PathBuf,
        stubs: Vec<ChatStub>,
        initial_rows: Vec<DisplayMessageRow>,
        startup_core_ms: f64,
        cli_bench_stream: bool,
        bench_out: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut snapshot = core.load_settings_snapshot().unwrap_or_default();
        if snapshot.profiles.is_empty() {
            snapshot.profiles = vec![
                make_default_profile(ProviderProtocol::OpenAiResponses),
                make_default_profile(ProviderProtocol::AnthropicMessages),
                make_default_profile(ProviderProtocol::GeminiInteractions),
            ];
            snapshot.active_profile_id = snapshot.profiles[0].id.clone();
        }
        let palette = ParchmentPalette::from_config(&snapshot.theme);
        apply_dwm_chrome_to_active_window(&palette);

        let (active_chat_id, active_title) = stubs
            .first()
            .map(|s| (s.id.clone(), s.title.clone()))
            .unwrap_or_else(|| ("chat-default".into(), strings::NEW_CHAT.into()));

        window.set_window_title(&strings::window_title_for_chat(&active_title));

        let initial_count = initial_rows.len();
        let scroller = cx.new(|cx| MessageScrollerState::new(initial_count, cx));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Write a message… (Ctrl+K commands · Ctrl+G jump · Ctrl+, settings)")
        });
        let search_input = cx.new(|cx| TextareaState::new(window, cx).placeholder("Search chats…"));

        let active_prof = snapshot
            .profiles
            .iter()
            .find(|p| p.id == snapshot.active_profile_id)
            .or_else(|| snapshot.profiles.first());
        let init_key = active_prof.map(|p| p.api_key.clone()).unwrap_or_default();
        let init_url = active_prof.map(|p| p.base_url.clone()).unwrap_or_default();
        let init_model = active_prof
            .map(|p| p.default_model.clone())
            .unwrap_or_default();

        let api_key_input = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx).placeholder("Paste API key…");
            if !init_key.is_empty() {
                st.set_value(&init_key, window, cx);
            }
            st
        });
        let base_url_input = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx).placeholder("https://api.openai.com");
            if !init_url.is_empty() {
                st.set_value(&init_url, window, cx);
            }
            st
        });
        let model_input = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx).placeholder("Model ID");
            if !init_model.is_empty() {
                st.set_value(&init_model, window, cx);
            }
            st
        });

        let open_tabs = vec![active_chat_id.clone()];
        let sidebar_open = snapshot.preferences.sidebar_open;
        let view_mode = snapshot.preferences.default_view_mode;
        let web_transport = Arc::new(
            PublicWebTransport::new()
                .unwrap_or_else(|_| PublicWebTransport::new_for_local_tests().unwrap()),
        );

        let mut app = Self {
            core,
            data_dir,
            focus_handle: cx.focus_handle(),
            snapshot,
            palette,
            power_lease: Arc::new(WindowsPowerLease::new()),
            web_transport,
            sidebar_open,
            sidebar_tab: SidebarTab::Chats,
            stubs,
            open_tabs,
            active_chat_id,
            active_title,
            view_mode,
            search_input,
            search_hits: Vec::new(),
            pinned_stubs: Vec::new(),
            workspace_entries: Vec::new(),
            messages: Rc::new(RefCell::new(initial_rows)),
            scroller,
            composer,
            overlay: ActiveOverlay::None,
            settings_section: SettingsSection::Providers,
            api_key_input,
            base_url_input,
            model_input,
            workspace_path: String::new(),
            workspace_mode: WorkspaceMode::Off,
            shell_access: ShellAccess::Disabled,
            web_access: false,
            computer_access: false,
            approval_mode: ToolApprovalMode::AskForChanges,
            active_run: None,
            notice: None,
            latest_release: None,
            update_status: String::new(),
            stream: StreamBenchState::new(bench_out, startup_core_ms),
        };

        if cli_bench_stream {
            app.start_stream_benchmark(true, window, cx);
        }

        app
    }

    fn persist_settings(&self) {
        let _ = self.core.save_settings_snapshot(&self.snapshot);
    }

    fn refresh_stubs(&mut self) {
        if let Ok(list) = self.core.list_chat_stubs() {
            self.stubs = list;
        }
    }

    fn push_mru_tab(&mut self, chat_id: &str) {
        self.open_tabs.retain(|id| id != chat_id);
        self.open_tabs.insert(0, chat_id.to_string());
        if self.open_tabs.len() > MAX_MRU_TABS {
            self.open_tabs.truncate(MAX_MRU_TABS);
        }
    }

    fn select_chat_by_id(&mut self, chat_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let title = self
            .stubs
            .iter()
            .find(|s| s.id == chat_id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| strings::NEW_CHAT.into());
        self.active_chat_id = chat_id.to_string();
        self.active_title = title.clone();
        self.push_mru_tab(chat_id);
        window.set_window_title(&strings::window_title_for_chat(&title));

        if let Ok(Some(session)) = self.core.load_full_session(chat_id) {
            let rows: Vec<DisplayMessageRow> = session
                .messages
                .into_iter()
                .map(DisplayMessageRow::from_node)
                .collect();
            let count = rows.len();
            *self.messages.borrow_mut() = rows;
            self.scroller.update(cx, |s, cx| s.reset(count, cx));
        } else if let Ok(nodes) = self.core.load_messages_window(chat_id, 200, 0) {
            let rows: Vec<DisplayMessageRow> = nodes
                .into_iter()
                .map(DisplayMessageRow::from_node)
                .collect();
            let count = rows.len();
            *self.messages.borrow_mut() = rows;
            self.scroller.update(cx, |s, cx| s.reset(count, cx));
        }
        self.overlay = ActiveOverlay::None;
        cx.notify();
    }

    fn create_new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = now_ms();
        let mut session = ChatSession::new(format!("chat-{now}"), strings::NEW_CHAT, now);
        session.profile_id = self.snapshot.active_profile_id.clone();
        let _ = self.core.save_full_session(&session);
        self.refresh_stubs();
        self.select_chat_by_id(&session.id, window, cx);
    }

    fn delete_chat_with_undo(
        &mut self,
        chat_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = self.core.load_full_session(chat_id).ok().flatten();
        let title = saved
            .as_ref()
            .map(|s| s.title.clone())
            .unwrap_or_else(|| chat_id.to_string());
        if self.core.delete_chat(chat_id).unwrap_or(false) {
            self.open_tabs.retain(|t| t != chat_id);
            self.refresh_stubs();
            self.notice = Some(UndoNotice {
                message: strings::deleted_chat_notice(&title),
                deleted_session: saved,
            });
            if self.active_chat_id == chat_id {
                if let Some(next_id) = self.stubs.first().map(|s| s.id.clone()) {
                    self.select_chat_by_id(&next_id, window, cx);
                } else {
                    self.create_new_chat(window, cx);
                }
            } else {
                cx.notify();
            }
        }
    }

    fn undo_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(UndoNotice {
            deleted_session: Some(session),
            ..
        }) = self.notice.take()
        {
            let id = session.id.clone();
            let _ = self.core.save_full_session(&session);
            self.refresh_stubs();
            self.select_chat_by_id(&id, window, cx);
        }
    }

    fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        let cur = self.active_chat_id.clone();
        self.open_tabs.retain(|id| id != &cur);
        if let Some(next) = self.open_tabs.first().cloned() {
            self.select_chat_by_id(&next, window, cx);
        }
    }

    fn cycle_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        if let Some(next) = self.open_tabs.get(1).cloned() {
            self.select_chat_by_id(&next, window, cx);
        }
    }

    fn run_fts_search(&mut self, cx: &mut Context<Self>) {
        let q = self.search_input.read(cx).value().to_string();
        if q.trim().is_empty() {
            self.search_hits.clear();
        } else if let Ok(hits) = self.core.search_messages(&q, 50) {
            self.search_hits = hits;
        }
        cx.notify();
    }

    fn refresh_pinned(&mut self) {
        if let Ok(list) = self.core.list_pinned_message_stubs(None, 60) {
            self.pinned_stubs = list;
        }
    }

    fn refresh_workspace_files(&mut self) {
        if self.workspace_path.trim().is_empty() {
            self.workspace_entries.clear();
            return;
        }
        if let Ok(listing) = WorkspacePathGuard::list_dir(&self.workspace_path, "", false, 120) {
            self.workspace_entries = listing.lines().map(str::to_string).collect();
        }
    }

    fn apply_theme_preset(&mut self, preset_idx: usize, cx: &mut Context<Self>) {
        let presets = theme_presets();
        if let Some(preset) = presets.get(preset_idx) {
            self.snapshot.theme = preset.clone();
            self.palette = ParchmentPalette::from_config(&self.snapshot.theme);
            apply_dwm_chrome_to_active_window(&self.palette);
            self.persist_settings();
            cx.notify();
        }
    }

    fn sync_provider_inputs_from_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prof) = self
            .snapshot
            .profiles
            .iter()
            .find(|p| p.id == self.snapshot.active_profile_id)
            .cloned()
        {
            self.api_key_input
                .update(cx, |i, cx| i.set_value(&prof.api_key, window, cx));
            self.base_url_input
                .update(cx, |i, cx| i.set_value(&prof.base_url, window, cx));
            self.model_input
                .update(cx, |i, cx| i.set_value(&prof.default_model, window, cx));
        }
    }

    fn save_active_provider_fields(&mut self, cx: &mut Context<Self>) {
        let key = self.api_key_input.read(cx).value().trim().to_string();
        let url = self.base_url_input.read(cx).value().trim().to_string();
        let model = self.model_input.read(cx).value().trim().to_string();
        if let Some(prof) = self
            .snapshot
            .profiles
            .iter_mut()
            .find(|p| p.id == self.snapshot.active_profile_id)
        {
            prof.api_key = key;
            prof.base_url = url;
            prof.default_model = model;
        }
        self.persist_settings();
        self.notice = Some(UndoNotice {
            message: "Saved provider settings.".into(),
            deleted_session: None,
        });
        cx.notify();
    }

    fn has_configured_provider(&self) -> bool {
        self.snapshot.profiles.iter().any(|p| {
            !p.api_key.trim().is_empty()
                || (p.protocol == ProviderProtocol::Custom && !p.base_url.trim().is_empty())
        })
    }

    fn send_composer_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).value().to_string();
        let trimmed = draft.trim();
        if trimmed.is_empty() || self.active_run.is_some() {
            return;
        }
        let now = now_ms();
        let msg_id = format!("user-{now}");
        let _ = self.core.append_message(
            &self.active_chat_id,
            msg_id,
            MessageRole::User,
            trimmed.to_string(),
            now,
        );
        self.messages
            .borrow_mut()
            .push(DisplayMessageRow::user(trimmed.to_string()));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });
        self.composer
            .update(cx, |input, cx| input.set_value("", window, cx));

        // If a provider is configured with credentials/URL, launch real streaming `AgentTurnRunner`!
        let active_prof = self
            .snapshot
            .profiles
            .iter()
            .find(|p| p.id == self.snapshot.active_profile_id)
            .or_else(|| self.snapshot.profiles.first())
            .cloned();

        if let Some(profile) = active_prof
            && (!profile.api_key.trim().is_empty()
                || (profile.protocol == ProviderProtocol::Custom
                    && !profile.base_url.trim().is_empty()))
        {
            self.start_live_generation(profile, window, cx);
        } else {
            self.refresh_stubs();
            cx.notify();
        }
    }

    fn start_live_generation(
        &mut self,
        profile: parchment_core::model::ProviderProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = profile.default_model.trim().to_string();
        if model.is_empty() {
            self.notice = Some(UndoNotice {
                message: "Enter or select a model ID in Settings before sending.".into(),
                deleted_session: None,
            });
            cx.notify();
            return;
        }
        let chat_id = self.active_chat_id.clone();
        let session = self
            .core
            .load_full_session(&chat_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| ChatSession::new(&chat_id, &self.active_title, now_ms()));
        let history = session.messages.clone();
        let run_id = format!("run-{}", now_ms());

        let policy = AgentPermissionPolicy {
            approval_mode: self.approval_mode,
            workspace_path: self.workspace_path.clone(),
            workspace_mode: self.workspace_mode,
            tools_enabled: true,
            shell_access: self.shell_access,
            web_access: self.web_access,
            computer_access: self.computer_access,
            ..Default::default()
        };
        let broker = ToolApprovalBroker::new(&run_id, &chat_id, ToolAccessScope::new(policy));
        let (tx, rx) = mpsc::unbounded_channel();

        // Append live placeholder assistant turn to viewport.
        self.messages
            .borrow_mut()
            .push(DisplayMessageRow::assistant_placeholder(
                SharedString::from(model.clone()),
            ));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });

        self.power_lease.acquire();
        let lease_bg = self.power_lease.clone();
        let core_bg = self.core.clone();
        let web_bg = self.web_transport.clone();
        let storage_root = self.data_dir.clone();
        let spend_limit = self.snapshot.preferences.chat_spend_limit_usd;
        let broker_bg = broker.clone();

        self.core.runtime().spawn(async move {
            let protocol = core_bg.protocol_client();
            let hctx = HarnessContext {
                chat_id: session.id.clone(),
                storage_root,
                broker: broker_bg,
                web: web_bg,
                mcp: None,
                computer_use: Some(Arc::new(Win32ComputerUse)),
                subagent_delegate: None,
                goal_state: Arc::new(AsyncMutex::new(GoalTrackerState::default())),
                expanded_mcp_tools: Arc::new(AsyncMutex::new(HashSet::new())),
                computer_use_settle_ms: 150,
            };
            let res = AgentTurnRunner::run_turn(
                &protocol,
                &profile,
                &model,
                &session,
                &history,
                &hctx,
                &run_id,
                spend_limit,
                0.0,
                None,
                |ev| {
                    let _ = tx.send(LiveRunUpdate::Progress(ev));
                },
            )
            .await;
            lease_bg.release();
            match res {
                Ok(outcome) => {
                    let _ = tx.send(LiveRunUpdate::Finished(Box::new(Ok((
                        outcome.variant,
                        outcome.elapsed_ms,
                    )))));
                }
                Err(err) => {
                    let _ = tx.send(LiveRunUpdate::Finished(Box::new(Err(err.to_string()))));
                }
            }
        });

        self.active_run = Some(ActiveRunState {
            chat_id,
            started_at: Instant::now(),
            broker,
            rx,
            status_line: "Connecting…".into(),
            retry_banner: None,
            pending_approvals: Vec::new(),
            accumulated_text: String::new(),
            accumulated_thinking: String::new(),
        });
        self.schedule_live_run_tick(window, cx);
        cx.notify();
    }

    fn schedule_live_run_tick(&self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                this.poll_live_run(window, cx);
            });
        });
    }

    fn poll_live_run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut finished: Option<Result<(MessageVariant, i64), String>> = None;
        let mut dirty = false;

        if let Some(run) = self.active_run.as_mut() {
            while let Ok(msg) = run.rx.try_recv() {
                dirty = true;
                match msg {
                    LiveRunUpdate::Progress(ev) => match ev {
                        RunProgressEvent::Status(s) => {
                            run.status_line = s;
                            run.retry_banner = None;
                        }
                        RunProgressEvent::TextDelta(d) => {
                            run.accumulated_text.push_str(&d);
                            run.retry_banner = None;
                        }
                        RunProgressEvent::ThinkingDelta(d) => {
                            run.accumulated_thinking.push_str(&d);
                        }
                        RunProgressEvent::RetryWait {
                            attempt,
                            wait_ms,
                            reason,
                        } => {
                            run.retry_banner = Some(format!(
                                "Trying again (attempt {attempt}, {}s): {reason}",
                                wait_ms.div_ceil(1000)
                            ));
                        }
                        RunProgressEvent::ToolStarted { name, .. } => {
                            run.status_line = format!("Running {name}…");
                        }
                        RunProgressEvent::ToolFinished { name, .. } => {
                            run.status_line = format!("Finished {name}");
                        }
                    },
                    LiveRunUpdate::Finished(res) => {
                        finished = Some(*res);
                        break;
                    }
                }
            }

            if let Some(pending) = run.broker.try_pending_requests()
                && pending.len() != run.pending_approvals.len()
            {
                run.pending_approvals = pending;
                dirty = true;
            }

            let elapsed = format_elapsed_whole_seconds(run.started_at.elapsed());
            window.set_window_title(&format!("Generating ({elapsed}) — {}", strings::APP_NAME));

            if dirty {
                let last_ix = {
                    let mut rows = self.messages.borrow_mut();
                    if let Some(last) = rows.last_mut() {
                        last.content = SharedString::from(run.accumulated_text.clone());
                        last.set_streaming_thinking(&run.accumulated_thinking);
                    }
                    rows.len().saturating_sub(1)
                };
                self.scroller.update(cx, |s, cx| {
                    s.remeasure_items(last_ix..last_ix + 1, cx);
                });
            }
        }

        if let Some(done) = finished {
            let chat_id = self
                .active_run
                .take()
                .map(|r| r.chat_id)
                .unwrap_or_else(|| self.active_chat_id.clone());
            window.set_window_title(&strings::window_title_for_chat(&self.active_title));
            match done {
                Ok((variant, _elapsed_ms)) => {
                    let _ = self.core.append_message(
                        &chat_id,
                        format!("asst-{}", now_ms()),
                        MessageRole::Assistant,
                        variant.content.clone(),
                        now_ms(),
                    );
                    if let Some(last) = self.messages.borrow_mut().last_mut() {
                        *last = DisplayMessageRow::from_variant(false, last.pinned, variant);
                    }
                    self.refresh_stubs();
                }
                Err(err_msg) => {
                    if let Some(last) = self.messages.borrow_mut().last_mut() {
                        last.content = SharedString::from(format!("**Error:** {err_msg}"));
                    }
                }
            }
            cx.notify();
            return;
        }

        if self.active_run.is_some() {
            if dirty {
                cx.notify();
            }
            self.schedule_live_run_tick(window, cx);
        }
    }

    fn stop_active_run(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.active_run.as_ref() {
            let broker = run.broker.clone();
            self.core.runtime().spawn(async move {
                broker.close().await;
            });
            cx.notify();
        }
    }

    fn respond_approval(
        &mut self,
        request_id: String,
        decision: ToolApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        if let Some(run) = self.active_run.as_ref() {
            let broker = run.broker.clone();
            self.core.runtime().spawn(async move {
                broker.respond(&request_id, decision).await;
            });
            cx.notify();
        }
    }

    fn handle_global_keydown(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ctrl = event.keystroke.modifiers.control;
        let key = event.keystroke.key.as_str();
        if ctrl {
            match key {
                "k" => {
                    self.overlay = if self.overlay == ActiveOverlay::CommandPalette {
                        ActiveOverlay::None
                    } else {
                        ActiveOverlay::CommandPalette
                    };
                    cx.notify();
                }
                "g" => {
                    self.overlay = if self.overlay == ActiveOverlay::PromptJump {
                        ActiveOverlay::None
                    } else {
                        ActiveOverlay::PromptJump
                    };
                    cx.notify();
                }
                "n" => self.create_new_chat(window, cx),
                "w" => self.close_active_tab(window, cx),
                "tab" => self.cycle_tabs(window, cx),
                "," => {
                    self.overlay = if self.overlay == ActiveOverlay::Settings {
                        ActiveOverlay::None
                    } else {
                        self.sync_provider_inputs_from_active(window, cx);
                        ActiveOverlay::Settings
                    };
                    cx.notify();
                }
                "b" => {
                    self.sidebar_open = !self.sidebar_open;
                    self.snapshot.preferences.sidebar_open = self.sidebar_open;
                    self.persist_settings();
                    cx.notify();
                }
                _ => {}
            }
        } else if key == "escape" && self.overlay != ActiveOverlay::None {
            self.overlay = ActiveOverlay::None;
            cx.notify();
        }
    }

    // ---- Phase 0 / Phase 4 quantitative streaming benchmark (preserved for `scripts/perf_v3_spike.py`) ----

    fn start_stream_benchmark(
        &mut self,
        exit_when_done: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stream.active {
            return;
        }
        self.stream.cursor = 0;
        self.stream.active = true;
        self.stream.exit_when_done = exit_when_done;
        self.stream.started_at = Some(Instant::now());
        self.stream.last_frame_at = None;
        self.stream.frame_deltas_ms.clear();
        self.stream.cpu_update_ms.clear();
        self.stream.cpu_render_ms.clear();
        self.stream.last_summary = SharedString::from("Streaming 87 KB code fence…");

        self.messages
            .borrow_mut()
            .push(DisplayMessageRow::assistant_placeholder(
                SharedString::new_static("benchmark"),
            ));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });
        self.schedule_stream_tick(window, cx);
        cx.notify();
    }

    fn schedule_stream_tick(&self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                this.advance_stream_frame(window, cx);
            });
        });
    }

    fn advance_stream_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.stream.active {
            return;
        }
        let frame_start = Instant::now();
        if let Some(prev) = self.stream.last_frame_at {
            self.stream
                .frame_deltas_ms
                .push(prev.elapsed().as_secs_f64() * 1000.0);
        }
        self.stream.last_frame_at = Some(frame_start);

        let total = self.stream.payload.len();
        let mut next_cursor = (self.stream.cursor + 360).min(total);
        while next_cursor < total && !self.stream.payload.is_char_boundary(next_cursor) {
            next_cursor += 1;
        }
        self.stream.cursor = next_cursor;

        let last_idx = {
            let mut rows = self.messages.borrow_mut();
            let last_idx = rows.len().saturating_sub(1);
            if let Some(last) = rows.get_mut(last_idx) {
                last.content = SharedString::from(self.stream.payload[..next_cursor].to_string());
            }
            last_idx
        };
        self.scroller.update(cx, |s, cx| {
            s.remeasure_items(last_idx..last_idx + 1, cx);
        });
        self.stream
            .cpu_update_ms
            .push(frame_start.elapsed().as_secs_f64() * 1000.0);

        if next_cursor >= total {
            self.finish_stream_benchmark(cx);
        } else {
            self.schedule_stream_tick(window, cx);
        }
        cx.notify();
    }

    fn finish_stream_benchmark(&mut self, cx: &mut Context<Self>) {
        self.stream.active = false;
        let total_ms = self
            .stream
            .started_at
            .map(|t| t.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let mut deltas = self.stream.frame_deltas_ms.clone();
        deltas.sort_by(|a, b| a.total_cmp(b));
        let mut cpu_upd = self.stream.cpu_update_ms.clone();
        cpu_upd.sort_by(|a, b| a.total_cmp(b));
        let mut cpu_ren = self.stream.cpu_render_ms.clone();
        cpu_ren.sort_by(|a, b| a.total_cmp(b));
        let frames = deltas.len();
        let vsync_p50 = percentile(&deltas, 0.50);
        let vsync_p95 = percentile(&deltas, 0.95);
        let vsync_max = deltas.last().copied().unwrap_or(0.0);
        let cpu_p50 = percentile(&cpu_upd, 0.50) + percentile(&cpu_ren, 0.50);
        let cpu_p95 = percentile(&cpu_upd, 0.95) + percentile(&cpu_ren, 0.95);
        let payload_len = self.stream.payload.len();
        let core_ms = self.stream.startup_core_ms;

        let report = format!(
            "stream_bench chars={payload_len} frames={frames} total_ms={total_ms:.1} vsync_p50_ms={vsync_p50:.2} vsync_p95_ms={vsync_p95:.2} vsync_max_ms={vsync_max:.2} cpu_p50_ms={cpu_p50:.3} cpu_p95_ms={cpu_p95:.3} core_startup_ms={core_ms:.2}"
        );
        self.stream.last_summary = SharedString::from(report.clone());
        if let Some(out_path) = &self.stream.bench_out {
            let _ = std::fs::write(out_path, format!("{report}\n"));
        }
        if self.stream.exit_when_done {
            cx.quit();
        }
    }

    // ---- Rendering helpers ----

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let sheet = self.palette.sheet_bg.to_gpui_rgb();
        let card = self.palette.card_bg.to_gpui_rgb();
        let border = self.palette.border.to_gpui_rgb();
        let ink = self.palette.primary_ink.to_gpui_rgb();
        let muted = self.palette.muted_ink.to_gpui_rgb();
        let accent = self.palette.accent.to_gpui_rgb();
        let danger = self.palette.danger.to_gpui_rgb();

        let tab_btn = |id: &'static str, label: &'static str, target: SidebarTab, active: bool| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded(px(6.))
                .cursor_pointer()
                .bg(if active { card } else { sheet })
                .text_xs()
                .font_semibold()
                .text_color(if active { ink } else { muted })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.sidebar_tab = target;
                    match target {
                        SidebarTab::Pinned => this.refresh_pinned(),
                        SidebarTab::Files => this.refresh_workspace_files(),
                        _ => {}
                    }
                    cx.notify();
                }))
                .child(label)
        };

        let body: AnyElement =
            match self.sidebar_tab {
                SidebarTab::Chats => {
                    let items: Vec<AnyElement> =
                        self.stubs
                            .iter()
                            .take(50)
                            .enumerate()
                            .map(|(ix, stub)| {
                                let cid = stub.id.clone();
                                let cid_del = stub.id.clone();
                                let is_active = stub.id == self.active_chat_id;
                                h_flex()
                                    .id(("chat-row", ix))
                                    .w_full()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .justify_between()
                                    .items_center()
                                    .bg(if is_active { card } else { sheet })
                                    .border_l_2()
                                    .border_color(if is_active { accent } else { sheet })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_chat_by_id(&cid, window, cx);
                                    }))
                                    .child(
                                        div().flex_1().text_sm().text_color(ink).truncate().child(
                                            format!("{} ({})", stub.title, stub.message_count),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .id(("chat-del", ix))
                                            .px_1()
                                            .text_xs()
                                            .text_color(danger)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.delete_chat_with_undo(&cid_del, window, cx);
                                            }))
                                            .child("×"),
                                    )
                                    .into_any_element()
                            })
                            .collect();
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .overflow_hidden()
                        .children(items)
                        .into_any_element()
                }
                SidebarTab::Search => {
                    let hits: Vec<AnyElement> = self
                        .search_hits
                        .iter()
                        .enumerate()
                        .map(|(ix, hit)| {
                            let cid = hit.chat_id.clone();
                            v_flex()
                                .id(("hit-row", ix))
                                .px_2()
                                .py_1()
                                .rounded(px(6.))
                                .bg(card)
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_chat_by_id(&cid, window, cx);
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(ink)
                                        .child(hit.chat_title.clone()),
                                )
                                .child(div().text_xs().text_color(muted).child(hit.snippet.clone()))
                                .into_any_element()
                        })
                        .collect();
                    v_flex()
                        .flex_1()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Textarea::new(&self.search_input).h(px(34.))),
                                )
                                .child(Button::new("fts-go").label("Go").on_click(
                                    cx.listener(|this, _, _, cx| this.run_fts_search(cx)),
                                )),
                        )
                        .children(hits)
                        .into_any_element()
                }
                SidebarTab::Pinned => {
                    if self.pinned_stubs.is_empty() {
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("No pinned replies yet.")
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_1()
                            .children(self.pinned_stubs.iter().enumerate().map(|(ix, p)| {
                                let cid = p.chat_id.clone();
                                v_flex()
                                    .id(("pin-row", ix))
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(card)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_chat_by_id(&cid, window, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(ink)
                                            .child(p.chat_title.clone()),
                                    )
                                    .child(
                                        div().text_xs().text_color(muted).child(p.preview.clone()),
                                    )
                            }))
                            .into_any_element()
                    }
                }
                SidebarTab::Files => {
                    let mut col = v_flex().gap_2().child(
                        Button::new("choose-folder-side")
                            .label(strings::CHOOSE_FOLDER)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(dir) = pick_folder_dialog("Choose workspace folder") {
                                    this.workspace_path = dir.to_string_lossy().into_owned();
                                    if this.workspace_mode == WorkspaceMode::Off {
                                        this.workspace_mode = WorkspaceMode::ReadOnly;
                                    }
                                    this.refresh_workspace_files();
                                    cx.notify();
                                }
                            })),
                    );
                    if self.workspace_entries.is_empty() {
                        col =
                            col.child(div().text_xs().text_color(muted).child("No folder chosen."));
                    } else {
                        for entry in self.workspace_entries.iter().take(40) {
                            col = col.child(div().text_xs().text_color(ink).child(entry.clone()));
                        }
                    }
                    col.into_any_element()
                }
            };

        v_flex()
            .w(px(248.))
            .h_full()
            .bg(sheet)
            .border_r_1()
            .border_color(border)
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(ink)
                            .child(format!("{} ({})", strings::APP_NAME, self.stubs.len())),
                    )
                    .child(
                        Button::new("new-chat-btn")
                            .primary()
                            .label(strings::NEW_CHAT)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_new_chat(window, cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(tab_btn(
                        "tab-chats",
                        strings::SIDEBAR_CHATS,
                        SidebarTab::Chats,
                        self.sidebar_tab == SidebarTab::Chats,
                    ))
                    .child(tab_btn(
                        "tab-search",
                        strings::SIDEBAR_SEARCH,
                        SidebarTab::Search,
                        self.sidebar_tab == SidebarTab::Search,
                    ))
                    .child(tab_btn(
                        "tab-pinned",
                        strings::SIDEBAR_PINNED,
                        SidebarTab::Pinned,
                        self.sidebar_tab == SidebarTab::Pinned,
                    ))
                    .child(tab_btn(
                        "tab-files",
                        strings::SIDEBAR_FILES,
                        SidebarTab::Files,
                        self.sidebar_tab == SidebarTab::Files,
                    )),
            )
            .child(body)
            .into_any_element()
    }

    fn render_settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let canvas = self.palette.canvas_bg.to_gpui_rgb();
        let sheet = self.palette.sheet_bg.to_gpui_rgb();
        let card = self.palette.card_bg.to_gpui_rgb();
        let border = self.palette.border.to_gpui_rgb();
        let ink = self.palette.primary_ink.to_gpui_rgb();
        let muted = self.palette.muted_ink.to_gpui_rgb();
        let accent = self.palette.accent.to_gpui_rgb();

        let nav = v_flex()
            .w(px(210.))
            .h_full()
            .bg(sheet)
            .border_r_1()
            .border_color(border)
            .p_3()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(ink)
                    .mb_2()
                    .child(strings::SETTINGS_TITLE),
            )
            .children(
                SettingsSection::ALL
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(ix, sec)| {
                        let active = sec == self.settings_section;
                        div()
                            .id(("set-nav", ix))
                            .px_3()
                            .py_2()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .bg(if active { card } else { sheet })
                            .border_l_2()
                            .border_color(if active { accent } else { sheet })
                            .text_sm()
                            .text_color(ink)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings_section = sec;
                                cx.notify();
                            }))
                            .child(sec.label())
                    }),
            );

        let detail: AnyElement = match self.settings_section {
            SettingsSection::Providers => {
                let active_prof = self
                    .snapshot
                    .profiles
                    .iter()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                    .or_else(|| self.snapshot.profiles.first());
                let cur_proto = active_prof
                    .map(|p| protocol_plain_label(p.protocol, &p.custom_dialect))
                    .unwrap_or("None");
                let cur_effort = active_prof
                    .map(|p| p.thinking_effort.clone())
                    .unwrap_or_else(|| "none".into());
                let is_custom = active_prof.is_some_and(|p| p.protocol == ProviderProtocol::Custom);

                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_semibold()
                            .text_color(ink)
                            .child("Providers & models"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .children(self.snapshot.profiles.iter().enumerate().map(|(ix, p)| {
                                let pid = p.id.clone();
                                let sel = p.id == self.snapshot.active_profile_id;
                                div()
                                    .id(("prof-pill", ix))
                                    .px_3()
                                    .py_1()
                                    .rounded(px(8.))
                                    .cursor_pointer()
                                    .bg(if sel { card } else { sheet })
                                    .border_1()
                                    .border_color(if sel { accent } else { border })
                                    .text_sm()
                                    .text_color(ink)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.snapshot.active_profile_id = pid.clone();
                                        this.sync_provider_inputs_from_active(window, cx);
                                        this.persist_settings();
                                        cx.notify();
                                    }))
                                    .child(p.name.clone())
                            }))
                            .child(Button::new("add-custom-prof").label("+ Custom").on_click(
                                cx.listener(|this, _, window, cx| {
                                    let mut p = make_default_profile(ProviderProtocol::Custom);
                                    p.id = format!("prof-custom-{}", now_ms());
                                    let pid = p.id.clone();
                                    this.snapshot.profiles.push(p);
                                    this.snapshot.active_profile_id = pid;
                                    this.sync_provider_inputs_from_active(window, cx);
                                    this.persist_settings();
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("{}: {cur_proto}", strings::API_FORMAT_LABEL)),
                            )
                            .children(is_custom.then(|| {
                                Button::new("cycle-custom-fmt")
                                    .label("Cycle API format")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = this
                                            .snapshot
                                            .profiles
                                            .iter_mut()
                                            .find(|p| p.id == this.snapshot.active_profile_id)
                                        {
                                            p.custom_dialect =
                                                cycle_custom_dialect(&p.custom_dialect).to_string();
                                        }
                                        this.persist_settings();
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(div().text_xs().text_color(muted).child("API key"))
                    .child(Textarea::new(&self.api_key_input).h(px(36.)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(strings::SERVER_URL_LABEL),
                    )
                    .child(Textarea::new(&self.base_url_input).h(px(36.)))
                    .child(div().text_xs().text_color(muted).child("Default model"))
                    .child(Textarea::new(&self.model_input).h(px(36.)))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("cycle-thinking")
                                    .label(format!("{}: {cur_effort}", strings::THINKING_LABEL))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = this
                                            .snapshot
                                            .profiles
                                            .iter_mut()
                                            .find(|p| p.id == this.snapshot.active_profile_id)
                                        {
                                            p.thinking_effort =
                                                cycle_thinking_effort(&p.thinking_effort)
                                                    .to_string();
                                        }
                                        this.persist_settings();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("save-prof-btn")
                                    .primary()
                                    .label("Save provider")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_active_provider_fields(cx);
                                    })),
                            ),
                    )
                    .into_any_element()
            }
            SettingsSection::Workspace => v_flex()
                .gap_3()
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .text_color(ink)
                        .child("Workspace & permissions"),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("pick-ws-btn")
                                .primary()
                                .label(strings::CHOOSE_FOLDER)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(dir) = pick_folder_dialog("Choose workspace folder")
                                    {
                                        this.workspace_path = dir.to_string_lossy().into_owned();
                                        if this.workspace_mode == WorkspaceMode::Off {
                                            this.workspace_mode = WorkspaceMode::ReadOnly;
                                        }
                                        this.refresh_workspace_files();
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(div().text_xs().text_color(muted).child(
                            if self.workspace_path.is_empty() {
                                "No folder mounted".to_string()
                            } else {
                                self.workspace_path.clone()
                            },
                        )),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("toggle-ws-mode")
                                .label(format!("Folder access: {:?}", self.workspace_mode))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace_mode = match this.workspace_mode {
                                        WorkspaceMode::Off => WorkspaceMode::ReadOnly,
                                        WorkspaceMode::ReadOnly => WorkspaceMode::ReadWrite,
                                        WorkspaceMode::ReadWrite => WorkspaceMode::Off,
                                    };
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("toggle-shell")
                                .label(format!("Shell access: {:?}", self.shell_access))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shell_access = match this.shell_access {
                                        ShellAccess::Disabled => ShellAccess::FullAccess,
                                        ShellAccess::FullAccess => ShellAccess::Disabled,
                                    };
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("toggle-web")
                                .label(format!(
                                    "Web access: {}",
                                    if self.web_access { "On" } else { "Off" }
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.web_access = !this.web_access;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("toggle-cu")
                                .label(format!(
                                    "Computer control: {}",
                                    if self.computer_access { "On" } else { "Off" }
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.computer_access = !this.computer_access;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    Button::new("toggle-approval-mode")
                        .label(format!("Approval policy: {:?}", self.approval_mode))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.approval_mode = match this.approval_mode {
                                ToolApprovalMode::AskForChanges => ToolApprovalMode::AskEveryTool,
                                ToolApprovalMode::AskEveryTool => ToolApprovalMode::Never,
                                ToolApprovalMode::Never => ToolApprovalMode::AskForChanges,
                            };
                            cx.notify();
                        })),
                )
                .into_any_element(),
            SettingsSection::Appearance => {
                let presets = theme_presets();
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_base()
                            .font_semibold()
                            .text_color(ink)
                            .child("Appearance & 9 themes"),
                    )
                    .children(presets.iter().enumerate().map(|(ix, preset)| {
                        let pal = ParchmentPalette::from_config(preset);
                        let is_selected = preset.name == self.snapshot.theme.name;
                        h_flex()
                            .id(("theme-preset", ix))
                            .px_3()
                            .py_2()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .justify_between()
                            .items_center()
                            .bg(pal.canvas_bg.to_gpui_rgb())
                            .border_1()
                            .border_color(if is_selected {
                                accent
                            } else {
                                pal.border.to_gpui_rgb()
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_theme_preset(ix, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(pal.primary_ink.to_gpui_rgb())
                                    .child(preset.name.clone()),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(pal.accent.to_gpui_rgb())
                                    .text_xs()
                                    .text_color(pal.on_accent_ink.to_gpui_rgb())
                                    .child(if is_selected { "Active" } else { "Apply" }),
                            )
                    }))
                    .into_any_element()
            }
            SettingsSection::Shortcuts => v_flex()
                .gap_2()
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .text_color(ink)
                        .child("Keyboard shortcuts"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+K — Open command palette"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+G — Jump to a prompt in this chat"),
                )
                .child(div().text_sm().text_color(ink).child("Ctrl+N — New chat"))
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+W — Close current tab"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+Tab — Switch between recent tabs"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+, — Open or close Settings"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Ctrl+B — Show or hide sidebar"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(ink)
                        .child("Escape — Close overlay"),
                )
                .into_any_element(),
            SettingsSection::BackupAndUpdates => v_flex()
                .gap_3()
                .child(
                    div()
                        .text_base()
                        .font_semibold()
                        .text_color(ink)
                        .child("Backup, migration & updates"),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("export-archive-btn")
                                .label(strings::EXPORT_ARCHIVE)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(dest) = pick_save_file_dialog(
                                        "Export Parchment archive",
                                        "parchment-backup.parchment",
                                        "parchment",
                                    ) {
                                        let ids: Vec<String> =
                                            this.stubs.iter().map(|s| s.id.clone()).collect();
                                        let msg = match this.core.export_sessions_archive(
                                            &dest,
                                            &ids,
                                            &this.data_dir,
                                        ) {
                                            Ok(()) => format!("Exported {} chats.", ids.len()),
                                            Err(e) => format!("Export failed: {e}"),
                                        };
                                        this.notice = Some(UndoNotice {
                                            message: msg,
                                            deleted_session: None,
                                        });
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new("import-archive-btn")
                                .label(strings::IMPORT_ARCHIVE)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if let Some(src) = pick_open_file_dialog(
                                        "Import Parchment archive or JSON",
                                        "Parchment Archives (*.parchment;*.zip;*.json)",
                                        "*.parchment;*.zip;*.json",
                                    ) && let Ok(bytes) = std::fs::read(&src)
                                    {
                                        let fname = src
                                            .file_name()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("import.parchment");
                                        match this.core.import_archive_bytes(
                                            fname,
                                            &bytes,
                                            &this.data_dir,
                                        ) {
                                            Ok(sessions) => {
                                                let count = sessions.len();
                                                this.refresh_stubs();
                                                if let Some(first) = sessions.first() {
                                                    this.select_chat_by_id(&first.id, window, cx);
                                                }
                                                this.notice = Some(UndoNotice {
                                                    message: format!("Imported {count} chat(s)."),
                                                    deleted_session: None,
                                                });
                                            }
                                            Err(e) => {
                                                this.notice = Some(UndoNotice {
                                                    message: format!("Import failed: {e}"),
                                                    deleted_session: None,
                                                });
                                            }
                                        }
                                        cx.notify();
                                    }
                                })),
                        )
                        .child(
                            Button::new("migrate-v2-btn")
                                .label(strings::MIGRATE_V2)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(v2_dir) =
                                        pick_folder_dialog("Select Parchment v2 data folder")
                                    {
                                        let msg =
                                            match this.core.migrate_v2_data_dir(&v2_dir, false) {
                                                Ok(rep) => format!(
                                                    "Migrated {} chats ({} live sidecars folded).",
                                                    rep.chats_migrated, rep.live_sidecars_folded
                                                ),
                                                Err(e) => format!("Migration failed: {e}"),
                                            };
                                        this.refresh_stubs();
                                        this.notice = Some(UndoNotice {
                                            message: msg,
                                            deleted_session: None,
                                        });
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("check-update-btn")
                                .label(strings::CHECK_FOR_UPDATES)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let http = this.core.http_client().clone();
                                    let repo = this.snapshot.ota_github_repo.clone();
                                    let res = this.core.runtime().block_on(async move {
                                        check_latest_github_release(&http, &repo, "3.0.0").await
                                    });
                                    this.update_status = match res {
                                        Ok(Some(info)) => {
                                            let summary = format!(
                                                "Update available: {} ({})",
                                                info.tag_name, info.asset_name
                                            );
                                            this.latest_release = Some(info);
                                            summary
                                        }
                                        Ok(None) => {
                                            this.latest_release = None;
                                            format!(
                                                "Parchment is up to date (power lease: {:?}).",
                                                this.power_lease.status()
                                            )
                                        }
                                        Err(e) => e,
                                    };
                                    cx.notify();
                                })),
                        )
                        .children(self.latest_release.as_ref().map(|rel| {
                            let url = rel.release_url.clone();
                            let sha = rel.expected_sha256_hex.clone().unwrap_or_default();
                            let sz = rel.expected_size_bytes;
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("open-rel-btn")
                                        .label("Open release page")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Err(e) = open_web_link(&url) {
                                                this.update_status = e;
                                                cx.notify();
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("verify-staged-btn")
                                        .label("Verify downloaded installer")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(p) = pick_open_file_dialog(
                                                "Verify Parchment installer",
                                                "Installers (*.exe)",
                                                "*.exe",
                                            ) {
                                                let runs = usize::from(this.active_run.is_some());
                                                this.update_status =
                                                    match OtaStagedFile::verify_staged(&p, sz, &sha)
                                                        .and_then(|s| {
                                                            s.ensure_safe_to_install(runs)
                                                                .map(|()| s)
                                                        }) {
                                                        Ok(st) => format!(
                                                            "Verified {} bytes (sha256: {}…).",
                                                            st.size_bytes,
                                                            &st.sha256_hex[..12]
                                                        ),
                                                        Err(e) => e,
                                                    };
                                                cx.notify();
                                            }
                                        })),
                                )
                        }))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(self.update_status.clone()),
                        ),
                )
                .into_any_element(),
        };

        h_flex()
            .flex_1()
            .size_full()
            .bg(canvas)
            .child(nav)
            .child(
                v_flex()
                    .flex_1()
                    .p_6()
                    .gap_4()
                    .child(
                        h_flex().justify_end().child(
                            Button::new("close-settings-btn")
                                .label("Done")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.overlay = ActiveOverlay::None;
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(detail),
            )
            .into_any_element()
    }
}

impl Render for ParchmentDesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        let canvas = self.palette.canvas_bg.to_gpui_rgb();
        let sheet = self.palette.sheet_bg.to_gpui_rgb();
        let card = self.palette.card_bg.to_gpui_rgb();
        let elev = self.palette.elevated_bg.to_gpui_rgb();
        let border = self.palette.border.to_gpui_rgb();
        let ink = self.palette.primary_ink.to_gpui_rgb();
        let muted = self.palette.muted_ink.to_gpui_rgb();
        let accent = self.palette.accent.to_gpui_rgb();

        let messages = self.messages.clone();
        let view_mode = self.view_mode;
        let palette_clone = self.palette.clone();
        let show_thinking = self.snapshot.preferences.show_thinking_cards;
        let show_tools = self.snapshot.preferences.show_tool_cards;

        let tab_bar = h_flex()
            .w_full()
            .px_3()
            .py_2()
            .bg(sheet)
            .border_b_1()
            .border_color(border)
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("toggle-sidebar-btn")
                            .label(if self.sidebar_open { "Hide" } else { "Chats" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sidebar_open = !this.sidebar_open;
                                cx.notify();
                            })),
                    )
                    .children(self.open_tabs.iter().enumerate().map(|(ix, tid)| {
                        let title = self
                            .stubs
                            .iter()
                            .find(|s| &s.id == tid)
                            .map(|s| s.title.clone())
                            .unwrap_or_else(|| tid.clone());
                        let is_active = tid == &self.active_chat_id;
                        let cid = tid.clone();
                        div()
                            .id(("mru-tab", ix))
                            .px_3()
                            .py_1()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .bg(if is_active { card } else { sheet })
                            .border_l_2()
                            .border_color(if is_active { accent } else { sheet })
                            .text_xs()
                            .text_color(ink)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_chat_by_id(&cid, window, cx);
                            }))
                            .child(title)
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("toggle-view-mode")
                            .label(match self.view_mode {
                                ViewMode::EditorialTurns => strings::VIEW_BUBBLES,
                                ViewMode::Manuscript => strings::VIEW_PAGE,
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.view_mode = match this.view_mode {
                                    ViewMode::EditorialTurns => ViewMode::Manuscript,
                                    ViewMode::Manuscript => ViewMode::EditorialTurns,
                                };
                                this.snapshot.preferences.default_view_mode = this.view_mode;
                                this.persist_settings();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("palette-btn")
                            .label("Ctrl+K")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.overlay = ActiveOverlay::CommandPalette;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-btn")
                            .label(strings::SETTINGS_TITLE)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.overlay = if this.overlay == ActiveOverlay::Settings {
                                    ActiveOverlay::None
                                } else {
                                    this.sync_provider_inputs_from_active(window, cx);
                                    ActiveOverlay::Settings
                                };
                                cx.notify();
                            })),
                    ),
            );

        // Main center surface: either Settings or Chat Viewport + Composer.
        let center_body: AnyElement = if self.overlay == ActiveOverlay::Settings {
            self.render_settings_panel(cx)
        } else {
            let empty_cta = if messages.borrow().is_empty() && !self.has_configured_provider() {
                Some(
                    v_flex()
                        .mx_auto()
                        .my_6()
                        .max_w(px(460.))
                        .p_5()
                        .rounded(px(12.))
                        .bg(card)
                        .border_1()
                        .border_color(border)
                        .gap_2()
                        .child(
                            div()
                                .text_base()
                                .font_semibold()
                                .text_color(ink)
                                .child(strings::EMPTY_NO_PROVIDER_TITLE),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(strings::EMPTY_NO_PROVIDER_BODY),
                        )
                        .child(
                            Button::new("empty-setup-btn")
                                .primary()
                                .label(strings::SETUP_PROVIDER_CTA)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.settings_section = SettingsSection::Providers;
                                    this.sync_provider_inputs_from_active(window, cx);
                                    this.overlay = ActiveOverlay::Settings;
                                    cx.notify();
                                })),
                        ),
                )
            } else {
                None
            };

            // Pending tool approval cards (one-use permits; Allow for run hidden for `computer_use`).
            let approval_cards: Vec<AnyElement> = self
                .active_run
                .as_ref()
                .map(|run| {
                    run.pending_approvals
                        .iter()
                        .enumerate()
                        .map(|(ix, req)| {
                            let req_id_once = req.request_id.clone();
                            let req_id_run = req.request_id.clone();
                            let req_id_deny = req.request_id.clone();
                            let req_id_stop = req.request_id.clone();
                            let is_cu = req.tool_name == "computer_use";
                            v_flex()
                                .id(("approval-card", ix))
                                .mx_6()
                                .mb_2()
                                .p_3()
                                .rounded(px(10.))
                                .bg(elev)
                                .border_1()
                                .border_color(accent)
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(ink)
                                        .child(format!("Approve action: {}", req.tool_name)),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(req.arguments_json.clone()),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new(("appr-once", ix))
                                                .primary()
                                                .label("Allow once")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.respond_approval(
                                                        req_id_once.clone(),
                                                        ToolApprovalDecision::AllowOnce,
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .children((!is_cu).then(|| {
                                            Button::new(("appr-run", ix))
                                                .label("Allow for run")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.respond_approval(
                                                    req_id_run.clone(),
                                                    ToolApprovalDecision::AllowExactActionForRun,
                                                    cx,
                                                );
                                                }))
                                        }))
                                        .child(
                                            Button::new(("appr-deny", ix)).label("Deny").on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.respond_approval(
                                                        req_id_deny.clone(),
                                                        ToolApprovalDecision::Deny,
                                                        cx,
                                                    );
                                                }),
                                            ),
                                        )
                                        .child(
                                            Button::new(("appr-stop", ix))
                                                .label(strings::STOP)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.respond_approval(
                                                        req_id_stop.clone(),
                                                        ToolApprovalDecision::Stop,
                                                        cx,
                                                    );
                                                })),
                                        ),
                                )
                                .into_any_element()
                        })
                        .collect()
                })
                .unwrap_or_default();

            let retry_banner = self
                .active_run
                .as_ref()
                .and_then(|r| r.retry_banner.clone())
                .map(|msg| {
                    h_flex()
                        .mx_6()
                        .mb_2()
                        .px_3()
                        .py_2()
                        .rounded(px(8.))
                        .bg(elev)
                        .border_1()
                        .border_color(accent)
                        .justify_between()
                        .items_center()
                        .child(div().text_xs().text_color(ink).child(msg))
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(accent)
                                .child(strings::RETRY_NOW),
                        )
                });

            let is_running = self.active_run.is_some();

            v_flex()
                .flex_1()
                .h_full()
                .bg(canvas)
                .children(empty_cta)
                .child(
                    div().flex_1().w_full().overflow_hidden().child(
                        MessageScroller::new(
                            "manuscript-scroller",
                            self.scroller.clone(),
                            move |ix, _window, _cx| {
                                let guard = messages.borrow();
                                let Some(msg) = guard.get(ix) else {
                                    return div().into_any_element();
                                };
                                render_message_row(
                                    ix,
                                    msg,
                                    view_mode,
                                    &palette_clone,
                                    show_thinking,
                                    show_tools,
                                )
                            },
                        )
                        .size_full(),
                    ),
                )
                .children(approval_cards)
                .children(retry_banner)
                .child(
                    h_flex()
                        .w_full()
                        .px_4()
                        .py_3()
                        .border_t_1()
                        .border_color(border)
                        .bg(sheet)
                        .items_end()
                        .gap_2()
                        .child(
                            div().flex_1().child(
                                Textarea::new(&self.composer)
                                    .h(px(68.))
                                    .aria_label("Message composer"),
                            ),
                        )
                        .child(if is_running {
                            Button::new("stop-btn")
                                .label(strings::STOP)
                                .on_click(cx.listener(|this, _, _, cx| this.stop_active_run(cx)))
                        } else {
                            Button::new("send-btn")
                                .primary()
                                .label(strings::SEND)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_composer_message(window, cx);
                                }))
                        }),
                )
                .into_any_element()
        };

        // Optional Command Palette (`Ctrl+K`) or Prompt Jump (`Ctrl+G`) banner card.
        let overlay_card: Option<AnyElement> = match self.overlay {
            ActiveOverlay::CommandPalette => Some(
                v_flex()
                    .mx_6()
                    .mt_2()
                    .p_3()
                    .rounded(px(10.))
                    .bg(elev)
                    .border_1()
                    .border_color(accent)
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(strings::COMMAND_PALETTE_TITLE),
                            )
                            .child(Button::new("close-pal").label("Esc").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.overlay = ActiveOverlay::None;
                                    cx.notify();
                                },
                            ))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Button::new("cmd-new").label(strings::NEW_CHAT).on_click(
                                cx.listener(|this, _, window, cx| this.create_new_chat(window, cx)),
                            ))
                            .child(
                                Button::new("cmd-settings")
                                    .label(strings::SETTINGS_TITLE)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.sync_provider_inputs_from_active(window, cx);
                                        this.overlay = ActiveOverlay::Settings;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("cmd-bench")
                                    .label("Stream 87 KB benchmark")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.overlay = ActiveOverlay::None;
                                        this.start_stream_benchmark(false, window, cx);
                                    })),
                            ),
                    )
                    .into_any_element(),
            ),
            ActiveOverlay::PromptJump => {
                let prompts: Vec<(usize, String)> = self
                    .messages
                    .borrow()
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.is_user)
                    .take(12)
                    .map(|(ix, m)| (ix, m.content.chars().take(90).collect()))
                    .collect();
                Some(
                    v_flex()
                        .mx_6()
                        .mt_2()
                        .p_3()
                        .rounded(px(10.))
                        .bg(elev)
                        .border_1()
                        .border_color(accent)
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(ink)
                                        .child(strings::PROMPT_JUMP_TITLE),
                                )
                                .child(Button::new("close-jump").label("Esc").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.overlay = ActiveOverlay::None;
                                        cx.notify();
                                    }),
                                )),
                        )
                        .children(prompts.into_iter().map(|(ix, snippet)| {
                            div()
                                .id(("jump-item", ix))
                                .px_2()
                                .py_1()
                                .rounded(px(6.))
                                .cursor_pointer()
                                .bg(card)
                                .text_xs()
                                .text_color(ink)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.overlay = ActiveOverlay::None;
                                    this.scroller.update(cx, |s, cx| s.scroll_to_item(ix, cx));
                                    cx.notify();
                                }))
                                .child(format!("#{}: {snippet}", ix + 1))
                        }))
                        .into_any_element(),
                )
            }
            _ => None,
        };

        // Floating Undo / Status notice with 48px-height dismiss control (per `AGENTS.md`).
        let notice_bar = self.notice.as_ref().map(|n| {
            let has_undo = n.deleted_session.is_some();
            h_flex()
                .mx_6()
                .my_2()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .bg(elev)
                .border_1()
                .border_color(border)
                .justify_between()
                .items_center()
                .child(div().text_sm().text_color(ink).child(n.message.clone()))
                .child(
                    h_flex()
                        .gap_2()
                        .children(has_undo.then(|| {
                            Button::new("undo-notice-btn")
                                .primary()
                                .label(strings::UNDO)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.undo_delete(window, cx);
                                }))
                        }))
                        .child(
                            div()
                                .id("dismiss-notice-btn")
                                .min_w(px(48.))
                                .min_h(px(48.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.notice = None;
                                    cx.notify();
                                }))
                                .child(strings::DISMISS),
                        ),
                )
        });

        let root_el = h_flex()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_global_keydown))
            .size_full()
            .bg(canvas)
            .text_color(ink)
            .children(self.sidebar_open.then(|| self.render_sidebar(cx)))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .bg(canvas)
                    .child(tab_bar)
                    .children(overlay_card)
                    .children(notice_bar)
                    .child(center_body),
            );

        if self.stream.active {
            self.stream
                .cpu_render_ms
                .push(render_started.elapsed().as_secs_f64() * 1000.0);
        }
        root_el
    }
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
