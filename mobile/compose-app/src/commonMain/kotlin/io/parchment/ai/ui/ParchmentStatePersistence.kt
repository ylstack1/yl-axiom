package io.parchment.ai.ui

import io.parchment.ai.harness.settledForDisk
import io.parchment.ai.model.AgentPermissionOverrides
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.CharacterOrScenarioCard
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ContextFragment
import io.parchment.ai.model.DefaultModelConfig
import io.parchment.ai.model.DesktopWindowConfig
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.SystemPromptPreset
import io.parchment.ai.model.ThemeConfig
import io.parchment.ai.model.mapProtocolToDialect
import io.parchment.ai.model.withImportedPermissions
import io.parchment.ai.platform.ChatBundleEntry
import io.parchment.ai.platform.ChatFileStamp
import io.parchment.ai.platform.ChatImportOutcome
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.protocol.isGeminiServerSideToolSignature
import io.parchment.ai.protocol.sanitizeGeminiThoughtSignature
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * Process-wide supervisor scope so switching apps on Android or recomposing UI
 * never cancels an active generation stream.
 */
internal val appProcessScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

internal val appDiskWriter by lazy {
    ParchmentDiskWriter(
        appProcessScope,
        saveSettings = ::saveSettingsOnlyToDisk,
        saveSession = ::saveSingleChatSessionToDisk,
        deleteSession = PlatformStorage::deleteChatSessionFile,
        saveLiveTurn = ::saveLiveTurnToDisk
    )
}

@OptIn(ExperimentalSerializationApi::class)
internal val compactStateJson = Json {
    ignoreUnknownKeys = true
    isLenient = true
    encodeDefaults = false
    explicitNulls = false
    prettyPrint = false
}

enum class SidebarTab {
    ASSISTANTS,
    TOPICS,
    PINNED
}

sealed interface DeleteConfirmRequest {
    data class SingleSession(val sessionId: String, val title: String) : DeleteConfirmRequest
    data class ClearAllSessions(val assistantName: String, val count: Int) : DeleteConfirmRequest
    data class ClearCurrentMessages(val title: String) : DeleteConfirmRequest
}

data class RegenerateConfirmRequest(
    val sessionId: String,
    val nodeIndex: Int
)

sealed interface ActiveMediaViewerItem {
    data class ImageViewer(
        val title: String,
        val mimeType: String,
        val localFilePath: String,
        val remoteUrl: String = ""
    ) : ActiveMediaViewerItem

    data class TextOrCodeViewer(
        val title: String,
        val mimeType: String,
        val textContent: String,
        val localFilePath: String = ""
    ) : ActiveMediaViewerItem
}

data class SessionStreamRuntime(
    val sessionId: String,
    val targetNodeId: String = "",
    val isThinkingPhase: Boolean = false,
    val statusText: String = "",
    val interactionId: String = "",
    val job: Job? = null,
    val toolApprovalBroker: io.parchment.ai.harness.ToolApprovalBroker? = null,
    internal val identity: AgentRunIdentity? = null,
    internal val phase: AgentRunPhase = AgentRunPhase.RUNNING,
    internal val providerContext: RunProviderContext? = null,
    val actualWireDialect: String = "",
    internal val isPreparingHandoff: Boolean = false,
    /** Set while the run waits to send its request again; [retryControl] ends the wait early. */
    val retryWait: io.parchment.ai.network.RetryWait? = null,
    val retryControl: io.parchment.ai.network.RetryControl? = null
)

data class SlashCommandSpec(
    val command: String,
    val label: String,
    val description: String,
    val takesArgument: Boolean
)

internal val BUILTIN_SLASH_COMMANDS = listOf(
    SlashCommandSpec("/goal", "🎯 /goal", Strings.autonomousMultiStepGoalVerificationChecklist, takesArgument = true),
    SlashCommandSpec("/btw", "💬 /btw", Strings.sideQuestionLeftOutOfThe, takesArgument = true),
    SlashCommandSpec("/compact", "⚡ /compact", Strings.summarizeOlderTurnsIntoMemoryMute, takesArgument = false),
    SlashCommandSpec("/summarize", "✦ /summarize", Strings.updateRollingStoryContinuitySummary, takesArgument = false),
    SlashCommandSpec("/continue", "→ /continue", Strings.continueTheLastAssistantReplySeamlessly, takesArgument = false),
    SlashCommandSpec("/rewind", "↺ /rewind", Strings.removeTheLastTurnPairFrom, takesArgument = false),
    SlashCommandSpec("/clear", "🗑 /clear", Strings.clearAllMessagesInTheCurrent, takesArgument = false),
    SlashCommandSpec("/plan", "🔍 /plan", Strings.switchToPlanModeDesc, takesArgument = false),
    SlashCommandSpec("/build", "🔧 /build", Strings.switchToBuildModeDesc, takesArgument = false)
)

/** Maximum number of simultaneously open chat tabs before MRU eviction kicks in. */
internal const val MAX_OPEN_TABS = 12

/**
 * Normalizes the open-tab strip: de-duplicates, guarantees [activeId] is present,
 * and evicts the least-recently-used tabs beyond [MAX_OPEN_TABS] (active always kept).
 */
internal fun normalizeOpenTabs(tabs: List<String>, activeId: String): List<String> {
    val deduped = tabs.filter { it.isNotBlank() }.distinct()
    val withActive = if (activeId.isBlank() || activeId in deduped) deduped else deduped + activeId
    if (withActive.size <= MAX_OPEN_TABS) return withActive
    val keep = withActive.takeLast(MAX_OPEN_TABS).toMutableList()
    if (activeId.isNotBlank() && activeId !in keep) {
        keep.removeAt(0)
        keep.add(activeId)
    }
    return keep
}

/**
 * One-shot, idempotent migration of legacy typeface defaults (16.5 / 12.5) to the
 * v2.0.53 baseline (14 / 14). Only rewrites values that still equal the OLD defaults,
 * so any deliberate user choice survives untouched.
 */
internal fun migrateThemeTypefaceDefaults(theme: ThemeConfig): ThemeConfig {
    val prose = if (theme.proseFontSizeSp == 16.5f) 14.0f else theme.proseFontSizeSp
    val code = if (theme.codeFontSizeSp == 12.5f) 14.0f else theme.codeFontSizeSp
    return if (prose != theme.proseFontSizeSp || code != theme.codeFontSizeSp) {
        theme.copy(proseFontSizeSp = prose, codeFontSizeSp = code)
    } else {
        theme
    }
}

/**
 * Migrates legacy provider JSON shapes before deserialization:
 *  - `BYTEDANCE_ARK` -> `OPENAI_RESPONSES`
 *  - `useExactEndpointUrl = true` -> `protocol = CUSTOM` (verbatim URL, dialect preserved)
 *  - legacy unsurfaced `useStatefulChaining = true` -> stateless/privacy-first defaults
 *  - settings saved before `providerStorageReset` -> built-in profiles reset to stateless once
 */
private val BUILT_IN_STORAGE_RESET_PROFILES = setOf("prof-gemini-interactions", "prof-openai-responses")

