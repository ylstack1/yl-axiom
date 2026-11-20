use super::*;

pub(super) fn render_worldbook_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("World Book & Cards");

    // 1. Character & Scenario Cards (`SettingsWorldBookPage.kt` editor + JSON & PNG export)
    let h_imp_card = on.clone();
    let h_exp_card = on.clone();
    let h_exp_png = on.clone();
    let h_clr_card = on.clone();
    let h_new_card = on.clone();
    let mut cards_card = section_card_with_action(
        "Character & scenario cards",
        None,
        Some(
            h_flex()
                .gap(px(6.0))
                .child(sepia_button(
                    "wb-imp-card",
                    "Import card (.PNG / .JSON)",
                    Some(KissIcon::Upload),
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_imp_card(SettingsAction::ImportCharacterCard, w, cx),
                ))
                .children(props.active_session.active_card.is_some().then(|| {
                    h_flex()
                        .gap(px(6.0))
                        .child(sepia_button(
                            "wb-exp-card",
                            "Export .JSON",
                            Some(KissIcon::Download),
                            SepiaButtonStyle::Outline,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_exp_card(SettingsAction::ExportActiveCard, w, cx),
                        ))
                        .child(sepia_button(
                            "wb-exp-card-png",
                            "Export .PNG",
                            Some(KissIcon::Image),
                            SepiaButtonStyle::Outline,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_exp_png(SettingsAction::ExportActiveCardPng, w, cx),
                        ))
                }))
                .into_any_element(),
        ),
        p,
    );

    if let Some(card) = &props.active_session.active_card {
        cards_card = cards_card.child(
            v_flex()
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.accent.with_alpha(0.08))
                .border_1()
                .border_color(p.accent.with_alpha(0.45))
                .gap(px(8.0))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_semibold()
                                .text_color(p.primary_ink.to_hsla())
                                .child(format!("Active card: {}", card.name)),
                        )
                        .child(
                            h_flex()
                                .gap(px(6.0))
                                .child({
                                    let h_save_lib = on.clone();
                                    sepia_button(
                                        "wb-save-card-lib",
                                        "Save to library",
                                        Some(KissIcon::Check),
                                        SepiaButtonStyle::Outline,
                                        true,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            h_save_lib(
                                                SettingsAction::SaveActiveCardToLibrary,
                                                w,
                                                cx,
                                            )
                                        },
                                    )
                                })
                                .child(sepia_button(
                                    "wb-detach-card",
                                    "Detach",
                                    None,
                                    SepiaButtonStyle::Ghost,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_clr_card(SettingsAction::ClearActiveCard, w, cx)
                                    },
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .gap(px(3.0))
                        .child(field_label("Character / scenario name", p))
                        .child(Input::new(&props.inputs.wb_card_name)),
                )
                .child(
                    v_flex()
                        .gap(px(3.0))
                        .child(field_label("Description & persona", p))
                        .child(Textarea::new(&props.inputs.wb_card_desc).h(px(72.0))),
                )
                .child(
                    v_flex()
                        .gap(px(3.0))
                        .child(field_label("Opening greeting (First message)", p))
                        .child(Textarea::new(&props.inputs.wb_card_first_mes).h(px(64.0))),
                ),
        );
    } else {
        cards_card = cards_card.child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(p.muted_ink.to_hsla())
                        .child("No active card attached to this chat."),
                )
                .child(sepia_button(
                    "wb-new-card",
                    "Create card",
                    Some(KissIcon::Plus),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_new_card(SettingsAction::CreateBlankActiveCard, w, cx),
                )),
        );
    }
    if !props.snapshot.saved_cards.is_empty() {
        cards_card = cards_card.child(field_label("Saved card library", p));
        for (ix, sc) in props.snapshot.saved_cards.iter().enumerate() {
            let cid_a = sc.name.clone();
            let cid_d = sc.name.clone();
            let h_a = on.clone();
            let h_d = on.clone();
            cards_card = cards_card.child(
                h_flex()
                    .id(("saved-card", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.0))
                            .font_medium()
                            .text_color(p.primary_ink.to_hsla())
                            .child(sc.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.0))
                            .child(sepia_button(
                                ("sc-apply", ix),
                                "Apply to chat",
                                None,
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_a(SettingsAction::ApplySavedCard(cid_a.clone()), w, cx)
                                },
                            ))
                            .child(icon_button(
                                ("sc-del", ix),
                                KissIcon::Trash,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_d(SettingsAction::DeleteSavedCard(cid_d.clone()), w, cx)
                                },
                            )),
                    ),
            );
        }
    }

    // 2. System Prompt Presets
    let h_save_pr = on.clone();
    let mut presets_card = section_card_with_action(
        "System prompt presets",
        None,
        Some(
            sepia_button(
                "wb-save-preset",
                "Save current prompt",
                Some(KissIcon::Plus),
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_save_pr(SettingsAction::SaveCurrentPromptAsPreset, w, cx),
            )
            .into_any_element(),
        ),
        p,
    );
    if props.snapshot.system_prompt_presets.is_empty() {
        presets_card = presets_card.child(
            div()
                .text_size(px(12.5))
                .text_color(p.muted_ink.to_hsla())
                .child("No saved presets yet."),
        );
    } else {
        for (ix, pr) in props.snapshot.system_prompt_presets.iter().enumerate() {
            let pid_a = pr.id.clone();
            let pid_d = pr.id.clone();
            let h_a = on.clone();
            let h_d = on.clone();
            presets_card = presets_card.child(
                h_flex()
                    .id(("pr-row", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.0))
                            .font_medium()
                            .text_color(p.primary_ink.to_hsla())
                            .child(pr.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.0))
                            .child(sepia_button(
                                ("pr-apply", ix),
                                "Apply",
                                None,
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_a(SettingsAction::ApplyPresetToChat(pid_a.clone()), w, cx)
                                },
                            ))
                            .child(icon_button(
                                ("pr-del", ix),
                                KissIcon::Close,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_d(SettingsAction::DeletePreset(pid_d.clone()), w, cx)
                                },
                            )),
                    ),
            );
        }
    }

    // 3. World Book entries (This chat vs Global World Book)
    let h_sc_chat = on.clone();
    let h_sc_glob = on.clone();
    let h_add_frag = on.clone();
    let active_frags = match props.world_book_scope {
        WorldBookScope::ActiveChat => &props.active_session.fragments,
        WorldBookScope::Global => &props.snapshot.global_world_book,
    };

    let mut wb_card = section_card("World Book entries", None, p)
        .child(
            h_flex()
                .gap(px(6.0))
                .child(sepia_chip(
                    "wb-sc-chat",
                    "This chat",
                    None,
                    props.world_book_scope == WorldBookScope::ActiveChat,
                    p,
                    move |_, w, cx| {
                        h_sc_chat(
                            SettingsAction::SetWorldBookScope(WorldBookScope::ActiveChat),
                            w,
                            cx,
                        )
                    },
                ))
                .child(sepia_chip(
                    "wb-sc-glob",
                    "Global World Book",
                    None,
                    props.world_book_scope == WorldBookScope::Global,
                    p,
                    move |_, w, cx| {
                        h_sc_glob(
                            SettingsAction::SetWorldBookScope(WorldBookScope::Global),
                            w,
                            cx,
                        )
                    },
                )),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Entry title", p))
                        .child(Input::new(&props.inputs.wb_title)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Keywords (comma-separated)", p))
                        .child(Input::new(&props.inputs.wb_keywords)),
                ),
        )
        .child(Textarea::new(&props.inputs.wb_content).h(px(72.0)))
        .child(h_flex().justify_end().child(sepia_button(
            "wb-add-btn",
            "Add entry",
            Some(KissIcon::Plus),
            SepiaButtonStyle::Primary,
            true,
            false,
            p,
            move |_, w, cx| h_add_frag(SettingsAction::AddWorldBookFragment, w, cx),
        )));

    for (ix, frag) in active_frags.iter().enumerate() {
        let fid_m = frag.id.clone();
        let fid_d = frag.id.clone();
        let h_m = on.clone();
        let h_d = on.clone();
        let mode_lbl = match frag.mode {
            FragmentActivationMode::Pinned => "Pinned",
            FragmentActivationMode::Keyword => "Keyword",
            FragmentActivationMode::Muted => "Muted",
        };
        wb_card = wb_card.child(
            h_flex()
                .id(("wb-frag", ix))
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.elevated_bg.with_alpha(0.45))
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_semibold()
                                .text_color(p.primary_ink.to_hsla())
                                .child(frag.title.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(p.muted_ink.to_hsla())
                                .child(if frag.keywords.is_empty() {
                                    frag.content.chars().take(100).collect()
                                } else {
                                    format!("Keywords: {}", frag.keywords.join(", "))
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(4.0))
                        .child(sepia_chip(
                            ("wb-mode", ix),
                            mode_lbl,
                            None,
                            frag.mode != FragmentActivationMode::Muted,
                            p,
                            move |_, w, cx| {
                                h_m(SettingsAction::CycleFragmentMode(fid_m.clone()), w, cx)
                            },
                        ))
                        .child(icon_button(
                            ("wb-del", ix),
                            KissIcon::Trash,
                            false,
                            p,
                            move |_, w, cx| {
                                h_d(
                                    SettingsAction::DeleteWorldBookFragment(fid_d.clone()),
                                    w,
                                    cx,
                                )
                            },
                        )),
                ),
        );
    }

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(cards_card)
        .child(presets_card)
        .child(wb_card)
        .into_any_element()
}

