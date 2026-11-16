package io.parchment.ai.ui

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.network.RetryControl
import io.parchment.ai.network.RetryWait
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import java.io.File
import kotlin.test.Test
import kotlin.test.assertSame
import kotlin.test.assertTrue

/**
 * What one second of a retry countdown costs in the full app with a 200-message chat: the run's stream state takes
 * the next wait (as the engine's countdown does each second) and one frame is drawn on the CPU. The countdown must
 * leave the app state alone, so the chat and the sidebar do not redraw. Written to
 * .parchment-local/verification/retry-countdown.txt (TODO 18.5).
 */
class RetryCountdownProbeTest {
    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    @Test fun countdownTicksLeaveTheChatAloneAndOfferRetryNow() = runBlocking<Unit>(Dispatchers.Swing) {
        val history = (0 until 200).map { i ->
            val role = if (i % 2 == 0) MessageRole.USER else MessageRole.ASSISTANT
            MessageNode("r$i", role, listOf(MessageVariant("r$i-v", content = "Message $i with enough words to wrap across a line or two.")))
        }
        val streaming = MessageNode("retry-node", MessageRole.ASSISTANT, listOf(MessageVariant("retry-variant", content = "", isStreaming = true)))
        val base = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val chat = base.sessions.first().copy(title = "Retrying", messages = history + streaming)
        val state = base.copy(sessions = listOf(chat), activeSessionId = chat.id, openTabSessionIds = listOf(chat.id))
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(state.copy(sessions = emptyList())))
        PlatformStorage.saveChatSessionJson(chat.id, compactStateJson.encodeToString(chat))
        parchmentProcessState.snapshot.value = state
        val identity = AgentRunIdentity("retry-probe", chat.id, "retry-node", "retry-variant", chat.profileId, "model", "")
        fun wait(seconds: Long) = RetryWait(2, 10, seconds, "Chat Completions HTTP 503: overloaded")
        fun status(wait: RetryWait) = Strings.tryingAgainIn(wait.remainingSeconds, wait.attempt, wait.maxAttempts) + "\n" + wait.reason
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        var time = 0L
        fun frame(): Long {
            Snapshot.sendApplyNotifications()
            time += 16_000_000L
            val started = System.nanoTime()
            scene.render(time).close()
            return (System.nanoTime() - started) / 1_000
        }
        try {
            scene.setContent { ParchmentApp() }
            repeat(60) { frame(); kotlinx.coroutines.delay(5) }
            parchmentProcessState.runs.attach(identity, SessionStreamRuntime(chat.id, targetNodeId = "retry-node",
                statusText = status(wait(10)), retryWait = wait(10), retryControl = RetryControl()), cancelPrevious = false)
            repeat(30) { frame() }
            val untouched = parchmentProcessState.snapshot.value
            val ticks = (9L downTo 0L).map { seconds ->
                parchmentProcessState.runs.update(identity) { it.copy(retryWait = wait(seconds), statusText = status(wait(seconds))) }
                frame().also { kotlinx.coroutines.yield() }
            }.sorted()
            val idle = (1..30).map { frame() }.sorted()
            assertSame(untouched, parchmentProcessState.snapshot.value, "A countdown must not replace the app state")
            assertTrue(nodes(scene).any { node ->
                node.config.getOrNull(SemanticsActions.OnClick) != null &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.retryNow }
            }, "The waiting run must offer Retry now")
            fun p(list: List<Long>, q: Double) = list[((list.size - 1) * q).toInt()] / 1000.0
            val line = "retry_countdown ticks=${ticks.size} p50_ms=${"%.1f".format(p(ticks, 0.5))} " +
                "p95_ms=${"%.1f".format(p(ticks, 0.95))} idle_p50_ms=${"%.1f".format(p(idle, 0.5))}"
            println(line)
            val repo = generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile }
            File(repo, ".parchment-local/verification/retry-countdown.txt").apply { parentFile.mkdirs() }.writeText(line + "\n")
            assertTrue(p(ticks, 0.95) < 250.0, line)
        } finally {
            parchmentProcessState.runs.finish(identity, AgentRunPhase.COMPLETED)
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            PlatformStorage.deleteChatSessionFile(chat.id)
        }
    }
}
