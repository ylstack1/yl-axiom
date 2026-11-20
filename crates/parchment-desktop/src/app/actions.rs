use super::*;

impl ParchmentDesktopApp {
    pub(super) fn handle_message_action(
        &mut self,
        action: MessageAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MessageAction::CopyPlain(ix) => {
                if let Some(row) = self.messages.borrow().get(ix) {
                    let plain = strip_markdown_to_plain_text(row.content.as_ref());
                    let nid = row.node_id.clone();
                    cx.write_to_clipboard(ClipboardItem::new_string(plain));
                    self.turn_ui_states.entry(nid).or_default().copied_kind = Some("plain");
                    cx.notify();
                }
            }
            MessageAction::CopyMarkdown(ix) => {
                if let Some(row) = self.messages.borrow().get(ix) {
                    let md = row.content.to_string();
                    let nid = row.node_id.clone();
                    cx.write_to_clipboard(ClipboardItem::new_string(md));
                    self.turn_ui_states.entry(nid).or_default().copied_kind = Some("md");
                    cx.notify();
                }
            }
            MessageAction::CopyText {
                node_id,
                code_block_index,
                text,
            } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let st = self.turn_ui_states.entry(node_id).or_default();
                if let Some(bi) = code_block_index {
                    st.copied_code_block = Some(bi);
                } else {
                    st.copied_kind = Some("plain");
                }
                cx.notify();
            }
            MessageAction::SaveCodeBlock { language, code } => {
                let ext = match language.trim().to_ascii_lowercase().as_str() {
                    "rust" | "rs" => "rs",
                    "kotlin" | "kt" => "kt",
                    "python" | "py" => "py",
                    "javascript" | "js" => "js",
                    "typescript" | "ts" => "ts",
                    "json" => "json",
                    "html" => "html",
                    "css" => "css",
                    "svg" => "svg",
                    "markdown" | "md" => "md",
                    "sh" | "bash" => "sh",
                    "ps1" | "powershell" => "ps1",
                    "toml" => "toml",
                    "yaml" | "yml" => "yaml",
                    "sql" => "sql",
                    _ => "txt",
                };
                let default_name = format!("snippet.{ext}");
                if let Some(dest) = pick_save_file_dialog("Save code block", &default_name, ext) {
                    match std::fs::write(&dest, code) {
                        Ok(()) => self.show_notice(format!("Saved to {}", dest.display()), cx),
                        Err(e) => self.show_notice(format!("Save failed: {e}"), cx),
                    }
                }
            }
            MessageAction::ToggleSpeak(ix) => {
                let maybe_row = self.messages.borrow().get(ix).cloned();
                if let Some(row) = maybe_row {
                    if self.speaking_node_id.as_deref() == Some(row.node_id.as_str()) {
                        stop_speaking_aloud();
                        self.speaking_node_id = None;
                    } else {
                        let plain = strip_markdown_to_plain_text(row.content.as_ref());
                        if !plain.trim().is_empty() {
                            let nid = row.node_id.clone();
                            let tx = self.bg_tx.clone();
                            self.pending_bg_tasks += 1;
                            speak_text_aloud(&plain, move || {
                                let _ = tx.send(BgTaskEvent::SpeechFinished(nid));
                            });
                            self.speaking_node_id = Some(row.node_id);
                            self.ensure_pump_scheduled(window, cx);
                        }
                    }
                    cx.notify();
                }
            }
            MessageAction::StartEdit(ix) => {
                if let Some(node) = self.active_session.messages.get(ix) {
                    let body = node.active_content().to_string();
                    self.editing_message_index = Some(ix);
                    self.edit_msg_input
                        .update(cx, |i, cx| i.set_value(&body, window, cx));
                    cx.notify();
                }
            }
            MessageAction::CancelEdit => {
                self.editing_message_index = None;
                cx.notify();
            }
            MessageAction::SaveEdit {
                index,
                regenerate,
                branch,
            } => {
                let updated = self.edit_msg_input.read(cx).value().to_string();
                self.editing_message_index = None;
                if branch {
                    let now = now_ms();
                    let mut forked = self.active_session.clone();
                    forked.id = format!("chat-branch-{now}");
                    forked.title = format!("{} (Branch)", self.active_session.title);
                    forked.created_at = now;
                    forked.updated_at = now;
                    forked
                        .messages
                        .truncate((index + 1).min(forked.messages.len()));
                    let mut is_user = false;
                    if let Some(node) = forked.messages.get_mut(index) {
                        is_user = node.role == MessageRole::User;
                        let vi = node
                            .active_variant_index
                            .min(node.variants.len().saturating_sub(1));
                        if let Some(v) = node.variants.get_mut(vi) {
                            v.content = updated;
                        }
                    }
                    let new_id = forked.id.clone();
                    let _ = self.core.save_full_session(&forked);
                    self.refresh_stubs();
                    self.select_chat_by_id(&new_id, window, cx);
                    self.show_notice("Saved edit into a new branched chat.", cx);
                    if regenerate || is_user {
                        self.launch_run_on_active_chat(None, false, window, cx);
                    }
                    return;
                }
                let mut is_user = false;
                if let Some(node) = self.active_session.messages.get_mut(index) {
                    is_user = node.role == MessageRole::User;
                    let vi = node
                        .active_variant_index
                        .min(node.variants.len().saturating_sub(1));
                    if let Some(v) = node.variants.get_mut(vi) {
                        v.content = updated;
                    }
                }
                if regenerate {
                    if is_user {
                        self.active_session.messages.truncate(index + 1);
                        self.persist_active_session();
                        self.rebuild_rows_from_active_session(cx, true);
                        self.launch_run_on_active_chat(None, false, window, cx);
                    } else {
                        self.execute_regenerate_turn(index, window, cx);
                    }
                } else {
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::Regenerate(ix) => {
                if self.snapshot.preferences.confirm_before_regenerating {
                    self.pending_confirm = Some(ConfirmDialogKind::RegenerateTurn {
                        index: ix,
                        replaces_reply: self
                            .snapshot
                            .preferences
                            .delete_previous_message_when_regenerating,
                    });
                    self.overlay = ActiveOverlay::ConfirmDialog;
                    cx.notify();
                } else {
                    self.execute_regenerate_turn(ix, window, cx);
                }
            }
            MessageAction::Continue(ix) => {
                if self
                    .active_session
                    .messages
                    .get(ix)
                    .is_some_and(|n| n.role == MessageRole::Assistant)
                {
                    self.launch_run_on_active_chat(Some(ix), true, window, cx);
                }
            }
            MessageAction::Translate(ix) => {
                self.spawn_translate_message(ix, window, cx);
            }
            MessageAction::SaveAsSkill(ix) => {
                if let Some(node) = self.active_session.messages.get(ix) {
                    let body = node.active_content().trim().to_string();
                    if body.is_empty() {
                        return;
                    }
                    let skills_dir = self.data_dir.join("skills");
                    let _ = std::fs::create_dir_all(&skills_dir);
                    let stem = sanitize_filename(&self.active_session.title);
                    let dest = skills_dir.join(format!("{stem}-{}.md", now_ms() % 10000));
                    match std::fs::write(&dest, body) {
                        Ok(()) => {
                            self.refresh_memory_and_skills();
                            self.show_notice(
                                format!(
                                    "Saved reply as skill {}.",
                                    dest.file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("skill.md")
                                ),
                                cx,
                            );
                        }
                        Err(e) => self.show_notice(format!("Could not save skill: {e}"), cx),
                    }
                }
            }
            MessageAction::Branch(ix) => {
                let now = now_ms();
                let mut forked = self.active_session.clone();
                forked.id = format!("chat-branch-{now}");
                forked.title = format!("{} (Branch)", self.active_session.title);
                forked.created_at = now;
                forked.updated_at = now;
                forked
                    .messages
                    .truncate((ix + 1).min(forked.messages.len()));
                if !self.snapshot.preferences.keep_message_versions_when_forking {
                    for node in &mut forked.messages {
                        if let Some(active) = node.active_variant().cloned() {
                            node.variants = vec![active];
                            node.active_variant_index = 0;
                        }
                    }
                }
                let new_id = forked.id.clone();
                let _ = self.core.save_full_session(&forked);
                self.refresh_stubs();
                self.select_chat_by_id(&new_id, window, cx);
                self.show_notice("Created branched chat.", cx);
            }
            MessageAction::Rewind(ix) => {
                if ix < self.active_session.messages.len() {
                    if self.active_session.messages[ix].role == MessageRole::User {
                        let prompt = self.active_session.messages[ix]
                            .active_content()
                            .to_string();
                        self.composer
                            .update(cx, |i, cx| i.set_value(&prompt, window, cx));
                    }
                    self.active_session.messages.truncate(ix);
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, true);
                    self.show_notice("Rewound conversation.", cx);
                }
            }
            MessageAction::TogglePin(ix) => {
                if let Some(node) = self.active_session.messages.get_mut(ix) {
                    node.pinned = !node.pinned;
                    let nid = node.id.clone();
                    let pinned = node.pinned;
                    let _ = self
                        .core
                        .set_message_pinned(&self.active_session.id, &nid, pinned);
                    self.persist_active_session();
                    self.refresh_pinned();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::ToggleContextMute(ix) => {
                if let Some(node) = self.active_session.messages.get_mut(ix) {
                    node.included_in_context = !node.included_in_context;
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::SelectVariant {
                index,
                variant_index,
            } => {
                if let Some(node) = self.active_session.messages.get_mut(index)
                    && variant_index < node.variants.len()
                {
                    node.active_variant_index = variant_index;
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::DeleteCurrentVariant(ix) => {
                if let Some(node) = self.active_session.messages.get_mut(ix) {
                    if node.variants.len() <= 1 {
                        self.active_session.messages.remove(ix);
                    } else {
                        let vi = node.active_variant_index.min(node.variants.len() - 1);
                        node.variants.remove(vi);
                        node.active_variant_index = vi.min(node.variants.len().saturating_sub(1));
                    }
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::DeleteNode(ix) => {
                if ix < self.active_session.messages.len() {
                    self.active_session.messages.remove(ix);
                    self.persist_active_session();
                    self.rebuild_rows_from_active_session(cx, false);
                    cx.notify();
                }
            }
            MessageAction::ToggleThinking(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                let current = entry
                    .thinking_expanded
                    .unwrap_or(!self.snapshot.preferences.auto_collapse_thinking);
                entry.thinking_expanded = Some(!current);
                cx.notify();
            }
            MessageAction::ToggleMoreActions(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.show_more_actions = !entry.show_more_actions;
                cx.notify();
            }
            MessageAction::ToggleSources(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.show_sources_drawer = !entry.show_sources_drawer;
                cx.notify();
            }
            MessageAction::ToggleRawWire(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.show_raw_wire_drawer = !entry.show_raw_wire_drawer;
                cx.notify();
            }
            MessageAction::ToggleUsageDetails(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.show_usage_details = !entry.show_usage_details;
                cx.notify();
            }
            MessageAction::ToggleExpandLong(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.expanded_long_message = !entry.expanded_long_message;
                cx.notify();
            }
            MessageAction::ToggleShowAllToolSteps(nid) => {
                let entry = self.turn_ui_states.entry(nid).or_default();
                entry.show_all_tool_steps = !entry.show_all_tool_steps;
                cx.notify();
            }
            MessageAction::ToggleQuietToolGroup { node_id, group_key } => {
                let entry = self.turn_ui_states.entry(node_id).or_default();
                if !entry.expanded_tool_groups.remove(&group_key) {
                    entry.expanded_tool_groups.insert(group_key);
                }
                cx.notify();
            }
            MessageAction::ToggleCodeCollapse {
                node_id,
                block_index,
            } => {
                let entry = self.turn_ui_states.entry(node_id).or_default();
                if !entry.toggled_code_collapse.remove(&block_index) {
                    entry.toggled_code_collapse.insert(block_index);
                }
                cx.notify();
            }
            MessageAction::ToggleCodeWrap {
                node_id,
                block_index,
            } => {
                let entry = self.turn_ui_states.entry(node_id).or_default();
                if !entry.toggled_code_wrap.remove(&block_index) {
                    entry.toggled_code_wrap.insert(block_index);
                }
                cx.notify();
            }
            MessageAction::InspectAttachment(att) => self.open_attachment_in_viewer(att, cx),
            MessageAction::InspectGeneratedMedia(m) => {
                let title = if !m.revised_prompt.trim().is_empty() {
                    m.revised_prompt.chars().take(60).collect()
                } else {
                    format!("{:?} media", m.kind)
                };
                self.open_viewer_item(
                    ActiveMediaViewerItem::Image {
                        title,
                        local_file_path: m.local_file_path,
                        remote_url: m.remote_url,
                        mime_type: m.mime_type,
                    },
                    cx,
                );
            }
            MessageAction::InspectSvg { title, raw_svg } => {
                self.open_viewer_item(
                    ActiveMediaViewerItem::TextOrCode {
                        title,
                        text_content: raw_svg,
                        mime_type: "image/svg+xml".into(),
                        local_file_path: String::new(),
                    },
                    cx,
                );
            }
            MessageAction::OpenUrl(url) => {
                if let Err(err) = open_web_link(&url) {
                    self.show_notice(err, cx);
                }
            }
        }
    }

    pub(super) fn handle_sidebar_action(
        &mut self,
        action: SidebarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SidebarAction::SelectTab(tab) => {
                self.sidebar_tab = tab;
                let ph = match tab {
                    SidebarTab::Assistants => strings::SEARCH_ASSISTANTS,
                    SidebarTab::Chats => strings::SEARCH_CHATS,
                    SidebarTab::Pinned => strings::SEARCH_PINNED,
                };
                self.sidebar_search_input
                    .update(cx, |i, cx| i.set_placeholder(ph, window, cx));
                if tab == SidebarTab::Pinned {
                    self.refresh_pinned();
                }
                cx.notify();
            }
            SidebarAction::NewChat => self.create_new_chat(window, cx),
            SidebarAction::NewAssistant => {
                let now = now_ms();
                let mut a = default_assistant_agent();
                a.id = format!("asst-{now}");
                a.name = format!("Assistant {}", self.snapshot.assistants.len() + 1);
                let aid = a.id.clone();
                self.snapshot.assistants.push(a);
                self.snapshot.active_assistant_id = aid.clone();
                self.persist_settings();
                self.open_settings_at(SettingsSection::Assistants, window, cx);
                self.assistant_subpage = AssistantSubpage::Edit(aid);
                cx.notify();
            }
            SidebarAction::SelectAssistant(aid) => {
                self.snapshot.active_assistant_id = aid.clone();
                self.persist_settings();
                if self.snapshot.preferences.auto_switch_to_topics {
                    self.sidebar_tab = SidebarTab::Chats;
                }
                if self.snapshot.preferences.new_chat_when_switching_assistants {
                    self.create_new_chat(window, cx);
                } else if let Some(stub) = self
                    .stubs
                    .iter()
                    .find(|s| s.assistant_id == aid)
                    .map(|s| s.id.clone())
                {
                    self.select_chat_by_id(&stub, window, cx);
                } else {
                    cx.notify();
                }
            }
            SidebarAction::EditAssistant(aid) => {
                self.snapshot.active_assistant_id = aid.clone();
                self.persist_settings();
                self.open_settings_at(SettingsSection::Assistants, window, cx);
                self.assistant_subpage = AssistantSubpage::Edit(aid);
                cx.notify();
            }
            SidebarAction::SelectChat(cid) => self.select_chat_by_id(&cid, window, cx),
            SidebarAction::TogglePinChat(cid) => {
                if cid == self.active_session.id {
                    self.active_session.is_pinned = !self.active_session.is_pinned;
                    self.persist_active_session();
                } else if let Ok(Some(mut sess)) = self.core.load_full_session(&cid) {
                    sess.is_pinned = !sess.is_pinned;
                    let _ = self.core.save_full_session(&sess);
                    self.refresh_stubs();
                }
                cx.notify();
            }
            SidebarAction::RegenerateChatTitle(cid) => {
                self.spawn_generate_title(&cid, window, cx);
            }
            SidebarAction::DeleteChat(cid) => {
                if self.snapshot.preferences.confirm_before_deleting_chat {
                    let title = self
                        .stubs
                        .iter()
                        .find(|s| s.id == cid)
                        .map(|s| s.title.clone())
                        .unwrap_or_else(|| self.active_session.title.clone());
                    self.pending_confirm = Some(ConfirmDialogKind::DeleteSingleChat {
                        chat_id: cid,
                        title,
                    });
                    self.overlay = ActiveOverlay::ConfirmDialog;
                    cx.notify();
                } else {
                    self.delete_chat_with_undo(&cid, window, cx);
                }
            }
            SidebarAction::ClearAllForAssistant => {
                if self.snapshot.preferences.confirm_before_deleting_chat {
                    let aid = self.snapshot.active_assistant_id.clone();
                    let count = self
                        .stubs
                        .iter()
                        .filter(|s| {
                            self.snapshot.assistants.len() <= 1
                                || s.assistant_id.is_empty()
                                || s.assistant_id == aid
                        })
                        .count();
                    let assistant_name = self
                        .active_assistant()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| strings::DEFAULT_ASSISTANT.into());
                    self.pending_confirm = Some(ConfirmDialogKind::ClearAllChats {
                        assistant_name,
                        count,
                    });
                    self.overlay = ActiveOverlay::ConfirmDialog;
                    cx.notify();
                } else {
                    self.execute_clear_all_for_assistant(window, cx);
                }
            }
            SidebarAction::OpenPinned {
                chat_id,
                message_id,
            } => {
                if chat_id != self.active_session.id {
                    self.select_chat_by_id(&chat_id, window, cx);
                }
                let maybe_ix = self
                    .messages
                    .borrow()
                    .iter()
                    .position(|m| m.node_id == message_id);
                if let Some(ix) = maybe_ix {
                    self.scroller.update(cx, |s, cx| s.scroll_to_item(ix, cx));
                }
                cx.notify();
            }
            SidebarAction::SetPinnedScopeAll(all) => {
                self.pinned_all_chats = all;
                cx.notify();
            }
            SidebarAction::UnpinReply {
                chat_id,
                message_id,
            } => {
                let _ = self.core.set_message_pinned(&chat_id, &message_id, false);
                if chat_id == self.active_session.id {
                    if let Some(n) = self
                        .active_session
                        .messages
                        .iter_mut()
                        .find(|m| m.id == message_id)
                    {
                        n.pinned = false;
                    }
                    self.rebuild_rows_from_active_session(cx, false);
                }
                self.refresh_pinned();
                cx.notify();
            }
        }
    }

    pub(super) fn handle_vault_action(
        &mut self,
        action: VaultAction,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            VaultAction::SetFilter(f) => {
                self.vault_filter = f;
                cx.notify();
            }
            VaultAction::InspectWorkspaceFile {
                path,
                fallback_content,
            } => {
                let ws_root = self.resolved_policy().workspace_path;
                let (live_text, full_path) = if !ws_root.trim().is_empty() {
                    match WorkspacePathGuard::resolve(&ws_root, &path) {
                        Ok(resolved) => {
                            let content = std::fs::read_to_string(&resolved)
                                .unwrap_or_else(|_| fallback_content.clone());
                            (content, resolved.to_string_lossy().to_string())
                        }
                        Err(_) => (fallback_content, String::new()),
                    }
                } else {
                    (fallback_content, String::new())
                };
                let ext = PathBuf::from(&path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                self.open_viewer_item(
                    ActiveMediaViewerItem::TextOrCode {
                        title: path,
                        text_content: live_text,
                        mime_type: guess_mime_type(&ext).to_string(),
                        local_file_path: full_path,
                    },
                    cx,
                );
            }
            VaultAction::InspectCodeSnippet {
                title,
                code,
                mime_type,
            } => {
                self.open_viewer_item(
                    ActiveMediaViewerItem::TextOrCode {
                        title,
                        text_content: code,
                        mime_type,
                        local_file_path: String::new(),
                    },
                    cx,
                );
            }
            VaultAction::InspectGeneratedMedia(m) => {
                let title = if !m.revised_prompt.trim().is_empty() {
                    m.revised_prompt.chars().take(60).collect()
                } else {
                    format!("{:?} media", m.kind)
                };
                self.open_viewer_item(
                    ActiveMediaViewerItem::Image {
                        title,
                        local_file_path: m.local_file_path,
                        remote_url: m.remote_url,
                        mime_type: m.mime_type,
                    },
                    cx,
                );
            }
            VaultAction::InspectAttachment(att) => self.open_attachment_in_viewer(att, cx),
            VaultAction::ReattachAttachment(mut att) => {
                att.id = format!("reatt-{}", now_ms());
                self.pending_attachments.push(att);
                self.show_notice("Attached file to composer.", cx);
            }
            VaultAction::OpenUrl(url) => {
                if let Err(err) = open_web_link(&url) {
                    self.show_notice(err, cx);
                }
            }
            VaultAction::CopySnippet(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.show_notice("Copied to clipboard.", cx);
            }
            VaultAction::CloseVault => {
                self.vault_open = false;
                cx.notify();
            }
        }
    }

    pub(super) fn handle_notes_action(
        &mut self,
        action: NotesAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            NotesAction::SummarizeChat => self.spawn_summarize_or_compact(false, window, cx),
            NotesAction::CompactContext => self.spawn_summarize_or_compact(true, window, cx),
            NotesAction::ExportMarkdown => self.export_active_chat_markdown(cx),
            NotesAction::ClearMessages => {
                if self.snapshot.preferences.confirm_before_deleting_chat {
                    self.pending_confirm = Some(ConfirmDialogKind::ClearCurrentMessages {
                        title: self.active_session.title.clone(),
                    });
                    self.overlay = ActiveOverlay::ConfirmDialog;
                    cx.notify();
                } else {
                    self.execute_clear_current_messages(cx);
                }
            }
            NotesAction::CloseNotes => {
                self.notes_open = false;
                cx.notify();
            }
        }
    }

    pub(super) fn export_active_chat_markdown(&mut self, cx: &mut Context<Self>) {
        let default_name = format!("{}.md", sanitize_filename(&self.active_session.title));
        if let Some(dest) = pick_save_file_dialog("Export chat as Markdown", &default_name, "md") {
            let mut out = format!("# {}\n\n", self.active_session.title);
            for node in &self.active_session.messages {
                let speaker = if node.role == MessageRole::User {
                    self.active_session.user_name.as_str()
                } else {
                    "Assistant"
                };
                out.push_str(&format!("## {speaker}\n\n{}\n\n", node.active_content()));
            }
            match std::fs::write(&dest, out) {
                Ok(()) => self.show_notice(format!("Exported to {}", dest.display()), cx),
                Err(e) => self.show_notice(format!("Export failed: {e}"), cx),
            }
        }
    }

    pub(super) fn queue_current_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let cid = self.active_session.id.clone();
        self.queued_prompts
            .entry(cid)
            .or_default()
            .push(QueuedPrompt {
                id: format!("q-{}", now_ms()),
                text,
            });
        self.composer
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.active_session.draft_input.clear();
        cx.notify();
    }

    pub(super) fn steer_with_text(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let cid = self.active_session.id.clone();
        self.stop_run_for_chat(&cid, cx);
        self.composer
            .update(cx, |i, cx| i.set_value(&trimmed, window, cx));
        self.send_composer_message(window, cx);
    }

    pub(super) fn attach_local_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = pick_open_file_dialog("Attach file", "All files", "*.*") else {
            return;
        };
        self.attach_file_from_path(&path, cx);
    }

    pub(super) fn attach_file_from_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                self.show_notice(format!("Could not read file: {e}"), cx);
                return;
            }
        };
        if meta.len() > MAX_ATTACHMENT_BYTES as u64 {
            self.show_notice("Attachment exceeds 50 MiB limit.", cx);
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.show_notice(format!("Could not read file: {e}"), cx);
                return;
            }
        };
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment")
            .to_string();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime_type = guess_mime_type(&ext).to_string();

        let (text_content, base64_data) = if ext == "docx" {
            match ParchmentCore::extract_docx_native(&bytes) {
                Ok(txt) => (txt, String::new()),
                Err(_) => (
                    String::new(),
                    base64::engine::general_purpose::STANDARD.encode(&bytes),
                ),
            }
        } else if is_inline_text_mime(&mime_type, &ext) && bytes.len() <= MAX_INLINE_TEXT_BYTES {
            match String::from_utf8(bytes.clone()) {
                Ok(txt) => (txt, String::new()),
                Err(_) => (
                    String::new(),
                    base64::engine::general_purpose::STANDARD.encode(&bytes),
                ),
            }
        } else {
            (
                String::new(),
                base64::engine::general_purpose::STANDARD.encode(&bytes),
            )
        };

        self.pending_attachments.push(PromptAttachment {
            id: format!("att-{}", now_ms()),
            file_name,
            mime_type,
            base64_data,
            text_content,
            local_file_path: path.to_string_lossy().to_string(),
        });
        cx.notify();
    }

    pub(super) fn run_slash_command(
        &mut self,
        cmd: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composer
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.active_session.draft_input.clear();
        let trimmed = cmd.trim();
        match trimmed.to_ascii_lowercase().as_str() {
            "/plan" => {
                self.update_active_permissions(
                    |p| {
                        p.workspace_mode = WorkspaceMode::ReadOnly;
                        p.shell_access = ShellAccess::Disabled;
                    },
                    cx,
                );
                self.show_notice("Switched to Plan mode (read-only folder access).", cx);
            }
            "/build" => {
                self.update_active_permissions(|p| p.workspace_mode = WorkspaceMode::ReadWrite, cx);
                self.show_notice("Switched to Build mode (read & write folder access).", cx);
            }
            "/compact" => self.spawn_summarize_or_compact(true, window, cx),
            "/summarize" => self.spawn_summarize_or_compact(false, window, cx),
            "/continue" => {
                if let Some(last_asst) = self
                    .active_session
                    .messages
                    .iter()
                    .rposition(|m| m.role == MessageRole::Assistant)
                {
                    self.launch_run_on_active_chat(Some(last_asst), true, window, cx);
                } else {
                    self.show_notice("No assistant reply to continue yet.", cx);
                }
            }
            "/rewind" => {
                if !self.active_session.messages.is_empty() {
                    let last = self.active_session.messages.len() - 1;
                    self.handle_message_action(MessageAction::Rewind(last), window, cx);
                }
            }
            "/clear" => {
                self.handle_notes_action(NotesAction::ClearMessages, window, cx);
            }
            other => {
                // Check custom workspace slash commands `.parchment/commands/<stem>.md`
                let stem = other.trim_start_matches('/');
                let ws = self.resolved_policy().workspace_path;
                if !ws.trim().is_empty() && !stem.is_empty() {
                    let cmd_file = PathBuf::from(&ws)
                        .join(".parchment")
                        .join("commands")
                        .join(format!("{stem}.md"));
                    if let Ok(template) = std::fs::read_to_string(&cmd_file)
                        && !template.trim().is_empty()
                    {
                        self.composer
                            .update(cx, |i, cx| i.set_value(&template, window, cx));
                        self.active_session.draft_input = template;
                        self.show_notice(format!("Loaded custom command {other}."), cx);
                        return;
                    }
                }
                self.show_notice(format!("Unknown command: {other}"), cx);
            }
        }
    }

    pub(super) fn handle_composer_action(
        &mut self,
        action: ComposerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ComposerAction::Send => self.send_composer_message(window, cx),
            ComposerAction::Stop => {
                self.is_drafting_reply = false;
                let cid = self.active_session.id.clone();
                self.stop_run_for_chat(&cid, cx);
                cx.notify();
            }
            ComposerAction::RetryNow => {
                let cid = self.active_session.id.clone();
                if let Some(r) = self.active_runs.get_mut(&cid) {
                    r.retry_now.notify_one();
                    r.retry_banner = None;
                    r.status_line = "Retrying now…".into();
                    cx.notify();
                }
            }
            ComposerAction::ToggleRetryCollapse => {
                self.retry_banner_collapsed = !self.retry_banner_collapsed;
                cx.notify();
            }
            ComposerAction::QueuePrompt => self.queue_current_draft(window, cx),
            ComposerAction::SteerNow => {
                let text = self.composer.read(cx).value().to_string();
                self.steer_with_text(text, window, cx);
            }
            ComposerAction::RemoveQueuedPrompt(qid) => {
                if let Some(list) = self.queued_prompts.get_mut(&self.active_session.id) {
                    list.retain(|q| q.id != qid);
                }
                cx.notify();
            }
            ComposerAction::SteerQueuedPrompt(qid) => {
                let text = self
                    .queued_prompts
                    .get_mut(&self.active_session.id)
                    .and_then(|list| {
                        list.iter()
                            .position(|q| q.id == qid)
                            .map(|ix| list.remove(ix).text)
                    });
                if let Some(t) = text {
                    self.steer_with_text(t, window, cx);
                }
            }
            ComposerAction::AttachFiles => self.attach_local_file(cx),
            ComposerAction::InspectAttachment(att) => self.open_attachment_in_viewer(att, cx),
            ComposerAction::RemoveAttachment(id) => {
                self.pending_attachments.retain(|a| a.id != id);
                cx.notify();
            }
            ComposerAction::ToggleVoiceInput => {
                if self.is_voice_listening {
                    stop_voice_input();
                    self.is_voice_listening = false;
                    cx.notify();
                } else {
                    self.is_voice_listening = true;
                    self.pending_bg_tasks += 1;
                    self.show_notice("Listening… Speak now.", cx);
                    let tx = self.bg_tx.clone();
                    start_voice_input(move |res| {
                        let _ = tx.send(BgTaskEvent::VoiceRecognized(res));
                    });
                    self.ensure_pump_scheduled(window, cx);
                    cx.notify();
                }
            }
            ComposerAction::ToggleToolsDrawer => {
                self.show_tools_drawer = !self.show_tools_drawer;
                if self.show_tools_drawer {
                    self.show_thinking_popover = false;
                    self.show_modality_popover = false;
                    self.show_context_popover = false;
                }
                cx.notify();
            }
            ComposerAction::ToggleThinkingPopover => {
                self.show_thinking_popover = !self.show_thinking_popover;
                if self.show_thinking_popover {
                    self.show_tools_drawer = false;
                    self.show_modality_popover = false;
                    self.show_context_popover = false;
                }
                cx.notify();
            }
            ComposerAction::SetThinkingEffort(eff) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.thinking_effort = eff;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::ToggleModalityPopover => {
                self.show_modality_popover = !self.show_modality_popover;
                if self.show_modality_popover {
                    self.show_tools_drawer = false;
                    self.show_thinking_popover = false;
                    self.show_context_popover = false;
                }
                cx.notify();
            }
            ComposerAction::SetImageAspect(aspect) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.media_aspect_ratio = aspect;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::SetImageResolution(res) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.media_resolution = res;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::SetVideoAspect(aspect) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.media_aspect_ratio = aspect;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::SetVideoDuration(secs) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.video_duration_seconds = secs;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::ToggleVideoAudio => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.video_generate_audio = !p.video_generate_audio;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::SetAudioVoice(voice) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.audio_voice = voice;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::SetAudioSpeed(speed) => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.audio_speed = speed;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::ToggleContextPopover => {
                self.show_context_popover = !self.show_context_popover;
                if self.show_context_popover {
                    self.show_tools_drawer = false;
                    self.show_thinking_popover = false;
                    self.show_modality_popover = false;
                }
                cx.notify();
            }
            ComposerAction::ToggleBackgroundUsage => {
                self.show_background_usage = !self.show_background_usage;
                cx.notify();
            }
            ComposerAction::OpenPricingSettings => {
                let prof = self.active_profile();
                let mid = self.effective_model_id(&prof);
                self.show_context_popover = false;
                self.open_settings_at(SettingsSection::Providers, window, cx);
                if !mid.is_empty() {
                    self.sync_model_cfg_inputs(&mid, window, cx);
                    self.providers_subpage = ProvidersSubpage::ModelConfig(mid);
                }
                cx.notify();
            }
            ComposerAction::ToggleGoalMode => {
                self.goal_mode_enabled = !self.goal_mode_enabled;
                cx.notify();
            }
            ComposerAction::TogglePrefillBox => {
                self.show_prefill_box = !self.show_prefill_box;
                if !self.show_prefill_box {
                    self.prefill_input
                        .update(cx, |i, cx| i.set_value("", window, cx));
                    self.active_session.draft_prefill.clear();
                }
                cx.notify();
            }
            ComposerAction::ToggleToolsEnabled => {
                self.update_active_permissions(|p| p.tools_enabled = !p.tools_enabled, cx);
            }
            ComposerAction::ResetPermissionOverrides => {
                self.handle_settings_action(
                    SettingsAction::ResetChatWorkspaceToAssistant,
                    window,
                    cx,
                );
            }
            ComposerAction::PickWorkspaceFolder => {
                if let Some(dir) = pick_folder_dialog("Add workspace folder") {
                    let s = dir.to_string_lossy().to_string();
                    self.update_active_permissions(|p| p.add_workspace_path(s), cx);
                }
            }
            ComposerAction::RemoveWorkspaceFolder(path) => {
                self.update_active_permissions(|p| p.remove_workspace_path(&path), cx);
            }
            ComposerAction::ClearWorkspaceFolder => {
                self.update_active_permissions(|p| p.clear_workspace_paths(), cx);
            }
            ComposerAction::SetWorkspaceMode(mode) => {
                self.update_active_permissions(|p| p.workspace_mode = mode, cx);
            }
            ComposerAction::SetShellAccess(mode) => {
                self.update_active_permissions(
                    |p| {
                        if mode == ShellAccess::FullAccess {
                            p.workspace_mode = WorkspaceMode::ReadWrite;
                            p.shell_access = ShellAccess::FullAccess;
                        } else {
                            p.shell_access = ShellAccess::Disabled;
                        }
                    },
                    cx,
                );
            }
            ComposerAction::ToggleWebAccess => {
                self.update_active_permissions(|p| p.web_access = !p.web_access, cx);
            }
            ComposerAction::ToggleComputerAccess => {
                self.update_active_permissions(|p| p.computer_access = !p.computer_access, cx);
            }
            ComposerAction::SetApprovalMode(mode) => {
                self.update_active_permissions(|p| p.approval_mode = mode, cx);
            }
            ComposerAction::ToggleProviderWebSearch => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.enable_web_search = !p.enable_web_search;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::ToggleProviderCodeExec => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.enable_code_execution = !p.enable_code_execution;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::ToggleProviderUrlContext => {
                let pid = self.active_profile().id;
                if let Some(p) = self.snapshot.profiles.iter_mut().find(|p| p.id == pid) {
                    p.enable_url_context = !p.enable_url_context;
                }
                self.persist_settings();
                cx.notify();
            }
            ComposerAction::RunSlashCommand(cmd) => self.run_slash_command(&cmd, window, cx),
            ComposerAction::InsertAtMention(val) => {
                if val == "__attach__" {
                    self.strip_trailing_at_token(window, cx);
                    self.attach_local_file(cx);
                } else if val == "__mount__" {
                    self.strip_trailing_at_token(window, cx);
                    self.handle_composer_action(ComposerAction::PickWorkspaceFolder, window, cx);
                } else {
                    let cur = self.composer.read(cx).value().to_string();
                    let next = if let Some(idx) = cur.rfind('@') {
                        format!("{}{val}", &cur[..idx])
                    } else {
                        format!("{cur}{val}")
                    };
                    self.composer
                        .update(cx, |i, cx| i.set_value(&next, window, cx));
                    self.active_session.draft_input = next;
                    cx.notify();
                }
            }
            ComposerAction::CompactContext => self.spawn_summarize_or_compact(true, window, cx),
            ComposerAction::SummarizeChat => self.spawn_summarize_or_compact(false, window, cx),
            ComposerAction::ToggleInlineToolRegistry => {
                self.show_inline_tool_registry = !self.show_inline_tool_registry;
                cx.notify();
            }
            ComposerAction::ToggleBuiltinTool(name, enabled) => {
                self.update_active_permissions(
                    |p| {
                        p.tool_overrides.insert(name, enabled);
                    },
                    cx,
                );
            }
            ComposerAction::DraftMyReply => {
                self.show_tools_drawer = false;
                self.spawn_draft_impersonate(window, cx);
            }
            ComposerAction::GuideNextReply => {
                self.show_tools_drawer = false;
                self.send_guided_direction(window, cx);
            }
        }
    }

    pub(super) fn spawn_draft_impersonate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.active_profile();
        if let Some(reason) = self.validate_provider_ready(&profile) {
            self.show_notice(reason, cx);
            return;
        }
        let model = self.effective_model_id(&profile);
        let transcript: String = self
            .active_session
            .messages
            .iter()
            .filter(|m| m.included_in_context)
            .rev()
            .take(16)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|m| {
                let who = if m.role == MessageRole::User {
                    "User"
                } else {
                    "Assistant"
                };
                format!("{who}: {}", m.active_content())
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let sys = "Write the next user message in the user's voice matching the tone, style, and context of this conversation. Output only the message text with no quotes or preamble.";
        let user_prompt = if transcript.trim().is_empty() {
            "Write a compelling opening message to start this conversation.".to_string()
        } else {
            transcript
        };
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        self.is_drafting_reply = true;
        self.pending_bg_tasks += 1;
        self.core.runtime().spawn(async move {
            let client = core_bg.protocol_client();
            let res = client
                .run_utility_prompt(&profile, &model, sys, &user_prompt, "impersonate")
                .await
                .map(|(txt, _)| txt)
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::DraftImpersonated(res));
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn send_guided_direction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.composer.read(cx).value().trim().to_string();
        if dir.is_empty() {
            self.show_notice(
                "Type a direction in the input box first, then click Guide next reply.",
                cx,
            );
            return;
        }
        self.pending_guided_direction = Some(dir);
        self.composer
            .update(cx, |i, cx| i.set_value("", window, cx));
        self.active_session.draft_input.clear();
        self.launch_run_on_active_chat(None, false, window, cx);
    }
    pub(super) fn strip_trailing_at_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cur = self.composer.read(cx).value().to_string();
        if let Some(idx) = cur.rfind('@') {
            let cleaned = cur[..idx].to_string();
            self.composer
                .update(cx, |i, cx| i.set_value(&cleaned, window, cx));
            self.active_session.draft_input = cleaned;
        }
    }

    pub(super) fn spawn_test_provider_connection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prof) = self
            .snapshot
            .profiles
            .iter()
            .find(|p| p.id == self.snapshot.active_profile_id)
            .or_else(|| self.snapshot.profiles.first())
            .cloned()
        else {
            return;
        };
        self.provider_test_status = "Testing connection…".into();
        self.pending_bg_tasks += 1;
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        let pid = prof.id.clone();
        self.core.runtime().spawn(async move {
            let started = Instant::now();
            let client = core_bg.protocol_client();
            let res = client
                .fetch_available_models(&prof)
                .await
                .map(|models| {
                    format!(
                        "Connected ({} ms) — {} model{} available.",
                        started.elapsed().as_millis(),
                        models.len(),
                        if models.len() == 1 { "" } else { "s" }
                    )
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::ProviderTested {
                profile_id: pid,
                result: res,
            });
        });
        self.ensure_pump_scheduled(window, cx);
        cx.notify();
    }

    pub(super) fn cycle_tabs_prev(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        let cur = self
            .open_tabs
            .iter()
            .position(|id| id == &self.active_session.id)
            .unwrap_or(0);
        let prev_idx = (cur + self.open_tabs.len() - 1) % self.open_tabs.len();
        if let Some(prev) = self.open_tabs.get(prev_idx).cloned() {
            self.select_chat_by_id(&prev, window, cx);
        }
    }
    pub(super) fn shortcut_hints_for_command(&self, cmd_id: &str) -> Vec<String> {
        let custom = &self.snapshot.preferences.custom_shortcuts;
        match cmd_id {
            "new_chat" => resolve_shortcut(AppShortcutAction::NewChat, custom).keycap_tokens(),
            "toggle_sidebar" => {
                resolve_shortcut(AppShortcutAction::ToggleSidebar, custom).keycap_tokens()
            }
            "toggle_vault" => vec!["Ctrl".into(), "Shift".into(), "F".into()],
            "prompt_jump" => {
                resolve_shortcut(AppShortcutAction::PromptJump, custom).keycap_tokens()
            }
            "open_settings" => {
                resolve_shortcut(AppShortcutAction::ToggleSettings, custom).keycap_tokens()
            }
            _ => Vec::new(),
        }
    }
}
