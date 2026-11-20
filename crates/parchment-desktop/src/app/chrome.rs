use super::*;

impl ParchmentDesktopApp {
    pub(super) fn render_top_editorial_bar(
        &self,
        profile: &ProviderProfile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = &self.palette;
        let ink = p.primary_ink.to_hsla();
        let muted = p.muted_ink.to_hsla();
        let hover_bg = p.hover_tint();

        let active_asst = self.active_assistant();
        let asst_symbol = active_asst.map(|a| a.icon_emoji.as_str()).unwrap_or("");
        let model_id = self.effective_model_id(profile);
        let model_label = if model_id.is_empty() {
            strings::SELECT_MODEL.to_string()
        } else if self.snapshot.preferences.show_provider_in_model_capsule {
            format!("{model_id} on {}", profile.name)
        } else {
            model_id
        };
        let vault_count = count_vault_items(&self.active_session, &self.pending_attachments);
        let has_messages = !self.active_session.messages.is_empty();
        let has_chat_notes = !self.active_session.system_prompt.trim().is_empty()
            || !self.active_session.authors_note.trim().is_empty()
            || !self.active_session.story_summary.trim().is_empty();
        let show_model_popover = self.overlay == ActiveOverlay::QuickModelPicker;
        let chat_id_title = self.active_session.id.clone();

        let title_row: AnyElement = if self.is_editing_title {
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(div().w(px(260.0)).child(Input::new(&self.title_edit_input)))
                .child(sepia_button(
                    "top-title-save",
                    strings::SAVE,
                    None,
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    cx.listener(|this, _, window, cx| this.commit_title_edit(window, cx)),
                ))
                .into_any_element()
        } else {
            h_flex()
                .items_center()
                .gap(px(5.0))
                .child(
                    div()
                        .id("top-chat-title")
                        .max_w(px(340.0))
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded(px(6.0))
                        .truncate()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .text_size(px(15.0))
                        .font_semibold()
                        .text_color(ink)
                        .on_click(cx.listener(|this, _, window, cx| {
                            let cur = this.active_session.title.clone();
                            this.is_editing_title = true;
                            this.title_edit_input
                                .update(cx, |i, cx| i.set_value(&cur, window, cx));
                            cx.notify();
                        }))
                        .child(self.active_session.title.clone()),
                )
                .children(has_messages.then(|| {
                    with_tooltip(
                        icon_button(
                            "top-regen-title",
                            KissIcon::Sparkles,
                            false,
                            p,
                            cx.listener(move |this, _, window, cx| {
                                this.spawn_generate_title(&chat_id_title, window, cx);
                            }),
                        ),
                        "Regenerate title",
                    )
                }))
                .into_any_element()
        };
        // Line 2 of editorial title stack: quiet muted `<model> on <Provider> ⌄` opening QuickModelPopover
        let model_capsule = h_flex()
            .id("top-model-capsule")
            .h(px(22.0))
            .px(px(4.0))
            .rounded(px(6.0))
            .items_center()
            .gap(px(4.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_click(cx.listener(|this, _, window, cx| {
                if this.overlay == ActiveOverlay::QuickModelPicker {
                    this.overlay = ActiveOverlay::None;
                    cx.notify();
                } else {
                    this.open_overlay(
                        ActiveOverlay::QuickModelPicker,
                        "Filter models or providers…",
                        window,
                        cx,
                    );
                }
            }))
            .child(
                div()
                    .max_w(px(360.0))
                    .truncate()
                    .text_size(px(12.0))
                    .text_color(muted)
                    .child(model_label),
            )
            .child(kiss_icon(
                if show_model_popover {
                    KissIcon::ChevronUp
                } else {
                    KissIcon::ChevronDown
                },
                12.0,
                muted,
            ));

        let title_stack = v_flex()
            .flex_1()
            .items_start()
            .overflow_hidden()
            .gap(px(1.0))
            .child(title_row)
            .child(kiss_bounce_subtle("hdr-model-kb", model_capsule));
        let left = h_flex()
            .items_center()
            .gap(px(8.0))
            .flex_1()
            .overflow_hidden()
            .child(with_tooltip(
                icon_button(
                    "top-sidebar-tog",
                    if self.sidebar_open {
                        KissIcon::SidebarClose
                    } else {
                        KissIcon::Menu
                    },
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.sidebar_open = !this.sidebar_open;
                        this.snapshot.preferences.sidebar_open = this.sidebar_open;
                        this.persist_settings();
                        cx.notify();
                    }),
                ),
                if self.sidebar_open {
                    "Hide sidebar"
                } else {
                    "Show sidebar"
                },
            ))
            .children(
                self.snapshot
                    .preferences
                    .show_assistant_avatar_in_title_bar
                    .then(|| {
                        kiss_bounce(
                            "top-asst-avatar-kb",
                            div()
                                .id("top-asst-avatar")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_settings_at(SettingsSection::Assistants, window, cx);
                                }))
                                .child(sepia_avatar_circle(asst_symbol, true, 28.0, p)),
                        )
                    }),
            )
            .child(title_stack);

        let right = h_flex()
            .items_center()
            .gap(px(6.0))
            .flex_shrink_0()
            .children((vault_count > 0 || self.vault_open).then(|| {
                sepia_button(
                    "top-vault-btn",
                    format!("{vault_count}"),
                    Some(KissIcon::Folder),
                    if self.vault_open {
                        SepiaButtonStyle::Outline
                    } else {
                        SepiaButtonStyle::Ghost
                    },
                    true,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.vault_open = !this.vault_open;
                        cx.notify();
                    }),
                )
            }))
            .child(with_tooltip(
                icon_button(
                    "top-notes-btn",
                    KissIcon::Edit,
                    self.notes_open || has_chat_notes,
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.notes_open = !this.notes_open;
                        if this.notes_open {
                            this.sync_notes_inputs_from_active(window, cx);
                        }
                        cx.notify();
                    }),
                ),
                "Chat notes & memory",
            ))
            .child(with_tooltip(
                icon_button(
                    "top-settings-btn",
                    KissIcon::Settings,
                    false,
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.open_settings_at(this.settings_section, window, cx);
                    }),
                ),
                "Settings",
            ));
        h_flex()
            .w_full()
            .min_h(px(52.0))
            .px(px(12.0))
            .py(px(6.0))
            .bg(p.canvas_bg.to_hsla())
            .justify_between()
            .items_center()
            .gap(px(10.0))
            .child(left)
            .child(right)
            .into_any_element()
    }

    pub(super) fn render_desktop_tab_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.open_tabs.len() <= 1 {
            return None;
        }
        let p = &self.palette;
        let sheet = p.sheet_bg.with_alpha(0.88);
        let card = p.card_bg.to_hsla();
        let ink = p.primary_ink.to_hsla();
        let narration = p.narration_ink.to_hsla();
        let muted = p.muted_ink.to_hsla();
        let accent = p.accent.to_hsla();
        let hover_bg = p.hover_tint();

        let mut tab_strip = h_flex()
            .flex_1()
            .overflow_hidden()
            .items_center()
            .gap(px(6.0));

        for (ix, tid) in self.open_tabs.iter().enumerate() {
            let is_active = tid == &self.active_session.id;
            let is_streaming = self.active_runs.contains_key(tid);
            let title = if is_active {
                self.active_session.title.clone()
            } else {
                self.stubs
                    .iter()
                    .find(|s| &s.id == tid)
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| strings::NEW_CHAT.into())
            };
            let tid_sel = tid.clone();
            let tid_close = tid.clone();
            let mut pill = h_flex()
                .id(("desk-tab", ix))
                .relative()
                .overflow_hidden()
                .max_w(px(210.0))
                .px(px(11.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .items_center()
                .gap(px(7.0))
                .cursor_pointer()
                .bg(if is_active {
                    card
                } else {
                    p.canvas_bg.with_alpha(0.0)
                })
                .hover(move |s| s.bg(if is_active { card } else { hover_bg }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_chat_by_id(&tid_sel, window, cx);
                }));
            if is_active {
                pill = pill.child(rubric_mark_overlay(7.0, p));
            }
            pill = pill
                .children(is_streaming.then(|| streaming_pulse_dot(("desk-tab-pulse", ix), accent)))
                .child(
                    div()
                        .flex_1()
                        .max_w(px(185.0))
                        .truncate()
                        .text_size(px(13.0))
                        .font_medium()
                        .text_color(if is_active { ink } else { narration })
                        .child(title),
                )
                .child(
                    div()
                        .id(("desk-tab-x", ix))
                        .w(px(22.0))
                        .h(px(22.0))
                        .rounded(px(999.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(&tid_close, window, cx);
                        }))
                        .child(kiss_icon(KissIcon::Close, 10.0, muted)),
                );
            tab_strip = tab_strip.child(kiss_bounce_subtle(("desk-tab-kb", ix), pill));
        }

        tab_strip = tab_strip.child(icon_button(
            "tabbar-new",
            KissIcon::Plus,
            false,
            p,
            cx.listener(|this, _, window, cx| this.create_new_chat(window, cx)),
        ));

        Some(
            h_flex()
                .w_full()
                .px(px(10.0))
                .py(px(4.0))
                .bg(sheet)
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(tab_strip)
                .child(sepia_button(
                    "tabbar-all",
                    format!("{} Tabs", self.open_tabs.len()),
                    Some(KissIcon::Tabs),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.overlay = ActiveOverlay::TabSwitcher;
                        cx.notify();
                    }),
                ))
                .into_any_element(),
        )
    }

    /// Empty chat intro matching `EmptyChatIntro` in `EmptyChatAndNotes.kt` (`desktop-empty.png`).
    pub(super) fn render_empty_chat_intro(
        &self,
        profile: &ProviderProfile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = &self.palette;
        let ink = p.primary_ink.to_hsla();
        let muted = p.muted_ink.to_hsla();
        let active_asst = self.active_assistant();
        let active_model = self.effective_model_id(profile);
        let card = self
            .active_session
            .active_card
            .as_ref()
            .or_else(|| active_asst.and_then(|a| a.active_card.as_ref()));
        let display_name = card
            .map(|c| c.name.as_str())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                active_asst
                    .map(|a| a.name.as_str())
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or(strings::DEFAULT_ASSISTANT);
        let heading = if display_name == strings::DEFAULT_ASSISTANT {
            strings::EMPTY_START_HEADING.to_string()
        } else {
            display_name.to_string()
        };
        let subtitle = if active_model.is_empty() {
            strings::CHOOSE_A_MODEL_IN_SETTINGS.to_string()
        } else {
            active_model
        };
        let asst_icon = active_asst.map(|a| a.icon_emoji.as_str()).unwrap_or("");
        let show_custom_avatar = !asst_icon.trim().is_empty() && asst_icon.trim() != "💬";
        let needs_key = Self::profile_needs_api_key(profile);
        let has_opening_scene = card.is_some_and(|c| !c.first_message.trim().is_empty());

        let mut col = v_flex()
            .relative()
            .w_full()
            .pl(px(16.0))
            .py(px(4.0))
            .gap(px(6.0))
            .child(rubric_mark_overlay(4.0, p));

        if show_custom_avatar {
            col = col.child(sepia_avatar_circle(asst_icon, true, 40.0, p));
        }

        col = col
            .child(
                div()
                    .text_size(px(21.0))
                    .line_height(px(28.0))
                    .font_semibold()
                    .text_color(ink)
                    .child(heading),
            )
            .child(div().text_size(px(12.5)).text_color(muted).child(subtitle));

        if needs_key {
            col = col
                .child(
                    div()
                        .pt(px(4.0))
                        .text_size(px(13.5))
                        .text_color(ink)
                        .child(strings::add_api_key_to_start(&profile.name)),
                )
                .child(h_flex().child(sepia_button(
                    "empty-setup-btn",
                    strings::set_up_provider(&profile.name),
                    None,
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    cx.listener(|this, _, window, cx| {
                        this.open_settings_at(SettingsSection::Providers, window, cx);
                    }),
                )));
        }

        if has_opening_scene {
            col = col.child(h_flex().pt(px(4.0)).child(sepia_button(
                "empty-opening-scene-btn",
                strings::START_OPENING_SCENE,
                None,
                SepiaButtonStyle::Primary,
                true,
                false,
                p,
                cx.listener(|this, _, _, cx| {
                    this.insert_card_opening_scene(cx);
                }),
            )));
        }

        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .w_full()
            .px(px(16.0))
            .pt(px(44.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(p.max_reading_width_px))
                    .mx_auto()
                    .child(col),
            )
            .into_any_element()
    }

    pub(super) fn insert_card_opening_scene(&mut self, cx: &mut Context<Self>) {
        let card = self
            .active_session
            .active_card
            .clone()
            .or_else(|| self.active_assistant().and_then(|a| a.active_card.clone()));
        let Some(card) = card else {
            return;
        };
        let mut all_greetings = Vec::new();
        if !card.first_message.trim().is_empty() {
            all_greetings.push(card.first_message.clone());
        }
        for alt in &card.alternate_greetings {
            if !alt.trim().is_empty() {
                all_greetings.push(alt.clone());
            }
        }
        if all_greetings.is_empty() {
            return;
        }
        let user_name = if !self.snapshot.preferences.profile_name.trim().is_empty() {
            self.snapshot.preferences.profile_name.clone()
        } else if !self.active_session.user_name.trim().is_empty() {
            self.active_session.user_name.clone()
        } else {
            "User".to_string()
        };
        let now = now_ms();
        let node = MessageNode {
            id: format!("node-greeting-{now}"),
            role: MessageRole::Assistant,
            variants: all_greetings
                .into_iter()
                .enumerate()
                .map(|(idx, g)| MessageVariant {
                    id: format!("var-greeting-{idx}-{now}"),
                    content: g
                        .replace("{{char}}", &card.name)
                        .replace("{{user}}", &user_name),
                    model_used: format!("Card Greeting #{}", idx + 1),
                    created_at: now,
                    ..Default::default()
                })
                .collect(),
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        };
        self.active_session.messages = vec![node];
        self.persist_active_session();
        self.rebuild_rows_from_active_session(cx, true);
        cx.notify();
    }

    pub(super) fn render_approval_and_retry_banners(
        &self,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = &self.palette;
        let ink = p.primary_ink.to_hsla();
        let muted = p.muted_ink.to_hsla();
        let accent = p.accent.to_hsla();
        let danger = p.danger.to_hsla();
        let elev = p.elevated_bg.to_hsla();
        let card = p.card_bg.to_hsla();
        let mut out = Vec::new();

        let Some(run) = self.active_runs.get(&self.active_session.id) else {
            return out;
        };

        // 1. Tool Approval Cards (with ShellCompoundBreakdown for shell_exec)
        for (ix, req) in run.pending_approvals.iter().enumerate() {
            let cid = run.chat_id.clone();
            let cid2 = cid.clone();
            let cid3 = cid.clone();
            let cid4 = cid.clone();
            let r1 = req.request_id.clone();
            let r2 = req.request_id.clone();
            let r3 = req.request_id.clone();
            let r4 = req.request_id.clone();
            let is_cu = req.tool_name == "computer_use";
            let is_shell = req.tool_name == "shell_exec";

            let body_preview: AnyElement = if is_shell {
                let cmd_text = serde_json::from_str::<serde_json::Value>(&req.arguments_json)
                    .ok()
                    .and_then(|v| {
                        v.get("command")
                            .and_then(|c| c.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| req.arguments_json.clone());
                let parts = split_shell_compound_segments(&cmd_text);
                let mut seg_col = v_flex().w_full().gap(px(4.0));
                for (s_ix, seg) in parts.iter().enumerate() {
                    let risky = is_risky_shell_segment(seg);
                    seg_col = seg_col.child(
                        h_flex()
                            .w_full()
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(6.0))
                            .bg(if risky {
                                p.danger.with_alpha(0.11)
                            } else {
                                card
                            })
                            .border_1()
                            .border_color(if risky {
                                p.danger.with_alpha(0.42)
                            } else {
                                p.subtle_border()
                            })
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .font_semibold()
                                    .text_color(if risky { danger } else { muted })
                                    .child(format!("{}.", s_ix + 1)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .font_family(p.code_font_family.clone())
                                    .text_size(px(12.0))
                                    .text_color(if risky { danger } else { ink })
                                    .child(seg.clone()),
                            )
                            .children(risky.then(|| {
                                div()
                                    .text_size(px(10.5))
                                    .font_semibold()
                                    .text_color(danger)
                                    .child("Destructive")
                            })),
                    );
                }
                seg_col.into_any_element()
            } else {
                div()
                    .w_full()
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .bg(card)
                    .font_family(p.code_font_family.clone())
                    .text_size(px(12.0))
                    .text_color(muted)
                    .child(req.arguments_json.clone())
                    .into_any_element()
            };

            out.push(
                v_flex()
                    .id(("approval-card", ix))
                    .w_full()
                    .max_w(px(p.max_reading_width_px))
                    .mx_auto()
                    .mb(px(6.0))
                    .p(px(12.0))
                    .rounded(px(10.0))
                    .bg(elev)
                    .border_1()
                    .border_color(accent)
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(kiss_icon(KissIcon::ShieldCheck, 15.0, accent))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(format!("Approve tool: {}", req.tool_name)),
                            ),
                    )
                    .child(body_preview)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(sepia_button(
                                ("appr-once", ix),
                                "Allow once",
                                Some(KissIcon::Check),
                                SepiaButtonStyle::Primary,
                                true,
                                false,
                                p,
                                cx.listener(move |this, _, _, cx| {
                                    this.respond_approval(
                                        &cid,
                                        r1.clone(),
                                        ToolApprovalDecision::AllowOnce,
                                        cx,
                                    );
                                }),
                            ))
                            .children((!is_cu).then(|| {
                                sepia_button(
                                    ("appr-run", ix),
                                    "Allow for run",
                                    None,
                                    SepiaButtonStyle::Outline,
                                    true,
                                    false,
                                    p,
                                    cx.listener(move |this, _, _, cx| {
                                        this.respond_approval(
                                            &cid2,
                                            r2.clone(),
                                            ToolApprovalDecision::AllowExactActionForRun,
                                            cx,
                                        );
                                    }),
                                )
                            }))
                            .child(sepia_button(
                                ("appr-deny", ix),
                                "Deny",
                                Some(KissIcon::Close),
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                cx.listener(move |this, _, _, cx| {
                                    this.respond_approval(
                                        &cid3,
                                        r3.clone(),
                                        ToolApprovalDecision::Deny,
                                        cx,
                                    );
                                }),
                            ))
                            .child(sepia_button(
                                ("appr-stop", ix),
                                strings::STOP,
                                Some(KissIcon::Stop),
                                SepiaButtonStyle::Danger,
                                true,
                                false,
                                p,
                                cx.listener(move |this, _, _, cx| {
                                    this.respond_approval(
                                        &cid4,
                                        r4.clone(),
                                        ToolApprovalDecision::Stop,
                                        cx,
                                    );
                                }),
                            )),
                    )
                    .into_any_element(),
            );
        }

        // 2. Interactive `ask_user` UserQuestionPanel cards
        for (q_ix, q) in run.pending_questions.iter().enumerate() {
            let cid_submit = run.chat_id.clone();
            let rid_submit = q.request_id.clone();
            let mut opt_row = h_flex().w_full().flex_wrap().gap(px(6.0));
            for (o_ix, opt) in q.options.iter().enumerate() {
                let cid_o = run.chat_id.clone();
                let rid_o = q.request_id.clone();
                let ans_o = opt.clone();
                opt_row = opt_row.child(sepia_button(
                    ("uq-opt", q_ix * 16 + o_ix),
                    opt.clone(),
                    None,
                    SepiaButtonStyle::Secondary,
                    true,
                    false,
                    p,
                    cx.listener(move |this, _, window, cx| {
                        this.answer_user_question(&cid_o, rid_o.clone(), ans_o.clone(), window, cx);
                    }),
                ));
            }
            out.push(
                v_flex()
                    .id(("user-question-card", q_ix))
                    .w_full()
                    .max_w(px(p.max_reading_width_px))
                    .mx_auto()
                    .mb(px(6.0))
                    .p(px(12.0))
                    .rounded(px(10.0))
                    .bg(elev)
                    .border_1()
                    .border_color(accent)
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(kiss_icon(KissIcon::MessageCircle, 15.0, accent))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(q.question.clone()),
                            ),
                    )
                    .children((!q.options.is_empty()).then_some(opt_row))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(6.0))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.question_answer_input)),
                            )
                            .child(sepia_button(
                                ("uq-send", q_ix),
                                "Answer",
                                Some(KissIcon::SendEnter),
                                SepiaButtonStyle::Primary,
                                true,
                                false,
                                p,
                                cx.listener(move |this, _, window, cx| {
                                    let txt =
                                        this.question_answer_input.read(cx).value().to_string();
                                    this.answer_user_question(
                                        &cid_submit,
                                        rid_submit.clone(),
                                        txt,
                                        window,
                                        cx,
                                    );
                                }),
                            )),
                    )
                    .into_any_element(),
            );
        }

        out
    }

    /// Floating `↓ Latest` pill + right-edge Message Navigator rail + Interactive Message Map
    /// matching `ManuscriptViewport.kt` & `MessageMinimap.kt`.
    pub(super) fn render_viewport_nav_overlays(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = &self.palette;
        let rows_ref = self.messages.borrow();
        let total = rows_ref.len();
        if total == 0 {
            return Vec::new();
        }
        let mut overlays: Vec<AnyElement> = Vec::new();
        let scroller_ref = self.scroller.read(cx);
        let is_following = scroller_ref.is_following_tail();
        let top_ix = scroller_ref.logical_scroll_top().item_ix.min(total - 1);

        // 1. Floating bottom-center `↓ Latest` accent pill when scrolled up
        if !is_following && total > 1 {
            let on_fill = p.on_accent_ink.to_hsla();
            overlays.push(
                div()
                    .absolute()
                    .bottom(px(12.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(spring_pop_enter(
                        "latest-pill-anim",
                        h_flex()
                            .id("scroll-to-latest-pill")
                            .px(px(14.0))
                            .py(px(7.0))
                            .rounded(px(999.0))
                            .bg(p.accent.to_hsla())
                            .shadow_md()
                            .items_center()
                            .gap(px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.opacity(0.92))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
                                cx.notify();
                            }))
                            .child(kiss_icon(KissIcon::ArrowDown, 13.0, on_fill))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(on_fill)
                                    .child("Latest"),
                            ),
                    ))
                    .into_any_element(),
            );
        }

        // 2. Right-edge Message Navigator rail + expandable Interactive Message Map
        let nav_vis = self.snapshot.preferences.message_nav_buttons;
        let lingering = self
            .minimap_scroll_linger_until
            .is_some_and(|until| Instant::now() < until);
        let show_rail = total > 1
            && match nav_vis {
                MessageNavVisibility::Always => true,
                MessageNavVisibility::WhileScrolling => self.minimap_open || lingering,
                MessageNavVisibility::Hidden => false,
            };

        if show_rail {
            let prev_ix = top_ix.saturating_sub(1);
            let next_ix = (top_ix + 1).min(total - 1);

            let rail = v_flex()
                .id("msg-nav-rail")
                .p(px(4.0))
                .rounded(px(10.0))
                .bg(p.elevated_bg.with_alpha(0.94))
                .border_1()
                .border_color(p.subtle_border())
                .shadow_md()
                .gap(px(2.0))
                .child(icon_button(
                    "nav-top",
                    KissIcon::ChevronsUp,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.scroller.update(cx, |s, cx| {
                            s.scroll_to_item(0, cx);
                        });
                        cx.notify();
                    }),
                ))
                .child(icon_button(
                    "nav-prev",
                    KissIcon::ChevronUp,
                    false,
                    p,
                    cx.listener(move |this, _, _, cx| {
                        this.scroller.update(cx, |s, cx| {
                            s.scroll_to_item(prev_ix, cx);
                        });
                        cx.notify();
                    }),
                ))
                .child(icon_button(
                    "nav-map",
                    KissIcon::MapTree,
                    self.minimap_open,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.minimap_open = !this.minimap_open;
                        cx.notify();
                    }),
                ))
                .child(icon_button(
                    "nav-next",
                    KissIcon::ChevronDown,
                    false,
                    p,
                    cx.listener(move |this, _, _, cx| {
                        this.scroller.update(cx, |s, cx| {
                            s.scroll_to_item(next_ix, cx);
                        });
                        cx.notify();
                    }),
                ))
                .child(icon_button(
                    "nav-end",
                    KissIcon::ChevronsDown,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.scroller.update(cx, |s, cx| s.scroll_to_end(cx));
                        cx.notify();
                    }),
                ));

            let mut container = h_flex()
                .absolute()
                .right(px(14.0))
                .bottom(px(18.0))
                .items_end()
                .gap(px(8.0));

            if self.minimap_open {
                let q_low = self
                    .minimap_filter_input
                    .read(cx)
                    .value()
                    .trim()
                    .to_ascii_lowercase();
                let hover_bg = p.hover_tint();
                let mut map_list = v_flex()
                    .id("minimap-scroll")
                    .max_h(px(340.0))
                    .w_full()
                    .gap(px(4.0))
                    .overflow_y_scrollbar();

                for (ix, row) in rows_ref.iter().enumerate() {
                    if self.minimap_prompts_only && !row.is_user {
                        continue;
                    }
                    let snippet: String = row.content.replace('\n', " ").chars().take(68).collect();
                    if !q_low.is_empty() && !snippet.to_ascii_lowercase().contains(&q_low) {
                        continue;
                    }
                    let is_cur = ix == top_ix;
                    let v_count = row.variant_count;
                    let v_active = row.variant_index;
                    let mut entry = v_flex()
                        .id(("mm-row", ix))
                        .relative()
                        .w_full()
                        .px(px(9.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .bg(if is_cur {
                            p.accent.with_alpha(0.11)
                        } else {
                            p.card_bg.to_hsla()
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .gap(px(3.0))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.scroller.update(cx, |s, cx| {
                                s.scroll_to_item(ix, cx);
                            });
                            cx.notify();
                        }));
                    if is_cur {
                        entry = entry.child(rubric_mark_overlay(5.0, p));
                    }
                    entry = entry.child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .font_semibold()
                                    .text_color(if row.is_user {
                                        p.accent.to_hsla()
                                    } else {
                                        p.muted_ink.to_hsla()
                                    })
                                    .child(format!(
                                        "#{} {}",
                                        ix + 1,
                                        if row.is_user { "You" } else { "AI" }
                                    )),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(if snippet.trim().is_empty() {
                                        "(empty)".to_string()
                                    } else {
                                        snippet
                                    }),
                            ),
                    );
                    if v_count > 1 {
                        let mut v_row = h_flex().gap(px(4.0)).flex_wrap();
                        for vi in 0..v_count.min(6) {
                            let is_v = vi == v_active;
                            v_row = v_row.child(
                                div()
                                    .id(("mm-v", ix * 16 + vi))
                                    .px(px(5.0))
                                    .py(px(1.0))
                                    .rounded(px(4.0))
                                    .bg(if is_v {
                                        p.accent.with_alpha(0.18)
                                    } else {
                                        p.sheet_bg.to_hsla()
                                    })
                                    .text_size(px(10.0))
                                    .text_color(if is_v {
                                        p.accent.to_hsla()
                                    } else {
                                        p.muted_ink.to_hsla()
                                    })
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.handle_message_action(
                                            MessageAction::SelectVariant {
                                                index: ix,
                                                variant_index: vi,
                                            },
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child(format!("v{}", vi + 1)),
                            );
                        }
                        entry = entry.child(v_row);
                    }
                    map_list = map_list.child(entry);
                }

                let map_panel = v_flex()
                    .id("minimap-panel")
                    .w(px(280.0))
                    .p(px(10.0))
                    .rounded(px(10.0))
                    .bg(p.elevated_bg.to_hsla())
                    .border_1()
                    .border_color(p.border.to_hsla())
                    .shadow_lg()
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(format!("Message map ({total})")),
                            )
                            .child(
                                h_flex()
                                    .gap(px(4.0))
                                    .child(sepia_button(
                                        "mm-prompts-only",
                                        "Prompts",
                                        None,
                                        if self.minimap_prompts_only {
                                            SepiaButtonStyle::Primary
                                        } else {
                                            SepiaButtonStyle::Outline
                                        },
                                        true,
                                        false,
                                        p,
                                        cx.listener(|this, _, _, cx| {
                                            this.minimap_prompts_only = !this.minimap_prompts_only;
                                            cx.notify();
                                        }),
                                    ))
                                    .child(icon_button(
                                        "mm-close",
                                        KissIcon::Close,
                                        false,
                                        p,
                                        cx.listener(|this, _, _, cx| {
                                            this.minimap_open = false;
                                            cx.notify();
                                        }),
                                    )),
                            ),
                    )
                    .child(Input::new(&self.minimap_filter_input))
                    .child(h_divider(p))
                    .child(map_list);

                container = container.child(spring_pop_enter("msg-minimap-anim", map_panel));
            }

            overlays.push(
                container
                    .child(spring_pop_enter("msg-nav-rail-anim", rail))
                    .into_any_element(),
            );
        }

        overlays
    }
}
