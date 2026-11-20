use super::*;

impl ParchmentDesktopApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        core: Arc<ParchmentCore>,
        data_dir: PathBuf,
        mut snapshot: AppStateSnapshot,
        mut stubs: Vec<ChatStub>,
        startup_core_ms: f64,
        cli_bench_stream: bool,
        bench_out: Option<PathBuf>,
        migration_warning: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut settings_dirty = false;
        if !snapshot.design_defaults_applied {
            apply_design_defaults_once(&mut snapshot);
            settings_dirty = true;
        }
        cx.set_reduce_motion(snapshot.preferences.reduce_motion);
        if snapshot.profiles.is_empty() {
            snapshot.profiles = vec![
                make_default_profile(ProviderProtocol::OpenAiResponses),
                make_default_profile(ProviderProtocol::AnthropicMessages),
                make_default_profile(ProviderProtocol::GeminiInteractions),
            ];
            snapshot.active_profile_id = snapshot.profiles[0].id.clone();
            settings_dirty = true;
        } else if !snapshot
            .profiles
            .iter()
            .any(|p| p.id == snapshot.active_profile_id)
        {
            snapshot.active_profile_id = snapshot.profiles[0].id.clone();
            settings_dirty = true;
        }

        if snapshot.assistants.is_empty() {
            snapshot.assistants.push(default_assistant_agent());
            snapshot.active_assistant_id = snapshot.assistants[0].id.clone();
            settings_dirty = true;
        } else if !snapshot
            .assistants
            .iter()
            .any(|a| a.id == snapshot.active_assistant_id)
        {
            snapshot.active_assistant_id = snapshot.assistants[0].id.clone();
            settings_dirty = true;
        }

        let palette = ParchmentPalette::from_config(&snapshot.theme);
        let init_palette = palette.clone();
        window.on_next_frame(move |_, _| apply_dwm_chrome_to_active_window(&init_palette));

        let now = now_ms();
        let should_start_fresh =
            stubs.is_empty() || (snapshot.preferences.new_chat_on_launch && !cli_bench_stream);
        let initial_chat_id = if should_start_fresh {
            let fresh = build_fresh_chat_session(&snapshot, now);
            let _ = core.save_full_session(&fresh);
            stubs = core.list_chat_stubs().unwrap_or_default();
            fresh.id
        } else if !snapshot.active_session_id.is_empty()
            && stubs.iter().any(|s| s.id == snapshot.active_session_id)
        {
            snapshot.active_session_id.clone()
        } else if let Some(first) = stubs.first() {
            first.id.clone()
        } else {
            let fresh = build_fresh_chat_session(&snapshot, now);
            let _ = core.save_full_session(&fresh);
            stubs = core.list_chat_stubs().unwrap_or_default();
            fresh.id
        };
        let active_session = core
            .load_full_session(&initial_chat_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let mut s = ChatSession::new(&initial_chat_id, strings::NEW_CHAT, now);
                s.profile_id = snapshot.active_profile_id.clone();
                s.assistant_id = snapshot.active_assistant_id.clone();
                s.view_mode = snapshot.preferences.default_view_mode;
                s
            });

        let mut open_tabs: Vec<String> = snapshot
            .open_tab_session_ids
            .iter()
            .filter(|id| stubs.iter().any(|s| &s.id == *id))
            .cloned()
            .collect();
        open_tabs.retain(|id| id != &active_session.id);
        open_tabs.insert(0, active_session.id.clone());
        open_tabs.truncate(MAX_MRU_TABS);

        snapshot.active_session_id = active_session.id.clone();
        snapshot.open_tab_session_ids = open_tabs.clone();
        if settings_dirty {
            let _ = core.save_settings_snapshot(&snapshot);
        }

        window.set_window_title(&strings::window_title_for_chat(&active_session.title));

        let initial_rows: Vec<DisplayMessageRow> = active_session
            .messages
            .iter()
            .cloned()
            .map(DisplayMessageRow::from_node)
            .collect();
        let initial_count = initial_rows.len();
        let auto_scroll_open = snapshot.preferences.auto_scroll_to_bottom_on_open;
        let scroller = cx.new(|cx| {
            let mut s = MessageScrollerState::new(initial_count, cx);
            if !auto_scroll_open && initial_count > 0 {
                s.scroll_to_item(0, cx);
            }
            s
        });

        let submit_on_enter = snapshot.preferences.send_shortcut == SendShortcutMode::Enter;
        let init_draft = active_session.draft_input.clone();
        let init_prefill = active_session.draft_prefill.clone();
        let show_prefill_box = !init_prefill.trim().is_empty();

        let initial_asst_name = snapshot
            .assistants
            .iter()
            .find(|a| a.id == snapshot.active_assistant_id)
            .or_else(|| snapshot.assistants.first())
            .map(|a| a.name.clone())
            .unwrap_or_else(|| strings::DEFAULT_ASSISTANT.into());
        let composer = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx)
                .placeholder(strings::message_assistant_placeholder(&initial_asst_name));
            st.set_submit_on_enter(submit_on_enter, cx);
            if !init_draft.is_empty() {
                st.set_value(&init_draft, window, cx);
            }
            st
        });
        let prefill_input = cx.new(|cx| {
            let mut st =
                TextareaState::new(window, cx).placeholder("Start the assistant reply with…");
            if !init_prefill.is_empty() {
                st.set_value(&init_prefill, window, cx);
            }
            st
        });
        let sidebar_search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(strings::SEARCH_CHATS));
        let title_edit_input = cx.new(|cx| InputState::new(window, cx).placeholder("Chat title"));
        let edit_msg_input = cx.new(|cx| TextareaState::new(window, cx));
        let overlay_filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(strings::COMMAND_PALETTE_PLACEHOLDER)
        });
        let custom_thinking_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Custom value"));
        let minimap_filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter turns…"));
        let question_answer_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type a custom answer…"));

        if !snapshot.theme.custom_prose_font_path.is_empty() {
            let _ = register_custom_font_file(&snapshot.theme.custom_prose_font_path, cx);
        }
        if !snapshot.theme.custom_code_font_path.is_empty() {
            let _ = register_custom_font_file(&snapshot.theme.custom_code_font_path, cx);
        }

        let notes_system_input = cx.new(|cx| {
            let mut st = TextareaState::new(window, cx)
                .placeholder("Extra instructions applied to this chat…");
            if !active_session.system_prompt.is_empty() {
                st.set_value(&active_session.system_prompt, window, cx);
            }
            st
        });
        let notes_authors_input = cx.new(|cx| {
            let mut st =
                TextareaState::new(window, cx).placeholder("Short tone or style reminder…");
            if !active_session.authors_note.is_empty() {
                st.set_value(&active_session.authors_note, window, cx);
            }
            st
        });
        let notes_summary_input = cx.new(|cx| {
            let mut st =
                TextareaState::new(window, cx).placeholder("Remembered summary of earlier turns…");
            if !active_session.story_summary.is_empty() {
                st.set_value(&active_session.story_summary, window, cx);
            }
            st
        });

        let settings_inputs = build_settings_inputs(&snapshot, window, cx);

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe(&scroller, |this, scroller, cx| {
            let top = scroller.read(cx).logical_scroll_top();
            let cur = (top.item_ix, f32::from(top.offset_in_item).round() as i32);
            if let Some(prev) = this.last_scroll_top
                && prev != cur
            {
                let deadline = Instant::now() + Duration::from_millis(1800);
                this.minimap_scroll_linger_until = Some(deadline);
                cx.spawn(async move |weak, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(1850))
                        .await;
                    let _ = weak.update(cx, |_, cx| cx.notify());
                })
                .detach();
            }
            this.last_scroll_top = Some(cur);
            cx.notify();
        }));
        subscriptions.push(cx.subscribe_in(
            &composer,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let prev_len = this.active_session.draft_input.chars().count();
                    let next_val = this.composer.read(cx).value().to_string();
                    let next_len = next_val.chars().count();
                    let thresh = this
                        .snapshot
                        .preferences
                        .paste_conversion_threshold_chars
                        .max(500) as usize;
                    if this.snapshot.preferences.paste_long_text_as_file
                        && next_len >= thresh
                        && next_len.saturating_sub(prev_len) >= thresh / 2
                    {
                        let ts = now_ms();
                        let fname = format!("pasted-text-{ts}.txt");
                        this.pending_attachments.push(PromptAttachment {
                            id: format!("att-paste-{ts}"),
                            file_name: fname.clone(),
                            mime_type: "text/plain".into(),
                            base64_data: String::new(),
                            text_content: next_val,
                            local_file_path: String::new(),
                        });
                        this.active_session.draft_input.clear();
                        this.composer
                            .update(cx, |i, cx| i.set_value("", window, cx));
                        this.show_notice(
                            format!("Attached long paste ({next_len} chars) as {fname}."),
                            cx,
                        );
                    } else {
                        this.active_session.draft_input = next_val;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { secondary, shift } => {
                    let should_send = match this.snapshot.preferences.send_shortcut {
                        SendShortcutMode::Enter => !*shift,
                        SendShortcutMode::CtrlEnter => *secondary,
                        SendShortcutMode::ShiftEnter => *shift && !*secondary,
                    };
                    if should_send {
                        this.submit_composer_shortcut(window, cx);
                    }
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &notes_system_input,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.active_session.system_prompt =
                        this.notes_system_input.read(cx).value().to_string();
                    this.persist_active_session();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &notes_authors_input,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.active_session.authors_note =
                        this.notes_authors_input.read(cx).value().to_string();
                    this.persist_active_session();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &notes_summary_input,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.active_session.story_summary =
                        this.notes_summary_input.read(cx).value().to_string();
                    this.persist_active_session();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &minimap_filter_input,
            window,
            |_this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.storage_search,
            window,
            |_this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &prefill_input,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.active_session.draft_prefill =
                        this.prefill_input.read(cx).value().to_string();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &sidebar_search_input,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.run_fts_search(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &overlay_filter_input,
            window,
            |_this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &title_edit_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                    && this.is_editing_title
                {
                    this.commit_title_edit(window, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.search,
            window,
            |_this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.model_filter,
            window,
            |_this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.user_profile_name,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let name = this
                        .settings_inputs
                        .user_profile_name
                        .read(cx)
                        .value()
                        .trim()
                        .to_string();
                    this.snapshot.preferences.profile_name = name.clone();
                    if !name.is_empty() {
                        this.active_session.user_name = name;
                        this.persist_active_session();
                    }
                    this.persist_settings();
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.spend_limit,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let spend_raw = this
                        .settings_inputs
                        .spend_limit
                        .read(cx)
                        .value()
                        .trim()
                        .to_string();
                    this.snapshot.preferences.chat_spend_limit_usd = spend_raw
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .unwrap_or(0.0);
                    this.persist_settings();
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &custom_thinking_input,
            window,
            |this, _, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let custom = this
                        .custom_thinking_input
                        .read(cx)
                        .value()
                        .trim()
                        .to_string();
                    let pid = this.active_profile().id;
                    if let Some(p) = this.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                        p.thinking_effort = custom;
                    }
                    this.persist_settings();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.show_thinking_popover = false;
                    cx.notify();
                }
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &settings_inputs.provider_name,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let val = this
                        .settings_inputs
                        .provider_name
                        .read(cx)
                        .value()
                        .trim()
                        .to_string();
                    if !val.is_empty()
                        && let Some(p) = this
                            .snapshot
                            .profiles
                            .iter_mut()
                            .find(|p| p.id == this.snapshot.active_profile_id)
                    {
                        p.name = val;
                        this.persist_settings();
                        cx.notify();
                    }
                }
            },
        ));
        register_settings_subscriptions(&mut subscriptions, &settings_inputs, window, cx);
        let launched_maximized = snapshot.desktop_window.is_maximized && !cli_bench_stream;
        let launched_at = Instant::now();
        let window_closing = Rc::new(std::cell::Cell::new(false));
        let closing_obs = window_closing.clone();
        subscriptions.push(cx.observe_window_bounds(window, move |this, window, _cx| {
            if closing_obs.get() {
                return;
            }
            if let Some(cfg) = capture_window_config(
                window,
                &this.snapshot.desktop_window,
                launched_maximized,
                launched_at,
            ) && cfg != this.snapshot.desktop_window
            {
                this.snapshot.desktop_window = cfg.clone();
                let _ = this.core.save_desktop_window(&cfg);
            }
        }));
        let close_entity = cx.entity().downgrade();
        let closing_flag = window_closing.clone();
        window.on_window_should_close(cx, move |window, cx| {
            let _ = close_entity.update(cx, |this, _cx| {
                if let Some(cfg) = capture_window_config(
                    window,
                    &this.snapshot.desktop_window,
                    launched_maximized,
                    launched_at,
                ) && cfg != this.snapshot.desktop_window
                {
                    this.snapshot.desktop_window = cfg.clone();
                    let _ = this.core.save_desktop_window(&cfg);
                }
            });
            closing_flag.set(true);
            true
        });
        let mcp_registry = McpRegistry::new(core.http_client().clone());
        let (bg_tx, bg_rx) = mpsc::unbounded_channel();

        let sidebar_open = snapshot.preferences.sidebar_open;
        let auto_ota = snapshot.auto_check_ota_on_startup && !cli_bench_stream;
        let initial_mcp = snapshot.preferences.mcp_servers.clone();

        // Defer cold catalog/MCP background sync past Frame 1 so startup stays at the OS/GPU floor.
        let reg_bg = core.registry().clone();
        let mcp_bg = mcp_registry.clone();
        core.runtime().spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let _ = reg_bg.ensure_sync_started(false).await;
            if !initial_mcp.is_empty() {
                mcp_bg.sync(&initial_mcp).await;
            }
        });
        let focus_handle = cx.focus_handle();
        let composer_focus = composer.read(cx).focus_handle(cx);
        window.focus(&composer_focus, cx);
        let entity_keys = cx.entity().downgrade();
        subscriptions.push(
            cx.intercept_keystrokes(move |event: &KeystrokeEvent, window, cx| {
                let handled = entity_keys
                    .update(cx, |this, cx| {
                        this.handle_global_keystroke(&event.keystroke, window, cx)
                    })
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }),
        );

        let mut app = Self {
            core,
            data_dir,
            focus_handle,
            snapshot,
            palette,
            power_lease: Arc::new(WindowsPowerLease::new()),
            mcp_registry,
            _subscriptions: subscriptions,
            sidebar_open,
            sidebar_tab: SidebarTab::Chats,
            sidebar_search_input,
            sidebar_chats_scroll: gpui::UniformListScrollHandle::new(),
            stubs,
            open_tabs,
            search_hits: Vec::new(),
            pinned_stubs: Vec::new(),
            pinned_all_chats: false,
            workspace_entries: Vec::new(),
            custom_slash_commands: Vec::new(),
            vault_open: false,
            vault_filter: VaultFilter::All,
            notes_open: false,
            notes_system_input,
            notes_authors_input,
            notes_summary_input,
            active_session,
            is_editing_title: false,
            title_edit_input,
            messages: Rc::new(RefCell::new(initial_rows)),
            turn_ui_states: HashMap::new(),
            editing_message_index: None,
            edit_msg_input,
            scroller,
            last_scroll_top: None,
            minimap_scroll_linger_until: None,
            minimap_open: false,
            minimap_prompts_only: false,
            minimap_filter_input,
            question_answer_input,
            speaking_node_id: None,
            composer,
            prefill_input,
            custom_thinking_input,
            pending_attachments: Vec::new(),
            queued_prompts: HashMap::new(),
            show_tools_drawer: false,
            show_inline_tool_registry: false,
            show_thinking_popover: false,
            show_modality_popover: false,
            show_context_popover: false,
            show_background_usage: false,
            show_prefill_box,
            goal_mode_enabled: false,
            pending_guided_direction: None,
            is_drafting_reply: false,
            retry_banner_collapsed: false,
            is_voice_listening: false,
            overlay: ActiveOverlay::None,
            overlay_filter_input,
            overlay_selected_index: 0,
            quick_model_provider_filter: None,
            active_viewer: None,
            viewer_zoom: 1.0,
            viewer_show_preview: true,
            viewer_wrap: false,
            viewer_copied: false,
            pending_confirm: None,
            settings_section: SettingsSection::Preferences,
            providers_subpage: ProvidersSubpage::Main,
            assistant_subpage: AssistantSubpage::List,
            show_api_key: false,
            show_add_api_key_box: false,
            show_add_model_box: false,
            show_hex_editor: false,
            world_book_scope: WorldBookScope::Global,
            recording_shortcut: None,
            shortcut_feedback: String::new(),
            shortcut_conflict: None,
            storage_files: Vec::new(),
            storage_category_filter: None,
            global_memory_bytes: None,
            project_memory_files: Vec::new(),
            installed_skills: Vec::new(),
            mcp_statuses: HashMap::new(),
            mcp_registry_results: Vec::new(),
            is_searching_mcp_registry: false,
            mcp_registry_no_token_only: false,
            provider_test_status: String::new(),
            settings_inputs,
            confirm_wipe_pending: false,
            active_runs: HashMap::new(),
            bg_tx,
            bg_rx,
            pending_bg_tasks: 0,
            pump_scheduled: false,
            is_fetching_models: false,
            models_fetch_status: String::new(),
            is_checking_updates: false,
            latest_release: None,
            update_status: String::new(),
            notice: None,
            cached_context_breakdown: RefCell::new(None),
            stream: StreamBenchState::new(bench_out, startup_core_ms),
        };

        app.refresh_pinned();
        app.refresh_workspace_files();
        app.refresh_memory_and_skills();
        app.refresh_storage_scan();

        if let Some(warn) = migration_warning {
            app.show_notice(warn, cx);
        }
        if cli_bench_stream {
            app.start_stream_benchmark(true, window, cx);
        } else if auto_ota {
            app.spawn_check_for_updates(window, cx);
        }

        app
    }

    // ---- Helpers: profiles, assistants, permissions, persistence ----
}

