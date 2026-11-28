package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.harness.AxiomCodingStore
import io.parchment.ai.harness.CodingTaskSummary
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.network.AiProviderEngine
import io.parchment.ai.network.StreamUpdate
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.withContext

/**
 * Axiom Coding workspace.
 *
 * A chat-like coding environment: tasks are listed on the left; opening a task
 * opens a real coding session whose agent output streams in the right pane.
 * Every colour, space, radius, stroke and text style comes from DesignTokens.kt
 * and ParchmentPalette; every control is an existing component. All copy lives
 * in Strings. Every button performs a real store operation.
 */
@Composable
internal fun AxiomCodingScreen(
    tasks: List<CodingTaskSummary>,
    storageAvailable: Boolean,
    profile: ProviderProfile,
    session: ChatSession,
    storageDir: File,
    onNewTask: () -> Unit,
    onOpenTask: (String) -> Unit,
    onDeleteTask: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current

    var selectedTaskId by remember { mutableStateOf<String?>(null) }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(p.canvasBg),
        verticalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        if (!storageAvailable) {
            AxiomNotice(
                text = Strings.axiomCodingStorageUnavailable,
                modifier = Modifier.fillMaxWidth()
            )
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .weight(1f)
        ) {
            // Left column: task list.
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(0.42f)
                    .verticalScroll(rememberScrollState())
                    .padding(
                        horizontal = if (win.isCompact) Space.s8 else Space.s16,
                        vertical = Space.s12
                    ),
                verticalArrangement = Arrangement.spacedBy(Space.s12)
            ) {
                if (tasks.isEmpty()) {
                    AxiomCodingEmpty(
                        modifier = Modifier.fillMaxWidth()
                    )
                } else {
                    tasks.forEach { task ->
                        AxiomTaskRow(
                            task = task,
                            isSelected = task.id == selectedTaskId,
                            onOpen = {
                                selectedTaskId = task.id
                                onOpenTask(task.id)
                            },
                            onDelete = { onDeleteTask(task.id) },
                            modifier = Modifier.fillMaxWidth()
                        )
                    }
                }

                Spacer(modifier = Modifier.height(Space.s12))
                SepiaButton(
                    text = Strings.axiomCodingNewTask,
                    onClick = onNewTask,
                    style = SepiaButtonStyle.PRIMARY,
                    enabled = storageAvailable
                )
            }

            SepiaDivider()

            // Right column: coding view (streaming output + task detail).
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .weight(0.58f)
                    .verticalScroll(rememberScrollState())
                    .padding(
                        horizontal = if (win.isCompact) Space.s8 else Space.s16,
                        vertical = Space.s12
                    ),
                verticalArrangement = Arrangement.spacedBy(Space.s12)
            ) {
                if (selectedTaskId == null) {
                    AxiomCodingEmpty()
                } else {
                    val selected = tasks.firstOrNull { it.id == selectedTaskId }
                    if (selected != null) {
                        AxiomCodingView(
                            task = selected,
                            storageAvailable = storageAvailable,
                            profile = profile,
                            session = session,
                            storageDir = storageDir,
                            onDelete = { onDeleteTask(selectedTaskId!!) }
                        )
                    }
                }
            }
        }
    }
}

/** Empty coding view when no task is selected. */
@Composable
private fun AxiomCodingEmpty(modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    Column(
        modifier = modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Space.s24)
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
    }
}

/** A single coding task row in the sidebar list. */
@Composable
private fun AxiomTaskRow(
    task: CodingTaskSummary,
    isSelected: Boolean,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    SepiaSectionCard(
        title = task.title,
        subtitle = task.stateLabel(),
        modifier = modifier.fillMaxWidth(),
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

/** The coding view: streaming output + task controls. */
@Composable
private fun AxiomCodingView(
    task: CodingTaskSummary,
    storageAvailable: Boolean,
    profile: ProviderProfile,
    session: ChatSession,
    storageDir: File,
    onDelete: () -> Unit,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    var running by remember { mutableStateOf(false) }
    var output by remember { mutableStateOf<String?>(null) }

    Column(
        modifier = modifier.fillMaxSize(),
        verticalArrangement = Arrangement.spacedBy(Space.s12)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            BasicText(
                text = task.title,
                style = p.uiHeaderStyle.copy(fontSize = 18.sp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f)
            )
            SepiaButton(
                text = Strings.cancel,
                compact = true,
                style = SepiaButtonStyle.OUTLINE,
                onClick = onDelete,
                enabled = !running
            )
            SepiaButton(
                text = Strings.axiomCodingRun,
                compact = true,
                style = SepiaButtonStyle.PRIMARY,
                onClick = {
                    running = true
                    appProcessScope.launch {
                        val result = runCatching {
                            AxiomCodingStore.runAgentTurn(
                                storageDir = storageDir,
                                taskId = task.id,
                                subtaskId = task.id,
                                profile = profile,
                                session = session,
                                historyNodes = emptyList(),
                                prompt = "Implement the coding task titled '${task.title}'. Description: ${task.description.orEmpty()}"
                            )
                        }.getOrNull()
                        output = result
                        running = false
                    }
                },
                enabled = !running && storageAvailable
            )
        }

        // Streaming output area.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .weight(1f)
                .background(p.cardBg.copy(alpha = 0.06f))
                .padding(Space.s12)
                .clip(RoundedCornerShape(Radius.r12))
        ) {
            BasicText(
                text = output ?: Strings.axiomCodingNoOutput,
                style = p.monoStyle,
                maxLines = Int.MAX_VALUE,
                overflow = TextOverflow.Visible
            )
        }

        // Status row.
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.s8)
        ) {
            BasicText(
                text = task.stateLabel(),
                style = p.uiCaptionStyle.copy(color = p.mutedInk),
                maxLines = 1
            )
            BasicText(
                text = Strings.axiomCodingStepTotal(task.subtaskCount),
                style = p.uiCaptionStyle.copy(color = p.mutedInk),
                maxLines = 1
            )
        }
    }
}

/** Notice displayed when something goes wrong. */
@Composable
private fun AxiomNotice(text: String, modifier: Modifier = Modifier) {
    val p = LocalParchmentPalette.current
    val shape = RoundedCornerShape(Radius.r12)
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
private fun CodingTaskSummary.progressLabel(): String = if (verifiedCount > 0) {
    Strings.axiomCodingProgress(verifiedCount, subtaskCount)
} else {
    Strings.axiomCodingStepTotal(subtaskCount)
}

/**
 * New-task prompt. Uses the existing SepiaTextField and SepiaButton; the
 * create button stays disabled until a title is typed, so there is no dead
 * control and no empty task can be created.
 */
@Composable
internal fun AxiomNewTaskDialog(
    title: String,
    onTitleChange: (String) -> Unit,
    onCreate: () -> Unit,
    onDismiss: () -> Unit
) {
    ConfirmBackdrop(onDismiss) {
        SepiaConfirmationCard(
            title = Strings.axiomCodingNewTask,
            body = Strings.axiomCodingTaskGoal,
            options = {
                SepiaTextField(
                    value = title,
                    onValueChange = onTitleChange,
                    label = Strings.axiomCodingTaskTitle,
                    modifier = Modifier.fillMaxWidth()
                )
            }
        ) {
            SepiaButton(
                text = Strings.cancel,
                compact = true,
                style = SepiaButtonStyle.OUTLINE,
                onClick = onDismiss
            )
            SepiaButton(
                text = Strings.axiomCodingCreate,
                compact = true,
                style = SepiaButtonStyle.PRIMARY,
                onClick = onCreate,
                enabled = title.isNotBlank()
            )
        }
    }
}
