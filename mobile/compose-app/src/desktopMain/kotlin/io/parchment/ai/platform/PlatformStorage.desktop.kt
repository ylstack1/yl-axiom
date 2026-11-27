package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import io.parchment.ai.model.StoredAppFileItem
import java.awt.Desktop
import java.io.File
import java.net.URI

private val DESKTOP_EXECUTABLE_EXTENSIONS = setOf(
    "exe", "com", "bat", "cmd", "msi", "msp", "scr", "pif", "cpl", "lnk", "url", "reg", "inf", "scf", "msc",
    "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "jar", "application", "appref-ms", "gadget"
)

/** Attachments can come from imported archives, so opening one must never launch it. */
internal fun isDesktopExecutableName(name: String): Boolean = name.trimEnd('.', ' ').substringAfterLast('.', "").lowercase() in DESKTOP_EXECUTABLE_EXTENSIONS

actual object PlatformStorage {
    private val imageCache = BoundedImageCache {
        if (PlatformRuntime.isLowMemoryDevice) 3 to 32L * 1024L * 1024L else 4 to 64L * 1024L * 1024L
    }

    private val snapshotStorage = SnapshotFileStorage(DesktopPlatformHost::resolveStorageDir)

    private val mediaStore by lazy {
        LocalMediaStore(DesktopPlatformHost::resolveStorageDir, ::chatSessionFolder, imageCache::remove)
    }

    actual fun getAppStorageDirectoryPath(): String = DesktopPlatformHost.resolveStorageDir().absolutePath

    actual fun isReadableLocalFile(filePath: String): Boolean = mediaStore.isReadable(filePath)

    @Volatile
    private var cachedChatsDirPair: Pair<File, File>? = null

    private fun chatsDir(): File {
        val root = DesktopPlatformHost.resolveStorageDir()
        cachedChatsDirPair?.let { (cachedRoot, cachedDir) ->
            if (cachedRoot == root && cachedDir.exists()) return cachedDir
        }
        return File(root, "chats").also {
            if (!it.exists()) it.mkdirs()
            cachedChatsDirPair = root to it
        }
    }

    actual fun getChatsStorageDirectoryPath(): String = chatsDir().absolutePath

    private fun chatSessionFolder(sessionId: String): File = snapshotStorage.chatDirectory(sessionId)

    actual fun loadAppStateJson(): String? = snapshotStorage.loadSettings()
    actual fun saveAppStateJson(json: String) = snapshotStorage.saveSettings(json)
    actual fun loadChatSessionJsonById(sessionId: String): String? = snapshotStorage.loadChat(sessionId)
    actual fun loadAllChatSessionJsons(): List<String> = snapshotStorage.loadAllChats()
    actual fun chatSessionJsons(skipFolders: Set<String>): Sequence<String> = snapshotStorage.chats(skipFolders)
    actual fun chatFileStamps(): Map<String, ChatFileStamp> = snapshotStorage.chatFileStamps()
    actual fun chatFolderName(sessionId: String): String = snapshotStorage.chatFolderName(sessionId)
    actual fun loadChatIndexJson(): String? = snapshotStorage.loadChatIndex()
    actual fun saveChatIndexJson(json: String) = snapshotStorage.saveChatIndex(json)
    actual fun saveChatSessionJson(sessionId: String, json: String) = snapshotStorage.saveChat(sessionId, json)
    actual fun saveChatLiveTurnJson(sessionId: String, json: String) = snapshotStorage.saveLiveTurn(sessionId, json)
    actual fun deleteChatSessionFile(sessionId: String) = snapshotStorage.deleteChat(sessionId)
    actual fun searchStoredChatIds(query: String, limit: Int): Set<String>? = null

    actual fun wipeAllLocalData(): Boolean = AppStorageInventory.wipeDirectory(DesktopPlatformHost.resolveStorageDir(), preserveRoot = false)

    actual fun pruneStaleTempFiles(maxAgeMs: Long): Int {
        val root = DesktopPlatformHost.resolveStorageDir()
        return AppStorageInventory.pruneStaleTempFiles(
            storageRoot = root,
            otaDirs = listOf(DesktopPlatformHost.otaTempDir(), File(root, "ota")),
            maxAgeMs = maxAgeMs
        )
    }

    actual fun listAppStorageFiles(): List<StoredAppFileItem> = AppStorageInventory.listFiles(DesktopPlatformHost.resolveStorageDir(), extraOtaDir = DesktopPlatformHost.otaTempDir())

    actual fun deleteAppStorageFile(fullPath: String): Boolean = AppStorageInventory.deleteFile(
        fullPath = fullPath,
        storageRoot = DesktopPlatformHost.resolveStorageDir(),
        extraAllowedDir = DesktopPlatformHost.otaTempDir(),
        beforeDelete = imageCache::remove
    )

    actual fun writeSessionTodoMarkdown(sessionId: String, markdown: String, storageRootOverride: String): Result<String> = AppAgentStorage.writeSessionTodo(DesktopPlatformHost::resolveStorageDir, sessionId, markdown, storageRootOverride)

    actual fun hasGlobalAgentMemory(storageRootOverride: String): Boolean = AppAgentStorage.hasGlobalMemory(DesktopPlatformHost::resolveStorageDir, storageRootOverride)

    actual fun readGlobalAgentMemory(maxLines: Int, storageRootOverride: String): Result<String> = AppAgentStorage.readGlobalMemory(DesktopPlatformHost::resolveStorageDir, maxLines, storageRootOverride)

    actual fun writeGlobalAgentMemory(content: String, storageRootOverride: String): Result<String> = AppAgentStorage.writeGlobalMemory(DesktopPlatformHost::resolveStorageDir, content, storageRootOverride)

    actual fun listCodingTaskIds(storageDir: File): List<String> = emptyList()
    actual fun loadCodingTaskJson(storageDir: File, taskId: String): String? = null
    actual fun saveCodingTaskJson(storageDir: File, json: String): Boolean = false
    actual fun deleteCodingTask(storageDir: File, taskId: String): Boolean = false

    actual fun runTurn(
        storageDir: File,
        taskId: String,
        subtaskId: String,
        content: String,
        toolCallsExecuted: Int,
    ): String = "{}"

    internal fun resolveLocalMediaFile(filePath: String): File? = mediaStore.resolve(filePath)

    actual fun saveGeneratedMediaFile(fileName: String, bytes: ByteArray, sessionId: String): String = mediaStore.save(fileName, bytes, sessionId)

    actual fun relocateLegacyMediaToChatFolder(filePath: String, sessionId: String): String = mediaStore.relocateToChatFolder(filePath, sessionId)

    actual fun localFileSizeBytes(filePath: String): Long = mediaStore.sizeOf(filePath)

    actual fun readBinaryFileBytes(filePath: String): ByteArray? = mediaStore.read(filePath)

    actual fun cachedImageBitmap(filePath: String): ImageBitmap? = imageCache.get(filePath)

    actual fun trimMemoryCaches() {
        imageCache.clear()
        io.parchment.ai.protocol.O200kTokenizer.clearCaches()
        io.parchment.ai.network.ModelsDevClient.trimMemoryCache()
    }

    actual fun loadImageBitmapFromFile(filePath: String): ImageBitmap? = imageCache.getOrLoad(filePath, ::resolveLocalMediaFile, DesktopImageDecoder::decode)

    actual fun openOrPlayMedia(localFilePathOrUrl: String) {
        runCatching {
            val f = resolveLocalMediaFile(localFilePathOrUrl) ?: File(localFilePathOrUrl)
            if (f.exists()) {
                if (f.isDirectory || !isDesktopExecutableName(f.name)) Desktop.getDesktop().open(f)
            } else if (localFilePathOrUrl.startsWith("http://") || localFilePathOrUrl.startsWith("https://")) {
                Desktop.getDesktop().browse(URI(localFilePathOrUrl))
            }
        }
    }
}
