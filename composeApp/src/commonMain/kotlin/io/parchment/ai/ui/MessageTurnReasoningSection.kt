package io.parchment.ai.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.TurnTextLayout
import io.parchment.ai.strings.Strings
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

/**
 * Animated collapsible reasoning & tool-step timeline drawer (full timeline or compact live peek).
 */
@Composable
internal fun MessageTurnReasoningDrawer(
    nodeId: String,
    activeVariantIndex: Int,
    activeVar: MessageVariant,
    turnText: TurnTextLayout,
    preferences: PreferencesConfig,
    enableMd: Boolean,
    isActivelyThinkingThisNode: Boolean,
    isReasoningExpanded: Boolean,
    isLivePeekOnly: Boolean,
    onTogglePeekOrCollapse: () -> Unit
) {
    val p = LocalParchmentPalette.current
    val showDrawer = (isReasoningExpanded || isLivePeekOnly) &&
        ((preferences.showThinkingCards && activeVar.thinkingSummary.isNotBlank()) ||
            (preferences.showToolCards && activeVar.toolCalls.isNotEmpty()))

    AnimatedVisibility(
        visible = showDrawer,
        enter = motionExpandEnter(),
        exit = motionShrinkExit()
    ) {
        val livePeekInfo = remember(isLivePeekOnly, activeVar.thinkingSummary, activeVar.trailingThinking, activeVar.toolCalls) {
            if (isLivePeekOnly) extractLatestAgentPeek(activeVar) else LiveAgentPeekInfo(isToolPeek = false, title = "", body = "")
        }
        Column(
            modifier = Modifier
                .padding(top = Space.s8)
                .fillMaxWidth()
                .clip(RoundedCornerShape(Radius.r10))
                .background(p.sheetBg.copy(alpha = 0.58f))
                .border(LineWidth.hairline, if (isActivelyThinkingThisNode) p.accent.copy(alpha = 0.8f) else p.border.copy(alpha = 0.45f), RoundedCornerShape(Radius.r10))
                .padding(Space.s12),
            verticalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            ReasoningDrawerHeader(
                activeVar = activeVar,
                isActivelyThinkingThisNode = isActivelyThinkingThisNode,
                isLivePeekOnly = isLivePeekOnly,
                livePeekInfo = livePeekInfo,
                onTogglePeekOrCollapse = onTogglePeekOrCollapse
            )

            if (isLivePeekOnly) {
                LiveAgentPeekContent(livePeekInfo)
            } else {
                val hasInterleavedSteps = activeVar.toolCalls.isNotEmpty() &&
                    (activeVar.toolCalls.any { it.precedingThinking.isNotBlank() } || activeVar.trailingThinking.isNotBlank() || turnText.hasSteps)
                if (hasInterleavedSteps) {
                    InterleavedReasoningTimeline(
                        nodeId = nodeId,
                        activeVariantIndex = activeVariantIndex,
                        activeVar = activeVar,
                        turnText = turnText,
                        preferences = preferences,
                        enableMd = enableMd,
                        isActivelyThinkingThisNode = isActivelyThinkingThisNode
                    )
                } else {
                    if (preferences.showThinkingCards && activeVar.thinkingSummary.isNotBlank()) {
                        LiteraryProseRenderer(
                            text = activeVar.thinkingSummary,
                            compactReasoningMode = true,
                            enableMarkdown = preferences.renderReasoningWithMarkdown,
                            enableMath = preferences.mathFormulaRendering,
                            enableInlineDollarMath = preferences.inlineDollarMathRendering,
                            autoCollapseCodeBlocks = preferences.autoCollapseCodeBlocks,
                            wrapCodeBlocks = preferences.wrapCodeBlocks
                        )
                    }
                    if (preferences.showToolCards && activeVar.toolCalls.isNotEmpty()) {
                        activeVar.toolCalls.forEach { tc ->
                            ToolExecutionCard(tc, showToolResultSummary = preferences.showToolResultSummary)
                        }
                    }
                }
            }

            if (!isLivePeekOnly && activeVar.thinkingSignature.isNotBlank()) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(5.dp)
                ) {
                    KissVectorIcon(
                        icon = KissIcon.SHIELD_CHECK,
                        tint = p.accent,
                        size = 12.dp
                    )
                    BasicText(
                        text = Strings.thinkingSavedForTheNext,
                        style = p.uiLabelStyle.copy(fontSize = 11.sp, fontFamily = p.codeFontFamily)
                    )
                }
            }
        }
    }
}

