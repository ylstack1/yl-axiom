package io.parchment.ai.ui

import androidx.compose.runtime.snapshots.Snapshot
import io.parchment.ai.strings.Strings
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import io.parchment.ai.handleUnconsumedWindowKey
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Shortcuts must work with nothing focused: at launch and after a click on empty space (2026-11-12 regression). */
@OptIn(ExperimentalComposeUiApi::class, InternalComposeUiApi::class)
class KeyboardShortcutTest {
    private var frame = 0L

    private suspend fun settle(scene: ImageComposeScene, frames: Int = 40) = repeat(frames) {
        Snapshot.sendApplyNotifications()
        frame += 16_000_000L
        scene.render(frame).close()
        delay(5)
    }

    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun anythingFocused(scene: ImageComposeScene) =
        nodes(scene).any { it.config.getOrNull(SemanticsProperties.Focused) == true }

    private fun paletteOpen(scene: ImageComposeScene) =
        nodes(scene).any { node -> node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.typeACommandOrA } }

    /** What the desktop window does with a key: the focused element first, then the window's own fallback. */
    private suspend fun pressInWindow(scene: ImageComposeScene, shortcuts: AppShortcutDispatcher, key: Key, ctrl: Boolean) {
        for (type in listOf(KeyEventType.KeyDown, KeyEventType.KeyUp)) {
            val event = KeyEvent(key, type, isCtrlPressed = ctrl)
            scene.sendKeyEvent(event) || handleUnconsumedWindowKey(event, shortcuts)
        }
        settle(scene)
    }

    @Test fun ctrlKOpensThePaletteWithNothingFocused() = runBlocking<Unit>(Dispatchers.Swing) {
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState().copy(autoCheckOtaOnStartup = false)))
        parchmentProcessState.snapshot.value = loadFastInitialAppState().first
        val shortcuts = AppShortcutDispatcher()
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp(shortcutDispatcher = shortcuts) }
            settle(scene, 120)
            assertFalse(anythingFocused(scene), "The test needs a window where nothing has focus, as at launch")
            assertFalse(paletteOpen(scene))

            pressInWindow(scene, shortcuts, Key.K, ctrl = true)
            assertTrue(paletteOpen(scene), "Ctrl+K must open the command palette when nothing has focus")
            val openClickables = nodes(scene).filter { it.config.contains(SemanticsActions.OnClick) }
            assertTrue(
                openClickables.none { node ->
                    node.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
                        .any { it == Strings.closeSidebar || it == Strings.openSidebar || it == Strings.chatNotes }
                },
                "Background chrome behind the command palette must be hidden from screen readers"
            )
            val selectedBeforeDown = openClickables.filter { it.config.getOrNull(SemanticsProperties.Selected) == true }
            assertEquals(1, selectedBeforeDown.size, "Highlighted command palette row must expose selected semantics")
            val firstSelectedText = selectedBeforeDown.single().config.getOrNull(SemanticsProperties.Text).orEmpty().first().text
            pressInWindow(scene, shortcuts, Key.DirectionDown, ctrl = false)
            val selectedAfterDown = nodes(scene).filter {
                it.config.contains(SemanticsActions.OnClick) && it.config.getOrNull(SemanticsProperties.Selected) == true
            }
            assertEquals(1, selectedAfterDown.size)
            assertFalse(
                selectedAfterDown.single().config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == firstSelectedText },
                "Arrow down must advance command palette selected semantics"
            )

            assertTrue(PlatformRuntime.handleSystemBackPressed(), "System back must dismiss the command palette")
            settle(scene)
            assertFalse(paletteOpen(scene), "Back handler must close the palette")

            pressInWindow(scene, shortcuts, Key.K, ctrl = true)
            assertTrue(paletteOpen(scene))

            pressInWindow(scene, shortcuts, Key.Escape, ctrl = false)
            assertFalse(paletteOpen(scene), "Escape must still close the palette")
        } finally {
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }

    private fun promptJumpOpen(scene: ImageComposeScene) =
        nodes(scene).any { node -> node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.jumpToPromptPlaceholder } }

    @Test fun ctrlGOpensThePromptJumpAndEscapeClosesIt() = runBlocking<Unit>(Dispatchers.Swing) {
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState().copy(autoCheckOtaOnStartup = false)))
        parchmentProcessState.snapshot.value = loadFastInitialAppState().first
        val shortcuts = AppShortcutDispatcher()
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp(shortcutDispatcher = shortcuts) }
            settle(scene, 120)
            pressInWindow(scene, shortcuts, Key.G, ctrl = true)
            assertTrue(promptJumpOpen(scene), "Ctrl+G must open the prompt jump list")
            assertFalse(paletteOpen(scene), "the prompt jump replaces, not stacks on, the command palette")
            pressInWindow(scene, shortcuts, Key.K, ctrl = true)
            assertTrue(paletteOpen(scene) && !promptJumpOpen(scene), "Ctrl+K switches to the command palette")
            assertTrue(
                nodes(scene).any { node -> node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.closeThisTab } },
                "Every shortcut has a palette row; Ctrl+W must too"
            )
            pressInWindow(scene, shortcuts, Key.G, ctrl = true)
            assertTrue(promptJumpOpen(scene) && !paletteOpen(scene), "Ctrl+G switches back")
            pressInWindow(scene, shortcuts, Key.Escape, ctrl = false)
            assertFalse(promptJumpOpen(scene), "Escape must close the prompt jump list")
        } finally {
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }
}
