package io.parchment.ai.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.text
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** A wrapping choice with one radio target and room for enlarged text. */
@Composable
fun SepiaChoiceRow(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    leading: (@Composable () -> Unit)? = null
) {
    val p = LocalParchmentPalette.current
    val interactionSource = remember { MutableInteractionSource() }
    val isHovered by interactionSource.collectIsHoveredAsState()
    val isPressed by interactionSource.collectIsPressedAsState()
    val isFocused by interactionSource.collectIsFocusedAsState()
    var focusFromPointer by remember { mutableStateOf(false) }
    LaunchedEffect(isPressed, isFocused) {
        if (isPressed) focusFromPointer = true else if (!isFocused) focusFromPointer = false
    }
    val rowShape = RoundedCornerShape(Radius.r8)
    val rowBg = when {
        isPressed -> p.accent.copy(alpha = 0.14f)
        isHovered -> p.accent.copy(alpha = 0.07f)
        else -> Color.Transparent
    }
    Row(
        modifier
            .fillMaxWidth()
            .heightIn(min = Space.s48)
            .pointerHoverIcon(PointerIcon.Hand)
            .clip(rowShape)
            .background(rowBg)
            .sepiaFocusRing(isFocused && !focusFromPointer, p.accent)
            .selectable(
                selected = selected,
                interactionSource = interactionSource,
                indication = null,
                role = Role.RadioButton,
                onClick = onClick
            )
            .padding(horizontal = Space.s6, vertical = Space.s10),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        leading?.invoke()
        BasicText(label, modifier = Modifier.weight(1f), style = p.uiBodyStyle)
        KissVectorIcon(
            if (selected) KissIcon.CIRCLE_DOT else KissIcon.CIRCLE_EMPTY,
            tint = if (selected) p.accent else p.mutedInk,
            size = IconSize.xl
        )
    }
}

/** A confirmation whose message scrolls, so long text cannot push its actions off screen. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SepiaConfirmationCard(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    options: @Composable ColumnScope.() -> Unit = {},
    actions: @Composable () -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    val shape = RoundedCornerShape(Radius.r12)
    Column(
        modifier
            .widthIn(max = 420.dp)
            .fillMaxWidth(0.9f)
            .heightIn(max = win.heightDp * 0.9f)
            .overlayShadow(shape)
            .clip(shape)
            .background(p.elevatedBg)
            .border(LineWidth.emphasis, p.border, shape)
            .blockTapThrough()
            .padding(18.dp),
        verticalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        Column(
            Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Space.s12)
        ) {
            BasicText(title, modifier = Modifier.semantics { heading() }, style = p.uiHeaderStyle.copy(fontSize = 17.sp))
            BasicText(body, style = p.uiBodyStyle.copy(color = p.narrationInk))
            options()
        }
        FlowRow(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
            verticalArrangement = Arrangement.spacedBy(Space.s8)
        ) { actions() }
    }
}

/** One accent choice: a 48dp target around a smaller visual swatch. */
@Composable
fun SepiaAccentSwatch(color: Color, name: String, selected: Boolean, onClick: () -> Unit) {
    val p = LocalParchmentPalette.current
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    val isFocused by interactionSource.collectIsFocusedAsState()
    var focusFromPointer by remember { mutableStateOf(false) }
    LaunchedEffect(isPressed, isFocused) {
        if (isPressed) focusFromPointer = true else if (!isFocused) focusFromPointer = false
    }
    Box(
        Modifier.size(48.dp)
            .pointerHoverIcon(PointerIcon.Hand)
            .then(if (isFocused && !focusFromPointer) Modifier.border(LineWidth.focus, p.accent, CircleShape) else Modifier)
            .selectable(
                selected = selected,
                interactionSource = interactionSource,
                indication = null,
                role = Role.RadioButton,
                onClick = onClick
            )
            .semantics { contentDescription = name },
        contentAlignment = Alignment.Center
    ) {
        Box(
            Modifier.size(24.dp).clip(CircleShape).background(color)
                .border(if (selected) 2.dp else LineWidth.hairline, if (selected) p.primaryInk else p.border, CircleShape)
        )
    }
}

