package io.parchment.ai.network

import androidx.compose.runtime.Immutable
import io.ktor.client.HttpClient
import io.ktor.client.plugins.HttpTimeout
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.serialization.kotlinx.json.json
import io.ktor.utils.io.ByteReadChannel
import io.parchment.ai.harness.AgentToolRunContext
import io.parchment.ai.model.AgentPermissionOverrides
import io.parchment.ai.model.ApiUsageRecord
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.TokenMetrics
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.WebSourceCitation
import io.parchment.ai.model.saturatingTokenSum
import io.parchment.ai.platform.MAX_INLINE_ATTACHMENT_BYTES
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.protocol.O200kTokenizer
import io.parchment.ai.runtime.AppForeground
import io.parchment.ai.runtime.runActivityFlow
import io.parchment.ai.strings.Strings
import kotlin.io.encoding.ExperimentalEncodingApi
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject

@Immutable
data class ModelModalityProfile(
    val outputModalities: List<String>,
    val inputModalities: List<String> = listOf("text"),
    val isDedicatedImage: Boolean = false,
    val isInterleavedImage: Boolean = false,
    val isDedicatedVideo: Boolean = false,
    val isDedicatedAudioTts: Boolean = false,
    val isLiveRealtimeAudio: Boolean = false
) {
    val isAnyImageModel: Boolean get() = isDedicatedImage || isInterleavedImage
    val isAnyVideoModel: Boolean get() = isDedicatedVideo
    val isAnyAudioModel: Boolean get() = isDedicatedAudioTts || isLiveRealtimeAudio
}

sealed class StreamUpdate {
    data class Status(val message: String, val interactionId: String = "") : StreamUpdate()
    data class ThinkingDelta(val delta: String, val signatureDelta: String = "") : StreamUpdate()
    data class ContentDelta(val delta: String) : StreamUpdate()
    data class ToolCallEvent(val toolCall: ToolCallRecord) : StreamUpdate()
    data class UsageUpdated(val record: ApiUsageRecord, val metrics: TokenMetrics) : StreamUpdate()
    data class GoalProgress(val goalTracker: GoalTrackerState) : StreamUpdate()
    data class MediaGenerated(val item: GeneratedMediaItem) : StreamUpdate()
    data class WebSourcesUpdated(val sources: List<WebSourceCitation>) : StreamUpdate()
    data class RawWireSnapshot(val requestJson: String, val latestResponseChunk: String) : StreamUpdate()
    data class Completed(
        val fullContent: String,
        val fullThinking: String,
        val trailingThinking: String = "",
        val thinkingSignature: String,
        val toolCalls: List<ToolCallRecord>,
        val goalTracker: GoalTrackerState?,
        val mediaItems: List<GeneratedMediaItem>,
        val webSources: List<WebSourceCitation> = emptyList(),
        val providerInteractionId: String,
        val metrics: TokenMetrics,
        val rawRequest: String,
        val rawResponse: String,
        val apiUsageRecords: List<ApiUsageRecord> = emptyList(),
        val providerWireDialect: String = ""
    ) : StreamUpdate()
    data class Error(val message: String, val rawDetails: String = "") : StreamUpdate()
}

internal const val MAX_DOWNLOADED_IMAGE_BYTES = 64 * 1024 * 1024
internal const val MAX_DOWNLOADED_AUDIO_BYTES = 128 * 1024 * 1024
internal const val MAX_DOWNLOADED_VIDEO_BYTES = 512 * 1024 * 1024

internal fun oversizedAttachmentMessage(fileName: String, sizeBytes: Long): String =
    "\"$fileName\" is ${sizeBytes / (1024L * 1024L)} MB, over the ${MAX_INLINE_ATTACHMENT_BYTES / (1024L * 1024L)} MB limit for files sent with a request. Remove it or attach a smaller file."

@OptIn(ExperimentalEncodingApi::class)
object AiProviderEngine {

    /**
     * Long-horizon autonomous execution engine.
     * Uses a Repeated-Failure Circuit Breaker + Stop-Hook Work-Check Gate instead of a brittle 64-step limit,
     * allowing tasks to run productively for hours, days, or weeks while halting repeated identical errors.
     */
    internal const val MAX_SAFETY_HORIZON_STEPS = 10_000
    internal const val MAX_PRE_TOKEN_RETRIES = 10
    internal const val RETRY_WAIT_SECONDS = 60

