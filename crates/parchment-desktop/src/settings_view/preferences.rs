use super::*;

// ============================================================================================
pub(super) fn render_preferences_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let prefs = &props.snapshot.preferences;
    let theme = &props.snapshot.theme;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("Preferences");

    // 1. Your profile (`ProfileCard` — auto-saves on edit, no redundant subtitle or Save button)
    let h_pic = on.clone();
    let h_rm_pic = on.clone();
    let pic_exists = !prefs.profile_picture_path.is_empty()
        && PathBuf::from(&prefs.profile_picture_path).exists();
    let avatar_el: AnyElement = if pic_exists {
        img(PathBuf::from(&prefs.profile_picture_path))
            .w(px(56.0))
            .h(px(56.0))
            .rounded(px(999.0))
            .into_any_element()
    } else {
        sepia_avatar_circle("👤", false, 56.0, p)
    };
    let profile_card = section_card("Your profile", None, p).child(
        h_flex()
            .w_full()
            .items_center()
            .gap(px(14.0))
            .child(avatar_el)
            .child(
                v_flex()
                    .flex_1()
                    .gap(px(8.0))
                    .child(Input::new(&props.inputs.user_profile_name))
                    .child(
                        h_flex()
                            .gap(px(6.0))
                            .child(sepia_button(
                                "pref-pick-pic",
                                if pic_exists {
                                    "Change picture"
                                } else {
                                    "Choose picture"
                                },
                                Some(KissIcon::Image),
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| h_pic(SettingsAction::PickProfilePicture, w, cx),
                            ))
                            .children(pic_exists.then(|| {
                                sepia_button(
                                    "pref-rm-pic",
                                    "Remove",
                                    None,
                                    SepiaButtonStyle::Ghost,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_rm_pic(SettingsAction::RemoveProfilePicture, w, cx)
                                    },
                                )
                            })),
                    ),
            ),
    );

    // 2. Spend limit (`SpendLimitCard` — auto-saves on edit, compact single input)
    let spend_card =
        section_card("Spend limit", None, p).child(Input::new(&props.inputs.spend_limit));

    // 3. Appearance & fonts (compact 132px miniature `ThemePreviewCard` tiles matching `ParchmentProfileSettings.kt`)
    let presets = theme_presets();
    let mut theme_grid = h_flex().w_full().flex_wrap().items_stretch().gap(px(8.0));
    for (ix, preset) in presets.iter().enumerate() {
        let pal = ParchmentPalette::from_config(preset);
        let is_sel = preset.name == theme.name;
        let h = on.clone();
        let short_name = preset.name.split('(').next().unwrap_or(&preset.name).trim();
        let mut tile = v_flex()
            .id(("pref-theme-tile", ix))
            .w(px(134.0))
            .p(px(6.0))
            .rounded(px(10.0))
            .bg(p.card_bg.to_hsla())
            .gap(px(6.0))
            .cursor_pointer()
            .on_click(move |_, w, cx| h(SettingsAction::ApplyThemePreset(ix), w, cx));
        tile = if is_sel {
            tile.border_2().border_color(p.accent.to_hsla())
        } else {
            tile.border_1().border_color(p.border.with_alpha(0.75))
        };
        theme_grid = theme_grid.child(kiss_bounce(
            ("pref-theme-kb", ix),
            tile
                // Miniature sidebar + page + `Aa` + accent rule (`ThemePreviewCard`)
                .child(
                    h_flex()
                        .w_full()
                        .h(px(56.0))
                        .rounded(px(6.0))
                        .overflow_hidden()
                        .bg(pal.canvas_bg.to_hsla())
                        .border_1()
                        .border_color(pal.border.with_alpha(0.55))
                        .child(div().w(px(24.0)).h_full().bg(pal.sheet_bg.to_hsla()))
                        .child(
                            v_flex()
                                .flex_1()
                                .p(px(8.0))
                                .gap(px(5.0))
                                .child(
                                    div()
                                        .text_size(px(13.5))
                                        .font_semibold()
                                        .text_color(pal.primary_ink.to_hsla())
                                        .child("Aa"),
                                )
                                .child(
                                    div()
                                        .w(px(40.0))
                                        .h(px(4.0))
                                        .rounded(px(999.0))
                                        .bg(pal.accent.to_hsla()),
                                ),
                        ),
                )
                .child({
                    let mut name_el = div()
                        .w_full()
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(if is_sel {
                            p.accent.to_hsla()
                        } else {
                            p.primary_ink.to_hsla()
                        })
                        .child(short_name.to_string());
                    if is_sel {
                        name_el = name_el.font_semibold();
                    }
                    name_el
                }),
        ));
    }

    let h_red_mot = on.clone();
    let h_hex_tog = on.clone();
    let h_hex_apply = on.clone();
    let h_pf_pick = on.clone();
    let h_pf_clr = on.clone();
    let h_cf_pick = on.clone();
    let h_cf_clr = on.clone();

    let mut appearance_card = section_card("Appearance & fonts", None, p)
        .child(
            div()
                .text_size(px(13.5))
                .font_medium()
                .text_color(p.primary_ink.to_hsla())
                .child("Color mode / preset"),
        )
        .child(theme_grid)
        .child(setting_toggle_row(
            "pref-reduce-motion",
            "Reduce motion",
            "",
            prefs.reduce_motion,
            p,
            move |v, w, cx| h_red_mot(SettingsAction::TogglePref("reduce_motion", *v), w, cx),
        ))
        .child(adaptive_setting_row(
            "Accent color",
            p,
            h_flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .children(
                    ACCENT_SWATCHES
                        .iter()
                        .enumerate()
                        .map(|(ix, (hex, _name))| {
                            let sw_rgb = parse_hex_color(hex, p.accent);
                            let is_sel = theme.accent_hex.eq_ignore_ascii_case(hex);
                            let hex_str = (*hex).to_string();
                            let h = on.clone();
                            let mut dot = div()
                                .id(("pref-accent", ix))
                                .w(px(24.0))
                                .h(px(24.0))
                                .rounded(px(999.0))
                                .bg(sw_rgb.to_hsla())
                                .cursor_pointer()
                                .on_click(move |_, w, cx| {
                                    h(SettingsAction::SetAccentHex(hex_str.clone()), w, cx)
                                });
                            dot = if is_sel {
                                dot.border_2().border_color(p.primary_ink.to_hsla())
                            } else {
                                dot.border_1().border_color(p.border.with_alpha(0.75))
                            };
                            kiss_bounce(("pref-accent-kb", ix), dot)
                        }),
                )
                .child(sepia_button(
                    "pref-hex-tog",
                    if props.show_hex_editor {
                        "Hex ▲"
                    } else {
                        "Hex"
                    },
                    None,
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_hex_tog(SettingsAction::ToggleHexEditor, w, cx),
                )),
        ));

    if props.show_hex_editor {
        appearance_card = appearance_card.child(
            v_flex()
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.elevated_bg.with_alpha(0.5))
                .border_1()
                .border_color(p.subtle_border())
                .gap(px(8.0))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(3.0))
                                .child(field_label("Canvas hex", p))
                                .child(Input::new(&props.inputs.hex_canvas)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(3.0))
                                .child(field_label("Card hex", p))
                                .child(Input::new(&props.inputs.hex_card)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(3.0))
                                .child(field_label("Text hex", p))
                                .child(Input::new(&props.inputs.hex_text)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(3.0))
                                .child(field_label("Accent hex", p))
                                .child(Input::new(&props.inputs.hex_accent)),
                        ),
                )
                .child(h_flex().justify_end().child(sepia_button(
                    "pref-hex-apply",
                    "Apply",
                    Some(KissIcon::Check),
                    SepiaButtonStyle::Primary,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_hex_apply(SettingsAction::ApplyCustomHexPalette, w, cx),
                ))),
        );
    }

    appearance_card = appearance_card
        .child(adaptive_setting_row(
            "Text font",
            p,
            h_flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(TEXT_FONTS.iter().enumerate().map(|(ix, f)| {
                    let sel = theme.prose_font_family.eq_ignore_ascii_case(f);
                    let val = (*f).to_string();
                    let h = on.clone();
                    sepia_chip(("pref-pf", ix), *f, None, sel, p, move |_, w, cx| {
                        h(SettingsAction::SetProseFont(val.clone()), w, cx);
                    })
                }))
                .child(sepia_button(
                    "pref-pf-file",
                    ".TTF / .OTF",
                    None,
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_pf_pick(SettingsAction::PickCustomProseFont, w, cx),
                ))
                .children((!theme.custom_prose_font_path.is_empty()).then(|| {
                    sepia_button(
                        "pref-pf-clr",
                        "Reset",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_pf_clr(SettingsAction::ClearCustomProseFont, w, cx),
                    )
                })),
        ))
        .child(adaptive_setting_row(
            "Code font",
            p,
            h_flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(CODE_FONTS.iter().enumerate().map(|(ix, f)| {
                    let sel = theme.code_font_family.eq_ignore_ascii_case(f);
                    let val = (*f).to_string();
                    let h = on.clone();
                    sepia_chip(("pref-cf", ix), *f, None, sel, p, move |_, w, cx| {
                        h(SettingsAction::SetCodeFont(val.clone()), w, cx);
                    })
                }))
                .child(sepia_button(
                    "pref-cf-file",
                    ".TTF / .OTF",
                    None,
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_cf_pick(SettingsAction::PickCustomCodeFont, w, cx),
                ))
                .children((!theme.custom_code_font_path.is_empty()).then(|| {
                    sepia_button(
                        "pref-cf-clr",
                        "Reset",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_cf_clr(SettingsAction::ClearCustomCodeFont, w, cx),
                    )
                })),
        ))
        .child(
            v_flex()
                .w_full()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(4.0))
                        .child(field_label("Prose font size (sp)", p))
                        .child(
                            div()
                                .font_family(p.code_font_family.clone())
                                .child(Input::new(&props.inputs.prose_font_size)),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(4.0))
                        .child(field_label("Code font size (sp)", p))
                        .child(
                            div()
                                .font_family(p.code_font_family.clone())
                                .child(Input::new(&props.inputs.code_font_size)),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(4.0))
                        .child(field_label("Max reading width (dp)", p))
                        .child(
                            div()
                                .font_family(p.code_font_family.clone())
                                .child(Input::new(&props.inputs.max_reading_width)),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(4.0))
                        .child(field_label("Line height", p))
                        .child(
                            div()
                                .font_family(p.code_font_family.clone())
                                .child(Input::new(&props.inputs.line_height)),
                        ),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(4.0))
                        .child(field_label("Paragraph gap (dp)", p))
                        .child(
                            div()
                                .font_family(p.code_font_family.clone())
                                .child(Input::new(&props.inputs.paragraph_spacing)),
                        ),
                ),
        );
    let h_bub = on.clone();
    let h_page = on.clone();
    let mut display_card = section_card("Chat item display", None, p).child(adaptive_setting_row(
        "Message layout",
        p,
        h_flex()
            .gap(px(6.0))
            .child(sepia_chip(
                "pref-vm-bubbles",
                strings::VIEW_BUBBLES,
                None,
                prefs.default_view_mode == ViewMode::EditorialTurns,
                p,
                move |_, w, cx| h_bub(SettingsAction::SetViewMode(ViewMode::EditorialTurns), w, cx),
            ))
            .child(sepia_chip(
                "pref-vm-page",
                strings::VIEW_PAGE,
                None,
                prefs.default_view_mode == ViewMode::Manuscript,
                p,
                move |_, w, cx| h_page(SettingsAction::SetViewMode(ViewMode::Manuscript), w, cx),
            )),
    ));

    let display_toggles: [(&str, &str, bool); 16] = [
        (
            "show_provider_in_model_capsule",
            "Show provider in top bar",
            prefs.show_provider_in_model_capsule,
        ),
        (
            "show_assistant_avatar_in_title_bar",
            "Show assistant icon in top bar",
            prefs.show_assistant_avatar_in_title_bar,
        ),
        (
            "show_user_avatar",
            "Show your avatar",
            prefs.show_user_avatar,
        ),
        ("show_user_name", "Show your name", prefs.show_user_name),
        (
            "show_user_timestamp",
            "Show your message timestamp",
            prefs.show_user_timestamp,
        ),
        (
            "show_user_message_actions",
            "Show actions under your messages",
            prefs.show_user_message_actions,
        ),
        ("show_model_icon", "Show model icon", prefs.show_model_icon),
        ("show_model_name", "Show model name", prefs.show_model_name),
        (
            "show_model_timestamp",
            "Show reply timestamp",
            prefs.show_model_timestamp,
        ),
        (
            "show_provider_after_model_name",
            "Show provider after model name",
            prefs.show_provider_after_model_name,
        ),
        (
            "show_token_and_context_stats",
            "Show usage stats",
            prefs.show_token_and_context_stats,
        ),
        (
            "show_tokens_for_entire_turn",
            "Count usage for the whole turn",
            prefs.show_tokens_for_entire_turn,
        ),
        (
            "show_thinking_cards",
            "Show thinking cards",
            prefs.show_thinking_cards,
        ),
        ("show_tool_cards", "Show tool cards", prefs.show_tool_cards),
        (
            "show_files_below_replies",
            "Show files below replies",
            prefs.show_files_below_replies,
        ),
        (
            "show_reasoning_level_on_button",
            "Show thinking level on button",
            prefs.show_reasoning_level_on_button,
        ),
    ];
    let mut disp_list = v_flex().w_full().gap(px(2.0));
    for (ix, (k, t, ch)) in display_toggles.into_iter().enumerate() {
        let h = on.clone();
        disp_list = disp_list.child(setting_toggle_row(
            ("pref-disp", ix),
            t,
            "",
            ch,
            p,
            move |val, w, cx| h(SettingsAction::TogglePref(k, *val), w, cx),
        ));
    }
    display_card = display_card.child(disp_list);

    // 5. Rendering settings (ALL 9 Kotlin toggles, zero redundant subtitles)
    let mut render_card = section_card("Rendering settings", None, p);
    let render_toggles: [(&str, &str, bool); 9] = [
        (
            "inline_dollar_math_rendering",
            "Inline $…$ math rendering",
            prefs.inline_dollar_math_rendering,
        ),
        (
            "math_formula_rendering",
            "Math formula rendering",
            prefs.math_formula_rendering,
        ),
        (
            "render_user_messages_with_markdown",
            "Render your messages with Markdown",
            prefs.render_user_messages_with_markdown,
        ),
        (
            "render_reasoning_with_markdown",
            "Format thinking text with Markdown",
            prefs.render_reasoning_with_markdown,
        ),
        (
            "render_assistant_messages_with_markdown",
            "Render assistant messages with Markdown",
            prefs.render_assistant_messages_with_markdown,
        ),
        (
            "highlight_dialogue_and_narration",
            "Highlight spoken dialogue & narration",
            theme.highlight_dialogue_and_narration,
        ),
        (
            "auto_collapse_code_blocks",
            "Auto-collapse code blocks",
            prefs.auto_collapse_code_blocks,
        ),
        (
            "wrap_code_blocks",
            "Wrap code blocks",
            prefs.wrap_code_blocks,
        ),
        (
            "collapse_long_messages",
            "Collapse long messages (1,800+ chars)",
            prefs.collapse_long_messages,
        ),
    ];
    let mut rend_list = v_flex().w_full().gap(px(2.0));
    for (ix, (k, t, ch)) in render_toggles.into_iter().enumerate() {
        let h = on.clone();
        rend_list = rend_list.child(setting_toggle_row(
            ("pref-rend", ix),
            t,
            "",
            ch,
            p,
            move |val, w, cx| h(SettingsAction::TogglePref(k, *val), w, cx),
        ));
    }
    render_card = render_card.child(rend_list);

    // 6. Behavior & startup (ALL 17 toggles + single-line Minimap & Send shortcut rows)
    let h_nav1 = on.clone();
    let h_nav2 = on.clone();
    let h_nav3 = on.clone();
    let mut behavior_card = section_card("Behavior & startup", None, p);

    let beh_toggles: [(&str, &str, bool); 16] = [
        (
            "auto_switch_to_topics",
            "Auto-switch to Chats",
            prefs.auto_switch_to_topics,
        ),
        (
            "auto_collapse_thinking",
            "Auto-collapse thinking",
            prefs.auto_collapse_thinking,
        ),
        (
            "collapse_thinking_steps",
            "Collapse intermediate thinking steps",
            prefs.collapse_thinking_steps,
        ),
        (
            "show_tool_result_summary",
            "Show tool result summary",
            prefs.show_tool_result_summary,
        ),
        (
            "delete_messages_below_when_regenerating",
            "Delete messages below when regenerating",
            prefs.delete_messages_below_when_regenerating,
        ),
        (
            "delete_previous_message_when_regenerating",
            "Replace previous reply when regenerating",
            prefs.delete_previous_message_when_regenerating,
        ),
        (
            "confirm_before_regenerating",
            "Confirm before regenerating",
            prefs.confirm_before_regenerating,
        ),
        (
            "confirm_before_deleting_chat",
            "Confirm before deleting or clearing",
            prefs.confirm_before_deleting_chat,
        ),
        (
            "keep_message_versions_when_forking",
            "Keep message versions when branching",
            prefs.keep_message_versions_when_forking,
        ),
        (
            "show_chat_list_dates",
            "Show chat list dates",
            prefs.show_chat_list_dates,
        ),
        (
            "new_chat_when_switching_assistants",
            "New chat when switching assistants",
            prefs.new_chat_when_switching_assistants,
        ),
        (
            "new_chat_after_deleting_topic",
            "New chat after deleting topic",
            prefs.new_chat_after_deleting_topic,
        ),
        (
            "new_chat_on_launch",
            "New chat on launch",
            prefs.new_chat_on_launch,
        ),
        (
            "auto_scroll_to_bottom_on_open",
            "Auto-scroll to bottom on open",
            prefs.auto_scroll_to_bottom_on_open,
        ),
        (
            "auto_follow_bottom",
            "Auto-follow bottom while streaming",
            prefs.auto_follow_bottom,
        ),
        (
            "auto_compact_when_near_context_limit",
            "Shorten conversation automatically near limit",
            prefs.auto_compact_when_near_context_limit,
        ),
    ];
    let mut beh_list = v_flex().w_full().gap(px(2.0));
    for (ix, (k, t, ch)) in beh_toggles.into_iter().enumerate() {
        let h = on.clone();
        beh_list = beh_list.child(setting_toggle_row(
            ("pref-beh", ix),
            t,
            "",
            ch,
            p,
            move |val, w, cx| h(SettingsAction::TogglePref(k, *val), w, cx),
        ));
    }
    behavior_card = behavior_card.child(beh_list);

    let h_paste = on.clone();
    behavior_card = behavior_card
        .child(adaptive_setting_row(
            "Message navigation minimap",
            p,
            h_flex()
                .gap(px(6.0))
                .child(sepia_chip(
                    "nav-always",
                    "Always",
                    None,
                    prefs.message_nav_buttons == MessageNavVisibility::Always,
                    p,
                    move |_, w, cx| {
                        h_nav1(
                            SettingsAction::SetMessageNavVisibility(MessageNavVisibility::Always),
                            w,
                            cx,
                        )
                    },
                ))
                .child(sepia_chip(
                    "nav-scroll",
                    "While scrolling",
                    None,
                    prefs.message_nav_buttons == MessageNavVisibility::WhileScrolling,
                    p,
                    move |_, w, cx| {
                        h_nav2(
                            SettingsAction::SetMessageNavVisibility(
                                MessageNavVisibility::WhileScrolling,
                            ),
                            w,
                            cx,
                        )
                    },
                ))
                .child(sepia_chip(
                    "nav-hidden",
                    "Hidden",
                    None,
                    prefs.message_nav_buttons == MessageNavVisibility::Hidden,
                    p,
                    move |_, w, cx| {
                        h_nav3(
                            SettingsAction::SetMessageNavVisibility(MessageNavVisibility::Hidden),
                            w,
                            cx,
                        )
                    },
                )),
        ))
        .child(setting_toggle_row(
            "pref-paste-file",
            "Paste long text as file",
            "",
            prefs.paste_long_text_as_file,
            p,
            move |val, w, cx| {
                h_paste(
                    SettingsAction::TogglePref("paste_long_text_as_file", *val),
                    w,
                    cx,
                )
            },
        ));

    if prefs.paste_long_text_as_file {
        behavior_card = behavior_card.child(
            v_flex()
                .w_full()
                .gap(px(6.0))
                .child(adaptive_setting_row(
                    "Conversion threshold",
                    p,
                    h_flex().gap(px(6.0)).children(
                        [2000, 5000, 10000, 20000]
                            .into_iter()
                            .enumerate()
                            .map(|(ix, lim)| {
                                let h = on.clone();
                                sepia_chip(
                                    ("paste-lim", ix),
                                    format!("{lim}"),
                                    None,
                                    prefs.paste_conversion_threshold_chars == lim,
                                    p,
                                    move |_, w, cx| {
                                        h(SettingsAction::SetPasteThreshold(lim), w, cx)
                                    },
                                )
                            }),
                    ),
                ))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(8.0))
                        .child(field_label("Custom threshold (chars)", p))
                        .child(
                            div()
                                .w(px(140.0))
                                .child(Input::new(&props.inputs.paste_threshold)),
                        ),
                ),
        );
    }
    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(profile_card)
        .child(spend_card)
        .child(appearance_card)
        .child(display_card)
        .child(render_card)
        .child(behavior_card)
        .into_any_element()
}

// ============================================================================================
// Page 2: Providers (`SettingsProvidersPage.kt` 2-column vertical list + detail pane)
// ============================================================================================
