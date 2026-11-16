package io.parchment.ai.network

import io.parchment.ai.model.CUSTOM_DIALECT_ANTHROPIC
import io.parchment.ai.model.CUSTOM_DIALECT_GEMINI
import io.parchment.ai.model.CUSTOM_DIALECT_OPENAI_CHAT
import io.parchment.ai.model.CUSTOM_DIALECT_OPENAI_RESPONSES
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.protocol.isCompleteGeminiSignatureProtobuf
import io.parchment.ai.protocol.isGeminiServerSideToolSignature
import io.parchment.ai.protocol.sanitizeGeminiThoughtSignature

// Opaque reasoning signatures and cross-provider reasoning handoff.

// ============================================================================================
// 5. Media and audio: images, video and speech, routed to the right endpoint.
//    Supports:
//    - Universal Image Generation (`POST /v1/images/generations` & `/api/v3/images/generations` with 3-stage payload self-healing)
//    - Universal Video Generation (`POST /contents/generations/tasks`, `/videos/generations`, `/video/generations`, `/videos` + sync/async polling)
//    - Universal Audio Speech / TTS (`POST /v1/audio/speech` & `/audio/speech` with fallback to `/chat/completions`)
//    - Falls back to `/v1/chat/completions` when a proxy offers media generation only through chat.
// ============================================================================================
/**
 * Cross-provider reasoning continuity.
 *
 * Signatures are provider-private receipts and are dropped on replay (see `isForeignProtocol`).
 * The *narrative* of prior reasoning is not a receipt, so it is carried forward as plain text
 * for every historical assistant turn minted by a different protocol — giving the new model
 * the reasoning context without ever handing it an unverifiable blob.
 *
 * Skipped entirely when server-side chaining is active: the provider already holds those steps,
 * so re-injecting them would duplicate context.
 */
internal fun normalizeReasoningModelId(name: String): String = name.trim().removePrefix("models/")

internal fun stepDownReasoningEffort(currentEffort: String, errMsg: String): Pair<String, String>? =
    when (currentEffort.trim().lowercase()) {
        "max" -> "xhigh" to io.parchment.ai.strings.Strings.endpointRejectedMaxThinkingRetryingWith
        "xhigh" -> "high" to io.parchment.ai.strings.Strings.endpointRejectedXhighThinkingRetryingWith
        "minimal" -> if (errMsg.contains("minimal") || errMsg.contains("effort") || errMsg.contains("reasoning") ||
            errMsg.contains("unsupported_value") || errMsg.contains("invalid_value")
        ) {
            "low" to io.parchment.ai.strings.Strings.endpointRejectedMinimalThinkingSteppingUp
        } else null
        else -> null
    }

internal fun isSameReasoningModel(
    savedModel: String,
    currentModel: String,
    allowBlank: Boolean = true
): Boolean {
    val s = normalizeReasoningModelId(savedModel)
    val c = normalizeReasoningModelId(currentModel)
    if (s.isBlank() || c.isBlank()) return allowBlank
    return s.equals(c, ignoreCase = true)
}

/**
 * True when [variant]'s native wire reasoning (`thinking`/`reasoning`/`thought`/`reasoning_content`
 * and opaque signatures) matches [targetProtocol]/[targetWireDialect] and [currentModel], and has
 * not already been rewritten into a plain-text [AiProviderEngine.REASONING_HANDOFF_MARKER] note.
 */
internal fun isNativeReasoningCompatible(
    variant: MessageVariant,
    targetProtocol: ProviderProtocol,
    targetWireDialect: String = "",
    currentModel: String = ""
): Boolean {
    if (variant.content.contains(AiProviderEngine.REASONING_HANDOFF_MARKER)) return false
    if (!isSameReasoningModel(variant.modelUsed, currentModel, allowBlank = true)) return false
    val targetDialect = targetWireDialect.trim().lowercase()
    val bornDialect = variant.effectiveWireDialect()
    if (targetDialect.isNotBlank() && bornDialect.isNotBlank()) {
        return bornDialect == targetDialect
    }
    val bornProtocol = variant.providerProtocol ?: return true
    return bornProtocol == targetProtocol
}

