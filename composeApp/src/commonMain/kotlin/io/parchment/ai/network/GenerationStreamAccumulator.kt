package io.parchment.ai.network

import io.parchment.ai.harness.AgentToolRunContext
import io.parchment.ai.model.ApiUsageRecord
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.ProviderTokenUsage
import io.parchment.ai.model.TokenMetrics
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.WebSourceCitation
import io.parchment.ai.network.AiProviderEngine.OUTPUT_LIMIT_NOTICE
import io.parchment.ai.network.AiProviderEngine.RawWebCitation
import io.parchment.ai.network.AiProviderEngine.SpendLimitException
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.protocol.O200kTokenizer
import io.parchment.ai.runtime.RunActivity
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.flow.FlowCollector

// Mutable per-turn output/thinking/usage/tool accumulators, burst decode timer, and completion emitter.

internal data class PassProgressSnapshot(
    val contentChars: Int,
    val thinkingChars: Int,
    val signatureChars: Int,
    val toolCount: Int,
    val mediaCount: Int
)

internal class GenerationStreamAccumulator(
    private val engine: AiProviderEngine,
    private val collector: FlowCollector<StreamUpdate>,
    private val activity: RunActivity,
    private val profile: ProviderProfile,
    private val session: ChatSession,
    private val model: String,
    private val historyNodes: List<MessageNode>,
    cleanInitialPrefix: String,
    initialThinking: String,
    initialSignature: String,
    initialToolCalls: List<ToolCallRecord>,
    initialMediaItems: List<GeneratedMediaItem>,
    initialWebSources: List<WebSourceCitation>,
    private val spendLimitUsd: Double,
    private val spendBeforeRunUsd: Double,
    private val initialElapsedMs: Long,
    initialApiUsageRecords: List<ApiUsageRecord>,
    initialTokenMetrics: TokenMetrics,
    usageOwnerSessionId: String,
    resolvedUsageRunId: String,
    private val usagePurpose: String
) {
    private val startTime = activity.elapsedMs
    private var firstTokenTime = -1L

    private val contentBuilder = StringBuilder(cleanInitialPrefix)
    private val thinkingBuilder = StringBuilder(initialThinking)
    private val currentStepThinkingBuilder = StringBuilder()
    private val signatureBuilder = StringBuilder(initialSignature)
    private val executedTools = initialToolCalls.toMutableList()
    private val generatedMedia = initialMediaItems.toMutableList()
    private val rawWebCitations = initialWebSources.mapTo(mutableListOf()) { existingSrc ->
        RawWebCitation(
            url = existingSrc.url,
            title = existingSrc.title,
            snippet = existingSrc.snippet,
            groundedText = existingSrc.groundedLines.firstOrNull().orEmpty()
        )
    }

    private var lastInteractionId = ""
    private val usageLedger = ProviderUsageLedger(
        profile = profile,
        model = model,
        initialRecords = initialApiUsageRecords,
        initialMetrics = initialTokenMetrics,
        ownerSessionId = usageOwnerSessionId,
        ownerRunId = resolvedUsageRunId,
        purpose = usagePurpose
    )
    private var latestUsageMetrics = usageLedger.metrics()
    private val ledgerSpendAtStart = spendTowardLimit(usageLedger.snapshots())

    private var activeDecodeAccumulatedMs = 0L
    private var currentBurstStartMs = -1L
    private var currentBurstLastMs = -1L
    private var lastRawReq = ""
    private var lastRawResp = ""
    private var helperCount = 0
    private var reachedOutputLimit = false
    private var activeWireDialect = engine.resolveWireDialect(profile)

    fun markActiveWireDialect(wireDialect: String) {
        if (wireDialect.isNotBlank()) activeWireDialect = wireDialect
    }

    val hasNoFinalAnswer: Boolean
        get() = contentBuilder.toString().isBlank() && generatedMedia.isEmpty()

    val hasThoughtsOrTools: Boolean
        get() = thinkingBuilder.isNotBlank() || signatureBuilder.isNotBlank() || executedTools.isNotEmpty()

    val hasNoMediaOrTextYet: Boolean
        get() = generatedMedia.isEmpty() && contentBuilder.isEmpty()

    fun currentContent(): String = contentBuilder.toString()

    fun snapshotProgress(): PassProgressSnapshot = PassProgressSnapshot(
        contentChars = contentBuilder.length,
        thinkingChars = thinkingBuilder.length,
        signatureChars = signatureBuilder.length,
        toolCount = executedTools.size,
        mediaCount = generatedMedia.size
    )

    fun hasProducedSince(before: PassProgressSnapshot): Boolean {
        val producedNewContent = contentBuilder.length > before.contentChars
        val producedNewThoughts = thinkingBuilder.length > before.thinkingChars || signatureBuilder.length > before.signatureChars
        val producedMediaOrTools = generatedMedia.size > before.mediaCount || executedTools.size > before.toolCount
        return producedNewContent || producedNewThoughts || producedMediaOrTools
    }

    fun buildPassHistoryAndPrefix(
        baseNodes: List<MessageNode>,
        continuePrefix: String,
        protocol: ProviderProtocol,
        passTag: String,
        forceVisibleThoughtSummary: Boolean = false,
        wireDialect: String = activeWireDialect
    ): Pair<List<MessageNode>, String> = engine.buildEffectiveHistoryAndPrefix(
        baseHistoryNodes = baseNodes,
        continuePrefix = continuePrefix,
        thinkingText = thinkingBuilder.toString(),
        trailingThinkingText = currentStepThinkingBuilder.toString().trim(),
        signatureText = signatureBuilder.toString(),
        executedTools = executedTools.toList(),
        generatedMedia = generatedMedia.toList(),
        protocol = protocol,
        model = model,
        passTag = passTag,
        forceVisibleThoughtSummary = forceVisibleThoughtSummary,
        wireDialect = wireDialect
    )

    suspend fun attachSubagentRunner() {
        currentCoroutineContext()[AgentToolRunContext]?.broker?.takeIf { usagePurpose != SUBAGENT_PURPOSE }?.let { broker ->
            broker.subagentRunner = { task, onProgress ->
                val spent = spendBeforeRunUsd + spendTowardLimit(usageLedger.snapshots()) - ledgerSpendAtStart
                engine.runSubagent(profile, session, broker, ++helperCount, task, spendLimitUsd, spent, onProgress) { record ->
                    usageLedger.adopt(record)
                    latestUsageMetrics = usageLedger.metrics()
                    emit(StreamUpdate.UsageUpdated(record, latestUsageMetrics))
                }
            }
        }
    }

    suspend fun emit(update: StreamUpdate) = collector.emit(update)

    suspend fun emitError(throwable: Throwable) {
        val cleanErr = engine.extractCleanApiErrorSummary(throwable.message ?: throwable::class.simpleName ?: "Unknown network error")
        emit(StreamUpdate.Error(message = cleanErr, rawDetails = lastRawResp))
    }

    suspend fun emitStatus(
        message: String,
        interactionId: String = "",
        includeInteractionId: Boolean = true
    ) {
        if (interactionId.isNotBlank()) lastInteractionId = interactionId
        emit(StreamUpdate.Status(message, if (includeInteractionId) lastInteractionId else ""))
    }

    suspend fun emitGoalProgress(goalState: GoalTrackerState) {
        emit(StreamUpdate.GoalProgress(goalState))
    }

    fun markOutputLimitReached() {
        reachedOutputLimit = true
    }

    fun recordTokenActivity() {
        val now = activity.elapsedMs
        if (firstTokenTime < 0L) firstTokenTime = now
        if (currentBurstStartMs < 0L) {
            currentBurstStartMs = now
            currentBurstLastMs = now
        } else {
            val gap = now - currentBurstLastMs
            if (gap > 2500L) {
                // Server paused >2.5s (e.g. server-side tool / web search / multi-round TTFT); close prior burst
                activeDecodeAccumulatedMs += (currentBurstLastMs - currentBurstStartMs).coerceAtLeast(40L)
                currentBurstStartMs = now
                currentBurstLastMs = now
            } else {
                currentBurstLastMs = now
            }
        }
    }

    private fun closeCurrentDecodeBurst() {
        if (currentBurstStartMs >= 0L && currentBurstLastMs >= currentBurstStartMs) {
            activeDecodeAccumulatedMs += (currentBurstLastMs - currentBurstStartMs).coerceAtLeast(40L)
        }
        currentBurstStartMs = -1L
        currentBurstLastMs = -1L
    }

    fun recordRawWireBounded(req: String, resp: String) {
        if (req.isNotEmpty()) {
            lastRawReq = if (req.length > 4096) req.substring(0, 4096) else req
        }
        if (resp.isNotEmpty()) {
            lastRawResp = if (resp.length > 4096) resp.substring(0, 4096) else resp
        }
    }

    private fun stopIfOverSpendLimit() {
        if (spendLimitUsd > 0.0 && spendBeforeRunUsd + spendTowardLimit(usageLedger.snapshots()) - ledgerSpendAtStart >= spendLimitUsd) {
            throw SpendLimitException(spendLimitUsd)
        }
    }

    suspend fun beginUsageRequest(dialect: UsageDialect): String {
        stopIfOverSpendLimit()
        val mediaInputs = buildSet {
            val fees = requestChargePricing(profile, model).perUnitUsd
            historyNodes.filter { it.includedInContext }.forEach { node ->
                node.activeVariant.attachments.forEach { attachment ->
                    if (attachment.base64Data.isNotBlank() || attachment.localFilePath.isNotBlank()) {
                        when {
                            attachment.mimeType.startsWith("audio/", true) -> add("input_audio")
                            attachment.mimeType.startsWith("video/", true) -> add("input_video")
                            attachment.mimeType.startsWith("image/", true) && (fees["input_image_count"] ?: 0.0) > 0 ->
                                add("input_image_count")
                        }
                    }
                }
            }
        }
        val record = usageLedger.begin(dialect, mediaInputs)
        latestUsageMetrics = usageLedger.metrics()
        emit(StreamUpdate.UsageUpdated(record, latestUsageMetrics))
        return record.requestId
    }

    suspend fun recordRequestUsage(requestId: String?, responseId: String, dialect: UsageDialect, usage: ProviderTokenUsage) {
        if (responseId.isNotBlank()) lastInteractionId = responseId
        val record = usageLedger.update(requestId, responseId, dialect, usage) ?: return
        latestUsageMetrics = usageLedger.metrics()
        emit(StreamUpdate.UsageUpdated(record, latestUsageMetrics))
    }

    suspend fun beginMediaUsage(wireDialect: String, component: String): String {
        stopIfOverSpendLimit()
        val record = usageLedger.beginMedia(wireDialect, component)
        latestUsageMetrics = usageLedger.metrics()
        emit(StreamUpdate.UsageUpdated(record, latestUsageMetrics))
        return record.requestId
    }

    suspend fun handleSignedThinking(delta: String, sig: String) {
        if (delta.isNotEmpty()) recordTokenActivity()
        thinkingBuilder.append(delta)
        currentStepThinkingBuilder.append(delta)
        val uniqueSig = engine.appendSignatureChunkSafe(signatureBuilder, sig)
        if (delta.isNotEmpty() || uniqueSig.isNotEmpty()) {
            emit(StreamUpdate.ThinkingDelta(delta, uniqueSig))
        }
    }

    suspend fun handleSimpleThinking(delta: String) {
        if (delta.isNotEmpty()) recordTokenActivity()
        thinkingBuilder.append(delta)
        currentStepThinkingBuilder.append(delta)
        emit(StreamUpdate.ThinkingDelta(delta))
    }

    suspend fun handleMediaThinking(delta: String) {
        if (delta.isNotEmpty()) recordTokenActivity()
        thinkingBuilder.append(delta)
        emit(StreamUpdate.ThinkingDelta(delta))
    }

    suspend fun handleContentDelta(delta: String) {
        if (delta.isNotEmpty()) recordTokenActivity()
        contentBuilder.append(delta)
        val approxOut = (contentBuilder.length / 4).coerceAtLeast(
            latestUsageMetrics.outputTokens.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
        )
        val elapsedSec = ((activity.elapsedMs - startTime) / 1000L).toInt()
        PlatformRuntime.notifyLongDecodeProgress(model, approxOut, elapsedSec)
        emit(StreamUpdate.ContentDelta(delta))
    }

    suspend fun handleMediaContentDelta(delta: String) {
        if (delta.isNotEmpty()) recordTokenActivity()
        contentBuilder.append(delta)
        emit(StreamUpdate.ContentDelta(delta))
    }

    suspend fun recordGeneratedMedia(item: GeneratedMediaItem) {
        recordTokenActivity()
        usageLedger.observeChargeComponent("output_${item.kind.name.lowercase()}")?.let { record ->
            latestUsageMetrics = usageLedger.metrics()
            emit(StreamUpdate.UsageUpdated(record, latestUsageMetrics))
        }
        generatedMedia.add(item)
        emit(StreamUpdate.MediaGenerated(item))
    }

    private fun attachStepThinkingToToolCall(tc: ToolCallRecord): ToolCallRecord {
        val stepThink = currentStepThinkingBuilder.toString().trim()
        val stepSig = signatureBuilder.toString().trim()
        currentStepThinkingBuilder.clear()
        signatureBuilder.clear()
        return tc.copy(
            precedingThinking = tc.precedingThinking.ifBlank { stepThink },
            thoughtSignature = tc.thoughtSignature.ifBlank { stepSig }
        )
    }

    suspend fun recordRawCitationAndEmit(citation: RawWebCitation) {
        if (citation.url.isBlank() || rawWebCitations.size >= 128) return
        val isDuplicate = rawWebCitations.any {
            it.url == citation.url && it.startIndex == citation.startIndex && it.groundedText == citation.groundedText
        }
        if (isDuplicate) return
        rawWebCitations.add(citation)
        val bound = engine.bindSourcesToMessageLines(contentBuilder.toString(), rawWebCitations)
        if (bound.isNotEmpty()) {
            emit(StreamUpdate.WebSourcesUpdated(bound))
        }
    }

    suspend fun upsertToolCallAndEmit(reported: ToolCallRecord) {
        val tc = reported.copy(screenshotJpegBase64 = "") // Screens go to the provider, not into the chat's state.
        closeCurrentDecodeBurst()
        val existingIdx = executedTools.indexOfFirst { it.id == tc.id && tc.id.isNotBlank() }
        val enriched = if (existingIdx >= 0) {
            val prev = executedTools[existingIdx]
            tc.copy(
                precedingThinking = tc.precedingThinking.ifBlank { prev.precedingThinking },
                thoughtSignature = tc.thoughtSignature.ifBlank { prev.thoughtSignature }
            )
        } else {
            attachStepThinkingToToolCall(tc)
        }
        if (existingIdx >= 0) {
            executedTools[existingIdx] = enriched
        } else {
            executedTools.add(enriched)
        }
        emit(StreamUpdate.ToolCallEvent(enriched))
        if (!enriched.isRunning) {
            engine.extractCitationsFromToolCall(enriched).forEach { recordRawCitationAndEmit(it) }
        }
    }

    suspend fun reportCompletedRun(goalState: GoalTrackerState?) {
        closeCurrentDecodeBurst()
        val endTime = activity.elapsedMs
        val totalDurationMs = (endTime - startTime + initialElapsedMs.coerceAtLeast(0L)).coerceAtLeast(1L)
        val ttftMs = if (firstTokenTime >= startTime) (firstTokenTime - startTime) else totalDurationMs
        val reportedUsage = usageLedger.metrics()
        val fallbackInTokens = if (usageLedger.hasReportedInput) {
            reportedUsage.inputTokens
        } else {
            val sysTok = O200kTokenizer.countTokens(engine.buildCompiledSystemPrompt(session, goalState))
            val histTok = historyNodes.filter { it.includedInContext }.sumOf {
                O200kTokenizer.countMessageVariantTokens(it.role, it.activeVariant, includeReasoningAndTools = false)
            }
            (sysTok.toLong() + histTok).coerceAtLeast(1L)
        }
        val rawMetrics = engine.computeTurnTokenMetrics(
            reportedInputTokens = reportedUsage.inputTokens,
            reportedCachedTokens = reportedUsage.cachedInputTokens,
            reportedReasoningTokens = reportedUsage.reasoningTokens,
            reportedOutputTokens = reportedUsage.outputTokens,
            fallbackInputTokens = fallbackInTokens,
            contentText = contentBuilder.toString(),
            thinkingText = thinkingBuilder.toString(),
            executedTools = executedTools,
            totalDurationMs = totalDurationMs,
            ttftMs = ttftMs,
            activeDecodeDurationMs = activeDecodeAccumulatedMs,
            totalToolExecutionMs = activity.toolMs,
            hasReportedInput = usageLedger.hasReportedInput,
            hasReportedOutput = usageLedger.hasReportedOutput,
            hasReportedReasoning = usageLedger.hasReportedReasoning
        ).copy(
            cacheCreationInputTokens = reportedUsage.cacheCreationInputTokens,
            cacheCreationCostUnits = reportedUsage.cacheCreationCostUnits,
            reportedRequests = reportedUsage.reportedRequests,
            usageIsComplete = reportedUsage.usageIsComplete,
            costIsAvailable = reportedUsage.costIsAvailable,
            estimatedCostUsd = reportedUsage.estimatedCostUsd,
            outputIncludesUnreportedReasoning = reportedUsage.outputIncludesUnreportedReasoning,
            unpricedCostComponents = reportedUsage.unpricedCostComponents,
            reportedCostRequests = reportedUsage.reportedCostRequests
        )

        // If web search or URL tools were active and the model included inline markdown citations, bind them too
        if (profile.enableWebSearch || profile.enableUrlContext || executedTools.any {
                it.name.equals("web_fetch", ignoreCase = true) || it.name.equals("web_search", ignoreCase = true)
            }
        ) {
            engine.extractMarkdownLinksAsCitations(contentBuilder.toString()).forEach { mdCit ->
                if (rawWebCitations.none { it.url.equals(mdCit.url, ignoreCase = true) }) {
                    rawWebCitations.add(mdCit)
                }
            }
        }
        val finalWebSources = engine.bindSourcesToMessageLines(contentBuilder.toString(), rawWebCitations)
        if (finalWebSources.isNotEmpty()) {
            emit(StreamUpdate.WebSourcesUpdated(finalWebSources))
        }

        emit(
            StreamUpdate.Completed(
                fullContent = contentBuilder.toString() + if (reachedOutputLimit) OUTPUT_LIMIT_NOTICE else "",
                fullThinking = thinkingBuilder.toString(),
                trailingThinking = if (executedTools.isNotEmpty()) currentStepThinkingBuilder.toString().trim() else "",
                thinkingSignature = signatureBuilder.toString(),
                toolCalls = executedTools,
                goalTracker = goalState,
                mediaItems = generatedMedia,
                webSources = finalWebSources,
                providerInteractionId = lastInteractionId,
                metrics = rawMetrics,
                rawRequest = lastRawReq,
                rawResponse = lastRawResp,
                apiUsageRecords = usageLedger.snapshots(),
                providerWireDialect = activeWireDialect
            )
        )
    }
}
