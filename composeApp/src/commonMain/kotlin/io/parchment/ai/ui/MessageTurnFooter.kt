package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupProperties
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.platform.PlatformVoice
import io.parchment.ai.protocol.O200kTokenizer
import io.parchment.ai.strings.Strings

/**
 * Turn footer containing copy/speak/branch/edit/delete actions and assistant token/cost stats.
 */
@Composable
internal fun MessageTurnActionFooter(
    node: MessageNode,
    activeVar: MessageVariant,
    isAssistant: Boolean,
    isLatestTurn: Boolean,
    isSentBubble: Boolean,
    preferences: PreferencesConfig,
    footerAlpha: () -> Float = { 1f },
    copiedBadge: String,
    onCopiedBadgeChange: (String) -> Unit,
    showMoreActions: Boolean,
    onToggleMoreActions: () -> Unit,
    showSourcesDrawer: Boolean,
    onToggleSourcesDrawer: () -> Unit,
    openUsageDetailsId: String?,
    onOpenUsageDetailsIdChange: (String?) -> Unit,
    onFooterFocusChange: (Boolean) -> Unit,
    onToggleEditing: () -> Unit,
    onToggleContextMute: () -> Unit,
    onTogglePin: () -> Unit,
    onDeleteCurrentVariant: () -> Unit,
    onDeleteNode: () -> Unit,
    onForkHere: () -> Unit,
    onRewindHere: () -> Unit,
    onRegenerate: () -> Unit,
    onTranslate: () -> Unit,
    onContinue: () -> Unit,
    onSaveAsSkill: (() -> Unit)? = null,
    onToggleRawWireDrawer: () -> Unit
) {
    if (!isAssistant && !preferences.showUserMessageActions) return
    val win = LocalParchmentWindowSize.current
    @Suppress("DEPRECATION")
    val clipboard = LocalClipboardManager.current

    Column(
        modifier = Modifier.fillMaxWidth().graphicsLayer { alpha = footerAlpha() }.onFocusChanged { onFooterFocusChange(it.hasFocus) },
        verticalArrangement = Arrangement.spacedBy(Space.s4)
    ) {
        @OptIn(ExperimentalLayoutApi::class)
        FlowRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = if (isSentBubble) Arrangement.spacedBy(4.dp, Alignment.End) else Arrangement.spacedBy(Space.s4),
            verticalArrangement = Arrangement.spacedBy(Space.s4)
        ) {
            var isSpeakingThisTurn by remember(node.id, node.activeVariantIndex) { mutableStateOf(false) }

            // On touch only the newest turns show every action; older ones open from the More button.
            if (!win.isCompact || isLatestTurn || showMoreActions) {
                SepiaButton(
                    text = if (copiedBadge == "txt") "✓" else "📋",
                    contentDescription = if (copiedBadge == "txt") Strings.copied else Strings.copy,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = {
                        clipboard.setText(AnnotatedString(stripMarkdownToPlainText(activeVar.content)))
                        onCopiedBadgeChange("txt")
                    }
                )
                if (activeVar.content.isNotBlank()) {
                    SepiaButton(
                        text = if (isSpeakingThisTurn) "🔇" else "🔊",
                        contentDescription = if (isSpeakingThisTurn) Strings.stopReadingAloud else Strings.readAloud,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = {
                            if (isSpeakingThisTurn) {
                                PlatformVoice.stopSpeakingAloud()
                                isSpeakingThisTurn = false
                            } else {
                                isSpeakingThisTurn = true
                                PlatformVoice.speakTextAloud(
                                    text = stripMarkdownToPlainText(activeVar.content),
                                    onFinished = { isSpeakingThisTurn = false }
                                )
                            }
                        }
                    )
                }
                SepiaButton(
                    text = "↻",
                    contentDescription = Strings.regenerateResponse,
                    compact = true,
                    enabled = !activeVar.isStreaming,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onRegenerate
                )
            }
            // A pinned reply always shows its pin, so the state is visible even where other actions are folded away.
            if (isAssistant && (!win.isCompact || isLatestTurn || showMoreActions || node.pinned)) {
                SepiaButton(
                    text = "📌",
                    contentDescription = if (node.pinned) Strings.unpinReply else Strings.pinReply,
                    hint = if (node.pinned) Strings.removeFromPinnedInTheSidebar else Strings.keepThisReplyInPinnedIn,
                    compact = true,
                    style = if (node.pinned) SepiaButtonStyle.OUTLINE else SepiaButtonStyle.GHOST,
                    onClick = onTogglePin
                )
            }
            SepiaButton(
                text = if (showMoreActions) "▴" else "⋯",
                contentDescription = if (showMoreActions) Strings.hideMoreActions else Strings.moreActions,
                compact = true,
                style = SepiaButtonStyle.GHOST,
                onClick = onToggleMoreActions
            )
            if (isAssistant && activeVar.webSources.isNotEmpty()) {
                val srcCount = activeVar.webSources.size
                val srcLabel = if (srcCount == 1) "🌐 1 Source" else "🌐 $srcCount Sources"
                val chevron = if (showSourcesDrawer) " ▴" else " ▾"
                SepiaChip(
                    label = "$srcLabel$chevron",
                    selected = showSourcesDrawer,
                    onClick = onToggleSourcesDrawer
                )
            }

            if (showMoreActions) {
                SepiaButton(
                    text = if (copiedBadge == "md") "✓ Markdown" else "📋 Markdown",
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = {
                        clipboard.setText(AnnotatedString(activeVar.content))
                        onCopiedBadgeChange("md")
                    }
                )
                if (isAssistant) {
                    SepiaButton(
                        text = Strings.continueLabel2,
                        compact = true,
                        enabled = !activeVar.isStreaming,
                        style = SepiaButtonStyle.GHOST,
                        onClick = onContinue
                    )
                }
                SepiaButton(
                    text = "✎",
                    contentDescription = Strings.editMessage,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onToggleEditing
                )
                if (activeVar.content.isNotBlank()) {
                    SepiaButton(
                        text = "🌐",
                        contentDescription = Strings.translateMessage,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = onTranslate
                    )
                    if (isAssistant && onSaveAsSkill != null) {
                        SepiaButton(
                            text = Strings.saveAsSkill,
                            compact = true,
                            style = SepiaButtonStyle.GHOST,
                            onClick = {
                                onToggleMoreActions()
                                onSaveAsSkill()
                            }
                        )
                    }
                }
                SepiaButton(
                    text = Strings.branch,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onForkHere
                )
                SepiaButton(
                    text = Strings.rewind,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onRewindHere
                )
                SepiaButton(
                    text = if (node.includedInContext) "🙈" else "👁",
                    contentDescription = if (node.includedInContext) Strings.excludeFromContext else Strings.includeInContext,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onToggleContextMute
                )
                if (activeVar.rawRequestPreview.isNotBlank()) {
                    SepiaButton(
                        text = "‹/›",
                        contentDescription = Strings.rawRequestAndResponse,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = onToggleRawWireDrawer
                    )
                }
                if (node.variants.size > 1) {
                    SepiaButton(
                        text = "🗑 ${node.activeVariantIndex + 1}/${node.variants.size}",
                        contentDescription = "${Strings.deleteMessage} (${node.activeVariantIndex + 1}/${node.variants.size})",
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = onDeleteCurrentVariant
                    )
                }
                SepiaButton(
                    text = "🗑",
                    contentDescription = Strings.deleteMessage,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onDeleteNode
                )
            }
        }

        // Token counts for a reply, with written text counted apart from tool payloads.
        if (isAssistant && preferences.showTokenAndContextStats && (!win.isCompact || showMoreActions) &&
            (activeVar.content.isNotBlank() || activeVar.toolCalls.isNotEmpty() || activeVar.tokenMetrics.outputTokens > 0)
        ) {
            AssistantTurnUsageStats(
                activeVar = activeVar,
                preferences = preferences,
                showMoreActions = showMoreActions,
                openUsageDetailsId = openUsageDetailsId,
                onOpenUsageDetailsIdChange = onOpenUsageDetailsIdChange
            )
        }
    }
}

