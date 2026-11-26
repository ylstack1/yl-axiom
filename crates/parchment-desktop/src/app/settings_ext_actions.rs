use super::*;

impl ParchmentDesktopApp {
    pub(super) fn handle_extended_settings_action(
        &mut self,
        action: SettingsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            // ---- Workspace, Memory, MCP, Skills & Tool Registry ----
            SettingsAction::ToggleToolsEnabled => {
                self.handle_composer_action(ComposerAction::ToggleToolsEnabled, window, cx);
            }
            SettingsAction::PickWorkspaceFolder => {
                self.handle_composer_action(ComposerAction::PickWorkspaceFolder, window, cx);
            }
            SettingsAction::RemoveWorkspaceFolder(path) => {
                self.handle_composer_action(
                    ComposerAction::RemoveWorkspaceFolder(path),
                    window,
                    cx,
                );
            }
            SettingsAction::ClearWorkspaceFolder => {
                self.handle_composer_action(ComposerAction::ClearWorkspaceFolder, window, cx);
            }
            SettingsAction::ResetChatWorkspaceToAssistant => {
                self.active_session.permission_overrides = None;
                if let Some(asst) = self.active_assistant().cloned() {
                    self.active_session.workspace_path = asst.workspace_path;
                    self.active_session.workspace_mode = asst.workspace_mode;
                } else {
                    self.active_session.workspace_path.clear();
                    self.active_session.workspace_mode = WorkspaceMode::Off;
                }
                self.persist_active_session();
                self.refresh_workspace_files();
                self.show_notice(
                    "Reset chat workspace permissions to assistant defaults.",
                    cx,
                );
            }
            SettingsAction::SetWorkspaceMode(m) => {
                self.handle_composer_action(ComposerAction::SetWorkspaceMode(m), window, cx);
            }
            SettingsAction::ToggleShellAccess => {
                let next = if self.resolved_policy().shell_access == ShellAccess::FullAccess {
                    ShellAccess::Disabled
                } else {
                    ShellAccess::FullAccess
                };
                self.handle_composer_action(ComposerAction::SetShellAccess(next), window, cx);
            }
            SettingsAction::ToggleWebAccess => {
                self.handle_composer_action(ComposerAction::ToggleWebAccess, window, cx);
            }
            SettingsAction::ToggleComputerAccess => {
                self.handle_composer_action(ComposerAction::ToggleComputerAccess, window, cx);
            }
            SettingsAction::SetApprovalMode(m) => {
                self.handle_composer_action(ComposerAction::SetApprovalMode(m), window, cx);
            }
            SettingsAction::ToggleBuiltinTool(name, enabled) => {
                self.update_active_permissions(
                    |p| {
                        p.tool_overrides.insert(name, enabled);
                    },
                    cx,
                );
            }
            SettingsAction::OpenOrCreateGlobalMemory => {
                let mem_dir = self.data_dir.join("memory");
                let _ = std::fs::create_dir_all(&mem_dir);
                let mem_path = mem_dir.join("AGENT.md");
                if !mem_path.exists() {
                    let _ = std::fs::write(
                        &mem_path,
                        "# Agent Memory\n\nPersistent preferences and notes remembered across chats.\n",
                    );
                }
                self.refresh_memory_and_skills();
                self.open_file_path_in_viewer(&mem_path.to_string_lossy(), cx);
            }
            SettingsAction::OpenProjectMemoryFile(path) => {
                self.open_file_path_in_viewer(&path, cx);
            }
            SettingsAction::AddMcpServer => {
                let name = self
                    .settings_inputs
                    .mcp_name
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let url = self
                    .settings_inputs
                    .mcp_url
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let cmd = self
                    .settings_inputs
                    .mcp_command
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let bearer = self
                    .settings_inputs
                    .mcp_bearer
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if name.is_empty() || (url.is_empty() && cmd.is_empty()) {
                    self.show_notice(
                        "Enter a server name and either an HTTP URL or stdio command.",
                        cx,
                    );
                    return;
                }
                self.snapshot.preferences.mcp_servers.push(McpServerConfig {
                    id: format!("mcp-{}", now_ms()),
                    name,
                    url,
                    command: cmd,
                    bearer_token: bearer,
                    enabled: true,
                });
                self.settings_inputs
                    .mcp_name
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.settings_inputs
                    .mcp_url
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.settings_inputs
                    .mcp_command
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.settings_inputs
                    .mcp_bearer
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.persist_settings();
                self.sync_mcp_servers_async(window, cx);
                cx.notify();
            }
            SettingsAction::BrowseMcpCommand => {
                if let Some(path) = pick_open_file_dialog(
                    "Choose local MCP executable or script",
                    "Executables & scripts",
                    "*.exe;*.cmd;*.bat;*.ps1;*.py;*.js;*.mjs;*.*",
                ) {
                    let s = path.to_string_lossy().to_string();
                    let quoted = if s.contains(' ') {
                        format!("\"{s}\"")
                    } else {
                        s
                    };
                    self.settings_inputs
                        .mcp_command
                        .update(cx, |i, cx| i.set_value(&quoted, window, cx));
                }
            }
            SettingsAction::SearchMcpRegistry => {
                let q = self
                    .settings_inputs
                    .mcp_registry_search
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                self.is_searching_mcp_registry = true;
                self.pending_bg_tasks += 1;
                let http = self.core.http_client().clone();
                let tx = self.bg_tx.clone();
                self.core.runtime().spawn(async move {
                    let mut url =
                        "https://registry.modelcontextprotocol.io/v0/servers?limit=30".to_string();
                    if !q.is_empty() {
                        let enc: String = q
                            .bytes()
                            .map(|b| match b {
                                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                                    (b as char).to_string()
                                }
                                b' ' => "%20".into(),
                                _ => format!("%{b:02X}"),
                            })
                            .collect();
                        url.push_str("&search=");
                        url.push_str(&enc);
                    }
                    let res = async {
                        let resp = http
                            .get(&url)
                            .header("Accept", "application/json")
                            .send()
                            .await
                            .map_err(|e| e.to_string())?;
                        if !resp.status().is_success() {
                            return Err(format!("HTTP {}", resp.status().as_u16()));
                        }
                        let val: serde_json::Value =
                            resp.json().await.map_err(|e| e.to_string())?;
                        let mut out = Vec::new();
                        if let Some(arr) = val.get("servers").and_then(|v| v.as_array()) {
                            for entry in arr {
                                let srv = entry.get("server").unwrap_or(entry);
                                let name = srv
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let title = srv
                                    .get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let desc = srv
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if let Some(remotes) = srv.get("remotes").and_then(|v| v.as_array())
                                {
                                    for rem in remotes {
                                        if let Some(rurl) = rem.get("url").and_then(|v| v.as_str())
                                            && (rurl.starts_with("https://")
                                                || rurl.starts_with("http://"))
                                        {
                                            let needs_token = rem
                                                .get("headers")
                                                .and_then(|h| h.as_array())
                                                .map(|a| !a.is_empty())
                                                .unwrap_or(false);
                                            out.push(McpDirectoryItem {
                                                name: name.clone(),
                                                title: title.clone(),
                                                description: desc.clone(),
                                                url: rurl.to_string(),
                                                needs_token,
                                            });
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(out)
                    }
                    .await;
                    let _ = tx.send(BgTaskEvent::McpRegistrySearched(res));
                });
                self.ensure_pump_scheduled(window, cx);
                cx.notify();
            }
            SettingsAction::ToggleMcpRegistryNoTokenOnly => {
                self.mcp_registry_no_token_only = !self.mcp_registry_no_token_only;
                cx.notify();
            }
            SettingsAction::AddMcpRegistryServer { name, url } => {
                if !self
                    .snapshot
                    .preferences
                    .mcp_servers
                    .iter()
                    .any(|s| s.url.eq_ignore_ascii_case(&url))
                {
                    self.snapshot.preferences.mcp_servers.push(McpServerConfig {
                        id: format!("mcp-dir-{}", now_ms()),
                        name: name.clone(),
                        url,
                        command: String::new(),
                        bearer_token: String::new(),
                        enabled: true,
                    });
                    self.persist_settings();
                    self.sync_mcp_servers_async(window, cx);
                    self.show_notice(format!("Added MCP server \"{name}\"."), cx);
                }
            }
            SettingsAction::ImportMcpJsonFile => {
                let Some(path) =
                    pick_open_file_dialog("Import MCP server config", "JSON files", "*.json")
                else {
                    return;
                };
                match std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|txt| {
                        serde_json::from_str::<serde_json::Value>(&txt).map_err(|e| e.to_string())
                    }) {
                    Ok(val) => {
                        let servers_obj = val.get("mcpServers").and_then(|v| v.as_object());
                        let mut added = 0usize;
                        let mut skipped_local = 0usize;
                        if let Some(map) = servers_obj {
                            for (name, cfg) in map {
                                let url = cfg
                                    .get("url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                let cmd = cfg
                                    .get("command")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                // Stable contract: imports drop local commands & disable remaining MCP servers.
                                if !cmd.is_empty() && url.is_empty() {
                                    skipped_local += 1;
                                    continue;
                                }
                                if !url.is_empty() {
                                    self.snapshot.preferences.mcp_servers.push(McpServerConfig {
                                        id: format!("mcp-imp-{}-{added}", now_ms()),
                                        name: name.clone(),
                                        url,
                                        command: String::new(),
                                        bearer_token: String::new(),
                                        enabled: false,
                                    });
                                    added += 1;
                                }
                            }
                        }
                        self.persist_settings();
                        self.show_notice(
                            format!(
                                "Imported {added} HTTP MCP server(s) disabled by default ({skipped_local} local command entry skipped for safety)."
                            ),
                            cx,
                        );
                    }
                    Err(e) => self.show_notice(format!("MCP import failed: {e}"), cx),
                }
            }
            SettingsAction::ToggleMcpServer(sid, enabled) => {
                if let Some(s) = self
                    .snapshot
                    .preferences
                    .mcp_servers
                    .iter_mut()
                    .find(|s| s.id == sid)
                {
                    s.enabled = enabled;
                }
                self.persist_settings();
                self.sync_mcp_servers_async(window, cx);
                cx.notify();
            }
            SettingsAction::RestartMcpServers => {
                self.sync_mcp_servers_async(window, cx);
                self.show_notice("Restarting enabled MCP servers…", cx);
            }
            SettingsAction::DeleteMcpServer(sid) => {
                self.snapshot
                    .preferences
                    .mcp_servers
                    .retain(|s| s.id != sid);
                self.persist_settings();
                self.sync_mcp_servers_async(window, cx);
                cx.notify();
            }
            SettingsAction::InstallSkillFile => {
                if let Some(src) = pick_open_file_dialog(
                    "Install skill (.md or .zip)",
                    "Skill files",
                    "*.md;*.zip",
                ) {
                    let skills_dir = self.data_dir.join("skills");
                    let _ = std::fs::create_dir_all(&skills_dir);
                    let ext = src
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if ext == "zip" {
                        match std::fs::read(&src)
                            .map_err(|e| e.to_string())
                            .and_then(|bytes| extract_md_skills_from_zip(&bytes))
                        {
                            Ok(files) => {
                                let mut installed = 0usize;
                                for (fname, content) in files {
                                    if std::fs::write(skills_dir.join(&fname), &content).is_ok() {
                                        installed += 1;
                                    }
                                }
                                self.refresh_memory_and_skills();
                                self.show_notice(
                                    format!("Installed {installed} skill(s) from ZIP archive."),
                                    cx,
                                );
                            }
                            Err(e) => {
                                self.show_notice(format!("Skill ZIP install failed: {e}"), cx)
                            }
                        }
                    } else {
                        let fname = src
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("skill.md");
                        let dest = skills_dir.join(fname);
                        match std::fs::copy(&src, &dest) {
                            Ok(_) => {
                                self.refresh_memory_and_skills();
                                self.show_notice(format!("Installed skill {fname}."), cx);
                            }
                            Err(e) => self.show_notice(format!("Skill install failed: {e}"), cx),
                        }
                    }
                }
            }
            SettingsAction::InstallSkillFolder => {
                if let Some(dir) = pick_folder_dialog("Import skills from folder") {
                    let skills_dir = self.data_dir.join("skills");
                    let _ = std::fs::create_dir_all(&skills_dir);
                    let mut count = 0usize;
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            if p.is_file()
                                && p.extension()
                                    .and_then(|e| e.to_str())
                                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                                && let Some(fname) = p.file_name()
                                && std::fs::copy(&p, skills_dir.join(fname)).is_ok()
                            {
                                count += 1;
                            }
                        }
                    }
                    self.refresh_memory_and_skills();
                    self.show_notice(
                        format!("Imported {count} Markdown skill(s) from folder."),
                        cx,
                    );
                }
            }
            SettingsAction::InstallSkillFromUrl => {
                let raw_url = self
                    .settings_inputs
                    .skill_github_url
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if raw_url.is_empty() {
                    self.show_notice("Enter a GitHub or Markdown URL first.", cx);
                    return;
                }
                let fetch_url = if let Some(rest) = raw_url.strip_prefix("https://github.com/") {
                    let normalized = rest.replacen("/blob/", "/", 1);
                    format!("https://raw.githubusercontent.com/{normalized}")
                } else {
                    raw_url.clone()
                };
                if !fetch_url.starts_with("https://") {
                    self.show_notice("Skill URL must start with https://", cx);
                    return;
                }
                let skills_dir = self.data_dir.join("skills");
                let http = self.core.http_client().clone();
                let tx = self.bg_tx.clone();
                self.pending_bg_tasks += 1;
                self.show_notice("Downloading skill…", cx);
                self.core.runtime().spawn(async move {
                    let res = async {
                        let resp = http
                            .get(&fetch_url)
                            .send()
                            .await
                            .map_err(|e| e.to_string())?;
                        if !resp.status().is_success() {
                            return Err(format!("HTTP {}", resp.status().as_u16()));
                        }
                        let text = resp.text().await.map_err(|e| e.to_string())?;
                        let _ = std::fs::create_dir_all(&skills_dir);
                        let seg = fetch_url
                            .rsplit('/')
                            .next()
                            .filter(|s| !s.is_empty())
                            .unwrap_or("github-skill.md");
                        let fname = if seg.to_ascii_lowercase().ends_with(".md") {
                            sanitize_filename(seg.trim_end_matches(".md")) + ".md"
                        } else {
                            sanitize_filename(seg) + ".md"
                        };
                        std::fs::write(skills_dir.join(&fname), text).map_err(|e| e.to_string())?;
                        Ok(fname)
                    }
                    .await;
                    let _ = tx.send(BgTaskEvent::SkillInstalledFromUrl(res));
                });
                self.ensure_pump_scheduled(window, cx);
            }
            SettingsAction::InspectSkillFile(path) => {
                self.open_file_path_in_viewer(&path, cx);
            }
            SettingsAction::DeleteSkillFile(path) => match std::fs::remove_file(&path) {
                Ok(()) => {
                    self.refresh_memory_and_skills();
                    self.show_notice("Removed skill file.", cx);
                }
                Err(e) => self.show_notice(format!("Could not remove skill: {e}"), cx),
            },
            // ---- World Book & Cards ----
            SettingsAction::SetWorldBookScope(scope) => {
                self.world_book_scope = scope;
                cx.notify();
            }
            SettingsAction::AddWorldBookFragment => {
                let title = self
                    .settings_inputs
                    .wb_title
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let kw_raw = self
                    .settings_inputs
                    .wb_keywords
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                let content = self
                    .settings_inputs
                    .wb_content
                    .read(cx)
                    .value()
                    .trim()
                    .to_string();
                if title.is_empty() || content.is_empty() {
                    self.show_notice(
                        "Enter both a title and content for the World Book entry.",
                        cx,
                    );
                    return;
                }
                let keywords: Vec<String> = kw_raw
                    .split(',')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(str::to_string)
                    .collect();
                let mode = if keywords.is_empty() {
                    FragmentActivationMode::Pinned
                } else {
                    FragmentActivationMode::Keyword
                };
                let frag = ContextFragment {
                    id: format!("wb-{}", now_ms()),
                    title,
                    content,
                    keywords,
                    mode,
                };
                match self.world_book_scope {
                    WorldBookScope::ActiveChat => {
                        self.active_session.fragments.push(frag);
                        self.persist_active_session();
                    }
                    WorldBookScope::Global => {
                        self.snapshot.global_world_book.push(frag);
                        self.persist_settings();
                    }
                }
                self.settings_inputs
                    .wb_title
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.settings_inputs
                    .wb_keywords
                    .update(cx, |i, cx| i.set_value("", window, cx));
                self.settings_inputs
                    .wb_content
                    .update(cx, |i, cx| i.set_value("", window, cx));
                cx.notify();
            }
            SettingsAction::CycleFragmentMode(fid) => {
                let target = match self.world_book_scope {
                    WorldBookScope::ActiveChat => &mut self.active_session.fragments,
                    WorldBookScope::Global => &mut self.snapshot.global_world_book,
                };
                if let Some(f) = target.iter_mut().find(|f| f.id == fid) {
                    f.mode = match f.mode {
                        FragmentActivationMode::Pinned => FragmentActivationMode::Keyword,
                        FragmentActivationMode::Keyword => FragmentActivationMode::Muted,
                        FragmentActivationMode::Muted => FragmentActivationMode::Pinned,
                    };
                }
                match self.world_book_scope {
                    WorldBookScope::ActiveChat => self.persist_active_session(),
                    WorldBookScope::Global => self.persist_settings(),
                }
                cx.notify();
            }
            SettingsAction::DeleteWorldBookFragment(fid) => {
                match self.world_book_scope {
                    WorldBookScope::ActiveChat => {
                        self.active_session.fragments.retain(|f| f.id != fid);
                        self.persist_active_session();
                    }
                    WorldBookScope::Global => {
                        self.snapshot.global_world_book.retain(|f| f.id != fid);
                        self.persist_settings();
                    }
                }
                cx.notify();
            }
            SettingsAction::SaveCurrentPromptAsPreset => {
                let prompt = self.active_session.system_prompt.trim().to_string();
                if prompt.is_empty() {
                    self.show_notice("Active chat has no custom instructions to save.", cx);
                    return;
                }
                let now = now_ms();
                self.snapshot
                    .system_prompt_presets
                    .push(SystemPromptPreset {
                        id: format!("preset-{now}"),
                        name: format!("{} preset", self.active_session.title),
                        description: String::new(),
                        system_prompt: prompt,
                        authors_note_default: self.active_session.authors_note.clone(),
                        suggested_temperature: None,
                        suggested_top_p: None,
                        suggested_thinking_effort: self.active_profile().thinking_effort,
                    });
                self.persist_settings();
                self.show_notice("Saved system prompt preset.", cx);
            }
            SettingsAction::ApplyPresetToChat(pid) => {
                if let Some(pre) = self
                    .snapshot
                    .system_prompt_presets
                    .iter()
                    .find(|p| p.id == pid)
                    .cloned()
                {
                    self.active_session.system_prompt = pre.system_prompt;
                    if !pre.authors_note_default.is_empty() {
                        self.active_session.authors_note = pre.authors_note_default;
                    }
                    self.persist_active_session();
                    self.sync_notes_inputs_from_active(window, cx);
                    self.show_notice(format!("Applied preset \"{}\".", pre.name), cx);
                }
            }
            SettingsAction::DeletePreset(pid) => {
                self.snapshot.system_prompt_presets.retain(|p| p.id != pid);
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ImportCharacterCard => {
                let Some(path) =
                    pick_open_file_dialog("Import character card", "Card / Preset", "*.png;*.json")
                else {
                    return;
                };
                match std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| import_card_or_prompt_bytes(&bytes))
                {
                    Ok(CardImportResult::Card { card, source_type }) => {
                        let name = card.name.clone();
                        let first_msg = card.first_message.clone();
                        let embedded_count = card.embedded_fragments.len();
                        for ef in &card.embedded_fragments {
                            if !self
                                .active_session
                                .fragments
                                .iter()
                                .any(|f| f.title == ef.title)
                            {
                                self.active_session.fragments.push(ef.clone());
                            }
                        }
                        self.active_session.active_card = Some(card.clone());
                        if !self.snapshot.saved_cards.iter().any(|c| c.name == name) {
                            self.snapshot.saved_cards.push(card);
                        }
                        if self.active_session.messages.is_empty() && !first_msg.trim().is_empty() {
                            self.insert_card_opening_scene(cx);
                        }
                        self.persist_settings();
                        self.persist_active_session();
                        self.sync_active_card_inputs(window, cx);
                        self.show_notice(
                            format!("Imported \"{name}\" ({source_type}, {embedded_count} World Book entries)."),
                            cx,
                        );
                    }
                    Ok(CardImportResult::SystemPrompt {
                        preset,
                        source_type,
                    }) => {
                        let name = preset.name.clone();
                        self.snapshot.system_prompt_presets.push(preset);
                        self.persist_settings();
                        self.show_notice(
                            format!("Imported preset \"{name}\" ({source_type})."),
                            cx,
                        );
                    }
                    Ok(CardImportResult::Theme(theme)) => {
                        let name = theme.name.clone();
                        self.snapshot.theme = theme.without_network_font_paths();
                        self.apply_palette_refresh(cx);
                        self.show_notice(format!("Imported theme \"{name}\"."), cx);
                    }
                    Err(err) => self.show_notice(format!("Card import failed: {err}"), cx),
                }
            }
            SettingsAction::ExportActiveCard => {
                let Some(card) = self.active_session.active_card.clone() else {
                    return;
                };
                let default_name = format!("{}.json", sanitize_filename(&card.name));
                if let Some(dest) =
                    pick_save_file_dialog("Export character card", &default_name, "json")
                {
                    match serde_json::to_string_pretty(&card)
                        .map_err(|e| e.to_string())
                        .and_then(|j| std::fs::write(&dest, j).map_err(|e| e.to_string()))
                    {
                        Ok(()) => {
                            self.show_notice(format!("Exported card to {}", dest.display()), cx)
                        }
                        Err(e) => self.show_notice(format!("Card export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ExportActiveCardPng => {
                let Some(card) = self.active_session.active_card.clone() else {
                    return;
                };
                let default_name = format!("{}.card.png", sanitize_filename(&card.name));
                if let Some(dest) =
                    pick_save_file_dialog("Export character card PNG", &default_name, "png")
                {
                    let png_bytes = export_card_to_png_bytes(&card);
                    match std::fs::write(&dest, png_bytes) {
                        Ok(()) => {
                            self.show_notice(format!("Exported card PNG to {}", dest.display()), cx)
                        }
                        Err(e) => self.show_notice(format!("PNG export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::CreateBlankActiveCard => {
                self.active_session.active_card = Some(CharacterOrScenarioCard {
                    name: "New Character".into(),
                    description: String::new(),
                    personality: String::new(),
                    scenario: String::new(),
                    first_message: String::new(),
                    alternate_greetings: Vec::new(),
                    example_dialogue: String::new(),
                    system_prompt: String::new(),
                    post_history_instructions: String::new(),
                    creator_notes: String::new(),
                    tags: Vec::new(),
                    embedded_fragments: Vec::new(),
                    source_format: "parchment".into(),
                    source_data: String::new(),
                });
                self.persist_active_session();
                self.sync_active_card_inputs(window, cx);
                self.show_notice("Created character card for this chat.", cx);
            }
            SettingsAction::SaveActiveCardToLibrary => {
                if let Some(card) = self.active_session.active_card.clone() {
                    let name = card.name.trim().to_string();
                    if name.is_empty() {
                        self.show_notice("Enter a character name before saving to library.", cx);
                        return;
                    }
                    if let Some(existing) = self
                        .snapshot
                        .saved_cards
                        .iter_mut()
                        .find(|c| c.name == name)
                    {
                        *existing = card;
                    } else {
                        self.snapshot.saved_cards.push(card);
                    }
                    self.persist_settings();
                    self.show_notice(format!("Saved \"{name}\" to card library."), cx);
                }
            }
            SettingsAction::ApplySavedCard(name) => {
                if let Some(card) = self
                    .snapshot
                    .saved_cards
                    .iter()
                    .find(|c| c.name == name)
                    .cloned()
                {
                    for ef in &card.embedded_fragments {
                        if !self
                            .active_session
                            .fragments
                            .iter()
                            .any(|f| f.title == ef.title)
                        {
                            self.active_session.fragments.push(ef.clone());
                        }
                    }
                    self.active_session.active_card = Some(card);
                    self.persist_active_session();
                    self.sync_active_card_inputs(window, cx);
                    self.show_notice(format!("Applied card \"{name}\" to active chat."), cx);
                }
            }
            SettingsAction::DeleteSavedCard(name) => {
                self.snapshot.saved_cards.retain(|c| c.name != name);
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ClearActiveCard => {
                self.active_session.active_card = None;
                self.persist_active_session();
                self.sync_active_card_inputs(window, cx);
                self.show_notice("Removed card from current chat.", cx);
            }
            SettingsAction::BeginRecordShortcut(act) => {
                self.recording_shortcut = Some(act);
                self.shortcut_conflict = None;
                self.shortcut_feedback.clear();
                window.focus(&self.focus_handle, cx);
                cx.notify();
            }
            SettingsAction::CancelRecordShortcut => {
                self.recording_shortcut = None;
                self.shortcut_conflict = None;
                self.shortcut_feedback.clear();
                cx.notify();
            }
            SettingsAction::AcceptShortcutSwap => {
                if let Some(conf) = self.shortcut_conflict.take() {
                    swap_custom_shortcuts(
                        &mut self.snapshot.preferences.custom_shortcuts,
                        conf.target,
                        conf.binding,
                        conf.conflict_with,
                    );
                    self.recording_shortcut = None;
                    self.shortcut_feedback = format!(
                        "Swapped shortcuts for {} and {}.",
                        conf.target.title(),
                        conf.conflict_with.title()
                    );
                    self.persist_settings();
                    cx.notify();
                }
            }
            SettingsAction::CancelShortcutSwap => {
                self.shortcut_conflict = None;
                self.recording_shortcut = None;
                cx.notify();
            }
            SettingsAction::ResetSingleShortcut(act) => {
                self.snapshot.preferences.custom_shortcuts.remove(act.id());
                self.recording_shortcut = None;
                self.shortcut_conflict = None;
                self.shortcut_feedback = format!("Reset {} to default.", act.title());
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::ResetAllShortcuts => {
                self.snapshot.preferences.custom_shortcuts.clear();
                self.recording_shortcut = None;
                self.shortcut_conflict = None;
                self.shortcut_feedback = "Restored all shortcuts to defaults.".into();
                self.persist_settings();
                cx.notify();
            }
            // ---- Storage & Updates ----
            SettingsAction::RefreshStorageScan => {
                self.refresh_storage_scan();
                cx.notify();
            }
            SettingsAction::SetStorageCategoryFilter(cat) => {
                self.storage_category_filter = cat;
                cx.notify();
            }
            SettingsAction::CleanOrphanStorageFiles => {
                let mut removed = 0usize;
                let mut freed = 0u64;
                for entry in &self.storage_files {
                    if entry.is_orphan_or_temp && std::fs::remove_file(&entry.full_path).is_ok() {
                        removed += 1;
                        freed += entry.size_bytes;
                    }
                }
                self.refresh_storage_scan();
                self.show_notice(
                    format!(
                        "Removed {removed} orphan/temp file(s), freeing {}.",
                        crate::ui_kit::format_bytes_compact(freed)
                    ),
                    cx,
                );
            }
            SettingsAction::PreviewStoredFile(path) => {
                self.open_file_path_in_viewer(&path, cx);
            }
            SettingsAction::OpenStoredFile(path) => {
                if let Err(e) = open_local_file_or_folder(&path) {
                    self.show_notice(e, cx);
                }
            }
            SettingsAction::DeleteStoredFile(path) => {
                if path.to_ascii_lowercase().ends_with(".db")
                    || path.to_ascii_lowercase().ends_with(".db-wal")
                    || path.to_ascii_lowercase().ends_with(".db-shm")
                {
                    self.show_notice("Active database files cannot be deleted individually.", cx);
                    return;
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        self.refresh_storage_scan();
                        self.show_notice("Deleted file from storage.", cx);
                    }
                    Err(e) => self.show_notice(format!("Delete failed: {e}"), cx),
                }
            }
            SettingsAction::OpenDataFolder => {
                if let Err(e) = open_local_file_or_folder(&self.data_dir.to_string_lossy()) {
                    self.show_notice(e, cx);
                }
            }
            SettingsAction::ExportCurrentChatArchive => {
                let default_name = format!(
                    "{}.parchment",
                    sanitize_filename(&self.active_session.title)
                );
                if let Some(dest) =
                    pick_save_file_dialog("Export current chat archive", &default_name, "parchment")
                {
                    let ids = vec![self.active_session.id.clone()];
                    match self
                        .core
                        .export_sessions_archive(&dest, &ids, &self.data_dir)
                    {
                        Ok(()) => self.show_notice(
                            format!("Exported chat archive to {}", dest.display()),
                            cx,
                        ),
                        Err(e) => self.show_notice(format!("Export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ExportCurrentChatJson => {
                let default_name =
                    format!("{}.json", sanitize_filename(&self.active_session.title));
                if let Some(dest) =
                    pick_save_file_dialog("Export current chat JSON", &default_name, "json")
                {
                    match serde_json::to_string_pretty(&self.active_session)
                        .map_err(|e| e.to_string())
                        .and_then(|j| std::fs::write(&dest, j).map_err(|e| e.to_string()))
                    {
                        Ok(()) => self
                            .show_notice(format!("Exported chat JSON to {}", dest.display()), cx),
                        Err(e) => self.show_notice(format!("JSON export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ExportArchive => {
                if let Some(dest) = pick_save_file_dialog(
                    "Export YL Axiom archive",
                    "parchment-backup.parchment",
                    "parchment",
                ) {
                    let ids: Vec<String> = self.stubs.iter().map(|s| s.id.clone()).collect();
                    match self
                        .core
                        .export_sessions_archive(&dest, &ids, &self.data_dir)
                    {
                        Ok(()) => {
                            self.show_notice(
                                format!("Exported {} chats to {}", ids.len(), dest.display()),
                                cx,
                            );
                        }
                        Err(e) => self.show_notice(format!("Export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ImportArchive => {
                let Some(src) = pick_open_file_dialog(
                    "Import chats or archive",
                    "YL Axiom Archive / JSON",
                    "*.parchment;*.zip;*.json",
                ) else {
                    return;
                };
                let fname = src
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("import.parchment");
                match std::fs::read(&src)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        self.core
                            .import_archive_bytes(fname, &bytes, &self.data_dir)
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(imported) => {
                        let n = imported.len();
                        self.refresh_stubs();
                        self.refresh_storage_scan();
                        if let Some(first) = imported.first() {
                            self.select_chat_by_id(&first.id, window, cx);
                        }
                        self.show_notice(
                            format!("Imported {n} chat(s) with safe permissions."),
                            cx,
                        );
                    }
                    Err(e) => self.show_notice(format!("Import failed: {e}"), cx),
                }
            }
            SettingsAction::ExportSettingsJson => {
                if let Some(dest) =
                    pick_save_file_dialog("Export settings JSON", "parchment-settings.json", "json")
                {
                    let mut clean = self.snapshot.clone();
                    clean.sessions.clear();
                    match serde_json::to_string_pretty(&clean)
                        .map_err(|e| e.to_string())
                        .and_then(|j| std::fs::write(&dest, j).map_err(|e| e.to_string()))
                    {
                        Ok(()) => {
                            self.show_notice(format!("Exported settings to {}", dest.display()), cx)
                        }
                        Err(e) => self.show_notice(format!("Settings export failed: {e}"), cx),
                    }
                }
            }
            SettingsAction::ImportSettingsJson => {
                let Some(src) =
                    pick_open_file_dialog("Import settings JSON", "JSON files", "*.json")
                else {
                    return;
                };
                match std::fs::read_to_string(&src)
                    .map_err(|e| e.to_string())
                    .and_then(|txt| {
                        serde_json::from_str::<AppStateSnapshot>(&txt).map_err(|e| e.to_string())
                    }) {
                    Ok(raw_snap) => {
                        let current_repo = self.snapshot.ota_github_repo.clone();
                        let safe = raw_snap.with_imported_permissions(&current_repo);
                        self.snapshot.theme = safe.theme;
                        self.snapshot.preferences = safe.preferences;
                        self.snapshot.default_models = safe.default_models;
                        if !safe.profiles.is_empty() {
                            self.snapshot.profiles = safe.profiles;
                            self.snapshot.active_profile_id = safe.active_profile_id;
                        }
                        if !safe.assistants.is_empty() {
                            self.snapshot.assistants = safe.assistants;
                            self.snapshot.active_assistant_id = safe.active_assistant_id;
                        }
                        self.snapshot.global_world_book = safe.global_world_book;
                        self.snapshot.system_prompt_presets = safe.system_prompt_presets;
                        self.snapshot.saved_cards = safe.saved_cards;
                        self.sync_all_settings_inputs(window, cx);
                        self.sync_hex_inputs_from_theme(window, cx);
                        self.apply_palette_refresh(cx);
                        cx.set_reduce_motion(self.snapshot.preferences.reduce_motion);
                        self.show_notice(
                            "Imported settings safely (local commands & unsafe paths stripped).",
                            cx,
                        );
                    }
                    Err(e) => self.show_notice(format!("Settings import failed: {e}"), cx),
                }
            }
            SettingsAction::ToggleAutoCheckOta(val) => {
                self.snapshot.auto_check_ota_on_startup = val;
                self.persist_settings();
                cx.notify();
            }
            SettingsAction::CheckForUpdates => self.spawn_check_for_updates(window, cx),
            SettingsAction::OpenReleasePage(url) => {
                if let Err(err) = open_web_link(&url) {
                    self.show_notice(err, cx);
                }
            }
            SettingsAction::VerifyDownloadedInstaller => {
                let Some(rel) = self.latest_release.clone() else {
                    return;
                };
                let Some(sha) = rel.expected_sha256_hex.as_deref() else {
                    self.show_notice("Release metadata does not expose a SHA-256 digest.", cx);
                    return;
                };
                let Some(path) = pick_open_file_dialog(
                    "Select downloaded installer",
                    "Windows Installer",
                    "*.exe",
                ) else {
                    return;
                };
                let active_count = self.active_runs.len()
                    + self.queued_prompts.values().map(Vec::len).sum::<usize>();
                match OtaStagedFile::verify_staged(&path, rel.expected_size_bytes, sha)
                    .and_then(|s| s.ensure_safe_to_install(active_count).map(|()| s))
                {
                    Ok(staged) => {
                        let sha_short = &staged.sha256_hex[..12.min(staged.sha256_hex.len())];
                        self.show_notice(
                            format!(
                                "Verified SHA-256 {sha_short}… ({} bytes): safe to run {}.",
                                staged.size_bytes,
                                staged.path.display()
                            ),
                            cx,
                        );
                    }
                    Err(e) => self.show_notice(e, cx),
                }
            }
            SettingsAction::RequestWipeAllData => {
                self.confirm_wipe_pending = true;
                cx.notify();
            }
            SettingsAction::CancelWipeAllData => {
                self.confirm_wipe_pending = false;
                cx.notify();
            }
            SettingsAction::ConfirmWipeAllData => {
                let active_ids: Vec<String> = self.active_runs.keys().cloned().collect();
                for cid in active_ids {
                    self.stop_run_for_chat(&cid, cx);
                }
                self.queued_prompts.clear();
                self.confirm_wipe_pending = false;
                let _ = self.core.wipe_all_data();
                self.snapshot = AppStateSnapshot::default();
                apply_design_defaults_once(&mut self.snapshot);
                self.snapshot.profiles = vec![
                    make_default_profile(ProviderProtocol::OpenAiResponses),
                    make_default_profile(ProviderProtocol::AnthropicMessages),
                    make_default_profile(ProviderProtocol::GeminiInteractions),
                ];
                self.snapshot.active_profile_id = self.snapshot.profiles[0].id.clone();
                self.snapshot.assistants.push(default_assistant_agent());
                self.snapshot.active_assistant_id = self.snapshot.assistants[0].id.clone();
                self.open_tabs.clear();
                self.search_hits.clear();
                self.pinned_stubs.clear();
                self.apply_palette_refresh(cx);
                cx.set_reduce_motion(self.snapshot.preferences.reduce_motion);
                self.create_new_chat(window, cx);
                self.sync_all_settings_inputs(window, cx);
                self.refresh_storage_scan();
                self.show_notice("Erased all local chats and settings.", cx);
            }
            _ => {}
        }
    }
}
