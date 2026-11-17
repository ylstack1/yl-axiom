package io.parchment.ai.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.FocusInteraction
import androidx.compose.foundation.interaction.HoverInteraction
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.PressInteraction
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusEventModifierNode
import androidx.compose.ui.focus.FocusState
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.ContentDrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerIcon
import androidx.compose.ui.input.pointer.pointerHoverIcon
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Measurable
import androidx.compose.ui.layout.MeasureResult
import androidx.compose.ui.layout.MeasureScope
import androidx.compose.ui.node.DrawModifierNode
import androidx.compose.ui.node.LayoutModifierNode
import androidx.compose.ui.node.ModifierNodeElement
import androidx.compose.ui.node.PointerInputModifierNode
import androidx.compose.ui.node.invalidateDraw
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
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
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

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

private data class KissMotionElement(
    val interactionSource: InteractionSource,
    val enabled: Boolean,
    val reduceMotion: Boolean,
    val pressScale: Float,
    val hoverScale: Float,
    val focusColor: Color
) : ModifierNodeElement<KissMotionNode>() {
    override fun create(): KissMotionNode = KissMotionNode(
        interactionSource = interactionSource,
        enabled = enabled,
        reduceMotion = reduceMotion,
        pressScale = pressScale,
        hoverScale = hoverScale,
        focusColor = focusColor
    )

    override fun update(node: KissMotionNode) {
        node.update(
            interactionSource = interactionSource,
            enabled = enabled,
            reduceMotion = reduceMotion,
            pressScale = pressScale,
            hoverScale = hoverScale,
            focusColor = focusColor
        )
    }
}

/**
 * Drives hover lift, press squeeze, and keyboard-only focus rings purely in the GPU layer and draw phases
 * (`Modifier.Node`), so pointer movement and focus traversal trigger zero Compose recompositions.
 */
