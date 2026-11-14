package io.parchment.ai.network

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.ApiUsageRecord
import io.parchment.ai.model.CUSTOM_DIALECT_OPENAI_CHAT
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.TokenMetrics
import io.parchment.ai.model.ToolApprovalMode
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.WebSourceCitation
import io.parchment.ai.network.AiProviderEngine.NoRetryException
import io.parchment.ai.network.AiProviderEngine.StepRetriesExhaustedException
import io.parchment.ai.runtime.RunActivity
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.FlowCollector

// Per-turn protocol dispatch, fallback/media routing, overflow compaction, and retry/nudge recovery.

private const val MAX_GOAL_NETWORK_RETRIES = 120
private const val MAX_MID_STREAM_CONTINUATION_PASSES = 64

internal class GenerationStreamSession(
    private val engine: AiProviderEngine,
    collector: FlowCollector<StreamUpdate>,
    activity: RunActivity,
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
    initialGoalTracker: GoalTrackerState?,
    private val resumeMediaTask: String,
    spendLimitUsd: Double,
    spendBeforeRunUsd: Double,
    initialElapsedMs: Long,
    initialApiUsageRecords: List<ApiUsageRecord>,
    initialTokenMetrics: TokenMetrics,
    usageOwnerSessionId: String,
    resolvedUsageRunId: String,
    usagePurpose: String
) {
    private val toolPolicy = AgentPermissionResolver.resolve(session, profile = profile)
    private val transportProfile = profile.copy(
        useLegacyFallbackEndpoint = profile.protocol != ProviderProtocol.CUSTOM && profile.useLegacyFallbackEndpoint,
        enableWebSearch = profile.enableWebSearch && toolPolicy.webAccess && toolPolicy.allowsTool("web_search"),
        enableUrlContext = profile.enableUrlContext && toolPolicy.webAccess && toolPolicy.allowsTool("web_fetch"),
        enableCodeExecution = profile.enableCodeExecution &&
            toolPolicy.allowsTool("code_execution") &&
            toolPolicy.approvalMode == ToolApprovalMode.NEVER,
        enableImageGeneration = profile.enableImageGeneration && toolPolicy.allowsTool("image_generation"),
        enableVideoGeneration = profile.enableVideoGeneration && toolPolicy.allowsTool("video_generation"),
        allowShellExecution = false
    )

    private val circuitBreaker = WorkspaceHarness.LoopCircuitBreaker(
        initialGoal = initialGoalTracker?.takeIf { toolPolicy.toolsEnabled }
            ?: engine.extractGoalObjectiveFromHistory(historyNodes)
                .takeIf { toolPolicy.toolsEnabled && it.isNotBlank() }
                ?.let { GoalTrackerState(goalObjective = it) }
    )

    private val state = GenerationStreamAccumulator(
        engine = engine,
        collector = collector,
        activity = activity,
        profile = profile,
        session = session,
        model = model,
        historyNodes = historyNodes,
        cleanInitialPrefix = cleanInitialPrefix,
        initialThinking = initialThinking,
        initialSignature = initialSignature,
        initialToolCalls = initialToolCalls,
        initialMediaItems = initialMediaItems,
        initialWebSources = initialWebSources,
        spendLimitUsd = spendLimitUsd,
        spendBeforeRunUsd = spendBeforeRunUsd,
        initialElapsedMs = initialElapsedMs,
        initialApiUsageRecords = initialApiUsageRecords,
        initialTokenMetrics = initialTokenMetrics,
        usageOwnerSessionId = usageOwnerSessionId,
        resolvedUsageRunId = resolvedUsageRunId,
        usagePurpose = usagePurpose
    )

    private var activeBaseHistoryNodes: List<MessageNode> = emptyList()
    private var currentContinuePrefix = cleanInitialPrefix
    private var retryAttempt = 0
    private var continuationPass = 0
    private var emptyAnswerNudgePass = 0
    private var emptyStreamRetryPass = 0

    suspend fun run() {
        state.attachSubagentRunner()
        circuitBreaker.goalState?.let { state.emitGoalProgress(it) }

        try {
            val wireProtocol = engine.resolveWireProtocol(profile)
            val sanitizedHistory = engine.sanitizeContextNodes(historyNodes)
            val lastAssistant = sanitizedHistory.lastOrNull { it.role == MessageRole.ASSISTANT }
            val willReusePriorChain = profile.useStatefulChaining &&
                currentContinuePrefix.isBlank() &&
                lastAssistant != null &&
                engine.reusableProviderChainId(lastAssistant.activeVariant, wireProtocol, model).isNotBlank()
            activeBaseHistoryNodes = engine.applyCrossProviderReasoningHandoff(
                nodes = historyNodes,
                currentProtocol = wireProtocol,
                chainingActive = willReusePriorChain
            )

            while (true) {
                val passBefore = state.snapshotProgress()

                // From the second nudge pass (the model finished reasoning, then spent the first nudge reasoning
                // again with no answer), lower the thinking effort and show it its earlier thought summary, so
                // it writes the answer.
                val effectiveProfileForPass = when {
                    emptyAnswerNudgePass == 2 -> transportProfile.copy(thinkingEffort = "low")
                    emptyAnswerNudgePass >= 3 -> transportProfile.copy(thinkingEffort = "minimal")
                    else -> transportProfile
                }

                // If continuing a turn that already accumulated thoughts or tool calls (either before clicking
                // '→ Continue', during an automatic zero-answer nudge pass, or after a mid-stream network drop),
                // synthesize the partial assistant turn in history so no protocol loses prior thoughts/signatures
                // or re-executes completed tools.
                val (effectiveHistoryNodes, effectiveContinuePrefix) = state.buildPassHistoryAndPrefix(
                    baseNodes = activeBaseHistoryNodes,
                    continuePrefix = currentContinuePrefix,
                    protocol = engine.resolveWireProtocol(effectiveProfileForPass),
                    passTag = "pass-$continuationPass-nudge-$emptyAnswerNudgePass",
                    forceVisibleThoughtSummary = emptyAnswerNudgePass >= 2
                )

                try {
                    dispatchProtocolPass(effectiveProfileForPass, effectiveHistoryNodes, effectiveContinuePrefix)
                    if (!shouldContinueAfterCleanPass()) break
                } catch (e: CancellationException) {
                    throw e
                } catch (err: Throwable) {
                    handlePassFailure(err, passBefore)
                }
            }

            state.reportCompletedRun(circuitBreaker.goalState)
        } catch (e: CancellationException) {
            throw e
        } catch (t: Throwable) {
            state.emitError(t)
        }
    }

    private suspend fun dispatchProtocolPass(
        effectiveProfile: ProviderProfile,
        effectiveHistoryNodes: List<MessageNode>,
        effectiveContinuePrefix: String
    ) {
        when (engine.resolveWireProtocol(effectiveProfile)) {
            ProviderProtocol.GEMINI_INTERACTIONS ->
                runGeminiPass(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            ProviderProtocol.OPENAI_RESPONSES ->
                runOpenAiResponsesPass(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            ProviderProtocol.ANTHROPIC_MESSAGES ->
                runAnthropicPass(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            ProviderProtocol.CUSTOM -> {
                // Unresolved dialect must never silently guess a wire shape.
                throw IllegalStateException(
                    "Custom Protocol has no wire dialect selected. Choose one in Provider settings → Wire Dialect."
                )
            }
        }
    }

    // Empty-answer nudge (up to three passes) and immediate-EOF retry (up to two passes).
    private suspend fun shouldContinueAfterCleanPass(): Boolean {
        val hasNoFinalAnswer = state.hasNoFinalAnswer
        val hasThoughtsOrTools = state.hasThoughtsOrTools
        val isBreakerTripped = circuitBreaker.checkTripped() != null
        val goalAlreadyFinished = circuitBreaker.goalState?.isCompleted == true

        if (hasNoFinalAnswer && hasThoughtsOrTools && !isBreakerTripped && !goalAlreadyFinished && emptyAnswerNudgePass < 3) {
            emptyAnswerNudgePass++
            currentContinuePrefix = ""
            val nudgeStatus = when (emptyAnswerNudgePass) {
                1 -> "Reasoning complete — auto-continuing to generate final answer (1/3)…"
                2 -> "Auto-nudging final answer with reduced thinking overhead (2/3)…"
                else -> "Auto-nudging final answer via direct completion pass (3/3)…"
            }
            state.emitStatus(nudgeStatus)
            return true
        }
        if (hasNoFinalAnswer && !hasThoughtsOrTools && emptyStreamRetryPass < 2) {
            // Proxy returned HTTP 200 OK followed by immediate EOF with 0 thoughts, 0 tools, and 0 text
            emptyStreamRetryPass++
            state.emitStatus("Endpoint returned empty stream — auto-retrying ($emptyStreamRetryPass/2)…")
            delay(1200L)
            return true
        }
        return false
    }

    private suspend fun handlePassFailure(err: Throwable, passBefore: PassProgressSnapshot) {
        val alreadyExhaustedStepRetries = err is StepRetriesExhaustedException
        val actualErr = if (err is StepRetriesExhaustedException && err.cause != null) err.cause!! else err
        val producedAnyOutput = state.hasProducedSince(passBefore)

        // Context overflow: when the provider reports the prompt is over the model's context window,
        // condense the oldest half of the active turns into a continuity digest and retry.
        val unmutedCount = activeBaseHistoryNodes.count { it.includedInContext }
        if (engine.isContextLengthExceededError(actualErr) && unmutedCount > 4) {
            activeBaseHistoryNodes = engine.compactHistoryNodesOnOverflow(activeBaseHistoryNodes)
            state.emitStatus("Context window limit reached on endpoint — auto-compacting older turns & retrying…")
            return
        }

        val isRetryable = engine.isTransientOrProxyTimeoutError(actualErr)
        val isLongGoalRun = circuitBreaker.goalState != null
        val maxRetries = if (isLongGoalRun) MAX_GOAL_NETWORK_RETRIES else AiProviderEngine.MAX_PRE_TOKEN_RETRIES

        if (!producedAnyOutput && !alreadyExhaustedStepRetries && isRetryable && retryAttempt < maxRetries) {
            retryAttempt++
            engine.waitWithRetryCountdown(
                attempt = retryAttempt,
                maxAttempts = maxRetries,
                waitSeconds = AiProviderEngine.RETRY_WAIT_SECONDS,
                reasonSummary = actualErr.message ?: "Transient network/proxy error",
                onStatus = { msg -> state.emitStatus(msg) }
            )
        } else if (producedAnyOutput && isRetryable &&
            (isLongGoalRun || continuationPass < MAX_MID_STREAM_CONTINUATION_PASSES)
        ) {
            // A stream cut by a proxy or a dropped connection: wait, then continue the reply in a new pass.
            continuationPass++
            retryAttempt = 0
            currentContinuePrefix = state.currentContent()
            val cleanInterruptReason = engine.extractCleanApiErrorSummary(actualErr.message ?: "connection reset")
            engine.waitWithRetryCountdown(
                attempt = continuationPass,
                maxAttempts = if (isLongGoalRun) MAX_GOAL_NETWORK_RETRIES else MAX_MID_STREAM_CONTINUATION_PASSES,
                waitSeconds = AiProviderEngine.RETRY_WAIT_SECONDS,
                reasonSummary = "Stream interrupted — resuming seamlessly: $cleanInterruptReason",
                onStatus = { msg -> state.emitStatus(msg) }
            )
        } else {
            throw actualErr
        }
    }

    // Each branch is its own function: one coroutine this large aborted Android 17 when a run was stopped.
    private suspend fun runGeminiPass(
        effectiveProfile: ProviderProfile,
        effectiveHistoryNodes: List<MessageNode>,
        effectiveContinuePrefix: String
    ) {
        // Try the Interactions API first. If it fails (or, with useLegacyFallbackEndpoint on,
        // its quick retries run out), hand the thoughts, tool calls and text so far to
        // :streamGenerateContent.
        if (emptyAnswerNudgePass >= 3 && effectiveProfile.useLegacyFallbackEndpoint) {
            runLegacyGeminiGenerateContent(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            return
        }
        try {
            with(engine) {
                executeGeminiInteractionsLoop(
                    profile = effectiveProfile,
                    model = model,
                    session = session,
                    historyNodes = effectiveHistoryNodes,
                    continuePrefix = effectiveContinuePrefix,
                    circuitBreaker = circuitBreaker,
                    onStatus = { statusMsg, id -> state.emitStatus(statusMsg, id) },
                    onGoalProgress = state::emitGoalProgress,
                    onFirstToken = state::recordTokenActivity,
                    onThinking = state::handleSignedThinking,
                    onContent = state::handleContentDelta,
                    onMedia = state::recordGeneratedMedia,
                    onOutputLimit = state::markOutputLimitReached,
                    onToolCall = state::upsertToolCallAndEmit,
                    onRawWire = state::recordRawWireBounded,
                    onRequestStarted = state::beginUsageRequest,
                    onUsage = state::recordRequestUsage,
                    onCitation = state::recordRawCitationAndEmit
                )
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Throwable) {
            if (profile.protocol != ProviderProtocol.CUSTOM &&
                engine.shouldFallbackToLegacyProtocol(e, profile.useLegacyFallbackEndpoint)
            ) {
                emitFallbackStatus("Primary /v1beta/interactions failed — falling back to :streamGenerateContent…", e)
                val (fbNodes, fbPrefix) = state.buildPassHistoryAndPrefix(
                    baseNodes = activeBaseHistoryNodes,
                    continuePrefix = state.currentContent(),
                    protocol = profile.protocol,
                    passTag = "fallback-gemini-$continuationPass"
                )
                runLegacyGeminiGenerateContent(effectiveProfile, fbNodes, fbPrefix)
            } else {
                throw e
            }
        }
    }

    private suspend fun runLegacyGeminiGenerateContent(
        effectiveProfile: ProviderProfile,
        targetHistoryNodes: List<MessageNode>,
        targetContinuePrefix: String
    ) {
        with(engine) {
            executeGeminiGenerateContentLoop(
                profile = effectiveProfile,
                model = model,
                session = session,
                historyNodes = targetHistoryNodes,
                continuePrefix = targetContinuePrefix,
                circuitBreaker = circuitBreaker,
                onStatus = { statusMsg -> state.emitStatus(statusMsg) },
                onGoalProgress = state::emitGoalProgress,
                onFirstToken = state::recordTokenActivity,
                onThinking = state::handleSignedThinking,
                onContent = state::handleContentDelta,
                onMedia = state::recordGeneratedMedia,
                onOutputLimit = state::markOutputLimitReached,
                onToolCall = state::upsertToolCallAndEmit,
                onRawWire = state::recordRawWireBounded,
                onRequestStarted = state::beginUsageRequest,
                onUsage = state::recordRequestUsage,
                onCitation = state::recordRawCitationAndEmit
            )
        }
    }

    private suspend fun runOpenAiResponsesPass(
        effectiveProfile: ProviderProfile,
        effectiveHistoryNodes: List<MessageNode>,
        effectiveContinuePrefix: String
    ) {
        val modalityProfile = engine.resolveModelModalityProfileAsync(effectiveProfile, model)
        val isExplicitVideoPath = profile.endpointPathOverride.contains("contents/generations/tasks", ignoreCase = true) ||
            profile.endpointPathOverride.contains("videos/generations", ignoreCase = true) ||
            profile.endpointPathOverride.contains("video/generations", ignoreCase = true)
        val isDirectImagesEndpoint = profile.endpointPathOverride.contains("images/generations", ignoreCase = true) ||
            profile.endpointPathOverride.contains("images/edits", ignoreCase = true) ||
            modalityProfile.isDedicatedImage
        val isDirectAudioEndpoint = profile.endpointPathOverride.contains("audio/speech", ignoreCase = true) ||
            modalityProfile.isDedicatedAudioTts

        val customProfile = profile.protocol == ProviderProtocol.CUSTOM
        val explicitChat = customProfile && profile.customDialect.trim().lowercase() == CUSTOM_DIALECT_OPENAI_CHAT

        when {
            explicitChat ->
                runLegacyOpenAiChatCompletions(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            !customProfile && (modalityProfile.isDedicatedVideo || isExplicitVideoPath) ->
                runMediaEndpointWithFallback(
                    effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix, rethrowNoRetry = true
                ) { runUniversalMediaGeneration(forceVideo = true) }
            !customProfile && isDirectImagesEndpoint ->
                runMediaEndpointWithFallback(
                    effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix, rethrowNoRetry = false
                ) { runUniversalMediaGeneration(forceVideo = false) }
            !customProfile && isDirectAudioEndpoint ->
                runMediaEndpointWithFallback(
                    effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix, rethrowNoRetry = false
                ) { runUniversalAudioSpeechGeneration() }
            // Try /v1/responses first. If it fails (or, with useLegacyFallbackEndpoint on, its
            // quick retries run out), hand the thoughts, tool calls and text so far to
            // /v1/chat/completions.
            emptyAnswerNudgePass >= 3 && effectiveProfile.useLegacyFallbackEndpoint ->
                runLegacyOpenAiChatCompletions(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            else -> {
                try {
                    with(engine) {
                        executeOpenAiResponsesLoop(
                            profile = effectiveProfile,
                            model = model,
                            session = session,
                            historyNodes = effectiveHistoryNodes,
                            continuePrefix = effectiveContinuePrefix,
                            circuitBreaker = circuitBreaker,
                            onStatus = { st -> state.emitStatus(st) },
                            onGoalProgress = state::emitGoalProgress,
                            onFirstToken = state::recordTokenActivity,
                            onThinking = state::handleSignedThinking,
                            onContent = state::handleContentDelta,
                            onMedia = state::recordGeneratedMedia,
                            onOutputLimit = state::markOutputLimitReached,
                            onToolCall = state::upsertToolCallAndEmit,
                            onRawWire = state::recordRawWireBounded,
                            onRequestStarted = state::beginUsageRequest,
                            onUsage = state::recordRequestUsage,
                            onCitation = state::recordRawCitationAndEmit
                        )
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Throwable) {
                    // A Custom profile declares its URL and dialect, so no failure may reroute it.
                    if (profile.protocol == ProviderProtocol.CUSTOM) throw e
                    if (tryRedirectMediaFromError(e, fallbackInterleavedImage = false, announceRouting = true)) {
                        return
                    }
                    if (!engine.shouldFallbackToLegacyProtocol(e, profile.useLegacyFallbackEndpoint)) {
                        throw e
                    }
                    emitFallbackStatus("Primary /v1/responses failed — falling back to /v1/chat/completions…", e)
                    val (fbNodes, fbPrefix) = state.buildPassHistoryAndPrefix(
                        baseNodes = activeBaseHistoryNodes,
                        continuePrefix = state.currentContent(),
                        protocol = profile.protocol,
                        passTag = "fallback-openai-$continuationPass"
                    )
                    try {
                        runLegacyOpenAiChatCompletions(effectiveProfile, fbNodes, fbPrefix)
                    } catch (chatErr: Throwable) {
                        if (chatErr is CancellationException) throw chatErr
                        if (!tryRedirectMediaFromError(
                                chatErr,
                                fallbackInterleavedImage = modalityProfile.isInterleavedImage,
                                announceRouting = false
                            )
                        ) {
                            throw chatErr
                        }
                    }
                }
            }
        }
    }

    private suspend fun runMediaEndpointWithFallback(
        effectiveProfile: ProviderProfile,
        effectiveHistoryNodes: List<MessageNode>,
        effectiveContinuePrefix: String,
        rethrowNoRetry: Boolean,
        block: suspend () -> Unit
    ) {
        try {
            block()
        } catch (err: Throwable) {
            if (err is CancellationException || (rethrowNoRetry && err is NoRetryException)) throw err
            if (state.hasNoMediaOrTextYet && profile.useLegacyFallbackEndpoint) {
                runLegacyOpenAiChatCompletions(effectiveProfile, effectiveHistoryNodes, effectiveContinuePrefix)
            } else {
                throw err
            }
        }
    }

    private suspend fun tryRedirectMediaFromError(
        error: Throwable,
        fallbackInterleavedImage: Boolean,
        announceRouting: Boolean
    ): Boolean {
        val hint = engine.detectMediaEndpointRedirectFromError(error)
        val allowBlindImageFallback = fallbackInterleavedImage &&
            state.hasNoMediaOrTextYet &&
            !engine.isHardAuthOrQuotaError(error) &&
            !engine.isTransientOrProxyTimeoutError(error)
        return when {
            hint == "image" || allowBlindImageFallback -> {
                engine.recordDiscoveredModality(model, listOf("image"))
                if (announceRouting) state.emitStatus("Model requires Image Generation endpoint — routing automatically…")
                runUniversalMediaGeneration(forceVideo = false)
                true
            }
            hint == "video" -> {
                engine.recordDiscoveredModality(model, listOf("video"))
                if (announceRouting) state.emitStatus("Model requires Video Generation endpoint — routing automatically…")
                runUniversalMediaGeneration(forceVideo = true)
                true
            }
            hint == "audio" -> {
                engine.recordDiscoveredModality(model, listOf("audio"))
                if (announceRouting) state.emitStatus("Model requires Audio Speech endpoint — routing automatically…")
                runUniversalAudioSpeechGeneration()
                true
            }
            else -> false
        }
    }

    private suspend fun runLegacyOpenAiChatCompletions(
        effectiveProfile: ProviderProfile,
        targetHistoryNodes: List<MessageNode>,
        targetContinuePrefix: String
    ) {
        with(engine) {
            executeOpenAiChatCompletionsLoop(
                profile = effectiveProfile,
                model = model,
                session = session,
                historyNodes = targetHistoryNodes,
                continuePrefix = targetContinuePrefix,
                circuitBreaker = circuitBreaker,
                onStatus = { st -> state.emitStatus(st, includeInteractionId = false) },
                onGoalProgress = state::emitGoalProgress,
                onFirstToken = state::recordTokenActivity,
                onThinking = state::handleSimpleThinking,
                onContent = state::handleContentDelta,
                onMedia = state::recordGeneratedMedia,
                onOutputLimit = state::markOutputLimitReached,
                onToolCall = state::upsertToolCallAndEmit,
                onRawWire = state::recordRawWireBounded,
                onRequestStarted = state::beginUsageRequest,
                onUsage = state::recordRequestUsage,
                onCitation = state::recordRawCitationAndEmit
            )
        }
    }

    private suspend fun runUniversalMediaGeneration(forceVideo: Boolean) {
        with(engine) {
            executeByteDanceArkGeneration(
                resumeTask = resumeMediaTask,
                profile = profile,
                model = model,
                session = session,
                historyNodes = historyNodes,
                circuitBreaker = circuitBreaker,
                forceVideoMode = forceVideo,
                onStatus = { st, id -> state.emitStatus(st, id) },
                onFirstToken = state::recordTokenActivity,
                onThinking = state::handleMediaThinking,
                onMedia = state::recordGeneratedMedia,
                onContent = state::handleMediaContentDelta,
                onRawWire = state::recordRawWireBounded,
                onRequestStarted = state::beginUsageRequest,
                onUsage = state::recordRequestUsage,
                onMediaRequestStarted = state::beginMediaUsage
            )
        }
    }

    private suspend fun runUniversalAudioSpeechGeneration() {
        with(engine) {
            executeOpenAiAudioSpeechEndpoint(
                profile = profile,
                model = model,
                session = session,
                historyNodes = historyNodes,
                circuitBreaker = circuitBreaker,
                onStatus = { st -> state.emitStatus(st) },
                onFirstToken = state::recordTokenActivity,
                onMedia = state::recordGeneratedMedia,
                onContent = state::handleMediaContentDelta,
                onRawWire = state::recordRawWireBounded,
                onRequestStarted = {
                    state.beginMediaUsage("openai_audio_speech", "output_audio")
                }
            )
        }
    }

    private suspend fun runAnthropicPass(
        effectiveProfile: ProviderProfile,
        effectiveHistoryNodes: List<MessageNode>,
        effectiveContinuePrefix: String
    ) {
        with(engine) {
            executeAnthropicMessagesLoop(
                profile = effectiveProfile,
                model = model,
                session = session,
                historyNodes = effectiveHistoryNodes,
                continuePrefix = effectiveContinuePrefix,
                circuitBreaker = circuitBreaker,
                onStatus = { st -> state.emitStatus(st) },
                onGoalProgress = state::emitGoalProgress,
                onFirstToken = state::recordTokenActivity,
                onThinking = state::handleSignedThinking,
                onContent = state::handleContentDelta,
                onOutputLimit = state::markOutputLimitReached,
                onToolCall = state::upsertToolCallAndEmit,
                onRawWire = state::recordRawWireBounded,
                onRequestStarted = state::beginUsageRequest,
                onUsage = state::recordRequestUsage,
                onCitation = state::recordRawCitationAndEmit
            )
        }
    }

    private suspend fun emitFallbackStatus(heading: String, error: Throwable) {
        val actualCause = if (error is StepRetriesExhaustedException && error.cause != null) error.cause!! else error
        val reasonClean = engine.extractCleanApiErrorSummary(actualCause.message ?: "endpoint error")
        state.emitStatus("$heading\n$reasonClean")
    }
}
