package io.parchment.ai.network

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.McpServerConfig
import io.parchment.ai.platform.LocalSkillBundle
import io.parchment.ai.platform.PlatformWorkspace
import io.ktor.http.encodeURLParameter
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

private const val REGISTRY_URL = "https://registry.modelcontextprotocol.io/v0/servers"
private const val MAX_RESPONSE_BYTES = 4 * 1024 * 1024
private const val REGISTRY_PAGE_SIZE = 30

internal data class RegistryServer(val name: String, val title: String, val description: String, val url: String, val needsToken: Boolean)
internal data class RegistryPage(val servers: List<RegistryServer>, val nextCursor: String)

private fun JsonObject.text(key: String): String = (this[key] as? JsonPrimitive)?.contentOrNull.orEmpty()

/** Keeps servers that can be reached directly and need nothing beyond an optional access token. */
internal fun parseRegistryPage(body: String): RegistryPage {
    val root = AiProviderEngine.json.parseToJsonElement(body) as? JsonObject ?: return RegistryPage(emptyList(), "")
    val servers = (root["servers"] as? JsonArray).orEmpty().mapNotNull { element ->
        val server = (element as? JsonObject)?.get("server") as? JsonObject ?: return@mapNotNull null
        val remote = (server["remotes"] as? JsonArray).orEmpty().filterIsInstance<JsonObject>().firstOrNull {
            it.text("type") == "streamable-http" && it.text("url").startsWith("https://")
        } ?: return@mapNotNull null
        val required = (remote["headers"] as? JsonArray).orEmpty().filterIsInstance<JsonObject>()
            .filter { (it["isRequired"] as? JsonPrimitive)?.booleanOrNull == true }
        if (required.any { !it.text("name").equals("Authorization", ignoreCase = true) }) return@mapNotNull null
        RegistryServer(server.text("name"), server.text("title"), server.text("description"), remote.text("url"), required.isNotEmpty())
    }.filter { it.name.isNotBlank() }.distinctBy { it.name }
    val cursor = ((root["metadata"] as? JsonObject)?.text("nextCursor")).orEmpty()
    return RegistryPage(servers, cursor)
}

internal suspend fun searchMcpRegistry(query: String, cursor: String = ""): Result<RegistryPage> = runCatching {
    val url = buildString {
        append(REGISTRY_URL).append("?limit=").append(REGISTRY_PAGE_SIZE).append("&version=latest")
        if (query.isNotBlank()) append("&search=").append(query.trim().encodeURLParameter())
        if (cursor.isNotBlank()) append("&cursor=").append(cursor.encodeURLParameter())
    }
    parseRegistryPage(withContext(Dispatchers.IO) { PlatformWorkspace.fetchPublicBytes(url, MAX_RESPONSE_BYTES).getOrThrow() }.decodeToString())
}

internal data class GitHubFolder(val owner: String, val repo: String, val ref: String, val path: String)

private val GITHUB_NAME = Regex("[A-Za-z0-9._-]+")

/** Accepts a repository, folder or SKILL.md link on github.com. */
internal fun parseGitHubFolder(link: String): GitHubFolder? {
    val trimmed = link.trim().removeSuffix("/")
    val rest = trimmed.removePrefix("https://github.com/").takeIf { it != trimmed } ?: return null
    val parts = rest.substringBefore('?').substringBefore('#').split('/').filter { it.isNotEmpty() }
    if (parts.size < 2 || !GITHUB_NAME.matches(parts[0]) || !GITHUB_NAME.matches(parts[1])) return null
    val repo = parts[1].removeSuffix(".git")
    if (parts.size == 2) return GitHubFolder(parts[0], repo, "", "")
    if (parts.size < 4 || parts[2] !in setOf("tree", "blob")) return null
    val inner = parts.drop(4)
    if (inner.any { it == ".." || it == "." }) return null
    val folder = if (inner.lastOrNull().equals("SKILL.md", ignoreCase = true)) inner.dropLast(1) else inner
    return GitHubFolder(parts[0], repo, parts[3], folder.joinToString("/"))
}

private const val MAX_SKILL_FILES = 64
private const val MAX_SKILL_FILE_BYTES = 1024 * 1024
private const val MAX_SKILL_TOTAL_BYTES = 8 * 1024 * 1024
private const val MAX_SKILL_DEPTH = 5
private val SKILL_FOLDER_NAME = Regex("[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}")
private val SAFE_FILE_NAME = Regex("[A-Za-z0-9._ -]+")

