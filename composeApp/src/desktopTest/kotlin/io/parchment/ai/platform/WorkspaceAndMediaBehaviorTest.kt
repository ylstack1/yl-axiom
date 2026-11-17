package io.parchment.ai.platform

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.AgentPermissionPolicy
import io.parchment.ai.model.ToolApprovalMode
import io.parchment.ai.model.WorkspaceMode
import java.io.File
import java.nio.file.Files
import kotlin.test.*

/** Pins the observable behavior of workspace edits and media storage through their public API. */
class WorkspaceAndMediaBehaviorTest {
    private val root: File = Files.createTempDirectory("parchment-workspace-behavior-").toFile()
    private val workspace = root.absolutePath

    @AfterTest fun cleanUp() { root.deleteRecursively() }

    private fun file(path: String) = File(root, path)

    @Test fun writeCreatesParentsAndReportsSize() {
        val result = PlatformWorkspace.workspaceWrite(workspace, "docs/notes.txt", "one\ntwo").getOrThrow()
        assertEquals("Successfully wrote 7 characters (2 lines) to 'docs/notes.txt'.", result)
        assertEquals("one\ntwo", file("docs/notes.txt").readText())
    }

    @Test fun appendAddsToExistingContent() {
        PlatformWorkspace.workspaceWrite(workspace, "log.txt", "a").getOrThrow()
        val result = PlatformWorkspace.workspaceAppend(workspace, "log.txt", "b").getOrThrow()
        assertEquals("Successfully appended 1 characters (1 lines) to 'log.txt'.", result)
        assertEquals("ab", file("log.txt").readText())
    }

    @Test fun editKeepsWindowsLineEndings() {
        file("crlf.txt").writeText("one\r\ntwo\r\n")
        val result = PlatformWorkspace.workspaceEdit(workspace, "crlf.txt", "two", "2", false).getOrThrow()
        assertEquals("one\r\n2\r\n", file("crlf.txt").readText())
        assertTrue(result.startsWith("Edited 'crlf.txt'"), result)
    }

    @Test fun editReportsAMissingFile() {
        val failure = PlatformWorkspace.workspaceEdit(workspace, "absent.txt", "a", "b", false).exceptionOrNull()
        assertEquals("File not found: absent.txt", failure?.message)
    }

    @Test fun multiEditAppliesEveryStepOrNone() {
        file("code.txt").writeText("alpha\nbeta\n")
        val applied = PlatformWorkspace.workspaceMultiEdit(workspace, "code.txt", listOf(
            Triple("alpha", "one", false), Triple("beta", "two", false)
        )).getOrThrow()
        assertTrue(applied.startsWith("Applied 2 edit(s) atomically"), applied)
        assertEquals("one\ntwo\n", file("code.txt").readText())

        val failure = PlatformWorkspace.workspaceMultiEdit(workspace, "code.txt", listOf(
            Triple("one", "first", false), Triple("no such text", "x", false)
        )).exceptionOrNull()
        assertTrue(failure?.message.orEmpty().startsWith("Edit #2 failed (rolled back all edits)"), failure?.message)
        assertEquals("one\ntwo\n", file("code.txt").readText(), "A failed step must leave the file unchanged")
    }

    @Test fun moveCreatesParentsAndRefusesTheRoot() {
        file("a.txt").writeText("x")
        assertEquals("Moved 'a.txt' -> 'sub/b.txt'.", PlatformWorkspace.workspaceMove(workspace, "a.txt", "sub/b.txt").getOrThrow())
        assertFalse(file("a.txt").exists())
        assertEquals("x", file("sub/b.txt").readText())
        assertEquals("Cannot move or replace the workspace root.", PlatformWorkspace.workspaceMove(workspace, "sub/b.txt", ".").exceptionOrNull()?.message)
        assertTrue(PlatformWorkspace.workspaceMove(workspace, "gone.txt", "c.txt").isFailure)
        assertFalse(file("c.txt").exists())
    }

