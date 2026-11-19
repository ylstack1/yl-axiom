package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.McpServerConfig
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformWorkspace
import io.parchment.ai.network.McpRegistry
import io.parchment.ai.network.McpServerState
import io.parchment.ai.network.RegistryServer
import io.parchment.ai.network.formatPickedMcpCommand
import io.parchment.ai.network.installSkillFromGitHub
import io.parchment.ai.network.installSkillFromLocalBundle
import io.parchment.ai.network.installSkillFromSingleFile
import io.parchment.ai.network.isZipPayload
import io.parchment.ai.network.mergeImportedMcpServers
import io.parchment.ai.network.parseGitHubFolder
import io.parchment.ai.network.parseMcpConfigJson
import io.parchment.ai.network.searchMcpRegistry
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Keeps connections matching the saved servers; the pause stops a connection per keystroke. */
@Composable
internal fun McpServerConnections(servers: List<McpServerConfig>) {
    val enabled = remember(servers) { servers.filter { it.enabled } }
    LaunchedEffect(enabled) {
        delay(600)
        McpRegistry.sync(enabled)
    }
}

/** Added servers start switched off, because the registry checks who owns a name, not what a server does. */
@Composable
private fun McpServerBrowser(prefs: PreferencesConfig, onUpdatePreferences: (PreferencesConfig) -> Unit) {
    val p = LocalParchmentPalette.current
    val scope = rememberCoroutineScope()
    var query by remember { mutableStateOf("") }
    var results by remember { mutableStateOf<List<RegistryServer>>(emptyList()) }
    var nextCursor by remember { mutableStateOf("") }
    var loadingMore by remember { mutableStateOf(false) }
    var tokenFreeOnly by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf(Strings.searchThePublicMcpDirectory) }
    LaunchedEffect(query) {
        val term = query.trim()
        nextCursor = ""
        if (term.length < 2) {
            results = emptyList()
            message = Strings.searchThePublicMcpDirectory
            return@LaunchedEffect
        }
        delay(400)
        message = Strings.searching
        searchMcpRegistry(term).fold(
            { page ->
                results = page.servers
                nextCursor = page.nextCursor
                message = if (page.servers.isEmpty()) Strings.noServersYouCanAddDirectly else ""
            },
            { error ->
                if (error is CancellationException) throw error
                results = emptyList()
                message = Strings.theDirectoryCouldNotBeReached
            }
        )
    }
    val shown = if (tokenFreeOnly) results.filter { !it.needsToken } else results
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.s8)) {
        BasicText(Strings.findAServer, style = p.uiBodyStyle.copy(fontWeight = FontWeight.SemiBold))
        BasicText(Strings.anyoneCanPublishToThisDirectory, style = p.uiLabelStyle)
        SepiaTextField(query, { query = it }, placeholder = Strings.searchForExampleGithub, modifier = Modifier.fillMaxWidth())
        if (results.isNotEmpty()) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.s8)) {
                SepiaChip(Strings.noTokenNeeded, selected = tokenFreeOnly, onClick = { tokenFreeOnly = !tokenFreeOnly })
                BasicText(plural(shown.size, "server"), style = p.uiLabelStyle)
            }
        }
        if (message.isNotEmpty()) BasicText(message, style = p.uiLabelStyle)
        if (shown.isNotEmpty() || nextCursor.isNotEmpty()) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(max = 380.dp)
                    .clip(RoundedCornerShape(Radius.r10))
                    .background(p.cardBg)
                    .border(LineWidth.hairline, p.border.copy(alpha = 0.75f), RoundedCornerShape(Radius.r10))
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = Space.s12, vertical = Space.s6)
            ) {
                shown.forEachIndexed { index, server ->
                    if (index > 0) SepiaDivider()
                    McpRegistryRow(server, added = prefs.mcpServers.any { it.url == server.url }) {
                        val entry = McpServerConfig("mcp-${PlatformRuntime.currentTimeMillis()}", server.title.ifBlank { server.name.substringAfterLast('/') }, url = server.url)
                        onUpdatePreferences(prefs.copy(mcpServers = prefs.mcpServers + entry))
                    }
                }
                if (nextCursor.isNotEmpty()) {
                    SepiaButton(if (loadingMore) Strings.loading else Strings.showMore, compact = true, style = SepiaButtonStyle.GHOST, enabled = !loadingMore, modifier = Modifier.padding(vertical = Space.s6), onClick = {
                        val term = query.trim()
                        val cursor = nextCursor
                        loadingMore = true
                        scope.launch {
                            searchMcpRegistry(term, cursor).fold(
                                { page ->
                                    if (query.trim() == term) {
                                        results = (results + page.servers).distinctBy { it.name }
                                        nextCursor = page.nextCursor
                                    }
                                },
                                { error ->
                                    if (error is CancellationException) throw error
                                    message = Strings.moreServersCouldNotBeLoaded
                                }
                            )
                            loadingMore = false
                        }
                    })
                }
            }
        }
    }
}

