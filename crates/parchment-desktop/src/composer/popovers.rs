use super::*;

pub(super) fn render_tools_drawer(
    props: &ComposerRenderProps<'_>,
    has_input: bool,
) -> Option<gpui::Deferred> {
    let p = props.palette;
    let card = p.card_bg.to_hsla();
    let elev = p.elevated_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    props.show_tools_drawer.then(|| {
        let pol = props.policy;
        let h_reset = props.on_action.clone();
        let h_ten = props.on_action.clone();
        let h_pick = props.on_action.clone();
        let h_clr = props.on_action.clone();
        let h_off = props.on_action.clone();
        let h_ro = props.on_action.clone();
        let h_rw = props.on_action.clone();
        let h_sh_off = props.on_action.clone();
        let h_sh_on = props.on_action.clone();
        let h_web = props.on_action.clone();
        let h_cu = props.on_action.clone();
        let h_a1 = props.on_action.clone();
        let h_a2 = props.on_action.clone();
        let h_a3 = props.on_action.clone();
        let h_goal = props.on_action.clone();
        let h_pre = props.on_action.clone();
        let h_pweb = props.on_action.clone();
        let h_pcode = props.on_action.clone();
        let h_purl = props.on_action.clone();

        let is_gemini = props.profile.protocol == ProviderProtocol::GeminiInteractions;
        let mut drawer_scroll = v_flex()
            .id("comp-tools-drawer-scroll")
            .size_full()
            .p(px(10.0))
            .gap(px(2.0))
            .overflow_y_scrollbar()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .px(px(6.0))
                    .pb(px(4.0))
                    .child(
                        v_flex()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Agent permissions"),
                            )
                            .child(div().text_size(px(11.0)).text_color(muted).child(
                                if props.has_chat_overrides {
                                    "Chat overrides are active"
                                } else {
                                    "Following assistant defaults"
                                },
                            )),
                    )
                    .children(props.has_chat_overrides.then(|| {
                        sepia_button(
                            "drw-reset-asst",
                            "Follow assistant defaults",
                            None,
                            SepiaButtonStyle::Outline,
                            true,
                            false,
                            p,
                            move |_, w, cx| {
                                h_reset(ComposerAction::ResetPermissionOverrides, w, cx)
                            },
                        )
                    })),
            )
            .child(permission_option_row(
                "drw-tools-en",
                "Enable agent tools",
                "",
                pol.tools_enabled,
                false,
                p,
                move |_, w, cx| h_ten(ComposerAction::ToggleToolsEnabled, w, cx),
            ))
            .child(h_divider(p))
            .child(
                div()
                    .px(px(6.0))
                    .pt(px(4.0))
                    .child(field_label("Approval", p)),
            )
            .child(permission_option_row(
                "ap-chg",
                "Ask for changes",
                "",
                pol.approval_mode == ToolApprovalMode::AskForChanges,
                true,
                p,
                move |_, w, cx| {
                    h_a1(
                        ComposerAction::SetApprovalMode(ToolApprovalMode::AskForChanges),
                        w,
                        cx,
                    );
                },
            ))
            .child(permission_option_row(
                "ap-all",
                "Ask before every tool",
                "",
                pol.approval_mode == ToolApprovalMode::AskEveryTool,
                true,
                p,
                move |_, w, cx| {
                    h_a2(
                        ComposerAction::SetApprovalMode(ToolApprovalMode::AskEveryTool),
                        w,
                        cx,
                    );
                },
            ))
            .child(permission_option_row(
                "ap-nev",
                "Never ask",
                "",
                pol.approval_mode == ToolApprovalMode::Never,
                true,
                p,
                move |_, w, cx| {
                    h_a3(
                        ComposerAction::SetApprovalMode(ToolApprovalMode::Never),
                        w,
                        cx,
                    );
                },
            ))
            .child(h_divider(p))
            .child(
                div()
                    .px(px(6.0))
                    .pt(px(4.0))
                    .child(field_label("File access", p)),
            )
            .child(permission_option_row(
                "ws-off",
                "Off",
                "",
                pol.workspace_mode == WorkspaceMode::Off,
                true,
                p,
                move |_, w, cx| h_off(ComposerAction::SetWorkspaceMode(WorkspaceMode::Off), w, cx),
            ))
            .child(permission_option_row(
                "ws-ro",
                "Read files",
                "",
                pol.workspace_mode == WorkspaceMode::ReadOnly,
                true,
                p,
                move |_, w, cx| {
                    h_ro(
                        ComposerAction::SetWorkspaceMode(WorkspaceMode::ReadOnly),
                        w,
                        cx,
                    )
                },
            ))
            .child(permission_option_row(
                "ws-rw",
                "Read and edit files",
                "",
                pol.workspace_mode == WorkspaceMode::ReadWrite,
                true,
                p,
                move |_, w, cx| {
                    h_rw(
                        ComposerAction::SetWorkspaceMode(WorkspaceMode::ReadWrite),
                        w,
                        cx,
                    )
                },
            ))
            .child({
                let all_ws = pol.all_workspace_paths();
                let mut ws_box = v_flex().w_full().px(px(6.0)).py(px(4.0)).gap(px(6.0));
                if all_ws.is_empty() {
                    ws_box = ws_box.child(
                        div()
                            .font_family(p.code_font_family.clone())
                            .text_size(px(11.5))
                            .text_color(muted)
                            .child("No folders mounted"),
                    );
                } else {
                    for (f_ix, folder) in all_ws.iter().enumerate() {
                        let fp = folder.clone();
                        let h_rm = props.on_action.clone();
                        ws_box = ws_box.child(
                            h_flex()
                                .id(("drw-ws-row", f_ix))
                                .w_full()
                                .px(px(8.0))
                                .py(px(4.0))
                                .rounded(px(6.0))
                                .bg(card)
                                .border_1()
                                .border_color(border)
                                .items_center()
                                .justify_between()
                                .gap(px(6.0))
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(kiss_icon(KissIcon::Folder, 13.0, accent))
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .font_family(p.code_font_family.clone())
                                                .text_size(px(11.5))
                                                .text_color(ink)
                                                .child(folder.clone()),
                                        ),
                                )
                                .child(with_tooltip(
                                    icon_button(
                                        ("drw-ws-rm", f_ix),
                                        KissIcon::Close,
                                        false,
                                        p,
                                        move |_, w, cx| {
                                            h_rm(
                                                ComposerAction::RemoveWorkspaceFolder(fp.clone()),
                                                w,
                                                cx,
                                            )
                                        },
                                    ),
                                    "Remove folder",
                                )),
                        );
                    }
                }
                ws_box.child(
                    h_flex()
                        .w_full()
                        .gap(px(6.0))
                        .child(
                            sepia_button(
                                "drw-pick-ws",
                                if all_ws.is_empty() {
                                    strings::CHOOSE_FOLDER
                                } else {
                                    "Add folder"
                                },
                                Some(KissIcon::Plus),
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| h_pick(ComposerAction::PickWorkspaceFolder, w, cx),
                            )
                            .flex_1(),
                        )
                        .children((!all_ws.is_empty()).then(|| {
                            sepia_button(
                                "drw-clr-ws",
                                if all_ws.len() > 1 {
                                    "Clear all"
                                } else {
                                    "Unmount"
                                },
                                None,
                                SepiaButtonStyle::Ghost,
                                true,
                                false,
                                p,
                                move |_, w, cx| h_clr(ComposerAction::ClearWorkspaceFolder, w, cx),
                            )
                        })),
                )
            })
            .child(h_divider(p))
            .child(permission_option_row(
                "ws-web",
                "Allow web search and URL fetching",
                "",
                pol.web_access,
                false,
                p,
                move |_, w, cx| h_web(ComposerAction::ToggleWebAccess, w, cx),
            ))
            .child(
                div()
                    .px(px(6.0))
                    .pt(px(4.0))
                    .child(field_label("Shell access", p)),
            )
            .child(permission_option_row(
                "ws-sh-off",
                "Off",
                "",
                pol.shell_access == ShellAccess::Disabled,
                true,
                p,
                move |_, w, cx| {
                    h_sh_off(ComposerAction::SetShellAccess(ShellAccess::Disabled), w, cx)
                },
            ))
            .child(permission_option_row(
                "ws-sh-on",
                "Run commands (unsandboxed)",
                "",
                pol.shell_access == ShellAccess::FullAccess,
                true,
                p,
                move |_, w, cx| {
                    h_sh_on(
                        ComposerAction::SetShellAccess(ShellAccess::FullAccess),
                        w,
                        cx,
                    )
                },
            ))
            .child(permission_option_row(
                "ws-cu",
                "Computer control",
                "",
                pol.computer_access,
                false,
                p,
                move |_, w, cx| h_cu(ComposerAction::ToggleComputerAccess, w, cx),
            ))
            .child(h_divider(p))
            .child(permission_option_row(
                "drw-goal",
                strings::GOAL_MODE,
                "",
                props.goal_mode_enabled,
                false,
                p,
                move |_, w, cx| h_goal(ComposerAction::ToggleGoalMode, w, cx),
            ))
            .child(permission_option_row(
                "drw-prefill",
                strings::PREFILL_LABEL,
                "",
                props.show_prefill_box,
                false,
                p,
                move |_, w, cx| h_pre(ComposerAction::TogglePrefillBox, w, cx),
            ))
            .child(permission_option_row(
                "drw-pweb",
                "Provider web search",
                "",
                props.profile.enable_web_search,
                false,
                p,
                move |_, w, cx| h_pweb(ComposerAction::ToggleProviderWebSearch, w, cx),
            ));

        if is_gemini {
            drawer_scroll = drawer_scroll
                .child(permission_option_row(
                    "drw-pcode",
                    "Code execution",
                    "",
                    props.profile.enable_code_execution,
                    false,
                    p,
                    move |_, w, cx| h_pcode(ComposerAction::ToggleProviderCodeExec, w, cx),
                ))
                .child(permission_option_row(
                    "drw-purl",
                    "URL context",
                    "",
                    props.profile.enable_url_context,
                    false,
                    p,
                    move |_, w, cx| h_purl(ComposerAction::ToggleProviderUrlContext, w, cx),
                ));
        }

        // Inline expandable Agent Tool Registry (`ManuscriptViewport.kt:1451-1477`)
        let has_ws = !pol.workspace_path.trim().is_empty();
        let ws_ro = has_ws
            && matches!(
                pol.workspace_mode,
                WorkspaceMode::ReadOnly | WorkspaceMode::ReadWrite
            );
        let ws_rw = has_ws && pol.workspace_mode == WorkspaceMode::ReadWrite;
        let shell_ok = ws_rw && pol.shell_access == ShellAccess::FullAccess;
        let builtin_tools: [(&str, bool); 13] = [
            ("fs_read", ws_ro),
            ("fs_list", ws_ro),
            ("fs_glob", ws_ro),
            ("fs_search", ws_ro),
            ("fs_write", ws_rw),
            ("fs_edit", ws_rw),
            ("shell_exec", shell_ok),
            ("web_search", pol.web_access),
            ("web_fetch", pol.web_access),
            ("computer_use", pol.computer_access),
            ("agent_delegate", true),
            ("todo_write", true),
            ("agent_memory", true),
        ];
        let avail_cnt = builtin_tools
            .iter()
            .filter(|(n, ok)| *ok && pol.allows_tool(n))
            .count();
        let h_reg = props.on_action.clone();
        drawer_scroll = drawer_scroll.child(h_divider(p)).child(
            h_flex()
                .w_full()
                .px(px(6.0))
                .py(px(4.0))
                .justify_between()
                .items_center()
                .child(sepia_chip(
                    "drw-tool-reg-tog",
                    format!("Tools ({avail_cnt}/{})", builtin_tools.len()),
                    Some(KissIcon::Wrench),
                    props.show_inline_tool_registry,
                    p,
                    move |_, w, cx| h_reg(ComposerAction::ToggleInlineToolRegistry, w, cx),
                )),
        );
        if props.show_inline_tool_registry {
            let mut reg_box = v_flex().w_full().gap(px(0.0));
            for (ix, (name, permitted)) in builtin_tools.into_iter().enumerate() {
                let enabled = pol.allows_tool(name);
                let tname = name.to_string();
                let h_t = props.on_action.clone();
                reg_box = reg_box.child(permission_option_row(
                    ("drw-reg-t", ix),
                    name,
                    if !pol.tools_enabled {
                        "Agent tools are off"
                    } else if !enabled {
                        "Disabled"
                    } else if permitted {
                        "Available"
                    } else {
                        "Requires permission above"
                    },
                    enabled,
                    false,
                    p,
                    move |_, w, cx| {
                        h_t(
                            ComposerAction::ToggleBuiltinTool(tname.clone(), !enabled),
                            w,
                            cx,
                        )
                    },
                ));
            }
            drawer_scroll = drawer_scroll.child(reg_box);
        }

        // Co-writing helpers (`ManuscriptViewport.kt:1479-1509`)
        let h_draft = props.on_action.clone();
        let h_guide = props.on_action.clone();
        drawer_scroll = drawer_scroll.child(h_divider(p)).child(
            h_flex()
                .w_full()
                .flex_wrap()
                .px(px(6.0))
                .py(px(4.0))
                .items_center()
                .gap(px(6.0))
                .child(field_label("Co-writing helpers", p))
                .child(sepia_button(
                    "drw-draft-reply",
                    "Draft my reply",
                    Some(KissIcon::Sparkles),
                    SepiaButtonStyle::Outline,
                    true,
                    props.is_streaming,
                    p,
                    move |_, w, cx| h_draft(ComposerAction::DraftMyReply, w, cx),
                ))
                .child(sepia_button(
                    "drw-guide-reply",
                    "Guide next reply",
                    Some(KissIcon::ArrowRight),
                    SepiaButtonStyle::Secondary,
                    true,
                    !has_input || props.is_streaming,
                    p,
                    move |_, w, cx| h_guide(ComposerAction::GuideNextReply, w, cx),
                )),
        );

        let drawer = div()
            .w_full()
            .h(px(340.0))
            .overflow_hidden()
            .rounded(px(12.0))
            .bg(elev)
            .border_1()
            .border_color(p.border.to_hsla())
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(drawer_scroll);

        deferred(
            div()
                .absolute()
                .bottom(px(54.0))
                .left_0()
                .right_0()
                .child(spring_pop_enter("comp-tools-drawer-anim", drawer)),
        )
        .with_priority(10)
    })
}

