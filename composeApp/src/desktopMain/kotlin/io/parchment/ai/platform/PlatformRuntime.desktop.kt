package io.parchment.ai.platform

import java.io.File

actual object PlatformRuntime {
    actual fun createRunId(): String = java.util.UUID.randomUUID().toString()
    private val backgroundNoticeState = kotlinx.coroutines.flow.MutableStateFlow("")
    actual val backgroundExecutionNotice: kotlinx.coroutines.flow.StateFlow<String> = backgroundNoticeState
    actual val platformName: String =
        "${System.getProperty("os.name") ?: "Desktop"} (${System.getProperty("os.arch") ?: "universal"})"
    actual val isAndroid: Boolean = false
    actual val isLowMemoryDevice: Boolean by lazy { WindowsMemory.totalPhysicalBytes() in 1 until LOW_MEMORY_BYTES }

    @Volatile
    var windowStatusCallback: ((String) -> Unit)? = null
    @Volatile
    private var stateFlushCallback: ((Boolean) -> Unit)? = null
    @Volatile
    private var backHandlerCallback: (() -> Boolean)? = null

    private val windowsPowerLeaseLazy = lazy { WindowsPowerLease() }
    private val windowsPowerLease: WindowsPowerLease
        get() = windowsPowerLeaseLazy.value
    private val isWindows: Boolean
        get() = System.getProperty("os.name").orEmpty().startsWith("Windows", ignoreCase = true)

    @Volatile
    private var lastReportedDecodeSec: Int = -1

    init {
        runCatching {
            Runtime.getRuntime().addShutdownHook(Thread {
                try {
                    flushPendingStateNow()
                } finally {
                    if (isWindows && windowsPowerLeaseLazy.isInitialized()) windowsPowerLease.close()
                }
            })
        }
    }

    actual fun currentTimeMillis(): Long = System.currentTimeMillis()
    actual fun monotonicTimeMillis(): Long = System.nanoTime() / 1_000_000L

    actual fun registerStateFlushCallback(callback: (blocking: Boolean) -> Unit) {
        stateFlushCallback = callback
    }

    actual fun flushPendingStateNow() {
        runCatching { stateFlushCallback?.invoke(true) }
    }

    actual fun takeCrashReport(): CrashReport? =
        CrashRecorder.takeReport(DesktopPlatformHost.resolveStorageDir())?.let { CrashReport(it, CrashRecorder.reportSaysSaved(it)) }

    actual fun flushPendingStateAsync() {
        runCatching { stateFlushCallback?.invoke(false) }
    }

    actual fun registerBackHandler(handler: (() -> Boolean)?) {
        backHandlerCallback = handler
    }

    actual fun handleSystemBackPressed(): Boolean {
        return runCatching { backHandlerCallback?.invoke() == true }.getOrDefault(false)
    }

    actual fun isIgnoringBatteryOptimizations(): Boolean = true

    actual fun requestUnrestrictedBackgroundBattery() {}

    private fun reportDecodeWindowStatus(text: String) {
        val leaseSnapshot = if (isWindows && windowsPowerLeaseLazy.isInitialized()) windowsPowerLease.snapshot else null
        val unavailable = leaseSnapshot?.status == WindowsPowerLeaseStatus.UNAVAILABLE
        backgroundNoticeState.value = if (unavailable) "Keep-awake unavailable: ${leaseSnapshot?.error.orEmpty()}" else ""
        windowStatusCallback?.invoke(text + if (unavailable) " — Keep-awake unavailable" else "")
    }

    actual fun synchronizeBackgroundExecution(owners: Int, runnable: Int, status: String) {
        if (isWindows && (runnable > 0 || windowsPowerLeaseLazy.isInitialized())) windowsPowerLease.reconcile(runnable)
        if (owners == 0) lastReportedDecodeSec = -1
        reportDecodeWindowStatus(status)
    }

    actual fun notifyLongDecodeProgress(modelName: String, outputTokens: Int, elapsedSeconds: Int) {
        if (elapsedSeconds < 0 || elapsedSeconds == lastReportedDecodeSec) return
        lastReportedDecodeSec = elapsedSeconds
        if (isWindows && windowsPowerLeaseLazy.isInitialized()) windowsPowerLease.refresh()
        reportDecodeWindowStatus("Parchment — " + modelName + " (+" + outputTokens + " tok • " + elapsedSeconds + "s)")
    }

    actual fun createOtaStagedDownload(assetName: String): OtaStagedDownload {
        val safeName = assetName.replace(Regex("[^a-zA-Z0-9._-]"), "_").ifBlank { "Parchment-Windows-Setup.exe" }
        // The installer runs silently and is not signature-checked by the system, so a published checksum is required.
        return OtaStagedFile(File(File(DesktopPlatformHost.resolveStorageDir(), "ota"), safeName), requireDigest = true, installer = ::installOtaExecutable)
    }

    actual suspend fun <T> decodeJsonStream(
        channel: io.ktor.utils.io.ByteReadChannel,
        json: kotlinx.serialization.json.Json,
        deserializer: kotlinx.serialization.DeserializationStrategy<T>
    ): T = decodeByteChannelJson(channel, json, deserializer)

    private fun installOtaExecutable(installerFile: File): Result<String> = runCatching {
        // Save everything pending before the installer starts.
        flushPendingStateNow()

        val tempDir = checkNotNull(installerFile.parentFile) { "The update has no folder." }

        val localAppData = System.getenv("LOCALAPPDATA") ?: ""
        val installedExe = File(localAppData, "Programs\\Parchment\\Parchment.exe")
        val fallbackExe = File(System.getProperty("user.dir"), "Parchment.exe")
        val relaunchTarget = if (installedExe.exists()) installedExe.absolutePath else fallbackExe.absolutePath

        val currentPid = runCatching { ProcessHandle.current().pid() }.getOrDefault(0L)

        // Write a tiny updater script that waits for the current process PID to exit completely,
        // runs the silent Inno Setup update, and automatically relaunches Parchment.exe
        val psScriptFile = File(tempDir, "apply-ota.ps1")
        psScriptFile.writeText(
            """
            if ($currentPid -gt 0) {
                Wait-Process -Id $currentPid -Timeout 15 -ErrorAction SilentlyContinue
            }
            Start-Sleep -Milliseconds 600
            Start-Process -FilePath "${installerFile.absolutePath}" -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CLOSEAPPLICATIONS' -Wait
            if (Test-Path "$relaunchTarget") {
                Start-Process -FilePath "$relaunchTarget"
            } elseif (Test-Path "${installedExe.absolutePath}") {
                Start-Process -FilePath "${installedExe.absolutePath}"
            }
            Remove-Item -Force "${installerFile.absolutePath}" -ErrorAction SilentlyContinue
            """.trimIndent(),
            Charsets.UTF_8
        )

        ProcessBuilder(
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy", "Bypass",
            "-WindowStyle", "Hidden",
            "-File", psScriptFile.absolutePath
        ).start()

        // Exit current instance after 400ms so the silent installer can replace files cleanly
        Thread {
            Thread.sleep(400L)
            kotlin.system.exitProcess(0)
        }.apply { isDaemon = true }.start()

        "Downloaded ${installerFile.length() / 1024} KB. Applying silent Windows update & restarting Parchment…"
    }
}
