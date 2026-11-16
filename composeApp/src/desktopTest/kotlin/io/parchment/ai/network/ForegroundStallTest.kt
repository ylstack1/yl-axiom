package io.parchment.ai.network

import io.ktor.utils.io.ByteChannel
import io.ktor.utils.io.close
import io.ktor.utils.io.writeStringUtf8
import io.parchment.ai.runtime.AppForeground
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlin.test.*

class ForegroundStallTest {
    private val events = mutableListOf<String>()

    private fun CoroutineScope.read(channel: ByteChannel, returnGraceMs: Long = 200L) = async(Dispatchers.Default + SupervisorJob()) {
        runCatching {
            AiProviderEngine.readSseOrJsonFallback(channel, { true }, { _, data -> events += data }, {}, returnGraceMs = returnGraceMs)
        }
    }

    private suspend fun ByteChannel.line(text: String) {
        writeStringUtf8("data: $text\n\n")
        flush()
    }

    private fun leaveAndReturn() {
        AppForeground.markLeft()
        assertFalse(AppForeground.isAppVisible.value)
        Thread.sleep(50)
        AppForeground.markResumed()
        assertTrue(AppForeground.isAppVisible.value)
    }

    @Test fun aConnectionCutWhileAwayIsDroppedSoonAfterReturning() = runBlocking<Unit> {
        val channel = ByteChannel(autoFlush = true)
        val reading = read(channel)
        channel.line("first")
        delay(300)
        leaveAndReturn()
        val failure = withTimeout(8_000) { reading.await() }.exceptionOrNull()
        assertTrue("SSE stream timed out" in failure?.message.orEmpty(), failure.toString())
        assertEquals(listOf("first"), events)
    }

    @Test fun aConnectionCutWhileAwayRetriesWithoutWaiting() = runBlocking<Unit> {
        val statuses = mutableListOf<String>()
        withTimeout(2_000) {
            AiProviderEngine.waitWithRetryCountdown(1, 3, waitMs = 60_000L, reasonSummary = "SSE stream timed out while the app was in the background.") { statuses += it }
        }
        assertEquals(listOf("Trying again now (1 of 3)…"), statuses)
    }

    @Test fun retryNowEndsTheWaitAtOnceAndTheWaitShowsUntilThen() = runBlocking<Unit> {
        val control = RetryControl()
        val statuses = java.util.Collections.synchronizedList(mutableListOf<String>())
        val waiting = async(Dispatchers.Default + control) {
            AiProviderEngine.waitWithRetryCountdown(2, 10, waitMs = 60_000L, reasonSummary = "HTTP 503: busy") { statuses += it }
        }
        val shown = withTimeout(2_000) { control.waiting.first { it != null } }
        assertEquals(RetryWait(2, 10, 60, "HTTP 503: busy"), shown)
        val asked = System.nanoTime()
        control.retryNow()
        withTimeout(2_000) { waiting.await() }
        val tookMs = (System.nanoTime() - asked) / 1_000_000L
        assertTrue(tookMs < 100L, "Retry now must not wait for the next second, took $tookMs ms")
        assertNull(control.waiting.value, "Nothing shows once the wait has ended")
        assertEquals("Trying again in 60 s (2 of 10)\nHTTP 503: busy", statuses.first())
        assertEquals("Trying again now (2 of 10)…", statuses.last())
    }

    @Test fun oneRetryNowEndsEveryWaitOfTheRun() = runBlocking<Unit> {
        val control = RetryControl()
        control.retryNow() // Nothing waits yet, so this must not end the waits that follow.
        val first = async(Dispatchers.Default + control) { AiProviderEngine.waitWithRetryCountdown(1, 3, 60_000L, "first") {} }
        withTimeout(2_000) { control.waiting.first { it?.reason == "first" } }
        val second = async(Dispatchers.Default + control) { AiProviderEngine.waitWithRetryCountdown(1, 3, 60_000L, "second") {} }
        withTimeout(2_000) { control.waiting.first { it?.reason == "second" } }
        assertFalse(first.isCompleted || second.isCompleted)
        control.retryNow()
        withTimeout(2_000) { first.await(); second.await() }
        assertNull(control.waiting.value)
    }

    @Test fun dataArrivingAfterReturningKeepsTheStream() = runBlocking<Unit> {
        val channel = ByteChannel(autoFlush = true)
        val reading = read(channel)
        channel.line("first")
        delay(300)
        leaveAndReturn()
        delay(50)
        channel.line("second")
        delay(350)
        channel.line("third")
        channel.close()
        assertNull(withTimeout(5_000) { reading.await() }.exceptionOrNull())
        assertEquals(listOf("first", "second", "third"), events)
    }

    @Test fun aSlowFirstReplyIsNotMistakenForACutConnection() = runBlocking<Unit> {
        val channel = ByteChannel(autoFlush = true)
        val reading = read(channel)
        delay(50)
        leaveAndReturn()
        delay(350)
        assertFalse(reading.isCompleted, "Silent thinking before the first line is normal")
        channel.line("first")
        channel.close()
        assertNull(withTimeout(5_000) { reading.await() }.exceptionOrNull())
    }

    @Test fun returningWithoutHavingLeftChangesNothing() = runBlocking<Unit> {
        val channel = ByteChannel(autoFlush = true)
        val reading = read(channel)
        channel.line("first")
        delay(200)
        val started = AppForeground.leftAt
        delay(100)
        assertEquals(started, AppForeground.leftAt)
        channel.line("second")
        channel.close()
        assertNull(withTimeout(5_000) { reading.await() }.exceptionOrNull())
    }
}