@Composable
private fun ReasoningDrawerHeader(
    activeVar: MessageVariant,
    isActivelyThinkingThisNode: Boolean,
    isLivePeekOnly: Boolean,
    livePeekInfo: LiveAgentPeekInfo,
    onTogglePeekOrCollapse: () -> Unit
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    @OptIn(ExperimentalLayoutApi::class)
    FlowRow(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalArrangement = Arrangement.spacedBy(Space.s6),
        itemVerticalAlignment = Alignment.CenterVertically
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Space.s6),
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.weight(1f, fill = false).padding(end = Space.s6)
        ) {
            if (isActivelyThinkingThisNode) {
                KissStreamingPulseDot(color = p.accent)
            } else {
                KissVectorIcon(icon = KissIcon.BRAIN, tint = p.accent, size = IconSize.md)
            }
            val wordCount = remember(activeVar.thinkingSummary) { countWordsFast(activeVar.thinkingSummary) }
            val toolCount = activeVar.toolCalls.size
            val headerSummary = buildString {
                if (isLivePeekOnly) {
                    append(if (livePeekInfo.isToolPeek) Strings.latestTool else Strings.latestThought)
                } else if (isActivelyThinkingThisNode) {
                    append(if (livePeekInfo.isToolPeek) Strings.runningToolsAndThinking else Strings.thinking)
                } else if (win.isCompact) {
                    append(Strings.reasoning)
                } else {
                    append(Strings.thinkingAndToolSteps)
                }
                if (wordCount > 0) append(" ($wordCount words")
                if (toolCount > 0) {
                    if (wordCount > 0) append(" • $toolCount tools)") else append(" ($toolCount tools)")
                } else if (wordCount > 0) {
                    append(")")
                }
            }
            BasicText(
                text = headerSummary,
                style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
            )
        }
        SepiaButton(
            text = if (isLivePeekOnly) "▾" else "▴",
            contentDescription = if (isLivePeekOnly) Strings.showThinking else Strings.hideThinking,
            compact = true,
            style = SepiaButtonStyle.GHOST,
            onClick = onTogglePeekOrCollapse
        )
    }
}

@Composable
private fun LiveAgentPeekContent(livePeekInfo: LiveAgentPeekInfo) {
    val p = LocalParchmentPalette.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .padding(vertical = Space.s2),
        verticalArrangement = Arrangement.spacedBy(Space.s4)
    ) {
        if (livePeekInfo.title.isNotEmpty()) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                if (livePeekInfo.isToolPeek) {
                    KissVectorIcon(
                        icon = resolveToolKissIcon(livePeekInfo.toolName),
                        tint = p.accent,
                        size = IconSize.md
                    )
                }
                BasicText(
                    text = livePeekInfo.title,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                    style = p.uiBodyStyle.copy(
                        color = p.primaryInk,
                        fontWeight = FontWeight.Bold,
                        fontSize = 13.sp
                    )
                )
                if (livePeekInfo.isToolRunning) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(Radius.r4))
                            .background(p.accent.copy(alpha = 0.16f))
                            .padding(horizontal = Space.s6, vertical = Space.s2)
                    ) {
                        BasicText(
                            text = Strings.running,
                            style = p.monoStyle.copy(
                                color = p.accent,
                                fontWeight = FontWeight.Bold,
                                fontSize = 11.sp
                            )
                        )
                    }
                }
            }
        }
        if (livePeekInfo.body.isNotEmpty()) {
            BasicText(
                text = livePeekInfo.body,
                maxLines = if (livePeekInfo.isToolPeek) 4 else 3,
                overflow = TextOverflow.Ellipsis,
                style = if (livePeekInfo.isToolPeek) {
                    p.monoStyle.copy(
                        color = p.narrationInk,
                        fontSize = 11.sp,
                        lineHeight = 16.sp
                    )
                } else {
                    p.uiBodyStyle.copy(
                        color = p.narrationInk,
                        fontSize = 13.sp,
                        lineHeight = 18.sp
                    )
                }
            )
        }
    }
}

