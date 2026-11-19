package io.parchment.ai.network

import io.ktor.client.request.get
import io.ktor.client.statement.bodyAsText
import io.ktor.http.isSuccess
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderTokenUsage
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.keepsProviderState
import io.parchment.ai.network.AiProviderEngine.RawWebCitation
import io.parchment.ai.protocol.isGeminiServerSideToolSignature
import io.parchment.ai.protocol.sanitizeGeminiThoughtSignature
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

// Per-round SSE event parsing, background poll recovery, and completed object decoding for Gemini Interactions.

/** Signals an explicit server-side `"status": "failed"` interaction event so callers skip extra poll lookups. */
internal class TerminalInteractionFailedException(message: String) : RuntimeException(message)

internal class GeminiInteractionsRoundParser(
    private val engine: AiProviderEngine,
    private val profile: ProviderProfile,
    private val usageRequestIdProvider: () -> String?,
    private val onStatus: suspend (String, String) -> Unit,
    private val onFirstToken: () -> Unit,
    private val onThinking: suspend (String, String) -> Unit,
    private val onContent: suspend (String) -> Unit,
    private val onMedia: suspend (GeneratedMediaItem) -> Unit,
    private val onOutputLimit: suspend () -> Unit,
    private val onRawWire: suspend (String, String) -> Unit,
    private val onUsage: UsageReporter,
    private val onCitation: suspend (RawWebCitation) -> Unit,
    private val onInteractionIdObserved: (String) -> Unit,
    private val currentInteractionId: () -> String,
    private val pollDelayMsOverride: Long? = null
) {
    val turnToolCalls = mutableListOf<Pair<String, Pair<String, String>>>() // callId -> (name, argsJson)
    val roundThoughtSig = StringBuilder()
    private val stepSigByIndex = mutableMapOf<Int, StringBuilder>()
    val roundThoughtText = StringBuilder()
    val roundModelText = StringBuilder()
    var roundStatus = ""
    var roundContinuationToken = ""
    var receivedAnyStreamContent = false
    var streamFinished = false

    private val pendingFnArgsByIndex = mutableMapOf<Int, Pair<String, Pair<String, StringBuilder>>>()
    private val stepTypeByIndex = mutableMapOf<Int, String>()
    private var fallbackStreamStepType = ""
    private var streamedImagesCount = 0

    fun reset() {
        turnToolCalls.clear()
        roundThoughtSig.clear()
        stepSigByIndex.clear()
        roundThoughtText.clear()
        roundModelText.clear()
        roundStatus = ""
        roundContinuationToken = ""
        receivedAnyStreamContent = false
        streamFinished = false
        pendingFnArgsByIndex.clear()
        stepTypeByIndex.clear()
        fallbackStreamStepType = ""
        streamedImagesCount = 0
    }

    fun flushPendingFunctionCalls() {
        for ((_, pair) in pendingFnArgsByIndex.entries.sortedBy { it.key }) {
            addUniqueToolCall(pair.first, pair.second.first, pair.second.second.toString())
        }
    }

    suspend fun consumeSseEvent(eventType: String, dataLine: String, requestJsonStr: String) {
        if (dataLine == "[DONE]") return
        onRawWire(requestJsonStr, dataLine)
        val evObj = runCatching { engine.json.parseToJsonElement(dataLine).jsonObject }.getOrNull() ?: return
        val type = evObj["event_type"]?.jsonPrimitive?.contentOrNull
            ?: evObj["type"]?.jsonPrimitive?.contentOrNull
            ?: eventType

        val interactionSub = evObj["interaction"] as? JsonObject
        val idCandidate = interactionSub?.get("id")?.jsonPrimitive?.contentOrNull
            ?: evObj["interaction_id"]?.jsonPrimitive?.contentOrNull
            ?: evObj["id"]?.jsonPrimitive?.contentOrNull
        if (!idCandidate.isNullOrBlank()) {
            onInteractionIdObserved(idCandidate)
        }

        val eventUsage = evObj["usage"] as? JsonObject
            ?: (evObj["metadata"] as? JsonObject)?.get("total_usage") as? JsonObject
        if (eventUsage != null) {
            onUsage(
                usageRequestIdProvider(),
                "",
                UsageDialect.GEMINI_INTERACTIONS,
                parseProviderUsage(UsageDialect.GEMINI_INTERACTIONS, eventUsage, response = evObj)
            )
        }

        val interactionStatus = interactionSub?.get("status")?.jsonPrimitive?.contentOrNull
            ?: evObj["status"]?.jsonPrimitive?.contentOrNull

        when (type) {
            "interaction.start", "interaction.created", "interaction.status_update" -> {
                val persistedId = if (profile.keepsProviderState) idCandidate.orEmpty() else ""
                val st = interactionStatus ?: "streaming"
                val isTerminalStatus = st in setOf("completed", "requires_action", "requires_continuation", "incomplete", "failed", "cancelled", "expired")
                onUsage(
                    usageRequestIdProvider(),
                    persistedId,
                    UsageDialect.GEMINI_INTERACTIONS,
                    parseProviderUsage(UsageDialect.GEMINI_INTERACTIONS, interactionSub?.get("usage") as? JsonObject, isFinal = isTerminalStatus, response = interactionSub ?: evObj)
                )
                onStatus(Strings.interaction(st), if (profile.keepsProviderState) currentInteractionId() else "")
                if (isTerminalStatus) {
                    handleInteractionCompleted(evObj, interactionSub)
                }
            }
            "content.start", "step.start" -> handleContentStart(evObj)
            "content.delta", "step.delta" -> handleContentDelta(evObj)
            "content.stop", "step.stop", "content.complete", "step.complete" -> handleContentStop(evObj)
            "interaction.complete", "interaction.completed", "interaction.requires_action",
            "interaction.incomplete", "interaction.failed", "interaction.cancelled", "interaction.expired" ->
                handleInteractionCompleted(evObj, interactionSub)
            "error" -> {
                val errObj = evObj["error"] as? JsonObject
                val msg = errObj?.get("message")?.jsonPrimitive?.contentOrNull ?: dataLine
                throw buildStreamOrJsonError("Gemini Interactions Stream Error", errObj, msg)
            }
            else -> {
                val unknownStatus = interactionStatus.orEmpty()
                if (evObj.containsKey("outputs") ||
                    evObj.containsKey("steps") ||
                    unknownStatus in setOf("completed", "requires_action", "requires_continuation", "incomplete", "failed", "cancelled", "expired")
                ) {
                    handleInteractionCompleted(evObj, interactionSub)
                }
            }
        }
    }

    private fun isNonRetryableGeminiError(code: Int?, status: String): Boolean {
        if (code != null && code in setOf(400, 401, 403, 404, 413, 422)) return true
        return status.trim().uppercase() in setOf(
            "INVALID_ARGUMENT",
            "PERMISSION_DENIED",
            "UNAUTHENTICATED",
            "NOT_FOUND",
            "FAILED_PRECONDITION",
            "INVALID_REQUEST",
            "SAFETY",
            "PROHIBITED_CONTENT"
        )
    }

    private fun buildStreamOrJsonError(prefix: String, errObj: JsonObject?, msg: String): RuntimeException {
        val code = errObj?.get("code")?.jsonPrimitive?.intOrNull
        val errStatus = (errObj?.get("status") as? JsonPrimitive)?.contentOrNull.orEmpty()
        val tag = errStatus.ifBlank { code?.toString().orEmpty() }
        val formatted = if (tag.isNotBlank()) "$prefix ($tag): $msg" else "$prefix: $msg"
        return if (isNonRetryableGeminiError(code, errStatus)) {
            AiProviderEngine.NoRetryException(formatted)
        } else {
            IllegalStateException(formatted)
        }
    }

    private fun throwIfInteractionFailed(status: String, obj: JsonObject, fallbackDetail: String = "") {
        if (status == "failed" || status == "cancelled" || status == "expired") {
            val errObj = obj["error"] as? JsonObject
            val msg = (errObj?.get("message") as? JsonPrimitive)?.contentOrNull
                ?: fallbackDetail.ifBlank { "status=$status" }
            val code = errObj?.get("code")?.jsonPrimitive?.intOrNull
            val errStatus = (errObj?.get("status") as? JsonPrimitive)?.contentOrNull.orEmpty()
            if (status == "cancelled" || status == "expired" || isNonRetryableGeminiError(code, errStatus)) {
                throw AiProviderEngine.NoRetryException("Gemini Interactions $status: $msg")
            }
            throw TerminalInteractionFailedException("Gemini Interactions $status: $msg")
        }
    }

    suspend fun pollUntilComplete(
        interactionIdToPoll: String,
        pollBasePath: String,
        requestJsonStr: String
    ) {
        var pollCount = 0
        var transientErrors = 0
        var status = "in_progress"
        var interactionObj = JsonObject(emptyMap())
        while ((status == "in_progress" || status == "queued" || status == "processing") &&
            interactionIdToPoll.isNotBlank() &&
            pollCount < AiProviderEngine.MAX_POLL_ITERATIONS
        ) {
            pollCount++
            onStatus(Strings.backgroundPollRecoveryPoll(status, pollCount), interactionIdToPoll)
            if (transientErrors == 0) delay(pollDelayMsOverride ?: 2000L)
            val pollUrl = engine.buildUrlWithAuthQuery(profile, "$pollBasePath/$interactionIdToPoll")
            try {
                val pollResp = engine.httpClient.get(pollUrl) {
                    with(engine) { applyAuthAndCustomHeaders(profile) }
                }
                val pollText = pollResp.bodyAsText()
                onRawWire(requestJsonStr, pollText)
                if (!pollResp.status.isSuccess()) {
                    throw providerHttpError("Gemini Interactions Poll", pollResp, pollText)
                }
                interactionObj = engine.json.parseToJsonElement(pollText).jsonObject
                status = (interactionObj["status"] as? JsonPrimitive)?.contentOrNull ?: "completed"
                transientErrors = 0
            } catch (e: CancellationException) {
                throw e
            } catch (pollErr: Throwable) {
                if (!engine.isTransientOrProxyTimeoutError(pollErr)) {
                    throw AiProviderEngine.AcceptedJobException(
                        "Gemini Interactions poll rejected for background job $interactionIdToPoll: ${pollErr.message ?: pollErr}",
                        pollErr
                    )
                }
                transientErrors++
                if (transientErrors > AiProviderEngine.MAX_POLL_TRANSIENT_ERRORS) {
                    throw AiProviderEngine.AcceptedJobException(
                        "Gemini Interactions poll failed after $transientErrors attempts for job $interactionIdToPoll: ${pollErr.message ?: pollErr}",
                        pollErr
                    )
                }
                onStatus(
                    "Poll error ($transientErrors/${AiProviderEngine.MAX_POLL_TRANSIENT_ERRORS}) — retrying status check…",
                    interactionIdToPoll
                )
                delay(pollDelayMsOverride ?: (transientErrors * 500L).coerceAtMost(2_500L))
            }
        }
        if (status == "in_progress" || status == "queued" || status == "processing") {
            throw AiProviderEngine.AcceptedJobException(
                "Gemini Interactions background job $interactionIdToPoll did not finish within $pollCount polls."
            )
        }
        throwIfInteractionFailed(status, interactionObj)
        if (interactionObj.isNotEmpty()) {
            roundStatus = status
            roundContinuationToken = (interactionObj["continuation_token"] as? JsonPrimitive)?.contentOrNull.orEmpty()
            if (roundStatus == "incomplete") onOutputLimit()
            applyCompletedInteractionObject(interactionObj)
        }
    }

    suspend fun handlePlainJsonFallback(
        fullBody: String,
        pollBasePath: String,
        requestJsonStr: String
    ) {
        onRawWire(requestJsonStr, fullBody)
        val rootObj = engine.json.parseToJsonElement(fullBody).jsonObject
        val errObj = rootObj["error"] as? JsonObject
        if (errObj != null) {
            val msg = (errObj["message"] as? JsonPrimitive)?.contentOrNull ?: fullBody
            throw buildStreamOrJsonError("Gemini Interactions Error", errObj, msg)
        }
        val id = (rootObj["id"] as? JsonPrimitive)?.contentOrNull ?: ""
        if (id.isNotBlank()) {
            onInteractionIdObserved(id)
        }
        val st = (rootObj["status"] as? JsonPrimitive)?.contentOrNull ?: "completed"
        roundStatus = st
        roundContinuationToken = (rootObj["continuation_token"] as? JsonPrimitive)?.contentOrNull.orEmpty()
        throwIfInteractionFailed(st, rootObj, fullBody)
        val trackedId = id.ifBlank { currentInteractionId() }
        if (profile.keepsProviderState && (st == "in_progress" || st == "queued" || st == "processing") && trackedId.isNotBlank()) {
            pollUntilComplete(trackedId, pollBasePath, requestJsonStr)
        } else {
            if (st == "incomplete") onOutputLimit()
            applyCompletedInteractionObject(rootObj)
        }
    }

    fun appendContinuationOutputSteps(extraLoopSteps: MutableList<JsonObject>) {
        val cleanSig = sanitizeGeminiThoughtSignature(roundThoughtSig.toString(), forFunctionCall = false)
        if (cleanSig.isNotEmpty()) {
            extraLoopSteps.add(geminiThoughtStep(signature = cleanSig, summary = roundThoughtText.toString()))
        }
        if (roundModelText.isNotEmpty()) {
            extraLoopSteps.add(geminiModelOutputStep(roundModelText.toString()))
        }
    }

    fun appendStopHookSteps(extraLoopSteps: MutableList<JsonObject>, stopHookPrompt: String) {
        val cleanStopSig = sanitizeGeminiThoughtSignature(roundThoughtSig.toString(), forFunctionCall = false)
        if (cleanStopSig.isNotEmpty()) {
            extraLoopSteps.add(geminiThoughtStep(signature = cleanStopSig, summary = roundThoughtText.toString()))
        }
        if (roundModelText.isNotEmpty()) {
            extraLoopSteps.add(geminiModelOutputStep(roundModelText.toString()))
        }
        extraLoopSteps.add(geminiUserInputStep(stopHookPrompt))
    }

    fun appendToolRoundSteps(
        extraLoopSteps: MutableList<JsonObject>,
        stepSigString: String,
        results: List<ToolCallRecord>
    ) {
        if (stepSigString.isNotEmpty()) {
            extraLoopSteps.add(geminiThoughtStep(signature = stepSigString, summary = roundThoughtText.toString()))
        }
        if (roundModelText.isNotEmpty()) {
            extraLoopSteps.add(geminiModelOutputStep(roundModelText.toString()))
        }
        for ((callId, fnPair) in turnToolCalls) {
            val parsedArgs = runCatching { engine.json.parseToJsonElement(fnPair.second).jsonObject }
                .getOrElse { JsonObject(emptyMap()) }
            extraLoopSteps.add(geminiFunctionCallStep(callId = callId, name = fnPair.first, arguments = parsedArgs))
        }
        for (record in results) {
            extraLoopSteps.add(
                geminiFunctionResultStep(
                    callId = record.id,
                    name = record.name,
                    isError = record.isError,
                    resultText = record.resultText
                )
            )
        }
        if (results.any { it.hasScreenshot }) extraLoopSteps.dropOlderScreenshotEntries()
        results.filter { it.hasScreenshot }.forEach { extraLoopSteps.add(interactionsScreenshotStep(it)) }
    }

    private suspend fun handleContentStart(evObj: JsonObject) {
        onUsage(
            usageRequestIdProvider(),
            "",
            UsageDialect.GEMINI_INTERACTIONS,
            parseProviderUsage(UsageDialect.GEMINI_INTERACTIONS, null, response = evObj)
        )
        val idx = evObj["index"]?.jsonPrimitive?.intOrNull ?: 0
        val contentObj = (evObj["content"] as? JsonObject) ?: (evObj["step"] as? JsonObject)
        val cType = contentObj?.get("type")?.jsonPrimitive?.contentOrNull
            ?: evObj["type"]?.jsonPrimitive?.contentOrNull.takeIf { it != "content.start" && it != "step.start" }
            ?: ""
        if (cType.isNotBlank()) {
            stepTypeByIndex[idx] = cType
            fallbackStreamStepType = cType
        }
        when (cType) {
            "function_call" -> {
                val callId = contentObj?.get("id")?.jsonPrimitive?.contentOrNull
                    ?: contentObj?.get("call_id")?.jsonPrimitive?.contentOrNull
                    ?: "call_$idx"
                val fnName = contentObj?.get("name")?.jsonPrimitive?.contentOrNull ?: ""
                val argEl = contentObj?.get("arguments") ?: contentObj?.get("arguments_delta")
                val initialArgs = when {
                    argEl is JsonPrimitive -> argEl.content
                    argEl is JsonObject && argEl.isNotEmpty() -> argEl.toString()
                    else -> ""
                }
                pendingFnArgsByIndex[idx] = callId to (fnName to StringBuilder(initialArgs))
            }
            "thought" -> {
                val sig = contentObj?.get("signature")?.jsonPrimitive?.contentOrNull ?: ""
                val uniqueSig = emitUniqueRoundSig(sig, idx)
                if (uniqueSig.isNotEmpty()) {
                    onThinking("", uniqueSig)
                }
                val summaryArr = (contentObj?.get("summary") as? JsonArray)
                    ?: (contentObj?.get("content") as? JsonArray)
                summaryArr?.forEach { sEl ->
                    val txt = (sEl as? JsonObject)?.get("text")?.jsonPrimitive?.contentOrNull ?: ""
                    emitThoughtDelta(txt)
                }
            }
            "model_output", "text" -> {
                emitContentDelta(contentObj?.get("text")?.jsonPrimitive?.contentOrNull ?: "")
                val contentArr = contentObj?.get("content") as? JsonArray
                contentArr?.forEach { cEl ->
                    emitContentDelta((cEl as? JsonObject)?.get("text")?.jsonPrimitive?.contentOrNull ?: "")
                }
            }
        }
        engine.extractCitationsFromGeminiObject(contentObj, onCitation)
    }

    private suspend fun handleContentDelta(evObj: JsonObject) {
        val idx = evObj["index"]?.jsonPrimitive?.intOrNull ?: 0
        val stepTypeForIdx = stepTypeByIndex[idx] ?: fallbackStreamStepType
        val deltaObj = evObj["delta"] as? JsonObject ?: evObj
        val deltaType = deltaObj["type"]?.jsonPrimitive?.contentOrNull ?: ""
        val sigDelta = deltaObj["signature"]?.jsonPrimitive?.contentOrNull ?: ""
        val uniqueSig = emitUniqueRoundSig(sigDelta, idx)
        if (uniqueSig.isNotEmpty()) {
            onThinking("", uniqueSig)
        }
        val deltaContentArr = deltaObj["content"] as? JsonArray
        if (deltaContentArr != null) {
            deltaContentArr.forEach { partEl ->
                val partObj = partEl as? JsonObject ?: return@forEach
                when (partObj["type"]?.jsonPrimitive?.contentOrNull ?: "text") {
                    "text" -> emitStepAwareText(partObj["text"]?.jsonPrimitive?.contentOrNull ?: "", stepTypeForIdx, deltaType)
                    "image" -> emitImageFromObject(partObj)
                }
            }
        } else {
            val hasArgsDelta = deltaObj.containsKey("arguments_delta") ||
                deltaObj.containsKey("arguments") ||
                deltaObj.containsKey("partial_json")
            when {
                deltaType == "text" ->
                    emitStepAwareText(deltaObj["text"]?.jsonPrimitive?.contentOrNull ?: "", stepTypeForIdx, deltaType = "")
                deltaType == "thought_summary" || deltaType == "thought" || deltaType == "thinking" -> {
                    val thoughtTxt = (deltaObj["content"] as? JsonObject)?.get("text")?.jsonPrimitive?.contentOrNull
                        ?: deltaObj["text"]?.jsonPrimitive?.contentOrNull
                        ?: deltaObj["thought"]?.jsonPrimitive?.contentOrNull
                        ?: ""
                    emitThoughtDelta(thoughtTxt)
                }
                deltaType == "thought_signature" -> Unit // Already handled via uniqueSig above
                deltaType == "image" -> emitImageFromObject(deltaObj)
                deltaType == "function_call" ||
                    deltaType == "arguments_delta" ||
                    (stepTypeForIdx == "function_call" && hasArgsDelta) -> accumulateFunctionCallDelta(idx, deltaObj)
                else -> {
                    val fallbackTxt = deltaObj["text"]?.jsonPrimitive?.contentOrNull
                    if (!fallbackTxt.isNullOrEmpty()) {
                        emitStepAwareText(fallbackTxt, stepTypeForIdx, deltaType = "")
                    }
                }
            }
        }
        engine.extractCitationsFromGeminiObject(deltaObj, onCitation)
    }

    private fun accumulateFunctionCallDelta(idx: Int, deltaObj: JsonObject) {
        val existing = pendingFnArgsByIndex[idx]
        val callId = deltaObj["id"]?.jsonPrimitive?.contentOrNull
            ?: deltaObj["call_id"]?.jsonPrimitive?.contentOrNull
            ?: existing?.first
            ?: "call_$idx"
        val fnName = deltaObj["name"]?.jsonPrimitive?.contentOrNull
            ?: existing?.second?.first
            ?: ""
        val rawArgEl = deltaObj["arguments_delta"]
            ?: deltaObj["arguments"]
            ?: deltaObj["partial_json"]
        when (rawArgEl) {
            is JsonPrimitive -> {
                val argChunk = rawArgEl.content
                if (existing == null) {
                    pendingFnArgsByIndex[idx] = callId to (fnName to StringBuilder(argChunk))
                } else {
                    val updatedName = fnName.ifBlank { existing.second.first }
                    existing.second.second.append(argChunk)
                    pendingFnArgsByIndex[idx] = callId to (updatedName to existing.second.second)
                }
            }
            is JsonObject -> {
                val prevObj = existing?.second?.second?.toString()
                    ?.let { runCatching { engine.json.parseToJsonElement(it).jsonObject }.getOrNull() }
                val mergedObj = if (prevObj != null) engine.deepMergeJsonObjects(prevObj, rawArgEl) else rawArgEl
                val updatedName = fnName.ifBlank { existing?.second?.first.orEmpty() }
                pendingFnArgsByIndex[idx] = callId to (updatedName to StringBuilder(mergedObj.toString()))
            }
            else -> {
                if (existing == null && (callId.isNotBlank() || fnName.isNotBlank())) {
                    pendingFnArgsByIndex[idx] = callId to (fnName to StringBuilder())
                } else if (existing != null && fnName.isNotBlank()) {
                    pendingFnArgsByIndex[idx] = callId to (fnName to existing.second.second)
                }
            }
        }
    }

    private suspend fun handleContentStop(evObj: JsonObject) {
        val idx = evObj["index"]?.jsonPrimitive?.intOrNull ?: 0
        val contentObj = (evObj["content"] as? JsonObject) ?: (evObj["step"] as? JsonObject)
        if (contentObj?.get("type")?.jsonPrimitive?.contentOrNull == "function_call") {
            val callId = contentObj["id"]?.jsonPrimitive?.contentOrNull
                ?: contentObj["call_id"]?.jsonPrimitive?.contentOrNull
                ?: pendingFnArgsByIndex[idx]?.first
                ?: "call_$idx"
            val fnName = contentObj["name"]?.jsonPrimitive?.contentOrNull
                ?: pendingFnArgsByIndex[idx]?.second?.first
                ?: ""
            val argEl = contentObj["arguments"]
            val argsStr = when {
                argEl is JsonPrimitive -> argEl.content
                argEl is JsonObject && argEl.isNotEmpty() -> argEl.toString()
                else -> pendingFnArgsByIndex[idx]?.second?.second?.toString().orEmpty()
            }
            if (fnName.isNotBlank()) {
                pendingFnArgsByIndex[idx] = callId to (fnName to StringBuilder(argsStr))
            }
        }
        engine.extractCitationsFromGeminiObject(contentObj, onCitation)
    }

    private suspend fun handleInteractionCompleted(evObj: JsonObject, interactionSub: JsonObject?) {
        streamFinished = true
        val completedObj = interactionSub ?: evObj
        roundStatus = (completedObj["status"] as? JsonPrimitive)?.contentOrNull
            ?: (evObj["status"] as? JsonPrimitive)?.contentOrNull.orEmpty()
        roundContinuationToken = (completedObj["continuation_token"] as? JsonPrimitive)?.contentOrNull
            ?: (evObj["continuation_token"] as? JsonPrimitive)?.contentOrNull.orEmpty()
        throwIfInteractionFailed(roundStatus, completedObj)
        if (roundStatus == "incomplete") onOutputLimit()
        flushPendingFunctionCalls()
        val itemsArray = (completedObj["steps"] as? JsonArray) ?: (completedObj["outputs"] as? JsonArray)
        itemsArray?.forEach { itemEl ->
            val item = itemEl as? JsonObject ?: return@forEach
            when (item["type"]?.jsonPrimitive?.contentOrNull) {
                "function_call" -> {
                    val callId = item["id"]?.jsonPrimitive?.contentOrNull
                        ?: item["call_id"]?.jsonPrimitive?.contentOrNull ?: ""
                    val name = item["name"]?.jsonPrimitive?.contentOrNull ?: ""
                    val args = item["arguments"]?.let {
                        if (it is JsonPrimitive) it.content else it.toString()
                    } ?: "{}"
                    addUniqueToolCall(callId, name, args)
                }
                "thought", "thought_summary" -> {
                    val sig = item["signature"]?.jsonPrimitive?.contentOrNull ?: ""
                    val uniqueSig = emitUniqueRoundSig(sig)
                    if (uniqueSig.isNotEmpty()) {
                        onThinking("", uniqueSig)
                    }
                }
            }
            engine.extractCitationsFromGeminiObject(item, onCitation)
        }
        engine.extractCitationsFromGeminiObject(completedObj, onCitation)
        val finalText = engine.extractGeminiInteractionText(completedObj)
        val streamed = roundModelText.toString()
        if (finalText.length > streamed.length && finalText.startsWith(streamed)) {
            emitContentDelta(finalText.substring(streamed.length))
        }
        val usageObj = completedObj["usage"] as? JsonObject
        val persistedId = if (profile.keepsProviderState) {
            (completedObj["id"] as? JsonPrimitive)?.contentOrNull ?: currentInteractionId()
        } else ""
        onUsage(
            usageRequestIdProvider(),
            persistedId,
            UsageDialect.GEMINI_INTERACTIONS,
            parseProviderUsage(UsageDialect.GEMINI_INTERACTIONS, usageObj, isFinal = true, response = completedObj)
        )
    }

    private suspend fun applyCompletedInteractionObject(interactionObj: JsonObject) {
        val streamedText = roundModelText.toString()
        val streamedThought = roundThoughtText.toString()
        val polledText = StringBuilder()
        val polledThought = StringBuilder()
        var polledImageCount = 0
        engine.parseCompletedGeminiInteractionObject(
            interactionObj = interactionObj,
            onFirstToken = onFirstToken,
            onThinking = { th, sig ->
                val thoughtDelta = if (th.isNotEmpty()) {
                    val prevLen = polledThought.length
                    polledThought.append(th)
                    val combined = polledThought.toString()
                    when {
                        streamedThought.isEmpty() -> th
                        combined.length <= streamedThought.length && streamedThought.startsWith(combined) -> ""
                        combined.startsWith(streamedThought) -> combined.substring(maxOf(prevLen, streamedThought.length))
                        else -> if (prevLen >= streamedThought.length) th else ""
                    }
                } else ""
                if (thoughtDelta.isNotEmpty()) roundThoughtText.append(thoughtDelta)
                val uniqueSig = emitUniqueRoundSig(sig)
                if (thoughtDelta.isNotEmpty() || uniqueSig.isNotEmpty()) {
                    onThinking(thoughtDelta, uniqueSig)
                }
            },
            onContent = { txt ->
                val prevLen = polledText.length
                polledText.append(txt)
                val combined = polledText.toString()
                val deltaToEmit = when {
                    streamedText.isEmpty() -> txt
                    combined.length <= streamedText.length && streamedText.startsWith(combined) -> ""
                    combined.startsWith(streamedText) -> combined.substring(maxOf(prevLen, streamedText.length))
                    else -> if (prevLen >= streamedText.length) txt else ""
                }
                if (deltaToEmit.isNotEmpty()) {
                    receivedAnyStreamContent = true
                    roundModelText.append(deltaToEmit)
                    onContent(deltaToEmit)
                }
            },
            onMedia = { item ->
                if (polledImageCount++ >= streamedImagesCount) {
                    streamedImagesCount++
                    onMedia(item)
                }
            },
            onFunctionCall = ::addUniqueToolCall,
            onUsage = { id, usage ->
                if (profile.keepsProviderState && id.isNotBlank()) onInteractionIdObserved(id)
                onUsage(usageRequestIdProvider(), id, UsageDialect.GEMINI_INTERACTIONS, usage)
            },
            onCitation = onCitation
        )
    }

    private suspend fun emitStepAwareText(txt: String, stepType: String, deltaType: String) {
        if (txt.isEmpty()) return
        if (stepType == "thought" || deltaType == "thought_summary") {
            emitThoughtDelta(txt)
        } else {
            emitContentDelta(txt)
        }
    }

    private suspend fun emitThoughtDelta(txt: String) {
        if (txt.isEmpty()) return
        onFirstToken()
        roundThoughtText.append(txt)
        onThinking(txt, "")
    }

    private suspend fun emitContentDelta(txt: String) {
        if (txt.isEmpty()) return
        onFirstToken()
        receivedAnyStreamContent = true
        roundModelText.append(txt)
        onContent(txt)
    }

    private suspend fun emitImageFromObject(obj: JsonObject) {
        val b64 = (obj["data"] as? JsonPrimitive)?.contentOrNull ?: ""
        val uri = (obj["uri"] as? JsonPrimitive)?.contentOrNull ?: (obj["url"] as? JsonPrimitive)?.contentOrNull ?: ""
        val mime = (obj["mime_type"] as? JsonPrimitive)?.contentOrNull ?: "image/png"
        if (b64.isNotBlank() || uri.isNotBlank()) streamedImagesCount++
        engine.saveAndEmitImage(b64, uri, mime, profile.mediaAspectRatio, "", onMedia)
    }

    private fun addUniqueToolCall(callId: String, fnName: String, argsStr: String) {
        if (fnName.isBlank()) return
        val cleanArgs = argsStr.ifBlank { "{}" }
        val existingIdx = turnToolCalls.indexOfFirst { it.first == callId && callId.isNotBlank() }
        if (existingIdx >= 0) {
            val prev = turnToolCalls[existingIdx]
            if ((prev.second.second == "{}" || prev.second.second.isBlank()) && cleanArgs != "{}") {
                turnToolCalls[existingIdx] = callId to (fnName to cleanArgs)
            }
        } else {
            turnToolCalls.add(callId to (fnName to cleanArgs))
        }
    }

    private fun emitUniqueRoundSig(sig: String, stepIdx: Int = -1): String {
        if (sig.isEmpty()) return ""
        if (stepIdx >= 0) {
            val stepBuilder = stepSigByIndex.getOrPut(stepIdx) { StringBuilder() }
            val emitted = engine.appendSignatureChunkSafe(stepBuilder, sig)
            val fullStepSig = stepBuilder.toString()
            if (!isGeminiServerSideToolSignature(fullStepSig) || roundThoughtSig.isEmpty()) {
                roundThoughtSig.setLength(0)
                roundThoughtSig.append(fullStepSig)
            }
            return emitted
        }
        return engine.appendSignatureChunkSafe(roundThoughtSig, sig)
    }
}

