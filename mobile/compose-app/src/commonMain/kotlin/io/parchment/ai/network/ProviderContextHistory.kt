package io.parchment.ai.network

import io.ktor.client.request.get
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.harness.wasCutOff
import io.parchment.ai.harness.withInterruptedOutcome
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ContextFragment
import io.parchment.ai.model.FragmentActivationMode
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.platform.PlatformRuntime

// The history and system prompt each provider request is built from.

private val INLINE_ERROR_PREFIXES = listOf(
    "**[Provider Error:",
    "**[Stream Error:",
    "*[Stream paused/interrupted",
    "*[Stream interrupted",
    "*[Interrupted:"
)

private val INLINE_ERROR_SUFFIXES = listOf(
    AiProviderEngine.OUTPUT_LIMIT_NOTICE,
    "\n\n*[Stream paused/interrupted:",
    "\n\n*[Stream interrupted",
    "\n\n*[Interrupted:"
)

private fun collapseWhitespaceBounded(raw: String, maxChars: Int): String {
    if (raw.isEmpty() || maxChars <= 0) return ""
    var i = 0
    val n = raw.length
    while (i < n && raw[i].isWhitespace()) i++
    if (i == n) return ""
    val sb = StringBuilder(minOf(maxChars, n - i))
    var pendingSpace = false
    while (i < n && sb.length < maxChars) {
        val ch = raw[i++]
        if (ch.isWhitespace()) {
            pendingSpace = true
        } else {
            if (pendingSpace) {
                sb.append(' ')
                if (sb.length >= maxChars) break
                pendingSpace = false
            }
            sb.append(ch)
        }
    }
    return sb.toString()
}

/**
 * Strips UI-only inline error banners and mid-stream interruption footers from assistant prose
 * so they are never sent back to LLM providers as part of conversation history or continuation prefixes.
 */
fun AiProviderEngine.stripInlineErrorNotices(raw: String): String {
    val len = raw.length
    if (len == 0) return ""
    var firstNonWs = 0
    while (firstNonWs < len && raw[firstNonWs].isWhitespace()) firstNonWs++
    if (firstNonWs == len) return ""
    for (prefix in INLINE_ERROR_PREFIXES) {
        if (raw.startsWith(prefix, startIndex = firstNonWs)) {
            return ""
        }
    }
    var endIdx = len
    for (marker in INLINE_ERROR_SUFFIXES) {
        val idx = raw.indexOf(marker)
        if (idx in 0 until endIdx) {
            endIdx = idx
        }
    }
    while (endIdx > 0 && raw[endIdx - 1].isWhitespace()) {
        endIdx--
    }
    return if (endIdx == len) raw else raw.substring(0, endIdx)
}

internal fun AiProviderEngine.hasMeaningfulAssistantContext(v: MessageVariant): Boolean {
    val cleanProse = stripInlineErrorNotices(v.content)
    return cleanProse.isNotBlank() ||
        v.thinkingSummary.isNotBlank() ||
        v.thinkingSignature.isNotBlank() ||
        v.toolCalls.isNotEmpty() ||
        v.mediaItems.isNotEmpty() ||
        v.attachments.isNotEmpty()
}

internal fun AiProviderEngine.sanitizeContextNodes(historyNodes: List<MessageNode>): List<MessageNode> {
    return historyNodes.filter { node ->
        if (!node.includedInContext) return@filter false
        if (node.role == MessageRole.ASSISTANT) {
            return@filter hasMeaningfulAssistantContext(node.activeVariant)
        }
        true
    }
}

/**
 * Synthesizes an in-progress assistant turn + resume user prompt whenever a turn has already
 * accumulated tool executions or thoughts (either before a mid-stream continuation pass or before
 * falling back from a newer protocol to a legacy protocol mid-turn), ensuring zero loss of
 * thoughts, cryptographic signatures, tool calls, or partial prose.
 */