/** One line for the name, one muted line for the publisher and token need, two lines of description. */
@Composable
private fun McpRegistryRow(server: RegistryServer, added: Boolean, onAdd: () -> Unit) {
    val p = LocalParchmentPalette.current
    val title = server.title.ifBlank { server.name.substringAfterLast('/') }
    val details = listOfNotNull(server.name.takeIf { it != title }, "Needs a token".takeIf { server.needsToken }).joinToString(" · ")
    Row(Modifier.fillMaxWidth().padding(vertical = Space.s8), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.s12)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Space.s2)) {
            BasicText(title, style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium), maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (details.isNotEmpty()) BasicText(details, style = p.uiLabelStyle, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (server.description.isNotBlank()) BasicText(server.description, style = p.uiLabelStyle.copy(color = p.primaryInk), maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        SepiaButton(if (added) Strings.added else Strings.add, compact = true, enabled = !added, onClick = onAdd)
    }
}

@Composable
internal fun SkillInstallCard(workspacePath: String, canWrite: Boolean, onNotify: (String) -> Unit) {
    val p = LocalParchmentPalette.current
    val scope = rememberCoroutineScope()
    var link by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    val writable = canWrite && workspacePath.isNotBlank()

    suspend fun existsInWorkspace(path: String): Boolean =
        withContext(Dispatchers.IO) { PlatformWorkspace.workspaceRead(workspacePath, path, 1, 1).isSuccess }

    suspend fun writeToWorkspace(path: String, text: String): Result<String> =
        withContext(Dispatchers.IO) { PlatformWorkspace.workspaceWrite(workspacePath, path, text) }

    SepiaSectionCard(title = Strings.installASkill, subtitle = Strings.skillsTeachTheAgentHow) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.s8)) {
            SepiaTextField(link, { link = it }, placeholder = Strings.httpsGithubComOwnerRepo, modifier = Modifier.fillMaxWidth())
            BasicText(
                if (writable) Strings.itIsCopiedIntoThisWorkspace
                else Strings.chooseAFolderWithWriteAccess,
                style = p.uiLabelStyle
            )
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.s8),
                verticalArrangement = Arrangement.spacedBy(Space.s8)
            ) {
                SepiaButton(if (busy) Strings.installing else Strings.install, compact = true, enabled = !busy && writable && link.isNotBlank(), onClick = {
                    val folder = parseGitHubFolder(link)
                    if (folder == null) {
                        onNotify(Strings.thatIsNotAGithubRepository)
                        return@SepiaButton
                    }
                    busy = true
                    scope.launch {
                        val result = installSkillFromGitHub(
                            folder,
                            exists = ::existsInWorkspace,
                            write = ::writeToWorkspace,
                            fetch = { url, limit -> withContext(Dispatchers.IO) { PlatformWorkspace.fetchPublicBytes(url, limit).getOrThrow() } }
                        )
                        busy = false
                        result.fold(
                            {
                                WorkspaceHarness.invalidateWorkspaceCache(workspacePath)
                                onNotify(it)
                                link = ""
                            },
                            { onNotify(it.message ?: Strings.theSkillCouldNotBeInstalled) }
                        )
                    }
                })
                SepiaButton(
                    text = Strings.importFolder,
                    compact = true,
                    style = SepiaButtonStyle.OUTLINE,
                    enabled = !busy && writable,
                    onClick = {
                        busy = true
                        scope.launch {
                            val selectedDir = PlatformPickers.pickDirectory(Strings.selectLocalSkillFolder, workspacePath)
                            if (selectedDir.isNullOrBlank()) {
                                busy = false
                                return@launch
                            }
                            val result = withContext(Dispatchers.IO) {
                                PlatformWorkspace.readLocalSkillFolder(selectedDir)
                            }.fold(
                                { bundle -> installSkillFromLocalBundle(bundle, ::existsInWorkspace, ::writeToWorkspace) },
                                { Result.failure(it) }
                            )
                            busy = false
                            result.fold(
                                {
                                    WorkspaceHarness.invalidateWorkspaceCache(workspacePath)
                                    onNotify(it)
                                },
                                { onNotify(it.message ?: Strings.theSkillCouldNotBeInstalled) }
                            )
                        }
                    }
                )
                SepiaButton(
                    text = Strings.importFileOrZip,
                    compact = true,
                    style = SepiaButtonStyle.OUTLINE,
                    enabled = !busy && writable,
                    onClick = {
                        busy = true
                        scope.launch {
                            val picked = PlatformPickers.pickFileForImport(listOf("zip", "md"))
                            if (picked == null) {
                                busy = false
                                return@launch
                            }
                            if (picked.oversized) {
                                busy = false
                                onNotify(Strings.selectedSkillFileTooLarge)
                                return@launch
                            }
                            val result = if (isZipPayload(picked.fileName, picked.bytes)) {
                                withContext(Dispatchers.IO) {
                                    PlatformWorkspace.readLocalSkillZip(picked.fileName, picked.bytes)
                                }.fold(
                                    { bundle -> installSkillFromLocalBundle(bundle, ::existsInWorkspace, ::writeToWorkspace) },
                                    { Result.failure(it) }
                                )
                            } else {
                                installSkillFromSingleFile(picked.fileName, picked.bytes, ::existsInWorkspace, ::writeToWorkspace)
                            }
                            busy = false
                            result.fold(
                                {
                                    WorkspaceHarness.invalidateWorkspaceCache(workspacePath)
                                    onNotify(it)
                                },
                                { onNotify(it.message ?: Strings.theSkillCouldNotBeInstalled) }
                            )
                        }
                    }
                )
            }
        }
    }
}

