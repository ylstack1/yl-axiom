package io.parchment.ai.ui

import io.parchment.ai.model.PreferencesConfig
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AppShortcutsTest {
    @Test
    fun parseFormatAndKeycapsRoundTrip() {
        val cases = listOf(
            "Ctrl+K" to ShortcutBinding("K", ctrl = true),
            "ctrl+shift+tab" to ShortcutBinding("Tab", ctrl = true, shift = true),
            "Ctrl+Comma" to ShortcutBinding(",", ctrl = true),
            "alt+f2" to ShortcutBinding("F2", alt = true),
            "F6" to ShortcutBinding("F6")
        )
        for ((raw, expected) in cases) {
            val parsed = ShortcutBinding.parse(raw)
            assertEquals(expected, parsed, "Failed to parse '$raw'")
            assertEquals(expected, ShortcutBinding.parse(expected.format()), "Round-trip failed for '$raw'")
        }
        assertEquals(listOf("Ctrl", "Shift", "Tab"), ShortcutBinding("Tab", ctrl = true, shift = true).keycapTokens())
        assertEquals(listOf("Ctrl", ","), ShortcutBinding(",", ctrl = true).keycapTokens())
        assertNull(ShortcutBinding.parse(""))
        assertNull(ShortcutBinding.parse("Ctrl+"))
        assertNull(ShortcutBinding.parse("Ctrl+A+B"))
        assertNull(ShortcutBinding.parse("Ctrl+UnknownKey"))
    }

    @Test
    fun validationProtectsTypingAndReservedEditingChords() {
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("K")))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("K", shift = true)))
        assertTrue(ShortcutBinding.isValidBinding(ShortcutBinding("F2")))
        assertTrue(ShortcutBinding.isValidBinding(ShortcutBinding("P", ctrl = true, shift = true)))
        assertTrue(ShortcutBinding.isValidBinding(ShortcutBinding("N", alt = true)))

        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("C", ctrl = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("V", ctrl = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("X", ctrl = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("A", ctrl = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("Z", ctrl = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("F4", alt = true)))
        assertFalse(ShortcutBinding.isValidBinding(ShortcutBinding("Insert", shift = true)))
    }

    @Test
    fun resolveConflictUpdateAndSwapBehaveCleanly() {
        val corrupt = mapOf(
            AppShortcutAction.COMMAND_PALETTE.id to "K",
            AppShortcutAction.NEW_CHAT.id to "Ctrl+C",
            AppShortcutAction.PROMPT_JUMP.id to "not-a-chord"
        )
        assertEquals(AppShortcutAction.COMMAND_PALETTE.defaultBinding, resolveShortcut(AppShortcutAction.COMMAND_PALETTE, corrupt))
        assertEquals(AppShortcutAction.NEW_CHAT.defaultBinding, resolveShortcut(AppShortcutAction.NEW_CHAT, corrupt))
        assertEquals(AppShortcutAction.PROMPT_JUMP.defaultBinding, resolveShortcut(AppShortcutAction.PROMPT_JUMP, corrupt))

        val rebind = updateCustomShortcut(emptyMap(), AppShortcutAction.COMMAND_PALETTE, ShortcutBinding("P", ctrl = true, shift = true))
        assertEquals("Ctrl+Shift+P", rebind[AppShortcutAction.COMMAND_PALETTE.id])
        assertTrue(isShortcutCustomized(AppShortcutAction.COMMAND_PALETTE, rebind))

        // Restoring to default removes the override entry instead of polluting preferences.
        val restored = updateCustomShortcut(rebind, AppShortcutAction.COMMAND_PALETTE, AppShortcutAction.COMMAND_PALETTE.defaultBinding)
        assertTrue(restored.isEmpty())

        // Conflict checks both default and custom bindings and swaps cleanly.
        val conflict = findShortcutConflict(AppShortcutAction.NEW_CHAT, ShortcutBinding("P", ctrl = true, shift = true), rebind)
        assertEquals(AppShortcutAction.COMMAND_PALETTE, conflict)
        assertNull(findShortcutConflict(AppShortcutAction.COMMAND_PALETTE, ShortcutBinding("P", ctrl = true, shift = true), rebind))

        val swapped = swapCustomShortcuts(
            customShortcuts = rebind,
            target = AppShortcutAction.NEW_CHAT,
            newBinding = ShortcutBinding("P", ctrl = true, shift = true),
            conflictingAction = AppShortcutAction.COMMAND_PALETTE
        )
        assertEquals(ShortcutBinding("P", ctrl = true, shift = true), resolveShortcut(AppShortcutAction.NEW_CHAT, swapped))
        assertEquals(ShortcutBinding("N", ctrl = true), resolveShortcut(AppShortcutAction.COMMAND_PALETTE, swapped))
    }

    @Test
    fun customShortcutsSerializeAndSearchIndexesLiveBindings() {
        val prefs = PreferencesConfig(customShortcuts = mapOf(AppShortcutAction.COMMAND_PALETTE.id to "Ctrl+Shift+P"))
        val encoded = compactStateJson.encodeToString(PreferencesConfig.serializer(), prefs)
        val decoded = compactStateJson.decodeFromString(PreferencesConfig.serializer(), encoded)
        assertEquals(prefs.customShortcuts, decoded.customShortcuts)

        // Legacy JSON lacking customShortcuts decodes cleanly to emptyMap().
        val legacy = compactStateJson.decodeFromString(PreferencesConfig.serializer(), "{}")
        assertTrue(legacy.customShortcuts.isEmpty())

        val state = createDefaultAppState().copy(preferences = prefs)
        val profile = state.profiles.first()
        val session = state.sessions.first()
        assertTrue(countCategorySearchMatches(SettingsCategory.SHORTCUTS, "Ctrl+Shift+P", state, profile, session) >= 1)
        assertTrue(countCategorySearchMatches(SettingsCategory.SHORTCUTS, "keyboard shortcut", state, profile, session) >= 1)
        assertTrue(countCategorySearchMatches(SettingsCategory.SHORTCUTS, "prompt", state, profile, session) >= 1)

        val keyboardGlyph = parseKissLabelIcons("⌨")
        assertNotNull(keyboardGlyph.leadingIcon)
        assertEquals(KissIcon.KEYBOARD, keyboardGlyph.leadingIcon)
    }
}