    /** Shown under an answer the model had to cut short; removed before the text is sent back. */
    internal const val OUTPUT_LIMIT_NOTICE = "\n\n*[Stopped at the model's output limit. Use Continue to keep going.]*"

    /** Marker prefixing a carried-forward reasoning narrative from a different provider. */
    internal const val REASONING_HANDOFF_MARKER = "[Prior reasoning summary]"

    /** Upper bound on carried reasoning text per historical turn (keeps context lean). */
    internal const val MAX_REASONING_HANDOFF_CHARS = 8_000
    internal const val MAX_POLL_ITERATIONS = 450
    internal const val MAX_POLL_TRANSIENT_ERRORS = 5
    internal const val LEGACY_FALLBACK_PRIMARY_MAX_RETRIES = 2
    internal const val LEGACY_FALLBACK_PRIMARY_WAIT_SECONDS = 3
    internal val discoveredModelModalities = mutableMapOf<String, List<String>>()
    internal val discoveredModelVendors = mutableMapOf<String, String>()

    internal class StepRetriesExhaustedException(message: String, cause: Throwable) : RuntimeException(message, cause)

    /** A stop that sending the request again could only make worse. */
    internal open class NoRetryException(message: String, cause: Throwable? = null) : RuntimeException(message, cause)

    /** The provider already accepted this job, so retrying the request could pay for it twice. */
    internal class AcceptedJobException(message: String, cause: Throwable? = null) : NoRetryException(message, cause)

    /** The chat reached the spend limit set in Settings. */
    internal class SpendLimitException(limitUsd: Double) :
        NoRetryException("This chat reached its ${"$"}$limitUsd spend limit. Raise or clear the limit in Settings to continue.")

    internal suspend fun waitWithRetryCountdown(
        attempt: Int,
        maxAttempts: Int,
        waitSeconds: Int = RETRY_WAIT_SECONDS,
        reasonSummary: String,
        onStatus: suspend (String) -> Unit
    ) {
        val cleanReason = extractCleanApiErrorSummary(reasonSummary)
        if (STREAM_CUT_WHILE_AWAY in reasonSummary) { // Nothing to back off from: the provider never failed.
            onStatus(Strings.retryingNowAttempt(attempt, maxAttempts))
            return
        }
        val resumedAtStart = AppForeground.resumedAt.value
        for (remaining in waitSeconds downTo 1) {
            if (AppForeground.resumedAt.value != resumedAtStart) break // Someone is back and waiting; a connection may work again.
            val statusMsg = if (cleanReason.isNotBlank()) {
                "Auto-retrying ($attempt/$maxAttempts in ${remaining}s)\n$cleanReason"
            } else {
                "Auto-retrying ($attempt/$maxAttempts in ${remaining}s)…"
            }
            onStatus(statusMsg)
            delay(1000L)
        }
        onStatus(Strings.retryingNowAttempt(attempt, maxAttempts))
    }

