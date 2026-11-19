package io.parchment.ai.ui

import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.WorkspaceMode
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ParchmentHydrationPersistenceTest {
    private fun full(id: String) = ChatSession(
        id = id,
        title = "original",
        createdAt = 1L,
        updatedAt = 1L,
        profileId = "profile",
        storySummary = "Complete continuity memory",
        authorsNote = "Keep the author note",
        messages = listOf(
            MessageNode(
                id = "message",
                role = MessageRole.ASSISTANT,
                variants = listOf(MessageVariant(id = "variant", content = "complete content ".repeat(50)))
            )
        )
    )

    @Test
    fun chatIndexServesUnchangedStubsAndRereadsEverythingElse() {
        val stamp = PlatformRuntime.currentTimeMillis()
        val ids = List(3) { "index-test-$it-$stamp" }
        val base = AppStateSnapshot().copy(activeSessionId = "no-such-chat-$stamp", sessions = emptyList())
        fun titleOf(state: AppStateSnapshot, id: String) = state.sessions.first { it.id == id }.title
        fun chatFile(id: String) = java.io.File(PlatformStorage.getChatsStorageDirectoryPath(), PlatformStorage.chatFolderName(id) + "/chat.json")

        /** Same length and modification time: only the index can tell the old content apart. */
        fun swapTitleKeepingStamp(id: String, from: String, to: String) {
            val file = chatFile(id)
            val modified = file.lastModified()
            file.writeText(file.readText().replace(from, to))
            check(file.setLastModified(modified))
        }
        try {
            ids.forEach { saveSingleChatSessionToDisk(full(it)) }
            val first = loadRemainingHistoricalSessionsDehydrated(base)
            ids.forEach { id -> assertTrue(first.sessions.first { it.id == id }.isCompactStub, "inactive chats load as stubs") }
            val index = PlatformStorage.loadChatIndexJson().orEmpty()
            ids.forEach { assertTrue(it in index, "the first load records $it in chat-index.json") }

            swapTitleKeepingStamp(ids[0], "original", "ORIGINAL")
            saveSingleChatSessionToDisk(full(ids[1]).copy(title = "renamed"))
            PlatformStorage.deleteChatSessionFile(ids[2])
            val second = loadRemainingHistoricalSessionsDehydrated(base)
            assertEquals("original", titleOf(second, ids[0]), "an unchanged stamp is served from the index without reading the chat")
            assertEquals("renamed", titleOf(second, ids[1]), "a rewritten chat is read again")
            assertTrue(second.sessions.none { it.id == ids[2] }, "a deleted chat disappears")
            assertFalse(ids[2] in PlatformStorage.loadChatIndexJson().orEmpty())

            val active = loadRemainingHistoricalSessionsDehydrated(base.copy(activeSessionId = ids[0]))
            assertEquals("ORIGINAL", titleOf(active, ids[0]), "the open chat is always read in full")
            assertFalse(active.sessions.first { it.id == ids[0] }.isCompactStub)

            PlatformStorage.saveChatIndexJson("{not json")
            assertEquals("ORIGINAL", titleOf(loadRemainingHistoricalSessionsDehydrated(base), ids[0]), "a corrupt index falls back")

            swapTitleKeepingStamp(ids[0], "ORIGINAL", "original")
            val current = PlatformStorage.loadChatIndexJson().orEmpty()
            assertTrue("\"stubFormat\":$CHAT_STUB_FORMAT" in current)
            PlatformStorage.saveChatIndexJson(current.replace("\"stubFormat\":$CHAT_STUB_FORMAT", "\"stubFormat\":0"))
            assertEquals("original", titleOf(loadRemainingHistoricalSessionsDehydrated(base), ids[0]), "an index of another stub format is ignored")
        } finally {
            ids.forEach { PlatformStorage.deleteChatSessionFile(it) }
        }
    }

    @Test
    fun everyMessageInAStubHasAUniqueId() {
        val messages = List(5) { MessageNode(id = "real-$it", role = MessageRole.USER, variants = listOf(MessageVariant(id = "v$it", content = "text"))) }
        val stub = dehydrateChatSessionToCompactStub(full("stub-ids").copy(messages = messages))
        assertEquals(5, stub.displayMessageCount)
        assertEquals(1, stub.messages.size)
        assertEquals(stub.messages.size, stub.messages.map { it.id }.toSet().size)
        assertTrue(stub.messages.none { it.id.startsWith("stub-node-") })
    }

    @Test
    fun queuedFullSnapshotWritesDespiteCurrentSessionDehydration() {
        val id = "test-full-after-stub-" + PlatformRuntime.currentTimeMillis()
        val original = full(id)
        try {
            val stub = dehydrateChatSessionToCompactStub(original)
            assertTrue(stub.isCompactStub)
            saveSingleChatSessionToDisk(original)
            val loaded = compactStateJson.decodeFromString<ChatSession>(PlatformStorage.loadChatSessionJsonById(id)!!)
            assertEquals(original.messages, loaded.messages)
            assertEquals(original.storySummary, loaded.storySummary)
            assertEquals(original.authorsNote, loaded.authorsNote)
        } finally {
            PlatformStorage.deleteChatSessionFile(id)
        }
    }

    @Test
    fun capturedStubCannotOverwriteHistoryAfterAnotherSnapshotHydrates() {
        val id = "test-stale-stub-" + PlatformRuntime.currentTimeMillis()
        val original = full(id)
        try {
            saveSingleChatSessionToDisk(original)
            val stub = dehydrateChatSessionToCompactStub(original).copy(title = "renamed")
            assertFalse(hydrateSessionFromDiskIfNeeded(stub).isCompactStub)
            saveSingleChatSessionToDisk(stub)
            val loaded = compactStateJson.decodeFromString<ChatSession>(PlatformStorage.loadChatSessionJsonById(id)!!)
            assertEquals("renamed", loaded.title)
            assertEquals(original.messages, loaded.messages)
            assertEquals(original.storySummary, loaded.storySummary)
        } finally {
            PlatformStorage.deleteChatSessionFile(id)
        }
    }

    @Test
    fun compactMarkerIsNotPersistedOrImported() {
        val original = full("test-transient")
        val stub = dehydrateChatSessionToCompactStub(original)
        val encoded = compactStateJson.encodeToString(ChatSession.serializer(), stub)
        assertFalse(encoded.contains("isCompactStub"))
        assertFalse(compactStateJson.decodeFromString<ChatSession>(encoded).isCompactStub)
        assertFalse(compactStateJson.decodeFromString<ChatSession>(encoded.dropLast(1) + ",\"isCompactStub\":true}").isCompactStub)
    }

    @Test
    fun pendingFullContentSurvivesStubMetadataUpdate() = runBlocking {
        val scope = CoroutineScope(SupervisorJob())
        val original = full("test-pending-metadata")
        var saved: ChatSession? = null
        try {
            val writer = ParchmentDiskWriter(scope, {}, { saved = it }, {}, debounceMs = 60_000)
            writer.enqueue(sessions = listOf(original), notify = false)
            val stub = dehydrateChatSessionToCompactStub(original).copy(title = "renamed")
            writer.enqueue(sessions = listOf(stub), notify = false)
            writer.flush()
            assertEquals("renamed", saved?.title)
            assertEquals(original.messages, saved?.messages)
            assertEquals(original.authorsNote, saved?.authorsNote)
            assertFalse(saved!!.isCompactStub)
        } finally {
            scope.cancel()
        }
    }

    @Test
    fun restoreDoesNotDiscardPendingFullHistoryForAStub() = runBlocking {
        val scope = CoroutineScope(SupervisorJob())
        val original = full("test-restore-stub")
        var saved: ChatSession? = null
        try {
            val writer = ParchmentDiskWriter(scope, {}, { saved = it }, {}, debounceMs = 60_000)
            writer.enqueue(sessions = listOf(original), notify = false)
            val stub = dehydrateChatSessionToCompactStub(original).copy(title = "restored metadata")
            writer.replaceState(AppStateSnapshot(sessions = listOf(stub)), emptySet())
            writer.flush()
            assertEquals("restored metadata", saved?.title)
            assertEquals(original.messages, saved?.messages)
        } finally {
            scope.cancel()
        }
    }

    @Test
    fun hydrationUsesTheUnsavedFullSnapshotBeforeDisk() {
        val id = "test-pending-hydrate-" + PlatformRuntime.currentTimeMillis()
        val original = full(id)
        try {
            appDiskWriter.enqueue(sessions = listOf(original), notify = false)
            val stub = dehydrateChatSessionToCompactStub(original).copy(title = "new title")
            val hydrated = hydrateSessionFromDiskIfNeeded(stub)
            assertFalse(hydrated.isCompactStub)
            assertEquals(original.messages, hydrated.messages)
            assertEquals("new title", hydrated.title)
        } finally {
            appDiskWriter.enqueue(deletedIds = setOf(id), urgent = true)
            appDiskWriter.flushBlocking()
            PlatformStorage.deleteChatSessionFile(id)
        }
    }

    @Test
    fun failedHydrationDoesNotPretendThePreviewIsComplete() {
        val id = "test-missing-hydrate-" + PlatformRuntime.currentTimeMillis()
        val stub = dehydrateChatSessionToCompactStub(full(id))
        val result = hydrateSessionFromDiskIfNeeded(stub)
        assertTrue(result.isCompactStub)
        assertFailsWith<IllegalStateException> { saveSingleChatSessionToDisk(stub) }
        assertFailsWith<IllegalStateException> { chatBundleEntry(stub) }
        assertFailsWith<IllegalStateException> { encodeChatSessionJsonForExport(stub) }
    }

    @Test
    fun hydrationPreservesClearedMetadataAndDrafts() {
        val id = "test-cleared-metadata-" + PlatformRuntime.currentTimeMillis()
        val original = full(id).copy(
            workspacePath = "previous-root",
            workspaceMode = WorkspaceMode.READ_WRITE,
            modelOverride = "previous-model",
            systemPrompt = "previous instructions"
        )
        try {
            saveSingleChatSessionToDisk(original)
            val edited = dehydrateChatSessionToCompactStub(original).copy(
                workspacePath = "",
                workspaceMode = WorkspaceMode.OFF,
                modelOverride = "",
                systemPrompt = "",
                draftInput = "new draft",
                updatedAt = 2L
            )
            val hydrated = hydrateSessionFromDiskIfNeeded(edited)
            assertEquals("", hydrated.workspacePath)
            assertEquals(WorkspaceMode.OFF, hydrated.workspaceMode)
            assertEquals("", hydrated.modelOverride)
            assertEquals("", hydrated.systemPrompt)
            assertEquals("new draft", hydrated.draftInput)
            assertEquals(original.messages, hydrated.messages)
            assertEquals(original.authorsNote, hydrated.authorsNote)
        } finally {
            PlatformStorage.deleteChatSessionFile(id)
        }
    }

    @Test
    fun backgroundHydrationPreservesEditsTabsAndDeletedChats() {
        val active = full("active")
        val deleted = full("deleted")
        val before = AppStateSnapshot(sessions = listOf(active, deleted), activeSessionId = active.id)
        val edited = active.copy(title = "new title", draftInput = "typed while loading")
        val current = before.copy(sessions = listOf(edited), activeSessionId = "new-tab", openTabSessionIds = listOf("new-tab"))
        val discovered = full("historical")
        val loaded = before.copy(sessions = listOf(active, deleted, dehydrateChatSessionToCompactStub(discovered)))
        val merged = mergeLoadedSessions(current, before, loaded)
        assertEquals(edited, merged.sessions.first { it.id == active.id })
        assertFalse(merged.sessions.any { it.id == deleted.id })
        assertTrue(merged.sessions.any { it.id == discovered.id })
        assertEquals("new-tab", merged.activeSessionId)
        assertEquals(listOf("new-tab"), merged.openTabSessionIds)
    }

    @Test
    fun loadingACompactChatKeepsConcurrentMetadataChangesAndLiveContent() {
        val original = full("loading")
        val stub = dehydrateChatSessionToCompactStub(original)
        val before = AppStateSnapshot(sessions = listOf(stub), activeSessionId = stub.id)
        val current = before.copy(sessions = listOf(stub.copy(title = "renamed during read", draftInput = "draft")))
        val loaded = before.copy(sessions = listOf(original))
        val merged = mergeLoadedSessions(current, before, loaded).sessions.single()
        assertEquals(original.messages, merged.messages)
        assertEquals("renamed during read", merged.title)
        assertEquals("draft", merged.draftInput)
        val live = original.copy(messages = original.messages + original.messages.single().copy(id = "live-node"))
        assertEquals(live, mergeLoadedSessions(before.copy(sessions = listOf(live)), before, loaded).sessions.single())
    }

    @Test
    fun missingLocalAttachmentRetainsItsOnlyInlineCopy() {
        val original = full("missing-inline").let { session ->
            session.copy(
                messages = session.messages.map { node ->
                    node.copy(
                        variants = node.variants.map { variant ->
                            variant.copy(
                                attachments = listOf(
                                    PromptAttachment(
                                        id = "inline",
                                        fileName = "missing.png",
                                        mimeType = "image/png",
                                        localFilePath = "missing-file-${PlatformRuntime.currentTimeMillis()}.png",
                                        base64Data = "AAAA"
                                    )
                                )
                            )
                        }
                    )
                }
            )
        }
        assertEquals("AAAA", stripRedundantAttachmentBase64(original).messages.single().activeVariant.attachments.single().base64Data)
    }

    @Test
    fun exportHydratesFullHistoryAndPreservesPartialUsageForRecovery() {
        val id = "test-usage-export-" + PlatformRuntime.currentTimeMillis()
        val record = io.parchment.ai.model.ApiUsageRecord(
            "request",
            "response",
            "openai_responses",
            "fixture",
            "fixture-model",
            usage = io.parchment.ai.model.ProviderTokenUsage(
                inputTokens = 0L,
                cachedInputTokens = 0L,
                cacheCreationInputTokens = 0L,
                outputTokens = null,
                outputIncludesReasoning = true
            )
        )
        val original = full(id).let { session ->
            session.copy(
                messages = session.messages.map { node ->
                    node.copy(variants = node.variants.map { it.copy(apiUsageRecords = listOf(record)) })
                }
            )
        }
        try {
            saveSingleChatSessionToDisk(original)
            val exported = encodeChatSessionJsonForExport(dehydrateChatSessionToCompactStub(original)).decodeToString()
            val imported = compactStateJson.decodeFromString<ChatSession>(exported)
            assertEquals(original.messages, imported.messages)
            assertEquals(original.storySummary, imported.storySummary)
            val restored = imported.messages.single().activeVariant.apiUsageRecords.single()
            assertEquals(record, restored)
            assertFalse(restored.usage.isFinal)
            assertEquals(null, restored.usage.outputTokens)
            val profile = io.parchment.ai.model.ProviderProfile("fixture", "Fixture", io.parchment.ai.model.ProviderProtocol.OPENAI_RESPONSES)
            val ledger = io.parchment.ai.network.ProviderUsageLedger(profile, "fixture-model", listOf(restored))
            ledger.update(
                null,
                "response",
                io.parchment.ai.network.UsageDialect.OPENAI_RESPONSES,
                io.parchment.ai.model.ProviderTokenUsage(
                    inputTokens = 100L,
                    cachedInputTokens = 0L,
                    cacheCreationInputTokens = 0L,
                    reasoningTokens = 0L,
                    outputTokens = 5L,
                    outputIncludesReasoning = true,
                    isFinal = true
                )
            )
            assertEquals(1, ledger.metrics().reportedRequests)
            assertEquals(100L, ledger.metrics().inputTokens)
            assertEquals(5L, ledger.metrics().outputTokens)
        } finally {
            PlatformStorage.deleteChatSessionFile(id)
        }
    }

    @Test
    fun recoverAppStateFromPartialJsonIgnoresNonPrimitiveOpenTabEntries() {
        val raw = """
            {
              "sessions": [],
              "activeSessionId": {"corrupt": 1},
              "openTabSessionIds": ["tab-alpha", {"nested": true}, ["bad"], null, 42]
            }
        """.trimIndent()
        val recovered = recoverAppStateFromPartialJson(raw)
        assertEquals("", recovered.activeSessionId)
        assertEquals(listOf("tab-alpha", "42"), recovered.openTabSessionIds)
    }

    @Test
    fun compactStubPreservesPinnedOrderLastPreviewAndBadgeCountWithoutDummyNodes() {
        val messages = listOf(
            MessageNode("m0", MessageRole.USER, variants = listOf(MessageVariant("v0", "hello"))),
            MessageNode("p1", MessageRole.ASSISTANT, variants = listOf(MessageVariant("v1", "first pinned reply")), pinned = true),
            MessageNode("m2", MessageRole.USER, variants = listOf(MessageVariant("v2", "follow up"))),
            MessageNode("m3", MessageRole.ASSISTANT, variants = listOf(MessageVariant("v3", "intermediate reply"))),
            MessageNode("p4", MessageRole.ASSISTANT, variants = listOf(MessageVariant("v4", "second pinned reply")), pinned = true),
            MessageNode("m5", MessageRole.ASSISTANT, variants = listOf(MessageVariant("v5", "latest message preview")))
        )
        val original = full("pin-stub-compact").copy(messages = messages)
        val stub = dehydrateChatSessionToCompactStub(original)
        assertTrue(stub.isCompactStub)
        assertEquals(6, stub.displayMessageCount)
        assertEquals(3, stub.messages.size)
        assertEquals(listOf("p1", "p4", "m5"), stub.messages.map { it.id })
        assertEquals(listOf("p1", "p4"), pinnedReplies(listOf(stub)).map { it.nodeId })
        assertTrue(stub.messages.none { it.id.startsWith("stub-node-") })
        val merged = mergeSessionMetadata(original, stub.copy(title = "Updated"))
        assertFalse(merged.isCompactStub)
        assertEquals(0, merged.stubMessageCount)
        assertEquals(6, merged.displayMessageCount)
    }

    @Test
    fun urgentAsyncFlushWritesPendingStateWithoutWaitingForDebounce() = runBlocking {
        val scope = CoroutineScope(SupervisorJob())
        var saved: ChatSession? = null
        try {
            val writer = ParchmentDiskWriter(scope, {}, { saved = it }, {}, debounceMs = 60_000)
            writer.enqueue(sessions = listOf(full("test-urgent-async")), urgent = false, notify = true)
            writer.flushUrgentlyAsync()
            withTimeout(2_000) {
                while (saved == null) delay(10)
            }
            assertEquals("test-urgent-async", saved?.id)
        } finally {
            scope.cancel()
        }
    }
}
