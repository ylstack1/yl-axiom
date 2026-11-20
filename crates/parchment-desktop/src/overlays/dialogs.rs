use super::*;

/// 2-column Tab Switcher modal with optional search and `Close empty (N)` action.
#[allow(clippy::too_many_arguments)]
pub fn render_tab_switcher_modal(
    filter_input: &Entity<InputState>,
    filter_query: &str,
    open_tabs: &[String],
    active_chat_id: &str,
    stubs: &[ChatStub],
    streaming_chat_ids: &[String],
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let card = p.card_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let border = p.subtle_border();
    let hover_bg = p.hover_tint();

    let empty_count = open_tabs
        .iter()
        .filter(|tid| {
            let is_run = streaming_chat_ids.iter().any(|id| id == *tid);
            let stub = stubs.iter().find(|s| &s.id == *tid);
            !is_run
                && stub.is_some_and(|s| {
                    s.message_count == 0
                        && (s.title.eq_ignore_ascii_case("New Chat")
                            || s.title.eq_ignore_ascii_case(strings::NEW_CHAT))
                })
        })
        .count();

    let q_low = filter_query.trim().to_ascii_lowercase();
    let visible_tabs: Vec<&String> = open_tabs
        .iter()
        .filter(|tid| {
            if q_low.is_empty() {
                return true;
            }
            stubs
                .iter()
                .find(|s| &s.id == *tid)
                .is_some_and(|s| s.title.to_ascii_lowercase().contains(&q_low))
        })
        .collect();

    let h_new = on_action.clone();
    let h_close = on_action.clone();
    let h_close_empty = on_action.clone();

    let mut grid = v_flex()
        .id("tab-grid-scroll")
        .max_h(px(420.0))
        .w_full()
        .gap(px(8.0))
        .overflow_y_scrollbar();

    for chunk in visible_tabs.chunks(2) {
        let mut pair = h_flex().w_full().gap(px(8.0));
        for (c_ix, tid) in chunk.iter().enumerate() {
            let stub = stubs.iter().find(|s| &s.id == *tid);
            let title = stub
                .map(|s| s.title.clone())
                .unwrap_or_else(|| strings::NEW_CHAT.into());
            let msgs = stub.map(|s| s.message_count).unwrap_or(0);
            let is_sel = *tid == active_chat_id;
            let is_run = streaming_chat_ids.iter().any(|id| id == *tid);
            let cid_s = (*tid).clone();
            let cid_c = (*tid).clone();
            let h_s = on_action.clone();
            let h_c = on_action.clone();

            pair = pair.child(
                h_flex()
                    .id((SharedString::from(format!("tab-ov-{tid}")), c_ix))
                    .relative()
                    .flex_1()
                    .p(px(12.0))
                    .rounded(px(10.0))
                    .bg(card)
                    .border_1()
                    .border_color(if is_sel { accent } else { border })
                    .justify_between()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| h_s(OverlayAction::SelectChat(cid_s.clone()), w, cx))
                    .children(is_sel.then(|| rubric_mark_overlay(8.0, p)))
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .gap(px(3.0))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .children(is_run.then(|| {
                                        div().w(px(7.0)).h(px(7.0)).rounded(px(999.0)).bg(accent)
                                    }))
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .text_size(px(13.5))
                                            .font_semibold()
                                            .text_color(if is_sel { accent } else { ink })
                                            .child(title),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(muted)
                                    .child(if is_run {
                                        "Generating reply…".to_string()
                                    } else {
                                        format!(
                                            "{msgs} message{}",
                                            if msgs == 1 { "" } else { "s" }
                                        )
                                    }),
                            ),
                    )
                    .child(icon_button(
                        (SharedString::from(format!("tab-ov-x-{tid}")), c_ix),
                        KissIcon::Close,
                        false,
                        p,
                        move |_, w, cx| {
                            cx.stop_propagation();
                            h_c(OverlayAction::CloseTab(cid_c.clone()), w, cx);
                        },
                    )),
            );
        }
        if chunk.len() == 1 {
            pair = pair.child(div().flex_1());
        }
        grid = grid.child(pair);
    }

    let body = v_flex()
        .w_full()
        .p(px(14.0))
        .gap(px(10.0))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(kiss_icon(KissIcon::Tabs, 15.0, accent))
                        .child(
                            div()
                                .text_size(px(14.5))
                                .font_semibold()
                                .text_color(ink)
                                .child(format!("Open tabs ({})", open_tabs.len())),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .children((empty_count > 0 && open_tabs.len() > 1).then(|| {
                            sepia_button(
                                "tab-ov-close-empty",
                                format!("Close empty ({empty_count})"),
                                None,
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| h_close_empty(OverlayAction::CloseEmptyTabs, w, cx),
                            )
                        }))
                        .child(sepia_button(
                            "tab-ov-new",
                            strings::NEW_TAB,
                            Some(KissIcon::Plus),
                            SepiaButtonStyle::Primary,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_new(OverlayAction::NewTab, w, cx),
                        ))
                        .child(icon_button(
                            "tab-ov-close-modal",
                            KissIcon::Close,
                            false,
                            p,
                            move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                        )),
                ),
        )
        .children(
            (open_tabs.len() > 3)
                .then(|| Input::new(filter_input).prefix(kiss_icon(KissIcon::Search, 13.0, muted))),
        )
        .child(h_divider(p))
        .child(grid)
        .into_any_element();

    modal_shell("overlay-tab-switcher", 660.0, p, on_action.clone(), body)
}

