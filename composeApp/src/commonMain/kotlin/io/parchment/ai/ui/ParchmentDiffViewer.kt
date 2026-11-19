package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.strings.Strings

// Line diffs for `fs_edit`, `fs_write` and ```diff blocks, shown as added and removed lines.
enum class DiffLineKind {
    CONTEXT,
    REMOVED,
    ADDED,
    COLLAPSED
}

data class DiffLine(
    val kind: DiffLineKind,
    val text: String
)

/**
 * A unified line diff using the longest common subsequence, with unchanged runs over 6 lines collapsed.
 */
fun computeUnifiedLineDiff(oldText: String, newText: String): List<DiffLine> {
    val oldLines = if (oldText.isEmpty()) emptyList() else oldText.lines()
    val newLines = if (newText.isEmpty()) emptyList() else newText.lines()
    if (oldLines.isEmpty() && newLines.isEmpty()) return emptyList()
    if (oldLines.isEmpty()) return newLines.take(120).map { DiffLine(DiffLineKind.ADDED, it) }
    if (newLines.isEmpty()) return oldLines.take(120).map { DiffLine(DiffLineKind.REMOVED, it) }

    // Trim the shared start and end first, so the costly comparison runs only on the changed middle.
    var prefixLen = 0
    val minTotal = minOf(oldLines.size, newLines.size)
    while (prefixLen < minTotal && oldLines[prefixLen] == newLines[prefixLen]) {
        prefixLen++
    }
    var suffixLen = 0
    while (
        suffixLen < (minTotal - prefixLen) &&
        oldLines[oldLines.size - 1 - suffixLen] == newLines[newLines.size - 1 - suffixLen]
    ) {
        suffixLen++
    }

    val midOld = oldLines.subList(prefixLen, oldLines.size - suffixLen)
    val midNew = newLines.subList(prefixLen, newLines.size - suffixLen)

    val rawDiff = mutableListOf<DiffLine>()
    for (k in 0 until prefixLen) {
        rawDiff.add(DiffLine(DiffLineKind.CONTEXT, oldLines[k]))
    }
    appendMiddleDiffLines(midOld, midNew, rawDiff)
    for (k in (oldLines.size - suffixLen) until oldLines.size) {
        rawDiff.add(DiffLine(DiffLineKind.CONTEXT, oldLines[k]))
    }
    return collapseUnchangedContextRuns(rawDiff)
}

private fun appendMiddleDiffLines(
    midOld: List<String>,
    midNew: List<String>,
    rawDiff: MutableList<DiffLine>
) {
    val maxDim = 320
    if (midOld.isEmpty() && midNew.isEmpty()) return
    if (midOld.isEmpty()) {
        midNew.take(200).forEach { rawDiff.add(DiffLine(DiffLineKind.ADDED, it)) }
        return
    }
    if (midNew.isEmpty()) {
        midOld.take(200).forEach { rawDiff.add(DiffLine(DiffLineKind.REMOVED, it)) }
        return
    }
    if (midOld.size > maxDim || midNew.size > maxDim) {
        midOld.take(100).forEach { rawDiff.add(DiffLine(DiffLineKind.REMOVED, it)) }
        midNew.take(100).forEach { rawDiff.add(DiffLine(DiffLineKind.ADDED, it)) }
        return
    }

    val m = midOld.size
    val n = midNew.size
    val stride = n + 1
    val dp = IntArray((m + 1) * stride)
    for (i in m - 1 downTo 0) {
        val rowOff = i * stride
        val nextRowOff = rowOff + stride
        val oldLine = midOld[i]
        for (j in n - 1 downTo 0) {
            dp[rowOff + j] = if (oldLine == midNew[j]) {
                1 + dp[nextRowOff + j + 1]
            } else {
                maxOf(dp[nextRowOff + j], dp[rowOff + j + 1])
            }
        }
    }
    var i = 0
    var j = 0
    while (i < m && j < n) {
        if (midOld[i] == midNew[j]) {
            rawDiff.add(DiffLine(DiffLineKind.CONTEXT, midOld[i]))
            i++
            j++
        } else if (dp[(i + 1) * stride + j] >= dp[i * stride + (j + 1)]) {
            rawDiff.add(DiffLine(DiffLineKind.REMOVED, midOld[i]))
            i++
        } else {
            rawDiff.add(DiffLine(DiffLineKind.ADDED, midNew[j]))
            j++
        }
    }
    while (i < m) {
        rawDiff.add(DiffLine(DiffLineKind.REMOVED, midOld[i]))
        i++
    }
    while (j < n) {
        rawDiff.add(DiffLine(DiffLineKind.ADDED, midNew[j]))
        j++
    }
}

