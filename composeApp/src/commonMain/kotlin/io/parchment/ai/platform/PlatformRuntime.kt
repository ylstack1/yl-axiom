package io.parchment.ai.platform

/** Devices with less physical memory than this count as low-memory (a nominal 4 GB device reports a little less). */
internal const val LOW_MEMORY_BYTES = 4L * 1024L * 1024L * 1024L

/** What the previous run left when an error escaped it: the redacted details, and whether its saves finished. */
data class CrashReport(val text: String, val saved: Boolean)

expect object PlatformRuntime {
    val platformName: String
    val isAndroid: Boolean

    /** Under 4 GB of RAM (or Android's low-RAM flag): caches shrink so the app stays responsive. */
    val isLowMemoryDevice: Boolean
    val backgroundExecutionNotice: kotlinx.coroutines.flow.StateFlow<String>

    fun currentTimeMillis(): Long
    fun monotonicTimeMillis(): Long
    fun createRunId(): String

    fun registerStateFlushCallback(callback: (blocking: Boolean) -> Unit)
    fun flushPendingStateNow()

    /** The report an uncaught error left in the previous run, once: reading it removes it. */
    fun takeCrashReport(): CrashReport?
    fun flushPendingStateAsync()
    fun registerBackHandler(handler: (() -> Boolean)?)
    fun handleSystemBackPressed(): Boolean
    fun isIgnoringBatteryOptimizations(): Boolean
    fun requestUnrestrictedBackgroundBattery()

    // Process-owned run snapshots include preparation, tools, compaction and approval waits.
    fun synchronizeBackgroundExecution(owners: Int, runnable: Int, status: String)
    fun notifyLongDecodeProgress(modelName: String, outputTokens: Int, elapsedSeconds: Int)

    // In-app update download and install.
    fun createOtaStagedDownload(assetName: String): OtaStagedDownload

    suspend fun <T> decodeJsonStream(
        channel: io.ktor.utils.io.ByteReadChannel,
        json: kotlinx.serialization.json.Json,
        deserializer: kotlinx.serialization.DeserializationStrategy<T>
    ): T
}