/// Built-in Zoomable Image & Text/Code/SVG/Markdown Viewer Modal (`ParchmentAttachmentViewerModal` parity).
#[allow(clippy::too_many_arguments)]
pub fn render_attachment_viewer_modal(
    item: &ActiveMediaViewerItem,
    zoom_scale: f32,
    show_rendered_preview: bool,
    is_wrapped: bool,
    copied: bool,
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let h_dismiss = on_action.clone();
    let scrim = gpui::hsla(0.0, 0.0, 0.0, 0.56);

    let inner: AnyElement = match item {
        ActiveMediaViewerItem::Image {
            title,
            local_file_path,
            remote_url,
            mime_type,
        } => {
            let h_out = on_action.clone();
            let h_reset = on_action.clone();
            let h_in = on_action.clone();
            let h_open = on_action.clone();
            let h_save = on_action.clone();
            let h_close = on_action.clone();
            let lp_open = local_file_path.clone();
            let ru_open = remote_url.clone();
            let lp_save = local_file_path.clone();
            let title_save = title.clone();
            let local_exists =
                !local_file_path.is_empty() && PathBuf::from(local_file_path).exists();
            let pct = (zoom_scale * 100.0).round() as i32;
            let disp_w = (720.0 * zoom_scale).clamp(240.0, 1800.0);
            let disp_h = (480.0 * zoom_scale).clamp(180.0, 1200.0);

            v_flex()
                .id("img-viewer-card")
                .w(px(920.0))
                .h(px(660.0))
                .p(px(16.0))
                .rounded(px(12.0))
                .bg(p.elevated_bg.to_hsla())
                .border_1()
                .border_color(p.border.to_hsla())
                .shadow_lg()
                .gap(px(10.0))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            h_flex()
                                .flex_1()
                                .overflow_hidden()
                                .items_center()
                                .gap(px(6.0))
                                .child(kiss_icon(KissIcon::Image, 16.0, accent))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_size(px(15.0))
                                        .font_semibold()
                                        .text_color(ink)
                                        .child(if mime_type.is_empty() {
                                            title.clone()
                                        } else {
                                            format!("{title} ({mime_type})")
                                        }),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(5.0))
                                .child(icon_button(
                                    "img-zoom-out",
                                    KissIcon::ZoomOut,
                                    false,
                                    p,
                                    move |_, w, cx| h_out(OverlayAction::ViewerZoomOut, w, cx),
                                ))
                                .child(sepia_button(
                                    "img-zoom-reset",
                                    format!("{pct}%"),
                                    None,
                                    SepiaButtonStyle::Outline,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| h_reset(OverlayAction::ViewerZoomReset, w, cx),
                                ))
                                .child(icon_button(
                                    "img-zoom-in",
                                    KissIcon::ZoomIn,
                                    false,
                                    p,
                                    move |_, w, cx| h_in(OverlayAction::ViewerZoomIn, w, cx),
                                ))
                                .children((local_exists || !remote_url.is_empty()).then(|| {
                                    sepia_button(
                                        "img-open-os",
                                        "Open",
                                        Some(KissIcon::ExternalLink),
                                        SepiaButtonStyle::Outline,
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            if !lp_open.is_empty() {
                                                h_open(
                                                    OverlayAction::ViewerOpenLocal(lp_open.clone()),
                                                    w,
                                                    cx,
                                                );
                                            } else if !ru_open.is_empty() {
                                                h_open(
                                                    OverlayAction::ViewerOpenUrl(ru_open.clone()),
                                                    w,
                                                    cx,
                                                );
                                            }
                                        },
                                    )
                                }))
                                .children(local_exists.then(|| {
                                    sepia_button(
                                        "img-save-file",
                                        "Save",
                                        Some(KissIcon::Download),
                                        SepiaButtonStyle::Secondary,
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            h_save(
                                                OverlayAction::ViewerSaveFile {
                                                    title: title_save.clone(),
                                                    local_path: lp_save.clone(),
                                                    text_content: None,
                                                },
                                                w,
                                                cx,
                                            );
                                        },
                                    )
                                }))
                                .child(sepia_button(
                                    "img-close",
                                    "Close",
                                    Some(KissIcon::Close),
                                    SepiaButtonStyle::Primary,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                                )),
                        ),
                )
                .child(h_divider(p))
                .child(
                    div()
                        .id("img-viewer-scroll")
                        .flex_1()
                        .w_full()
                        .rounded(px(8.0))
                        .bg(p.sheet_bg.with_alpha(0.65))
                        .border_1()
                        .border_color(p.subtle_border())
                        .overflow_y_scrollbar()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(12.0))
                        .child(if local_exists {
                            img(PathBuf::from(local_file_path))
                                .max_w(px(disp_w))
                                .max_h(px(disp_h))
                                .rounded(px(6.0))
                                .into_any_element()
                        } else {
                            div()
                                .text_size(px(13.0))
                                .text_color(muted)
                                .child(format!(
                                    "Image preview unavailable locally ({})",
                                    if !remote_url.is_empty() {
                                        remote_url.as_str()
                                    } else {
                                        local_file_path.as_str()
                                    }
                                ))
                                .into_any_element()
                        }),
                )
                .into_any_element()
        }
        ActiveMediaViewerItem::TextOrCode {
            title,
            text_content,
            mime_type,
            local_file_path,
        } => {
            let lower = title.to_ascii_lowercase();
            let is_svg = lower.ends_with(".svg")
                || mime_type.eq_ignore_ascii_case("image/svg+xml")
                || (text_content.contains("<svg") && text_content.contains("</svg>"));
            let is_md = lower.ends_with(".md")
                || lower.ends_with(".markdown")
                || mime_type.eq_ignore_ascii_case("text/markdown");
            let lines_count = text_content.lines().count().max(1);
            let chars_count = text_content.chars().count();

            let h_prev = on_action.clone();
            let h_wrap = on_action.clone();
            let h_copy = on_action.clone();
            let h_save = on_action.clone();
            let h_open = on_action.clone();
            let h_close = on_action.clone();
            let copy_text = text_content.clone();
            let save_text = text_content.clone();
            let save_title = title.clone();
            let save_lp = local_file_path.clone();
            let open_lp = local_file_path.clone();

            let content_body: AnyElement = if is_svg && show_rendered_preview {
                if let Some(doc) = parse_svg_bounded(text_content) {
                    let safe = if text_content.contains("xmlns=") {
                        text_content.clone()
                    } else {
                        text_content.replacen(
                            "<svg",
                            r#"<svg xmlns="http://www.w3.org/2000/svg""#,
                            1,
                        )
                    };
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(12.0))
                        .child(
                            Icon::default()
                                .data(safe.as_bytes())
                                .w(px(doc.width.clamp(120.0, 760.0)))
                                .h(px(doc.height.clamp(90.0, 480.0)))
                                .text_color(ink),
                        )
                        .into_any_element()
                } else {
                    div()
                        .text_size(px(13.0))
                        .text_color(muted)
                        .child("Unable to render SVG preview.")
                        .into_any_element()
                }
            } else if is_md && show_rendered_preview {
                div()
                    .w_full()
                    .line_height(gpui::relative(p.line_height_mult))
                    .child(
                        TextView::markdown("viewer-md", SharedString::from(text_content.clone()))
                            .selectable(true),
                    )
                    .into_any_element()
            } else {
                let mut text_box = div()
                    .w_full()
                    .font_family(p.code_font_family.clone())
                    .text_size(px(12.5))
                    .line_height(px(18.0))
                    .text_color(ink)
                    .child(if text_content.is_empty() {
                        "(Empty file)".to_string()
                    } else {
                        text_content.clone()
                    });
                if !is_wrapped {
                    text_box = text_box.whitespace_nowrap();
                }
                text_box.into_any_element()
            };

            v_flex()
                .id("img-viewer-card")
                .w(px(920.0))
                .h(px(660.0))
                .p(px(16.0))
                .rounded(px(12.0))
                .bg(p.elevated_bg.to_hsla())
                .border_1()
                .border_color(p.border.to_hsla())
                .shadow_lg()
                .gap(px(10.0))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(kiss_icon(KissIcon::FileText, 16.0, accent))
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(px(15.0))
                                                .font_semibold()
                                                .text_color(ink)
                                                .child(title.clone()),
                                        ),
                                )
                                .child(div().text_size(px(11.5)).text_color(muted).child(format!(
                                    "{lines_count} lines · {chars_count} chars · {mime_type}"
                                ))),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(5.0))
                                .children((is_svg || is_md).then(|| {
                                    sepia_button(
                                        "txt-viewer-prev",
                                        if show_rendered_preview {
                                            "Source"
                                        } else {
                                            "Preview"
                                        },
                                        Some(if show_rendered_preview {
                                            KissIcon::Code
                                        } else {
                                            KissIcon::Eye
                                        }),
                                        if show_rendered_preview {
                                            SepiaButtonStyle::Secondary
                                        } else {
                                            SepiaButtonStyle::Outline
                                        },
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            h_prev(OverlayAction::ViewerTogglePreview, w, cx)
                                        },
                                    )
                                }))
                                .children((!(is_svg && show_rendered_preview)).then(|| {
                                    icon_button(
                                        "txt-viewer-wrap",
                                        KissIcon::WrapText,
                                        is_wrapped,
                                        p,
                                        move |_, w, cx| {
                                            h_wrap(OverlayAction::ViewerToggleWrap, w, cx)
                                        },
                                    )
                                }))
                                .child(sepia_button(
                                    "txt-viewer-copy",
                                    if copied { strings::COPIED } else { "Copy" },
                                    Some(if copied {
                                        KissIcon::Check
                                    } else {
                                        KissIcon::Copy
                                    }),
                                    SepiaButtonStyle::Outline,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_copy(
                                            OverlayAction::ViewerCopyText(copy_text.clone()),
                                            w,
                                            cx,
                                        );
                                    },
                                ))
                                .child(sepia_button(
                                    "txt-viewer-save",
                                    "Save",
                                    Some(KissIcon::Download),
                                    SepiaButtonStyle::Secondary,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_save(
                                            OverlayAction::ViewerSaveFile {
                                                title: save_title.clone(),
                                                local_path: save_lp.clone(),
                                                text_content: Some(save_text.clone()),
                                            },
                                            w,
                                            cx,
                                        );
                                    },
                                ))
                                .children((!local_file_path.is_empty()).then(|| {
                                    icon_button(
                                        "txt-viewer-open",
                                        KissIcon::Folder,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            h_open(
                                                OverlayAction::ViewerOpenLocal(open_lp.clone()),
                                                w,
                                                cx,
                                            );
                                        },
                                    )
                                }))
                                .child(sepia_button(
                                    "txt-viewer-close",
                                    "Close",
                                    Some(KissIcon::Close),
                                    SepiaButtonStyle::Primary,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| h_close(OverlayAction::Dismiss, w, cx),
                                )),
                        ),
                )
                .child(h_divider(p))
                .child(
                    div()
                        .id("txt-viewer-scroll")
                        .flex_1()
                        .w_full()
                        .p(px(12.0))
                        .rounded(px(8.0))
                        .bg(p.sheet_bg.with_alpha(0.65))
                        .border_1()
                        .border_color(p.subtle_border())
                        .overflow_y_scrollbar()
                        .child(content_body),
                )
                .into_any_element()
        }
    };

    div()
        .id("overlay-attachment-viewer")
        .absolute()
        .inset_0()
        .bg(scrim)
        .flex()
        .items_center()
        .justify_center()
        .on_click(move |_, w, cx| h_dismiss(OverlayAction::Dismiss, w, cx))
        .child(spring_pop_enter("att-viewer-anim", inner))
        .into_any_element()
}