@Composable
fun SepiaTextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    label: String = "",
    placeholder: String = "",
    singleLine: Boolean = true,
    monospace: Boolean = false,
    borderless: Boolean = false,
    minHeight: Dp = 36.dp,
    maxHeight: Dp = 260.dp,
    maxLines: Int = if (singleLine) 1 else Int.MAX_VALUE,
    onEnterKeySend: ((isCtrlOrMetaPressed: Boolean, isShiftPressed: Boolean) -> Boolean)? = null,
    onClipboardPaste: (() -> Boolean)? = null,
    onKeyNavigation: ((KeyEvent) -> Boolean)? = null,
    visualTransformation: VisualTransformation = VisualTransformation.None
) {
    val p = LocalParchmentPalette.current
    val shape = RoundedCornerShape(Radius.r8)
    val textStyle = if (monospace) p.monoStyle else p.uiBodyStyle
    val interactionSource = remember { MutableInteractionSource() }
    val isFocused by interactionSource.collectIsFocusedAsState()
    var consumeNextEnterKeyUp by remember { mutableStateOf(false) }

    var textFieldValueState by remember {
        mutableStateOf(TextFieldValue(text = value, selection = TextRange(value.length)))
    }
    var lastTextValue by remember { mutableStateOf(value) }
    if (value != lastTextValue) {
        // The text changed from outside (a picked `/command ` or a streamed draft): sync and put the cursor
        // at the end.
        lastTextValue = value
        textFieldValueState = TextFieldValue(text = value, selection = TextRange(value.length))
    }
    val textFieldValue = textFieldValueState
    val borderColor by animateColorAsState(
        targetValue = when {
            borderless -> Color.Transparent
            isFocused -> p.accent.copy(alpha = 0.85f)
            else -> p.border.copy(alpha = 0.75f)
        },
        animationSpec = motionSpring(Spring.StiffnessMedium)
    )

    Column(modifier = modifier) {
        if (label.isNotBlank()) {
            BasicText(
                text = label,
                // The input carries this label below; readers should announce it only once.
                modifier = Modifier.semantics { hideFromAccessibility() },
                style = p.uiLabelStyle.copy(color = p.narrationInk, fontWeight = FontWeight.SemiBold)
            )
            Spacer(Modifier.height(Space.s4))
        }
        BasicTextField(
            value = textFieldValue,
            onValueChange = { newTextFieldValue ->
                textFieldValueState = newTextFieldValue
                val textChanged = lastTextValue != newTextFieldValue.text
                lastTextValue = newTextFieldValue.text
                if (textChanged) {
                    onValueChange(newTextFieldValue.text)
                }
            },
            singleLine = singleLine,
            maxLines = maxLines,
            textStyle = textStyle,
            interactionSource = interactionSource,
            visualTransformation = visualTransformation,
            cursorBrush = SolidColor(p.accent),
            modifier = Modifier
                .fillMaxWidth()
                .then(
                    if (!singleLine || onEnterKeySend != null || onClipboardPaste != null || onKeyNavigation != null) {
                        Modifier.onPreviewKeyEvent { keyEvent ->
                            val isEnter = keyEvent.key == Key.Enter || keyEvent.key == Key.NumPadEnter
                            if (keyEvent.type == KeyEventType.KeyUp && isEnter && consumeNextEnterKeyUp) {
                                consumeNextEnterKeyUp = false
                                return@onPreviewKeyEvent true
                            }
                            if (onKeyNavigation?.invoke(keyEvent) == true) {
                                if (isEnter && keyEvent.type == KeyEventType.KeyDown) {
                                    consumeNextEnterKeyUp = true
                                }
                                return@onPreviewKeyEvent true
                            }
                            // 1. Check Ctrl+V / Cmd+V / Shift+Insert for Clipboard Image / Video / File Paste
                            if (onClipboardPaste != null &&
                                keyEvent.type == KeyEventType.KeyDown &&
                                !keyEvent.isAltPressed &&
                                (((keyEvent.isCtrlPressed || keyEvent.isMetaPressed) && keyEvent.key == Key.V) ||
                                    (keyEvent.isShiftPressed && keyEvent.key == Key.Insert))
                            ) {
                                if (onClipboardPaste()) {
                                    return@onPreviewKeyEvent true
                                }
                            }

                            // 2. Check Enter / Shift+Enter / Ctrl+Enter for Send vs Newline
                            if (isEnter && !keyEvent.isAltPressed) {
                                val isCtrlOrMeta = keyEvent.isCtrlPressed || keyEvent.isMetaPressed
                                val isShift = keyEvent.isShiftPressed
                                return@onPreviewKeyEvent when (keyEvent.type) {
                                    KeyEventType.KeyDown -> {
                                        val consumedBySend = onEnterKeySend?.invoke(isCtrlOrMeta, isShift) == true
                                        if (consumedBySend) {
                                            consumeNextEnterKeyUp = true
                                            true
                                        } else if (!singleLine && (isShift || isCtrlOrMeta)) {
                                            // Compose Foundation's defaultKeyMapping only maps unmodified Enter to NEW_LINE
                                            // and ignores Shift+Enter / Ctrl+Enter. Insert '\n' directly at the active selection.
                                            val curText = textFieldValue.text
                                            val start = textFieldValue.selection.min.coerceIn(0, curText.length)
                                            val end = textFieldValue.selection.max.coerceIn(0, curText.length)
                                            val newText = curText.substring(0, start) + "\n" + curText.substring(end)
                                            val nextCursor = start + 1
                                            val updatedValue = TextFieldValue(
                                                text = newText,
                                                selection = TextRange(nextCursor)
                                            )
                                            textFieldValueState = updatedValue
                                            if (lastTextValue != newText) {
                                                lastTextValue = newText
                                                onValueChange(newText)
                                            }
                                            consumeNextEnterKeyUp = true
                                            true
                                        } else {
                                            consumeNextEnterKeyUp = false
                                            false
                                        }
                                    }
                                    KeyEventType.KeyUp -> {
                                        if (consumeNextEnterKeyUp) {
                                            consumeNextEnterKeyUp = false
                                            true
                                        } else {
                                            false
                                        }
                                    }
                                    else -> false
                                }
                            }
                            false
                        }
                    } else Modifier
                ),
            decorationBox = { innerTextField ->
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = minHeight, max = if (singleLine) Dp.Infinity else maxHeight)
                        .clip(shape)
                        .background(if (borderless) Color.Transparent else p.cardBg)
                        .then(
                            if (!borderless) {
                                Modifier.border(if (isFocused) 1.dp else LineWidth.hairline, borderColor, shape)
                            } else Modifier
                        )
                        .padding(
                            horizontal = if (borderless) 4.dp else 11.dp,
                            vertical = if (borderless) 6.dp else 8.dp
                        ),
                    contentAlignment = if (singleLine) Alignment.CenterStart else Alignment.TopStart
                ) {
                    val parsedPlaceholder = remember(placeholder) { parseKissLabelIcons(placeholder) }
                    val semanticLabel = label.ifBlank {
                        if (value.isNotEmpty()) parsedPlaceholder.cleanText else ""
                    }
                    if (semanticLabel.isNotBlank()) {
                        // Merges into the input's semantics, naming it without replacing its value.
                        Box(Modifier.semantics { text = AnnotatedString(semanticLabel) })
                    }
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(7.dp),
                        verticalAlignment = if (singleLine) Alignment.CenterVertically else Alignment.Top
                    ) {
                        if (parsedPlaceholder.leadingIcon != null) {
                            KissVectorIcon(
                                icon = parsedPlaceholder.leadingIcon,
                                tint = if (isFocused) p.accent else p.mutedInk.copy(alpha = 0.75f),
                                size = 14.dp
                            )
                        }
                        Box(modifier = Modifier.weight(1f)) {
                            if (value.isEmpty() && parsedPlaceholder.cleanText.isNotEmpty()) {
                                BasicText(
                                    text = parsedPlaceholder.cleanText,
                                    maxLines = if (singleLine) 1 else 2,
                                    overflow = TextOverflow.Ellipsis,
                                    style = textStyle.copy(color = p.mutedInk.copy(alpha = Alpha.placeholder))
                                )
                            }
                            innerTextField()
                        }
                    }
                }
            }
        )
    }
}