/**
 * Reasoning tokens for the footer: a constant-time estimate while streaming or until the exact count is ready,
 * then the exact o200k count from the shared per-variant cache, computed on [Dispatchers.Default] because a long
 * reasoning summary takes a regex pass the UI thread should never run.
 */
@Composable
private fun rememberThinkingTokens(variant: MessageVariant, reportedReasoningTokens: Long): Long {
    val thinking = variant.thinkingSummary
    val bucketKey = if (variant.isStreaming) thinking.length / 64 else thinking.length
    val estimate = remember(bucketKey, variant.isStreaming, reportedReasoningTokens) {
        if (thinking.isBlank()) reportedReasoningTokens else O200kTokenizer.estimateTokensFast(thinking).toLong()
    }
    val exact by produceState<Long?>(null, variant.id, thinking, variant.isStreaming) {
        value = if (variant.isStreaming || thinking.isBlank()) null else withContext(Dispatchers.Default) {
            O200kTokenizer.completedVariantTokenBreakdown(variant).thinkingTokens.toLong()
        }
    }
    return exact ?: estimate
}

@Composable
private fun rememberVariantProseTokens(
    role: MessageRole,
    variant: MessageVariant,
    isStreaming: Boolean
): Int {
    val bucketKey = if (isStreaming) variant.content.length / 64 else variant.content.hashCode()
    val fastEstimate = remember(bucketKey, isStreaming, variant.attachments) {
        4 + O200kTokenizer.estimateTokensFast(variant.content) +
            variant.attachments.sumOf { O200kTokenizer.countAttachmentTokens(it) }
    }
    if (isStreaming) return fastEstimate
    val hasHeavyContent = variant.content.length > 1500 || variant.attachments.any { it.textContent.length > 1500 }
    if (!hasHeavyContent) {
        return remember(variant.id, variant.content, variant.attachments, role) {
            O200kTokenizer.countMessageVariantTokens(role, variant, includeReasoningAndTools = false)
        }
    }
    val exactCount by produceState(
        initialValue = fastEstimate,
        role,
        variant.id,
        variant.content,
        variant.attachments
    ) {
        value = withContext(Dispatchers.Default) {
            O200kTokenizer.countMessageVariantTokens(role, variant, includeReasoningAndTools = false)
        }
    }
    return exactCount
}

