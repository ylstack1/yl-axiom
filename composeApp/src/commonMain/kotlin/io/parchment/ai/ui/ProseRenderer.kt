package io.parchment.ai.ui

import androidx.compose.animation.AnimatedVisibility
import io.parchment.ai.strings.Strings
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.platform.PlatformPickers
import kotlinx.coroutines.launch

internal sealed class ProseBlock {
    data class Heading(val level: Int, val text: String) : ProseBlock()
    data class SceneBreak(val raw: String) : ProseBlock()
    data class BlockQuote(val text: String) : ProseBlock()
    data class CodeFence(val language: String, val code: String) : ProseBlock()
    data class MathBlock(val rawLatex: String) : ProseBlock()
    data class TableBlock(val headers: List<String>, val rows: List<List<String>>) : ProseBlock()
    data class Paragraph(val text: String) : ProseBlock()
}

/**
 * Incremental prefix-cached Markdown block parser for 120Hz VSync letter-by-letter streaming.
 * Keeps completed paragraphs/code-fences/tables cached in `stableBlocks` and only parses the active
 * trailing <600-character tail on each frame, turning $O(N)$ per-frame parsing into $O(1)$.
 */
/**
 * CRLF and lone CR become LF so fences, tables and headings parse the same on Windows-style replies; without it
 * a `\r` left at each line end keeps a fence open and disables the cache's fast paths. A trailing CR is dropped
 * until its LF arrives, so the result only ever grows by appending while a reply streams.
 */
internal fun normalizeLineEndings(raw: String): String {
    if (raw.indexOf('\r') == -1) return raw
    val out = StringBuilder(raw.length)
    for (i in raw.indices) {
        val c = raw[i]
        if (c != '\r') {
            out.append(c)
        } else if (i + 1 < raw.length && raw[i + 1] != '\n') {
            out.append('\n')
        }
    }
    return out.toString()
}

internal class IncrementalProseBlockCache {
    private var lastEnableMath: Boolean = true
    private var stablePrefixLen: Int = 0
    private var stableBlocks: List<ProseBlock> = emptyList()
    private var lastFullText: String = ""
    private var lastFullBlocks: List<ProseBlock> = emptyList()
    /** A split that changed the parse stays wrong until the text reaches a new break, so don't re-test it each frame. */
    private var rejectedSplitEnd = -1

    /** A still-open code fence, so each frame scans only the newly arrived lines instead of the whole fence. */
    private class OpenFence(val blocksBefore: List<ProseBlock>, val language: String, val bodyStart: Int, var scanFrom: Int)
    private var openFence: OpenFence? = null
    private var rejectedFenceLine = -1

    private fun lineClosesFence(raw: String, lineStart: Int, lineEnd: Int): Boolean {
        var at = lineStart
        while (at < lineEnd && raw[at].isWhitespace()) at++
        return at + 3 <= lineEnd && raw.startsWith("```", at)
    }

    private fun extendOpenFence(raw: String, enableMath: Boolean): List<ProseBlock>? {
        val fence = openFence ?: return null
        var lineStart = fence.scanFrom
        val stillOpen = enableMath == lastEnableMath && raw.startsWith(lastFullText) && raw.indexOf('\r', lineStart) == -1
        while (stillOpen) {
            val newline = raw.indexOf('\n', lineStart)
            if (lineClosesFence(raw, lineStart, if (newline == -1) raw.length else newline)) break
            if (newline == -1) {
                fence.scanFrom = lineStart
                var body = fence.bodyStart
                while (body < raw.length && raw[body] == '\n') body++ // The block parser drops leading blank lines.
                return fence.blocksBefore + ProseBlock.CodeFence(fence.language, raw.substring(body))
            }
            lineStart = newline + 1
        }
        openFence = null
        return null
    }

    /** Enters fence mode only after proving the shortcut gives the same blocks as the real parse. */
    private fun enterOpenFence(raw: String, tailStart: Int, tailBlocks: List<ProseBlock>, combined: List<ProseBlock>, enableMath: Boolean) {
        val last = tailBlocks.lastOrNull() as? ProseBlock.CodeFence ?: return
        if (raw.length - tailStart <= 512 || raw.indexOf('\r', tailStart) != -1) return
        var opener = raw.lastIndexOf("```")
        while (opener >= tailStart) {
            val lineStart = raw.lastIndexOf('\n', opener - 1).coerceAtLeast(tailStart - 1) + 1
            if (isRangeBlank(raw, lineStart, opener)) {
                val newline = raw.indexOf('\n', opener)
                if (newline == -1 || lineStart == rejectedFenceLine) return
                var body = newline + 1
                while (body < raw.length && raw[body] == '\n') body++
                val head = parseProseBlocks(raw, enableMath, tailStart, lineStart)
                if (head + ProseBlock.CodeFence(last.language, raw.substring(body)) == tailBlocks) {
                    openFence = OpenFence(combined.dropLast(1), last.language, newline + 1, raw.lastIndexOf('\n') + 1)
                } else {
                    rejectedFenceLine = lineStart
                }
                return
            }
            opener = raw.lastIndexOf("```", opener - 1)
        }
    }

    fun parseIncremental(text: String, enableMath: Boolean = true): List<ProseBlock> {
        val raw = normalizeLineEndings(text)
        if (raw.isBlank()) return emptyList()
        if (raw == lastFullText && enableMath == lastEnableMath) {
            return lastFullBlocks
        }
        extendOpenFence(raw, enableMath)?.let { blocks ->
            lastFullText = raw
            lastFullBlocks = blocks
            return blocks
        }
        val prefixValid = enableMath == lastEnableMath &&
            stablePrefixLen > 0 &&
            raw.length >= stablePrefixLen &&
            lastFullText.length >= stablePrefixLen &&
            raw.regionMatches(0, lastFullText, 0, stablePrefixLen)
        if (!prefixValid) {
            lastEnableMath = enableMath
            val full = parseProseBlocks(raw, enableMath)
            lastFullText = raw
            lastFullBlocks = full
            // Try to seed a stable prefix if raw is already long
            val splitAt = raw.lastIndexOf("\n\n")
            // Text must follow the break: at the very end of an open fence, a split looks valid and isn't.
            if (splitAt >= 256 && splitAt + 2 != rejectedSplitEnd && !isRangeBlank(raw, splitAt + 2, raw.length)) {
                val splitEnd = splitAt + 2
                val headB = parseProseBlocks(raw, enableMath, 0, splitEnd)
                val tailB = parseProseBlocks(raw, enableMath, splitEnd, raw.length)
                if (headB + tailB == full) {
                    stablePrefixLen = splitEnd
                    stableBlocks = headB
                } else {
                    rejectedSplitEnd = splitEnd
                    stablePrefixLen = 0
                    stableBlocks = emptyList()
                }
            } else {
                stablePrefixLen = 0
                stableBlocks = emptyList()
            }
            enterOpenFence(raw, 0, full, full, enableMath)
            return full
        }

        val tailStart = stablePrefixLen
        val tailBlocks = parseProseBlocks(raw, enableMath, tailStart, raw.length)
        val combined = if (stableBlocks.isEmpty()) tailBlocks else stableBlocks + tailBlocks

        // Advance stablePrefixLen once the active tail grows past 512 chars and has a paragraph break `\n\n`
        if (raw.length - tailStart > 512) {
            val splitAt = raw.lastIndexOf("\n\n")
            val relSplit = splitAt - tailStart
            if (relSplit >= 180 && splitAt + 2 != rejectedSplitEnd && !isRangeBlank(raw, splitAt + 2, raw.length)) {
                val splitEnd = splitAt + 2
                val headB = parseProseBlocks(raw, enableMath, tailStart, splitEnd)
                val remB = parseProseBlocks(raw, enableMath, splitEnd, raw.length)
                if (headB + remB == tailBlocks) {
                    stablePrefixLen = splitEnd
                    stableBlocks = stableBlocks + headB
                } else {
                    rejectedSplitEnd = splitEnd
                }
            }
        }

        enterOpenFence(raw, tailStart, tailBlocks, combined, enableMath)
        lastFullText = raw
        lastFullBlocks = combined
        return combined
    }
}

