use super::*;

pub(super) fn render_providers_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;
    let active_prof = props
        .snapshot
        .profiles
        .iter()
        .find(|pr| pr.id == props.snapshot.active_profile_id)
        .or_else(|| props.snapshot.profiles.first());

    let Some(prof) = active_prof else {
        return div()
            .p(px(16.0))
            .child(section_card("No providers configured", None, p))
            .into_any_element();
    };

    let h_add_oai = on.clone();
    let h_add_ant = on.clone();
    let h_add_gem = on.clone();
    let h_add_cus = on.clone();

    // Left vertical provider list rail (`SettingsProvidersPage.kt:137-252`)
    let prof_list_h = ((props.snapshot.profiles.len() as f32) * 48.0).clamp(48.0, 420.0);
    let mut prof_list = v_flex()
        .id("prov-rail-scroll")
        .h(px(prof_list_h))
        .w_full()
        .gap(px(5.0))
        .overflow_y_scrollbar();
    for (ix, pr) in props.snapshot.profiles.iter().enumerate() {
        let is_sel = pr.id == prof.id;
        let is_on = pr.enabled
            && (pr.has_any_api_key() || pr.base_url.trim() != pr.protocol.default_base_url());
        let pid = pr.id.clone();
        let h_sel = on.clone();
        let hover_bg = p.hover_tint();
        let mut row = h_flex()
            .id(("prov-card", ix))
            .relative()
            .overflow_hidden()
            .w_full()
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(10.0))
            .bg(if is_sel {
                p.card_bg.to_hsla()
            } else {
                p.canvas_bg.with_alpha(0.0)
            })
            .border_1()
            .border_color(if is_sel {
                p.subtle_border()
            } else {
                p.canvas_bg.with_alpha(0.0)
            })
            .items_center()
            .justify_between()
            .gap(px(6.0))
            .cursor_pointer()
            .hover(move |s| {
                s.bg(if is_sel {
                    p.card_bg.to_hsla()
                } else {
                    hover_bg
                })
            })
            .on_click(move |_, w, cx| h_sel(SettingsAction::SelectProfile(pid.clone()), w, cx));
        if is_sel {
            row = row.child(rubric_mark_overlay(7.0, p));
        }
        prof_list = prof_list.child(kiss_bounce_subtle(
            ("prov-rail-kb", ix),
            row.child(
                h_flex()
                    .flex_1()
                    .overflow_hidden()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(px(26.0))
                            .h(px(26.0))
                            .rounded(px(999.0))
                            .bg(if is_sel {
                                p.accent.with_alpha(0.16)
                            } else {
                                p.elevated_bg.to_hsla()
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .child(model_lab_icon(
                                if pr.default_model.trim().is_empty() {
                                    &pr.name
                                } else {
                                    &pr.default_model
                                },
                                14.0,
                                if is_sel {
                                    p.accent.to_hsla()
                                } else {
                                    p.narration_ink.to_hsla()
                                },
                                pr.protocol.as_db_str(),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(13.0))
                            .font_medium()
                            .text_color(if is_sel {
                                p.accent.to_hsla()
                            } else {
                                p.primary_ink.to_hsla()
                            })
                            .child(pr.name.clone()),
                    ),
            )
            .child(status_pill(is_on, p)),
        ));
    }

    let left_rail = v_flex()
        .w(px(228.0))
        .h_full()
        .flex_shrink_0()
        .bg(p.sheet_bg.with_alpha(0.42))
        .p(px(12.0))
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(15.0))
                .font_semibold()
                .text_color(p.primary_ink.to_hsla())
                .child("Providers"),
        )
        .child(prof_list)
        .child(h_divider(p))
        .child(
            v_flex()
                .w_full()
                .gap(px(4.0))
                .child(
                    sepia_button(
                        "add-oai",
                        "+ OpenAI",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_add_oai(
                                SettingsAction::AddProfile(ProviderProtocol::OpenAiResponses),
                                w,
                                cx,
                            )
                        },
                    )
                    .w_full()
                    .justify_start(),
                )
                .child(
                    sepia_button(
                        "add-ant",
                        "+ Anthropic",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_add_ant(
                                SettingsAction::AddProfile(ProviderProtocol::AnthropicMessages),
                                w,
                                cx,
                            )
                        },
                    )
                    .w_full()
                    .justify_start(),
                )
                .child(
                    sepia_button(
                        "add-gem",
                        "+ Gemini",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_add_gem(
                                SettingsAction::AddProfile(ProviderProtocol::GeminiInteractions),
                                w,
                                cx,
                            )
                        },
                    )
                    .w_full()
                    .justify_start(),
                )
                .child(
                    sepia_button(
                        "add-cus",
                        "+ Custom",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_add_cus(SettingsAction::AddProfile(ProviderProtocol::Custom), w, cx)
                        },
                    )
                    .w_full()
                    .justify_start(),
                ),
        );

    let right_body: AnyElement = match props.providers_subpage {
        ProvidersSubpage::Advanced => render_provider_advanced_subpage(props, prof),
        ProvidersSubpage::ModelConfig(model_id) => {
            render_model_config_subpage(props, prof, model_id)
        }
        ProvidersSubpage::Main => {
            let h_en = on.clone();
            let h_test = on.clone();
            let h_adv = on.clone();
            let h_del = on.clone();
            let h_eye = on.clone();
            let h_def_url = on.clone();
            let h_clr_url = on.clone();
            let h_fetch = on.clone();
            let h_add_box = on.clone();
            let h_add_man = on.clone();
            let h_clr_mods = on.clone();
            let pid_del = prof.id.clone();
            let mut detail_card = section_card_with_action(
                &prof.name,
                None,
                Some(
                    h_flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(sepia_toggle_switch(
                            "prov-enabled-sw",
                            prof.enabled,
                            p,
                            move |v, w, cx| {
                                h_en(SettingsAction::ToggleProviderFlag("enabled", *v), w, cx);
                            },
                        ))
                        .child(sepia_button(
                            "prov-test",
                            "Test",
                            Some(KissIcon::HeartPulse),
                            SepiaButtonStyle::Outline,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_test(SettingsAction::TestProviderConnection, w, cx),
                        ))
                        .child(sepia_button(
                            "prov-adv",
                            "Advanced",
                            Some(KissIcon::Settings),
                            SepiaButtonStyle::Outline,
                            true,
                            false,
                            p,
                            move |_, w, cx| {
                                h_adv(
                                    SettingsAction::OpenProviderSubpage(ProvidersSubpage::Advanced),
                                    w,
                                    cx,
                                )
                            },
                        ))
                        .children((props.snapshot.profiles.len() > 1).then(|| {
                            icon_button("prov-del", KissIcon::Trash, false, p, move |_, w, cx| {
                                h_del(SettingsAction::DeleteProfile(pid_del.clone()), w, cx)
                            })
                        }))
                        .into_any_element(),
                ),
                p,
            );

            if !props.provider_test_status.is_empty() {
                detail_card = detail_card.child(
                    div()
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(8.0))
                        .bg(p.elevated_bg.to_hsla())
                        .text_size(px(12.0))
                        .text_color(p.accent.to_hsla())
                        .child(props.provider_test_status.to_string()),
                );
            }

            detail_card = detail_card
                .child(
                    h_flex()
                        .items_end()
                        .gap(px(10.0))
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(4.0))
                                .child(
                                    h_flex()
                                        .h(px(26.0))
                                        .items_center()
                                        .child(field_label("Profile name", p)),
                                )
                                .child(Input::new(&props.inputs.provider_name)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap(px(4.0))
                                .child({
                                    let h_add_k_tog = on.clone();
                                    let key_count = prof.all_api_keys().len();
                                    let key_lbl = if key_count > 1 {
                                        format!("API keys ({key_count})")
                                    } else {
                                        "API key".to_string()
                                    };
                                    h_flex()
                                        .w_full()
                                        .h(px(26.0))
                                        .justify_between()
                                        .items_center()
                                        .child(field_label(&key_lbl, p))
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .child(
                                                    sepia_button(
                                                        "prov-add-key-tog",
                                                        "Add key",
                                                        Some(KissIcon::Plus),
                                                        SepiaButtonStyle::Ghost,
                                                        true,
                                                        false,
                                                        p,
                                                        move |_, w, cx| {
                                                            h_add_k_tog(
                                                                SettingsAction::ToggleAddApiKeyBox,
                                                                w,
                                                                cx,
                                                            )
                                                        },
                                                    )
                                                    .h(px(26.0))
                                                    .px(px(6.0)),
                                                )
                                                .child(
                                                    icon_button(
                                                        "prov-key-eye",
                                                        if props.show_api_key {
                                                            KissIcon::EyeOff
                                                        } else {
                                                            KissIcon::Eye
                                                        },
                                                        props.show_api_key,
                                                        p,
                                                        move |_, w, cx| {
                                                            h_eye(
                                                                SettingsAction::ToggleShowApiKey,
                                                                w,
                                                                cx,
                                                            )
                                                        },
                                                    )
                                                    .h(px(26.0))
                                                    .px(px(6.0)),
                                                ),
                                        )
                                })
                                .child(Input::new(&props.inputs.api_key)),
                        ),
                )
                .children(
                    (!prof.extra_api_keys.is_empty() || props.show_add_api_key_box).then(|| {
                        let h_add_k = on.clone();
                        let mut keys_box = v_flex()
                            .w_full()
                            .p(px(10.0))
                            .rounded(px(8.0))
                            .bg(p.elevated_bg.with_alpha(0.40))
                            .border_1()
                            .border_color(p.subtle_border())
                            .gap(px(6.0));
                        if !prof.extra_api_keys.is_empty() {
                            keys_box = keys_box.child(field_label(
                                &format!(
                                    "Additional keys ({} — auto-rotates & fails over on 429)",
                                    prof.extra_api_keys.len()
                                ),
                                p,
                            ));
                            for (k_ix, k_val) in prof.extra_api_keys.iter().enumerate() {
                                let h_prom = on.clone();
                                let h_rm = on.clone();
                                let display = if props.show_api_key {
                                    k_val.clone()
                                } else {
                                    let chars: Vec<char> = k_val.chars().collect();
                                    if chars.len() <= 8 {
                                        "••••••••".to_string()
                                    } else {
                                        let head: String = chars.iter().take(4).collect();
                                        let tail: String =
                                            chars.iter().skip(chars.len() - 4).collect();
                                        format!("{head}••••{tail}")
                                    }
                                };
                                keys_box = keys_box.child(
                                    h_flex()
                                        .id(("prov-extra-key", k_ix))
                                        .w_full()
                                        .px(px(8.0))
                                        .py(px(5.0))
                                        .rounded(px(6.0))
                                        .bg(p.card_bg.to_hsla())
                                        .border_1()
                                        .border_color(p.subtle_border())
                                        .justify_between()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            h_flex()
                                                .flex_1()
                                                .overflow_hidden()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(
                                                    div()
                                                        .px(px(6.0))
                                                        .py(px(1.5))
                                                        .rounded(px(4.0))
                                                        .bg(p.accent.with_alpha(0.14))
                                                        .text_size(px(11.0))
                                                        .font_semibold()
                                                        .text_color(p.accent.to_hsla())
                                                        .child(format!("#{}", k_ix + 2)),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .truncate()
                                                        .font_family(p.code_font_family.clone())
                                                        .text_size(px(12.0))
                                                        .text_color(p.primary_ink.to_hsla())
                                                        .child(display),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .child(sepia_button(
                                                    ("prov-key-prom", k_ix),
                                                    "Primary",
                                                    None,
                                                    SepiaButtonStyle::Ghost,
                                                    true,
                                                    false,
                                                    p,
                                                    move |_, w, cx| {
                                                        h_prom(
                                                            SettingsAction::PromoteExtraApiKey(
                                                                k_ix,
                                                            ),
                                                            w,
                                                            cx,
                                                        )
                                                    },
                                                ))
                                                .child(icon_button(
                                                    ("prov-key-rm", k_ix),
                                                    KissIcon::Close,
                                                    false,
                                                    p,
                                                    move |_, w, cx| {
                                                        h_rm(
                                                            SettingsAction::RemoveExtraApiKey(k_ix),
                                                            w,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        ),
                                );
                            }
                        }
                        keys_box.child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(Input::new(&props.inputs.extra_api_key_add)),
                                )
                                .child(sepia_button(
                                    "prov-extra-key-add-btn",
                                    "Add key",
                                    Some(KissIcon::Plus),
                                    SepiaButtonStyle::Primary,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| h_add_k(SettingsAction::AddExtraApiKey, w, cx),
                                )),
                        )
                    }),
                )
                .child(
                    v_flex()
                        .gap(px(4.0))
                        .child(
                            h_flex()
                                .justify_between()
                                .items_center()
                                .child(field_label(strings::SERVER_URL_LABEL, p))
                                .child(
                                    h_flex()
                                        .gap(px(4.0))
                                        .child(sepia_button(
                                            "prov-def-url",
                                            "Default URL",
                                            None,
                                            SepiaButtonStyle::Ghost,
                                            true,
                                            false,
                                            p,
                                            move |_, w, cx| {
                                                h_def_url(
                                                    SettingsAction::ResetProviderDefaultUrl,
                                                    w,
                                                    cx,
                                                )
                                            },
                                        ))
                                        .child(sepia_button(
                                            "prov-clr-url",
                                            "Clear URL",
                                            None,
                                            SepiaButtonStyle::Ghost,
                                            true,
                                            false,
                                            p,
                                            move |_, w, cx| {
                                                h_clr_url(SettingsAction::ClearProviderUrl, w, cx)
                                            },
                                        )),
                                ),
                        )
                        .child(Input::new(&props.inputs.base_url)),
                );

            if prof.protocol == ProviderProtocol::Custom {
                let cur_d = prof.resolve_wire_dialect();
                let mut fmt_col = v_flex()
                    .w_full()
                    .gap(px(2.0))
                    .child(field_label("API format (required for Custom)", p));
                for (ix, (code, label, desc)) in [
                    (
                        CUSTOM_DIALECT_OPENAI_RESPONSES,
                        "OpenAI Responses API",
                        "Modern /v1/responses streaming format",
                    ),
                    (
                        CUSTOM_DIALECT_OPENAI_CHAT,
                        "OpenAI Chat Completions",
                        "Compatible with OpenRouter, Ollama, LM Studio, DeepSeek, etc.",
                    ),
                    (
                        CUSTOM_DIALECT_ANTHROPIC,
                        "Anthropic Messages API",
                        "Compatible with Claude & Anthropic-format proxies",
                    ),
                    (
                        CUSTOM_DIALECT_GEMINI,
                        "Google Gemini API",
                        "Compatible with Gemini generateContent / interactions endpoints",
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let h = on.clone();
                    let c = code.to_string();
                    fmt_col = fmt_col.child(permission_option_row(
                        ("prov-dial", ix),
                        label,
                        desc,
                        cur_d == code,
                        true,
                        p,
                        move |_, w, cx| h(SettingsAction::SetCustomDialect(c.clone()), w, cx),
                    ));
                }
                detail_card = detail_card.child(fmt_col);
            }

            detail_card = detail_card.child(
                v_flex()
                    .w_full()
                    .gap(px(4.0))
                    .child(field_label("Default model ID", p))
                    .child(Input::new(&props.inputs.default_model)),
            );

            if !props.models_fetch_status.is_empty() {
                detail_card = detail_card.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(p.muted_ink.to_hsla())
                        .child(props.models_fetch_status.to_string()),
                );
            }

            // Searchable vertical Models list (`SettingsProvidersPage.kt` — search + vertical list)
            let mq = props.model_filter_query.trim().to_ascii_lowercase();
            let visible_models: Vec<&String> = prof
                .saved_models
                .iter()
                .filter(|m| mq.is_empty() || m.to_ascii_lowercase().contains(&mq))
                .take(120)
                .collect();

            let mut models_card = section_card_with_action(
                &format!("Models ({})", prof.saved_models.len()),
                None,
                Some(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(icon_button(
                            "prov-add-model-tog",
                            KissIcon::Plus,
                            props.show_add_model_box,
                            p,
                            move |_, w, cx| h_add_box(SettingsAction::ToggleAddModelBox, w, cx),
                        ))
                        .children((!prof.saved_models.is_empty()).then(|| {
                            icon_button(
                                "prov-clear-models",
                                KissIcon::Trash,
                                false,
                                p,
                                move |_, w, cx| h_clr_mods(SettingsAction::ClearSavedModels, w, cx),
                            )
                        }))
                        .child(sepia_button(
                            "prov-fetch",
                            if props.is_fetching_models {
                                "Fetching…"
                            } else {
                                "Fetch"
                            },
                            Some(KissIcon::Refresh),
                            SepiaButtonStyle::Primary,
                            true,
                            props.is_fetching_models,
                            p,
                            move |_, w, cx| h_fetch(SettingsAction::FetchProviderModels, w, cx),
                        ))
                        .into_any_element(),
                ),
                p,
            );
            if props.show_add_model_box || prof.saved_models.is_empty() {
                models_card = models_card.child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&props.inputs.manual_model_add)),
                        )
                        .child(sepia_button(
                            "prov-add-model-btn",
                            "Add & select",
                            Some(KissIcon::Plus),
                            SepiaButtonStyle::Primary,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_add_man(SettingsAction::AddManualModel, w, cx),
                        )),
                );
            }
            if !prof.saved_models.is_empty() {
                models_card =
                    models_card.child(Input::new(&props.inputs.model_filter).prefix(kiss_icon(
                        KissIcon::Search,
                        13.0,
                        p.muted_ink.to_hsla(),
                    )));
            }
            let mut m_list = v_flex()
                .id("prov-models-scroll")
                .max_h(px(340.0))
                .w_full()
                .gap(px(4.0))
                .overflow_y_scrollbar();

            if visible_models.is_empty() {
                m_list = m_list.child(
                    div()
                        .p(px(8.0))
                        .text_size(px(12.5))
                        .text_color(p.muted_ink.to_hsla())
                        .child(if prof.saved_models.is_empty() {
                            "No models yet. Add a model ID above or click Fetch."
                        } else {
                            "No models match the filter."
                        }),
                );
            } else {
                let hover_bg = p.hover_tint();
                for (ix, mid) in visible_models.into_iter().enumerate() {
                    let is_def = prof.default_model == *mid;
                    let mid_sel = mid.clone();
                    let mid_cfg = mid.clone();
                    let mid_rm = mid.clone();
                    let h_sel = on.clone();
                    let h_cfg = on.clone();
                    let h_rm = on.clone();
                    let spec = prof.custom_model_specs.get(mid);
                    let ctx_tok = spec
                        .map(|s| s.context_window_tokens)
                        .filter(|&v| v > 0)
                        .unwrap_or(prof.context_window_tokens);
                    let badge = primary_model_privacy_badge(prof, mid, spec);
                    let mut row = h_flex()
                        .id(("prov-m-row", ix))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(8.0))
                        .bg(if is_def {
                            p.accent.with_alpha(0.11)
                        } else {
                            p.elevated_bg.with_alpha(0.45)
                        })
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, w, cx| {
                            h_sel(SettingsAction::SelectSavedModel(mid_sel.clone()), w, cx)
                        });
                    if is_def {
                        row = row.child(rubric_mark_overlay(6.0, p));
                    }
                    m_list = m_list.child(
                        row.child(
                            h_flex()
                                .flex_1()
                                .overflow_hidden()
                                .items_center()
                                .gap(px(8.0))
                                .child(model_lab_icon(
                                    mid,
                                    15.0,
                                    if is_def {
                                        p.accent.to_hsla()
                                    } else {
                                        p.narration_ink.to_hsla()
                                    },
                                    prof.protocol.as_db_str(),
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .font_family(p.code_font_family.clone())
                                        .text_size(px(12.5))
                                        .text_color(if is_def {
                                            p.accent.to_hsla()
                                        } else {
                                            p.primary_ink.to_hsla()
                                        })
                                        .child(mid.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(5.0))
                                .flex_shrink_0()
                                .children(badge.map(|b| model_privacy_badge_chip(&b, p)))
                                .children((ctx_tok > 0).then(|| {
                                    div()
                                        .px(px(5.0))
                                        .py(px(2.0))
                                        .rounded(px(5.0))
                                        .bg(p.sheet_bg.to_hsla())
                                        .font_family(p.code_font_family.clone())
                                        .text_size(px(10.5))
                                        .text_color(p.muted_ink.to_hsla())
                                        .child(format_tokens_short(ctx_tok as i64))
                                }))
                                .child(icon_button(
                                    ("prov-m-cfg", ix),
                                    KissIcon::Settings,
                                    spec.is_some(),
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_cfg(
                                            SettingsAction::OpenProviderSubpage(
                                                ProvidersSubpage::ModelConfig(mid_cfg.clone()),
                                            ),
                                            w,
                                            cx,
                                        );
                                    },
                                ))
                                .child(icon_button(
                                    ("prov-m-rm", ix),
                                    KissIcon::Close,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        cx.stop_propagation();
                                        h_rm(
                                            SettingsAction::RemoveSavedModel(mid_rm.clone()),
                                            w,
                                            cx,
                                        );
                                    },
                                )),
                        ),
                    );
                }
            }
            models_card = models_card.child(m_list);

            v_flex()
                .w_full()
                .gap(px(14.0))
                .child(detail_card)
                .child(models_card)
                .into_any_element()
        }
    };

    h_flex()
        .size_full()
        .child(left_rail)
        .child(v_divider(p))
        .child({
            let sub_key = match props.providers_subpage {
                ProvidersSubpage::Main => 0usize,
                ProvidersSubpage::Advanced => 1usize,
                ProvidersSubpage::ModelConfig(_) => 2usize,
            };
            div()
                .id(("prov-pane-wrap", sub_key))
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(
                    div()
                        .id(("prov-detail-scroll", sub_key))
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(
                            v_flex()
                                .w_full()
                                .p(px(16.0))
                                .pb(px(36.0))
                                .gap(px(14.0))
                                .child(right_body),
                        ),
                )
        })
        .into_any_element()
}
