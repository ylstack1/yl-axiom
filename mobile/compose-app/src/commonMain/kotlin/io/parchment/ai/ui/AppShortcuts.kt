package io.parchment.ai.ui

import androidx.compose.runtime.Immutable
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.type
import io.parchment.ai.strings.Strings

/**
 * Hands a key event to the app's shortcuts from outside the composition.
 *
 * Compose delivers key events to a modifier only while something inside it has focus, so the root handler misses
 * every shortcut at launch or after a click on empty space. A desktop window passes its unconsumed key events here,
 * and [ParchmentApp] keeps [handler] pointed at the same function its root uses, so the list lives in one place.
 */
class AppShortcutDispatcher {
    internal var handler: (KeyEvent) -> Boolean = { false }

    /** Runs the matching shortcut and returns true if one ran. */
    fun dispatch(event: KeyEvent): Boolean = handler(event)
}

/**
 * Canonical keyboard shortcut chord (e.g. `Ctrl+K`, `Ctrl+Shift+Tab`, `F2`).
 * Persisted as a human-readable string inside `PreferencesConfig.customShortcuts`.
 */
@Immutable
internal data class ShortcutBinding(
    val keyCode: String,
    val ctrl: Boolean = false,
    val alt: Boolean = false,
    val shift: Boolean = false
) {
    fun keycapTokens(): List<String> = buildList {
        if (ctrl) add("Ctrl")
        if (alt) add("Alt")
        if (shift) add("Shift")
        add(keyCode)
    }

    fun format(): String = keycapTokens().joinToString("+")

    fun matches(event: KeyEvent): Boolean {
        if (event.type != KeyEventType.KeyDown) return false
        val eventCtrl = event.isCtrlPressed || event.isMetaPressed
        if (eventCtrl != ctrl || event.isAltPressed != alt || event.isShiftPressed != shift) return false
        return keyToCode(event.key) == keyCode
    }

    companion object {
        private val FUNCTION_KEYS = (1..12).map { "F$it" }.toSet()

        private val RESERVED_CHORDS = setOf(
            ShortcutBinding("C", ctrl = true),
            ShortcutBinding("V", ctrl = true),
            ShortcutBinding("X", ctrl = true),
            ShortcutBinding("A", ctrl = true),
            ShortcutBinding("Z", ctrl = true),
            ShortcutBinding("Y", ctrl = true),
            ShortcutBinding("Z", ctrl = true, shift = true),
            ShortcutBinding("F4", alt = true),
            ShortcutBinding("Insert", shift = true)
        )

        private val CODE_ALIASES: Map<String, String> = buildMap {
            for (c in 'A'..'Z') put(c.toString().lowercase(), c.toString())
            for (d in '0'..'9') put(d.toString(), d.toString())
            for (n in 1..12) put("f$n", "F$n")
            put(",", ",")
            put("comma", ",")
            put(".", ".")
            put("period", ".")
            put("/", "/")
            put("slash", "/")
            put(";", ";")
            put("semicolon", ";")
            put("'", "'")
            put("apostrophe", "'")
            put("[", "[")
            put("]", "]")
            put("\\", "\\")
            put("backslash", "\\")
            put("-", "-")
            put("minus", "-")
            put("=", "=")
            put("equals", "=")
            put("`", "`")
            put("grave", "`")
            put("tab", "Tab")
            put("space", "Space")
            put("enter", "Enter")
            put("backspace", "Backspace")
            put("delete", "Delete")
            put("insert", "Insert")
            put("home", "Home")
            put("end", "End")
            put("pageup", "PageUp")
            put("pagedown", "PageDown")
            put("up", "Up")
            put("down", "Down")
            put("left", "Left")
            put("right", "Right")
        }

        internal fun isFunctionKeyCode(code: String): Boolean = code in FUNCTION_KEYS

        internal fun isReserved(binding: ShortcutBinding): Boolean = binding in RESERVED_CHORDS

        internal fun hasAllowedModifiers(binding: ShortcutBinding): Boolean = binding.ctrl || binding.alt || isFunctionKeyCode(binding.keyCode)

        internal fun isValidBinding(binding: ShortcutBinding): Boolean = hasAllowedModifiers(binding) && !isReserved(binding)

        fun parse(raw: String): ShortcutBinding? {
            val trimmed = raw.trim()
            if (trimmed.isEmpty()) return null
            val parts = trimmed.split('+').map { it.trim() }.filter { it.isNotEmpty() }
            if (parts.isEmpty()) return null
            var ctrl = false
            var alt = false
            var shift = false
            var keyToken: String? = null
            for (part in parts) {
                when (part.lowercase()) {
                    "ctrl", "control", "cmd", "meta", "mod" -> ctrl = true
                    "alt", "option" -> alt = true
                    "shift" -> shift = true
                    else -> {
                        if (keyToken != null) return null
                        keyToken = CODE_ALIASES[part.lowercase()] ?: return null
                    }
                }
            }
            val code = keyToken ?: return null
            return ShortcutBinding(keyCode = code, ctrl = ctrl, alt = alt, shift = shift)
        }

        internal fun isModifierOnlyKey(key: Key): Boolean = when (key) {
            Key.CtrlLeft, Key.CtrlRight,
            Key.ShiftLeft, Key.ShiftRight,
            Key.AltLeft, Key.AltRight,
            Key.MetaLeft, Key.MetaRight,
            Key.CapsLock, Key.NumLock, Key.ScrollLock, Key.Function -> true
            else -> false
        }

        internal fun keyToCode(key: Key): String? = when (key) {
            Key.A -> "A"
            Key.B -> "B"
            Key.C -> "C"
            Key.D -> "D"
            Key.E -> "E"
            Key.F -> "F"
            Key.G -> "G"
            Key.H -> "H"
            Key.I -> "I"
            Key.J -> "J"
            Key.K -> "K"
            Key.L -> "L"
            Key.M -> "M"
            Key.N -> "N"
            Key.O -> "O"
            Key.P -> "P"
            Key.Q -> "Q"
            Key.R -> "R"
            Key.S -> "S"
            Key.T -> "T"
            Key.U -> "U"
            Key.V -> "V"
            Key.W -> "W"
            Key.X -> "X"
            Key.Y -> "Y"
            Key.Z -> "Z"
            Key.Zero -> "0"
            Key.One -> "1"
            Key.Two -> "2"
            Key.Three -> "3"
            Key.Four -> "4"
            Key.Five -> "5"
            Key.Six -> "6"
            Key.Seven -> "7"
            Key.Eight -> "8"
            Key.Nine -> "9"
            Key.F1 -> "F1"
            Key.F2 -> "F2"
            Key.F3 -> "F3"
            Key.F4 -> "F4"
            Key.F5 -> "F5"
            Key.F6 -> "F6"
            Key.F7 -> "F7"
            Key.F8 -> "F8"
            Key.F9 -> "F9"
            Key.F10 -> "F10"
            Key.F11 -> "F11"
            Key.F12 -> "F12"
            Key.Comma -> ","
            Key.Period -> "."
            Key.Slash -> "/"
            Key.Semicolon -> ";"
            Key.Apostrophe -> "'"
            Key.LeftBracket -> "["
            Key.RightBracket -> "]"
            Key.Backslash -> "\\"
            Key.Minus -> "-"
            Key.Equals -> "="
            Key.Grave -> "`"
            Key.Tab -> "Tab"
            Key.Spacebar -> "Space"
            Key.Enter -> "Enter"
            Key.Backspace -> "Backspace"
            Key.Delete -> "Delete"
            Key.Insert -> "Insert"
            Key.MoveHome -> "Home"
            Key.MoveEnd -> "End"
            Key.PageUp -> "PageUp"
            Key.PageDown -> "PageDown"
            Key.DirectionUp -> "Up"
            Key.DirectionDown -> "Down"
            Key.DirectionLeft -> "Left"
            Key.DirectionRight -> "Right"
            else -> null
        }
    }
}

