package io.parchment.ai

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.painter.BitmapPainter
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toComposeImageBitmap
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.WindowPlacement
import androidx.compose.ui.window.WindowPosition
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import io.parchment.ai.platform.CrashRecorder
import io.parchment.ai.platform.DesktopPlatformHost
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.WindowsChrome
import io.parchment.ai.runtime.AppForeground
import io.parchment.ai.ui.AppShortcutDispatcher
import io.parchment.ai.ui.LocalParchmentPalette
import io.parchment.ai.ui.ParchmentApp
import io.parchment.ai.ui.initialAppStateLoad
import io.parchment.ai.ui.preloadProcessStateInBackground
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.withContext
import org.jetbrains.skia.FontMgr
import org.jetbrains.skia.Image
import java.awt.Component
import java.awt.Container
import java.awt.Dimension
import java.awt.GraphicsEnvironment
import java.awt.MouseInfo
import java.awt.Rectangle
import java.awt.datatransfer.DataFlavor
import java.awt.dnd.DnDConstants
import java.awt.dnd.DropTarget
import java.awt.dnd.DropTargetAdapter
import java.awt.dnd.DropTargetDragEvent
import java.awt.dnd.DropTargetDropEvent
import java.awt.dnd.DropTargetEvent
import java.awt.event.ContainerAdapter
import java.awt.event.ContainerEvent
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.io.File
import java.util.Properties
import kotlin.system.exitProcess
import java.awt.Color as AwtColor
import org.jetbrains.skia.impl.Library as SkiaLibrary
internal data class SavedWindowGeometry(
    val isMaximized: Boolean,
    val widthDp: Int,
    val heightDp: Int,
    val xDp: Int,
    val yDp: Int
)

private fun windowBoundsFile(): File = File(PlatformStorage.getAppStorageDirectoryPath(), "window-bounds.properties")

internal fun loadSavedWindowGeometry(): SavedWindowGeometry {
    return runCatching {
        val f = windowBoundsFile()
        if (!f.exists() || !f.isFile) {
            return SavedWindowGeometry(isMaximized = true, widthDp = 1400, heightDp = 900, xDp = -1, yDp = -1)
        }
        val props = Properties()
        f.inputStream().use { props.load(it) }
        SavedWindowGeometry(
            isMaximized = props.getProperty("isMaximized", "true").toBoolean(),
            widthDp = (props.getProperty("widthDp", "1400").toIntOrNull() ?: 1400).coerceIn(640, 7680),
            heightDp = (props.getProperty("heightDp", "900").toIntOrNull() ?: 900).coerceIn(480, 4320),
            xDp = props.getProperty("xDp", "-1").toIntOrNull() ?: -1,
            yDp = props.getProperty("yDp", "-1").toIntOrNull() ?: -1
        )
    }.getOrDefault(SavedWindowGeometry(isMaximized = true, widthDp = 1400, heightDp = 900, xDp = -1, yDp = -1))
}

/**
 * True when the window's title strip (top 40 dp of a saved floating window) still overlaps a connected screen by
 * at least 100x40 dp, so a window saved on a monitor that has since been unplugged reopens centered instead of
 * off-screen. Screen bounds are AWT user-space, which matches Compose dp on scaled Windows displays.
 */
internal fun savedPositionIsVisible(xDp: Int, yDp: Int, widthDp: Int, screens: List<Rectangle>): Boolean {
    val titleStrip = Rectangle(xDp, yDp, widthDp.coerceAtLeast(1), 40)
    return screens.any { screen ->
        val overlap = screen.intersection(titleStrip)
        !overlap.isEmpty && overlap.width >= 100 && overlap.height >= 40
    }
}

private fun connectedScreenBounds(): List<Rectangle> = runCatching {
    GraphicsEnvironment.getLocalGraphicsEnvironment().screenDevices.map { it.defaultConfiguration.bounds }
}.getOrDefault(emptyList())

