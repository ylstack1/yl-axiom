package io.parchment.ai.network

import io.ktor.client.request.header
import io.ktor.client.request.post
import io.ktor.client.request.preparePost
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsChannel
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.contentType
import io.ktor.http.isSuccess
import io.parchment.ai.harness.ToolAccessScope
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.CUSTOM_DIALECT_GEMINI
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.ToolApprovalMode
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.keepsProviderState
import io.parchment.ai.network.AiProviderEngine.RawWebCitation
import io.parchment.ai.network.AiProviderEngine.StepRetriesExhaustedException
import io.parchment.ai.protocol.sanitizeGeminiThoughtSignature
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

// Gemini Interactions: background job cancellation and multi-step streaming agent loop.

/** Cancels a server-side background interaction on Google Gemini Interactions API. */
suspend fun AiProviderEngine.cancelBackgroundInteraction(profile: ProviderProfile, interactionId: String): Result<String> = runCatching {
    if (interactionId.isBlank()) return@runCatching "No interaction ID to cancel."
    // Only a stored Gemini interaction exists to cancel; other wires have no such endpoint.
    if (!profile.keepsProviderState || resolveWireProtocol(profile) != ProviderProtocol.GEMINI_INTERACTIONS) {
        return@runCatching "Nothing stored to cancel."
    }
    val rawUrl = resolveSmartEndpointUrl(
        profile = profile,
        defaultPathWithV1 = "/v1beta/interactions",
        defaultPathWithoutVersion = "/interactions"
    )
    val url = buildUrlWithAuthQuery(profile, "${rawUrl.substringBefore("?").trimEnd('#').trimEnd('/')}/$interactionId/cancel")
    val response = httpClient.post(url) {
        applyAuthAndCustomHeaders(profile)
        contentType(ContentType.Application.Json)
        setBody("{}")
    }
    response.bodyAsText()
}

// ============================================================================================
// 1. Gemini Interactions API (/v1beta/interactions)
//    Supports Gemini 4 Argon Long Decode, SSE Streaming, Background=true Polling,
//    previous_interaction_id stateful chaining, Built-in Tools & Local Workspace Harness.
// ============================================================================================
internal suspend fun AiProviderEngine.executeGeminiInteractionsLoop(
    profile: ProviderProfile,
    model: String,
    session: ChatSession,
    historyNodes: List<MessageNode>,
    continuePrefix: String,
    circuitBreaker: WorkspaceHarness.LoopCircuitBreaker,
    onStatus: suspend (String, String) -> Unit,
    onGoalProgress: suspend (GoalTrackerState) -> Unit,
    onFirstToken: () -> Unit,
    onThinking: suspend (String, String) -> Unit,
    onContent: suspend (String) -> Unit,
    onMedia: suspend (GeneratedMediaItem) -> Unit,
    onToolCall: suspend (ToolCallRecord) -> Unit,
    onOutputLimit: suspend () -> Unit = {},
    onRawWire: suspend (String, String) -> Unit,
    onRequestStarted: UsageRequestStarter,
    onUsage: UsageReporter,
    onCitation: suspend (RawWebCitation) -> Unit = {},
    pollDelayMsOverride: Long? = null
) {
    GeminiInteractionsSession(
        engine = this,
        profile = profile,
        model = model,
        session = session,
        historyNodes = historyNodes,
        continuePrefix = continuePrefix,
        circuitBreaker = circuitBreaker,
        onStatus = onStatus,
        onGoalProgress = onGoalProgress,
        onFirstToken = onFirstToken,
        onThinking = onThinking,
        onContent = onContent,
        onMedia = onMedia,
        onToolCall = onToolCall,
        onOutputLimit = onOutputLimit,
        onRawWire = onRawWire,
        onRequestStarted = onRequestStarted,
        onUsage = onUsage,
        onCitation = onCitation,
        pollDelayMsOverride = pollDelayMsOverride
    ).run()
}

private enum class AttemptRecoveryAction { RETRY_ATTEMPT, BREAK_AFTER_POLL }

