package io.parchment.ai.ui

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.Density
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
import kotlin.test.assertTrue

/**
 * What one keystroke in the composer costs in the full app with a 200-message chat: the text changes through the
 * field's own SetText action (the path typing takes), then one frame is composed, laid out and drawn on the CPU.
 * Written to .parchment-local/verification/keystroke-frames.txt; it decides whether the composer needs its own
 * TextFieldState leaf (TODO 17.3).
 */
class KeystrokeFrameProbeTest {
    @Test fun composerKeystrokeFrameTimes() = runBlocking<Unit>(Dispatchers.Swing) {
        val history = (0 until 200).map { i ->
            val role = if (i % 2 == 0) MessageRole.USER else MessageRole.ASSISTANT
            MessageNode("k$i", role, listOf(MessageVariant("k$i-v", content = "Message $i with enough words to wrap across a line or two.")))
        }
        val base = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val chat = base.sessions.first().copy(title = "Typing", messages = history)
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
        fun composer(): SemanticsNode? {
            fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
            return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
                .lastOrNull { it.config.getOrNull(SemanticsActions.SetText) != null }
        }
        try {
            scene.setContent { ParchmentApp() }
            repeat(60) { frame(); kotlinx.coroutines.delay(5) }
            val sentence = "Could you summarize the last ten messages and list the open questions? "
            val keystrokes = mutableListOf<Long>()
            for (i in 1..sentence.length) {
                val field = composer() ?: error("composer not found")
                field.config[SemanticsActions.SetText].action?.invoke(AnnotatedString(sentence.substring(0, i)))
                val spent = frame()
                if (i > 10) keystrokes += spent
                kotlinx.coroutines.yield()
            }
            val idle = (1..30).map { frame() }.sorted()
            val sorted = keystrokes.sorted()
            fun p(list: List<Long>, q: Double) = list[((list.size - 1) * q).toInt()] / 1000.0
            val line = "keystroke frames=${sorted.size} p50_ms=${"%.1f".format(p(sorted, 0.5))} p95_ms=${"%.1f".format(p(sorted, 0.95))} " +
                "idle_p50_ms=${"%.1f".format(p(idle, 0.5))}"
            println(line)
            val repo = generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile }
            File(repo, ".parchment-local/verification/keystroke-frames.txt").apply { parentFile.mkdirs() }.writeText(line + "\n")
            assertTrue(p(sorted, 0.95) < 250.0, line)
        } finally {
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            PlatformStorage.deleteChatSessionFile(chat.id)
        }
    }
}
