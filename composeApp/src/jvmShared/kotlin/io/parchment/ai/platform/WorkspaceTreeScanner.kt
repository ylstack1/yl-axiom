package io.parchment.ai.platform

import java.io.File
import java.nio.file.Path
import java.util.concurrent.atomic.AtomicInteger
import java.util.stream.Collectors

internal data class WorkspaceScanConfig(
    val missingDirMessage: (String) -> String,
    val notDirMessage: (String) -> String,
    val emptyDirectoryMessage: String,
    val useTreeCache: Boolean,
    val canonicalizeRelativize: Boolean,
    val allowEmptyPlainGlobFallback: Boolean,
    val requirePositiveSearchFileBytes: Boolean,
    val maxSearchFileBytes: Long,
    val maxSearchHits: Int,
    val maxSearchLineChars: Int,
    val parallelSearch: Boolean,
    val filterSearchByRelPath: Boolean,
    val maxTreeEntries: Int = DEFAULT_MAX_TREE_ENTRIES
)

private const val DEFAULT_MAX_TREE_ENTRIES = 25_000

/** Shared sandboxed workspace directory listing, glob matching, and text search with optional tree caching. */
internal class WorkspaceTreeScanner(
    private val config: WorkspaceScanConfig
) {
    private data class CachedEntry(
        val relPath: String,
        val name: String,
        val file: File,
        val isDirectory: Boolean,
        val length: Long
    ) {
        fun formatListing(): String =
            if (isDirectory) "$relPath/" else "$relPath ($length bytes)"
    }

    private data class CachedWorkspaceTree(
        val rootCanonicalPath: String,
        val timestampMs: Long,
        val entries: List<CachedEntry>
    )

    @Volatile
    private var cachedTree: CachedWorkspaceTree? = null

    fun invalidate() {
        cachedTree = null
    }

    fun list(workspaceRoot: String, relativePath: String, recursive: Boolean): Result<String> = runCatching {
        val target = WorkspacePathGuard.resolve(workspaceRoot, relativePath).toFile()
        val rootFile = WorkspacePathGuard.root(workspaceRoot).toFile()
        val rootPath = rootFile.toPath().normalize()
        if (!target.exists()) throw IllegalArgumentException(config.missingDirMessage(relativePath))
        require(target.isDirectory) { config.notDirMessage(relativePath) }

        val entries = when {
            recursive && config.useTreeCache && relativePath.trim().isEmpty() -> {
                getOrBuildRootTree(rootFile).take(MAX_RECURSIVE_LIST_ENTRIES).map { it.formatListing() }
            }
            recursive -> {
                walkEntries(target, rootPath).take(MAX_RECURSIVE_LIST_ENTRIES).map { it.formatListing() }.toList()
            }
            else -> {
                (target.listFiles()?.toList() ?: emptyList())
                    .filter { WorkspacePathGuard.canTraverse(rootPath, it.toPath()) }
                    .sortedWith(compareBy<File> { !it.isDirectory }.thenBy { it.name.lowercase() })
                    .take(MAX_FLAT_LIST_ENTRIES)
                    .map { toEntry(rootPath, it).formatListing() }
            }
        }
        if (entries.isEmpty()) config.emptyDirectoryMessage else entries.joinToString("\n")
    }

    fun glob(workspaceRoot: String, pattern: String, maxResults: Int): Result<String> = runCatching {
        val rootFile = WorkspacePathGuard.root(workspaceRoot).toFile()
        val rootPath = rootFile.toPath().normalize()
        require(rootFile.exists() && rootFile.isDirectory) { "Workspace root does not exist: $workspaceRoot" }
        val cleanPat = pattern.trim().replace('\\', '/').removePrefix("./")
        require(cleanPat.isNotEmpty()) { "Glob pattern must not be empty." }

        val globRegex = compileGlobRegex(cleanPat)
        val hasSlash = cleanPat.contains('/')
        val hasWildcard = '*' in cleanPat || '?' in cleanPat
        val plainSub = if (!hasWildcard) cleanPat else ""
        val entries = if (config.useTreeCache) {
            getOrBuildRootTree(rootFile).asSequence()
        } else {
            walkEntries(rootFile, rootPath)
        }
        val matches = entries
            .mapNotNull { entry ->
                val matchTarget = if (!hasSlash) entry.name else entry.relPath
                val plainMatch = !hasWildcard &&
                    (config.allowEmptyPlainGlobFallback || plainSub.isNotEmpty()) &&
                    entry.relPath.contains(plainSub, ignoreCase = true)
                if (globRegex.matches(matchTarget) || plainMatch) entry.formatListing() else null
            }
            .take(maxResults.coerceIn(1, MAX_GLOB_RESULTS))
            .toList()
        if (matches.isEmpty()) "No files matched glob '$pattern'." else matches.joinToString("\n")
    }

    private data class SearchHit(
        val formatted: String,
        val isImportLine: Boolean
    )

    fun search(
        workspaceRoot: String,
        query: String,
        fileGlob: String,
        relativePath: String = "",
        maxResults: Int = 0
    ): Result<String> = runCatching {
        val rootFile = WorkspacePathGuard.root(workspaceRoot).toFile()
        val rootPath = rootFile.toPath().normalize()
        require(rootFile.exists() && rootFile.isDirectory) { "Workspace root does not exist: $workspaceRoot" }
        require(query.isNotBlank()) { "Search query must not be blank." }

        val effectiveMaxHits = if (maxResults > 0) {
            maxResults.coerceIn(1, MAX_SEARCH_RESULTS_CAP)
        } else {
            config.maxSearchHits
        }
        val regex = runCatching { Regex(query, RegexOption.IGNORE_CASE) }.getOrNull()
        val cleanFilter = fileGlob.trim().trimStart('*', '/')
        val cleanScope = relativePath.trim().replace('\\', '/').trim('/')

        val sourceEntries: List<CachedEntry> = if (cleanScope.isEmpty()) {
            if (config.useTreeCache) {
                getOrBuildRootTree(rootFile)
            } else {
                walkEntries(rootFile, rootPath).take(config.maxTreeEntries.coerceAtLeast(1)).toList()
            }
        } else {
            val scopedFile = WorkspacePathGuard.resolve(workspaceRoot, cleanScope).toFile()
            require(scopedFile.exists()) { "Path not found: $relativePath" }
            require(WorkspacePathGuard.canTraverse(rootPath, scopedFile.toPath())) {
                "Access denied: Linked workspace targets cannot be searched."
            }
            if (scopedFile.isFile) {
                listOf(toEntry(rootPath, scopedFile))
            } else {
                val hasIgnoredSeg = cleanScope.split('/').any { WorkspacePathGuard.isIgnoredDirectory(it) }
                if (config.useTreeCache && !hasIgnoredSeg) {
                    val normPrefix = "$cleanScope/"
                    getOrBuildRootTree(rootFile).filter { it.relPath == cleanScope || it.relPath.startsWith(normPrefix) }
                } else {
                    walkEntries(scopedFile, rootPath).take(config.maxTreeEntries.coerceAtLeast(1)).toList()
                }
            }
        }

        val candidates = sourceEntries.filter { entry ->
            !entry.isDirectory &&
                entry.length <= config.maxSearchFileBytes &&
                (!config.requirePositiveSearchFileBytes || entry.length > 0L) &&
                (cleanFilter.isEmpty() ||
                    entry.name.contains(cleanFilter, ignoreCase = true) ||
                    (config.filterSearchByRelPath && entry.relPath.contains(cleanFilter, ignoreCase = true)))
        }.sortedBy { it.relPath }

        val nonImportHits = AtomicInteger(0)
        val importHits = AtomicInteger(0)
        val allHits: List<SearchHit> = if (config.parallelSearch && candidates.size > 1) {
            val perFileHits = candidates.parallelStream()
                .map { entry -> searchSingleFile(entry, rootPath, query, regex, nonImportHits, importHits, effectiveMaxHits) }
                .collect(Collectors.toList())
            buildList {
                for (hits in perFileHits) {
                    if (hits != null) addAll(hits)
                }
            }
        } else {
            buildList {
                for (entry in candidates) {
                    if (nonImportHits.get() >= effectiveMaxHits) break
                    searchSingleFile(entry, rootPath, query, regex, nonImportHits, importHits, effectiveMaxHits)?.let(::addAll)
                }
            }
        }

        val selected = if (allHits.size <= effectiveMaxHits) {
            allHits.map { it.formatted }
        } else {
            val primary = allHits.asSequence().filter { !it.isImportLine }.map { it.formatted }.take(effectiveMaxHits).toList()
            if (primary.size >= effectiveMaxHits) {
                primary
            } else {
                val needed = effectiveMaxHits - primary.size
                primary + allHits.asSequence().filter { it.isImportLine }.map { it.formatted }.take(needed).toList()
            }
        }
        if (selected.isEmpty()) "No matches found for '$query'." else selected.joinToString("\n")
    }

    private fun searchSingleFile(
        entry: CachedEntry,
        rootPath: Path,
        query: String,
        regex: Regex?,
        nonImportHits: AtomicInteger,
        importHits: AtomicInteger,
        maxCap: Int
    ): List<SearchHit>? {
        if (nonImportHits.get() >= maxCap) return null
        val fileHits = mutableListOf<SearchHit>()
        runCatching {
            require(WorkspacePathGuard.canTraverseVerifiedChild(rootPath, entry.file.toPath())) {
                "Workspace path changed during search."
            }
            entry.file.bufferedReader(Charsets.UTF_8).use { reader ->
                var idx = 0
                while (nonImportHits.get() < maxCap) {
                    val line = reader.readLine() ?: break
                    val hit = regex?.containsMatchIn(line) ?: line.contains(query, ignoreCase = true)
                    if (hit) {
                        val trimmed = line.trim()
                        val isImport = trimmed.startsWith("import ")
                        val claimed = if (isImport) importHits.incrementAndGet() else nonImportHits.incrementAndGet()
                        if (claimed <= maxCap) {
                            fileHits.add(
                                SearchHit(
                                    formatted = "${entry.relPath}:${idx + 1}: ${trimmed.take(config.maxSearchLineChars)}",
                                    isImportLine = isImport
                                )
                            )
                        } else if (!isImport) {
                            break
                        }
                    }
                    idx++
                }
            }
        }
        return fileHits.ifEmpty { null }
    }

    private val treeLock = Any()

    private fun getOrBuildRootTree(rootFile: File): List<CachedEntry> {
        val canonPath = rootFile.canonicalPath
        val now = System.currentTimeMillis()
        val existing = cachedTree
        if (existing != null &&
            existing.rootCanonicalPath == canonPath &&
            (now - existing.timestampMs) in 0L..TREE_CACHE_TTL_MS
        ) {
            return existing.entries
        }
        synchronized(treeLock) {
            val freshNow = System.currentTimeMillis()
            val inside = cachedTree
            if (inside != null &&
                inside.rootCanonicalPath == canonPath &&
                (freshNow - inside.timestampMs) in 0L..TREE_CACHE_TTL_MS
            ) {
                return inside.entries
            }
            val rootPath = rootFile.toPath().normalize()
            val built = walkEntries(rootFile, rootPath).take(config.maxTreeEntries.coerceAtLeast(1)).toList()
            cachedTree = CachedWorkspaceTree(canonPath, freshNow, built)
            return built
        }
    }

    private fun walkEntries(startDir: File, rootPath: Path): Sequence<CachedEntry> =
        startDir.walkTopDown()
            .onEnter { dir ->
                (dir == startDir || !WorkspacePathGuard.isIgnoredDirectory(dir.name)) &&
                    WorkspacePathGuard.canTraverse(rootPath, dir.toPath())
            }
            .maxDepth(MAX_WALK_DEPTH)
            .filter { file ->
                file != startDir &&
                    !WorkspacePathGuard.isIgnoredDirectory(file.name) &&
                    (file.isDirectory || WorkspacePathGuard.canTraverseVerifiedChild(rootPath, file.toPath()))
            }
            .map { file -> toEntry(rootPath, file) }

    private fun toEntry(rootPath: Path, file: File): CachedEntry {
        val pathForRel = if (config.canonicalizeRelativize) file.canonicalFile.toPath() else file.toPath()
        val rel = rootPath.relativize(pathForRel).toString().replace('\\', '/')
        val isDir = file.isDirectory
        return CachedEntry(
            relPath = rel,
            name = file.name,
            file = file,
            isDirectory = isDir,
            length = if (isDir) 0L else file.length()
        )
    }

    private fun compileGlobRegex(cleanPat: String): Regex {
        val regexStr = buildString {
            append("^")
            var i = 0
            while (i < cleanPat.length) {
                val c = cleanPat[i]
                if (c == '*' && i + 1 < cleanPat.length && cleanPat[i + 1] == '*') {
                    if (i + 2 < cleanPat.length && cleanPat[i + 2] == '/') {
                        append("(?:.*/)?")
                        i += 3
                        continue
                    } else {
                        append(".*")
                        i += 2
                        continue
                    }
                } else if (c == '*') {
                    append("[^/]*")
                } else if (c == '?') {
                    append("[^/]")
                } else if (".+()[]{}|^$\\".contains(c)) {
                    append('\\').append(c)
                } else {
                    append(c)
                }
                i++
            }
            append("$")
        }
        return Regex(regexStr, RegexOption.IGNORE_CASE)
    }

    private companion object {
        const val TREE_CACHE_TTL_MS = 10_000L
        const val MAX_WALK_DEPTH = 24
        const val MAX_FLAT_LIST_ENTRIES = 300
        const val MAX_RECURSIVE_LIST_ENTRIES = 400
        const val MAX_GLOB_RESULTS = 500
        const val MAX_SEARCH_RESULTS_CAP = 300
    }
}