internal fun migrateLegacyProviderJson(raw: String): String {
    val preprocessed = raw.replace("\"BYTEDANCE_ARK\"", "\"OPENAI_RESPONSES\"")
    if (!preprocessed.contains("useExactEndpointUrl") && !preprocessed.contains("\"useStatefulChaining\"")) {
        return preprocessed
    }
    val root = runCatching { compactStateJson.parseToJsonElement(preprocessed) as? JsonObject }.getOrNull()
        ?: return preprocessed
    val profiles = root["profiles"] as? JsonArray ?: return preprocessed
    val resetBuiltInStorage = !root.containsKey("providerStorageReset")
    var mutated = false
    val migratedProfiles = profiles.map { elem ->
        val obj = elem as? JsonObject ?: return@map elem
        val legacyProtocol = obj["protocol"]?.jsonPrimitive?.contentOrNull.orEmpty()
        val exactUrl = obj["useExactEndpointUrl"]?.jsonPrimitive?.booleanOrNull == true
        val hadStoreKey = obj.containsKey("storeServerSide")
        val legacyChain = obj["useStatefulChaining"]?.jsonPrimitive?.booleanOrNull == true
        val stored = obj["storeServerSide"]?.jsonPrimitive?.booleanOrNull == true
        // These profiles once shipped with storage on, which users never chose.
        val resetStorage = resetBuiltInStorage &&
            (legacyChain || stored) &&
            obj["id"]?.jsonPrimitive?.contentOrNull in BUILT_IN_STORAGE_RESET_PROFILES
        val withoutLegacy = JsonObject(
            obj.filterKeys { it != "useExactEndpointUrl" }.mapValues { (_, v) -> v } +
                buildMap {
                    if (exactUrl) {
                        // Keep the concrete dialect so payload shape is unchanged; only URL handling differs.
                        put("protocol", kotlinx.serialization.json.JsonPrimitive("CUSTOM"))
                        if (legacyProtocol.isNotBlank() && !obj.containsKey("customDialect")) {
                            put("customDialect", kotlinx.serialization.json.JsonPrimitive(mapProtocolToDialect(legacyProtocol)))
                        }
                    }
                    if ((legacyChain && !hadStoreKey) || resetStorage) {
                        // Previously an unsurfaced default; reset to privacy-first stateless.
                        put("useStatefulChaining", kotlinx.serialization.json.JsonPrimitive(false))
                        put("storeServerSide", kotlinx.serialization.json.JsonPrimitive(false))
                    }
                }
        )
        if (exactUrl || (legacyChain && !hadStoreKey) || resetStorage) mutated = true
        withoutLegacy
    }
    if (!mutated) return preprocessed
    val rebuilt = JsonObject(root + ("profiles" to JsonArray(migratedProfiles)))
    return compactStateJson.encodeToString(JsonObject.serializer(), rebuilt)
}

internal fun formatDurationFromMs(ms: Long): String {
    val totalSeconds = ms.coerceAtLeast(0L) / 1000L
    return when {
        totalSeconds < 60L -> "${totalSeconds}s"
        totalSeconds < 3600L -> {
            val mins = totalSeconds / 60L
            val secs = totalSeconds % 60L
            "${mins}m${secs}s"
        }
        totalSeconds < 86_400L -> {
            val hrs = totalSeconds / 3600L
            val mins = (totalSeconds % 3600L) / 60L
            "${hrs}h${mins}m"
        }
        else -> {
            val days = totalSeconds / 86_400L
            val hrs = (totalSeconds % 86_400L) / 3600L
            val mins = (totalSeconds % 3600L) / 60L
            "${days}d${hrs}h${mins}m"
        }
    }
}

internal fun formatCompactTimestamp(epochMs: Long): String {
    if (epochMs <= 0L) return ""
    val now = PlatformRuntime.currentTimeMillis()
    val diffSec = ((now - epochMs) / 1000L).coerceAtLeast(0L)
    return when {
        diffSec < 60L -> Strings.justNow
        diffSec < 3600L -> "${diffSec / 60L}m ago"
        diffSec < 86_400L -> "${diffSec / 3600L}h ago"
        diffSec < 172_800L -> Strings.yesterday
        diffSec < 604_800L -> "${diffSec / 86_400L}d ago"
        else -> "${diffSec / 604_800L}w ago"
    }
}

internal data class LiveAgentPeekInfo(
    val isToolPeek: Boolean,
    val title: String,
    val body: String,
    val toolName: String = "",
    val isToolRunning: Boolean = false
)

/**
 * Extracts either the active/latest Tool Peek (`isToolPeek = true`) or the latest Thought Peek (`isToolPeek = false`)
 * when the user collapses the full Thinking and tool steps into Compact Peek mode while the agent is working.
 */
internal fun extractLatestAgentPeek(variant: MessageVariant): LiveAgentPeekInfo {
    val tools = variant.toolCalls
    val runningToolIdx = tools.indexOfLast { it.isRunning }
    val showToolPeek = runningToolIdx >= 0 || (tools.isNotEmpty() && variant.trailingThinking.isBlank())
    if (showToolPeek && tools.isNotEmpty()) {
        val targetIdx = if (runningToolIdx >= 0) runningToolIdx else tools.lastIndex
        val tc = tools[targetIdx]
        val intent = io.parchment.ai.harness.WorkspaceHarness.describeToolCallIntent(tc.name, tc.argumentsJson)
        val intentLines = intent.lines()
        val headline = Strings.tool(targetIdx + 1, tools.size, intentLines.firstOrNull().orEmpty())
        val bodyText = buildString {
            if (intentLines.size > 1) {
                appendLine(intentLines.drop(1).joinToString("\n"))
            }
            val cleanRes = tc.resultText.trim()
            if (cleanRes.isNotEmpty() && cleanRes != "Running ${tc.name}…" && cleanRes != "Running shell command…") {
                val tailLines = cleanRes.lines().takeLast(4).joinToString("\n")
                append(tailLines)
            }
        }.trim()
        return LiveAgentPeekInfo(
            isToolPeek = true,
            title = headline,
            body = bodyText,
            toolName = tc.name,
            isToolRunning = tc.isRunning
        )
    }

    val (thoughtTitle, thoughtBody) = extractLatestThoughtPeek(variant)
    return LiveAgentPeekInfo(
        isToolPeek = false,
        title = thoughtTitle,
        body = thoughtBody
    )
}

private val THOUGHT_BLOCK_SPLIT_REGEX = Regex("(?m)(?=^\\*\\*[^*\\n]+\\*\\*\\s*$|^#{1,4}\\s+)|\\n\\s*\\n+")
private val HEADING_HASH_PREFIX_REGEX = Regex("^#{1,4}\\s+")
private val PERSISTENCE_WHITESPACE_REGEX = Regex("\\s+")
private val EXPORT_UNSAFE_CHAR_REGEX = Regex("[^a-zA-Z0-9._-]")
internal const val MAX_TEXT_PREVIEW_BYTES = 4L * 1024L * 1024L
private val MULTIPLE_UNDERSCORES_REGEX = Regex("_+")
private val EXPORT_SHORT_ID_REGEX = Regex("[^a-zA-Z0-9_-]")

private const val MAX_THOUGHT_PEEK_SCAN_CHARS = 8192

/**
 * Extracts the latest single thought block (title + body) for the Compact Single-Thought Peek Window
 * when the user presses `Hide ▲` while the model is still actively reasoning.
 */