private suspend fun installValidatedSkillFiles(
    fallbackName: String,
    files: Map<String, String>,
    skipped: Int,
    exists: suspend (String) -> Boolean,
    write: suspend (String, String) -> Result<String>
): String {
    check(files.size <= MAX_SKILL_FILES) { "This skill has too many files." }
    for (relative in files.keys) {
        val segments = relative.split('/')
        check(segments.size - 1 <= MAX_SKILL_DEPTH) { "This skill's folders are nested too deeply." }
        for (seg in segments) {
            if (seg.isEmpty() || !SAFE_FILE_NAME.matches(seg) || seg.all { it == '.' }) {
                error("This skill contains an unsafe file name.")
            }
        }
    }
    val manifest = files.entries.firstOrNull { it.key.equals("SKILL.md", ignoreCase = true) }?.value
        ?: error("That folder has no SKILL.md, so it is not a skill.")
    val (declared, description) = WorkspaceHarness.parseSkillHeader(manifest) ?: error("This skill's SKILL.md has no valid header.")
    val name = declared.ifBlank { fallbackName.trim() }
    check(SKILL_FOLDER_NAME.matches(name)) { "This skill has an unusable name." }
    check(description.isNotBlank()) { "This skill has no description, so the agent could never choose it." }
    val base = ".parchment/skills/$name"
    check(!exists("$base/SKILL.md")) { "A skill named $name is already installed." }
    // The manifest goes last: it is what marks a skill as installed, so a failed copy can be retried.
    files.entries.sortedBy { it.key.equals("SKILL.md", ignoreCase = true) }.forEach { (relative, text) ->
        write("$base/$relative", text).getOrThrow()
    }
    return if (skipped > 0) "Installed $name, without $skipped binary file${if (skipped == 1) "" else "s"}." else "Installed $name."
}

/**
 * Copies a skill's text files into the workspace. Nothing is written until the whole folder has
 * been read and checked, and a skill is never overwritten. Scripts are copied but never run here.
 */
internal suspend fun installSkillFromGitHub(
    folder: GitHubFolder,
    exists: suspend (String) -> Boolean,
    write: suspend (String, String) -> Result<String>,
    fetch: suspend (String, Int) -> ByteArray
): Result<String> = runCatching {
    val files = linkedMapOf<String, String>()
    var total = 0
    var skipped = 0
    suspend fun walk(path: String, relative: String, depth: Int) {
        check(depth <= MAX_SKILL_DEPTH) { "This skill's folders are nested too deeply." }
        val encoded = path.split('/').filter { it.isNotEmpty() }.joinToString("/") { it.encodeURLParameter() }
        val query = if (folder.ref.isBlank()) "" else "?ref=${folder.ref.encodeURLParameter()}"
        val listing = fetch("https://api.github.com/repos/${folder.owner}/${folder.repo}/contents/$encoded$query", MAX_RESPONSE_BYTES).decodeToString()
        val entries = AiProviderEngine.json.parseToJsonElement(listing) as? JsonArray ?: error("That link is not a folder.")
        for (element in entries.filterIsInstance<JsonObject>()) {
            val name = element.text("name")
            val target = if (relative.isEmpty()) name else "$relative/$name"
            if (!SAFE_FILE_NAME.matches(name) || name.all { it == '.' }) error("This skill contains an unsafe file name.")
            when (element.text("type")) {
                "dir" -> walk(element.text("path"), target, depth + 1)
                "file" -> {
                    check(files.size < MAX_SKILL_FILES) { "This skill has too many files." }
                    val download = element.text("download_url")
                    check(download.startsWith("https://raw.githubusercontent.com/")) { "This skill has a file from an unexpected address." }
                    val bytes = fetch(download, MAX_SKILL_FILE_BYTES)
                    total += bytes.size
                    check(total <= MAX_SKILL_TOTAL_BYTES) { "This skill is too large." }
                    val text = runCatching { bytes.decodeToString(throwOnInvalidSequence = true) }.getOrNull()
                    if (text == null || '\u0000' in text) skipped++ else files[target] = text
                }
            }
        }
    }
    walk(folder.path, "", 0)
    val fallback = folder.path.substringAfterLast('/').ifBlank { folder.repo }
    installValidatedSkillFiles(fallback, files, skipped, exists, write)
}

