package io.parchment.ai.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import io.parchment.ai.model.isWebLink
import io.parchment.ai.platform.PlatformStorage
import kotlinx.coroutines.delay

/**
 * Adaptive Window Size Breakpoints (WindowSizeClass pattern for Mobile Portrait vs Tablet vs Desktop).
 */
data class ParchmentWindowSize(
    val widthDp: Dp = 1024.dp,
    val heightDp: Dp = 768.dp
) {
    val isCompact: Boolean get() = widthDp < 600.dp
    val isVeryNarrow: Boolean get() = widthDp < 410.dp
}

val LocalParchmentWindowSize = compositionLocalOf { ParchmentWindowSize() }

/** True when the person turned off decorative movement in Settings. */
val LocalReduceMotion = compositionLocalOf { false }

/** Counts for interface text: "1 chat", "3 chats". */
fun plural(count: Int, noun: String): String = "$count $noun${if (count == 1) "" else "s"}"

/**
 * A settings row: label left and content right on wide screens; on compact screens the label sits above
 * full-width content, so chips and buttons never clip.
 */
@Composable
fun SepiaAdaptiveRow(
    label: String,
    modifier: Modifier = Modifier,
    forceStackOnMobile: Boolean = true,
    content: @Composable () -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    if (win.isCompact && forceStackOnMobile) {
        Column(
            modifier = modifier.fillMaxWidth().padding(vertical = Space.s4),
            verticalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            BasicText(
                text = label,
                style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium, fontSize = 14.sp)
            )
            content()
        }
    } else {
        Row(
            modifier = modifier.fillMaxWidth().padding(vertical = Space.s4),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            BasicText(
                text = label,
                style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium),
                modifier = Modifier.padding(end = Space.s12)
            )
            content()
        }
    }
}

enum class SepiaButtonStyle {
    PRIMARY,
    SECONDARY,
    OUTLINE,
    GHOST,
    DANGER
}

private val COMPACT_BTN_SHAPE = RoundedCornerShape(Radius.r8)
private val REGULAR_BTN_SHAPE = RoundedCornerShape(Radius.r8)

/**
 * Clickable with feedback:
 * - Hover (pointer devices): hand cursor and a slight lift (1.018 scale, -0.75 dp).
 * - Press: a spring squeeze (0.945 scale) and haptic feedback.
 */
@Composable
fun Modifier.kissClickable(
    enabled: Boolean = true,
    pressScale: Float = 0.945f,
    hoverScale: Float = 1.018f,
    strongHaptic: Boolean = false,
    interactionSource: MutableInteractionSource = remember { MutableInteractionSource() },
    role: Role? = Role.Button,
    selected: Boolean? = null,
    onClickLabel: String? = null,
    onClick: () -> Unit
): Modifier {
    val haptics = LocalHapticFeedback.current
    val focusColor = LocalParchmentPalette.current.accent
    val isPressed by interactionSource.collectIsPressedAsState()
    val isHovered by interactionSource.collectIsHoveredAsState()
    val isFocused by interactionSource.collectIsFocusedAsState()
    val reduceMotion = LocalReduceMotion.current
    // A click also moves focus, so the ring is kept for focus that arrives from the keyboard.
    var focusFromPointer by remember { mutableStateOf(false) }
    LaunchedEffect(isPressed, isFocused) {
        if (isPressed) focusFromPointer = true else if (!isFocused) focusFromPointer = false
    }
    val targetScale = when {
        !enabled || reduceMotion -> 1f
        isPressed -> pressScale
        isHovered -> hoverScale
        else -> 1f
    }
    val scale = if (reduceMotion || !enabled) {
        1f
    } else {
        animateFloatAsState(
            targetValue = targetScale,
            animationSpec = Motion.SCALE_SPRING
        ).value
    }
    val liftY = if (reduceMotion || !enabled) {
        0f
    } else {
        animateFloatAsState(
            targetValue = if (isHovered && !isPressed) -1.2f else 0f,
            animationSpec = Motion.LIFT_SPRING
        ).value
    }
    return this
        .then(if (enabled) Modifier.pointerHoverIcon(PointerIcon.Hand) else Modifier)
        .graphicsLayer {
            scaleX = scale
            scaleY = scale
            translationY = liftY
        }
        .then(if (selected != null) Modifier.semantics { this.selected = selected } else Modifier)
        .sepiaFocusRing(isFocused && !focusFromPointer, focusColor)
        .clickable(
            interactionSource = interactionSource,
            indication = null,
            enabled = enabled,
            onClickLabel = onClickLabel,
            role = role,
            onClick = {
                runCatching {
                    haptics.performHapticFeedback(
                        if (strongHaptic) HapticFeedbackType.LongPress else HapticFeedbackType.TextHandleMove
                    )
                }
                onClick()
            }
        )
}

