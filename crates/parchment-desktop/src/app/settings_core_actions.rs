use super::*;

impl ParchmentDesktopApp {
    pub(super) fn handle_settings_action(
        &mut self,
        action: SettingsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SettingsAction::Close => {
                self.recording_shortcut = None;
                self.shortcut_conflict = None;
                self.overlay = ActiveOverlay::None;
                self.sync_window_title(window);
                cx.notify();
            }
            SettingsAction::SelectSection(sec) => {
                self.settings_section = sec;
                self.providers_subpage = ProvidersSubpage::Main;
                self.assistant_subpage = AssistantSubpage::List;
                self.recording_shortcut = None;
                self.shortcut_conflict = None;
                self.confirm_wipe_pending = false;
                if sec == SettingsSection::BackupAndUpdates {
                    self.refresh_storage_scan();
                } else if sec == SettingsSection::Workspace {
                    self.refresh_memory_and_skills();
                }
                cx.notify();
            }
            SettingsAction::ClearSearch => {
                self.settings_inputs
                    .search
                    .update(cx, |i, cx| i.set_value("", window, cx));
                cx.notify();
            }
            // ---- Preferences ----
            SettingsAction::PickProfilePicture => {
                if let Some(src) = pick_open_file_dialog(
                    "Choose profile picture",
                    "Image files",
                    "*.png;*.jpg;*.jpeg;*.webp",
                ) {
                    let avatars_dir = self.data_dir.join("media");
                    let _ = std::fs::create_dir_all(&avatars_dir);
                    let ext = src
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("png")
                        .to_ascii_lowercase();
                    let dest = avatars_dir.join(format!("profile-avatar.{ext}"));
                    match std::fs::copy(&src, &dest) {
                        Ok(_) => {
                            self.snapshot.preferences.profile_picture_path =
                                dest.to_string_lossy().to_string();
                            self.persist_settings();
                            self.refresh_storage_scan();
                            cx.notify();
                        }
                        Err(e) => self.show_notice(format!("Could not copy image: {e}"), cx),
                    }
                }
            }
            SettingsAction::RemoveProfilePicture => {
                self.snapshot.preferences.profile_picture_path.clear();
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetViewMode(mode) => {
                self.snapshot.preferences.default_view_mode = mode;
                self.active_session.view_mode = mode;
                self.persist_settings();
                self.persist_active_session();
                cx.notify();
            }
            SettingsAction::ApplyThemePreset(ix) => {
                let presets = theme_presets();
                if let Some(preset) = presets.get(ix) {
                    let old_pf = self.snapshot.theme.prose_font_family.clone();
                    let old_cf = self.snapshot.theme.code_font_family.clone();
                    let old_cpf = self.snapshot.theme.custom_prose_font_path.clone();
                    let old_ccf = self.snapshot.theme.custom_code_font_path.clone();
                    let old_fs = self.snapshot.theme.prose_font_size_sp;
                    let old_cfs = self.snapshot.theme.code_font_size_sp;
                    let old_lh = self.snapshot.theme.prose_line_height_multiplier;
                    let old_ps = self.snapshot.theme.paragraph_spacing_dp;
                    let old_rw = self.snapshot.theme.max_reading_width_dp;
                    let old_hl = self.snapshot.theme.highlight_dialogue_and_narration;
                    self.snapshot.theme = preset.clone();
                    self.snapshot.theme.prose_font_family = old_pf;
                    self.snapshot.theme.code_font_family = old_cf;
                    self.snapshot.theme.custom_prose_font_path = old_cpf;
                    self.snapshot.theme.custom_code_font_path = old_ccf;
                    self.snapshot.theme.prose_font_size_sp = old_fs;
                    self.snapshot.theme.code_font_size_sp = old_cfs;
                    self.snapshot.theme.prose_line_height_multiplier = old_lh;
                    self.snapshot.theme.paragraph_spacing_dp = old_ps;
                    self.snapshot.theme.max_reading_width_dp = old_rw;
                    self.snapshot.theme.highlight_dialogue_and_narration = old_hl;
                    self.sync_hex_inputs_from_theme(window, cx);
                    self.apply_palette_refresh(cx);
                }
            }
            SettingsAction::SetAccentHex(hex) => {
                self.snapshot.theme.accent_hex = hex;
                self.sync_hex_inputs_from_theme(window, cx);
                self.apply_palette_refresh(cx);
            }
            SettingsAction::ToggleHexEditor => {
                self.show_hex_editor = !self.show_hex_editor;
                if self.show_hex_editor {
                    self.sync_hex_inputs_from_theme(window, cx);
                }
                cx.notify();
            }
            SettingsAction::ApplyCustomHexPalette => {
                let c_hex = self
                    .settings_inputs
                    .hex_canvas
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let card_hex = self
                    .settings_inputs
                    .hex_card
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let txt_hex = self
                    .settings_inputs
                    .hex_text
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let acc_hex = self
                    .settings_inputs
                    .hex_accent
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let valid = |s: &str| {
                    let t = s.trim_start_matches('#');
                    t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit())
                };
                if valid(&c_hex) {
                    self.snapshot.theme.canvas_bg_hex =
                        format!("#{}", c_hex.trim_start_matches('#'));
                }
                if valid(&card_hex) {
                    self.snapshot.theme.card_bg_hex =
                        format!("#{}", card_hex.trim_start_matches('#'));
                }
                if valid(&txt_hex) {
                    self.snapshot.theme.primary_ink_hex =
                        format!("#{}", txt_hex.trim_start_matches('#'));
                }
                if valid(&acc_hex) {
                    self.snapshot.theme.accent_hex =
                        format!("#{}", acc_hex.trim_start_matches('#'));
                }
                self.snapshot.theme.name = "Custom".into();
                self.apply_palette_refresh(cx);
                self.show_notice("Applied custom hex colors.", cx);
            }
            SettingsAction::SetProseFont(font) => {
                self.snapshot.theme.prose_font_family = font;
                self.snapshot.theme.custom_prose_font_path.clear();
                self.apply_palette_refresh(cx);
            }
            SettingsAction::PickCustomProseFont => {
                if let Some(path) =
                    pick_open_file_dialog("Choose prose font", "Font files", "*.ttf;*.otf")
                {
                    let p_str = path.to_string_lossy().to_string();
                    let label = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Custom")
                        .to_string();
                    match register_custom_font_file(&p_str, cx) {
                        Ok(()) => {
                            self.snapshot.theme.custom_prose_font_path = p_str;
                            self.snapshot.theme.prose_font_family = label.clone();
                            self.apply_palette_refresh(cx);
                            self.show_notice(format!("Loaded custom font \"{label}\"."), cx);
                        }
                        Err(e) => self.show_notice(format!("Font load failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ClearCustomProseFont => {
                self.snapshot.theme.custom_prose_font_path.clear();
                self.snapshot.theme.prose_font_family = "Inter".into();
                self.apply_palette_refresh(cx);
            }
            SettingsAction::SetCodeFont(font) => {
                self.snapshot.theme.code_font_family = font;
                self.snapshot.theme.custom_code_font_path.clear();
                self.apply_palette_refresh(cx);
            }
            SettingsAction::PickCustomCodeFont => {
                if let Some(path) =
                    pick_open_file_dialog("Choose code font", "Font files", "*.ttf;*.otf")
                {
                    let p_str = path.to_string_lossy().to_string();
                    let label = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Custom Mono")
                        .to_string();
                    match register_custom_font_file(&p_str, cx) {
                        Ok(()) => {
                            self.snapshot.theme.custom_code_font_path = p_str;
                            self.snapshot.theme.code_font_family = label.clone();
                            self.apply_palette_refresh(cx);
                            self.show_notice(format!("Loaded custom code font \"{label}\"."), cx);
                        }
                        Err(e) => self.show_notice(format!("Font load failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ClearCustomCodeFont => {
                self.snapshot.theme.custom_code_font_path.clear();
                self.snapshot.theme.code_font_family = "JetBrains Mono".into();
                self.apply_palette_refresh(cx);
            }
            SettingsAction::SetSendShortcut(mode) => {
                self.snapshot.preferences.send_shortcut = mode;
                self.composer.update(cx, |s, cx| {
                    s.set_submit_on_enter(mode == SendShortcutMode::Enter, cx)
                });
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetMessageNavVisibility(vis) => {
                self.snapshot.preferences.message_nav_buttons = vis;
                if vis == MessageNavVisibility::Hidden {
                    self.minimap_open = false;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetPasteThreshold(chars) => {
                self.snapshot.preferences.paste_conversion_threshold_chars =
                    chars.clamp(500, 100_000);
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::TogglePref(key, val) => {
                if key == "highlight_dialogue_and_narration" {
                    self.snapshot.theme.highlight_dialogue_and_narration = val;
                    self.apply_palette_refresh(cx);
                    return;
                }
                let pr = &mut self.snapshot.preferences;
                match key {
                    "show_user_avatar" => pr.show_user_avatar = val,
                    "show_user_name" => pr.show_user_name = val,
                    "show_user_timestamp" => pr.show_user_timestamp = val,
                    "show_user_message_actions" => pr.show_user_message_actions = val,
                    "show_model_icon" => pr.show_model_icon = val,
                    "show_assistant_avatar_in_title_bar" => {
                        pr.show_assistant_avatar_in_title_bar = val
                    }
                    "show_model_name" => pr.show_model_name = val,
                    "show_model_timestamp" => pr.show_model_timestamp = val,
                    "show_provider_after_model_name" => pr.show_provider_after_model_name = val,
                    "show_provider_in_model_capsule" => pr.show_provider_in_model_capsule = val,
                    "show_token_and_context_stats" => pr.show_token_and_context_stats = val,
                    "show_tokens_for_entire_turn" => pr.show_tokens_for_entire_turn = val,
                    "show_thinking_cards" => pr.show_thinking_cards = val,
                    "show_tool_cards" => pr.show_tool_cards = val,
                    "show_files_below_replies" => pr.show_files_below_replies = val,
                    "show_reasoning_level_on_button" => pr.show_reasoning_level_on_button = val,
                    "render_assistant_messages_with_markdown" => {
                        pr.render_assistant_messages_with_markdown = val
                    }
                    "render_user_messages_with_markdown" => {
                        pr.render_user_messages_with_markdown = val
                    }
                    "render_reasoning_with_markdown" => pr.render_reasoning_with_markdown = val,
                    "math_formula_rendering" => pr.math_formula_rendering = val,
                    "inline_dollar_math_rendering" => pr.inline_dollar_math_rendering = val,
                    "auto_collapse_code_blocks" => pr.auto_collapse_code_blocks = val,
                    "wrap_code_blocks" => pr.wrap_code_blocks = val,
                    "collapse_long_messages" => pr.collapse_long_messages = val,
                    "auto_collapse_thinking" => pr.auto_collapse_thinking = val,
                    "collapse_thinking_steps" => pr.collapse_thinking_steps = val,
                    "show_tool_result_summary" => pr.show_tool_result_summary = val,
                    "delete_messages_below_when_regenerating" => {
                        pr.delete_messages_below_when_regenerating = val
                    }
                    "delete_previous_message_when_regenerating" => {
                        pr.delete_previous_message_when_regenerating = val
                    }
                    "confirm_before_regenerating" => pr.confirm_before_regenerating = val,
                    "confirm_before_deleting_chat" => pr.confirm_before_deleting_chat = val,
                    "keep_message_versions_when_forking" => {
                        pr.keep_message_versions_when_forking = val
                    }
                    "show_chat_list_dates" => pr.show_chat_list_dates = val,
                    "auto_switch_to_topics" => pr.auto_switch_to_topics = val,
                    "new_chat_when_switching_assistants" => {
                        pr.new_chat_when_switching_assistants = val
                    }
                    "new_chat_after_deleting_topic" => pr.new_chat_after_deleting_topic = val,
                    "new_chat_on_launch" => pr.new_chat_on_launch = val,
                    "auto_scroll_to_bottom_on_open" => pr.auto_scroll_to_bottom_on_open = val,
                    "auto_follow_bottom" => pr.auto_follow_bottom = val,
                    "auto_compact_when_near_context_limit" => {
                        pr.auto_compact_when_near_context_limit = val
                    }
                    "paste_long_text_as_file" => pr.paste_long_text_as_file = val,
                    "reduce_motion" => {
                        pr.reduce_motion = val;
                        cx.set_reduce_motion(val);
                    }
                    _ => {}
                }
                self.persist_settings();
                cx.notify();
            }
            // ---- Providers ----
            SettingsAction::SelectProfile(pid) => {
                self.snapshot.active_profile_id = pid.clone();
                self.active_session.profile_id = pid;
                self.active_session.model_override.clear();
                self.models_fetch_status.clear();
                self.provider_test_status.clear();
                self.providers_subpage = ProvidersSubpage::Main;
                self.persist_settings();
                self.persist_active_session();
                self.sync_provider_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::AddProfile(proto) => {
                let mut prof = make_default_profile(proto);
                let now = now_ms();
                prof.id = format!("prof-{now}");
                let pid = prof.id.clone();
                self.snapshot.profiles.push(prof);
                self.snapshot.active_profile_id = pid.clone();
                self.active_session.profile_id = pid;
                self.providers_subpage = ProvidersSubpage::Main;
                self.persist_settings();
                self.persist_active_session();
                self.sync_provider_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::DeleteProfile(pid) => {
                if self.snapshot.profiles.len() > 1 {
                    self.snapshot.profiles.retain(|p| p.id != pid);
                    if self.snapshot.active_profile_id == pid
                        && let Some(first) = self.snapshot.profiles.first()
                    {
                        self.snapshot.active_profile_id = first.id.clone();
                    }
                    self.providers_subpage = ProvidersSubpage::Main;
                    self.persist_settings();
                    self.sync_provider_inputs_from_active(window, cx);
                    cx.notify();
                }
            }
            SettingsAction::OpenProviderSubpage(sub) => {
                if let ProvidersSubpage::ModelConfig(ref mid) = sub {
                    self.sync_model_cfg_inputs(mid, window, cx);
                }
                self.providers_subpage = sub;
                cx.notify();
            }
            SettingsAction::TestProviderConnection => {
                self.handle_settings_action(SettingsAction::SaveProviderFields, window, cx);
                self.spawn_test_provider_connection(window, cx);
            }
            SettingsAction::ResetProviderDefaultUrl => {
                let def = self
                    .active_profile()
                    .protocol
                    .default_base_url()
                    .to_string();
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.base_url = def.clone();
                }
                self.settings_inputs
                    .base_url
                    .update(cx, |i, cx| i.set_value(&def, window, cx));
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ClearProviderUrl => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.base_url.clear();
                }
                self.settings_inputs
                    .base_url
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetProfileProtocol(proto) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    let old_def = p.protocol.default_base_url();
                    p.protocol = proto;
                    if p.base_url.trim().is_empty() || p.base_url.trim() == old_def {
                        p.base_url = proto.default_base_url().into();
                    }
                }
                self.persist_settings();
                self.sync_provider_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::SetCustomDialect(dial) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.custom_dialect = dial;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetAuthHeaderMode(mode) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.auth_header_mode = mode;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetThinkingEffort(eff) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.thinking_effort = eff;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleProviderFlag(key, val) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    match key {
                        "enabled" => p.enabled = val,
                        "store_server_side" => p.store_server_side = val,
                        "use_stateful_chaining" => {
                            p.use_stateful_chaining = val;
                            if val {
                                p.store_server_side = true;
                            }
                        }
                        "enable_web_search" => p.enable_web_search = val,
                        "enable_code_execution" => p.enable_code_execution = val,
                        "enable_url_context" => p.enable_url_context = val,
                        "auto_fetch_models" => p.auto_fetch_models = val,
                        "use_background_long_decode" => p.use_background_long_decode = val,
                        "use_legacy_fallback_endpoint" => p.use_legacy_fallback_endpoint = val,
                        _ => {}
                    }
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleShowApiKey => {
                self.show_api_key = !self.show_api_key;
                let masked = !self.show_api_key;
                self.settings_inputs
                    .api_key
                    .update(cx, |i, cx| i.set_masked(masked, window, cx));
                self.settings_inputs
                    .extra_api_key_add
                    .update(cx, |i, cx| i.set_masked(masked, window, cx));
                cx.notify();
            }
            SettingsAction::ToggleAddApiKeyBox => {
                self.show_add_api_key_box = !self.show_add_api_key_box;
                cx.notify();
            }
            SettingsAction::AddExtraApiKey => {
                let new_key = self
                    .settings_inputs
                    .extra_api_key_add
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if new_key.is_empty() {
                    self.show_notice("Paste an API key first.", cx);
                    return;
                }
                let mut added = false;
                let mut total = 0usize;
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    added = p.add_api_key(new_key);
                    total = p.all_api_keys().len();
                }
                self.settings_inputs
                    .extra_api_key_add
                    .update(cx, |i, cx| i.set_value("", window, cx));
                if added {
                    self.persist_settings();
                    self.sync_provider_inputs_from_active(window, cx);
                    self.show_notice(format!("Added API key ({total} configured)."), cx);
                } else {
                    self.show_notice("That API key is already saved on this provider.", cx);
                }
            }
            SettingsAction::RemoveExtraApiKey(idx) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.remove_extra_api_key(idx);
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::PromoteExtraApiKey(idx) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.promote_extra_api_key(idx);
                }
                self.persist_settings();
                self.sync_provider_inputs_from_active(window, cx);
                self.show_notice("Promoted API key to primary.", cx);
            }
            SettingsAction::ToggleAddModelBox => {
                self.show_add_model_box = !self.show_add_model_box;
                cx.notify();
            }
            SettingsAction::AddManualModel => {
                let mid = self
                    .settings_inputs
                    .manual_model_add
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if !mid.is_empty() {
                    if let Some(p) = self
                        .snapshot
                        .profiles
                        .iter_mut()
                        .find(|p| p.id == self.snapshot.active_profile_id)
                    {
                        if !p.saved_models.iter().any(|m| m == &mid) {
                            p.saved_models.insert(0, mid.clone());
                        }
                        p.default_model = mid.clone();
                    }
                    self.active_session.model_override = mid.clone();
                    self.active_session.profile_id = self.snapshot.active_profile_id.clone();
                    self.settings_inputs
                        .default_model
                        .update(cx, |i, cx| i.set_value(&mid, window, cx));
                    self.settings_inputs
                        .manual_model_add
                        .update(cx, |i, cx| i.set_value("", window, cx));
                    self.show_add_model_box = false;
                    self.persist_settings();
                    self.persist_active_session();
                    self.show_notice(format!("Selected {mid}."), cx);
                }
            }
            SettingsAction::RemoveSavedModel(mid) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.saved_models.retain(|m| m != &mid);
                    if p.default_model == mid {
                        p.default_model = p.saved_models.first().cloned().unwrap_or_default();
                        let next = p.default_model.clone();
                        self.settings_inputs
                            .default_model
                            .update(cx, |i, cx| i.set_value(&next, window, cx));
                    }
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ClearSavedModels => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.saved_models.clear();
                }
                self.persist_settings();
                self.show_notice("Cleared saved model list.", cx);
            }
            SettingsAction::SaveProviderFields => {
                let name = self
                    .settings_inputs
                    .provider_name
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let api_key = self
                    .settings_inputs
                    .api_key
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let base_url = self
                    .settings_inputs
                    .base_url
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let ep = self
                    .settings_inputs
                    .endpoint_override
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let def_model = self
                    .settings_inputs
                    .default_model
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let temp = self
                    .settings_inputs
                    .temperature
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 2.0);
                let max_tok = self
                    .settings_inputs
                    .max_output_tokens
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|&v| v > 0);
                let ctx_win = self
                    .settings_inputs
                    .context_window
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<i32>()
                    .ok()
                    .filter(|&v| v >= 1024);
                let in_p = self
                    .settings_inputs
                    .input_price
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);
                let out_p = self
                    .settings_inputs
                    .output_price
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0);

                let mut should_auto_fetch = false;
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    if !name.is_empty() {
                        p.name = name;
                    }
                    p.api_key = api_key;
                    p.base_url = base_url;
                    p.endpoint_path_override = ep;
                    p.default_model = def_model.clone();
                    p.temperature = temp;
                    p.max_output_tokens = max_tok;
                    if let Some(cw) = ctx_win {
                        p.context_window_tokens = cw;
                    }
                    p.input_price_per_m = in_p;
                    p.output_price_per_m = out_p;
                    if !def_model.is_empty() && !p.saved_models.iter().any(|m| m == &def_model) {
                        p.saved_models.insert(0, def_model);
                    }
                    should_auto_fetch = p.auto_fetch_models
                        && p.saved_models.is_empty()
                        && (!p.api_key.is_empty() || !p.base_url.is_empty());
                }
                self.persist_settings();
                self.show_notice("Saved provider settings.", cx);
                if should_auto_fetch {
                    self.spawn_fetch_provider_models(window, cx);
                }
            }
            SettingsAction::FetchProviderModels => {
                self.handle_settings_action(SettingsAction::SaveProviderFields, window, cx);
                self.spawn_fetch_provider_models(window, cx);
            }
            SettingsAction::SelectSavedModel(mid) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.default_model = mid.clone();
                }
                self.active_session.model_override = mid.clone();
                self.settings_inputs
                    .default_model
                    .update(cx, |i, cx| i.set_value(&mid, window, cx));
                self.persist_settings();
                self.persist_active_session();
                cx.notify();
            }
            SettingsAction::SyncModelSpecFromCatalog(mid) => {
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
                    self.show_notice(format!("Synced catalog pricing & limits for {mid}."), cx);
                } else {
                    self.show_notice("No cached catalog entry found for this model yet.", cx);
                }
            }
            SettingsAction::FetchModelCatalog => {
                self.is_fetching_models = true;
                self.pending_bg_tasks += 1;
                let reg = self.core.registry().clone();
                let tx = self.bg_tx.clone();
                self.core.runtime().spawn(async move {
                    let ok = reg.ensure_sync_started(true).await;
                    let _ = tx.send(BgTaskEvent::ModelCatalogSynced(ok));
                });
                self.ensure_pump_scheduled(window, cx);
                cx.notify();
            }
            SettingsAction::ResetModelSpec(mid) => {
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == self.snapshot.active_profile_id)
                {
                    p.custom_model_specs.remove(&mid);
                }
                self.persist_settings();
                self.sync_model_cfg_inputs(&mid, window, cx);
                self.show_notice(format!("Reset custom overrides for {mid}."), cx);
            }
            // ---- Assistants ----
            SettingsAction::OpenAssistantSubpage(sub) => {
                if let AssistantSubpage::Edit(ref aid) = sub {
                    self.snapshot.active_assistant_id = aid.clone();
                    self.sync_assistant_inputs_from_active(window, cx);
                }
                self.assistant_subpage = sub;
                cx.notify();
            }
            SettingsAction::CreateAssistant => {
                let now = now_ms();
                let mut a = default_assistant_agent();
                a.id = format!("asst-{now}");
                a.name = format!("Assistant {}", self.snapshot.assistants.len() + 1);
                let aid = a.id.clone();
                self.snapshot.assistants.push(a);
                self.snapshot.active_assistant_id = aid.clone();
                self.assistant_subpage = AssistantSubpage::Edit(aid);
                self.persist_settings();
                self.sync_assistant_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::DuplicateAssistant(aid) => {
                if let Some(mut copy) = self
                    .snapshot
                    .assistants
                    .iter()
                    .find(|a| a.id == aid)
                    .cloned()
                {
                    let now = now_ms();
                    copy.id = format!("asst-{now}");
                    copy.name = format!("{} (Copy)", copy.name);
                    let new_id = copy.id.clone();
                    self.snapshot.assistants.push(copy);
                    self.snapshot.active_assistant_id = new_id.clone();
                    self.assistant_subpage = AssistantSubpage::Edit(new_id);
                    self.persist_settings();
                    self.sync_assistant_inputs_from_active(window, cx);
                    cx.notify();
                }
            }
            SettingsAction::DeleteAssistant(aid) => {
                if self.snapshot.assistants.len() > 1 {
                    self.snapshot.assistants.retain(|a| a.id != aid);
                    if self.snapshot.active_assistant_id == aid
                        && let Some(first) = self.snapshot.assistants.first()
                    {
                        self.snapshot.active_assistant_id = first.id.clone();
                    }
                    self.assistant_subpage = AssistantSubpage::List;
                    self.persist_settings();
                    self.sync_assistant_inputs_from_active(window, cx);
                    cx.notify();
                }
            }
            SettingsAction::SetAssistantIcon(icon_key) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    a.icon_emoji = icon_key;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleAssistantFollowGlobalModel(follow) => {
                let fallback_model = if follow {
                    None
                } else {
                    let p = self.active_profile();
                    let mid = self.effective_model_id(&p);
                    Some((p.id, mid))
                };
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    if let Some((pid, mid)) = fallback_model {
                        a.default_profile_id = pid;
                        a.default_model_id = mid;
                    } else {
                        a.default_profile_id.clear();
                        a.default_model_id.clear();
                    }
                }
                self.persist_settings();
                self.sync_assistant_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::SetAssistantModel {
                profile_id,
                model_id,
            } => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    a.default_profile_id = profile_id;
                    a.default_model_id = model_id;
                }
                self.persist_settings();
                self.sync_assistant_inputs_from_active(window, cx);
                cx.notify();
            }
            SettingsAction::ToggleAssistantToolsEnabled => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.tools_enabled = !pol.tools_enabled;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleAssistantBuiltinTool(name, enabled) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.tool_overrides.insert(name, enabled);
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetAssistantThinkingEffort(eff) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    a.thinking_effort = eff;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetAssistantWorkspaceMode(mode) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    a.workspace_mode = mode;
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.workspace_mode = mode;
                    if mode != WorkspaceMode::ReadWrite {
                        pol.shell_access = ShellAccess::Disabled;
                    }
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::PickAssistantWorkspaceFolder => {
                if let Some(dir) = pick_folder_dialog("Add default assistant workspace folder") {
                    let s = dir.to_string_lossy().to_string();
                    if let Some(a) = self
                        .snapshot
                        .assistants
                        .iter_mut()
                        .find(|a| a.id == self.snapshot.active_assistant_id)
                    {
                        let pol = a
                            .permission_defaults
                            .get_or_insert_with(AgentPermissionPolicy::default);
                        pol.add_workspace_path(s);
                        a.workspace_path = pol.workspace_path.clone();
                        a.workspace_mode = pol.workspace_mode;
                    }
                    self.persist_settings();
                    cx.notify();
                }
            }
            SettingsAction::RemoveAssistantWorkspaceFolder(path) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.remove_workspace_path(&path);
                    a.workspace_path = pol.workspace_path.clone();
                    a.workspace_mode = pol.workspace_mode;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ClearAssistantWorkspaceFolder => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    a.workspace_path.clear();
                    a.workspace_mode = WorkspaceMode::Off;
                    if let Some(pol) = a.permission_defaults.as_mut() {
                        pol.clear_workspace_paths();
                    }
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleAssistantShell => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    if pol.shell_access == ShellAccess::FullAccess {
                        pol.shell_access = ShellAccess::Disabled;
                    } else {
                        pol.workspace_mode = WorkspaceMode::ReadWrite;
                        a.workspace_mode = WorkspaceMode::ReadWrite;
                        pol.shell_access = ShellAccess::FullAccess;
                    }
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleAssistantWeb => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.web_access = !pol.web_access;
                    a.enable_web_search = pol.web_access;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleAssistantComputer => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.computer_access = !pol.computer_access;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetAssistantApprovalMode(mode) => {
                if let Some(a) = self
                    .snapshot
                    .assistants
                    .iter_mut()
                    .find(|a| a.id == self.snapshot.active_assistant_id)
                {
                    let pol = a
                        .permission_defaults
                        .get_or_insert_with(AgentPermissionPolicy::default);
                    pol.approval_mode = mode;
                }
                self.persist_settings();
                cx.notify();
            }
            // ---- Default models ----
            SettingsAction::SetGlobalDefaultModel {
                profile_id,
                model_id,
            } => {
                self.snapshot.default_models.global_profile_id = profile_id.clone();
                self.snapshot.default_models.global_model_id = model_id.clone();
                self.snapshot.active_profile_id = profile_id.clone();
                if let Some(p) = self
                    .snapshot
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == profile_id)
                {
                    p.default_model = model_id;
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetUtilityModel {
                slot,
                profile_id,
                model_id,
            } => {
                let dm = &mut self.snapshot.default_models;
                match slot {
                    "title" => {
                        dm.title_follow_global = false;
                        dm.title_profile_id = profile_id;
                        dm.title_model_id = model_id;
                    }
                    "summary" => {
                        dm.summary_follow_global = false;
                        dm.summary_profile_id = profile_id;
                        dm.summary_model_id = model_id;
                    }
                    "translation" => {
                        dm.translation_follow_global = false;
                        dm.translation_profile_id = profile_id;
                        dm.translation_model_id = model_id;
                    }
                    _ => {}
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ToggleDefaultModelFlag(key, val) => {
                match key {
                    "title_follow_global" | "title_follow" => {
                        self.snapshot.default_models.title_follow_global = val
                    }
                    "summary_follow_global" | "summary_follow" => {
                        self.snapshot.default_models.summary_follow_global = val
                    }
                    "translation_follow_global" | "translation_follow" => {
                        self.snapshot.default_models.translation_follow_global = val
                    }
                    _ => {}
                }
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::SetTranslationLanguage(lang) => {
                self.snapshot.default_models.translation_target_language = lang.clone();
                self.settings_inputs
                    .translation_lang
                    .update(cx, |i, cx| i.set_value(&lang, window, cx));
                self.persist_settings();
                cx.notify();
            }
            other => self.handle_extended_settings_action(other, window, cx),
        }
    }
    pub(super) fn sync_mcp_servers_async(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mcp = self.mcp_registry.clone();
        let cfgs = self.snapshot.preferences.mcp_servers.clone();
        let tx = self.bg_tx.clone();
        self.pending_bg_tasks += 1;
        self.core.runtime().spawn(async move {
            mcp.sync(&cfgs).await;
            let statuses = mcp.statuses().await;
            let _ = tx.send(BgTaskEvent::McpStatusesUpdated(statuses));
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn refresh_mcp_statuses_async(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mcp = self.mcp_registry.clone();
        let tx = self.bg_tx.clone();
        self.pending_bg_tasks += 1;
        self.core.runtime().spawn(async move {
            let statuses = mcp.statuses().await;
            let _ = tx.send(BgTaskEvent::McpStatusesUpdated(statuses));
        });
        self.ensure_pump_scheduled(window, cx);
    }

    pub(super) fn save_model_spec_silent(&mut self, mid: &str, cx: &mut Context<Self>) {
        let parse_opt_f64 = |e: &Entity<InputState>, cx: &Context<Self>| {
            e.read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
        };
        let ctx_win = self
            .settings_inputs
            .model_cfg_context
            .read(cx)
            .value()
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|&v| v >= 0)
            .unwrap_or(self.active_profile().context_window_tokens);
        let max_out = self
            .settings_inputs
            .model_cfg_max_out
            .read(cx)
            .value()
            .trim()
            .parse::<i32>()
            .ok()
            .filter(|&v| v > 0);
        let in_p = parse_opt_f64(&self.settings_inputs.model_cfg_in_price, cx);
        let out_p = parse_opt_f64(&self.settings_inputs.model_cfg_out_price, cx);
        let cr = parse_opt_f64(&self.settings_inputs.model_cfg_cache_read, cx);
        let cw = parse_opt_f64(&self.settings_inputs.model_cfg_cache_write, cx);
        let lcw = parse_opt_f64(&self.settings_inputs.model_cfg_long_cache_write, cx);
        let rp = parse_opt_f64(&self.settings_inputs.model_cfg_reasoning_price, cx);
        let known = if in_p.is_some()
            || out_p.is_some()
            || cr.is_some()
            || cw.is_some()
            || lcw.is_some()
            || rp.is_some()
        {
            Some(TokenPricing {
                input_per_m: in_p,
                output_per_m: out_p,
                cache_read_per_m: cr,
                cache_write_per_m: cw,
                reasoning_per_m: rp,
                long_cache_write_per_m: lcw,
            })
        } else {
            None
        };
        if let Some(p) = self
            .snapshot
            .profiles
            .iter_mut()
            .find(|p| p.id == self.snapshot.active_profile_id)
        {
            let existing = p.custom_model_specs.get(mid).cloned();
            let spec = ModelPricingSpec {
                model_id: mid.to_string(),
                display_name: existing
                    .as_ref()
                    .map(|s| s.display_name.clone())
                    .unwrap_or_default(),
                provider_name: p.name.clone(),
                input_price_per_m: in_p.unwrap_or(0.0),
                output_price_per_m: out_p.unwrap_or(0.0),
                cache_read_price_per_m: cr.unwrap_or(0.0),
                cache_write_price_per_m: cw.unwrap_or(0.0),
                reasoning_price_per_m: rp.unwrap_or(0.0),
                known_pricing: known,
                context_pricing_tiers: existing
                    .as_ref()
                    .map(|s| s.context_pricing_tiers.clone())
                    .unwrap_or_default(),
                charge_pricing: existing
                    .as_ref()
                    .map(|s| s.charge_pricing.clone())
                    .unwrap_or_default(),
                context_window_tokens: ctx_win,
                max_output_tokens: max_out,
                output_modalities: existing
                    .as_ref()
                    .map(|s| s.output_modalities.clone())
                    .unwrap_or_default(),
                input_modalities: existing
                    .as_ref()
                    .map(|s| s.input_modalities.clone())
                    .unwrap_or_default(),
            };
            p.custom_model_specs.insert(mid.to_string(), spec);
        }
        self.persist_settings();
    }
}
