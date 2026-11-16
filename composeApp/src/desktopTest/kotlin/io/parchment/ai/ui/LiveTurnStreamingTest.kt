package io.parchment.ai.ui

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.platform.PlatformStorage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertSame
import kotlin.test.assertTrue

class LiveTurnStreamingTest {
    private fun variant(id: String, text: String) = MessageVariant(id, content = text)

    @Test fun liveTextReplacesOnlyItsOwnReplyAndEndsCleanly() {
        val chat = ChatSession("live-chat", "Chat", 1, 1, "p", messages = listOf(
            MessageNode("u", MessageRole.USER, listOf(variant("uv", "Question"))),
            MessageNode("a", MessageRole.ASSISTANT, listOf(variant("old", "Older version"), variant("av", "Checkpoint")), activeVariantIndex = 1)
        ))
        val live = LiveTurnOverlay("live-chat", "a", "av", "Checkpoint and more", "thinking", "", "sig", 1200, 42.0)
        try {
            LiveTurns.publish(live)
            val merged = chat.withLiveTurns()
            assertEquals("Checkpoint and more", merged.messages[1].variants[1].content)
            assertEquals(42.0, merged.messages[1].variants[1].tokenMetrics.tokensPerSec)
            assertEquals("Older version", merged.messages[1].variants[0].content, "other versions keep their own text")
            assertEquals("Question", merged.messages[0].activeVariant.content)
            val otherChat = chat.copy(id = "other-chat")
            assertSame(otherChat, otherChat.withLiveTurns(), "other chats are untouched")
        } finally {
            LiveTurns.end("av")
        }
        assertEquals(chat, chat.withLiveTurns(), "after the stream ends, nothing is overlaid")
    }

    private fun texts(scene: ImageComposeScene): List<String> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
            .flatMap { it.config.getOrNull(SemanticsProperties.Text).orEmpty().map { text -> text.text } }
    }

    /**
     * The app-level reason for the overlay: before it, every streamed frame replaced the app state and recomposed the
     * app root (sidebar, tabs, header, viewport); now a frame changes only the reply's live state. Same chat, same
     * text, same frames; both distributions are written to .parchment-local/verification/live-turn-ab.txt.
     */
    @Test fun streamingThroughTheLiveOverlayLeavesTheAppStateAloneAndIsFaster() = runBlocking<Unit>(Dispatchers.Swing) {
        val reply = (1..40).joinToString("\n\n") { "Paragraph $it streams with **bold** and `code` like a normal answer." }
        val history = (0 until 200).map { i ->
            val role = if (i % 2 == 0) MessageRole.USER else MessageRole.ASSISTANT
            MessageNode("m$i", role, listOf(variant("m$i-v", "Message $i with enough words to wrap across a line or two in the chat.")))
        }
        val streaming = MessageNode("live-node", MessageRole.ASSISTANT, listOf(MessageVariant("live-variant", content = "", isStreaming = true)))
        val base = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val chat = base.sessions.first().copy(title = "Streaming", messages = history + streaming)
        val state = base.copy(sessions = listOf(chat), activeSessionId = chat.id, openTabSessionIds = listOf(chat.id))
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(state.copy(sessions = emptyList())))
        PlatformStorage.saveChatSessionJson(chat.id, compactStateJson.encodeToString(chat))
        parchmentProcessState.snapshot.value = state
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        var time = 0L
        fun frame(): Long {
            Snapshot.sendApplyNotifications()
            time += 16_000_000L
            val started = System.nanoTime()
            scene.render(time).close()
            return (System.nanoTime() - started) / 1_000
        }
        suspend fun stream(publish: (String) -> Unit): List<Long> {
            val micros = mutableListOf<Long>()
            var shown = 0
            var index = 0
            while (shown < reply.length) {
                shown = minOf(reply.length, shown + 24)
                publish(reply.substring(0, shown))
                val spent = frame()
                if (index++ >= 20) micros += spent
                kotlinx.coroutines.yield()
            }
            return micros.sorted()
        }
        try {
            scene.setContent { ParchmentApp() }
            repeat(60) { frame(); kotlinx.coroutines.delay(5) }

            // Before: each frame copies the chat into a new app state (what patchVariantMemory did per frame).
            val oldPath = stream { text ->
                val current = parchmentProcessState.snapshot.value
                parchmentProcessState.snapshot.value = current.copy(sessions = current.sessions.map { session ->
                    if (session.id != chat.id) session else session.copy(messages = session.messages.dropLast(1) +
                        streaming.copy(variants = listOf(streaming.activeVariant.copy(content = text))))
                })
            }
            parchmentProcessState.snapshot.value = state
            repeat(10) { frame() }

            // Now: each frame only updates the reply's live state.
            val untouched = parchmentProcessState.snapshot.value
            val newPath = stream { text ->
                LiveTurns.publish(LiveTurnOverlay(chat.id, "live-node", "live-variant", text, "", "", "", 0, 0.0))
            }
            repeat(5) { frame() }
            assertSame(untouched, parchmentProcessState.snapshot.value, "streaming frames must not replace the app state")
            assertTrue(texts(scene).any { "Paragraph 40 streams" in it }, "the reply must show its live text")

            fun p(sorted: List<Long>, q: Double) = sorted[((sorted.size - 1) * q).toInt()] / 1000.0
            val line = "live_turn_ab frames=${newPath.size} old_p50_ms=${"%.1f".format(p(oldPath, 0.5))} " +
                "new_p50_ms=${"%.1f".format(p(newPath, 0.5))} old_p95_ms=${"%.1f".format(p(oldPath, 0.95))} " +
                "new_p95_ms=${"%.1f".format(p(newPath, 0.95))}"
            println(line)
            val repo = generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile }
            File(repo, ".parchment-local/verification/live-turn-ab.txt").apply { parentFile.mkdirs() }.writeText(line + "\n")
            assertTrue(p(newPath, 0.5) <= p(oldPath, 0.5) * 1.1, "the overlay must not make streaming frames slower: $line")
        } finally {
            LiveTurns.end("live-variant")
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            PlatformStorage.deleteChatSessionFile(chat.id)
        }
    }
}
