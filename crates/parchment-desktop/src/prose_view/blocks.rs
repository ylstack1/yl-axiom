use super::*;

pub(super) fn render_turn_body(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
) -> AnyElement {
    let p = ctx.palette;
    let prefs = ctx.prefs;
    let on_action = ctx.on_action;
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let elev = p.elevated_bg.to_hsla();
    let border = p.subtle_border();

    let enable_md = if msg.is_user {
        prefs.render_user_messages_with_markdown
    } else {
        prefs.render_assistant_messages_with_markdown
    };

    let should_collapse_long = !msg.is_streaming
        && prefs.collapse_long_messages
        && msg.content.len() > COLLAPSE_LONG_MESSAGE_CHARS
        && !ctx.ui_state.expanded_long_message;

    let body_content: AnyElement = if msg.content.is_empty() && msg.is_streaming {
        div()
            .text_size(px(13.0))
            .text_color(muted)
            .child("Thinking…")
            .into_any_element()
    } else if should_collapse_long {
        let preview = clip_preview(msg.content.as_ref(), 700);
        let nid = msg.node_id.clone();
        let h_exp = on_action.clone();
        v_flex()
            .w_full()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(p.prose_font_size_px))
                    .line_height(gpui::relative(p.line_height_mult))
                    .text_color(ink)
                    .child(preview),
            )
            .child(h_flex().child(sepia_button(
                ("expand-long", ix),
                format!("Show full message ({} chars)", msg.content.chars().count()),
                Some(KissIcon::ChevronDown),
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_exp(MessageAction::ToggleExpandLong(nid.clone()), w, cx),
            )))
            .into_any_element()
    } else if enable_md && !msg.prose_blocks.is_empty() {
        let mut stack = v_flex().w_full().gap(px(8.0));
        for (b_idx, block) in msg.prose_blocks.iter().enumerate() {
            match block {
                DisplayProseBlock::Markdown(md) => {
                    stack = stack.child(
                        div()
                            .w_full()
                            .line_height(gpui::relative(p.line_height_mult))
                            .child(
                                TextView::markdown(("msg-md", ix * 128 + b_idx), md.clone())
                                    .selectable(true),
                            ),
                    );
                }
                DisplayProseBlock::CodeFence {
                    language,
                    code,
                    line_count,
                    highlighted_lines,
                } => {
                    stack = stack.child(render_warm_code_card(
                        ix,
                        b_idx,
                        &msg.node_id,
                        language,
                        code,
                        *line_count,
                        highlighted_lines,
                        ctx,
                    ));
                }
                DisplayProseBlock::Latex(rendered) if prefs.math_formula_rendering => {
                    stack = stack.child(
                        div()
                            .id(("math-blk", ix * 128 + b_idx))
                            .px(px(12.0))
                            .py(px(8.0))
                            .rounded(px(8.0))
                            .bg(elev)
                            .border_1()
                            .border_color(border)
                            .text_size(px(p.prose_font_size_px))
                            .text_color(ink)
                            .child(rendered.clone()),
                    );
                }
                DisplayProseBlock::Svg {
                    raw_bytes,
                    raw_svg,
                    width,
                    height,
                } => {
                    let h_svg = on_action.clone();
                    let svg_src = raw_svg.to_string();
                    stack = stack.child(
                        div()
                            .id(("svg-blk", ix * 128 + b_idx))
                            .p(px(10.0))
                            .rounded(px(8.0))
                            .bg(elev)
                            .border_1()
                            .border_color(border)
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(move |_, w, cx| {
                                h_svg(
                                    MessageAction::InspectSvg {
                                        title: format!("diagram-{}.svg", b_idx + 1),
                                        raw_svg: svg_src.clone(),
                                    },
                                    w,
                                    cx,
                                );
                            })
                            .child(
                                Icon::default()
                                    .data(raw_bytes)
                                    .w(px(*width))
                                    .h(px(*height))
                                    .text_color(ink),
                            ),
                    );
                }
                DisplayProseBlock::Latex(_) => {}
            }
        }
        if prefs.collapse_long_messages && msg.content.len() > COLLAPSE_LONG_MESSAGE_CHARS {
            let nid = msg.node_id.clone();
            let h_col = on_action.clone();
            stack = stack.child(h_flex().child(sepia_button(
                ("collapse-long", ix),
                "Collapse message",
                Some(KissIcon::ChevronUp),
                SepiaButtonStyle::Ghost,
                true,
                false,
                p,
                move |_, w, cx| h_col(MessageAction::ToggleExpandLong(nid.clone()), w, cx),
            )));
        }
        stack.into_any_element()
    } else if ctx.highlight_dialogue_and_narration && !msg.is_user {
        render_dialogue_narration_text(msg.content.as_ref(), p)
    } else {
        div()
            .text_size(px(p.prose_font_size_px))
            .line_height(gpui::relative(p.line_height_mult))
            .text_color(ink)
            .child(msg.content.clone())
            .into_any_element()
    };

    let media_strip = render_generated_media_section(ix, &msg.media_items, p, on_action);
    let attachment_strip = if prefs.show_files_below_replies && !msg.attachments.is_empty() {
        Some(render_attachments_section(
            ix,
            &msg.attachments,
            p,
            on_action,
        ))
    } else {
        None
    };

    v_flex()
        .w_full()
        .gap(px(6.0))
        .text_size(px(p.prose_font_size_px))
        .text_color(ink)
        .child(body_content)
        .children(media_strip)
        .children(attachment_strip)
        .into_any_element()
}

