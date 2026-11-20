use super::*;

pub(super) fn register_settings_subscriptions(
    subscriptions: &mut Vec<Subscription>,
    settings_inputs: &SettingsInputs,
    window: &mut Window,
    cx: &mut Context<ParchmentDesktopApp>,
) {
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.api_key,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .api_key
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.api_key = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.base_url,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .base_url
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.base_url = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.default_model,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .default_model
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.default_model = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.extra_api_key_add,
        window,
        |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.handle_settings_action(SettingsAction::AddExtraApiKey, window, cx);
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.manual_model_add,
        window,
        |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.handle_settings_action(SettingsAction::AddManualModel, window, cx);
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.default_model_filter,
        window,
        |_this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.title_prompt,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .title_prompt
                    .read(cx)
                    .value()
                    .to_string();
                if !val.trim().is_empty() {
                    this.snapshot.default_models.title_prompt_template = val;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.translation_lang,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .translation_lang
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !val.is_empty() {
                    this.snapshot.default_models.translation_target_language = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_name,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .assistant_name
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !val.is_empty()
                    && let Some(a) = this
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.name = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_system_prompt,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .assistant_system_prompt
                    .read(cx)
                    .value()
                    .to_string();
                if let Some(a) = this
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.system_prompt = val;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_authors_note,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .assistant_authors_note
                    .read(cx)
                    .value()
                    .to_string();
                if let Some(a) = this
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.authors_note = val;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_default_model,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .assistant_default_model
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Some(a) = this
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.default_model_id = val;
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_temperature,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .assistant_temperature
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 2.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(a) = this
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.temperature = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_max_output,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .assistant_max_output
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw.parse::<i32>().ok().filter(|&v| v > 0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(a) = this
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.max_output_tokens = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.paste_threshold,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .paste_threshold
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<i32>()
                    && v > 0
                {
                    this.snapshot.preferences.paste_conversion_threshold_chars =
                        v.clamp(500, 100_000);
                    this.persist_settings();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.prose_font_size,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .prose_font_size
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<f32>()
                    && v.is_finite()
                    && v > 4.0
                    && v <= 48.0
                {
                    this.snapshot.theme.prose_font_size_sp = v;
                    this.apply_palette_refresh(cx);
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.code_font_size,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .code_font_size
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<f32>()
                    && v.is_finite()
                    && v > 4.0
                    && v <= 48.0
                {
                    this.snapshot.theme.code_font_size_sp = v;
                    this.apply_palette_refresh(cx);
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.max_reading_width,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .max_reading_width
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<f32>()
                    && v.is_finite()
                    && (300.0..=2400.0).contains(&v)
                {
                    this.snapshot.theme.max_reading_width_dp = v;
                    this.apply_palette_refresh(cx);
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.line_height,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .line_height
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<f32>()
                    && v.is_finite()
                    && (1.0..=3.0).contains(&v)
                {
                    this.snapshot.theme.prose_line_height_multiplier = v;
                    this.apply_palette_refresh(cx);
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.paragraph_spacing,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .paragraph_spacing
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<f32>()
                    && v.is_finite()
                    && (0.0..=64.0).contains(&v)
                {
                    this.snapshot.theme.paragraph_spacing_dp = v;
                    this.apply_palette_refresh(cx);
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.endpoint_override,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .endpoint_override
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.endpoint_path_override = val;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.temperature,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .temperature
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 2.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.temperature = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.top_p,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .top_p
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.top_p = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.max_output_tokens,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .max_output_tokens
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw.parse::<i32>().ok().filter(|&v| v > 0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.max_output_tokens = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.context_window,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .context_window
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if let Ok(v) = raw.parse::<i32>()
                    && v >= 0
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.context_window_tokens = v;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.thinking_budget_tokens,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .thinking_budget_tokens
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw.parse::<i32>().ok().filter(|&v| v >= 0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.thinking_budget_tokens = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.input_price,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .input_price
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.input_price_per_m = parsed.unwrap_or(0.0);
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.output_price,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .output_price
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(p) = this
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.output_price_per_m = parsed.unwrap_or(0.0);
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.custom_headers_raw,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .custom_headers_raw
                    .read(cx)
                    .value()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.custom_headers_raw = val;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.raw_json_extra_body,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let val = this
                    .settings_inputs
                    .raw_json_extra_body
                    .read(cx)
                    .value()
                    .to_string();
                if let Some(p) = this
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == this.snapshot.active_profile_id)
                {
                    p.raw_json_extra_body = val;
                    this.persist_settings();
                }
            }
        },
    ));
    for mcfg_in in [
        &settings_inputs.model_cfg_context,
        &settings_inputs.model_cfg_max_out,
        &settings_inputs.model_cfg_in_price,
        &settings_inputs.model_cfg_out_price,
        &settings_inputs.model_cfg_cache_read,
        &settings_inputs.model_cfg_cache_write,
        &settings_inputs.model_cfg_long_cache_write,
        &settings_inputs.model_cfg_reasoning_price,
    ] {
        subscriptions.push(cx.subscribe_in(
            mcfg_in,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change)
                    && let ProvidersSubpage::ModelConfig(mid) = this.providers_subpage.clone()
                {
                    this.save_model_spec_silent(&mid, cx);
                }
            },
        ));
    }
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_top_p,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .assistant_top_p
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(a) = this
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.top_p = parsed;
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.assistant_context_msg_limit,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let raw = this
                    .settings_inputs
                    .assistant_context_msg_limit
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let parsed = raw.parse::<i32>().ok().filter(|&v| v >= 0);
                if (raw.is_empty() || parsed.is_some())
                    && let Some(a) = this
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == this.snapshot.active_assistant_id)
                {
                    a.context_message_limit = parsed.unwrap_or(0);
                    this.persist_settings();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.tool_registry_search,
        window,
        |_this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.mcp_registry_search,
        window,
        |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.handle_settings_action(SettingsAction::SearchMcpRegistry, window, cx);
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.skill_github_url,
        window,
        |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.handle_settings_action(SettingsAction::InstallSkillFromUrl, window, cx);
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.wb_card_name,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(card) = this.active_session.active_card.as_mut()
            {
                let val = this
                    .settings_inputs
                    .wb_card_name
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !val.is_empty() {
                    card.name = val;
                    this.persist_active_session();
                    cx.notify();
                }
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.wb_card_desc,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(card) = this.active_session.active_card.as_mut()
            {
                card.description = this
                    .settings_inputs
                    .wb_card_desc
                    .read(cx)
                    .value()
                    .to_string();
                this.persist_active_session();
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.wb_card_first_mes,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(card) = this.active_session.active_card.as_mut()
            {
                card.first_message = this
                    .settings_inputs
                    .wb_card_first_mes
                    .read(cx)
                    .value()
                    .to_string();
                this.persist_active_session();
            }
        },
    ));
    subscriptions.push(cx.subscribe_in(
        &settings_inputs.ota_repo,
        window,
        |this, _, event: &InputEvent, _window, cx| {
            if matches!(event, InputEvent::Change) {
                let repo = this
                    .settings_inputs
                    .ota_repo
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !repo.is_empty() {
                    this.snapshot.ota_github_repo = repo;
                    this.persist_settings();
                }
            }
        },
    ));
}

pub(super) fn build_settings_inputs(
    snapshot: &AppStateSnapshot,
    window: &mut Window,
    cx: &mut Context<ParchmentDesktopApp>,
) -> SettingsInputs {
    let active_prof = snapshot
        .profiles
        .iter()
        .find(|p| p.id == snapshot.active_profile_id)
        .or_else(|| snapshot.profiles.first());
    let active_asst = snapshot
        .assistants
        .iter()
        .find(|a| a.id == snapshot.active_assistant_id)
        .or_else(|| snapshot.assistants.first());

    let mk_in = |ph: &str,
                 init: &str,
                 masked: bool,
                 w: &mut Window,
                 cx: &mut Context<ParchmentDesktopApp>| {
        let ph = ph.to_string();
        let init = init.to_string();
        cx.new(|cx| {
            let mut st = InputState::new(w, cx).placeholder(ph);
            if masked {
                st = st.masked(true);
            }
            if !init.is_empty() {
                st.set_value(&init, w, cx);
            }
            st
        })
    };
    let mk_ta = |ph: &str, init: &str, w: &mut Window, cx: &mut Context<ParchmentDesktopApp>| {
        let ph = ph.to_string();
        let init = init.to_string();
        cx.new(|cx| {
            let mut st = TextareaState::new(w, cx).placeholder(ph);
            if !init.is_empty() {
                st.set_value(&init, w, cx);
            }
            st
        })
    };

    let spend_str = if snapshot.preferences.chat_spend_limit_usd > 0.0 {
        format!("{}", snapshot.preferences.chat_spend_limit_usd)
    } else {
        String::new()
    };
    let paste_thresh_str = snapshot
        .preferences
        .paste_conversion_threshold_chars
        .to_string();
    let temp_str = active_prof
        .and_then(|p| p.temperature)
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let top_p_str = active_prof
        .and_then(|p| p.top_p)
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let max_tok_str = active_prof
        .and_then(|p| p.max_output_tokens)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let ctx_str = active_prof
        .map(|p| p.context_window_tokens.to_string())
        .unwrap_or_else(|| "128000".into());
    let t_budget_str = active_prof
        .and_then(|p| p.thinking_budget_tokens)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let in_p_str = active_prof
        .filter(|p| p.input_price_per_m > 0.0)
        .map(|p| format!("{}", p.input_price_per_m))
        .unwrap_or_default();
    let out_p_str = active_prof
        .filter(|p| p.output_price_per_m > 0.0)
        .map(|p| format!("{}", p.output_price_per_m))
        .unwrap_or_default();

    let asst_temp_str = active_asst
        .and_then(|a| a.temperature)
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let asst_top_p_str = active_asst
        .and_then(|a| a.top_p)
        .map(|v| format!("{v}"))
        .unwrap_or_default();
    let asst_max_str = active_asst
        .and_then(|a| a.max_output_tokens)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let asst_ctx_msg_str = active_asst
        .filter(|a| a.context_message_limit > 0)
        .map(|a| a.context_message_limit.to_string())
        .unwrap_or_default();

    SettingsInputs {
        search: mk_in("Search settings…", "", false, window, cx),
        user_profile_name: mk_in(
            "Your name",
            &snapshot.preferences.profile_name,
            false,
            window,
            cx,
        ),
        spend_limit: mk_in("No limit (USD per chat)", &spend_str, false, window, cx),
        paste_threshold: mk_in("5000", &paste_thresh_str, false, window, cx),
        prose_font_size: mk_in(
            "14",
            &format!("{}", snapshot.theme.prose_font_size_sp),
            false,
            window,
            cx,
        ),
        code_font_size: mk_in(
            "14",
            &format!("{}", snapshot.theme.code_font_size_sp),
            false,
            window,
            cx,
        ),
        max_reading_width: mk_in(
            "820",
            &format!("{}", snapshot.theme.max_reading_width_dp.round() as i32),
            false,
            window,
            cx,
        ),
        line_height: mk_in(
            "1.54",
            &format!("{:.2}", snapshot.theme.prose_line_height_multiplier),
            false,
            window,
            cx,
        ),
        paragraph_spacing: mk_in(
            "18",
            &format!("{}", snapshot.theme.paragraph_spacing_dp.round() as i32),
            false,
            window,
            cx,
        ),
        hex_canvas: mk_in("#F4ECD8", &snapshot.theme.canvas_bg_hex, false, window, cx),
        hex_card: mk_in("#FAF6EB", &snapshot.theme.card_bg_hex, false, window, cx),
        hex_text: mk_in(
            "#261C14",
            &snapshot.theme.primary_ink_hex,
            false,
            window,
            cx,
        ),
        hex_accent: mk_in("#8B5A2B", &snapshot.theme.accent_hex, false, window, cx),
        provider_name: mk_in(
            "Provider name",
            active_prof.map(|p| p.name.as_str()).unwrap_or(""),
            false,
            window,
            cx,
        ),
        api_key: mk_in(
            "Paste API key…",
            active_prof.map(|p| p.api_key.as_str()).unwrap_or(""),
            true,
            window,
            cx,
        ),
        extra_api_key_add: mk_in("Add backup / rotation API key…", "", true, window, cx),
        base_url: mk_in(
            "https://api.openai.com",
            active_prof.map(|p| p.base_url.as_str()).unwrap_or(""),
            false,
            window,
            cx,
        ),
        endpoint_override: mk_in(
            "/v1/responses",
            active_prof
                .map(|p| p.endpoint_path_override.as_str())
                .unwrap_or(""),
            false,
            window,
            cx,
        ),
        default_model: mk_in(
            "Model ID",
            active_prof.map(|p| p.default_model.as_str()).unwrap_or(""),
            false,
            window,
            cx,
        ),
        manual_model_add: mk_in("Enter model ID…", "", false, window, cx),
        model_filter: mk_in("Filter fetched models…", "", false, window, cx),
        temperature: mk_in("Blank = default", &temp_str, false, window, cx),
        top_p: mk_in("Blank = default", &top_p_str, false, window, cx),
        max_output_tokens: mk_in("Blank = default", &max_tok_str, false, window, cx),
        context_window: mk_in("128000", &ctx_str, false, window, cx),
        thinking_budget_tokens: mk_in("Blank = automatic", &t_budget_str, false, window, cx),
        input_price: mk_in("0.00", &in_p_str, false, window, cx),
        output_price: mk_in("0.00", &out_p_str, false, window, cx),
        custom_headers_raw: mk_ta(
            "X-Header: value (one per line)",
            active_prof
                .map(|p| p.custom_headers_raw.as_str())
                .unwrap_or(""),
            window,
            cx,
        ),
        raw_json_extra_body: mk_ta(
            "{ \"key\": \"value\" }",
            active_prof
                .map(|p| p.raw_json_extra_body.as_str())
                .unwrap_or(""),
            window,
            cx,
        ),
        model_cfg_context: mk_in("128000", "", false, window, cx),
        model_cfg_max_out: mk_in("Blank = default", "", false, window, cx),
        model_cfg_in_price: mk_in("0.00", "", false, window, cx),
        model_cfg_out_price: mk_in("0.00", "", false, window, cx),
        model_cfg_cache_read: mk_in("Blank = unpriced", "", false, window, cx),
        model_cfg_cache_write: mk_in("Blank = unpriced", "", false, window, cx),
        model_cfg_long_cache_write: mk_in("Blank = unpriced", "", false, window, cx),
        model_cfg_reasoning_price: mk_in("Blank = unpriced", "", false, window, cx),
        assistant_name: mk_in(
            "Assistant name",
            active_asst.map(|a| a.name.as_str()).unwrap_or(""),
            false,
            window,
            cx,
        ),
        assistant_system_prompt: mk_ta(
            "System instructions for this assistant…",
            active_asst.map(|a| a.system_prompt.as_str()).unwrap_or(""),
            window,
            cx,
        ),
        assistant_authors_note: mk_ta(
            "Short style reminder…",
            active_asst.map(|a| a.authors_note.as_str()).unwrap_or(""),
            window,
            cx,
        ),
        assistant_default_model: mk_in(
            "Optional model override",
            active_asst
                .map(|a| a.default_model_id.as_str())
                .unwrap_or(""),
            false,
            window,
            cx,
        ),
        assistant_temperature: mk_in(
            "Blank = provider default",
            &asst_temp_str,
            false,
            window,
            cx,
        ),
        assistant_top_p: mk_in(
            "Blank = provider default",
            &asst_top_p_str,
            false,
            window,
            cx,
        ),
        assistant_max_output: mk_in("Blank = provider default", &asst_max_str, false, window, cx),
        assistant_context_msg_limit: mk_in("0 = unlimited", &asst_ctx_msg_str, false, window, cx),
        default_model_filter: mk_in("Filter models or providers…", "", false, window, cx),
        title_prompt: mk_ta(
            "Title prompt template…",
            &snapshot.default_models.title_prompt_template,
            window,
            cx,
        ),
        translation_lang: mk_in(
            "English",
            &snapshot.default_models.translation_target_language,
            false,
            window,
            cx,
        ),
        mcp_name: mk_in("github", "", false, window, cx),
        mcp_url: mk_in("https://…/mcp", "", false, window, cx),
        mcp_command: mk_in("npx -y @modelcontextprotocol/…", "", false, window, cx),
        mcp_bearer: mk_in("Optional token", "", true, window, cx),
        mcp_registry_search: mk_in(
            "Search public MCP servers (e.g. github, notion)…",
            "",
            false,
            window,
            cx,
        ),
        skill_github_url: mk_in(
            "https://github.com/owner/repo/blob/main/SKILL.md",
            "",
            false,
            window,
            cx,
        ),
        tool_registry_search: mk_in(
            "Filter tools by name or description…",
            "",
            false,
            window,
            cx,
        ),
        wb_title: mk_in("Entry title", "", false, window, cx),
        wb_keywords: mk_in("keyword1, keyword2", "", false, window, cx),
        wb_content: mk_ta("Background lore or reference text…", "", window, cx),
        wb_card_name: mk_in("Character or scenario name", "", false, window, cx),
        wb_card_desc: mk_ta(
            "Character description, persona, or scenario…",
            "",
            window,
            cx,
        ),
        wb_card_first_mes: mk_ta("Opening greeting message…", "", window, cx),
        storage_search: mk_in("Search stored files…", "", false, window, cx),
        ota_repo: mk_in(
            "baros4294/parchment",
            &snapshot.ota_github_repo,
            false,
            window,
            cx,
        ),
    }
}