@Composable
private fun InterleavedReasoningTimeline(
    nodeId: String,
    activeVariantIndex: Int,
    activeVar: MessageVariant,
    turnText: TurnTextLayout,
    preferences: PreferencesConfig,
    enableMd: Boolean,
    isActivelyThinkingThisNode: Boolean
) {
    // Long runs show a tail window so dozens of text renderers are never composed at once.
    val totalToolSteps = activeVar.toolCalls.size
    var showAllInterleavedSteps by remember(nodeId, activeVariantIndex) { mutableStateOf(false) }
    val maxVisibleTailSteps = if (isActivelyThinkingThisNode) 12 else 24
    val hiddenStepCount = if (!showAllInterleavedSteps && totalToolSteps > maxVisibleTailSteps) {
        totalToolSteps - maxVisibleTailSteps
    } else 0
    if (hiddenStepCount > 0) {
        SepiaButton(
            text = Strings.showEarlierToolStepsTotal(hiddenStepCount, totalToolSteps),
            compact = true,
            style = SepiaButtonStyle.OUTLINE,
            onClick = { showAllInterleavedSteps = true }
        )
    }
    val visibleTools = if (hiddenStepCount > 0) {
        activeVar.toolCalls.subList(hiddenStepCount, totalToolSteps)
    } else {
        activeVar.toolCalls
    }
    val stepRuns = remember(visibleTools, hiddenStepCount, turnText, preferences.showThinkingCards, preferences.showToolCards) {
        if (!preferences.showToolCards) {
            visibleTools.indices.map { it..it }
        } else {
            groupConsecutiveTimelineSteps(visibleTools, preferences.showThinkingCards) { i ->
                turnText.beforeTool.getOrElse(hiddenStepCount + i) { "" }
            }
        }
    }
    var thoughtStepCounter = hiddenStepCount
    stepRuns.forEach { run ->
        val first = visibleTools[run.first]
        // A quiet group keeps the first step's key, so a single read that gains a neighbour keeps its slot.
        key(first.id) {
            if (preferences.showThinkingCards && first.precedingThinking.isNotBlank()) {
                thoughtStepCounter++
                ReasoningThoughtStepBlock(
                    stepNumber = thoughtStepCounter,
                    text = first.precedingThinking,
                    preferences = preferences
                )
            }
            val stepText = turnText.beforeTool.getOrElse(hiddenStepCount + run.first) { "" }.trim()
            if (stepText.isNotEmpty()) {
                LiteraryProseRenderer(
                    text = stepText,
                    enableMarkdown = enableMd,
                    enableMath = preferences.mathFormulaRendering,
                    enableInlineDollarMath = preferences.inlineDollarMathRendering,
                    autoCollapseCodeBlocks = preferences.autoCollapseCodeBlocks,
                    wrapCodeBlocks = preferences.wrapCodeBlocks
                )
            }
            if (preferences.showToolCards) {
                if (run.first == run.last) {
                    ToolExecutionCard(first, showToolResultSummary = preferences.showToolResultSummary)
                } else {
                    QuietToolGroup(
                        tools = visibleTools.subList(run.first, run.last + 1),
                        showToolResultSummary = preferences.showToolResultSummary
                    )
                }
            }
        }
    }

    if (preferences.showThinkingCards && activeVar.trailingThinking.isNotBlank()) {
        thoughtStepCounter++
        ReasoningThoughtStepBlock(
            stepNumber = thoughtStepCounter,
            text = activeVar.trailingThinking,
            preferences = preferences
        )
    }
}

/** Tools that only look (read, list, search, fetch). Runs of them fold into one quiet row once finished. */
private val QUIET_LOOKUP_TOOLS = setOf("fs_read", "fs_list", "fs_glob", "fs_search", "web_search", "web_fetch")

/**
 * Splits a turn's tool steps into runs: each run is one step, or two or more consecutive finished, successful
 * lookup steps with nothing shown between them (no step text, and no thinking when thinking cards are on).
 * Running or failed steps, edits and shell commands always stand alone, so anything that needs attention stays
 * visible; a group only hides the routine reading an agent does before it acts.
 */
internal fun groupConsecutiveTimelineSteps(
    tools: List<ToolCallRecord>,
    showThinking: Boolean,
    stepTextBefore: (Int) -> String
): List<IntRange> {
    fun quiet(i: Int): Boolean {
        val tc = tools[i]
        return !tc.isRunning && !tc.isError && tc.name.lowercase() in QUIET_LOOKUP_TOOLS
    }
    fun joinsPrevious(i: Int): Boolean =
        stepTextBefore(i).isBlank() && !(showThinking && tools[i].precedingThinking.isNotBlank())

    val runs = ArrayList<IntRange>(tools.size)
    var start = 0
    while (start < tools.size) {
        var end = start
        if (quiet(start)) {
            while (end + 1 < tools.size && quiet(end + 1) && joinsPrevious(end + 1)) end++
        }
        runs += start..end
        start = end + 1
    }
    return runs
}

