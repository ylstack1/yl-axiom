package io.parchment.ai

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.platform.AtomicFileStorage
import io.parchment.ai.platform.DesktopPlatformHost
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.WindowsNative
import io.parchment.ai.protocol.O200kTokenizer
import io.parchment.ai.ui.ParchmentApp
import io.parchment.ai.ui.compactStateJson
import io.parchment.ai.ui.createDefaultAppState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import org.jetbrains.skia.FontMgr
import java.io.File
import java.nio.file.Files
import kotlin.system.exitProcess
import org.jetbrains.skia.impl.Library as SkiaLibrary

/** Packaging runs `MainKt --cds-warmup` under `-XX:AOTCacheOutput=$APPDIR/parchment.aot` to record classes & JIT profiles. */
internal const val CDS_WARMUP_FLAG = "--cds-warmup"

/** Generous for 2-core machines; this i5 needs about 13 s including the archive dump. */
private const val WARMUP_TIME_LIMIT_MS = 60_000L

/** Exit code of a warm-up the watchdog ended; the installer ignores it, `perf.py` reports it. */
private const val WARMUP_TIMED_OUT = 3
private const val WARMUP_FRAMES = 90

private const val WARMUP_MARKDOWN = "# Warm-up\n\nSome *prose* with `code` and \$\$x^2\$\$.\n\n```kotlin\nval x = 1\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"

/**
 * Starts the real app once without a window, then exits, so the JVM writes the app's class archive
 * (`$APPDIR/parchment.jsa`, `-XX:+AutoCreateSharedArchive`) at the installed path before the first real launch.
 * The archive keeps only the classes this run loads and is not rebuilt later, so it renders the actual UI (a
 * narrower warm-up measured 1.72 s per start instead of 1.40 s).
 *
 * Safe inside a silent install: it works in a throwaway folder (never the user's data) seeded with one chat, no
 * model, no MCP servers and the update check off, so start-up sends no network request; the folder is deleted
 * before exit. If anything hangs, the watchdog terminates the process without an archive and the first real launch
 * builds it.
 */
internal fun runCdsWarmup(): Nothing {
    Thread({
        Thread.sleep(WARMUP_TIME_LIMIT_MS)
        // Not Runtime.halt: it still dumps the archive, and a partial one would stay (the JVM only replaces an invalid
        // archive), leaving every later start slower. Terminating writes nothing; the first real launch then writes
        // a complete archive, as before the warm-up existed.
        runCatching { WindowsNative.terminateCurrentProcess(WARMUP_TIMED_OUT) }
        Runtime.getRuntime().halt(WARMUP_TIMED_OUT)
    }, "cds-warmup-watchdog").apply { isDaemon = true; start() }

    val dataDir = Files.createTempDirectory("parchment-cds-warmup-").toFile()
    try {
        DesktopPlatformHost.useStorageDirectory(dataDir)
        runCatching { SkiaLibrary.staticLoad() }
        runCatching { FontMgr.default }
        runCatching { PlatformPickers.preloadStartupFonts() }
        runCatching { predecodedAppIconPainter }
        runCatching { loadSavedWindowGeometry() }
        runCatching { windowCaption("Parchment", "Warm-up") }
        val loader = DesktopPlatformHost::class.java.classLoader
        listOf(
            "androidx.compose.ui.window.Application_desktopKt",
            "androidx.compose.ui.window.Window_desktopKt",
            "androidx.compose.ui.awt.ComposeWindow",
            "androidx.compose.ui.scene.WindowComposeSceneLayer",
            "org.jetbrains.skiko.SkiaLayer",
            "org.jetbrains.skiko.redrawer.Direct3DRedrawer",
            "org.jetbrains.skiko.context.Direct3DContextHandler",
            "io.parchment.ai.platform.WindowsChrome",
            "io.parchment.ai.platform.WindowsNative"
        ).forEach { name -> runCatching { Class.forName(name, false, loader) } }
        runCatching { seedWarmupData() }
        runCatching { O200kTokenizer.warmUp() }
        runCatching { renderAppHeadless() }
        // Write what the app queued now, then stop later flushes from recreating the deleted folder.
        runCatching { PlatformRuntime.flushPendingStateNow() }
        PlatformRuntime.registerStateFlushCallback { }
    } finally {
        deleteWarmupData(dataDir)
    }
    exitProcess(0)
}

private fun seedWarmupData() {
    val base = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
    val template = base.sessions.first()
    val primary = template.copy(
        title = "Warm-up",
        messages = listOf(
            MessageNode("u", MessageRole.USER, listOf(MessageVariant("uv", content = "Show me the formats."))),
            MessageNode(
                "a",
                MessageRole.ASSISTANT,
                listOf(
                    MessageVariant(
                        "av",
                        content = WARMUP_MARKDOWN,
                        thinkingSummary = "Thinking about the formats.",
                        toolCalls = listOf(ToolCallRecord("t", "fs_read", "{\"path\":\"README.md\"}", resultText = "text"))
                    )
                )
            )
        )
    )
    PlatformStorage.saveChatSessionJson(primary.id, compactStateJson.encodeToString(primary))
    for (idx in 1..6) {
        val extra = template.copy(
            id = "warmup-$idx",
            title = "Archived conversation $idx",
            updatedAt = 1_700_000_000_000L + idx,
            messages = listOf(
                MessageNode("u$idx", MessageRole.USER, listOf(MessageVariant("uv$idx", content = "Question $idx"))),
                MessageNode("a$idx", MessageRole.ASSISTANT, listOf(MessageVariant("av$idx", content = "Short reply $idx.")))
            )
        )
        PlatformStorage.saveChatSessionJson(extra.id, compactStateJson.encodeToString(extra))
    }
    PlatformStorage.saveAppStateJson(
        compactStateJson.encodeToString(base.copy(sessions = emptyList(), activeSessionId = primary.id, openTabSessionIds = listOf(primary.id)))
    )
}

private fun renderAppHeadless() = runBlocking(Dispatchers.Swing) {
    val scene = ImageComposeScene(1280, 800, density = Density(1f), coroutineContext = Dispatchers.Swing)
    try {
        scene.setContent { ParchmentApp() }
        var time = 0L
        repeat(WARMUP_FRAMES) {
            Snapshot.sendApplyNotifications()
            scene.render(time).close()
            time += 16_000_000L
            delay(16)
        }
    } finally {
        scene.close()
    }
}

private fun deleteWarmupData(dataDir: File) {
    val temporaryRoot = File(System.getProperty("java.io.tmpdir")).canonicalFile.toPath()
    val path = dataDir.canonicalFile.toPath()
    if (path.startsWith(temporaryRoot) && dataDir.name.startsWith("parchment-cds-warmup-")) {
        runCatching { AtomicFileStorage.deleteTree(path) }
    }
}
