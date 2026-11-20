use super::*;

impl ParchmentDesktopApp {
    pub(super) fn handle_overlay_action(
        &mut self,
        action: OverlayAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            OverlayAction::Dismiss => {
                self.overlay = ActiveOverlay::None;
                self.pending_confirm = None;
                self.active_viewer = None;
                self.sync_window_title(window);
                cx.notify();
            }
            OverlayAction::RunCommand(cmd_id) => {
                self.overlay = ActiveOverlay::None;
                match cmd_id {
                    "retry_now" => {
                        self.handle_composer_action(ComposerAction::RetryNow, window, cx);
                    }
                    "new_chat" => self.create_new_chat(window, cx),
                    "open_model_picker" => {
                        self.open_overlay(
                            ActiveOverlay::QuickModelPicker,
                            "Filter models or providers…",
                            window,
                            cx,
                        );
                    }
                    "toggle_view" => {
                        let next = if self.active_session.view_mode == ViewMode::EditorialTurns {
                            ViewMode::Manuscript
                        } else {
                            ViewMode::EditorialTurns
                        };
                        self.handle_settings_action(SettingsAction::SetViewMode(next), window, cx);
                    }
                    "toggle_sidebar" => {
                        self.sidebar_open.toggle();
                        self.snapshot.preferences.sidebar_open = self.sidebar_open;
                        self.persist_settings();
                        cx.notify();
                    }
                    "toggle_vault" => {
                        self.vault_open.toggle();
                        cx.notify();
                    }
                    "toggle_notes" => {
                        self.notes_open.toggle();
                        if self.notes_open {
                            self.sync_notes_inputs_from_active(window, cx);
                        }
                        cx.notify();
                    }
                    "open_settings" => {
                        self.open_settings_at(SettingsSection::Preferences, window, cx)
                    }
                    "settings_providers" => {
                        self.open_settings_at(SettingsSection::Providers, window, cx)
                    }
                    "settings_assistants" => {
                        self.open_settings_at(SettingsSection::Assistants, window, cx)
                    }
                    "settings_default_model" => {
                        self.open_settings_at(SettingsSection::DefaultModel, window, cx)
                    }
                    "settings_workspace" => {
                        self.open_settings_at(SettingsSection::Workspace, window, cx)
                    }
                    "settings_world_book" => {
                        self.open_settings_at(SettingsSection::WorldBook, window, cx)
                    }
                    "settings_shortcuts" => {
                        self.open_settings_at(SettingsSection::Shortcuts, window, cx)
                    }
                    "settings_storage" => {
                        self.open_settings_at(SettingsSection::BackupAndUpdates, window, cx)
                    }
                    "prompt_jump" => {
                        self.open_overlay(
                            ActiveOverlay::PromptJump,
                            strings::PROMPT_JUMP_PLACEHOLDER,
                            window,
                            cx,
                        );
                    }
                    "tab_switcher" => {
                        self.open_overlay(
                            ActiveOverlay::TabSwitcher,
                            "Filter open tabs…",
                            window,
                            cx,
                        );
                    }
                    "stream_bench" => self.start_stream_benchmark(false, window, cx),
                    _ => {}
                }
            }
            OverlayAction::SelectChat(cid) => self.select_chat_by_id(&cid, window, cx),
            OverlayAction::JumpToMessageIndex(ix) => {
                self.overlay = ActiveOverlay::None;
                self.scroller.update(cx, |s, cx| s.scroll_to_item(ix, cx));
                cx.notify();
            }
            OverlayAction::SelectModelQuick {
                profile_id,
                model_id,
            } => {
                self.snapshot.active_profile_id = profile_id.clone();
                self.active_session.profile_id = profile_id.clone();
                self.active_session.model_override = model_id.clone();
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == profile_id)
                {
                    p.default_model = model_id;
                }
                self.persist_settings();
                self.persist_active_session();
                self.overlay = ActiveOverlay::None;
                cx.notify();
            }
            OverlayAction::SetQuickModelProviderFilter(pf) => {
                self.quick_model_provider_filter = pf;
                cx.notify();
            }
            OverlayAction::OpenProviderSettings => {
                self.open_settings_at(SettingsSection::Providers, window, cx);
            }
            OverlayAction::CloseTab(cid) => self.close_tab(&cid, window, cx),
            OverlayAction::CloseEmptyTabs => {
                let empty_ids: Vec<String> = self
                    .open_tabs
                    .iter()
                    .filter(|tid| {
                        !self.active_runs.contains_key(*tid)
                            && self.stubs.iter().find(|s| &s.id == *tid).is_some_and(|s| {
                                s.message_count == 0
                                    && (s.title.eq_ignore_ascii_case("New Chat")
                                        || s.title.eq_ignore_ascii_case(strings::NEW_CHAT))
                            })
                    })
                    .cloned()
                    .collect();
                for id in empty_ids {
                    if self.open_tabs.len() > 1 {
                        self.close_tab(&id, window, cx);
                    }
                }
                cx.notify();
            }
            OverlayAction::NewTab => {
                self.overlay = ActiveOverlay::None;
                self.create_new_chat(window, cx);
            }
            // Viewer modal actions
            OverlayAction::ViewerZoomIn => {
                self.viewer_zoom = (self.viewer_zoom + 0.25).clamp(0.5, 4.0);
                cx.notify();
            }
            OverlayAction::ViewerZoomOut => {
                self.viewer_zoom = (self.viewer_zoom - 0.25).clamp(0.5, 4.0);
                cx.notify();
            }
            OverlayAction::ViewerZoomReset => {
                self.viewer_zoom = 1.0;
                cx.notify();
            }
            OverlayAction::ViewerTogglePreview => {
                self.viewer_show_preview.toggle();
                cx.notify();
            }
            OverlayAction::ViewerToggleWrap => {
                self.viewer_wrap.toggle();
                cx.notify();
            }
            OverlayAction::ViewerCopyText(txt) => {
                cx.write_to_clipboard(ClipboardItem::new_string(txt));
                self.viewer_copied = true;
                cx.notify();
            }
            OverlayAction::ViewerSaveFile {
                title,
                local_path,
                text_content,
            } => {
                let ext_owned = PathBuf::from(&title)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt")
                    .to_string();
                if let Some(dest) = pick_save_file_dialog("Save file", &title, &ext_owned) {
                    let res = if let Some(txt) = text_content {
                        std::fs::write(&dest, txt).map_err(|e| e.to_string())
                    } else if !local_path.is_empty() {
                        std::fs::copy(&local_path, &dest)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    } else {
                        Err("No file content available to save.".into())
                    };
                    match res {
                        Ok(()) => self.show_notice(format!("Saved to {}", dest.display()), cx),
                        Err(e) => self.show_notice(format!("Save failed: {e}"), cx),
                    }
                }
            }
            OverlayAction::ViewerOpenLocal(lp) => {
                if let Err(e) = open_local_file_or_folder(&lp) {
                    self.show_notice(e, cx);
                }
            }
            OverlayAction::ViewerOpenUrl(url) => {
                if let Err(e) = open_web_link(&url) {
                    self.show_notice(e, cx);
                }
            }
            // Confirm dialog actions
            OverlayAction::SetConfirmDeletePref(v) => {
                self.snapshot.preferences.confirm_before_deleting_chat = v;
                self.persist_settings();
                cx.notify();
            }
            OverlayAction::SetConfirmRegeneratePref(v) => {
                self.snapshot.preferences.confirm_before_regenerating = v;
                self.persist_settings();
                cx.notify();
            }
            OverlayAction::ConfirmDialogAccept => {
                self.overlay = ActiveOverlay::None;
                let Some(kind) = self.pending_confirm.take() else {
                    cx.notify();
                    return;
                };
                match kind {
                    ConfirmDialogKind::DeleteSingleChat { chat_id, .. } => {
                        self.delete_chat_with_undo(&chat_id, window, cx);
                    }
                    ConfirmDialogKind::ClearAllChats { .. } => {
                        self.execute_clear_all_for_assistant(window, cx);
                    }
                    ConfirmDialogKind::ClearCurrentMessages { .. } => {
                        self.execute_clear_current_messages(cx);
                    }
                    ConfirmDialogKind::RegenerateTurn { index, .. } => {
                        self.execute_regenerate_turn(index, window, cx);
                    }
                }
            }
        }
    }

    // ---- Global Keyboard Shortcuts ----

    pub(super) fn handle_global_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // 1. Interactive Shortcut Recorder in Settings > Shortcuts
        if self.overlay == ActiveOverlay::Settings
            && let Some(target_action) = self.recording_shortcut
        {
            match capture_shortcut_event(keystroke) {
                ShortcutCaptureResult::Accepted(binding) => {
                    if let Some(conflict_with) = find_shortcut_conflict(
                        target_action,
                        &binding,
                        &self.snapshot.preferences.custom_shortcuts,
                    ) {
                        self.shortcut_conflict = Some(ShortcutConflictPrompt {
                            target: target_action,
                            conflict_with,
                            binding,
                        });
                        self.shortcut_feedback.clear();
                    } else {
                        let formatted = binding.format();
                        update_custom_shortcut(
                            &mut self.snapshot.preferences.custom_shortcuts,
                            target_action,
                            binding,
                        );
                        self.recording_shortcut = None;
                        self.shortcut_conflict = None;
                        self.shortcut_feedback =
                            format!("Set {} to {formatted}.", target_action.title());
                        self.persist_settings();
                    }
                    cx.notify();
                }
                ShortcutCaptureResult::Cancelled => {
                    self.recording_shortcut = None;
                    self.shortcut_conflict = None;
                    self.shortcut_feedback = "Cancelled shortcut recording.".into();
                    cx.notify();
                }
                ShortcutCaptureResult::IgnoredModifier => {}
                ShortcutCaptureResult::Rejected(msg) => {
                    self.shortcut_feedback = msg;
                    cx.notify();
                }
            }
            return true;
        }

        let key = keystroke.key.as_str();
        let ctrl = keystroke.modifiers.control || keystroke.modifiers.platform;
        let shift = keystroke.modifiers.shift;

        // 2. Keyboard navigation inside Command Palette / Prompt Jump
        if self.overlay == ActiveOverlay::CommandPalette {
            let q = self.overlay_filter_input.read(cx).value().to_string();
            let can_retry = self
                .active_runs
                .get(&self.active_session.id)
                .is_some_and(|r| r.retry_banner.is_some());
            let entries = command_palette_entries(&q, &self.stubs, can_retry, &|cid| {
                self.shortcut_hints_for_command(cid)
            });
            if !entries.is_empty() {
                match key {
                    "down" => {
                        self.overlay_selected_index =
                            (self.overlay_selected_index + 1) % entries.len();
                        cx.notify();
                        return true;
                    }
                    "up" => {
                        self.overlay_selected_index = if self.overlay_selected_index == 0 {
                            entries.len() - 1
                        } else {
                            self.overlay_selected_index - 1
                        };
                        cx.notify();
                        return true;
                    }
                    "enter" => {
                        let idx = self.overlay_selected_index.min(entries.len() - 1);
                        match &entries[idx] {
                            PaletteEntry::Command { id, .. } => {
                                self.handle_overlay_action(
                                    OverlayAction::RunCommand(id),
                                    window,
                                    cx,
                                );
                            }
                            PaletteEntry::Chat { id, .. } => {
                                self.handle_overlay_action(
                                    OverlayAction::SelectChat(id.clone()),
                                    window,
                                    cx,
                                );
                            }
                        }
                        return true;
                    }
                    _ => {}
                }
            }
        } else if self.overlay == ActiveOverlay::PromptJump {
            let q = self.overlay_filter_input.read(cx).value().to_string();
            let targets = {
                let rows = self.messages.borrow();
                prompt_jump_targets(&q, &rows)
            };
            if !targets.is_empty() {
                match key {
                    "down" => {
                        self.overlay_selected_index =
                            (self.overlay_selected_index + 1) % targets.len();
                        cx.notify();
                        return true;
                    }
                    "up" => {
                        self.overlay_selected_index = if self.overlay_selected_index == 0 {
                            targets.len() - 1
                        } else {
                            self.overlay_selected_index - 1
                        };
                        cx.notify();
                        return true;
                    }
                    "enter" => {
                        let idx = self.overlay_selected_index.min(targets.len() - 1);
                        let msg_ix = targets[idx].0;
                        self.handle_overlay_action(
                            OverlayAction::JumpToMessageIndex(msg_ix),
                            window,
                            cx,
                        );
                        return true;
                    }
                    _ => {}
                }
            }
        }

        // 3. Customizable global shortcut chords
        if let Some(action) =
            match_shortcut_action(keystroke, &self.snapshot.preferences.custom_shortcuts)
        {
            match action {
                AppShortcutAction::CommandPalette => {
                    if self.overlay == ActiveOverlay::CommandPalette {
                        self.overlay = ActiveOverlay::None;
                        cx.notify();
                    } else {
                        self.open_overlay(
                            ActiveOverlay::CommandPalette,
                            strings::COMMAND_PALETTE_PLACEHOLDER,
                            window,
                            cx,
                        );
                    }
                }
                AppShortcutAction::PromptJump => {
                    if self.overlay == ActiveOverlay::PromptJump {
                        self.overlay = ActiveOverlay::None;
                        cx.notify();
                    } else {
                        self.open_overlay(
                            ActiveOverlay::PromptJump,
                            strings::PROMPT_JUMP_PLACEHOLDER,
                            window,
                            cx,
                        );
                    }
                }
                AppShortcutAction::NewChat => self.create_new_chat(window, cx),
                AppShortcutAction::CloseTab => {
                    let cid = self.active_session.id.clone();
                    self.close_tab(&cid, window, cx);
                }
                AppShortcutAction::NextTab => self.cycle_tabs(window, cx),
                AppShortcutAction::PrevTab => self.cycle_tabs_prev(window, cx),
                AppShortcutAction::ToggleSettings => {
                    if self.overlay == ActiveOverlay::Settings {
                        self.overlay = ActiveOverlay::None;
                        self.sync_window_title(window);
                        cx.notify();
                    } else {
                        self.open_settings_at(self.settings_section, window, cx);
                    }
                }
                AppShortcutAction::ToggleSidebar => {
                    self.sidebar_open.toggle();
                    self.snapshot.preferences.sidebar_open = self.sidebar_open;
                    self.persist_settings();
                    cx.notify();
                }
            }
            return true;
        }

        // 4. Built-in secondary shortcuts & Escape cascade
        if ctrl && shift && key == "f" {
            self.vault_open.toggle();
            cx.notify();
            return true;
        }
        if key == "escape" {
            if self.is_editing_title {
                self.is_editing_title = false;
                cx.notify();
                return true;
            } else if self.editing_message_index.is_some() {
                self.editing_message_index = None;
                cx.notify();
                return true;
            } else if self.overlay != ActiveOverlay::None {
                self.overlay = ActiveOverlay::None;
                self.pending_confirm = None;
                self.active_viewer = None;
                self.sync_window_title(window);
                cx.notify();
                return true;
            } else if self.minimap_open {
                self.minimap_open = false;
                cx.notify();
                return true;
            } else if self.show_tools_drawer
                || self.show_thinking_popover
                || self.show_modality_popover
                || self.show_context_popover
            {
                self.show_tools_drawer = false;
                self.show_thinking_popover = false;
                self.show_modality_popover = false;
                self.show_context_popover = false;
                cx.notify();
                return true;
            }
        }
        false
    }

    // ---- Quantitative 87 KB streaming benchmark (preserved for `cargo xtask perf`) ----

    pub(super) fn start_stream_benchmark(
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
                format!("bench-{}", now_ms()),
                SharedString::new_static("benchmark"),
                now_ms(),
            ));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });
        self.schedule_stream_tick(window, cx);
        cx.notify();
    }

    pub(super) fn schedule_stream_tick(&self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                this.advance_stream_frame(window, cx);
            });
        });
    }

    pub(super) fn advance_stream_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                if next_cursor >= total {
                    last.is_streaming = false;
                    last.recompute_extras_for_completed();
                }
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

    pub(super) fn finish_stream_benchmark(&mut self, cx: &mut Context<Self>) {
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

    // ---- Top Chrome Rendering (Editorial Bar + Desktop Browser Tab Strip) ----
}
