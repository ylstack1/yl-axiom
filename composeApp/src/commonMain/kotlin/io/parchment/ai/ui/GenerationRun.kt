package io.parchment.ai.ui

import io.parchment.ai.harness.wasCutOff
import io.parchment.ai.strings.Strings
import io.parchment.ai.harness.withInterruptedOutcome
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.key
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.harness.AgentToolRunContext
import io.parchment.ai.network.appendSignatureChunkSafe
import io.parchment.ai.network.stripInlineErrorNotices
import io.parchment.ai.runtime.appRunLeases
import io.parchment.ai.harness.ToolApprovalBroker
import io.parchment.ai.harness.ToolAccessScope
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.asOverrides
import io.parchment.ai.model.unclaimedStepText
import io.parchment.ai.network.AiProviderEngine
import io.parchment.ai.network.RetryControl
import io.parchment.ai.network.StreamUpdate
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.PlatformVoice
import io.parchment.ai.protocol.O200kTokenizer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/** How often a streaming run saves the whole chat; checkpoints in between save the reply alone. */
private const val FULL_CHECKPOINT_INTERVAL_MS = 30_000L

internal fun GenerationHost.beginGeneration(
    sessionId: String,
    historyForCall: List<MessageNode>,
    targetAssistantNodeId: String,
    targetVariantId: String,
    continuePrefix: String = "",
    autoMuteNodeIdsOnFinish: Set<String> = emptySet()
) {
    val sessionSnap = appState.sessions.find { it.id == sessionId } ?: return
    if (sessionSnap.isCompactStub) {
        bannerNotification = Strings.chatHistoryIsStillLoadingTry
        return
    }
    if (sessionSnap.messages.none { node -> node.id == targetAssistantNodeId && node.variants.any { it.id == targetVariantId } }) {
        bannerNotification = Strings.theRequestedTurnIsNoLonger
        return
    }
    val assistantSnap = appState.assistants.find { it.id == sessionSnap.assistantId } ?: activeAssistant
    val profileSnap = appState.profiles.find { it.id == sessionSnap.profileId } ?: activeProfile

    // Merge Assistant-level system prompt, fragments, and nullable sampling overrides
    val mergedSystemPrompt = buildString {
        if (assistantSnap.systemPrompt.isNotBlank()) {
            appendLine(assistantSnap.systemPrompt.trim())
        }
        if (sessionSnap.systemPrompt.isNotBlank()) {
            if (isNotEmpty()) appendLine()
            appendLine(sessionSnap.systemPrompt.trim())
        }
    }.trim()

    val toolPolicy = AgentPermissionResolver.resolve(sessionSnap, assistantSnap, profileSnap)

    val mergedSessionForApi = sessionSnap.copy(
        systemPrompt = mergedSystemPrompt,
        authorsNote = sessionSnap.authorsNote.ifBlank { assistantSnap.authorsNote },
        activeCard = sessionSnap.activeCard ?: assistantSnap.activeCard,
        fragments = (appState.globalWorldBook + assistantSnap.fragments + sessionSnap.fragments).distinctBy { it.id },
        workspacePath = toolPolicy.workspacePath,
        workspaceMode = toolPolicy.workspaceMode,
        permissionOverrides = toolPolicy.asOverrides()
    )

    val effectiveHistory = if (assistantSnap.contextMessageLimit > 0 && historyForCall.size > assistantSnap.contextMessageLimit) {
        historyForCall.takeLast(assistantSnap.contextMessageLimit)
    } else {
        historyForCall
    }

    val effectiveProfileForApi = profileSnap.copy(
        temperature = assistantSnap.temperature ?: profileSnap.temperature,
        topP = assistantSnap.topP ?: profileSnap.topP,
        maxOutputTokens = assistantSnap.maxOutputTokens ?: profileSnap.maxOutputTokens,
        thinkingEffort = assistantSnap.thinkingEffort.ifBlank { profileSnap.thinkingEffort },
        enableWebSearch = toolPolicy.toolsEnabled && toolPolicy.webAccess && toolPolicy.allowsTool("web_search"),
        enableUrlContext = toolPolicy.toolsEnabled && toolPolicy.webAccess && toolPolicy.allowsTool("web_fetch") && profileSnap.enableUrlContext,
        allowShellExecution = false
    )

    val modelName = mergedSessionForApi.modelOverride.ifBlank {
        assistantSnap.defaultModelId.ifBlank {
            effectiveProfileForApi.defaultModel.ifBlank { appState.defaultModels.globalModelId }
        }
    }

    val existingTargetVariant = sessionSnap.messages
        .find { it.id == targetAssistantNodeId }
        ?.variants
        ?.find { it.id == targetVariantId }
    val cleanContinuePrefix = AiProviderEngine.stripInlineErrorNotices(continuePrefix)
    val initialThinking = existingTargetVariant?.thinkingSummary.orEmpty()
    val initialTrailingThinking = existingTargetVariant?.trailingThinking.orEmpty()
    val initialSignature = existingTargetVariant?.thinkingSignature.orEmpty()
    val initialTools = existingTargetVariant?.toolCalls.orEmpty()
    val initialMedia = existingTargetVariant?.mediaItems.orEmpty()
    val initialWebSources = existingTargetVariant?.webSources.orEmpty()
    val initialDurationMs = existingTargetVariant?.thinkingDurationMs ?: 0L
    val identity = AgentRunIdentity(
        PlatformRuntime.createRunId(), sessionId, targetAssistantNodeId, targetVariantId,
        effectiveProfileForApi.id, modelName,
        if (effectiveProfileForApi.protocol == ProviderProtocol.CUSTOM) effectiveProfileForApi.customDialect
        else io.parchment.ai.model.mapProtocolToDialect(effectiveProfileForApi.protocol.name)
    )

    var owningJob: kotlinx.coroutines.Job? = null
    val inheritedGoalTracker = existingTargetVariant?.goalTracker
        ?: historyForCall.asReversed().firstNotNullOfOrNull { it.activeVariant.goalTracker }?.takeIf { !it.isCompleted }
    val runStartedWithGoal = inheritedGoalTracker != null || historyForCall.any { it.activeVariant.content.startsWith("/goal", true) }
    val approvalBroker = ToolApprovalBroker(
        runId = identity.runId,
        chatId = sessionId,
        policySource = {
            val current = appState.sessions.find { it.id == sessionId }
            if (current == null || owningJob?.isActive == false) null else ToolAccessScope(
                AgentPermissionResolver.resolve(current, appState.assistants.find { it.id == current.assistantId }, appState.profiles.find { it.id == current.profileId }),
                goalModeActive = isGoalModeEnabled || runStartedWithGoal
            )
        },
        onStop = { processState.runs.requestStop(identity); owningJob?.cancel() }
    )
    fun updateOwnedStream(updater: (SessionStreamRuntime?) -> SessionStreamRuntime?) {
        processState.runs.update(identity) { current -> updater(current) ?: current }
    }
    // In the run's context, so the engine (and any helper it starts) shows its retry waits here and Retry now ends them.
    val retryControl = RetryControl()
    val newJob = appRunLeases.launch(appProcessScope, modelName, AgentToolRunContext(approvalBroker) + retryControl,
        start = kotlinx.coroutines.CoroutineStart.LAZY) { activity ->
        var terminalPhase = AgentRunPhase.INTERRUPTED
        updateOwnedStream { it?.copy(phase = AgentRunPhase.RUNNING) }
        val approvalObserver = launch {
            activity.waitingForApproval.collect { waiting ->
                updateOwnedStream { current -> current?.copy(phase = when {
                    waiting -> AgentRunPhase.WAITING_APPROVAL
                    current.phase == AgentRunPhase.WAITING_APPROVAL -> AgentRunPhase.RUNNING
                    else -> current.phase
                }) }
            }
        }
        val retryObserver = launch {
            retryControl.waiting.collect { wait -> updateOwnedStream { it?.copy(retryWait = wait) } }
        }
        val contentWords = StreamWordCounter()
        val thinkingWords = StreamWordCounter()
        val networkContentTarget = StringBuilder(cleanContinuePrefix)
        val networkThinkingTarget = StringBuilder(initialThinking)
        val currentStepThinkingTarget = StringBuilder(initialTrailingThinking)
        val signatureSb = StringBuilder(initialSignature)
        var displayedContentLen = cleanContinuePrefix.length
        var displayedThinkingLen = initialThinking.length
        var displayedStepThinkingLen = initialTrailingThinking.length
        val accumulatedTools = initialTools.toMutableList()
        var latestGoalTracker: GoalTrackerState? = inheritedGoalTracker
        var accumulatedUsageRecords = existingTargetVariant?.apiUsageRecords.orEmpty()
        var latestTokenMetrics = existingTargetVariant?.tokenMetrics ?: io.parchment.ai.model.TokenMetrics()
        val accumulatedMedia = initialMedia.toMutableList()
        var accumulatedWebSources = initialWebSources
        var localInteractionId = existingTargetVariant?.providerInteractionId.orEmpty()
        var resumePendingBgId = existingTargetVariant?.pendingBackgroundInteractionId.orEmpty()
        var rawReq = ""
        var rawResp = ""
        var networkDone = false
        val runFlags = object { @kotlin.concurrent.Volatile var aborted = false }
        val turnRequestStartMs = activity.elapsedMs
        var activeThinkingPhaseStartMs = -1L
        var accumulatedThinkingMs = initialDurationMs
        var thinkingDurationMs = initialDurationMs
        var firstLiveTokenMs = -1L
        var liveTokensPerSec = 0.0
        var lastStopwatchTickMs = 0L
        var lastDiskCheckpointMs = turnRequestStartMs
        // Checkpoints save the reply alone (live.json); the whole chat is saved at most every 30 s and at the end,
        // so a long chat costs the reply's size per checkpoint instead of its own. The run starts from a full save.
        val liveTurn = LiveTurnRef(targetAssistantNodeId, targetVariantId)
        var lastFullCheckpointMs = PlatformRuntime.monotonicTimeMillis()
        fun pauseActiveThinkingStopwatch() {
            if (activeThinkingPhaseStartMs >= 0L) {
                val now = activity.elapsedMs
                accumulatedThinkingMs += (now - activeThinkingPhaseStartMs).coerceAtLeast(0L)
                thinkingDurationMs = accumulatedThinkingMs.coerceAtLeast(100L)
                activeThinkingPhaseStartMs = -1L
            }
        }
        fun patchVariantMemory(eventUpdate: (MessageVariant) -> MessageVariant) {
            // The variant arrives with the live text (completeSessionForAction); afterwards the live overlay takes
            // the patched fields, so an event (say a tool step clearing trailing thinking) shows at once even when
            // the pacer has nothing new to publish.
            var patched: MessageVariant? = null
            val updater: (MessageVariant) -> MessageVariant = { v -> eventUpdate(v).also { patched = it } }
            updateLiveTurnInMemory(sessionId, liveTurn) { sess ->
                val msgs = sess.messages
                val lastIdx = msgs.lastIndex
                if (lastIdx >= 0 && msgs[lastIdx].id == targetAssistantNodeId) {
                    // Fast O(1) path for the active streaming turn at the bottom of the chat
                    val lastNode = msgs[lastIdx]
                    val updatedLast = lastNode.copy(
                        variants = lastNode.variants.map { v ->
                            if (v.id == targetVariantId) updater(v) else v
                        }
                    )
                    val nextList = ArrayList<MessageNode>(msgs.size)
                    for (i in 0 until lastIdx) nextList.add(msgs[i])
                    nextList.add(updatedLast)
                    sess.copy(messages = nextList)
                } else {
                    sess.copy(
                        messages = msgs.map { node ->
                            if (node.id != targetAssistantNodeId) node
                            else node.copy(
                                variants = node.variants.map { v ->
                                    if (v.id == targetVariantId) updater(v) else v
                                }
                            )
                        }
                    )
                }
            }
            val result = patched
            val live = LiveTurns.stateOf(targetVariantId)?.value
            if (result != null && live != null) {
                LiveTurns.publish(live.copy(content = result.content, thinkingSummary = result.thinkingSummary,
                    trailingThinking = result.trailingThinking, thinkingSignature = result.thinkingSignature,
                    thinkingDurationMs = result.thinkingDurationMs))
            }
        }

        fun flushCheckpointToDisk(isFinal: Boolean, finalUpdater: ((MessageVariant) -> MessageVariant)? = null) {
            if (isFinal) {
                pauseActiveThinkingStopwatch()
                // The final text below is authoritative; a live overlay left behind would be merged over it later.
                LiveTurns.end(targetVariantId)
            }
            val curContent = if (isFinal) networkContentTarget.toString() else networkContentTarget.substring(0, displayedContentLen.coerceAtMost(networkContentTarget.length))
            val curThink = if (isFinal) networkThinkingTarget.toString() else networkThinkingTarget.substring(0, displayedThinkingLen.coerceAtMost(networkThinkingTarget.length))
            val curStepThink = if (accumulatedTools.isEmpty()) "" else if (isFinal) currentStepThinkingTarget.toString() else currentStepThinkingTarget.substring(0, displayedStepThinkingLen.coerceAtMost(currentStepThinkingTarget.length))
            val curSig = signatureSb.toString()
            val curTools = if (isFinal) {
                accumulatedTools.map { tc -> if (tc.wasCutOff()) tc.withInterruptedOutcome() else tc }
            } else {
                accumulatedTools.toList()
            }
            val curGoal = latestGoalTracker
            val curMedia = accumulatedMedia.toList()
            val curWebSources = accumulatedWebSources
            val now = PlatformRuntime.monotonicTimeMillis()
            val replyOnly = !isFinal && now - lastFullCheckpointMs < FULL_CHECKPOINT_INTERVAL_MS
            if (!replyOnly) lastFullCheckpointMs = now
            val save: (String, SessionUpdate) -> Unit = if (replyOnly) {
                { id, transform -> updateLiveTurnById(id, liveTurn, transform) }
            } else {
                { id, transform -> updateSessionById(id, immediate = true, transform = transform) }
            }
            save(sessionId) { sess ->
                sess.copy(
                    messages = sess.messages.map { node ->
                        val shouldAutoMute = isFinal && terminalPhase == AgentRunPhase.COMPLETED && node.id in autoMuteNodeIdsOnFinish
                        if (node.id != targetAssistantNodeId) {
                            if (shouldAutoMute) node.copy(includedInContext = false) else node
                        } else {
                            node.copy(
                                includedInContext = if (shouldAutoMute) false else node.includedInContext,
                                variants = node.variants.map { v ->
                                    if (v.id != targetVariantId) v
                                    else {
                                        val base = v.copy(
                                            content = curContent,
                                            thinkingSummary = curThink,
                                            trailingThinking = curStepThink,
                                            thinkingSignature = curSig,
                                            thinkingDurationMs = thinkingDurationMs,
                                            toolCalls = curTools,
                                            goalTracker = curGoal ?: v.goalTracker,
                                            mediaItems = curMedia,
                                            webSources = curWebSources,
                                            pendingBackgroundInteractionId = if (isFinal) "" else localInteractionId,
                                            isStreaming = !isFinal
                                        )
                                        if (finalUpdater != null) finalUpdater(base) else base
                                    }
                                }
                            )
                        }
                    }
                )
            }
        }

        // Wakes the pacer when the stream delivers anything; conflated, so a burst of updates is one wake.
        val pacerWake = Channel<Unit>(Channel.CONFLATED)
        // Reveals text in step with the display's frame clock, carrying fractional characters between frames,
        // so text and thoughts appear smoothly instead of in word-sized jumps.
        val pacerJob = launch {
            var lastFrameNanos = 0L
            var thinkCarry = 0f
            var stepThinkCarry = 0f
            var contCarry = 0f

            fun computeLetterStep(diff: Int, dtMs: Long, carry: Float, done: Boolean): Pair<Int, Float> {
                if (diff <= 0) return 0 to 0f
                if (processState.frameClock.value == null) return diff to 0f
                val dtSec = (dtMs.coerceIn(2L, 32L)) / 1000f
                // Base letter-by-letter speed (~92 chars/sec -> ~0.76 chars per 120Hz frame, ~1.4 chars per 60Hz frame)
                // plus a gentle elastic catch-up only when buffer backlog grows beyond 28 chars
                val backlog = (diff - 28).coerceAtLeast(0)
                val catchUpPerSec = if (done) {
                    maxOf(220f, diff * 10f)
                } else {
                    backlog * 4.2f
                }
                val exactChars = carry + (92f + catchUpPerSec) * dtSec
                val rawStep = exactChars.toInt()
                return if (rawStep <= 0) {
                    0 to exactChars
                } else {
                    val maxPerFrame = when {
                        done -> maxOf(4, diff / 4)
                        diff <= 28 -> 1
                        diff <= 72 -> 2
                        diff <= 160 -> 4
                        else -> maxOf(6, diff / 14)
                    }
                    val clamped = rawStep.coerceAtMost(maxPerFrame).coerceAtMost(diff)
                    clamped to (exactChars - clamped).coerceIn(0f, 0.99f)
                }
            }

            var cachedVisibleCont = networkContentTarget.substring(0, displayedContentLen.coerceAtMost(networkContentTarget.length))
            var cachedVisibleContLen = displayedContentLen
            var lastOverlayCopyMs = 0L
            var cachedVisibleThink = networkThinkingTarget.substring(0, displayedThinkingLen.coerceAtMost(networkThinkingTarget.length))
            var cachedVisibleThinkLen = displayedThinkingLen
            var cachedVisibleStepThink = ""
            var cachedVisibleStepThinkLen = -1
            var cachedSig = signatureSb.toString()
            var cachedSigLen = signatureSb.length
            var cachedThinkWords = 0
            var lastThinkWordScanIdx = 0
            var lastThinkCharWasSpace = true
            var lastPublishedThinkWholeSec = -1L

            // True VSync frame-locked streaming (low-memory devices publish every other frame, ~30 fps).
            val framesPerPublish = if (PlatformRuntime.isLowMemoryDevice) 2 else 1
            while (!networkDone || displayedContentLen < networkContentTarget.length || displayedThinkingLen < networkThinkingTarget.length) {
                val caughtUp = displayedContentLen >= networkContentTarget.length &&
                    displayedThinkingLen >= networkThinkingTarget.length &&
                    displayedStepThinkingLen >= currentStepThinkingTarget.length &&
                    displayedContentLen == cachedVisibleContLen &&
                    displayedThinkingLen == cachedVisibleThinkLen
                val dtMs = if (caughtUp && !networkDone) {
                    // Nothing left to reveal (model thinking or tool running): sleep until new SSE bytes arrive,
                    // polling every 250 ms so whole-second stopwatch boundaries (1s, 2s, ..., 30s) tick steadily.
                    withTimeoutOrNull(250L) { pacerWake.receive() }
                    lastFrameNanos = 0L
                    8L
                } else {
                    var frameNanos = processState.nextFrameNanos()
                    for (skipped in 1 until framesPerPublish) {
                        if (frameNanos == null) break
                        frameNanos = processState.nextFrameNanos() ?: frameNanos
                    }
                    if (frameNanos != null) {
                        val delta = if (lastFrameNanos > 0L) ((frameNanos - lastFrameNanos) / 1_000_000L).coerceIn(2L, 32L) else 8L
                        lastFrameNanos = frameNanos
                        delta
                    } else {
                        delay(if (processState.frameClock.value == null && !networkDone) 100L else 8L)
                        lastFrameNanos = 0L
                        8L
                    }
                }
                var changed = false
                val now = activity.elapsedMs
                if (now - lastStopwatchTickMs >= 250L) {
                    lastStopwatchTickMs = now
                    if (firstLiveTokenMs >= 0L) {
                        val netGenerationMs = ((now - turnRequestStartMs) - activity.toolMs).coerceAtLeast(500L)
                        val approxToolChars = accumulatedTools.sumOf { it.name.length + it.argumentsJson.length }
                        val approxGeneratedTok = ((networkContentTarget.length + networkThinkingTarget.length + approxToolChars) / 4).coerceAtLeast(1)
                        val rawLiveRate = (approxGeneratedTok * 1000.0) / netGenerationMs.toDouble()
                        val newLiveRate = ((rawLiveRate * 10.0).toInt()) / 10.0
                        if (newLiveRate != liveTokensPerSec) {
                            liveTokensPerSec = newLiveRate
                        }
                    }
                    if (activeThinkingPhaseStartMs >= 0L) {
                        thinkingDurationMs = (accumulatedThinkingMs + (now - activeThinkingPhaseStartMs)).coerceAtLeast(0L)
                        val wholeSec = thinkingDurationMs / 1000L
                        val curThinkTargetLen = networkThinkingTarget.length
                        if (curThinkTargetLen < lastThinkWordScanIdx) {
                            cachedThinkWords = 0
                            lastThinkWordScanIdx = 0
                            lastThinkCharWasSpace = true
                        }
                        while (lastThinkWordScanIdx < curThinkTargetLen) {
                            val c = networkThinkingTarget[lastThinkWordScanIdx]
                            val isSp = c == ' ' || c == '\n' || c == '\r' || c == '\t'
                            if (!isSp && lastThinkCharWasSpace) cachedThinkWords++
                            lastThinkCharWasSpace = isSp
                            lastThinkWordScanIdx++
                        }
                        if (wholeSec != lastPublishedThinkWholeSec) {
                            lastPublishedThinkWholeSec = wholeSec
                            changed = true
                        }
                        val hasRunningToolNow = accumulatedTools.any { it.isRunning }
                        if (!hasRunningToolNow) {
                            val thinkWords = cachedThinkWords
                            val toolSuffix = if (accumulatedTools.isNotEmpty()) " • ${accumulatedTools.size} tools" else ""
                            val nextStatus = Strings.thinkingWords(modelName, thinkWords, toolSuffix, formatDurationFromMs(thinkingDurationMs))
                            updateOwnedStream { cur ->
                                if (cur != null && cur.isThinkingPhase && cur.retryWait == null && !cur.statusText.startsWith("Context ") && cur.statusText != nextStatus) {
                                    cur.copy(statusText = nextStatus)
                                } else cur
                            }
                        }
                    }
                }
                val targetThinkLen = networkThinkingTarget.length
                if (displayedThinkingLen < targetThinkLen) {
                    val (step, nextCarry) = computeLetterStep(targetThinkLen - displayedThinkingLen, dtMs, thinkCarry, networkDone)
                    thinkCarry = nextCarry
                    if (step > 0) {
                        displayedThinkingLen = (displayedThinkingLen + step).coerceAtMost(targetThinkLen)
                        changed = true
                    }
                } else if (displayedThinkingLen > targetThinkLen) {
                    displayedThinkingLen = targetThinkLen
                    thinkCarry = 0f
                    changed = true
                } else {
                    thinkCarry = 0f
                }
                val targetStepThinkLen = currentStepThinkingTarget.length
                if (displayedStepThinkingLen < targetStepThinkLen) {
                    val (step, nextCarry) = computeLetterStep(targetStepThinkLen - displayedStepThinkingLen, dtMs, stepThinkCarry, networkDone)
                    stepThinkCarry = nextCarry
                    if (step > 0) {
                        displayedStepThinkingLen = (displayedStepThinkingLen + step).coerceAtMost(targetStepThinkLen)
                        changed = true
                    }
                } else if (displayedStepThinkingLen > targetStepThinkLen) {
                    displayedStepThinkingLen = targetStepThinkLen
                    stepThinkCarry = 0f
                    changed = true
                } else {
                    stepThinkCarry = 0f
                }
                val targetContLen = networkContentTarget.length
                if (displayedContentLen < targetContLen) {
                    val (step, nextCarry) = computeLetterStep(targetContLen - displayedContentLen, dtMs, contCarry, networkDone)
                    contCarry = nextCarry
                    if (step > 0) {
                        displayedContentLen = (displayedContentLen + step).coerceAtMost(targetContLen)
                        changed = true
                    }
                } else if (displayedContentLen > targetContLen) {
                    displayedContentLen = targetContLen
                    contCarry = 0f
                    changed = true
                } else {
                    contCarry = 0f
                }
                val effStepLen = if (accumulatedTools.isEmpty()) 0 else displayedStepThinkingLen.coerceAtMost(currentStepThinkingTarget.length)
                val contentBehind = displayedContentLen != cachedVisibleContLen
                val thinkingBehind = displayedThinkingLen != cachedVisibleThinkLen || effStepLen != cachedVisibleStepThinkLen
                val longestActiveLen = maxOf(displayedContentLen, displayedThinkingLen, effStepLen)
                val holdLongText = !networkDone && (contentBehind || thinkingBehind) &&
                    now - lastOverlayCopyMs < visibleTextCopyIntervalMs(longestActiveLen)
                if ((changed || contentBehind || thinkingBehind) && !holdLongText) {
                    if (contentBehind) {
                        cachedVisibleCont = networkContentTarget.substring(0, displayedContentLen.coerceAtMost(networkContentTarget.length))
                        cachedVisibleContLen = displayedContentLen
                    }
                    if (displayedThinkingLen != cachedVisibleThinkLen) {
                        cachedVisibleThink = networkThinkingTarget.substring(0, displayedThinkingLen.coerceAtMost(networkThinkingTarget.length))
                        cachedVisibleThinkLen = displayedThinkingLen
                    }
                    if (effStepLen != cachedVisibleStepThinkLen) {
                        cachedVisibleStepThink = if (effStepLen == 0) "" else currentStepThinkingTarget.substring(0, effStepLen)
                        cachedVisibleStepThinkLen = effStepLen
                    }
                    lastOverlayCopyMs = now
                    if (signatureSb.length != cachedSigLen) {
                        cachedSig = signatureSb.toString()
                        cachedSigLen = signatureSb.length
                    }
                    // Every frame goes to the live overlay, so only this reply redraws; the app state takes the text
                    // at the checkpoint below and at the end (flushCheckpointToDisk).
                    if (!runFlags.aborted) {
                        LiveTurns.publish(
                            LiveTurnOverlay(
                                sessionId = sessionId,
                                nodeId = targetAssistantNodeId,
                                variantId = targetVariantId,
                                content = cachedVisibleCont,
                                thinkingSummary = cachedVisibleThink,
                                trailingThinking = cachedVisibleStepThink,
                                thinkingSignature = cachedSig,
                                thinkingDurationMs = thinkingDurationMs,
                                tokensPerSec = liveTokensPerSec
                            )
                        )
                    }
                }
                // Save a checkpoint every 3.5 seconds: switching apps never loses progress, and the disk isn't written on every token.
                if (!runFlags.aborted && now - lastDiskCheckpointMs >= 3500L) {
                    lastDiskCheckpointMs = now
                    flushCheckpointToDisk(isFinal = false)
                }
            }
        }

        try {
            var finalSessionForCall = mergedSessionForApi
            var finalHistoryForCall = effectiveHistory
            val effectiveCtxLimit = (effectiveProfileForApi.customModelSpecs[modelName]?.contextWindowTokens
                ?: effectiveProfileForApi.contextWindowTokens)
            val autoCompactThresholdTokens = (effectiveCtxLimit * 85) / 100
            var minTokensForNextAutoCompact = autoCompactThresholdTokens
            var lastMidStreamCompactCheckMs = activity.elapsedMs

            class MidStreamAutoCompactSignal(val usagePct: Long) : Exception(Strings.autoCompactTriggeredAt(usagePct))

            // Token counting runs regexes over the whole chat, so it happens on Dispatchers.Default from inputs
            // captured here; the live StringBuilders are only ever touched on this coroutine.
            suspend fun computeBaseContextTokens(): Int {
                val session = finalSessionForCall.copy(messages = finalHistoryForCall)
                val goalModeActive = latestGoalTracker != null
                val worldBook = appState.globalWorldBook
                return withContext(Dispatchers.Default) {
                    O200kTokenizer.computeContextWindowBreakdown(
                        session = session,
                        assistant = assistantSnap,
                        profile = effectiveProfileForApi,
                        draftPrompt = "",
                        draftPrefill = "",
                        pendingAttachments = emptyList(),
                        goalModeActive = goalModeActive,
                        globalWorldBook = worldBook
                    ).totalUsedTokens
                }
            }

            var cachedBaseTokens = computeBaseContextTokens()

            suspend fun computeLiveTurnAndContextTokens(forceExact: Boolean = false): Int {
                if (!forceExact) {
                    // Fast O(1) upper-bound check (~3 chars/token) so 1.5s mid-stream checks skip regex tokenization when well below threshold
                    val approxToolsChars = accumulatedTools.sumOf { 24 + it.name.length + it.argumentsJson.length + it.resultText.length }
                    val approxUpperBound = cachedBaseTokens + ((networkContentTarget.length + networkThinkingTarget.length + approxToolsChars) / 3)
                    if (approxUpperBound < (minTokensForNextAutoCompact * 85) / 100) {
                        return approxUpperBound
                    }
                }
                val content = networkContentTarget.toString()
                val thinking = networkThinkingTarget.toString()
                val tools = accumulatedTools.toList()
                val liveAssistantTokens = withContext(Dispatchers.Default) {
                    O200kTokenizer.countTokens(content) + O200kTokenizer.countTokens(thinking) +
                        tools.sumOf { tc ->
                            6 + O200kTokenizer.countTokens(tc.name) +
                                O200kTokenizer.countTokens(tc.argumentsJson) +
                                O200kTokenizer.countTokens(tc.resultText)
                        }
                }
                return cachedBaseTokens + liveAssistantTokens
            }

            suspend fun executeAutoCompactionInPlace(usagePct: Long, isMidStream: Boolean): Boolean {
                val summarySnap = finalSessionForCall.storySummary
                val historySnap = finalHistoryForCall
                val liveToolsSnap = accumulatedTools.toList()
                val liveThinkingSnap = networkThinkingTarget.toString()
                val plan = withContext(Dispatchers.Default) {
                    O200kTokenizer.prepareCompactionPlan(
                        existingStorySummary = summarySnap,
                        historyNodes = historySnap,
                        keepRecentTurnsCount = 2,
                        liveTargetTools = liveToolsSnap,
                        liveTargetThinking = liveThinkingSnap
                    )
                }
                if (!plan.didCompactAnything) return false

                val statusPrefix = if (isMidStream) {
                    Strings.conversationFullPausingToShortenIt(usagePct)
                } else {
                    Strings.conversationFullShorteningOlderMessages(usagePct)
                }
                updateOwnedStream { cur ->
                    cur?.copy(isThinkingPhase = false, statusText = statusPrefix, phase = AgentRunPhase.COMPACTING)
                }

                val dm = appState.defaultModels
                val sumProf = if (!dm.summaryFollowGlobal && dm.summaryProfileId.isNotBlank()) {
                    appState.profiles.find { it.id == dm.summaryProfileId } ?: effectiveProfileForApi
                } else effectiveProfileForApi
                val sumModel = if (!dm.summaryFollowGlobal && dm.summaryModelId.isNotBlank()) {
                    dm.summaryModelId
                } else modelName

                val sumPrompt = "Update the comprehensive bulleted continuity summary with all key events, facts, files/tools inspected, decisions, and character/project states from the context below. Do not truncate or omit important details:\n\n${plan.transcriptToSummarize}"
                val generatedSummary = withTimeoutOrNull(180_000L) {
                    generateTrackedUtility(sessionId, "compaction", sumProf, sumModel, sumPrompt, maxOutputTokens = null).getOrNull()
                }?.takeIf { it.isNotBlank() } ?: plan.fallbackSummary.ifBlank { finalSessionForCall.storySummary }

                finalSessionForCall = finalSessionForCall.copy(storySummary = generatedSummary)
                finalHistoryForCall = plan.compactedHistoryNodes
                accumulatedTools.clear()
                accumulatedTools.addAll(plan.compactedLiveTools)
                if (plan.compactedLiveThinking != networkThinkingTarget.toString()) {
                    networkThinkingTarget.clear()
                    thinkingWords.reset()
                    networkThinkingTarget.append(plan.compactedLiveThinking)
                    displayedThinkingLen = displayedThinkingLen.coerceAtMost(networkThinkingTarget.length)
                }
                currentStepThinkingTarget.clear()
                displayedStepThinkingLen = 0
                localInteractionId = ""
                resumePendingBgId = ""

                val compactedHistoryById = plan.compactedHistoryNodes.associateBy { it.id }
                val toolsSnap = accumulatedTools.toList()
                val thinkSnap = networkThinkingTarget.toString()
                updateSessionById(sessionId, immediate = true) { sess ->
                    sess.copy(
                        storySummary = generatedSummary,
                        messages = sess.messages.map { n ->
                            when {
                                n.id == targetAssistantNodeId -> n.copy(
                                    variants = n.variants.map { v ->
                                        if (v.id == targetVariantId) {
                                            v.copy(
                                                toolCalls = toolsSnap,
                                                thinkingSummary = thinkSnap,
                                                trailingThinking = "",
                                                providerInteractionId = "",
                                                pendingBackgroundInteractionId = ""
                                            )
                                        } else v
                                    }
                                )
                                compactedHistoryById.containsKey(n.id) -> compactedHistoryById[n.id]!!
                                n.id in plan.mutedNodeIds -> n.copy(includedInContext = false)
                                else -> n
                            }
                        }
                    )
                }
                cachedBaseTokens = computeBaseContextTokens()
                val newTokens = computeLiveTurnAndContextTokens()
                val newPct = if (effectiveCtxLimit > 0) ((newTokens.toLong() * 100L) / effectiveCtxLimit.toLong()) else 0L
                minTokensForNextAutoCompact = maxOf(
                    autoCompactThresholdTokens,
                    newTokens + maxOf(1000, effectiveCtxLimit / 15)
                )
                updateOwnedStream { cur ->
                    cur?.copy(
                        statusText = Strings.conversationShortenedContinuing(usagePct, newPct, modelName),
                        interactionId = "",
                        phase = AgentRunPhase.RUNNING
                    )
                }
                return true
            }

            suspend fun checkMidStreamAutoCompactAndThrowIfNeeded() {
                if (!appState.preferences.autoCompactWhenNearContextLimit || effectiveCtxLimit <= 0) return
                if (accumulatedTools.any { it.isRunning }) return
                val liveTokens = computeLiveTurnAndContextTokens()
                if (liveTokens >= minTokensForNextAutoCompact &&
                    O200kTokenizer.hasCompactableContext(
                        historyNodes = finalHistoryForCall,
                        keepRecentTurnsCount = 2,
                        liveTargetTools = accumulatedTools,
                        liveTargetThinking = networkThinkingTarget.toString()
                    )
                ) {
                    val usagePct = ((liveTokens.toLong() * 100L) / effectiveCtxLimit.toLong()).coerceAtMost(999L)
                    throw MidStreamAutoCompactSignal(usagePct)
                }
            }

            // Layer 1: Proactive Pre-Send Auto-Compaction when active context is already >= 85%
            val initialTokens = computeLiveTurnAndContextTokens()
            if (appState.preferences.autoCompactWhenNearContextLimit &&
                effectiveCtxLimit > 0 &&
                initialTokens >= minTokensForNextAutoCompact &&
                O200kTokenizer.hasCompactableContext(
                    historyNodes = finalHistoryForCall,
                    keepRecentTurnsCount = 2,
                    liveTargetTools = accumulatedTools,
                    liveTargetThinking = networkThinkingTarget.toString()
                )
            ) {
                val usagePct = ((initialTokens.toLong() * 100L) / effectiveCtxLimit.toLong()).coerceAtMost(999L)
                executeAutoCompactionInPlace(usagePct = usagePct, isMidStream = false)
            }

            // Layer 2: Real-Time Stream Loop with Mid-Stream >=85% Auto-Compaction & Instant Auto-Resume
            val streamStatusThrottle = StreamStatusThrottle()
            while (true) {
                try {
                    AiProviderEngine.streamGeneration(
                        profile = effectiveProfileForApi,
                        session = finalSessionForCall.withProfileName(appState.preferences),
                        historyNodes = finalHistoryForCall,
                        continueFromAssistantPrefix = AiProviderEngine.stripInlineErrorNotices(networkContentTarget.toString()),
                        initialThinking = networkThinkingTarget.toString(),
                        initialSignature = signatureSb.toString(),
                        resumeMediaTask = resumePendingBgId,
                        spendLimitUsd = appState.preferences.chatSpendLimitUsd,
                        spendBeforeRunUsd = if (appState.preferences.chatSpendLimitUsd > 0.0) sessionSpendSummary(finalSessionForCall, effectiveProfileForApi).towardLimitUsd else 0.0,
                        initialToolCalls = accumulatedTools.toList(),
                        initialMediaItems = accumulatedMedia.toList(),
                        initialWebSources = accumulatedWebSources,
                        initialGoalTracker = latestGoalTracker,
                        initialElapsedMs = (activity.elapsedMs - turnRequestStartMs).coerceAtLeast(0L),
                        initialApiUsageRecords = accumulatedUsageRecords,
                        initialTokenMetrics = latestTokenMetrics
                    ).collect { update ->
                        // The pacer runs on this dispatcher too, so it reads the targets after this update is applied.
                        pacerWake.trySend(Unit)
                        when (update) {
                            is StreamUpdate.Status -> {
                                if (update.interactionId.isNotBlank()) {
                                    localInteractionId = update.interactionId
                                    flushCheckpointToDisk(isFinal = false)
                                }
                                streamStatusThrottle.onExplicitStatus(update.message)
                                updateOwnedStream { cur ->
                                    cur?.copy(statusText = update.message, interactionId = localInteractionId)
                                }
                            }
                            is StreamUpdate.ThinkingDelta -> {
                                val now = activity.elapsedMs
                                if (firstLiveTokenMs < 0L) firstLiveTokenMs = now
                                if (activeThinkingPhaseStartMs < 0L) activeThinkingPhaseStartMs = now
                                thinkingDurationMs = (accumulatedThinkingMs + (now - activeThinkingPhaseStartMs)).coerceAtLeast(100L)
                                networkThinkingTarget.append(update.delta)
                                currentStepThinkingTarget.append(update.delta)
                                AiProviderEngine.appendSignatureChunkSafe(signatureSb, update.signatureDelta)
                                streamStatusThrottle.onPhaseStatus(now, isThinkingPhase = true) {
                                    val thinkWords = thinkingWords.count(networkThinkingTarget)
                                    val toolSuffix = if (accumulatedTools.isNotEmpty()) " • ${accumulatedTools.size} tools" else ""
                                    Strings.thinkingWords(modelName, thinkWords, toolSuffix, formatDurationFromMs(thinkingDurationMs))
                                }?.let { statusText ->
                                    updateOwnedStream { cur ->
                                        cur?.copy(isThinkingPhase = true, statusText = statusText)
                                    }
                                }
                                if (now - lastMidStreamCompactCheckMs >= 1500L) {
                                    lastMidStreamCompactCheckMs = now
                                    checkMidStreamAutoCompactAndThrowIfNeeded()
                                }
                            }
                            is StreamUpdate.ContentDelta -> {
                                val now = activity.elapsedMs
                                if (firstLiveTokenMs < 0L) firstLiveTokenMs = now
                                pauseActiveThinkingStopwatch()
                                networkContentTarget.append(update.delta)
                                streamStatusThrottle.onPhaseStatus(now, isThinkingPhase = false) {
                                    val wordCount = contentWords.count(networkContentTarget)
                                    Strings.writingWords(modelName, wordCount)
                                }?.let { statusText ->
                                    updateOwnedStream { cur ->
                                        cur?.copy(isThinkingPhase = false, statusText = statusText)
                                    }
                                }
                                if (now - lastMidStreamCompactCheckMs >= 1500L) {
                                    lastMidStreamCompactCheckMs = now
                                    checkMidStreamAutoCompactAndThrowIfNeeded()
                                }
                            }
                            is StreamUpdate.ToolCallEvent -> {
                                val now = activity.elapsedMs
                                // The reasoning and tool timer keeps running across tool calls.
                                if (activeThinkingPhaseStartMs < 0L) activeThinkingPhaseStartMs = now
                                thinkingDurationMs = (accumulatedThinkingMs + (now - activeThinkingPhaseStartMs)).coerceAtLeast(100L)
                                val stepThinkFallback = currentStepThinkingTarget.toString().trim()
                                val stepSigFallback = signatureSb.toString().trim()
                                if (stepThinkFallback.isNotEmpty()) {
                                    currentStepThinkingTarget.clear()
                                    displayedStepThinkingLen = 0
                                }
                                if (stepSigFallback.isNotEmpty()) {
                                    signatureSb.clear()
                                }
                                val existingIdx = accumulatedTools.indexOfFirst {
                                    it.id == update.toolCall.id && it.id.isNotBlank()
                                }
                                val enrichedTool = if (existingIdx >= 0) {
                                    val prev = accumulatedTools[existingIdx]
                                    update.toolCall.copy(
                                        precedingThinking = update.toolCall.precedingThinking.ifBlank { prev.precedingThinking },
                                        precedingText = prev.precedingText,
                                        thoughtSignature = update.toolCall.thoughtSignature.ifBlank { prev.thoughtSignature }
                                    )
                                } else {
                                    update.toolCall.copy(
                                        precedingThinking = update.toolCall.precedingThinking.ifBlank { stepThinkFallback },
                                        precedingText = unclaimedStepText(networkContentTarget, accumulatedTools),
                                        thoughtSignature = update.toolCall.thoughtSignature.ifBlank { stepSigFallback }
                                    )
                                }
                                if (existingIdx >= 0) {
                                    accumulatedTools[existingIdx] = enrichedTool
                                } else {
                                    accumulatedTools.add(enrichedTool)
                                }
                                val toolsSnap = accumulatedTools.toList()
                                patchVariantMemory { v ->
                                    v.copy(
                                        toolCalls = toolsSnap,
                                        trailingThinking = "",
                                        thinkingDurationMs = thinkingDurationMs
                                    )
                                }
                                if (!enrichedTool.isRunning) {
                                    val toolStatus = Strings.executedTool(enrichedTool.name, formatDurationFromMs(thinkingDurationMs))
                                    streamStatusThrottle.onExplicitStatus(toolStatus)
                                    updateOwnedStream { cur ->
                                        cur?.copy(
                                            isThinkingPhase = true,
                                            statusText = toolStatus
                                        )
                                    }
                                    flushCheckpointToDisk(isFinal = false)
                                    checkMidStreamAutoCompactAndThrowIfNeeded()
                                } else {
                                    updateOwnedStream { cur ->
                                        cur?.copy(isThinkingPhase = true)
                                    }
                                    // Saved before it runs, so a crash mid-change leaves a record instead of a blank.
                                    if (!WorkspaceHarness.isParallelSafeReadOnlyTool(enrichedTool.name)) flushCheckpointToDisk(isFinal = false)
                                }
                            }
                            is StreamUpdate.GoalProgress -> {
                                latestGoalTracker = update.goalTracker
                                val done = update.goalTracker.completedCount
                                val total = update.goalTracker.todos.size
                                val goalMsg = if (total > 0) {
                                    Strings.goalProgressStepsDone(done, total)
                                } else {
                                    Strings.planningGoal(update.goalTracker.goalObjective.take(48))
                                }
                                streamStatusThrottle.onExplicitStatus(goalMsg)
                                updateOwnedStream { cur -> cur?.copy(statusText = goalMsg) }
                                patchVariantMemory { v -> v.copy(goalTracker = update.goalTracker) }
                                flushCheckpointToDisk(isFinal = false)
                            }
                            is StreamUpdate.MediaGenerated -> {
                                val relocatedItem = if (update.item.localFilePath.isNotBlank()) {
                                    update.item.copy(localFilePath = PlatformStorage.relocateLegacyMediaToChatFolder(update.item.localFilePath, sessionId))
                                } else update.item
                                accumulatedMedia.add(relocatedItem)
                                val mediaSnap = accumulatedMedia.toList()
                                patchVariantMemory { v -> v.copy(mediaItems = mediaSnap) }
                                val mediaStatus = "Generated ${relocatedItem.kind.name.lowercase()} (${accumulatedMedia.size})"
                                streamStatusThrottle.onExplicitStatus(mediaStatus)
                                updateOwnedStream { cur ->
                                    cur?.copy(statusText = mediaStatus)
                                }
                            }
                            is StreamUpdate.WebSourcesUpdated -> {
                                accumulatedWebSources = update.sources
                                patchVariantMemory { v -> v.copy(webSources = update.sources) }
                            }
                            is StreamUpdate.RawWireSnapshot -> {
                                if (update.requestJson.isNotEmpty()) rawReq = update.requestJson.take(4096)
                                if (update.latestResponseChunk.isNotEmpty()) rawResp = update.latestResponseChunk.take(4096)
                            }
                            is StreamUpdate.UsageUpdated -> {
                                updateOwnedStream { it?.copy(actualWireDialect = update.record.wireDialect) }
                                val index = accumulatedUsageRecords.indexOfFirst { it.requestId == update.record.requestId }
                                accumulatedUsageRecords = if (index < 0) accumulatedUsageRecords + update.record else {
                                    accumulatedUsageRecords.toMutableList().also { it[index] = update.record }
                                }
                                latestTokenMetrics = update.metrics.copy(durationMs = latestTokenMetrics.durationMs,
                                    ttftMs = latestTokenMetrics.ttftMs, tokensPerSec = latestTokenMetrics.tokensPerSec)
                                patchVariantMemory { v -> v.copy(apiUsageRecords = accumulatedUsageRecords, tokenMetrics = latestTokenMetrics) }
                            }
                            is StreamUpdate.Completed -> {
                                terminalPhase = if ((update.goalTracker ?: latestGoalTracker)?.let {
                                    it.goalObjective.isNotBlank() && !it.isCompleted
                                } == true) AgentRunPhase.INTERRUPTED else AgentRunPhase.COMPLETED
                                accumulatedUsageRecords = update.apiUsageRecords
                                latestTokenMetrics = update.metrics
                                if (update.fullContent.length > networkContentTarget.length) {
                                    networkContentTarget.clear()
                                    contentWords.reset()
                                    networkContentTarget.append(update.fullContent)
                                }
                                if (update.fullThinking.length > networkThinkingTarget.length) {
                                    networkThinkingTarget.clear()
                                    thinkingWords.reset()
                                    networkThinkingTarget.append(update.fullThinking)
                                }
                                if (update.goalTracker != null) {
                                    latestGoalTracker = update.goalTracker
                                }
                                networkDone = true
                                pacerWake.trySend(Unit)
                                pacerJob.join()
                                flushCheckpointToDisk(isFinal = true) { v ->
                                    v.copy(
                                        content = update.fullContent,
                                        thinkingSummary = update.fullThinking,
                                        trailingThinking = update.trailingThinking,
                                        thinkingSignature = update.thinkingSignature,
                                        thinkingDurationMs = thinkingDurationMs,
                                        toolCalls = update.toolCalls,
                                        goalTracker = update.goalTracker ?: latestGoalTracker,
                                        mediaItems = update.mediaItems,
                                        webSources = update.webSources.ifEmpty { accumulatedWebSources },
                                        providerInteractionId = update.providerInteractionId,
                                        pendingBackgroundInteractionId = "",
                                        isStreaming = false,
                                        providerProtocol = AiProviderEngine.resolveWireProtocol(effectiveProfileForApi),
                                        providerWireDialect = update.providerWireDialect.ifBlank { AiProviderEngine.resolveWireDialect(effectiveProfileForApi) },
                                        modelUsed = modelName,
                                        tokenMetrics = update.metrics,
                                        apiUsageRecords = update.apiUsageRecords,
                                        rawRequestPreview = update.rawRequest.take(4096),
                                        rawResponsePreview = update.rawResponse.take(4096)
                                    )
                                }
                                if (isLiveVoiceMode && sessionId == appState.activeSessionId && update.fullContent.isNotBlank()) {
                                    PlatformVoice.speakTextAloud(stripMarkdownToPlainText(update.fullContent))
                                }
                            }
                            is StreamUpdate.Error -> {
                                runFlags.aborted = true
                                networkDone = true
                                pacerJob.cancel()
                                val partialProse = networkContentTarget.toString()
                                val hasPartialReasoningOrTools = networkThinkingTarget.isNotEmpty() || accumulatedTools.isNotEmpty()
                                flushCheckpointToDisk(isFinal = true) { v ->
                                    val errorNotice = when {
                                        partialProse.isNotBlank() -> {
                                            "$partialProse\n\n*[Stream paused/interrupted: ${update.message} — tap '→ Continue' to resume seamlessly]*"
                                        }
                                        hasPartialReasoningOrTools -> {
                                            Strings.streamPausedInterruptedTapContinueOr(update.message)
                                        }
                                        else -> {
                                            Strings.providerErrorCheckYourApiKey(update.message, effectiveProfileForApi.baseUrl, modelName)
                                        }
                                    }
                                    v.copy(
                                        content = errorNotice,
                                        thinkingSummary = networkThinkingTarget.toString(),
                                        trailingThinking = if (accumulatedTools.isNotEmpty()) currentStepThinkingTarget.toString().trim() else "",
                                        thinkingSignature = signatureSb.toString(),
                                        thinkingDurationMs = thinkingDurationMs,
                                        toolCalls = v.toolCalls,
                                        goalTracker = latestGoalTracker ?: v.goalTracker,
                                        mediaItems = accumulatedMedia.toList(),
                                        webSources = accumulatedWebSources,
                                        isStreaming = false,
                                        rawRequestPreview = rawReq,
                                        rawResponsePreview = update.rawDetails.ifBlank { rawResp }.take(4096)
                                    )
                                }
                                // A cause the person can fix gets a notice that leads there, instead of an attempt to retry.
                                update.kind?.let { kind ->
                                    runErrorNotice = RunErrorNotice(sessionId, kind)
                                    bannerNotification = Strings.runErrorNotice(kind)
                                }
                            }
                        }
                    }
                    break
                } catch (compactSignal: MidStreamAutoCompactSignal) {
                    pauseActiveThinkingStopwatch()
                    flushCheckpointToDisk(isFinal = false)
                    val compacted = executeAutoCompactionInPlace(
                        usagePct = compactSignal.usagePct,
                        isMidStream = true
                    )
                    if (!compacted) {
                        minTokensForNextAutoCompact = Int.MAX_VALUE
                    }
                    continue
                }
            }
        } catch (c: kotlinx.coroutines.CancellationException) {
            terminalPhase = if (processState.runs.isStopRequested(identity) || activeStreamsBySession[sessionId]?.phase == AgentRunPhase.STOPPED) {
                AgentRunPhase.STOPPED
            } else {
                AgentRunPhase.INTERRUPTED
            }
            runFlags.aborted = true
            networkDone = true
            kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) {
                pacerJob.cancel()
                pacerJob.join()
                flushCheckpointToDisk(isFinal = true)
            }
            throw c
        } catch (t: Throwable) {
            terminalPhase = AgentRunPhase.INTERRUPTED
            runFlags.aborted = true
            networkDone = true
            kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) {
                pacerJob.cancel()
                pacerJob.join()
            }
            val partialProse = networkContentTarget.toString()
            val hasPartialReasoningOrTools = networkThinkingTarget.isNotEmpty() || accumulatedTools.isNotEmpty()
            flushCheckpointToDisk(isFinal = true) { v ->
                val errLabel = t.message ?: t::class.simpleName ?: Strings.unknownError
                val notice = when {
                    partialProse.isNotBlank() -> "$partialProse\n\n*[Interrupted: $errLabel]*"
                    hasPartialReasoningOrTools -> Strings.interruptedTapContinueOrReplyContinue(errLabel)
                    else -> Strings.streamError(errLabel)
                }
                v.copy(
                    content = notice,
                    thinkingSummary = networkThinkingTarget.toString(),
                    trailingThinking = if (accumulatedTools.isNotEmpty()) currentStepThinkingTarget.toString().trim() else "",
                    thinkingSignature = signatureSb.toString(),
                    thinkingDurationMs = thinkingDurationMs,
                    toolCalls = v.toolCalls,
                    goalTracker = latestGoalTracker ?: v.goalTracker,
                    mediaItems = accumulatedMedia.toList(),
                    webSources = accumulatedWebSources,
                    isStreaming = false
                )
            }
        } finally {
            LiveTurns.end(targetVariantId)
            approvalObserver.cancel()
            retryObserver.cancel()
            networkDone = true
            if (processState.runs.isStopRequested(identity) || activeStreamsBySession[sessionId]?.phase == AgentRunPhase.STOPPED) {
                terminalPhase = AgentRunPhase.STOPPED
            }
            processState.runs.clearStopRequested(identity)
            kotlinx.coroutines.withContext(kotlinx.coroutines.NonCancellable) { approvalBroker.close() }
            val finishedOwnRun = processState.runs.finish(identity, terminalPhase)

            // Auto-generate Topic Title after the first assistant turn completes with non-empty content
            val updatedSess = appState.sessions.find { it.id == sessionId }
            if (finishedOwnRun && terminalPhase == AgentRunPhase.COMPLETED && networkContentTarget.isNotBlank() && updatedSess != null && !updatedSess.isTitleManuallySet && updatedSess.title == "New Chat" && updatedSess.messages.size in 2..3) {
                generateTopicTitle(sessionId, forceRegenerate = false)
            }

            // Send the next queued message for this chat, if there is one.
            val sessQueue = queuedPromptsBySession[sessionId].orEmpty()
            if (finishedOwnRun && terminalPhase == AgentRunPhase.COMPLETED && sessQueue.isNotEmpty()) {
                val nextItem = sessQueue.first()
                queuedPromptsBySession = queuedPromptsBySession + (sessionId to sessQueue.drop(1))
                dispatchNextQueuedItem?.invoke(sessionId, nextItem)
            }
        }
    }

    processState.runs.attach(identity,
        SessionStreamRuntime(
            sessionId = sessionId,
            targetNodeId = targetAssistantNodeId,
            isThinkingPhase = false,
            statusText = Strings.connectingTo(effectiveProfileForApi.name, modelName),
            interactionId = "",
            job = newJob,
            toolApprovalBroker = approvalBroker,
            retryControl = retryControl,
            phase = AgentRunPhase.PREPARING,
            providerContext = RunProviderContext(effectiveProfileForApi),
            actualWireDialect = identity.declaredWireDialect
        ),
        cancelPrevious = false
    )
    owningJob = newJob
    newJob.start()
}

/**
 * Coalesces per-delta status updates during SSE streaming so rapid chunks never flood Compose state.
 * Emits immediately on phase transitions or after explicit status/tool boundaries, then throttles by [minIntervalMs].
 */
internal class StreamStatusThrottle(private val minIntervalMs: Long = 100L) {
    private var lastPublishMs: Long = Long.MIN_VALUE
    private var lastThinkingPhase: Boolean? = null
    private var lastStatusText: String = ""

    fun onPhaseStatus(nowMs: Long, isThinkingPhase: Boolean, buildText: () -> String): String? {
        val phaseChanged = lastThinkingPhase != isThinkingPhase
        if (!phaseChanged && lastPublishMs != Long.MIN_VALUE && (nowMs - lastPublishMs) < minIntervalMs) {
            return null
        }
        val text = buildText()
        if (!phaseChanged && text == lastStatusText) {
            lastPublishMs = nowMs
            return null
        }
        lastThinkingPhase = isThinkingPhase
        lastPublishMs = nowMs
        lastStatusText = text
        return text
    }

    fun onExplicitStatus(statusText: String) {
        lastThinkingPhase = null
        lastPublishMs = Long.MIN_VALUE
        lastStatusText = statusText
    }
}
