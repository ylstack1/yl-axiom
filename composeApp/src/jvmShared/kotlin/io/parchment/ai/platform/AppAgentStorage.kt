package io.parchment.ai.platform

import java.io.File

/**
 * Keeps Parchment's own session TODOs (`chats/<sessionId>/TODO.md`) and global agent memory
 * (`memory/AGENT.md`) inside application storage so user project workspaces stay untouched.
 */
internal object AppAgentStorage {
    private const val GLOBAL_MEMORY_RELATIVE = "memory/AGENT.md"
    private const val MAX_MEMORY_READ_CHARS = 250_000
    private val unsafeSessionCharacters = Regex("[^a-zA-Z0-9._-]")

    private fun resolveStorageRoot(defaultRoot: () -> File, overridePath: String): File =
        if (overridePath.isNotBlank()) File(overridePath) else defaultRoot()

    private fun sessionTodoFile(storageRoot: File, sessionId: String): File {
        val chatsRoot = File(storageRoot, "chats").canonicalFile
        val safeId = sessionId.trim().replace(unsafeSessionCharacters, "_")
        require(safeId.isNotEmpty() && safeId != "." && safeId != "..") { "Invalid chat identifier." }
        val canonicalFolder = File(chatsRoot, safeId).canonicalFile
        check(canonicalFolder.toPath().startsWith(chatsRoot.toPath())) {
            "Chat directory must remain inside application storage."
        }
        return File(canonicalFolder, "TODO.md")
    }

    private fun globalMemoryFile(storageRoot: File): File {
        val root = storageRoot.canonicalFile
        val target = File(File(root, "memory"), "AGENT.md").canonicalFile
        check(target.toPath().startsWith(root.toPath())) {
            "Global memory file must remain inside application storage."
        }
        return target
    }

    fun writeSessionTodo(defaultRoot: () -> File, sessionId: String, markdown: String, storageRootOverride: String = ""): Result<String> = runCatching {
        val root = resolveStorageRoot(defaultRoot, storageRootOverride)
        val target = sessionTodoFile(root, sessionId)
        AtomicFileStorage.writeText(target, markdown)
        "chats/${target.parentFile.name}/TODO.md"
    }

    fun hasGlobalMemory(defaultRoot: () -> File, storageRootOverride: String = ""): Boolean = runCatching {
        val target = globalMemoryFile(resolveStorageRoot(defaultRoot, storageRootOverride))
        target.isFile && target.length() > 0L
    }.getOrDefault(false)

    fun readGlobalMemory(defaultRoot: () -> File, maxLines: Int, storageRootOverride: String = ""): Result<String> = runCatching {
        val target = globalMemoryFile(resolveStorageRoot(defaultRoot, storageRootOverride))
        require(target.isFile) { "$GLOBAL_MEMORY_RELATIVE does not exist yet." }
        WorkspaceTextReader.read(target, GLOBAL_MEMORY_RELATIVE, startLine = 1, maxLines = maxLines, maxTotalChars = MAX_MEMORY_READ_CHARS)
    }

    fun writeGlobalMemory(defaultRoot: () -> File, content: String, storageRootOverride: String = ""): Result<String> = runCatching {
        val target = globalMemoryFile(resolveStorageRoot(defaultRoot, storageRootOverride))
        AtomicFileStorage.writeText(target, content)
        "Successfully wrote ${content.length} characters (${content.lines().size} lines) to '$GLOBAL_MEMORY_RELATIVE'."
    }
}