internal fun extractLatestThoughtPeek(variant: MessageVariant): Pair<String, String> {
    val rawFull = variant.trailingThinking.ifBlank { variant.thinkingSummary }
    if (rawFull.isBlank()) return "" to ""
    val raw = if (rawFull.length > MAX_THOUGHT_PEEK_SCAN_CHARS) {
        val targetStart = rawFull.length - MAX_THOUGHT_PEEK_SCAN_CHARS
        val prevBreak = rawFull.lastIndexOf("\n\n", startIndex = targetStart)
        val sliceStart = when {
            prevBreak >= targetStart - 2048 && prevBreak >= 0 -> prevBreak + 2
            else -> {
                val nl = rawFull.indexOf('\n', startIndex = targetStart)
                if (nl in targetStart until (rawFull.length - 128)) nl + 1 else targetStart
            }
        }
        rawFull.substring(sliceStart).trim()
    } else {
        rawFull.trim()
    }
    if (raw.isEmpty()) return "" to ""

    val blocks = raw.split(THOUGHT_BLOCK_SPLIT_REGEX)
        .map { it.trim() }
        .filter { it.isNotEmpty() }
    if (blocks.isEmpty()) return "" to raw

    val lastIdx = blocks.lastIndex
    val lastBlock = blocks[lastIdx]
    val lines = lastBlock.lines()
    val firstLine = lines.first().trim()
    val isHeadingLine = (firstLine.startsWith("**") && firstLine.endsWith("**") && firstLine.length > 4) ||
        firstLine.startsWith("#")
    return if (isHeadingLine && lines.size > 1) {
        val cleanTitle = firstLine.removeSurrounding("**").replace(HEADING_HASH_PREFIX_REGEX, "").trim()
        val body = lines.drop(1).joinToString(" ").replace(PERSISTENCE_WHITESPACE_REGEX, " ").trim()
        cleanTitle to body
    } else if (isHeadingLine && lines.size == 1) {
        val cleanTitle = firstLine.removeSurrounding("**").replace(HEADING_HASH_PREFIX_REGEX, "").trim()
        cleanTitle to ""
    } else {
        val prevBlock = if (lastIdx > 0) blocks[lastIdx - 1].trim() else ""
        val prevIsStandaloneHeading = prevBlock.isNotEmpty() &&
            !prevBlock.contains('\n') &&
            ((prevBlock.startsWith("**") && prevBlock.endsWith("**") && prevBlock.length > 4) || prevBlock.startsWith("#"))
        val cleanTitle = if (prevIsStandaloneHeading) {
            prevBlock.removeSurrounding("**").replace(HEADING_HASH_PREFIX_REGEX, "").trim()
        } else ""
        val cleanBody = lastBlock.replace(PERSISTENCE_WHITESPACE_REGEX, " ").trim()
        cleanTitle to cleanBody
    }
}

/**
 * Builds a fresh `ChatSession` for a given `AssistantAgent`, faithfully preserving the Assistant's
 * bound profile, default model, active Character/Scenario Card, opening greeting (with `userName`),
 * and workspace configuration.
 */
internal fun buildFreshSessionForAssistant(
    appState: AppStateSnapshot,
    activeProfile: ProviderProfile,
    assistant: AssistantAgent,
    userName: String = Strings.user,
    now: Long = PlatformRuntime.currentTimeMillis()
): ChatSession {
    val profId = assistant.defaultProfileId.ifBlank {
        appState.defaultModels.globalProfileId.ifBlank { activeProfile.id }
    }
    val modId = assistant.defaultModelId.ifBlank {
        appState.defaultModels.globalModelId.ifBlank {
            (appState.profiles.find { it.id == profId } ?: activeProfile).defaultModel
        }
    }
    val card = assistant.activeCard
    val cleanUser = userName.ifBlank { Strings.user }
    val initialMessages = if (card != null && card.firstMessage.isNotBlank()) {
        val allGreetings = (listOf(card.firstMessage) + card.alternateGreetings).filter { it.isNotBlank() }
        listOf(
            MessageNode(
                id = "node-greeting-$now",
                role = MessageRole.ASSISTANT,
                variants = allGreetings.mapIndexed { idx, g ->
                    MessageVariant(
                        id = "var-greeting-$idx-$now",
                        content = g.replace("{{char}}", card.name, ignoreCase = true)
                            .replace("{{user}}", cleanUser, ignoreCase = true),
                        modelUsed = "Card Greeting #${idx + 1}",
                        createdAt = now
                    )
                }
            )
        )
    } else emptyList()

    return ChatSession(
        id = "sess-$now",
        title = Strings.newChat,
        createdAt = now,
        updatedAt = now,
        profileId = profId,
        modelOverride = modId,
        assistantId = assistant.id,
        userName = cleanUser,
        activeCard = card,
        viewMode = appState.preferences.defaultViewMode,
        permissionOverrides = AgentPermissionOverrides(),
        messages = initialMessages
    )
}

/**
 * Strips duplicate in-memory base64 strings from attachments when `localFilePath` is already persisted on disk,
 * automatically relocates any legacy root `media/` files into `chats/<sessionId>/media/`, and caps raw wire previews.
 */
internal fun stripRedundantAttachmentBase64(session: ChatSession): ChatSession {
    var changed = false
    val sessId = session.id
    val cleanedMessages = session.messages.map { node ->
        var nodeChanged = false
        val cleanedVariants = node.variants.map { v ->
            var varAttChanged = false
            val updatedAtts = if (v.attachments.isNotEmpty()) {
                v.attachments.map { att ->
                    val relocatedPath = if (att.localFilePath.isNotBlank() && sessId.isNotBlank()) {
                        PlatformStorage.relocateLegacyMediaToChatFolder(att.localFilePath, sessId)
                    } else att.localFilePath
                    val shouldStripB64 = relocatedPath.isNotBlank() &&
                        att.base64Data.isNotBlank() &&
                        PlatformStorage.isReadableLocalFile(relocatedPath)
                    if (relocatedPath != att.localFilePath || shouldStripB64) {
                        varAttChanged = true
                        att.copy(
                            localFilePath = relocatedPath,
                            base64Data = if (shouldStripB64) "" else att.base64Data
                        )
                    } else att
                }
            } else v.attachments

            var varMediaChanged = false
            val updatedMedia = if (v.mediaItems.isNotEmpty() && sessId.isNotBlank()) {
                v.mediaItems.map { m ->
                    val relocatedPath = if (m.localFilePath.isNotBlank()) {
                        PlatformStorage.relocateLegacyMediaToChatFolder(m.localFilePath, sessId)
                    } else m.localFilePath
                    if (relocatedPath != m.localFilePath) {
                        varMediaChanged = true
                        m.copy(localFilePath = relocatedPath)
                    } else m
                }
            } else v.mediaItems

            val rawVarSig = v.thinkingSignature
            val cleanVarSig = if (rawVarSig.length > 32 && rawVarSig.indexOf('=') in 0 until rawVarSig.trimEnd('=').length) {
                sanitizeGeminiThoughtSignature(rawVarSig, forFunctionCall = false)
            } else rawVarSig
            var varToolsSigChanged = false
            val updatedTools = if (v.toolCalls.isNotEmpty()) {
                v.toolCalls.map { tc ->
                    val rawTcSig = tc.thoughtSignature
                    val hasGluedTcSig = rawTcSig.length > 32 &&
                        (
                            rawTcSig.indexOf('=') in 0 until rawTcSig.trimEnd('=').length ||
                                isGeminiServerSideToolSignature(rawTcSig)
                            )
                    if (hasGluedTcSig) {
                        val sanitizedTcSig = sanitizeGeminiThoughtSignature(rawTcSig, forFunctionCall = true)
                        if (sanitizedTcSig != rawTcSig) {
                            varToolsSigChanged = true
                            tc.copy(thoughtSignature = sanitizedTcSig)
                        } else tc
                    } else tc
                }
            } else v.toolCalls

            val hasOversizedWire = v.rawRequestPreview.length > 4096 || v.rawResponsePreview.length > 4096
            val varSigChanged = cleanVarSig != rawVarSig
            if (varAttChanged || varMediaChanged || hasOversizedWire || varSigChanged || varToolsSigChanged) {
                nodeChanged = true
                v.copy(
                    attachments = updatedAtts,
                    mediaItems = updatedMedia,
                    thinkingSignature = cleanVarSig,
                    toolCalls = updatedTools,
                    rawRequestPreview = if (v.rawRequestPreview.length > 4096) v.rawRequestPreview.take(4096) else v.rawRequestPreview,
                    rawResponsePreview = if (v.rawResponsePreview.length > 4096) v.rawResponsePreview.take(4096) else v.rawResponsePreview
                )
            } else v
        }
        if (nodeChanged) {
            changed = true
            node.copy(variants = cleanedVariants)
        } else node
    }
    return if (changed) session.copy(messages = cleanedMessages) else session
}