/// Renders dialogue in `palette.dialogue_ink` and narration in `palette.narration_ink` for plain literary prose.
pub(super) fn render_dialogue_narration_text(text: &str, p: &ParchmentPalette) -> AnyElement {
    let mut col = v_flex()
        .w_full()
        .gap(px(6.0))
        .text_size(px(p.prose_font_size_px))
        .line_height(gpui::relative(p.line_height_mult));

    for para in text.split("\n\n") {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_dialogue = trimmed.starts_with('"')
            || trimmed.starts_with('“')
            || trimmed.contains(" \"")
            || trimmed.contains(" “");
        let color = if is_dialogue {
            p.dialogue_ink.to_hsla()
        } else {
            p.narration_ink.to_hsla()
        };
        col = col.child(div().text_color(color).child(trimmed.to_string()));
    }
    col.into_any_element()
}

/// Parchment warm code card matching `desktop-chat.png`:
/// `sheetBg` header with `</> lang (N lines)` + Wrap / Collapse / Download / Copy, hairline divider,
/// and warm syntax-highlighted monospace body.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_warm_code_card(
    row_ix: usize,
    b_idx: usize,
    node_id: &str,
    language: &SharedString,
    code: &SharedString,
    line_count: usize,
    highlighted_lines: &[Vec<(TokenKind, SharedString)>],
    ctx: &TurnRenderContext<'_>,
) -> AnyElement {
    let p = ctx.palette;
    let prefs = ctx.prefs;
    let on_action = ctx.on_action;

    let is_collapsed =
        prefs.auto_collapse_code_blocks ^ ctx.ui_state.toggled_code_collapse.contains(&b_idx);
    let is_wrapped = prefs.wrap_code_blocks ^ ctx.ui_state.toggled_code_wrap.contains(&b_idx);
    let is_copied = ctx.ui_state.copied_code_block == Some(b_idx);

    let lang_label = if language.trim().is_empty() {
        "code".to_string()
    } else {
        language.trim().to_string()
    };
    let header_title = format!(
        "{lang_label} ({line_count} line{})",
        if line_count == 1 { "" } else { "s" }
    );

    let nid_wrap = node_id.to_string();
    let nid_col = node_id.to_string();
    let nid_copy = node_id.to_string();
    let code_copy = code.to_string();
    let code_save = code.to_string();
    let lang_save = lang_label.clone();

    let h_wrap = on_action.clone();
    let h_col = on_action.clone();
    let h_save = on_action.clone();
    let h_copy = on_action.clone();
    let uid = row_ix * 256 + b_idx;

    let header = h_flex()
        .w_full()
        .px(px(10.0))
        .py(px(4.0))
        .bg(p.sheet_bg.to_hsla())
        .justify_between()
        .items_center()
        .gap(px(8.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(kiss_icon(KissIcon::Code, 13.0, p.accent.to_hsla()))
                .child(
                    div()
                        .font_family(p.code_font_family.clone())
                        .text_size(px(11.5))
                        .font_medium()
                        .text_color(p.narration_ink.to_hsla())
                        .child(header_title),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(2.0))
                .child(icon_button(
                    ("code-wrap", uid),
                    KissIcon::WrapText,
                    is_wrapped,
                    p,
                    move |_, w, cx| {
                        h_wrap(
                            MessageAction::ToggleCodeWrap {
                                node_id: nid_wrap.clone(),
                                block_index: b_idx,
                            },
                            w,
                            cx,
                        );
                    },
                ))
                .child(icon_button(
                    ("code-collapse", uid),
                    if is_collapsed {
                        KissIcon::ChevronDown
                    } else {
                        KissIcon::ChevronUp
                    },
                    is_collapsed,
                    p,
                    move |_, w, cx| {
                        h_col(
                            MessageAction::ToggleCodeCollapse {
                                node_id: nid_col.clone(),
                                block_index: b_idx,
                            },
                            w,
                            cx,
                        );
                    },
                ))
                .child(icon_button(
                    ("code-save", uid),
                    KissIcon::Download,
                    false,
                    p,
                    move |_, w, cx| {
                        h_save(
                            MessageAction::SaveCodeBlock {
                                language: lang_save.clone(),
                                code: code_save.clone(),
                            },
                            w,
                            cx,
                        );
                    },
                ))
                .child(icon_button(
                    ("code-copy", uid),
                    if is_copied {
                        KissIcon::Check
                    } else {
                        KissIcon::Copy
                    },
                    is_copied,
                    p,
                    move |_, w, cx| {
                        h_copy(
                            MessageAction::CopyText {
                                node_id: nid_copy.clone(),
                                code_block_index: Some(b_idx),
                                text: code_copy.clone(),
                            },
                            w,
                            cx,
                        );
                    },
                )),
        );

    let mut card = v_flex()
        .id(("code-card", uid))
        .w_full()
        .rounded(px(8.0))
        .overflow_hidden()
        .bg(p.card_bg.to_hsla())
        .border_1()
        .border_color(p.subtle_border())
        .child(header);

    if !is_collapsed {
        let mut body = v_flex()
            .w_full()
            .px(px(12.0))
            .py(px(10.0))
            .bg(p.elevated_bg.with_alpha(0.35))
            .font_family(p.code_font_family.clone())
            .text_size(px(p.code_font_size_px.clamp(11.0, 22.0)))
            .line_height(px((p.code_font_size_px * 1.45).clamp(16.0, 32.0)))
            .gap(px(1.0));

        for line_tokens in highlighted_lines {
            if line_tokens.is_empty() {
                body = body.child(div().h(px(14.0)));
                continue;
            }
            let mut line_row = h_flex().w_full();
            if is_wrapped {
                line_row = line_row.flex_wrap();
            }
            for (kind, tok) in line_tokens {
                let col = syntax_token_color(*kind, p);
                let mut span = div().text_color(col).child(tok.clone());
                if *kind == TokenKind::Keyword {
                    span = span.font_semibold();
                } else if *kind == TokenKind::Comment {
                    span = span.italic();
                }
                line_row = line_row.child(span);
            }
            body = body.child(line_row);
        }

        card = card.child(h_divider(p)).child(body);
    }

    card.into_any_element()
}

fn syntax_token_color(kind: TokenKind, p: &ParchmentPalette) -> gpui::Hsla {
    match kind {
        TokenKind::Keyword => p.accent.to_hsla(),
        TokenKind::TypeName | TokenKind::Number => p.accent_secondary.to_hsla(),
        TokenKind::String => p.success.to_hsla(),
        TokenKind::Comment => p.muted_ink.to_hsla(),
        TokenKind::Function => p.primary_ink.to_hsla(),
        TokenKind::Operator | TokenKind::Punctuation => p.narration_ink.to_hsla(),
        TokenKind::Plain => p.primary_ink.to_hsla(),
    }
}

pub(super) fn render_attachments_section(
    ix: usize,
    attachments: &[PromptAttachment],
    p: &ParchmentPalette,
    on_action: &MessageActionHandler,
) -> AnyElement {
    let mut images = Vec::new();
    let mut others = Vec::new();
    for att in attachments {
        let m = att.mime_type.to_ascii_lowercase();
        let l = att.file_name.to_ascii_lowercase();
        if m.starts_with("image/")
            || l.ends_with(".png")
            || l.ends_with(".jpg")
            || l.ends_with(".jpeg")
            || l.ends_with(".webp")
            || l.ends_with(".gif")
        {
            images.push(att);
        } else {
            others.push(att);
        }
    }

    let mut stack = v_flex().w_full().mt(px(4.0)).gap(px(8.0));

    if !images.is_empty() {
        let mut img_row = h_flex().w_full().flex_wrap().gap(px(10.0));
        for (a_ix, att) in images.into_iter().enumerate() {
            let att_clone = att.clone();
            let h_open = on_action.clone();
            let local_exists =
                !att.local_file_path.is_empty() && PathBuf::from(&att.local_file_path).exists();
            let preview: AnyElement = if local_exists {
                img(PathBuf::from(&att.local_file_path))
                    .max_h(px(240.0))
                    .max_w(px(340.0))
                    .rounded(px(6.0))
                    .into_any_element()
            } else {
                h_flex()
                    .w(px(220.0))
                    .h(px(120.0))
                    .rounded(px(6.0))
                    .bg(p.sheet_bg.with_alpha(0.6))
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(kiss_icon(KissIcon::Image, 16.0, p.accent.to_hsla()))
                    .child(
                        div()
                            .max_w(px(160.0))
                            .truncate()
                            .text_size(px(12.0))
                            .font_semibold()
                            .text_color(p.accent.to_hsla())
                            .child(att.file_name.clone()),
                    )
                    .into_any_element()
            };

            img_row = img_row.child(
                v_flex()
                    .id(("turn-img", ix * 32 + a_ix))
                    .max_w(px(360.0))
                    .p(px(8.0))
                    .rounded(px(10.0))
                    .bg(p.card_bg.to_hsla())
                    .border_1()
                    .border_color(p.subtle_border())
                    .gap(px(6.0))
                    .cursor_pointer()
                    .on_click(move |_, w, cx| {
                        h_open(MessageAction::InspectAttachment(att_clone.clone()), w, cx);
                    })
                    .child(preview)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(11.5))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(att.file_name.clone()),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .child(kiss_icon(KissIcon::Search, 11.0, p.accent.to_hsla()))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_semibold()
                                            .text_color(p.accent.to_hsla())
                                            .child("Zoom"),
                                    ),
                            ),
                    ),
            );
        }
        stack = stack.child(img_row);
    }

    if !others.is_empty() {
        let mut chip_row = h_flex().w_full().flex_wrap().gap(px(6.0));
        for (a_ix, att) in others.into_iter().enumerate() {
            let att_clone = att.clone();
            let h_open = on_action.clone();
            let lower = att.file_name.to_ascii_lowercase();
            let icon = if att.mime_type.starts_with("video/") || lower.ends_with(".mp4") {
                KissIcon::Video
            } else if att.mime_type.starts_with("audio/")
                || lower.ends_with(".mp3")
                || lower.ends_with(".wav")
            {
                KissIcon::Audio
            } else if lower.ends_with(".md")
                || lower.ends_with(".txt")
                || lower.ends_with(".rs")
                || lower.ends_with(".kt")
            {
                KissIcon::FileText
            } else {
                KissIcon::Paperclip
            };
            let hover_bg = p.hover_tint();
            chip_row = chip_row.child(
                h_flex()
                    .id(("turn-att", ix * 32 + a_ix))
                    .px(px(8.0))
                    .py(px(5.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.to_hsla())
                    .border_1()
                    .border_color(p.subtle_border())
                    .items_center()
                    .gap(px(5.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h_open(MessageAction::InspectAttachment(att_clone.clone()), w, cx);
                    })
                    .child(kiss_icon(icon, 13.0, p.accent.to_hsla()))
                    .child(
                        div()
                            .max_w(px(220.0))
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(p.primary_ink.to_hsla())
                            .child(att.file_name.clone()),
                    ),
            );
        }
        stack = stack.child(chip_row);
    }

    stack.into_any_element()
}

