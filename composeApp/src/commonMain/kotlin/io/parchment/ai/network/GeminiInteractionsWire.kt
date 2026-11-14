package io.parchment.ai.network

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.turnTextLayout
import io.parchment.ai.network.AiProviderEngine.ResolvedBinaryAttachment
import io.parchment.ai.protocol.sanitizeGeminiThoughtSignature
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

// Gemini Interactions API step construction, history turn serialization, and rolling step compaction.

internal fun geminiThoughtStep(signature: String, summary: String = ""): JsonObject = buildJsonObject {
    put("type", "thought")
    put("signature", signature)
    if (summary.isNotBlank()) {
        putJsonArray("summary") {
            add(buildJsonObject {
                put("type", "text")
                put("text", summary)
            })
        }
    }
}

internal fun geminiModelOutputStep(text: String): JsonObject = buildJsonObject {
    put("type", "model_output")
    putJsonArray("content") {
        add(buildJsonObject {
            put("type", "text")
            put("text", text)
        })
    }
}

internal fun geminiUserInputStep(text: String): JsonObject = buildJsonObject {
    put("type", "user_input")
    putJsonArray("content") {
        add(buildJsonObject {
            put("type", "text")
            put("text", text)
        })
    }
}

internal fun geminiFunctionCallStep(callId: String, name: String, arguments: JsonObject): JsonObject = buildJsonObject {
    put("type", "function_call")
    put("id", callId)
    put("name", name)
    put("arguments", arguments)
}

internal fun geminiFunctionResultStep(
    callId: String,
    name: String,
    isError: Boolean,
    resultText: String
): JsonObject = buildJsonObject {
    put("type", "function_result")
    put("name", name)
    put("call_id", callId)
    put("is_error", isError)
    putJsonObject("result") {
        put("result", resultText)
    }
}

internal fun AiProviderEngine.extractGeminiInteractionText(interaction: JsonObject): String = buildString {
    val steps = interaction["steps"] as? JsonArray ?: interaction["outputs"] as? JsonArray
    steps?.forEach stepLoop@{ element ->
        val step = element as? JsonObject ?: return@stepLoop
        when (step["type"]?.jsonPrimitive?.contentOrNull) {
            "text" -> append(step["text"]?.jsonPrimitive?.contentOrNull.orEmpty())
            "model_output" -> (step["content"] as? JsonArray)?.forEach contentLoop@{ content ->
                val block = content as? JsonObject ?: return@contentLoop
                if (block["type"]?.jsonPrimitive?.contentOrNull in listOf("text", null)) {
                    append(block["text"]?.jsonPrimitive?.contentOrNull.orEmpty())
                }
            }
        }
    }
}

/**
 * Builds the official Gemini Interactions API `input` Steps array:
 * - User turns -> `{"type": "user_input", "content": [{"type": "text"|"image"|"video"|"audio"|"document", ...}]}`
 *   (automatically separating consecutive `user_input` turns with `\n\n` so prompts never glue together)
 * - Assistant turns -> chronological `thought` (`signature` + `summary`) + `function_call` + `function_result` + `model_output` steps.
 */
internal fun AiProviderEngine.buildGeminiInteractionsTurnsArray(
    nodes: List<MessageNode>,
    session: ChatSession,
    continuePrefix: String,
    foldHistoricalToolsToText: Boolean = false
): JsonArray {
    val stepsList = mutableListOf<JsonObject>()
    val replayIds = ReplayToolCallIds()
    val withAuthorsNote = injectAuthorsNoteIntoHistory(nodes, session)
    val lastAssistantIdx = nodes.indexOfLast { it.role == MessageRole.ASSISTANT }

    for ((idx, pair) in withAuthorsNote.withIndex()) {
        val (role, text) = pair
        val nodeVar = nodes.getOrNull(idx)?.activeVariant
        val binaryAttachments = resolveBinaryAttachments(nodeVar)

        if (role == MessageRole.ASSISTANT) {
            appendAssistantInteractionSteps(
                stepsList = stepsList,
                replayIds = replayIds,
                text = text,
                nodeVar = nodeVar,
                isOlderAssistantTurn = idx < lastAssistantIdx,
                foldHistoricalToolsToText = foldHistoricalToolsToText
            )
        } else {
            appendMergedUserInteractionStep(stepsList, text, binaryAttachments)
        }
    }

    val cleanContinue = stripInlineErrorNotices(continuePrefix)
    if (cleanContinue.isNotBlank()) {
        appendMergedUserInteractionStep(
            stepsList = stepsList,
            text = "[System Note: Continue the narrative seamlessly from the exact end of this partial passage without repeating it:\n\"$cleanContinue\"]",
            attachments = emptyList()
        )
    }
    return JsonArray(stepsList)
}

