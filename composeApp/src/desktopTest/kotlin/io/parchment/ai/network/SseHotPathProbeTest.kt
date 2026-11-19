package io.parchment.ai.network

import io.ktor.utils.io.ByteReadChannel
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.TokenMetrics
import io.parchment.ai.runtime.RunActivity
import kotlinx.coroutines.flow.FlowCollector
import kotlinx.coroutines.runBlocking
import java.io.File
import java.lang.management.ManagementFactory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * What the stream reader and the raw-wire record cost per event: 10,000 events of about 300 characters against a
 * 1 MB request, as bytes allocated on this thread and events per second. Written to
 * .parchment-local/verification/sse-hot-path.txt (TODO 18.2).
 */
class SseHotPathProbeTest {
    private val threads = ManagementFactory.getThreadMXBean() as com.sun.management.ThreadMXBean

    private fun accumulator(collector: FlowCollector<StreamUpdate> = FlowCollector { }) = GenerationStreamAccumulator(
        engine = AiProviderEngine, collector = collector, activity = RunActivity(),
        profile = ProviderProfile("p", "Probe", ProviderProtocol.OPENAI_RESPONSES, defaultModel = "m"),
        session = ChatSession("s", "Probe", 1L, 1L, "p"), model = "m", historyNodes = emptyList(),
        cleanInitialPrefix = "", initialThinking = "", initialSignature = "", initialToolCalls = emptyList(),
        initialMediaItems = emptyList(), initialWebSources = emptyList(), spendLimitUsd = 0.0, spendBeforeRunUsd = 0.0,
        externalRunSpendUsd = null, initialElapsedMs = 0L, initialApiUsageRecords = emptyList(),
        initialTokenMetrics = TokenMetrics(), usageOwnerSessionId = "s", resolvedUsageRunId = "r", usagePurpose = "chat"
    )

    private suspend fun read(payload: ByteArray, onEvent: (String, String) -> Unit) = AiProviderEngine.readSseOrJsonFallback(
        channel = ByteReadChannel(payload),
        streamFinished = { true },
        onSseEvent = { type, data -> onEvent(type, data) },
        onPlainJsonFallback = {}
    )

    @Test fun streamEventCosts() = runBlocking<Unit> {
        val events = 10_000
        val delta = "word ".repeat(50)
        val payload = buildString {
            repeat(events) { append("data: {\"type\":\"response.output_text.delta\",\"delta\":\"").append(delta).append("\"}\n\n") }
        }.toByteArray()
        val request = "{\"input\":\"" + "x".repeat(1_000_000) + "\"}"
        val state = accumulator()
        repeat(3) { read(payload) { _, data -> state.recordRawWireBounded(request, data) } } // Class loading and the JIT.
        var delivered = 0
        val readerStart = threads.currentThreadAllocatedBytes
        read(payload) { _, _ -> delivered++ }
        val readerBytes = threads.currentThreadAllocatedBytes - readerStart
        val started = System.nanoTime()
        val wireStart = threads.currentThreadAllocatedBytes
        read(payload) { _, data -> state.recordRawWireBounded(request, data) }
        val wireBytes = threads.currentThreadAllocatedBytes - wireStart
        val seconds = (System.nanoTime() - started) / 1e9
        assertEquals(events, delivered)
        val line = "sse events=$events reader_bytes_per_event=${readerBytes / events} " +
            "with_raw_wire_bytes_per_event=${wireBytes / events} events_per_s=${(events / seconds).toLong()}"
        println(line)
        val repo = generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile }
        File(repo, ".parchment-local/verification/sse-hot-path.txt").apply { parentFile.mkdirs() }.writeText(line + "\n")
        // Before 18.2 the unchanged request was copied on every event: 3300 bytes for the reader, 7461 with it.
        assertTrue((wireBytes - readerBytes) / events < 64, "The raw-wire record must not copy an unchanged request: $line")
    }

    @Test fun aNewRequestStillReplacesTheRawWirePreview() = runBlocking<Unit> {
        val updates = mutableListOf<StreamUpdate>()
        val state = accumulator { updates += it }
        state.recordRawWireBounded("a".repeat(5000), "first")
        state.recordRawWireBounded("a".repeat(5000), "again")
        state.recordRawWireBounded("b".repeat(5000), "second")
        state.reportCompletedRun(null)
        val completed = updates.filterIsInstance<StreamUpdate.Completed>().single()
        assertEquals("b".repeat(4096), completed.rawRequest, "A request of the same length but other text is a new request")
        assertEquals("second", completed.rawResponse)
    }

    /** Whitespace, field order and multi-line data that every provider stream has sent at some point. */
    @Test fun fieldValuesAreDeliveredExactlyAsBefore() = runBlocking<Unit> {
        val stream = listOf(
            "data:{\"a\":1}", "",
            "data:   {\"b\":2}   ", "",
            "event: custom", "data: {\"c\":3}", "",
            "event:\tspaced\t", "data: line one", "data:  line two ", "",
            ": keep-alive", "",
            "data:\t{\"d\":4} ", "",
            "data: {\"e\":5}\r", "\r",
            "data: [DONE]", ""
        ).joinToString("\n", postfix = "\n")
        val seen = mutableListOf<Pair<String, String>>()
        read(stream.toByteArray()) { type, data -> seen += type to data }
        assertEquals(
            listOf(
                "" to "{\"a\":1}",
                "" to "{\"b\":2}",
                "custom" to "{\"c\":3}",
                "spaced" to "line one\nline two",
                "" to "{\"d\":4}",
                "" to "{\"e\":5}",
                "" to "[DONE]"
            ),
            seen
        )
    }
}
