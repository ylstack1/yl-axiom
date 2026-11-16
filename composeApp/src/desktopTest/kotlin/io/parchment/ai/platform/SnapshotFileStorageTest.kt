package io.parchment.ai.platform

import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.ui.compactStateJson
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SnapshotFileStorageTest {
    private val json = Json { encodeDefaults = true }
    private fun settings(name: String) = json.encodeToString(AppStateSnapshot.serializer(), AppStateSnapshot(
        profiles = listOf(ProviderProfile(id = "profile", name = name, protocol = ProviderProtocol.OPENAI_RESPONSES,
            apiKey = "test-only-key")), activeProfileId = "profile", activeSessionId = "chat"
    ))
    private fun chat(id: String, text: String) = json.encodeToString(ChatSession.serializer(), ChatSession(
        id = id, title = id, createdAt = 1, updatedAt = 2, profileId = "profile",
        messages = listOf(MessageNode(id = "message", role = MessageRole.ASSISTANT,
            variants = listOf(MessageVariant(id = "variant", content = text))))
    ))

    @Test
    fun corruptSettingsPrimaryRecoversWithoutDestroyingValidBackup() = withStorage { root, storage ->
        val first = settings("first")
        storage.saveSettings(first)
        storage.saveSettings(settings("second"))
        val primary = File(root, "parchment-settings.json")
        val corrupt = "{\"profiles\":["
        primary.writeText(corrupt)
        assertEquals(first, storage.loadSettings())
        storage.saveSettings(first)
        assertEquals(first, primary.readText())
        assertEquals(first, File(root, "backups/parchment-settings.prev.json").readText())
        assertEquals(corrupt, File(root, "backups/parchment-settings.corrupt.json").readText())
    }

    @Test
    fun corruptChatPrimaryRecoversWithoutReplacingItsValidBackup() = withStorage { root, storage ->
        val first = chat("chat", "完整 history")
        storage.saveChat("chat", first)
        storage.saveChat("chat", chat("chat", "second"))
        val primary = File(root, "chats/chat/chat.json")
        primary.writeText("{broken")
        assertEquals(first, storage.loadChat("chat"))
        storage.saveChat("chat", first)
        assertEquals(first, primary.readText())
        assertEquals(first, File(primary.parentFile, "chat.prev.json").readText())
        assertEquals("{broken", File(primary.parentFile, "chat.corrupt.json").readText())
    }

    @Test
    fun failedLegacyMigrationRetainsSourceAndReadableHistory() = withStorage { root, storage ->
        val blocked = File(root, "chats/legacy/chat.json")
        check(blocked.mkdirs())
        File(blocked, "blocker").writeText("preserve")
        val legacy = File(root, "chats/legacy.json")
        val raw = chat("legacy", "preserved legacy content")
        legacy.writeText(raw)
        assertEquals(raw, storage.loadChat("legacy"))
        assertEquals(raw, legacy.readText())
        assertTrue(blocked.isDirectory)
        assertEquals("preserve", File(blocked, "blocker").readText())
        assertEquals(raw, File(blocked.parentFile, "chat.json.tmp").readText())
    }

    @Test
    fun newerCorruptLegacyFileCannotReplaceAValidChat() = withStorage { root, storage ->
        val valid = chat("chat", "valid history")
        storage.saveChat("chat", valid)
        val legacy = File(root, "chats/chat.json")
        legacy.writeText("{broken newer file")
        check(legacy.setLastModified(System.currentTimeMillis() + 60_000))
        assertEquals(valid, storage.loadChat("chat"))
        assertEquals(valid, File(root, "chats/chat/chat.json").readText())
        assertTrue(legacy.exists())
    }

    @Test
    fun successfulMigrationDeletesOnlyAfterACompleteDestinationExists() = withStorage { root, storage ->
        val legacy = File(root, "chats/legacy.json")
        check(legacy.parentFile.mkdirs())
        val raw = chat("legacy", "legacy content")
        legacy.writeText(raw)
        assertEquals(raw, storage.loadChat("legacy"))
        assertFalse(legacy.exists())
        assertEquals(raw, File(root, "chats/legacy/chat.json").readText())
    }

    @Test
    fun nativeWriteFailurePropagatesAndPreservesStagedContent() = withStorage { root, storage ->
        val target = File(root, "parchment-settings.json")
        check(target.mkdir())
        File(target, "blocker").writeText("preserve")
        val raw = settings("new")
        assertFailsWith<IOException> { storage.saveSettings(raw) }
        assertTrue(target.isDirectory)
        assertEquals(raw, File(root, "parchment-settings.json.tmp").readText())
    }

    @Test
    fun invalidAndMismatchedChatWritesCannotReplaceExistingHistory() = withStorage { root, storage ->
        val original = chat("chat", "original")
        storage.saveChat("chat", original)
        assertFailsWith<IllegalArgumentException> { storage.saveChat("chat", "{broken") }
        assertFailsWith<IllegalArgumentException> { storage.saveChat("chat", "{\"id\":\"chat\",\"messages\":[broken") }
        assertFailsWith<IllegalArgumentException> { storage.saveChat("chat", chat("different", "wrong ID")) }
        assertEquals(original, storage.loadChat("chat"))
    }

    @Test
    fun compactDefaultSettingsKeepStructuralAndResetMarkersForSave() = withStorage { _, storage ->
        val encoded = compactStateJson.encodeToString(AppStateSnapshot.serializer(), AppStateSnapshot())
        assertTrue("\"profiles\":[]" in encoded)
        assertTrue("\"assistants\":[]" in encoded)
        assertTrue("\"providerStorageReset\":true" in encoded)
        storage.saveSettings(encoded)
        assertEquals(encoded, storage.loadSettings())
    }

    @Test
    fun staleTempPruningCleansOldArtifactsAndKeepsSoleRecoverySnapshots() {
        val root = Files.createTempDirectory("parchment-prune-test-").toFile()
        val otaDir = Files.createTempDirectory("parchment-prune-ota-").toFile()
        try {
            val oldMs = 1_000_000L
            val nowMs = oldMs + 48L * 60L * 60L * 1000L
            val freshMs = nowMs - 60_000L

            val staleOta = File(otaDir, "old-setup.exe").apply { writeText("old"); setLastModified(oldMs) }
            val freshOta = File(otaDir, "fresh-setup.exe").apply { writeText("new"); setLastModified(freshMs) }
            val intactPrimary = File(root, "parchment-settings.json").apply { writeText("{}"); setLastModified(nowMs) }
            val orphanTmpBesidePrimary = File(root, "parchment-settings.json.tmp").apply { writeText("{old"); setLastModified(oldMs) }
            val subChatDir = File(root, "chats/sole-tmp").apply { mkdirs() }
            val soleRecoveryTmp = File(subChatDir, "chat.json.tmp").apply { writeText("""{"id":"sole-tmp"}"""); setLastModified(oldMs) }
            val freshTmp = File(root, "recent.tmp").apply { writeText("active"); setLastModified(freshMs) }

            val removed = AppStorageInventory.pruneStaleTempFiles(root, listOf(otaDir), nowMs = nowMs)
            assertEquals(2, removed)
            assertFalse(staleOta.exists())
            assertTrue(freshOta.exists())
            assertTrue(intactPrimary.exists())
            assertFalse(orphanTmpBesidePrimary.exists())
            assertTrue(soleRecoveryTmp.exists(), "Lone .json.tmp recovery file must survive without an intact primary .json")
            assertTrue(freshTmp.exists())
        } finally {
            AtomicFileStorage.deleteTree(root.toPath())
            AtomicFileStorage.deleteTree(otaDir.toPath())
        }
    }

    @Test
    fun initialDefaultsCannotEraseConfiguredOrUnrecoverableSettings() = withStorage { root, storage ->
        val configured = settings("configured")
        storage.saveSettings(configured)
        val defaults = json.encodeToString(AppStateSnapshot.serializer(), AppStateSnapshot())
        assertFailsWith<IOException> { storage.saveSettings(defaults) }
        assertEquals(configured, storage.loadSettings())
        File(root, "parchment-settings.json").writeText("{broken")
        assertFailsWith<IOException> { storage.saveSettings(defaults) }
        assertEquals("{broken", File(root, "parchment-settings.json").readText())
    }

    @Test
    fun sanitizedImportedIdsRemainLoadableAndCannotCollide() = withStorage { root, storage ->
        val original = chat("imported:chat", "imported content")
        val regularPrev = chat("normal-chat", "previous content")
        val regular = chat("normal-chat", "regular content")
        storage.saveChat("imported:chat", original)
        storage.saveChat("normal-chat", regularPrev)
        storage.saveChat("normal-chat", regular)
        assertEquals(setOf(original, regular), storage.loadAllChats().toSet())

        // Single-pass fast path in chats() must still recover cleanly via chat.prev.json when primary chat.json is corrupt
        File(root, "chats/normal-chat/chat.json").writeText("{corrupt")
        assertEquals(setOf(original, regularPrev), storage.loadAllChats().toSet())

        assertFailsWith<IOException> { storage.saveChat("imported/chat", chat("imported/chat", "collision")) }
        assertEquals(original, storage.loadChat("imported:chat"))
    }

    @Test
    fun dotIdentifiersCannotDeleteTheChatRoot() = withStorage { root, storage ->
        val original = chat("safe", "safe history")
        storage.saveChat("safe", original)
        assertFailsWith<IllegalArgumentException> { storage.deleteChat(".") }
        assertFailsWith<IllegalArgumentException> { storage.deleteChat("..") }
        assertEquals(original, File(root, "chats/safe/chat.json").readText())
    }

    @Test
    fun chatStampsChangeOnEveryWriteAndSkipLegacyFolders() = withStorage { root, storage ->
        storage.saveChat("a", chat("a", "first"))
        storage.saveChat("b", chat("b", "first"))
        val before = storage.chatFileStamps()
        assertEquals(setOf("a", "b"), before.keys)
        File(root, "chats/a/chat.json").setLastModified(before.getValue("a").lastModifiedMs - 5_000)
        storage.saveChat("a", chat("a", "second, longer"))
        assertTrue(storage.chatFileStamps().getValue("a") != before.getValue("a"), "a rewrite must invalidate the index entry")
        // A pending legacy file means migration still has to run through loadChat, so no stamp is offered.
        File(root, "chats/b.json").writeText(chat("b", "legacy"))
        assertFalse("b" in storage.chatFileStamps())
        // Folders served from the index are not read again.
        assertEquals(listOf("a"), storage.chats(skipFolders = setOf("b")).map { compactStateJson.decodeFromString<ChatSession>(it).id }.toList())
        storage.saveChatIndex("{\"version\":1}")
        assertEquals("{\"version\":1}", storage.loadChatIndex())
    }

    @Test
    fun indexedStubsRoundTripTheirTransientStubFields() {
        val full = ChatSession(
            id = "c", title = "Trip", createdAt = 1, updatedAt = 2, profileId = "p",
            messages = List(4) { MessageNode(id = "m$it", role = MessageRole.USER, variants = listOf(MessageVariant(id = "v$it", content = "text $it"))) }
        )
        val stub = io.parchment.ai.ui.dehydrateChatSessionToCompactStub(full)
        val entry = io.parchment.ai.ui.ChatIndexEntry.of(ChatFileStamp(10, 20), stub)
        val decoded = compactStateJson.decodeFromString(
            io.parchment.ai.ui.ChatIndexEntry.serializer(),
            compactStateJson.encodeToString(io.parchment.ai.ui.ChatIndexEntry.serializer(), entry)
        )
        val restored = decoded.restoredStub()
        assertTrue(restored.isCompactStub)
        assertEquals(4, restored.displayMessageCount)
        assertEquals(stub.messages, restored.messages)
        assertTrue(decoded.matches(ChatFileStamp(10, 20)))
        assertFalse(decoded.matches(ChatFileStamp(10, 21)))
    }

    private fun withStorage(block: (File, SnapshotFileStorage) -> Unit) {
        val root = Files.createTempDirectory("parchment-snapshot-test-").toFile()
        try {
            block(root, SnapshotFileStorage({ root }))
        } finally {
            val temporaryRoot = File(System.getProperty("java.io.tmpdir")).canonicalFile.toPath()
            check(root.canonicalFile.toPath().startsWith(temporaryRoot) && root.name.startsWith("parchment-snapshot-test-"))
            AtomicFileStorage.deleteTree(root.toPath())
        }
    }
}