internal sealed interface ShortcutCaptureResult {
    data class Accepted(val binding: ShortcutBinding) : ShortcutCaptureResult
    data object Cancelled : ShortcutCaptureResult
    data object IgnoredModifier : ShortcutCaptureResult
    data class Rejected(val reason: String) : ShortcutCaptureResult
}

internal fun captureShortcutKeyEvent(event: KeyEvent): ShortcutCaptureResult {
    if (event.key == Key.Escape && !event.isCtrlPressed && !event.isMetaPressed && !event.isAltPressed) {
        return ShortcutCaptureResult.Cancelled
    }
    if (ShortcutBinding.isModifierOnlyKey(event.key)) {
        return ShortcutCaptureResult.IgnoredModifier
    }
    val code = ShortcutBinding.keyToCode(event.key)
        ?: return ShortcutCaptureResult.Rejected(Strings.shortcutUnsupportedKey)
    val binding = ShortcutBinding(
        keyCode = code,
        ctrl = event.isCtrlPressed || event.isMetaPressed,
        alt = event.isAltPressed,
        shift = event.isShiftPressed
    )
    if (!ShortcutBinding.hasAllowedModifiers(binding)) {
        return ShortcutCaptureResult.Rejected(Strings.shortcutNeedsModifier)
    }
    if (ShortcutBinding.isReserved(binding)) {
        return ShortcutCaptureResult.Rejected(Strings.shortcutReserved(binding.format()))
    }
    return ShortcutCaptureResult.Accepted(binding)
}

