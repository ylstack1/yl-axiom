use super::*;

/// Collapsible reasoning & tool-step timeline drawer (`MessageTurnReasoningSection.kt` + `desktop-thought.png`).
pub(super) fn render_reasoning_drawer(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
    has_thinking: bool,
    has_tools: bool,
    is_live_peek_only: bool,
) -> AnyElement {
    let p = ctx.palette;
    let prefs = ctx.prefs;
    let on_action = ctx.on_action;
    let accent = p.accent.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let border_col = if msg.is_streaming {
        p.accent.with_alpha(0.80)
    } else {
        p.border.with_alpha(0.45)
    };

    let words = count_words_fast(msg.thinking_summary.as_ref());
    let tools_len = msg.tool_cards.len();
    let mut header_text = if is_live_peek_only {
        if msg.tool_cards.last().is_some_and(|t| t.is_running) {
            "Latest tool".to_string()
        } else {
            "Latest thought".to_string()
        }
    } else if msg.is_streaming {
        if has_tools {
            "Running tools & thinking".to_string()
        } else {
            "Thinking".to_string()
        }
    } else {
        "Thinking and tool steps".to_string()
    };
    if words > 0 && tools_len > 0 {
        header_text.push_str(&format!(
            " ({words} words • {tools_len} tool{})",
            if tools_len == 1 { "" } else { "s" }
        ));
    } else if words > 0 {
        header_text.push_str(&format!(" ({words} words)"));
    } else if tools_len > 0 {
        header_text.push_str(&format!(
            " ({tools_len} tool{})",
            if tools_len == 1 { "" } else { "s" }
        ));
    }

    let nid_tog = msg.node_id.clone();
    let h_tog = on_action.clone();
    let mut drawer = v_flex()
        .w_full()
        .mb(px(6.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .bg(p.sheet_bg.with_alpha(0.58))
        .border_1()
        .border_color(border_col)
        .gap(px(8.0))
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(if msg.is_streaming {
                            streaming_pulse_dot(("reason-pulse", ix), accent)
                        } else {
                            kiss_icon(KissIcon::Brain, 14.0, accent)
                        })
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_semibold()
                                .text_color(accent)
                                .child(header_text),
                        ),
                )
                .child(icon_button(
                    ("reason-hdr-tog", ix),
                    if is_live_peek_only {
                        KissIcon::ChevronDown
                    } else {
                        KissIcon::ChevronUp
                    },
                    false,
                    p,
                    move |_, w, cx| h_tog(MessageAction::ToggleThinking(nid_tog.clone()), w, cx),
                )),
        );

    if is_live_peek_only {
        // Compact Live Peek showing only the latest running tool or latest thought lines
        if let Some(last_tc) = msg.tool_cards.last().filter(|t| t.is_running) {
            drawer = drawer.child(
                v_flex()
                    .gap(px(4.0))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(kiss_icon(tool_kiss_icon(&last_tc.tool_name), 13.0, accent))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(last_tc.title.clone()),
                            )
                            .child(
                                div()
                                    .px(px(6.0))
                                    .py(px(2.0))
                                    .rounded(px(4.0))
                                    .bg(p.accent.with_alpha(0.16))
                                    .font_family(p.code_font_family.clone())
                                    .text_size(px(10.5))
                                    .font_bold()
                                    .text_color(accent)
                                    .child("Running"),
                            ),
                    )
                    .children((!last_tc.result_preview.is_empty()).then(|| {
                        div()
                            .font_family(p.code_font_family.clone())
                            .text_size(px(11.0))
                            .text_color(narration)
                            .child(clip_preview(last_tc.result_preview.as_ref(), 240))
                    })),
            );
        } else {
            let tail_text = if !msg.trailing_thinking.is_empty() {
                msg.trailing_thinking.as_ref()
            } else {
                msg.thinking_summary.as_ref()
            };
            let peek_tail: String = tail_text
                .lines()
                .rev()
                .filter(|l| !l.trim().is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            drawer = drawer.child(
                div()
                    .text_size(px(12.5))
                    .text_color(narration)
                    .child(clip_preview(&peek_tail, 320)),
            );
        }
        return drawer.into_any_element();
    }

    // Full Interleaved Timeline or Simple Thinking + Tool list
    let has_interleaved = has_tools
        && (msg
            .tool_cards
            .iter()
            .any(|t| !t.preceding_thinking.is_empty() || !t.preceding_text.is_empty())
            || !msg.trailing_thinking.is_empty());

    if !has_interleaved && has_thinking && !msg.thinking_summary.is_empty() {
        drawer = drawer.child(
            div()
                .relative()
                .pl(px(10.0))
                .child(rubric_mark_overlay(2.0, p))
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(gpui::relative(1.48))
                        .text_color(narration)
                        .child(clip_preview(msg.thinking_summary.as_ref(), 2400)),
                ),
        );
    }

    if has_tools {
        let total = msg.tool_cards.len();
        let max_tail = if msg.is_streaming { 12 } else { 24 };
        let hidden = if !ctx.ui_state.show_all_tool_steps && total > max_tail {
            total - max_tail
        } else {
            0
        };
        if hidden > 0 {
            let nid_all = msg.node_id.clone();
            let h_all = on_action.clone();
            drawer = drawer.child(h_flex().child(sepia_button(
                ("show-all-tools", ix),
                format!("Show earlier {hidden} tool steps ({total} total)"),
                None,
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| {
                    h_all(
                        MessageAction::ToggleShowAllToolSteps(nid_all.clone()),
                        w,
                        cx,
                    )
                },
            )));
        }
        let visible = &msg.tool_cards[hidden..];
        let mut cursor = 0usize;
        while cursor < visible.len() {
            let first = &visible[cursor];
            if has_interleaved && has_thinking && !first.preceding_thinking.is_empty() {
                drawer = drawer.child(
                    div()
                        .relative()
                        .pl(px(10.0))
                        .child(rubric_mark_overlay(2.0, p))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(narration)
                                .child(clip_preview(first.preceding_thinking.as_ref(), 1000)),
                        ),
                );
            }
            if has_interleaved && !first.preceding_text.is_empty() {
                drawer = drawer.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(ink)
                        .child(clip_preview(first.preceding_text.as_ref(), 800)),
                );
            }

            // Group consecutive quiet finished lookup tools when nothing sits between them
            let mut end = cursor;
            if is_quiet_lookup_tool(first) {
                while end + 1 < visible.len() {
                    let next = &visible[end + 1];
                    if is_quiet_lookup_tool(next)
                        && next.preceding_text.is_empty()
                        && (!has_thinking || next.preceding_thinking.is_empty())
                    {
                        end += 1;
                    } else {
                        break;
                    }
                }
            }

            if end > cursor {
                let group_slice: Vec<&DisplayToolCard> = visible[cursor..=end].iter().collect();
                let group_key = format!("grp-{}", first.id);
                let is_open = ctx.ui_state.expanded_tool_groups.contains(&group_key);
                let summary = quiet_tool_group_summary(&group_slice);
                let nid_grp = msg.node_id.clone();
                let gk_clone = group_key.clone();
                let h_grp = on_action.clone();
                let hover_bg = p.hover_tint();
                let mut grp_box = v_flex()
                    .w_full()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(p.border.with_alpha(0.6))
                    .bg(p.card_bg.with_alpha(0.65))
                    .child(
                        h_flex()
                            .id(("quiet-tool-grp", ix * 64 + cursor))
                            .w_full()
                            .px(px(10.0))
                            .py(px(6.0))
                            .justify_between()
                            .items_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(move |_, w, cx| {
                                h_grp(
                                    MessageAction::ToggleQuietToolGroup {
                                        node_id: nid_grp.clone(),
                                        group_key: gk_clone.clone(),
                                    },
                                    w,
                                    cx,
                                );
                            })
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(kiss_icon(KissIcon::Search, 12.0, muted))
                                    .child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(narration)
                                            .child(summary),
                                    ),
                            )
                            .child(kiss_icon(
                                if is_open {
                                    KissIcon::ChevronUp
                                } else {
                                    KissIcon::ChevronDown
                                },
                                12.0,
                                muted,
                            )),
                    );
                if is_open {
                    let mut inner = v_flex().p(px(6.0)).gap(px(5.0));
                    for (sub_i, tc) in group_slice.iter().enumerate() {
                        inner = inner.child(render_single_tool_card(
                            ix * 128 + cursor + sub_i,
                            tc,
                            prefs,
                            p,
                        ));
                    }
                    grp_box = grp_box.child(h_divider(p)).child(inner);
                }
                drawer = drawer.child(grp_box);
            } else {
                drawer = drawer.child(render_single_tool_card(ix * 128 + cursor, first, prefs, p));
            }
            cursor = end + 1;
        }
    }

    if has_interleaved && has_thinking && !msg.trailing_thinking.is_empty() {
        drawer = drawer.child(
            div()
                .relative()
                .pl(px(10.0))
                .child(rubric_mark_overlay(2.0, p))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(narration)
                        .child(clip_preview(msg.trailing_thinking.as_ref(), 1000)),
                ),
        );
    }

    if !msg.thinking_signature.is_empty() {
        drawer = drawer.child(
            h_flex()
                .items_center()
                .gap(px(5.0))
                .child(kiss_icon(KissIcon::ShieldCheck, 12.0, accent))
                .child(
                    div()
                        .font_family(p.code_font_family.clone())
                        .text_size(px(11.0))
                        .text_color(muted)
                        .child("Thinking saved for the next turn"),
                ),
        );
    }

    drawer.into_any_element()
}

