package io.parchment.ai.platform

import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

internal enum class WindowsPowerLeaseStatus {
    IDLE, STARTING, ACTIVE, UNAVAILABLE
}

internal data class WindowsPowerLeaseSnapshot(
    val owners: Int,
    val status: WindowsPowerLeaseStatus,
    val error: String = ""
)

/**
 * Keeps Windows from sleeping while generations run. One daemon thread holds
 * `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` (an FFM downcall, [WindowsNative]) and clears it when
 * the last owner leaves. Windows ties the request to that thread, so it cannot outlive the app: no child process,
 * no heartbeat. A failed request is reported as UNAVAILABLE and retried at most every [retryIntervalMs].
 * Windows power policy and explicit suspend can still override the request.
 */
internal class WindowsPowerLease(
    private val setExecutionState: (Int) -> Int = WindowsNative::setThreadExecutionState,
    private val nanoTime: () -> Long = System::nanoTime,
    private val retryIntervalMs: Long = RETRY_INTERVAL_MS
) : AutoCloseable {
    private val lock = Any()
    private var owners = 0
    private var closed = false
    /** Released to end the current hold; the holder thread then clears the request on its own thread. */
    private var holdRelease: CountDownLatch? = null
    private var holder: Thread? = null
    private var lastFailureAt: Long? = null

    @Volatile
    var snapshot = WindowsPowerLeaseSnapshot(0, WindowsPowerLeaseStatus.IDLE)
        private set

    fun acquire() = synchronized(lock) {
        check(!closed) { "Windows power lease is closed." }
        owners++
        ensureHolderLocked()
    }

    fun reconcile(activeOwners: Int) = synchronized(lock) {
        check(!closed) { "Windows power lease is closed." }
        require(activeOwners >= 0)
        owners = activeOwners
        if (owners == 0) stopLocked() else ensureHolderLocked()
    }

    /** Called on long-decode progress: restarts a hold that failed, once the retry interval has passed. */
    fun refresh() = synchronized(lock) {
        if (owners > 0 && !closed) ensureHolderLocked()
    }

    fun release() = synchronized(lock) {
        if (owners > 0) owners--
        if (owners == 0) stopLocked() else publish(snapshot.status, snapshot.error)
    }

    override fun close() {
        val thread = synchronized(lock) {
            closed = true
            owners = 0
            stopLocked()
        }
        // On shutdown, give the holder a moment to clear its request before the process exits.
        thread?.join(TimeUnit.SECONDS.toMillis(2))
    }

    private fun ensureHolderLocked() {
        if (holder?.isAlive == true) {
            publish(snapshot.status, snapshot.error)
            return
        }
        val failedAt = lastFailureAt
        if (failedAt != null && nanoTime() - failedAt < TimeUnit.MILLISECONDS.toNanos(retryIntervalMs)) {
            publish(WindowsPowerLeaseStatus.UNAVAILABLE, snapshot.error)
            return
        }
        val release = CountDownLatch(1)
        holdRelease = release
        publish(WindowsPowerLeaseStatus.STARTING)
        holder = Thread({ hold(release) }, "parchment-keep-awake").apply {
            isDaemon = true
            start()
        }
    }

    private fun hold(release: CountDownLatch) {
        val previous = runCatching { setExecutionState(ES_HOLD) }
        synchronized(lock) {
            val current = holdRelease === release
            if (previous.getOrNull()?.let { it != 0 } == true) {
                if (current) publish(WindowsPowerLeaseStatus.ACTIVE)
            } else {
                if (current) {
                    lastFailureAt = nanoTime()
                    val reason = previous.exceptionOrNull()?.message ?: "SetThreadExecutionState failed."
                    publish(WindowsPowerLeaseStatus.UNAVAILABLE, reason.take(512))
                    holdRelease = null
                    holder = null // This thread is ending; the next refresh after the interval starts a new one.
                }
                return
            }
        }
        try {
            release.await()
        } catch (_: InterruptedException) {
            Thread.currentThread().interrupt()
        } finally {
            runCatching { setExecutionState(ES_CLEAR) }
        }
    }

    private fun publish(status: WindowsPowerLeaseStatus, error: String = "") {
        snapshot = WindowsPowerLeaseSnapshot(owners, status, error)
    }

    /** Ends the current hold; returns the holder thread so [close] can wait for it. */
    private fun stopLocked(): Thread? {
        holdRelease?.countDown()
        holdRelease = null
        val thread = holder
        holder = null
        lastFailureAt = null
        publish(WindowsPowerLeaseStatus.IDLE)
        return thread
    }

    internal companion object {
        const val RETRY_INTERVAL_MS = 5_000L
        val ES_HOLD = WindowsNative.ES_CONTINUOUS or WindowsNative.ES_SYSTEM_REQUIRED
        const val ES_CLEAR = WindowsNative.ES_CONTINUOUS
    }
}