private fun AiProviderEngine.hasMissingNativeToolSignature(
    variant: MessageVariant,
    targetProtocol: ProviderProtocol,
    targetWireDialect: String
): Boolean {
    if (variant.toolCalls.isEmpty() || variant.providerProtocol == null) return false
    val resolvedDialect = targetWireDialect.trim().lowercase().ifBlank {
        when (targetProtocol) {
            ProviderProtocol.GEMINI_INTERACTIONS -> CUSTOM_DIALECT_GEMINI
            ProviderProtocol.ANTHROPIC_MESSAGES -> CUSTOM_DIALECT_ANTHROPIC
            else -> ""
        }
    }
    return when (resolvedDialect) {
        CUSTOM_DIALECT_GEMINI ->
            sanitizeGeminiThoughtSignature(variant.thinkingSignature, forFunctionCall = false).isBlank() &&
                variant.toolCalls.none { sanitizeGeminiThoughtSignature(it.thoughtSignature, forFunctionCall = true).isNotBlank() }
        CUSTOM_DIALECT_ANTHROPIC ->
            !isValidAnthropicSignature(variant.thinkingSignature, variant.providerProtocol, variant.modelUsed) &&
                variant.toolCalls.none { isValidAnthropicSignature(it.thoughtSignature, variant.providerProtocol, variant.modelUsed) }
        CUSTOM_DIALECT_OPENAI_RESPONSES ->
            !isValidOpenAiEncryptedReasoning(variant.thinkingSignature, variant.providerProtocol, variant.modelUsed, savedWireDialect = variant.effectiveWireDialect()) &&
                variant.toolCalls.none { isValidOpenAiEncryptedReasoning(it.thoughtSignature, variant.providerProtocol, variant.modelUsed, savedWireDialect = variant.effectiveWireDialect()) }
        else -> false
    }
}

internal fun AiProviderEngine.applyCrossProviderReasoningHandoff(
    nodes: List<MessageNode>,
    currentProtocol: ProviderProtocol,
    chainingActive: Boolean,
    currentWireDialect: String = "",
    currentModel: String = ""
): List<MessageNode> {
    if (chainingActive) return nodes
    var changed = false
    val out = nodes.map { node ->
        if (node.role != MessageRole.ASSISTANT || !node.includedInContext) return@map node
        val variant = node.activeVariant
        val summary = variant.thinkingSummary.trim()
        if (summary.isEmpty()) return@map node
        if (variant.content.contains(REASONING_HANDOFF_MARKER)) return@map node
        val nativeCompat = isNativeReasoningCompatible(variant, currentProtocol, currentWireDialect, currentModel)
        val missingNativeSig = nativeCompat && hasMissingNativeToolSignature(variant, currentProtocol, currentWireDialect)
        if (nativeCompat && !missingNativeSig) return@map node

        val cleanBase = stripInlineErrorNotices(variant.content)
        val hasTools = variant.toolCalls.isNotEmpty()
        val handoffHeader = buildString {
            append(REASONING_HANDOFF_MARKER)
            append('\n')
            append(clipReasoningSummaryForHandoff(summary, MAX_REASONING_HANDOFF_CHARS))
        }
        val handoffPrefix = "$handoffHeader\n\n"
        val handoffContent = when {
            hasTools -> "$handoffPrefix$cleanBase"
            cleanBase.isBlank() -> handoffHeader
            else -> "$handoffPrefix$cleanBase"
        }
        // Shift the first tool step's precedingText by the same prefix so turnTextLayout preserves step order.
        val updatedTools = if (hasTools) {
            val first = variant.toolCalls.first()
            listOf(first.copy(precedingText = "$handoffPrefix${first.precedingText}")) + variant.toolCalls.drop(1)
        } else {
            variant.toolCalls
        }
        val newVariants = node.variants.toMutableList()
        val idx = node.activeVariantIndex.coerceIn(0, (newVariants.size - 1).coerceAtLeast(0))
        if (idx !in newVariants.indices) return@map node
        newVariants[idx] = variant.copy(content = handoffContent, toolCalls = updatedTools)
        changed = true
        node.copy(variants = newVariants)
    }
    return if (changed) out else nodes
}

/**
 * Keeps opening framing (~25%) and closing conclusions (~75%) when a foreign turn's reasoning
 * summary exceeds [maxChars], deterministically so prompt-cache prefixes stay stable.
 */
