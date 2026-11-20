package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.strings.Strings

/** One thing the command palette can do. [shortcut] is shown beside it, for example "Ctrl+N". */
internal data class PaletteCommand(val label: String, val shortcut: String = "", val run: () -> Unit)

private const val PALETTE_MAX_ROWS = 8

/**
 * Commands whose label contains every word of [query], in any order, ignoring case. Labels that start with the
 * query come first; otherwise the given order is kept. When no label contains the words (a typo, or initials
 * such as "nc" for New chat), labels whose letters hold the query in order are ranked by [fuzzyMatchScore].
 */
internal fun filterPaletteCommands(commands: List<PaletteCommand>, query: String): List<PaletteCommand> {
    val q = query.trim().lowercase()
    if (q.isEmpty()) return commands
    val words = q.split(Regex("\\s+"))
    val exact = commands
        .filter { command -> val label = command.label.lowercase(); words.all { it in label } }
        .sortedByDescending { it.label.lowercase().startsWith(q) }
    if (exact.isNotEmpty()) return exact
    val compact = words.joinToString("")
    return commands
        .mapNotNull { command -> fuzzyMatchScore(command.label, compact)?.let { command to it } }
        .sortedByDescending { it.second }
        .map { it.first }
}

/**
 * Scores [text] for a query whose characters appear in it in order (case-insensitive), or null when they don't.
 * Consecutive characters and characters at word starts score higher, so "jtp" ranks "Jump to prompt" above a label
 * that merely contains j, t and p somewhere. Greedy left-to-right: linear in the text, fine for palette-sized lists.
 */
internal fun fuzzyMatchScore(text: String, query: String): Int? {
    if (query.isEmpty()) return 0
    var score = 0
    var qi = 0
    var previousMatch = -2
    for (ti in text.indices) {
        if (qi == query.length) break
        if (text[ti].lowercaseChar() != query[qi].lowercaseChar()) continue
        score += 1
        if (ti == previousMatch + 1) score += 3
        if (ti == 0 || !text[ti - 1].isLetterOrDigit()) score += 2
        previousMatch = ti
        qi++
    }
    return if (qi == query.length) score - text.length / 32 else null
}

/** One user prompt the Ctrl+G jump list can scroll to; [number] counts prompts from the start of the chat. */
internal data class PromptJumpTarget(val nodeId: String, val number: Int, val preview: String)

/** The chat's prompts, newest first, each previewed as its first 140 characters on one line. */
internal fun promptJumpTargets(messages: List<MessageNode>): List<PromptJumpTarget> {
    var number = 0
    return messages.mapNotNull { node ->
        if (node.role != MessageRole.USER) return@mapNotNull null
        number++
        val preview = node.activeVariant.content.replace(Regex("\\s+"), " ").trim().take(140)
        if (preview.isEmpty()) null else PromptJumpTarget(node.id, number, preview)
    }.asReversed()
}

/**
 * The Message Map's rows: turns whose text or thinking contains [query], optionally only the user's prompts.
 * Chat order is kept so the map still reads top to bottom.
 */
internal fun filterMessageMapTurns(messages: List<MessageNode>, query: String, promptsOnly: Boolean): List<Pair<Int, MessageNode>> {
    val q = query.trim()
    return messages.mapIndexedNotNull { idx, node ->
        val roleOk = !promptsOnly || node.role == MessageRole.USER
        val textOk = q.isEmpty() ||
            node.activeVariant.content.contains(q, ignoreCase = true) ||
            node.activeVariant.thinkingSummary.contains(q, ignoreCase = true)
        if (roleOk && textOk) idx to node else null
    }
}

/**
 * Ctrl+K: type to find any action or chat, arrows to move, Enter to run, Escape or a click outside to close.
 */
@Composable
internal fun CommandPalette(
    commands: List<PaletteCommand>,
    onDismiss: () -> Unit,
    placeholder: String = Strings.typeACommandOrA
) {
    val p = LocalParchmentPalette.current
    var query by remember { mutableStateOf("") }
    var selected by remember { mutableStateOf(0) }
    val matches = remember(commands, query) { filterPaletteCommands(commands, query).take(PALETTE_MAX_ROWS) }
    if (selected > matches.lastIndex) selected = 0
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }

    fun runSelected() {
        val command = matches.getOrNull(selected) ?: return
        onDismiss()
        command.run()
    }

    Box(
        Modifier.fillMaxSize().background(Scrim.subtle).dismissOnOutsideTap(onDismiss),
        contentAlignment = Alignment.TopCenter
    ) {
        val shape = RoundedCornerShape(Radius.r12)
        Column(
            Modifier
                .padding(top = 72.dp, start = Space.s16, end = Space.s16)
                .widthIn(max = 560.dp * textGrowth())
                .fillMaxWidth()
                .overlayShadow(shape)
                .clip(shape)
                .background(p.elevatedBg)
                .border(LineWidth.hairline, p.border, shape)
                .blockTapThrough()
                .padding(Space.s8),
            verticalArrangement = Arrangement.spacedBy(Space.s6)
        ) {
            SepiaTextField(
                value = query,
                onValueChange = { query = it; selected = 0 },
                placeholder = placeholder,
                onEnterKeySend = { _, _ -> runSelected(); true },
                modifier = Modifier
                    .fillMaxWidth()
                    .focusRequester(focus)
                    .onPreviewKeyEvent { event ->
                        if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                        when (event.key) {
                            Key.DirectionDown -> { if (matches.isNotEmpty()) selected = (selected + 1) % matches.size; true }
                            Key.DirectionUp -> { if (matches.isNotEmpty()) selected = (selected - 1 + matches.size) % matches.size; true }
                            Key.Escape -> { onDismiss(); true }
                            else -> false
                        }
                    }
            )
            if (matches.isEmpty()) {
                BasicText(Strings.nothingMatches, style = p.uiLabelStyle, modifier = Modifier.padding(Space.s8))
            }
            val win = LocalParchmentWindowSize.current
            matches.forEachIndexed { index, command ->
                val isSelected = index == selected
                Row(
                    Modifier
                        .fillMaxWidth()
                        .heightIn(min = if (win.isCompact) Space.s48 else 40.dp)
                        .clip(RoundedCornerShape(Radius.r8))
                        .background(if (isSelected) p.accent.copy(alpha = 0.12f) else Color.Transparent)
                        .kissClickable(role = Role.Button, selected = isSelected, pressScale = 0.99f, hoverScale = 1f, onClick = { selected = index; runSelected() })
                        .padding(horizontal = Space.s10, vertical = Space.s6),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s12)
                ) {
                    BasicText(
                        command.label,
                        style = p.uiBodyStyle.copy(
                            color = if (isSelected) p.accent else p.primaryInk,
                            fontWeight = if (isSelected) FontWeight.Medium else FontWeight.Normal
                        ),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                    if (command.shortcut.isNotEmpty()) {
                        val badgeShape = RoundedCornerShape(Radius.r6)
                        Box(
                            modifier = Modifier
                                .clip(badgeShape)
                                .background(p.sheetBg)
                                .border(LineWidth.hairline, p.border, badgeShape)
                                .padding(horizontal = Space.s8, vertical = 2.dp),
                            contentAlignment = Alignment.Center
                        ) {
                            BasicText(
                                text = command.shortcut,
                                style = p.monoStyle.copy(fontSize = 11.sp, lineHeight = 16.sp, color = p.mutedInk)
                            )
                        }
                    }
                }
            }
        }
    }
}
