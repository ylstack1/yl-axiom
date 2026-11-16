package io.parchment.ai.platform

import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import io.parchment.ai.ParchmentFileProvider
import io.parchment.ai.model.StoredAppFileItem
import java.io.File

actual object PlatformStorage {
    private val imageCache = BoundedImageCache {
        if (PlatformRuntime.isLowMemoryDevice) 3 to 18L * 1024L * 1024L else 6 to 40L * 1024L * 1024L
    }

    private val snapshotStorage = SnapshotFileStorage(AndroidPlatformHost::resolveStorageDir) {
        AndroidPlatformHost.appContext?.noBackupFilesDir?.let { listOf(File(it, "parchment-state.json")) }.orEmpty()
    }

    private val mediaStore by lazy {
        LocalMediaStore(AndroidPlatformHost::resolveStorageDir, ::chatSessionFolder, imageCache::remove)
    }

    actual fun getAppStorageDirectoryPath(): String = AndroidPlatformHost.resolveStorageDir().absolutePath

    actual fun isReadableLocalFile(filePath: String): Boolean = mediaStore.isReadable(filePath)

    @Volatile
    private var cachedChatsDirPair: Pair<File, File>? = null

    private fun chatsDir(): File {
        val root = AndroidPlatformHost.resolveStorageDir()
        cachedChatsDirPair?.let { (cachedRoot, cachedDir) ->
            if (cachedRoot == root && cachedDir.exists()) return cachedDir
        }
        return File(root, "chats").also {
            if (!it.exists()) it.mkdirs()
            cachedChatsDirPair = root to it
        }
    }

    actual fun getChatsStorageDirectoryPath(): String = chatsDir().absolutePath

    private fun chatSessionFolder(sessionId: String): File =
        snapshotStorage.chatDirectory(sessionId)

    actual fun loadAppStateJson(): String? = snapshotStorage.loadSettings()
    actual fun saveAppStateJson(json: String) = snapshotStorage.saveSettings(json)
    actual fun loadChatSessionJsonById(sessionId: String): String? = snapshotStorage.loadChat(sessionId)
    actual fun loadAllChatSessionJsons(): List<String> = snapshotStorage.loadAllChats()
    actual fun chatSessionJsons(): Sequence<String> = snapshotStorage.chats()
    actual fun saveChatSessionJson(sessionId: String, json: String) = snapshotStorage.saveChat(sessionId, json)
    actual fun deleteChatSessionFile(sessionId: String) = snapshotStorage.deleteChat(sessionId)

    actual fun wipeAllLocalData(): Boolean =
        AppStorageInventory.wipeDirectory(AndroidPlatformHost.resolveStorageDir(), preserveRoot = true)

    actual fun pruneStaleTempFiles(maxAgeMs: Long): Int {
        val root = AndroidPlatformHost.resolveStorageDir()
        val otaDirs = buildList {
            AndroidPlatformHost.appContext?.cacheDir?.let { add(File(it, "ota")) }
            add(File(root, "ota"))
        }
        return AppStorageInventory.pruneStaleTempFiles(
            storageRoot = root,
            otaDirs = otaDirs,
            maxAgeMs = maxAgeMs
        )
    }

    actual fun listAppStorageFiles(): List<StoredAppFileItem> =
        AppStorageInventory.listFiles(AndroidPlatformHost.resolveStorageDir(), otaTempExtensions = setOf(".tmp", ".apk"))

    actual fun deleteAppStorageFile(fullPath: String): Boolean =
        AppStorageInventory.deleteFile(fullPath, AndroidPlatformHost.resolveStorageDir()) { imageCache.remove(it) }

    actual fun writeSessionTodoMarkdown(sessionId: String, markdown: String, storageRootOverride: String): Result<String> =
        AppAgentStorage.writeSessionTodo(AndroidPlatformHost::resolveStorageDir, sessionId, markdown, storageRootOverride)

    actual fun hasGlobalAgentMemory(storageRootOverride: String): Boolean =
        AppAgentStorage.hasGlobalMemory(AndroidPlatformHost::resolveStorageDir, storageRootOverride)

    actual fun readGlobalAgentMemory(maxLines: Int, storageRootOverride: String): Result<String> =
        AppAgentStorage.readGlobalMemory(AndroidPlatformHost::resolveStorageDir, maxLines, storageRootOverride)

    actual fun writeGlobalAgentMemory(content: String, storageRootOverride: String): Result<String> =
        AppAgentStorage.writeGlobalMemory(AndroidPlatformHost::resolveStorageDir, content, storageRootOverride)

    internal fun resolveLocalMediaFile(filePath: String): File? = mediaStore.resolve(filePath)

    actual fun saveGeneratedMediaFile(fileName: String, bytes: ByteArray, sessionId: String): String =
        mediaStore.save(fileName, bytes, sessionId)

    actual fun relocateLegacyMediaToChatFolder(filePath: String, sessionId: String): String =
        mediaStore.relocateToChatFolder(filePath, sessionId)

    actual fun localFileSizeBytes(filePath: String): Long = mediaStore.sizeOf(filePath)

    actual fun readBinaryFileBytes(filePath: String): ByteArray? = mediaStore.read(filePath)

    actual fun cachedImageBitmap(filePath: String): ImageBitmap? = imageCache.get(filePath)

    actual fun loadImageBitmapFromFile(filePath: String): ImageBitmap? =
        imageCache.getOrLoad(filePath, ::resolveLocalMediaFile, ::decodeSampledBitmap)

    private const val MAX_ENCODED_IMAGE_BYTES = 128L * 1024L * 1024L

    private fun decodeSampledBitmap(file: File): ImageBitmap? {
        if (file.length() !in 1..MAX_ENCODED_IMAGE_BYTES) return null
        // Decode bounds first and downsample large 2K/4K images for UI display to minimize RAM.
        val boundsOpts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.absolutePath, boundsOpts)
        if (boundsOpts.outWidth <= 0 || boundsOpts.outHeight <= 0) return null
        val maxDim = 1600
        var sampleSize = 1
        while ((boundsOpts.outWidth / sampleSize) > maxDim || (boundsOpts.outHeight / sampleSize) > maxDim) {
            sampleSize *= 2
        }
        val decodeOpts = BitmapFactory.Options().apply { inSampleSize = sampleSize }
        return BitmapFactory.decodeFile(file.absolutePath, decodeOpts)?.asImageBitmap()
    }

    actual fun openOrPlayMedia(localFilePathOrUrl: String) {
        val ctx = AndroidPlatformHost.appContext ?: return
        val act = AndroidPlatformHost.mainActivityRef?.get()
        runCatching {
            val target = localFilePathOrUrl.trim()
            if (target.isEmpty()) return@runCatching
            if (target.startsWith("http://") || target.startsWith("https://")) {
                val browserIntent = Intent(Intent.ACTION_VIEW, Uri.parse(target)).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                (act ?: ctx).startActivity(browserIntent)
            } else {
                val f = resolveLocalMediaFile(target) ?: File(target)
                if (!f.exists() || !f.isFile) return@runCatching
                val contentUri = ParchmentFileProvider.getUriForFile(ctx, f)
                val mimeType = ParchmentFileProvider.resolveMimeType(f)
                val viewIntent = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(contentUri, mimeType)
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                (act ?: ctx).startActivity(viewIntent)
            }
        }
    }
}