private val MD_CODE_FENCE_OPEN_REGEX = Regex("```[a-zA-Z0-9_-]*\\n?")
private val MD_HEADING_PREFIX_REGEX = Regex("(?m)^#{1,6}\\s+")
private val MD_INLINE_LINK_REGEX = Regex("\\[([^\\]]+)\\]\\([^)]+\\)")
private val MD_BOLD_AST_REGEX = Regex("\\*\\*([^*]+)\\*\\*")
private val MD_BOLD_UND_REGEX = Regex("__([^_]+)__")
private val MD_STRIKE_REGEX = Regex("~~([^~]+)~~")
private val MD_ITALIC_AST_REGEX = Regex("(?<!\\*)\\*([^*\n]+)\\*(?!\\*)")
private val MD_INLINE_CODE_REGEX = Regex("`([^`]+)`")

internal fun countLinesFast(s: String): Int {
    if (s.isEmpty()) return 1
    var count = 1
    for (i in 0 until s.length) {
        if (s[i] == '\n') count++
    }
    return count
}

/**
 * Splits long fenced code bodies into ~[chunkLines]-line segments at exact newline boundaries so
 * Compose skips Skia paragraph relayout on settled leading chunks while streaming.
 */
internal fun splitCodeFenceChunks(
    code: String,
    chunkLines: Int = 40,
    minLinesToChunk: Int = 60
): List<String> {
    if (code.isEmpty()) return listOf(code)
    var totalNewlines = 0
    var scan = 0
    while (totalNewlines < minLinesToChunk) {
        val nl = code.indexOf('\n', scan)
        if (nl == -1) return listOf(code)
        totalNewlines++
        scan = nl + 1
    }
    val chunks = ArrayList<String>((totalNewlines / chunkLines) + 4)
    var chunkStart = 0
    var linesInChunk = 0
    var pos = 0
    while (pos < code.length) {
        val nl = code.indexOf('\n', pos)
        if (nl == -1) break
        linesInChunk++
        if (linesInChunk == chunkLines && nl + 1 < code.length) {
            chunks.add(code.substring(chunkStart, nl))
            chunkStart = nl + 1
            linesInChunk = 0
        }
        pos = nl + 1
    }
    if (chunkStart < code.length) {
        chunks.add(code.substring(chunkStart))
    }
    return chunks
}

/**
 * Strips Markdown syntax (`**bold**`, `*italic*`, `# Heading`, `` `code` ``) to clean plain text
 * for the 1-tap `📋 Copy (Text)` button.
 */
fun stripMarkdownToPlainText(raw: String): String {
    if (raw.isEmpty()) return ""
    return raw
        .replace(MD_CODE_FENCE_OPEN_REGEX, "")
        .replace("```", "")
        .replace(MD_HEADING_PREFIX_REGEX, "")
        .replace(MD_INLINE_LINK_REGEX, "$1")
        .replace(MD_BOLD_AST_REGEX, "$1")
        .replace(MD_BOLD_UND_REGEX, "$1")
        .replace(MD_STRIKE_REGEX, "$1")
        .replace(MD_ITALIC_AST_REGEX, "$1")
        .replace(MD_INLINE_CODE_REGEX, "$1")
        .trim()
}