internal fun Modifier.sepiaFocusRing(focused: Boolean, color: Color): Modifier =
    if (focused) border(LineWidth.focus, color, REGULAR_BTN_SHAPE) else this

/** Links from model output and imported chats may only open in a browser, never as local files. */
internal fun openWebLink(target: String) {
    if (isWebLink(target)) PlatformStorage.openOrPlayMedia(target.trim())
}

/** Names a control that has no visible text. */
fun Modifier.accessibilityLabel(label: String?): Modifier =
    if (label == null) this else semantics { contentDescription = label }

/** Dismisses from a scrim by pointer only, so it adds no reader target and absorbs no dialog text. */
@Composable
fun Modifier.dismissOnOutsideTap(onDismiss: () -> Unit): Modifier {
    val currentOnDismiss by rememberUpdatedState(onDismiss)
    return pointerInput(Unit) { detectTapGestures(onTap = { currentOnDismiss() }) }
}

/** Stops taps inside a sheet from reaching its scrim without merging the sheet's semantics. */
fun Modifier.blockTapThrough(): Modifier = pointerInput(Unit) { detectTapGestures(onTap = {}) }

@Composable
fun SepiaButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    style: SepiaButtonStyle = SepiaButtonStyle.OUTLINE,
    enabled: Boolean = true,
    compact: Boolean = false,
    wrapText: Boolean = false,
    contentDescription: String? = null,
    hint: String? = contentDescription,
    tabularNums: Boolean = false
) {
    val p = LocalParchmentPalette.current
    val interactionSource = remember { MutableInteractionSource() }
    val isHovered by interactionSource.collectIsHoveredAsState()
    val isPressed by interactionSource.collectIsPressedAsState()
    val shape = if (compact) COMPACT_BTN_SHAPE else REGULAR_BTN_SHAPE

    val baseBg = when (style) {
        SepiaButtonStyle.PRIMARY -> if (enabled) p.accent else p.border.copy(alpha = 0.55f)
        SepiaButtonStyle.SECONDARY -> if (enabled) p.elevatedBg else p.border.copy(alpha = 0.55f)
        SepiaButtonStyle.OUTLINE -> if (enabled) p.cardBg else p.elevatedBg.copy(alpha = 0.4f)
        SepiaButtonStyle.GHOST -> Color.Transparent
        SepiaButtonStyle.DANGER -> if (enabled) p.danger else p.border.copy(alpha = 0.55f)
    }

    val bgColor = when {
        !enabled -> baseBg
        isPressed -> when (style) {
            SepiaButtonStyle.PRIMARY, SepiaButtonStyle.SECONDARY, SepiaButtonStyle.DANGER -> baseBg.copy(alpha = 0.82f)
            SepiaButtonStyle.OUTLINE -> p.accent.copy(alpha = 0.18f)
            SepiaButtonStyle.GHOST -> p.accent.copy(alpha = 0.16f)
        }
        isHovered -> when (style) {
            SepiaButtonStyle.PRIMARY, SepiaButtonStyle.SECONDARY, SepiaButtonStyle.DANGER -> baseBg.copy(alpha = 0.91f)
            SepiaButtonStyle.OUTLINE -> p.accent.copy(alpha = 0.09f)
            SepiaButtonStyle.GHOST -> p.accent.copy(alpha = 0.09f)
        }
        else -> baseBg
    }

    val textColor = when (style) {
        SepiaButtonStyle.PRIMARY, SepiaButtonStyle.DANGER -> if (enabled) onFillInk(baseBg) else p.mutedInk
        SepiaButtonStyle.SECONDARY -> if (enabled) p.primaryInk else p.mutedInk
        SepiaButtonStyle.OUTLINE -> when {
            !enabled -> p.mutedInk.copy(alpha = Alpha.disabledInk)
            isPressed || isHovered -> p.accent
            else -> p.primaryInk
        }
        SepiaButtonStyle.GHOST -> when {
            !enabled -> p.mutedInk.copy(alpha = Alpha.disabledInk)
            isPressed || isHovered -> p.accent
            else -> p.narrationInk
        }
    }

    // A disabled filled button keeps an outline, so on dark themes it still reads as a button, just an unavailable one.
    val borderColor = when (style) {
        SepiaButtonStyle.PRIMARY -> if (enabled) p.accent else p.border
        SepiaButtonStyle.SECONDARY -> p.border
        SepiaButtonStyle.OUTLINE -> when {
            !enabled -> p.border.copy(alpha = 0.32f)
            isPressed -> p.accent
            isHovered -> p.accent.copy(alpha = 0.65f)
            else -> p.border.copy(alpha = 0.52f)
        }
        SepiaButtonStyle.GHOST -> if (enabled && (isPressed || isHovered)) p.accent.copy(alpha = 0.28f) else Color.Transparent
        SepiaButtonStyle.DANGER -> if (enabled) p.danger else p.border
    }

    val win = LocalParchmentWindowSize.current
    val parsed = remember(text) { parseKissLabelIcons(text) }
    val hPad = when {
        compact && win.isCompact -> 8.dp
        compact -> 10.dp
        win.isCompact -> 11.dp
        else -> 14.dp
    }
    val fontSize = when {
        compact && win.isVeryNarrow -> 12.sp
        compact -> 12.sp
        win.isCompact -> 13.sp
        else -> 13.sp
    }

    Box(
        modifier = modifier
            .kissClickable(
                enabled = enabled,
                strongHaptic = style == SepiaButtonStyle.PRIMARY || style == SepiaButtonStyle.DANGER,
                interactionSource = interactionSource,
                onClick = onClick
            )
            .accessibilityLabel(contentDescription ?: parsed.iconOnlyName())
            .clip(shape)
            .background(bgColor)
            .then(
                if (borderColor != Color.Transparent) {
                    Modifier.border(LineWidth.hairline, borderColor, shape)
                } else Modifier
            )
            .padding(
                horizontal = hPad,
                vertical = if (compact) 6.dp else 8.dp
            ),
        contentAlignment = Alignment.Center
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(if (compact) 4.dp else 6.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (parsed.leadingIcon != null) {
                KissVectorIcon(
                    icon = parsed.leadingIcon,
                    tint = textColor,
                    size = if (compact) IconSize.sm else IconSize.md
                )
            }
            if (parsed.cleanText.isNotEmpty()) {
                BasicText(
                    text = parsed.cleanText,
                    maxLines = if (wrapText) Int.MAX_VALUE else 1,
                    softWrap = wrapText,
                    overflow = TextOverflow.Ellipsis,
                    style = p.uiBodyStyle.copy(
                        color = textColor,
                        fontSize = fontSize,
                        fontFeatureSettings = if (tabularNums) "tnum" else null,
                        fontWeight = if (style == SepiaButtonStyle.PRIMARY || style == SepiaButtonStyle.SECONDARY) {
                            FontWeight.SemiBold
                        } else {
                            FontWeight.Medium
                        }
                    )
                )
            }
            if (parsed.trailingIcon != null) {
                KissVectorIcon(
                    icon = parsed.trailingIcon,
                    tint = textColor,
                    size = if (compact) IconSize.sm else 13.dp
                )
            }
        }
        HoverHint(hint, isHovered && !isPressed)
    }
}