// ============================================================================================
// Page 7: Shortcuts (`SettingsShortcutsPage.kt` 1:1 — Interactive Recorder & Conflict Swap)
// ============================================================================================
pub(super) fn render_shortcuts_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let prefs = &props.snapshot.preferences;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("Shortcuts");

    let any_custom = AppShortcutAction::ALL
        .iter()
        .any(|a| is_shortcut_customized(*a, &prefs.custom_shortcuts));
    let h_reset_all = on.clone();

    let mut custom_card = section_card_with_action(
        "Keyboard shortcuts",
        Some("Click any shortcut to record a new key combination. Press Esc to cancel."),
        any_custom.then(|| {
            sepia_button(
                "sc-reset-all",
                "Reset all",
                Some(KissIcon::Refresh),
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_reset_all(SettingsAction::ResetAllShortcuts, w, cx),
            )
            .into_any_element()
        }),
        p,
    );

    if !props.shortcut_feedback.is_empty() {
        custom_card = custom_card.child(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .bg(p.danger.with_alpha(0.12))
                .text_size(px(12.0))
                .text_color(p.danger.to_hsla())
                .child(props.shortcut_feedback.to_string()),
        );
    }

    if let Some(conf) = props.shortcut_conflict {
        let h_sw = on.clone();
        let h_cn = on.clone();
        custom_card = custom_card.child(
            h_flex()
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.accent.with_alpha(0.12))
                .border_1()
                .border_color(p.accent.to_hsla())
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(12.5))
                        .text_color(p.primary_ink.to_hsla())
                        .child(format!(
                            "{} is already used by \"{}\". Swap them?",
                            conf.binding.format(),
                            conf.conflict_with.title()
                        )),
                )
                .child(
                    h_flex()
                        .gap(px(6.0))
                        .child(sepia_button(
                            "sc-swap-yes",
                            "Swap shortcuts",
                            None,
                            SepiaButtonStyle::Primary,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_sw(SettingsAction::AcceptShortcutSwap, w, cx),
                        ))
                        .child(sepia_button(
                            "sc-swap-no",
                            strings::CANCEL,
                            None,
                            SepiaButtonStyle::Ghost,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_cn(SettingsAction::CancelShortcutSwap, w, cx),
                        )),
                ),
        );
    }

    let hover_bg = p.hover_tint();
    let mut sc_list = v_flex().w_full().gap(px(0.0));
    for (ix, action) in AppShortcutAction::ALL.iter().copied().enumerate() {
        if ix > 0 {
            sc_list = sc_list.child(h_divider(p));
        }
        let is_rec = props.recording_shortcut == Some(action);
        let binding = resolve_shortcut(action, &prefs.custom_shortcuts);
        let is_mod = is_shortcut_customized(action, &prefs.custom_shortcuts);
        let h_rec = on.clone();
        let h_rst = on.clone();

        sc_list = sc_list.child(
            h_flex()
                .id(("sc-row", ix))
                .w_full()
                .px(px(6.0))
                .py(px(10.0))
                .rounded(px(8.0))
                .bg(if is_rec {
                    p.accent.with_alpha(0.08)
                } else {
                    p.canvas_bg.with_alpha(0.0)
                })
                .justify_between()
                .items_center()
                .gap(px(12.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(14.0))
                                .font_medium()
                                .text_color(p.primary_ink.to_hsla())
                                .child(action.title()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(p.muted_ink.to_hsla())
                                .child(action.description()),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            h_flex()
                                .id(("sc-pill", ix))
                                .h(px(34.0))
                                .px(px(10.0))
                                .rounded(px(8.0))
                                .bg(if is_rec {
                                    p.accent.with_alpha(0.16)
                                } else {
                                    p.elevated_bg.with_alpha(0.65)
                                })
                                .border_1()
                                .border_color(if is_rec {
                                    p.accent.to_hsla()
                                } else {
                                    p.border.with_alpha(0.85)
                                })
                                .items_center()
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(move |_, w, cx| {
                                    if is_rec {
                                        h_rec(SettingsAction::CancelRecordShortcut, w, cx);
                                    } else {
                                        h_rec(SettingsAction::BeginRecordShortcut(action), w, cx);
                                    }
                                })
                                .child(if is_rec {
                                    div()
                                        .text_size(px(12.0))
                                        .font_semibold()
                                        .text_color(p.accent.to_hsla())
                                        .child("Press keys… (Esc)")
                                        .into_any_element()
                                } else {
                                    shortcut_keycap_row(&binding.keycap_tokens(), is_mod, p)
                                }),
                        )
                        .children(is_mod.then(|| {
                            icon_button(
                                ("sc-rst", ix),
                                KissIcon::Refresh,
                                false,
                                p,
                                move |_, w, cx| {
                                    cx.stop_propagation();
                                    h_rst(SettingsAction::ResetSingleShortcut(action), w, cx);
                                },
                            )
                        })),
                ),
        );
    }
    custom_card = custom_card.child(sc_list);

    let h_s1 = on.clone();
    let h_s2 = on.clone();
    let h_s3 = on.clone();
    let send_card = section_card("Composer send shortcut", None, p).child(
        h_flex()
            .gap(px(6.0))
            .child(sepia_chip(
                "scs-enter",
                "Enter",
                None,
                prefs.send_shortcut == SendShortcutMode::Enter,
                p,
                move |_, w, cx| {
                    h_s1(
                        SettingsAction::SetSendShortcut(SendShortcutMode::Enter),
                        w,
                        cx,
                    )
                },
            ))
            .child(sepia_chip(
                "scs-ctrl",
                "Ctrl+Enter",
                None,
                prefs.send_shortcut == SendShortcutMode::CtrlEnter,
                p,
                move |_, w, cx| {
                    h_s2(
                        SettingsAction::SetSendShortcut(SendShortcutMode::CtrlEnter),
                        w,
                        cx,
                    )
                },
            ))
            .child(sepia_chip(
                "scs-shift",
                "Shift+Enter",
                None,
                prefs.send_shortcut == SendShortcutMode::ShiftEnter,
                p,
                move |_, w, cx| {
                    h_s3(
                        SettingsAction::SetSendShortcut(SendShortcutMode::ShiftEnter),
                        w,
                        cx,
                    )
                },
            )),
    );

    let builtin_card = section_card("Built-in shortcuts", None, p)
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(p.primary_ink.to_hsla())
                        .child("Close open overlay, dialog, or Settings"),
                )
                .child(shortcut_keycap_row(&["Esc".into()], false, p)),
        )
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(p.primary_ink.to_hsla())
                        .child("Paste text or convert long pastes to file"),
                )
                .child(shortcut_keycap_row(&["Ctrl".into(), "V".into()], false, p)),
        );

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(custom_card)
        .child(send_card)
        .child(builtin_card)
        .into_any_element()
}

