package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.SendShortcutMode
import io.parchment.ai.strings.Strings

private data class PendingShortcutConflict(
    val target: AppShortcutAction,
    val candidate: ShortcutBinding,
    val conflicting: AppShortcutAction
)

@Composable
internal fun ShortcutKeycapRow(
    tokens: List<String>,
    highlighted: Boolean = false,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    Row(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(Space.s4),
        verticalAlignment = Alignment.CenterVertically
    ) {
        tokens.forEachIndexed { idx, token ->
            if (idx > 0) {
                BasicText(
                    text = "+",
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, color = p.mutedInk)
                )
            }
            val shape = RoundedCornerShape(Radius.r6)
            Box(
                modifier = Modifier
                    .clip(shape)
                    .background(if (highlighted) p.accent.copy(alpha = 0.14f) else p.sheetBg)
                    .border(
                        width = if (highlighted) LineWidth.emphasis else LineWidth.hairline,
                        color = if (highlighted) p.accent else p.border,
                        shape = shape
                    )
                    .padding(horizontal = Space.s8, vertical = 3.dp),
                contentAlignment = Alignment.Center
            ) {
                BasicText(
                    text = token,
                    style = p.monoStyle.copy(
                        fontSize = 12.sp,
                        lineHeight = 16.sp,
                        fontWeight = FontWeight.Medium,
                        color = if (highlighted) p.accent else p.primaryInk
                    )
                )
            }
        }
    }
}

