package io.parchment.ai.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.hoverable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.ViewMode
import io.parchment.ai.model.turnTextLayout
import io.parchment.ai.strings.Strings

// One message: its reasoning (open while the model thinks, folded when done), the text, copy actions,
// and the footer, where actions sit above the token counts so More never hides buttons on a phone.
private const val MAX_SHRINKABLE_BUBBLE_CHARS = 800
private const val SENT_BUBBLE_WIDTH_FRACTION = 0.84f
private const val REPLY_BUBBLE_WIDTH_FRACTION = 0.94f

/** A messenger bubble: yours on the right, the assistant's on the left, each with one tucked corner. */
private fun Modifier.messageBubble(fromUser: Boolean, shrink: Boolean, fill: Color, edge: Color): Modifier {
    val shape = RoundedCornerShape(16.dp, 16.dp, if (fromUser) 4.dp else 16.dp, if (fromUser) 16.dp else 4.dp)
    return layout { measurable, constraints ->
        val capped = constraints.copy(minWidth = 0, maxWidth = (constraints.maxWidth * if (fromUser) SENT_BUBBLE_WIDTH_FRACTION else REPLY_BUBBLE_WIDTH_FRACTION).toInt())
        val bubble = measurable.measure(capped)
        layout(bubble.width, bubble.height) { bubble.place(0, 0) }
    }
        .then(if (shrink) Modifier.width(IntrinsicSize.Max) else Modifier)
        .clip(shape)
        .background(fill)
        .border(LineWidth.hairline, edge, shape)
        .padding(horizontal = 13.dp, vertical = 9.dp)
}