pub(super) fn render_thinking_popover(
    props: &ComposerRenderProps<'_>,
    raw_effort: &str,
    effort_display: &str,
) -> Option<gpui::Deferred> {
    let p = props.palette;
    let elev = p.elevated_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    props.show_thinking_popover.then(|| {
        let cur = raw_effort.to_ascii_lowercase();
        let selected_stop = THINKING_STOPS
            .iter()
            .position(|(_, val)| (cur.is_empty() && *val == "none") || cur == *val);
        let h_slider = props.on_action.clone();
        let h_ad = props.on_action.clone();
        let h_au = props.on_action.clone();

        let pop = v_flex()
            .w(px(290.0))
            .p(px(12.0))
            .rounded(px(12.0))
            .bg(elev)
            .border_1()
            .border_color(p.border.to_hsla())
            .shadow_lg()
            .gap(px(6.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_semibold()
                            .text_color(ink)
                            .child("Thinking"),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_semibold()
                            .text_color(accent)
                            .child(effort_display.to_string()),
                    ),
            )
            .child(step_slider(
                "thk-step-slider",
                THINKING_STOPS.len(),
                selected_stop,
                p,
                move |ix, w, cx| {
                    if let Some((_, val)) = THINKING_STOPS.get(ix) {
                        h_slider(ComposerAction::SetThinkingEffort((*val).to_string()), w, cx);
                    }
                },
            ))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(div().text_size(px(11.0)).text_color(muted).child("Faster"))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(muted)
                            .child("More thorough"),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(sepia_chip(
                        "thk-adaptive",
                        "Adaptive",
                        None,
                        cur == "adaptive",
                        p,
                        move |_, w, cx| {
                            h_ad(ComposerAction::SetThinkingEffort("adaptive".into()), w, cx)
                        },
                    ))
                    .child(sepia_chip(
                        "thk-auto",
                        "Automatic",
                        None,
                        cur == "auto",
                        p,
                        move |_, w, cx| {
                            h_au(ComposerAction::SetThinkingEffort("auto".into()), w, cx)
                        },
                    )),
            )
            .child(Input::new(props.custom_thinking_input));

        deferred(
            div()
                .absolute()
                .bottom(px(54.0))
                .left(px(86.0))
                .child(spring_pop_enter("thk-pop-anim", pop)),
        )
        .with_priority(10)
    })
}