/// Delete & Regenerate Confirmation Dialogs matching `AppDialogs.kt`.
pub fn render_confirm_dialog(
    kind: &ConfirmDialogKind,
    ask_before_deleting: bool,
    ask_before_regenerating: bool,
    palette: &ParchmentPalette,
    on_action: &OverlayActionHandler,
) -> AnyElement {
    let p = palette;
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let h_cancel = on_action.clone();
    let h_accept = on_action.clone();
    let h_toggle = on_action.clone();

    let (title, body_text, confirm_label, is_danger, toggle_row) = match kind {
        ConfirmDialogKind::DeleteSingleChat { title, .. } => (
            "Delete chat?".to_string(),
            format!(
                "Remove \"{title}\" from your history? You can still undo right after deleting."
            ),
            "Delete chat".to_string(),
            true,
            setting_toggle_row(
                "dlg-ask-del",
                "Ask before deleting chats",
                "",
                ask_before_deleting,
                p,
                move |v, w, cx| h_toggle(OverlayAction::SetConfirmDeletePref(*v), w, cx),
            ),
        ),
        ConfirmDialogKind::ClearAllChats {
            assistant_name,
            count,
        } => (
            format!("Delete all {count} chats?"),
            format!("This removes every conversation under {assistant_name}."),
            "Delete all".to_string(),
            true,
            setting_toggle_row(
                "dlg-ask-del-all",
                "Ask before deleting chats",
                "",
                ask_before_deleting,
                p,
                move |v, w, cx| h_toggle(OverlayAction::SetConfirmDeletePref(*v), w, cx),
            ),
        ),
        ConfirmDialogKind::ClearCurrentMessages { title } => (
            "Clear messages?".to_string(),
            format!("Remove all messages from \"{title}\" while keeping its notes and settings."),
            "Clear messages".to_string(),
            true,
            setting_toggle_row(
                "dlg-ask-clr-msg",
                "Ask before deleting chats",
                "",
                ask_before_deleting,
                p,
                move |v, w, cx| h_toggle(OverlayAction::SetConfirmDeletePref(*v), w, cx),
            ),
        ),
        ConfirmDialogKind::RegenerateTurn { replaces_reply, .. } => (
            "Regenerate reply?".to_string(),
            if *replaces_reply {
                "The previous reply will be replaced with a fresh generation.".to_string()
            } else {
                "A new reply version will be added alongside the existing one.".to_string()
            },
            "Regenerate".to_string(),
            false,
            setting_toggle_row(
                "dlg-ask-regen",
                "Ask before regenerating",
                "",
                ask_before_regenerating,
                p,
                move |v, w, cx| h_toggle(OverlayAction::SetConfirmRegeneratePref(*v), w, cx),
            ),
        ),
    };

    let body = v_flex()
        .w_full()
        .p(px(18.0))
        .gap(px(12.0))
        .child(
            div()
                .text_size(px(16.0))
                .font_semibold()
                .text_color(ink)
                .child(title),
        )
        .child(div().text_size(px(13.5)).text_color(muted).child(body_text))
        .child(toggle_row)
        .child(h_divider(p))
        .child(
            h_flex()
                .w_full()
                .justify_end()
                .gap(px(8.0))
                .child(sepia_button(
                    "dlg-cancel",
                    strings::CANCEL,
                    None,
                    SepiaButtonStyle::Ghost,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_cancel(OverlayAction::Dismiss, w, cx),
                ))
                .child(sepia_button(
                    "dlg-confirm",
                    confirm_label,
                    None,
                    if is_danger {
                        SepiaButtonStyle::Danger
                    } else {
                        SepiaButtonStyle::Primary
                    },
                    true,
                    false,
                    p,
                    move |_, w, cx| h_accept(OverlayAction::ConfirmDialogAccept, w, cx),
                )),
        )
        .into_any_element();

    modal_shell("overlay-confirm-dialog", 440.0, p, on_action.clone(), body)
}