private fun appendMergedUserInteractionStep(
    stepsList: MutableList<JsonObject>,
    text: String,
    attachments: List<ResolvedBinaryAttachment>
) {
    val newContentItems = mutableListOf<JsonObject>()
    if (text.isNotBlank()) {
        newContentItems.add(buildJsonObject {
            put("type", "text")
            put("text", text)
        })
    }
    for (att in attachments) {
        val m = att.mimeType.lowercase()
        val cType = when {
            m.startsWith("video/") -> "video"
            m.startsWith("audio/") -> "audio"
            m == "application/pdf" || m.contains("document") || m.contains("msword") ||
                m.contains("officedocument") || m == "text/csv" -> "document"
            else -> "image"
        }
        newContentItems.add(buildJsonObject {
            put("type", cType)
            put("mime_type", att.mimeType)
            put("data", att.base64Data)
        })
    }
    if (newContentItems.isEmpty()) {
        newContentItems.add(buildJsonObject {
            put("type", "text")
            put("text", " ")
        })
    }

    val lastStep = stepsList.lastOrNull()
    if (lastStep != null && lastStep["type"]?.jsonPrimitive?.contentOrNull == "user_input") {
        val prevContent = (lastStep["content"] as? JsonArray)?.mapNotNull { it as? JsonObject }.orEmpty().toMutableList()
        val existingTextIdx = prevContent.indexOfLast { it["type"]?.jsonPrimitive?.contentOrNull == "text" }
        val firstNewItem = newContentItems.firstOrNull()
        if (existingTextIdx >= 0 && firstNewItem != null &&
            firstNewItem["type"]?.jsonPrimitive?.contentOrNull == "text"
        ) {
            val prevTxt = prevContent[existingTextIdx]["text"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val newTxt = firstNewItem["text"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val joined = if (prevTxt.isBlank()) newTxt else if (newTxt.isBlank()) prevTxt else "$prevTxt\n\n$newTxt"
            prevContent[existingTextIdx] = buildJsonObject {
                put("type", "text")
                put("text", joined)
            }
            prevContent.addAll(newContentItems.drop(1))
        } else {
            prevContent.addAll(newContentItems)
        }
        stepsList[stepsList.lastIndex] = buildJsonObject {
            put("type", "user_input")
            put("content", JsonArray(prevContent))
        }
    } else {
        stepsList.add(buildJsonObject {
            put("type", "user_input")
            put("content", JsonArray(newContentItems))
        })
    }
}

private fun AiProviderEngine.appendAssistantInteractionSteps(
    stepsList: MutableList<JsonObject>,
    replayIds: ReplayToolCallIds,
    text: String,
    nodeVar: io.parchment.ai.model.MessageVariant?,
    isOlderAssistantTurn: Boolean,
    foldHistoricalToolsToText: Boolean
) {
    val cleanText = stripInlineErrorNotices(text)
    val isForeignProtocol = nodeVar?.providerProtocol != null &&
        nodeVar.providerProtocol != ProviderProtocol.GEMINI_INTERACTIONS
    val rawSig = if (isForeignProtocol) "" else nodeVar?.thinkingSignature.orEmpty()
    val sig = sanitizeGeminiThoughtSignature(rawSig, forFunctionCall = false)
    val thinkSummary = nodeVar?.thinkingSummary.orEmpty()
    val trailingThink = nodeVar?.trailingThinking.orEmpty()
    val toolCalls = nodeVar?.toolCalls.orEmpty()
    val turnText = turnTextLayout(text, toolCalls)
    val finalText = stripInlineErrorNotices(turnText.finalText)
    var emittedThoughtForTurn = false
    var emittedAnyStepForTurn = false

    if (foldHistoricalToolsToText && toolCalls.isNotEmpty()) {
        val foldedText = buildFoldedToolHistoryText(toolCalls, thinkSummary, cleanText)
        stepsList.add(geminiModelOutputStep(foldedText))
        return
    }

    if (toolCalls.isEmpty() && sig.isNotBlank()) {
        stepsList.add(geminiThoughtStep(signature = sig, summary = thinkSummary))
        emittedThoughtForTurn = true
        emittedAnyStepForTurn = true
    }

    if (toolCalls.isNotEmpty()) {
        val batches = groupToolCallsIntoStepBatches(
            toolCalls = toolCalls,
            beforeTool = turnText.beforeTool,
            fallbackTurnSignature = if (isForeignProtocol) "" else rawSig,
            fallbackTurnThinking = thinkSummary
        )
        val totalCalls = toolCalls.size
        var globalCallIdx = 0
        for (batch in batches) {
            val stepSig = if (isForeignProtocol) "" else sanitizeGeminiThoughtSignature(batch.thoughtSignature, forFunctionCall = true)
            if (stepSig.isNotBlank()) {
                stepsList.add(geminiThoughtStep(signature = stepSig, summary = batch.precedingThinking))
                emittedThoughtForTurn = true
                emittedAnyStepForTurn = true
            }
            if (batch.precedingText.isNotBlank()) {
                stepsList.add(geminiModelOutputStep(batch.precedingText))
                emittedAnyStepForTurn = true
            }
            val callIds = replayIds.assign(batch.calls)
            for ((tc, callId) in batch.calls.zip(callIds)) {
                val parsedArgs = runCatching { json.parseToJsonElement(tc.argumentsJson.ifBlank { "{}" }).jsonObject }
                    .getOrElse { JsonObject(emptyMap()) }
                stepsList.add(geminiFunctionCallStep(callId = callId, name = tc.name, arguments = parsedArgs))
            }
            for ((tc, callId) in batch.calls.zip(callIds)) {
                val wireResult = compactHistoricalToolResultForWire(
                    rawResult = tc.resultText,
                    toolName = tc.name,
                    isOlderAssistantTurn = isOlderAssistantTurn,
                    callIndexInTurn = globalCallIdx++,
                    totalCallsInTurn = totalCalls
                )
                stepsList.add(
                    geminiFunctionResultStep(
                        callId = callId,
                        name = tc.name.ifBlank { "tool" },
                        isError = tc.isError,
                        resultText = wireResult
                    )
                )
            }
            emittedAnyStepForTurn = true
        }
    }

    if (toolCalls.isNotEmpty() && trailingThink.isNotBlank() && sig.isNotBlank() &&
        toolCalls.none { sanitizeGeminiThoughtSignature(it.thoughtSignature, forFunctionCall = false) == sig }
    ) {
        stepsList.add(geminiThoughtStep(signature = sig, summary = trailingThink))
        emittedThoughtForTurn = true
        emittedAnyStepForTurn = true
    }

    val outputText = when {
        finalText.isNotBlank() -> finalText
        !emittedThoughtForTurn && thinkSummary.isNotBlank() && toolCalls.isEmpty() -> {
            "[Interrupted during reasoning — partial thoughts:\n$thinkSummary]"
        }
        !emittedAnyStepForTurn -> "..."
        else -> ""
    }
    if (outputText.isNotBlank()) {
        stepsList.add(geminiModelOutputStep(outputText))
    }
}

/** 3-Layer Rolling Context Compaction helper for Gemini Interactions extra loop steps. */
internal fun compactGeminiInteractionsExtraSteps(steps: List<JsonObject>): List<JsonObject> {
    if (steps.size <= 16) return steps
    val total = steps.size
    return steps.mapIndexed { idx, obj ->
        val type = obj["type"]?.jsonPrimitive?.contentOrNull
        if (type == "function_result") {
            val name = obj["name"]?.jsonPrimitive?.contentOrNull ?: "tool"
            val resultEl = obj["result"]
            val raw = when (resultEl) {
                is JsonPrimitive -> resultEl.contentOrNull.orEmpty()
                is JsonObject -> resultEl["result"]?.jsonPrimitive?.contentOrNull ?: resultEl.toString()
                else -> resultEl?.toString().orEmpty()
            }
            val compacted = WorkspaceHarness.compactStaleToolOutputText(idx, total, name, raw)
            if (compacted != raw) {
                buildJsonObject {
                    for ((k, v) in obj) {
                        if (k == "result") {
                            putJsonObject("result") {
                                put("result", compacted)
                            }
                        } else {
                            put(k, v)
                        }
                    }
                }
            } else obj
        } else obj
    }
}