internal fun AiProviderEngine.buildEffectiveHistoryAndPrefix(
    baseHistoryNodes: List<MessageNode>,
    continuePrefix: String,
    thinkingText: String,
    trailingThinkingText: String = "",
    signatureText: String,
    executedTools: List<ToolCallRecord>,
    generatedMedia: List<GeneratedMediaItem>,
    protocol: ProviderProtocol,
    model: String,
    passTag: String,
    forceVisibleThoughtSummary: Boolean = false,
    wireDialect: String = ""
): Pair<List<MessageNode>, String> {
    val sanitizedTools = executedTools.map { tc -> if (tc.wasCutOff()) tc.withInterruptedOutcome() else tc }
    val hasPriorTurnProgress = sanitizedTools.isNotEmpty() ||
        (continuePrefix.isBlank() && (thinkingText.isNotEmpty() || signatureText.isNotEmpty()))
    return if (hasPriorTurnProgress) {
        val visibleThoughtInjected = forceVisibleThoughtSummary && continuePrefix.isBlank() && thinkingText.isNotBlank()
        val effectiveAssistantContent = when {
            sanitizedTools.isNotEmpty() && continuePrefix.isNotBlank() -> continuePrefix
            visibleThoughtInjected -> "[Completed internal reasoning:\n$thinkingText]"
            else -> ""
        }
        // When pure-reasoning nudge already surfaces the thoughts in content, omit duplicate native thinking fields.
        val suppressDuplicateNativeThinking = visibleThoughtInjected && sanitizedTools.isEmpty()
        val partialVar = MessageVariant(
            id = "var-in-progress-$passTag",
            content = effectiveAssistantContent,
            thinkingSummary = if (suppressDuplicateNativeThinking) "" else thinkingText,
            trailingThinking = if (sanitizedTools.isNotEmpty()) trailingThinkingText else "",
            thinkingSignature = if (suppressDuplicateNativeThinking) "" else signatureText,
            toolCalls = sanitizedTools,
            mediaItems = generatedMedia,
            providerProtocol = protocol,
            providerWireDialect = wireDialect,
            modelUsed = model
        )
        val partialAssistantNode = MessageNode(
            id = "node-in-progress-$passTag",
            role = MessageRole.ASSISTANT,
            variants = listOf(partialVar)
        )
        val resumePrompt = if (executedTools.isNotEmpty() && continuePrefix.isNotBlank()) {
            "[System Note: Continue seamlessly from the exact end of this partial passage without repeating completed tool calls or text:\n\"$continuePrefix\"]"
        } else {
            val stepDesc = if (executedTools.isNotEmpty()) "internal reasoning and tool executions" else "internal reasoning"
            "[System Note: You have already completed your $stepDesc above, but your turn ended before you wrote your final response to the user. Do NOT repeat completed tool calls or thoughts—write the complete final response to the user now.]"
        }
        val resumeUserNode = MessageNode(
            id = "node-resume-user-$passTag",
            role = MessageRole.USER,
            variants = listOf(MessageVariant(id = "var-resume-user-$passTag", content = resumePrompt))
        )
        (baseHistoryNodes + partialAssistantNode + resumeUserNode) to ""
    } else {
        baseHistoryNodes to continuePrefix
    }
}

private fun ChatSession.triggeredKeywordFragments(recentTurns: List<Pair<MessageRole, String>>): List<ContextFragment> {
    val cardEmbedded = activeCard?.embeddedFragments
    if (fragments.isEmpty() && cardEmbedded.isNullOrEmpty()) return emptyList()
    val windowStart = (recentTurns.size - 8).coerceAtLeast(0)
    fun matchesWindow(frag: ContextFragment): Boolean {
        if (frag.mode != FragmentActivationMode.KEYWORD) return false
        for (rawKw in frag.keywords) {
            val kw = rawKw.trim()
            if (kw.isEmpty()) continue
            for (idx in windowStart until recentTurns.size) {
                if (recentTurns[idx].second.contains(kw, ignoreCase = true)) return true
            }
            if ('\n' in kw) {
                val joined = recentTurns.subList(windowStart, recentTurns.size).joinToString("\n") { it.second }
                if (joined.contains(kw, ignoreCase = true)) return true
            }
        }
        return false
    }
    val out = mutableListOf<ContextFragment>()
    for (frag in fragments) {
        if (matchesWindow(frag)) out.add(frag)
    }
    if (cardEmbedded != null) {
        for (frag in cardEmbedded) {
            if (matchesWindow(frag)) out.add(frag)
        }
    }
    return out
}