@Composable
fun LiteraryProseRenderer(
    text: String,
    modifier: Modifier = Modifier,
    compactReasoningMode: Boolean = false,
    enableMarkdown: Boolean = true,
    enableMath: Boolean = true,
    enableInlineDollarMath: Boolean = true,
    autoCollapseCodeBlocks: Boolean = false,
    wrapCodeBlocks: Boolean = false,
    highlightedLineSubstring: String = ""
) {
    val p = LocalParchmentPalette.current
    val scope = rememberCoroutineScope()
    @Suppress("DEPRECATION")
    val clipboard = LocalClipboardManager.current
    val baseTextStyle = if (compactReasoningMode) {
        p.uiBodyStyle.copy(
            fontFamily = p.proseFontFamily,
            fontSize = (p.config.proseFontSizeSp * 0.86f).coerceAtLeast(13f).sp,
            lineHeight = (p.config.proseFontSizeSp * 1.36f).coerceAtLeast(19f).sp,
            color = p.narrationInk
        )
    } else {
        p.proseStyle
    }

    if (!enableMarkdown) {
        SelectionContainer {
            BasicText(
                text = text,
                style = baseTextStyle,
                modifier = modifier.fillMaxWidth()
            )
        }
        return
    }

    val blockCache = remember { IncrementalProseBlockCache() }
    val blocks = remember(text, enableMath) { blockCache.parseIncremental(text, enableMath) }

    SelectionContainer {
        Column(modifier = modifier.fillMaxWidth()) {
            blocks.forEachIndexed { index, block ->
                key(index) {
                when (block) {
                    is ProseBlock.SceneBreak -> {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = if (compactReasoningMode) 4.dp else 10.dp),
                            horizontalArrangement = Arrangement.Center,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            BasicText(
                                text = "—   ",
                                style = baseTextStyle.copy(
                                    color = p.accentSecondary,
                                    fontSize = (p.config.proseFontSizeSp * 0.88f).sp
                                )
                            )
                            KissVectorIcon(
                                icon = KissIcon.SPARKLES,
                                tint = p.accentSecondary,
                                size = if (compactReasoningMode) 11.dp else 13.dp
                            )
                            BasicText(
                                text = "   —",
                                style = baseTextStyle.copy(
                                    color = p.accentSecondary,
                                    fontSize = (p.config.proseFontSizeSp * 0.88f).sp
                                )
                            )
                        }
                    }

                    is ProseBlock.Heading -> {
                        val scale = if (compactReasoningMode) {
                            when (block.level) {
                                1 -> 1.12f
                                2 -> 1.06f
                                else -> 1.02f
                            }
                        } else {
                            when (block.level) {
                                1 -> 1.34f
                                2 -> 1.20f
                                else -> 1.08f
                            }
                        }
                        val styledHeading = remember(block.text, p, enableMath, enableInlineDollarMath) {
                            styleLiteraryParagraph(
                                raw = block.text,
                                palette = p,
                                indentChars = 0,
                                enableMath = enableMath,
                                enableInlineDollarMath = enableInlineDollarMath
                            )
                        }
                        val isHighlightedHeading = remember(block.text, highlightedLineSubstring) {
                            highlightedLineSubstring.isNotBlank() &&
                                (block.text.contains(highlightedLineSubstring, ignoreCase = true) ||
                                    stripMarkdownToPlainText(block.text).contains(highlightedLineSubstring, ignoreCase = true))
                        }
                        BasicText(
                            text = styledHeading,
                            style = baseTextStyle.copy(
                                fontSize = (baseTextStyle.fontSize.value * scale).sp,
                                fontWeight = FontWeight.SemiBold,
                                color = p.primaryInk
                            ),
                            modifier = Modifier
                                .then(
                                    if (isHighlightedHeading) {
                                        Modifier
                                            .fillMaxWidth()
                                            .background(p.accent.copy(alpha = 0.14f), RoundedCornerShape(Radius.r6))
                                            .border(1.dp, p.accent.copy(alpha = 0.7f), RoundedCornerShape(Radius.r6))
                                            .padding(horizontal = Space.s8, vertical = Space.s4)
                                    } else Modifier
                                )
                                .padding(
                                    top = if (compactReasoningMode) 3.dp else 6.dp,
                                    bottom = if (compactReasoningMode) 2.dp else 4.dp
                                )
                        )
                    }

                    is ProseBlock.BlockQuote -> {
                        val styledQuote = remember(block.text, p, enableMath, enableInlineDollarMath) {
                            styleLiteraryParagraph(
                                raw = block.text,
                                palette = p,
                                indentChars = 0,
                                enableMath = enableMath,
                                enableInlineDollarMath = enableInlineDollarMath
                            )
                        }
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 3.dp)
                                .background(p.elevatedBg.copy(alpha = 0.5f), RoundedCornerShape(Radius.r6))
                                .border(LineWidth.hairline, p.border.copy(alpha = 0.7f), RoundedCornerShape(Radius.r6))
                                .padding(horizontal = Space.s12, vertical = Space.s8)
                        ) {
                            BasicText(
                                text = styledQuote,
                                style = baseTextStyle.copy(
                                    fontStyle = FontStyle.Italic,
                                    color = p.narrationInk
                                )
                            )
                        }
                    }

                    is ProseBlock.CodeFence -> {
                        var isCollapsed by remember(index, autoCollapseCodeBlocks) {
                            mutableStateOf(autoCollapseCodeBlocks)
                        }
                        var isWrapped by remember(index, wrapCodeBlocks) {
                            mutableStateOf(wrapCodeBlocks)
                        }
                        val scanState = remember(index) { CodeFenceScanState() }.update(block.code)
                        val isSvgArtifact = block.language.equals("svg", ignoreCase = true) ||
                            (scanState.hasSvgOpen && scanState.hasSvgClose)
                        val isDiffLang = block.language.equals("diff", ignoreCase = true) ||
                            block.language.equals("patch", ignoreCase = true)
                        var showSvgPreview by remember(index, isSvgArtifact) { mutableStateOf(isSvgArtifact) }
                        var copiedCode by remember(index) { mutableStateOf(false) }
                        var savedCode by remember(index) { mutableStateOf(false) }
                        val shape = RoundedCornerShape(Radius.r10)
                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = Space.s4)
                                .clip(shape)
                                .background(p.elevatedBg)
                                .border(LineWidth.hairline, p.border.copy(alpha = 0.52f), shape)
                        ) {
                            @OptIn(ExperimentalLayoutApi::class)
                            FlowRow(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(p.sheetBg.copy(alpha = 0.85f))
                                    .padding(horizontal = Space.s10, vertical = Space.s4),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalArrangement = Arrangement.spacedBy(Space.s4)
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(Space.s6),
                                    modifier = Modifier.padding(top = Space.s4)
                                ) {
                                    KissVectorIcon(
                                        icon = if (isSvgArtifact) KissIcon.IMAGE else KissIcon.CODE,
                                        tint = p.accent,
                                        size = 12.dp
                                    )
                                    val lineCount = scanState.lineCount
                                    BasicText(
                                        text = Strings.lines(block.language.ifBlank { "code" }, lineCount),
                                        style = p.uiLabelStyle.copy(fontFamily = p.codeFontFamily)
                                    )
                                }
                                Row(horizontalArrangement = Arrangement.spacedBy(Space.s4), verticalAlignment = Alignment.CenterVertically) {
                                    if (isSvgArtifact) {
                                        SepiaButton(
                                            text = if (showSvgPreview) "‹/›" else "👁",
                                            compact = true,
                                            style = if (showSvgPreview) SepiaButtonStyle.SECONDARY else SepiaButtonStyle.GHOST,
                                            contentDescription = if (showSvgPreview) Strings.showSource else Strings.showPreview,
                                            onClick = {
                                                showSvgPreview = !showSvgPreview
                                                isCollapsed = false
                                            }
                                        )
                                    }
                                    if (!isDiffLang && !(isSvgArtifact && showSvgPreview)) {
                                        SepiaButton(
                                            text = "↩",
                                            compact = true,
                                            style = if (isWrapped) SepiaButtonStyle.SECONDARY else SepiaButtonStyle.GHOST,
                                            contentDescription = if (isWrapped) Strings.unwrapCode else Strings.wrapCode,
                                            onClick = { isWrapped = !isWrapped }
                                        )
                                    }
                                    SepiaButton(
                                        text = if (isCollapsed) "▸" else "▾",
                                        compact = true,
                                        style = SepiaButtonStyle.GHOST,
                                        contentDescription = if (isCollapsed) Strings.expandCode else Strings.collapseCode,
                                        onClick = { isCollapsed = !isCollapsed }
                                    )
                                    SepiaButton(
                                        text = if (savedCode) "✓" else "📥",
                                        compact = true,
                                        style = SepiaButtonStyle.GHOST,
                                        contentDescription = Strings.saveFile,
                                        onClick = {
                                            scope.launch {
                                                val ext = inferCodeFenceExtension(block.language, isSvgArtifact)
                                                val saved = PlatformPickers.saveFileForExport(
                                                    "artifact-${index + 1}.$ext",
                                                    block.code.encodeToByteArray()
                                                )
                                                if (saved != null) savedCode = true
                                            }
                                        }
                                    )
                                    SepiaButton(
                                        text = if (copiedCode) "✓" else "📋",
                                        compact = true,
                                        style = SepiaButtonStyle.GHOST,
                                        contentDescription = if (copiedCode) Strings.copied else Strings.copyCode,
                                        onClick = {
                                            clipboard.setText(AnnotatedString(block.code))
                                            copiedCode = true
                                        }
                                    )
                                }
                            }
                            AnimatedVisibility(
                                visible = !isCollapsed,
                                enter = motionExpandEnter(),
                                exit = motionShrinkExit()
                            ) {
                                Column {
                                    SepiaDivider()
                                    if (isSvgArtifact && showSvgPreview) {
                                        InlineSvgArtifactPreview(rawSvg = block.code)
                                    } else if (isDiffLang) {
                                        SepiaRawDiffLinesView(
                                            rawDiffText = block.code,
                                            modifier = Modifier.padding(vertical = Space.s4)
                                        )
                                    } else {
                                        val codeChunks = remember(block.code) { splitCodeFenceChunks(block.code) }
                                        val scrollState = rememberScrollState()
                                        Box(
                                            modifier = Modifier
                                                .fillMaxWidth()
                                                .then(if (isWrapped) Modifier else Modifier.horizontalScroll(scrollState))
                                                .padding(11.dp)
                                        ) {
                                            if (codeChunks.size == 1) {
                                                BasicText(
                                                    text = codeChunks[0],
                                                    style = p.monoStyle,
                                                    softWrap = isWrapped
                                                )
                                            } else {
                                                // Chunk long code blocks into ~40-line paragraphs so settled prefixes skip Skia relayout while streaming.
                                                Column {
                                                    codeChunks.forEachIndexed { chunkIdx, chunk ->
                                                        key(chunkIdx) {
                                                            BasicText(
                                                                text = chunk,
                                                                style = p.monoStyle,
                                                                softWrap = isWrapped
                                                            )
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    is ProseBlock.MathBlock -> {
                        if (enableMath) {
                            LatexMathRenderer.DisplayLatexBlock(rawLatex = block.rawLatex)
                        } else {
                            BasicText(text = block.rawLatex, style = p.monoStyle)
                        }
                    }

                    is ProseBlock.TableBlock -> {
                        val win = LocalParchmentWindowSize.current
                        var copiedTable by remember(index) { mutableStateOf(false) }
                        val colCount = maxOf(
                            block.headers.size,
                            block.rows.maxOfOrNull { it.size } ?: 0
                        ).coerceAtLeast(1)
                        val needsHorizontalScroll = colCount >= 4 || win.isCompact
                        val shape = RoundedCornerShape(Radius.r8)

                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(vertical = 5.dp)
                                .clip(shape)
                                .background(p.cardBg)
                                .border(LineWidth.hairline, p.border, shape)
                        ) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .background(p.sheetBg.copy(alpha = 0.7f))
                                    .padding(horizontal = Space.s10, vertical = Space.s4),
                                horizontalArrangement = Arrangement.SpaceBetween,
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(5.dp)
                                ) {
                                    KissVectorIcon(
                                        icon = KissIcon.LAYOUT_GRID,
                                        tint = p.accent,
                                        size = IconSize.sm
                                    )
                                    BasicText(
                                        text = Strings.tableColsRows(colCount, block.rows.size),
                                        style = p.uiLabelStyle.copy(fontSize = 10.5.sp)
                                    )
                                }
                                SepiaButton(
                                    text = if (copiedTable) "✓" else "📋",
                                    compact = true,
                                    style = SepiaButtonStyle.GHOST,
                                    contentDescription = if (copiedTable) Strings.copied else Strings.copyTable,
                                    onClick = {
                                        val tsv = buildString {
                                            if (block.headers.isNotEmpty()) {
                                                appendLine(block.headers.joinToString("\t"))
                                            }
                                            block.rows.forEach { r ->
                                                appendLine(r.joinToString("\t"))
                                            }
                                        }.trim()
                                        clipboard.setText(AnnotatedString(tsv))
                                        copiedTable = true
                                    }
                                )
                            }
                            SepiaDivider()

                            val tableBodyMod = if (needsHorizontalScroll) {
                                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
                            } else {
                                Modifier.fillMaxWidth()
                            }
                            Column(modifier = tableBodyMod) {
                                if (block.headers.isNotEmpty()) {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .background(p.sheetBg)
                                            .padding(horizontal = Space.s10, vertical = 7.dp),
                                        horizontalArrangement = Arrangement.spacedBy(Space.s10)
                                    ) {
                                        for (cIdx in 0 until colCount) {
                                            val h = block.headers.getOrElse(cIdx) { "" }
                                            Box(
                                                modifier = if (needsHorizontalScroll) Modifier.width(145.dp) else Modifier.weight(1f)
                                            ) {
                                                val styledHeader = remember(h, p, enableMath, enableInlineDollarMath) {
                                                    styleLiteraryParagraph(h, p, 0, enableMath, enableInlineDollarMath)
                                                }
                                                BasicText(
                                                    text = styledHeader,
                                                    style = p.uiBodyStyle.copy(fontWeight = FontWeight.SemiBold)
                                                )
                                            }
                                        }
                                    }
                                    SepiaDivider()
                                }
                                block.rows.forEachIndexed { rIdx, rowCells ->
                                    key(rIdx) {
                                    Row(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .background(if (rIdx % 2 == 1) p.elevatedBg.copy(alpha = 0.35f) else p.cardBg)
                                            .padding(horizontal = Space.s10, vertical = Space.s6),
                                        horizontalArrangement = Arrangement.spacedBy(Space.s10)
                                    ) {
                                        for (cIdx in 0 until colCount) {
                                            val cellText = rowCells.getOrElse(cIdx) { "" }
                                            Box(
                                                modifier = if (needsHorizontalScroll) Modifier.width(145.dp) else Modifier.weight(1f)
                                            ) {
                                                val styledCell = remember(cellText, p, enableMath, enableInlineDollarMath) {
                                                    styleLiteraryParagraph(cellText, p, 0, enableMath, enableInlineDollarMath)
                                                }
                                                BasicText(
                                                    text = styledCell,
                                                    style = p.uiBodyStyle
                                                )
                                            }
                                        }
                                    }
                                    if (rIdx < block.rows.lastIndex) {
                                        SepiaDivider()
                                    }
                                    }
                                }
                            }
                        }
                    }

                    is ProseBlock.Paragraph -> {
                        val styled = remember(block.text, p, compactReasoningMode, enableMath, enableInlineDollarMath) {
                            styleLiteraryParagraph(
                                raw = block.text,
                                palette = p,
                                indentChars = if (compactReasoningMode) 0 else p.config.firstLineIndentChars,
                                enableMath = enableMath,
                                enableInlineDollarMath = enableInlineDollarMath
                            )
                        }
                        val isHighlightedPara = remember(block.text, highlightedLineSubstring) {
                            if (highlightedLineSubstring.isBlank()) {
                                false
                            } else if (block.text.contains(highlightedLineSubstring, ignoreCase = true)) {
                                true
                            } else {
                                val plain = stripMarkdownToPlainText(block.text)
                                plain.contains(highlightedLineSubstring, ignoreCase = true) ||
                                    (block.text.length >= 24 && highlightedLineSubstring.contains(plain, ignoreCase = true))
                            }
                        }
                        BasicText(
                            text = styled,
                            style = baseTextStyle,
                            modifier = if (isHighlightedPara) {
                                Modifier
                                    .fillMaxWidth()
                                    .background(p.accent.copy(alpha = 0.14f), RoundedCornerShape(Radius.r6))
                                    .border(1.dp, p.accent.copy(alpha = 0.7f), RoundedCornerShape(Radius.r6))
                                    .padding(horizontal = Space.s8, vertical = Space.s4)
                            } else Modifier
                        )
                    }
                }

                if (index < blocks.lastIndex) {
                    val nextBlock = blocks[index + 1]
                    val bothAreListItems = block is ProseBlock.Paragraph && nextBlock is ProseBlock.Paragraph &&
                        isListItemParagraph(block.text) && isListItemParagraph(nextBlock.text)
                    val gap = when {
                        bothAreListItems -> if (compactReasoningMode) 7.dp else 9.dp
                        compactReasoningMode -> 10.dp
                        else -> p.paragraphSpacing.coerceAtLeast(16.dp)
                    }
                    Spacer(Modifier.height(gap))
                }
                }
            }
        }
    }
}

