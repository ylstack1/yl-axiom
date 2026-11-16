package io.parchment.ai.ui

import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.ThemeConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlin.random.Random
import kotlin.test.*

/** Model text is untrusted: no input may overflow the stack or stall the frame that renders it. */
class RendererBoundsTest {
    private val palette = buildPaletteFromConfig(ThemeConfig())

    private fun bounded(name: String, block: () -> Unit) {
        val started = System.nanoTime()
        block()
        val millis = (System.nanoTime() - started) / 1_000_000
        assertTrue(millis < 4_000, "$name took ${millis}ms")
    }

    private fun renderAll(text: String) {
        parseProseBlocks(text)
        styleLiteraryParagraph(text, palette, 0)
        LatexMathRenderer.formatInlineLatexToAnnotatedString(text, palette)
        LatexMathRenderer.parseDisplayMathNodes(text, palette)
        parseSvgSceneDocument(text, Color.Black)
    }

    @Test fun deeplyNestedMathRendersAsTextInsteadOfOverflowing() {
        val depth = 20_000
        listOf(
            "\\boxed{".repeat(depth) + "x" + "}".repeat(depth),
            "\\boxed{".repeat(depth),
            "\\frac{".repeat(depth),
            "x^{".repeat(depth),
            "\\begin{a}".repeat(depth)
        ).forEach { latex -> bounded(latex.take(10)) { renderAll(latex) } }
        val nested = "\\boxed{".repeat(100) + "x" + "}".repeat(100)
        assertTrue("x" in LatexMathRenderer.formatInlineLatexToAnnotatedString(nested, palette).text)
    }

    @Test fun ordinaryMathStillFormats() {
        val text = LatexMathRenderer.formatInlineLatexToAnnotatedString("\\frac{a}{b} + \\sqrt{x}", palette).text
        assertTrue("⁄" in text && "√" in text, text)
    }

    @Test fun pathologicalInlineMarkdownStaysBounded() {
        listOf("*", "**", "_", "~~", "`", "$", "$$", "\"", "[", "](", "\\(").forEach { unit ->
            bounded("repeat $unit") { styleLiteraryParagraph(unit.repeat(200_000), palette, 0) }
            bounded("spaced $unit") { styleLiteraryParagraph("$unit ".repeat(5_000), palette, 0) }
        }
        val deep = (1..4_000).joinToString("") { if (it % 2 == 0) "**a " else "~~b " }
        val closed = deep + deep.reversed()
        bounded("nested emphasis") { assertTrue(styleLiteraryParagraph(closed, palette, 0).text.isNotEmpty()) }
        assertEquals("plain", styleLiteraryParagraph("**plain**", palette, 0).text)
    }

    @Test fun pathologicalBlocksStayBounded() {
        listOf("```\n", "$$\n", "\\[\n", "| a |\n", "> \n", "\\begin{align}\n", "# x\n", "\n\n").forEach { unit ->
            bounded("block $unit") { parseProseBlocks(unit.repeat(40_000)) }
        }
        bounded("one long line") { parseProseBlocks("a|".repeat(500_000)) }
    }

    @Test fun malformedSvgStaysBoundedAndLimitsItsWork() {
        listOf(
            "<svg>" + "<a".repeat(100_000),
            "<svg>" + "<!--".repeat(100_000),
            "<svg>" + "<style>".repeat(60_000),
            "<svg><style>" + "a".repeat(400_000) + "</style>",
            "<svg><rect " + "a".repeat(400_000) + "/>",
            "<svg><g transform=\"" + "a".repeat(400_000) + "\"/>",
            "<" + "a".repeat(400_000)
        ).forEach { svg -> bounded(svg.take(12)) { parseSvgSceneDocument(svg, Color.Black) } }
        val tooMany = "<svg viewBox=\"0 0 10 10\">" + "<rect width=\"1\" height=\"1\"/>".repeat(15_000) + "</svg>"
        bounded("many shapes") { assertNull(parseSvgSceneDocument(tooMany, Color.Black)) }
        val tooLarge = "<svg>" + " ".repeat(600_000) + "</svg>"
        assertNull(parseSvgSceneDocument(tooLarge, Color.Black))
        val nestedGroups = "<svg viewBox=\"0 0 10 10\">" + "<g>".repeat(15_000) + "<rect width=\"1\" height=\"1\"/>" + "</g>".repeat(15_000) + "</svg>"
        bounded("nested groups") { parseSvgSceneDocument(nestedGroups, Color.Black) }
    }