@Composable
fun SepiaSectionCard(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String = "",
    headerAction: @Composable (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    val shape = RoundedCornerShape(Radius.r12)
    val cardPadding = if (win.isCompact) 13.dp else 16.dp
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(p.cardBg)
            .border(LineWidth.hairline, p.border.copy(alpha = 0.52f), shape)
            .animateContentSize(animationSpec = motionSpring(Spring.StiffnessMedium))
            .padding(cardPadding)
    ) {
        if (title.isNotBlank() || headerAction != null) {
            val parsedTitle = remember(title) { parseKissLabelIcons(title) }
            @Composable
            fun RenderSectionTitle() {
                if (title.isNotBlank()) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(7.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        if (parsedTitle.leadingIcon != null) {
                            KissVectorIcon(
                                icon = parsedTitle.leadingIcon,
                                tint = p.accent,
                                size = IconSize.lg
                            )
                        }
                        BasicText(
                            text = parsedTitle.cleanText.ifEmpty { title },
                            style = p.uiHeaderStyle.copy(fontSize = 15.sp)
                        )
                    }
                }
            }

            if (win.isCompact && headerAction != null) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(Space.s8)
                ) {
                    RenderSectionTitle()
                    if (subtitle.isNotBlank()) {
                        BasicText(
                            text = subtitle,
                            style = p.uiLabelStyle
                        )
                    }
                    headerAction()
                }
            } else {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(modifier = Modifier.weight(1f).padding(end = if (headerAction != null) 8.dp else 0.dp)) {
                        RenderSectionTitle()
                        if (subtitle.isNotBlank()) {
                            Spacer(Modifier.height(Space.s2))
                            BasicText(
                                text = subtitle,
                                style = p.uiLabelStyle
                            )
                        }
                    }
                    if (headerAction != null) {
                        headerAction()
                    }
                }
            }
            Spacer(Modifier.height(Space.s12))
        }
        content()
    }
}