private class GeminiInteractionsSession(
    private val engine: AiProviderEngine,
    private val profile: ProviderProfile,
    private val model: String,
    private val session: ChatSession,
    historyNodes: List<MessageNode>,
    private val continuePrefix: String,
    private val circuitBreaker: WorkspaceHarness.LoopCircuitBreaker,
    private val onStatus: suspend (String, String) -> Unit,
    private val onGoalProgress: suspend (GoalTrackerState) -> Unit,
    onFirstToken: () -> Unit,
    onThinking: suspend (String, String) -> Unit,
    onContent: suspend (String) -> Unit,
    onMedia: suspend (GeneratedMediaItem) -> Unit,
    private val onToolCall: suspend (ToolCallRecord) -> Unit,
    onOutputLimit: suspend () -> Unit,
    private val onRawWire: suspend (String, String) -> Unit,
    private val onRequestStarted: UsageRequestStarter,
    onUsage: UsageReporter,
    onCitation: suspend (RawWebCitation) -> Unit,
    pollDelayMsOverride: Long? = null
) {
    private var usageRequestId: String? = null
    private val activeNodes = engine.sanitizeContextNodes(historyNodes)
    private val systemInstruction = engine.buildCompiledSystemPrompt(session, circuitBreaker.goalState)
    private val toolPolicy = AgentPermissionResolver.resolve(session, profile = profile)

    // Check if we can use stateful previous_interaction_id chaining
    private val previousAssistantIdx = activeNodes.indexOfLast { it.role == MessageRole.ASSISTANT }
    private var currentPrevInteractionId = if (profile.useStatefulChaining && continuePrefix.isBlank() && previousAssistantIdx >= 0) {
        // Chaining omits local history, so a chain from another model would leave the new one with nothing.
        engine.reusableProviderChainId(
            activeNodes[previousAssistantIdx].activeVariant,
            ProviderProtocol.GEMINI_INTERACTIONS,
            model,
            CUSTOM_DIALECT_GEMINI
        )
    } else ""
    // ID minted during the active round; only promoted after a round settles so retries never poll a prior turn.
    private var currentRoundInteractionId = ""
    private var currentContinuationToken = ""
    private var pendingContinuationHop = false

    private var omitBackgroundOnRetry = false
    private var stripSamplingOnRetry = false
    private var upgradeMinimalToLowOnRetry = false
    private var downgradeXhighToHighOnRetry = false
    private var simplifyThinkingOnRetry = false
    private var omitResponseFormatOnRetry = false
    private var forceImageResponseFormatOnRetry = false
    private var omitBuiltInToolsOnRetry = toolPolicy.approvalMode != ToolApprovalMode.NEVER
    private var foldHistoricalToolsOnRetry = false

    private val hasHistoricalToolsOrSigs = activeNodes.any {
        it.role == MessageRole.ASSISTANT && (it.activeVariant.toolCalls.isNotEmpty() || it.activeVariant.thinkingSignature.isNotBlank())
    }
    private val extraLoopSteps = mutableListOf<JsonObject>()
    private var pendingToolResults: List<ToolCallRecord> = emptyList()
    private var pendingStopHookPrompt = ""
    private var loopStep = 0
    private val modelOutputsImages = engine.resolveKnownOutputModalities(profile, model).contains("image")

    private val round = GeminiInteractionsRoundParser(
        engine = engine,
        profile = profile,
        usageRequestIdProvider = { usageRequestId },
        onStatus = onStatus,
        onFirstToken = onFirstToken,
        onThinking = onThinking,
        onContent = onContent,
        onMedia = onMedia,
        onOutputLimit = onOutputLimit,
        onRawWire = onRawWire,
        onUsage = onUsage,
        onCitation = onCitation,
        onInteractionIdObserved = { id -> currentRoundInteractionId = id },
        currentInteractionId = { currentRoundInteractionId.ifBlank { currentPrevInteractionId } },
        pollDelayMsOverride = pollDelayMsOverride
    )

    suspend fun run() {
        while (loopStep < AiProviderEngine.MAX_SAFETY_HORIZON_STEPS) {
            loopStep++
            var stepRetryAttempt = 0

            while (true) {
                round.reset()
                currentRoundInteractionId = ""

                val isContinuationHop = pendingContinuationHop &&
                    currentPrevInteractionId.isNotBlank() &&
                    currentContinuationToken.isNotBlank()
                val useChainedId = isContinuationHop || (profile.useStatefulChaining && currentPrevInteractionId.isNotBlank())
                val requestJsonStr = buildRequestJson(useChainedId, isContinuationHop)

                val rawUrl = engine.resolveSmartEndpointUrl(
                    profile = profile,
                    defaultPathWithV1 = "/v1beta/interactions",
                    defaultPathWithoutVersion = "/interactions"
                )
                val pollBasePath = rawUrl.substringBefore("?").trimEnd('#').trimEnd('/')
                val finalUrl = engine.buildUrlWithAuthQuery(profile, rawUrl)

                onRawWire(requestJsonStr, "Dispatching to $finalUrl ...")

                try {
                    executeStreamingRequest(finalUrl, requestJsonStr, pollBasePath)
                    round.flushPendingFunctionCalls()
                    break
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Throwable) {
                    val action = handleAttemptError(
                        error = e,
                        useChainedId = useChainedId,
                        stepRetryAttemptProvider = { ++stepRetryAttempt },
                        requestJsonStr = requestJsonStr,
                        pollBasePath = pollBasePath
                    )
                    if (action == AttemptRecoveryAction.BREAK_AFTER_POLL) break
                }
            }

            // Official Long-Decode Continuation Hop (`status == "requires_continuation"`)
            if (round.roundStatus == "requires_continuation" && round.roundContinuationToken.isNotBlank()) {
                val hopAnchor = currentRoundInteractionId.ifBlank { currentPrevInteractionId }
                if (hopAnchor.isNotBlank()) {
                    if (!profile.useStatefulChaining) {
                        round.appendContinuationOutputSteps(extraLoopSteps)
                    }
                    currentPrevInteractionId = hopAnchor
                    currentContinuationToken = round.roundContinuationToken
                    pendingContinuationHop = true
                    pendingToolResults = emptyList()
                    pendingStopHookPrompt = ""
                    onStatus(Strings.longDecodeHopCompletedContinuingVia, hopAnchor)
                    continue
                }
            }
            currentContinuationToken = ""
            pendingContinuationHop = false
            if (profile.useStatefulChaining && currentRoundInteractionId.isNotBlank()) {
                currentPrevInteractionId = currentRoundInteractionId
            } else if (!profile.useStatefulChaining) {
                currentPrevInteractionId = ""
            }

            if (round.turnToolCalls.isEmpty()) {
                val stopHookPrompt = circuitBreaker.evaluateStopHookPrompt()
                if (stopHookPrompt != null) {
                    pendingToolResults = emptyList()
                    pendingStopHookPrompt = stopHookPrompt
                    round.appendStopHookSteps(extraLoopSteps, stopHookPrompt)
                    onStatus(Strings.goalWorkCheckContinuingIncompleteChecklist, currentRoundInteractionId.ifBlank { currentPrevInteractionId })
                    continue
                }
                break
            }
            pendingStopHookPrompt = ""

            val stepSigString = sanitizeGeminiThoughtSignature(round.roundThoughtSig.toString(), forFunctionCall = true)
            val stepThoughtTxt = round.roundThoughtText.toString().trim()
            val results = with(engine) {
                executeWorkspaceToolBatchWithLiveProgress(
                    workspaceRoot = session.workspacePath,
                    mode = session.workspaceMode,
                    allowShell = toolPolicy.allowShell,
                    accessScope = ToolAccessScope(
                        toolPolicy,
                        goalModeActive = circuitBreaker.goalState != null,
                        includeLocalWebSearchTool = profile.enableWebSearch && omitBuiltInToolsOnRetry,
                        expandedMcpTools = currentExpandedMcpTools()
                    ),
                    calls = round.turnToolCalls.map { (callId, fnPair) -> Triple(callId, fnPair.first, fnPair.second) },
                    firstThoughtSignature = stepSigString,
                    firstPrecedingThinking = stepThoughtTxt,
                    circuitBreaker = circuitBreaker,
                    onStatus = { st -> onStatus(st, currentRoundInteractionId.ifBlank { currentPrevInteractionId }) },
                    onToolCall = onToolCall,
                    onGoalProgress = onGoalProgress
                )
            }
            round.appendToolRoundSteps(extraLoopSteps, stepSigString, results)
            pendingToolResults = results
            val trippedReason = circuitBreaker.checkTripped()
            if (trippedReason != null) {
                onStatus(trippedReason, currentRoundInteractionId.ifBlank { currentPrevInteractionId })
                break
            }
        }
    }

    private suspend fun buildRequestJson(useChainedId: Boolean, isContinuationHop: Boolean): String {
        val sourceNodes = if (useChainedId && previousAssistantIdx >= 0 && previousAssistantIdx < activeNodes.size - 1) {
            activeNodes.subList(previousAssistantIdx + 1, activeNodes.size)
        } else {
            activeNodes
        }
        val baseInputSteps = withBinaryAttachmentIo(sourceNodes) {
            engine.buildGeminiInteractionsTurnsArray(sourceNodes, session, continuePrefix, foldHistoricalToolsOnRetry, currentModel = model)
        }

        // 3-Layer Rolling Context Compaction: prune verbose tool outputs older than 8 steps in-place in RAM
        val compactedExtraSteps = compactGeminiInteractionsExtraSteps(extraLoopSteps)
        if (compactedExtraSteps !== extraLoopSteps) {
            extraLoopSteps.clear()
            extraLoopSteps.addAll(compactedExtraSteps)
        }

        val payloadObj = buildJsonObject {
            put("model", model.trim().removePrefix("models/"))
            if (systemInstruction.isNotBlank()) {
                put("system_instruction", systemInstruction)
            }
            if (!profile.keepsProviderState) {
                put("store", false)
            } else if (profile.useBackgroundLongDecode && !omitBackgroundOnRetry) {
                put("background", true)
            }
            // Always stream. With `background = true`, a dropped stream falls back to polling `/interactions/{id}`.
            put("stream", true)
            if (useChainedId) {
                put("previous_interaction_id", currentPrevInteractionId)
            }
            if (isContinuationHop) {
                put("continuation_token", currentContinuationToken)
            }

            // Official polymorphic `response_format` array when model outputs images
            if ((modelOutputsImages || forceImageResponseFormatOnRetry) && !omitResponseFormatOnRetry) {
                putJsonArray("response_format") {
                    add(buildJsonObject { put("type", "text") })
                    add(buildJsonObject {
                        put("type", "image")
                        if (profile.mediaAspectRatio.isNotBlank()) {
                            put("aspect_ratio", profile.mediaAspectRatio)
                        }
                        if (profile.mediaResolution in listOf("512", "1K", "2K", "4K")) {
                            put("image_size", profile.mediaResolution)
                        }
                    })
                }
            }

            // Generation config (self-heals sampling & thinking_level dynamically on HTTP 400)
            putJsonObject("generation_config") {
                if (!stripSamplingOnRetry) {
                    profile.temperature?.let { put("temperature", it) }
                    profile.topP?.let { put("top_p", it) }
                }
                val maxOut = profile.maxOutputTokens
                if (maxOut != null && maxOut > 0) {
                    put("max_output_tokens", maxOut)
                }
                val rawEffort = profile.thinkingEffort.trim()
                val lowerEffort = rawEffort.lowercase()
                if (lowerEffort != "none" && lowerEffort != "off") {
                    put("thinking_summaries", "auto")
                    if (!simplifyThinkingOnRetry && rawEffort.isNotEmpty() && lowerEffort != "default" && lowerEffort != "auto") {
                        val mappedLevel = when (lowerEffort) {
                            "minimal" -> if (upgradeMinimalToLowOnRetry) "low" else "minimal"
                            "low" -> "low"
                            "medium" -> "medium"
                            "high" -> "high"
                            "xhigh", "max" -> if (downgradeXhighToHighOnRetry) "high" else "xhigh"
                            else -> lowerEffort
                        }
                        put("thinking_level", mappedLevel)
                    }
                }
            }

            // Built-in Tools + Local Workspace Harness Tools before `input` so static prefix stays cacheable
            val expandedMcpTools = currentExpandedMcpTools()
            val workspaceTools = WorkspaceHarness.buildGeminiInteractionsTools(
                session.workspaceMode,
                session.workspacePath,
                toolPolicy.allowShell,
                goalModeActive = circuitBreaker.goalState != null,
                enableWebTools = (profile.enableWebSearch && omitBuiltInToolsOnRetry) || profile.enableUrlContext,
                includeLocalWebSearchTool = profile.enableWebSearch && omitBuiltInToolsOnRetry,
                policy = toolPolicy,
                expandedMcpTools = expandedMcpTools
            )
            val toolsArray = buildJsonArray {
                if (!omitBuiltInToolsOnRetry || workspaceTools.isEmpty()) {
                    if (profile.enableWebSearch) add(buildJsonObject { put("type", "google_search") })
                    if (profile.enableCodeExecution) add(buildJsonObject { put("type", "code_execution") })
                    if (profile.enableUrlContext) add(buildJsonObject { put("type", "url_context") })
                }
                workspaceTools.forEach { add(it) }
            }
            if (toolsArray.isNotEmpty()) {
                put("tools", toolsArray)
            }

            // Build `input` using official Interactions API Steps schema (omitted when continuing a foreground hop via continuation_token)
            if (!isContinuationHop) {
                when {
                    pendingToolResults.isNotEmpty() && useChainedId -> putJsonArray("input") {
                        for (tr in pendingToolResults) {
                            add(geminiFunctionResultStep(callId = tr.id, name = tr.name, isError = tr.isError, resultText = tr.resultText))
                        }
                        pendingToolResults.filter { it.hasScreenshot }.forEach { add(interactionsScreenshotStep(it)) }
                    }
                    pendingStopHookPrompt.isNotBlank() && useChainedId -> putJsonArray("input") {
                        add(geminiUserInputStep(pendingStopHookPrompt))
                    }
                    else -> putJsonArray("input") {
                        baseInputSteps.forEach { add(it) }
                        compactedExtraSteps.forEach { add(it) }
                    }
                }
            }
        }

        val finalPayload = engine.mergeRawJsonExtraBody(payloadObj, profile.rawJsonExtraBody)
        return engine.json.encodeToString(JsonObject.serializer(), finalPayload)
    }

    private suspend fun executeStreamingRequest(finalUrl: String, requestJsonStr: String, pollBasePath: String) {
        // Live SSE Streaming Mode (`stream = true`) with automatic Background Poll Recovery
        engine.ensureAgentPolicyCurrent(session, profile)
        usageRequestId = onRequestStarted(UsageDialect.GEMINI_INTERACTIONS)
        engine.httpClient.preparePost(finalUrl) {
            with(engine) { applyAuthAndCustomHeaders(profile) }
            header("Accept", "text/event-stream")
            contentType(ContentType.Application.Json)
            setBody(requestJsonStr)
        }.execute { response ->
            if (!response.status.isSuccess()) {
                val errText = response.bodyAsText()
                onRawWire(requestJsonStr, errText)
                throw providerHttpError("Gemini Interactions", response, errText)
            }

            engine.readSseOrJsonFallback(
                streamFinished = { round.streamFinished },
                channel = response.bodyAsChannel(),
                onSseEvent = { eventType, dataLine -> round.consumeSseEvent(eventType, dataLine, requestJsonStr) },
                onPlainJsonFallback = { fullBody -> round.handlePlainJsonFallback(fullBody, pollBasePath, requestJsonStr) }
            )
        }
    }

    private suspend fun handleAttemptError(
        error: Throwable,
        useChainedId: Boolean,
        stepRetryAttemptProvider: () -> Int,
        requestJsonStr: String,
        pollBasePath: String
    ): AttemptRecoveryAction {
        val errMsg = (error.message ?: error.toString()).lowercase()
        if (!round.receivedAnyStreamContent && errMsg.contains("http 400") && tryHealHttp400(errMsg, useChainedId)) {
            return AttemptRecoveryAction.RETRY_ATTEMPT
        }
        if (useChainedId && !round.receivedAnyStreamContent &&
            (errMsg.contains("http 404") || errMsg.contains("previous_interaction_id") ||
                errMsg.contains("continuation_token") || errMsg.contains("function response turn") || errMsg.contains("function_result"))
        ) {
            currentPrevInteractionId = ""
            currentContinuationToken = ""
            pendingContinuationHop = false
            onStatus(Strings.statefulInteractionChainRejectedRetryingStep, "")
            return AttemptRecoveryAction.RETRY_ATTEMPT
        }
        if (error !is TerminalInteractionFailedException &&
            profile.keepsProviderState &&
            profile.useBackgroundLongDecode &&
            currentRoundInteractionId.isNotBlank() &&
            engine.isTransientOrProxyTimeoutError(error)
        ) {
            round.pollUntilComplete(currentRoundInteractionId, pollBasePath, requestJsonStr)
            return AttemptRecoveryAction.BREAK_AFTER_POLL
        }
        if (!round.receivedAnyStreamContent && round.turnToolCalls.isEmpty() && engine.isTransientOrProxyTimeoutError(error)) {
            val maxPrimaryRetries = if (profile.useLegacyFallbackEndpoint) {
                AiProviderEngine.LEGACY_FALLBACK_PRIMARY_MAX_RETRIES
            } else {
                AiProviderEngine.MAX_PRE_TOKEN_RETRIES
            }
            // The legacy-endpoint probe waits briefly before handing over; a server's own hint still wins.
            val probeWaitMs = if (profile.useLegacyFallbackEndpoint) AiProviderEngine.LEGACY_FALLBACK_PRIMARY_WAIT_SECONDS * 1000L else null
            val attempt = stepRetryAttemptProvider()
            if (attempt <= maxPrimaryRetries) {
                engine.waitWithRetryCountdown(
                    attempt = attempt,
                    maxAttempts = maxPrimaryRetries,
                    waitMs = retryDelayMs(attempt, error, fixedMs = probeWaitMs),
                    reasonSummary = error.message ?: "Transient API error in step $loopStep",
                    onStatus = { msg -> onStatus(msg, currentRoundInteractionId) }
                )
                return AttemptRecoveryAction.RETRY_ATTEMPT
            } else {
                throw StepRetriesExhaustedException(error.message ?: "Retries exhausted", error)
            }
        }
        throw error
    }

    private suspend fun tryHealHttp400(errMsg: String, useChainedId: Boolean): Boolean {
        if (useChainedId) {
            currentPrevInteractionId = ""
            currentContinuationToken = ""
            pendingContinuationHop = false
            onStatus(Strings.statefulInteractionChainRejectedRetryingStep, "")
            return true
        }
        if (!omitBackgroundOnRetry && profile.keepsProviderState && profile.useBackgroundLongDecode &&
            errMsg.contains("background")
        ) {
            omitBackgroundOnRetry = true
            onStatus(Strings.endpointRejectedBackgroundFlagRetryingWithout, "")
            return true
        }
        if (!stripSamplingOnRetry && (errMsg.contains("temperature") || errMsg.contains("top_p") || errMsg.contains("sampling"))) {
            stripSamplingOnRetry = true
            onStatus(Strings.endpointRejectedSamplingParametersRetryingWith, "")
            return true
        }
        if (!upgradeMinimalToLowOnRetry && profile.thinkingEffort.equals("minimal", ignoreCase = true) &&
            (errMsg.contains("minimal") || errMsg.contains("thinking_level") || errMsg.contains("thinking"))
        ) {
            upgradeMinimalToLowOnRetry = true
            onStatus(Strings.modelDoesNotSupportMinimalThinking, "")
            return true
        }
        val lowerEffort = profile.thinkingEffort.trim().lowercase()
        if (!downgradeXhighToHighOnRetry && (lowerEffort == "xhigh" || lowerEffort == "max") &&
            (errMsg.contains("xhigh") || errMsg.contains("thinking_level") || errMsg.contains("thinking"))
        ) {
            downgradeXhighToHighOnRetry = true
            onStatus(Strings.endpointRejectedXhighThinkingRetryingWith, "")
            return true
        }
        if (!simplifyThinkingOnRetry && (errMsg.contains("thinking") || errMsg.contains("thinking_level"))) {
            simplifyThinkingOnRetry = true
            onStatus(Strings.endpointRejectedThinkingLevelRetryingWith, "")
            return true
        }
        if (!forceImageResponseFormatOnRetry && !omitResponseFormatOnRetry &&
            (errMsg.contains("only supports image") || (errMsg.contains("response_format") && errMsg.contains("image")))
        ) {
            forceImageResponseFormatOnRetry = true
            engine.recordDiscoveredModality(model, listOf("text", "image"))
            onStatus(Strings.modelOutputsImagesEnablingImageResponse, "")
            return true
        }
        if (!omitResponseFormatOnRetry && (errMsg.contains("response_format") || errMsg.contains("image_size") || errMsg.contains("aspect_ratio"))) {
            omitResponseFormatOnRetry = true
            onStatus(Strings.endpointRejectedResponseFormatRetryingWithout, "")
            return true
        }
        if (!omitBuiltInToolsOnRetry && (errMsg.contains("google_search") || errMsg.contains("url_context") ||
                errMsg.contains("built-in") || errMsg.contains("multiple tools"))
        ) {
            omitBuiltInToolsOnRetry = true
            onStatus(Strings.endpointRejectedMixedBuiltInFunction, "")
            return true
        }
        if (!foldHistoricalToolsOnRetry && hasHistoricalToolsOrSigs &&
            (errMsg.contains("couldn't process") || errMsg.contains("invalid_request") || errMsg.contains("thought") ||
                errMsg.contains("signature") || errMsg.contains("function_call") || errMsg.contains("tool type"))
        ) {
            foldHistoricalToolsOnRetry = true
            onStatus(Strings.endpointRejectedHistoricalToolSignatureContext, "")
            return true
        }
        return false
    }
}
