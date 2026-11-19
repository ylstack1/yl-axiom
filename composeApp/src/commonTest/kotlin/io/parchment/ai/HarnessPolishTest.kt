package io.parchment.ai

import io.parchment.ai.harness.ShellPreviewPart
import io.parchment.ai.harness.parseShellApprovalPreview
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.network.stripInvisibleCredentialChars
import io.parchment.ai.ui.groupConsecutiveTimelineSteps
import io.parchment.ai.ui.quietToolGroupSummary
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class HarnessPolishTest {
    @Test fun pastedKeysLoseInvisibleCharacters() {
        assertEquals("sk-abc123", stripInvisibleCredentialChars("\u200Bsk-abc\u200D123\uFEFF\r\n"))
        assertEquals("sk-abc123", stripInvisibleCredentialChars("\u00A0 sk-abc123 \u00A0"))
        assertEquals("sk-abc123", stripInvisibleCredentialChars("sk-abc\u2060123\u0000"))
        assertEquals("Bearer token", stripInvisibleCredentialChars(" Bearer token\t"), "inner spaces in header values stay")
    }

    private fun tool(id: String, name: String, running: Boolean = false, error: Boolean = false, thinking: String = "") = ToolCallRecord(id = id, name = name, argumentsJson = "{}", isRunning = running, isError = error, precedingThinking = thinking)

    @Test fun consecutiveFinishedLookupsFoldIntoOneRun() {
        val tools = listOf(
            tool("1", "fs_read"),
            tool("2", "fs_search"),
            tool("3", "fs_glob"),
            tool("4", "fs_edit"),
            tool("5", "fs_read"),
            tool("6", "web_fetch", running = true)
        )
        val runs = groupConsecutiveTimelineSteps(tools, showThinking = true) { "" }
        assertEquals(listOf(0..2, 3..3, 4..4, 5..5), runs)
    }

    @Test fun shownTextThinkingAndFailuresBreakAQuietRun() {
        val tools = listOf(tool("1", "fs_read"), tool("2", "fs_read"), tool("3", "fs_read", thinking = "hmm"), tool("4", "fs_read", error = true), tool("5", "fs_list"))
        assertEquals(listOf(0..0, 1..1, 2..2, 3..3, 4..4), groupConsecutiveTimelineSteps(tools, showThinking = true) { i -> if (i == 1) "Now the config." else "" })
        // With thinking cards hidden, the thought before step 3 is not on screen, so it no longer separates.
        assertEquals(listOf(0..2, 3..3, 4..4), groupConsecutiveTimelineSteps(tools, showThinking = false) { "" })
        assertEquals("Read 2 · searched 1 · listed 1", quietToolGroupSummary(listOf(tool("a", "fs_read"), tool("b", "web_fetch"), tool("c", "fs_search"), tool("d", "fs_list"))))
    }

    @Test fun compoundShellLinesAreBrokenDownWithoutSplittingQuotes() {
        val preview = parseShellApprovalPreview("cd app && npm test || echo 'a && b; c' | tee out.txt; git status")
        assertEquals(
            listOf(
                ShellPreviewPart("", "cd app"),
                ShellPreviewPart("&&", "npm test"),
                ShellPreviewPart("||", "echo 'a && b; c'"),
                ShellPreviewPart("|", "tee out.txt"),
                ShellPreviewPart(";", "git status")
            ),
            preview.parts
        )
        assertFalse(preview.writesFiles)
        assertTrue(preview.isCompound)
    }

    @Test fun shellCuesFlagRedirectsNestingAndBackgroundButNotPlainCommands() {
        val plain = parseShellApprovalPreview("git log --oneline -5 2>&1")
        assertFalse(plain.isCompound, "one command with a stream merge is not compound")
        assertTrue(parseShellApprovalPreview("echo hi > notes.txt").writesFiles)
        assertFalse(parseShellApprovalPreview("echo \"a > b\"").writesFiles, "quoted > is text")
        assertTrue(parseShellApprovalPreview("echo \"$(whoami)\"").nestedCommands, "\$( expands inside double quotes")
        assertFalse(parseShellApprovalPreview("echo '$(whoami)'").nestedCommands, "single quotes are literal")
        val bg = parseShellApprovalPreview("npm run dev &")
        assertTrue(bg.runsInBackground)
        assertEquals(listOf(ShellPreviewPart("", "npm run dev")), bg.parts)
        assertFalse(parseShellApprovalPreview("& 'C:\\Tools\\x.exe' -v").runsInBackground, "PowerShell call operator")
        assertEquals(listOf(ShellPreviewPart("", "a"), ShellPreviewPart(";", "b")), parseShellApprovalPreview(";a;;b;").parts)
        assertEquals(1, parseShellApprovalPreview("echo a`;b").parts.size, "PowerShell escape keeps ; literal")
    }
}
