package io.parchment.ai.protocol

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.FragmentActivationMode
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.AgentPermissionResolver

data class ContextCompactionPlan(
    val mutedNodeIds: Set<String>,
    val compactedHistoryNodes: List<MessageNode>,
    val compactedLiveTools: List<ToolCallRecord>,
    val compactedLiveThinking: String,
    val transcriptToSummarize: String,
    val fallbackSummary: String,
    val didCompactAnything: Boolean
)

/**
 * Unified `o200k_base` Tokenizer & Context Window Breakdown Engine.
 *
 * Implements OpenAI's official `o200k_base` pre-tokenization regex pattern (`tiktoken_ext/openai_public.py`)
 * combined with UTF-8 subword BPE chunking, so the context ring, context breakdown and message token counts agree.
 */
    @androidx.compose.runtime.Immutable
    data class ContextWindowBreakdown(
    val systemAndPersonaTokens: Int,
    val lorebookAndMemoryTokens: Int,
    val workspaceAndToolsTokens: Int,
    val uploadedFilesTokens: Int,
    val uploadedFilesCount: Int,
    val chatHistoryTokens: Int,
    val activeTurnsCount: Int,
    val mutedTurnsCount: Int,
    val composerDraftTokens: Int,
    val contextLimitTokens: Int
) {
    val totalUsedTokens: Int
        get() = systemAndPersonaTokens +
            lorebookAndMemoryTokens +
            workspaceAndToolsTokens +
            uploadedFilesTokens +
            chatHistoryTokens +
            composerDraftTokens

    val freeSpaceTokens: Int
        get() = if (contextLimitTokens > 0) (contextLimitTokens - totalUsedTokens).coerceAtLeast(0) else 0
}

object O200kTokenizer {

    /**
     * Official OpenAI `o200k_base` pre-tokenization regex pattern:
     * 1. Mixed/lowercase Unicode words with optional English contractions ('s, 't, 're, 've, 'm, 'll, 'd)
     * 2. Uppercase/Titlecase Unicode words with optional contractions
     * 3. 1-to-3 digit numeric chunks (`\p{N}{1,3}`)
     * 4. Punctuation / operator clusters (` ?[^\s\p{L}\p{N}]+[\r\n/]*`)
     * 5. Line breaks and whitespace runs
     */
    private val O200K_PAT_REGEX by lazy(LazyThreadSafetyMode.PUBLICATION) {
        Regex(
            pattern = listOf(
                """[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]*[\p{Ll}\p{Lm}\p{Lo}\p{M}]+(?:'s|'t|'re|'ve|'m|'ll|'d|'S|'T|'RE|'VE|'M|'LL|'D)?""",
                """[^\r\n\p{L}\p{N}]?[\p{Lu}\p{Lt}\p{Lm}\p{Lo}\p{M}]+[\p{Ll}\p{Lm}\p{Lo}\p{M}]*(?:'s|'t|'re|'ve|'m|'ll|'d|'S|'T|'RE|'VE|'M|'LL|'D)?""",
                """\p{N}{1,3}""",
                """ ?[^\s\p{L}\p{N}]+[\r\n/]*""",
                """\s*[\r\n]+""",
                """\s+(?!\S)""",
                """\s+"""
            ).joinToString("|")
        )
    }

    private val WHITESPACE_RUN_REGEX = Regex("\\s+")