internal fun isZipPayload(fileName: String, bytes: ByteArray): Boolean =
    fileName.trim().endsWith(".zip", ignoreCase = true) ||
        (bytes.size >= 4 && bytes[0] == 0x50.toByte() && bytes[1] == 0x4B.toByte() &&
            ((bytes[2] == 0x03.toByte() && bytes[3] == 0x04.toByte()) ||
                (bytes[2] == 0x05.toByte() && bytes[3] == 0x06.toByte())))

/** Installs a validated local folder or `.zip` skill bundle using identical guards to GitHub install. */
internal suspend fun installSkillFromLocalBundle(
    bundle: LocalSkillBundle,
    exists: suspend (String) -> Boolean,
    write: suspend (String, String) -> Result<String>
): Result<String> = runCatching {
    val files = linkedMapOf<String, String>()
    var total = 0
    for (entry in bundle.files) {
        check(files.size < MAX_SKILL_FILES) { "This skill has too many files." }
        val byteLen = entry.content.encodeToByteArray().size
        check(byteLen <= MAX_SKILL_FILE_BYTES) { "This skill contains a file larger than 1 MB." }
        total += byteLen
        check(total <= MAX_SKILL_TOTAL_BYTES) { "This skill is too large." }
        files[entry.relativePath] = entry.content
    }
    installValidatedSkillFiles(bundle.fallbackName, files, bundle.skippedBinaryCount, exists, write)
}

/** Installs a standalone `SKILL.md` file directly from disk without requiring a folder or zip. */
internal suspend fun installSkillFromSingleFile(
    fileName: String,
    bytes: ByteArray,
    exists: suspend (String) -> Boolean,
    write: suspend (String, String) -> Result<String>
): Result<String> = runCatching {
    check(bytes.size <= MAX_SKILL_FILE_BYTES) { "This skill contains a file larger than 1 MB." }
    val text = runCatching { bytes.decodeToString(throwOnInvalidSequence = true) }.getOrNull()
        ?.takeUnless { '\u0000' in it }
        ?: error("Selected file is not valid UTF-8 text.")
    val fallback = fileName.substringAfterLast('/').substringAfterLast('\\')
        .substringBeforeLast('.')
        .takeUnless { it.equals("skill", ignoreCase = true) }
        .orEmpty()
    installValidatedSkillFiles(fallback, linkedMapOf("SKILL.md" to text), 0, exists, write)
}

internal data class McpImportOutcome(
    val servers: List<McpServerConfig>,
    val skippedLocalCommandCount: Int = 0,
    val skippedInvalidCount: Int = 0
)

private fun quoteShellTokenIfNeeded(token: String): String {
    if (token.isEmpty()) return "\"\""
    val needsQuotes = token.any { it.isWhitespace() || it == '"' || it == '&' || it == '|' || it == '(' || it == ')' || it == ';' }
    return if (needsQuotes) "\"${token.replace("\"", "\\\"")}\"" else token
}

internal fun formatLocalMcpCommand(command: String, args: List<String>): String {
    val base = command.trim()
    if (base.isEmpty()) return ""
    if (args.isEmpty()) return base
    val formattedBase = if (base.startsWith("\"") && base.endsWith("\"")) base else quoteShellTokenIfNeeded(base)
    return (listOf(formattedBase) + args.map(::quoteShellTokenIfNeeded)).joinToString(" ")
}

internal fun formatPickedMcpCommand(filePath: String): String {
    val clean = filePath.trim()
    if (clean.isEmpty()) return ""
    val quoted = quoteShellTokenIfNeeded(clean)
    val lower = clean.lowercase()
    return when {
        lower.endsWith(".py") -> "python $quoted"
        lower.endsWith(".js") || lower.endsWith(".mjs") || lower.endsWith(".cjs") -> "node $quoted"
        else -> quoted
    }
}

private fun isSafeMcpHttpUrl(url: String): Boolean {
    val trimmed = url.trim()
    if (trimmed.isEmpty() || trimmed.any { it.isWhitespace() || it.code < 0x20 }) return false
    return trimmed.startsWith("https://", ignoreCase = true) || trimmed.startsWith("http://", ignoreCase = true)
}

