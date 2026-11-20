package io.parchment.ai.ui

import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.platform.PlatformStorage
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * What one streaming checkpoint costs on disk while the last reply grows, for chats of 200 messages (about 2 MB) and
 * 1000 messages (about 10 MB): a full save (the new chat.json plus the copy of the previous one kept as
 * chat.prev.json) against a live save of the reply alone (live.json). Measured 2026-11-17 before live saves: 2 MB chat
 * p95 29.5 ms and 3.9 MB written per checkpoint, 10 MB chat p95 125 ms and 19.3 MB.
 * Written to .parchment-local/verification/checkpoint-cost.txt (TODO 18.3).
 */
class CheckpointCostProbeTest {
    private fun chat(id: String, messages: Int, bytes: Int): ChatSession {
        val sentence = "Long chats keep every word on disk, so each checkpoint pays for all of them. "
        val body = sentence.repeat(bytes / messages / sentence.length + 1)
        return ChatSession(
            id,
            "Checkpoint probe",
            1L,
            1L,
            "p",
            messages = (0 until messages).map { i ->
                MessageNode("$id-m$i", if (i % 2 == 0) MessageRole.USER else MessageRole.ASSISTANT, listOf(MessageVariant("$id-v$i", content = body)))
            }
        )
    }

    private fun streamingAt(session: ChatSession, step: Int): ChatSession {
        val last = session.messages.last()
        val reply = last.activeVariant.copy(content = "Streaming reply. ".repeat(40 * (step + 1)), isStreaming = true)
        return session.copy(messages = session.messages.dropLast(1) + last.copy(variants = listOf(reply)))
    }

    @Test fun checkpointCosts() {
        val checkpoints = 15
        val lines = listOf(200 to 2_000_000, 1000 to 10_000_000).map { (messages, bytes) ->
            val session = chat("checkpoint-probe-$messages", messages, bytes)
            val folder = File(PlatformStorage.getAppStorageDirectoryPath(), "chats/${session.id}")
            try {
                saveSingleChatSessionToDisk(session)
                val times = mutableListOf<Double>()
                var written = 0L
                repeat(checkpoints) { step ->
                    val previous = File(folder, "chat.json").length()
                    val started = System.nanoTime()
                    saveSingleChatSessionToDisk(streamingAt(session, step))
                    times += (System.nanoTime() - started) / 1e6
                    written += File(folder, "chat.json").length() + previous
                }
                val turn = session.messages.last().let { LiveTurnRef(it.id, it.activeVariant.id) }
                val liveTimes = mutableListOf<Double>()
                var liveWritten = 0L
                repeat(checkpoints) { step ->
                    val streaming = streamingAt(session, step)
                    val started = System.nanoTime()
                    saveLiveTurnToDisk(streaming, turn)
                    liveTimes += (System.nanoTime() - started) / 1e6
                    val sidecar = File(folder, "live.json").length()
                    val reply = streaming.messages.last().activeVariant.content.length
                    assertTrue(sidecar <= reply + 4096, "A live save writes the reply, not the chat: $sidecar bytes for a $reply-character reply")
                    liveWritten += sidecar
                }
                fun p(list: List<Double>, q: Double) = list.drop(3).sorted().let { it[((it.size - 1) * q).toInt()] } // Skip warm-up.
                "checkpoint messages=$messages chat_mb=${"%.1f".format(File(folder, "chat.json").length() / 1048576.0)} " +
                    "full_p50_ms=${"%.1f".format(p(times, 0.5))} full_p95_ms=${"%.1f".format(p(times, 0.95))} " +
                    "full_written_mb=${"%.1f".format(written / checkpoints / 1048576.0)} " +
                    "live_p50_ms=${"%.1f".format(p(liveTimes, 0.5))} live_p95_ms=${"%.1f".format(p(liveTimes, 0.95))} " +
                    "live_written_kb=${"%.1f".format(liveWritten / checkpoints / 1024.0)}"
            } finally {
                PlatformStorage.deleteChatSessionFile(session.id)
            }
        }
        lines.forEach(::println)
        val repo = generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile }
        File(repo, ".parchment-local/verification/checkpoint-cost.txt").apply { parentFile.mkdirs() }.writeText(lines.joinToString("\n", postfix = "\n"))
        assertEquals(2, lines.size)
    }
}
