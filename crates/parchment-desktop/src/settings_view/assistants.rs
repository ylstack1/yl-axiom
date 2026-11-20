use super::*;

pub(super) fn render_assistants_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;

    if let AssistantSubpage::Edit(edit_id) = props.assistant_subpage
        && let Some(asst) = props.snapshot.assistants.iter().find(|a| a.id == *edit_id)
    {
        return render_assistant_editor_subpage(props, asst);
    }

    let h_new = on.clone();
    let header = h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .child(
            div()
                .text_size(px(19.0))
                .font_semibold()
                .text_color(p.primary_ink.to_hsla())
                .child("Assistant"),
        )
        .child(sepia_button(
            "asst-create-btn",
            "New assistant",
            Some(KissIcon::Plus),
            SepiaButtonStyle::Primary,
            true,
            false,
            p,
            move |_, w, cx| h_new(SettingsAction::CreateAssistant, w, cx),
        ));

    let mut list_col = v_flex().w_full().gap(px(10.0));
    let hover_bg = p.hover_tint();
    for (ix, asst) in props.snapshot.assistants.iter().enumerate() {
        let is_active = asst.id == props.snapshot.active_assistant_id;
        let aid_edit = asst.id.clone();
        let aid_dup = asst.id.clone();
        let aid_del = asst.id.clone();
        let h_edit = on.clone();
        let h_dup = on.clone();
        let h_del = on.clone();

        let prompt_preview = if asst.system_prompt.trim().is_empty() {
            "No custom system prompt".to_string()
        } else {
            asst.system_prompt
                .trim()
                .replace('\n', " ")
                .chars()
                .take(120)
                .collect()
        };

        let mut card = h_flex()
            .id(("asst-list-card", ix))
            .relative()
            .w_full()
            .p(px(14.0))
            .rounded(px(12.0))
            .bg(p.card_bg.to_hsla())
            .border_1()
            .border_color(if is_active {
                p.accent.to_hsla()
            } else {
                p.subtle_border()
            })
            .justify_between()
            .items_center()
            .gap(px(12.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_click(move |_, w, cx| {
                h_edit(
                    SettingsAction::OpenAssistantSubpage(AssistantSubpage::Edit(aid_edit.clone())),
                    w,
                    cx,
                )
            });
        if is_active {
            card = card.child(rubric_mark_overlay(10.0, p));
        }

        list_col = list_col.child(
            card.child(
                h_flex()
                    .flex_1()
                    .overflow_hidden()
                    .items_center()
                    .gap(px(12.0))
                    .child(sepia_avatar_circle(&asst.icon_emoji, is_active, 42.0, p))
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .gap(px(3.0))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .text_size(px(14.5))
                                            .font_semibold()
                                            .text_color(p.primary_ink.to_hsla())
                                            .child(asst.name.clone()),
                                    )
                                    .children(is_active.then(|| {
                                        div()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded(px(6.0))
                                            .bg(p.accent.with_alpha(0.14))
                                            .text_size(px(10.5))
                                            .font_semibold()
                                            .text_color(p.accent.to_hsla())
                                            .child("Active")
                                    })),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.0))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(prompt_preview),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(icon_button(
                        ("asst-dup", ix),
                        KissIcon::Copy,
                        false,
                        p,
                        move |_, w, cx| {
                            cx.stop_propagation();
                            h_dup(SettingsAction::DuplicateAssistant(aid_dup.clone()), w, cx);
                        },
                    ))
                    .children((props.snapshot.assistants.len() > 1).then(|| {
                        icon_button(
                            ("asst-del", ix),
                            KissIcon::Trash,
                            false,
                            p,
                            move |_, w, cx| {
                                cx.stop_propagation();
                                h_del(SettingsAction::DeleteAssistant(aid_del.clone()), w, cx);
                            },
                        )
                    })),
            ),
        );
    }

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(header)
        .child(list_col)
        .into_any_element()
}