pub(super) fn render_modality_popover(
    props: &ComposerRenderProps<'_>,
    modality: ActiveModalityKind,
) -> Option<gpui::Deferred> {
    let p = props.palette;
    let elev = p.elevated_bg.to_hsla();
    let ink = p.primary_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    (props.show_modality_popover && modality != ActiveModalityKind::Text).then(|| {
        let pop = match modality {
            ActiveModalityKind::Image => v_flex()
                .w(px(290.0))
                .p(px(12.0))
                .rounded(px(12.0))
                .bg(elev)
                .border_1()
                .border_color(p.border.to_hsla())
                .shadow_lg()
                .gap(px(8.0))
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_semibold()
                                .text_color(ink)
                                .child("Shape"),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_semibold()
                                .text_color(accent)
                                .child(props.profile.media_aspect_ratio.clone()),
                        ),
                )
                .child(h_flex().flex_wrap().gap(px(3.0)).children(
                    IMAGE_ASPECTS.iter().enumerate().map(|(ix, ar)| {
                        let v = (*ar).to_string();
                        let h = props.on_action.clone();
                        aspect_tile(
                            ("img-ar", ix),
                            ar,
                            props.profile.media_aspect_ratio == *ar,
                            p,
                            move |_, w, cx| h(ComposerAction::SetImageAspect(v.clone()), w, cx),
                        )
                    }),
                ))
                .child(h_divider(p))
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_semibold()
                                .text_color(ink)
                                .child("Quality"),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_semibold()
                                .text_color(accent)
                                .child(props.profile.media_resolution.clone()),
                        ),
                )
                .child(h_flex().flex_wrap().gap(px(6.0)).children(
                    IMAGE_RES.iter().enumerate().map(|(ix, res)| {
                        let v = (*res).to_string();
                        let h = props.on_action.clone();
                        sepia_chip(
                            ("img-res", ix),
                            *res,
                            None,
                            props.profile.media_resolution.eq_ignore_ascii_case(res),
                            p,
                            move |_, w, cx| h(ComposerAction::SetImageResolution(v.clone()), w, cx),
                        )
                    }),
                )),
            ActiveModalityKind::Video => {
                let secs = props
                    .profile
                    .video_duration_seconds
                    .clamp(MIN_VIDEO_SECONDS, MAX_VIDEO_SECONDS);
                let dur_steps = (MAX_VIDEO_SECONDS - MIN_VIDEO_SECONDS + 1) as usize;
                let dur_ix = Some((secs - MIN_VIDEO_SECONDS) as usize);
                let h_dur = props.on_action.clone();
                v_flex()
                    .w(px(290.0))
                    .p(px(12.0))
                    .rounded(px(12.0))
                    .bg(elev)
                    .border_1()
                    .border_color(p.border.to_hsla())
                    .shadow_lg()
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Shape"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(props.profile.media_aspect_ratio.clone()),
                            ),
                    )
                    .child(h_flex().flex_wrap().gap(px(3.0)).children(
                        VIDEO_ASPECTS.iter().enumerate().map(|(ix, ar)| {
                            let v = (*ar).to_string();
                            let h = props.on_action.clone();
                            aspect_tile(
                                ("vid-ar", ix),
                                ar,
                                props.profile.media_aspect_ratio == *ar,
                                p,
                                move |_, w, cx| h(ComposerAction::SetVideoAspect(v.clone()), w, cx),
                            )
                        }),
                    ))
                    .child(h_divider(p))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Quality"),
                            )
                            .child(sepia_chip(
                                "vid-audio-tog",
                                if props.profile.video_generate_audio {
                                    "Audio"
                                } else {
                                    "Mute"
                                },
                                Some(if props.profile.video_generate_audio {
                                    KissIcon::VolumeOn
                                } else {
                                    KissIcon::VolumeOff
                                }),
                                props.profile.video_generate_audio,
                                p,
                                {
                                    let h = props.on_action.clone();
                                    move |_, w, cx| h(ComposerAction::ToggleVideoAudio, w, cx)
                                },
                            )),
                    )
                    .child(h_flex().flex_wrap().gap(px(6.0)).children(
                        VIDEO_RES.iter().enumerate().map(|(ix, res)| {
                            let v = (*res).to_string();
                            let h = props.on_action.clone();
                            let cur_res = match props.profile.media_resolution.as_str() {
                                "2K" => "1080p",
                                "" | "512" | "1K" => "720p",
                                other => other,
                            };
                            sepia_chip(
                                ("vid-res", ix),
                                *res,
                                None,
                                cur_res.eq_ignore_ascii_case(res),
                                p,
                                move |_, w, cx| {
                                    h(ComposerAction::SetImageResolution(v.clone()), w, cx)
                                },
                            )
                        }),
                    ))
                    .child(h_divider(p))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Length"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(format!("{secs} seconds")),
                            ),
                    )
                    .child(step_slider(
                        "vid-dur-slider",
                        dur_steps,
                        dur_ix,
                        p,
                        move |ix, w, cx| {
                            h_dur(
                                ComposerAction::SetVideoDuration(MIN_VIDEO_SECONDS + ix as i32),
                                w,
                                cx,
                            );
                        },
                    ))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_size(px(11.0)).text_color(muted).child("4 s"))
                            .child(div().text_size(px(11.0)).text_color(muted).child("15 s")),
                    )
            }
            ActiveModalityKind::Audio => {
                let spd_ix = AUDIO_SPEEDS
                    .iter()
                    .position(|s| (*s - props.profile.audio_speed).abs() < 0.05);
                let h_spd = props.on_action.clone();
                v_flex()
                    .w(px(290.0))
                    .p(px(12.0))
                    .rounded(px(12.0))
                    .bg(elev)
                    .border_1()
                    .border_color(p.border.to_hsla())
                    .shadow_lg()
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Voice"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(props.profile.audio_voice.clone()),
                            ),
                    )
                    .child(h_flex().flex_wrap().gap(px(6.0)).children(
                        AUDIO_VOICES.iter().enumerate().map(|(ix, voice)| {
                            let v = (*voice).to_string();
                            let h = props.on_action.clone();
                            sepia_chip(
                                ("aud-v", ix),
                                *voice,
                                None,
                                props.profile.audio_voice.eq_ignore_ascii_case(voice),
                                p,
                                move |_, w, cx| h(ComposerAction::SetAudioVoice(v.clone()), w, cx),
                            )
                        }),
                    ))
                    .child(h_divider(p))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("Speed"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(accent)
                                    .child(format!("{:.2}x", props.profile.audio_speed)),
                            ),
                    )
                    .child(step_slider(
                        "aud-spd-slider",
                        AUDIO_SPEEDS.len(),
                        spd_ix,
                        p,
                        move |ix, w, cx| {
                            if let Some(&sp) = AUDIO_SPEEDS.get(ix) {
                                h_spd(ComposerAction::SetAudioSpeed(sp), w, cx);
                            }
                        },
                    ))
            }
            ActiveModalityKind::Text => v_flex(),
        };
        let pop = pop.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        deferred(
            div()
                .absolute()
                .bottom(px(54.0))
                .left(px(86.0))
                .child(spring_pop_enter("mod-pop-anim", pop)),
        )
        .with_priority(10)
    })
}

