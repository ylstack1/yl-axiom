package io.parchment.ai.platform

import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.DefaultModelConfig
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.ThemeConfig
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.attribute.BasicFileAttributes

/** Reads only a chat's id: the rest of the JSON is syntax-checked and skipped, not built into objects. */
@Serializable
private class ChatIdentity(val id: String = "")

/** Shared Android/desktop snapshot validation, recovery and atomic persistence. */
internal class SnapshotFileStorage(
    private val directory: () -> File,
    private val extraLegacySettings: () -> List<File> = { emptyList() }
) {
    private val json = Json { ignoreUnknownKeys = true; isLenient = true }
    private val unsafeSessionCharacters = Regex("[^a-zA-Z0-9._-]")

    private fun decodeSettings(raw: String): AppStateSnapshot? = runCatching {
        val migrated = if (raw.contains("\"BYTEDANCE_ARK\"")) {
            raw.replace("\"BYTEDANCE_ARK\"", "\"OPENAI_RESPONSES\"")
        } else raw
        if (!hasTopLevelSettingsKey(migrated)) return@runCatching null
        json.decodeFromString<AppStateSnapshot>(migrated)
    }.getOrNull()

    /** Scans only top-level object keys without allocating a DOM JsonElement tree. */
    private fun hasTopLevelSettingsKey(jsonText: String): Boolean {
        val len = jsonText.length
        var i = 0
        while (i < len && jsonText[i].isWhitespace()) i++
        if (i >= len || jsonText[i] != '{') return false
        i++

        var braceDepth = 1
        var bracketDepth = 0
        var inString = false
        var escaped = false
        var expectingKey = true

        while (i < len && braceDepth > 0) {
            val ch = jsonText[i]
            if (inString) {
                if (escaped) {
                    escaped = false
                } else if (ch == '\\') {
                    escaped = true
                } else if (ch == '"') {
                    inString = false
                }
                i++
                continue
            }

            when (ch) {
                '"' -> {
                    if (braceDepth == 1 && bracketDepth == 0 && expectingKey) {
                        val keyStart = i + 1
                        var j = keyStart
                        var keyEscaped = false
                        var simpleKey = true
                        while (j < len) {
                            val kc = jsonText[j]
                            if (keyEscaped) {
                                keyEscaped = false
                            } else if (kc == '\\') {
                                keyEscaped = true
                                simpleKey = false
                            } else if (kc == '"') {
                                break
                            }
                            j++
                        }
                        if (j >= len) return false
                        var k = j + 1
                        while (k < len && jsonText[k].isWhitespace()) k++
                        if (k < len && jsonText[k] == ':') {
                            if (simpleKey) {
                                val keyLen = j - keyStart
                                if ((keyLen == 8 && jsonText.regionMatches(keyStart, "profiles", 0, 8)) ||
                                    (keyLen == 10 && jsonText.regionMatches(keyStart, "assistants", 0, 10)) ||
                                    (keyLen == 8 && jsonText.regionMatches(keyStart, "sessions", 0, 8))
                                ) {
                                    return true
                                }
                            }
                            expectingKey = false
                        }
                        i = j + 1
                        continue
                    } else {
                        inString = true
                    }
                }
                '{' -> braceDepth++
                '}' -> {
                    braceDepth--
                    expectingKey = false
                }
                '[' -> bracketDepth++
                ']' -> bracketDepth = (bracketDepth - 1).coerceAtLeast(0)
                ',' -> if (braceDepth == 1 && bracketDepth == 0) expectingKey = true
                ':' -> if (braceDepth == 1 && bracketDepth == 0) expectingKey = false
            }
            i++
        }
        return false
    }

    private fun validChat(raw: String, sessionId: String): Boolean = runCatching {
        json.decodeFromString<ChatIdentity>(raw).id == sessionId
    }.getOrDefault(false)

    private fun read(file: File): String? = runCatching {
        if (file.isFile) AtomicFileStorage.readText(file) else null
    }.getOrNull()

    private fun isInitialSettings(state: AppStateSnapshot): Boolean = state.profiles.none { it.apiKey.isNotBlank() || it.defaultModel.isNotBlank() } &&
        state.assistants.none { it.id != "asst-default" || it.systemPrompt.isNotBlank() } &&
        state.sessions.none { it.messages.isNotEmpty() } &&
        state.savedCards.isEmpty() &&
        state.systemPromptPresets.isEmpty() &&
        state.globalWorldBook.isEmpty() &&
        state.preferences == PreferencesConfig() &&
        state.defaultModels == DefaultModelConfig() &&
        state.theme == ThemeConfig() &&
        state.activeSessionId in setOf("", "sess-initial")

    private fun settingsCandidates(): List<File> {
        val root = directory()
        val backups = File(root, "backups")
        return listOf(
            File(root, "parchment-settings.json"), File(root, "parchment-settings.json.tmp"),
            File(backups, "parchment-settings.prev.json"), File(backups, "parchment-settings.user-backup.json"),
            File(root, "parchment-settings.prev.json"), File(root, "parchment-settings.user-backup.json"),
            File(root, "parchment-state.json"), File(root, "parchment-state.json.tmp"),
            File(root, "parchment-state.prev.json"), File(root, "parchment-state.user-backup.json")
        ) + extraLegacySettings()
    }

    @Synchronized
    fun loadSettings(): String? {
        val candidates = settingsCandidates()
        var partial: String? = null
        for (candidate in candidates) {
            val raw = read(candidate) ?: continue
            if (raw.isBlank()) continue
            if (partial == null) partial = raw
            val decoded = decodeSettings(raw) ?: continue
            if (isInitialSettings(decoded)) {
                for (backup in candidates.filter { it.name.endsWith("user-backup.json") }) {
                    val backupRaw = read(backup) ?: continue
                    val backupState = decodeSettings(backupRaw) ?: continue
                    if (!isInitialSettings(backupState)) return backupRaw
                }
            }
            return raw
        }
        // Let shared partial-JSON recovery inspect the original without replacing it with defaults.
        return partial
    }

    @Synchronized
    fun saveSettings(raw: String) {
        val incoming = decodeSettings(raw) ?: throw IOException("Settings snapshot is invalid.")
        val root = directory()
        val target = File(root, "parchment-settings.json")
        val backups = File(root, "backups")
        val existingRaw = read(target)
        if (target.isFile && existingRaw == null) throw IOException("Unable to read the existing settings snapshot.")
        val existing = existingRaw?.let(::decodeSettings)
        if (isInitialSettings(incoming) &&
            settingsCandidates().any { candidate ->
                val content = read(candidate)
                content != null && content.isNotBlank() && (decodeSettings(content)?.let { !isInitialSettings(it) } ?: true)
            }
        ) {
            throw IOException("Existing settings need recovery; refusing to replace them with initial defaults.")
        }
        if (existingRaw != null && existing == null) {
            val corrupt = File(backups, "parchment-settings.corrupt.json")
            if (!corrupt.exists()) AtomicFileStorage.copy(target, corrupt)
        }
        AtomicFileStorage.writeText(
            target,
            raw,
            previousBackup = File(backups, "parchment-settings.prev.json").takeIf { existing != null }
        )
        if (!isInitialSettings(incoming)) AtomicFileStorage.copy(target, File(backups, "parchment-settings.user-backup.json"))
        // Legacy embedded state stays available until every chat has been migrated by the shared writer.
        for (name in listOf("parchment-settings.prev.json", "parchment-settings.user-backup.json")) {
            val source = File(root, name)
            val destination = File(backups, name)
            if (source.isFile && !destination.exists()) AtomicFileStorage.copy(source, destination)
            if (destination.isFile) deleteFile(source)
        }
    }

    private fun chatFolder(sessionId: String): File {
        val root = File(directory(), "chats").canonicalFile
        val safeId = sessionId.trim().replace(unsafeSessionCharacters, "_")
        require(safeId.isNotEmpty() && safeId != "." && safeId != "..") { "Invalid chat identifier." }
        val folder = File(root, safeId)
        check(folder.canonicalFile.toPath().startsWith(root.toPath()) && folder.canonicalFile == folder.absoluteFile) {
            "Chat directory must remain inside application storage."
        }
        return folder
    }

    fun chatDirectory(sessionId: String): File = chatFolder(sessionId)

    @Synchronized
    fun loadChat(sessionId: String): String? {
        if (sessionId.isBlank()) return null
        val folder = chatFolder(sessionId)
        val target = File(folder, "chat.json")
        val legacy = File(folder.parentFile, folder.name + ".json")
        val legacyTemp = File(folder.parentFile, folder.name + ".json.tmp")
        val current = read(target)?.takeIf { validChat(it, sessionId) }
        val legacyRaw = read(legacy)?.takeIf { validChat(it, sessionId) }
        if (legacyRaw != null && (current == null || legacy.lastModified() > target.lastModified())) {
            // Failure retains the migration source and still makes its complete content readable.
            runCatching { saveChat(sessionId, legacyRaw) }
            return legacyRaw
        }
        if (current != null) return foldLiveTurn(sessionId, folder, current)
        for (candidate in listOf(File(folder, "chat.json.tmp"), File(folder, "chat.prev.json"), legacyTemp)) {
            val raw = read(candidate)?.takeIf { validChat(it, sessionId) } ?: continue
            return raw
        }
        return null
    }

    @Synchronized
    fun loadAllChats(): List<String> = chats().toList()

    /**
     * Every stored chat, read one at a time so a caller that decodes and drops each one never holds them all.
     * Finding a folder's real id decodes only the id; loadChat still validates the full chat.
     */
    fun chats(skipFolders: Set<String> = emptySet()): Sequence<String> = sequence {
        val chats = File(directory(), "chats")
        val ids = chats.listFiles().orEmpty().mapNotNull { entry ->
            when {
                entry.isDirectory -> entry.name
                entry.name.endsWith(".json.tmp") -> entry.name.removeSuffix(".json.tmp")
                entry.name.endsWith(".json") -> entry.name.removeSuffix(".json")
                else -> null
            }
        }.distinct().filterNot { it in skipFolders }
        for (id in ids) {
            val loaded = runCatching {
                val folder = File(chats, id)
                val primary = File(folder, "chat.json")
                val legacy = File(chats, "$id.json")
                // Fast path: healthy chats with no pending legacy file or live reply validate in a single read/parse pass.
                if (!legacy.isFile && !File(folder, LIVE_TURN_FILE).isFile) {
                    val fastRaw = read(primary)
                    if (fastRaw != null) {
                        val parsed = runCatching { json.decodeFromString<ChatIdentity>(fastRaw) }.getOrNull()
                        if (parsed != null &&
                            parsed.id.isNotBlank() &&
                            parsed.id.trim().replace(unsafeSessionCharacters, "_") == id
                        ) {
                            return@runCatching fastRaw
                        }
                    }
                }
                val candidates = listOf(
                    primary,
                    File(folder, "chat.json.tmp"),
                    File(folder, "chat.prev.json"),
                    legacy,
                    File(chats, "$id.json.tmp")
                )
                val actualId = candidates.firstNotNullOfOrNull { candidate ->
                    read(candidate)?.let { raw -> runCatching { json.decodeFromString<ChatIdentity>(raw).id }.getOrNull() }
                        ?.takeIf { it.isNotBlank() && it.trim().replace(unsafeSessionCharacters, "_") == id }
                } ?: return@runCatching null
                loadChat(actualId)
            }.getOrNull()
            if (loaded != null) yield(loaded)
        }
    }

    fun chatFolderName(sessionId: String): String = sessionId.trim().replace(unsafeSessionCharacters, "_")

    /**
     * One attribute read per chat folder (no file contents). A folder that still has a legacy `<id>.json` beside it,
     * or a live reply left by a crash, is left out, so it goes through [chats] and [loadChat], which finish it.
     */
    fun chatFileStamps(): Map<String, ChatFileStamp> = runCatching {
        val chats = File(directory(), "chats")
        val entries = chats.listFiles().orEmpty()
        val names = entries.mapTo(HashSet()) { it.name }
        buildMap {
            for (entry in entries) {
                if ("${entry.name}.json" in names || "${entry.name}.json.tmp" in names) continue
                if (File(entry, LIVE_TURN_FILE).isFile) continue
                val attributes = runCatching {
                    Files.readAttributes(File(entry, "chat.json").toPath(), BasicFileAttributes::class.java)
                }.getOrNull() ?: continue
                if (!attributes.isRegularFile || attributes.size() <= 0L) continue
                put(entry.name, ChatFileStamp(attributes.size(), attributes.lastModifiedTime().toMillis()))
            }
        }
    }.getOrDefault(emptyMap())

    private fun chatIndexFile(): File = File(directory(), "chat-index.json")

    fun loadChatIndex(): String? = read(chatIndexFile())

    @Synchronized
    fun saveChatIndex(raw: String) = AtomicFileStorage.writeText(chatIndexFile(), raw)

    @Synchronized
    fun saveChat(sessionId: String, raw: String) {
        require(validChat(raw, sessionId)) { "Chat snapshot does not match its identifier." }
        val folder = chatFolder(sessionId)
        val target = File(folder, "chat.json")
        val existing = read(target)
        if (target.isFile && existing == null) throw IOException("Unable to read the existing chat snapshot.")
        val existingSession = existing?.let { runCatching { json.decodeFromString<ChatIdentity>(it) }.getOrNull() }
        if (existingSession != null && existingSession.id != sessionId) {
            throw IOException("Chat identifier collides with an existing storage path.")
        }
        val validExisting = existingSession?.id == sessionId
        if (existing != null && !validExisting) {
            val corrupt = File(folder, "chat.corrupt.json")
            if (!corrupt.exists()) AtomicFileStorage.copy(target, corrupt)
        }
        AtomicFileStorage.writeText(target, raw, previousBackup = File(folder, "chat.prev.json").takeIf { validExisting })
        // The full chat now holds the reply; a sidecar that cannot be removed is recognized as stale by its base.
        runCatching { deleteFile(File(folder, LIVE_TURN_FILE)) }
        deleteFile(File(folder.parentFile, folder.name + ".json"))
        deleteFile(File(folder.parentFile, folder.name + ".json.tmp"))
    }

    /**
     * A streaming reply's newest checkpoint, saved beside its chat (`live.json`) instead of rewriting a chat.json
     * that can be megabytes. [raw] is `{"nodeId":...,"variant":{...}}`; the file also records the chat.json it
     * extends (size and modification time), so it is only ever folded into that exact file. The next full save
     * deletes it; [loadChat] folds a sidecar left by a crash into chat.json. Nothing is written for a chat that
     * has no chat.json yet: there is nothing for the reply to extend.
     */
    @Synchronized
    fun saveLiveTurn(sessionId: String, raw: String) {
        val folder = chatFolder(sessionId)
        val base = stampOf(File(folder, "chat.json")) ?: return
        AtomicFileStorage.writeText(
            File(folder, LIVE_TURN_FILE),
            "{\"chatLength\":${base.length},\"chatModifiedMs\":${base.lastModifiedMs},\"turn\":$raw}"
        )
    }

    private fun stampOf(file: File): ChatFileStamp? = runCatching {
        val attributes = Files.readAttributes(file.toPath(), BasicFileAttributes::class.java)
        if (attributes.isRegularFile) ChatFileStamp(attributes.size(), attributes.lastModifiedTime().toMillis()) else null
    }.getOrNull()

    /**
     * Returns [chatRaw] with a crash-surviving live reply folded in, and saves the result so every reader sees the
     * same text. A sidecar written against another chat.json, unreadable, or naming a reply the chat no longer has
     * is dropped. If saving the fold fails, the folded text is still returned and the sidecar kept for next time.
     */
    private fun foldLiveTurn(sessionId: String, folder: File, chatRaw: String): String {
        val sidecar = File(folder, LIVE_TURN_FILE)
        if (!sidecar.isFile) return chatRaw
        val base = stampOf(File(folder, "chat.json"))
        val folded = read(sidecar)?.let { raw -> runCatching { withLiveTurn(chatRaw, raw, base) }.getOrNull() }
        if (folded == null) {
            runCatching { deleteFile(sidecar) }
            return chatRaw
        }
        runCatching { saveChat(sessionId, folded) }
        return folded
    }

    private fun withLiveTurn(chatRaw: String, sidecarRaw: String, base: ChatFileStamp?): String? {
        val sidecar = json.parseToJsonElement(sidecarRaw).jsonObject
        val writtenAgainst = ChatFileStamp(
            sidecar["chatLength"]?.jsonPrimitive?.longOrNull ?: return null,
            sidecar["chatModifiedMs"]?.jsonPrimitive?.longOrNull ?: return null
        )
        if (writtenAgainst != base) return null
        val turn = sidecar["turn"]?.jsonObject ?: return null
        val nodeId = turn["nodeId"]?.jsonPrimitive?.contentOrNull ?: return null
        val variant = turn["variant"] as? JsonObject ?: return null
        val variantId = variant["id"]?.jsonPrimitive?.contentOrNull ?: return null
        val chat = json.parseToJsonElement(chatRaw).jsonObject
        var replaced = false
        val messages = (chat["messages"] as? JsonArray ?: return null).map { node ->
            val nodeObject = node as? JsonObject
            val variants = nodeObject?.get("variants") as? JsonArray
            if (nodeObject?.get("id")?.jsonPrimitive?.contentOrNull != nodeId || variants == null) return@map node
            JsonObject(
                nodeObject + (
                    "variants" to JsonArray(
                        variants.map { existing ->
                            if ((existing as? JsonObject)?.get("id")?.jsonPrimitive?.contentOrNull == variantId) {
                                replaced = true
                                variant
                            } else existing
                        }
                    )
                    )
            )
        }
        if (!replaced) return null
        return json.encodeToString(JsonObject.serializer(), JsonObject(chat + ("messages" to JsonArray(messages))))
    }

    @Synchronized
    fun deleteChat(sessionId: String) {
        if (sessionId.isBlank()) return
        val folder = chatFolder(sessionId)
        AtomicFileStorage.deleteTree(folder.toPath())
        deleteFile(File(folder.parentFile, folder.name + ".json"))
        deleteFile(File(folder.parentFile, folder.name + ".json.tmp"))
    }

    private fun deleteFile(file: File) {
        if (file.exists() && !file.delete()) throw IOException("Unable to remove obsolete snapshot: " + file.name)
    }

    private companion object {
        const val LIVE_TURN_FILE = "live.json"
    }
}