// ============================================================================================
// Page 8: Storage & Updates (`desktop-page-stora.png` 1:1)
// ============================================================================================
pub(super) fn render_backup_ota_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("Storage & Updates");

    // 1. Storage & File Manager (`SettingsBackupPage.kt` 1:1)
    let total_bytes: u64 = props.storage_files.iter().map(|f| f.size_bytes).sum();
    let orphan_bytes: u64 = props
        .storage_files
        .iter()
        .filter(|f| f.is_orphan_or_temp)
        .map(|f| f.size_bytes)
        .sum();
    let orphan_count = props
        .storage_files
        .iter()
        .filter(|f| f.is_orphan_or_temp)
        .count();

    let h_open_dir = on.clone();
    let h_refresh = on.clone();
    let h_clean = on.clone();
    let h_cat_all = on.clone();

    // Multi-segment proportional bar
    let cat_color = |cat: StorageFileCategory| match cat {
        StorageFileCategory::Database => p.accent.to_hsla(),
        StorageFileCategory::Uploads => CAT_SLATE.to_hsla(),
        StorageFileCategory::GeneratedMedia => CAT_SAGE.to_hsla(),
        StorageFileCategory::Backups => CAT_OCHRE.to_hsla(),
        StorageFileCategory::TempOrLogs => CAT_BRASS.to_hsla(),
    };
    let mut bar = h_flex()
        .w_full()
        .h(px(10.0))
        .rounded(px(999.0))
        .overflow_hidden()
        .bg(p.elevated_bg.to_hsla())
        .border_1()
        .border_color(p.border.with_alpha(0.65));
    let denom = total_bytes.max(1) as f32;
    for cat in StorageFileCategory::ALL {
        let sum: u64 = props
            .storage_files
            .iter()
            .filter(|f| f.category == cat)
            .map(|f| f.size_bytes)
            .sum();
        if sum > 0 {
            let frac = (sum as f32 / denom).max(0.02);
            bar = bar.child(div().h_full().w(gpui::relative(frac)).bg(cat_color(cat)));
        }
    }

    let mut cat_row = h_flex().flex_wrap().gap(px(5.0)).child(sepia_chip(
        "st-cat-all",
        format!(
            "All ({} · {})",
            props.storage_files.len(),
            format_bytes_compact(total_bytes)
        ),
        None,
        props.storage_category_filter.is_none(),
        p,
        move |_, w, cx| h_cat_all(SettingsAction::SetStorageCategoryFilter(None), w, cx),
    ));
    for (ix, cat) in StorageFileCategory::ALL.iter().copied().enumerate() {
        let mut cnt = 0usize;
        let mut cat_bytes = 0u64;
        for f in props.storage_files.iter().filter(|f| f.category == cat) {
            cnt += 1;
            cat_bytes += f.size_bytes;
        }
        if cnt == 0 {
            continue;
        }
        let h_c = on.clone();
        cat_row = cat_row.child(sepia_chip(
            ("st-cat", ix),
            format!(
                "{} ({cnt} · {})",
                cat.label(),
                format_bytes_compact(cat_bytes)
            ),
            None,
            props.storage_category_filter == Some(cat),
            p,
            move |_, w, cx| h_c(SettingsAction::SetStorageCategoryFilter(Some(cat)), w, cx),
        ));
    }
    let sq = props.storage_search_query.trim().to_ascii_lowercase();
    let visible_files: Vec<&StoredFileEntry> = props
        .storage_files
        .iter()
        .filter(|f| {
            let cat_ok = props
                .storage_category_filter
                .map(|c| f.category == c)
                .unwrap_or(true);
            let q_ok = sq.is_empty()
                || f.file_name.to_ascii_lowercase().contains(&sq)
                || f.relative_path.to_ascii_lowercase().contains(&sq);
            cat_ok && q_ok
        })
        .take(120)
        .collect();

    let mut file_list = v_flex()
        .id("storage-files-scroll")
        .max_h(px(260.0))
        .w_full()
        .gap(px(4.0))
        .overflow_y_scrollbar();

    for (ix, f) in visible_files.into_iter().enumerate() {
        let fp_prev = f.full_path.clone();
        let fp_open = f.full_path.clone();
        let fp_del = f.full_path.clone();
        let h_prev = on.clone();
        let h_open = on.clone();
        let h_del = on.clone();
        file_list = file_list.child(
            h_flex()
                .id(("st-file", ix))
                .w_full()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .bg(p.elevated_bg.with_alpha(0.42))
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .flex_1()
                        .overflow_hidden()
                        .items_center()
                        .gap(px(7.0))
                        .child(
                            div()
                                .w(px(7.0))
                                .h(px(7.0))
                                .rounded(px(999.0))
                                .bg(cat_color(f.category))
                                .flex_shrink_0(),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.5))
                                        .font_medium()
                                        .text_color(p.primary_ink.to_hsla())
                                        .child(f.relative_path.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(p.muted_ink.to_hsla())
                                        .child(format!(
                                            "{} · {}{}",
                                            f.category.label(),
                                            format_bytes_compact(f.size_bytes),
                                            if f.is_orphan_or_temp {
                                                " · unused"
                                            } else {
                                                ""
                                            }
                                        )),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .gap(px(2.0))
                        .child(icon_button(
                            ("st-f-prev", ix),
                            KissIcon::Eye,
                            false,
                            p,
                            move |_, w, cx| {
                                h_prev(SettingsAction::PreviewStoredFile(fp_prev.clone()), w, cx)
                            },
                        ))
                        .child(icon_button(
                            ("st-f-open", ix),
                            KissIcon::Folder,
                            false,
                            p,
                            move |_, w, cx| {
                                h_open(SettingsAction::OpenStoredFile(fp_open.clone()), w, cx)
                            },
                        ))
                        .children((f.category != StorageFileCategory::Database).then(|| {
                            icon_button(
                                ("st-f-del", ix),
                                KissIcon::Trash,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_del(SettingsAction::DeleteStoredFile(fp_del.clone()), w, cx)
                                },
                            )
                        })),
                ),
        );
    }

    let storage_card = section_card_with_action(
        &format!(
            "Storage & file manager ({} files · {})",
            props.storage_files.len(),
            format_bytes_compact(total_bytes)
        ),
        Some(props.data_dir_display),
        Some(
            h_flex()
                .gap(px(6.0))
                .child(sepia_button(
                    "st-open-dir",
                    "Open folder",
                    Some(KissIcon::Folder),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_open_dir(SettingsAction::OpenDataFolder, w, cx),
                ))
                .child(icon_button(
                    "st-refresh",
                    KissIcon::Refresh,
                    false,
                    p,
                    move |_, w, cx| h_refresh(SettingsAction::RefreshStorageScan, w, cx),
                ))
                .into_any_element(),
        ),
        p,
    )
    .child(bar)
    .child(
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .gap(px(8.0))
            .child(cat_row)
            .children((orphan_count > 0).then(|| {
                sepia_button(
                    "st-clean-orphans",
                    format!("Clean unused ({})", format_bytes_compact(orphan_bytes)),
                    Some(KissIcon::Trash),
                    SepiaButtonStyle::Secondary,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_clean(SettingsAction::CleanOrphanStorageFiles, w, cx),
                )
            })),
    )
    .child(Input::new(&props.inputs.storage_search).prefix(kiss_icon(
        KissIcon::Search,
        13.0,
        p.muted_ink.to_hsla(),
    )))
    .child(file_list);

    // 2. App updates (auto-saves GitHub repository on edit)
    let h_ota_auto = on.clone();
    let h_ota_chk = on.clone();
    let h_ota_ver = on.clone();
    let mut ota_card = section_card(
        "App updates",
        Some(&format!(
            "Parchment v{} • Windows • {} stored chat{}",
            env!("CARGO_PKG_VERSION"),
            props.chat_count,
            if props.chat_count == 1 { "" } else { "s" }
        )),
        p,
    )
    .child(
        v_flex()
            .w_full()
            .gap(px(3.0))
            .child(field_label("GitHub repository (owner/repo)", p))
            .child(Input::new(&props.inputs.ota_repo)),
    )
    .child(setting_toggle_row(
        "ota-auto-chk",
        "Check for updates at launch",
        "",
        props.snapshot.auto_check_ota_on_startup,
        p,
        move |v, w, cx| h_ota_auto(SettingsAction::ToggleAutoCheckOta(*v), w, cx),
    ))
    .child(
        h_flex()
            .gap(px(8.0))
            .child(sepia_button(
                "ota-check-now",
                if props.is_checking_updates {
                    "Checking…"
                } else {
                    strings::CHECK_FOR_UPDATES
                },
                Some(KissIcon::Refresh),
                SepiaButtonStyle::Primary,
                true,
                props.is_checking_updates,
                p,
                move |_, w, cx| h_ota_chk(SettingsAction::CheckForUpdates, w, cx),
            ))
            .child(sepia_button(
                "ota-verify-staged",
                "Verify downloaded installer",
                Some(KissIcon::ShieldCheck),
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_ota_ver(SettingsAction::VerifyDownloadedInstaller, w, cx),
            )),
    );
    if !props.update_status.is_empty() {
        ota_card = ota_card.child(
            div()
                .text_size(px(12.0))
                .text_color(p.muted_ink.to_hsla())
                .child(props.update_status.to_string()),
        );
    }
    if let Some(rel) = props.latest_release {
        let url = rel.release_url.clone();
        let h_rel = on.clone();
        ota_card = ota_card.child(
            h_flex()
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.accent.with_alpha(0.10))
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_semibold()
                        .text_color(p.accent.to_hsla())
                        .child(format!("Release {} is available", rel.tag_name)),
                )
                .child(sepia_button(
                    "ota-open-rel",
                    "Open release page",
                    Some(KissIcon::ExternalLink),
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_rel(SettingsAction::OpenReleasePage(url.clone()), w, cx),
                )),
        );
    }

    // 3. Portable chats & media + Settings JSON backup (`SettingsBackupPage.kt`)
    let h_exp_arch = on.clone();
    let h_exp_cur_arch = on.clone();
    let h_exp_cur_json = on.clone();
    let h_imp_arch = on.clone();
    let h_exp_json = on.clone();
    let h_imp_json = on.clone();
    let backup_card = section_card("Portable chats & settings backup", None, p)
        .child(
            h_flex()
                .flex_wrap()
                .gap(px(8.0))
                .child(sepia_button(
                    "bk-exp-cur-arch",
                    "Export current chat (.parchment)",
                    Some(KissIcon::Download),
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_exp_cur_arch(SettingsAction::ExportCurrentChatArchive, w, cx),
                ))
                .child(sepia_button(
                    "bk-exp-cur-json",
                    "Export current chat (.json)",
                    Some(KissIcon::FileText),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_exp_cur_json(SettingsAction::ExportCurrentChatJson, w, cx),
                ))
                .child(sepia_button(
                    "bk-exp-arch",
                    strings::EXPORT_ARCHIVE,
                    Some(KissIcon::Download),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_exp_arch(SettingsAction::ExportArchive, w, cx),
                ))
                .child(sepia_button(
                    "bk-imp-arch",
                    strings::IMPORT_ARCHIVE,
                    Some(KissIcon::Upload),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_imp_arch(SettingsAction::ImportArchive, w, cx),
                )),
        )
        .child(h_divider(p))
        .child(
            h_flex()
                .flex_wrap()
                .gap(px(8.0))
                .child(sepia_button(
                    "bk-exp-json",
                    "Export settings (.json)",
                    Some(KissIcon::Download),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_exp_json(SettingsAction::ExportSettingsJson, w, cx),
                ))
                .child(sepia_button(
                    "bk-imp-json",
                    "Import settings (.json)",
                    Some(KissIcon::Upload),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_imp_json(SettingsAction::ImportSettingsJson, w, cx),
                )),
        );

    // 4. Danger zone
    let h_req_w = on.clone();
    let h_conf_w = on.clone();
    let h_canc_w = on.clone();
    let danger_card = section_card("Danger zone", None, p).child(if props.confirm_wipe_pending {
        h_flex()
            .gap(px(8.0))
            .items_center()
            .child(sepia_button(
                "wipe-confirm",
                "Yes, wipe everything",
                Some(KissIcon::Trash),
                SepiaButtonStyle::Danger,
                true,
                false,
                p,
                move |_, w, cx| h_conf_w(SettingsAction::ConfirmWipeAllData, w, cx),
            ))
            .child(sepia_button(
                "wipe-cancel",
                strings::CANCEL,
                None,
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_canc_w(SettingsAction::CancelWipeAllData, w, cx),
            ))
    } else {
        h_flex().child(sepia_button(
            "wipe-req",
            strings::WIPE_ALL_DATA,
            Some(KissIcon::Trash),
            SepiaButtonStyle::Danger,
            true,
            false,
            p,
            move |_, w, cx| h_req_w(SettingsAction::RequestWipeAllData, w, cx),
        ))
    });

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(storage_card)
        .child(ota_card)
        .child(backup_card)
        .child(danger_card)
        .into_any_element()
}