private fun collapseUnchangedContextRuns(rawDiff: List<DiffLine>): List<DiffLine> {
    // Collapse runs of more than 6 unchanged lines to keep the diff focused.
    val compacted = mutableListOf<DiffLine>()
    var idx = 0
    while (idx < rawDiff.size) {
        if (rawDiff[idx].kind == DiffLineKind.CONTEXT) {
            var endCtx = idx
            while (endCtx < rawDiff.size && rawDiff[endCtx].kind == DiffLineKind.CONTEXT) {
                endCtx++
            }
            val runLen = endCtx - idx
            if (runLen > 6) {
                compacted.add(rawDiff[idx])
                compacted.add(rawDiff[idx + 1])
                compacted.add(DiffLine(DiffLineKind.COLLAPSED, "⋯ ${runLen - 4} unchanged lines ⋯"))
                compacted.add(rawDiff[endCtx - 2])
                compacted.add(rawDiff[endCtx - 1])
            } else {
                for (k in idx until endCtx) compacted.add(rawDiff[k])
            }
            idx = endCtx
        } else {
            compacted.add(rawDiff[idx])
            idx++
        }
    }
    return compacted
}

internal data class SplitDiffRow(
    val left: DiffLine?,
    val right: DiffLine?
)

internal fun buildSplitDiffRows(diffLines: List<DiffLine>): List<SplitDiffRow> {
    val rows = ArrayList<SplitDiffRow>(diffLines.size)
    var idx = 0
    while (idx < diffLines.size) {
        when (diffLines[idx].kind) {
            DiffLineKind.CONTEXT, DiffLineKind.COLLAPSED -> {
                rows.add(SplitDiffRow(diffLines[idx], diffLines[idx]))
                idx++
            }
            DiffLineKind.REMOVED, DiffLineKind.ADDED -> {
                val removed = ArrayList<DiffLine>()
                val added = ArrayList<DiffLine>()
                while (idx < diffLines.size && (diffLines[idx].kind == DiffLineKind.REMOVED || diffLines[idx].kind == DiffLineKind.ADDED)) {
                    if (diffLines[idx].kind == DiffLineKind.REMOVED) removed.add(diffLines[idx])
                    else added.add(diffLines[idx])
                    idx++
                }
                val pairs = maxOf(removed.size, added.size)
                for (pIdx in 0 until pairs) {
                    rows.add(SplitDiffRow(removed.getOrNull(pIdx), added.getOrNull(pIdx)))
                }
            }
        }
    }
    return rows
}

@Composable
internal fun SepiaSplitDiffLinesList(
    diffLines: List<DiffLine>,
    wrapLines: Boolean = false,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val rows = remember(diffLines) { buildSplitDiffRows(diffLines) }
    Column(
        modifier = modifier.fillMaxWidth().padding(vertical = Space.s4),
        verticalArrangement = Arrangement.spacedBy(1.dp)
    ) {
        rows.forEach { row ->
            if (row.left?.kind == DiffLineKind.COLLAPSED) {
                BasicText(
                    text = row.left.text,
                    style = p.uiLabelStyle.copy(color = p.mutedInk, fontFamily = p.codeFontFamily, fontSize = 11.sp),
                    modifier = Modifier.padding(horizontal = Space.s12, vertical = Space.s2)
                )
            } else {
                Row(modifier = Modifier.fillMaxWidth()) {
                    SplitDiffCell(line = row.left, wrapLines = wrapLines, modifier = Modifier.weight(1f))
                    Box(Modifier.width(LineWidth.hairline).height(20.dp).background(p.border.copy(alpha = 0.6f)))
                    SplitDiffCell(line = row.right, wrapLines = wrapLines, modifier = Modifier.weight(1f))
                }
            }
        }
    }
}

@Composable
private fun SplitDiffCell(
    line: DiffLine?,
    wrapLines: Boolean = false,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val tone = when (line?.kind) {
        DiffLineKind.REMOVED -> p.danger
        DiffLineKind.ADDED -> p.success
        else -> null
    }
    val prefix = when (line?.kind) {
        DiffLineKind.REMOVED -> "- "
        DiffLineKind.ADDED -> "+ "
        DiffLineKind.CONTEXT -> "  "
        else -> ""
    }
    Box(
        modifier = modifier
            .background(tone?.copy(alpha = 0.14f) ?: Color.Transparent)
            .then(if (wrapLines) Modifier else Modifier.horizontalScroll(rememberScrollState()))
            .padding(horizontal = Space.s8, vertical = 1.dp)
    ) {
        BasicText(
            text = if (line != null) "$prefix${line.text}" else "",
            style = p.monoStyle.copy(color = tone ?: p.narrationInk, fontSize = 12.sp),
            softWrap = wrapLines,
            modifier = if (wrapLines) Modifier.fillMaxWidth() else Modifier
        )
    }
}