    private val tokenCache = object : LinkedHashMap<Long, Int>(256, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Long, Int>?): Boolean = size > 2048
    }

    private data class CompletedToolTokenKey(
        val id: String,
        val nameHash: Long,
        val argsHash: Long,
        val resultHash: Long
    )

    private data class CompletedVariantTokenKey(
        val id: String,
        val contentHash: Long,
        val thinkingHash: Long,
        val attachmentsSignature: Long,
        val toolsSignature: Long
    )

    internal data class VariantTokenBreakdown(
        val proseTokens: Int,
        val attachmentsTokens: Int,
        val thinkingTokens: Int,
        val toolCallsTokens: Int
    )

    private val toolCallTokenCache = object : LinkedHashMap<CompletedToolTokenKey, Int>(256, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<CompletedToolTokenKey, Int>?): Boolean = size > 512
    }

    private val variantTokenCache = object : LinkedHashMap<CompletedVariantTokenKey, VariantTokenBreakdown>(256, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<CompletedVariantTokenKey, VariantTokenBreakdown>?): Boolean = size > 512
    }

    private fun textFingerprint64(text: String): Long {
        if (text.isEmpty()) return 0L
        val len = text.length
        val edge = (text[0].code.toLong() shl 24) xor
            (text[len ushr 1].code.toLong() shl 12) xor
            text[len - 1].code.toLong()
        return (text.hashCode().toLong() shl 32) xor ((len.toLong() shl 16) xor edge)
    }

    private fun attachmentsFingerprint64(attachments: List<PromptAttachment>): Long {
        if (attachments.isEmpty()) return 0L
        var acc = attachments.size.toLong() * 0x9E3779B97F4A7C15uL.toLong()
        for (att in attachments) {
            acc = acc * 31L + textFingerprint64(att.id)
            acc = acc * 31L + textFingerprint64(att.fileName)
            acc = acc * 31L + textFingerprint64(att.mimeType)
            acc = acc * 31L + textFingerprint64(att.textContent)
        }
        return acc
    }

    private fun toolCallsFingerprint64(toolCalls: List<ToolCallRecord>): Long {
        if (toolCalls.isEmpty()) return 0L
        var acc = toolCalls.size.toLong() * 0x517CC1B727220A95L
        for (tc in toolCalls) {
            acc = acc * 31L + textFingerprint64(tc.id)
            acc = acc * 31L + textFingerprint64(tc.name)
            acc = acc * 31L + textFingerprint64(tc.argumentsJson)
            acc = acc * 31L + textFingerprint64(tc.resultText)
        }
        return acc
    }

    // Ensure warmUp() pre-compiles O200kTokenizer regex off the UI thread during startup
    fun warmUp() {
        countTokens("Warm up o200k_base regex and BPE subword counter.")
    }

    fun clearCaches() {
        synchronized(tokenCache) { tokenCache.clear() }
        synchronized(toolCallTokenCache) { toolCallTokenCache.clear() }
        synchronized(variantTokenCache) { variantTokenCache.clear() }
    }

    /**
     * Constant-time token estimate for partial streaming buffers, so frames
     * never run regexes on transient substrings or evict completed turns from `tokenCache`.
     */
    fun estimateTokensFast(text: String): Int {
        if (text.isEmpty()) return 0
        return ((text.length + 3) / 4).coerceAtLeast(1)
    }

    /**
     * Counts `o200k_base` tokens for a text string using the official `o200k_base` pre-tokenization
     * regex and UTF-8 subword BPE splitting rules.
     */
    fun countTokens(text: String, cacheResult: Boolean = true): Int {
        if (text.isEmpty()) return 0
        val cacheKey = textFingerprint64(text)
        synchronized(tokenCache) {
            tokenCache[cacheKey]?.let { return it }
        }

        // Fast sampling path for huge tool/file payloads (> 24,000 chars) to prevent UI/regex stalls
        if (text.length > 24_000) {
            val sampleHead = text.substring(0, 6_000)
            val sampleTail = text.substring(text.length - 6_000)
            var sampleTokens = 0
            for (match in O200K_PAT_REGEX.findAll(sampleHead)) {
                if (match.value.isNotEmpty()) sampleTokens += estimateBpeSubwordsForPiece(match.value)
            }
            for (match in O200K_PAT_REGEX.findAll(sampleTail)) {
                if (match.value.isNotEmpty()) sampleTokens += estimateBpeSubwordsForPiece(match.value)
            }
            val scaled = ((sampleTokens.toDouble() * text.length.toDouble()) / 12_000.0).toInt().coerceAtLeast(1)
            if (cacheResult) {
                synchronized(tokenCache) {
                    tokenCache[cacheKey] = scaled
                }
            }
            return scaled
        }

        var total = 0
        for (match in O200K_PAT_REGEX.findAll(text)) {
            val piece = match.value
            if (piece.isEmpty()) continue
            total += estimateBpeSubwordsForPiece(piece)
        }
        val finalCount = total.coerceAtLeast(1)
        if (cacheResult) {
            synchronized(tokenCache) {
                tokenCache[cacheKey] = finalCount
            }
        }
        return finalCount
    }

    private fun estimateBpeSubwordsForPiece(piece: String): Int {
        val trimmed = piece.trimStart(' ')
        if (trimmed.isEmpty()) {
            // Pure whitespace run: o200k_base merges up to 4 spaces or 2 newlines per token
            return ((piece.length + 3) / 4).coerceAtLeast(1)
        }

        val firstChar = trimmed[0]
        // 1-to-3 digit numbers are always 1 token in o200k_base
        if (firstChar.isDigit()) return 1

        // Check for non-ASCII / CJK / emoji characters
        var nonAsciiChars = 0
        for (i in 0 until trimmed.length) {
            if (trimmed[i].code > 127) nonAsciiChars++
        }
        if (nonAsciiChars > 0) {
            // In o200k_base's 200k multilingual vocabulary, common CJK characters are 1 token (~1.25 tokens/char on rare hanzi)
            val asciiPart = trimmed.length - nonAsciiChars
            val asciiToks = if (asciiPart == 0) 0 else ((asciiPart + 4) / 5).coerceAtLeast(1)
            val unicodeToks = ((nonAsciiChars * 5 + 3) / 4).coerceAtLeast(1)
            return asciiToks + unicodeToks
        }

        // ASCII word or punctuation cluster:
        // In o200k_base, words <= 7 chars are almost always 1 token; longer words split into ~4.5-char subwords
        val len = trimmed.length
        if (!firstChar.isLetter()) {
            // Punctuation/operator run (e.g. `});`, `->`, `===`, `/**`): ~1 token per 2.5 chars
            return ((len + 2) / 3).coerceAtLeast(1)
        }
        return when {
            len <= 7 -> 1
            len <= 12 -> 2
            else -> ((len + 3) / 5).coerceAtLeast(2)
        }
    }

    /**
     * Counts `o200k_base` tokens for a single `PromptAttachment` (either inline text/code content
     * or multimodal vision/document tile budget).
     */
    fun countAttachmentTokens(att: PromptAttachment): Int {
        if (att.textContent.isNotBlank()) {
            return countTokens(att.textContent) + 8 // includes `[Attached File: ...]` header overhead
        }
        val mime = att.mimeType.lowercase()
        val lower = att.fileName.lowercase()
        return when {
            mime == "application/pdf" || lower.endsWith(".pdf") -> 1024
            mime.startsWith("video/") || lower.endsWith(".mp4") || lower.endsWith(".webm") -> 1536
            mime.startsWith("audio/") || lower.endsWith(".mp3") || lower.endsWith(".wav") -> 512
            mime.startsWith("image/") || lower.endsWith(".png") || lower.endsWith(".jpg") ||
                lower.endsWith(".jpeg") || lower.endsWith(".webp") || lower.endsWith(".gif") -> 258
            else -> 128
        }
    }

    fun countToolCallTokens(tc: ToolCallRecord, isLiveOverride: Boolean = false): Int {
        if (isLiveOverride || tc.isRunning) {
            return 6 + countTokens(tc.name) +
                estimateTokensFast(tc.argumentsJson) +
                estimateTokensFast(tc.resultText)
        }
        val key = CompletedToolTokenKey(
            id = tc.id,
            nameHash = textFingerprint64(tc.name),
            argsHash = textFingerprint64(tc.argumentsJson),
            resultHash = textFingerprint64(tc.resultText)
        )
        synchronized(toolCallTokenCache) {
            toolCallTokenCache[key]?.let { return it }
        }
        val computed = 6 + countTokens(tc.name) + countTokens(tc.argumentsJson) + countTokens(tc.resultText)
        synchronized(toolCallTokenCache) {
            toolCallTokenCache[key] = computed
        }
        return computed
    }

    /** Cached per completed variant (bounded LRU); call off the main thread for long content. */
    internal fun completedVariantTokenBreakdown(variant: MessageVariant): VariantTokenBreakdown {
        val key = CompletedVariantTokenKey(
            id = variant.id,
            contentHash = textFingerprint64(variant.content),
            thinkingHash = textFingerprint64(variant.thinkingSummary),
            attachmentsSignature = attachmentsFingerprint64(variant.attachments),
            toolsSignature = toolCallsFingerprint64(variant.toolCalls)
        )
        synchronized(variantTokenCache) {
            variantTokenCache[key]?.let { return it }
        }
        val proseTokens = if (variant.content.isNotBlank()) countTokens(variant.content) else 0
        var attachmentsTokens = 0
        for (att in variant.attachments) {
            attachmentsTokens += countAttachmentTokens(att)
        }
        val thinkingTokens = if (variant.thinkingSummary.isNotBlank()) countTokens(variant.thinkingSummary) else 0
        var toolCallsTokens = 0
        for (tc in variant.toolCalls) {
            toolCallsTokens += countToolCallTokens(tc)
        }
        return VariantTokenBreakdown(
            proseTokens = proseTokens,
            attachmentsTokens = attachmentsTokens,
            thinkingTokens = thinkingTokens,
            toolCallsTokens = toolCallsTokens
        ).also { breakdown ->
            synchronized(variantTokenCache) {
                variantTokenCache[key] = breakdown
            }
        }
    }

    /**
     * Counts the exact `o200k_base` tokens for a single message turn (`MessageVariant`),
     * including its prose, attachments, tool calls, and reasoning summary.
     */
    fun countMessageVariantTokens(
        role: MessageRole,
        variant: MessageVariant,
        includeReasoningAndTools: Boolean = true
    ): Int {
        val isLive = variant.isStreaming
        if (!isLive && variant.toolCalls.none { it.isRunning }) {
            val b = completedVariantTokenBreakdown(variant)
            return 4 + b.proseTokens + b.attachmentsTokens +
                if (role == MessageRole.ASSISTANT && includeReasoningAndTools) {
                    b.thinkingTokens + b.toolCallsTokens
                } else 0
        }
        var total = 4 // ChatML / turn framing overhead (`<|start|>role\n...<|end|>`)
        if (variant.content.isNotBlank()) {
            total += estimateTokensFast(variant.content)
        }
        for (att in variant.attachments) {
            total += countAttachmentTokens(att)
        }
        if (role == MessageRole.ASSISTANT && includeReasoningAndTools) {
            if (variant.thinkingSummary.isNotBlank()) {
                total += estimateTokensFast(variant.thinkingSummary)
            }
            for (tc in variant.toolCalls) {
                total += countToolCallTokens(tc, isLiveOverride = true)
            }
        }
        return total
    }

    private data class TurnTokenContribution(
        val proseAndToolsTokens: Int,
        val uploadedFilesCount: Int,
        val uploadedFilesTokens: Int
    )

    private fun countSingleTurnTokens(
        node: MessageNode,
        isLastInEffective: Boolean
    ): TurnTokenContribution {
        val v = node.activeVariant
        val isLive = v.isStreaming
        if (!isLive && v.toolCalls.none { it.isRunning }) {
            val b = completedVariantTokenBreakdown(v)
            var turnProseAndTools = 4 + b.proseTokens
            if (node.role == MessageRole.ASSISTANT) {
                turnProseAndTools += b.toolCallsTokens
                if (v.content.isBlank() || isLastInEffective) {
                    turnProseAndTools += b.thinkingTokens
                }
            }
            return TurnTokenContribution(turnProseAndTools, v.attachments.size, b.attachmentsTokens)
        }
        var turnProseAndTools = 4
        if (v.content.isNotBlank()) {
            turnProseAndTools += if (isLive) estimateTokensFast(v.content) else countTokens(v.content)
        }
        if (node.role == MessageRole.ASSISTANT) {
            for (tc in v.toolCalls) {
                turnProseAndTools += countToolCallTokens(tc, isLiveOverride = isLive)
            }
            if ((v.content.isBlank() || isLive || isLastInEffective) && v.thinkingSummary.isNotBlank()) {
                turnProseAndTools += if (isLive) estimateTokensFast(v.thinkingSummary) else countTokens(v.thinkingSummary)
            }
        }
        var attachmentsTokens = 0
        for (att in v.attachments) {
            attachmentsTokens += countAttachmentTokens(att)
        }
        return TurnTokenContribution(turnProseAndTools, v.attachments.size, attachmentsTokens)
    }

    /**
     * Computes static context sections plus all history turns except the final live turn.
     * During streaming, the UI reuses this prefix across frames and applies only the active final turn.
     */
    fun computeBaseContextPrefixBreakdown(
        session: ChatSession,
        assistant: AssistantAgent,
        profile: ProviderProfile,
        goalModeActive: Boolean = false,
        globalWorldBook: List<io.parchment.ai.model.ContextFragment> = emptyList(),
        excludeLastFromKeywords: Boolean = true
    ): ContextWindowBreakdown {
        val activeModelId = session.modelOverride.ifBlank {
            assistant.defaultModelId.ifBlank { profile.defaultModel }
        }.trim()
        val contextLimit = profile.customModelSpecs[activeModelId]?.contextWindowTokens
            ?: profile.contextWindowTokens

        // 1. System & Persona Prompt (Assistant systemPrompt + Session systemPrompt + Card + Author's Note)
        val cardCompiled = session.activeCard?.buildCompiledInstructions(session.userName)
            ?: assistant.activeCard?.buildCompiledInstructions(session.userName).orEmpty()
        val effectiveAuthorsNote = session.authorsNote.ifBlank { assistant.authorsNote }
        val systemAndPersonaTokens = countTokens(assistant.systemPrompt) +
            countTokens(session.systemPrompt) +
            countTokens(cardCompiled) +
            countTokens(effectiveAuthorsNote)

        // 2. Lorebook & Story Continuity Memory (Global World Book + Assistant + Session + Card Fragments + Story Summary)
        val allFragments = (
            globalWorldBook +
                assistant.fragments +
                session.fragments +
                session.activeCard?.embeddedFragments.orEmpty() +
                assistant.activeCard?.embeddedFragments.orEmpty()
            ).distinctBy { it.id }
        val keywordMessages = if (excludeLastFromKeywords && session.messages.isNotEmpty()) {
            session.messages.dropLast(1).takeLast(8)
        } else {
            session.messages.takeLast(8)
        }
        val recentWindowText = keywordMessages.joinToString("\n") { it.activeVariant.content }
        val activeFragments = allFragments.filter { frag ->
            when (frag.mode) {
                FragmentActivationMode.PINNED -> true
                FragmentActivationMode.MUTED -> false
                FragmentActivationMode.KEYWORD -> {
                    frag.keywords.isEmpty() || frag.keywords.any { kw ->
                        kw.isNotBlank() && recentWindowText.contains(kw.trim(), ignoreCase = true)
                    }
                }
            }
        }
        val lorebookAndMemoryTokens = activeFragments.sumOf { countTokens(it.title) + countTokens(it.content) + 4 } +
            countTokens(session.storySummary)

        // 3. Workspace & Harness Tool Schemas
        val toolPolicy = AgentPermissionResolver.resolve(session, assistant, profile)
        val wsPath = toolPolicy.workspacePath
        val effectiveWsMode = toolPolicy.workspaceMode
        val activeTools = WorkspaceHarness.activeToolsForMode(
            mode = effectiveWsMode,
            workspacePath = wsPath,
            allowShell = toolPolicy.allowShell,
            goalModeActive = goalModeActive,
            enableWebTools = toolPolicy.webAccess,
            policy = toolPolicy
        )
        val toolSchemaTokens = activeTools.sumOf { t ->
            countTokens(t.name) + countTokens(t.description) + countTokens(t.parametersSchemaString) + 8
        }
        val wsContextTokens = if (toolPolicy.hasWorkspace && toolPolicy.allowsTool("fs_read")) {
            countTokens(WorkspaceHarness.discoverWorkspaceContextFiles(wsPath)) + 95
        } else 0
        val goalModeTokens = if (goalModeActive) 110 else 0
        val workspaceAndToolsTokens = toolSchemaTokens + wsContextTokens + goalModeTokens

        // 4 & 5. Active Unmuted Conversation Turns prior to the final turn (honoring assistant.contextMessageLimit)
        val effectiveMessages = if (assistant.contextMessageLimit > 0 && session.messages.size > assistant.contextMessageLimit) {
            session.messages.takeLast(assistant.contextMessageLimit)
        } else {
            session.messages
        }
        var uploadedFilesTokens = 0
        var uploadedFilesCount = 0
        var chatHistoryTokens = 0
        var activeTurnsCount = 0
        var mutedTurnsCount = (session.messages.size - effectiveMessages.size).coerceAtLeast(0)

        val prefixEndExclusive = (effectiveMessages.size - 1).coerceAtLeast(0)
        for (i in 0 until prefixEndExclusive) {
            val node = effectiveMessages[i]
            if (!node.includedInContext) {
                mutedTurnsCount++
                continue
            }
            activeTurnsCount++
            val contribution = countSingleTurnTokens(node, isLastInEffective = false)
            chatHistoryTokens += contribution.proseAndToolsTokens
            uploadedFilesCount += contribution.uploadedFilesCount
            uploadedFilesTokens += contribution.uploadedFilesTokens
        }

        return ContextWindowBreakdown(
            systemAndPersonaTokens = systemAndPersonaTokens,
            lorebookAndMemoryTokens = lorebookAndMemoryTokens,
            workspaceAndToolsTokens = workspaceAndToolsTokens,
            uploadedFilesTokens = uploadedFilesTokens,
            uploadedFilesCount = uploadedFilesCount,
            chatHistoryTokens = chatHistoryTokens,
            activeTurnsCount = activeTurnsCount,
            mutedTurnsCount = mutedTurnsCount,
            composerDraftTokens = 0,
            contextLimitTokens = contextLimit
        )
    }

    /**
     * A constant-time-per-message stand-in for [computeBaseContextPrefixBreakdown] (about four characters per token,
     * no regex, no disk), shown for the moment the exact count takes off the UI thread.
     */
    fun estimateBaseContextPrefixBreakdown(
        session: ChatSession,
        assistant: AssistantAgent,
        profile: ProviderProfile
    ): ContextWindowBreakdown {
        val activeModelId = session.modelOverride.ifBlank { assistant.defaultModelId.ifBlank { profile.defaultModel } }.trim()
        val contextLimit = profile.customModelSpecs[activeModelId]?.contextWindowTokens ?: profile.contextWindowTokens
        val effectiveMessages = if (assistant.contextMessageLimit > 0 && session.messages.size > assistant.contextMessageLimit) {
            session.messages.takeLast(assistant.contextMessageLimit)
        } else {
            session.messages
        }
        var chatHistoryTokens = 0
        var activeTurnsCount = 0
        var mutedTurnsCount = (session.messages.size - effectiveMessages.size).coerceAtLeast(0)
        for (i in 0 until (effectiveMessages.size - 1).coerceAtLeast(0)) {
            val node = effectiveMessages[i]
            if (!node.includedInContext) { mutedTurnsCount++; continue }
            activeTurnsCount++
            chatHistoryTokens += 4 + estimateTokensFast(node.activeVariant.content) + estimateTokensFast(node.activeVariant.thinkingSummary)
        }
        return ContextWindowBreakdown(
            systemAndPersonaTokens = estimateTokensFast(assistant.systemPrompt) + estimateTokensFast(session.systemPrompt),
            lorebookAndMemoryTokens = estimateTokensFast(session.storySummary),
            workspaceAndToolsTokens = 0,
            uploadedFilesTokens = 0,
            uploadedFilesCount = 0,
            chatHistoryTokens = chatHistoryTokens,
            activeTurnsCount = activeTurnsCount,
            mutedTurnsCount = mutedTurnsCount,
            composerDraftTokens = 0,
            contextLimitTokens = contextLimit
        )
    }

    /**
     * Adds the final turn (`effectiveMessages.lastOrNull()`) onto a cached [prefixBreakdown] in $O(1)$
     * while streaming, using [countSingleTurnTokens] consistently with full-history scans.
     */
    fun applyLastTurnToBreakdown(
        prefixBreakdown: ContextWindowBreakdown,
        lastNode: MessageNode?
    ): ContextWindowBreakdown {
        if (lastNode == null) return prefixBreakdown
        if (!lastNode.includedInContext) {
            return prefixBreakdown.copy(mutedTurnsCount = prefixBreakdown.mutedTurnsCount + 1)
        }
        val contribution = countSingleTurnTokens(lastNode, isLastInEffective = true)
        return prefixBreakdown.copy(
            uploadedFilesTokens = prefixBreakdown.uploadedFilesTokens + contribution.uploadedFilesTokens,
            uploadedFilesCount = prefixBreakdown.uploadedFilesCount + contribution.uploadedFilesCount,
            chatHistoryTokens = prefixBreakdown.chatHistoryTokens + contribution.proseAndToolsTokens,
            activeTurnsCount = prefixBreakdown.activeTurnsCount + 1
        )
    }

    /**
     * Computes the session/system/history portion of `ContextWindowBreakdown` (excluding the live Composer draft).
     * Kept separate so the UI can cache it: typing in the composer then never re-scans history or tool schemas.
     */
    fun computeBaseContextWindowBreakdown(
        session: ChatSession,
        assistant: AssistantAgent,
        profile: ProviderProfile,
        goalModeActive: Boolean = false,
        globalWorldBook: List<io.parchment.ai.model.ContextFragment> = emptyList()
    ): ContextWindowBreakdown {
        val prefix = computeBaseContextPrefixBreakdown(
            session = session,
            assistant = assistant,
            profile = profile,
            goalModeActive = goalModeActive,
            globalWorldBook = globalWorldBook,
            excludeLastFromKeywords = false
        )
        return applyLastTurnToBreakdown(prefix, session.messages.lastOrNull())
    }

    fun countAttachmentsTokens(pendingAttachments: List<PromptAttachment>): Int {
        var total = 0
        for (staged in pendingAttachments) {
            total += countAttachmentTokens(staged)
        }
        return total
    }

    /**
     * Combines a pre-computed `baseBreakdown` with the live Composer draft & staged attachments in $O(1)$.
     */
    fun applyComposerDraftToBreakdown(
        baseBreakdown: ContextWindowBreakdown,
        draftPrompt: String,
        draftPrefill: String,
        pendingAttachments: List<PromptAttachment>,
        estimateDraft: Boolean = false,
        precomputedAttachmentsTokens: Int? = null
    ): ContextWindowBreakdown {
        val draftTextTokens = if (estimateDraft) {
            estimateTokensFast(draftPrompt) + estimateTokensFast(draftPrefill)
        } else {
            countTokens(draftPrompt) + countTokens(draftPrefill)
        }
        val attachmentTokens = precomputedAttachmentsTokens ?: countAttachmentsTokens(pendingAttachments)
        return baseBreakdown.copy(
            uploadedFilesCount = baseBreakdown.uploadedFilesCount + pendingAttachments.size,
            composerDraftTokens = draftTextTokens + attachmentTokens
        )
    }

    /**
     * Computes the itemized `ContextWindowBreakdown` across all 7 categories, so the context ring, the
     * breakdown popover and the per-turn token counts always agree.
     */
    fun computeContextWindowBreakdown(
        session: ChatSession,
        assistant: AssistantAgent,
        profile: ProviderProfile,
        draftPrompt: String,
        draftPrefill: String,
        pendingAttachments: List<PromptAttachment>,
        goalModeActive: Boolean = false,
        globalWorldBook: List<io.parchment.ai.model.ContextFragment> = emptyList()
    ): ContextWindowBreakdown {
        val base = computeBaseContextWindowBreakdown(
            session = session,
            assistant = assistant,
            profile = profile,
            goalModeActive = goalModeActive,
            globalWorldBook = globalWorldBook
        )
        return applyComposerDraftToBreakdown(base, draftPrompt, draftPrefill, pendingAttachments)
    }

    const val COMPACTED_TOOL_MARKER = "[Compacted into Continuity Memory]"

    fun isToolResultCompacted(tc: ToolCallRecord): Boolean =
        tc.resultText.contains(COMPACTED_TOOL_MARKER)

    fun compactToolCallRecord(tc: ToolCallRecord, maxKeepChars: Int = 220): ToolCallRecord {
        val cleanSig = if (tc.thoughtSignature.length > 2048) {
            sanitizeGeminiThoughtSignature(tc.thoughtSignature, forFunctionCall = true)
        } else tc.thoughtSignature
        if (tc.isRunning || (tc.resultText.length <= maxKeepChars + 60 && cleanSig == tc.thoughtSignature) || isToolResultCompacted(tc)) {
            return if (cleanSig != tc.thoughtSignature) tc.copy(thoughtSignature = cleanSig) else tc
        }
        val head = tc.resultText.trim().take(maxKeepChars).trimEnd()
        return tc.copy(
            resultText = "$head\n… $COMPACTED_TOOL_MARKER",
            thoughtSignature = cleanSig,
            precedingThinking = if (tc.precedingThinking.length > 240) {
                tc.precedingThinking.take(240).trimEnd() + "…"
            } else tc.precedingThinking
        )
    }

    /**
     * Checks whether a session or in-flight generation has older unmuted turns, heavy completed
     * tool call outputs, or long reasoning traces that can be compacted into `storySummary`.
     */
    fun hasCompactableContext(
        historyNodes: List<MessageNode>,
        keepRecentTurnsCount: Int = 2,
        liveTargetTools: List<ToolCallRecord> = emptyList(),
        liveTargetThinking: String = ""
    ): Boolean {
        val unmuted = historyNodes.filter { it.includedInContext }
        if (unmuted.size > keepRecentTurnsCount) return true
        val hasUncompactedNodeTools = unmuted.any { node ->
            val v = node.activeVariant
            if (v.thinkingSignature.length > 4096) return@any true
            val completedTools = v.toolCalls.filter { !it.isRunning }
            completedTools.any { it.thoughtSignature.length > 4096 } ||
                (completedTools.size > 2 && completedTools.dropLast(2).any { tc ->
                    tc.resultText.length > 280 && !isToolResultCompacted(tc)
                }) || completedTools.any { tc -> tc.resultText.length > 1800 && !isToolResultCompacted(tc) }
        }
        if (hasUncompactedNodeTools) return true
        val completedLiveTools = liveTargetTools.filter { !it.isRunning }
        val hasUncompactedLiveTools = (completedLiveTools.size > 2 && completedLiveTools.dropLast(2).any { tc ->
            tc.resultText.length > 280 && !isToolResultCompacted(tc)
        }) || completedLiveTools.any { tc -> tc.resultText.length > 1800 && !isToolResultCompacted(tc) }
        if (hasUncompactedLiveTools) return true
        if (liveTargetThinking.length > 3600) return true
        return false
    }

    /**
     * Builds a comprehensive `ContextCompactionPlan` that:
     * 1. Mutes older unmuted turns beyond `keepRecentTurnsCount`
     * 2. Compacts older or oversized tool call outputs inside kept unmuted turns & `liveTargetTools`
     * 3. Trims oversized `liveTargetThinking` while folding its contents into the summary transcript
     * 4. Clears `providerInteractionId` on compacted variants so resumed requests use the compacted context
     */
    fun prepareCompactionPlan(
        existingStorySummary: String,
        historyNodes: List<MessageNode>,
        keepRecentTurnsCount: Int = 2,
        liveTargetTools: List<ToolCallRecord> = emptyList(),
        liveTargetThinking: String = ""
    ): ContextCompactionPlan {
        val activeIndices = historyNodes.mapIndexedNotNull { idx, n -> if (n.includedInContext) idx else null }
        val safeKeepCount = keepRecentTurnsCount.coerceAtLeast(1)
        val indicesToMute = if (activeIndices.size > safeKeepCount) {
            activeIndices.dropLast(safeKeepCount).toSet()
        } else {
            emptySet()
        }
        val mutedNodeIds = indicesToMute.map { historyNodes[it].id }.toSet()
        val turnsToSummarize = indicesToMute.map { historyNodes[it] }

        val compactedToolNotes = mutableListOf<String>()
        var anyToolOrThinkingCompacted = false

        fun compactToolListAndCollectNotes(tools: List<ToolCallRecord>): List<ToolCallRecord> {
            if (tools.isEmpty()) return tools
            val lastIndex = tools.lastIndex
            return tools.mapIndexed { idx, tc ->
                val isRecentTwo = idx >= lastIndex - 1
                val hasBloatedSig = tc.thoughtSignature.length > 2048
                val shouldCompact = !tc.isRunning && (!isToolResultCompacted(tc) && (
                    (!isRecentTwo && tc.resultText.length > 280) ||
                        (isRecentTwo && tc.resultText.length > 2400)
                    ) || hasBloatedSig)
                if (shouldCompact) {
                    anyToolOrThinkingCompacted = true
                    if (!isToolResultCompacted(tc) && tc.resultText.length > 280) {
                        val snippet = tc.resultText.replace(WHITESPACE_RUN_REGEX, " ").trim().take(260)
                        compactedToolNotes.add("${tc.name}(${tc.argumentsJson.take(120)}) -> $snippet")
                    }
                    compactToolCallRecord(tc, maxKeepChars = if (isRecentTwo) 600 else 220)
                } else {
                    tc
                }
            }
        }

        val compactedHistoryNodes = historyNodes.mapIndexed { idx, node ->
            if (idx in indicesToMute) {
                node.copy(includedInContext = false)
            } else if (node.includedInContext) {
                val activeVar = node.activeVariant
                val newTools = compactToolListAndCollectNotes(activeVar.toolCalls)
                val newThinking = if (activeVar.thinkingSummary.length > 3600) {
                    anyToolOrThinkingCompacted = true
                    "… [Earlier reasoning compacted into Continuity Memory]\n" + activeVar.thinkingSummary.takeLast(1800)
                } else activeVar.thinkingSummary
                val newSig = if (activeVar.thinkingSignature.length > 2048) {
                    val sanitized = sanitizeGeminiThoughtSignature(
                        activeVar.thinkingSignature,
                        forFunctionCall = false
                    )
                    if (sanitized != activeVar.thinkingSignature) {
                        anyToolOrThinkingCompacted = true
                    }
                    sanitized
                } else activeVar.thinkingSignature
                if (newTools != activeVar.toolCalls ||
                    newThinking != activeVar.thinkingSummary ||
                    newSig != activeVar.thinkingSignature ||
                    activeVar.providerInteractionId.isNotEmpty()
                ) {
                    node.copy(
                        variants = node.variants.mapIndexed { vIdx, v ->
                            if (vIdx == node.activeVariantIndex) {
                                v.copy(
                                    toolCalls = newTools,
                                    thinkingSummary = newThinking,
                                    thinkingSignature = newSig,
                                    providerInteractionId = ""
                                )
                            } else v
                        }
                    )
                } else node
            } else node
        }

        val compactedLiveTools = compactToolListAndCollectNotes(liveTargetTools)
        val compactedLiveThinking = if (liveTargetThinking.length > 3600) {
            anyToolOrThinkingCompacted = true
            "… [Earlier reasoning compacted into Continuity Memory]\n" + liveTargetThinking.takeLast(1800)
        } else {
            liveTargetThinking
        }

        val transcriptToSummarize = buildString {
            if (existingStorySummary.isNotBlank()) {
                appendLine("Existing Continuity Summary:")
                appendLine(existingStorySummary.trim())
                appendLine()
            }
            if (turnsToSummarize.isNotEmpty()) {
                appendLine("Older Conversation Turns to Fold into Continuity Summary:")
                for (turn in turnsToSummarize.takeLast(32)) {
                    val roleLabel = if (turn.role == MessageRole.ASSISTANT) "Assistant" else "User"
                    val v = turn.activeVariant
                    val txt = v.content.trim().take(1600)
                    if (txt.isNotEmpty()) {
                        appendLine("$roleLabel: $txt")
                    }
                    if (v.toolCalls.isNotEmpty()) {
                        val toolSummary = v.toolCalls.takeLast(8).joinToString("; ") { tc ->
                            "${tc.name}: ${tc.resultText.replace(WHITESPACE_RUN_REGEX, " ").trim().take(240)}"
                        }
                        appendLine("$roleLabel Tools: $toolSummary")
                    }
                }
                appendLine()
            }
            if (compactedToolNotes.isNotEmpty()) {
                appendLine("Completed Agent Tool Executions to Fold into Continuity Summary:")
                for (note in compactedToolNotes.takeLast(28)) {
                    appendLine("• $note")
                }
                appendLine()
            }
            if (liveTargetThinking.length > 500) {
                appendLine("Recent Agent Reasoning Context:")
                appendLine(liveTargetThinking.takeLast(2000).trim())
            }
        }.trim()

        val fallbackSummary = buildString {
            if (existingStorySummary.isNotBlank()) {
                appendLine(existingStorySummary.trim())
            }
            for (turn in turnsToSummarize.takeLast(14)) {
                val roleLabel = if (turn.role == MessageRole.ASSISTANT) "AI" else "User"
                val txt = turn.activeVariant.content.replace(WHITESPACE_RUN_REGEX, " ").trim().take(400)
                if (txt.isNotEmpty()) appendLine("• $roleLabel: $txt")
            }
            for (note in compactedToolNotes.takeLast(14)) {
                appendLine("• Tool: ${note.take(320)}")
            }
        }.trim()

        val didCompact = mutedNodeIds.isNotEmpty() || anyToolOrThinkingCompacted
        return ContextCompactionPlan(
            mutedNodeIds = mutedNodeIds,
            compactedHistoryNodes = compactedHistoryNodes,
            compactedLiveTools = compactedLiveTools,
            compactedLiveThinking = compactedLiveThinking,
            transcriptToSummarize = transcriptToSummarize,
            fallbackSummary = fallbackSummary,
            didCompactAnything = didCompact
        )
    }
}
