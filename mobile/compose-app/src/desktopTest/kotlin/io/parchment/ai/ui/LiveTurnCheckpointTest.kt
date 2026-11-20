package io.parchment.ai.ui

import io.parchment.ai.harness.INTERRUPTED_REPLY_NOTICE
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.platform.PlatformStorage
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

/** Streaming checkpoints save the reply alone; a run killed between them loses nothing past its last checkpoint. */
class LiveTurnCheckpointTest {
    @Test fun aReplyCutOffByAKillComesBackFromItsLastCheckpoint() {
        val reply = MessageVariant("reply-v", content = "", isStreaming = true)
        val prompt = MessageNode("user", MessageRole.USER, listOf(MessageVariant("user-v", content = "Write a long answer")))
        val session = ChatSession("live-turn-kill", "Kill", 1L, 1L, "p", messages = listOf(prompt, MessageNode("reply", MessageRole.ASSISTANT, listOf(reply))))
        val folder = File(PlatformStorage.getAppStorageDirectoryPath(), "chats/${session.id}")
        try {
            saveSingleChatSessionToDisk(session) // A run starts from a full save of its new reply.
            val turn = LiveTurnRef("reply", "reply-v")
            listOf("First", "First words", "First words and more").forEach { text ->
                val streaming = MessageNode("reply", MessageRole.ASSISTANT, listOf(reply.copy(content = text)))
                saveLiveTurnToDisk(session.copy(messages = listOf(prompt, streaming)), turn)
            }
            // The process dies here: no final save. The next start reads the chat.
            val loaded = compactStateJson.decodeFromString(ChatSession.serializer(), checkNotNull(PlatformStorage.loadChatSessionJsonById(session.id)))
            val recovered = loaded.messages.last().activeVariant
            assertEquals("First words and more$INTERRUPTED_REPLY_NOTICE", recovered.content)
            assertFalse(recovered.isStreaming)
            assertEquals("Write a long answer", loaded.messages.first().activeVariant.content)
            assertFalse(File(folder, "live.json").exists(), "Loading folds the reply into chat.json")
        } finally {
            PlatformStorage.deleteChatSessionFile(session.id)
        }
    }
}