@Composable
private fun AssistantTurnUsageStats(
    activeVar: MessageVariant,
    preferences: PreferencesConfig,
    showMoreActions: Boolean,
    openUsageDetailsId: String?,
    onOpenUsageDetailsIdChange: (String?) -> Unit
) {
    val p = LocalParchmentPalette.current
    val m = activeVar.tokenMetrics
    val countedProse = rememberVariantProseTokens(MessageRole.ASSISTANT, activeVar, activeVar.isStreaming)
    val outProseTok = if (activeVar.content.isNotBlank()) countedProse.toLong() else m.outputTokens
    val toolPayloadTok = remember(activeVar.toolCalls, activeVar.isStreaming) {
        activeVar.toolCalls.sumOf { tc ->
            O200kTokenizer.countToolCallTokens(tc, isLiveOverride = activeVar.isStreaming)
        }
    }
    val thinkTok = rememberThinkingTokens(activeVar, m.reasoningTokens)
    val costVal = if (m.reportedRequests > 0 || m.estimatedCostUsd > 0.0) {
        m.estimatedCostUsd
    } else {
        0.0
    }
    val hasReportedRequests = m.reportedRequests > 0
    val costUnavailable = hasReportedRequests && !m.costIsAvailable && costVal == 0.0
    val costIsKnown = costVal > 0.0 || (hasReportedRequests && m.costIsAvailable)
    val isPartial = hasReportedRequests && (!m.usageIsComplete || !m.costIsAvailable || m.unpricedCostComponents.isNotEmpty())
    val isReportedExact = hasReportedRequests && m.reportedCostRequests == m.reportedRequests &&
        m.costIsAvailable && m.usageIsComplete && m.unpricedCostComponents.isEmpty()
    val hasWholeTurnCounts = m.inputTokens > 0L || m.outputTokens > 0L ||
        m.cachedInputTokens > 0L || m.cacheCreationInputTokens > 0L
    val showCostUnavailableBadge = costUnavailable && !activeVar.isStreaming
    val usageDetailLines = remember(
        costUnavailable, hasWholeTurnCounts, hasReportedRequests,
        m.inputTokens, m.outputTokens, m.cachedInputTokens, m.cacheCreationInputTokens,
        m.reportedRequests, m.usageIsComplete, m.costIsAvailable, m.reportedCostRequests,
        costVal, m.outputIncludesUnreportedReasoning, m.unpricedCostComponents
    ) {
        buildList {
            if (costUnavailable) add(Strings.costUnavailable)
            if (hasWholeTurnCounts || hasReportedRequests) {
                add(Strings.inputOutput(formatCompactTokens(m.inputTokens), formatCompactTokens(m.outputTokens)))
            }
            if (m.cachedInputTokens > 0L || m.cacheCreationInputTokens > 0L) {
                add(Strings.reusedSavedForReuse(formatCompactTokens(m.cachedInputTokens), formatCompactTokens(m.cacheCreationInputTokens)))
            }
            if (hasReportedRequests) {
                val status = when {
                    !m.usageIsComplete || !m.costIsAvailable -> Strings.usagePartialSuffix
                    m.reportedCostRequests == m.reportedRequests -> Strings.usageReportedCostSuffix
                    else -> Strings.usageEstimateSuffix
                }
                add(Strings.request(m.reportedRequests, if (m.reportedRequests == 1) "" else "s") + status)
            } else if (costVal > 0.0) {
                add(Strings.estimatedCost)
            }
            if (m.outputIncludesUnreportedReasoning) add(Strings.thinkingCountedInOutput)
            if (m.unpricedCostComponents.isNotEmpty()) {
                add(Strings.unpriced(m.unpricedCostComponents.sorted().joinToString(", ")))
            }
        }
    }

    @OptIn(ExperimentalLayoutApi::class)
    FlowRow(
        modifier = Modifier.fillMaxWidth().padding(top = Space.s2),
        horizontalArrangement = Arrangement.spacedBy(5.dp),
        verticalArrangement = Arrangement.spacedBy(Space.s4),
        itemVerticalAlignment = Alignment.CenterVertically
    ) {
        // Pill 1: output tokens, with reasoning and tool payload tokens shown separately.
        Box(modifier = Modifier) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s4)
            ) {
                BasicText(
                    text = Strings.tokens(formatCompactTokens(outProseTok)),
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.narrationInk, fontWeight = FontWeight.Medium)
                )
                if (thinkTok > 0) {
                    BasicText(
                        text = "•",
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk)
                    )
                    KissVectorIcon(
                        icon = KissIcon.BRAIN,
                        tint = p.accent,
                        size = IconSize.xs
                    )
                    BasicText(
                        text = formatCompactTokens(thinkTok),
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.narrationInk, fontWeight = FontWeight.Medium)
                    )
                }
                if (toolPayloadTok > 0) {
                    BasicText(
                        text = "•",
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk)
                    )
                    KissVectorIcon(
                        icon = KissIcon.WRENCH,
                        tint = p.accentSecondary,
                        size = 10.dp
                    )
                    BasicText(
                        text = formatCompactTokens(toolPayloadTok),
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk, fontWeight = FontWeight.Medium)
                    )
                }
            }
        }

        // Pill 2: Speed & Duration
        if (m.tokensPerSec > 0.0 || m.durationMs > 0L) {
            val durationSuffix = buildString {
                if (m.durationMs > 0L) {
                    if (m.tokensPerSec > 0.0) append(" • ")
                    append(formatDurationFromMs(m.durationMs))
                }
                if ((showMoreActions || preferences.showTokensForEntireTurn) && m.ttftMs > 0L) {
                    append(" (TTFT ${formatDurationFromMs(m.ttftMs)})")
                }
            }
            Box(modifier = Modifier) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s4)
                ) {
                    if (m.tokensPerSec > 0.0) {
                        KissVectorIcon(
                            icon = KissIcon.ZAP,
                            tint = p.accentSecondary,
                            size = 10.dp
                        )
                        BasicText(
                            text = Strings.tokS(m.tokensPerSec, durationSuffix),
                            style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk)
                        )
                    } else {
                        BasicText(
                            text = durationSuffix,
                            style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk)
                        )
                    }
                }
            }
        }

        // Pill 3: Cost / Unpriced Warning Badge (full breakdown on hover or tap)
        if (costIsKnown || showCostUnavailableBadge) {
            UsageDetailsPill(
                details = usageDetailLines,
                isOpen = openUsageDetailsId == "cost",
                onToggle = { onOpenUsageDetailsIdChange(if (openUsageDetailsId == "cost") null else "cost") },
                onDismiss = { if (openUsageDetailsId == "cost") onOpenUsageDetailsIdChange(null) },
                accessibilitySummary = if (!costIsKnown) usageDetailLines.joinToString(", ") else null
            ) { highlighted ->
                if (costIsKnown) {
                    BasicText(
                        text = (if (isReportedExact) "" else "~") + formatUsdCost(costVal),
                        style = p.uiLabelStyle.copy(
                            fontSize = 11.sp,
                            fontFeatureSettings = "tnum",
                            color = if (highlighted) p.accent else p.narrationInk,
                            fontWeight = FontWeight.Medium
                        )
                    )
                }
                if (showCostUnavailableBadge || (isPartial && !activeVar.isStreaming)) {
                    KissVectorIcon(
                        icon = KissIcon.WARNING,
                        tint = if (highlighted) p.accent else p.accentSecondary,
                        size = IconSize.sm
                    )
                }
            }
        }

        // Pill 4: Compact single-line input/output/cache summary when expanded
        if ((showMoreActions || preferences.showTokensForEntireTurn) && hasWholeTurnCounts) {
            UsageDetailsPill(
                details = usageDetailLines,
                isOpen = openUsageDetailsId == "turn",
                onToggle = { onOpenUsageDetailsIdChange(if (openUsageDetailsId == "turn") null else "turn") },
                onDismiss = { if (openUsageDetailsId == "turn") onOpenUsageDetailsIdChange(null) }
            ) { highlighted ->
                val tint = if (highlighted) p.accent else p.mutedInk
                val textColor = if (highlighted) p.accent else p.mutedInk
                KissVectorIcon(icon = KissIcon.ARROW_UP, tint = tint, size = IconSize.xs)
                BasicText(
                    text = formatCompactTokens(m.inputTokens),
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = textColor)
                )
                BasicText(
                    text = "•",
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, color = p.mutedInk)
                )
                KissVectorIcon(icon = KissIcon.ARROW_DOWN, tint = tint, size = IconSize.xs)
                BasicText(
                    text = formatCompactTokens(m.outputTokens),
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = textColor)
                )
                if (m.cachedInputTokens > 0L) {
                    BasicText(
                        text = "•",
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, color = p.mutedInk)
                    )
                    KissVectorIcon(
                        icon = KissIcon.REFRESH,
                        tint = if (highlighted) p.accent else p.accentSecondary,
                        size = IconSize.xs
                    )
                    BasicText(
                        text = formatCompactTokens(m.cachedInputTokens),
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = textColor)
                    )
                }
            }
        }
    }
}

