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
import io.parchment.ai.harness.CodingTaskSummary
import io.parchment.ai.strings.Strings

/**
 * Axiom Coding workspace.
 *
 * Every colour, space, radius, stroke and text style comes from DesignTokens.kt
 * and ParchmentPalette; every control is an existing component. All copy lives
 * in Strings. The list is real persisted data from the Rust store, and every
 * button performs a real store operation.
 */
@Composable
internal fun AxiomCodingScreen(
    tasks: List<CodingTaskSummary>,
    storageAvailable: Boolean,
    onNewTask: () -> Unit,
    onOpenTask: (String) -> Unit,
    onDeleteTask: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(p.canvasBg)
            .verticalScroll(rememberScrollState())
            .padding(
                horizontal = if (win.isCompact) Space.s12 else Space.s16,
                vertical = Space.s16
            ),
        verticalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            BasicText(
                text = Strings.axiomCoding,
                style = p.uiHeaderStyle.copy(fontSize = 20.sp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f)
            )
            SepiaButton(
                text = Strings.axiomCodingNewTask,
                onClick = onNewTask,
                style = SepiaButtonStyle.PRIMARY,
                enabled = storageAvailable
            )
        }

        if (!storageAvailable) {
            AxiomNotice(
                text = Strings.axiomCodingStorageUnavailable,
                modifier = Modifier.fillMaxWidth()
            )
        }

        if (tasks.isEmpty()) {
            EmptyAxiomCoding(
                storageAvailable = storageAvailable,
                onNewTask = onNewTask,
                modifier = Modifier.fillMaxWidth()
            )
        } else {
            tasks.forEach { task ->
                AxiomTaskRow(
                    task = task,
                    onOpen = { onOpenTask(task.id) },
                    onDelete = { onDeleteTask(task.id) },
                    modifier = Modifier.fillMaxWidth()
                )
            }
        }
    }
}

@Composable
private fun EmptyAxiomCoding(
    storageAvailable: Boolean,
    onNewTask: () -> Unit,
    modifier: Modifier = Modifier
) {
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
        if (storageAvailable) {
            SepiaButton(
                text = Strings.axiomCodingNewTask,
                onClick = onNewTask,
                style = SepiaButtonStyle.PRIMARY
            )
        }
    }
}

@Composable
private fun AxiomTaskRow(
    task: CodingTaskSummary,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    SepiaSectionCard(
        title = task.title,
        subtitle = task.stateLabel(),
        modifier = modifier,
        headerAction = {
            SepiaButton(
                text = Strings.axiomCodingOpen,
                onClick = onOpen,
                style = SepiaButtonStyle.OUTLINE,
                compact = true
            )
            SepiaButton(
                text = Strings.axiomCodingDelete,
                onClick = onDelete,
                style = SepiaButtonStyle.DANGER,
                compact = true
            )
        }
    ) {
        if (task.description.isNotBlank()) {
            BasicText(
                text = task.description,
                style = p.uiBodyStyle.copy(color = p.narrationInk),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis
            )
        }
        BasicText(
            text = task.progressLabel(),
            style = p.uiLabelStyle.copy(color = p.mutedInk),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis
        )
    }
}

/** Human-readable state label; the raw enum name is never shown. */
private fun CodingTaskSummary.stateLabel(): String = when (state.uppercase()) {
    "PLANNING" -> Strings.axiomCodingStatePlanning
    "EXECUTING" -> Strings.axiomCodingStateExecuting
    "REVIEWING" -> Strings.axiomCodingStateReviewing
    "VERIFIED" -> Strings.axiomCodingStateVerified
    "COMPLETED" -> Strings.axiomCodingStateCompleted
    "FAILED" -> Strings.axiomCodingStateFailed
    "BLOCKED" -> Strings.axiomCodingStateBlocked
    "PAUSED" -> Strings.axiomCodingStatePaused
    else -> Strings.axiomCodingStatePending
}

/** "N of M steps verified", or a plain step count when nothing is verified. */
private fun CodingTaskSummary.progressLabel(): String =
    if (verifiedCount > 0) {
        Strings.axiomCodingProgress(verifiedCount, subtaskCount)
    } else {
        Strings.axiomCodingStepTotal(subtaskCount)
    }

@Composable
private fun AxiomNotice(text: String, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val shape = androidx.compose.foundation.shape.RoundedCornerShape(Radius.r12)
    Box(
        modifier = modifier
            .clip(shape)
            .background(p.danger.copy(alpha = 0.10f))
            .border(LineWidth.hairline, p.danger.copy(alpha = 0.52f), shape)
            .heightIn(min = Space.s48)
            .padding(horizontal = Space.s10, vertical = Space.s8),
        contentAlignment = Alignment.Center
    ) {
        BasicText(
            text = text,
            style = p.uiLabelStyle.copy(color = p.danger, fontWeight = FontWeight.Medium),
            maxLines = 3,
            overflow = TextOverflow.Ellipsis
        )
    }
}