    @Test fun deleteRemovesTreesAndRefusesTheRoot() {
        file("dir/inner.txt").apply { parentFile.mkdirs(); writeText("x") }
        assertEquals("Deleted 'dir'.", PlatformWorkspace.workspaceDelete(workspace, "dir").getOrThrow())
        assertFalse(file("dir").exists())
        assertEquals("Cannot delete workspace root directory.", PlatformWorkspace.workspaceDelete(workspace, ".").exceptionOrNull()?.message)
        assertEquals("Path does not exist: gone", PlatformWorkspace.workspaceDelete(workspace, "gone").exceptionOrNull()?.message)
    }

    @Test fun workspacePathsCannotEscapeTheRoot() {
        assertTrue(PlatformWorkspace.workspaceWrite(workspace, "../escaped.txt", "x").isFailure)
        assertFalse(File(root.parentFile, "escaped.txt").exists())
    }

    @Test fun ignoredDirectoriesAreCaseInsensitive() {
        listOf(".git", "BUILD", ".gradle", "Node_Modules", ".idea", ".kotlin").forEach { assertTrue(WorkspacePathGuard.isIgnoredDirectory(it), it) }
        assertFalse(WorkspacePathGuard.isIgnoredDirectory("src"))
    }

    @Test fun savedMediaIsSanitizedAndFoundByPortablePath() {
        val saved = PlatformStorage.saveGeneratedMediaFile("my photo?.png", byteArrayOf(1, 2, 3), sessionId = "behavior-chat")
        assertTrue(saved.replace('\\', '/').endsWith("/media/my_photo_.png"), saved)
        assertContentEquals(byteArrayOf(1, 2, 3), PlatformStorage.readBinaryFileBytes(saved))
        assertEquals(3L, PlatformStorage.localFileSizeBytes(saved))
        assertContentEquals(byteArrayOf(1, 2, 3), PlatformStorage.readBinaryFileBytes("D:/other/device/chats/behavior-chat/media/my_photo_.png"))
        assertNull(PlatformStorage.readBinaryFileBytes("chats/behavior-chat/media/absent.png"))
    }

    @Test fun legacySharedMediaMovesIntoItsChatFolder() {
        val legacy = PlatformStorage.saveGeneratedMediaFile("shared.png", byteArrayOf(7), sessionId = "")
        val moved = PlatformStorage.relocateLegacyMediaToChatFolder(legacy, "behavior-move")
        assertNotEquals(legacy, moved)
        assertTrue(moved.replace('\\', '/').contains("behavior-move"), moved)
        assertFalse(File(legacy).exists(), "A file from the legacy shared folder is moved, not copied")
        assertContentEquals(byteArrayOf(7), File(moved).readBytes())
        assertEquals(moved, PlatformStorage.relocateLegacyMediaToChatFolder(moved, "behavior-move"), "Already in place")
    }

    @Test fun filesOutsideLegacyStorageAreCopiedAndLeftAlone() {
        val outside = file("outside.png").apply { writeBytes(byteArrayOf(9)) }
        val copied = PlatformStorage.relocateLegacyMediaToChatFolder(outside.absolutePath, "behavior-copy")
        assertNotEquals(outside.absolutePath, copied)
        assertTrue(outside.exists())
        assertContentEquals(byteArrayOf(9), File(copied).readBytes())
    }

    @Test fun blankOrMissingRelocationsReturnTheOriginalPath() {
        assertEquals("", PlatformStorage.relocateLegacyMediaToChatFolder("", "chat"))
        assertEquals("missing.png", PlatformStorage.relocateLegacyMediaToChatFolder("missing.png", "chat"))
        assertEquals("x.png", PlatformStorage.relocateLegacyMediaToChatFolder("x.png", ""))
    }

