use super::*;

struct DesktopSubagentDelegate {
    core: Arc<ParchmentCore>,
    profile: ProviderProfile,
    model: String,
    base_session: ChatSession,
    base_policy: AgentPermissionPolicy,
    storage_root: PathBuf,
    web: Arc<PublicWebTransport>,
    mcp: Option<Arc<McpRegistry>>,
    parent_run_id: String,
    spend_limit: f64,
    prior_spend: f64,
    catalog_spec: Option<ModelPricingSpec>,
    tx: mpsc::UnboundedSender<LiveRunUpdate>,
}

#[async_trait::async_trait]
impl SubagentDelegate for DesktopSubagentDelegate {
    async fn run_helper(&self, task: &str) -> Result<String, String> {
        let now = now_ms();
        let helper_run_id = format!("{}-sub-{now}", self.parent_run_id);
        let _ = self
            .tx
            .send(LiveRunUpdate::Progress(RunProgressEvent::Status(
                "Running helper subagent…".into(),
            )));
        let mut helper_policy = self.base_policy.clone();
        helper_policy.workspace_mode = match helper_policy.workspace_mode {
            WorkspaceMode::ReadWrite => WorkspaceMode::ReadOnly,
            other => other,
        };
        helper_policy.shell_access = ShellAccess::Disabled;
        helper_policy.computer_access = false;
        let broker = ToolApprovalBroker::new(
            &helper_run_id,
            &self.base_session.id,
            ToolAccessScope::new(helper_policy),
        );
        let hctx = HarnessContext {
            chat_id: self.base_session.id.clone(),
            storage_root: self.storage_root.clone(),
            broker,
            web: self.web.clone(),
            mcp: self.mcp.clone(),
            computer_use: None,
            subagent_delegate: None,
            goal_state: Arc::new(AsyncMutex::new(GoalTrackerState::default())),
            expanded_mcp_tools: Arc::new(AsyncMutex::new(HashSet::new())),
            computer_use_settle_ms: 0,
        };
        let mut helper_session = ChatSession::new(
            format!("{}-sub-{now}", self.base_session.id),
            "Helper Subagent",
            now,
        );
        helper_session.system_prompt =
            "You are a focused read-only helper subagent. Complete the delegated task concisely and accurately."
                .into();
        let history = vec![MessageNode {
            id: format!("sub-u-{now}"),
            role: MessageRole::User,
            variants: vec![MessageVariant {
                id: format!("sub-u-{now}-v0"),
                content: task.to_string(),
                created_at: now,
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        }];
        let protocol = self.core.protocol_client();
        let tx_bg = self.tx.clone();
        let outcome = AgentTurnRunner::run_turn(
            &protocol,
            &self.profile,
            &self.model,
            &helper_session,
            &history,
            "",
            &hctx,
            &helper_run_id,
            self.spend_limit,
            self.prior_spend,
            self.catalog_spec.clone(),
            move |ev| {
                if let RunProgressEvent::Status(s) = ev {
                    let _ = tx_bg.send(LiveRunUpdate::Progress(RunProgressEvent::Status(format!(
                        "Helper: {s}"
                    ))));
                }
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(outcome.variant.content)
    }
}

impl ParchmentDesktopApp {
    pub(super) fn submit_composer_shortcut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).value().to_string();
        let trimmed = draft.trim();
        if trimmed.starts_with('/') && !trimmed.contains('\n') && !trimmed.contains(' ') {
            self.run_slash_command(trimmed, window, cx);
            return;
        }
        if self.active_runs.contains_key(&self.active_session.id) {
            if !trimmed.is_empty() {
                self.queue_current_draft(window, cx);
            }
        } else {
            self.send_composer_message(window, cx);
        }
    }

    pub(super) fn send_composer_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self.composer.read(cx).value().to_string();
        let trimmed = draft.trim().to_string();
        if trimmed.is_empty() && self.pending_attachments.is_empty() {
            return;
        }
        if trimmed.starts_with('/') && !trimmed.contains('\n') && !trimmed.contains(' ') {
            self.run_slash_command(&trimmed, window, cx);
            return;
        }
        if self.active_runs.contains_key(&self.active_session.id) {
            self.queue_current_draft(window, cx);
            return;
        }

        let profile = self.active_profile();
        if let Some(block_reason) = self.validate_provider_ready(&profile) {
            self.show_notice(block_reason, cx);
            return;
        }

        let now = now_ms();
        let attachments = std::mem::take(&mut self.pending_attachments);
        let user_node = MessageNode {
            id: format!("user-{now}"),
            role: MessageRole::User,
            variants: vec![MessageVariant {
                id: format!("user-{now}-v0"),
                content: trimmed.clone(),
                attachments: attachments.clone(),
                created_at: now,
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        };
        self.active_session.messages.push(user_node.clone());
        self.active_session.draft_input.clear();
        self.active_session.draft_prefill = self.prefill_input.read(cx).value().to_string();
        let _ = self
            .core
            .append_message_node(&self.active_session.id, &user_node, now);
        self.persist_active_session();

        self.messages.borrow_mut().push(DisplayMessageRow::user(
            user_node.id,
            trimmed,
            attachments,
            now,
        ));
        self.scroller.update(cx, |s, cx| {
            s.append(1, cx);
            s.scroll_to_end(cx);
        });
        self.composer
            .update(cx, |input, cx| input.set_value("", window, cx));

        self.launch_run_on_active_chat(None, false, window, cx);
    }

    pub(super) fn validate_provider_ready(&self, profile: &ProviderProfile) -> Option<String> {
        if profile.protocol == ProviderProtocol::Custom {
            if profile.resolve_wire_dialect().is_empty() {
                return Some(
                    "Choose an API format for your Custom provider in Settings before sending."
                        .into(),
                );
            }
            if profile.effective_base_url().trim().is_empty() {
                return Some("Enter a Server URL for your Custom provider in Settings.".into());
            }
        } else if !profile.has_any_api_key() {
            return Some(format!(
                "Add an API key for {} in Settings before sending.",
                profile.name
            ));
        }
        if self.effective_model_id(profile).is_empty() {
            return Some("Choose or enter a model ID before sending.".into());
        }
        None
    }

    pub(super) fn launch_run_on_active_chat(
        &mut self,
        target_assistant_node_idx: Option<usize>,
        is_continue: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile = self.active_profile();
        if let Some(reason) = self.validate_provider_ready(&profile) {
            self.show_notice(reason, cx);
            return;
        }
        let model = self.effective_model_id(&profile);
        let chat_id = self.active_session.id.clone();
        let now = now_ms();
        let run_id = format!("run-{now}");

        // Build history + determine target assistant node & variant slot.
        let mut session_snapshot = self.active_session.clone();
        if let Some(asst) = self.active_assistant() {
            if session_snapshot.system_prompt.trim().is_empty() {
                session_snapshot.system_prompt = asst.system_prompt.clone();
            }
            if session_snapshot.authors_note.trim().is_empty() {
                session_snapshot.authors_note = asst.authors_note.clone();
            }
            if session_snapshot.active_card.is_none() {
                session_snapshot.active_card = asst.active_card.clone();
            }
        }
        if let Some(dir) = self.pending_guided_direction.take()
            && !dir.trim().is_empty()
        {
            if !session_snapshot.authors_note.trim().is_empty() {
                session_snapshot.authors_note.push_str("\n\n");
            }
            session_snapshot
                .authors_note
                .push_str(&format!("[Direction for this reply: {}]", dir.trim()));
        }
        // Inject active global World Book entries alongside chat fragments (late lore).
        for frag in &self.snapshot.global_world_book {
            if !session_snapshot.fragments.iter().any(|f| f.id == frag.id) {
                session_snapshot.fragments.push(frag.clone());
            }
        }
        let (node_id, variant_index, mut continue_prefix, history_nodes) = if let Some(idx) =
            target_assistant_node_idx
            && let Some(node) = self.active_session.messages.get_mut(idx)
            && node.role == MessageRole::Assistant
        {
            let nid = node.id.clone();
            if is_continue {
                let v_idx = node
                    .active_variant_index
                    .min(node.variants.len().saturating_sub(1));
                let prefix = node
                    .variants
                    .get(v_idx)
                    .map(|v| v.content.clone())
                    .unwrap_or_default();
                let mut hist = self.active_session.messages[..=idx].to_vec();
                hist.push(MessageNode {
                        id: format!("continue-cue-{now}"),
                        role: MessageRole::User,
                        variants: vec![MessageVariant {
                            id: format!("continue-cue-{now}-v0"),
                            content: "Continue seamlessly from exactly where you stopped without repeating previous text.".into(),
                            created_at: now,
                            ..Default::default()
                        }],
                        active_variant_index: 0,
                        included_in_context: true,
                        pinned: false,
                    });
                (nid, v_idx, prefix, hist)
            } else {
                let v_idx = node.variants.len();
                node.variants.push(MessageVariant {
                    id: format!("{nid}-v{v_idx}"),
                    model_used: model.clone(),
                    created_at: now,
                    is_streaming: true,
                    ..Default::default()
                });
                node.active_variant_index = v_idx;
                let hist = self.active_session.messages[..idx].to_vec();
                (nid, v_idx, String::new(), hist)
            }
        } else {
            let nid = format!("asst-{now}");
            let new_node = MessageNode {
                id: nid.clone(),
                role: MessageRole::Assistant,
                variants: vec![MessageVariant {
                    id: format!("{nid}-v0"),
                    model_used: model.clone(),
                    created_at: now,
                    is_streaming: true,
                    ..Default::default()
                }],
                active_variant_index: 0,
                included_in_context: true,
                pinned: false,
            };
            let hist = self.active_session.messages.clone();
            self.active_session.messages.push(new_node);
            (nid, 0, String::new(), hist)
        };

        let prefill_text = self.prefill_input.read(cx).value().trim().to_string();
        if !prefill_text.is_empty() && !is_continue {
            continue_prefix = prefill_text;
            self.active_session.draft_prefill.clear();
            self.show_prefill_box = false;
            self.prefill_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        }

        let policy = self.resolved_policy();
        let mut scope = ToolAccessScope::new(policy.clone());
        scope.goal_mode_active = self.goal_mode_enabled;
        let broker = ToolApprovalBroker::new(&run_id, &chat_id, scope);
        let retry_now = Arc::new(Notify::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let prior_spend = self.prior_chat_spend_usd();
        let spend_limit = self.snapshot.preferences.chat_spend_limit_usd;
        let catalog_spec = self.core.registry().get_cached_spec(&model);
        let goal_state = if self.goal_mode_enabled {
            GoalTrackerState {
                goal_objective: "Complete the user's request thoroughly and verify each step."
                    .into(),
                ..Default::default()
            }
        } else {
            GoalTrackerState::default()
        };

        self.power_lease.acquire();
        let lease_bg = self.power_lease.clone();
        let core_bg = self.core.clone();
        let mcp_bg = Some(self.mcp_registry.clone());
        let storage_root = self.data_dir.clone();
        let broker_bg = broker.clone();
        let retry_bg = retry_now.clone();
        let model_bg = model.clone();
        let continue_prefix_bg = continue_prefix.clone();
        let subagent_policy = policy;

        let join_handle = self.core.runtime().spawn(async move {
            static WEB_TRANSPORT: std::sync::LazyLock<Arc<PublicWebTransport>> =
                std::sync::LazyLock::new(|| {
                    Arc::new(
                        PublicWebTransport::new()
                            .unwrap_or_else(|e| PublicWebTransport::fail_closed(e.to_string())),
                    )
                });
            let web_bg = Arc::clone(&WEB_TRANSPORT);
            let protocol = core_bg.protocol_client().with_retry_notify(retry_bg);
            let subagent_delegate: Arc<dyn SubagentDelegate> = Arc::new(DesktopSubagentDelegate {
                core: core_bg.clone(),
                profile: profile.clone(),
                model: model_bg.clone(),
                base_session: session_snapshot.clone(),
                base_policy: subagent_policy,
                storage_root: storage_root.clone(),
                web: web_bg.clone(),
                mcp: mcp_bg.clone(),
                parent_run_id: run_id.clone(),
                spend_limit,
                prior_spend,
                catalog_spec: catalog_spec.clone(),
                tx: tx.clone(),
            });
            let hctx = HarnessContext {
                chat_id: session_snapshot.id.clone(),
                storage_root,
                broker: broker_bg,
                web: web_bg,
                mcp: mcp_bg,
                computer_use: Some(Arc::new(Win32ComputerUse)),
                subagent_delegate: Some(subagent_delegate),
                goal_state: Arc::new(AsyncMutex::new(goal_state)),
                expanded_mcp_tools: Arc::new(AsyncMutex::new(HashSet::new())),
                computer_use_settle_ms: 150,
            };
            let res = AgentTurnRunner::run_turn(
                &protocol,
                &profile,
                &model_bg,
                &session_snapshot,
                &history_nodes,
                &continue_prefix_bg,
                &hctx,
                &run_id,
                spend_limit,
                prior_spend,
                catalog_spec,
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

        self.active_runs.insert(
            chat_id.clone(),
            ActiveRunState {
                chat_id,
                node_id,
                variant_index,
                continue_prefix,
                model_used: model,
                started_at: Instant::now(),
                abort_handle: join_handle.abort_handle(),
                retry_now,
                broker,
                rx,
                status_line: "Connecting…".into(),
                retry_banner: None,
                pending_approvals: Vec::new(),
                pending_questions: Vec::new(),
                accumulated_text: String::new(),
                accumulated_thinking: String::new(),
                live_tool_calls: Vec::new(),
                last_prose_parse_at: Instant::now() - Duration::from_millis(200),
                last_checkpoint_at: Instant::now(),
            },
        );

        self.composer.update(cx, |i, cx| {
            i.set_placeholder(strings::STREAMING_FOLLOW_UP_PLACEHOLDER, window, cx)
        });
        self.rebuild_rows_from_active_session(cx, true);
        self.ensure_pump_scheduled(window, cx);
        cx.notify();
    }

    pub(super) fn stop_run_for_chat(&mut self, chat_id: &str, cx: &mut Context<Self>) {
        let Some(run) = self.active_runs.remove(chat_id) else {
            return;
        };
        run.abort_handle.abort();
        let broker = run.broker.clone();
        self.core.runtime().spawn(async move {
            broker.close().await;
        });
        self.power_lease.release();

        let partial = if !run.continue_prefix.is_empty() {
            format!("{}{}", run.continue_prefix, run.accumulated_text)
        } else {
            run.accumulated_text
        };

        let mut finished_variant = MessageVariant {
            id: format!("{}-v{}", run.node_id, run.variant_index),
            content: partial,
            thinking_summary: run.accumulated_thinking,
            model_used: run.model_used,
            created_at: now_ms(),
            is_streaming: false,
            ..Default::default()
        };
        if finished_variant.content.trim().is_empty() {
            finished_variant.content = "_Stopped before any text arrived._".into();
        }
        self.commit_completed_variant(
            chat_id,
            &run.node_id,
            run.variant_index,
            finished_variant,
            cx,
        );
    }

    pub(super) fn commit_completed_variant(
        &mut self,
        chat_id: &str,
        node_id: &str,
        variant_index: usize,
        mut variant: MessageVariant,
        cx: &mut Context<Self>,
    ) {
        variant.is_streaming = false;
        if variant.id.is_empty() {
            variant.id = format!("{node_id}-v{variant_index}");
        }
        if variant.created_at <= 0 {
            variant.created_at = now_ms();
        }

        if chat_id == self.active_session.id {
            if let Some(node) = self
                .active_session
                .messages
                .iter_mut()
                .find(|n| n.id == node_id)
            {
                if variant_index < node.variants.len() {
                    node.variants[variant_index] = variant.clone();
                } else {
                    node.variants.push(variant.clone());
                }
                node.active_variant_index =
                    variant_index.min(node.variants.len().saturating_sub(1));
            } else {
                self.active_session.messages.push(MessageNode {
                    id: node_id.to_string(),
                    role: MessageRole::Assistant,
                    variants: vec![variant.clone()],
                    active_variant_index: 0,
                    included_in_context: true,
                    pinned: false,
                });
            }
            self.persist_active_session();
            let auto_follow = self.snapshot.preferences.auto_follow_bottom;
            self.rebuild_rows_from_active_session(cx, auto_follow);
        } else if let Ok(Some(mut sess)) = self.core.load_full_session(chat_id) {
            if let Some(node) = sess.messages.iter_mut().find(|n| n.id == node_id) {
                if variant_index < node.variants.len() {
                    node.variants[variant_index] = variant;
                } else {
                    node.variants.push(variant);
                }
                node.active_variant_index =
                    variant_index.min(node.variants.len().saturating_sub(1));
            } else {
                sess.messages.push(MessageNode {
                    id: node_id.to_string(),
                    role: MessageRole::Assistant,
                    variants: vec![variant],
                    active_variant_index: 0,
                    included_in_context: true,
                    pinned: false,
                });
            }
            sess.updated_at = now_ms();
            let _ = self.core.save_full_session(&sess);
            self.refresh_stubs();
        }
        cx.notify();
    }

    pub(super) fn pump_active_work(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut any_dirty = false;

        // 1. Drain background utility tasks (models fetch, OTA, title, summary, translation).
        let mut bg_events = Vec::new();
        while let Ok(ev) = self.bg_rx.try_recv() {
            bg_events.push(ev);
        }
        for ev in bg_events {
            self.pending_bg_tasks = self.pending_bg_tasks.saturating_sub(1);
            any_dirty = true;
            self.handle_bg_task_event(ev, window, cx);
        }

        // 2. Drain all active multi-tab streaming runs.
        type FinishedRun = (String, Result<(MessageVariant, i64), String>);
        let mut finished_runs: Vec<FinishedRun> = Vec::new();
        let active_id = self.active_session.id.clone();

        for (cid, run) in self.active_runs.iter_mut() {
            let mut run_dirty = false;
            while let Ok(msg) = run.rx.try_recv() {
                run_dirty = true;
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
                        RunProgressEvent::ToolStarted {
                            call_id,
                            name,
                            arguments_json,
                        } => {
                            let starts_tool_step = run.live_tool_calls.is_empty();
                            run.live_tool_calls.push(ToolCallRecord {
                                id: call_id,
                                name: name.clone(),
                                arguments_json,
                                is_running: true,
                                starts_tool_step,
                                ..Default::default()
                            });
                            run.status_line = format!("Running {name}…");
                        }
                        RunProgressEvent::ToolFinished {
                            call_id,
                            name,
                            is_error,
                            duration_ms,
                            result_preview,
                        } => {
                            if let Some(tc) = run
                                .live_tool_calls
                                .iter_mut()
                                .rev()
                                .find(|t| t.id == call_id || (t.is_running && t.name == name))
                            {
                                tc.is_running = false;
                                tc.is_error = is_error;
                                tc.duration_ms = duration_ms;
                                tc.result_text = result_preview;
                            }
                            run.status_line = format!("Finished {name}");
                        }
                    },
                    LiveRunUpdate::Finished(res) => {
                        finished_runs.push((cid.clone(), *res));
                        break;
                    }
                }
            }

            if let Some(pending) = run.broker.try_pending_requests()
                && pending.len() != run.pending_approvals.len()
            {
                run.pending_approvals = pending;
                run_dirty = true;
            }
            if let Some(questions) = run.broker.try_pending_questions()
                && questions.len() != run.pending_questions.len()
            {
                run.pending_questions = questions;
                run_dirty = true;
            }

            // Periodic 3.5 s SQLite streaming checkpoint so crashes never lose generated text.
            if run.last_checkpoint_at.elapsed() >= STREAM_CHECKPOINT_INTERVAL
                && (!run.accumulated_text.is_empty() || !run.accumulated_thinking.is_empty())
            {
                run.last_checkpoint_at = Instant::now();
                let cp_content = if !run.continue_prefix.is_empty() {
                    format!("{}{}", run.continue_prefix, run.accumulated_text)
                } else {
                    run.accumulated_text.clone()
                };
                let cp_var = MessageVariant {
                    id: format!("{}-v{}", run.node_id, run.variant_index),
                    content: cp_content,
                    thinking_summary: run.accumulated_thinking.clone(),
                    model_used: run.model_used.clone(),
                    created_at: now_ms(),
                    is_streaming: true,
                    ..Default::default()
                };
                let _ = self.core.upsert_streaming_variant(
                    &run.chat_id,
                    &run.node_id,
                    run.variant_index,
                    &cp_var,
                    now_ms(),
                );
            }

            if run_dirty {
                any_dirty = true;
                if cid == &active_id {
                    let live_text = if !run.continue_prefix.is_empty() {
                        format!("{}{}", run.continue_prefix, run.accumulated_text)
                    } else {
                        run.accumulated_text.clone()
                    };
                    let should_reparse =
                        run.last_prose_parse_at.elapsed() >= Duration::from_millis(90);
                    if should_reparse {
                        run.last_prose_parse_at = Instant::now();
                    }
                    let mut updated_idx = None;
                    {
                        let mut rows = self.messages.borrow_mut();
                        if let Some((ix, row)) = rows
                            .iter_mut()
                            .enumerate()
                            .find(|(_, r)| r.node_id == run.node_id)
                        {
                            row.set_streaming_content(&live_text, should_reparse);
                            row.set_streaming_thinking(&run.accumulated_thinking);
                            row.set_streaming_tools(&run.live_tool_calls);
                            row.is_streaming = true;
                            updated_idx = Some(ix);
                        }
                    }
                    if let Some(ix) = updated_idx {
                        let follow = self.snapshot.preferences.auto_follow_bottom;
                        self.scroller.update(cx, |s, cx| {
                            s.remeasure_items(ix..ix + 1, cx);
                            if follow {
                                s.scroll_to_end(cx);
                            }
                        });
                    }
                }
            }
        }

        // Update window title with active chat elapsed stopwatch or Settings state.
        self.sync_window_title(window);

        // Process finished runs.
        for (cid, done) in finished_runs {
            let Some(run) = self.active_runs.remove(&cid) else {
                continue;
            };
            any_dirty = true;
            if cid == self.active_session.id {
                let asst_name = self
                    .active_assistant()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| strings::DEFAULT_ASSISTANT.into());
                self.composer.update(cx, |i, cx| {
                    i.set_placeholder(
                        strings::message_assistant_placeholder(&asst_name),
                        window,
                        cx,
                    )
                });
                self.sync_window_title(window);
            }
            match done {
                Ok((mut variant, _elapsed_ms)) => {
                    if !run.continue_prefix.is_empty() {
                        variant.content = format!("{}{}", run.continue_prefix, variant.content);
                    }
                    self.commit_completed_variant(
                        &cid,
                        &run.node_id,
                        run.variant_index,
                        variant,
                        cx,
                    );
                    // Auto-title after first completed turn if title wasn't manually edited.
                    if cid == self.active_session.id
                        && !self.active_session.is_title_manually_set
                        && (self.active_session.title == strings::NEW_CHAT
                            || self.active_session.title == strings::UNTITLED)
                    {
                        self.spawn_generate_title(&cid, window, cx);
                    }
                    // Auto-drain next queued prompt for this chat if present.
                    if cid == self.active_session.id
                        && let Some(next_q) = self
                            .queued_prompts
                            .get_mut(&cid)
                            .and_then(|q| (!q.is_empty()).then(|| q.remove(0)))
                    {
                        self.composer
                            .update(cx, |i, cx| i.set_value(&next_q.text, window, cx));
                        self.send_composer_message(window, cx);
                    }
                }
                Err(err_msg) => {
                    self.show_run_error_notice(&err_msg, cx);
                    let err_var = MessageVariant {
                        id: format!("{}-v{}", run.node_id, run.variant_index),
                        content: format!("**[Provider Error: {err_msg}]**"),
                        thinking_summary: run.accumulated_thinking,
                        model_used: run.model_used,
                        created_at: now_ms(),
                        is_streaming: false,
                        ..Default::default()
                    };
                    self.commit_completed_variant(
                        &cid,
                        &run.node_id,
                        run.variant_index,
                        err_var,
                        cx,
                    );
                }
            }
        }

        if any_dirty {
            cx.notify();
        }
        if !self.active_runs.is_empty() || self.pending_bg_tasks > 0 {
            self.ensure_pump_scheduled(window, cx);
        }
    }

    pub(super) fn handle_bg_task_event(
        &mut self,
        ev: BgTaskEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            BgTaskEvent::ModelsFetched { profile_id, result } => {
                self.is_fetching_models = false;
                match result {
                    Ok(models) => {
                        let count = models.len();
                        if let Some(prof) = self
                            .snapshot
                            .profiles
                            .iter_mut()
                            .find(|p| p.id == profile_id)
                        {
                            prof.saved_models = models;
                            if prof.default_model.trim().is_empty()
                                && let Some(first) = prof.saved_models.first()
                            {
                                prof.default_model = first.clone();
                            }
                        }
                        self.persist_settings();
                        self.sync_provider_inputs_from_active(window, cx);
                        self.models_fetch_status = format!("Loaded {count} models.");
                    }
                    Err(err) => {
                        self.models_fetch_status = format!("Could not fetch models: {err}");
                    }
                }
            }
            BgTaskEvent::ProviderTested { profile_id, result } => {
                if profile_id == self.snapshot.active_profile_id {
                    self.provider_test_status = match result {
                        Ok(msg) => msg,
                        Err(err) => format!("Test failed: {err}"),
                    };
                }
            }
            BgTaskEvent::VoiceRecognized(result) => {
                self.is_voice_listening = false;
                match result {
                    Ok(text) => {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            let cur = self.composer.read(cx).value().to_string();
                            let next = if cur.trim().is_empty() {
                                trimmed.to_string()
                            } else {
                                format!("{cur} {trimmed}")
                            };
                            self.composer
                                .update(cx, |i, cx| i.set_value(&next, window, cx));
                            self.active_session.draft_input = next;
                        }
                    }
                    Err(err) => self.show_notice(format!("Voice input: {err}"), cx),
                }
            }
            BgTaskEvent::SpeechFinished(nid) => {
                if self.speaking_node_id.as_deref() == Some(nid.as_str()) {
                    self.speaking_node_id = None;
                }
            }
            BgTaskEvent::UpdateChecked(result) => {
                self.is_checking_updates = false;
                match result {
                    Ok(Some(info)) => {
                        self.update_status =
                            format!("Update available: {} ({})", info.tag_name, info.asset_name);
                        self.latest_release = Some(info);
                    }
                    Ok(None) => {
                        self.latest_release = None;
                        self.update_status = format!(
                            "You're up to date (v{}).",
                            parchment_core::model::APP_VERSION
                        );
                    }
                    Err(err) => {
                        self.update_status = format!("Update check failed: {err}");
                    }
                }
            }
            BgTaskEvent::TitleGenerated { chat_id, result } => match result {
                Ok((raw_title, usage)) => {
                    let clean = raw_title
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .trim()
                        .to_string();
                    if !clean.is_empty() {
                        if chat_id == self.active_session.id {
                            self.active_session.title = clean.clone();
                            self.active_session.utility_usage_records.extend(usage);
                            self.persist_active_session();
                            window.set_window_title(&strings::window_title_for_chat(&clean));
                        } else if let Ok(Some(mut sess)) = self.core.load_full_session(&chat_id) {
                            sess.title = clean;
                            sess.utility_usage_records.extend(usage);
                            let _ = self.core.save_full_session(&sess);
                            self.refresh_stubs();
                        }
                    }
                }
                Err(err) => self.show_notice(format!("Title generation failed: {err}"), cx),
            },
            BgTaskEvent::SummaryGenerated {
                chat_id,
                compact,
                result,
            } => match result {
                Ok((summary, usage)) => {
                    if chat_id == self.active_session.id {
                        self.active_session.story_summary = summary.clone();
                        self.active_session.utility_usage_records.extend(usage);
                        if compact && self.active_session.messages.len() > 6 {
                            let mute_end = self.active_session.messages.len().saturating_sub(4);
                            for node in &mut self.active_session.messages[..mute_end] {
                                node.included_in_context = false;
                            }
                        }
                        self.persist_active_session();
                        self.sync_notes_inputs_from_active(window, cx);
                        self.rebuild_rows_from_active_session(cx, false);
                        self.show_notice(
                            if compact {
                                "Shortened conversation and updated remembered summary."
                            } else {
                                "Updated remembered summary."
                            },
                            cx,
                        );
                    }
                }
                Err(err) => self.show_notice(format!("Summary failed: {err}"), cx),
            },
            BgTaskEvent::TranslationGenerated {
                chat_id,
                node_id,
                result,
            } => match result {
                Ok((translated, usage)) => {
                    if chat_id == self.active_session.id
                        && let Some(node) = self
                            .active_session
                            .messages
                            .iter_mut()
                            .find(|n| n.id == node_id)
                    {
                        let v_idx = node.variants.len();
                        node.variants.push(MessageVariant {
                            id: format!("{node_id}-tr-{v_idx}"),
                            content: translated,
                            model_used: "Translation".into(),
                            created_at: now_ms(),
                            api_usage_records: usage,
                            ..Default::default()
                        });
                        node.active_variant_index = v_idx;
                        self.persist_active_session();
                        self.rebuild_rows_from_active_session(cx, false);
                        self.show_notice("Added translation as a new variant.", cx);
                    }
                }
                Err(err) => self.show_notice(format!("Translation failed: {err}"), cx),
            },
            BgTaskEvent::DraftImpersonated(res) => {
                self.is_drafting_reply = false;
                match res {
                    Ok(draft) => {
                        let clean = draft.trim().to_string();
                        if !clean.is_empty() {
                            self.composer
                                .update(cx, |i, cx| i.set_value(&clean, window, cx));
                            self.active_session.draft_input = clean;
                            self.show_notice("Drafted reply into composer.", cx);
                        }
                    }
                    Err(err) => self.show_notice(format!("Draft failed: {err}"), cx),
                }
            }
            BgTaskEvent::ModelCatalogSynced(ok) => {
                self.is_fetching_models = false;
                if ok {
                    if let ProvidersSubpage::ModelConfig(mid) = self.providers_subpage.clone() {
                        if let Some(spec) = self.core.registry().get_cached_spec(&mid) {
                            if let Some(p) = self
                                .snapshot
                                .profiles
                                .iter_mut()
                                .find(|p| p.id == self.snapshot.active_profile_id)
                            {
                                p.custom_model_specs.insert(mid.clone(), spec);
                            }
                            self.persist_settings();
                            self.sync_model_cfg_inputs(&mid, window, cx);
                            self.show_notice(
                                format!("Synced live catalog pricing & limits for {mid}."),
                                cx,
                            );
                        } else {
                            self.show_notice(
                                "Synced catalog (no entry matched this model ID).",
                                cx,
                            );
                        }
                    } else {
                        self.show_notice("Synced models.dev pricing & capability catalog.", cx);
                    }
                } else {
                    self.show_notice("Could not refresh models.dev catalog.", cx);
                }
            }
            BgTaskEvent::McpStatusesUpdated(map) => {
                self.mcp_statuses = map;
            }
            BgTaskEvent::McpRegistrySearched(res) => {
                self.is_searching_mcp_registry = false;
                match res {
                    Ok(items) => {
                        let n = items.len();
                        self.mcp_registry_results = items;
                        if n == 0 {
                            self.show_notice("No matching MCP servers found.", cx);
                        }
                    }
                    Err(err) => self.show_notice(format!("MCP registry search failed: {err}"), cx),
                }
            }
            BgTaskEvent::SkillInstalledFromUrl(res) => match res {
                Ok(name) => {
                    self.refresh_memory_and_skills();
                    self.settings_inputs
                        .skill_github_url
                        .update(cx, |i, cx| i.set_value("", window, cx));
                    self.show_notice(format!("Installed skill \"{name}\"."), cx);
                }
                Err(err) => self.show_notice(format!("Skill install failed: {err}"), cx),
            },
        }
    }

    // ---- Async background task launchers (zero `block_on`) ----

    pub(super) fn resolve_utility_profile_and_model(
        &self,
        kind: &str,
    ) -> Option<(ProviderProfile, String)> {
        let dm = &self.snapshot.default_models;
        let (follow, pid, mid) = match kind {
            "title" => (
                dm.title_follow_global,
                &dm.title_profile_id,
                &dm.title_model_id,
            ),
            "summary" => (
                dm.summary_follow_global,
                &dm.summary_profile_id,
                &dm.summary_model_id,
            ),
            "translation" => (
                dm.translation_follow_global,
                &dm.translation_profile_id,
                &dm.translation_model_id,
            ),
            _ => (true, &dm.global_profile_id, &dm.global_model_id),
        };
        let prof = if !follow && !pid.is_empty() {
            self.snapshot
                .profiles
                .iter()
                .find(|p| p.id == *pid)
                .cloned()
                .unwrap_or_else(|| self.active_profile())
        } else {
            self.active_profile()
        };
        let model = if !follow && !mid.trim().is_empty() {
            mid.trim().to_string()
        } else {
            self.effective_model_id(&prof)
        };
        if self.validate_provider_ready(&prof).is_some() || model.is_empty() {
            return None;
        }
        Some((prof, model))
    }

    pub(super) fn spawn_fetch_provider_models(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_fetching_models {
            return;
        }
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
        self.is_fetching_models = true;
        self.models_fetch_status = "Fetching models…".into();
        self.pending_bg_tasks += 1;
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        let pid = prof.id.clone();
        self.core.runtime().spawn(async move {
            let client = core_bg.protocol_client();
            let res = client
                .fetch_available_models(&prof)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::ModelsFetched {
                profile_id: pid,
                result: res,
            });
        });
        self.ensure_pump_scheduled(window, cx);
        cx.notify();
    }

    pub(super) fn spawn_check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_checking_updates {
            return;
        }
        self.is_checking_updates = true;
        self.update_status = "Checking GitHub Releases…".into();
        self.pending_bg_tasks += 1;
        let http = self.core.http_client().clone();
        let repo = self.snapshot.ota_github_repo.clone();
        let ver = parchment_core::model::APP_VERSION.to_string();
        let tx = self.bg_tx.clone();
        self.core.runtime().spawn(async move {
            let res = check_latest_github_release(&http, &repo, &ver).await;
            let _ = tx.send(BgTaskEvent::UpdateChecked(res));
        });
        self.ensure_pump_scheduled(window, cx);
        cx.notify();
    }