pub(super) fn render_assistant_editor_subpage(
    props: &SettingsRenderProps<'_>,
    asst: &parchment_core::model::AssistantAgent,
) -> AnyElement {
    let p = props.palette;
    let on = props.on_action;
    let h_back = on.clone();
    let h_pick_ws = on.clone();
    let h_clr_ws = on.clone();
    let h_ws_off = on.clone();
    let h_ws_ro = on.clone();
    let h_ws_rw = on.clone();
    let h_sh = on.clone();
    let h_web = on.clone();
    let h_cu = on.clone();
    let h_ap1 = on.clone();
    let h_ap2 = on.clone();
    let h_ap3 = on.clone();

    let header = h_flex()
        .w_full()
        .items_center()
        .gap(px(10.0))
        .child(sepia_button(
            "asst-ed-back",
            "Back",
            Some(KissIcon::ArrowLeft),
            SepiaButtonStyle::Outline,
            true,
            false,
            p,
            move |_, w, cx| {
                h_back(
                    SettingsAction::OpenAssistantSubpage(AssistantSubpage::List),
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
                .child(format!("Edit assistant — {}", asst.name)),
        );
    // Identity + 18 vector icon choices (`SettingsAssistantsPage.kt:108-144`)
    let identity_card = section_card("Identity & icon", None, p)
        .child(
            h_flex()
                .w_full()
                .items_end()
                .gap(px(12.0))
                .child(sepia_avatar_circle(&asst.icon_emoji, true, 42.0, p))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Assistant name", p))
                        .child(Input::new(&props.inputs.assistant_name)),
                ),
        )
        .child(field_label("Icon", p))
        .child(
            h_flex().flex_wrap().gap(px(6.0)).children(
                ASSISTANT_ICON_CHOICES
                    .iter()
                    .enumerate()
                    .map(|(ix, (sym, icon))| {
                        let sel =
                            asst.icon_emoji == *sym || (asst.icon_emoji.is_empty() && ix == 0);
                        let s = (*sym).to_string();
                        let h = on.clone();
                        sepia_chip(("asst-ic", ix), "", Some(*icon), sel, p, move |_, w, cx| {
                            h(SettingsAction::SetAssistantIcon(s.clone()), w, cx)
                        })
                    }),
            ),
        );

    // Prompts & model overrides
    // Prompts & model overrides (`SettingsAssistantsPage.kt:145-318`)
    let follows_global = asst.default_model_id.trim().is_empty();
    let h_follow_glob = on.clone();
    let mut prompts_card = section_card("Instructions, model & sampling", None, p)
        .child(field_label("System prompt", p))
        .child(Textarea::new(&props.inputs.assistant_system_prompt).h(px(120.0)))
        .child(field_label("Author's note / style reminder", p))
        .child(Textarea::new(&props.inputs.assistant_authors_note).h(px(72.0)))
        .child(setting_toggle_row(
            "asst-follow-glob-model",
            "Follow global default model",
            "",
            follows_global,
            p,
            move |v, w, cx| {
                h_follow_glob(SettingsAction::ToggleAssistantFollowGlobalModel(*v), w, cx)
            },
        ));
    if !follows_global {
        let mut mlist = v_flex()
            .id("asst-model-pick-scroll")
            .w_full()
            .max_h(px(190.0))
            .p(px(6.0))
            .rounded(px(8.0))
            .bg(p.sheet_bg.with_alpha(0.45))
            .border_1()
            .border_color(p.subtle_border())
            .gap(px(4.0))
            .overflow_y_scrollbar();
        let mut m_idx = 0usize;
        let hover_bg = p.hover_tint();
        let sel_pid = if asst.default_profile_id.is_empty() {
            props.snapshot.active_profile_id.as_str()
        } else {
            asst.default_profile_id.as_str()
        };
        for prof in &props.snapshot.profiles {
            let cands: Vec<&str> = if !prof.saved_models.is_empty() {
                prof.saved_models.iter().map(|s| s.as_str()).collect()
            } else if !prof.default_model.trim().is_empty() {
                vec![prof.default_model.as_str()]
            } else {
                Vec::new()
            };
            for mid in cands {
                let is_sel = prof.id == sel_pid && mid == asst.default_model_id;
                let pid = prof.id.clone();
                let m = mid.to_string();
                let h = on.clone();
                let mut row = h_flex()
                    .id(("asst-mod-opt", m_idx))
                    .relative()
                    .overflow_hidden()
                    .w_full()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(8.0))
                    .bg(if is_sel {
                        p.accent.with_alpha(0.13)
                    } else {
                        p.card_bg.to_hsla()
                    })
                    .border_1()
                    .border_color(if is_sel {
                        p.accent.to_hsla()
                    } else {
                        p.border.with_alpha(0.45)
                    })
                    .justify_between()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_, w, cx| {
                        h(
                            SettingsAction::SetAssistantModel {
                                profile_id: pid.clone(),
                                model_id: m.clone(),
                            },
                            w,
                            cx,
                        )
                    });
                if is_sel {
                    row = row.child(rubric_mark_overlay(5.0, p));
                }
                mlist = mlist.child(
                    row.child(
                        h_flex()
                            .flex_1()
                            .overflow_hidden()
                            .items_center()
                            .gap(px(8.0))
                            .child(model_lab_icon(
                                mid,
                                14.0,
                                if is_sel {
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
                                    .text_size(px(12.5))
                                    .font_medium()
                                    .text_color(if is_sel {
                                        p.accent.to_hsla()
                                    } else {
                                        p.primary_ink.to_hsla()
                                    })
                                    .child(mid.to_string()),
                            ),
                    )
                    .child(
                        div()
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(5.0))
                            .bg(p.sheet_bg.to_hsla())
                            .text_size(px(10.5))
                            .text_color(p.narration_ink.to_hsla())
                            .child(prof.name.clone()),
                    ),
                );
                m_idx += 1;
            }
        }
        prompts_card = prompts_card
            .child(mlist)
            .child(Input::new(&props.inputs.assistant_default_model));
    }
    prompts_card = prompts_card
        .child({
            const ASST_THK_STOPS: [(&str, &str); 6] = [
                ("Off", "none"),
                ("Low", "low"),
                ("Medium", "medium"),
                ("High", "high"),
                ("Extra", "xhigh"),
                ("Max", "max"),
            ];
            let cur_eff = asst.thinking_effort.trim().to_ascii_lowercase();
            let sel_stop = ASST_THK_STOPS.iter().position(|(_, v)| cur_eff == *v);
            let eff_lbl = match cur_eff.as_str() {
                "" => "Follow provider",
                "none" => "Off",
                "low" => "Low",
                "medium" => "Medium",
                "high" => "High",
                "xhigh" => "Extra",
                "max" => "Max",
                "adaptive" => "Adaptive",
                "auto" => "Automatic",
                _ => asst.thinking_effort.trim(),
            };
            let h_sl = on.clone();
            let h_inh = on.clone();
            let h_ad = on.clone();
            let h_au = on.clone();
            v_flex()
                .w_full()
                .gap(px(4.0))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(field_label("Thinking effort override", p))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_semibold()
                                .text_color(p.accent.to_hsla())
                                .child(eff_lbl.to_string()),
                        ),
                )
                .child(step_slider(
                    "asst-thk-slider",
                    ASST_THK_STOPS.len(),
                    sel_stop,
                    p,
                    move |ix, w, cx| {
                        if let Some((_, val)) = ASST_THK_STOPS.get(ix) {
                            h_sl(
                                SettingsAction::SetAssistantThinkingEffort((*val).to_string()),
                                w,
                                cx,
                            );
                        }
                    },
                ))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .child(sepia_chip(
                            "asst-thk-inh",
                            "Follow provider",
                            None,
                            cur_eff.is_empty(),
                            p,
                            move |_, w, cx| {
                                h_inh(
                                    SettingsAction::SetAssistantThinkingEffort(String::new()),
                                    w,
                                    cx,
                                )
                            },
                        ))
                        .child(sepia_chip(
                            "asst-thk-ad",
                            "Adaptive",
                            None,
                            cur_eff == "adaptive",
                            p,
                            move |_, w, cx| {
                                h_ad(
                                    SettingsAction::SetAssistantThinkingEffort("adaptive".into()),
                                    w,
                                    cx,
                                )
                            },
                        ))
                        .child(sepia_chip(
                            "asst-thk-au",
                            "Automatic",
                            None,
                            cur_eff == "auto",
                            p,
                            move |_, w, cx| {
                                h_au(
                                    SettingsAction::SetAssistantThinkingEffort("auto".into()),
                                    w,
                                    cx,
                                )
                            },
                        )),
                )
        })
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Temperature override", p))
                        .child(Input::new(&props.inputs.assistant_temperature)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Top-P override", p))
                        .child(Input::new(&props.inputs.assistant_top_p)),
                ),
        )
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("Max output tokens override", p))
                        .child(Input::new(&props.inputs.assistant_max_output)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(4.0))
                        .child(field_label("History message limit (0 = unlimited)", p))
                        .child(Input::new(&props.inputs.assistant_context_msg_limit)),
                ),
        );

    // Embedded default workspace & tool permissions (`SettingsAssistantsPage.kt:248-259`)
    let def_pol = asst.permission_defaults.clone().unwrap_or_default();
    let ws_mode = if asst.workspace_mode != WorkspaceMode::Off {
        asst.workspace_mode
    } else {
        def_pol.workspace_mode
    };
    let ws_path = if !asst.workspace_path.is_empty() {
        asst.workspace_path.as_str()
    } else {
        def_pol.workspace_path.as_str()
    };
    let h_aten = on.clone();

    let perms_card = section_card("Default workspace & permissions for new chats", None, p)
        .child(permission_option_row(
            "asst-tools-en",
            "Enable agent tools",
            "",
            def_pol.tools_enabled,
            false,
            p,
            move |_, w, cx| h_aten(SettingsAction::ToggleAssistantToolsEnabled, w, cx),
        ))
        .child(h_divider(p))
        .child({
            let asst_folders = def_pol.all_workspace_paths();
            let mut ws_box = v_flex().w_full().gap(px(6.0));
            if asst_folders.is_empty() {
                ws_box = ws_box.child(
                    div()
                        .text_size(px(12.5))
                        .text_color(p.muted_ink.to_hsla())
                        .child("No default folders"),
                );
            } else {
                for (f_ix, folder) in asst_folders.iter().enumerate() {
                    let fp = folder.clone();
                    let h_rm = on.clone();
                    ws_box = ws_box.child(
                        h_flex()
                            .id(("asst-ws-row", f_ix))
                            .w_full()
                            .px(px(8.0))
                            .py(px(5.0))
                            .rounded(px(8.0))
                            .bg(p.elevated_bg.with_alpha(0.42))
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
                                    .gap(px(6.0))
                                    .child(kiss_icon(KissIcon::Folder, 14.0, p.accent.to_hsla()))
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .font_family(p.code_font_family.clone())
                                            .text_size(px(12.0))
                                            .text_color(p.primary_ink.to_hsla())
                                            .child(folder.clone()),
                                    ),
                            )
                            .child(icon_button(
                                ("asst-ws-rm", f_ix),
                                KissIcon::Close,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_rm(
                                        SettingsAction::RemoveAssistantWorkspaceFolder(fp.clone()),
                                        w,
                                        cx,
                                    )
                                },
                            )),
                    );
                }
            }
            ws_box.child(
                h_flex()
                    .gap(px(6.0))
                    .child(sepia_button(
                        "asst-ws-pick",
                        if asst_folders.is_empty() {
                            strings::CHOOSE_FOLDER
                        } else {
                            "Add folder"
                        },
                        Some(KissIcon::Plus),
                        SepiaButtonStyle::Outline,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_pick_ws(SettingsAction::PickAssistantWorkspaceFolder, w, cx)
                        },
                    ))
                    .children((!asst_folders.is_empty()).then(|| {
                        sepia_button(
                            "asst-ws-clr",
                            if asst_folders.len() > 1 {
                                "Clear all"
                            } else {
                                strings::CLEAR_FOLDER
                            },
                            None,
                            SepiaButtonStyle::Ghost,
                            true,
                            false,
                            p,
                            move |_, w, cx| {
                                h_clr_ws(SettingsAction::ClearAssistantWorkspaceFolder, w, cx)
                            },
                        )
                    })),
            )
        })
        .child(
            v_flex()
                .w_full()
                .gap(px(0.0))
                .child(div().pb(px(2.0)).child(field_label("File access", p)))
                .child(permission_option_row(
                    "aws-off",
                    "Off",
                    "",
                    ws_mode == WorkspaceMode::Off,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ws_off(
                            SettingsAction::SetAssistantWorkspaceMode(WorkspaceMode::Off),
                            w,
                            cx,
                        )
                    },
                ))
                .child(permission_option_row(
                    "aws-ro",
                    "Read files",
                    "",
                    ws_mode == WorkspaceMode::ReadOnly,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ws_ro(
                            SettingsAction::SetAssistantWorkspaceMode(WorkspaceMode::ReadOnly),
                            w,
                            cx,
                        )
                    },
                ))
                .child(permission_option_row(
                    "aws-rw",
                    "Read and edit files",
                    "",
                    ws_mode == WorkspaceMode::ReadWrite,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ws_rw(
                            SettingsAction::SetAssistantWorkspaceMode(WorkspaceMode::ReadWrite),
                            w,
                            cx,
                        )
                    },
                )),
        )
        .child(h_divider(p))
        .child(
            v_flex()
                .w_full()
                .gap(px(0.0))
                .child(permission_option_row(
                    "aws-sh",
                    "Shell commands (unsandboxed)",
                    "",
                    def_pol.shell_access == ShellAccess::FullAccess,
                    false,
                    p,
                    move |_, w, cx| h_sh(SettingsAction::ToggleAssistantShell, w, cx),
                ))
                .child(permission_option_row(
                    "aws-web",
                    "Allow web search and URL fetching",
                    "",
                    def_pol.web_access,
                    false,
                    p,
                    move |_, w, cx| h_web(SettingsAction::ToggleAssistantWeb, w, cx),
                ))
                .child(permission_option_row(
                    "aws-cu",
                    "Computer control",
                    "",
                    def_pol.computer_access,
                    false,
                    p,
                    move |_, w, cx| h_cu(SettingsAction::ToggleAssistantComputer, w, cx),
                )),
        )
        .child(h_divider(p))
        .child(
            v_flex()
                .w_full()
                .gap(px(0.0))
                .child(div().pb(px(2.0)).child(field_label("Approval", p)))
                .child(permission_option_row(
                    "aap-chg",
                    "Ask for changes",
                    "",
                    def_pol.approval_mode == ToolApprovalMode::AskForChanges,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ap1(
                            SettingsAction::SetAssistantApprovalMode(
                                ToolApprovalMode::AskForChanges,
                            ),
                            w,
                            cx,
                        )
                    },
                ))
                .child(permission_option_row(
                    "aap-all",
                    "Ask before every tool",
                    "",
                    def_pol.approval_mode == ToolApprovalMode::AskEveryTool,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ap2(
                            SettingsAction::SetAssistantApprovalMode(
                                ToolApprovalMode::AskEveryTool,
                            ),
                            w,
                            cx,
                        )
                    },
                ))
                .child(permission_option_row(
                    "aap-nev",
                    "Never ask",
                    "",
                    def_pol.approval_mode == ToolApprovalMode::Never,
                    true,
                    p,
                    move |_, w, cx| {
                        h_ap3(
                            SettingsAction::SetAssistantApprovalMode(ToolApprovalMode::Never),
                            w,
                            cx,
                        )
                    },
                )),
        );

    // 4. Per-Assistant Tool Registry (`SettingsAssistantsPage.kt:259`)
    let asst_has_ws = !ws_path.trim().is_empty();
    let asst_can_read =
        asst_has_ws && matches!(ws_mode, WorkspaceMode::ReadOnly | WorkspaceMode::ReadWrite);
    let asst_can_write = asst_has_ws && ws_mode == WorkspaceMode::ReadWrite;
    let asst_can_shell = asst_has_ws && def_pol.shell_access == ShellAccess::FullAccess;
    let asst_builtin_tools: [(&str, &str, bool); 13] = [
        ("fs_list", "List files and folders", asst_can_read),
        ("fs_read", "Read lines from a workspace file", asst_can_read),
        ("fs_glob", "Match file paths by glob pattern", asst_can_read),
        (
            "fs_search",
            "Search text inside workspace files",
            asst_can_read,
        ),
        (
            "fs_write",
            "Create or replace workspace files",
            asst_can_write,
        ),
        (
            "fs_edit",
            "Edit a region of a workspace file",
            asst_can_write,
        ),
        ("shell_exec", "Run shell commands", asst_can_shell),
        ("web_search", "Search the public web", def_pol.web_access),
        ("web_fetch", "Fetch a public webpage", def_pol.web_access),
        (
            "computer_use",
            "Inspect and interact with the screen",
            def_pol.computer_access,
        ),
        ("agent_delegate", "Delegate sub-tasks in parallel", true),
        ("todo_write", "Maintain chat checklist (TODO.md)", true),
        ("agent_memory", "Update persistent memory/AGENT.md", true),
    ];
    let asst_avail = asst_builtin_tools
        .iter()
        .filter(|(n, _, ok)| *ok && def_pol.allows_tool(n))
        .count();
    let mut asst_reg_card = section_card(
        &format!(
            "Assistant tool registry ({asst_avail} of {} available)",
            asst_builtin_tools.len()
        ),
        None,
        p,
    );
    for (ix, (tname, tdesc, permitted)) in asst_builtin_tools.into_iter().enumerate() {
        let enabled = def_pol.allows_tool(tname);
        let active = permitted && enabled;
        let tn = tname.to_string();
        let h_t = on.clone();
        let hover_bg = p.hover_tint();
        let active_bg = p.active_tint();
        let next_en = !enabled;
        asst_reg_card = asst_reg_card.child(
            h_flex()
                .id(("asst-reg-tool", ix))
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .bg(p.elevated_bg.with_alpha(0.35))
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .active(move |s| s.bg(active_bg))
                .on_click(move |_, w, cx| {
                    h_t(
                        SettingsAction::ToggleAssistantBuiltinTool(tn.clone(), next_en),
                        w,
                        cx,
                    )
                })
                .child(
                    h_flex()
                        .flex_1()
                        .overflow_hidden()
                        .items_center()
                        .gap(px(8.0))
                        .child(kiss_icon(
                            tool_kiss_icon(tname),
                            14.0,
                            if active {
                                p.accent.to_hsla()
                            } else {
                                p.muted_ink.to_hsla()
                            },
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .font_family(p.code_font_family.clone())
                                        .text_size(px(12.5))
                                        .font_semibold()
                                        .text_color(if active {
                                            p.primary_ink.to_hsla()
                                        } else {
                                            p.muted_ink.to_hsla()
                                        })
                                        .child(tname),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(p.muted_ink.to_hsla())
                                        .child(tdesc),
                                ),
                        ),
                )
                .child(crate::ui_kit::sepia_toggle_track(
                    ("asst-reg-sw", ix),
                    enabled,
                    p,
                )),
        );
    }

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(header)
        .child(identity_card)
        .child(prompts_card)
        .child(perms_card)
        .child(asst_reg_card)
        .into_any_element()
}

