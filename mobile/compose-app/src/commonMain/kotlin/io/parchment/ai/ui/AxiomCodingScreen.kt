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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.sp
import io.parchment.ai.strings.Strings

// Axiom Coding workspace. Every colour, space, radius, stroke and text style comes
// from DesignTokens.kt and ParchmentPalette; every control is an existing
// component (SepiaButton, SepiaChip, SepiaSectionCard). No inline user text:
// all copy lives in Strings.

/** Read-only view state for the coding workspace. */
data class AxiomCodingViewState(
    val taskTitle: String = "",
    val taskGoal: String = "",
    val steps: List<AxiomCodingStep> = emptyList(),
    val changedFiles: List<String> = emptyList(),
    val checks: List<AxiomCodingCheck> = emptyList(),
    val waitingForApproval: Boolean = false,
    val running: Boolean = false
)

data class AxiomCodingStep(
    val title: String,
    val done: Boolean,
    val active: Boolean = false,
    val blocked: Boolean = false
)

data class AxiomCodingCheck(
    val label: String,
    val passed: Boolean
)

/**
 * The Axiom Coding screen: plan visibility, changed files and verification
 * results, with pause/cancel controls that appear only while a task runs.
 */
@Composable
internal fun AxiomCodingScreen(
    state: AxiomCodingViewState,
    onNewTask: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onStop: () -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(p.canvasBg)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = if (win.isCompact) Space.s12 else Space.s16, vertical = Space.s16),
        verticalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        BasicText(
            text = Strings.axiomCoding,
            style = p.uiHeaderStyle.copy(fontSize = 20.sp),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis
        )
        if (state.taskTitle.isBlank()) {
            EmptyAxiomCoding(onNewTask = onNewTask, modifier = Modifier.fillMaxWidth())
        } else {
            AxiomTaskHeader(state = state, onPause = onPause, onResume = onResume, onStop = onStop)
            AxiomPlanCard(state = state, modifier = Modifier.fillMaxWidth())
            AxiomChangesCard(state = state, modifier = Modifier.fillMaxWidth())
            AxiomVerificationCard(state = state, modifier = Modifier.fillMaxWidth())
        }
    }
}

@Composable
private fun EmptyAxiomCoding(onNewTask: () -> Unit, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    Column(
        modifier = modifier.padding(vertical = Space.s24),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.s10)
    ) {
        KissVectorIcon(icon = KissIcon.CODE, tint = p.accent, size = IconSize.xl)
        BasicText(
            text = Strings.axiomCodingEmptyTitle,
            style = p.uiHeaderStyle.copy(fontSize = 16.sp),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis
        )
        BasicText(
            text = Strings.axiomCodingEmptyBody,
            style = p.uiBodyStyle.copy(color = p.mutedInk),
            maxLines = 3,
            overflow = TextOverflow.Ellipsis
        )
        SepiaButton(
            text = Strings.axiomCodingNewTask,
            onClick = onNewTask,
            style = SepiaButtonStyle.PRIMARY
        )
    }
}

@Composable
private fun AxiomTaskHeader(
    state: AxiomCodingViewState,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onStop: () -> Unit
) {
    val p = LocalParchmentPalette.current
    SepiaSectionCard(title = state.taskTitle, modifier = Modifier.fillMaxWidth()) {
        if (state.taskGoal.isNotBlank()) {
            BasicText(
                text = state.taskGoal,
                style = p.uiBodyStyle.copy(color = p.narrationInk),
                maxLines = 4,
                overflow = TextOverflow.Ellipsis
            )
        }
        if (state.waitingForApproval) {
            AxiomNotice(text = Strings.axiomCodingWaitingApproval, modifier = Modifier.fillMaxWidth())
        }
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = Space.s8),
            horizontalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            if (state.running) {
                SepiaButton(
                    text = Strings.axiomCodingPause,
                    onClick = onPause,
                    style = SepiaButtonStyle.OUTLINE,
                    compact = true
                )
                SepiaButton(
                    text = Strings.stop,
                    onClick = onStop,
                    style = SepiaButtonStyle.DANGER,
                    compact = true
                )
            } else {
                SepiaButton(
                    text = Strings.axiomCodingResume,
                    onClick = onResume,
                    style = SepiaButtonStyle.PRIMARY,
                    compact = true
                )
            }
        }
    }
}

