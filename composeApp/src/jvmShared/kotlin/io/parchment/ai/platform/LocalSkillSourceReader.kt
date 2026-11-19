package io.parchment.ai.platform

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.util.zip.ZipInputStream

/**
 * Reads local skill folders and `.zip` archives with the exact same safety bounds as GitHub skill
 * installation: max 64 files, 1 MB per file, 8 MB total, depth <= 5, safe segment names only,
 * symlink/junction rejection, and bounded decompression to stop zip bombs.
 */
internal object LocalSkillSourceReader {
    private const val MAX_SKILL_FILES = 64
    private const val MAX_SKILL_FILE_BYTES = 1024 * 1024
    private const val MAX_SKILL_TOTAL_BYTES = 8 * 1024 * 1024
    private const val MAX_SKILL_DEPTH = 5
    private const val MAX_ZIP_ENTRIES = 256
    private val SAFE_FILE_NAME = Regex("[A-Za-z0-9._ -]+")

    private fun isIgnoredMetadataEntry(segment: String): Boolean {
        val low = segment.lowercase()
        return low == "__macosx" || low == ".ds_store" || low == "thumbs.db" || low == ".git"
    }

    private fun validateRelativePath(relative: String) {
        require(relative.isNotEmpty() && !relative.startsWith('/') && !relative.startsWith('\\') && ':' !in relative && '\u0000' !in relative) {
            "This skill contains an unsafe file name."
        }
        val segments = relative.split('/')
        require(segments.size - 1 <= MAX_SKILL_DEPTH) { "This skill's folders are nested too deeply." }
        for (segment in segments) {
            if (segment.isEmpty() || !SAFE_FILE_NAME.matches(segment) || segment.all { it == '.' }) {
                error("This skill contains an unsafe file name.")
            }
        }
    }

    private fun decodeTextOrNull(bytes: ByteArray): String? {
        val text = runCatching { bytes.decodeToString(throwOnInvalidSequence = true) }.getOrNull() ?: return null
        return text.takeUnless { '\u0000' in it }
    }

    fun readFolder(folderPath: String): Result<LocalSkillBundle> = runCatching {
        val rawPath = folderPath.trim()
        require(rawPath.isNotEmpty()) { "Choose a local skill folder first." }
        val rawFile = File(rawPath)
        require(rawFile.exists() && rawFile.isDirectory) { "That folder does not exist." }
        val root: Path = rawFile.toPath().toAbsolutePath().normalize().toRealPath()

        val files = mutableListOf<LocalSkillTextFile>()
        val seenLower = HashSet<String>()
        var totalBytes = 0
        var skippedBinary = 0

        fun walk(dir: Path, relativePrefix: String, depth: Int) {
            check(depth <= MAX_SKILL_DEPTH) { "This skill's folders are nested too deeply." }
            val children = Files.list(dir).use { stream ->
                stream.sorted(Comparator.comparing { it.fileName.toString() }).toList()
            }
            for (child in children) {
                val name = child.fileName?.toString().orEmpty()
                if (isIgnoredMetadataEntry(name)) continue
                if (!SAFE_FILE_NAME.matches(name) || name.all { it == '.' }) {
                    error("This skill contains an unsafe file name.")
                }
                val attrs = Files.readAttributes(child, java.nio.file.attribute.BasicFileAttributes::class.java, LinkOption.NOFOLLOW_LINKS)
                if (attrs.isSymbolicLink || attrs.isOther || !WorkspacePathGuard.canTraverseVerifiedChild(root, child)) {
                    error("This skill contains a symbolic link or unsafe path.")
                }
                val rel = if (relativePrefix.isEmpty()) name else "$relativePrefix/$name"
                if (attrs.isDirectory) {
                    walk(child, rel, depth + 1)
                } else if (attrs.isRegularFile) {
                    check(files.size + skippedBinary < MAX_SKILL_FILES) { "This skill has too many files." }
                    val size = attrs.size()
                    check(size <= MAX_SKILL_FILE_BYTES.toLong()) { "This skill contains a file larger than 1 MB." }
                    check(totalBytes + size <= MAX_SKILL_TOTAL_BYTES.toLong()) { "This skill is too large." }
                    val bytes = Files.readAllBytes(child)
                    totalBytes += bytes.size
                    check(totalBytes <= MAX_SKILL_TOTAL_BYTES) { "This skill is too large." }
                    check(seenLower.add(rel.lowercase())) { "This skill contains duplicate file names." }
                    val text = decodeTextOrNull(bytes)
                    if (text == null) {
                        skippedBinary++
                    } else {
                        files += LocalSkillTextFile(rel, text)
                    }
                }
            }
        }

        walk(root, "", 0)
        LocalSkillBundle(
            fallbackName = root.fileName?.toString().orEmpty(),
            files = files,
            skippedBinaryCount = skippedBinary
        )
    }