private fun saveWindowGeometry(geom: SavedWindowGeometry) {
    runCatching {
        val f = windowBoundsFile()
        f.parentFile?.mkdirs()
        val props = Properties().apply {
            setProperty("isMaximized", geom.isMaximized.toString())
            setProperty("widthDp", geom.widthDp.toString())
            setProperty("heightDp", geom.heightDp.toString())
            setProperty("xDp", geom.xDp.toString())
            setProperty("yDp", geom.yDp.toString())
        }
        f.outputStream().use { props.store(it, "Parchment Desktop Window Geometry") }
    }
}

private fun attachDropTargetRecursively(component: Component, createTarget: () -> DropTarget) {
    runCatching {
        component.dropTarget = createTarget()
        if (component is Container) {
            component.addContainerListener(object : ContainerAdapter() {
                override fun componentAdded(e: ContainerEvent) {
                    e.child?.let { attachDropTargetRecursively(it, createTarget) }
                }
            })
            for (child in component.components) {
                attachDropTargetRecursively(child, createTarget)
            }
        }
    }
}

/** "Chat — Parchment" while idle; a run's progress text (which starts with the app name) replaces it. */
internal fun windowCaption(runStatus: String, captionTitle: String): String {
    val idle = runStatus.isBlank() || runStatus == "Parchment"
    val title = captionTitle.trim()
    return when {
        !idle -> runStatus
        title.isEmpty() -> "Parchment"
        else -> "$title — Parchment"
    }
}

internal val predecodedAppIconPainter: BitmapPainter? by lazy {
    runCatching {
        val stream = Thread.currentThread().contextClassLoader?.getResourceAsStream("icon/parchment-icon.png")
            ?: DesktopPlatformHost::class.java.classLoader?.getResourceAsStream("icon/parchment-icon.png")
        val bytes = stream?.use { it.readBytes() }
        if (bytes != null && bytes.isNotEmpty()) {
            BitmapPainter(Image.makeFromEncoded(bytes).use { it.toComposeImageBitmap() })
        } else null
    }.getOrNull()
}

private fun preloadDesktopGraphicsInBackground() {
    Thread({
        runCatching { SkiaLibrary.staticLoad() }
        runCatching { FontMgr.default }
        runCatching { PlatformPickers.preloadStartupFonts() }
        runCatching { predecodedAppIconPainter }
    }, "parchment-gfx-preload").apply {
        isDaemon = true
        start()
    }
}

