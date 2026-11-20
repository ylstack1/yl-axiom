package io.parchment.ai.platform

import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class WindowsPowerLeaseTest {
    private val isWindows = System.getProperty("os.name").orEmpty().startsWith("Windows", ignoreCase = true)

    /** Records each call with the thread that made it, like Windows' per-thread execution state. */
    private class FakeExecutionState(var result: Int = 1) {
        val calls = CopyOnWriteArrayList<Pair<Int, Thread>>()
        fun call(flags: Int): Int { calls.add(flags to Thread.currentThread()); return result }
        fun heldThreads(): Set<Thread> {
            val held = LinkedHashSet<Thread>()
            calls.forEach { (flags, thread) -> if (flags == WindowsPowerLease.ES_HOLD) held += thread else held -= thread }
            return held
        }
    }

    @Test
    fun absoluteSnapshotsReleaseApprovalOnlyHoldsAndCanResume() {
        val native = FakeExecutionState()
        WindowsPowerLease(setExecutionState = native::call).use { lease ->
            lease.reconcile(2)
            await("Hold was not acquired.") { lease.snapshot.status == WindowsPowerLeaseStatus.ACTIVE }
            lease.reconcile(1)
            assertEquals(1, lease.snapshot.owners)
            assertEquals(1, native.calls.count { it.first == WindowsPowerLease.ES_HOLD }, "one hold for any number of owners")
            lease.reconcile(0)
            assertEquals(WindowsPowerLeaseStatus.IDLE, lease.snapshot.status)
            await("Approval-only wait kept the request.") { native.heldThreads().isEmpty() }
            lease.reconcile(1)
            await("Hold did not resume.") { lease.snapshot.status == WindowsPowerLeaseStatus.ACTIVE }
        }
        await("Close kept the request.") { native.heldThreads().isEmpty() }
    }

    @Test
    fun completingOneOwnerKeepsTheOtherOwnerProtected() {
        val native = FakeExecutionState()
        WindowsPowerLease(setExecutionState = native::call).use { lease ->
            lease.acquire()
            lease.acquire()
            await("Hold was not acquired.") { lease.snapshot.status == WindowsPowerLeaseStatus.ACTIVE }
            lease.release()
            assertEquals(WindowsPowerLeaseStatus.ACTIVE, lease.snapshot.status)
            assertEquals(1, native.heldThreads().size)
            lease.release()
            await("Final release kept the request.") { native.heldThreads().isEmpty() }
        }
    }

    @Test
    fun aFailedRequestIsReportedAndRetriedOnlyAfterTheInterval() {
        val native = FakeExecutionState(result = 0)
        val clock = AtomicLong(0)
        WindowsPowerLease(setExecutionState = native::call, nanoTime = clock::get, retryIntervalMs = 1_000).use { lease ->
            lease.acquire()
            await("Failure was not reported.") { lease.snapshot.status == WindowsPowerLeaseStatus.UNAVAILABLE }
            lease.refresh()
            assertEquals(1, native.calls.size, "no retry inside the interval")
            native.result = 1
            clock.addAndGet(TimeUnit.SECONDS.toNanos(2))
            lease.refresh()
            await("Retry did not take hold.") { lease.snapshot.status == WindowsPowerLeaseStatus.ACTIVE }
        }
    }

    @Test
    fun theRealWindowsRequestIsAcceptedAndCleared() {
        if (!isWindows) return
        WindowsPowerLease().use { lease ->
            lease.acquire()
            await("Windows refused the keep-awake request: ${lease.snapshot.error}") {
                lease.snapshot.status == WindowsPowerLeaseStatus.ACTIVE
            }
            lease.release()
            assertEquals(WindowsPowerLeaseStatus.IDLE, lease.snapshot.status)
        }
    }

    private fun await(message: String, seconds: Long = 15, condition: () -> Boolean) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(seconds)
        while (System.nanoTime() - deadline < 0) {
            if (condition()) return
            Thread.sleep(10)
        }
        assertTrue(condition(), message)
    }
}