pub(super) fn render_single_tool_card(
    uid: usize,
    tc: &DisplayToolCard,
    prefs: &PreferencesConfig,
    p: &ParchmentPalette,
) -> AnyElement {
    let status_col = if tc.is_error {
        p.danger.to_hsla()
    } else if tc.is_running {
        p.accent.to_hsla()
    } else {
        p.success.to_hsla()
    };
    let mut tc_box = v_flex()
        .id(("tool-card", uid))
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(p.card_bg.to_hsla())
        .border_1()
        .border_color(p.subtle_border())
        .gap(px(4.0))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .flex_1()
                        .overflow_hidden()
                        .items_center()
                        .gap(px(6.0))
                        .child(kiss_icon(
                            tool_kiss_icon(&tc.tool_name),
                            13.0,
                            p.accent.to_hsla(),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .font_family(p.code_font_family.clone())
                                .text_size(px(12.0))
                                .font_semibold()
                                .text_color(p.primary_ink.to_hsla())
                                .child(tc.title.clone()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(status_col)
                        .child(tc.status_meta.clone()),
                ),
        );
    if !tc.diff_lines.is_empty() {
        let mut diff_col = v_flex()
            .w_full()
            .p(px(6.0))
            .rounded(px(6.0))
            .bg(p.elevated_bg.to_hsla())
            .font_family(p.code_font_family.clone())
            .text_size(px(11.5))
            .gap(px(1.0));
        for (kind, line) in &tc.diff_lines {
            let (line_col, line_bg) = match kind {
                DiffLineKind::Added => (p.success.to_hsla(), p.success.with_alpha(0.10)),
                DiffLineKind::Removed => (p.danger.to_hsla(), p.danger.with_alpha(0.10)),
                DiffLineKind::Context | DiffLineKind::Collapsed => {
                    (p.muted_ink.to_hsla(), p.canvas_bg.with_alpha(0.0))
                }
            };
            diff_col = diff_col.child(
                div()
                    .w_full()
                    .px(px(4.0))
                    .bg(line_bg)
                    .text_color(line_col)
                    .child(line.clone()),
            );
        }
        tc_box = tc_box.child(diff_col);
    } else if prefs.show_tool_result_summary && !tc.result_preview.is_empty() {
        tc_box = tc_box.child(
            div()
                .font_family(p.code_font_family.clone())
                .text_size(px(11.5))
                .text_color(p.muted_ink.to_hsla())
                .child(tc.result_preview.clone()),
        );
    }
    tc_box.into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_turn_header(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
    has_thinking: bool,
    has_tools: bool,
    thinking_expanded: bool,
    narration: gpui::Hsla,
    muted: gpui::Hsla,
    accent2: gpui::Hsla,
    danger: gpui::Hsla,
) -> Option<AnyElement> {
    let prefs = ctx.prefs;
    let p = ctx.palette;
    let show_header = !msg.is_user
        || prefs.show_user_avatar
        || prefs.show_user_name
        || prefs.show_user_timestamp
        || msg.variant_count > 1
        || !msg.included_in_context;
    if !show_header {
        return None;
    }

    let speaker = if msg.is_user {
        if prefs.show_user_name {
            if ctx.user_name.trim().is_empty() {
                "You".to_string()
            } else {
                ctx.user_name.to_string()
            }
        } else {
            String::new()
        }
    } else if prefs.show_model_name {
        let model_str = msg.model_used.as_ref();
        let named_char = if ctx.assistant_name != strings::DEFAULT_ASSISTANT
            && !ctx.assistant_name.trim().is_empty()
        {
            ctx.assistant_name
        } else {
            ""
        };
        if !named_char.is_empty() {
            if !model_str.is_empty() {
                format!("{named_char} · {model_str}")
            } else {
                named_char.to_string()
            }
        } else if !model_str.is_empty() {
            if prefs.show_provider_after_model_name && !ctx.provider_name.is_empty() {
                format!("{model_str} ({})", ctx.provider_name)
            } else {
                model_str.to_string()
            }
        } else {
            "Assistant".to_string()
        }
    } else {
        String::new()
    };

    let show_ts =
        (!msg.is_user && prefs.show_model_timestamp) || (msg.is_user && prefs.show_user_timestamp);
    let ts_str = if show_ts {
        format_compact_timestamp(msg.created_at)
    } else {
        String::new()
    };

    let mut left = h_flex().items_center().gap(px(6.0));
    if msg.is_user && prefs.show_user_avatar {
        let pic_exists =
            !ctx.user_picture_path.is_empty() && PathBuf::from(ctx.user_picture_path).exists();
        if pic_exists {
            left = left.child(
                img(PathBuf::from(ctx.user_picture_path))
                    .w(px(20.0))
                    .h(px(20.0))
                    .rounded(px(999.0)),
            );
        } else {
            left = left.child(sepia_avatar_circle("👤", false, 20.0, p));
        }
    } else if !msg.is_user && prefs.show_model_icon {
        let hint = if !msg.model_used.is_empty() {
            msg.model_used.as_ref()
        } else {
            ctx.provider_name
        };
        left = left.child(model_lab_icon(
            hint,
            14.0,
            muted,
            msg.provider_protocol.map(|p| p.as_db_str()).unwrap_or(""),
        ));
    }
    if !speaker.is_empty() {
        left = left.child(
            div()
                .text_size(px(12.0))
                .font_medium()
                .text_color(narration)
                .child(speaker),
        );
    }
    if !ts_str.is_empty() {
        left = left.child(div().text_size(px(11.0)).text_color(muted).child(ts_str));
    }
    if !msg.included_in_context {
        left = left.child(
            div()
                .text_size(px(11.0))
                .font_semibold()
                .text_color(danger)
                .child(strings::LEFT_OUT_OF_CONTEXT),
        );
    }

    let mut right = h_flex().items_center().gap(px(4.0));
    if msg.variant_count > 1 {
        let h_prev = ctx.on_action.clone();
        let h_next = ctx.on_action.clone();
        let cur = msg.variant_index;
        let total = msg.variant_count;
        right = right
            .child(sepia_button(
                ("var-prev", ix),
                "",
                Some(KissIcon::ArrowLeft),
                SepiaButtonStyle::Ghost,
                true,
                cur == 0,
                p,
                move |_, w, cx| {
                    h_prev(
                        MessageAction::SelectVariant {
                            index: ix,
                            variant_index: cur.saturating_sub(1),
                        },
                        w,
                        cx,
                    );
                },
            ))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_semibold()
                    .text_color(accent2)
                    .child(format!("{}/{total}", cur + 1)),
            )
            .child(sepia_button(
                ("var-next", ix),
                "",
                Some(KissIcon::ArrowRight),
                SepiaButtonStyle::Ghost,
                true,
                cur + 1 >= total,
                p,
                move |_, w, cx| {
                    h_next(
                        MessageAction::SelectVariant {
                            index: ix,
                            variant_index: cur + 1,
                        },
                        w,
                        cx,
                    );
                },
            ));
    }

    if !msg.is_user && (has_thinking || has_tools) {
        let mut lbl = if msg.is_streaming {
            "Thinking…".to_string()
        } else {
            "Thought".to_string()
        };
        if msg.thinking_duration_ms >= 1000 {
            lbl.push_str(&format!(" {}s", msg.thinking_duration_ms / 1000));
        }
        if has_tools {
            let n = msg.tool_cards.len();
            lbl.push_str(&format!(", {n} tool{}", if n == 1 { "" } else { "s" }));
        }
        let nid = msg.node_id.clone();
        let handler = ctx.on_action.clone();
        right = right.child(sepia_button(
            ("toggle-think", ix),
            lbl,
            Some(if thinking_expanded {
                KissIcon::ChevronDown
            } else {
                KissIcon::ChevronRight
            }),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| handler(MessageAction::ToggleThinking(nid.clone()), w, cx),
        ));
    }

    Some(
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap(px(8.0))
            .child(left)
            .child(right)
            .into_any_element(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_turn_footer(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
    is_sent_bubble: bool,
    turn_group: &SharedString,
    muted: gpui::Hsla,
    accent: gpui::Hsla,
) -> Option<AnyElement> {
    let prefs = ctx.prefs;
    let p = ctx.palette;
    if msg.is_user && !prefs.show_user_message_actions {
        return None;
    }
    if ctx.is_editing {
        return None;
    }

    let on_action = ctx.on_action;
    let mut actions = h_flex().flex_wrap().items_center().gap(px(3.0));
    if is_sent_bubble {
        actions = actions.justify_end();
    }

    // 1. Copy plain text
    {
        let h = on_action.clone();
        let copied = ctx.ui_state.copied_kind == Some("plain");
        actions = actions.child(icon_button(
            ("act-copy", ix),
            if copied {
                KissIcon::Check
            } else {
                KissIcon::Copy
            },
            copied,
            p,
            move |_, w, cx| h(MessageAction::CopyPlain(ix), w, cx),
        ));
    }

    // 2. Read aloud (`PlatformVoice.speakTextAloud` parity)
    if !msg.content.trim().is_empty() {
        let h = on_action.clone();
        actions = actions.child(icon_button(
            ("act-speak", ix),
            if ctx.is_speaking_this_turn {
                KissIcon::VolumeOff
            } else {
                KissIcon::VolumeOn
            },
            ctx.is_speaking_this_turn,
            p,
            move |_, w, cx| h(MessageAction::ToggleSpeak(ix), w, cx),
        ));
    }

    // 3. Regenerate
    {
        let h = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-regen", ix),
            "",
            Some(KissIcon::Refresh),
            SepiaButtonStyle::Ghost,
            true,
            msg.is_streaming,
            p,
            move |_, w, cx| h(MessageAction::Regenerate(ix), w, cx),
        ));
    }

    // 4. Pin / Unpin (assistant turns)
    if !msg.is_user {
        let h = on_action.clone();
        actions = actions.child(icon_button(
            ("act-pin", ix),
            KissIcon::Pin,
            msg.pinned,
            p,
            move |_, w, cx| h(MessageAction::TogglePin(ix), w, cx),
        ));
    }

    // 5. More actions toggle
    {
        let h = on_action.clone();
        let nid = msg.node_id.clone();
        actions = actions.child(icon_button(
            ("act-more", ix),
            if ctx.ui_state.show_more_actions {
                KissIcon::ChevronUp
            } else {
                KissIcon::MoreHorizontal
            },
            ctx.ui_state.show_more_actions,
            p,
            move |_, w, cx| h(MessageAction::ToggleMoreActions(nid.clone()), w, cx),
        ));
    }

    // 6. Web sources pill (when citations present)
    if !msg.is_user && !msg.web_sources.is_empty() {
        let h = on_action.clone();
        let nid = msg.node_id.clone();
        let n = msg.web_sources.len();
        let label = if n == 1 {
            "1 source".to_string()
        } else {
            format!("{n} sources")
        };
        actions = actions.child(sepia_chip(
            ("act-sources", ix),
            label,
            Some(KissIcon::Globe),
            ctx.ui_state.show_sources_drawer,
            p,
            move |_, w, cx| h(MessageAction::ToggleSources(nid.clone()), w, cx),
        ));
    }

    // Expanded secondary actions
    if ctx.ui_state.show_more_actions {
        let h_md = on_action.clone();
        let copied_md = ctx.ui_state.copied_kind == Some("md");
        actions = actions.child(sepia_button(
            ("act-copymd", ix),
            if copied_md {
                strings::COPIED
            } else {
                strings::COPY_MARKDOWN
            },
            Some(if copied_md {
                KissIcon::Check
            } else {
                KissIcon::Markdown
            }),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| h_md(MessageAction::CopyMarkdown(ix), w, cx),
        ));

        if !msg.is_user {
            let h_cont = on_action.clone();
            actions = actions.child(sepia_button(
                ("act-continue", ix),
                strings::CONTINUE,
                Some(KissIcon::Play),
                SepiaButtonStyle::Ghost,
                true,
                msg.is_streaming,
                p,
                move |_, w, cx| h_cont(MessageAction::Continue(ix), w, cx),
            ));
        }

        let h_edit = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-edit", ix),
            strings::EDIT,
            Some(KissIcon::Edit),
            SepiaButtonStyle::Ghost,
            true,
            msg.is_streaming,
            p,
            move |_, w, cx| h_edit(MessageAction::StartEdit(ix), w, cx),
        ));

        if !msg.content.is_empty() {
            let h_tr = on_action.clone();
            actions = actions.child(sepia_button(
                ("act-translate", ix),
                strings::TRANSLATE,
                Some(KissIcon::Translate),
                SepiaButtonStyle::Ghost,
                true,
                msg.is_streaming,
                p,
                move |_, w, cx| h_tr(MessageAction::Translate(ix), w, cx),
            ));
            if !msg.is_user {
                let h_skill = on_action.clone();
                actions = actions.child(sepia_button(
                    ("act-skill", ix),
                    "Save as skill",
                    Some(KissIcon::Sparkles),
                    SepiaButtonStyle::Ghost,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_skill(MessageAction::SaveAsSkill(ix), w, cx),
                ));
            }
        }

        let h_branch = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-branch", ix),
            strings::BRANCH,
            Some(KissIcon::GitBranch),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| h_branch(MessageAction::Branch(ix), w, cx),
        ));

        let h_rewind = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-rewind", ix),
            strings::REWIND,
            Some(KissIcon::Rewind),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| h_rewind(MessageAction::Rewind(ix), w, cx),
        ));

        let h_mute = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-mute", ix),
            if msg.included_in_context {
                strings::EXCLUDE_CONTEXT
            } else {
                strings::INCLUDE_CONTEXT
            },
            Some(if msg.included_in_context {
                KissIcon::EyeOff
            } else {
                KissIcon::Eye
            }),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| h_mute(MessageAction::ToggleContextMute(ix), w, cx),
        ));

        if !msg.raw_request_preview.is_empty() || !msg.raw_response_preview.is_empty() {
            let h_raw = on_action.clone();
            let nid = msg.node_id.clone();
            actions = actions.child(sepia_button(
                ("act-raw", ix),
                strings::RAW_WIRE,
                Some(KissIcon::Code),
                SepiaButtonStyle::Ghost,
                true,
                false,
                p,
                move |_, w, cx| h_raw(MessageAction::ToggleRawWire(nid.clone()), w, cx),
            ));
        }

        if msg.variant_count > 1 {
            let h_dv = on_action.clone();
            actions = actions.child(sepia_button(
                ("act-delvar", ix),
                format!("Delete {}/{}", msg.variant_index + 1, msg.variant_count),
                Some(KissIcon::Trash),
                SepiaButtonStyle::Ghost,
                true,
                false,
                p,
                move |_, w, cx| h_dv(MessageAction::DeleteCurrentVariant(ix), w, cx),
            ));
        }

        let h_del = on_action.clone();
        actions = actions.child(sepia_button(
            ("act-del", ix),
            strings::DELETE,
            Some(KissIcon::Trash),
            SepiaButtonStyle::Ghost,
            true,
            false,
            p,
            move |_, w, cx| h_del(MessageAction::DeleteNode(ix), w, cx),
        ));
    }

    // Token & Cost Stats Row + Clickable Usage Details Popover
    let stats_el = render_usage_stats_row(ix, msg, ctx, is_sent_bubble, muted, accent);

    let always_visible = msg.is_streaming
        || ctx.ui_state.show_more_actions
        || ctx.ui_state.show_usage_details
        || ctx.is_speaking_this_turn;

    let mut wrap = v_flex()
        .w_full()
        .gap(px(3.0))
        .child(actions)
        .children(stats_el);
    if is_sent_bubble {
        wrap = wrap.items_end();
    }
    if !always_visible {
        wrap = wrap
            .opacity(0.0)
            .group_hover(turn_group.clone(), |s| s.opacity(1.0));
    }
    Some(wrap.into_any_element())
}

