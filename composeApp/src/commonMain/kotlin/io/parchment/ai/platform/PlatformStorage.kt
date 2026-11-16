package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import io.parchment.ai.model.StoredAppFileItem

expect object PlatformStorage {
    // Local persistent storage (split into parchment-settings.json + per-chat files in chats/)
    fun getAppStorageDirectoryPath(): String
    fun getChatsStorageDirectoryPath(): String
    fun loadAppStateJson(): String?
    fun saveAppStateJson(json: String)
    fun loadChatSessionJsonById(sessionId: String): String?
    fun loadAllChatSessionJsons(): List<String>
    /** The same chats one at a time, so start-up never holds every chat's text at once. */
    fun chatSessionJsons(): Sequence<String>
    fun saveChatSessionJson(sessionId: String, json: String)
    fun deleteChatSessionFile(sessionId: String)
    fun wipeAllLocalData(): Boolean
    fun pruneStaleTempFiles(maxAgeMs: Long = 24L * 60L * 60L * 1000L): Int
    fun listAppStorageFiles(): List<StoredAppFileItem>
    fun deleteAppStorageFile(fullPath: String): Boolean
    fun writeSessionTodoMarkdown(sessionId: String, markdown: String, storageRootOverride: String = ""): Result<String>
    fun hasGlobalAgentMemory(storageRootOverride: String = ""): Boolean
    fun readGlobalAgentMemory(maxLines: Int = 500, storageRootOverride: String = ""): Result<String>
    fun writeGlobalAgentMemory(content: String, storageRootOverride: String = ""): Result<String>

    // Generated Image & Video Media Storage, Per-Chat Media Bundling, and Native Playback
    fun saveGeneratedMediaFile(fileName: String, bytes: ByteArray, sessionId: String = ""): String
    fun relocateLegacyMediaToChatFolder(filePath: String, sessionId: String): String
    fun readBinaryFileBytes(filePath: String): ByteArray?
    /** Size of a resolvable local media file, or -1 when it does not exist. */
    fun localFileSizeBytes(filePath: String): Long
    fun isReadableLocalFile(filePath: String): Boolean
    fun loadImageBitmapFromFile(filePath: String): ImageBitmap?
    /** An already decoded image, without touching the disk. */
    fun cachedImageBitmap(filePath: String): ImageBitmap?
    fun openOrPlayMedia(localFilePathOrUrl: String)
}
