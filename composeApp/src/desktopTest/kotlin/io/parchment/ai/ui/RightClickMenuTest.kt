package io.parchment.ai.ui

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.input.pointer.PointerButton
import androidx.compose.ui.input.pointer.PointerButtons
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

@OptIn(ExperimentalComposeUiApi::class)
class RightClickMenuTest {
    private var time = 0L

    private suspend fun settle(scene: ImageComposeScene, frames: Int = 20) = repeat(frames) {
        Snapshot.sendApplyNotifications()
        time += 16_000_000L
        scene.render(time).close()
        delay(5)
    }

    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun menuShows(scene: ImageComposeScene, label: String) =
        nodes(scene).any { node -> node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == label } }

    @Test fun rightClickingAChatOffersItsActions() = runBlocking<Unit>(Dispatchers.Swing) {
        val base = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val chat = base.sessions.first().copy(title = "Kyoto trip notes")
        val state = base.copy(sessions = listOf(chat), activeSessionId = chat.id, openTabSessionIds = listOf(chat.id))
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(state.copy(sessions = emptyList())))
        PlatformStorage.saveChatSessionJson(chat.id, compactStateJson.encodeToString(chat))
        parchmentProcessState.snapshot.value = state
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp() }
            settle(scene, 60)
            assertFalse(menuShows(scene, Strings.deleteChat), "no menu before the click")
            val row = nodes(scene).first { node ->
                node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == "Kyoto trip notes" } &&
                    node.boundsInRoot.left < 400f
            }
            val at = row.boundsInRoot.center
            scene.sendPointerEvent(PointerEventType.Move, at, type = PointerType.Mouse)
            scene.sendPointerEvent(PointerEventType.Press, at, type = PointerType.Mouse,
                buttons = PointerButtons(isSecondaryPressed = true), button = PointerButton.Secondary)
            scene.sendPointerEvent(PointerEventType.Release, at, type = PointerType.Mouse,
                buttons = PointerButtons(), button = PointerButton.Secondary)
            settle(scene)
            assertTrue(menuShows(scene, Strings.deleteChat), "right-click must open the chat's menu")
            assertTrue(menuShows(scene, Strings.pinChat))
        } finally {
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            PlatformStorage.deleteChatSessionFile(chat.id)
        }
    }
}