pub(super) fn render_usage_stats_row(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
    is_sent_bubble: bool,
    muted: gpui::Hsla,
    accent: gpui::Hsla,
) -> Option<AnyElement> {
    let prefs = ctx.prefs;
    let p = ctx.palette;
    if !prefs.show_token_and_context_stats {
        return None;
    }

    if msg.is_user {
        if msg.estimated_prose_tokens <= 0 {
            return None;
        }
        return Some(
            div()
                .text_size(px(11.0))
                .text_color(muted)
                .child(format!(
                    "~{} tokens",
                    format_tokens_short(msg.estimated_prose_tokens)
                ))
                .into_any_element(),
        );
    }

    let m = &msg.token_metrics;
    let out_tok = if m.output_tokens > 0 {
        m.output_tokens
    } else {
        msg.estimated_prose_tokens
    };
    if out_tok <= 0 && m.input_tokens <= 0 && msg.cost_usd <= 0.0 && m.duration_ms <= 0 {
        return None;
    }

    let mut summary_parts = Vec::new();
    if out_tok > 0 {
        summary_parts.push(format!("{} tokens", format_tokens_short(out_tok)));
    }
    if m.reasoning_tokens > 0 {
        summary_parts.push(format!(
            "{} thinking",
            format_tokens_short(m.reasoning_tokens)
        ));
    }
    if m.tokens_per_sec > 0.0 {
        summary_parts.push(format!("{:.1} tok/s", m.tokens_per_sec));
    }
    if m.duration_ms >= 1000 {
        summary_parts.push(format!("{:.1}s", m.duration_ms as f64 / 1000.0));
    }
    let cost_str = format_usd_short(msg.cost_usd);
    if !cost_str.is_empty() {
        summary_parts.push(cost_str);
    }
    if summary_parts.is_empty() {
        return None;
    }

    let nid = msg.node_id.clone();
    let h_usage = ctx.on_action.clone();
    let hover_bg = p.hover_tint();

    let mut container = v_flex().gap(px(4.0));
    if is_sent_bubble {
        container = container.items_end();
    }

    container = container.child(
        h_flex()
            .id(("usage-pill", ix))
            .px(px(6.0))
            .py(px(2.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .items_center()
            .gap(px(5.0))
            .on_click(move |_, w, cx| {
                h_usage(MessageAction::ToggleUsageDetails(nid.clone()), w, cx)
            })
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child(summary_parts.join(" · ")),
            )
            .child(kiss_icon(
                if ctx.ui_state.show_usage_details {
                    KissIcon::ChevronUp
                } else {
                    KissIcon::ChevronDown
                },
                10.0,
                muted,
            )),
    );

    if ctx.ui_state.show_usage_details {
        let mut lines = Vec::new();
        if m.input_tokens > 0 || m.output_tokens > 0 {
            lines.push(format!(
                "Input: {} · Output: {}",
                format_tokens_short(m.input_tokens),
                format_tokens_short(m.output_tokens)
            ));
        }
        if m.cached_input_tokens > 0 || m.cache_creation_input_tokens > 0 {
            lines.push(format!(
                "Reused: {} · Saved for reuse: {}",
                format_tokens_short(m.cached_input_tokens),
                format_tokens_short(m.cache_creation_input_tokens)
            ));
        }
        if m.ttft_ms > 0 {
            lines.push(format!("First token: {}ms", m.ttft_ms));
        }
        if m.reported_requests > 0 {
            let status = if !m.usage_is_complete || !m.cost_is_available {
                " (partial)"
            } else if m.reported_cost_requests == m.reported_requests {
                " (reported)"
            } else {
                " (estimated)"
            };
            lines.push(format!(
                "{} request{}{status}",
                m.reported_requests,
                if m.reported_requests == 1 { "" } else { "s" }
            ));
        }
        if m.output_includes_unreported_reasoning {
            lines.push("Thinking tokens counted inside output".into());
        }
        if !m.unpriced_cost_components.is_empty() {
            let unp: Vec<&str> = m
                .unpriced_cost_components
                .iter()
                .map(|s| s.as_str())
                .collect();
            lines.push(format!("Unpriced: {}", unp.join(", ")));
        }
        if lines.is_empty() {
            lines.push(format!(
                "Estimated prose: ~{} tokens",
                format_tokens_short(msg.estimated_prose_tokens)
            ));
        }

        let mut card = v_flex()
            .p(px(8.0))
            .rounded(px(8.0))
            .bg(p.elevated_bg.to_hsla())
            .border_1()
            .border_color(p.subtle_border())
            .gap(px(2.0))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_semibold()
                    .text_color(accent)
                    .child("Turn usage details"),
            );
        for line in lines {
            card = card.child(
                div()
                    .font_family(p.code_font_family.clone())
                    .text_size(px(11.0))
                    .text_color(p.narration_ink.to_hsla())
                    .child(line),
            );
        }
        container = container.child(card);
    }

    Some(container.into_any_element())
}
