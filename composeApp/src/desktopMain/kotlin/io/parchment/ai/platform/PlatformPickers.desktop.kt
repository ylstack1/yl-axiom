package io.parchment.ai.platform

import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.platform.Font
import io.parchment.ai.ui.appProcessScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.awt.FileDialog
import java.awt.Frame
import java.awt.Toolkit
import java.awt.datatransfer.DataFlavor
import java.awt.datatransfer.Transferable
import java.awt.image.BufferedImage
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FilenameFilter
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.ConcurrentHashMap
import javax.imageio.ImageIO
import javax.swing.ImageIcon
import javax.swing.JFileChooser

private val RAW_IMAGE_MIMES = setOf("image/gif", "image/webp", "image/png", "image/jpeg")

actual object PlatformPickers {
    private val fontCache = ConcurrentHashMap<String, FontFamily>()
    private val preloadedFontBytes = ConcurrentHashMap<String, ByteArray>()

    @Volatile
    var dragHoverCallback: ((Boolean) -> Unit)? = null

    @Volatile
    var filesDroppedCallback: ((List<PickedBinaryFile>) -> Unit)? = null

    private fun activeWindowHwnd(): Long = (
        java.awt.KeyboardFocusManager.getCurrentKeyboardFocusManager().activeWindow
            as? androidx.compose.ui.awt.ComposeWindow
        )?.windowHandle
        ?: java.awt.Window.getWindows().firstOrNull { it.isVisible && it is androidx.compose.ui.awt.ComposeWindow }
            ?.let { (it as androidx.compose.ui.awt.ComposeWindow).windowHandle }
        ?: 0L

    actual suspend fun pickFileForImport(allowedExtensions: List<String>): PickedBinaryFile? = withContext(Dispatchers.IO) {
        runCatching {
            chooseImportFiles(allowedExtensions, allowMultiple = false).firstOrNull { it.isFile }?.let {
                pickedFileFromDisk(it, maxPickedFileBytes(allowedExtensions), PlatformRuntime.currentTimeMillis())
            }
        }.getOrNull()
    }

    actual suspend fun pickFilesForImport(allowedExtensions: List<String>): List<PickedBinaryFile> = withContext(Dispatchers.IO) {
        runCatching {
            val maxBytes = maxPickedFileBytes(allowedExtensions)
            val now = PlatformRuntime.currentTimeMillis()
            chooseImportFiles(allowedExtensions, allowMultiple = true)
                .filter { it.isFile }
                .mapIndexed { idx, file -> pickedFileFromDisk(file, maxBytes, now + idx) }
        }.getOrDefault(emptyList())
    }

    private fun chooseImportFiles(allowedExtensions: List<String>, allowMultiple: Boolean = true): List<File> {
        val allowAll = allowedExtensions.isEmpty() || allowedExtensions.contains("*")
        val title = when {
            allowAll && allowMultiple -> "Attach Files to Parchment"
            allowAll -> "Attach File to Parchment"
            allowMultiple -> "Import Files into Parchment"
            else -> "Import File into Parchment"
        }
        if (WindowsNative.isWindows) {
            WindowsFolderDialog.pickFiles(
                title = title,
                filters = WindowsFolderDialog.buildOpenFilters(allowedExtensions),
                allowMultiple = allowMultiple,
                ownerHwnd = activeWindowHwnd()
            ).onSuccess { paths -> return paths.map(::File) }
        }
        val dialog = FileDialog(null as Frame?, title, FileDialog.LOAD)
        dialog.isMultipleMode = allowMultiple
        if (!allowAll && allowedExtensions.isNotEmpty()) {
            dialog.file = allowedExtensions.joinToString(";") { "*.$it" }
            dialog.filenameFilter = FilenameFilter { _, name ->
                allowedExtensions.any { ext -> name.endsWith(".$ext", ignoreCase = true) }
            }
        }
        dialog.isVisible = true
        return dialog.files?.toList().orEmpty().ifEmpty {
            val dir = dialog.directory
            val file = dialog.file
            if (dir != null && file != null) listOf(File(dir, file)) else emptyList()
        }
    }

    actual suspend fun <T> pickAndImportChatFiles(decode: (chatJsons: List<String>) -> T): List<ChatImportOutcome<T>> = withContext(Dispatchers.IO) {
        chooseImportFiles(listOf("zip", "json"), allowMultiple = true).filter { it.isFile }.map { file ->
            ChatImportOutcome(
                file.name,
                runCatching {
                    ChatArchiveExtractor.importChatFile(
                        file.name,
                        FileInputStream(file),
                        DesktopPlatformHost.resolveStorageDir(),
                        decode = decode
                    )
                }
            )
        }
    }

    /** Refuses an oversized file from its length alone, so none of it is read. */
    internal fun pickedFileFromDisk(target: File, maxBytes: Long, fallbackTimestamp: Long): PickedBinaryFile = if (target.length() > maxBytes) {
        PickedBinaryFile(target.name, ByteArray(0), oversized = true)
    } else {
        normalizePickedBinaryFile(rawFileName = target.name, bytes = target.readBytes(), fallbackTimestamp = fallbackTimestamp)
    }

    actual suspend fun saveFileForExport(defaultFileName: String, bytes: ByteArray): String? = runCatching { exportStreamed(defaultFileName) { it.write(bytes) } }.getOrNull()

    actual suspend fun exportChatBundle(defaultFileName: String, chats: Sequence<ChatBundleEntry>): String? = exportStreamed(defaultFileName) { ChatArchiveWriter.writeBundle(it, chats, PlatformStorage::resolveLocalMediaFile) }

    actual suspend fun exportLocalFile(defaultFileName: String, filePath: String): String? {
        val source = PlatformStorage.resolveLocalMediaFile(filePath) ?: return null
        return exportStreamed(defaultFileName) { output -> FileInputStream(source).use { it.copyTo(output, 64 * 1024) } }
    }

    private suspend fun exportStreamed(defaultFileName: String, write: (OutputStream) -> Unit): String? = withContext(Dispatchers.IO) {
        val chosenTarget = if (WindowsNative.isWindows) {
            WindowsFolderDialog.saveFile(
                title = "Export from Parchment",
                defaultFileName = defaultFileName,
                ownerHwnd = activeWindowHwnd()
            ).getOrElse {
                chooseSaveFallback(defaultFileName)
            }?.let(::File)
        } else {
            chooseSaveFallback(defaultFileName)?.let(::File)
        } ?: return@withContext null
        chosenTarget.also { AtomicFileStorage.writeStream(it, write = write) }.absolutePath
    }

    private fun chooseSaveFallback(defaultFileName: String): String? {
        val dialog = FileDialog(null as Frame?, "Export from Parchment", FileDialog.SAVE)
        dialog.file = defaultFileName
        dialog.isVisible = true
        val dir = dialog.directory
        val file = dialog.file
        return if (dir != null && file != null) File(dir, file).absolutePath else null
    }

    actual suspend fun pickWorkspaceDirectory(currentPath: String): String? = pickDirectory("Select Local Workspace Folder for Agent Harness", currentPath)

    actual suspend fun pickDirectory(dialogTitle: String, currentPath: String): String? = withContext(Dispatchers.IO) {
        // The native Windows folder picker first (Quick access, OneDrive, search); Swing only if COM fails.
        if (WindowsNative.isWindows) {
            WindowsFolderDialog.pickFolder(dialogTitle, currentPath, activeWindowHwnd())
                .onSuccess { return@withContext it }
        }
        runCatching {
            val chooser = JFileChooser()
            chooser.dialogTitle = dialogTitle
            chooser.fileSelectionMode = JFileChooser.DIRECTORIES_ONLY
            if (currentPath.isNotBlank()) {
                val cur = File(currentPath)
                if (cur.exists()) chooser.currentDirectory = cur
            }
            val res = chooser.showOpenDialog(null)
            if (res == JFileChooser.APPROVE_OPTION && chooser.selectedFile != null) {
                chooser.selectedFile.absolutePath
            } else null
        }.getOrNull()
    }

    actual suspend fun pickLocalCommandFile(dialogTitle: String): String? = withContext(Dispatchers.IO) {
        if (WindowsNative.isWindows) {
            WindowsFolderDialog.pickFiles(
                title = dialogTitle,
                filters = listOf(
                    FileDialogFilter("Executables & Scripts (*.exe; *.cmd; *.bat; *.py; *.js; *.mjs; *.cjs; *.ps1)", "*.exe;*.cmd;*.bat;*.py;*.js;*.mjs;*.cjs;*.ps1"),
                    FileDialogFilter("All Files (*.*)", "*.*")
                ),
                allowMultiple = false,
                ownerHwnd = activeWindowHwnd()
            ).onSuccess { paths ->
                return@withContext paths.firstOrNull()?.let(::File)?.takeIf { it.isFile }?.absolutePath
            }
        }
        runCatching {
            val dialog = FileDialog(null as Frame?, dialogTitle, FileDialog.LOAD)
            dialog.isVisible = true
            val dir = dialog.directory
            val file = dialog.file
            if (dir != null && file != null) {
                File(dir, file).takeIf { it.isFile }?.absolutePath
            } else null
        }.getOrNull()
    }

    actual suspend fun pickFontFile(dialogTitle: String): String? = withContext(Dispatchers.IO) {
        if (WindowsNative.isWindows) {
            WindowsFolderDialog.pickFiles(
                title = dialogTitle,
                filters = listOf(
                    FileDialogFilter("Font Files (*.ttf; *.otf; *.ttc)", "*.ttf;*.otf;*.ttc"),
                    FileDialogFilter("All Files (*.*)", "*.*")
                ),
                allowMultiple = false,
                ownerHwnd = activeWindowHwnd()
            ).onSuccess { paths ->
                return@withContext paths.firstOrNull()?.let(::File)?.takeIf { it.isFile }?.absolutePath
            }
        }
        runCatching {
            val dialog = FileDialog(null as Frame?, dialogTitle, FileDialog.LOAD)
            dialog.file = "*.ttf;*.otf;*.ttc"
            dialog.filenameFilter = FilenameFilter { _, name ->
                name.endsWith(".ttf", ignoreCase = true) ||
                    name.endsWith(".otf", ignoreCase = true) ||
                    name.endsWith(".ttc", ignoreCase = true)
            }
            dialog.isVisible = true
            val dir = dialog.directory
            val file = dialog.file
            if (dir != null && file != null) {
                val selected = File(dir, file)
                if (selected.exists() && selected.isFile) {
                    selected.absolutePath
                } else null
            } else null
        }.getOrNull()
    }

    actual fun loadCustomFontFamily(fontPath: String): FontFamily? = AppStorageInventory.loadCachedFontFamily(fontCache, fontPath) { file ->
        FontFamily(
            Font(file = file, weight = FontWeight.Normal, style = FontStyle.Normal),
            Font(file = file, weight = FontWeight.Medium, style = FontStyle.Normal),
            Font(file = file, weight = FontWeight.SemiBold, style = FontStyle.Normal),
            Font(file = file, weight = FontWeight.Bold, style = FontStyle.Normal),
            Font(file = file, weight = FontWeight.Normal, style = FontStyle.Italic),
            Font(file = file, weight = FontWeight.Bold, style = FontStyle.Italic)
        )
    }

    actual fun copyImageToClipboard(bytes: ByteArray): Boolean = runCatching {
        val image = ImageIO.read(ByteArrayInputStream(bytes)) ?: return false
        val transferable = object : Transferable {
            override fun getTransferDataFlavors(): Array<DataFlavor> = arrayOf(DataFlavor.imageFlavor)

            override fun isDataFlavorSupported(flavor: DataFlavor): Boolean = flavor == DataFlavor.imageFlavor

            override fun getTransferData(flavor: DataFlavor): Any = image
        }
        Toolkit.getDefaultToolkit().systemClipboard.setContents(transferable, null)
        true
    }.getOrDefault(false)

    internal fun preloadStartupFonts() {
        if (!preloadedFontBytes.containsKey("fonts/Inter-Regular.ttf")) {
            readBundledResourceBytes("fonts/Inter-Regular.ttf")?.let { preloadedFontBytes.putIfAbsent("fonts/Inter-Regular.ttf", it) }
        }
        loadBundledFontFamily("Inter")
    }

    private fun bundledResourceExists(resPath: String): Boolean = preloadedFontBytes.containsKey(resPath) ||
        Thread.currentThread().contextClassLoader?.getResource(resPath) != null ||
        PlatformPickers::class.java.classLoader?.getResource(resPath) != null

    private fun readBundledResourceBytes(resPath: String): ByteArray? = preloadedFontBytes.remove(resPath)
        ?: Thread.currentThread().contextClassLoader?.getResourceAsStream(resPath)?.use { it.readBytes() }
        ?: PlatformPickers::class.java.classLoader?.getResourceAsStream(resPath)?.use { it.readBytes() }

    private fun requireBundledResourceBytes(primaryPath: String, fallbackPath: String = primaryPath): ByteArray = readBundledResourceBytes(primaryPath)
        ?: (if (fallbackPath != primaryPath) readBundledResourceBytes(fallbackPath) else null)
        ?: error("Missing bundled font resource: $primaryPath")

    actual fun loadBundledFontFamily(bundleName: String): FontFamily? {
        val canonical = AppStorageInventory.canonicalBundledFontKey(bundleName)
        if (canonical.isEmpty()) return null
        val key = "bundled:$canonical"
        fontCache[key]?.let { return it }

        return runCatching {
            val family = when (canonical) {
                "inter" -> {
                    if (!bundledResourceExists("fonts/Inter-Regular.ttf")) return null
                    FontFamily(
                        Font(
                            identity = "Inter-Regular",
                            getData = { requireBundledResourceBytes("fonts/Inter-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "Inter-Italic",
                            getData = { requireBundledResourceBytes("fonts/Inter-Italic.ttf", "fonts/Inter-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Italic
                        )
                    )
                }
                "jetbrains_mono" -> {
                    if (!bundledResourceExists("fonts/JetBrainsMono-Regular.ttf")) return null
                    FontFamily(
                        Font(
                            identity = "JBM-Regular",
                            getData = { requireBundledResourceBytes("fonts/JetBrainsMono-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "JBM-SemiBold",
                            getData = { requireBundledResourceBytes("fonts/JetBrainsMono-SemiBold.ttf", "fonts/JetBrainsMono-Regular.ttf") },
                            weight = FontWeight.SemiBold,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "JBM-Bold",
                            getData = { requireBundledResourceBytes("fonts/JetBrainsMono-Bold.ttf", "fonts/JetBrainsMono-Regular.ttf") },
                            weight = FontWeight.Bold,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "JBM-Italic",
                            getData = { requireBundledResourceBytes("fonts/JetBrainsMono-Italic.ttf", "fonts/JetBrainsMono-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Italic
                        )
                    )
                }
                "latex" -> {
                    if (!bundledResourceExists("fonts/KaTeX_Main-Regular.ttf")) return null
                    FontFamily(
                        Font(
                            identity = "KaTeX-Regular",
                            getData = { requireBundledResourceBytes("fonts/KaTeX_Main-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "KaTeX-Bold",
                            getData = { requireBundledResourceBytes("fonts/KaTeX_Main-Bold.ttf", "fonts/KaTeX_Main-Regular.ttf") },
                            weight = FontWeight.Bold,
                            style = FontStyle.Normal
                        ),
                        Font(
                            identity = "KaTeX-MathItalic",
                            getData = { requireBundledResourceBytes("fonts/KaTeX_Math-Italic.ttf", "fonts/KaTeX_Main-Regular.ttf") },
                            weight = FontWeight.Normal,
                            style = FontStyle.Italic
                        ),
                        Font(
                            identity = "NotoSansMath-Regular",
                            getData = { requireBundledResourceBytes("fonts/NotoSansMath-Regular.ttf", "fonts/KaTeX_Main-Regular.ttf") },
                            weight = FontWeight.Medium,
                            style = FontStyle.Normal
                        )
                    )
                }
                else -> return null
            }
            fontCache[key] = family
            family
        }.getOrNull()
    }

    fun extractTransferableFiles(contents: Transferable): List<PickedBinaryFile> = runCatching {
        // 1. Check for copied/dropped files from Explorer (images, GIFs, videos, PDFs, audio, code, etc.)
        if (contents.isDataFlavorSupported(DataFlavor.javaFileListFlavor)) {
            val fileList = contents.getTransferData(DataFlavor.javaFileListFlavor) as? List<*>
            val picked = fileList?.mapNotNull { item ->
                val f = item as? File
                if (f != null && f.exists() && f.isFile) {
                    runCatching { pickedFileFromDisk(f, MAX_INLINE_ATTACHMENT_BYTES, PlatformRuntime.currentTimeMillis()) }.getOrNull()
                } else null
            }.orEmpty()
            if (picked.isNotEmpty()) return@runCatching picked
        }

        // 2. Check for raw binary image InputStream flavors (preserves animated GIFs & WebP from browsers)
        val rawImageFlavor = contents.transferDataFlavors.firstOrNull { flavor ->
            val mime = "${flavor.primaryType}/${flavor.subType}".lowercase()
            mime in RAW_IMAGE_MIMES && InputStream::class.java.isAssignableFrom(flavor.representationClass)
        }
        if (rawImageFlavor != null) {
            val mime = "${rawImageFlavor.primaryType}/${rawImageFlavor.subType}".lowercase()
            val streamBytes = runCatching {
                (contents.getTransferData(rawImageFlavor) as? InputStream)?.use { readAtMost(it, MAX_INLINE_ATTACHMENT_BYTES) }
            }.getOrNull()
            if (streamBytes != null && streamBytes.isNotEmpty()) {
                return@runCatching listOf(
                    normalizePickedBinaryFile(
                        rawFileName = "",
                        bytes = streamBytes,
                        explicitMimeType = mime,
                        fallbackTimestamp = PlatformRuntime.currentTimeMillis()
                    )
                )
            }
        }

        // 3. Check for raw clipboard/dragged AWT Image (Snipping Tool Win+Shift+S, PrintScreen, Browser Copy Image)
        if (contents.isDataFlavorSupported(DataFlavor.imageFlavor)) {
            val rawAwtImage = contents.getTransferData(DataFlavor.imageFlavor) as? java.awt.Image
            if (rawAwtImage != null) {
                val buffered = if (rawAwtImage is BufferedImage) {
                    rawAwtImage
                } else {
                    // Use ImageIcon's MediaTracker so asynchronous ToolkitImage dimensions (>0) finish loading
                    val loaded = ImageIcon(rawAwtImage).image
                    val w = loaded.getWidth(null).coerceAtLeast(1)
                    val h = loaded.getHeight(null).coerceAtLeast(1)
                    val b = BufferedImage(w, h, BufferedImage.TYPE_INT_ARGB)
                    val g = b.createGraphics()
                    g.drawImage(loaded, 0, 0, null)
                    g.dispose()
                    b
                }
                val baos = ByteArrayOutputStream()
                ImageIO.write(buffered, "png", baos)
                val bytes = baos.toByteArray()
                if (bytes.isNotEmpty()) {
                    return@runCatching listOf(
                        normalizePickedBinaryFile(
                            rawFileName = "clipboard-${PlatformRuntime.currentTimeMillis()}.png",
                            bytes = bytes,
                            explicitMimeType = "image/png",
                            fallbackTimestamp = PlatformRuntime.currentTimeMillis()
                        )
                    )
                }
            }
        }

        emptyList()
    }.getOrDefault(emptyList())

    actual fun readClipboardFiles(): List<PickedBinaryFile> = runCatching {
        val clipboard = Toolkit.getDefaultToolkit().systemClipboard
        val contents = clipboard.getContents(null) ?: return@runCatching emptyList()
        extractTransferableFiles(contents)
    }.getOrDefault(emptyList())

    actual fun pasteClipboardMediaAsync(onFiles: (List<PickedBinaryFile>) -> Unit): Boolean {
        // Listing formats does not copy the clipboard data; getContents() reads every format up front.
        val flavors = runCatching { Toolkit.getDefaultToolkit().systemClipboard.availableDataFlavors }.getOrNull()
        if (flavors == null || flavors.none(::isMediaFlavor)) return false
        appProcessScope.launch {
            val files = withContext(Dispatchers.IO) { readClipboardFiles() }
            if (files.isNotEmpty()) onFiles(files)
        }
        return true
    }

    private fun isMediaFlavor(flavor: DataFlavor): Boolean {
        if (flavor == DataFlavor.javaFileListFlavor || flavor == DataFlavor.imageFlavor) return true
        val mime = "${flavor.primaryType}/${flavor.subType}".lowercase()
        return mime in RAW_IMAGE_MIMES && InputStream::class.java.isAssignableFrom(flavor.representationClass)
    }

    actual fun registerFileDropCallbacks(
        onDragHover: (Boolean) -> Unit,
        onFilesDropped: (List<PickedBinaryFile>) -> Unit
    ) {
        dragHoverCallback = onDragHover
        filesDroppedCallback = onFilesDropped
    }
}