@Composable
fun SepiaDivider(modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(LineWidth.hairline)
            .background(p.border.copy(alpha = 0.42f))
    )
}

/**
 * Parchment Soft Circular Avatar / Icon Badge (for Assistants, Providers, and Models).
 */
@Composable
fun SepiaAvatarCircle(
    symbol: String,
    selected: Boolean = false,
    sizeDp: Dp = 32.dp,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val bg by animateColorAsState(
        targetValue = if (selected) p.accent.copy(alpha = 0.18f) else p.sheetBg.copy(alpha = 0.85f),
        animationSpec = motionSpring(Spring.StiffnessMedium)
    )
    val fgColor = if (selected) p.accent else p.primaryInk
    val parsedIcon = remember(symbol) { parseKissLabelIcons(symbol).leadingIcon }

    Box(
        modifier = modifier
            .size(sizeDp)
            .clip(CircleShape)
            .background(bg)
            .border(LineWidth.hairline, if (selected) p.accent.copy(alpha = 0.6f) else p.border.copy(alpha = 0.6f), CircleShape),
        contentAlignment = Alignment.Center
    ) {
        if (parsedIcon != null) {
            KissVectorIcon(
                icon = parsedIcon,
                tint = fgColor,
                size = (sizeDp.value * 0.50f).dp
            )
        } else {
            val cleanChars = symbol.filter { it.code < 128 && it.isLetterOrDigit() }.take(2).uppercase()
            if (cleanChars.isNotEmpty()) {
                BasicText(
                    text = cleanChars,
                    style = p.uiBodyStyle.copy(
                        fontSize = (sizeDp.value * 0.42f).sp,
                        fontWeight = FontWeight.SemiBold,
                        color = fgColor
                    )
                )
            } else {
                KissVectorIcon(
                    icon = KissIcon.SPARKLES,
                    tint = fgColor,
                    size = (sizeDp.value * 0.48f).dp
                )
            }
        }
    }
}

