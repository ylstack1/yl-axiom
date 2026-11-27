package io.parchment.ai.platform

import java.io.File

/**
 * Direct zero-JNA JNI bridge into Rust `parchment-core` (`libparchment_core.so`).
 *
 * Keeps Android APK lean (no JNA runtime or reflection rules) while giving Android:
 * - SQLite WAL + FTS5 persistent storage and instant cross-chat search
 * - Native non-destructive one-shot v2 migration on first open
 * - Native bounded workspace text search (`fs_search`)
 * - Native bounded `.docx` text extraction
 */
internal object ParchmentNativeCore {
    private val libraryLoaded: Boolean = runCatching {
        System.loadLibrary("parchment_core")
        true
    }.getOrDefault(false)

    @Volatile
    private var openPair: Pair<String, Long>? = null

    private fun handleFor(storageDir: File): Long {
        if (!libraryLoaded) return 0L
        val path = storageDir.absolutePath
        openPair?.let { (cachedPath, cachedHandle) ->
            if (cachedPath == path && cachedHandle != 0L) return cachedHandle
        }
        synchronized(this) {
            openPair?.let { (cachedPath, cachedHandle) ->
                if (cachedPath == path && cachedHandle != 0L) return cachedHandle
            }
            val handle = runCatching { nativeOpen(path) }.getOrDefault(0L)
            if (handle != 0L) {
                openPair = path to handle
            }
            return handle
        }
    }

    fun loadSettingsJson(storageDir: File): String? {
        val h = handleFor(storageDir)
        if (h == 0L) return null
        return runCatching { nativeLoadSettingsJson(h) }.getOrNull()
    }

    fun saveSettingsJson(storageDir: File, json: String): Boolean {
        val h = handleFor(storageDir)
        if (h == 0L) return false
        return runCatching { nativeSaveSettingsJson(h, json) }.getOrDefault(false)
    }

    fun loadChatJson(storageDir: File, chatId: String): String? {
        val h = handleFor(storageDir)
        if (h == 0L) return null
        return runCatching { nativeLoadChatJson(h, chatId) }.getOrNull()
    }

    fun saveChatJson(storageDir: File, json: String): Boolean {
        val h = handleFor(storageDir)
        if (h == 0L) return false
        return runCatching { nativeSaveChatJson(h, json) }.getOrDefault(false)
    }

    fun deleteChat(storageDir: File, chatId: String): Boolean {
        val h = handleFor(storageDir)
        if (h == 0L) return false
        return runCatching { nativeDeleteChat(h, chatId) }.getOrDefault(false)
    }

    fun searchChatIds(storageDir: File, query: String, limit: Int): Set<String>? {
        val h = handleFor(storageDir)
        if (h == 0L) return null
        val raw = runCatching { nativeSearchChatIds(h, query, limit) }.getOrNull() ?: return null
        if (raw.isBlank()) return emptySet()
        return raw.lineSequence().map { it.trim() }.filter { it.isNotEmpty() }.toSet()
    }

    fun workspaceSearch(
        workspaceRoot: String,
        query: String,
        scopePath: String,
        fileGlob: String,
        maxResults: Int
    ): String? {
        if (!libraryLoaded) return null
        return runCatching {
            nativeWorkspaceSearch(workspaceRoot, query, scopePath, fileGlob, maxResults)
        }.getOrNull()
    }

    fun extractDocx(bytes: ByteArray): String? {
        if (!libraryLoaded) return null
        return runCatching { nativeExtractDocx(bytes) }.getOrNull()
    }

    /** Newest-first coding task ids, or null when the native core is unavailable. */
    fun listCodingTaskIds(storageDir: File): List<String> {
        val h = handleFor(storageDir)
        if (h == 0L) return emptyList()
        val raw = runCatching { nativeListCodingTaskIds(h) }.getOrNull() ?: return emptyList()
        return raw.lineSequence().map { it.trim() }.filter { it.isNotEmpty() }.toList()
    }

    fun loadCodingTaskJson(storageDir: File, taskId: String): String? {
        val h = handleFor(storageDir)
        if (h == 0L) return null
        return runCatching { nativeLoadCodingTaskJson(h, taskId) }.getOrNull()
    }

    fun saveCodingTaskJson(storageDir: File, json: String): Boolean {
        val h = handleFor(storageDir)
        if (h == 0L) return false
        return runCatching { nativeSaveCodingTaskJson(h, json) }.getOrDefault(false)
    }

    fun deleteCodingTask(storageDir: File, taskId: String): Boolean {
        val h = handleFor(storageDir)
        if (h == 0L) return false
        return runCatching { nativeDeleteCodingTask(h, taskId) }.getOrDefault(false)
    }

    @JvmStatic private external fun nativeOpen(storageDir: String): Long

    @JvmStatic private external fun nativeLoadSettingsJson(handle: Long): String?

    @JvmStatic private external fun nativeSaveSettingsJson(handle: Long, json: String): Boolean

    @JvmStatic private external fun nativeLoadChatJson(handle: Long, chatId: String): String?

    @JvmStatic private external fun nativeSaveChatJson(handle: Long, json: String): Boolean

    @JvmStatic private external fun nativeDeleteChat(handle: Long, chatId: String): Boolean

    @JvmStatic private external fun nativeSearchChatIds(handle: Long, query: String, limit: Int): String?

    @JvmStatic private external fun nativeWorkspaceSearch(
        rootPath: String,
        query: String,
        scopePath: String,
        fileGlob: String,
        maxResults: Int
    ): String?

    @JvmStatic private external fun nativeExtractDocx(bytes: ByteArray): String?

    @JvmStatic private external fun nativeListCodingTaskIds(handle: Long): String?

    @JvmStatic private external fun nativeLoadCodingTaskJson(handle: Long, taskId: String): String?

    @JvmStatic private external fun nativeSaveCodingTaskJson(handle: Long, json: String): Boolean

    @JvmStatic private external fun nativeDeleteCodingTask(handle: Long, taskId: String): Boolean
}