@Composable
internal fun MessageTurnItem(
    node: MessageNode,
    nodeIndex: Int,
    isLatestTurn: Boolean = false,
    userPicture: ImageBitmap? = null,
    viewMode: ViewMode,
    preferences: PreferencesConfig,
    userName: String,
    characterName: String,
    assistantIconEmoji: String = "💬",
    providerName: String,
    inputPricePerM: Double = 0.0,
    outputPricePerM: Double = 0.0,
    isActivelyThinkingThisNode: Boolean,
    onSelectVariant: (Int) -> Unit,
    onSaveEditedText: (String, Boolean, Boolean) -> Unit,
    onToggleContextMute: () -> Unit,
    onTogglePin: () -> Unit = {},
    onDeleteCurrentVariant: () -> Unit = {},
    onDeleteNode: () -> Unit,
    onForkHere: () -> Unit,
    onRewindHere: () -> Unit = {},
    onRegenerate: () -> Unit,
    onTranslate: () -> Unit = {},
    onContinue: () -> Unit,
    onSaveAsSkill: (() -> Unit)? = null,
    onInspectAttachment: (PromptAttachment) -> Unit = {},
    onInspectGeneratedImage: (GeneratedMediaItem) -> Unit = {}
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    // While this reply streams, its text comes from its live state (read here, so frames redraw only this item).
    val liveTurn = LiveTurns.stateOf(node.activeVariant.id)?.value
    val activeVar = remember(node.activeVariant, liveTurn) { node.activeVariant.withLiveTurn(liveTurn) }
    val isAssistant = node.role == MessageRole.ASSISTANT

    var isEditing by remember(node.id, node.activeVariantIndex) { mutableStateOf(false) }
    var editBuffer by remember(node.id, node.activeVariantIndex) { mutableStateOf("") }
    var manualThinkingToggle by remember(node.id, node.activeVariantIndex) { mutableStateOf<Boolean?>(null) }
    var showRawWireDrawer by remember(node.id) { mutableStateOf(false) }
    var showMoreActions by remember(node.id) { mutableStateOf(false) }
    var openUsageDetailsId by remember(node.id, node.activeVariantIndex) { mutableStateOf<String?>(null) }
    var showSourcesDrawer by remember(node.id, node.activeVariantIndex) { mutableStateOf(false) }
    var highlightedSourceLineText by remember(node.id, node.activeVariantIndex) { mutableStateOf("") }
    var copiedBadge by remember(node.id, node.activeVariantIndex) { mutableStateOf("") }
    val enableMd = if (isAssistant) preferences.renderAssistantMessagesWithMarkdown else preferences.renderUserMessagesWithMarkdown
    // How reasoning is shown:
    // - While the model reasons: all thinking and tool steps are open by default; Hide switches to a peek
    //   (`isLivePeekOnly`) that shows only the latest thought.
    // - Once it finishes: Hide (or `autoCollapseThinking`) folds the thinking away entirely.
    val isReasoningExpanded = when {
        isActivelyThinkingThisNode -> manualThinkingToggle != false
        manualThinkingToggle != null -> manualThinkingToggle == true
        // Tool-only turns (models that call tools without visible reasoning) open like reasoning turns.
        !preferences.autoCollapseThinking &&
            (activeVar.thinkingSummary.isNotBlank() || activeVar.toolCalls.isNotEmpty()) -> true
        else -> false
    }
    val isLivePeekOnly = isActivelyThinkingThisNode && manualThinkingToggle == false
    val turnText = remember(activeVar.content, activeVar.toolCalls) { turnTextLayout(activeVar.content, activeVar.toolCalls) }
    val stepsOnScreen = isReasoningExpanded && !isLivePeekOnly &&
        ((preferences.showThinkingCards && activeVar.thinkingSummary.isNotBlank()) || (preferences.showToolCards && activeVar.toolCalls.isNotEmpty()))
    // Text written between tools sits in the timeline when it is open, and joins the reply when it is not.
    val replyText = when {
        !turnText.hasSteps -> activeVar.content
        stepsOnScreen -> turnText.finalText
        else -> turnText.joinedText
    }

    var variantSwipeDelta by remember(node.id, node.activeVariantIndex) { mutableStateOf(0f) }

    // On pointer devices the quiet footer appears with the turn being read; touch has no hover, so it stays.
    val turnHoverSource = remember { MutableInteractionSource() }
    val turnHovered by turnHoverSource.collectIsHoveredAsState()
    var footerHasFocus by remember { mutableStateOf(false) }
    val footerVisible = win.isCompact || turnHovered || footerHasFocus || showMoreActions || openUsageDetailsId != null || activeVar.isStreaming
    val footerAlphaState = animateFloatAsState(if (footerVisible) 1f else 0f, motionTween(Motion.QUICK))
    val showBubbles = viewMode == ViewMode.EDITORIAL_TURNS
    val isSentBubble = showBubbles && !isAssistant

    // No size animation while streaming: text grows every frame, and a spring animating each new line lags behind it.
    val baseContainerModifier = if (viewMode == ViewMode.MANUSCRIPT) {
        Modifier
            .fillMaxWidth()
            .hoverable(turnHoverSource)
            .then(
                if (!node.includedInContext) Modifier.background(p.elevatedBg.copy(alpha = 0.35f), RoundedCornerShape(Radius.r8))
                else Modifier
            )
            .then(
                if (!activeVar.isStreaming) Modifier.animateContentSize(animationSpec = motionSpring())
                else Modifier
            )
            .padding(vertical = Space.s6)
    } else {
        Modifier
            .fillMaxWidth()
            .hoverable(turnHoverSource)
            .then(if (!node.includedInContext) Modifier.graphicsLayer { alpha = 0.62f } else Modifier)
            .then(
                if (!activeVar.isStreaming) Modifier.animateContentSize(animationSpec = motionSpring())
                else Modifier
            )
    }
    val containerModifier = if (node.variants.size > 1 && !isEditing) {
        baseContainerModifier.pointerInput(node.id, node.activeVariantIndex, node.variants.size, isEditing) {
            detectHorizontalDragGestures(
                onDragStart = { variantSwipeDelta = 0f },
                onHorizontalDrag = { _, dragAmount -> variantSwipeDelta += dragAmount },
                onDragEnd = {
                    if (variantSwipeDelta < -64.dp.toPx() && node.activeVariantIndex < node.variants.lastIndex) {
                        onSelectVariant(node.activeVariantIndex + 1)
                    } else if (variantSwipeDelta > 64.dp.toPx() && node.activeVariantIndex > 0) {
                        onSelectVariant(node.activeVariantIndex - 1)
                    }
                    variantSwipeDelta = 0f
                }
            )
        }
    } else {
        baseContainerModifier
    }

    @Suppress("DEPRECATION")
    val turnClipboard = LocalClipboardManager.current
    // Right-click (desktop) offers the turn's main actions; they are also on the footer buttons.
    PlatformContextMenu(items = {
        buildList {
            add(PlatformMenuItem(Strings.copy) { turnClipboard.setText(AnnotatedString(activeVar.content)) })
            if (!activeVar.isStreaming) {
                add(PlatformMenuItem(Strings.editMessage) {
                    editBuffer = activeVar.content
                    isEditing = true
                })
            }
            if (isAssistant) add(PlatformMenuItem(if (node.pinned) Strings.unpinReply else Strings.pinReply, onTogglePin))
            if (isAssistant && !activeVar.isStreaming) add(PlatformMenuItem(Strings.regenerateResponse, onRegenerate))
            add(PlatformMenuItem(Strings.branch, onForkHere))
        }
    }) {
        Column(modifier = containerModifier, horizontalAlignment = if (isSentBubble) Alignment.End else Alignment.Start) {
            MessageTurnHeader(
                node = node,
                activeVar = activeVar,
                isAssistant = isAssistant,
                userPicture = userPicture,
                preferences = preferences,
                userName = userName,
                characterName = characterName,
                assistantIconEmoji = assistantIconEmoji,
                providerName = providerName,
                isActivelyThinkingThisNode = isActivelyThinkingThisNode,
                isReasoningExpanded = isReasoningExpanded,
                isLivePeekOnly = isLivePeekOnly,
                onSelectVariant = onSelectVariant,
                onToggleReasoningChip = {
                    manualThinkingToggle = if (isActivelyThinkingThisNode) {
                        if (isLivePeekOnly) true else false
                    } else {
                        !isReasoningExpanded
                    }
                }
            )

            MessageTurnReasoningDrawer(
                nodeId = node.id,
                activeVariantIndex = node.activeVariantIndex,
                activeVar = activeVar,
                turnText = turnText,
                preferences = preferences,
                enableMd = enableMd,
                isActivelyThinkingThisNode = isActivelyThinkingThisNode,
                isReasoningExpanded = isReasoningExpanded,
                isLivePeekOnly = isLivePeekOnly,
                onTogglePeekOrCollapse = { manualThinkingToggle = if (isLivePeekOnly) true else false }
            )

            Spacer(Modifier.height(Space.s8))

            MessageTurnGoalTrackerCard(activeVar.goalTracker)

            // Main Prose Content OR In-Place Editor
            val shrinkBubble = !isEditing && !activeVar.isStreaming && activeVar.attachments.isEmpty() &&
                activeVar.mediaItems.isEmpty() && activeVar.content.length < MAX_SHRINKABLE_BUBBLE_CHARS &&
                !activeVar.content.contains("```")
            val contentModifier = when {
                showBubbles -> Modifier.messageBubble(fromUser = !isAssistant, shrink = shrinkBubble, fill = if (isAssistant) p.cardBg else p.accent.copy(alpha = 0.16f), edge = if (isAssistant) p.border else Color.Transparent)
                isAssistant -> Modifier
                else -> Modifier.rubricMark(p.accent).padding(start = 14.dp, top = Space.s2, bottom = Space.s2)
            }
            Column(contentModifier) {
                MessageTurnProseOrEditor(
                    nodeId = node.id,
                    activeVariantIndex = node.activeVariantIndex,
                    activeVar = activeVar,
                    isAssistant = isAssistant,
                    isEditing = isEditing,
                    editBuffer = editBuffer,
                    onEditBufferChange = { editBuffer = it },
                    onCancelEdit = {
                        editBuffer = ""
                        isEditing = false
                    },
                    onSaveEditedText = { text, asBranch, regenerate ->
                        onSaveEditedText(text, asBranch, regenerate)
                        editBuffer = ""
                        isEditing = false
                    },
                    preferences = preferences,
                    enableMd = enableMd,
                    replyText = replyText,
                    turnTextHasSteps = turnText.hasSteps,
                    isActivelyThinkingThisNode = isActivelyThinkingThisNode,
                    highlightedSourceLineText = highlightedSourceLineText
                )

                TurnAttachmentsSection(
                    visible = preferences.showFilesBelowReplies,
                    attachments = activeVar.attachments,
                    onInspectAttachment = onInspectAttachment
                )

                TurnGeneratedMediaSection(
                    mediaItems = activeVar.mediaItems,
                    onInspectGeneratedImage = onInspectGeneratedImage
                )
            }

            Spacer(Modifier.height(Space.s6))

            MessageTurnActionFooter(
                node = node,
                activeVar = activeVar,
                isAssistant = isAssistant,
                isLatestTurn = isLatestTurn,
                isSentBubble = isSentBubble,
                preferences = preferences,
                footerAlpha = { footerAlphaState.value },
                copiedBadge = copiedBadge,
                onCopiedBadgeChange = { copiedBadge = it },
                showMoreActions = showMoreActions,
                onToggleMoreActions = { showMoreActions = !showMoreActions },
                showSourcesDrawer = showSourcesDrawer,
                onToggleSourcesDrawer = {
                    showSourcesDrawer = !showSourcesDrawer
                    if (!showSourcesDrawer) highlightedSourceLineText = ""
                },
                openUsageDetailsId = openUsageDetailsId,
                onOpenUsageDetailsIdChange = { openUsageDetailsId = it },
                onFooterFocusChange = { footerHasFocus = it },
                onToggleEditing = {
                    editBuffer = if (!isEditing) activeVar.content else ""
                    isEditing = !isEditing
                },
                onToggleContextMute = onToggleContextMute,
                onTogglePin = onTogglePin,
                onDeleteCurrentVariant = onDeleteCurrentVariant,
                onDeleteNode = onDeleteNode,
                onForkHere = onForkHere,
                onRewindHere = onRewindHere,
                onRegenerate = onRegenerate,
                onTranslate = onTranslate,
                onContinue = onContinue,
                onSaveAsSkill = onSaveAsSkill,
                onToggleRawWireDrawer = { showRawWireDrawer = !showRawWireDrawer }
            )

            if (!isAssistant) {
                UserTurnUsageStats(
                    activeVar = activeVar,
                    showTokenAndContextStats = preferences.showTokenAndContextStats,
                    isSentBubble = isSentBubble,
                    inputPricePerM = inputPricePerM,
                    footerAlpha = { footerAlphaState.value }
                )
            }

            if (isAssistant) {
                TurnWebSourcesDrawer(
                    visible = showSourcesDrawer,
                    webSources = activeVar.webSources,
                    highlightedSourceLineText = highlightedSourceLineText,
                    onHighlightLineChange = { highlightedSourceLineText = it },
                    onClose = {
                        showSourcesDrawer = false
                        highlightedSourceLineText = ""
                    }
                )
            }

            TurnRawWireDrawer(
                visible = showRawWireDrawer,
                nodeId = node.id,
                activeVariantIndex = node.activeVariantIndex,
                rawRequestPreview = activeVar.rawRequestPreview,
                rawResponsePreview = activeVar.rawResponsePreview
            )

            if (viewMode == ViewMode.MANUSCRIPT) {
                Spacer(Modifier.height(Space.s8))
                SepiaDivider()
            }
        }
    }
}