internal enum class AppShortcutAction(
    val id: String,
    val defaultBinding: ShortcutBinding
) {
    COMMAND_PALETTE("command_palette", ShortcutBinding("K", ctrl = true)),
    PROMPT_JUMP("prompt_jump", ShortcutBinding("G", ctrl = true)),
    NEW_CHAT("new_chat", ShortcutBinding("N", ctrl = true)),
    TOGGLE_SETTINGS("open_settings", ShortcutBinding(",", ctrl = true)),
    TOGGLE_SIDEBAR("toggle_sidebar", ShortcutBinding("B", ctrl = true)),
    CLOSE_TAB("close_tab", ShortcutBinding("W", ctrl = true)),
    NEXT_TAB("next_tab", ShortcutBinding("Tab", ctrl = true)),
    PREV_TAB("prev_tab", ShortcutBinding("Tab", ctrl = true, shift = true));

    fun displayTitle(): String = when (this) {
        COMMAND_PALETTE -> Strings.commandPaletteLabel
        PROMPT_JUMP -> Strings.jumpToPromptInChat
        NEW_CHAT -> Strings.newChat2
        TOGGLE_SETTINGS -> Strings.openSettings
        TOGGLE_SIDEBAR -> Strings.showOrHideTheSidebar
        CLOSE_TAB -> Strings.closeThisTab
        NEXT_TAB -> Strings.nextTab
        PREV_TAB -> Strings.previousTab
    }

    fun displayDescription(): String = when (this) {
        COMMAND_PALETTE -> Strings.commandPaletteDesc
        PROMPT_JUMP -> Strings.promptJumpDesc
        NEW_CHAT -> Strings.newChatDesc
        TOGGLE_SETTINGS -> Strings.toggleSettingsDesc
        TOGGLE_SIDEBAR -> Strings.toggleSidebarDesc
        CLOSE_TAB -> Strings.closeTabDesc
        NEXT_TAB -> Strings.nextTabDesc
        PREV_TAB -> Strings.prevTabDesc
    }
}

internal fun resolveShortcut(
    action: AppShortcutAction,
    customShortcuts: Map<String, String>
): ShortcutBinding {
    val raw = customShortcuts[action.id] ?: return action.defaultBinding
    val parsed = ShortcutBinding.parse(raw) ?: return action.defaultBinding
    return if (ShortcutBinding.isValidBinding(parsed)) parsed else action.defaultBinding
}

internal fun isShortcutCustomized(
    action: AppShortcutAction,
    customShortcuts: Map<String, String>
): Boolean = resolveShortcut(action, customShortcuts) != action.defaultBinding

internal fun findShortcutConflict(
    target: AppShortcutAction,
    candidate: ShortcutBinding,
    customShortcuts: Map<String, String>
): AppShortcutAction? = AppShortcutAction.entries.firstOrNull { other ->
    other != target && resolveShortcut(other, customShortcuts) == candidate
}

internal fun updateCustomShortcut(
    customShortcuts: Map<String, String>,
    action: AppShortcutAction,
    binding: ShortcutBinding
): Map<String, String> = LinkedHashMap(customShortcuts).apply {
    if (binding == action.defaultBinding) remove(action.id) else put(action.id, binding.format())
}

internal fun swapCustomShortcuts(
    customShortcuts: Map<String, String>,
    target: AppShortcutAction,
    newBinding: ShortcutBinding,
    conflictingAction: AppShortcutAction
): Map<String, String> {
    val previousTargetBinding = resolveShortcut(target, customShortcuts)
    val afterTarget = updateCustomShortcut(customShortcuts, target, newBinding)
    return updateCustomShortcut(afterTarget, conflictingAction, previousTargetBinding)
}

internal fun matchShortcutAction(
    event: KeyEvent,
    customShortcuts: Map<String, String>
): AppShortcutAction? {
    if (event.type != KeyEventType.KeyDown) return null
    return AppShortcutAction.entries.firstOrNull { action ->
        resolveShortcut(action, customShortcuts).matches(event)
    }
}