/**
 * Condenses the oldest 50% of active unmuted conversation turns into a compact Continuity Digest
 * while keeping the newest turns in full verbatim fidelity.
 */
internal fun AiProviderEngine.compactHistoryNodesOnOverflow(nodes: List<MessageNode>): List<MessageNode> {
    val activeIndices = nodes.mapIndexedNotNull { idx, n -> if (n.includedInContext) idx else null }
    if (activeIndices.size <= 4) return nodes
    val compactCount = (activeIndices.size / 2).coerceAtLeast(2)
    val indicesToCompact = activeIndices.take(compactCount).toSet()
    val digestLines = mutableListOf<String>()
    for (idx in indicesToCompact) {
        val n = nodes[idx]
        val speaker = if (n.role == MessageRole.ASSISTANT) "Assistant" else "User"
        val snippet = collapseWhitespaceBounded(stripInlineErrorNotices(n.activeVariant.content), 260)
        if (snippet.isNotEmpty()) {
            digestLines.add("- $speaker: $snippet")
        }
    }
    val digestText = "[Auto-Compacted Earlier Conversation Continuity Digest (${indicesToCompact.size} turns condensed):\n" +
        digestLines.take(24).joinToString("\n") + "]"
    val digestNode = MessageNode(
        id = "node-auto-digest-${PlatformRuntime.currentTimeMillis()}",
        role = MessageRole.USER,
        variants = listOf(MessageVariant(id = "var-auto-digest", content = digestText))
    )
    val remaining = nodes.mapIndexed { idx, n ->
        if (idx in indicesToCompact) n.copy(includedInContext = false) else n
    }
    return listOf(digestNode) + remaining
}

internal fun AiProviderEngine.extractGoalObjectiveFromHistory(historyNodes: List<MessageNode>): String {
    val lastUserText = historyNodes.lastOrNull { it.role == MessageRole.USER }?.activeVariant?.content?.trim().orEmpty()
    if (lastUserText.startsWith("/goal", ignoreCase = true)) {
        return lastUserText.substringAfter("/goal").trim().ifBlank { "Complete the user's requested objective thoroughly." }
    }
    if (lastUserText.startsWith("[Autonomous Goal Mode:", ignoreCase = true)) {
        return lastUserText.substringAfter("[Autonomous Goal Mode:").substringBefore("]").trim()
            .ifBlank { lastUserText }
    }
    return ""
}

internal fun AiProviderEngine.compactHistoricalToolResultForWire(
    rawResult: String,
    toolName: String,
    isOlderAssistantTurn: Boolean,
    callIndexInTurn: Int,
    totalCallsInTurn: Int
): String {
    if (isOlderAssistantTurn) {
        if (rawResult.length <= 2_000) return rawResult
        return WorkspaceHarness.compactStaleToolOutputText(
            stepIndex = callIndexInTurn,
            totalSteps = totalCallsInTurn.coerceAtLeast(callIndexInTurn + 1),
            toolName = toolName,
            rawOutput = rawResult,
            keepRecentCount = 0,
            minCharsToCompact = 2_000
        )
    }
    // Quantize intra-turn compaction via compactStaleToolOutputText (COMPACTION_BATCH = 16) so prompt-cache prefixes never slide step-by-step.
    return WorkspaceHarness.compactStaleToolOutputText(
        stepIndex = callIndexInTurn,
        totalSteps = totalCallsInTurn,
        toolName = toolName,
        rawOutput = rawResult,
        keepRecentCount = 8,
        minCharsToCompact = 2_400
    )
}