@Composable
private fun MessageTurnHeader(
    node: MessageNode,
    activeVar: MessageVariant,
    isAssistant: Boolean,
    userPicture: ImageBitmap?,
    preferences: PreferencesConfig,
    userName: String,
    characterName: String,
    assistantIconEmoji: String,
    providerName: String,
    isActivelyThinkingThisNode: Boolean,
    isReasoningExpanded: Boolean,
    isLivePeekOnly: Boolean,
    onSelectVariant: (Int) -> Unit,
    onToggleReasoningChip: () -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    // The stock assistant name says nothing the model name does not; characters keep theirs.
    val namedCharacter = characterName.takeUnless { it == Strings.defaultAssistant }.orEmpty()
    val speakerTitle = if (isAssistant) {
        characterName.ifBlank { activeVar.modelUsed.ifBlank { "Assistant" } }
    } else {
        userName.ifBlank { Strings.user }
    }
    // Header and version bar; a wrapping row so the name, model and version pills never overlap on a phone.
    val showTurnHeader = isAssistant || preferences.showUserAvatar || preferences.showUserName ||
        preferences.showUserTimestamp || node.variants.size > 1 || !node.includedInContext
    if (!showTurnHeader) return

    @OptIn(ExperimentalLayoutApi::class)
    FlowRow(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalArrangement = Arrangement.spacedBy(Space.s6)
    ) {
        Row(
            modifier = Modifier.weight(1f, fill = false).padding(top = Space.s2, end = Space.s8),
            horizontalArrangement = Arrangement.spacedBy(7.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (isAssistant && preferences.showModelIcon) {
                if (namedCharacter.isNotBlank() && assistantIconEmoji != "💬") {
                    SepiaAvatarCircle(
                        symbol = assistantIconEmoji,
                        selected = true,
                        sizeDp = 22.dp
                    )
                } else {
                    ModelLabSvgIcon(
                        modelOrProvider = activeVar.modelUsed.ifBlank { providerName },
                        tint = p.mutedInk,
                        size = IconSize.md,
                        fallbackProtocolName = providerName
                    )
                }
            } else if (!isAssistant && preferences.showUserAvatar) {
                ProfilePicture(userPicture, 22.dp)
            }

            val showTurnTimestamp = (isAssistant && preferences.showModelTimestamp) || (!isAssistant && preferences.showUserTimestamp)
            val timeSuffix = if (showTurnTimestamp && activeVar.createdAt > 0L) {
                formatCompactTimestamp(activeVar.createdAt)
            } else ""

            Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp)
                ) {
                    if (isAssistant && preferences.showModelName) {
                        val provSuffix = if (preferences.showProviderAfterModelName && namedCharacter.isBlank()) " ($providerName)" else ""
                        val primaryHeader = if (namedCharacter.isNotBlank()) {
                            namedCharacter
                        } else {
                            "${activeVar.modelUsed.ifBlank { "Assistant" }}$provSuffix"
                        }
                        BasicText(
                            text = primaryHeader,
                            maxLines = if (win.isCompact) 2 else 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                            style = p.uiLabelStyle.copy(color = p.narrationInk, fontWeight = FontWeight.Medium, fontSize = 12.sp)
                        )
                    } else if (!isAssistant && preferences.showUserName) {
                        BasicText(
                            text = speakerTitle,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                            style = p.uiLabelStyle.copy(color = p.narrationInk, fontWeight = FontWeight.Medium, fontSize = 12.sp)
                        )
                    }
                    if (timeSuffix.isNotEmpty()) {
                        BasicText(
                            text = timeSuffix,
                            maxLines = 1,
                            style = p.uiLabelStyle.copy(fontSize = 11.sp, color = p.mutedInk)
                        )
                    }
                    if (!node.includedInContext) {
                        BasicText(
                            text = Strings.leftOutOfContext,
                            style = p.uiLabelStyle.copy(color = p.danger, fontWeight = FontWeight.SemiBold)
                        )
                    }
                }

                if (isAssistant && preferences.showModelName && namedCharacter.isNotBlank() && activeVar.modelUsed.isNotBlank()) {
                    val provSuffix = if (preferences.showProviderAfterModelName) " • $providerName" else ""
                    val modelDisplay = if (win.isCompact && activeVar.modelUsed.contains('/')) {
                        activeVar.modelUsed.substringAfterLast('/')
                    } else {
                        activeVar.modelUsed
                    }
                    BasicText(
                        text = "$modelDisplay$provSuffix",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiLabelStyle.copy(fontSize = 11.sp)
                    )
                }
            }
        }

        // Swipe / Branch Variant Selector (< 1 / 3 >) + Reasoning Accordion Trigger
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.s4),
            verticalAlignment = Alignment.CenterVertically
        ) {
            if (node.variants.size > 1) {
                SepiaButton(
                    text = "←",
                    contentDescription = Strings.previousVersion,
                    compact = true,
                    enabled = node.activeVariantIndex > 0,
                    style = SepiaButtonStyle.GHOST,
                    onClick = { onSelectVariant(node.activeVariantIndex - 1) }
                )
                BasicText(
                    text = "${node.activeVariantIndex + 1}/${node.variants.size}",
                    style = p.uiLabelStyle.copy(
                        color = p.accentSecondary,
                        fontWeight = FontWeight.SemiBold,
                        fontFeatureSettings = "tnum"
                    )
                )
                SepiaButton(
                    text = "→",
                    contentDescription = Strings.nextVersion,
                    compact = true,
                    enabled = node.activeVariantIndex < node.variants.lastIndex,
                    style = SepiaButtonStyle.GHOST,
                    onClick = { onSelectVariant(node.activeVariantIndex + 1) }
                )
            }

            val hasThinking = preferences.showThinkingCards && activeVar.thinkingSummary.isNotBlank()
            val hasTools = preferences.showToolCards && activeVar.toolCalls.isNotEmpty()
            val hasRunningToolInVar = activeVar.toolCalls.any { it.isRunning }
            val isCurrentlyToolStep = hasRunningToolInVar ||
                (isActivelyThinkingThisNode && hasTools && activeVar.trailingThinking.isBlank())
            if (hasThinking || hasTools) {
                val sec = if (activeVar.thinkingDurationMs > 0) " ${formatDurationFromMs(activeVar.thinkingDurationMs)}" else ""
                val toolBadge = if (hasTools) {
                    val tools = activeVar.toolCalls.size
                    val prefix = if (activeVar.thinkingSummary.isNotBlank()) ", " else " "
                    "$prefix$tools tool${if (tools == 1) "" else "s"}"
                } else ""
                val chipText = when {
                    isLivePeekOnly && isCurrentlyToolStep -> Strings.toolPeek(sec)
                    isLivePeekOnly -> Strings.peek(sec)
                    isActivelyThinkingThisNode && isCurrentlyToolStep -> Strings.working(sec, toolBadge)
                    isActivelyThinkingThisNode -> Strings.thinking2(sec, toolBadge)
                    isReasoningExpanded -> Strings.thought2(sec, toolBadge)
                    else -> Strings.thought3(sec, toolBadge)
                }
                SepiaButton(
                    text = chipText,
                    compact = true,
                    hint = Strings.showOrHideTheThinking,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onToggleReasoningChip
                )
            }
        }
    }
}

