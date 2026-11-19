package io.parchment.ai.platform

import java.io.File
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.StandardCopyOption
import java.security.MessageDigest

/** Path-guarded workspace mutations, shared so both platforms enforce exactly the same rules. */
internal object WorkspaceFileEditor {
    fun write(workspaceRoot: String, relativePath: String, content: String): String {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        target.parentFile?.mkdirs()
        target.writeText(content, Charsets.UTF_8)
        return "Successfully wrote ${content.length} characters (${content.lines().size} lines) to '$relativePath'."
    }

    fun append(workspaceRoot: String, relativePath: String, content: String): String {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        target.parentFile?.mkdirs()
        target.appendText(content, Charsets.UTF_8)
        return "Successfully appended ${content.length} characters (${content.lines().size} lines) to '$relativePath'."
    }

    fun edit(workspaceRoot: String, relativePath: String, oldText: String, newText: String, replaceAll: Boolean): String {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        require(target.exists() && target.isFile) { "File not found: $relativePath" }
        val existingRaw = target.readText(Charsets.UTF_8)
        val usesCrlf = existingRaw.contains("\r\n")
        val existingLf = existingRaw.replace("\r\n", "\n")

        val editRes = WorkspaceFuzzyEditEngine.applySingleEditWithFuzzySelfHealing(
            existingLf = existingLf,
            rawOldText = oldText,
            rawNewText = newText,
            replaceAll = replaceAll,
            relativePath = relativePath
        )

        val finalContent = if (usesCrlf) editRes.updatedLf.replace("\n", "\r\n") else editRes.updatedLf
        target.writeText(finalContent, Charsets.UTF_8)
        val fuzzyNote = if (editRes.usedFuzzyWhitespace) " (fuzzy whitespace self-healed)" else ""
        return "Edited '$relativePath' (+${editRes.addedLines} / -${editRes.removedLines} lines)$fuzzyNote."
    }

    fun multiEdit(workspaceRoot: String, relativePath: String, edits: List<Triple<String, String, Boolean>>): String {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        require(target.exists() && target.isFile) { "File not found: $relativePath" }
        require(edits.isNotEmpty()) { "edits list must not be empty." }
        val existingRaw = target.readText(Charsets.UTF_8)
        val usesCrlf = existingRaw.contains("\r\n")
        var currentLf = existingRaw.replace("\r\n", "\n")
        var totalAdded = 0
        var totalRemoved = 0

        edits.forEachIndexed { idx, (oldText, newText, replaceAll) ->
            require(oldText.isNotEmpty()) { "Edit #${idx + 1}: old_text must not be empty." }
            val stepRes = runCatching {
                WorkspaceFuzzyEditEngine.applySingleEditWithFuzzySelfHealing(
                    existingLf = currentLf,
                    rawOldText = oldText,
                    rawNewText = newText,
                    replaceAll = replaceAll,
                    relativePath = relativePath
                )
            }.getOrElse { err ->
                error("Edit #${idx + 1} failed (rolled back all edits): ${err.message}")
            }
            currentLf = stepRes.updatedLf
            totalRemoved += stepRes.removedLines
            totalAdded += stepRes.addedLines
        }

        val finalContent = if (usesCrlf) currentLf.replace("\n", "\r\n") else currentLf
        target.writeText(finalContent, Charsets.UTF_8)
        return "Applied ${edits.size} edit(s) atomically to '$relativePath' (+$totalAdded / -$totalRemoved lines)."
    }

    fun move(workspaceRoot: String, sourceRelativePath: String, destRelativePath: String): String {
        val srcFile = WorkspacePathGuard.resolve(workspaceRoot, sourceRelativePath).toFile()
        val dstFile = WorkspacePathGuard.resolve(workspaceRoot, destRelativePath).toFile()
        val guardedRoot = WorkspacePathGuard.root(workspaceRoot)
        require(srcFile.toPath() != guardedRoot && dstFile.toPath() != guardedRoot) { "Cannot move or replace the workspace root." }
        WorkspacePathGuard.requireUnlinkedTree(guardedRoot, srcFile)
        require(srcFile.exists()) { "Source path does not exist: $sourceRelativePath" }
        dstFile.parentFile?.mkdirs()
        Files.move(srcFile.toPath(), dstFile.toPath(), StandardCopyOption.REPLACE_EXISTING)
        return "Moved '$sourceRelativePath' -> '$destRelativePath'."
    }

    fun delete(workspaceRoot: String, relativePath: String): String {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        val rootCanon = WorkspacePathGuard.root(workspaceRoot).toFile()
        require(target.canonicalFile != rootCanon) { "Cannot delete workspace root directory." }
        require(target.exists()) { "Path does not exist: $relativePath" }
        WorkspacePathGuard.requireUnlinkedTree(rootCanon.toPath(), target)
        val deleted = if (target.isDirectory) target.deleteRecursively() else target.delete()
        require(deleted) { "Failed to delete '$relativePath'." }
        return "Deleted '$relativePath'."
    }

    private const val MAX_HASH_STAMP_BYTES = 4L * 1024L * 1024L

    fun directoryExists(path: String): Boolean {
        val trimmed = path.trim()
        if (trimmed.isEmpty()) return false
        return runCatching {
            val file = File(trimmed)
            file.exists() && file.isDirectory
        }.getOrDefault(false)
    }

    fun fileStamp(workspaceRoot: String, relativePath: String): String? {
        val path = WorkspacePathGuard.resolve(workspaceRoot, relativePath)
        if (!Files.exists(path, LinkOption.NOFOLLOW_LINKS) || !Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)) {
            return null
        }
        val size = Files.size(path)
        return if (size <= MAX_HASH_STAMP_BYTES) {
            val digest = MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(path))
            val hex = digest.joinToString("") { "%02x".format(it) }
            "$size:$hex"
        } else {
            val mtime = Files.getLastModifiedTime(path, LinkOption.NOFOLLOW_LINKS).toInstant()
            "$size:${mtime.epochSecond}:${mtime.nano}"
        }
    }
}
