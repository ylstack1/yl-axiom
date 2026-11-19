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

    private suspend fun settle(scene: ImageComposeScene, frames: Int = 24) {
        val yieldCount = if (frames >= 60) 4 else 2
        var quietFrames = 0
        for (i in 0 until frames) {
            Snapshot.sendApplyNotifications()
            frame += 16_000_000L
            scene.render(frame).close()
            if (i < yieldCount) {
                delay(2)
            } else if (!scene.hasInvalidations()) {
                if (++quietFrames >= 2) break
            } else {
                quietFrames = 0
            }
        }
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
    private suspend fun pressInWindow(
        scene: ImageComposeScene,
        shortcuts: AppShortcutDispatcher,
        key: Key,
        ctrl: Boolean = false,
        shift: Boolean = false,
        alt: Boolean = false
    ) {
        for (type in listOf(KeyEventType.KeyDown, KeyEventType.KeyUp)) {
            val event = KeyEvent(key, type, isCtrlPressed = ctrl, isShiftPressed = shift, isAltPressed = alt)
            scene.sendKeyEvent(event) || handleUnconsumedWindowKey(event, shortcuts)
        }
        settle(scene)
    }

    private fun clickNode(node: SemanticsNode) {
        val action = node.config.getOrNull(SemanticsActions.OnClick)?.action
        assertTrue(action != null, "Expected clickable node for ${node.config}")
        action.invoke()
    }

    private fun hasText(scene: ImageComposeScene, needle: String): Boolean =
        nodes(scene).any { node -> node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == needle } }

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

    @Test fun rebindingInShortcutsSettingsUpdatesLiveDispatchCommandPaletteAndResets() = runBlocking<Unit>(Dispatchers.Swing) {
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState().copy(autoCheckOtaOnStartup = false)))
        parchmentProcessState.snapshot.value = loadFastInitialAppState().first
        val shortcuts = AppShortcutDispatcher()
        val scene = ImageComposeScene(1360, 860, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp(shortcutDispatcher = shortcuts) }
            settle(scene, 120)

            // Open Settings via Ctrl+, and navigate to Shortcuts subpage.
            pressInWindow(scene, shortcuts, Key.Comma, ctrl = true)
            assertTrue(hasText(scene, Strings.settings), "Ctrl+, must open Settings")
            val shortcutsTab = nodes(scene).first { node ->
                node.config.contains(SemanticsActions.OnClick) &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.shortcuts }
            }
            clickNode(shortcutsTab)
            settle(scene, 50)
            assertTrue(hasText(scene, Strings.appShortcuts), "Shortcuts subpage must render App Shortcuts card")

            fun rebindBtn(action: AppShortcutAction): SemanticsNode = nodes(scene).first { node ->
                node.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty()
                    .any { it == Strings.rebindShortcutLabel(action.displayTitle()) }
            }

            // 1. Escape cancels recording without closing Settings.
            clickNode(rebindBtn(AppShortcutAction.COMMAND_PALETTE))
            settle(scene, 20)
            assertTrue(hasText(scene, Strings.pressKeysPrompt), "Clicking a shortcut must enter recording mode")
            pressInWindow(scene, shortcuts, Key.Escape)
            assertFalse(hasText(scene, Strings.pressKeysPrompt), "Escape must exit recording mode")
            assertTrue(hasText(scene, Strings.appShortcuts), "Escape during recording must not close Settings")

            // 2. Plain letter and reserved Ctrl+C are rejected while keeping recording active; Ctrl+Shift+P succeeds.
            clickNode(rebindBtn(AppShortcutAction.COMMAND_PALETTE))
            settle(scene, 20)
            pressInWindow(scene, shortcuts, Key.P)
            assertTrue(hasText(scene, Strings.shortcutNeedsModifier), "Plain key without modifier must show guidance")
            assertTrue(hasText(scene, Strings.pressKeysPrompt), "Rejected key must stay in recording mode")

            pressInWindow(scene, shortcuts, Key.C, ctrl = true)
            assertTrue(hasText(scene, Strings.shortcutReserved("Ctrl+C")), "Reserved Ctrl+C must be rejected")

            pressInWindow(scene, shortcuts, Key.P, ctrl = true, shift = true)
            assertFalse(hasText(scene, Strings.pressKeysPrompt), "Valid Ctrl+Shift+P must finish recording")
            assertTrue(hasText(scene, Strings.resetAll), "Customized shortcut must surface Reset all button")

            // 3. Rebinding New chat to Ctrl+Shift+P surfaces conflict banner and Swaps cleanly.
            clickNode(rebindBtn(AppShortcutAction.NEW_CHAT))
            settle(scene, 20)
            pressInWindow(scene, shortcuts, Key.P, ctrl = true, shift = true)
            assertTrue(
                hasText(scene, Strings.shortcutConflict("Ctrl+Shift+P", AppShortcutAction.COMMAND_PALETTE.displayTitle())),
                "Assigning an already-used chord must display a conflict banner"
            )
            val swapBtn = nodes(scene).first { node ->
                node.config.contains(SemanticsActions.OnClick) &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.swapShortcuts }
            }
            clickNode(swapBtn)
            settle(scene, 30)

            // Close Settings with Ctrl+, -> Command palette is now Ctrl+N, New chat is Ctrl+Shift+P.
            pressInWindow(scene, shortcuts, Key.Comma, ctrl = true)
            assertFalse(hasText(scene, Strings.appShortcuts), "Ctrl+, must close Settings")

            pressInWindow(scene, shortcuts, Key.K, ctrl = true)
            assertFalse(paletteOpen(scene), "Old Ctrl+K must no longer open Command Palette after rebind")

            pressInWindow(scene, shortcuts, Key.N, ctrl = true)
            assertTrue(paletteOpen(scene), "Swapped Ctrl+N must now open Command Palette live")
            assertTrue(hasText(scene, "Ctrl+Shift+P"), "Command Palette must reflect rebound New chat shortcut Ctrl+Shift+P")

            pressInWindow(scene, shortcuts, Key.Escape)
            assertFalse(paletteOpen(scene))

            // 4. Reset all restores defaults immediately, and Send shortcut chips update preferences cleanly.
            pressInWindow(scene, shortcuts, Key.Comma, ctrl = true)
            val shortcutsTabAgain = nodes(scene).first { node ->
                node.config.contains(SemanticsActions.OnClick) &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.shortcuts }
            }
            clickNode(shortcutsTabAgain)
            settle(scene, 50)

            val ctrlEnterChip = nodes(scene).first { node ->
                node.config.contains(SemanticsActions.OnClick) &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == io.parchment.ai.model.SendShortcutMode.CTRL_ENTER.label }
            }
            clickNode(ctrlEnterChip)
            settle(scene, 20)
            assertTrue(
                hasText(scene, Strings.sendShortcutHint("Ctrl+Enter", "Enter")),
                "Selecting Ctrl+Enter on Shortcuts page must update send shortcut hint immediately"
            )

            val resetAllBtn = nodes(scene).first { node ->
                node.config.contains(SemanticsActions.OnClick) &&
                    node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.resetAll }
            }
            clickNode(resetAllBtn)
            settle(scene, 30)
            assertFalse(hasText(scene, Strings.resetAll), "Reset all must disappear once back on defaults")

            pressInWindow(scene, shortcuts, Key.Comma, ctrl = true)
            pressInWindow(scene, shortcuts, Key.K, ctrl = true)
            assertTrue(paletteOpen(scene), "Default Ctrl+K must open Command Palette again after Reset all")
            assertTrue(hasText(scene, "Ctrl+N"), "Command Palette must show default Ctrl+N after Reset all")
        } finally {
            scene.close()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }
}
