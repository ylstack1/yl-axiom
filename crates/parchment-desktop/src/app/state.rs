use super::*;

impl ParchmentDesktopApp {
    pub(super) fn persist_settings(&self) {
        let _ = self.core.save_settings_snapshot(&self.snapshot);
    }

    pub(super) fn persist_active_session(&mut self) {
        self.active_session.updated_at = now_ms();
        let _ = self.core.save_full_session(&self.active_session);
        self.refresh_stubs();
    }

    pub(super) fn refresh_stubs(&mut self) {
        if let Ok(list) = self.core.list_chat_stubs() {
            self.stubs = list;
        }
    }

    pub(super) fn refresh_pinned(&mut self) {
        if let Ok(list) = self.core.list_pinned_message_stubs(None, 80) {
            self.pinned_stubs = list;
        }
    }

    pub(super) fn active_profile(&self) -> ProviderProfile {
        let wanted_id = if !self.active_session.profile_id.is_empty() {
            self.active_session.profile_id.as_str()
        } else {
            self.snapshot.active_profile_id.as_str()
        };
        self.snapshot
            .profiles
            .iter()
            .find(|p| p.id == wanted_id)
            .or_else(|| {
                self.snapshot
                    .profiles
                    .iter()
                    .find(|p| p.id == self.snapshot.active_profile_id)
            })
            .or_else(|| self.snapshot.profiles.first())
            .cloned()
            .unwrap_or_else(|| make_default_profile(ProviderProtocol::OpenAiResponses))
    }

    pub(super) fn active_assistant(&self) -> Option<&AssistantAgent> {
        let wanted = if !self.active_session.assistant_id.is_empty() {
            self.active_session.assistant_id.as_str()
        } else {
            self.snapshot.active_assistant_id.as_str()
        };
        self.snapshot
            .assistants
            .iter()
            .find(|a| a.id == wanted)
            .or_else(|| {
                self.snapshot
                    .assistants
                    .iter()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
            })
            .or_else(|| self.snapshot.assistants.first())
    }

    pub(super) fn effective_model_id(&self, profile: &ProviderProfile) -> String {
        if !self.active_session.model_override.trim().is_empty() {
            return self.active_session.model_override.trim().to_string();
        }
        if let Some(asst) = self.active_assistant()
            && !asst.default_model_id.trim().is_empty()
        {
            return asst.default_model_id.trim().to_string();
        }
        profile.default_model.trim().to_string()
    }

    pub(super) fn resolved_policy(&self) -> AgentPermissionPolicy {
        let prof = self.active_profile();
        AgentPermissionResolver::resolve(&self.active_session, self.active_assistant(), Some(&prof))
    }

    pub(super) fn update_active_permissions(
        &mut self,
        f: impl FnOnce(&mut AgentPermissionPolicy),
        cx: &mut Context<Self>,
    ) {
        let mut pol = self.resolved_policy();
        f(&mut pol);
        if pol.workspace_mode != WorkspaceMode::ReadWrite {
            pol.shell_access = ShellAccess::Disabled;
        }
        self.active_session.workspace_path = pol.workspace_path.clone();
        self.active_session.workspace_mode = pol.workspace_mode;
        self.active_session.permission_overrides = Some(pol.as_overrides());
        self.persist_active_session();
        self.refresh_workspace_files();
        cx.notify();
    }

    pub(super) fn refresh_workspace_files(&mut self) {
        let roots = self.resolved_policy().all_workspace_paths();
        self.custom_slash_commands.clear();
        self.workspace_entries.clear();
        if roots.is_empty() {
            self.project_memory_files.clear();
            return;
        }
        let multi = roots.len() > 1;
        for path in &roots {
            let prefix = if multi {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| format!("{n}/"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            if let Ok(listing) = WorkspacePathGuard::list_dir(path, "", false, 120) {
                for line in listing.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    self.workspace_entries.push(format!("{prefix}{line}"));
                }
            }
        }
        let path = &roots[0];
        // Load custom workspace slash commands from `.parchment/commands/*.md`
        let cmd_dir = PathBuf::from(path).join(".parchment").join("commands");
        if let Ok(entries) = std::fs::read_dir(&cmd_dir) {
            for ent in entries.flatten() {
                let p = ent.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                {
                    let desc = std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|t| {
                            t.lines().find(|l| !l.trim().is_empty()).map(|l| {
                                l.trim()
                                    .trim_start_matches('#')
                                    .trim()
                                    .chars()
                                    .take(72)
                                    .collect::<String>()
                            })
                        })
                        .unwrap_or_else(|| "Workspace custom command".into());
                    self.custom_slash_commands.push((format!("/{stem}"), desc));
                }
            }
        }
        self.refresh_memory_and_skills();
    }