private val NUMBERED_LIST_ITEM_REGEX = Regex("""^\s*\d+[.)]\s+.*""")
private val STANDALONE_LATEX_ENV_OPEN_REGEX = Regex("""^\\begin\{(equation\*?|align\*?|aligned|gather\*?|gathered|pmatrix|bmatrix|Bmatrix|vmatrix|Vmatrix|matrix|cases|split)\}""")

private fun isListItemParagraph(text: String): Boolean {
    val t = text.trimStart()
    return t.startsWith("• ") || t.startsWith("☐ ") || t.startsWith("☑ ") || NUMBERED_LIST_ITEM_REGEX.matches(t)
}

private class CodeFenceScanState {
    var lastCode: String = ""
    var lineCount: Int = 1
    var hasSvgOpen: Boolean = false
    var hasSvgClose: Boolean = false

    fun update(code: String): CodeFenceScanState {
        if (code === lastCode || code == lastCode) return this
        val prev = lastCode
        val isAppend = prev.isNotEmpty() && code.length > prev.length && code.startsWith(prev)
        if (!isAppend) {
            lineCount = countLinesFast(code)
            hasSvgOpen = code.contains("<svg", ignoreCase = true)
            hasSvgClose = code.contains("</svg>", ignoreCase = true)
            lastCode = code
            return this
        }
        val start = prev.length
        var extraLines = 0
        for (idx in start until code.length) {
            if (code[idx] == '\n') extraLines++
        }
        lineCount += extraLines
        if (!hasSvgOpen) {
            hasSvgOpen = code.indexOf("<svg", startIndex = (start - 3).coerceAtLeast(0), ignoreCase = true) != -1
        }
        if (!hasSvgClose) {
            hasSvgClose = code.indexOf("</svg>", startIndex = (start - 5).coerceAtLeast(0), ignoreCase = true) != -1
        }
        lastCode = code
        return this
    }
}

private fun isRangeBlank(s: String, start: Int, end: Int): Boolean {
    for (idx in start until end) {
        if (!s[idx].isWhitespace()) return false
    }
    return true
}

private fun trimStartIdx(s: String, start: Int, end: Int): Int {
    var idx = start
    while (idx < end && s[idx].isWhitespace()) idx++
    return idx
}

private fun trimEndIdx(s: String, start: Int, end: Int): Int {
    var idx = end
    while (idx > start && s[idx - 1].isWhitespace()) idx--
    return idx
}

private fun startsWithRange(s: String, start: Int, end: Int, prefix: String, ignoreCase: Boolean = false): Boolean =
    end - start >= prefix.length && s.regionMatches(start, prefix, 0, prefix.length, ignoreCase = ignoreCase)

private fun endsWithRange(s: String, start: Int, end: Int, suffix: String): Boolean =
    end - start >= suffix.length && s.regionMatches(end - suffix.length, suffix, 0, suffix.length)

private fun equalsRange(s: String, start: Int, end: Int, target: String): Boolean =
    end - start == target.length && s.regionMatches(start, target, 0, target.length)

private fun countCharInRange(s: String, start: Int, end: Int, ch: Char): Int {
    var count = 0
    for (idx in start until end) {
        if (s[idx] == ch) count++
    }
    return count
}

private fun containsRange(s: String, start: Int, end: Int, needle: String): Boolean {
    if (needle.isEmpty()) return true
    if (end - start < needle.length) return false
    val idx = s.indexOf(needle, startIndex = start)
    return idx != -1 && idx + needle.length <= end
}

