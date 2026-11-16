package io.parchment.ai.platform

import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/**
 * Records an uncaught exception before the platform handles it: pending saves are flushed (within [FLUSH_LIMIT_MS])
 * and a short report without secrets goes to `crash/last-crash.txt` for the next launch to mention once. The
 * handler that was installed before then runs as it would have, so Android still ends the process and the desktop
 * still prints the error.
 */
internal object CrashRecorder {
    private const val MAX_REPORT_CHARS = 64 * 1024
    private const val MAX_MESSAGE_CHARS = 300
    private const val MAX_FRAMES = 40
    private const val MAX_CAUSES = 8
    internal const val FLUSH_LIMIT_MS = 3_000L
    /** An error that repeats (a failing background loop) is recorded at most this often. */
    private const val MIN_RECORD_INTERVAL_MS = 10_000L

    private val installed = AtomicBoolean(false)
    private val recording = AtomicBoolean(false)
    private val recordedOnce = AtomicBoolean(false)
    private val lastRecordedAt = AtomicLong(0L)

    // Each pattern keeps its first group (the name of the secret) and drops the value.
    private val secrets = listOf(
        Regex("""(?i)([?&](?:key|api[_-]?key|token|access_token|auth|secret)=)[^&\s"'<>]+"""),
        Regex("""(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+"""),
        Regex("""(?i)((?:x-api-key|x-goog-api-key|api[_-]?key)["']?\s*[:=]\s*["']?)[^\s"',}]+"""),
        Regex("""(?i)(authorization["']?\s*[:=]\s*["']?(?:(?:bearer|basic|token)\s+)?)[^\s"',}]+"""),
        Regex("""()\b(?:sk|pk|rk)-[A-Za-z0-9_-]{8,}"""),
        Regex("""()\bAIza[0-9A-Za-z_-]{20,}""")
    )

    fun install(storageDir: () -> File, flush: () -> Unit) {
        if (!installed.compareAndSet(false, true)) return
        val previous = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            val now = System.nanoTime() / 1_000_000L
            val due = !recordedOnce.get() || now - lastRecordedAt.get() >= MIN_RECORD_INTERVAL_MS
            if (due && recording.compareAndSet(false, true)) {
                try {
                    recordedOnce.set(true)
                    lastRecordedAt.set(now)
                    record(storageDir(), thread, error, flush)
                } catch (_: Throwable) {
                    // Recording is best effort; the platform's own handling below must still run.
                } finally {
                    recording.set(false)
                }
            }
            if (previous != null) {
                previous.uncaughtException(thread, error)
            } else {
                System.err.print("Exception in thread \"" + thread.name + "\" ")
                error.printStackTrace()
            }
        }
    }

    /** Flushes within the limit, then writes the report. Returns whether the flush finished. */
    internal fun record(storageDir: File, thread: Thread, error: Throwable, flush: () -> Unit): Boolean {
        val saved = flushWithin(FLUSH_LIMIT_MS, flush)
        val report = File(storageDir, "crash/last-crash.txt")
        report.parentFile?.mkdirs()
        AtomicFileStorage.writeText(report, describe(thread, error, saved))
        return saved
    }

    /** The report left by the previous run, removed once read so it is mentioned only once. */
    fun takeReport(storageDir: File): String? {
        val report = File(storageDir, "crash/last-crash.txt")
        if (!report.isFile) return null
        return runCatching { AtomicFileStorage.readText(report) }.getOrNull().also { report.delete() }
    }

    /** Whether a report says the newest changes reached the disk before the error. */
    fun reportSaysSaved(report: String): Boolean = report.lineSequence().any { it == "saved=true" }

    internal fun redact(text: String): String = secrets.fold(text) { result, pattern ->
        pattern.replace(result) { match -> match.groupValues[1] + "[removed]" }
    }

    private fun flushWithin(limitMs: Long, flush: () -> Unit): Boolean {
        val done = CountDownLatch(1)
        val finished = AtomicBoolean(false)
        Thread({
            try {
                flush()
                finished.set(true)
            } catch (_: Throwable) {
            } finally {
                done.countDown()
            }
        }, "parchment-crash-flush").apply { isDaemon = true }.start()
        return done.await(limitMs, TimeUnit.MILLISECONDS) && finished.get()
    }

    private fun describe(thread: Thread, error: Throwable, saved: Boolean): String = buildString {
        appendLine("saved=$saved")
        appendLine("thread=${redact(thread.name)}")
        val seen = HashSet<Throwable>()
        var current: Throwable? = error
        var depth = 0
        while (current != null && depth < MAX_CAUSES && seen.add(current)) {
            if (depth > 0) append("Caused by: ")
            append(current.javaClass.name)
            current.message?.let { append(": ").append(redact(it).take(MAX_MESSAGE_CHARS)) }
            appendLine()
            current.stackTrace.take(MAX_FRAMES).forEach { frame -> appendLine("    at $frame") }
            current = current.cause
            depth++
        }
    }.take(MAX_REPORT_CHARS)
}