    @Test fun svgStylesCommentsAndAttributesStillParse() {
        val svg = """<svg viewBox='0 0 10 10'><!-- <rect width="9" height="9"/> --><style>.a, circle { fill: red } rect { opacity: 0.5 }</style>
            <g transform="translate(1 1) scale(2)"><rect class="a" width="1" height="1"/><circle r="1"/></g></svg>"""
        val scene = assertNotNull(parseSvgSceneDocument(svg, Color.Black))
        assertEquals(2, scene.drawItems.size, "The commented rectangle must not draw")
        assertEquals(0.5f, scene.drawItems[0].style.opacity)
        assertEquals(SvgPaintSpec.Solid(Color.Red), scene.drawItems[1].style.fill)
    }

    @Test fun randomDelimiterSoupNeverThrowsOrStalls() {
        val parts = listOf("*", "**", "_", "`", "```\n", "$", "$$", "\\[", "\\]", "{", "}", "\\frac", "\\boxed", "^", "<svg", "<g>", "</g>",
            "<!--", "-->", "\"", "'", "|", "\n", "\n\n", "[", "](", ")", "\\begin{a}", "\\end{a}", "é", "😀", "\u0000", " ", "x")
        val random = Random(7)
        repeat(300) {
            val text = buildString { repeat(random.nextInt(1, 2_000)) { append(parts[random.nextInt(parts.size)]) } }
            bounded("seeded soup $it") { renderAll(text) }
        }
    }

    @Test fun streamingFencesMatchAFullParseAtEveryFrame() {
        val lines = (1..200).joinToString("\n") { if (it % 5 == 0) "" else "line $it é😀" }
        val intro = "Intro paragraph.\n\n" + "x".repeat(300) + "\n\n"
        listOf(
            "$intro```kotlin\n$lines\n```\n\nAfter the fence.",
            "Tight intro:\n```\n$lines\n```\nTight outro.",
            "```py\n\n\n$lines",
            "$intro```a\n$lines\n   ```  \n\n```b\n$lines\n",
            "$intro```a\nnot ``` a close\n$lines\n```",
            "$intro```math\n$lines\n```",
            "$intro```a\r\n" + lines.replace("\n", "\r\n") + "\r\n```\r\nDone",
            "$intro$$\n$lines\n```\n$lines\n$$\nAfter"
        ).forEach { text ->
            for (step in listOf(1, 7, 64)) {
                val cache = IncrementalProseBlockCache()
                for (end in 1..text.length step step) {
                    val partial = text.substring(0, end)
                    assertEquals(parseProseBlocks(normalizeLineEndings(partial)), cache.parseIncremental(partial), "step $step at $end")
                }
                assertEquals(parseProseBlocks(normalizeLineEndings(text)), cache.parseIncremental(text))
            }
        }
        val rewrites = IncrementalProseBlockCache()
        val open = "$intro```a\n$lines"
        rewrites.parseIncremental(open)
        rewrites.parseIncremental(open + "\nmore")
        val rewritten = "Different text\n\n```b\ncode"
        assertEquals(parseProseBlocks(rewritten), rewrites.parseIncremental(rewritten))
        assertEquals(parseProseBlocks(rewritten, enableMath = false), rewrites.parseIncremental(rewritten, enableMath = false))
    }

    @Test fun crlfRepliesRenderLikeLfReplies() {
        val lf = "Intro\n\n```kotlin\nval a = 1\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nDone"
        val crlf = lf.replace("\n", "\r\n")
        assertEquals(parseProseBlocks(lf), IncrementalProseBlockCache().parseIncremental(crlf))
        // Mid-stream a CR waits for its LF, so the normalized text only grows.
        assertEquals("a", normalizeLineEndings("a\r"))
        assertEquals("a\nb", normalizeLineEndings("a\r\nb"))
        assertEquals("a\nb", normalizeLineEndings("a\rb"))
    }