private fun indexOfCharBounded(s: String, ch: Char, startIndex: Int, endIndex: Int): Int {
    for (idx in startIndex until endIndex) {
        if (s[idx] == ch) return idx
    }
    return -1
}

private fun indexOfBounded(s: String, needle: String, startIndex: Int, endIndex: Int): Int {
    val needleLen = needle.length
    val maxStart = endIndex - needleLen
    if (maxStart < startIndex) return -1
    if (endIndex == s.length) {
        return s.indexOf(needle, startIndex = startIndex)
    }
    val first = needle[0]
    var idx = startIndex
    while (idx <= maxStart) {
        if (s[idx] == first && s.regionMatches(idx, needle, 0, needleLen)) {
            return idx
        }
        idx++
    }
    return -1
}

private fun buildPrefixedItem(glyph: String, raw: String, bodyStart: Int, lineEnd: Int, indentLevel: Int): String {
    val indent = if (indentLevel > 0) "   ".repeat(indentLevel) else ""
    val trimmedBodyEnd = trimEndIdx(raw, bodyStart, lineEnd)
    return if (trimmedBodyEnd > bodyStart) {
        buildString(indent.length + 2 + (trimmedBodyEnd - bodyStart)) {
            append(indent)
            append(glyph)
            append(' ')
            append(raw, bodyStart, trimmedBodyEnd)
        }
    } else {
        "$indent$glyph"
    }
}

internal fun parseProseBlocks(
    raw: String,
    enableMath: Boolean = true,
    rangeStart: Int = 0,
    rangeEnd: Int = raw.length
): List<ProseBlock> {
    if (rangeStart >= rangeEnd || isRangeBlank(raw, rangeStart, rangeEnd)) return emptyList()

    var cap = ((rangeEnd - rangeStart) ushr 5).coerceIn(16, 4096)
    var lineStarts = IntArray(cap)
    var lineEnds = IntArray(cap)
    var lineCount = 0

    fun pushLine(s: Int, e: Int) {
        if (lineCount == cap) {
            cap = cap shl 1
            lineStarts = lineStarts.copyOf(cap)
            lineEnds = lineEnds.copyOf(cap)
        }
        lineStarts[lineCount] = s
        lineEnds[lineCount] = e
        lineCount++
    }

    var pos = rangeStart
    while (pos < rangeEnd) {
        var end = pos
        while (end < rangeEnd) {
            val c = raw[end]
            if (c == '\n' || c == '\r') break
            end++
        }
        pushLine(pos, end)
        if (end >= rangeEnd) {
            pos = rangeEnd
        } else {
            val sepLen = if (raw[end] == '\r' && end + 1 < rangeEnd && raw[end + 1] == '\n') 2 else 1
            pos = end + sepLen
            if (pos == rangeEnd) {
                pushLine(pos, pos)
            }
        }
    }

    val blocks = mutableListOf<ProseBlock>()
    var i = 0

    fun appendClosingMathLine(
        mathBuf: StringBuilder,
        mts: Int,
        mte: Int,
        suffixLen: Int
    ) {
        val closeEnd = trimEndIdx(raw, mts, mte - suffixLen)
        if (mts < closeEnd) {
            if (mathBuf.isNotEmpty()) mathBuf.append('\n')
            mathBuf.append(raw, mts, closeEnd)
        }
    }

    while (i < lineCount) {
        val ls = lineStarts[i]
        val le = lineEnds[i]
        val ts = trimStartIdx(raw, ls, le)
        val te = trimEndIdx(raw, ts, le)

        when {
            ts >= te -> {
                // Blank line already breaks blocks; non-empty lines emit immediately.
            }

            startsWithRange(raw, ts, te, "```") -> {
                val langStart = trimStartIdx(raw, ts + 3, te)
                val lang = raw.substring(langStart, te)
                i++
                var bodyFirstNonEmpty = -1
                var bodyLastLine = -1
                var hasNonLfSep = false
                var prevEnd = -1
                while (i < lineCount) {
                    val bls = lineStarts[i]
                    val ble = lineEnds[i]
                    val bts = trimStartIdx(raw, bls, ble)
                    if (startsWithRange(raw, bts, ble, "```")) break
                    if (bodyFirstNonEmpty == -1) {
                        if (ble > bls) {
                            bodyFirstNonEmpty = i
                            bodyLastLine = i
                            prevEnd = ble
                        }
                    } else {
                        if (bls != prevEnd + 1 || raw[prevEnd] != '\n') {
                            hasNonLfSep = true
                        }
                        bodyLastLine = i
                        prevEnd = ble
                    }
                    i++
                }
                val codeText = when {
                    bodyFirstNonEmpty == -1 -> ""
                    !hasNonLfSep -> raw.substring(lineStarts[bodyFirstNonEmpty], lineEnds[bodyLastLine])
                    else -> buildString(lineEnds[bodyLastLine] - lineStarts[bodyFirstNonEmpty]) {
                        for (li in bodyFirstNonEmpty..bodyLastLine) {
                            if (li > bodyFirstNonEmpty) append('\n')
                            append(raw, lineStarts[li], lineEnds[li])
                        }
                    }
                }
                if (enableMath && (lang.equals("math", ignoreCase = true) || lang.equals("latex", ignoreCase = true) || lang.equals("tex", ignoreCase = true))) {
                    blocks.add(ProseBlock.MathBlock(codeText))
                } else {
                    blocks.add(ProseBlock.CodeFence(lang, codeText))
                }
            }

            enableMath && startsWithRange(raw, ts, te, "$$") -> {
                if (te - ts > 4 && endsWithRange(raw, ts, te, "$$")) {
                    blocks.add(ProseBlock.MathBlock(raw.substring(ts + 2, te - 2)))
                } else {
                    val mathBuf = StringBuilder()
                    val firstStart = trimStartIdx(raw, ts + 2, te)
                    if (firstStart < te) mathBuf.append(raw, firstStart, te)
                    i++
                    var mts = 0
                    var mte = 0
                    while (i < lineCount) {
                        val mls = lineStarts[i]
                        val mle = lineEnds[i]
                        mts = trimStartIdx(raw, mls, mle)
                        mte = trimEndIdx(raw, mts, mle)
                        if (endsWithRange(raw, mts, mte, "$$")) break
                        if (mathBuf.isNotEmpty()) mathBuf.append('\n')
                        mathBuf.append(raw, mls, mle)
                        i++
                    }
                    if (i < lineCount) {
                        appendClosingMathLine(mathBuf, mts, mte, 2)
                    }
                    blocks.add(ProseBlock.MathBlock(mathBuf.toString()))
                }
            }

            enableMath && startsWithRange(raw, ts, te, "\\[") -> {
                if (endsWithRange(raw, ts, te, "\\]")) {
                    blocks.add(ProseBlock.MathBlock(raw.substring(ts + 2, te - 2)))
                } else {
                    val mathBuf = StringBuilder()
                    val firstStart = trimStartIdx(raw, ts + 2, te)
                    if (firstStart < te) mathBuf.append(raw, firstStart, te)
                    i++
                    var mts = 0
                    var mte = 0
                    while (i < lineCount) {
                        val mls = lineStarts[i]
                        val mle = lineEnds[i]
                        mts = trimStartIdx(raw, mls, mle)
                        mte = trimEndIdx(raw, mts, mle)
                        if (endsWithRange(raw, mts, mte, "\\]")) break
                        if (mathBuf.isNotEmpty()) mathBuf.append('\n')
                        mathBuf.append(raw, mls, mle)
                        i++
                    }
                    if (i < lineCount) {
                        appendClosingMathLine(mathBuf, mts, mte, 2)
                    }
                    blocks.add(ProseBlock.MathBlock(mathBuf.toString()))
                }
            }

            enableMath && startsWithRange(raw, ts, te, "\\begin{") -> {
                val trimmed = raw.substring(ts, te)
                val envMatch = STANDALONE_LATEX_ENV_OPEN_REGEX.find(trimmed)
                if (envMatch != null) {
                    val closeTag = "\\end{${envMatch.groupValues[1]}}"
                    val mathBuf = StringBuilder(trimmed)
                    while (i + 1 < lineCount && !containsRange(raw, lineStarts[i], lineEnds[i], closeTag)) {
                        i++
                        mathBuf.append('\n').append(raw, lineStarts[i], lineEnds[i])
                    }
                    blocks.add(ProseBlock.MathBlock(mathBuf.toString()))
                } else {
                    blocks.add(ProseBlock.Paragraph(trimmed))
                }
            }

            raw[ts] == '|' && raw[te - 1] == '|' && countCharInRange(raw, ts, te, '|') >= 2 -> {
                val tableLines = mutableListOf<String>()
                while (i < lineCount) {
                    val tls = trimStartIdx(raw, lineStarts[i], lineEnds[i])
                    val tle = trimEndIdx(raw, tls, lineEnds[i])
                    if (tls >= tle || raw[tls] != '|' || indexOfCharBounded(raw, '|', tls, tle) == -1) break
                    tableLines.add(raw.substring(tls, tle))
                    i++
                }
                val parsedRows = tableLines.map { rowStr ->
                    rowStr.trim('|').split('|').map { it.trim() }
                }
                if (parsedRows.size >= 2 && parsedRows[1].all { cell -> cell.all { c -> c == '-' || c == ':' || c == ' ' } }) {
                    blocks.add(ProseBlock.TableBlock(headers = parsedRows.first(), rows = parsedRows.drop(2)))
                } else {
                    blocks.add(ProseBlock.TableBlock(headers = parsedRows.firstOrNull().orEmpty(), rows = parsedRows.drop(1)))
                }
                continue
            }

            te - ts in 3..5 && (equalsRange(raw, ts, te, "---") || equalsRange(raw, ts, te, "***") ||
                equalsRange(raw, ts, te, "* * *") || equalsRange(raw, ts, te, "___")) -> {
                blocks.add(ProseBlock.SceneBreak(raw.substring(ts, te)))
            }

            raw[ts] == '#' -> {
                var hashes = 0
                while (ts + hashes < te && raw[ts + hashes] == '#') hashes++
                val level = hashes.coerceIn(1, 4)
                val hStart = trimStartIdx(raw, ts + level, te)
                if (hStart < te) {
                    blocks.add(ProseBlock.Heading(level, raw.substring(hStart, te)))
                }
            }

            raw[ts] == '>' -> {
                val quoteBuf = StringBuilder()
                while (i < lineCount) {
                    val qls = lineStarts[i]
                    val qle = lineEnds[i]
                    val qts = trimStartIdx(raw, qls, qle)
                    if (qts >= qle || raw[qts] != '>') break
                    val qte = trimEndIdx(raw, qts, qle)
                    val qContentStart = trimStartIdx(raw, qts + 1, qte)
                    if (quoteBuf.isNotEmpty()) quoteBuf.append('\n')
                    if (qContentStart < qte) quoteBuf.append(raw, qContentStart, qte)
                    i++
                }
                blocks.add(ProseBlock.BlockQuote(quoteBuf.toString()))
                continue
            }

            else -> {
                var k = ls
                var rawIndentCols = 0
                while (k < le && (raw[k] == ' ' || raw[k] == '\t')) {
                    rawIndentCols += if (raw[k] == '\t') 2 else 1
                    k++
                }
                val indentLevel = (rawIndentCols / 2).coerceIn(0, 6)
                val normalizedLine = when {
                    startsWithRange(raw, ts, le, "- [ ] ") || startsWithRange(raw, ts, le, "* [ ] ") ->
                        buildPrefixedItem("☐", raw, ts + 6, le, indentLevel)
                    startsWithRange(raw, ts, le, "- [x] ", ignoreCase = true) ||
                        startsWithRange(raw, ts, le, "* [x] ", ignoreCase = true) ->
                        buildPrefixedItem("☑", raw, ts + 6, le, indentLevel)
                    startsWithRange(raw, ts, le, "* ") ||
                        startsWithRange(raw, ts, le, "- ") ||
                        startsWithRange(raw, ts, le, "+ ") ->
                        buildPrefixedItem("•", raw, ts + 2, le, indentLevel)
                    indentLevel > 0 && raw[ts].isDigit() -> {
                        val afterSpaces = raw.substring(ts, le)
                        if (NUMBERED_LIST_ITEM_REGEX.matches(afterSpaces)) {
                            ("${"   ".repeat(indentLevel)}$afterSpaces").trimEnd()
                        } else {
                            raw.substring(ts, te)
                        }
                    }
                    else -> raw.substring(ts, te)
                }
                blocks.add(ProseBlock.Paragraph(normalizedLine))
            }
        }
        i++
    }
    return blocks
}