/**
 * Compact token and input-price row displayed beneath a user message when token stats are enabled.
 */
@Composable
internal fun UserTurnUsageStats(
    activeVar: MessageVariant,
    showTokenAndContextStats: Boolean,
    isSentBubble: Boolean,
    inputPricePerM: Double,
    footerAlpha: () -> Float = { 1f }
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    if (win.isCompact || !showTokenAndContextStats || (activeVar.content.isBlank() && activeVar.attachments.isEmpty())) return
    val userTok = rememberVariantProseTokens(MessageRole.USER, activeVar, isStreaming = activeVar.isStreaming)
    val userCost = (userTok / 1_000_000.0) * inputPricePerM
    @OptIn(ExperimentalLayoutApi::class)
    FlowRow(
        modifier = Modifier.fillMaxWidth().padding(top = Space.s4).graphicsLayer { alpha = footerAlpha() },
        horizontalArrangement = if (isSentBubble) Arrangement.spacedBy(5.dp, Alignment.End) else Arrangement.spacedBy(5.dp),
        verticalArrangement = Arrangement.spacedBy(Space.s4)
    ) {
        val attSuffix = if (activeVar.attachments.isNotEmpty()) {
            " • ${activeVar.attachments.size} file${if (activeVar.attachments.size > 1) "s" else ""}"
        } else ""
        Box(modifier = Modifier) {
            BasicText(
                text = Strings.tok(formatCompactTokens(userTok), attSuffix),
                style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.mutedInk, fontWeight = FontWeight.Medium)
            )
        }
        if (userCost > 0.0) {
            Box(modifier = Modifier) {
                BasicText(
                    text = formatUsdCost(userCost),
                    style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFeatureSettings = "tnum", color = p.narrationInk, fontWeight = FontWeight.Medium)
                )
            }
        }
    }
}