    @Test fun storedChatSearchMatchesVisibleTextOnly() {
        val chat = io.parchment.ai.model.ChatSession("chat", "Title", 1L, 1L, "p", messages = listOf(
            io.parchment.ai.model.MessageNode("m", io.parchment.ai.model.MessageRole.USER, listOf(
                io.parchment.ai.model.MessageVariant("old", content = "Discarded draft"),
                io.parchment.ai.model.MessageVariant("new", content = "He said \"héllo\"\nthen left")), activeVariantIndex = 1)))
        val raw = compactStateJson.encodeToString(io.parchment.ai.model.ChatSession.serializer(), chat)
        assertTrue(storedChatJsonContains(raw, "chat", "HÉLLO"))
        assertTrue(storedChatJsonContains(raw, "chat", "\"héllo\"\nthen"), "Quotes and newlines are escaped in storage")
        assertFalse(storedChatJsonContains(raw, "chat", "Discarded"), "A replaced version is not the visible text")
        assertFalse(storedChatJsonContains(raw, "chat", "Title"), "Titles are searched separately, without reading the disk")
        assertFalse(storedChatJsonContains(raw, "other", "hello"), "A file must belong to the chat it is read for")
        assertFalse(storedChatJsonContains("{broken héllo", "chat", "héllo"))
    }

    @Test fun onlyVeryLongRepliesRefreshLessOften() {
        assertEquals(0L, visibleTextCopyIntervalMs(31_999))
        assertTrue(visibleTextCopyIntervalMs(32_000) in 1..100)
        assertTrue(visibleTextCopyIntervalMs(128_000) in 1..100)
    }

    @Test fun theStreamingWordCountMatchesAFullCountAcrossAnySplit() {
        val random = Random(11)
        val alphabet = listOf("a", "é", "😀", " ", "\n", "\t", "\u00A0", "\u2003", "word", "  ")
        repeat(200) {
            val text = buildString { repeat(random.nextInt(0, 300)) { append(alphabet[random.nextInt(alphabet.size)]) } }
            val counter = StreamWordCounter()
            val buffer = StringBuilder()
            var at = 0
            while (at < text.length) {
                val end = (at + random.nextInt(1, 9)).coerceAtMost(text.length)
                buffer.append(text, at, end)
                assertEquals(countWordsFast(buffer), counter.count(buffer))
                at = end
            }
            buffer.clear()
            buffer.append("replaced with fewer")
            counter.reset()
            assertEquals(3, counter.count(buffer))
        }
    }

    @Test fun boundedStylePreservesNestedFormattingAndSpanIsolation() {
        val sample = styleLiteraryParagraph(
            raw = "**unclosed *italic** then *outside* and [site *name*](https://example.com/a_(b))",
            palette = palette,
            indentChars = 0
        )
        assertEquals("unclosed *italic then outside and site name", sample.text)
        assertEquals(1, sample.getLinkAnnotations(0, sample.length).size)
        assertEquals(
            "https://example.com/a_(b)",
            (sample.getLinkAnnotations(0, sample.length).single().item as? androidx.compose.ui.text.LinkAnnotation.Url)?.url
        )

        val escapedAndCode = styleLiteraryParagraph(
            raw = """\*\*lit\*\* and ``  a `b`  `` and _word_snake_case_ and **bold $50 to $100**""",
            palette = palette,
            indentChars = 1
        )
        assertEquals("  **lit** and a `b` and word_snake_case and bold \$50 to \$100", escapedAndCode.text)
        assertTrue(escapedAndCode.spanStyles.any { it.item.background == palette.accent.copy(alpha = 0.12f) })

        val dialogue = styleLiteraryParagraph(
            raw = "\"Wait **now** *please*\" and _lead_",
            palette = palette,
            indentChars = 1
        )
        assertEquals("  \"Wait now please\" and lead", dialogue.text)
        val dialogueSpans = dialogue.spanStyles.count { it.item.color == palette.dialogueInk }
        assertEquals(1, dialogueSpans)
    }