private const val MAX_INLINE_NESTING = 16
private const val MAX_STYLED_PARAGRAPH_CHARS = 20_000

private class ParagraphStyleSheet(private val palette: ParchmentPalette) {
    private var _boldItalic: SpanStyle? = null
    private var _bold: SpanStyle? = null
    private var _strike: SpanStyle? = null
    private var _link: SpanStyle? = null
    private var _linkTextStyles: TextLinkStyles? = null
    private var _italic: SpanStyle? = null
    private var _dialogue: SpanStyle? = null
    private var _code: SpanStyle? = null

    fun boldItalic(): SpanStyle = _boldItalic ?: SpanStyle(
        color = palette.primaryInk,
        fontWeight = FontWeight.Bold,
        fontStyle = FontStyle.Italic
    ).also { _boldItalic = it }

    fun bold(): SpanStyle = _bold ?: SpanStyle(
        color = palette.primaryInk,
        fontWeight = FontWeight.SemiBold
    ).also { _bold = it }

    fun strike(): SpanStyle = _strike ?: SpanStyle(
        color = palette.mutedInk,
        textDecoration = TextDecoration.LineThrough
    ).also { _strike = it }

    fun link(): SpanStyle = _link ?: SpanStyle(
        color = palette.accent,
        fontWeight = FontWeight.Medium,
        textDecoration = TextDecoration.Underline
    ).also { _link = it }

    fun linkTextStyles(): TextLinkStyles = _linkTextStyles ?: TextLinkStyles(style = link()).also { _linkTextStyles = it }

    fun italic(): SpanStyle = _italic ?: SpanStyle(
        color = palette.narrationInk,
        fontStyle = FontStyle.Italic
    ).also { _italic = it }

    fun dialogue(): SpanStyle = _dialogue ?: SpanStyle(
        color = palette.dialogueInk,
        fontWeight = FontWeight.Normal
    ).also { _dialogue = it }

    fun code(): SpanStyle = _code ?: SpanStyle(
        fontFamily = palette.codeFontFamily,
        fontSize = (palette.config.proseFontSizeSp * 0.90f).coerceAtLeast(11f).sp,
        background = palette.accent.copy(alpha = 0.12f),
        color = palette.accent,
        fontWeight = FontWeight.Medium
    ).also { _code = it }
}

internal fun styleLiteraryParagraph(
    raw: String,
    palette: ParchmentPalette,
    indentChars: Int,
    enableMath: Boolean = true,
    enableInlineDollarMath: Boolean = true,
    depth: Int = 0
): AnnotatedString = buildAnnotatedString {
    val indentSpaces = if (indentChars > 0) indentChars * 2 else 0
    if (depth > MAX_INLINE_NESTING || indentSpaces + raw.length > MAX_STYLED_PARAGRAPH_CHARS) {
        repeat(indentSpaces) { append(' ') }
        append(raw)
        return@buildAnnotatedString
    }
    repeat(indentSpaces) { append(' ') }
    if (raw.isEmpty()) return@buildAnnotatedString
    val styles = ParagraphStyleSheet(palette)
    appendStyledRange(
        builder = this,
        text = raw,
        start = 0,
        end = raw.length,
        palette = palette,
        styles = styles,
        enableMath = enableMath,
        enableInlineDollarMath = enableInlineDollarMath,
        allowDialogue = palette.config.highlightDialogueAndNarration,
        depth = depth
    )
}