@Composable
private fun UsageDetailsPill(
    details: List<String>,
    isOpen: Boolean,
    onToggle: () -> Unit,
    onDismiss: () -> Unit,
    accessibilitySummary: String? = null,
    content: @Composable RowScope.(highlighted: Boolean) -> Unit
) {
    val p = LocalParchmentPalette.current
    val interaction = remember { MutableInteractionSource() }
    val hovered by interaction.collectIsHoveredAsState()
    val pressed by interaction.collectIsPressedAsState()
    val detailsText = remember(details) { details.joinToString("\n") }
    val semanticSummary = remember(details) { details.joinToString(", ") }
    val shape = RoundedCornerShape(Radius.r4)

    Box(
        modifier = Modifier
            .clip(shape)
            .background(if (hovered || isOpen) p.accent.copy(alpha = 0.08f) else Color.Transparent)
            .kissClickable(interactionSource = interaction, onClick = onToggle)
            .then(
                if (accessibilitySummary != null) Modifier.accessibilityLabel(accessibilitySummary)
                else Modifier.semantics { if (semanticSummary.isNotBlank()) stateDescription = semanticSummary }
            )
            .padding(horizontal = Space.s4, vertical = Space.s2),
        contentAlignment = Alignment.Center
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.s4)
        ) {
            content(hovered || isOpen)
        }
        HoverHint(detailsText.takeIf { it.isNotBlank() }, hovered && !pressed && !isOpen)
        if (isOpen && details.isNotEmpty()) {
            Popup(
                popupPositionProvider = HoverHintPosition,
                onDismissRequest = onDismiss,
                properties = PopupProperties(focusable = true)
            ) {
                val cardShape = RoundedCornerShape(Radius.r8)
                Column(
                    modifier = Modifier
                        .widthIn(max = 280.dp)
                        .overlayShadow(cardShape)
                        .clip(cardShape)
                        .background(p.cardBg)
                        .border(LineWidth.hairline, p.border, cardShape)
                        .padding(horizontal = Space.s10, vertical = Space.s8),
                    verticalArrangement = Arrangement.spacedBy(3.dp)
                ) {
                    details.forEachIndexed { index, line ->
                        BasicText(
                            text = line,
                            style = p.uiLabelStyle.copy(
                                fontSize = 11.sp,
                                fontFeatureSettings = "tnum",
                                color = if (index == 0) p.primaryInk else p.mutedInk,
                                fontWeight = if (index == 0) FontWeight.Medium else FontWeight.Normal
                            )
                        )
                    }
                }
            }
        }
    }
}