    @Test fun editAndMultiEditInvalidateCachedWorkspaceTreeListing() {
        file("notes.txt").writeText("short")
        val initialListing = PlatformWorkspace.workspaceList(workspace, "", recursive = true).getOrThrow()
        assertTrue(initialListing.contains("notes.txt (5 bytes)"), initialListing)

        PlatformWorkspace.workspaceEdit(workspace, "notes.txt", "short", "much longer updated content", false).getOrThrow()
        val afterEditListing = PlatformWorkspace.workspaceList(workspace, "", recursive = true).getOrThrow()
        assertTrue(afterEditListing.contains("notes.txt (27 bytes)"), afterEditListing)

        PlatformWorkspace.workspaceMultiEdit(
            workspace,
            "notes.txt",
            listOf(Triple("updated", "mutated", false), Triple("content", "payload!!", false))
        ).getOrThrow()
        val afterMultiEditListing = PlatformWorkspace.workspaceList(workspace, "", recursive = true).getOrThrow()
        assertTrue(afterMultiEditListing.contains("notes.txt (29 bytes)"), afterMultiEditListing)
    }

    @Test fun isReadableLocalFileResolvesDirectAndPortableMediaPaths() {
        val saved = PlatformStorage.saveGeneratedMediaFile("check-readable.png", byteArrayOf(4, 5, 6), sessionId = "behavior-readable")
        assertTrue(PlatformStorage.isReadableLocalFile(saved))
        assertTrue(PlatformStorage.isReadableLocalFile("D:/other/device/chats/behavior-readable/media/check-readable.png"))
        assertFalse(PlatformStorage.isReadableLocalFile("chats/behavior-readable/media/missing.png"))
        assertFalse(PlatformStorage.isReadableLocalFile("   "))
    }