private fun extractBearerToken(obj: JsonObject): String {
    val direct = obj.text("bearerToken").trim()
    if (direct.isNotEmpty()) return direct
    val headers = obj["headers"] as? JsonObject ?: return ""
    val auth = headers.entries.firstOrNull { it.key.equals("Authorization", ignoreCase = true) }
        ?.value?.let { (it as? JsonPrimitive)?.contentOrNull }?.trim().orEmpty()
    return when {
        auth.startsWith("Bearer ", ignoreCase = true) -> auth.substring(7).trim()
        else -> ""
    }
}

/**
 * Parses standard local MCP config files (`mcp.json`, `.mcp.json`, VS Code `servers`, Claude/Cursor
 * `mcpServers`, arrays, or single server objects). Every imported server starts with `enabled = false`,
 * and desktop-only local commands are dropped when [supportsLocalCommands] is false.
 */
internal fun parseMcpConfigJson(
    rawJson: String,
    supportsLocalCommands: Boolean
): Result<McpImportOutcome> = runCatching {
    val root = AiProviderEngine.json.parseToJsonElement(rawJson)
    val candidates = mutableListOf<Pair<String, JsonObject>>()

    fun collectContainer(element: JsonElement) {
        when (element) {
            is JsonArray -> element.filterIsInstance<JsonObject>().forEach { obj ->
                candidates += obj.text("name") to obj
            }
            is JsonObject -> {
                if (element["url"] != null || element["serverUrl"] != null || element["command"] != null) {
                    candidates += element.text("name") to element
                } else {
                    element.forEach { (key, value) ->
                        if (value is JsonObject) candidates += key to value
                    }
                }
            }
            else -> Unit
        }
    }

    when (root) {
        is JsonObject -> {
            val container = root["mcpServers"] ?: root["servers"]
            if (container != null) {
                collectContainer(container)
            } else {
                collectContainer(root)
            }
        }
        is JsonArray -> collectContainer(root)
        else -> error("This file does not contain a valid MCP JSON configuration.")
    }

    require(candidates.isNotEmpty()) { "No MCP servers were found in this JSON file." }

    val parsed = mutableListOf<McpServerConfig>()
    var skippedLocal = 0
    var skippedInvalid = 0

    for ((keyName, obj) in candidates) {
        val rawUrl = obj.text("url").ifBlank { obj.text("serverUrl") }.trim()
        val rawCmd = obj.text("command").trim()
        val args = (obj["args"] as? JsonArray).orEmpty()
            .mapNotNull { (it as? JsonPrimitive)?.contentOrNull }
        val resolvedName = obj.text("name").trim().ifBlank { keyName.trim() }

        when {
            rawUrl.isNotEmpty() -> {
                if (!isSafeMcpHttpUrl(rawUrl)) {
                    skippedInvalid++
                    continue
                }
                val displayName = resolvedName.ifBlank { rawUrl.substringAfter("://").substringBefore('/') }
                parsed += McpServerConfig(
                    id = "",
                    name = displayName,
                    url = rawUrl,
                    bearerToken = extractBearerToken(obj),
                    enabled = false
                )
            }
            rawCmd.isNotEmpty() -> {
                val formatted = formatLocalMcpCommand(rawCmd, args)
                if (formatted.isBlank() || formatted.any { it == '\n' || it == '\r' || it == '\u0000' }) {
                    skippedInvalid++
                    continue
                }
                if (!supportsLocalCommands) {
                    skippedLocal++
                    continue
                }
                val displayName = resolvedName.ifBlank { rawCmd.substringAfterLast('/').substringAfterLast('\\') }
                parsed += McpServerConfig(
                    id = "",
                    name = displayName,
                    command = formatted,
                    enabled = false
                )
            }
            else -> skippedInvalid++
        }
    }

    McpImportOutcome(
        servers = parsed,
        skippedLocalCommandCount = skippedLocal,
        skippedInvalidCount = skippedInvalid
    )
}

internal fun mergeImportedMcpServers(
    existing: List<McpServerConfig>,
    imported: List<McpServerConfig>,
    idPrefix: String
): Pair<List<McpServerConfig>, Int> {
    val result = existing.toMutableList()
    var added = 0
    for (server in imported) {
        val duplicate = result.any { cur ->
            (server.url.isNotBlank() && cur.url.equals(server.url, ignoreCase = true)) ||
                (server.command.isNotBlank() && cur.command.equals(server.command, ignoreCase = true) &&
                    cur.name.equals(server.name, ignoreCase = true))
        }
        if (!duplicate) {
            result += server.copy(id = "$idPrefix-${added++}", enabled = false)
        }
    }
    return result to added
}