@Composable
fun SepiaChip(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    contentDescription: String? = null,
    hint: String? = contentDescription
) {
    val p = LocalParchmentPalette.current
    val interactionSource = remember { MutableInteractionSource() }
    val isHovered by interactionSource.collectIsHoveredAsState()
    val isPressed by interactionSource.collectIsPressedAsState()
    val shape = COMPACT_BTN_SHAPE
    val bg = when {
        isPressed -> p.accent.copy(alpha = 0.24f)
        selected -> p.accent.copy(alpha = 0.15f)
        isHovered -> p.accent.copy(alpha = 0.09f)
        else -> p.cardBg.copy(alpha = 0.78f)
    }
    val borderCol = when {
        selected || isPressed -> p.accent
        isHovered -> p.accent.copy(alpha = 0.6f)
        else -> p.border.copy(alpha = 0.48f)
    }
    val txtCol = if (selected || isPressed || isHovered) p.accent else p.narrationInk

    val parsed = remember(label) { parseKissLabelIcons(label) }
    val win = LocalParchmentWindowSize.current

    Box(
        modifier = modifier
            .kissClickable(interactionSource = interactionSource, selected = selected, onClick = onClick)
            .accessibilityLabel(contentDescription ?: parsed.iconOnlyName())
            .clip(shape)
            .background(bg)
            .border(LineWidth.hairline, borderCol, shape)
            .padding(horizontal = if (win.isCompact) 7.dp else 9.dp, vertical = 5.dp),
        contentAlignment = Alignment.Center
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(5.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (parsed.leadingIcon != null) {
                KissVectorIcon(
                    icon = parsed.leadingIcon,
                    tint = txtCol,
                    size = IconSize.md
                )
            }
            if (parsed.cleanText.isNotEmpty()) {
                BasicText(
                    text = parsed.cleanText,
                    maxLines = 1,
                    softWrap = false,
                    overflow = TextOverflow.Ellipsis,
                    style = p.uiLabelStyle.copy(
                        color = txtCol,
                        fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal
                    )
                )
            }
            if (parsed.trailingIcon != null) {
                KissVectorIcon(
                    icon = parsed.trailingIcon,
                    tint = txtCol,
                    size = if (parsed.trailingIcon == KissIcon.CIRCLE_DOT) 8.dp else IconSize.sm
                )
            }
        }
        HoverHint(hint, isHovered && !isPressed)
    }
}