@Composable
private fun MessageTurnProseOrEditor(
    nodeId: String,
    activeVariantIndex: Int,
    activeVar: MessageVariant,
    isAssistant: Boolean,
    isEditing: Boolean,
    editBuffer: String,
    onEditBufferChange: (String) -> Unit,
    onCancelEdit: () -> Unit,
    onSaveEditedText: (String, Boolean, Boolean) -> Unit,
    preferences: PreferencesConfig,
    enableMd: Boolean,
    replyText: String,
    turnTextHasSteps: Boolean,
    isActivelyThinkingThisNode: Boolean,
    highlightedSourceLineText: String
) {
    if (isEditing) {
        Column(verticalArrangement = Arrangement.spacedBy(Space.s8)) {
            SepiaTextField(
                value = editBuffer,
                onValueChange = onEditBufferChange,
                placeholder = Strings.editMessage,
                singleLine = false,
                minHeight = 120.dp,
                modifier = Modifier.fillMaxWidth()
            )
            @OptIn(ExperimentalLayoutApi::class)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Space.s8),
                verticalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                if (!isAssistant) {
                    SepiaButton(
                        text = Strings.saveRegenerate,
                        compact = true,
                        style = SepiaButtonStyle.PRIMARY,
                        onClick = {
                            val asBranch = !preferences.deletePreviousMessageWhenRegenerating
                            onSaveEditedText(editBuffer, asBranch, true)
                        }
                    )
                }
                SepiaButton(
                    text = Strings.save,
                    compact = true,
                    style = if (isAssistant) SepiaButtonStyle.PRIMARY else SepiaButtonStyle.OUTLINE,
                    onClick = { onSaveEditedText(editBuffer, false, false) }
                )
                SepiaButton(
                    text = Strings.saveAsNewVersion,
                    compact = true,
                    style = SepiaButtonStyle.SECONDARY,
                    onClick = { onSaveEditedText(editBuffer, true, false) }
                )
                SepiaButton(
                    text = Strings.cancel,
                    compact = true,
                    style = SepiaButtonStyle.GHOST,
                    onClick = onCancelEdit
                )
            }
        }
    } else {
        val hasNonEmptyProseOrIdle = replyText.isNotBlank() ||
            (!isActivelyThinkingThisNode && activeVar.mediaItems.isEmpty() && activeVar.attachments.isEmpty())
        if (hasNonEmptyProseOrIdle) {
            val isLongCollapsible = preferences.collapseLongMessages && !activeVar.isStreaming && replyText.length > 1800
            var isLongExpanded by remember(nodeId, activeVariantIndex) { mutableStateOf(false) }
            val displayedProse = if (isLongCollapsible && !isLongExpanded) {
                replyText.take(1400) + "…"
            } else {
                replyText.ifBlank { if (activeVar.isStreaming && !turnTextHasSteps) "…" else "" }
            }
            LiteraryProseRenderer(
                text = displayedProse,
                enableMarkdown = enableMd,
                enableMath = preferences.mathFormulaRendering,
                enableInlineDollarMath = preferences.inlineDollarMathRendering,
                autoCollapseCodeBlocks = preferences.autoCollapseCodeBlocks,
                wrapCodeBlocks = preferences.wrapCodeBlocks,
                highlightedLineSubstring = highlightedSourceLineText
            )
            if (isLongCollapsible) {
                Spacer(Modifier.height(Space.s4))
                SepiaButton(
                    text = if (isLongExpanded) Strings.collapseLongMessage else Strings.readFullMessageChars(activeVar.content.length),
                    compact = true,
                    style = SepiaButtonStyle.OUTLINE,
                    onClick = { isLongExpanded = !isLongExpanded }
                )
            }
        }
    }
}