internal fun mergeSessionMetadata(full: ChatSession, metadata: ChatSession): ChatSession = full.copy(
    title = metadata.title,
    isPinned = metadata.isPinned,
    isTitleManuallySet = metadata.isTitleManuallySet,
    updatedAt = maxOf(full.updatedAt, metadata.updatedAt),
    assistantId = metadata.assistantId,
    profileId = metadata.profileId,
    modelOverride = metadata.modelOverride,
    workspacePath = metadata.workspacePath,
    workspaceMode = metadata.workspaceMode,
    permissionOverrides = metadata.permissionOverrides,
    draftInput = metadata.draftInput,
    draftPrefill = metadata.draftPrefill,
    userName = metadata.userName,
    viewMode = metadata.viewMode,
    systemPrompt = metadata.systemPrompt,
    activeCard = metadata.activeCard,
    authorsNoteDepth = metadata.authorsNoteDepth,
    stubMessageCount = 0,
    isCompactStub = false
)

/**
 * Converts an inactive `ChatSession` into a compact preview while preserving
 * exact `messages.size` and a 140-char preview of the final message so sidebar badges and tab switchers
 * work identically without retaining full conversation content.
 */
/**
 * The shape of a compact stub. `chat-index.json` stores stubs, so bump this whenever
 * [dehydrateChatSessionToCompactStub] keeps something new or different; old indexes are then ignored and rebuilt.
 */
internal const val CHAT_STUB_FORMAT = 1

internal fun dehydrateChatSessionToCompactStub(session: ChatSession): ChatSession {
    if (session.isCompactStub || session.id.isBlank() || session.messages.isEmpty()) return session
    val msgCount = session.messages.size
    val lastNode = session.messages.last()
    val lastPreviewText = lastNode.activeVariant.content.take(
        if (lastNode.pinned) maxOf(140, PIN_PREVIEW_CHARS) else 140
    )
    var hasAttachments = false
    var hasMedia = false
    for (msg in session.messages) {
        for (v in msg.variants) {
            if (!hasAttachments && v.attachments.isNotEmpty()) hasAttachments = true
            if (!hasMedia && v.mediaItems.isNotEmpty()) hasMedia = true
            if (hasAttachments && hasMedia) break
        }
        if (hasAttachments && hasMedia) break
    }
    // Preserve only localFilePath references so Storage Manager orphan detection knows which media files belong to this chat
    val refAttachments = if (!hasAttachments) emptyList() else {
        session.messages
            .asSequence()
            .flatMap { it.variants.asSequence() }
            .flatMap { it.attachments.asSequence() }
            .filter { it.localFilePath.isNotBlank() }
            .map { PromptAttachment(id = "", fileName = "", mimeType = "", localFilePath = it.localFilePath) }
            .distinctBy { it.localFilePath }
            .toList()
    }
    val refMedia = if (!hasMedia) emptyList() else {
        session.messages
            .asSequence()
            .flatMap { it.variants.asSequence() }
            .flatMap { it.mediaItems.asSequence() }
            .filter { it.localFilePath.isNotBlank() }
            .map { GeneratedMediaItem(id = "", kind = io.parchment.ai.model.MediaKind.IMAGE, mimeType = "", localFilePath = it.localFilePath) }
            .distinctBy { it.localFilePath }
            .toList()
    }
    val stubLastNode = MessageNode(
        id = lastNode.id,
        role = lastNode.role,
        variants = listOf(
            MessageVariant(
                id = lastNode.activeVariant.id,
                content = lastPreviewText,
                attachments = refAttachments,
                mediaItems = refMedia
            )
        ),
        pinned = lastNode.pinned
    )
    val stubList = buildList {
        for (i in 0 until msgCount - 1) {
            val node = session.messages[i]
            // Pinned replies keep their id and a preview so the all-chats pin list works without loading the chat.
            if (node.pinned) {
                add(
                    MessageNode(
                        id = node.id,
                        role = node.role,
                        variants = listOf(MessageVariant(id = node.activeVariant.id, content = node.activeVariant.content.take(PIN_PREVIEW_CHARS))),
                        pinned = true
                    )
                )
            }
        }
        add(stubLastNode)
    }
    return session.copy(
        storySummary = "",
        fragments = emptyList(),
        authorsNote = "",
        messages = stubList,
        stubMessageCount = msgCount,
        isCompactStub = true
    )
}

/**
 * Loads the full `ChatSession` from pending writes or `chats/<sessionId>/chat.json` if `session` is
 * a dehydrated stub in RAM.
 */
internal fun hydrateSessionFromDiskIfNeeded(session: ChatSession): ChatSession {
    if (!session.isCompactStub) return session
    val pendingFull = appDiskWriter.pendingFullSession(session.id)
    if (pendingFull != null) {
        return stripRedundantAttachmentBase64(mergeSessionMetadata(pendingFull, session))
    }
    val raw = runCatching { PlatformStorage.loadChatSessionJsonById(session.id) }.getOrNull()
    if (!raw.isNullOrBlank()) {
        val decoded = runCatching { compactStateJson.decodeFromString<ChatSession>(raw) }.getOrNull()
        if (decoded != null && decoded.id == session.id) {
            return stripRedundantAttachmentBase64(mergeSessionMetadata(decoded, session))
        }
    }
    return session
}

/**
 * Keeps the active session (and any streaming sessions) fully loaded in memory while
 * dehydrating inactive historical sessions to compact stubs after saving them to disk if needed.
 */
internal fun ensureActiveSessionHydratedAndDehydrateOthers(
    state: AppStateSnapshot,
    keepHydratedIds: Set<String> = emptySet()
): AppStateSnapshot {
    val activeId = state.activeSessionId
    var anyChanged = false
    val updatedSessions = state.sessions.map { sess ->
        val shouldStayHydrated = sess.id == activeId || sess.id in keepHydratedIds
        if (shouldStayHydrated) {
            if (sess.isCompactStub) {
                hydrateSessionFromDiskIfNeeded(sess).also { if (it !== sess) anyChanged = true }
            } else {
                val stripped = stripRedundantAttachmentBase64(sess)
                if (stripped !== sess) anyChanged = true
                stripped
            }
        } else {
            if (!sess.isCompactStub && sess.messages.isNotEmpty()) {
                anyChanged = true
                dehydrateChatSessionToCompactStub(sess)
            } else {
                sess
            }
        }
    }
    return if (anyChanged) state.copy(sessions = updatedSessions) else state
}

internal fun saveSettingsOnlyToDisk(snapshot: AppStateSnapshot) {
    val settingsSnapshot = snapshot.copy(sessions = emptyList())
    val encoded = compactStateJson.encodeToString(AppStateSnapshot.serializer(), settingsSnapshot)
    PlatformStorage.saveAppStateJson(encoded)
}