pub(super) fn render_context_popover(
    props: &ComposerRenderProps<'_>,
    total_used: i64,
    is_unlimited: bool,
    safe_limit: i64,
) -> Option<gpui::Deferred> {
    let p = props.palette;
    let bd = props.breakdown;
    let card = p.card_bg.to_hsla();
    let elev = p.elevated_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    props.show_context_popover.then(|| {
        let pct_tenths = ((total_used as f64 / safe_limit.max(1) as f64) * 1000.0).floor() / 10.0;
        let limit_text = if is_unlimited {
            "∞".to_string()
        } else {
            format_tokens_short(bd.context_limit_tokens)
        };
        let cost_lbl = format_usd_short(props.session_cost_usd);
        let cost_suffix = if cost_lbl.is_empty() {
            String::new()
        } else {
            format!(" · Session {cost_lbl}")
        };

        let history_label = if bd.muted_turns_count > 0 {
            format!(
                "Chat history ({} active, {} excluded)",
                bd.active_turns_count, bd.muted_turns_count
            )
        } else {
            format!("Chat history ({} active)", bd.active_turns_count)
        };
        let slices: [(String, i64, gpui::Hsla, bool); 7] = [
            (
                "System & persona prompt".into(),
                bd.system_and_persona_tokens,
                p.accent.to_hsla(),
                false,
            ),
            (
                "World Book & story memory".into(),
                bd.lorebook_and_memory_tokens,
                p.accent_secondary.to_hsla(),
                false,
            ),
            (
                "Workspace & tool schemas".into(),
                bd.workspace_and_tools_tokens,
                CAT_SAGE.to_hsla(),
                false,
            ),
            (
                format!("Uploaded files ({})", bd.uploaded_files_count),
                bd.uploaded_files_tokens,
                CAT_SLATE.to_hsla(),
                false,
            ),
            (
                history_label,
                bd.chat_history_tokens,
                CAT_OCHRE.to_hsla(),
                false,
            ),
            (
                "Current composer draft".into(),
                bd.composer_draft_tokens,
                CAT_BRASS.to_hsla(),
                false,
            ),
            (
                "Free space remaining".into(),
                bd.free_space_tokens(),
                p.border.with_alpha(0.55),
                true,
            ),
        ];

        let bar_denom = safe_limit.max(total_used).max(1) as f32;
        let mut bar_row = h_flex()
            .w_full()
            .h(px(10.0))
            .rounded(px(999.0))
            .overflow_hidden()
            .bg(elev)
            .border_1()
            .border_color(p.border.with_alpha(0.70));
        for (_, toks, col, is_free) in &slices {
            if *toks > 0 {
                let frac = (*toks as f32 / bar_denom).max(if *is_free { 0.01 } else { 0.015 });
                bar_row = bar_row.child(div().h_full().w(gpui::relative(frac)).bg(*col));
            }
        }

        let mut slice_rows = v_flex().w_full().gap(px(5.0));
        for (label, toks, col, is_free) in &slices {
            let s_pct = ((*toks as f64 / safe_limit.max(1) as f64) * 1000.0).floor() / 10.0;
            let text_col = if *is_free {
                narration
            } else if *toks > 0 {
                ink
            } else {
                muted
            };
            slice_rows = slice_rows.child(
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
                            .gap(px(7.0))
                            .child(
                                div()
                                    .w(px(8.0))
                                    .h(px(8.0))
                                    .rounded(px(999.0))
                                    .bg(*col)
                                    .flex_shrink_0(),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(text_col)
                                    .child(label.clone()),
                            ),
                    )
                    .child(
                        div()
                            .font_family(p.code_font_family.clone())
                            .text_size(px(11.0))
                            .text_color(if *toks > 0 || *is_free {
                                narration
                            } else {
                                muted
                            })
                            .child(format!("{} ({s_pct:.1}%)", format_tokens_short(*toks))),
                    ),
            );
        }

        let h_close = props.on_action.clone();
        let h_comp = props.on_action.clone();
        let h_sum = props.on_action.clone();
        let h_price = props.on_action.clone();

        let ctx_card = v_flex()
            .w(px(460.0))
            .p(px(14.0))
            .rounded(px(12.0))
            .bg(elev)
            .border_1()
            .border_color(p.border.to_hsla())
            .shadow_lg()
            .gap(px(8.0))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(14.5))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child("What fills this conversation"),
                            )
                            .child(
                                div()
                                    .font_family(p.code_font_family.clone())
                                    .text_size(px(11.0))
                                    .text_color(muted)
                                    .child(format!(
                                        "{} / {limit_text} tokens ({pct_tenths:.1}%){cost_suffix}",
                                        format_tokens_short(total_used)
                                    )),
                            ),
                    )
                    .child(icon_button(
                        "ctx-close-pop",
                        KissIcon::Close,
                        false,
                        p,
                        move |_, w, cx| h_close(ComposerAction::ToggleContextPopover, w, cx),
                    )),
            )
            .child(bar_row)
            .child(h_divider(p))
            .child(slice_rows)
            .children((bd.utility_requests > 0).then(|| {
                let u_cost = format_usd_short(bd.utility_cost_usd);
                let h_bg_tog = props.on_action.clone();
                let mut bg_col = v_flex().w_full().gap(px(5.0)).child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(div().text_size(px(12.0)).text_color(ink).child(format!(
                            "Background requests: {}{}",
                            bd.utility_requests,
                            if u_cost.is_empty() {
                                String::new()
                            } else {
                                format!(" · {u_cost}")
                            }
                        )))
                        .child(sepia_button(
                            "ctx-bg-toggle",
                            if props.show_background_usage {
                                "Hide background usage"
                            } else {
                                "Show background usage"
                            },
                            None,
                            SepiaButtonStyle::Ghost,
                            true,
                            false,
                            p,
                            move |_, w, cx| h_bg_tog(ComposerAction::ToggleBackgroundUsage, w, cx),
                        )),
                );
                if props.show_background_usage && !props.utility_records.is_empty() {
                    let mut list = v_flex()
                        .id("ctx-bg-list")
                        .w_full()
                        .max_h(px(130.0))
                        .gap(px(3.0))
                        .p(px(6.0))
                        .rounded(px(6.0))
                        .bg(card)
                        .border_1()
                        .border_color(border)
                        .overflow_y_scrollbar();
                    for rec in props.utility_records.iter().rev().take(25) {
                        let in_tok = rec.usage.input_tokens.unwrap_or(0);
                        let out_tok = rec.usage.output_tokens.unwrap_or(0);
                        let est_usd = rec
                            .usage
                            .charges
                            .reported_total_usd
                            .or_else(|| {
                                let tm = TokenMetrics {
                                    input_tokens: in_tok,
                                    cached_input_tokens: rec.usage.cached_input_tokens.unwrap_or(0),
                                    reasoning_tokens: rec.usage.reasoning_tokens.unwrap_or(0),
                                    output_tokens: out_tok,
                                    cache_creation_input_tokens: rec
                                        .usage
                                        .cache_creation_input_tokens
                                        .unwrap_or(0),
                                    ..Default::default()
                                };
                                rec.pricing.estimate(
                                    &tm,
                                    rec.usage.short_cache_creation_input_tokens,
                                    rec.usage.long_cache_creation_input_tokens,
                                )
                            })
                            .unwrap_or(0.0);
                        let c_str = format_usd_short(est_usd);
                        let op_lbl = if rec.purpose.trim().is_empty() {
                            "utility"
                        } else {
                            rec.purpose.as_str()
                        };
                        list = list.child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(narration)
                                        .child(format!("{op_lbl} · {}", rec.model)),
                                )
                                .child(
                                    div()
                                        .font_family(p.code_font_family.clone())
                                        .text_size(px(10.5))
                                        .text_color(muted)
                                        .child(format!(
                                            "{}+{} tok{}",
                                            format_tokens_short(in_tok),
                                            format_tokens_short(out_tok),
                                            if c_str.is_empty() {
                                                String::new()
                                            } else {
                                                format!(" · {c_str}")
                                            }
                                        )),
                                ),
                        );
                    }
                    bg_col = bg_col.child(list);
                }
                bg_col
            }))
            .child(h_divider(p))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.0))
                    .child(sepia_button(
                        "ctx-compact",
                        if props.is_streaming {
                            "Shorten & continue"
                        } else {
                            strings::SHORTEN_CONVERSATION
                        },
                        None,
                        SepiaButtonStyle::Secondary,
                        true,
                        !props.has_messages,
                        p,
                        move |_, w, cx| h_comp(ComposerAction::CompactContext, w, cx),
                    ))
                    .child(sepia_button(
                        "ctx-summarize",
                        if props.is_streaming {
                            "Summarize & resume"
                        } else {
                            "Summarize"
                        },
                        None,
                        SepiaButtonStyle::Outline,
                        true,
                        !props.has_messages,
                        p,
                        move |_, w, cx| h_sum(ComposerAction::SummarizeChat, w, cx),
                    ))
                    .child(sepia_button(
                        "ctx-limit-pricing",
                        "Limit & pricing",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_price(ComposerAction::OpenPricingSettings, w, cx),
                    )),
            );
        deferred(
            div()
                .absolute()
                .bottom(px(54.0))
                .right(px(12.0))
                .child(spring_pop_enter("ctx-pop-anim", ctx_card)),
        )
        .with_priority(10)
    })
}
