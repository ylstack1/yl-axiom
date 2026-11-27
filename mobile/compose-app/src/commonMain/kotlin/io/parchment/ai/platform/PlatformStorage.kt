package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import io.parchment.ai.model.StoredAppFileItem
import java.io.File

/** A chat file's identity for the stub index: any write changes its size or modification time. */
data class ChatFileStamp(val length: Long, val lastModifiedMs: Long)

expect object PlatformStorage {
    // Local persistent storage (split into parchment-settings.json + per-chat files in chats/)
    fun getAppStorageDirectoryPath(): String
    fun getChatsStorageDirectoryPath(): String
    fun loadAppStateJson(): String?
    fun saveAppStateJson(json: String)
    fun loadChatSessionJsonById(sessionId: String): String?
    fun loadAllChatSessionJsons(): List<String>

    /**
     * The same chats one at a time, so start-up never holds every chat's text at once. Folders named in
     * [skipFolders] (see [chatFolderName]) are not read: their stubs came from the chat index.
     */
    fun chatSessionJsons(skipFolders: Set<String> = emptySet()): Sequence<String>

    /** Size and modification time of each chat folder's `chat.json`, by folder name; chats awaiting migration are left out. */
    fun chatFileStamps(): Map<String, ChatFileStamp>

    /** The folder under `chats/` that holds [sessionId]. */
    fun chatFolderName(sessionId: String): String

    /** `chat-index.json`: sidebar stubs cached by chat file stamp. A cache only; `chats/<id>/chat.json` is the truth. */
    fun loadChatIndexJson(): String?
    fun saveChatIndexJson(json: String)
    fun saveChatSessionJson(sessionId: String, json: String)

    /** A streaming reply's checkpoint beside its chat (`live.json`); the next full save of the chat replaces it. */
    fun saveChatLiveTurnJson(sessionId: String, json: String)
    fun deleteChatSessionFile(sessionId: String)

    /** Fast native full-text search (SQLite FTS5 on Android via `parchment-core`), or null to fall back. */
    fun searchStoredChatIds(query: String, limit: Int = 50): Set<String>?
    fun wipeAllLocalData(): Boolean
    fun pruneStaleTempFiles(maxAgeMs: Long = 24L * 60L * 60L * 1000L): Int
    fun listAppStorageFiles(): List<StoredAppFileItem>
    fun deleteAppStorageFile(fullPath: String): Boolean
    fun writeSessionTodoMarkdown(sessionId: String, markdown: String, storageRootOverride: String = ""): Result<String>

    // Native coding-task store (Rust `parchment-core` via `ParchmentNativeCore` on Android).
    fun listCodingTaskIds(storageDir: File): List<String>
    fun loadCodingTaskJson(storageDir: File, taskId: String): String?
    fun saveCodingTaskJson(storageDir: File, json: String): Boolean
    fun deleteCodingTask(storageDir: File, taskId: String): Boolean

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

    /** Drops decoded image bitmaps and transient token caches under memory pressure or backgrounding. */
    fun trimMemoryCaches()
    fun openOrPlayMedia(localFilePathOrUrl: String)
}
