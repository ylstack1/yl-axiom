package io.parchment.ai.runtime

import io.parchment.ai.platform.PlatformRuntime
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Some phones cut an app's network soon after it leaves the screen, even with a foreground
 * service. Coming back is the moment to find out whether a connection survived that.
 */
internal object AppForeground {
    private val resumedAtState = MutableStateFlow(0L)
    val resumedAt: StateFlow<Long> = resumedAtState.asStateFlow()
    private val visibleState = MutableStateFlow(true)
    val isAppVisible: StateFlow<Boolean> = visibleState.asStateFlow()

    @Volatile var leftAt: Long = 0L
        private set

    fun markLeft() {
        leftAt = PlatformRuntime.monotonicTimeMillis()
        visibleState.value = false
    }

    fun markResumed() {
        resumedAtState.value = PlatformRuntime.monotonicTimeMillis()
        visibleState.value = true
    }
}
