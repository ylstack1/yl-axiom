package io.parchment.ai.network

import io.ktor.utils.io.ByteReadChannel
import io.ktor.utils.io.readLine
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.runtime.AppForeground
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

// SSE and plain-JSON fallback stream reader with background stall detection and adaptive line timeouts.

internal const val DEFAULT_RETURN_GRACE_MS = 3_000L
internal const val DEFAULT_MAX_SSE_BUFFER_CHARS = 64 * 1024 * 1024
internal const val STREAM_CUT_WHILE_AWAY = "SSE stream timed out while the app was in the background."

internal suspend fun readProviderSseOrJsonFallback(
    channel: ByteReadChannel,
    streamFinished: () -> Boolean,
    onSseEvent: suspend (eventType: String, data: String) -> Unit,
    onPlainJsonFallback: suspend (fullJson: String) -> Unit,
    maxBufferedChars: Int = DEFAULT_MAX_SSE_BUFFER_CHARS,
    returnGraceMs: Long = DEFAULT_RETURN_GRACE_MS
) {
    var lastLineAt = PlatformRuntime.monotonicTimeMillis()
    var sawAnyData = false
    coroutineScope {
        val stallWatch = launch {
            AppForeground.resumedAt.drop(1).collectLatest { resumedAt ->
                delay(returnGraceMs)
                if (AppForeground.leftAt in 1..resumedAt && sawAnyData && lastLineAt <= resumedAt) {
                    channel.cancel(IllegalStateException(STREAM_CUT_WHILE_AWAY))
                }
            }
        }
        try {
            var currentEvent = ""
            val dataLines = mutableListOf<String>()
            var dataLinesChars = 0
            val rawFallbackBuffer = StringBuilder()
            var sawSseLine = false
            var sawSseComment = false
            var sawEvent = false
            var sawDone = false
            suspend fun deliver(eventType: String, data: String) {
                if (data == "[DONE]") sawDone = true else sawEvent = true
                onSseEvent(eventType, data)
            }

            while (!channel.isClosedForRead) {
                // Adaptive per-line SSE idle watchdog:
                // - 90 seconds once lines have started arriving (catches a proxy that stalls mid-reply)
                // - 300 seconds before the first SSE line arrives (allows deep pre-token reasoning on slow endpoints)
                val perLineTimeoutMs = if (sawSseLine) 90_000L else 300_000L
                val line = withTimeoutOrNull(perLineTimeoutMs) {
                    channel.readLine().also { if (it != null) lastLineAt = PlatformRuntime.monotonicTimeMillis() }
                } ?: if (channel.isClosedForRead) {
                    break
                } else {
                    error("SSE stream timed out waiting for next event after ${perLineTimeoutMs / 1000}s (connection stalled).")
                }
                when {
                    line.startsWith(":") -> {
                        sawSseComment = true
                    }
                    line.startsWith("id:") || line.startsWith("retry:") -> {
                        sawSseLine = true
                    }
                    line.startsWith("event:") -> {
                        sawSseLine = true
                        sawAnyData = true
                        if (dataLines.isNotEmpty()) {
                            val combined = dataLines.joinToString("\n")
                            dataLines.clear()
                            dataLinesChars = 0
                            deliver(currentEvent, combined)
                        }
                        currentEvent = fieldValue(line, "event:".length)
                    }
                    line.startsWith("data:") -> {
                        sawSseLine = true
                        sawAnyData = true
                        val dataVal = fieldValue(line, "data:".length)
                        if (dataVal == "[DONE]") {
                            if (dataLines.isNotEmpty()) {
                                deliver(currentEvent, dataLines.joinToString("\n"))
                                dataLines.clear()
                                dataLinesChars = 0
                            }
                            deliver(currentEvent, "[DONE]")
                            currentEvent = ""
                        } else if (dataLines.isEmpty() && isCompleteJsonObject(dataVal)) {
                            // A complete JSON object on one data: line is delivered at once, without waiting for
                            // the blank line: some proxies hold that line back or merge records.
                            deliver(currentEvent, dataVal)
                            currentEvent = ""
                        } else {
                            check(dataLinesChars.toLong() + dataVal.length.toLong() <= maxBufferedChars.toLong()) {
                                "SSE event exceeded the $maxBufferedChars character limit."
                            }
                            dataLines.add(dataVal)
                            dataLinesChars += dataVal.length + 1
                        }
                    }
                    line.isBlank() -> {
                        if (dataLines.isNotEmpty()) {
                            val combinedData = dataLines.joinToString("\n")
                            dataLines.clear()
                            dataLinesChars = 0
                            deliver(currentEvent, combinedData)
                            currentEvent = ""
                        }
                    }
                    else -> {
                        if (!sawSseLine) {
                            check(rawFallbackBuffer.length.toLong() + line.length.toLong() + 1L <= maxBufferedChars.toLong()) {
                                "Response body exceeded the $maxBufferedChars character limit."
                            }
                            rawFallbackBuffer.appendLine(line)
                        }
                    }
                }
            }

            if (dataLines.isNotEmpty()) {
                deliver(currentEvent, dataLines.joinToString("\n"))
            } else if (!sawSseLine && rawFallbackBuffer.isNotBlank()) {
                onPlainJsonFallback(rawFallbackBuffer.toString().trim())
            }
            // A clean close is not a finished answer: proxies can cut a stream without a socket error.
            if ((sawEvent || sawSseComment) && !sawDone && !streamFinished()) {
                error("Stream ended before the provider finished its response.")
            }
        } finally {
            stallWatch.cancel()
        }
    }
}

/** `line.substring(start).trim()` with one copy instead of two: the value after an SSE field name. */
private fun fieldValue(line: String, start: Int): String {
    var from = start
    var to = line.length
    while (from < to && line[from].isWhitespace()) from++
    while (to > from && line[to - 1].isWhitespace()) to--
    return line.substring(from, to)
}

private fun isCompleteJsonObject(text: String): Boolean {
    if (!text.startsWith("{")) return false
    var depth = 0
    var inString = false
    var escaped = false
    for ((index, ch) in text.withIndex()) {
        when {
            escaped -> escaped = false
            inString -> if (ch == '\\') escaped = true else if (ch == '"') inString = false
            ch == '"' -> inString = true
            ch == '{' || ch == '[' -> depth++
            ch == '}' || ch == ']' -> if (--depth == 0) return index == text.lastIndex
        }
    }
    return false
}