/**
 * Parchment Animated Pill Toggle Switch (Pure Compose Foundation).
 */
@Composable
fun SepiaToggleSwitch(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    label: String,
    modifier: Modifier = Modifier
) {
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    val isFocused by interactionSource.collectIsFocusedAsState()
    var focusFromPointer by remember { mutableStateOf(false) }
    LaunchedEffect(isPressed, isFocused) {
        if (isPressed) focusFromPointer = true else if (!isFocused) focusFromPointer = false
    }
    val focusColor = LocalParchmentPalette.current.accent
    Box(
        modifier.sizeIn(minWidth = 48.dp, minHeight = 48.dp)
            .pointerHoverIcon(PointerIcon.Hand)
            .sepiaFocusRing(isFocused && !focusFromPointer, focusColor)
            .toggleable(
                checked,
                interactionSource = interactionSource,
                indication = null,
                role = Role.Switch,
                onValueChange = onCheckedChange
            )
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center
    ) {
        SepiaToggleTrack(checked)
    }
}

@Composable
private fun SepiaToggleTrack(checked: Boolean) {
    val p = LocalParchmentPalette.current
    val trackColor by animateColorAsState(
        targetValue = if (checked) p.accent else p.border.copy(alpha = 0.7f),
        animationSpec = motionSpring(Spring.StiffnessMedium)
    )
    val thumbBias by animateFloatAsState(
        targetValue = if (checked) 1f else 0f,
        animationSpec = motionSpring(Spring.StiffnessMedium)
    )

    Box(
        modifier = Modifier
            .width(42.dp)
            .height(24.dp)
            .clip(RoundedCornerShape(Radius.pill))
            .background(trackColor)
            .padding(3.dp),
        contentAlignment = Alignment.CenterStart
    ) {
        Box(
            modifier = Modifier
                .graphicsLayer {
                    translationX = thumbBias * 18.dp.toPx()
                }
                .size(18.dp)
                .clip(CircleShape)
                .background(WARM_FILL_INK)
        )
    }
}

/**
 * Parchment Settings Preference Toggle Row (Title + Subtitle on left, Switch on right).
 */