/** Applies background I/O results without replacing concurrent edits or resurrecting deleted chats. */
internal fun mergeLoadedSessions(
    current: AppStateSnapshot,
    beforeLoad: AppStateSnapshot,
    loaded: AppStateSnapshot
): AppStateSnapshot {
    val originals = beforeLoad.sessions.associateBy { it.id }
    val results = loaded.sessions.associateBy { it.id }
    val currentIds = current.sessions.map { it.id }.toSet()
    val sessions = current.sessions.map { session ->
        val result = results[session.id] ?: return@map session
        when {
            session.isCompactStub && !result.isCompactStub -> mergeSessionMetadata(result, session)
            session === originals[session.id] -> result
            else -> session
        }
    } + loaded.sessions.filter { it.id !in currentIds && it.id !in originals }
    return current.copy(sessions = sessions)
}

internal fun saveSingleChatSessionToDisk(session: ChatSession) {
    if (session.isCompactStub) {
        // Never overwrite a populated chat file on disk with a dehydrated stub; if metadata changed, merge onto disk copy
        val pendingFull = appDiskWriter.pendingFullSession(session.id)
        val raw = if (pendingFull == null) PlatformStorage.loadChatSessionJsonById(session.id) else null
        val fullDiskSession = pendingFull ?: raw?.let {
            runCatching { compactStateJson.decodeFromString<ChatSession>(it) }.getOrNull()
        }?.takeIf { it.id == session.id }
        check(fullDiskSession != null) { "This chat's full data isn't available, so it wasn't saved." }
        val merged = mergeSessionMetadata(fullDiskSession, session)
        val encoded = compactStateJson.encodeToString(ChatSession.serializer(), merged)
        PlatformStorage.saveChatSessionJson(merged.id, encoded)
        return
    }
    val cleanSession = stripRedundantAttachmentBase64(session).let { stripped ->
        stripped.copy(
            messages = stripped.messages.map { node ->
                node.copy(
                    variants = node.variants.map { v -> v.settledForDisk() }
                )
            }
        )
    }
    val encoded = compactStateJson.encodeToString(ChatSession.serializer(), cleanSession)
    PlatformStorage.saveChatSessionJson(cleanSession.id, encoded)
}

@Serializable
private class LiveTurnSidecar(val nodeId: String, val variant: MessageVariant)

/**
 * A streaming checkpoint that saves the reply alone (`chats/<id>/live.json`), so its cost follows the reply's size,
 * not the chat's. Settled like a full save: after a crash the reply shows as interrupted.
 */
internal fun saveLiveTurnToDisk(session: ChatSession, turn: LiveTurnRef) {
    val node = session.messages.find { it.id == turn.nodeId } ?: return
    val variant = node.variants.find { it.id == turn.variantId } ?: return
    val cleaned = stripRedundantAttachmentBase64(session.copy(messages = listOf(node.copy(variants = listOf(variant)))))
    val settled = cleaned.messages.single().variants.single().settledForDisk()
    PlatformStorage.saveChatLiveTurnJson(
        session.id,
        compactStateJson.encodeToString(LiveTurnSidecar.serializer(), LiveTurnSidecar(turn.nodeId, settled))
    )
}

internal fun decodeChatSessionsFromRawJson(rawJsonText: String): List<ChatSession> {
    val trimmed = rawJsonText.trim()
    if (trimmed.isEmpty()) return emptyList()
    // 1. Single ChatSession JSON object
    runCatching {
        val elem = compactStateJson.parseToJsonElement(trimmed)
        if (elem is JsonObject) {
            if (elem.containsKey("id") &&
                elem.containsKey("title") &&
                elem.containsKey("profileId") &&
                !elem.containsKey("profiles") &&
                !elem.containsKey("sessions")
            ) {
                return listOf(compactStateJson.decodeFromJsonElement(ChatSession.serializer(), elem).withImportedPermissions())
            }
            if (elem.containsKey("sessions")) {
                val arr = elem["sessions"] as? JsonArray
                if (arr != null) {
                    return arr.mapNotNull { item ->
                        runCatching { compactStateJson.decodeFromJsonElement(ChatSession.serializer(), item).withImportedPermissions() }.getOrNull()
                    }
                }
            }
        } else if (elem is JsonArray) {
            return elem.mapNotNull { item ->
                runCatching { compactStateJson.decodeFromJsonElement(ChatSession.serializer(), item).withImportedPermissions() }.getOrNull()
            }
        }
    }
    return emptyList()
}

internal fun buildExportFileNameForChat(session: ChatSession): String {
    val safeTitle = session.title.trim()
        .replace(EXPORT_UNSAFE_CHAR_REGEX, "_")
        .replace(MULTIPLE_UNDERSCORES_REGEX, "_")
        .trim('_')
        .take(42)
        .ifBlank { "chat" }
    val shortId = session.id.takeLast(8).replace(EXPORT_SHORT_ID_REGEX, "")
    return if (shortId.isNotEmpty()) "$safeTitle-$shortId.chat.json" else "$safeTitle.chat.json"
}

internal fun buildExportZipFileNameForChat(session: ChatSession): String = buildExportFileNameForChat(session).removeSuffix(".json") + ".zip"

internal fun encodeChatSessionJsonForExport(session: ChatSession): ByteArray {
    val full = hydrateSessionFromDiskIfNeeded(session)
    check(!full.isCompactStub) { "This chat's full data isn't available, so it wasn't exported." }
    return io.parchment.ai.protocol.CardAndPromptCodec.prettyJson
        .encodeToString(ChatSession.serializer(), full).encodeToByteArray()
}

/** Streams [sessions] to a chosen file, hydrating one chat at a time. Null means the user cancelled. */
internal suspend fun exportChatSessionsBundle(fileName: String, sessions: List<ChatSession>): String? = PlatformPickers.exportChatBundle(fileName, sessions.asSequence().map(::chatBundleEntry))

internal fun chatBundleEntry(rawSession: ChatSession): ChatBundleEntry {
    val mediaFileEntries = mutableListOf<Pair<String, String>>()
    val prettyJson = io.parchment.ai.protocol.CardAndPromptCodec.prettyJson
    val fullSession = stripRedundantAttachmentBase64(hydrateSessionFromDiskIfNeeded(rawSession).withLiveTurns())
    check(!fullSession.isCompactStub) { "This chat's full data isn't available, so it wasn't exported." }
    val safeSessId = fullSession.id.trim().replace(EXPORT_UNSAFE_CHAR_REGEX, "_").ifBlank { "session" }
    val portableMessages = fullSession.messages.map { node ->
        node.copy(
            variants = node.variants.map { v ->
                val portableAtts = v.attachments.map { att ->
                    if (att.localFilePath.isNotBlank()) {
                        val mediaName = att.localFilePath.replace('\\', '/').substringAfterLast('/')
                            .replace(EXPORT_UNSAFE_CHAR_REGEX, "_")
                        if (mediaName.isNotEmpty()) {
                            val zipEntryPath = "$safeSessId/media/$mediaName"
                            mediaFileEntries.add(zipEntryPath to att.localFilePath)
                            att.copy(localFilePath = "chats/$zipEntryPath", base64Data = "")
                        } else att
                    } else att
                }
                val portableMedia = v.mediaItems.map { m ->
                    if (m.localFilePath.isNotBlank()) {
                        val mediaName = m.localFilePath.replace('\\', '/').substringAfterLast('/')
                            .replace(EXPORT_UNSAFE_CHAR_REGEX, "_")
                        if (mediaName.isNotEmpty()) {
                            val zipEntryPath = "$safeSessId/media/$mediaName"
                            mediaFileEntries.add(zipEntryPath to m.localFilePath)
                            m.copy(localFilePath = "chats/$zipEntryPath")
                        } else m
                    } else m
                }
                v.copy(
                    isStreaming = false,
                    attachments = portableAtts,
                    mediaItems = portableMedia
                )
            }
        )
    }
    val portableSession = fullSession.copy(messages = portableMessages)
    val chatJsonBytes = prettyJson.encodeToString(ChatSession.serializer(), portableSession).encodeToByteArray()
    return ChatBundleEntry("$safeSessId/chat.json", chatJsonBytes, mediaFileEntries)
}