pub(super) fn render_generated_media_section(
    ix: usize,
    media_items: &[GeneratedMediaItem],
    p: &ParchmentPalette,
    on_action: &MessageActionHandler,
) -> Option<AnyElement> {
    if media_items.is_empty() {
        return None;
    }
    let mut row = h_flex().w_full().flex_wrap().gap(px(10.0)).mt(px(4.0));
    for (m_ix, item) in media_items.iter().enumerate() {
        let item_clone = item.clone();
        let h_open = on_action.clone();
        let local_exists =
            !item.local_file_path.is_empty() && PathBuf::from(&item.local_file_path).exists();
        let kind_icon = match item.kind {
            MediaKind::Image => KissIcon::Image,
            MediaKind::Video => KissIcon::Video,
            MediaKind::Audio => KissIcon::Audio,
        };
        let label = match item.kind {
            MediaKind::Image => "Generated image",
            MediaKind::Video => "Generated video",
            MediaKind::Audio => "Generated audio",
        };
        let preview: AnyElement = if item.kind == MediaKind::Image && local_exists {
            img(PathBuf::from(&item.local_file_path))
                .max_h(px(260.0))
                .max_w(px(360.0))
                .rounded(px(6.0))
                .into_any_element()
        } else {
            h_flex()
                .w(px(220.0))
                .h(px(64.0))
                .rounded(px(6.0))
                .bg(p.sheet_bg.with_alpha(0.6))
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(kiss_icon(kind_icon, 16.0, p.accent.to_hsla()))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_semibold()
                        .text_color(p.accent.to_hsla())
                        .child(label),
                )
                .into_any_element()
        };
        row = row.child(
            v_flex()
                .id(("gen-media", ix * 32 + m_ix))
                .max_w(px(380.0))
                .p(px(8.0))
                .rounded(px(10.0))
                .bg(p.card_bg.to_hsla())
                .border_1()
                .border_color(p.subtle_border())
                .gap(px(6.0))
                .cursor_pointer()
                .on_click(move |_, w, cx| {
                    h_open(
                        MessageAction::InspectGeneratedMedia(item_clone.clone()),
                        w,
                        cx,
                    );
                })
                .child(preview)
                .children((!item.revised_prompt.trim().is_empty()).then(|| {
                    div()
                        .text_size(px(11.0))
                        .text_color(p.muted_ink.to_hsla())
                        .child(clip_preview(&item.revised_prompt, 140))
                })),
        );
    }
    Some(row.into_any_element())
}