@Composable
internal fun McpServersCard(
    prefs: PreferencesConfig,
    onUpdatePreferences: (PreferencesConfig) -> Unit,
    onNotify: (String) -> Unit = {}
) {
    val p = LocalParchmentPalette.current
    val scope = rememberCoroutineScope()
    val statuses by McpRegistry.statuses.collectAsState()
    val localAllowed = remember { PlatformWorkspace.supportsLocalMcpServers }
    fun update(server: McpServerConfig, change: (McpServerConfig) -> McpServerConfig) {
        onUpdatePreferences(prefs.copy(mcpServers = prefs.mcpServers.map { if (it.id == server.id) change(it) else it }))
    }
    SepiaSectionCard(title = Strings.mcpServers, subtitle = Strings.giveTheAgentToolsFrom) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.s12)) {
            prefs.mcpServers.forEach { server ->
                Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Space.s6)) {
                    SepiaToggleSwitch(checked = server.enabled, onCheckedChange = { on -> update(server) { it.copy(enabled = on) } }, label = server.name.ifBlank { Strings.newServer })
                    SepiaTextField(server.name, { text -> update(server) { it.copy(name = text) } }, placeholder = Strings.name, modifier = Modifier.fillMaxWidth())
                    SepiaTextField(server.url, { text -> update(server) { it.copy(url = text) } }, placeholder = Strings.serverUrl, modifier = Modifier.fillMaxWidth())
                    if (server.url.isNotBlank()) {
                        SepiaTextField(server.bearerToken, { text -> update(server) { it.copy(bearerToken = text) } }, placeholder = Strings.accessTokenOptional, modifier = Modifier.fillMaxWidth())
                    } else if (localAllowed) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(Space.s6),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            SepiaTextField(
                                value = server.command,
                                onValueChange = { text -> update(server) { it.copy(command = text) } },
                                placeholder = Strings.orALocalCommand,
                                monospace = true,
                                modifier = Modifier.weight(1f)
                            )
                            SepiaButton(
                                text = Strings.browseLocalCommand,
                                contentDescription = Strings.browseLocalCommandDesc,
                                compact = true,
                                style = SepiaButtonStyle.OUTLINE,
                                onClick = {
                                    scope.launch {
                                        val pickedPath = PlatformPickers.pickLocalCommandFile() ?: return@launch
                                        val formatted = formatPickedMcpCommand(pickedPath)
                                        if (formatted.isNotBlank()) {
                                            val inferredName = server.name.ifBlank {
                                                pickedPath.substringAfterLast('\\').substringAfterLast('/').substringBeforeLast('.')
                                            }
                                            update(server) { it.copy(name = inferredName, command = formatted) }
                                        }
                                    }
                                }
                            )
                        }
                        if (server.command.isNotBlank()) BasicText(Strings.aLocalCommandRunsWithYour, style = p.uiLabelStyle)
                    }
                    statuses[server.id]?.let { status ->
                        BasicText(when (status.state) {
                            McpServerState.CONNECTING -> status.message.ifBlank { Strings.connecting }
                            McpServerState.READY -> Strings.connected(plural(status.toolCount, "tool"))
                            McpServerState.FAILED -> status.message
                        }, style = p.uiLabelStyle)
                        if (status.state == McpServerState.FAILED && server.enabled) {
                            SepiaButton(Strings.restartServer, compact = true, style = SepiaButtonStyle.OUTLINE, hint = Strings.restartServerHint,
                                onClick = { McpRegistry.restartNow(server.id) })
                        }
                    }
                    SepiaButton(Strings.remove, compact = true, style = SepiaButtonStyle.GHOST, onClick = {
                        onUpdatePreferences(prefs.copy(mcpServers = prefs.mcpServers.filter { it.id != server.id }))
                    })
                    SepiaDivider()
                }
            }
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.s8),
                verticalArrangement = Arrangement.spacedBy(Space.s8)
            ) {
                SepiaButton(Strings.addServer, compact = true, onClick = {
                    onUpdatePreferences(prefs.copy(mcpServers = prefs.mcpServers + McpServerConfig("mcp-${PlatformRuntime.currentTimeMillis()}")))
                })
                SepiaButton(Strings.importMcpJson, compact = true, style = SepiaButtonStyle.OUTLINE, onClick = {
                    scope.launch {
                        val picked = PlatformPickers.pickFileForImport(listOf("json")) ?: return@launch
                        if (picked.oversized) {
                            onNotify(Strings.selectedMcpFileTooLarge)
                            return@launch
                        }
                        val rawText = runCatching { picked.bytes.decodeToString(throwOnInvalidSequence = true) }.getOrNull()
                        if (rawText == null || '\u0000' in rawText) {
                            onNotify(Strings.mcpJsonNotUtf8)
                            return@launch
                        }
                        parseMcpConfigJson(rawText, supportsLocalCommands = localAllowed).fold(
                            { outcome ->
                                val (merged, addedCount) = mergeImportedMcpServers(
                                    existing = prefs.mcpServers,
                                    imported = outcome.servers,
                                    idPrefix = "mcp-${PlatformRuntime.currentTimeMillis()}"
                                )
                                if (addedCount > 0) {
                                    onUpdatePreferences(prefs.copy(mcpServers = merged))
                                }
                                val msg = when {
                                    addedCount > 0 && outcome.skippedLocalCommandCount > 0 ->
                                        Strings.importedMcpServersDesktopOnlySkipped(
                                            plural(addedCount, "MCP server"),
                                            plural(outcome.skippedLocalCommandCount, "local command")
                                        )
                                    addedCount > 0 ->
                                        Strings.importedMcpServers(plural(addedCount, "MCP server"))
                                    outcome.skippedLocalCommandCount > 0 ->
                                        Strings.skippedOnlyLocalMcpServers(plural(outcome.skippedLocalCommandCount, "local server"))
                                    else -> Strings.allMcpServersAlreadyAdded
                                }
                                onNotify(msg)
                            },
                            { error ->
                                onNotify(error.message ?: Strings.mcpConfigCouldNotBeImported)
                            }
                        )
                    }
                })
            }
            SepiaDivider()
            McpServerBrowser(prefs, onUpdatePreferences)
        }
    }
}