internal fun clipReasoningSummaryForHandoff(
    summary: String,
    maxChars: Int = AiProviderEngine.MAX_REASONING_HANDOFF_CHARS
): String {
    if (summary.length <= maxChars) return summary
    val omittedMarker = "\n\n[...middle reasoning omitted...]\n\n"
    if (maxChars <= omittedMarker.length + 16) return summary.take(maxChars)
    val budget = maxChars - omittedMarker.length
    var headEnd = budget / 4
    if (headEnd > 0 && summary[headEnd - 1].isHighSurrogate()) headEnd--
    var tailStart = summary.length - (budget - headEnd)
    if (tailStart < summary.length && summary[tailStart].isLowSurrogate()) tailStart++
    val head = summary.substring(0, headEnd).trimEnd()
    val tail = summary.substring(tailStart).trimStart()
    if (head.isEmpty()) return tail.take(maxChars)
    return "$head$omittedMarker$tail"
}

/** A server-side chain is reusable only by the protocol, wire dialect, and model that created it. */
internal fun AiProviderEngine.reusableProviderChainId(
    previous: MessageVariant,
    protocol: ProviderProtocol,
    model: String,
    currentWireDialect: String = ""
): String {
    val targetDialect = currentWireDialect.trim().lowercase()
    val prevDialect = previous.effectiveWireDialect()
    if (targetDialect.isNotBlank() && prevDialect.isNotBlank() && targetDialect != prevDialect) return ""
    val prevModel = normalizeReasoningModelId(previous.modelUsed)
    val curModel = normalizeReasoningModelId(model)
    val sameModel = (prevModel.isBlank() && curModel.isBlank()) ||
        (prevModel.isNotBlank() && curModel.isNotBlank() && prevModel.equals(curModel, ignoreCase = true))
    return if (previous.providerProtocol == protocol && sameModel) previous.providerInteractionId else ""
}

internal fun AiProviderEngine.isValidOpenAiEncryptedReasoning(
    rawSig: String,
    savedProtocol: ProviderProtocol? = null,
    savedModel: String = "",
    currentModel: String = "",
    savedWireDialect: String = ""
): Boolean {
    if (savedProtocol != null && savedProtocol != ProviderProtocol.OPENAI_RESPONSES) return false
    if (savedWireDialect.equals(CUSTOM_DIALECT_OPENAI_CHAT, ignoreCase = true)) return false
    if (!isSameReasoningModel(savedModel, currentModel, allowBlank = true)) return false
    val trimmed = rawSig.trim()
    if (trimmed.isEmpty() || trimmed.contains(' ')) return false
    // Tokens end in normal base64 padding; padding mid-string means two tokens were glued together.
    if (trimmed.indexOf('=') in 0 until trimmed.trimEnd('=').length) return false
    if (isCompleteGeminiSignatureProtobuf(trimmed) || isGeminiServerSideToolSignature(trimmed)) return false
    return true
}

internal fun AiProviderEngine.isValidAnthropicSignature(
    rawSig: String,
    savedProtocol: ProviderProtocol?,
    savedModel: String = "",
    currentModel: String = ""
): Boolean {
    if (savedProtocol != null && savedProtocol != ProviderProtocol.ANTHROPIC_MESSAGES) return false
    if (!isSameReasoningModel(savedModel, currentModel, allowBlank = true)) return false
    val trimmed = rawSig.trim()
    if (trimmed.isEmpty() || trimmed.startsWith("gAAAAAB")) return false
    val hasMidPadding = trimmed.indexOf('=') in 0 until trimmed.trimEnd('=').length
    if (hasMidPadding) return false
    return !isCompleteGeminiSignatureProtobuf(trimmed) && !isGeminiServerSideToolSignature(trimmed)
}

/**
 * Appends an incoming signature chunk to `builder` while preventing distinct complete Base64
 * signatures (e.g. ending in `=` padding or starting a new Gemini `0x12 ...` protobuf) from
 * being concatenated into an invalid Franken-signature.
 */
internal fun AiProviderEngine.appendSignatureChunkSafe(builder: StringBuilder, incoming: String): String {
    if (incoming.isEmpty()) return ""
    val cur = builder.toString()
    if (cur == incoming || cur.endsWith(incoming)) return ""
    if (cur.isEmpty()) {
        builder.append(incoming)
        return incoming
    }
    val curEndsWithPadding = cur.endsWith('=')
    val bothAreCompleteGeminiSigs = isCompleteGeminiSignatureProtobuf(cur) && isCompleteGeminiSignatureProtobuf(incoming)
    if (curEndsWithPadding || bothAreCompleteGeminiSigs) {
        if (isGeminiServerSideToolSignature(incoming) && !isGeminiServerSideToolSignature(cur)) {
            return ""
        }
        builder.setLength(0)
        builder.append(incoming)
        return incoming
    }
    builder.append(incoming)
    return incoming
}