    pub(super) fn spawn_generate_title(
        &mut self,
        chat_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((prof, model)) = self.resolve_utility_profile_and_model("title") else {
            return;
        };
        let sess = if chat_id == self.active_session.id {
            self.active_session.clone()
        } else if let Ok(Some(s)) = self.core.load_full_session(chat_id) {
            s
        } else {
            return;
        };
        let transcript: String = sess
            .messages
            .iter()
            .take(6)
            .map(|m| {
                let who = if m.role == MessageRole::User {
                    "User"
                } else {
                    "Assistant"
                };
                let body: String = m.active_content().chars().take(400).collect();
                format!("{who}: {body}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if transcript.trim().is_empty() {
            return;
        }
        let sys = self.snapshot.default_models.title_prompt_template.clone();
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        let cid = chat_id.to_string();
        self.pending_bg_tasks += 1;
        self.core.runtime().spawn(async move {
            let client = core_bg.protocol_client();
            let res = client
                .run_utility_prompt(&prof, &model, &sys, &transcript, "title")
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::TitleGenerated {
                chat_id: cid,
                result: res,
            });
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn spawn_summarize_or_compact(
        &mut self,
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((prof, model)) = self.resolve_utility_profile_and_model("summary") else {
            self.show_notice("Configure a provider and model first.", cx);
            return;
        };
        let transcript: String = self
            .active_session
            .messages
            .iter()
            .filter(|m| m.included_in_context)
            .map(|m| {
                let who = if m.role == MessageRole::User {
                    "User"
                } else {
                    "Assistant"
                };
                let body: String = m.active_content().chars().take(1200).collect();
                format!("{who}: {body}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        if transcript.trim().is_empty() {
            return;
        }
        let sys = "Summarize the key facts, decisions, code paths, constraints, and open questions from this conversation concisely so future replies stay accurate.";
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        let cid = self.active_session.id.clone();
        self.pending_bg_tasks += 1;
        self.show_notice(
            if compact {
                "Shortening conversation…"
            } else {
                "Summarizing conversation…"
            },
            cx,
        );
        self.core.runtime().spawn(async move {
            let client = core_bg.protocol_client();
            let res = client
                .run_utility_prompt(&prof, &model, sys, &transcript, "summary")
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::SummaryGenerated {
                chat_id: cid,
                compact,
                result: res,
            });
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn spawn_translate_message(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((prof, model)) = self.resolve_utility_profile_and_model("translation") else {
            self.show_notice("Configure a provider and model first.", cx);
            return;
        };
        let Some(node) = self.active_session.messages.get(index) else {
            return;
        };
        let text = node.active_content().to_string();
        if text.trim().is_empty() {
            return;
        }
        let target_lang = self
            .snapshot
            .default_models
            .translation_target_language
            .trim()
            .to_string();
        let lang = if target_lang.is_empty() {
            "English".to_string()
        } else {
            target_lang
        };
        let sys = format!(
            "Translate the following text naturally and accurately into {lang}. Preserve Markdown formatting and code blocks. Return ONLY the translation."
        );
        let core_bg = self.core.clone();
        let tx = self.bg_tx.clone();
        let cid = self.active_session.id.clone();
        let nid = node.id.clone();
        self.pending_bg_tasks += 1;
        self.show_notice(format!("Translating to {lang}…"), cx);
        self.core.runtime().spawn(async move {
            let client = core_bg.protocol_client();
            let res = client
                .run_utility_prompt(&prof, &model, &sys, &text, "translation")
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(BgTaskEvent::TranslationGenerated {
                chat_id: cid,
                node_id: nid,
                result: res,
            });
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn respond_approval(
        &mut self,
        chat_id: &str,
        request_id: String,
        decision: ToolApprovalDecision,
        cx: &mut Context<Self>,
    ) {
        if let Some(run) = self.active_runs.get(chat_id) {
            let broker = run.broker.clone();
            self.core.runtime().spawn(async move {
                broker.respond(&request_id, decision).await;
            });
            cx.notify();
        }
    }

    pub(super) fn answer_user_question(
        &mut self,
        chat_id: &str,
        request_id: String,
        answer: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trimmed = answer.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        if let Some(run) = self.active_runs.get(chat_id) {
            let broker = run.broker.clone();
            self.core.runtime().spawn(async move {
                broker.answer_question(&request_id, trimmed).await;
            });
            self.question_answer_input
                .update(cx, |i, cx| i.set_value("", window, cx));
            cx.notify();
        }
    }

    pub(super) fn execute_regenerate_turn(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(role) = self.active_session.messages.get(ix).map(|n| n.role) else {
            return;
        };
        if role == MessageRole::User {
            self.active_session.messages.truncate(ix + 1);
            self.persist_active_session();
            self.rebuild_rows_from_active_session(cx, true);
            self.launch_run_on_active_chat(None, false, window, cx);
        } else {
            if self
                .snapshot
                .preferences
                .delete_messages_below_when_regenerating
            {
                self.active_session.messages.truncate(ix + 1);
            }
            if self
                .snapshot
                .preferences
                .delete_previous_message_when_regenerating
                && let Some(node) = self.active_session.messages.get_mut(ix)
                && !node.variants.is_empty()
            {
                let vi = node.active_variant_index.min(node.variants.len() - 1);
                node.variants.remove(vi);
                node.active_variant_index = node.variants.len().saturating_sub(1);
            }
            self.persist_active_session();
            self.launch_run_on_active_chat(Some(ix), false, window, cx);
        }
    }

    pub(super) fn execute_clear_all_for_assistant(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let aid = self.snapshot.active_assistant_id.clone();
        let ids: Vec<String> = self
            .stubs
            .iter()
            .filter(|s| {
                self.snapshot.assistants.len() <= 1
                    || s.assistant_id.is_empty()
                    || s.assistant_id == aid
            })
            .map(|s| s.id.clone())
            .collect();
        for id in &ids {
            self.stop_run_for_chat(id, cx);
            let _ = self.core.delete_chat(id);
            self.open_tabs.retain(|t| t != id);
        }
        self.refresh_stubs();
        self.refresh_pinned();
        self.create_new_chat(window, cx);
        self.show_notice(format!("Cleared {} chats.", ids.len()), cx);
    }

    pub(super) fn execute_clear_current_messages(&mut self, cx: &mut Context<Self>) {
        self.active_session.messages.clear();
        self.persist_active_session();
        self.rebuild_rows_from_active_session(cx, false);
        self.show_notice("Cleared messages in this chat.", cx);
    }

    // ---- Action Dispatchers ----
}