internal fun AiProviderEngine.buildFoldedToolHistoryText(
    toolCalls: List<ToolCallRecord>,
    thinkingSummary: String,
    cleanText: String
): String = buildString {
    if (thinkingSummary.isNotBlank() && !cleanText.contains(REASONING_HANDOFF_MARKER)) {
        appendLine("[Completed reasoning: ${thinkingSummary.take(600).trim()}]")
    }
    if (toolCalls.isNotEmpty()) {
        appendLine("[Completed ${toolCalls.size} workspace tool call(s):")
        for (tc in toolCalls.takeLast(12)) {
            val snippet = collapseWhitespaceBounded(tc.resultText, 180)
            appendLine("- ${tc.name}(${tc.argumentsJson.take(80)}) -> $snippet")
        }
        appendLine("]")
    }
    if (cleanText.isNotBlank()) {
        if (isNotEmpty()) appendLine()
        append(cleanText)
    } else if (isEmpty()) {
        append("...")
    }
}.trim()

// ============================================================================================
// Shared helpers: compiled system prompt builder and cache-friendly author's note placement.
// ============================================================================================
private fun ChatSession.expandMacros(text: String): String {
    val charName = activeCard?.name?.ifBlank { "Character" } ?: "Character"
    return text.replace("{{char}}", charName, ignoreCase = true)
        .replace("{{user}}", userName, ignoreCase = true)
}

internal fun AiProviderEngine.buildCompiledSystemPrompt(
    session: ChatSession,
    activeGoalState: GoalTrackerState? = null,
    appStorageRoot: String = ""
): String {
    val sb = StringBuilder()
    fun expandMacros(text: String): String = session.expandMacros(text)

    if (session.systemPrompt.isNotBlank()) {
        sb.appendLine(expandMacros(session.systemPrompt.trim()))
        sb.appendLine()
    }
    session.activeCard?.let { card ->
        val compiledCard = card.buildCompiledInstructions(session.userName)
        if (compiledCard.isNotBlank()) {
            sb.appendLine(compiledCard)
            sb.appendLine()
        }
    }

    // Composable Context Fragments & Card Lorebook
    // Keyword matches change from turn to turn, so they join the late note; only constants stay up here.
    val allFragments = session.fragments + session.activeCard?.embeddedFragments.orEmpty()
    if (allFragments.isNotEmpty()) {
        val alwaysOnFragments = allFragments.filter { frag ->
            frag.mode == FragmentActivationMode.PINNED || (frag.mode == FragmentActivationMode.KEYWORD && frag.keywords.isEmpty())
        }
        if (alwaysOnFragments.isNotEmpty()) {
            sb.appendLine("### Context Fragments & World Knowledge")
            for (frag in alwaysOnFragments) {
                sb.appendLine("#### ${frag.title}")
                sb.appendLine(expandMacros(frag.content.trim()))
                sb.appendLine()
            }
        }
    }

    // Rolling Story Summary / Continuity Memory
    if (session.storySummary.isNotBlank()) {
        sb.appendLine("### Story Summary & Continuity So Far")
        sb.appendLine(expandMacros(session.storySummary.trim()))
        sb.appendLine()
    }

    val workspacePolicy = AgentPermissionResolver.resolve(session)
    val anyAgentToolsActive = workspacePolicy.toolsEnabled &&
        (
            workspacePolicy.hasWorkspace ||
                workspacePolicy.webAccess ||
                workspacePolicy.computerAccess ||
                activeGoalState != null
            )
    if (workspacePolicy.hasWorkspace && workspacePolicy.allowsTool("fs_read")) {
        sb.appendLine("### Local Workspace Agent Harness")
        sb.appendLine("Use only the tools advertised for the mounted directory (`${workspacePolicy.workspacePath}`). Inspect files before editing, keep edits surgical, and keep the project tree clean: `todo_write` updates this chat session's TODO checklist (never project files), and `agent_memory` reads or updates Parchment's global `memory/AGENT.md` when needed or requested by the user. Verify before `goal_complete`. An approval does not expand permissions; denied tools remain disabled.")
        if (workspacePolicy.workspaceMode == io.parchment.ai.model.WorkspaceMode.READ_ONLY) {
            sb.appendLine("Workspace is currently in Read-Only (/plan) mode: inspect files and propose a clear plan without modifying workspace files.")
        }
        val discoveredContext = WorkspaceHarness.discoverWorkspaceContextFiles(workspacePolicy.workspacePath)
        if (discoveredContext.isNotBlank()) {
            sb.appendLine()
            sb.appendLine(discoveredContext)
        }
    }
    val deferredMcpCatalog = WorkspaceHarness.deferredMcpToolsCatalogPrompt(workspacePolicy)
    if (deferredMcpCatalog.isNotBlank()) {
        sb.appendLine()
        sb.appendLine(deferredMcpCatalog)
    }
    if (anyAgentToolsActive && workspacePolicy.allowsTool("agent_memory")) {
        val globalMemory = WorkspaceHarness.discoverGlobalAgentMemory(appStorageRoot)
        if (globalMemory.isNotBlank()) {
            sb.appendLine()
            sb.appendLine(globalMemory)
        }
    }

    if (activeGoalState != null) {
        sb.appendLine()
        sb.appendLine("### Autonomous Goal & Verification Mode")
        sb.appendLine("Active Goal Objective: ${activeGoalState.goalObjective}")
        sb.appendLine("1. Plan: Immediately call `todo_write` to break the objective into concrete, verifiable steps (mark step 1 `in_progress`).")
        sb.appendLine("2. Execute & Verify: Complete each step thoroughly. After verifying a step, call `todo_write` to mark it `completed` and set the next step `in_progress`.")
        sb.appendLine("3. Complete: Do NOT stop early while steps remain `pending` or `in_progress`. Only when 100% of acceptance criteria are verified, call `goal_complete`.")
    }
    return sb.toString().trim()
}