    fun readZip(fileName: String, zipBytes: ByteArray): Result<LocalSkillBundle> = runCatching {
        require(zipBytes.isNotEmpty()) { "That zip archive is empty." }
        check(zipBytes.size <= MAX_SKILL_TOTAL_BYTES) { "This skill archive is too large." }

        val rawEntries = mutableListOf<Pair<String, ByteArray>>()
        var inspectedEntries = 0
        var totalBytes = 0

        ZipInputStream(ByteArrayInputStream(zipBytes)).use { zip ->
            val buf = ByteArray(16 * 1024)
            while (true) {
                val entry = zip.nextEntry ?: break
                if (++inspectedEntries > MAX_ZIP_ENTRIES) error("This skill has too many files.")
                val normalized = entry.name.replace('\\', '/')
                if (normalized.startsWith('/') || ':' in normalized || '\u0000' in normalized) {
                    error("This skill contains an unsafe file name.")
                }
                val segments = normalized.split('/').filter { it.isNotEmpty() }
                if (segments.any { it == ".." || it == "." }) {
                    error("This skill contains an unsafe file name.")
                }
                if (segments.isEmpty() || segments.any(::isIgnoredMetadataEntry)) {
                    zip.closeEntry()
                    continue
                }
                if (entry.isDirectory) {
                    zip.closeEntry()
                    continue
                }
                check(rawEntries.size < MAX_SKILL_FILES) { "This skill has too many files." }
                val out = ByteArrayOutputStream()
                var entryBytes = 0
                while (true) {
                    val read = zip.read(buf)
                    if (read <= 0) break
                    entryBytes += read
                    totalBytes += read
                    check(entryBytes <= MAX_SKILL_FILE_BYTES) { "This skill contains a file larger than 1 MB." }
                    check(totalBytes <= MAX_SKILL_TOTAL_BYTES) { "This skill is too large." }
                    out.write(buf, 0, read)
                }
                rawEntries += segments.joinToString("/") to out.toByteArray()
                zip.closeEntry()
            }
        }

        require(rawEntries.isNotEmpty()) { "That zip archive has no files." }

        // Support standard single-root-folder zips (e.g. `my-skill/SKILL.md`) seamlessly.
        val hasRootManifest = rawEntries.any { it.first.equals("SKILL.md", ignoreCase = true) }
        val firstSegments = rawEntries.map { it.first.substringBefore('/', "") }.toSet()
        val singleRootPrefix = if (!hasRootManifest && firstSegments.size == 1 && firstSegments.first().isNotEmpty() && rawEntries.all { '/' in it.first }) {
            firstSegments.first()
        } else {
            ""
        }

        val files = mutableListOf<LocalSkillTextFile>()
        val seenLower = HashSet<String>()
        var skippedBinary = 0
        for ((rawPath, bytes) in rawEntries) {
            val rel = if (singleRootPrefix.isNotEmpty()) rawPath.removePrefix("$singleRootPrefix/") else rawPath
            validateRelativePath(rel)
            check(seenLower.add(rel.lowercase())) { "This skill contains duplicate file names." }
            val text = decodeTextOrNull(bytes)
            if (text == null) {
                skippedBinary++
            } else {
                files += LocalSkillTextFile(rel, text)
            }
        }

        val fallback = singleRootPrefix.ifBlank {
            fileName.substringAfterLast('/').substringAfterLast('\\').removeSuffix(".zip").removeSuffix(".ZIP")
        }
        LocalSkillBundle(fallbackName = fallback, files = files, skippedBinaryCount = skippedBinary)
    }
}
