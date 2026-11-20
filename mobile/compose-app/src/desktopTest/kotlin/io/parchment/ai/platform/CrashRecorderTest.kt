package io.parchment.ai.platform

import java.io.File
import java.nio.file.Files
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class CrashRecorderTest {
    private val secrets = listOf("sk-live1234567890abcdef", "AIza" + "SyA1234567890abcdefghijklmn", "hunter2-token-value")

    private fun failure(): Throwable {
        val cause = IllegalStateException("Request to https://api.example.com/v1?key=${secrets[1]}&alt=sse failed: Bearer ${secrets[2]}")
        return RuntimeException("Provider said: invalid key ${secrets[0]}, x-api-key: ${secrets[2]}", cause)
    }

    private fun withFolder(block: (File) -> Unit) {
        val root = Files.createTempDirectory("parchment-crash-test-").toFile()
        try {
            block(root)
        } finally {
            AtomicFileStorage.deleteTree(root.toPath())
        }
    }

    @Test fun pendingSavesReachDiskBeforeTheReportIsWrittenWithoutSecrets() = withFolder { root ->
        val flushed = AtomicBoolean(false)
        val saved = CrashRecorder.record(root, Thread.currentThread(), failure()) { flushed.set(true) }
        assertTrue(saved && flushed.get())
        val report = checkNotNull(CrashRecorder.takeReport(root))
        assertTrue(CrashRecorder.reportSaysSaved(report))
        assertTrue("RuntimeException" in report && "Caused by: java.lang.IllegalStateException" in report, report)
        secrets.forEach { secret -> assertFalse(secret in report, "A secret reached the report: $report") }
        assertTrue(report.length <= 64 * 1024)
        assertNull(CrashRecorder.takeReport(root), "A report is mentioned once")
    }

    @Test fun aFlushThatHangsIsAbandonedWithinItsLimit() = withFolder { root ->
        val started = System.nanoTime()
        val saved = CrashRecorder.record(root, Thread.currentThread(), failure()) { Thread.sleep(60_000) }
        val tookMs = (System.nanoTime() - started) / 1_000_000L
        assertFalse(saved)
        assertTrue(tookMs < CrashRecorder.FLUSH_LIMIT_MS + 1_000L, "Took $tookMs ms")
        assertFalse(CrashRecorder.reportSaysSaved(checkNotNull(CrashRecorder.takeReport(root))))
    }

    @Test fun aDeepCauseChainStaysWithinTheSizeLimit() = withFolder { root ->
        var chain: Throwable = IllegalStateException("x".repeat(100_000))
        repeat(50) { chain = RuntimeException("level $it " + "y".repeat(10_000), chain) }
        CrashRecorder.record(root, Thread.currentThread(), chain) {}
        val report = checkNotNull(CrashRecorder.takeReport(root))
        assertTrue(report.length <= 64 * 1024, "${report.length} characters")
        assertEquals("saved=true", report.lineSequence().first())
    }

    @Test fun headersAndQueryKeysKeepTheirNamesButLoseTheirValues() {
        assertEquals("GET /v1?key=[removed]&alt=sse", CrashRecorder.redact("GET /v1?key=abc123&alt=sse"))
        assertEquals("Authorization: Bearer [removed]", CrashRecorder.redact("Authorization: Bearer abc.def"))
        assertEquals("\"x-goog-api-key\": \"[removed]\"", CrashRecorder.redact("\"x-goog-api-key\": \"AIza-not-really\""))
    }
}