    @Test fun harnessSearchScopingImportPriorityNearMissAndTokenEfficiency() {
        // 1. Scoped fs_search + non-import priority when hitting max_results
        for (idx in 0 until 6) {
            PlatformWorkspace.workspaceWrite(workspace, "src/User$idx.kt", "package a\nimport x.TargetWidget\nval v$idx = $idx\n").getOrThrow()
        }
        PlatformWorkspace.workspaceWrite(workspace, "src/z_Def.kt", "package x\nclass TargetWidget {\n    fun run() = 1\n}\n").getOrThrow()
        PlatformWorkspace.workspaceWrite(workspace, "sub/only.kt", "val scopedNeedle = \"TargetWidget in sub\"\n").getOrThrow()

        val prioritized = WorkspaceHarness.executeToolCall(
            workspaceRoot = workspace,
            mode = WorkspaceMode.READ_ONLY,
            callId = "s1",
            toolName = "fs_search",
            argumentsJson = """{"query":"TargetWidget","file_glob":".kt","max_results":2}"""
        )
        assertFalse(prioritized.isError, prioritized.resultText)
        assertTrue("class TargetWidget" in prioritized.resultText, prioritized.resultText)

        val scopedFile = WorkspaceHarness.executeToolCall(
            workspaceRoot = workspace,
            mode = WorkspaceMode.READ_ONLY,
            callId = "s2",
            toolName = "fs_search",
            argumentsJson = """{"query":"TargetWidget","path":"sub/only.kt"}"""
        )
        assertFalse(scopedFile.isError, scopedFile.resultText)
        assertEquals(1, scopedFile.resultText.lines().size)
        assertTrue(scopedFile.resultText.startsWith("sub/only.kt:1:"), scopedFile.resultText)

        // 2. Near-miss line hint on fs_edit mismatch
        val multiLine = (1..30).joinToString("\n") { "val item$it = ${it * 10}" }
        PlatformWorkspace.workspaceWrite(workspace, "src/NearMiss.kt", multiLine).getOrThrow()
        val miss = WorkspaceHarness.executeToolCall(
            workspaceRoot = workspace,
            mode = WorkspaceMode.READ_WRITE,
            callId = "e1",
            toolName = "fs_edit",
            argumentsJson = """{"path":"src/NearMiss.kt","old_text":"val item14 = 140\nval item15 = 999\nval item16 = 160","new_text":"val item15 = 150"}"""
        )
        assertTrue(miss.isError)
        assertTrue("Closest match at lines" in miss.resultText && "14: val item14 = 140" in miss.resultText, miss.resultText)

        // 3. Auto-loaded AGENTS.md and skill_load strip reader line numbers
        PlatformWorkspace.workspaceWrite(workspace, "AGENTS.md", "# Rules\nKeep edits surgical.").getOrThrow()
        PlatformWorkspace.workspaceWrite(workspace, ".parchment/skills/craft/SKILL.md", "---\nname: craft\ndescription: Rigor skill\n---\n# Craft\nVerify first.").getOrThrow()
        WorkspaceHarness.invalidateWorkspaceCache(workspace)
        val ctx = WorkspaceHarness.discoverWorkspaceContextFiles(workspace)
        assertTrue("# Rules\nKeep edits surgical." in ctx && "1: # Rules" !in ctx && "File: AGENTS.md" !in ctx, ctx)
        val skillLoaded = WorkspaceHarness.executeToolCall(
            workspaceRoot = workspace,
            mode = WorkspaceMode.READ_ONLY,
            callId = "sk1",
            toolName = "skill_load",
            argumentsJson = """{"name":"craft"}"""
        )
        assertFalse(skillLoaded.isError, skillLoaded.resultText)
        assertTrue("1: ---" !in skillLoaded.resultText && "# Craft\nVerify first." in skillLoaded.resultText, skillLoaded.resultText)

        // 4. PowerShell CLIXML error text preserved instead of wiped
        val cliXml = "#< CLIXML\r\n<Objs Version=\"1.1.0.1\" xmlns=\"http://schemas.microsoft.com/powershell/2004/04\"><S S=\"Error\">ParserError: Unexpected token '&amp;&amp;'_x000D__x000A_</S></Objs>"
        val cleaned = WorkspaceHarness.compressToolOutputWithSizeValve("shell_exec", cliXml)
        assertTrue("ParserError: Unexpected token '&&'" in cleaned, cleaned)

        // 5. Compacted fs_read receipt preserves symbol summary
        val bigKotlin = "File: Engine.kt (Lines 1..80 of 80)\n1: package demo\n2: class AlphaEngine {\n3:     suspend fun runFast() {}\n" +
            (4..80).joinToString("\n") { "$it:     val pad$it = \"padding_content_line_$it\"" }
        val receipt = WorkspaceHarness.compactStaleToolOutputText(3, 30, "fs_read", bigKotlin)
        assertTrue(receipt.startsWith("[Compacted step #3 (fs_read):") && "symbols=AlphaEngine,runFast;" in receipt, receipt)

        // 6. Cached wire tool schemas & token budget (< 8,200 chars for full 21-tool flat schema)
        val policy = AgentPermissionPolicy(
            workspacePath = workspace,
            workspaceMode = WorkspaceMode.READ_WRITE,
            webAccess = true,
            computerAccess = true,
            approvalMode = ToolApprovalMode.NEVER
        )
        val firstTools = WorkspaceHarness.buildFlatFunctionTools(
            mode = WorkspaceMode.READ_WRITE,
            workspacePath = workspace,
            allowShell = true,
            goalModeActive = true,
            enableWebTools = true,
            includeLocalWebSearchTool = true,
            policy = policy
        )
        val secondTools = WorkspaceHarness.buildFlatFunctionTools(
            mode = WorkspaceMode.READ_WRITE,
            workspacePath = workspace,
            allowShell = true,
            goalModeActive = true,
            enableWebTools = true,
            includeLocalWebSearchTool = true,
            policy = policy
        )
        assertEquals(21, firstTools.size)
        assertSame(firstTools[0], secondTools[0], "ToolDefinition wire JsonObject should be cached across requests")
        val serializedChars = firstTools.toString().length
        assertTrue(serializedChars < 8_200, "21-tool schema exceeded token budget ($serializedChars chars >= 8200)")
    }
}