internal fun decodeImportedChats(rawJsons: List<String>): List<ChatSession> = rawJsons.flatMap(::decodeChatSessionsFromRawJson).also {
    check(it.isNotEmpty()) { "No valid chats were found." }
}

/** Lets the user choose chat exports and decodes each one, reporting per-file success or failure. */
internal suspend fun pickAndDecodeChatImports(): List<ChatImportOutcome<List<ChatSession>>> = PlatformPickers.pickAndImportChatFiles(::decodeImportedChats).map { outcome ->
    ChatImportOutcome(outcome.fileName, outcome.result.mapCatching { chats -> chats.map(::stripRedundantAttachmentBase64) })
}

/**
 * Start-up loader for the first frame: reads only `parchment-settings.json` and the single active
 * `chats/<activeSessionId>/chat.json` file synchronously before drawing the first frame.
 * Remaining historical chats are hydrated as lightweight stubs in the background on `Dispatchers.Default`.
 */
/** Active chats larger than this open from `chat-index.json` on Frame 1 and hydrate off-UI right after paint. */
private const val DEFERRED_ACTIVE_CHAT_DECODE_BYTES = 512L * 1024L

internal fun loadFastInitialAppState(): Pair<AppStateSnapshot, Boolean> {
    val rawSettings = PlatformStorage.loadAppStateJson()?.let { migrateLegacyProviderJson(it) }
    if (!rawSettings.isNullOrBlank()) {
        val baseLoaded: AppStateSnapshot? = runCatching {
            compactStateJson.decodeFromString<AppStateSnapshot>(rawSettings)
        }.getOrElse {
            runCatching { recoverAppStateFromPartialJson(rawSettings) }.getOrNull()
        }
        if (baseLoaded != null && baseLoaded.sessions.isEmpty()) {
            val activeId = baseLoaded.activeSessionId
            val activeFolder = PlatformStorage.chatFolderName(activeId)
            val stamps = runCatching { PlatformStorage.chatFileStamps() }.getOrDefault(emptyMap())
            val activeStamp = stamps[activeFolder]
            if (activeStamp != null && activeStamp.length > DEFERRED_ACTIVE_CHAT_DECODE_BYTES) {
                val indexEntry = readChatStubIndex().entries[activeFolder]
                if (indexEntry != null && indexEntry.matches(activeStamp) && indexEntry.stub.id == activeId) {
                    val sanitized = sanitizeLoadedSnapshot(baseLoaded.copy(sessions = listOf(indexEntry.restoredStub())))
                    return sanitized to true
                }
            }
            val activeRaw = PlatformStorage.loadChatSessionJsonById(activeId)
            val activeSession = activeRaw?.let {
                runCatching { compactStateJson.decodeFromString<ChatSession>(it) }.getOrNull()
            }?.let { stripRedundantAttachmentBase64(it) }
            if (activeSession != null) {
                val sanitized = sanitizeLoadedSnapshot(baseLoaded.copy(sessions = listOf(activeSession)))
                return sanitized to true
            }
        }
    } else {
        val stamps = runCatching { PlatformStorage.chatFileStamps() }.getOrDefault(emptyMap())
        if (stamps.isNotEmpty()) {
            val newestFolder = stamps.entries.maxWithOrNull(
                compareBy<Map.Entry<String, io.parchment.ai.platform.ChatFileStamp>> { it.value.lastModifiedMs }.thenBy { it.key }
            )?.key
            val newestRaw = newestFolder?.let { PlatformStorage.loadChatSessionJsonById(it) }
            val newestSession = newestRaw?.let {
                runCatching { compactStateJson.decodeFromString<ChatSession>(it) }.getOrNull()
            }?.takeIf { it.id.isNotBlank() }?.let { stripRedundantAttachmentBase64(it) }
            if (newestSession != null) {
                val fallback = createDefaultAppState()
                val sanitized = sanitizeLoadedSnapshot(
                    fallback.copy(
                        sessions = listOf(newestSession),
                        activeSessionId = newestSession.id,
                        openTabSessionIds = listOf(newestSession.id)
                    )
                )
                appDiskWriter.enqueue(settings = sanitized, sessions = emptyList(), urgent = true)
                return sanitized to true
            }
        }
    }
    return ensureActiveSessionHydratedAndDehydrateOthers(loadOrCreateDefaultAppState()) to false
}

/**
 * Background worker (`Dispatchers.Default`) that loads remaining `chats/<sessionId>/chat.json` files after Frame 1,
 * converting inactive sessions into compact previews.
 */
internal fun loadRemainingHistoricalSessionsDehydrated(
    currentState: AppStateSnapshot,
    keepHydratedIds: Set<String> = emptySet()
): AppStateSnapshot {
    val activeId = currentState.activeSessionId
    val mergedById = LinkedHashMap<String, ChatSession>()
    for (existing in currentState.sessions) {
        if (existing.id.isNotBlank()) mergedById[existing.id] = existing
    }
    var loadedAny = false
    // Stamps first: a chat written after its stamp was read gets a newer stamp, so the index never vouches for it.
    val stamps = runCatching { PlatformStorage.chatFileStamps() }.getOrDefault(emptyMap())
    val index = readChatStubIndex()
    val nextEntries = HashMap<String, ChatIndexEntry>()
    for ((folder, stamp) in stamps) {
        val entry = index.entries[folder] ?: continue
        val id = entry.stub.id
        if (!entry.matches(stamp) || id.isBlank() || id == activeId || id in keepHydratedIds) continue
        if (PlatformStorage.chatFolderName(id) != folder) continue
        loadedAny = true
        nextEntries[folder] = entry
        mergedById[id] = entry.restoredStub()
    }
    // One chat at a time: each file's text and full decode are dropped once its stub is kept.
    // Skip re-reading the active chat folder when it is already present in state (hydrated separately on tab effect).
    val skipFolders = HashSet<String>(nextEntries.keys)
    val activeFolderName = PlatformStorage.chatFolderName(activeId)
    if (activeId.isNotBlank() && activeId in mergedById && activeFolderName in stamps) {
        val existingActive = mergedById[activeId]
        val existingEntry = index.entries[activeFolderName]
        if (existingActive != null && existingEntry != null && existingEntry.matches(stamps.getValue(activeFolderName))) {
            nextEntries[activeFolderName] = existingEntry
            skipFolders += activeFolderName
        } else if (existingActive != null && !existingActive.isCompactStub) {
            nextEntries[activeFolderName] = ChatIndexEntry.of(stamps.getValue(activeFolderName), dehydrateChatSessionToCompactStub(existingActive))
            skipFolders += activeFolderName
        }
    }
    for (rawChat in PlatformStorage.chatSessionJsons(skipFolders = skipFolders)) {
        loadedAny = true
        val decoded = runCatching { compactStateJson.decodeFromString<ChatSession>(rawChat) }.getOrNull() ?: continue
        if (decoded.id.isBlank()) continue
        val keepFull = decoded.id == activeId || decoded.id in keepHydratedIds
        val stub = if (keepFull) null else dehydrateChatSessionToCompactStub(decoded)
        val folder = PlatformStorage.chatFolderName(decoded.id)
        stamps[folder]?.let { stamp ->
            nextEntries[folder] = ChatIndexEntry.of(stamp, stub ?: dehydrateChatSessionToCompactStub(decoded))
        }
        val alreadyInState = mergedById[decoded.id]
        if (alreadyInState != null && keepFull) {
            // Preserve live in-memory state of active / streaming sessions
            continue
        }
        mergedById[decoded.id] = stub ?: stripRedundantAttachmentBase64(decoded)
    }
    if (nextEntries != index.entries) {
        runCatching {
            val updated = ChatStubIndex(stubFormat = CHAT_STUB_FORMAT, entries = nextEntries)
            PlatformStorage.saveChatIndexJson(compactStateJson.encodeToString(ChatStubIndex.serializer(), updated))
        }
    }
    if (!loadedAny) return currentState
    val combinedSessions = mergedById.values
        .sortedWith(compareByDescending<ChatSession> { it.isPinned }.thenByDescending { it.updatedAt })
    val validSessionIdSet = combinedSessions.map { it.id }.toSet()
    val safeOpenTabs = (currentState.openTabSessionIds.filter { it in validSessionIdSet } + activeId).distinct()
    return currentState.copy(
        sessions = combinedSessions,
        openTabSessionIds = safeOpenTabs
    )
}