@Composable
fun SepiaToggleRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    subtitle: String = "",
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val interactionSource = remember { MutableInteractionSource() }
    val isHovered by interactionSource.collectIsHoveredAsState()
    val isPressed by interactionSource.collectIsPressedAsState()
    val rowBg by animateColorAsState(
        targetValue = when {
            isPressed -> p.accent.copy(alpha = 0.12f)
            isHovered -> p.accent.copy(alpha = 0.06f)
            else -> Color.Transparent
        },
        animationSpec = motionSpring(if (isPressed) Spring.StiffnessHigh else Spring.StiffnessMedium)
    )
    val shape = RoundedCornerShape(Radius.r8)
    val isFocused by interactionSource.collectIsFocusedAsState()
    var focusFromPointer by remember { mutableStateOf(false) }
    LaunchedEffect(isPressed, isFocused) {
        if (isPressed) focusFromPointer = true else if (!isFocused) focusFromPointer = false
    }
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = Space.s48)
            .clip(shape)
            .background(rowBg)
            .sepiaFocusRing(isFocused && !focusFromPointer, p.accent)
            .pointerHoverIcon(PointerIcon.Hand)
            .toggleable(
                checked,
                interactionSource = interactionSource,
                indication = null,
                role = Role.Switch,
                onValueChange = onCheckedChange
            )
            .padding(horizontal = Space.s6, vertical = Space.s8),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically
    ) {
        Column(modifier = Modifier.weight(1f).padding(end = Space.s12)) {
            BasicText(
                text = title,
                style = p.uiBodyStyle.copy(
                    fontSize = 14.sp,
                    fontWeight = FontWeight.Medium,
                    color = if (isHovered || isPressed) p.accent else p.primaryInk
                )
            )
            if (subtitle.isNotBlank()) {
                Spacer(Modifier.height(Space.s2))
                BasicText(
                    text = subtitle,
                    style = p.uiLabelStyle.copy(fontSize = 11.sp)
                )
            }
        }
        SepiaToggleTrack(checked)
    }
}

/**
 * Breathing pulse dot for active streaming & long-horizon deep thinking.
 */
@Composable
fun KissStreamingPulseDot(
    color: Color = LocalParchmentPalette.current.accent,
    modifier: Modifier = Modifier
) {
    val dot = modifier.size(8.dp)
    // With Reduce motion the dot holds still, and no frame-by-frame animation runs at all.
    if (LocalReduceMotion.current) {
        Box(dot.clip(CircleShape).background(color))
        return
    }
    val transition = rememberInfiniteTransition()
    val pulse = infiniteRepeatable<Float>(
        animation = tween(durationMillis = Motion.LOOP, easing = FastOutSlowInEasing),
        repeatMode = RepeatMode.Reverse
    )
    val pulseAlpha by transition.animateFloat(initialValue = 0.35f, targetValue = 1.0f, animationSpec = pulse)
    val pulseScale by transition.animateFloat(initialValue = 0.85f, targetValue = 1.15f, animationSpec = pulse)
    Box(
        modifier = dot
            .graphicsLayer {
                scaleX = pulseScale
                scaleY = pulseScale
                alpha = pulseAlpha
            }
            .clip(CircleShape)
            .background(color)
    )
}

/** Calm determinate progress bar whose fill interpolates with [motionTween]. */
@Composable
internal fun SepiaProgressBar(
    fraction: Float,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val clampedFraction = fraction.coerceIn(0f, 1f)
    val smoothFraction by animateFloatAsState(
        targetValue = clampedFraction.coerceAtLeast(0.02f),
        animationSpec = motionTween(Motion.MEDIUM)
    )
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(6.dp)
            .semantics { progressBarRangeInfo = ProgressBarRangeInfo(clampedFraction, 0f..1f) }
            .clip(RoundedCornerShape(Radius.pill))
            .background(p.border.copy(alpha = 0.45f))
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth(smoothFraction)
                .height(6.dp)
                .clip(RoundedCornerShape(Radius.pill))
                .background(p.accent)
        )
    }
}

private class NumericDraftSync<T>(var lastExternal: T, val draft: MutableState<String>)

/**
 * Keeps an editable numeric text field stable while typing partial inputs (`"1"`, `"1."`, `""`),
 * syncing back to [externalValue] synchronously only when the underlying value changes out-of-band.
 */
@Composable
internal fun <T> rememberSyncedNumericDraft(
    externalValue: T,
    key: Any? = Unit,
    format: (T) -> String,
    parse: (String) -> T?
): MutableState<String> {
    val holder = remember(key) { NumericDraftSync(externalValue, mutableStateOf(format(externalValue))) }
    if (holder.lastExternal != externalValue) {
        holder.lastExternal = externalValue
        if (parse(holder.draft.value) != externalValue) {
            holder.draft.value = format(externalValue)
        }
    }
    return holder.draft
}
