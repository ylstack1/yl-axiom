package io.parchment.ai.network

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext

/** A run waiting to send its request again: which attempt, how long is left and why. */
data class RetryWait(val attempt: Int, val maxAttempts: Int, val remainingSeconds: Long, val reason: String)

/**
 * A run's hold on its retry waits. It travels in the coroutine context of whoever collects the run's stream, so the
 * engine finds it wherever it waits (helpers included): each countdown shows in [waiting], and [retryNow] ends every
 * wait in progress. The engine still decides whether to retry at all; this only shortens the wait.
 */
class RetryControl : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<RetryControl>

    private val waitState = MutableStateFlow<RetryWait?>(null)
    val waiting: StateFlow<RetryWait?> = waitState.asStateFlow()
    private val requests = MutableStateFlow(0L)

    /** Sends the waiting request now. Does nothing while nothing waits. */
    fun retryNow() {
        if (waitState.value != null) requests.update { it + 1 }
    }

    internal fun requestsSoFar(): Long = requests.value

    internal suspend fun awaitRequestAfter(seen: Long) {
        requests.first { it != seen }
    }

    internal fun show(wait: RetryWait) {
        waitState.value = wait
    }

    /** Clears [wait] only if it is still the one shown, so parallel waits never clear each other. */
    internal fun clear(wait: RetryWait?) {
        if (wait != null) waitState.compareAndSet(wait, null)
    }
}