private fun appendStyledRange(
    builder: AnnotatedString.Builder,
    text: String,
    start: Int,
    end: Int,
    palette: ParchmentPalette,
    styles: ParagraphStyleSheet,
    enableMath: Boolean,
    enableInlineDollarMath: Boolean,
    allowDialogue: Boolean,
    depth: Int
) {
    if (depth > MAX_INLINE_NESTING) {
        if (end > start) builder.append(text, start, end)
        return
    }

    var i = start
    var literalStart = start

    fun flushLiterals() {
        if (i > literalStart) {
            builder.append(text, literalStart, i)
            literalStart = i
        }
    }

    fun appendStyledSpan(style: SpanStyle, innerStart: Int, innerEnd: Int, afterCloseIdx: Int) {
        flushLiterals()
        builder.pushStyle(style)
        appendStyledRange(
            builder = builder,
            text = text,
            start = innerStart,
            end = innerEnd,
            palette = palette,
            styles = styles,
            enableMath = enableMath,
            enableInlineDollarMath = enableInlineDollarMath,
            allowDialogue = allowDialogue,
            depth = depth + 1
        )
        builder.pop()
        i = afterCloseIdx
        literalStart = i
    }

    while (i < end) {
        val ch = text[i]
        when {
            // Escaped Markdown / LaTeX punctuation (\*, \_, \$, \`, \[)
            ch == '\\' && i + 1 < end && text[i + 1] in "*_~`[]$\\" && text[i + 1] != '(' && text[i + 1] != '[' -> {
                flushLiterals()
                literalStart = i + 1
                i += 2
            }

            // Inline Display Math: $$...$$
            enableMath && i + 1 < end && ch == '$' && text[i + 1] == '$' -> {
                val endIdx = indexOfBounded(text, "$$", i + 2, end)
                if (endIdx != -1) {
                    flushLiterals()
                    builder.append(LatexMathRenderer.formatInlineLatexToAnnotatedString(text.substring(i + 2, endIdx), palette))
                    i = endIdx + 2
                    literalStart = i
                } else {
                    i++
                }
            }

            // Inline LaTeX Math: \( ... \)
            enableMath && i + 1 < end && ch == '\\' && text[i + 1] == '(' -> {
                val endIdx = indexOfBounded(text, "\\)", i + 2, end)
                if (endIdx != -1) {
                    flushLiterals()
                    builder.append(LatexMathRenderer.formatInlineLatexToAnnotatedString(text.substring(i + 2, endIdx), palette))
                    i = endIdx + 2
                    literalStart = i
                } else {
                    i++
                }
            }

            // Inline LaTeX Math: $...$ (context-aware: avoids currency "$50 to $100", shell vars "$env:FOO", and `$code$`)
            enableMath && enableInlineDollarMath && ch == '$' && i + 1 < end && !text[i + 1].isWhitespace() -> {
                val endIdx = findClosingInlineDollar(text, i + 1, end)
                if (endIdx != -1) {
                    flushLiterals()
                    builder.append(LatexMathRenderer.formatInlineLatexToAnnotatedString(text.substring(i + 1, endIdx), palette))
                    i = endIdx + 1
                    literalStart = i
                } else {
                    i++
                }
            }

            // Bold + Italic: ***...***
            i + 2 < end && ch == '*' && text[i + 1] == '*' && text[i + 2] == '*' -> {
                val endIdx = findClosingTripleAsterisk(text, i + 3, end)
                if (endIdx != -1) {
                    appendStyledSpan(styles.boldItalic(), i + 3, endIdx, endIdx + 3)
                } else {
                    i++
                }
            }

            // Bold: **...** (skips inner `code` spans and handles overlapping *** closers)
            i + 1 < end && ch == '*' && text[i + 1] == '*' -> {
                val endIdx = findClosingDoubleAsterisk(text, i + 2, end)
                if (endIdx != -1) {
                    appendStyledSpan(styles.bold(), i + 2, endIdx, endIdx + 2)
                } else {
                    i++
                }
            }

            // Bold with underscores: __...__
            i + 1 < end && ch == '_' && text[i + 1] == '_' -> {
                val endIdx = findClosingDoubleDelimiter(text, i + 2, end, '_')
                if (endIdx != -1) {
                    appendStyledSpan(styles.bold(), i + 2, endIdx, endIdx + 2)
                } else {
                    i++
                }
            }

            // Strikethrough: ~~...~~
            i + 1 < end && ch == '~' && text[i + 1] == '~' -> {
                val endIdx = findClosingDoubleDelimiter(text, i + 2, end, '~')
                if (endIdx != -1) {
                    appendStyledSpan(styles.strike(), i + 2, endIdx, endIdx + 2)
                } else {
                    i++
                }
            }

            // Markdown Link: [label](url) with balanced-parenthesis URL support, nested label styling & 1-tap browser launch
            ch == '[' -> {
                val closeBracket = findClosingLinkBracket(text, i + 1, end)
                val openParen = if (closeBracket != -1 && closeBracket + 1 < end && text[closeBracket + 1] == '(') closeBracket + 1 else -1
                val closeParen = if (openParen != -1) findClosingMarkdownLinkParen(text, openParen, end) else -1
                if (closeBracket != -1 && openParen != -1 && closeParen != -1) {
                    flushLiterals()
                    val linkUrl = text.substring(openParen + 1, closeParen).trim()
                    val linkStyle = styles.link()
                    builder.pushLink(
                        LinkAnnotation.Url(
                            url = linkUrl,
                            styles = styles.linkTextStyles(),
                            linkInteractionListener = { openWebLink(linkUrl) }
                        )
                    )
                    builder.pushStyle(linkStyle)
                    appendStyledRange(
                        builder = builder,
                        text = text,
                        start = i + 1,
                        end = closeBracket,
                        palette = palette,
                        styles = styles,
                        enableMath = enableMath,
                        enableInlineDollarMath = enableInlineDollarMath,
                        allowDialogue = allowDialogue,
                        depth = depth + 1
                    )
                    builder.pop()
                    builder.pop()
                    i = closeParen + 1
                    literalStart = i
                } else {
                    i++
                }
            }

            // Italic narration or action: *...* (skips nested **bold**, `code` and $math$ spans).
            ch == '*' && i + 1 < end && !text[i + 1].isWhitespace() -> {
                val endIdx = findClosingSingleAsterisk(text, i + 1, end)
                if (endIdx != -1) {
                    appendStyledSpan(styles.italic(), i + 1, endIdx, endIdx + 1)
                } else {
                    i++
                }
            }

            // Underscore Italic: _..._ (word-boundary guarded so snake_case_identifiers are never mangled)
            ch == '_' && i + 1 < end && !text[i + 1].isWhitespace() && (i == start || !text[i - 1].isLetterOrDigit()) -> {
                val endIdx = findClosingSingleUnderscore(text, i + 1, end)
                if (endIdx != -1) {
                    appendStyledSpan(styles.italic(), i + 1, endIdx, endIdx + 1)
                } else {
                    i++
                }
            }

            // Spoken dialogue: "..." or “...”, with inner *italic*, **bold** and `code` spans parsed too.
            (ch == '"' || ch == '“') && allowDialogue &&
                (i == start || text[i - 1].isWhitespace() || text[i - 1] in "([*_—-") -> {
                val closeChar = if (ch == '“') '”' else '"'
                val endIdx = findClosingQuoteSkippingCode(text, i + 1, end, closeChar)
                if (endIdx != -1) {
                    flushLiterals()
                    builder.pushStyle(styles.dialogue())
                    builder.append(ch)
                    appendStyledRange(
                        builder = builder,
                        text = text,
                        start = i + 1,
                        end = endIdx,
                        palette = palette,
                        styles = styles,
                        enableMath = enableMath,
                        enableInlineDollarMath = enableInlineDollarMath,
                        allowDialogue = false,
                        depth = depth + 1
                    )
                    builder.append(closeChar)
                    builder.pop()
                    i = endIdx + 1
                    literalStart = i
                } else {
                    i++
                }
            }

            // Double-Backtick Inline Code: ``...`` (allows literal single backticks inside inline code)
            i + 1 < end && ch == '`' && text[i + 1] == '`' -> {
                val endIdx = indexOfBounded(text, "``", i + 2, end)
                if (endIdx != -1) {
                    flushLiterals()
                    val innerStart = trimStartIdx(text, i + 2, endIdx)
                    val innerEnd = trimEndIdx(text, innerStart, endIdx)
                    builder.pushStyle(styles.code())
                    if (innerEnd > innerStart) builder.append(text, innerStart, innerEnd)
                    builder.pop()
                    i = endIdx + 2
                    literalStart = i
                } else {
                    i++
                }
            }

            // Single-Backtick Inline Code: `...`
            ch == '`' -> {
                val endIdx = indexOfCharBounded(text, '`', i + 1, end)
                if (endIdx != -1) {
                    flushLiterals()
                    builder.pushStyle(styles.code())
                    if (endIdx > i + 1) builder.append(text, i + 1, endIdx)
                    builder.pop()
                    i = endIdx + 1
                    literalStart = i
                } else {
                    i++
                }
            }

            else -> {
                i++
            }
        }
    }
    flushLiterals()
}