/** One calm row for a run of lookups ("Read 3 · searched 2"); opening it shows each step's own card. */
@Composable
private fun QuietToolGroup(tools: List<ToolCallRecord>, showToolResultSummary: Boolean) {
    val p = LocalParchmentPalette.current
    var expanded by remember(tools.first().id) { mutableStateOf(false) }
    val summary = remember(tools) { quietToolGroupSummary(tools) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r8))
            .border(LineWidth.hairline, p.border.copy(alpha = 0.6f), RoundedCornerShape(Radius.r8)),
        verticalArrangement = Arrangement.spacedBy(5.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 36.dp)
                .kissClickable(
                    pressScale = 0.99f,
                    hoverScale = 1.004f,
                    role = Role.Button,
                    onClickLabel = if (expanded) Strings.collapseToolGroup else Strings.expandToolGroup
                ) { expanded = !expanded }
                .padding(horizontal = 9.dp, vertical = Space.s6),
            horizontalArrangement = Arrangement.spacedBy(Space.s6),
            verticalAlignment = Alignment.CenterVertically
        ) {
            KissVectorIcon(
                icon = remember(tools.first().name) { resolveToolKissIcon(tools.first().name) },
                tint = p.accentSecondary,
                size = IconSize.md
            )
            BasicText(
                text = summary,
                modifier = Modifier.weight(1f),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = p.uiLabelStyle.copy(color = p.narrationInk, fontSize = 12.sp)
            )
            BasicText(
                text = if (expanded) "▴" else "▾",
                style = p.uiLabelStyle.copy(color = p.narrationInk, fontSize = 12.sp)
            )
        }
        if (expanded) {
            Column(
                modifier = Modifier.padding(start = Space.s6, end = Space.s6, bottom = Space.s6),
                verticalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                tools.forEach { tc ->
                    key(tc.id) { ToolExecutionCard(tc, showToolResultSummary = showToolResultSummary) }
                }
            }
        }
    }
}

internal fun quietToolGroupSummary(tools: List<ToolCallRecord>): String {
    var reads = 0
    var searches = 0
    var lists = 0
    tools.forEach { tc ->
        when (tc.name.lowercase()) {
            "fs_read", "web_fetch" -> reads++
            "fs_list" -> lists++
            else -> searches++
        }
    }
    return Strings.quietToolGroup(reads, searches, lists)
}

@Composable
private fun ReasoningThoughtStepBlock(
    stepNumber: Int,
    text: String,
    preferences: PreferencesConfig
) {
    val p = LocalParchmentPalette.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = Space.s2),
        verticalArrangement = Arrangement.spacedBy(Space.s4)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(5.dp)
        ) {
            KissVectorIcon(icon = KissIcon.SPARKLES, tint = p.accent, size = IconSize.sm)
            BasicText(
                text = Strings.thought(stepNumber),
                style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold, fontSize = 11.sp)
            )
        }
        LiteraryProseRenderer(
            text = text,
            compactReasoningMode = true,
            enableMarkdown = preferences.renderReasoningWithMarkdown,
            enableMath = preferences.mathFormulaRendering,
            enableInlineDollarMath = preferences.inlineDollarMathRendering,
            autoCollapseCodeBlocks = preferences.autoCollapseCodeBlocks,
            wrapCodeBlocks = preferences.wrapCodeBlocks
        )
    }
}