internal suspend fun AiProviderEngine.parseCompletedGeminiInteractionObject(
    interactionObj: JsonObject,
    onFirstToken: () -> Unit,
    onThinking: suspend (String, String) -> Unit,
    onContent: suspend (String) -> Unit,
    onMedia: suspend (GeneratedMediaItem) -> Unit,
    onFunctionCall: (String, String, String) -> Unit,
    onUsage: suspend (String, ProviderTokenUsage) -> Unit,
    onCitation: suspend (RawWebCitation) -> Unit = {},
    sessionId: String = ""
) {
    val id = interactionObj["id"]?.jsonPrimitive?.contentOrNull ?: ""
    val itemsArray = (interactionObj["steps"] as? JsonArray)
        ?: (interactionObj["outputs"] as? JsonArray)
    itemsArray?.forEach { itemEl ->
        val item = itemEl as? JsonObject ?: return@forEach
        when (item["type"]?.jsonPrimitive?.contentOrNull) {
            "text" -> {
                val txt = item["text"]?.jsonPrimitive?.contentOrNull ?: ""
                if (txt.isNotEmpty()) {
                    onFirstToken()
                    onContent(txt)
                }
            }
            "model_output" -> {
                val contentArr = item["content"] as? JsonArray
                contentArr?.forEach { cEl ->
                    val cObj = cEl as? JsonObject ?: return@forEach
                    when (cObj["type"]?.jsonPrimitive?.contentOrNull) {
                        "text", null -> {
                            val txt = cObj["text"]?.jsonPrimitive?.contentOrNull ?: ""
                            if (txt.isNotEmpty()) {
                                onFirstToken()
                                onContent(txt)
                            }
                        }
                        "image" -> emitCompletedInteractionImage(cObj, onMedia, sessionId)
                    }
                }
            }
            "image" -> emitCompletedInteractionImage(item, onMedia, sessionId)
            "thought", "thought_summary" -> {
                val summary = item["summary"]?.let { s ->
                    when (s) {
                        is JsonPrimitive -> s.content
                        is JsonArray -> s.joinToString("\n") {
                            ((it as? JsonObject)?.get("text") as? JsonPrimitive)?.contentOrNull ?: it.toString()
                        }
                        else -> s.toString()
                    }
                } ?: item["text"]?.jsonPrimitive?.contentOrNull ?: ""
                val sig = item["signature"]?.jsonPrimitive?.contentOrNull ?: ""
                if (summary.isNotEmpty() || sig.isNotEmpty()) {
                    onFirstToken()
                    onThinking(summary, sig)
                }
            }
            "function_call" -> {
                val callId = item["id"]?.jsonPrimitive?.contentOrNull
                    ?: item["call_id"]?.jsonPrimitive?.contentOrNull
                    ?: "call_${kotlin.random.Random.nextInt(1000, 9999)}"
                val name = item["name"]?.jsonPrimitive?.contentOrNull ?: ""
                val args = item["arguments"]?.let {
                    if (it is JsonPrimitive) it.content else it.toString()
                } ?: "{}"
                if (name.isNotBlank()) {
                    onFunctionCall(callId, name, args)
                }
            }
        }
        extractCitationsFromGeminiObject(item, onCitation)
    }
    extractCitationsFromGeminiObject(interactionObj, onCitation)

    val usageObj = interactionObj["usage"] as? JsonObject
    onUsage(id, parseProviderUsage(UsageDialect.GEMINI_INTERACTIONS, usageObj, isFinal = true, response = interactionObj))
}

private suspend fun AiProviderEngine.emitCompletedInteractionImage(
    obj: JsonObject,
    onMedia: suspend (GeneratedMediaItem) -> Unit,
    sessionId: String
) {
    val b64 = obj["data"]?.jsonPrimitive?.contentOrNull ?: ""
    val uri = obj["uri"]?.jsonPrimitive?.contentOrNull ?: obj["url"]?.jsonPrimitive?.contentOrNull ?: ""
    val mime = obj["mime_type"]?.jsonPrimitive?.contentOrNull ?: "image/png"
    saveAndEmitImage(b64, uri, mime, "", "", onMedia, sessionId = sessionId)
}
