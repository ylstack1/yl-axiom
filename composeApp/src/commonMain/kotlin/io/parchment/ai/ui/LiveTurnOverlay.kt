package io.parchment.ai.ui

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageVariant

/** What a streaming reply shows right now; see [LiveTurns]. */
@Immutable
internal data class LiveTurnOverlay(
    val sessionId: String,
    val nodeId: String,
    val variantId: String,
    val content: String,
    val thinkingSummary: String,
    val trailingThinking: String,
    val thinkingSignature: String,
    val thinkingDurationMs: Long,
    val tokensPerSec: Double
)

/**
 * The visible text of replies while they stream, one state per reply variant. The stream pacer writes here every
 * frame instead of into the app state, so a frame redraws only that reply, not the app root, sidebar or tabs. The
 * app state and the disk take the text at each checkpoint (every 3.5 s) and at the end, when the entry is removed.
 *
 * Anything that reads a streaming chat from the app state, other than the reply's own item, must see the live
 * text: [ChatSession.withLiveTurns] gives it, and `completeSessionForAction`, export and the pause flush apply it.
 */
internal object LiveTurns {
    private val byVariant = mutableStateMapOf<String, MutableState<LiveTurnOverlay>>()

    /** Reading the map subscribes to streams starting or ending; reading the returned state, to that reply's frames. */
    fun stateOf(variantId: String): State<LiveTurnOverlay>? = byVariant[variantId]

    fun publish(overlay: LiveTurnOverlay) {
        val holder = byVariant[overlay.variantId]
        if (holder == null) byVariant[overlay.variantId] = mutableStateOf(overlay) else holder.value = overlay
    }

    fun end(variantId: String) {
        byVariant.remove(variantId)
    }

    fun forSession(sessionId: String): List<LiveTurnOverlay> =
        byVariant.values.map { it.value }.filter { it.sessionId == sessionId }

    fun sessionIds(): Set<String> = byVariant.values.mapTo(HashSet()) { it.value.sessionId }
}

/** This variant with [live]'s text, when [live] is this variant's stream. */
internal fun MessageVariant.withLiveTurn(live: LiveTurnOverlay?): MessageVariant =
    if (live == null || live.variantId != id) this else copy(
        content = live.content,
        thinkingSummary = live.thinkingSummary,
        trailingThinking = live.trailingThinking,
        thinkingSignature = live.thinkingSignature,
        thinkingDurationMs = live.thinkingDurationMs,
        tokenMetrics = if (live.tokensPerSec > 0.0) tokenMetrics.copy(tokensPerSec = live.tokensPerSec) else tokenMetrics
    )

/** This chat with each streaming reply's live text in place of its last checkpoint. */
internal fun ChatSession.withLiveTurns(): ChatSession {
    val lives = LiveTurns.forSession(id)
    if (lives.isEmpty()) return this
    return copy(messages = messages.map { node ->
        val live = lives.firstOrNull { it.nodeId == node.id } ?: return@map node
        node.copy(variants = node.variants.map { it.withLiveTurn(live) })
    })
}