@Composable
private fun AxiomPlanCard(state: AxiomCodingViewState, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val done = state.steps.count { it.done }
    SepiaSectionCard(
        title = Strings.axiomCodingPlan,
        subtitle = if (state.steps.isEmpty()) "" else Strings.axiomCodingStepCount(done, state.steps.size),
        modifier = modifier
    ) {
        if (state.steps.isEmpty()) {
            BasicText(
                text = Strings.axiomCodingNoSteps,
                style = p.uiBodyStyle.copy(color = p.mutedInk),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
        } else {
            state.steps.forEach { step -> AxiomStepRow(step, modifier = Modifier.fillMaxWidth()) }
        }
    }
}

@Composable
private fun AxiomStepRow(step: AxiomCodingStep, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val mark = when {
        step.done -> KissIcon.CHECK
        step.blocked -> KissIcon.WARNING
        else -> KissIcon.CIRCLE_EMPTY
    }
    val tint = when {
        step.done -> p.success
        step.blocked -> p.danger
        else -> p.mutedInk
    }
    Row(
        modifier = modifier.heightIn(min = ControlHeight.touch).padding(vertical = Space.s4),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.s8)
    ) {
        KissVectorIcon(icon = mark, tint = tint, size = IconSize.md)
        BasicText(
            text = step.title,
            style = if (step.active) {
                p.uiBodyStyle.copy(color = p.accent, fontWeight = FontWeight.Medium)
            } else {
                p.uiBodyStyle
            },
            maxLines = 2,
            overflow = TextOverflow.Ellipsis
        )
    }
}

@Composable
private fun AxiomChangesCard(state: AxiomCodingViewState, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    SepiaSectionCard(
        title = Strings.axiomCodingChanges,
        subtitle = if (state.changedFiles.isEmpty()) {
            ""
        } else {
            Strings.axiomCodingChangedFileCount(state.changedFiles.size)
        },
        modifier = modifier
    ) {
        if (state.changedFiles.isEmpty()) {
            BasicText(
                text = Strings.axiomCodingNoChanges,
                style = p.uiBodyStyle.copy(color = p.mutedInk),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
        } else {
            state.changedFiles.forEach { path ->
                BasicText(
                    text = path,
                    style = p.monoStyle,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.fillMaxWidth().padding(vertical = Space.s2)
                )
            }
        }
    }
}

@Composable
private fun AxiomVerificationCard(state: AxiomCodingViewState, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    SepiaSectionCard(title = Strings.axiomCodingVerification, modifier = modifier) {
        if (state.checks.isEmpty()) {
            BasicText(
                text = Strings.axiomCodingNoVerification,
                style = p.uiBodyStyle.copy(color = p.mutedInk),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
        } else {
            state.checks.forEach { check -> AxiomCheckRow(check, modifier = Modifier.fillMaxWidth()) }
        }
    }
}

@Composable
private fun AxiomCheckRow(check: AxiomCodingCheck, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val mark = if (check.passed) KissIcon.CHECK else KissIcon.CLOSE
    val tint = if (check.passed) p.success else p.danger
    val label = if (check.passed) Strings.axiomCodingPassed else Strings.axiomCodingFailed
    Row(
        modifier = modifier.heightIn(min = ControlHeight.touch).padding(vertical = Space.s4),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Space.s8)
    ) {
        KissVectorIcon(icon = mark, tint = tint, size = IconSize.md)
        BasicText(text = check.label, style = p.uiBodyStyle, maxLines = 2, overflow = TextOverflow.Ellipsis)
        BasicText(text = label, style = p.uiLabelStyle.copy(color = tint), maxLines = 1)
    }
}

@Composable
private fun AxiomNotice(text: String, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val shape = androidx.compose.foundation.shape.RoundedCornerShape(Radius.r12)
    Box(
        modifier = modifier
            .clip(shape)
            .background(p.accent.copy(alpha = 0.10f))
            .border(LineWidth.hairline, p.accent.copy(alpha = 0.52f), shape)
            .padding(horizontal = Space.s10, vertical = Space.s8)
    ) {
        BasicText(
            text = text,
            style = p.uiLabelStyle.copy(color = p.accent),
            maxLines = 2,
            overflow = TextOverflow.Ellipsis
        )
    }
}