/**
 * `chat-index.json`: each chat's sidebar stub, keyed by chat folder and valid only while the chat file's size and
 * modification time match. It spares start-up a full decode of every chat; a missing, corrupt or stale entry just
 * falls back to reading that `chat.json`, which stays the only source of truth.
 */
@Serializable
internal data class ChatStubIndex(
    val version: Int = 1,
    /** [CHAT_STUB_FORMAT] the stubs were made with; 0 (missing) means an index older than the field. */
    val stubFormat: Int = 0,
    val entries: Map<String, ChatIndexEntry> = emptyMap()
)

@Serializable
internal data class ChatIndexEntry(
    val fileLength: Long,
    val lastModifiedMs: Long,
    /** [ChatSession.stubMessageCount] and [ChatSession.isCompactStub] are transient, so they travel here. */
    val messageCount: Int,
    val isStub: Boolean,
    val stub: ChatSession
) {
    fun matches(stamp: ChatFileStamp): Boolean = fileLength == stamp.length && lastModifiedMs == stamp.lastModifiedMs

    fun restoredStub(): ChatSession = stub.copy(isCompactStub = isStub, stubMessageCount = if (isStub) messageCount else 0)

    companion object {
        fun of(stamp: ChatFileStamp, stub: ChatSession) = ChatIndexEntry(
            fileLength = stamp.length,
            lastModifiedMs = stamp.lastModifiedMs,
            messageCount = stub.stubMessageCount,
            isStub = stub.isCompactStub,
            stub = stub
        )
    }
}

private fun readChatStubIndex(): ChatStubIndex = runCatching {
    PlatformStorage.loadChatIndexJson()?.let { compactStateJson.decodeFromString<ChatStubIndex>(it) }
}.getOrNull()?.takeIf { it.version == 1 && it.stubFormat == CHAT_STUB_FORMAT } ?: ChatStubIndex(stubFormat = CHAT_STUB_FORMAT)

internal fun loadOrCreateDefaultAppState(): AppStateSnapshot {
    val rawSettings = PlatformStorage.loadAppStateJson()?.let { migrateLegacyProviderJson(it) }
    val rawChatFiles = PlatformStorage.loadAllChatSessionJsons()
    val diskSessions = rawChatFiles.mapNotNull { rawChat ->
        runCatching { compactStateJson.decodeFromString<ChatSession>(rawChat) }.getOrNull()
    }

    if (!rawSettings.isNullOrBlank()) {
        val baseLoaded: AppStateSnapshot? = runCatching {
            compactStateJson.decodeFromString<AppStateSnapshot>(rawSettings)
        }.getOrElse {
            runCatching { recoverAppStateFromPartialJson(rawSettings) }.getOrNull()
        }

        if (baseLoaded != null) {
            // Merge sessions from chats/*.json with any legacy embedded sessions in parchment-state.json
            val mergedById = LinkedHashMap<String, ChatSession>()
            for (ds in diskSessions) {
                if (ds.id.isNotBlank()) mergedById[ds.id] = ds
            }
            val migratedLegacySessions = mutableListOf<ChatSession>()
            for (legacySess in baseLoaded.sessions) {
                if (legacySess.id.isBlank()) continue
                val existing = mergedById[legacySess.id]
                if (existing == null || legacySess.updatedAt > existing.updatedAt) {
                    mergedById[legacySess.id] = legacySess
                    migratedLegacySessions.add(legacySess)
                }
            }
            val combinedSessions = mergedById.values
                .sortedWith(compareByDescending<ChatSession> { it.isPinned }.thenByDescending { it.updatedAt })
            val sanitized = sanitizeLoadedSnapshot(baseLoaded.copy(sessions = combinedSessions))
            if (migratedLegacySessions.isNotEmpty() || baseLoaded.sessions.isNotEmpty()) {
                // The writer saves full chats before committing settings without embedded history.
                appDiskWriter.enqueue(settings = sanitized, sessions = migratedLegacySessions, urgent = true, settingsRequiresSessions = true)
            } else if (diskSessions.isEmpty()) {
                appDiskWriter.enqueue(settings = sanitized, sessions = sanitized.sessions, urgent = true, settingsRequiresSessions = true)
            }
            return sanitized
        }
    }

    if (diskSessions.isNotEmpty()) {
        val fallback = createDefaultAppState()
        val sanitized = sanitizeLoadedSnapshot(fallback.copy(sessions = diskSessions))
        appDiskWriter.enqueue(settings = sanitized, sessions = sanitized.sessions, urgent = true, settingsRequiresSessions = true)
        return sanitized
    }

    val fresh = createDefaultAppState()
    appDiskWriter.enqueue(settings = fresh, sessions = fresh.sessions, urgent = true, settingsRequiresSessions = true)
    return fresh
}

/** Moves display settings to the page design once. Every choice stays available in Settings. */
private fun applyDesignDefaultsOnce(saved: AppStateSnapshot): AppStateSnapshot {
    if (saved.designDefaultsApplied) return saved
    val builtInProfiles = setOf("prof-gemini-interactions", "prof-openai-responses", "prof-anthropic-messages")
    return saved.copy(
        designDefaultsApplied = true,
        profiles = saved.profiles.map {
            if (it.id in builtInProfiles && it.thinkingEffort == "high") it.copy(thinkingEffort = "none") else it
        },
        preferences = saved.preferences.copy(
            showUserAvatar = false,
            showUserName = false,
            showUserTimestamp = false,
            showProviderAfterModelName = false,
            showAssistantAvatarInTitleBar = false
        )
    )
}