@Composable
fun SepiaDiffLinesList(
    diffLines: List<DiffLine>,
    wrapLines: Boolean = false,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val addedColor = p.success
    val removedColor = p.danger

    Column(
        modifier = modifier
            .fillMaxWidth()
            .then(if (wrapLines) Modifier else Modifier.horizontalScroll(rememberScrollState()))
            .padding(vertical = Space.s4),
        verticalArrangement = Arrangement.spacedBy(1.dp)
    ) {
        diffLines.forEach { line ->
            when (line.kind) {
                DiffLineKind.REMOVED -> DiffSignedLineRow(prefix = "-", text = line.text, lineColor = removedColor, wrapLines = wrapLines)
                DiffLineKind.ADDED -> DiffSignedLineRow(prefix = "+", text = line.text, lineColor = addedColor, wrapLines = wrapLines)
                DiffLineKind.COLLAPSED -> {
                    BasicText(
                        text = line.text,
                        style = p.uiLabelStyle.copy(
                            color = p.mutedInk,
                            fontFamily = p.codeFontFamily,
                            fontSize = 11.sp
                        ),
                        modifier = Modifier.padding(horizontal = Space.s12, vertical = Space.s2)
                    )
                }
                DiffLineKind.CONTEXT -> {
                    BasicText(
                        text = "  ${line.text}",
                        style = p.monoStyle.copy(color = p.narrationInk, fontSize = 12.sp),
                        softWrap = wrapLines,
                        modifier = (if (wrapLines) Modifier.fillMaxWidth() else Modifier)
                            .padding(horizontal = Space.s10, vertical = 1.dp)
                    )
                }
            }
        }
    }
}

@Composable
private fun DiffSignedLineRow(
    prefix: String,
    text: String,
    lineColor: Color,
    wrapLines: Boolean = false
) {
    val p = LocalParchmentPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(lineColor.copy(alpha = 0.14f))
            .drawBehind {
                drawRect(color = lineColor, size = Size(Space.s4.toPx(), size.height))
            }
            .padding(start = 11.dp, end = Space.s10, top = 1.dp, bottom = 1.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        BasicText(
            text = "$prefix $text",
            style = p.monoStyle.copy(color = lineColor, fontSize = 12.sp),
            softWrap = wrapLines,
            modifier = if (wrapLines) Modifier.fillMaxWidth() else Modifier
        )
    }
}

@Composable
fun SepiaRawDiffLinesView(
    rawDiffText: String,
    wrapLines: Boolean = false,
    modifier: Modifier = Modifier
) {
    val parsed = remember(rawDiffText) {
        rawDiffText.lines().map { rawLine ->
            when {
                rawLine.startsWith("+") && !rawLine.startsWith("+++") ->
                    DiffLine(DiffLineKind.ADDED, rawLine.removePrefix("+").removePrefix(" "))
                rawLine.startsWith("-") && !rawLine.startsWith("---") ->
                    DiffLine(DiffLineKind.REMOVED, rawLine.removePrefix("-").removePrefix(" "))
                rawLine.startsWith("@@") ->
                    DiffLine(DiffLineKind.COLLAPSED, rawLine)
                else ->
                    DiffLine(DiffLineKind.CONTEXT, rawLine.removePrefix(" "))
            }
        }
    }
    SepiaDiffLinesList(diffLines = parsed, wrapLines = wrapLines, modifier = modifier)
}