pub(super) fn build_fresh_chat_session(snapshot: &AppStateSnapshot, now: i64) -> ChatSession {
    let asst_clone = snapshot
        .assistants
        .iter()
        .find(|a| a.id == snapshot.active_assistant_id)
        .or_else(|| snapshot.assistants.first())
        .cloned();
    let mut session = ChatSession::new(format!("chat-{now}"), strings::NEW_CHAT, now);
    session.profile_id = asst_clone
        .as_ref()
        .filter(|a| !a.default_profile_id.is_empty())
        .map(|a| a.default_profile_id.clone())
        .unwrap_or_else(|| snapshot.active_profile_id.clone());
    session.assistant_id = asst_clone
        .as_ref()
        .map(|a| a.id.clone())
        .unwrap_or_else(|| snapshot.active_assistant_id.clone());
    session.view_mode = snapshot.preferences.default_view_mode;
    if !snapshot.preferences.profile_name.trim().is_empty() {
        session.user_name = snapshot.preferences.profile_name.clone();
    }
    if let Some(a) = asst_clone {
        session.model_override = a.default_model_id;
        session.system_prompt = a.system_prompt;
        session.authors_note = a.authors_note;
        session.authors_note_depth = a.authors_note_depth;
        session.active_card = a.active_card;
        session.workspace_path = a.workspace_path;
        session.workspace_mode = a.workspace_mode;
    }
    session
}

