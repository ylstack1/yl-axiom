package io.parchment.ai.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.AgentPermissionOverrides
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.WorkspaceMode
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// ============================================================================================
// Settings page 5: Workspace and agent tools (the folder, permissions, skills and MCP servers).
// ============================================================================================
@Composable
internal fun WorkspaceSubpage(
    appState: AppStateSnapshot,
    activeSession: ChatSession,
    activeProfile: ProviderProfile,
    onUpdateSession: ((ChatSession) -> ChatSession) -> Unit,
    onUpdateProfile: ((ProviderProfile) -> ProviderProfile) -> Unit,
    onUpdatePreferences: (io.parchment.ai.model.PreferencesConfig) -> Unit,
    onNotify: (String) -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    val scope = rememberCoroutineScope()

    val activeAssistant = appState.assistants.find { it.id == activeSession.assistantId }
        ?: appState.assistants.find { it.id == appState.activeAssistantId }
        ?: appState.assistants.firstOrNull()
    val permissionDefaults = AgentPermissionResolver.defaults(activeAssistant)
    val permissionOverrides = activeSession.permissionOverrides ?: AgentPermissionResolver.legacyOverrides(activeSession, activeProfile)
    val toolPolicy = permissionOverrides.applyTo(permissionDefaults)
    val effectiveWsPath = toolPolicy.workspacePath
    val effectiveWsMode = toolPolicy.workspaceMode

    var memoryRefreshTick by remember { mutableStateOf(0) }
    val hasGlobalMemory by produceState(false, memoryRefreshTick) {
        value = withContext(Dispatchers.IO) { PlatformStorage.hasGlobalAgentMemory() }
    }
    val discoveredFiles by produceState(emptyList<String>(), effectiveWsPath) {
        value = withContext(Dispatchers.IO) { WorkspaceHarness.listDiscoveredContextFileNames(effectiveWsPath) }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(if (win.isCompact) Space.s12 else Space.s16),
        verticalArrangement = Arrangement.spacedBy(Space.s16)
    ) {
        BasicText(text = Strings.workspaceAndAgentTools, style = p.uiHeaderStyle.copy(fontSize = 19.sp))

        SepiaSectionCard(
            title = Strings.workspaceAndAgentPermissions
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.s12)) {
                AgentPermissionEditor(
                    defaults = permissionDefaults, overrides = permissionOverrides,
                    onChange = { value -> onUpdateSession { it.copy(permissionOverrides = value) } },
                    onReset = { onUpdateSession { it.copy(permissionOverrides = AgentPermissionOverrides()) } }
                )
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(Space.s10)
                ) {
                    Column {
                        BasicText(
                            text = Strings.globalAgentMemoryTitle,
                            style = p.uiBodyStyle.copy(fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                        )
                        BasicText(
                            text = if (hasGlobalMemory) Strings.globalMemoryAutoInjected else Strings.globalMemoryCreatedOnDemand,
                            style = p.uiLabelStyle.copy(color = if (hasGlobalMemory) p.accent else p.mutedInk)
                        )
                    }
                    if (!hasGlobalMemory) {
                        SepiaButton(
                            text = Strings.createProjectMemoryFile,
                            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp), wrapText = true,
                            enabled = effectiveWsMode != WorkspaceMode.READ_ONLY,
                            style = SepiaButtonStyle.OUTLINE,
                            onClick = {
                                scope.launch(Dispatchers.IO) {
                                    PlatformStorage.writeGlobalAgentMemory(
                                        content = "# Global Agent Memory\n\n- Record durable preferences, workflow conventions, and cross-project notes here.\n"
                                    ).onSuccess {
                                        WorkspaceHarness.invalidateWorkspaceCache()
                                        withContext(Dispatchers.Main) {
                                            memoryRefreshTick++
                                            onNotify(Strings.createdParchmentMemoryMdInWorkspace)
                                        }
                                    }
                                }
                            }
                        )
                    }
                }
                if (effectiveWsPath.isNotBlank()) {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(Space.s10)
                    ) {
                        Column {
                            BasicText(
                                text = Strings.autoLoadedProjectMemoryContext,
                                style = p.uiBodyStyle.copy(fontWeight = FontWeight.SemiBold, fontSize = 13.sp)
                            )
                            BasicText(
                                text = if (discoveredFiles.isEmpty()) {
                                    Strings.noAgentsMdClaudeMdParchment
                                } else {
                                    Strings.autoInjectedIntoSystemPrompt(discoveredFiles.joinToString(", "))
                                },
                                style = p.uiLabelStyle.copy(color = if (discoveredFiles.isNotEmpty()) p.accent else p.mutedInk)
                            )
                        }
                    }
                }
            }
        }

        McpServersCard(appState.preferences, onUpdatePreferences, onNotify)
        SkillInstallCard(effectiveWsPath, effectiveWsMode == io.parchment.ai.model.WorkspaceMode.READ_WRITE, onNotify)

        SepiaSectionCard(title = Strings.agentTools) {
            AgentToolRegistry(
                policy = toolPolicy, overrides = permissionOverrides,
                onChange = { value -> onUpdateSession { it.copy(permissionOverrides = value) } }
            )
        }
    }
}