    @Test fun parseProseBlocksHandlesEdgeCasesWithoutLineSplitRegressions() {
        val markdown = listOf(
            "##### Deep Heading",
            "> quote 1\n> quote 2",
            "```py\n\n\nprint(1)\n\n```\r\n",
            "  - [ ] todo  ",
            "- ",
            "  1. subitem",
            "$$ x + y $$",
            "\\[\na + b\n\\]"
        ).joinToString("\n\n")
        val blocks = parseProseBlocks(markdown, enableMath = true)
        assertEquals(
            listOf(
                ProseBlock.Heading(4, "# Deep Heading"),
                ProseBlock.BlockQuote("quote 1\nquote 2"),
                ProseBlock.CodeFence("py", "print(1)\n"),
                ProseBlock.Paragraph("   ☐ todo"),
                ProseBlock.Paragraph("•"),
                ProseBlock.Paragraph("   1. subitem"),
                ProseBlock.MathBlock(" x + y "),
                ProseBlock.MathBlock("a + b")
            ),
            blocks
        )
    }

    @Test fun latexAndSvgOptimizationsPreserveFormattingAndGeometry() {
        val inline = LatexMathRenderer.formatInlineLatexToAnnotatedString(
            "  xyz^{ab} + \\mathbb{R} - \\sqrt[3]{k_i}  ",
            palette
        )
        assertEquals("xyzab + ℝ − 3√(ki)", inline.text)
        // Outer base style + coalesced italic "xyz" + italic "ab" + italic "k" + italic "i" (NOT 1 span per letter)
        val italicSpans = inline.spanStyles.filter { it.item.fontStyle == androidx.compose.ui.text.font.FontStyle.Italic }
        assertTrue(italicSpans.any { inline.text.substring(it.start, it.end) == "xyz" })
        assertTrue(italicSpans.any { inline.text.substring(it.start, it.end) == "ab" })
        assertTrue(
            inline.spanStyles.any {
                it.item.baselineShift == androidx.compose.ui.text.style.BaselineShift.Superscript &&
                    inline.text.substring(it.start, it.end) == "3"
            }
        )

        val displayNodes = LatexMathRenderer.parseDisplayMathNodes("\\frac{alpha + 1}{beta - 2}", palette)
        assertEquals(1, displayNodes.size)
        val frac = displayNodes.single() as LatexMathRenderer.DisplayMathNode.StackedFraction
        assertEquals("alpha + 1", frac.numerator.text)
        assertEquals("beta − 2", frac.denominator.text)

        val svg = """
            <svg viewBox="0, 0 120 80">
              <defs>
                <linearGradient id="g"><stop offset="0%" stop-color="#112233"/><stop offset="100%" stop-color="#445566"/></linearGradient>
              </defs>
              <g transform="translate(10, 5)">
                <rect x="0" y="0" width="40" height="20" fill="url(#g)" stroke="#000000" stroke-dasharray="3,2"/>
              </g>
            </svg>
        """.trimIndent()
        val scene = assertNotNull(parseSvgSceneDocument(svg, Color.Black))
        assertEquals(120f, scene.width)
        assertEquals(80f, scene.height)
        assertEquals(1, scene.drawItems.size)
        assertTrue(scene.gradients.containsKey("g"))
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test fun longCodeFenceChunksRoundTripAndStreamRenderQuickly() = runBlocking<Unit>(Dispatchers.Swing) {
        val shortCode = (1..60).joinToString("\n") { "line $it" }
        assertEquals(listOf(shortCode), splitCodeFenceChunks(shortCode))

        val edgeCode = (1..80).joinToString("\n") { if (it == 41) "" else "val v$it = $it" } + "\n"
        val chunks = splitCodeFenceChunks(edgeCode)
        assertEquals(2, chunks.size)
        assertEquals(edgeCode, chunks.joinToString("\n"))

        val longReply = "```kotlin\n" + (1..240).joinToString("\n") { "val entry$it = computeValue($it) // rendered chunk line" } + "\n```"
        val text = mutableStateOf("")
        val scene = ImageComposeScene(760, 860, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate)
        try {
            scene.setContent { LiteraryProseRenderer(text.value) }
            var time = 0L
            var pos = 0
            while (pos < longReply.length) {
                pos = minOf(longReply.length, pos + 180)
                text.value = longReply.substring(0, pos)
                Snapshot.sendApplyNotifications()
                time += 16_000_000L
                val started = System.nanoTime()
                scene.render(time).close()
                val elapsedMs = (System.nanoTime() - started) / 1_000_000
                assertTrue(elapsedMs < 1_000, "Chunked long code fence frame took ${elapsedMs}ms at $pos chars")
            }
        } finally {
            scene.close()
        }
    }
}