@Composable
private fun ToolExecutionCard(
    tc: ToolCallRecord,
    showToolResultSummary: Boolean
) {
    val p = LocalParchmentPalette.current
    val lowerTool = tc.name.lowercase()
    if (lowerTool == "fs_edit" || lowerTool == "fs_write" || lowerTool == "fs_multi_edit") {
        val parsedObj: JsonObject? = remember(tc.argumentsJson) {
            runCatching { compactStateJson.parseToJsonElement(tc.argumentsJson) as? JsonObject }.getOrNull()
        }
        val pathStr = parsedObj?.get("path")?.jsonPrimitive?.contentOrNull.orEmpty()
        val (oldTxt, newTxt) = remember(lowerTool, parsedObj) {
            when (lowerTool) {
                "fs_edit" -> {
                    val o = parsedObj?.get("old_text")?.jsonPrimitive?.contentOrNull.orEmpty()
                    val n = parsedObj?.get("new_text")?.jsonPrimitive?.contentOrNull.orEmpty()
                    o to n
                }
                "fs_multi_edit" -> {
                    val editsArr = parsedObj?.get("edits") as? JsonArray
                    val o = editsArr?.mapNotNull { (it as? JsonObject)?.get("old_text")?.jsonPrimitive?.contentOrNull }
                        ?.joinToString("\n...\n").orEmpty()
                    val n = editsArr?.mapNotNull { (it as? JsonObject)?.get("new_text")?.jsonPrimitive?.contentOrNull }
                        ?.joinToString("\n...\n").orEmpty()
                    o to n
                }
                else -> "" to parsedObj?.get("content")?.jsonPrimitive?.contentOrNull.orEmpty()
            }
        }
        SepiaFileDiffCard(
            filePath = pathStr,
            actionLabel = when (lowerTool) {
                "fs_edit" -> Strings.edited
                "fs_multi_edit" -> Strings.multiEdited
                else -> Strings.wrote
            },
            oldText = oldTxt,
            newText = newTxt,
            resultSummary = tc.resultText,
            isError = tc.isError
        )
        return
    }

    var isToolResultExpanded by remember(tc.id) { mutableStateOf(false) }
    val effectiveExpanded = isToolResultExpanded || tc.isRunning
    val canExpandTool = tc.resultText.length > 160 || tc.argumentsJson.length > 80
    val toolIcon = remember(tc.name) { resolveToolKissIcon(tc.name) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r8))
            .background(p.cardBg)
            .border(
                width = if (tc.isRunning) 1.dp else LineWidth.hairline,
                color = when {
                    tc.isError -> p.danger.copy(alpha = 0.7f)
                    tc.isRunning -> p.accent
                    else -> p.border.copy(alpha = 0.75f)
                },
                shape = RoundedCornerShape(Radius.r8)
            )
            .padding(9.dp),
        verticalArrangement = Arrangement.spacedBy(5.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .then(
                    if (canExpandTool) {
                        Modifier.kissClickable(
                            pressScale = 0.99f,
                            hoverScale = 1.004f,
                            role = Role.Button,
                            onClickLabel = if (isToolResultExpanded) Strings.collapseToolResult else Strings.expandToolResult
                        ) { isToolResultExpanded = !isToolResultExpanded }
                    } else Modifier
                ),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            Row(
                modifier = Modifier.weight(1f).padding(end = Space.s6),
                horizontalArrangement = Arrangement.spacedBy(Space.s6),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (tc.isRunning) {
                    KissStreamingPulseDot(color = p.accent)
                } else {
                    KissVectorIcon(
                        icon = toolIcon,
                        tint = if (tc.isError) p.danger else p.accentSecondary,
                        size = IconSize.md
                    )
                }
                BasicText(
                    text = "${tc.name}(${tc.argumentsJson})",
                    maxLines = if (effectiveExpanded) 8 else 2,
                    overflow = TextOverflow.Ellipsis,
                    style = p.monoStyle.copy(fontWeight = FontWeight.SemiBold, fontSize = 12.sp)
                )
            }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp)
            ) {
                if (tc.isRunning) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(Radius.r4))
                            .background(p.accent.copy(alpha = 0.16f))
                            .padding(horizontal = Space.s6, vertical = Space.s2)
                    ) {
                        BasicText(
                            text = Strings.running,
                            style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold, fontSize = 11.sp)
                        )
                    }
                }
                if (canExpandTool) {
                    SepiaButton(
                        text = if (effectiveExpanded) "▴" else "▾",
                        contentDescription = if (effectiveExpanded) Strings.collapseToolResult else Strings.expandToolResult,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = { isToolResultExpanded = !isToolResultExpanded }
                    )
                }
            }
        }
        if ((showToolResultSummary || tc.isRunning) && tc.resultText.isNotBlank()) {
            val shownResult = if (effectiveExpanded) tc.resultText.take(6000) else tc.resultText.take(360)
            BasicText(
                text = if (!effectiveExpanded && tc.resultText.length > 360) "$shownResult…" else shownResult,
                style = p.monoStyle.copy(
                    color = when {
                        tc.isError -> p.danger
                        tc.isRunning -> p.primaryInk
                        else -> p.narrationInk
                    },
                    fontSize = 11.sp
                )
            )
        }
    }
}

