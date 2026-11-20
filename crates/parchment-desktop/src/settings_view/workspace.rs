use super::*;

pub(super) fn render_workspace_page(props: &SettingsRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let pol = props.resolved_policy;
    let on = props.on_action;

    let heading = div()
        .text_size(px(19.0))
        .font_semibold()
        .text_color(p.primary_ink.to_hsla())
        .child("Workspace");

    // 1. Agent Permission Editor (`AgentPermissionEditor.kt` 48px rows)
    let h_pick = on.clone();
    let h_clr = on.clone();
    let h_reset = on.clone();
    let h_off = on.clone();
    let h_ro = on.clone();
    let h_rw = on.clone();
    let h_sh = on.clone();
    let h_web = on.clone();
    let h_cu = on.clone();
    let h_a1 = on.clone();
    let h_a2 = on.clone();
    let h_a3 = on.clone();
    let h_ten = on.clone();
    let ws_tools_enabled = pol.tools_enabled;
    let perms_card = section_card_with_action(
        "Workspace & agent permissions",
        None,
        props
            .active_session
            .permission_overrides
            .is_some()
            .then(|| {
                sepia_button(
                    "ws-reset-asst",
                    "Follow assistant defaults",
                    None,
                    SepiaButtonStyle::Ghost,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_reset(SettingsAction::ResetChatWorkspaceToAssistant, w, cx),
                )
                .into_any_element()
            }),
        p,
    )
    .child(permission_option_row(
        "ws-tools-en",
        "Enable agent tools",
        if ws_tools_enabled {
            "On for this chat"
        } else {
            "Off"
        },
        ws_tools_enabled,
        false,
        p,
        move |_, w, cx| h_ten(SettingsAction::ToggleToolsEnabled, w, cx),
    ))
    .child(h_divider(p))
    .child({
        let ws_folders = pol.all_workspace_paths();
        let mut ws_box = v_flex().w_full().gap(px(6.0));
        if ws_folders.is_empty() {
            ws_box = ws_box.child(
                div()
                    .text_size(px(13.0))
                    .text_color(p.muted_ink.to_hsla())
                    .child("No folders mounted"),
            );
        } else {
            for (f_ix, folder) in ws_folders.iter().enumerate() {
                let fp = folder.clone();
                let h_rm = on.clone();
                ws_box = ws_box.child(
                    h_flex()
                        .id(("set-ws-row", f_ix))
                        .w_full()
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(8.0))
                        .bg(p.elevated_bg.with_alpha(0.45))
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
                                        .text_size(px(12.5))
                                        .text_color(p.primary_ink.to_hsla())
                                        .child(folder.clone()),
                                ),
                        )
                        .child(icon_button(
                            ("set-ws-rm", f_ix),
                            KissIcon::Close,
                            false,
                            p,
                            move |_, w, cx| {
                                h_rm(SettingsAction::RemoveWorkspaceFolder(fp.clone()), w, cx)
                            },
                        )),
                );
            }
        }
        ws_box.child(
            h_flex()
                .gap(px(6.0))
                .child(sepia_button(
                    "set-ws-pick",
                    if ws_folders.is_empty() {
                        strings::CHOOSE_FOLDER
                    } else {
                        "Add folder"
                    },
                    Some(KissIcon::Plus),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_pick(SettingsAction::PickWorkspaceFolder, w, cx),
                ))
                .children((!ws_folders.is_empty()).then(|| {
                    sepia_button(
                        "set-ws-clr",
                        if ws_folders.len() > 1 {
                            "Clear all"
                        } else {
                            strings::CLEAR_FOLDER
                        },
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_clr(SettingsAction::ClearWorkspaceFolder, w, cx),
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
                "perm-ws-off",
                "Off",
                "",
                pol.workspace_mode == WorkspaceMode::Off,
                true,
                p,
                move |_, w, cx| h_off(SettingsAction::SetWorkspaceMode(WorkspaceMode::Off), w, cx),
            ))
            .child(permission_option_row(
                "perm-ws-ro",
                "Read files",
                "",
                pol.workspace_mode == WorkspaceMode::ReadOnly,
                true,
                p,
                move |_, w, cx| {
                    h_ro(
                        SettingsAction::SetWorkspaceMode(WorkspaceMode::ReadOnly),
                        w,
                        cx,
                    )
                },
            ))
            .child(permission_option_row(
                "perm-ws-rw",
                "Read and edit files",
                "",
                pol.workspace_mode == WorkspaceMode::ReadWrite,
                true,
                p,
                move |_, w, cx| {
                    h_rw(
                        SettingsAction::SetWorkspaceMode(WorkspaceMode::ReadWrite),
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
                "perm-shell",
                "Shell commands (unsandboxed)",
                "",
                pol.shell_access == ShellAccess::FullAccess,
                false,
                p,
                move |_, w, cx| h_sh(SettingsAction::ToggleShellAccess, w, cx),
            ))
            .child(permission_option_row(
                "perm-web",
                "Allow web search and URL fetching",
                "",
                pol.web_access,
                false,
                p,
                move |_, w, cx| h_web(SettingsAction::ToggleWebAccess, w, cx),
            ))
            .child(permission_option_row(
                "perm-cu",
                "Computer control",
                "",
                pol.computer_access,
                false,
                p,
                move |_, w, cx| h_cu(SettingsAction::ToggleComputerAccess, w, cx),
            )),
    )
    .child(h_divider(p))
    .child(
        v_flex()
            .w_full()
            .gap(px(0.0))
            .child(div().pb(px(2.0)).child(field_label("Approval", p)))
            .child(permission_option_row(
                "perm-ap-chg",
                "Ask for changes",
                "",
                pol.approval_mode == ToolApprovalMode::AskForChanges,
                true,
                p,
                move |_, w, cx| {
                    h_a1(
                        SettingsAction::SetApprovalMode(ToolApprovalMode::AskForChanges),
                        w,
                        cx,
                    )
                },
            ))
            .child(permission_option_row(
                "perm-ap-all",
                "Ask before every tool",
                "",
                pol.approval_mode == ToolApprovalMode::AskEveryTool,
                true,
                p,
                move |_, w, cx| {
                    h_a2(
                        SettingsAction::SetApprovalMode(ToolApprovalMode::AskEveryTool),
                        w,
                        cx,
                    )
                },
            ))
            .child(permission_option_row(
                "perm-ap-nev",
                "Never ask",
                "",
                pol.approval_mode == ToolApprovalMode::Never,
                true,
                p,
                move |_, w, cx| {
                    h_a3(
                        SettingsAction::SetApprovalMode(ToolApprovalMode::Never),
                        w,
                        cx,
                    )
                },
            )),
    );

    // 2. Global Agent Memory & Auto-loaded Project Memory
    let h_gmem = on.clone();
    let mut mem_card = section_card("Agent memory & project instructions", None, p).child(
        h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .child(
                v_flex()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_medium()
                            .text_color(p.primary_ink.to_hsla())
                            .child("Global memory (memory/AGENT.md)"),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(p.muted_ink.to_hsla())
                            .child(match props.global_memory_bytes {
                                Some(sz) => format!("Active · {}", format_bytes_compact(sz)),
                                None => "Not created yet".to_string(),
                            }),
                    ),
            )
            .child(sepia_button(
                "ws-gmem-btn",
                if props.global_memory_bytes.is_some() {
                    "Inspect"
                } else {
                    "Create AGENT.md"
                },
                Some(KissIcon::FileText),
                SepiaButtonStyle::Outline,
                true,
                false,
                p,
                move |_, w, cx| h_gmem(SettingsAction::OpenOrCreateGlobalMemory, w, cx),
            )),
    );
    if !props.project_memory_files.is_empty() {
        mem_card = mem_card.child(field_label("Detected project instruction files", p));
        for (ix, (name, full_p)) in props.project_memory_files.iter().enumerate() {
            let fp = full_p.clone();
            let h_pm = on.clone();
            mem_card = mem_card.child(
                h_flex()
                    .id(("proj-mem", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .font_family(p.code_font_family.clone())
                            .text_size(px(12.0))
                            .text_color(p.primary_ink.to_hsla())
                            .child(name.clone()),
                    )
                    .child(sepia_button(
                        ("proj-mem-open", ix),
                        "Inspect",
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_pm(SettingsAction::OpenProjectMemoryFile(fp.clone()), w, cx)
                        },
                    )),
            );
        }
    }

    // 3. MCP Servers Card
    let h_mcp_add = on.clone();
    let h_mcp_imp = on.clone();
    let h_mcp_rst = on.clone();
    let mut mcp_card = section_card_with_action(
        "MCP servers",
        None,
        Some(
            h_flex()
                .gap(px(6.0))
                .child(sepia_button(
                    "mcp-import-json",
                    "Import JSON",
                    Some(KissIcon::Upload),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_mcp_imp(SettingsAction::ImportMcpJsonFile, w, cx),
                ))
                .child(sepia_button(
                    "mcp-restart",
                    "Restart all",
                    Some(KissIcon::Refresh),
                    SepiaButtonStyle::Ghost,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_mcp_rst(SettingsAction::RestartMcpServers, w, cx),
                ))
                .into_any_element(),
        ),
        p,
    );

    for (ix, srv) in props.snapshot.preferences.mcp_servers.iter().enumerate() {
        let sid_t = srv.id.clone();
        let sid_d = srv.id.clone();
        let h_t = on.clone();
        let h_d = on.clone();
        let transport = if !srv.url.trim().is_empty() {
            format!("HTTP · {}", srv.url)
        } else {
            format!("Stdio · {}", srv.command)
        };
        let (status_text, status_color) = if !srv.enabled {
            ("Disabled".to_string(), p.muted_ink.to_hsla())
        } else if let Some(st) = props.mcp_statuses.get(&srv.id) {
            match st.state {
                McpServerState::Ready => (
                    format!(
                        "Connected · {} tool{}",
                        st.tool_count,
                        if st.tool_count == 1 { "" } else { "s" }
                    ),
                    p.success.to_hsla(),
                ),
                McpServerState::Connecting => ("Connecting…".to_string(), p.accent.to_hsla()),
                McpServerState::Failed => (
                    if st.message.trim().is_empty() {
                        "Connection failed".to_string()
                    } else {
                        st.message.clone()
                    },
                    p.danger.to_hsla(),
                ),
            }
        } else {
            ("Waiting to connect".to_string(), p.muted_ink.to_hsla())
        };
        mcp_card = mcp_card.child(
            h_flex()
                .id(("mcp-row", ix))
                .w_full()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(p.elevated_bg.with_alpha(0.45))
                .border_1()
                .border_color(p.subtle_border())
                .justify_between()
                .items_center()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .flex_1()
                        .overflow_hidden()
                        .gap(px(2.0))
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .font_semibold()
                                        .text_color(p.primary_ink.to_hsla())
                                        .child(srv.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .font_medium()
                                        .text_color(status_color)
                                        .child(status_text),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_family(p.code_font_family.clone())
                                .text_size(px(11.0))
                                .text_color(p.muted_ink.to_hsla())
                                .child(transport),
                        ),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(sepia_toggle_switch(
                            ("mcp-en", ix),
                            srv.enabled,
                            p,
                            move |v, w, cx| {
                                h_t(SettingsAction::ToggleMcpServer(sid_t.clone(), *v), w, cx)
                            },
                        ))
                        .child(icon_button(
                            ("mcp-del", ix),
                            KissIcon::Trash,
                            false,
                            p,
                            move |_, w, cx| {
                                h_d(SettingsAction::DeleteMcpServer(sid_d.clone()), w, cx)
                            },
                        )),
                ),
        );
    }

    let h_mcp_browse = on.clone();
    mcp_card = mcp_card
        .child(
            h_flex()
                .gap(px(8.0))
                .child(
                    v_flex()
                        .w(px(150.0))
                        .gap(px(3.0))
                        .child(field_label("Server name", p))
                        .child(Input::new(&props.inputs.mcp_name)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("HTTP URL (or leave blank for stdio)", p))
                        .child(Input::new(&props.inputs.mcp_url)),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.0))
                .items_end()
                .child(
                    v_flex()
                        .flex_1()
                        .gap(px(3.0))
                        .child(field_label("Local stdio command", p))
                        .child(
                            h_flex()
                                .gap(px(6.0))
                                .child(div().flex_1().child(Input::new(&props.inputs.mcp_command)))
                                .child(sepia_button(
                                    "mcp-browse-cmd",
                                    "Browse…",
                                    Some(KissIcon::Folder),
                                    SepiaButtonStyle::Outline,
                                    true,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_mcp_browse(SettingsAction::BrowseMcpCommand, w, cx)
                                    },
                                )),
                        ),
                )
                .child(
                    v_flex()
                        .w(px(170.0))
                        .gap(px(3.0))
                        .child(field_label("Bearer token (optional)", p))
                        .child(Input::new(&props.inputs.mcp_bearer)),
                )
                .child(sepia_button(
                    "mcp-add-btn",
                    "Add server",
                    Some(KissIcon::Plus),
                    SepiaButtonStyle::Primary,
                    false,
                    false,
                    p,
                    move |_, w, cx| h_mcp_add(SettingsAction::AddMcpServer, w, cx),
                )),
        );

    // Public MCP Directory Search (`McpSettings.kt` McpServerBrowser)
    let h_dir_search = on.clone();
    let h_dir_notok = on.clone();
    mcp_card = mcp_card
        .child(h_divider(p))
        .child(field_label("Find a server (MCP Registry)", p))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(8.0))
                .child(div().flex_1().child(
                    Input::new(&props.inputs.mcp_registry_search).prefix(kiss_icon(
                        KissIcon::Search,
                        13.0,
                        p.muted_ink.to_hsla(),
                    )),
                ))
                .child(sepia_chip(
                    "mcp-reg-notok",
                    "No token needed",
                    None,
                    props.mcp_registry_no_token_only,
                    p,
                    move |_, w, cx| {
                        h_dir_notok(SettingsAction::ToggleMcpRegistryNoTokenOnly, w, cx)
                    },
                ))
                .child(sepia_button(
                    "mcp-reg-search",
                    if props.is_searching_mcp_registry {
                        "Searching…"
                    } else {
                        "Search"
                    },
                    Some(KissIcon::Search),
                    SepiaButtonStyle::Outline,
                    true,
                    props.is_searching_mcp_registry,
                    p,
                    move |_, w, cx| h_dir_search(SettingsAction::SearchMcpRegistry, w, cx),
                )),
        );
    if !props.mcp_registry_results.is_empty() {
        let mut dir_list = v_flex()
            .id("mcp-dir-scroll")
            .max_h(px(210.0))
            .w_full()
            .gap(px(4.0))
            .overflow_y_scrollbar();
        for (ix, item) in props.mcp_registry_results.iter().enumerate() {
            if props.mcp_registry_no_token_only && item.needs_token {
                continue;
            }
            let already_added = props
                .snapshot
                .preferences
                .mcp_servers
                .iter()
                .any(|s| s.url.eq_ignore_ascii_case(&item.url));
            let title = if item.title.is_empty() {
                item.name.clone()
            } else {
                item.title.clone()
            };
            let add_name = title.clone();
            let add_url = item.url.clone();
            let h_add = on.clone();
            dir_list = dir_list.child(
                h_flex()
                    .id(("mcp-dir-item", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.38))
                    .justify_between()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.0))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(format!(
                                        "{}{}",
                                        if item.needs_token {
                                            "Needs a token · "
                                        } else {
                                            ""
                                        },
                                        if item.description.is_empty() {
                                            item.url.as_str()
                                        } else {
                                            item.description.as_str()
                                        }
                                    )),
                            ),
                    )
                    .child(sepia_button(
                        ("mcp-dir-add", ix),
                        if already_added { "Added" } else { "Add" },
                        if already_added {
                            Some(KissIcon::Check)
                        } else {
                            Some(KissIcon::Plus)
                        },
                        SepiaButtonStyle::Outline,
                        true,
                        already_added,
                        p,
                        move |_, w, cx| {
                            h_add(
                                SettingsAction::AddMcpRegistryServer {
                                    name: add_name.clone(),
                                    url: add_url.clone(),
                                },
                                w,
                                cx,
                            )
                        },
                    )),
            );
        }
        mcp_card = mcp_card.child(dir_list);
    }

    // 4. Agent Skills Card (`McpSettings.kt` SkillInstallCard: GitHub URL, Folder, File)
    let h_sk_add = on.clone();
    let h_sk_dir = on.clone();
    let h_sk_url = on.clone();
    let mut skills_card = section_card_with_action(
        &format!("Agent skills ({})", props.installed_skills.len()),
        None,
        Some(
            h_flex()
                .gap(px(6.0))
                .child(sepia_button(
                    "skill-install-folder",
                    "Import folder",
                    Some(KissIcon::Folder),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_sk_dir(SettingsAction::InstallSkillFolder, w, cx),
                ))
                .child(sepia_button(
                    "skill-install-btn",
                    "Install .md / .zip",
                    Some(KissIcon::Plus),
                    SepiaButtonStyle::Outline,
                    true,
                    false,
                    p,
                    move |_, w, cx| h_sk_add(SettingsAction::InstallSkillFile, w, cx),
                ))
                .into_any_element(),
        ),
        p,
    )
    .child(
        h_flex()
            .w_full()
            .items_end()
            .gap(px(8.0))
            .child(
                v_flex()
                    .flex_1()
                    .gap(px(3.0))
                    .child(field_label("Install from GitHub URL", p))
                    .child(Input::new(&props.inputs.skill_github_url)),
            )
            .child(sepia_button(
                "skill-install-url",
                "Install",
                Some(KissIcon::Download),
                SepiaButtonStyle::Primary,
                false,
                false,
                p,
                move |_, w, cx| h_sk_url(SettingsAction::InstallSkillFromUrl, w, cx),
            )),
    );
    if props.installed_skills.is_empty() {
        skills_card = skills_card.child(
            div()
                .text_size(px(12.5))
                .text_color(p.muted_ink.to_hsla())
                .child("No skills installed yet. Save any assistant reply as a skill or import a `.md` file."),
        );
    } else {
        for (ix, sk) in props.installed_skills.iter().enumerate() {
            let sp_open = sk.path.clone();
            let sp_del = sk.path.clone();
            let h_open = on.clone();
            let h_del = on.clone();
            skills_card = skills_card.child(
                h_flex()
                    .id(("sk-row", ix))
                    .w_full()
                    .px(px(10.0))
                    .py(px(7.0))
                    .rounded(px(8.0))
                    .bg(p.elevated_bg.with_alpha(0.45))
                    .justify_between()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .overflow_hidden()
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_medium()
                                    .text_color(p.primary_ink.to_hsla())
                                    .child(sk.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(p.muted_ink.to_hsla())
                                    .child(sk.source_label.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.0))
                            .child(icon_button(
                                ("sk-open", ix),
                                KissIcon::Eye,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_open(SettingsAction::InspectSkillFile(sp_open.clone()), w, cx)
                                },
                            ))
                            .child(icon_button(
                                ("sk-del", ix),
                                KissIcon::Trash,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_del(SettingsAction::DeleteSkillFile(sp_del.clone()), w, cx)
                                },
                            )),
                    ),
            );
        }
    }

    // 5. Agent Tool Registry (`X of Y tools available`)
    let has_ws = !pol.workspace_path.trim().is_empty();
    let ws_ro = has_ws
        && matches!(
            pol.workspace_mode,
            WorkspaceMode::ReadOnly | WorkspaceMode::ReadWrite
        );
    let ws_rw = has_ws && pol.workspace_mode == WorkspaceMode::ReadWrite;
    let shell_ok = ws_rw && pol.shell_access == ShellAccess::FullAccess;
    let builtin_tools: [(&str, &str, bool, &str); 13] = [
        (
            "fs_read",
            "Read file slices in workspace",
            ws_ro,
            "Enable Read only or Read & write workspace",
        ),
        (
            "fs_list",
            "List workspace directory",
            ws_ro,
            "Enable Read only or Read & write workspace",
        ),
        (
            "fs_glob",
            "Match files by glob pattern",
            ws_ro,
            "Enable Read only or Read & write workspace",
        ),
        (
            "fs_search",
            "Search text inside workspace files",
            ws_ro,
            "Enable Read only or Read & write workspace",
        ),
        (
            "fs_write",
            "Create or overwrite workspace files",
            ws_rw,
            "Enable Read & write workspace",
        ),
        (
            "fs_edit",
            "Apply exact edits to workspace files",
            ws_rw,
            "Enable Read & write workspace",
        ),
        (
            "shell_exec",
            "Run terminal commands",
            shell_ok,
            "Enable Read & write workspace + Shell access",
        ),
        (
            "web_search",
            "Search public web",
            pol.web_access,
            "Enable Web search & fetch",
        ),
        (
            "web_fetch",
            "Fetch readable web pages",
            pol.web_access,
            "Enable Web search & fetch",
        ),
        (
            "computer_use",
            "Inspect screen & desktop actions",
            pol.computer_access,
            "Enable Computer control",
        ),
        (
            "agent_delegate",
            "Delegate sub-tasks in parallel (max 4)",
            true,
            "",
        ),
        ("todo_write", "Maintain chat checklist (TODO.md)", true, ""),
        (
            "agent_memory",
            "Update persistent memory/AGENT.md",
            true,
            "",
        ),
    ];
    let avail_count = builtin_tools
        .iter()
        .filter(|(name, _, permitted, _)| *permitted && pol.allows_tool(name))
        .count();

    let mut reg_card = section_card(
        &format!(
            "Agent tool registry ({avail_count} of {} available)",
            builtin_tools.len()
        ),
        None,
        p,
    )
    .child(
        Input::new(&props.inputs.tool_registry_search).prefix(kiss_icon(
            KissIcon::Search,
            13.0,
            p.muted_ink.to_hsla(),
        )),
    );
    let tool_q = props.tool_registry_query.trim().to_ascii_lowercase();
    for (ix, (name, desc, permitted, denial)) in builtin_tools.into_iter().enumerate() {
        if !tool_q.is_empty()
            && !name.to_ascii_lowercase().contains(&tool_q)
            && !desc.to_ascii_lowercase().contains(&tool_q)
        {
            continue;
        }
        let enabled = pol.allows_tool(name);
        let active = permitted && enabled;
        let tname = name.to_string();
        let h_tog = on.clone();
        let hover_bg = p.hover_tint();
        let active_bg = p.active_tint();
        let next_en = !enabled;
        reg_card = reg_card.child(
            h_flex()
                .id(("reg-tool", ix))
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
                    h_tog(
                        SettingsAction::ToggleBuiltinTool(tname.clone(), next_en),
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
                            tool_kiss_icon(name),
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
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(p.muted_ink.to_hsla())
                                        .child(if permitted { desc } else { denial }),
                                ),
                        ),
                )
                .child(crate::ui_kit::sepia_toggle_track(
                    ("reg-sw", ix),
                    enabled,
                    p,
                )),
        );
    }

    v_flex()
        .w_full()
        .gap(px(14.0))
        .child(heading)
        .child(perms_card)
        .child(mem_card)
        .child(mcp_card)
        .child(skills_card)
        .child(reg_card)
        .into_any_element()
}

// ============================================================================================
// Page 6: World Book & Cards (`desktop-page-world.png` 1:1)
// ============================================================================================