/**
 * Injects the session's Author's Note / Post-History Instructions at `session.authorsNoteDepth`
 * turns from the bottom of the conversation history so the top prefix remains 100% static
 * for maximum Prompt Cache hit rates.
 */
internal fun AiProviderEngine.injectAuthorsNoteIntoHistory(
    nodes: List<MessageNode>,
    session: ChatSession
): List<Pair<MessageRole, String>> {
    val baseList = nodes.map { node ->
        val v = node.activeVariant
        val baseContent = if (node.role == MessageRole.ASSISTANT) {
            stripInlineErrorNotices(v.content)
        } else {
            v.content
        }
        val textAttachmentsBlock = v.attachments.filter { it.textContent.isNotBlank() }
            .joinToString("\n\n") { att ->
                "[Attached File: ${att.fileName}]\n```\n${att.textContent}\n```"
            }
        val combinedText = if (textAttachmentsBlock.isNotBlank()) {
            if (baseContent.isNotBlank()) "$baseContent\n\n$textAttachmentsBlock" else textAttachmentsBlock
        } else {
            baseContent
        }
        node.role to combinedText
    }.toMutableList()
    val combinedNote = buildString {
        val cardPost = session.activeCard?.postHistoryInstructions?.trim().orEmpty()
        if (cardPost.isNotEmpty()) appendLine(session.expandMacros(cardPost))
        if (session.authorsNote.isNotBlank()) appendLine(session.expandMacros(session.authorsNote.trim()))
    }.trim()

    // Matches what is being sent: after a rewind or regenerate the saved chat holds later messages too.
    val triggeredKnowledge = session.triggeredKeywordFragments(baseList)
        .joinToString("\n\n") { "#### ${it.title}\n${session.expandMacros(it.content.trim())}" }
    if ((combinedNote.isBlank() && triggeredKnowledge.isBlank()) || baseList.isEmpty()) return baseList

    val depth = session.authorsNoteDepth.coerceIn(0, baseList.size)
    val targetIdx = (baseList.size - 1 - depth).coerceIn(0, baseList.size - 1)
    val (targetRole, targetContent) = baseList[targetIdx]
    val note = buildString {
        append(targetContent)
        if (triggeredKnowledge.isNotBlank()) append("\n\n[Relevant World Knowledge:\n$triggeredKnowledge]")
        if (combinedNote.isNotBlank()) append("\n\n[Author's Note / Scene Direction: $combinedNote]")
    }
    baseList[targetIdx] = targetRole to note
    return baseList
}