@Composable
internal fun ShortcutsSubpage(
    prefs: PreferencesConfig,
    onUpdatePreferences: (PreferencesConfig) -> Unit,
    onCaptureInterceptorChange: (((KeyEvent) -> Boolean)?) -> Unit = {},
    onNotify: (String) -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    var recordingAction by remember { mutableStateOf<AppShortcutAction?>(null) }
    var recordingError by remember { mutableStateOf<String?>(null) }
    var pendingConflict by remember { mutableStateOf<PendingShortcutConflict?>(null) }
    val captureFocus = remember { FocusRequester() }
    val latestPrefs by rememberUpdatedState(prefs)
    val latestOnUpdate by rememberUpdatedState(onUpdatePreferences)
    val latestOnNotify by rememberUpdatedState(onNotify)

    fun handleCapturedEvent(event: KeyEvent): Boolean {
        val target = recordingAction ?: return false
        if (event.type != KeyEventType.KeyDown) return true
        when (val result = captureShortcutKeyEvent(event)) {
            ShortcutCaptureResult.Cancelled -> {
                recordingAction = null
                recordingError = null
                pendingConflict = null
            }
            ShortcutCaptureResult.IgnoredModifier -> Unit
            is ShortcutCaptureResult.Rejected -> {
                recordingError = result.reason
                pendingConflict = null
            }
            is ShortcutCaptureResult.Accepted -> {
                val custom = latestPrefs.customShortcuts
                val conflict = findShortcutConflict(target, result.binding, custom)
                if (conflict != null) {
                    pendingConflict = PendingShortcutConflict(target, result.binding, conflict)
                    recordingAction = null
                    recordingError = null
                } else {
                    val updated = updateCustomShortcut(custom, target, result.binding)
                    latestOnUpdate(latestPrefs.copy(customShortcuts = updated))
                    recordingAction = null
                    recordingError = null
                    pendingConflict = null
                    latestOnNotify(Strings.shortcutUpdated(target.displayTitle(), result.binding.format()))
                }
            }
        }
        return true
    }

    DisposableEffect(recordingAction) {
        if (recordingAction != null) {
            onCaptureInterceptorChange(::handleCapturedEvent)
        } else {
            onCaptureInterceptorChange(null)
        }
        onDispose { onCaptureInterceptorChange(null) }
    }

    LaunchedEffect(recordingAction) {
        if (recordingAction != null) {
            runCatching { captureFocus.requestFocus() }
        }
    }

    val customizedCount = remember(prefs.customShortcuts) {
        AppShortcutAction.entries.count { isShortcutCustomized(it, prefs.customShortcuts) }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .focusRequester(captureFocus)
            .focusable()
            .onPreviewKeyEvent { event ->
                if (recordingAction != null) handleCapturedEvent(event) else false
            }
            .verticalScroll(rememberScrollState())
            .padding(if (win.isCompact) Space.s12 else Space.s16),
        verticalArrangement = Arrangement.spacedBy(Space.s16)
    ) {
        SepiaSectionCard(
            title = Strings.appShortcuts,
            subtitle = Strings.clickAnyShortcutToRecord,
            headerAction = if (customizedCount > 0) {
                {
                    SepiaButton(
                        text = Strings.resetAll,
                        compact = true,
                        style = SepiaButtonStyle.OUTLINE,
                        onClick = {
                            recordingAction = null
                            recordingError = null
                            pendingConflict = null
                            onUpdatePreferences(prefs.copy(customShortcuts = emptyMap()))
                            onNotify(Strings.allShortcutsReset)
                        }
                    )
                }
            } else null
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.s6)) {
                val conflict = pendingConflict
                if (conflict != null) {
                    val bannerShape = RoundedCornerShape(Radius.r8)
                    @OptIn(ExperimentalLayoutApi::class)
                    FlowRow(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(bannerShape)
                            .background(p.accent.copy(alpha = 0.10f))
                            .border(LineWidth.emphasis, p.accent.copy(alpha = 0.65f), bannerShape)
                            .padding(horizontal = Space.s12, vertical = Space.s10),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalArrangement = Arrangement.spacedBy(Space.s8),
                        itemVerticalAlignment = Alignment.CenterVertically
                    ) {
                        BasicText(
                            text = Strings.shortcutConflict(conflict.candidate.format(), conflict.conflicting.displayTitle()),
                            style = p.uiBodyStyle.copy(color = p.primaryInk, fontWeight = FontWeight.Medium),
                            modifier = Modifier.weight(1f, fill = false).padding(end = Space.s8)
                        )
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(Space.s6),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            SepiaButton(
                                text = Strings.swapShortcuts,
                                compact = true,
                                style = SepiaButtonStyle.PRIMARY,
                                onClick = {
                                    val swapped = swapCustomShortcuts(
                                        customShortcuts = prefs.customShortcuts,
                                        target = conflict.target,
                                        newBinding = conflict.candidate,
                                        conflictingAction = conflict.conflicting
                                    )
                                    onUpdatePreferences(prefs.copy(customShortcuts = swapped))
                                    pendingConflict = null
                                    onNotify(Strings.shortcutSwapped(conflict.target.displayTitle(), conflict.conflicting.displayTitle()))
                                }
                            )
                            SepiaButton(
                                text = Strings.cancel,
                                compact = true,
                                style = SepiaButtonStyle.OUTLINE,
                                onClick = { pendingConflict = null }
                            )
                        }
                    }
                }

                if (recordingError != null) {
                    BasicText(
                        text = recordingError!!,
                        style = p.uiLabelStyle.copy(color = p.danger, fontWeight = FontWeight.Medium),
                        modifier = Modifier.padding(horizontal = Space.s4, vertical = Space.s2)
                    )
                }

                AppShortcutAction.entries.forEachIndexed { idx, action ->
                    if (idx > 0) SepiaDivider()
                    val binding = resolveShortcut(action, prefs.customShortcuts)
                    val isCustom = binding != action.defaultBinding
                    val isRecording = recordingAction == action
                    val rowShape = RoundedCornerShape(Radius.r8)

                    @OptIn(ExperimentalLayoutApi::class)
                    FlowRow(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(rowShape)
                            .background(if (isRecording) p.accent.copy(alpha = 0.08f) else Color.Transparent)
                            .padding(horizontal = Space.s6, vertical = Space.s8),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalArrangement = Arrangement.spacedBy(Space.s8),
                        itemVerticalAlignment = Alignment.CenterVertically
                    ) {
                        Column(
                            modifier = Modifier.weight(1f, fill = false).padding(end = Space.s12),
                            verticalArrangement = Arrangement.spacedBy(Space.s2)
                        ) {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(Space.s6),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                BasicText(
                                    text = action.displayTitle(),
                                    style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium)
                                )
                                if (isCustom) {
                                    BasicText(
                                        text = Strings.customBadge,
                                        style = p.uiLabelStyle.copy(
                                            fontSize = 11.sp,
                                            color = p.accent,
                                            fontWeight = FontWeight.SemiBold
                                        )
                                    )
                                }
                            }
                            BasicText(
                                text = action.displayDescription(),
                                style = p.uiLabelStyle
                            )
                        }

                        Row(
                            horizontalArrangement = Arrangement.spacedBy(Space.s6),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            val pillShape = RoundedCornerShape(Radius.r8)
                            Box(
                                modifier = Modifier
                                    .heightIn(min = ControlHeight.compact)
                                    .clip(pillShape)
                                    .background(
                                        if (isRecording) p.accent.copy(alpha = 0.16f)
                                        else p.elevatedBg.copy(alpha = 0.65f)
                                    )
                                    .border(
                                        width = if (isRecording) LineWidth.selected else LineWidth.hairline,
                                        color = if (isRecording) p.accent else p.border,
                                        shape = pillShape
                                    )
                                    .kissClickable(
                                        pressScale = 0.97f,
                                        hoverScale = 1.01f,
                                        role = Role.Button,
                                        selected = isRecording,
                                        onClickLabel = Strings.rebindShortcutLabel(action.displayTitle()),
                                        onClick = {
                                            recordingError = null
                                            pendingConflict = null
                                            recordingAction = if (isRecording) null else action
                                        }
                                    )
                                    .accessibilityLabel(Strings.rebindShortcutLabel(action.displayTitle()))
                                    .padding(horizontal = Space.s10, vertical = Space.s6),
                                contentAlignment = Alignment.Center
                            ) {
                                if (isRecording) {
                                    BasicText(
                                        text = Strings.pressKeysPrompt,
                                        style = p.uiLabelStyle.copy(
                                            color = p.accent,
                                            fontWeight = FontWeight.SemiBold
                                        )
                                    )
                                } else {
                                    ShortcutKeycapRow(tokens = binding.keycapTokens(), highlighted = isCustom)
                                }
                            }

                            if (isCustom) {
                                SepiaButton(
                                    text = Strings.resetShortcut,
                                    contentDescription = Strings.resetShortcutLabel(action.displayTitle()),
                                    compact = true,
                                    style = SepiaButtonStyle.GHOST,
                                    onClick = {
                                        if (recordingAction == action) recordingAction = null
                                        recordingError = null
                                        pendingConflict = null
                                        val updated = updateCustomShortcut(prefs.customShortcuts, action, action.defaultBinding)
                                        onUpdatePreferences(prefs.copy(customShortcuts = updated))
                                        onNotify(Strings.shortcutReset(action.displayTitle()))
                                    }
                                )
                            }
                        }
                    }
                }
            }
        }

        SepiaSectionCard(
            title = Strings.composerAndSending,
            subtitle = when (prefs.sendShortcut) {
                SendShortcutMode.ENTER -> Strings.sendShortcutHint("Enter", "Shift+Enter")
                SendShortcutMode.CTRL_ENTER -> Strings.sendShortcutHint("Ctrl+Enter", "Enter")
                SendShortcutMode.SHIFT_ENTER -> Strings.sendShortcutHint("Shift+Enter", "Enter")
            }
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Space.s12)) {
                SepiaAdaptiveRow(label = Strings.sendShortcutDesktop) {
                    @OptIn(ExperimentalLayoutApi::class)
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(Space.s6),
                        verticalArrangement = Arrangement.spacedBy(Space.s6)
                    ) {
                        SendShortcutMode.entries.forEach { mode ->
                            SepiaChip(
                                label = mode.label,
                                selected = prefs.sendShortcut == mode,
                                onClick = { onUpdatePreferences(prefs.copy(sendShortcut = mode)) }
                            )
                        }
                    }
                }

                SepiaDivider()

                BasicText(
                    text = Strings.builtInShortcuts,
                    style = p.uiLabelStyle.copy(fontWeight = FontWeight.SemiBold, color = p.narrationInk)
                )

                BuiltInShortcutRow(label = Strings.closeOverlayOrBack, tokens = listOf("Esc"))
                BuiltInShortcutRow(label = Strings.pasteMediaOrFiles, tokens = listOf("Ctrl", "V"))
            }
        }
    }
}

@Composable
private fun BuiltInShortcutRow(
    label: String,
    tokens: List<String>
) {
    val p = LocalParchmentPalette.current
    @OptIn(ExperimentalLayoutApi::class)
    FlowRow(
        modifier = Modifier.fillMaxWidth().padding(vertical = Space.s2),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalArrangement = Arrangement.spacedBy(Space.s6),
        itemVerticalAlignment = Alignment.CenterVertically
    ) {
        BasicText(
            text = label,
            style = p.uiBodyStyle.copy(color = p.narrationInk),
            modifier = Modifier.weight(1f, fill = false).padding(end = Space.s8)
        )
        ShortcutKeycapRow(tokens = tokens)
    }
}