private const val HOVER_HINT_DELAY_MS = 450L

/** Centers a hint under its control, flipping above it and staying inside the window. */
internal object HoverHintPosition : PopupPositionProvider {
    private const val GAP = 6
    private const val MARGIN = 8

    override fun calculatePosition(
        anchorBounds: IntRect,
        windowSize: IntSize,
        layoutDirection: LayoutDirection,
        popupContentSize: IntSize
    ): IntOffset {
        val x = (anchorBounds.center.x - popupContentSize.width / 2)
            .coerceIn(MARGIN, (windowSize.width - popupContentSize.width - MARGIN).coerceAtLeast(MARGIN))
        val below = anchorBounds.bottom + GAP
        val fitsBelow = below + popupContentSize.height <= windowSize.height - MARGIN
        return IntOffset(x, if (fitsBelow) below else (anchorBounds.top - GAP - popupContentSize.height).coerceAtLeast(MARGIN))
    }
}

/**
 * A short explanation shown after the pointer rests on a control. Touch has no hover, and readers
 * already get the control's own name, so the hint is visual only.
 */
@Composable
internal fun HoverHint(text: String?, hovered: Boolean) {
    if (text.isNullOrBlank() || LocalParchmentWindowSize.current.isCompact) return
    var visible by remember { mutableStateOf(false) }
    LaunchedEffect(hovered) {
        if (hovered) delay(HOVER_HINT_DELAY_MS)
        visible = hovered
    }
    if (!visible) return
    val p = LocalParchmentPalette.current
    Popup(popupPositionProvider = HoverHintPosition, properties = PopupProperties(focusable = false)) {
        Box(
            Modifier
                .semantics { hideFromAccessibility() }
                .widthIn(max = 280.dp)
                .clip(RoundedCornerShape(Radius.r6))
                .background(p.primaryInk)
                .padding(horizontal = 9.dp, vertical = 5.dp)
        ) {
            BasicText(text, style = p.uiLabelStyle.copy(color = p.canvasBg, fontWeight = FontWeight.Normal))
        }
    }
}

/** The lapis rule that marks prompts on the page and the current item in lists. */
fun Modifier.rubricMark(color: Color, verticalInset: Dp = 0.dp): Modifier = drawBehind {
    val inset = verticalInset.toPx()
    drawRoundRect(
        color = color,
        topLeft = Offset(0f, inset),
        size = Size(2.5.dp.toPx(), size.height - inset * 2),
        cornerRadius = CornerRadius(2.dp.toPx())
    )
}