@OptIn(kotlinx.coroutines.FlowPreview::class)
fun main(args: Array<String>) {
    if ("--aot-verify" in args) exitProcess(0)
    if (CDS_WARMUP_FLAG in args) runCdsWarmup()
    // After the warm-up check: headless AOT training runs never record a crash into a throwaway folder.
    CrashRecorder.install({ DesktopPlatformHost.resolveStorageDir() }) { PlatformRuntime.flushPendingStateNow() }
    // Enable hardware VSync and Swing frame pacing so Skia renders smoothly at 60Hz / 120Hz / 144Hz / 240Hz.
    System.setProperty("skiko.vsync.enabled", "true")
    System.setProperty("skiko.swing.frame.pacing", "true")
    // Cap Skia Ganesh GPU resource cache at 48 MiB (32 MiB on <4 GiB PCs): large enough for 4K HiDPI surfaces and
    // DirectWrite glyph atlases with zero corruption, while preventing D3D12MA from ballooning past 250 MB on Intel Xe UMA.
    if (System.getProperty("skiko.gpu.resourceCacheLimit") == null) {
        val limitBytes = if (PlatformRuntime.isLowMemoryDevice) "33554432" else "50331648"
        System.setProperty("skiko.gpu.resourceCacheLimit", limitBytes)
    }
    preloadProcessStateInBackground()
    preloadDesktopGraphicsInBackground()
    val startupGeometry = loadSavedWindowGeometry()

    application {
        // Pre-seed the chat title from initialAppStateLoad so Frame 1 opens directly with "Chat — Parchment".
        val initialCaption = remember {
            val snap = initialAppStateLoad.value.first
            (snap.sessions.find { it.id == snap.activeSessionId } ?: snap.sessions.firstOrNull())
                ?.title?.trim().orEmpty()
        }
        var runStatus by remember { mutableStateOf("Parchment") }
        var captionTitle by remember { mutableStateOf(initialCaption) }
        PlatformRuntime.windowStatusCallback = { newTitle -> runStatus = newTitle }
        val windowTitle = windowCaption(runStatus, captionTitle)

        val initialGeom = remember { startupGeometry }

        val windowState = rememberWindowState(
            placement = if (initialGeom.isMaximized) WindowPlacement.Maximized else WindowPlacement.Floating,
            size = DpSize(initialGeom.widthDp.dp, initialGeom.heightDp.dp),
            // (-1, -1) is the "never placed" marker; other negative positions are real left/upper monitors.
            position = if (!initialGeom.isMaximized &&
                !(initialGeom.xDp == -1 && initialGeom.yDp == -1) &&
                savedPositionIsVisible(initialGeom.xDp, initialGeom.yDp, initialGeom.widthDp, connectedScreenBounds())
            ) {
                WindowPosition(initialGeom.xDp.dp, initialGeom.yDp.dp)
            } else {
                WindowPosition.Aligned(Alignment.Center)
            }
        )

        // Keep track of the last floating (non-maximized) size & position so restoring from Maximized works cleanly
        var lastFloatingWidth by remember { mutableStateOf(initialGeom.widthDp) }
        var lastFloatingHeight by remember { mutableStateOf(initialGeom.heightDp) }
        var lastFloatingX by remember { mutableStateOf(initialGeom.xDp) }
        var lastFloatingY by remember { mutableStateOf(initialGeom.yDp) }

        LaunchedEffect(windowState) {
            snapshotFlow {
                Triple(
                    windowState.placement,
                    windowState.size,
                    windowState.position
                )
            }
                .debounce(250L)
                .collectLatest { (placement, size, pos) ->
                    val isMax = placement == WindowPlacement.Maximized || placement == WindowPlacement.Fullscreen
                    if (!isMax) {
                        val w = size.width.value.toInt()
                        val h = size.height.value.toInt()
                        if (w >= 400 && h >= 300) {
                            lastFloatingWidth = w
                            lastFloatingHeight = h
                        }
                        if (pos.isSpecified) {
                            lastFloatingX = pos.x.value.toInt()
                            lastFloatingY = pos.y.value.toInt()
                        }
                    }
                    val geometry = SavedWindowGeometry(
                        isMaximized = isMax,
                        widthDp = lastFloatingWidth,
                        heightDp = lastFloatingHeight,
                        xDp = lastFloatingX,
                        yDp = lastFloatingY
                    )
                    // A resize drag emits many sizes; the debounced write still must not touch the disk on the EDT.
                    withContext(Dispatchers.IO) { saveWindowGeometry(geometry) }
                }
        }

        val shortcuts = remember { AppShortcutDispatcher() }

        val appIconPainter = remember { predecodedAppIconPainter }

        val closeAndSave: () -> Unit = {
            val isMax = windowState.placement == WindowPlacement.Maximized || windowState.placement == WindowPlacement.Fullscreen
            saveWindowGeometry(
                SavedWindowGeometry(
                    isMaximized = isMax,
                    widthDp = lastFloatingWidth,
                    heightDp = lastFloatingHeight,
                    xDp = lastFloatingX,
                    yDp = lastFloatingY
                )
            )
            PlatformRuntime.flushPendingStateNow()
            exitApplication()
        }

        Window(
            onCloseRequest = closeAndSave,
            state = windowState,
            title = windowTitle,
            icon = appIconPainter,
            onKeyEvent = { keyEvent -> handleUnconsumedWindowKey(keyEvent, shortcuts) }
        ) {
            LaunchedEffect(window) {
                fun isSupportedDrop(dtde: DropTargetDragEvent): Boolean = dtde.isDataFlavorSupported(DataFlavor.javaFileListFlavor) ||
                    dtde.isDataFlavorSupported(DataFlavor.imageFlavor)

                val makeDropTarget: () -> DropTarget = {
                    DropTarget(
                        null,
                        DnDConstants.ACTION_COPY_OR_MOVE,
                        object : DropTargetAdapter() {
                            override fun dragEnter(dtde: DropTargetDragEvent) {
                                if (isSupportedDrop(dtde)) {
                                    dtde.acceptDrag(DnDConstants.ACTION_COPY)
                                    PlatformPickers.dragHoverCallback?.invoke(true)
                                }
                            }

                            override fun dragOver(dtde: DropTargetDragEvent) {
                                if (isSupportedDrop(dtde)) {
                                    dtde.acceptDrag(DnDConstants.ACTION_COPY)
                                    PlatformPickers.dragHoverCallback?.invoke(true)
                                }
                            }

                            override fun dragExit(dte: DropTargetEvent) {
                                // Only clear drag hover if cursor actually left the window bounds (prevents child-component flicker)
                                val stillInside = runCatching {
                                    val mousePt = MouseInfo.getPointerInfo()?.location
                                    val winBounds = window.bounds
                                    mousePt != null && winBounds.contains(mousePt)
                                }.getOrDefault(false)
                                if (!stillInside) {
                                    PlatformPickers.dragHoverCallback?.invoke(false)
                                }
                            }

                            override fun drop(dtde: DropTargetDropEvent) {
                                PlatformPickers.dragHoverCallback?.invoke(false)
                                runCatching {
                                    if (dtde.isDataFlavorSupported(DataFlavor.javaFileListFlavor) ||
                                        dtde.isDataFlavorSupported(DataFlavor.imageFlavor)
                                    ) {
                                        dtde.acceptDrop(DnDConstants.ACTION_COPY)
                                        val pickedFiles = PlatformPickers.extractTransferableFiles(dtde.transferable)
                                        if (pickedFiles.isNotEmpty()) {
                                            PlatformPickers.filesDroppedCallback?.invoke(pickedFiles)
                                            dtde.dropComplete(true)
                                        } else {
                                            dtde.dropComplete(false)
                                        }
                                    } else {
                                        dtde.rejectDrop()
                                    }
                                }.onFailure {
                                    runCatching { dtde.dropComplete(false) }
                                }
                            }
                        },
                        true
                    )
                }
                attachDropTargetRecursively(window, makeDropTarget)
                window.minimumSize = Dimension(640, 480)
            }
            DisposableEffect(window) {
                val listener = object : WindowAdapter() {
                    override fun windowIconified(e: WindowEvent?) {
                        AppForeground.markLeft()
                        PlatformStorage.trimMemoryCaches()
                        Thread({ System.gc() }, "parchment-concurrent-uncommit").apply { isDaemon = true; start() }
                    }
                    override fun windowDeiconified(e: WindowEvent?) = AppForeground.markResumed()
                }
                window.addWindowListener(listener)
                onDispose { window.removeWindowListener(listener) }
            }
            // Concurrent post-hydration sweep so Generational Shenandoah uncommits startup allocation churn within ~2 s.
            LaunchedEffect(Unit) {
                kotlinx.coroutines.delay(1_800L)
                Thread({ System.gc() }, "parchment-idle-sweep-1").apply { isDaemon = true; start() }
                kotlinx.coroutines.delay(1_600L)
                Thread({ System.gc() }, "parchment-idle-sweep-2").apply { isDaemon = true; start() }
            }
            ParchmentApp(
                shortcutDispatcher = shortcuts,
                platformWindowChrome = { title ->
                    SideEffect { if (captionTitle != title) captionTitle = title }
                    val p = LocalParchmentPalette.current
                    LaunchedEffect(window, p.canvasBg, p.primaryInk, p.border) {
                        val awtCanvas = AwtColor(p.canvasBg.toArgb(), false)
                        window.background = awtCanvas
                        window.contentPane?.background = awtCanvas
                        WindowsChrome.applyCaption(
                            window.windowHandle,
                            p.canvasBg.toArgb(),
                            p.primaryInk.toArgb(),
                            p.border.toArgb(),
                            p.canvasBg.luminance() < 0.5f
                        )
                    }
                }
            )
        }
    }
}