@Composable
fun SepiaFileDiffCard(
    filePath: String,
    actionLabel: String,
    oldText: String,
    newText: String,
    resultSummary: String = "",
    isError: Boolean = false,
    wrapCodeBlocks: Boolean = false,
    modifier: Modifier = Modifier
) {
    val p = LocalParchmentPalette.current
    val win = LocalParchmentWindowSize.current
    val diffLines = remember(oldText, newText) { computeUnifiedLineDiff(oldText, newText) }
    val addedCount = remember(diffLines) { diffLines.count { it.kind == DiffLineKind.ADDED } }
    val removedCount = remember(diffLines) { diffLines.count { it.kind == DiffLineKind.REMOVED } }
    var showDiffBody by remember { mutableStateOf(true) }
    var splitDiffMode by remember { mutableStateOf(false) }
    var isWrapped by rememberSaveable(filePath, wrapCodeBlocks) { mutableStateOf(wrapCodeBlocks) }
    val addedGreen = p.success
    val shape = RoundedCornerShape(Radius.r8)

    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(p.cardBg)
            .border(
                width = LineWidth.hairline,
                color = if (isError) p.danger.copy(alpha = 0.7f) else p.border.copy(alpha = 0.8f),
                shape = shape
            )
    ) {
        // Diff Card Header Bar
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .background(p.sheetBg.copy(alpha = 0.85f))
                .padding(horizontal = Space.s10, vertical = Space.s6),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically
        ) {
            val cleanPath = filePath.replace('\\', '/').ifBlank { "workspace file" }
            val shortFileName = cleanPath.substringAfterLast('/')
            val parentDir = if (cleanPath.contains('/')) cleanPath.substringBeforeLast('/') else ""
            Row(
                modifier = Modifier.weight(1f).padding(end = Space.s8),
                horizontalArrangement = Arrangement.spacedBy(Space.s6),
                verticalAlignment = Alignment.CenterVertically
            ) {
                KissVectorIcon(
                    icon = KissIcon.DIFF,
                    tint = if (isError) p.danger else p.accent,
                    size = IconSize.md
                )
                Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(5.dp),
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        BasicText(
                            text = actionLabel,
                            style = p.uiLabelStyle.copy(color = p.narrationInk, fontWeight = FontWeight.SemiBold, fontSize = 11.sp)
                        )
                        BasicText(
                            text = shortFileName,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = p.monoStyle.copy(color = p.accent, fontWeight = FontWeight.Bold, fontSize = 12.sp)
                        )
                    }
                    if (parentDir.isNotEmpty()) {
                        BasicText(
                            text = parentDir,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            style = p.monoStyle.copy(color = p.mutedInk, fontSize = 11.sp)
                        )
                    }
                }
            }

            Row(
                horizontalArrangement = Arrangement.spacedBy(5.dp),
                verticalAlignment = Alignment.CenterVertically
            ) {
                if (addedCount > 0) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(Radius.r6))
                            .background(addedGreen.copy(alpha = 0.16f))
                            .padding(horizontal = Space.s6, vertical = Space.s2)
                    ) {
                        BasicText(
                            text = "+$addedCount",
                            style = p.monoStyle.copy(color = addedGreen, fontWeight = FontWeight.Bold, fontSize = 11.sp)
                        )
                    }
                }
                if (removedCount > 0) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedCornerShape(Radius.r6))
                            .background(p.danger.copy(alpha = 0.16f))
                            .padding(horizontal = Space.s6, vertical = Space.s2)
                    ) {
                        BasicText(
                            text = "−$removedCount",
                            style = p.monoStyle.copy(color = p.danger, fontWeight = FontWeight.Bold, fontSize = 11.sp)
                        )
                    }
                }
                if (diffLines.isNotEmpty()) {
                    SepiaButton(
                        text = "↩",
                        contentDescription = if (isWrapped) Strings.unwrapCode else Strings.wrapCode,
                        compact = true,
                        style = if (isWrapped) SepiaButtonStyle.SECONDARY else SepiaButtonStyle.GHOST,
                        onClick = { isWrapped = !isWrapped }
                    )
                    if (!win.isCompact) {
                        SepiaButton(
                            text = "◫",
                            contentDescription = if (splitDiffMode) Strings.unifiedDiffView else Strings.splitDiffView,
                            compact = true,
                            style = if (splitDiffMode) SepiaButtonStyle.SECONDARY else SepiaButtonStyle.GHOST,
                            onClick = { splitDiffMode = !splitDiffMode }
                        )
                    }
                    SepiaButton(
                        text = if (showDiffBody) "▴" else "▾",
                        contentDescription = if (showDiffBody) Strings.hideChanges else Strings.showChanges,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = { showDiffBody = !showDiffBody }
                    )
                }
            }
        }

        if (isError && resultSummary.isNotBlank()) {
            SepiaDivider()
            BasicText(
                text = resultSummary,
                style = p.monoStyle.copy(color = p.danger, fontSize = 11.sp),
                modifier = Modifier.padding(9.dp)
            )
        } else if (showDiffBody && diffLines.isNotEmpty()) {
            SepiaDivider()
            key(isWrapped, splitDiffMode) {
                if (splitDiffMode && !win.isCompact) {
                    SepiaSplitDiffLinesList(diffLines = diffLines, wrapLines = isWrapped)
                } else {
                    SepiaDiffLinesList(diffLines = diffLines, wrapLines = isWrapped)
                }
            }
        } else if (resultSummary.isNotBlank()) {
            SepiaDivider()
            BasicText(
                text = resultSummary,
                style = p.monoStyle.copy(color = p.narrationInk, fontSize = 11.sp),
                modifier = Modifier.padding(horizontal = Space.s10, vertical = Space.s6)
            )
        }
    }
}