// ============================================================================================
// Page 4: Default Model (`SettingsDefaultModelPage.kt` vertical model picker + 4 cards)
// ============================================================================================
pub(super) fn render_default_model_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let dm = &props.snapshot.default_models;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("Default Model");

    let dm_q = props.default_model_filter_query.trim().to_ascii_lowercase();
    let vertical_model_picker =
        |id_prefix: &'static str, sel_pid: &str, sel_mid: &str, slot: Option<&'static str>| {
            let mut list = v_flex()
                .id(SharedString::from(format!("{id_prefix}-scroll")))
                .w_full()
                .max_h(px(210.0))
                .p(px(6.0))
                .rounded(px(8.0))
                .bg(p.sheet_bg.with_alpha(0.45))
                .border_1()
                .border_color(p.subtle_border())
                .gap(px(4.0))
                .overflow_y_scrollbar();
            let mut idx = 0usize;
            let hover_bg = p.hover_tint();
            for prof in &props.snapshot.profiles {
                let candidates: Vec<&str> = if !prof.saved_models.is_empty() {
                    prof.saved_models.iter().map(|s| s.as_str()).collect()
                } else if !prof.default_model.trim().is_empty() {
                    vec![prof.default_model.as_str()]
                } else {
                    Vec::new()
                };
                for mid in candidates {
                    if !dm_q.is_empty()
                        && !mid.to_ascii_lowercase().contains(&dm_q)
                        && !prof.name.to_ascii_lowercase().contains(&dm_q)
                    {
                        continue;
                    }
                    let is_sel = prof.id == sel_pid && mid == sel_mid;
                    let pid = prof.id.clone();
                    let m = mid.to_string();
                    let h = on.clone();
                    let spec = prof.custom_model_specs.get(mid);
                    let ctx_tok = spec
                        .map(|s| s.context_window_tokens)
                        .filter(|&v| v > 0)
                        .unwrap_or(prof.context_window_tokens);
                    let badge = primary_model_privacy_badge(prof, mid, spec);
                    let mut row = h_flex()
                        .id((id_prefix, idx))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(8.0))
                        .bg(if is_sel {
                            p.accent.with_alpha(0.13)
                        } else {
                            p.card_bg.to_hsla()
                        })
                        .border_1()
                        .border_color(if is_sel {
                            p.accent.to_hsla()
                        } else {
                            p.border.with_alpha(0.45)
                        })
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_, w, cx| {
                            if let Some(sl) = slot {
                                h(
                                    SettingsAction::SetUtilityModel {
                                        slot: sl,
                                        profile_id: pid.clone(),
                                        model_id: m.clone(),
                                    },
                                    w,
                                    cx,
                                );
                            } else {
                                h(
                                    SettingsAction::SetGlobalDefaultModel {
                                        profile_id: pid.clone(),
                                        model_id: m.clone(),
                                    },
                                    w,
                                    cx,
                                );
                            }
                        });
                    if is_sel {
                        row = row.child(rubric_mark_overlay(5.0, p));
                    }
                    list =
                        list.child(
                            row.child(
                                h_flex()
                                    .flex_1()
                                    .overflow_hidden()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(model_lab_icon(
                                        mid,
                                        14.0,
                                        if is_sel {
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
                                            .text_size(px(12.5))
                                            .font_medium()
                                            .text_color(if is_sel {
                                                p.accent.to_hsla()
                                            } else {
                                                p.primary_ink.to_hsla()
                                            })
                                            .child(mid.to_string()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .flex_shrink_0()
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded(px(5.0))
                                            .bg(p.sheet_bg.to_hsla())
                                            .text_size(px(10.5))
                                            .text_color(p.narration_ink.to_hsla())
                                            .child(prof.name.clone()),
                                    )
                                    .children(badge.map(|b| model_privacy_badge_chip(&b, p)))
                                    .children((ctx_tok > 0).then(|| {
                                        div()
                                            .px(px(5.0))
                                            .py(px(2.0))
                                            .rounded(px(5.0))
                                            .bg(p.elevated_bg.to_hsla())
                                            .font_family(p.code_font_family.clone())
                                            .text_size(px(10.5))
                                            .text_color(p.muted_ink.to_hsla())
                                            .child(format_tokens_short(ctx_tok as i64))
                                    }))
                                    .children(is_sel.then(|| {
                                        kiss_icon(KissIcon::Check, 13.0, p.accent.to_hsla())
                                    })),
                            ),
                        );
                    idx += 1;
                }
            }
            if idx == 0 {
                list = list.child(
                    div()
                        .p(px(8.0))
                        .text_size(px(12.0))
                        .text_color(p.muted_ink.to_hsla())
                        .child("No models configured yet."),
                );
            }
            list
        };

    let c1 = section_card("1. Global default model", None, p)
        .child(
            Input::new(&props.inputs.default_model_filter).prefix(kiss_icon(
                KissIcon::Search,
                13.0,
                p.muted_ink.to_hsla(),
            )),
        )
        .child(vertical_model_picker(
            "dm-glob",
            &dm.global_profile_id,
            &dm.global_model_id,
            None,
        ));

    let h_tf = on.clone();
    let mut c2 = section_card("2. Title generation model", None, p).child(setting_toggle_row(
        "dm-title-glob",
        "Follow global model",
        "",
        dm.title_follow_global,
        p,
        move |v, w, cx| {
            h_tf(
                SettingsAction::ToggleDefaultModelFlag("title_follow_global", *v),
                w,
                cx,
            )
        },
    ));
    if !dm.title_follow_global {
        c2 = c2.child(vertical_model_picker(
            "dm-title",
            &dm.title_profile_id,
            &dm.title_model_id,
            Some("title"),
        ));
    }
    c2 = c2
        .child(field_label("Title prompt", p))
        .child(Textarea::new(&props.inputs.title_prompt).h(px(64.0)));
    let h_sf = on.clone();
    let mut c3 =
        section_card("3. Summary & context compression model", None, p).child(setting_toggle_row(
            "dm-sum-glob",
            "Follow global model",
            "",
            dm.summary_follow_global,
            p,
            move |v, w, cx| {
                h_sf(
                    SettingsAction::ToggleDefaultModelFlag("summary_follow_global", *v),
                    w,
                    cx,
                )
            },
        ));
    if !dm.summary_follow_global {
        c3 = c3.child(vertical_model_picker(
            "dm-sum",
            &dm.summary_profile_id,
            &dm.summary_model_id,
            Some("summary"),
        ));
    }

    let h_trf = on.clone();
    let mut c4 = section_card("4. Translation model & target language", None, p)
        .child(field_label("Target language", p))
        .child(Input::new(&props.inputs.translation_lang))
        .child(
            h_flex().flex_wrap().gap(px(6.0)).children(
                [
                    "English",
                    "Vietnamese",
                    "Japanese",
                    "Chinese",
                    "Korean",
                    "Spanish",
                    "French",
                    "German",
                ]
                .into_iter()
                .enumerate()
                .map(|(ix, lang)| {
                    let l = lang.to_string();
                    let h = on.clone();
                    sepia_chip(
                        ("tr-lang", ix),
                        lang,
                        None,
                        dm.translation_target_language.eq_ignore_ascii_case(lang),
                        p,
                        move |_, w, cx| h(SettingsAction::SetTranslationLanguage(l.clone()), w, cx),
                    )
                }),
            ),
        )
        .child(setting_toggle_row(
            "dm-tr-glob",
            "Follow global model",
            "",
            dm.translation_follow_global,
            p,
            move |v, w, cx| {
                h_trf(
                    SettingsAction::ToggleDefaultModelFlag("translation_follow_global", *v),
                    w,
                    cx,
                )
            },
        ));
    if !dm.translation_follow_global {
        c4 = c4.child(vertical_model_picker(
            "dm-tr",
            &dm.translation_profile_id,
            &dm.translation_model_id,
            Some("translation"),
        ));
    }

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(c1)
        .child(c2)
        .child(c3)
        .child(c4)
        .into_any_element()
}

// ============================================================================================
// Page 5: Workspace (`desktop-page-works.png` 1:1)
// ============================================================================================
