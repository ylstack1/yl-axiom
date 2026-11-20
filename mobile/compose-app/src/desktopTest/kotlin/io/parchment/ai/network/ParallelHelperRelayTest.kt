package io.parchment.ai.network

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ParallelHelperRelayTest {
    /**
     * Helpers run in their own coroutines on a thread pool, yet every update they produce must reach the flow from
     * the flow's own coroutine: a direct emit() from a helper throws "Flow invariant is violated".
     */
    @Test fun parallelTasksRelayEmissionsToTheFlowCoroutineAndOverlap() = runBlocking {
        val running = AtomicInteger(0)
        val peak = AtomicInteger(0)
        val allStarted = CompletableDeferred<Unit>()
        val updates = withTimeout(10_000) {
            flow {
                val results = runConcurrentlyRelayingToFlow(3) { idx ->
                    kotlinx.coroutines.withContext(Dispatchers.IO) {
                        peak.accumulateAndGet(running.incrementAndGet()) { a, b -> maxOf(a, b) }
                        if (running.get() == 3) allStarted.complete(Unit)
                    }
                    allStarted.await() // Proves the three run at the same time, not one after another.
                    onFlowCoroutine { emit("progress $idx") }
                    running.decrementAndGet()
                    "result $idx"
                }
                results.forEach { emit(it) }
            }.toList()
        }
        assertEquals(3, peak.get())
        assertEquals(listOf("result 0", "result 1", "result 2"), updates.takeLast(3), "results keep task order")
        assertEquals(setOf("progress 0", "progress 1", "progress 2"), updates.dropLast(3).toSet())
    }

    @Test fun withoutARelayTheBlockRunsInPlace() = runBlocking {
        var ran = false
        launch { onFlowCoroutine { ran = true } }.join()
        assertTrue(ran)
    }
}