private class KissMotionNode(
    private var interactionSource: InteractionSource,
    private var enabled: Boolean,
    private var reduceMotion: Boolean,
    private var pressScale: Float,
    private var hoverScale: Float,
    private var focusColor: Color
) : Modifier.Node(), LayoutModifierNode, DrawModifierNode, FocusEventModifierNode, PointerInputModifierNode {
    private val presses = HashSet<PressInteraction.Press>()
    private val hovers = HashSet<HoverInteraction.Enter>()
    private var hasFocus = false
    private var focusFromPointer = false
    private val showFocusRingState = mutableStateOf(false)

    private val scaleState = mutableFloatStateOf(1f)
    private val liftDpState = mutableFloatStateOf(0f)
    private val scaleAnim = Animatable(1f)
    private val liftAnim = Animatable(0f)
    private var lastTargetScale = 1f
    private var lastTargetLiftDp = 0f

    private var collectJob: Job? = null
    private var scaleJob: Job? = null
    private var liftJob: Job? = null

    private var cachedStrokePx = -1f
    private var cachedFocusStroke: Stroke? = null

    override fun onAttach() {
        restartInteractionCollector()
    }

    override fun onDetach() {
        resetState()
    }

    override fun onReset() {
        resetState()
    }

    override fun onFocusEvent(focusState: FocusState) {
        hasFocus = focusState.isFocused
        if (!hasFocus) focusFromPointer = false
        syncFocusRing()
    }

    override fun onPointerEvent(pointerEvent: PointerEvent, pass: PointerEventPass, bounds: IntSize) {
        if (pass == PointerEventPass.Initial && pointerEvent.type == PointerEventType.Press) {
            focusFromPointer = true
            syncFocusRing()
        }
    }

    override fun onCancelPointerInput() = Unit

    fun update(
        interactionSource: InteractionSource,
        enabled: Boolean,
        reduceMotion: Boolean,
        pressScale: Float,
        hoverScale: Float,
        focusColor: Color
    ) {
        val sourceChanged = this.interactionSource !== interactionSource
        val focusColorChanged = this.focusColor != focusColor
        this.interactionSource = interactionSource
        this.enabled = enabled
        this.reduceMotion = reduceMotion
        this.pressScale = pressScale
        this.hoverScale = hoverScale
        this.focusColor = focusColor
        if (sourceChanged && isAttached) {
            presses.clear()
            hovers.clear()
            restartInteractionCollector()
        } else if (isAttached) {
            syncTargets(animate = true)
            if (focusColorChanged && showFocusRingState.value) invalidateDraw()
        }
    }

    private fun resetState() {
        collectJob?.cancel()
        collectJob = null
        scaleJob?.cancel()
        scaleJob = null
        liftJob?.cancel()
        liftJob = null
        presses.clear()
        hovers.clear()
        hasFocus = false
        focusFromPointer = false
        showFocusRingState.value = false
        lastTargetScale = 1f
        lastTargetLiftDp = 0f
        scaleState.floatValue = 1f
        liftDpState.floatValue = 0f
    }

    private fun syncFocusRing() {
        val nextFocusRing = enabled && hasFocus && !focusFromPointer
        if (nextFocusRing != showFocusRingState.value) {
            showFocusRingState.value = nextFocusRing
            if (isAttached) invalidateDraw()
        }
    }

    private fun restartInteractionCollector() {
        collectJob?.cancel()
        val source = interactionSource
        collectJob = coroutineScope.launch(start = CoroutineStart.UNDISPATCHED) {
            source.interactions.collect { interaction ->
                when (interaction) {
                    is PressInteraction.Press -> {
                        presses.add(interaction)
                        focusFromPointer = true
                    }
                    is PressInteraction.Release -> presses.remove(interaction.press)
                    is PressInteraction.Cancel -> presses.remove(interaction.press)
                    is HoverInteraction.Enter -> hovers.add(interaction)
                    is HoverInteraction.Exit -> hovers.remove(interaction.enter)
                    is FocusInteraction.Focus -> hasFocus = true
                    is FocusInteraction.Unfocus -> {
                        hasFocus = false
                        focusFromPointer = false
                    }
                }
                syncTargets(animate = true)
            }
        }
        syncTargets(animate = false)
    }

    private fun syncTargets(animate: Boolean) {
        syncFocusRing()
        val targetScale = when {
            !enabled || reduceMotion -> 1f
            presses.isNotEmpty() -> pressScale
            hovers.isNotEmpty() -> hoverScale
            else -> 1f
        }
        val targetLiftDp = when {
            !enabled || reduceMotion -> 0f
            hovers.isNotEmpty() && presses.isEmpty() -> -0.75f
            else -> 0f
        }
        if (!animate || !enabled || reduceMotion || !isAttached) {
            scaleJob?.cancel()
            liftJob?.cancel()
            lastTargetScale = targetScale
            lastTargetLiftDp = targetLiftDp
            scaleState.floatValue = targetScale
            liftDpState.floatValue = targetLiftDp
            return
        }
        if (targetScale != lastTargetScale) {
            lastTargetScale = targetScale
            scaleJob?.cancel()
            scaleJob = coroutineScope.launch {
                if (scaleAnim.value != scaleState.floatValue) scaleAnim.snapTo(scaleState.floatValue)
                scaleAnim.animateTo(targetScale, Motion.SCALE_SPRING) {
                    scaleState.floatValue = value
                }
            }
        }
        if (targetLiftDp != lastTargetLiftDp) {
            lastTargetLiftDp = targetLiftDp
            liftJob?.cancel()
            liftJob = coroutineScope.launch {
                if (liftAnim.value != liftDpState.floatValue) liftAnim.snapTo(liftDpState.floatValue)
                liftAnim.animateTo(targetLiftDp, Motion.LIFT_SPRING) {
                    liftDpState.floatValue = value
                }
            }
        }
    }

    override fun MeasureScope.measure(measurable: Measurable, constraints: Constraints): MeasureResult {
        val placeable = measurable.measure(constraints)
        return layout(placeable.width, placeable.height) {
            placeable.placeWithLayer(0, 0) {
                val s = scaleState.floatValue
                scaleX = s
                scaleY = s
                translationY = liftDpState.floatValue.dp.toPx()
            }
        }
    }

    override fun ContentDrawScope.draw() {
        drawContent()
        if (showFocusRingState.value && enabled) {
            val strokePx = LineWidth.focus.toPx()
            val stroke = if (strokePx == cachedStrokePx && cachedFocusStroke != null) {
                cachedFocusStroke!!
            } else {
                Stroke(width = strokePx).also {
                    cachedStrokePx = strokePx
                    cachedFocusStroke = it
                }
            }
            val half = strokePx / 2f
            val r = Radius.r8.toPx()
            drawRoundRect(
                color = focusColor,
                topLeft = Offset(half, half),
                size = Size((size.width - strokePx).coerceAtLeast(0f), (size.height - strokePx).coerceAtLeast(0f)),
                cornerRadius = CornerRadius(r, r),
                style = stroke
            )
        }
    }
}

/**
 * Clickable with layer-phase feedback:
 * - Hover (pointer devices): hand cursor and a slight GPU-layer lift (zero recomposition).
 * - Press: spring squeeze and haptic feedback.
 * - Keyboard focus: 2 dp accent focus ring drawn without layout shift.
 */