internal fun sanitizeLoadedSnapshot(saved: AppStateSnapshot): AppStateSnapshot {
    val loaded = applyDesignDefaultsOnce(saved)
    val defaultFallback = createDefaultAppState()
    val safeProfiles = (if (loaded.profiles.isNotEmpty()) loaded.profiles else defaultFallback.profiles)
        .map { if (it.allowShellExecution) it.copy(allowShellExecution = false) else it }
    val safeActiveProfileId = safeProfiles.find { it.id == loaded.activeProfileId }?.id ?: safeProfiles.first().id
    val safeActiveProfile = safeProfiles.first { it.id == safeActiveProfileId }

    val safeAssistants = if (loaded.assistants.isNotEmpty()) {
        loaded.assistants.map { assistant ->
            if (assistant.permissionDefaults == null) assistant.copy(permissionDefaults = AgentPermissionResolver.defaults(assistant)) else assistant
        }
    } else {
        defaultFallback.assistants
    }
    val safeActiveAssistantId = safeAssistants.find { it.id == loaded.activeAssistantId }?.id
        ?: safeAssistants.first().id

    val recoveredSessions = if (loaded.sessions.isNotEmpty()) {
        loaded.sessions.map { sess ->
            sess.copy(
                assistantId = sess.assistantId.ifBlank { safeActiveAssistantId },
                permissionOverrides = sess.permissionOverrides ?: AgentPermissionResolver.legacyOverrides(
                    sess,
                    safeProfiles.find { it.id == sess.profileId }
                ),
                messages = sess.messages.map { node ->
                    node.copy(
                        variants = node.variants.map { v -> v.settledForDisk() }
                    )
                }
            )
        }
    } else {
        val now = PlatformRuntime.currentTimeMillis()
        listOf(
            ChatSession(
                id = "sess-$now",
                title = Strings.newChat,
                createdAt = now,
                updatedAt = now,
                profileId = safeActiveProfile.id,
                modelOverride = safeActiveProfile.defaultModel,
                assistantId = safeActiveAssistantId
            )
        )
    }

    val safeActiveSessionId = recoveredSessions.find { it.id == loaded.activeSessionId }?.id ?: recoveredSessions.first().id
    val validSessionIdSet = recoveredSessions.map { it.id }.toSet()
    val safeOpenTabs = (loaded.openTabSessionIds.filter { it in validSessionIdSet } + safeActiveSessionId).distinct()

    val migratedTheme = migrateThemeTypefaceDefaults(
        if (loaded.theme.name.endsWith("Warm Ivory (Light)") && !loaded.theme.name.startsWith("YL Axiom ")) {
            loaded.theme.copy(name = "YL Axiom Warm Ivory (Light)")
        } else {
            loaded.theme
        }
    )

    return loaded.copy(
        profiles = safeProfiles,
        activeProfileId = safeActiveProfileId,
        assistants = safeAssistants,
        activeAssistantId = safeActiveAssistantId,
        sessions = recoveredSessions,
        activeSessionId = safeActiveSessionId,
        openTabSessionIds = safeOpenTabs,
        theme = migratedTheme
    )
}

internal fun recoverAppStateFromPartialJson(raw: String): AppStateSnapshot {
    val fallback = createDefaultAppState()
    val migratedRaw = migrateLegacyProviderJson(raw)
    val root = compactStateJson.parseToJsonElement(migratedRaw) as? JsonObject ?: return fallback

    fun <T> decodeListItems(key: String, itemDeserializer: kotlinx.serialization.DeserializationStrategy<T>): List<T> {
        val arr = root[key] as? JsonArray ?: return emptyList()
        return arr.mapNotNull { elem ->
            runCatching { compactStateJson.decodeFromJsonElement(itemDeserializer, elem) }.getOrNull()
        }
    }

    val profiles = decodeListItems("profiles", ProviderProfile.serializer()).ifEmpty { fallback.profiles }
    val activeProfileId = (root["activeProfileId"] as? JsonPrimitive)?.contentOrNull ?: profiles.first().id
    val assistants = decodeListItems("assistants", AssistantAgent.serializer()).ifEmpty { fallback.assistants }
    val activeAssistantId = (root["activeAssistantId"] as? JsonPrimitive)?.contentOrNull ?: assistants.firstOrNull()?.id.orEmpty()
    val defaultModels = root["defaultModels"]?.let {
        runCatching { compactStateJson.decodeFromJsonElement(DefaultModelConfig.serializer(), it) }.getOrNull()
    } ?: fallback.defaultModels
    val preferences = root["preferences"]?.let {
        runCatching { compactStateJson.decodeFromJsonElement(PreferencesConfig.serializer(), it) }.getOrNull()
    } ?: fallback.preferences
    val desktopWindow = root["desktopWindow"]?.let {
        runCatching { compactStateJson.decodeFromJsonElement(DesktopWindowConfig.serializer(), it) }.getOrNull()
    } ?: fallback.desktopWindow
    val sessions = decodeListItems("sessions", ChatSession.serializer())
    val activeSessionId = (root["activeSessionId"] as? JsonPrimitive)?.contentOrNull ?: sessions.firstOrNull()?.id.orEmpty()
    val openTabs = (root["openTabSessionIds"] as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.orEmpty()
    val presets = decodeListItems("systemPromptPresets", SystemPromptPreset.serializer())
    val savedCards = decodeListItems("savedCards", CharacterOrScenarioCard.serializer())
    val globalWorldBook = decodeListItems("globalWorldBook", ContextFragment.serializer())
    val theme = root["theme"]?.let {
        runCatching { compactStateJson.decodeFromJsonElement(ThemeConfig.serializer(), it) }.getOrNull()
    }?.let { migrateThemeTypefaceDefaults(it) } ?: fallback.theme
    val otaRepo = (root["otaGithubRepo"] as? JsonPrimitive)?.contentOrNull ?: fallback.otaGithubRepo
    val autoOta = (root["autoCheckOtaOnStartup"] as? JsonPrimitive)?.booleanOrNull ?: fallback.autoCheckOtaOnStartup

    return AppStateSnapshot(
        profiles = profiles,
        activeProfileId = activeProfileId,
        assistants = assistants,
        activeAssistantId = activeAssistantId,
        defaultModels = defaultModels,
        preferences = preferences,
        desktopWindow = desktopWindow,
        sessions = sessions,
        activeSessionId = activeSessionId,
        openTabSessionIds = openTabs,
        systemPromptPresets = presets,
        savedCards = savedCards,
        globalWorldBook = globalWorldBook,
        theme = theme,
        otaGithubRepo = otaRepo,
        autoCheckOtaOnStartup = autoOta
    )
}

internal fun createDefaultAppState(): AppStateSnapshot {
    val now = PlatformRuntime.currentTimeMillis()
    val geminiProfile = ProviderProfile(
        id = "prof-gemini-interactions",
        name = "Google Gemini",
        protocol = ProviderProtocol.GEMINI_INTERACTIONS
    )
    val openAiProfile = ProviderProfile(
        id = "prof-openai-responses",
        name = "OpenAI",
        protocol = ProviderProtocol.OPENAI_RESPONSES
    )
    val anthropicProfile = ProviderProfile(
        id = "prof-anthropic-messages",
        name = "Anthropic",
        protocol = ProviderProtocol.ANTHROPIC_MESSAGES
    )

    val defaultAssistant = AssistantAgent(
        id = "asst-default",
        name = "Default Assistant",
        iconEmoji = "💬",
        systemPrompt = ""
    )

    val initialSession = ChatSession(
        id = "sess-initial",
        title = Strings.newChat,
        createdAt = now,
        updatedAt = now,
        profileId = geminiProfile.id,
        assistantId = defaultAssistant.id
    )

    return AppStateSnapshot(
        profiles = listOf(geminiProfile, openAiProfile, anthropicProfile),
        activeProfileId = geminiProfile.id,
        assistants = listOf(defaultAssistant),
        activeAssistantId = defaultAssistant.id,
        sessions = listOf(initialSession),
        activeSessionId = initialSession.id,
        openTabSessionIds = listOf(initialSession.id),
        theme = ThemeConfig(),
        designDefaultsApplied = true,
        preferences = PreferencesConfig(
            showUserAvatar = false,
            showUserName = false,
            showUserTimestamp = false,
            showProviderAfterModelName = false,
            showAssistantAvatarInTitleBar = false
        )
    )
}
