use super::*;

pub(super) fn render_provider_advanced_subpage(
    props: &SettingsRenderProps<'_>,
    prof: &ProviderProfile,
) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;
    let h_back = on.clone();
    let h_ws = on.clone();
    let h_ce = on.clone();
    let h_uc = on.clone();
    let h_af = on.clone();
    let h_store = on.clone();
    let h_chain = on.clone();
    let h_bg = on.clone();
    let h_leg = on.clone();

    let header = h_flex()
        .w_full()
        .items_center()
        .gap(px(10.0))
        .child(sepia_button(
            "prov-adv-back",
            "Back",
            Some(KissIcon::ArrowLeft),
            SepiaButtonStyle::Outline,
            true,
            false,
            p,
            move |_, w, cx| {
                h_back(
                    SettingsAction::OpenProviderSubpage(ProvidersSubpage::Main),
                    w,
                    cx,
                )
            },
        ))
        .child(
            div()
                .text_size(px(18.0))
                .font_semibold()
                .text_color(p.primary_ink.to_hsla())
                .child(format!("{} — Advanced settings", prof.name)),
        );
    let mut proto_group = v_flex()
        .w_full()
        .gap(px(0.0))
        .child(div().pb(px(2.0)).child(field_label("Protocol", p)));
    for (ix, (proto, label)) in [
        (ProviderProtocol::OpenAiResponses, "OpenAI Responses API"),
        (
            ProviderProtocol::AnthropicMessages,
            "Anthropic Messages API",
        ),
        (ProviderProtocol::GeminiInteractions, "Google Gemini API"),
        (ProviderProtocol::Custom, "Custom / OpenAI-compatible"),
    ]
    .into_iter()
    .enumerate()
    {
        let h = on.clone();
        proto_group = proto_group.child(permission_option_row(
            ("adv-proto", ix),
            label,
            "",
            prof.protocol == proto,
            true,
            p,
            move |_, w, cx| h(SettingsAction::SetProfileProtocol(proto), w, cx),
        ));
    }

    let mut auth_group = v_flex()
        .w_full()
        .gap(px(0.0))
        .child(div().pb(px(2.0)).child(field_label("Auth header mode", p)));
    for (ix, mode) in [
        AuthHeaderMode::Native,
        AuthHeaderMode::BearerOnly,
        AuthHeaderMode::QueryParam,
    ]
    .into_iter()
    .enumerate()
    {
        let h = on.clone();
        auth_group = auth_group.child(permission_option_row(
            ("auth-m", ix),
            auth_mode_label(mode),
            "",
            prof.auth_header_mode == mode,
            true,
            p,
            move |_, w, cx| h(SettingsAction::SetAuthHeaderMode(mode), w, cx),
        ));
    }

    let mut auth_card = section_card("Protocol, authentication & endpoint", None, p)
        .child(proto_group)
        .child(field_label("Endpoint path override (optional)", p))
        .child(Input::new(&props.inputs.endpoint_override));

    if prof.protocol == ProviderProtocol::Custom {
        let cur_d = prof.resolve_wire_dialect();
        let mut dial_group = v_flex()
            .w_full()
            .gap(px(0.0))
            .child(div().pb(px(2.0)).child(field_label("API format", p)));
        for (ix, (code, label)) in [
            (CUSTOM_DIALECT_OPENAI_RESPONSES, "OpenAI Responses API"),
            (CUSTOM_DIALECT_OPENAI_CHAT, "OpenAI Chat Completions"),
            (CUSTOM_DIALECT_ANTHROPIC, "Anthropic Messages API"),
            (CUSTOM_DIALECT_GEMINI, "Google Gemini API"),
        ]
        .into_iter()
        .enumerate()
        {
            let h = on.clone();
            let c = code.to_string();
            dial_group = dial_group.child(permission_option_row(
                ("adv-dial", ix),
                label,
                "",
                cur_d == code,
                true,
                p,
                move |_, w, cx| h(SettingsAction::SetCustomDialect(c.clone()), w, cx),
            ));
        }
        auth_card = auth_card.child(dial_group);
    }

    auth_card = auth_card.child(h_divider(p)).child(auth_group);

    const ADV_THK_STOPS: [(&str, &str); 6] = [
        ("Off", "none"),
        ("Low", "low"),
        ("Medium", "medium"),
        ("High", "high"),
        ("Extra", "xhigh"),
        ("Max", "max"),
    ];
    let cur_eff = prof.thinking_effort.trim().to_ascii_lowercase();
    let sel_stop = ADV_THK_STOPS
        .iter()
        .position(|(_, v)| (cur_eff.is_empty() && *v == "none") || cur_eff == *v);
    let eff_lbl = match cur_eff.as_str() {
        "" | "none" => "Off",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra",
        "max" => "Max",
        "adaptive" => "Adaptive",
        "auto" => "Automatic",
        _ => prof.thinking_effort.trim(),
    };
    let h_sl = on.clone();
    let h_ad = on.clone();
    let h_au = on.clone();
    let sampling_card = section_card("Reply settings, sampling & request overrides", None, p)
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(field_label("Default thinking effort", p))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_semibold()
                        .text_color(p.accent.to_hsla())
                        .child(eff_lbl.to_string()),
                ),
        )
        .child(step_slider(
            "adv-thk-slider",
            ADV_THK_STOPS.len(),
            sel_stop,
            p,
            move |ix, w, cx| {
                if let Some((_, val)) = ADV_THK_STOPS.get(ix) {
                    h_sl(SettingsAction::SetThinkingEffort((*val).to_string()), w, cx);
                }
            },
        ))
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(p.muted_ink.to_hsla())
                        .child("Faster"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(p.muted_ink.to_hsla())
                        .child("More thorough"),
                ),
        )
        .child(
            h_flex()
                .flex_wrap()
                .gap(px(6.0))
                .child(sepia_chip(
                    "adv-thk-ad",
                    "Adaptive",
                    None,
                    cur_eff == "adaptive",
                    p,
                    move |_, w, cx| {
                        h_ad(SettingsAction::SetThinkingEffort("adaptive".into()), w, cx)
                    },
                ))
                .child(sepia_chip(
                    "adv-thk-au",
                    "Automatic",
                    None,
                    cur_eff == "auto",
                    p,
                    move |_, w, cx| h_au(SettingsAction::SetThinkingEffort("auto".into()), w, cx),
                )),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Temperature (blank = default)", p))
                        .child(Input::new(&props.inputs.temperature)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Top-P (blank = default)", p))
                        .child(Input::new(&props.inputs.top_p)),
                ),
        )
        .child({
            let show_legacy_budget = prof.protocol == ProviderProtocol::AnthropicMessages
                || (prof.protocol == ProviderProtocol::Custom
                    && prof.resolve_wire_dialect() == CUSTOM_DIALECT_ANTHROPIC);
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Max output tokens (0 = unlimited)", p))
                        .child(Input::new(&props.inputs.max_output_tokens)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Context window", p))
                        .child(Input::new(&props.inputs.context_window)),
                )
                .children(show_legacy_budget.then(|| {
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Legacy thinking budget", p))
                        .child(Input::new(&props.inputs.thinking_budget_tokens))
                }))
        })
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Default input $/1M", p))
                        .child(Input::new(&props.inputs.input_price)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Default output $/1M", p))
                        .child(Input::new(&props.inputs.output_price)),
                ),
        )
        .child(field_label(
            "Custom HTTP headers (one Header: Value per line)",
            p,
        ))
        .child(Textarea::new(&props.inputs.custom_headers_raw).h(px(64.0)))
        .child(field_label("Extra request options (JSON)", p))
        .child(Textarea::new(&props.inputs.raw_json_extra_body).h(px(72.0)));

    let badges = resolve_model_privacy_badges(
        prof,
        &prof.default_model,
        prof.custom_model_specs.get(&prof.default_model),
    );
    let mut caps_toggles = v_flex().w_full().gap(px(2.0));
    if !badges.is_empty() {
        caps_toggles = caps_toggles.child(
            h_flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.0))
                .pb(px(4.0))
                .children(badges.iter().map(|b| model_privacy_badge_chip(b, p))),
        );
    }
    caps_toggles = caps_toggles
        .child(setting_toggle_row(
            "prov-autofetch",
            "Auto-fetch models on startup",
            "",
            prof.auto_fetch_models,
            p,
            move |v, w, cx| {
                h_af(
                    SettingsAction::ToggleProviderFlag("auto_fetch_models", *v),
                    w,
                    cx,
                )
            },
        ))
        .child(setting_toggle_row(
            "prov-web-search",
            "Provider web search",
            "",
            prof.enable_web_search,
            p,
            move |v, w, cx| {
                h_ws(
                    SettingsAction::ToggleProviderFlag("enable_web_search", *v),
                    w,
                    cx,
                )
            },
        ))
        .child(setting_toggle_row(
            "prov-code-exec",
            "Provider code execution",
            "",
            prof.enable_code_execution,
            p,
            move |v, w, cx| {
                h_ce(
                    SettingsAction::ToggleProviderFlag("enable_code_execution", *v),
                    w,
                    cx,
                )
            },
        ))
        .child(setting_toggle_row(
            "prov-url-ctx",
            "Provider URL context",
            "",
            prof.enable_url_context,
            p,
            move |v, w, cx| {
                h_uc(
                    SettingsAction::ToggleProviderFlag("enable_url_context", *v),
                    w,
                    cx,
                )
            },
        ))
        .child(h_divider(p))
        .child(setting_toggle_row(
            "prov-store-srv",
            "Keep responses on provider servers",
            "",
            prof.store_server_side,
            p,
            move |v, w, cx| {
                h_store(
                    SettingsAction::ToggleProviderFlag("store_server_side", *v),
                    w,
                    cx,
                )
            },
        ))
        .child(setting_toggle_row(
            "prov-chain",
            "Stateful response chaining",
            "",
            prof.use_stateful_chaining,
            p,
            move |v, w, cx| {
                h_chain(
                    SettingsAction::ToggleProviderFlag("use_stateful_chaining", *v),
                    w,
                    cx,
                )
            },
        ));
    if prof.protocol == ProviderProtocol::GeminiInteractions {
        caps_toggles = caps_toggles.child(setting_toggle_row(
            "prov-bg-poll",
            "Background poll & stream recovery",
            "",
            prof.use_background_long_decode,
            p,
            move |v, w, cx| {
                h_bg(
                    SettingsAction::ToggleProviderFlag("use_background_long_decode", *v),
                    w,
                    cx,
                )
            },
        ));
    }
    caps_toggles = caps_toggles.child(setting_toggle_row(
        "prov-legacy-fb",
        "Try older API endpoint on failure",
        "",
        prof.use_legacy_fallback_endpoint,
        p,
        move |v, w, cx| {
            h_leg(
                SettingsAction::ToggleProviderFlag("use_legacy_fallback_endpoint", *v),
                w,
                cx,
            )
        },
    ));

    let caps_card = section_card("Provider capabilities & privacy", None, p).child(caps_toggles);

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(header)
        .child(auth_card)
        .child(sampling_card)
        .child(caps_card)
        .into_any_element()
}
pub(super) fn render_model_config_subpage(
    props: &SettingsRenderProps<'_>,
    prof: &ProviderProfile,
    model_id: &str,
) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;
    let h_back = on.clone();
    let h_sync = on.clone();
    let h_save = on.clone();
    let h_reset = on.clone();
    let m_sync = model_id.to_string();
    let m_reset = model_id.to_string();

    let header = h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .gap(px(8.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(10.0))
                .child(sepia_button(
                    "mcfg-back",
                    "Back",
                    Some(KissIcon::ArrowLeft),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| {
                        h_back(
                            SettingsAction::OpenProviderSubpage(ProvidersSubpage::Main),
                            w,
                            cx,
                        )
                    },
                ))
                .child(model_lab_icon(
                    model_id,
                    18.0,
                    p.accent.to_hsla(),
                    prof.protocol.as_db_str(),
                ))
                .child(
                    div()
                        .font_family(p.code_font_family.clone())
                        .text_size(px(15.0))
                        .font_semibold()
                        .text_color(p.primary_ink.to_hsla())
                        .child(model_id.to_string()),
                ),
        )
        .child(sepia_button(
            "mcfg-sync",
            "Fill in from model catalog",
            Some(KissIcon::Sparkles),
            SepiaButtonStyle::Secondary,
            true,
            false,
            p,
            move |_, w, cx| {
                h_sync(
                    SettingsAction::SyncModelSpecFromCatalog(m_sync.clone()),
                    w,
                    cx,
                )
            },
        ));

    let limits_card = section_card("Limits & per-million token pricing (USD)", None, p)
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Conversation limit (context tokens)", p))
                        .child(Input::new(&props.inputs.model_cfg_context)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Max output tokens", p))
                        .child(Input::new(&props.inputs.model_cfg_max_out)),
                ),
        )
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Input ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_in_price)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Output ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_out_price)),
                ),
        )
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Reused input ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_cache_read)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Saved for reuse ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_cache_write)),
                ),
        )
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Saved longer for reuse ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_long_cache_write)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Reasoning ($ / 1M)", p))
                        .child(Input::new(&props.inputs.model_cfg_reasoning_price)),
                ),
        )
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(sepia_button(
                    "mcfg-reset",
                    "Reset to provider defaults",
                    None,
                    SepiaButtonStyle::Ghost,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_reset(SettingsAction::ResetModelSpec(m_reset.clone()), w, cx),
                ))
                .child(sepia_button(
                    "mcfg-catalog",
                    if props.is_fetching_models {
                        "Fetching…"
                    } else {
                        "Fetch catalog pricing"
                    },
                    Some(KissIcon::Refresh),
                    SepiaButtonStyle::Outline,
                    true,
                    props.is_fetching_models,
                    p,
                    move |_, w, cx| h_save(SettingsAction::FetchModelCatalog, w, cx),
                )),
        );
    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(header)
        .child(limits_card)
        .into_any_element()
}

// ============================================================================================
// Page 3: Assistant (`desktop-page-assis.png` 1:1 + Dedicated Editor Subpage)
// ============================================================================================