    pub(super) fn refresh_memory_and_skills(&mut self) {
        let gmem = self.data_dir.join("memory").join("AGENT.md");
        self.global_memory_bytes = std::fs::metadata(&gmem).ok().map(|m| m.len());

        self.project_memory_files.clear();
        let ws = self.resolved_policy().workspace_path;
        if !ws.trim().is_empty() {
            let root = PathBuf::from(&ws);
            for fname in ["AGENTS.md", "CLAUDE.md", "PARCHMENT.md"] {
                let fp = root.join(fname);
                if fp.is_file() {
                    self.project_memory_files
                        .push((fname.to_string(), fp.to_string_lossy().to_string()));
                }
            }
        }

        self.installed_skills.clear();
        let mut dirs: Vec<(PathBuf, &str)> = vec![(self.data_dir.join("skills"), "Global skill")];
        if !ws.trim().is_empty() {
            dirs.push((
                PathBuf::from(&ws).join(".parchment").join("skills"),
                "Workspace skill",
            ));
        }
        for (dir, label) in dirs {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for ent in entries.flatten() {
                    let p = ent.path();
                    if p.is_file()
                        && p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                    {
                        let name = p
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("skill")
                            .to_string();
                        self.installed_skills.push(InstalledSkillInfo {
                            name,
                            path: p.to_string_lossy().to_string(),
                            source_label: label.to_string(),
                        });
                    }
                }
            }
        }
    }

    pub(super) fn refresh_storage_scan(&mut self) {
        let mut referenced: HashSet<String> = HashSet::new();
        if !self.snapshot.preferences.profile_picture_path.is_empty() {
            referenced.insert(
                self.snapshot
                    .preferences
                    .profile_picture_path
                    .replace('\\', "/")
                    .to_ascii_lowercase(),
            );
        }
        for node in &self.active_session.messages {
            for v in &node.variants {
                for att in &v.attachments {
                    if !att.local_file_path.is_empty() {
                        referenced
                            .insert(att.local_file_path.replace('\\', "/").to_ascii_lowercase());
                    }
                }
                for m in &v.media_items {
                    if !m.local_file_path.is_empty() {
                        referenced
                            .insert(m.local_file_path.replace('\\', "/").to_ascii_lowercase());
                    }
                }
            }
        }

        let mut files = Vec::new();
        let root = self.data_dir.clone();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for ent in entries.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Ok(meta) = ent.metadata() {
                    let full_str = p.to_string_lossy().to_string();
                    let rel = p
                        .strip_prefix(&root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let rel_low = rel.to_ascii_lowercase();
                    let fname = p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    let category = if rel_low.ends_with(".db")
                        || rel_low.ends_with(".db-wal")
                        || rel_low.ends_with(".db-shm")
                    {
                        StorageFileCategory::Database
                    } else if rel_low.starts_with("media/") || rel_low.starts_with("generated/") {
                        StorageFileCategory::GeneratedMedia
                    } else if rel_low.starts_with("uploads/") || rel_low.starts_with("attachments/")
                    {
                        StorageFileCategory::Uploads
                    } else if rel_low.contains("backup") || rel_low.ends_with(".parchment") {
                        StorageFileCategory::Backups
                    } else if rel_low.contains("crash")
                        || rel_low.contains("temp")
                        || rel_low.contains("ota")
                        || rel_low.ends_with(".log")
                        || rel_low.ends_with(".tmp")
                    {
                        StorageFileCategory::TempOrLogs
                    } else {
                        StorageFileCategory::Backups
                    };
                    let norm_full = full_str.replace('\\', "/").to_ascii_lowercase();
                    let is_orphan_or_temp = category == StorageFileCategory::TempOrLogs
                        || (matches!(
                            category,
                            StorageFileCategory::Uploads | StorageFileCategory::GeneratedMedia
                        ) && !referenced.contains(&norm_full)
                            && !referenced.contains(&rel_low));
                    files.push(StoredFileEntry {
                        file_name: fname,
                        relative_path: rel,
                        full_path: full_str,
                        size_bytes: meta.len(),
                        category,
                        is_orphan_or_temp,
                    });
                }
            }
        }
        files.sort_by_key(|a| std::cmp::Reverse(a.size_bytes));
        self.storage_files = files;
    }

    pub(super) fn open_viewer_item(&mut self, item: ActiveMediaViewerItem, cx: &mut Context<Self>) {
        self.viewer_zoom = 1.0;
        self.viewer_show_preview = true;
        self.viewer_wrap = self.snapshot.preferences.wrap_code_blocks;
        self.viewer_copied = false;
        self.active_viewer = Some(item);
        self.overlay = ActiveOverlay::AttachmentViewer;
        cx.notify();
    }

    pub(super) fn open_attachment_in_viewer(
        &mut self,
        att: PromptAttachment,
        cx: &mut Context<Self>,
    ) {
        let lower = att.file_name.to_ascii_lowercase();
        let is_img = att.mime_type.to_ascii_lowercase().starts_with("image/")
            || lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".webp")
            || lower.ends_with(".gif");
        if is_img {
            self.open_viewer_item(
                ActiveMediaViewerItem::Image {
                    title: att.file_name,
                    local_file_path: att.local_file_path,
                    remote_url: String::new(),
                    mime_type: att.mime_type,
                },
                cx,
            );
        } else {
            let text_content = if !att.text_content.is_empty() {
                att.text_content
            } else if !att.local_file_path.is_empty() {
                std::fs::read_to_string(&att.local_file_path).unwrap_or_default()
            } else {
                String::new()
            };
            self.open_viewer_item(
                ActiveMediaViewerItem::TextOrCode {
                    title: att.file_name,
                    text_content,
                    mime_type: att.mime_type,
                    local_file_path: att.local_file_path,
                },
                cx,
            );
        }
    }

    pub(super) fn open_file_path_in_viewer(&mut self, path_str: &str, cx: &mut Context<Self>) {
        let path = PathBuf::from(path_str);
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path_str)
            .to_string();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = guess_mime_type(&ext).to_string();
        if mime.starts_with("image/") {
            self.open_viewer_item(
                ActiveMediaViewerItem::Image {
                    title,
                    local_file_path: path_str.to_string(),
                    remote_url: String::new(),
                    mime_type: mime,
                },
                cx,
            );
        } else {
            let text_content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| format!("Could not read text preview: {e}"));
            self.open_viewer_item(
                ActiveMediaViewerItem::TextOrCode {
                    title,
                    text_content,
                    mime_type: mime,
                    local_file_path: path_str.to_string(),
                },
                cx,
            );
        }
    }

    pub(super) fn apply_palette_refresh(&mut self, cx: &mut Context<Self>) {
        self.palette = ParchmentPalette::from_config(&self.snapshot.theme);
        sync_gpui_component_theme(&self.palette, cx);
        apply_dwm_chrome_to_active_window(&self.palette);
        self.persist_settings();
        cx.notify();
    }

    pub(super) fn show_notice(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        let id = now_ms() as u64;
        self.notice = Some(FloatingNotice {
            id,
            message: msg.into(),
            action_label: None,
            action: NoticeAction::None,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|n| n.id == id) {
                    this.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn show_actionable_notice(
        &mut self,
        msg: impl Into<String>,
        action_label: &'static str,
        action: NoticeAction,
        cx: &mut Context<Self>,
    ) {
        let id = now_ms() as u64;
        self.notice = Some(FloatingNotice {
            id,
            message: msg.into(),
            action_label: Some(action_label),
            action,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(6)).await;
            let _ = this.update(cx, |this, cx| {
                if this.notice.as_ref().is_some_and(|n| n.id == id) {
                    this.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn show_run_error_notice(&mut self, err: &str, cx: &mut Context<Self>) {
        let low = err.to_ascii_lowercase();
        if low.contains("api key")
            || low.contains("unauthorized")
            || low.contains("401")
            || low.contains("403")
            || low.contains("invalid_api_key")
            || low.contains("authentication")
        {
            self.show_actionable_notice(
                err.to_string(),
                strings::CHECK_API_KEY,
                NoticeAction::OpenProviders,
                cx,
            );
        } else if low.contains("spend limit") || low.contains("budget") {
            self.show_actionable_notice(
                err.to_string(),
                strings::SETTINGS_TITLE,
                NoticeAction::OpenPreferences,
                cx,
            );
        } else if low.contains("model") && (low.contains("not found") || low.contains("404")) {
            self.show_actionable_notice(
                err.to_string(),
                strings::CHOOSE_MODEL,
                NoticeAction::OpenModelPicker,
                cx,
            );
        } else if low.contains("context length") || low.contains("too many tokens") {
            self.show_actionable_notice(
                err.to_string(),
                strings::SHORTEN_CONVERSATION,
                NoticeAction::ShortenConversation,
                cx,
            );
        } else {
            self.show_notice(err.to_string(), cx);
        }
    }

    pub(super) fn sync_window_title(&self, window: &mut Window) {
        if self.overlay == ActiveOverlay::Settings {
            window.set_window_title(&strings::window_title_for_settings());
        } else if let Some(run) = self.active_runs.get(&self.active_session.id) {
            let elapsed = format_elapsed_whole_seconds(run.started_at.elapsed());
            window.set_window_title(&strings::window_title_for_stream(
                &run.status_line,
                &elapsed,
                &self.active_session.title,
            ));
        } else {
            window.set_window_title(&strings::window_title_for_chat(&self.active_session.title));
        }
    }

    pub(super) fn profile_needs_api_key(profile: &ProviderProfile) -> bool {
        match profile.protocol {
            ProviderProtocol::Custom => {
                profile.resolve_wire_dialect().is_empty()
                    || profile.effective_base_url().trim().is_empty()
            }
            _ => !profile.has_any_api_key(),
        }
    }

    pub(super) fn rebuild_rows_from_active_session(
        &mut self,
        cx: &mut Context<Self>,
        scroll_end: bool,
    ) {
        let mut rows: Vec<DisplayMessageRow> = self
            .active_session
            .messages
            .iter()
            .cloned()
            .map(DisplayMessageRow::from_node)
            .collect();

        // Overlay any active live stream for this chat onto its row.
        if let Some(run) = self.active_runs.get(&self.active_session.id) {
            let live_text = if run.is_continue && !run.continue_prefix.is_empty() {
                format!("{}{}", run.continue_prefix, run.accumulated_text)
            } else {
                run.accumulated_text.clone()
            };
            if let Some(row) = rows.iter_mut().find(|r| r.node_id == run.node_id) {
                row.content = SharedString::from(live_text);
                row.set_streaming_thinking(&run.accumulated_thinking);
                row.is_streaming = true;
            } else {
                let mut placeholder = DisplayMessageRow::assistant_placeholder(
                    run.node_id.clone(),
                    SharedString::from(run.model_used.clone()),
                    now_ms(),
                );
                placeholder.content = SharedString::from(live_text);
                placeholder.set_streaming_thinking(&run.accumulated_thinking);
                rows.push(placeholder);
            }
        }

        let count = rows.len();
        *self.messages.borrow_mut() = rows;
        self.scroller.update(cx, |s, cx| {
            let prev_top = s.logical_scroll_top();
            let was_following = s.is_following_tail();
            s.reset(count, cx);
            if scroll_end && count > 0 {
                s.scroll_to_end(cx);
            } else if !scroll_end && !was_following && count > 0 {
                let mut target = prev_top;
                if target.item_ix >= count {
                    target.item_ix = count - 1;
                    target.offset_in_item = px(0.0);
                }
                s.scroll_to_offset(target, cx);
            }
        });
    }

    /// Cached wrapper over `compute_context_breakdown` so 60/120Hz render frames never re-tokenize
    /// 200+ chat messages when conversation state and draft text are unchanged.
    pub(super) fn cached_context_breakdown(
        &self,
        profile: &ProviderProfile,
        policy: &AgentPermissionPolicy,
        draft: &str,
    ) -> (ContextBreakdownInfo, f64) {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mix_u64 = |h: &mut u64, v: u64| {
            *h ^= v;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        let mix_bytes = |h: &mut u64, bytes: &[u8]| {
            for &b in bytes {
                *h ^= b as u64;
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            mix_u64(h, bytes.len() as u64);
        };
        mix_bytes(&mut h, self.active_session.id.as_bytes());
        mix_u64(&mut h, self.active_session.updated_at as u64);
        mix_u64(&mut h, self.active_session.messages.len() as u64);
        if let Some(last) = self.active_session.messages.last() {
            mix_u64(&mut h, last.active_variant_index as u64);
            mix_u64(&mut h, last.included_in_context as u64);
            if let Some(v) = last.active_variant() {
                mix_u64(&mut h, v.content.len() as u64);
                mix_u64(&mut h, v.token_metrics.output_tokens as u64);
                mix_u64(&mut h, v.api_usage_records.len() as u64);
            }
        }
        mix_u64(&mut h, self.active_session.fragments.len() as u64);
        mix_u64(&mut h, self.active_session.system_prompt.len() as u64);
        mix_u64(&mut h, self.active_session.story_summary.len() as u64);
        mix_u64(&mut h, self.active_session.authors_note.len() as u64);
        mix_u64(
            &mut h,
            self.active_session.utility_usage_records.len() as u64,
        );
        mix_u64(&mut h, self.pending_attachments.len() as u64);
        mix_bytes(&mut h, draft.as_bytes());
        mix_bytes(&mut h, profile.id.as_bytes());
        mix_bytes(&mut h, self.effective_model_id(profile).as_bytes());
        mix_u64(&mut h, profile.context_window_tokens as u64);
        mix_u64(&mut h, policy.workspace_mode as u64);
        mix_u64(&mut h, policy.shell_access as u64);
        mix_u64(&mut h, policy.web_access as u64);
        mix_u64(&mut h, policy.computer_access as u64);
        mix_u64(&mut h, self.snapshot.global_world_book.len() as u64);
        mix_u64(
            &mut h,
            self.snapshot
                .preferences
                .mcp_servers
                .iter()
                .filter(|s| s.enabled)
                .count() as u64,
        );
        if let Some((prev_hash, ref bd, cost)) = *self.cached_context_breakdown.borrow()
            && prev_hash == h
        {
            return (bd.clone(), cost);
        }
        let (bd, cost) = self.compute_context_breakdown(profile, policy, draft);
        *self.cached_context_breakdown.borrow_mut() = Some((h, bd.clone(), cost));
        (bd, cost)
    }

    pub(super) fn compute_context_breakdown(
        &self,
        profile: &ProviderProfile,
        policy: &AgentPermissionPolicy,
        draft: &str,
    ) -> (ContextBreakdownInfo, f64) {
        let mut bd = ContextBreakdownInfo::default();
        let mut cost: f64 = 0.0;
        let mut seen_req_ids = HashSet::new();

        // 1. System & persona prompt
        if let Some(asst) = self.active_assistant() {
            bd.system_and_persona_tokens += self.core.count_tokens(&asst.system_prompt) as i64;
            if let Some(card) = self
                .active_session
                .active_card
                .as_ref()
                .or(asst.active_card.as_ref())
            {
                bd.system_and_persona_tokens += self.core.count_tokens(&card.description) as i64;
                bd.system_and_persona_tokens += self.core.count_tokens(&card.personality) as i64;
                bd.system_and_persona_tokens += self.core.count_tokens(&card.scenario) as i64;
            }
        }
        bd.system_and_persona_tokens +=
            self.core.count_tokens(&self.active_session.system_prompt) as i64;

        // 2. World Book & story memory
        bd.lorebook_and_memory_tokens +=
            self.core.count_tokens(&self.active_session.story_summary) as i64;
        bd.lorebook_and_memory_tokens +=
            self.core.count_tokens(&self.active_session.authors_note) as i64;
        for frag in self
            .active_session
            .fragments
            .iter()
            .chain(self.snapshot.global_world_book.iter())
        {
            if frag.mode != FragmentActivationMode::Muted {
                bd.lorebook_and_memory_tokens += self.core.count_tokens(&frag.content) as i64;
            }
        }

        // 3. Workspace & tool schemas overhead estimate
        let mut active_tool_schemas = 0i64;
        if policy.workspace_mode != WorkspaceMode::Off {
            active_tool_schemas += 4;
        }
        if policy.workspace_mode == WorkspaceMode::ReadWrite {
            active_tool_schemas += 2;
        }
        if policy.shell_access == ShellAccess::FullAccess {
            active_tool_schemas += 1;
        }
        if policy.web_access {
            active_tool_schemas += 2;
        }
        if policy.computer_access {
            active_tool_schemas += 1;
        }
        active_tool_schemas += self
            .snapshot
            .preferences
            .mcp_servers
            .iter()
            .filter(|s| s.enabled)
            .count() as i64
            * 2;
        bd.workspace_and_tools_tokens = active_tool_schemas * 140;

        // 4. Uploaded files & 5. Chat history
        for att in &self.pending_attachments {
            bd.uploaded_files_count += 1;
            bd.uploaded_files_tokens += if !att.text_content.is_empty() {
                self.core.count_tokens(&att.text_content) as i64
            } else {
                258
            };
        }
        for node in &self.active_session.messages {
            if !node.included_in_context {
                bd.muted_turns_count += 1;
                continue;
            }
            bd.active_turns_count += 1;
            if let Some(v) = node.active_variant() {
                for att in &v.attachments {
                    bd.uploaded_files_count += 1;
                    bd.uploaded_files_tokens += if !att.text_content.is_empty() {
                        self.core.count_tokens(&att.text_content) as i64
                    } else {
                        258
                    };
                }
                if v.token_metrics.output_tokens > 0 {
                    bd.chat_history_tokens += v
                        .token_metrics
                        .output_tokens
                        .max(self.core.count_tokens(&v.content) as i64);
                } else {
                    bd.chat_history_tokens += self.core.count_tokens(&v.content) as i64;
                }
                if v.api_usage_records.is_empty() {
                    if v.token_metrics.cost_is_available
                        && v.token_metrics.estimated_cost_usd.is_finite()
                    {
                        cost += v.token_metrics.estimated_cost_usd;
                    }
                } else {
                    for rec in &v.api_usage_records {
                        if !rec.owner_session_id.is_empty()
                            && rec.owner_session_id != self.active_session.id
                        {
                            continue;
                        }
                        if rec.request_id.is_empty() || seen_req_ids.insert(rec.request_id.clone())
                        {
                            cost += estimate_request_cost(rec).usd;
                        }
                    }
                }
            }
        }
        for rec in &self.active_session.utility_usage_records {
            if !rec.owner_session_id.is_empty() && rec.owner_session_id != self.active_session.id {
                continue;
            }
            if rec.request_id.is_empty() || seen_req_ids.insert(rec.request_id.clone()) {
                let c = estimate_request_cost(rec).usd;
                bd.utility_requests += 1;
                bd.utility_cost_usd += c;
                cost += c;
            }
        }

        // 6. Composer draft
        if !draft.is_empty() {
            bd.composer_draft_tokens = self.core.count_tokens(draft) as i64;
        }

        let model = self.effective_model_id(profile);
        bd.context_limit_tokens = profile
            .custom_model_specs
            .get(&model)
            .map(|s| s.context_window_tokens as i64)
            .filter(|&w| w > 0)
            .or_else(|| {
                self.core
                    .registry()
                    .get_cached_spec(&model)
                    .map(|s| s.context_window_tokens as i64)
                    .filter(|&w| w > 0)
            })
            .unwrap_or_else(|| (profile.context_window_tokens.max(8_192)) as i64);

        (bd, cost)
    }

    pub(super) fn prior_chat_spend_usd(&self) -> f64 {
        let mut seen = HashSet::new();
        let mut all_recs = Vec::new();
        for node in &self.active_session.messages {
            for v in &node.variants {
                for rec in &v.api_usage_records {
                    if !rec.owner_session_id.is_empty()
                        && rec.owner_session_id != self.active_session.id
                    {
                        continue;
                    }
                    if rec.request_id.is_empty() || seen.insert(rec.request_id.clone()) {
                        all_recs.push(rec.clone());
                    }
                }
            }
        }
        for rec in &self.active_session.utility_usage_records {
            if !rec.owner_session_id.is_empty() && rec.owner_session_id != self.active_session.id {
                continue;
            }
            if rec.request_id.is_empty() || seen.insert(rec.request_id.clone()) {
                all_recs.push(rec.clone());
            }
        }
        spend_toward_limit(&all_recs)
    }

    // ---- Tab, Chat & Assistant navigation ----

    pub(super) fn push_mru_tab(&mut self, chat_id: &str) {
        self.open_tabs.retain(|id| id != chat_id);
        self.open_tabs.insert(0, chat_id.to_string());
        if self.open_tabs.len() > MAX_MRU_TABS {
            self.open_tabs.truncate(MAX_MRU_TABS);
        }
        self.snapshot.open_tab_session_ids = self.open_tabs.clone();
        self.snapshot.active_session_id = chat_id.to_string();
        self.persist_settings();
    }

    pub(super) fn sync_notes_inputs_from_active(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sp = self.active_session.system_prompt.clone();
        let an = self.active_session.authors_note.clone();
        let ss = self.active_session.story_summary.clone();
        self.notes_system_input
            .update(cx, |i, cx| i.set_value(&sp, window, cx));
        self.notes_authors_input
            .update(cx, |i, cx| i.set_value(&an, window, cx));
        self.notes_summary_input
            .update(cx, |i, cx| i.set_value(&ss, window, cx));
    }

    pub(super) fn select_chat_by_id(
        &mut self,
        chat_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Save current drafts before switching chats.
        self.active_session.draft_input = self.composer.read(cx).value().to_string();
        self.active_session.draft_prefill = self.prefill_input.read(cx).value().to_string();
        let _ = self.core.save_full_session(&self.active_session);

        let loaded = self
            .core
            .load_full_session(chat_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let title = self
                    .stubs
                    .iter()
                    .find(|s| s.id == chat_id)
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| strings::NEW_CHAT.into());
                let mut s = ChatSession::new(chat_id, title, now_ms());
                s.profile_id = self.snapshot.active_profile_id.clone();
                s.assistant_id = self.snapshot.active_assistant_id.clone();
                s
            });

        self.active_session = loaded;
        if !self.active_session.assistant_id.is_empty()
            && self
                .snapshot
                .assistants
                .iter()
                .any(|a| a.id == self.active_session.assistant_id)
        {
            self.snapshot.active_assistant_id = self.active_session.assistant_id.clone();
        }
        self.push_mru_tab(chat_id);
        self.is_editing_title = false;
        self.editing_message_index = None;
        self.pending_attachments.clear();
        self.show_tools_drawer = false;
        self.show_thinking_popover = false;
        self.show_context_popover = false;

        let draft = self.active_session.draft_input.clone();
        let prefill = self.active_session.draft_prefill.clone();
        self.show_prefill_box = !prefill.trim().is_empty();
        self.composer
            .update(cx, |i, cx| i.set_value(&draft, window, cx));
        self.prefill_input
            .update(cx, |i, cx| i.set_value(&prefill, window, cx));
        self.sync_notes_inputs_from_active(window, cx);
        self.sync_active_card_inputs(window, cx);
        self.refresh_workspace_files();
        let auto_scroll = self.snapshot.preferences.auto_scroll_to_bottom_on_open;
        self.rebuild_rows_from_active_session(cx, auto_scroll);
        if !auto_scroll && !self.active_session.messages.is_empty() {
            self.scroller.update(cx, |s, cx| {
                s.scroll_to_item(0, cx);
            });
        }
        window.set_window_title(&strings::window_title_for_chat(&self.active_session.title));
        self.overlay = ActiveOverlay::None;
        cx.notify();
    }

    pub(super) fn create_new_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = now_ms();
        let asst_id = self.snapshot.active_assistant_id.clone();
        let asst_clone = self
            .snapshot
            .assistants
            .iter()
            .find(|a| a.id == asst_id)
            .cloned();
        let mut session = ChatSession::new(format!("chat-{now}"), strings::NEW_CHAT, now);
        session.profile_id = asst_clone
            .as_ref()
            .filter(|a| !a.default_profile_id.is_empty())
            .map(|a| a.default_profile_id.clone())
            .unwrap_or_else(|| self.snapshot.active_profile_id.clone());
        session.assistant_id = asst_id;
        session.view_mode = self.snapshot.preferences.default_view_mode;
        if !self.snapshot.preferences.profile_name.trim().is_empty() {
            session.user_name = self.snapshot.preferences.profile_name.clone();
        }
        if let Some(a) = asst_clone {
            session.model_override = a.default_model_id;
            session.system_prompt = a.system_prompt;
            session.authors_note = a.authors_note;
            session.authors_note_depth = a.authors_note_depth;
            session.workspace_path = a.workspace_path;
            session.workspace_mode = a.workspace_mode;
        }
        let id = session.id.clone();
        let _ = self.core.save_full_session(&session);
        self.refresh_stubs();
        self.select_chat_by_id(&id, window, cx);
    }

    pub(super) fn delete_chat_with_undo(
        &mut self,
        chat_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_run_for_chat(chat_id, cx);
        let saved = self.core.load_full_session(chat_id).ok().flatten();
        let title = saved
            .as_ref()
            .map(|s| s.title.clone())
            .unwrap_or_else(|| chat_id.to_string());
        if self.core.delete_chat(chat_id).unwrap_or(false) {
            self.open_tabs.retain(|t| t != chat_id);
            self.queued_prompts.remove(chat_id);
            self.refresh_stubs();
            self.refresh_pinned();
            if let Some(sess) = saved {
                self.show_actionable_notice(
                    strings::deleted_chat_notice(&title),
                    strings::UNDO,
                    NoticeAction::UndoDelete(Box::new(sess)),
                    cx,
                );
            } else {
                self.show_notice(strings::deleted_chat_notice(&title), cx);
            }
            if self.active_session.id == chat_id {
                if let Some(next_id) = self
                    .open_tabs
                    .first()
                    .cloned()
                    .or_else(|| self.stubs.first().map(|s| s.id.clone()))
                {
                    self.select_chat_by_id(&next_id, window, cx);
                } else {
                    self.create_new_chat(window, cx);
                }
            } else {
                self.snapshot.open_tab_session_ids = self.open_tabs.clone();
                self.persist_settings();
                cx.notify();
            }
        }
    }

    pub(super) fn trigger_notice_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(notice) = self.notice.take() else {
            return;
        };
        match notice.action {
            NoticeAction::None => {}
            NoticeAction::UndoDelete(session) => {
                let id = session.id.clone();
                let _ = self.core.save_full_session(&session);
                self.refresh_stubs();
                self.refresh_pinned();
                self.select_chat_by_id(&id, window, cx);
            }
            NoticeAction::OpenProviders => {
                self.open_settings_at(SettingsSection::Providers, window, cx);
            }
            NoticeAction::OpenPreferences => {
                self.open_settings_at(SettingsSection::Preferences, window, cx);
            }
            NoticeAction::OpenModelPicker => {
                self.open_overlay(
                    ActiveOverlay::QuickModelPicker,
                    "Filter models or providers…",
                    window,
                    cx,
                );
            }
            NoticeAction::ShortenConversation => {
                self.spawn_summarize_or_compact(true, window, cx);
            }
        }
    }

    pub(super) fn close_tab(&mut self, chat_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        self.open_tabs.retain(|id| id != chat_id);
        self.snapshot.open_tab_session_ids = self.open_tabs.clone();
        self.persist_settings();
        if self.active_session.id == chat_id
            && let Some(next) = self.open_tabs.first().cloned()
        {
            self.select_chat_by_id(&next, window, cx);
        } else {
            cx.notify();
        }
    }

    pub(super) fn cycle_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        let cur = self
            .open_tabs
            .iter()
            .position(|id| id == &self.active_session.id)
            .unwrap_or(0);
        let next_idx = (cur + 1) % self.open_tabs.len();
        if let Some(next) = self.open_tabs.get(next_idx).cloned() {
            self.select_chat_by_id(&next, window, cx);
        }
    }

    pub(super) fn commit_title_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let new_title = self.title_edit_input.read(cx).value().trim().to_string();
        self.is_editing_title = false;
        if !new_title.is_empty() && new_title != self.active_session.title {
            self.active_session.title = new_title.clone();
            self.active_session.is_title_manually_set = true;
            self.persist_active_session();
            window.set_window_title(&strings::window_title_for_chat(&new_title));
        }
        cx.notify();
    }

    pub(super) fn run_fts_search(&mut self, cx: &mut Context<Self>) {
        let q = self.sidebar_search_input.read(cx).value().to_string();
        if q.trim().is_empty() {
            self.search_hits.clear();
        } else if let Ok(hits) = self.core.search_messages(&q, 50) {
            self.search_hits = hits;
        }
        cx.notify();
    }

    // ---- Settings input sync ----

    pub(super) fn sync_all_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prof_name = self.snapshot.preferences.profile_name.clone();
        let spend = if self.snapshot.preferences.chat_spend_limit_usd > 0.0 {
            format!("{}", self.snapshot.preferences.chat_spend_limit_usd)
        } else {
            String::new()
        };
        let title_p = self.snapshot.default_models.title_prompt_template.clone();
        let trans_l = self
            .snapshot
            .default_models
            .translation_target_language
            .clone();
        let ota_r = self.snapshot.ota_github_repo.clone();

        self.settings_inputs
            .user_profile_name
            .update(cx, |i, cx| i.set_value(&prof_name, window, cx));
        self.settings_inputs
            .spend_limit
            .update(cx, |i, cx| i.set_value(&spend, window, cx));
        self.settings_inputs
            .title_prompt
            .update(cx, |i, cx| i.set_value(&title_p, window, cx));
        self.settings_inputs
            .translation_lang
            .update(cx, |i, cx| i.set_value(&trans_l, window, cx));
        let paste_t = self
            .snapshot
            .preferences
            .paste_conversion_threshold_chars
            .to_string();
        self.settings_inputs
            .paste_threshold
            .update(cx, |i, cx| i.set_value(&paste_t, window, cx));
        self.settings_inputs
            .ota_repo
            .update(cx, |i, cx| i.set_value(&ota_r, window, cx));

        self.sync_provider_inputs_from_active(window, cx);
        self.sync_assistant_inputs_from_active(window, cx);
        self.sync_active_card_inputs(window, cx);
    }

    pub(super) fn sync_active_card_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (name, desc, first) = self
            .active_session
            .active_card
            .as_ref()
            .map(|c| {
                (
                    c.name.clone(),
                    c.description.clone(),
                    c.first_message.clone(),
                )
            })
            .unwrap_or_default();
        self.settings_inputs
            .wb_card_name
            .update(cx, |i, cx| i.set_value(&name, window, cx));
        self.settings_inputs
            .wb_card_desc
            .update(cx, |i, cx| i.set_value(&desc, window, cx));
        self.settings_inputs
            .wb_card_first_mes
            .update(cx, |i, cx| i.set_value(&first, window, cx));
    }
    pub(super) fn sync_provider_inputs_from_active(
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
        let temp = prof.temperature.map_or_default(|v| format!("{v}"));
        let top_p = prof.top_p.map_or_default(|v| format!("{v}"));
        let max_tok = prof.max_output_tokens.map_or_default(|v| v.to_string());
        let ctx_win = prof.context_window_tokens.to_string();
        let t_budget = prof
            .thinking_budget_tokens
            .map_or_default(|v| v.to_string());
        let in_p = if prof.input_price_per_m > 0.0 {
            format!("{}", prof.input_price_per_m)
        } else {
            String::new()
        };
        let out_p = if prof.output_price_per_m > 0.0 {
            format!("{}", prof.output_price_per_m)
        } else {
            String::new()
        };

        self.settings_inputs
            .provider_name
            .update(cx, |i, cx| i.set_value(&prof.name, window, cx));
        self.settings_inputs
            .api_key
            .update(cx, |i, cx| i.set_value(&prof.api_key, window, cx));
        self.settings_inputs
            .base_url
            .update(cx, |i, cx| i.set_value(&prof.base_url, window, cx));
        self.settings_inputs.endpoint_override.update(cx, |i, cx| {
            i.set_value(&prof.endpoint_path_override, window, cx)
        });
        self.settings_inputs
            .default_model
            .update(cx, |i, cx| i.set_value(&prof.default_model, window, cx));
        self.settings_inputs
            .temperature
            .update(cx, |i, cx| i.set_value(&temp, window, cx));
        self.settings_inputs
            .top_p
            .update(cx, |i, cx| i.set_value(&top_p, window, cx));
        self.settings_inputs
            .max_output_tokens
            .update(cx, |i, cx| i.set_value(&max_tok, window, cx));
        self.settings_inputs
            .context_window
            .update(cx, |i, cx| i.set_value(&ctx_win, window, cx));
        self.settings_inputs
            .thinking_budget_tokens
            .update(cx, |i, cx| i.set_value(&t_budget, window, cx));
        self.settings_inputs
            .input_price
            .update(cx, |i, cx| i.set_value(&in_p, window, cx));
        self.settings_inputs
            .output_price
            .update(cx, |i, cx| i.set_value(&out_p, window, cx));
        self.settings_inputs.custom_headers_raw.update(cx, |i, cx| {
            i.set_value(&prof.custom_headers_raw, window, cx)
        });
        self.settings_inputs
            .raw_json_extra_body
            .update(cx, |i, cx| {
                i.set_value(&prof.raw_json_extra_body, window, cx)
            });
    }
    pub(super) fn sync_assistant_inputs_from_active(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(asst) = self
            .snapshot
            .assistants
            .iter()
            .find(|a| a.id == self.snapshot.active_assistant_id)
            .or_else(|| self.snapshot.assistants.first())
            .cloned()
        else {
            return;
        };
        self.settings_inputs
            .assistant_name
            .update(cx, |i, cx| i.set_value(&asst.name, window, cx));
        self.settings_inputs
            .assistant_system_prompt
            .update(cx, |i, cx| i.set_value(&asst.system_prompt, window, cx));
        self.settings_inputs
            .assistant_authors_note
            .update(cx, |i, cx| i.set_value(&asst.authors_note, window, cx));
        self.settings_inputs
            .assistant_default_model
            .update(cx, |i, cx| i.set_value(&asst.default_model_id, window, cx));
        let t_str = asst.temperature.map_or_default(|v| format!("{v}"));
        let tp_str = asst.top_p.map_or_default(|v| format!("{v}"));
        let m_str = asst.max_output_tokens.map_or_default(|v| v.to_string());
        let cm_str = if asst.context_message_limit > 0 {
            asst.context_message_limit.to_string()
        } else {
            String::new()
        };
        self.settings_inputs
            .assistant_temperature
            .update(cx, |i, cx| i.set_value(&t_str, window, cx));
        self.settings_inputs
            .assistant_top_p
            .update(cx, |i, cx| i.set_value(&tp_str, window, cx));
        self.settings_inputs
            .assistant_max_output
            .update(cx, |i, cx| i.set_value(&m_str, window, cx));
        self.settings_inputs
            .assistant_context_msg_limit
            .update(cx, |i, cx| i.set_value(&cm_str, window, cx));
    }
    pub(super) fn sync_hex_inputs_from_theme(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let th = &self.snapshot.theme;
        let pf_s = format!("{}", th.prose_font_size_sp);
        let cf_s = format!("{}", th.code_font_size_sp);
        let rw_s = format!("{}", th.max_reading_width_dp.round() as i32);
        let lh_s = format!("{:.2}", th.prose_line_height_multiplier);
        let ps_s = format!("{}", th.paragraph_spacing_dp.round() as i32);
        self.settings_inputs
            .prose_font_size
            .update(cx, |i, cx| i.set_value(&pf_s, window, cx));
        self.settings_inputs
            .code_font_size
            .update(cx, |i, cx| i.set_value(&cf_s, window, cx));
        self.settings_inputs
            .max_reading_width
            .update(cx, |i, cx| i.set_value(&rw_s, window, cx));
        self.settings_inputs
            .line_height
            .update(cx, |i, cx| i.set_value(&lh_s, window, cx));
        self.settings_inputs
            .paragraph_spacing
            .update(cx, |i, cx| i.set_value(&ps_s, window, cx));
        self.settings_inputs
            .hex_canvas
            .update(cx, |i, cx| i.set_value(&th.canvas_bg_hex, window, cx));
        self.settings_inputs
            .hex_card
            .update(cx, |i, cx| i.set_value(&th.card_bg_hex, window, cx));
        self.settings_inputs
            .hex_text
            .update(cx, |i, cx| i.set_value(&th.primary_ink_hex, window, cx));
        self.settings_inputs
            .hex_accent
            .update(cx, |i, cx| i.set_value(&th.accent_hex, window, cx));
    }
    pub(super) fn sync_model_cfg_inputs(
        &mut self,
        model_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prof = self.active_profile();
        let spec = prof.custom_model_specs.get(model_id);
        let ctx = spec
            .map(|s| s.context_window_tokens)
            .filter(|&v| v > 0)
            .unwrap_or(prof.context_window_tokens);
        let max_out = spec
            .and_then(|s| s.max_output_tokens)
            .or(prof.max_output_tokens)
            .map_or_default(|v| v.to_string());
        let kp = spec.and_then(|s| s.known_pricing.as_ref());
        let fmt_opt = |v: Option<f64>| v.map_or_default(|x| format!("{x}"));
        let in_p = fmt_opt(
            kp.and_then(|p| p.input_per_m)
                .or_else(|| spec.map(|s| s.input_price_per_m).filter(|&v| v > 0.0)),
        );
        let out_p = fmt_opt(
            kp.and_then(|p| p.output_per_m)
                .or_else(|| spec.map(|s| s.output_price_per_m).filter(|&v| v > 0.0)),
        );
        let cr = fmt_opt(kp.and_then(|p| p.cache_read_per_m));
        let cw = fmt_opt(kp.and_then(|p| p.cache_write_per_m));
        let lcw = fmt_opt(kp.and_then(|p| p.long_cache_write_per_m));
        let rp = fmt_opt(kp.and_then(|p| p.reasoning_per_m));

        self.settings_inputs
            .model_cfg_context
            .update(cx, |i, cx| i.set_value(ctx.to_string(), window, cx));
        self.settings_inputs
            .model_cfg_max_out
            .update(cx, |i, cx| i.set_value(&max_out, window, cx));
        self.settings_inputs
            .model_cfg_in_price
            .update(cx, |i, cx| i.set_value(&in_p, window, cx));
        self.settings_inputs
            .model_cfg_out_price
            .update(cx, |i, cx| i.set_value(&out_p, window, cx));
        self.settings_inputs
            .model_cfg_cache_read
            .update(cx, |i, cx| i.set_value(&cr, window, cx));
        self.settings_inputs
            .model_cfg_cache_write
            .update(cx, |i, cx| i.set_value(&cw, window, cx));
        self.settings_inputs
            .model_cfg_long_cache_write
            .update(cx, |i, cx| i.set_value(&lcw, window, cx));
        self.settings_inputs
            .model_cfg_reasoning_price
            .update(cx, |i, cx| i.set_value(&rp, window, cx));
    }

    pub(super) fn open_settings_at(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_section = section;
        self.providers_subpage = ProvidersSubpage::Main;
        self.assistant_subpage = AssistantSubpage::List;
        self.recording_shortcut = None;
        self.shortcut_conflict = None;
        self.shortcut_feedback.clear();
        self.confirm_wipe_pending = false;
        self.sync_all_settings_inputs(window, cx);
        self.sync_hex_inputs_from_theme(window, cx);
        self.refresh_memory_and_skills();
        self.refresh_storage_scan();
        self.refresh_mcp_statuses_async(window, cx);
        self.overlay = ActiveOverlay::Settings;
        self.sync_window_title(window);
    }

    pub(super) fn open_overlay(
        &mut self,
        target: ActiveOverlay,
        placeholder: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.overlay = target;
        self.overlay_selected_index = 0;
        self.overlay_filter_input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.set_placeholder(placeholder.to_string(), window, cx);
        });
        cx.notify();
    }

    // ---- Multi-tab streaming engine & background pump ----

    pub(super) fn ensure_pump_scheduled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pump_scheduled {
            return;
        }
        if self.active_runs.is_empty() && self.pending_bg_tasks == 0 {
            return;
        }
        self.pump_scheduled = true;
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                this.pump_scheduled = false;
                this.pump_active_work(window, cx);
            });
        });
    }
}