/// Captures the window's persistent geometry while protecting against transient startup
/// maximization races, minimized `0×0` frames, and clobbering the user's floating size
/// while maximized.
fn capture_window_config(
    window: &Window,
    prev: &DesktopWindowConfig,
    launched_maximized: bool,
    launched_at: Instant,
) -> Option<DesktopWindowConfig> {
    let vp = window.viewport_size();
    // Minimized windows report near-zero client bounds; never clobber saved geometry or state.
    if f32::from(vp.width) <= 200.0 || f32::from(vp.height) <= 150.0 {
        return None;
    }
    let wb = window.window_bounds();
    let b = wb.get_bounds();
    // Reject zero/negative rects (e.g. failed `GetWindowPlacement` on teardown/destroyed HWND).
    if f32::from(b.size.width) < 200.0 || f32::from(b.size.height) < 150.0 {
        return None;
    }
    let is_maximized = matches!(
        wb,
        gpui::WindowBounds::Maximized(_) | gpui::WindowBounds::Fullscreen(_)
    ) || window.is_maximized();
    // `ShowWindowAsync(SW_MAXIMIZE)` finishes asynchronously on startup; ignore transient
    // pre-maximize callbacks during the first 500ms so default-maximized stays sticky.
    if launched_maximized && !is_maximized && launched_at.elapsed() < Duration::from_millis(500) {
        return None;
    }
    if is_maximized {
        return Some(DesktopWindowConfig {
            is_maximized: true,
            ..prev.clone()
        });
    }
    let x = f32::from(b.origin.x).round() as i32;
    let y = f32::from(b.origin.y).round() as i32;
    if x <= -20_000 || y <= -20_000 {
        return None;
    }
    Some(DesktopWindowConfig {
        is_maximized: false,
        width_dp: (f32::from(b.size.width).round() as i32).clamp(480, 7680),
        height_dp: (f32::from(b.size.height).round() as i32).clamp(360, 4320),
        x_dp: x,
        y_dp: y,
    })
}