/**
 * Skips over an inline code span (`` `...` `` or ```` ``...`` ````) starting at `idx` so delimiters
 * inside code spans are never misinterpreted as Markdown emphasis or LaTeX math closers.
 */
private fun skipInlineCodeSpan(text: String, idx: Int, rangeEnd: Int): Int {
    if (idx + 1 < rangeEnd && text[idx] == '`' && text[idx + 1] == '`') {
        val close = indexOfBounded(text, "``", idx + 2, rangeEnd)
        return if (close != -1) close + 2 else idx + 2
    }
    if (idx < rangeEnd && text[idx] == '`') {
        val close = indexOfCharBounded(text, '`', idx + 1, rangeEnd)
        return if (close != -1) close + 1 else idx + 1
    }
    return idx + 1
}

private fun findClosingTripleAsterisk(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    while (cursor + 2 < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == '*' && text[cursor + 1] == '*' && text[cursor + 2] == '*') {
            return cursor
        }
        cursor++
    }
    return -1
}

private fun findClosingDoubleAsterisk(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    var singleItalicOpen = false
    while (cursor + 1 < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (cursor + 2 < rangeEnd && c == '*' && text[cursor + 1] == '*' && text[cursor + 2] == '*') {
            return if (singleItalicOpen) cursor + 1 else cursor
        }
        if (c == '*' && text[cursor + 1] == '*') {
            return cursor
        }
        if (c == '*') {
            singleItalicOpen = !singleItalicOpen
        }
        cursor++
    }
    return -1
}

private fun findClosingSingleAsterisk(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    var doubleBoldOpen = false
    while (cursor < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (cursor + 2 < rangeEnd && c == '*' && text[cursor + 1] == '*' && text[cursor + 2] == '*') {
            if (doubleBoldOpen) {
                if (cursor > startIdx && !text[cursor - 1].isWhitespace()) {
                    return cursor + 2
                }
            } else {
                val endTriple = findClosingTripleAsterisk(text, cursor + 3, rangeEnd)
                cursor = if (endTriple != -1) endTriple + 3 else cursor + 3
                continue
            }
        }
        if (cursor + 1 < rangeEnd && c == '*' && text[cursor + 1] == '*') {
            doubleBoldOpen = !doubleBoldOpen
            cursor += 2
            continue
        }
        if (c == '*' && !doubleBoldOpen) {
            if (cursor > startIdx && !text[cursor - 1].isWhitespace()) {
                return cursor
            }
        }
        cursor++
    }
    return -1
}

private fun findClosingSingleUnderscore(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    var doubleUnderOpen = false
    while (cursor < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == '$') {
            val endMath = indexOfCharBounded(text, '$', cursor + 1, rangeEnd)
            if (endMath != -1 && endMath > cursor) {
                cursor = endMath + 1
                continue
            }
        }
        if (cursor + 1 < rangeEnd && c == '_' && text[cursor + 1] == '_') {
            doubleUnderOpen = !doubleUnderOpen
            cursor += 2
            continue
        }
        if (c == '_' && !doubleUnderOpen) {
            val notFollowedByAlnum = cursor + 1 >= rangeEnd || !text[cursor + 1].isLetterOrDigit()
            if (cursor > startIdx && !text[cursor - 1].isWhitespace() && notFollowedByAlnum) {
                return cursor
            }
        }
        cursor++
    }
    return -1
}

private fun findClosingDoubleDelimiter(text: String, startIdx: Int, rangeEnd: Int, delim: Char): Int {
    var cursor = startIdx
    while (cursor + 1 < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == delim && text[cursor + 1] == delim) {
            return cursor
        }
        cursor++
    }
    return -1
}

private fun findClosingLinkBracket(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    var depth = 0
    while (cursor < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == '[') {
            depth++
        } else if (c == ']') {
            if (depth == 0) return cursor
            depth--
        }
        cursor++
    }
    return -1
}

private fun findClosingInlineDollar(text: String, startIdx: Int, rangeEnd: Int): Int {
    var cursor = startIdx
    while (cursor < rangeEnd) {
        val c = text[cursor]
        if (c == '\n') return -1
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == '$') {
            if (cursor > startIdx && !text[cursor - 1].isWhitespace()) {
                // Reject if immediately followed by a digit or identifier letter (e.g. "$50 to $100" or "$env:FOO ... $bar")
                val followedByAlnum = cursor + 1 < rangeEnd && (text[cursor + 1].isDigit() || text[cursor + 1].isLetter())
                if (!followedByAlnum) {
                    var allCurrencyChars = true
                    var hasSpace = false
                    var hasMathOp = false
                    for (k in startIdx until cursor) {
                        val ic = text[k]
                        if (allCurrencyChars && !ic.isDigit() && ic != '.' && ic != ',') {
                            allCurrencyChars = false
                        }
                        if (ic == ' ') hasSpace = true
                        if (ic in "\\^_=+-*/<>()") hasMathOp = true
                    }
                    val startsWithCurrencyDigitWithoutMathOp = text[startIdx].isDigit() && hasSpace && !hasMathOp
                    if (!allCurrencyChars && !startsWithCurrencyDigitWithoutMathOp) {
                        return cursor
                    }
                }
            }
            return -1
        }
        cursor++
    }
    return -1
}

private fun findClosingQuoteSkippingCode(text: String, startIdx: Int, rangeEnd: Int, closeChar: Char): Int {
    var cursor = startIdx
    while (cursor < rangeEnd) {
        val c = text[cursor]
        if (c == '\\' && cursor + 1 < rangeEnd) {
            cursor += 2
            continue
        }
        if (c == '`') {
            cursor = skipInlineCodeSpan(text, cursor, rangeEnd)
            continue
        }
        if (c == closeChar) {
            return cursor
        }
        cursor++
    }
    return -1
}

/**
 * Finds the closing `)` for a Markdown link `[label](url)`, properly balancing nested parentheses
 * inside the URL (such as Wikipedia URLs `https://en.wikipedia.org/wiki/Kotlin_(programming_language)`).
 */
internal fun findClosingMarkdownLinkParen(text: String, openParenIdx: Int, rangeEnd: Int = text.length): Int {
    var depth = 0
    var idx = openParenIdx + 1
    while (idx < rangeEnd) {
        val c = text[idx]
        if (c == '\n' || c == '\r') return -1
        if (c == '(') {
            depth++
        } else if (c == ')') {
            if (depth == 0) return idx
            depth--
        }
        idx++
    }
    return -1
}

private fun inferCodeFenceExtension(language: String, isSvg: Boolean): String {
    if (isSvg) return "svg"
    return when (language.trim().lowercase()) {
        "kotlin", "kt" -> "kt"
        "python", "py" -> "py"
        "javascript", "js" -> "js"
        "typescript", "ts" -> "ts"
        "html", "htm" -> "html"
        "css" -> "css"
        "json" -> "json"
        "markdown", "md" -> "md"
        "mermaid", "mmd" -> "mmd"
        "sh", "bash", "zsh" -> "sh"
        "powershell", "ps1" -> "ps1"
        "sql" -> "sql"
        "yaml", "yml" -> "yml"
        "xml" -> "xml"
        "rust", "rs" -> "rs"
        "go", "golang" -> "go"
        "java" -> "java"
        "c" -> "c"
        "cpp", "c++", "cxx" -> "cpp"
        else -> "txt"
    }
}

