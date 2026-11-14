package io.parchment.ai.harness

import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformWorkspace

/**
 * 10-second TTL cache for workspace instruction files (`AGENTS.md`, `CLAUDE.md`, `PARCHMENT.md`,
 * `PARCHMENT_MEMORY.md`) and `.parchment/.agents/.claude` SKILL.md manifests.
 */
internal object WorkspaceContextCache {

    const val SKILL_MAX_LINES = 2_000

    private data class CachedWorkspaceContext(
        val timestampMs: Long,
        val fileNames: List<String>,
        val combinedContent: String,
        val skills: List<WorkspaceHarness.WorkspaceSkill> = emptyList()
    )

    private val workspaceContextCache = mutableMapOf<String, CachedWorkspaceContext>()
    private const val WORKSPACE_CONTEXT_TTL_MS = 10_000L
    private const val SKILL_MAX_DESCRIPTION_CHARS = 1_024
    private const val SKILL_MAX_COUNT = 64
    private val SKILL_FOLDERS = listOf(".parchment/skills", ".agents/skills", ".claude/skills")
    private val SKILL_NAME = Regex("[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}")

    fun invalidate(workspaceRoot: String = "") {
        synchronized(workspaceContextCache) {
            if (workspaceRoot.isBlank()) {
                workspaceContextCache.clear()
            } else {
                workspaceContextCache.remove(workspaceRoot.trim())
            }
        }
    }

    /** Reads the `name` and `description` of a SKILL.md, including folded YAML descriptions. */
    fun parseSkillHeader(text: String): Pair<String, String>? {
        val lines = text.lines()
        if (lines.firstOrNull()?.trim() != "---") return null
        val end = lines.drop(1).indexOfFirst { it.trim() == "---" }
        if (end < 0) return null
        val header = lines.subList(1, end + 1)
        fun field(key: String): String {
            val at = header.indexOfFirst { it.startsWith("$key:") }
            if (at < 0) return ""
            val first = header[at].substringAfter(':').trim()
            if (first.isNotEmpty() && first.first() !in ">|") return first.removeSurrounding("\"").removeSurrounding("'")
            return header.drop(at + 1).takeWhile { it.isBlank() || it.first().isWhitespace() }.joinToString(" ") { it.trim() }.trim()
        }
        return field("name") to field("description")
    }

    private fun scanSkills(root: String): List<WorkspaceHarness.WorkspaceSkill> {
        val found = linkedMapOf<String, WorkspaceHarness.WorkspaceSkill>()
        for (folder in SKILL_FOLDERS) {
            val entries = PlatformWorkspace.workspaceList(root, folder, recursive = false).getOrNull() ?: continue
            for (entry in entries.lines().map { it.trim() }.filter { it.endsWith("/") }.sorted()) {
                val path = "${entry}SKILL.md"
                val numbered = PlatformWorkspace.workspaceRead(root, path, startLine = 1, maxLines = 40).getOrNull() ?: continue
                // The reader adds a header line and line numbers for the model.
                val text = numbered.lines().drop(1).joinToString("\n") { it.substringAfter(": ", "") }
                val (declared, description) = parseSkillHeader(text) ?: continue
                val name = declared.ifBlank { entry.trimEnd('/').substringAfterLast('/') }
                if (description.isNotBlank() && SKILL_NAME.matches(name) && name !in found && found.size < SKILL_MAX_COUNT) {
                    found[name] = WorkspaceHarness.WorkspaceSkill(name, description.take(SKILL_MAX_DESCRIPTION_CHARS), path)
                }
            }
        }
        return found.values.toList()
    }

    private fun getOrLoad(workspaceRoot: String): CachedWorkspaceContext {
        val cleanRoot = workspaceRoot.trim()
        if (cleanRoot.isEmpty()) return CachedWorkspaceContext(0L, emptyList(), "")
        val now = PlatformRuntime.currentTimeMillis()
        synchronized(workspaceContextCache) {
            val cached = workspaceContextCache[cleanRoot]
            if (cached != null && now - cached.timestampMs <= WORKSPACE_CONTEXT_TTL_MS) {
                return cached
            }
        }
        val candidates = listOf("AGENTS.md", "CLAUDE.md", "PARCHMENT.md", "PARCHMENT_MEMORY.md")
        val foundNames = mutableListOf<String>()
        val sb = StringBuilder()
        for (fileName in candidates) {
            PlatformWorkspace.workspaceRead(cleanRoot, fileName, startLine = 1, maxLines = 10_000).onSuccess { content ->
                if (content.isNotBlank()) {
                    foundNames.add(fileName)
                    sb.appendLine("#### Auto-Loaded Workspace File: `$fileName`")
                    sb.appendLine(content)
                    sb.appendLine()
                }
            }
        }
        val skills = scanSkills(cleanRoot)
        if (skills.isNotEmpty()) {
            sb.appendLine("#### Available skills")
            sb.appendLine("Call `skill_load` with a skill's name when its description fits the task.")
            skills.forEach { sb.appendLine("- ${it.name}: ${it.description}") }
        }
        val fresh = CachedWorkspaceContext(
            timestampMs = now,
            fileNames = foundNames,
            skills = skills,
            combinedContent = sb.toString().trim()
        )
        synchronized(workspaceContextCache) {
            workspaceContextCache[cleanRoot] = fresh
        }
        return fresh
    }

    fun discoverSkills(workspaceRoot: String): List<WorkspaceHarness.WorkspaceSkill> =
        if (workspaceRoot.isBlank()) emptyList() else getOrLoad(workspaceRoot).skills

    fun listDiscoveredContextFileNames(workspaceRoot: String): List<String> =
        if (workspaceRoot.isBlank()) emptyList() else getOrLoad(workspaceRoot).fileNames

    fun discoverContextFiles(workspaceRoot: String): String =
        if (workspaceRoot.isBlank()) "" else getOrLoad(workspaceRoot).combinedContent
}