/**
 * Live autonomous goal objective, todo checklist, and verification summary card.
 */
@Composable
internal fun MessageTurnGoalTrackerCard(goalTracker: GoalTrackerState?) {
    if (goalTracker == null || (goalTracker.goalObjective.isBlank() && goalTracker.todos.isEmpty())) return
    val p = LocalParchmentPalette.current
    val doneCount = goalTracker.completedCount
    val totalCount = goalTracker.todos.size
    val statusIcon = when {
        goalTracker.isCompleted -> KissIcon.CHECK
        totalCount > 0 -> KissIcon.REFRESH
        else -> KissIcon.TARGET
    }
    val statusText = when {
        goalTracker.isCompleted -> Strings.doneOf(doneCount, totalCount)
        totalCount > 0 -> "$doneCount of $totalCount done"
        else -> Strings.planning
    }
    Column(
        modifier = Modifier
            .padding(bottom = Space.s8)
            .fillMaxWidth()
            .clip(RoundedCornerShape(Radius.r8))
            .background(p.sheetBg.copy(alpha = 0.78f))
            .border(
                width = LineWidth.hairline,
                color = if (goalTracker.isCompleted) p.border else p.accent.copy(alpha = 0.55f),
                shape = RoundedCornerShape(Radius.r8)
            )
            .padding(11.dp),
        verticalArrangement = Arrangement.spacedBy(Space.s6)
    ) {
        @OptIn(ExperimentalLayoutApi::class)
        FlowRow(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalArrangement = Arrangement.spacedBy(Space.s4)
        ) {
            Row(
                modifier = Modifier.weight(1f, fill = false),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                KissVectorIcon(icon = KissIcon.TARGET, tint = p.accent, size = IconSize.md)
                BasicText(
                    text = Strings.goal(goalTracker.goalObjective.ifBlank { Strings.autonomousTaskChecklist }),
                    style = p.uiLabelStyle.copy(color = p.primaryInk, fontWeight = FontWeight.Bold, fontSize = 12.sp)
                )
            }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Space.s4)
            ) {
                KissVectorIcon(
                    icon = statusIcon,
                    tint = if (goalTracker.isCompleted) p.accentSecondary else p.accent,
                    size = 12.dp
                )
                BasicText(
                    text = statusText,
                    style = p.uiLabelStyle.copy(
                        color = if (goalTracker.isCompleted) p.accentSecondary else p.accent,
                        fontWeight = FontWeight.Bold,
                        fontSize = 11.sp
                    )
                )
            }
        }
        if (goalTracker.todos.isNotEmpty()) {
            SepiaDivider()
            Column(verticalArrangement = Arrangement.spacedBy(Space.s4)) {
                goalTracker.todos.forEach { item ->
                    val st = item.status.lowercase()
                    val rowIcon = when (st) {
                        "completed" -> KissIcon.CHECK
                        "in_progress" -> KissIcon.REFRESH
                        "cancelled" -> KissIcon.MINUS
                        else -> KissIcon.CIRCLE_EMPTY
                    }
                    val itemColor = when (st) {
                        "completed" -> p.mutedInk
                        "in_progress" -> p.accent
                        else -> p.primaryInk
                    }
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Space.s6)
                    ) {
                        KissVectorIcon(icon = rowIcon, tint = itemColor, size = IconSize.sm)
                        BasicText(
                            text = "[${item.id}] ${item.content}",
                            style = p.uiBodyStyle.copy(
                                fontSize = 13.sp,
                                color = itemColor,
                                fontWeight = if (st == "in_progress") FontWeight.SemiBold else FontWeight.Normal
                            )
                        )
                    }
                }
            }
        }
        if (goalTracker.verificationSummary.isNotBlank()) {
            SepiaDivider()
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(5.dp)
            ) {
                KissVectorIcon(icon = KissIcon.CHECK, tint = p.accent, size = 12.dp)
                BasicText(
                    text = Strings.verified(goalTracker.verificationSummary),
                    style = p.uiLabelStyle.copy(color = p.accent, fontWeight = FontWeight.Medium)
                )
            }
        }
    }
}