@Composable
fun Modifier.kissClickable(
    enabled: Boolean = true,
    pressScale: Float = 0.96f,
    hoverScale: Float = 1.012f,
    strongHaptic: Boolean = false,
    interactionSource: MutableInteractionSource = remember { MutableInteractionSource() },
    role: Role? = Role.Button,
    selected: Boolean? = null,
    onClickLabel: String? = null,
    onClick: () -> Unit
): Modifier {
    val haptics = LocalHapticFeedback.current
    val focusColor = LocalParchmentPalette.current.accent
    val reduceMotion = LocalReduceMotion.current
    return this
        .then(if (enabled) Modifier.pointerHoverIcon(PointerIcon.Hand) else Modifier)
        .then(
            KissMotionElement(
                interactionSource = interactionSource,
                enabled = enabled,
                reduceMotion = reduceMotion,
                pressScale = pressScale,
                hoverScale = hoverScale,
                focusColor = focusColor
            )
        )
        .then(if (selected != null) Modifier.semantics { this.selected = selected } else Modifier)
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
        SepiaButtonStyle.OUTLINE -> if (enabled) p.cardBg else p.elevatedBg.copy(alpha = 0.45f)
        SepiaButtonStyle.GHOST -> Color.Transparent
        SepiaButtonStyle.DANGER -> if (enabled) p.danger else p.border.copy(alpha = 0.55f)
    }

    val bgColor = when {
        !enabled -> baseBg
        isPressed -> when (style) {
            SepiaButtonStyle.PRIMARY, SepiaButtonStyle.SECONDARY, SepiaButtonStyle.DANGER -> baseBg.copy(alpha = 0.84f)
            SepiaButtonStyle.OUTLINE -> p.accent.copy(alpha = 0.16f)
            SepiaButtonStyle.GHOST -> p.accent.copy(alpha = 0.14f)
        }
        isHovered -> when (style) {
            SepiaButtonStyle.PRIMARY, SepiaButtonStyle.SECONDARY, SepiaButtonStyle.DANGER -> baseBg.copy(alpha = 0.92f)
            SepiaButtonStyle.OUTLINE -> p.accent.copy(alpha = 0.08f)
            SepiaButtonStyle.GHOST -> p.accent.copy(alpha = 0.08f)
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
            !enabled -> p.border.copy(alpha = 0.45f)
            isPressed -> p.accent
            isHovered -> p.accent.copy(alpha = 0.78f)
            else -> p.border
        }
        SepiaButtonStyle.GHOST -> if (enabled && (isPressed || isHovered)) p.accent.copy(alpha = 0.30f) else Color.Transparent
        SepiaButtonStyle.DANGER -> if (enabled) p.danger else p.border
    }

    val win = LocalParchmentWindowSize.current
    val parsed = remember(text) { parseKissLabelIcons(text) }
    val minHeight = if (compact) ControlHeight.compact else ControlHeight.regular
    val hPad = when {
        compact && win.isCompact -> Space.s8
        compact -> Space.s10
        win.isCompact -> Space.s12
        else -> 14.dp
    }
    val fontSize = when {
        compact -> 12.sp
        else -> 13.sp
    }

    Box(
        modifier = modifier
            .heightIn(min = minHeight)
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
                    Modifier.border(LineWidth.emphasis, borderColor, shape)
                } else Modifier
            )
            .padding(
                horizontal = hPad,
                vertical = if (compact) Space.s6 else Space.s8
            ),
        contentAlignment = Alignment.Center
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(if (compact) Space.s4 else Space.s6),
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
                    modifier = if (contentDescription != null) Modifier.clearAndSetSemantics {} else Modifier,
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
                    size = if (compact) IconSize.sm else IconSize.md
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
        isPressed -> p.accent.copy(alpha = 0.22f)
        selected -> p.accent.copy(alpha = 0.14f)
        isHovered -> p.accent.copy(alpha = 0.08f)
        else -> p.cardBg
    }
    val borderCol = when {
        selected || isPressed -> p.accent
        isHovered -> p.accent.copy(alpha = 0.7f)
        else -> p.border
    }
    val txtCol = if (selected || isPressed || isHovered) p.accent else p.narrationInk

    val parsed = remember(label) { parseKissLabelIcons(label) }
    val win = LocalParchmentWindowSize.current

    Box(
        modifier = modifier
            .heightIn(min = ControlHeight.compact)
            .kissClickable(interactionSource = interactionSource, selected = selected, onClick = onClick)
            .accessibilityLabel(contentDescription ?: parsed.iconOnlyName())
            .clip(shape)
            .background(bg)
            .border(if (selected) LineWidth.emphasis else LineWidth.hairline, borderCol, shape)
            .padding(horizontal = if (win.isCompact) Space.s8 else Space.s10, vertical = Space.s6),
        contentAlignment = Alignment.Center
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.s4),
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
                    modifier = if (contentDescription != null) Modifier.clearAndSetSemantics {} else Modifier,
                    maxLines = 1,
                    softWrap = false,
                    overflow = TextOverflow.Ellipsis,
                    style = p.uiLabelStyle.copy(
                        color = txtCol,
                        fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium
                    )
                )
            }
            if (parsed.trailingIcon != null) {
                KissVectorIcon(
                    icon = parsed.trailingIcon,
                    tint = txtCol,
                    size = if (parsed.trailingIcon == KissIcon.CIRCLE_DOT) Space.s8 else IconSize.sm
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
                .padding(horizontal = Space.s10, vertical = Space.s6)
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