    internal val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        encodeDefaults = true
        prettyPrint = false
    }

    internal val httpClient by lazy {
        HttpClient(providerHttpEngine) {
            install(ContentNegotiation) {
                json(this@AiProviderEngine.json)
            }
            install(HttpTimeout) {
                requestTimeoutMillis = 24 * 60 * 60 * 1000L // 24-hour per-request ceiling for ultra-long reasoning & decodes
                connectTimeoutMillis = 45_000L
                socketTimeoutMillis = 24 * 60 * 60 * 1000L
            }
        }
    }

    internal val mcpHttpClient by lazy {
        httpClient.config {
            followRedirects = false
        }
    }

    /**
     * Streams or polls a generation turn across Gemini Interactions API, OpenAI Responses API,
     * or Anthropic Messages API, including multi-step local workspace tool loops and
     * mid-sentence "Continue" / Assistant Prefill support.
     */
    fun streamGeneration(
        profile: ProviderProfile,
        session: ChatSession,
        historyNodes: List<MessageNode>,
        continueFromAssistantPrefix: String = "",
        initialThinking: String = "",
        initialSignature: String = "",
        initialToolCalls: List<ToolCallRecord> = emptyList(),
        initialMediaItems: List<GeneratedMediaItem> = emptyList(),
        initialWebSources: List<WebSourceCitation> = emptyList(),
        initialGoalTracker: GoalTrackerState? = null,
        resumeMediaTask: String = "",
        spendLimitUsd: Double = 0.0,
        spendBeforeRunUsd: Double = 0.0,
        /** Set for helpers: the whole run's live spend, so siblings working in parallel share one limit. */
        externalRunSpendUsd: (() -> Double)? = null,
        initialElapsedMs: Long = 0L,
        initialApiUsageRecords: List<ApiUsageRecord> = emptyList(),
        initialTokenMetrics: TokenMetrics = TokenMetrics(),
        usageOwnerSessionId: String = session.id,
        usageOwnerRunId: String = "",
        usagePurpose: String = "chat",
        pollDelayMsOverride: Long? = null
    ): Flow<StreamUpdate> = runActivityFlow(session.modelOverride.ifBlank { profile.defaultModel }) { activity ->
        val model = session.modelOverride.ifBlank { profile.defaultModel }.trim()
        if (model.isBlank()) {
            emit(
                StreamUpdate.Error(
                    message = "No Model ID is selected. Fetch models automatically from the endpoint or enter a Model ID manually in Providers & Proxy."
                )
            )
            return@runActivityFlow
        }
        val resolvedRunId = usageOwnerRunId.ifBlank {
            currentCoroutineContext()[AgentToolRunContext]?.broker?.runId ?: PlatformRuntime.createRunId()
        }
        GenerationStreamSession(
            engine = this@AiProviderEngine,
            collector = this,
            activity = activity,
            profile = profile,
            session = session,
            model = model,
            historyNodes = historyNodes,
            cleanInitialPrefix = stripInlineErrorNotices(continueFromAssistantPrefix),
            initialThinking = initialThinking,
            initialSignature = initialSignature,
            initialToolCalls = initialToolCalls,
            initialMediaItems = initialMediaItems,
            initialWebSources = initialWebSources,
            initialGoalTracker = initialGoalTracker,
            resumeMediaTask = resumeMediaTask,
            spendLimitUsd = spendLimitUsd,
            spendBeforeRunUsd = spendBeforeRunUsd,
            externalRunSpendUsd = externalRunSpendUsd,
            initialElapsedMs = initialElapsedMs,
            initialApiUsageRecords = initialApiUsageRecords,
            initialTokenMetrics = initialTokenMetrics,
            usageOwnerSessionId = usageOwnerSessionId,
            resolvedUsageRunId = resolvedRunId,
            usagePurpose = usagePurpose,
            pollDelayMsOverride = pollDelayMsOverride
        ).run()
    }.flowOn(Dispatchers.IO)

    /**
     * Computes accurate turn-level `TokenMetrics` and `tokensPerSec`:
     * - Numerator includes both `finalOutTokens` (prose + tool call arguments across all rounds) and `finalThinkTokens`.
     * - Denominator uses `activeDecodeDurationMs` (active token streaming bursts excluding TTFT wait and local tool execution time).
     */
    private data class UtilityRequestKey(
        val fastProfile: ProviderProfile,
        val model: String,
        val prompt: String,
        val ownerSessionId: String,
        val purpose: String
    )

    private class UtilityWaiter(
        val ownerRunId: String,
        val onUsage: suspend (ApiUsageRecord, TokenMetrics) -> Unit,
        val result: CompletableDeferred<Result<String>> = CompletableDeferred()
    )

    private class InFlightUtilityCall(val primaryRunId: String) {
        val stateLock = Mutex()
        val waiters = mutableListOf<UtilityWaiter>()
        var latestUsage: Pair<ApiUsageRecord, TokenMetrics>? = null
        var job: Job? = null
    }

    private val utilityScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val inFlightUtilityLock = Mutex()
    private val inFlightUtilityCalls = mutableMapOf<UtilityRequestKey, InFlightUtilityCall>()

    internal fun computeTurnTokenMetrics(
        reportedInputTokens: Long,
        reportedCachedTokens: Long,
        reportedReasoningTokens: Long,
        reportedOutputTokens: Long,
        fallbackInputTokens: Long,
        contentText: String,
        thinkingText: String,
        executedTools: List<ToolCallRecord>,
        totalDurationMs: Long,
        ttftMs: Long,
        activeDecodeDurationMs: Long,
        totalToolExecutionMs: Long,
        hasReportedInput: Boolean = reportedInputTokens > 0L,
        hasReportedOutput: Boolean = reportedOutputTokens > 0L,
        hasReportedReasoning: Boolean = reportedReasoningTokens > 0L
    ): TokenMetrics {
        val finalInTokens = if (hasReportedInput) reportedInputTokens.coerceAtLeast(0L) else fallbackInputTokens.coerceAtLeast(0L)
        val finalThinkTokens = when {
            hasReportedReasoning -> reportedReasoningTokens.coerceAtLeast(0L)
            hasReportedOutput -> 0L
            thinkingText.isNotEmpty() -> O200kTokenizer.countTokens(thinkingText).coerceAtLeast(1).toLong()
            else -> 0L
        }
        val finalOutTokens = if (hasReportedOutput) {
            reportedOutputTokens.coerceAtLeast(0L)
        } else {
            val bpeContentTokens = if (contentText.isNotEmpty()) {
                O200kTokenizer.countTokens(contentText).coerceAtLeast(1).toLong()
            } else 0L
            val rawCallTokens = executedTools.sumOf { tc ->
                O200kTokenizer.countTokens(tc.name).toLong() + O200kTokenizer.countTokens(tc.argumentsJson)
            }
            saturatingTokenSum(bpeContentTokens, rawCallTokens)
        }
        val totalGeneratedTokens = saturatingTokenSum(finalOutTokens, finalThinkTokens)

        val netLlmGenerationMs = (totalDurationMs - totalToolExecutionMs.coerceAtLeast(0L)).coerceAtLeast(250L)
        val minRealisticBurstMs = maxOf(500L, totalGeneratedTokens.coerceAtMost(Long.MAX_VALUE / 8L) * 8L)
        val effectiveDecodeMs = if (activeDecodeDurationMs >= minRealisticBurstMs && activeDecodeDurationMs <= netLlmGenerationMs) {
            activeDecodeDurationMs
        } else {
            netLlmGenerationMs
        }
        val tokPerSec = (totalGeneratedTokens * 1000.0) / effectiveDecodeMs.toDouble()
        return TokenMetrics(
            inputTokens = finalInTokens,
            cachedInputTokens = reportedCachedTokens,
            reasoningTokens = finalThinkTokens,
            outputTokens = finalOutTokens,
            ttftMs = ttftMs,
            durationMs = totalDurationMs,
            tokensPerSec = kotlin.math.floor(tokPerSec * 10.0) / 10.0
        )
    }

    /**
     * Lightweight one-shot utility text generation for Auto-Title Generation,
     * Title Regeneration (`✦`), Context Summarization, and Translation.
     * `maxOutputTokens` defaults to `null` (unlimited) so summaries, compactions, and translations
     * are never truncated mid-sentence.
     */
    suspend fun generateUtilityText(
        profile: ProviderProfile,
        model: String,
        prompt: String,
        maxOutputTokens: Int? = null,
        ownerSessionId: String = "",
        ownerRunId: String = "",
        purpose: String = "utility",
        onUsage: suspend (ApiUsageRecord, TokenMetrics) -> Unit = { _, _ -> }
    ): Result<String> = withContext(Dispatchers.IO + AgentToolRunContext(null)) {
        val cleanModel = model.trim().ifBlank { profile.defaultModel.trim() }
        if (cleanModel.isBlank()) {
            return@withContext Result.failure(IllegalArgumentException("No model selected for utility generation."))
        }
        val fastProfile = profile.copy(
            thinkingEffort = "low",
            maxOutputTokens = maxOutputTokens,
            enableWebSearch = false,
            enableCodeExecution = false,
            enableUrlContext = false,
            enableImageGeneration = false,
            enableVideoGeneration = false,
            useBackgroundLongDecode = false
        )
        val key = UtilityRequestKey(
            fastProfile = fastProfile,
            model = cleanModel,
            prompt = prompt,
            ownerSessionId = ownerSessionId,
            purpose = purpose
        )
        val waiter = UtilityWaiter(ownerRunId = ownerRunId, onUsage = onUsage)
        var snapshotUsage: Pair<ApiUsageRecord, TokenMetrics>? = null
        val (flight, isLeader) = inFlightUtilityLock.withLock {
            val existing = inFlightUtilityCalls[key]
            if (existing != null && existing.job?.isCompleted != true) {
                existing.stateLock.withLock {
                    existing.waiters.add(waiter)
                    snapshotUsage = existing.latestUsage
                }
                existing to false
            } else {
                InFlightUtilityCall(primaryRunId = ownerRunId).also { created ->
                    created.waiters.add(waiter)
                    inFlightUtilityCalls[key] = created
                } to true
            }
        }
        if (isLeader) {
            val job = utilityScope.launch(AgentToolRunContext(null)) {
                var finishedOutcome: Result<String>? = null
                try {
                    val tempSession = ChatSession(
                        id = "util-temp",
                        title = "Utility",
                        createdAt = 0L,
                        updatedAt = 0L,
                        profileId = profile.id,
                        modelOverride = cleanModel,
                        permissionOverrides = AgentPermissionOverrides(toolsEnabled = false)
                    )
                    val userNode = MessageNode(
                        id = "util-node",
                        role = MessageRole.USER,
                        variants = listOf(MessageVariant(id = "util-var", content = prompt))
                    )
                    val sb = StringBuilder()
                    var streamErr: String? = null
                    var completed = false
                    streamGeneration(
                        profile = fastProfile,
                        session = tempSession,
                        historyNodes = listOf(userNode),
                        usageOwnerSessionId = ownerSessionId,
                        usageOwnerRunId = ownerRunId,
                        usagePurpose = purpose
                    ).collect { upd ->
                        when (upd) {
                            is StreamUpdate.ContentDelta -> sb.append(upd.delta)
                            is StreamUpdate.Completed -> { completed = true; sb.clear(); sb.append(upd.fullContent) }
                            is StreamUpdate.UsageUpdated -> {
                                val snapshot = flight.stateLock.withLock {
                                    flight.latestUsage = upd.record to upd.metrics
                                    flight.waiters.toList()
                                }
                                for (w in snapshot) {
                                    val attributed = if (w.ownerRunId == flight.primaryRunId || w.ownerRunId.isBlank()) {
                                        upd.record
                                    } else {
                                        upd.record.copy(ownerRunId = w.ownerRunId)
                                    }
                                    try {
                                        w.onUsage(attributed, upd.metrics)
                                    } catch (cancelled: CancellationException) {
                                        val emptyAfter = flight.stateLock.withLock {
                                            flight.waiters.remove(w)
                                            flight.waiters.isEmpty()
                                        }
                                        w.result.completeExceptionally(cancelled)
                                        if (emptyAfter) throw cancelled
                                    }
                                }
                            }
                            is StreamUpdate.Error -> streamErr = upd.message
                            else -> {}
                        }
                    }
                    val finalStr = sb.toString().trim().removeSurrounding("\"").removeSurrounding("'").trim()
                    if (!streamErr.isNullOrBlank()) {
                        error(streamErr)
                    }
                    require(completed && finalStr.isNotBlank()) { "Utility response ended without a complete result." }
                    finishedOutcome = Result.success(finalStr)
                } catch (cancelled: CancellationException) {
                    val remaining = withContext(NonCancellable) {
                        inFlightUtilityLock.withLock {
                            if (inFlightUtilityCalls[key] === flight) inFlightUtilityCalls.remove(key)
                        }
                        flight.stateLock.withLock {
                            val copy = flight.waiters.toList()
                            flight.waiters.clear()
                            copy
                        }
                    }
                    remaining.forEach { it.result.completeExceptionally(cancelled) }
                    throw cancelled
                } catch (err: Throwable) {
                    finishedOutcome = Result.failure(err)
                } finally {
                    val outcome = finishedOutcome
                    if (outcome != null) {
                        val remaining = withContext(NonCancellable) {
                            inFlightUtilityLock.withLock {
                                if (inFlightUtilityCalls[key] === flight) inFlightUtilityCalls.remove(key)
                            }
                            flight.stateLock.withLock {
                                val copy = flight.waiters.toList()
                                flight.waiters.clear()
                                copy
                            }
                        }
                        remaining.forEach { it.result.complete(outcome) }
                    } else {
                        withContext(NonCancellable) {
                            inFlightUtilityLock.withLock {
                                if (inFlightUtilityCalls[key] === flight) inFlightUtilityCalls.remove(key)
                            }
                        }
                    }
                }
            }
            val shouldCancelEarly = flight.stateLock.withLock {
                flight.job = job
                flight.waiters.isEmpty()
            }
            if (shouldCancelEarly) job.cancel()
        } else if (snapshotUsage != null) {
            val (rec, met) = snapshotUsage!!
            val attributed = if (ownerRunId == flight.primaryRunId || ownerRunId.isBlank()) rec else rec.copy(ownerRunId = ownerRunId)
            onUsage(attributed, met)
        }
        try {
            waiter.result.await()
        } finally {
            withContext(NonCancellable) {
                val shouldCancel = flight.stateLock.withLock {
                    flight.waiters.remove(waiter) && flight.waiters.isEmpty()
                }
                if (shouldCancel) {
                    inFlightUtilityLock.withLock {
                        if (inFlightUtilityCalls[key] === flight) inFlightUtilityCalls.remove(key)
                    }
                    flight.job?.cancel()
                }
            }
        }
    }

    internal fun resolveWireProtocol(profile: ProviderProfile): ProviderProtocol =
        resolveProviderWireProtocol(profile)

    internal fun resolveWireDialect(profile: ProviderProfile): String =
        resolveProviderWireDialect(profile)

    internal fun isCustomDialectUnresolved(profile: ProviderProfile): Boolean =
        isProviderCustomDialectUnresolved(profile)

    internal fun resolveSmartEndpointUrl(
        profile: ProviderProfile,
        defaultPathWithV1: String,
        defaultPathWithoutVersion: String = defaultPathWithV1
            .removePrefix("/v1beta")
            .removePrefix("/v1")
    ): String = resolveProviderSmartEndpointUrl(profile, defaultPathWithV1, defaultPathWithoutVersion)

    internal data class ResolvedBinaryAttachment(
        val fileName: String,
        val mimeType: String,
        val base64Data: String
    )

    internal data class ToolStepBatch(
        val thoughtSignature: String,
        val precedingThinking: String,
        val calls: List<ToolCallRecord>,
        val precedingText: String = ""
    )

    internal fun buildUrlWithAuthQuery(profile: ProviderProfile, rawUrl: String): String =
        buildProviderUrlWithAuthQuery(profile, rawUrl)

    internal fun HttpRequestBuilder.applyAuthAndCustomHeaders(profile: ProviderProfile) =
        applyProviderAuthAndCustomHeaders(profile)

    internal fun mergeRawJsonExtraBody(base: JsonObject, rawExtraJson: String): JsonObject =
        mergeProviderRawJsonExtraBody(base, rawExtraJson, json)

    internal fun deepMergeJsonObjects(target: JsonObject, overrideObj: JsonObject): JsonObject =
        deepMergeProviderJsonObjects(target, overrideObj)

    internal suspend fun readSseOrJsonFallback(
        channel: ByteReadChannel,
        streamFinished: () -> Boolean,
        onSseEvent: suspend (eventType: String, data: String) -> Unit,
        onPlainJsonFallback: suspend (fullJson: String) -> Unit,
        maxBufferedChars: Int = DEFAULT_MAX_SSE_BUFFER_CHARS,
        returnGraceMs: Long = DEFAULT_RETURN_GRACE_MS
    ) = readProviderSseOrJsonFallback(channel, streamFinished, onSseEvent, onPlainJsonFallback, maxBufferedChars, returnGraceMs)

    internal data class RawWebCitation(
        val url: String,
        val title: String = "",
        val snippet: String = "",
        val groundedText: String = "",
        val startIndex: Int = -1,
        val endIndex: Int = -1
    )
}
