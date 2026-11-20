package io.parchment.ai.harness

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.AgentPermissionOverrides
import io.parchment.ai.model.AgentPermissionPolicy
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ToolApprovalMode
import io.parchment.ai.model.WorkspaceMode
import io.parchment.ai.network.AiProviderEngine
import io.parchment.ai.network.buildCompiledSystemPrompt
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.PlatformWorkspace
import io.parchment.ai.strings.Strings
import io.parchment.ai.ui.BUILTIN_SLASH_COMMANDS
import io.parchment.ai.ui.KissIcon
import io.parchment.ai.ui.ParchmentApp
import io.parchment.ai.ui.compactStateJson
import io.parchment.ai.ui.createDefaultAppState
import io.parchment.ai.ui.loadFastInitialAppState
import io.parchment.ai.ui.parchmentProcessState
import io.parchment.ai.ui.parseKissLabelIcons
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.encodeToString
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class Batch5HarnessSafetyTest {

    @Test
    fun staleReadGuardBlocksUnreadAndExternallyModifiedEditsWhileAllowingNewFilesAndChainedEdits() {
        val root = Files.createTempDirectory("parchment-stale-read-guard")
        try {
            val rootPath = root.toString()
            val existing = root.resolve("App.kt")
            Files.writeString(existing, "val version = 1\n")
            val tracker = WorkspaceFileRevisionTracker()

            fun runTool(name: String, argsJson: String) = WorkspaceHarness.executeToolCall(
                workspaceRoot = rootPath,
                mode = WorkspaceMode.READ_WRITE,
                callId = "call-$name",
                toolName = name,
                argumentsJson = argsJson,
                fileRevisions = tracker
            )

            // 1. Editing or overwriting an existing file before fs_read is blocked.
            val blindEdit = runTool("fs_edit", """{"path":"App.kt","old_text":"val version = 1","new_text":"val version = 2"}""")
            assertTrue(blindEdit.isError, "Blind fs_edit on unread file must fail")
            assertTrue("Stale-read guard" in blindEdit.resultText, blindEdit.resultText)
            assertEquals("val version = 1\n", Files.readString(existing))

            val blindOverwrite = runTool("fs_write", """{"path":"App.kt","content":"clobbered\n"}""")
            assertTrue(blindOverwrite.isError, "Blind fs_write overwrite on unread existing file must fail")
            assertTrue("Stale-read guard" in blindOverwrite.resultText, blindOverwrite.resultText)
            assertEquals("val version = 1\n", Files.readString(existing))

            // 2. Creating a brand-new file does not require a prior fs_read.
            val newFile = runTool("fs_write", """{"path":"BrandNew.kt","content":"val created = true\n"}""")
            assertFalse(newFile.isError, "Creating a new file must succeed: ${newFile.resultText}")
            assertEquals("val created = true\n", Files.readString(root.resolve("BrandNew.kt")))

            // 3. After fs_read, sequential edits by this run succeed without redundant re-reads.
            val readOk = runTool("fs_read", """{"path":"App.kt"}""")
            assertFalse(readOk.isError, readOk.resultText)

            val firstEdit = runTool("fs_edit", """{"path":"App.kt","old_text":"val version = 1","new_text":"val version = 2"}""")
            assertFalse(firstEdit.isError, firstEdit.resultText)

            val chainedMulti = runTool(
                "fs_multi_edit",
                """{"path":"App.kt","edits":[{"old_text":"val version = 2","new_text":"val version = 3"}]}"""
            )
            assertFalse(chainedMulti.isError, chainedMulti.resultText)
            assertEquals("val version = 3\n", Files.readString(existing))

            // 4. External disk change (even identical byte length within the same millisecond) triggers stale-read guard.
            Files.writeString(existing, "val version = 9\n")
            val staleEdit = runTool("fs_edit", """{"path":"App.kt","old_text":"val version = 9","new_text":"val version = 4"}""")
            assertTrue(staleEdit.isError, "Editing externally modified file must be blocked")
            assertTrue("changed on disk" in staleEdit.resultText, staleEdit.resultText)
            assertEquals("val version = 9\n", Files.readString(existing))

            // 5. Re-reading refreshes the revision stamp and allows the edit cleanly.
            assertFalse(runTool("fs_read", """{"path":"App.kt"}""").isError)
            val refreshedEdit = runTool("fs_edit", """{"path":"App.kt","old_text":"val version = 9","new_text":"val version = 4"}""")
            assertFalse(refreshedEdit.isError, refreshedEdit.resultText)
            assertEquals("val version = 4\n", Files.readString(existing))
        } finally {
            root.toFile().deleteRecursively()
        }
    }

    @Test
    fun planModeSystemPromptAndBrokerInvalidationEnforceReadOnlyAndSlashCatalog() = runBlocking<Unit> {
        val planSpec = BUILTIN_SLASH_COMMANDS.firstOrNull { it.command == "/plan" }
        val buildSpec = BUILTIN_SLASH_COMMANDS.firstOrNull { it.command == "/build" }
        assertNotNull(planSpec, "/plan must be listed in BUILTIN_SLASH_COMMANDS")
        assertNotNull(buildSpec, "/build must be listed in BUILTIN_SLASH_COMMANDS")
        assertEquals(KissIcon.SEARCH, parseKissLabelIcons(planSpec.label).leadingIcon)
        assertEquals(KissIcon.WRENCH, parseKissLabelIcons(buildSpec.label).leadingIcon)

        val root = Files.createTempDirectory("parchment-plan-mode")
        try {
            val rootPath = root.toString()
            assertTrue(PlatformWorkspace.workspaceDirectoryExists(rootPath))
            assertFalse(PlatformWorkspace.workspaceDirectoryExists(root.resolve("missing-sub").toString()))

            val readOnlySession = ChatSession(
                id = "s-plan",
                title = "Plan",
                createdAt = 1L,
                updatedAt = 1L,
                profileId = "p1",
                workspacePath = rootPath,
                workspaceMode = WorkspaceMode.READ_ONLY,
                permissionOverrides = AgentPermissionOverrides(workspacePath = rootPath, workspaceMode = WorkspaceMode.READ_ONLY)
            )
            val prompt = AiProviderEngine.buildCompiledSystemPrompt(readOnlySession)
            assertTrue("Read-Only (/plan) mode" in prompt, "Read-Only session prompt should note /plan mode: $prompt")

            // Switching from Build (READ_WRITE) to Plan (READ_ONLY) + invalidate() clears run-wide approval grants immediately.
            var mode = WorkspaceMode.READ_WRITE
            val broker = ToolApprovalBroker(
                runId = "r-plan",
                chatId = "c-plan",
                policySource = {
                    ToolAccessScope(
                        AgentPermissionPolicy(
                            workspacePath = rootPath,
                            workspaceMode = mode,
                            approvalMode = ToolApprovalMode.ASK_FOR_CHANGES
                        )
                    )
                }
            )
            val writeArgs = """{"path":"note.txt","content":"hi"}"""
            val firstAuth = async { broker.authorize("c1", "fs_write", writeArgs) }
            val req = withTimeout(4_000L) { broker.requests.first { it.isNotEmpty() }.single() }
            assertTrue(broker.respond(req.requestId, ToolApprovalDecision.ALLOW_EXACT_ACTION_FOR_RUN))
            assertTrue(firstAuth.await() is ToolAuthorization.Allowed)

            // Switching to /plan (READ_ONLY) and invalidating denies fs_write immediately without reusing run-wide grant.
            mode = WorkspaceMode.READ_ONLY
            broker.invalidate()
            val afterPlan = withTimeout(4_000L) { broker.authorize("c2", "fs_write", writeArgs) }
            assertTrue(afterPlan is ToolAuthorization.Denied, "fs_write must be denied after switching to /plan: $afterPlan")
            broker.close()
        } finally {
            root.toFile().deleteRecursively()
        }
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun missingWorkspaceFolderShowsRecoveryBannerAndClearFolderRecoversCleanly() = runBlocking<Unit>(Dispatchers.Swing) {
        val parentTmp = Files.createTempDirectory("parchment-missing-ws-check")
        val missingDir = parentTmp.resolve("deleted-folder").toString()
        val defaults = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val seededSession = defaults.sessions.first().copy(
            workspacePath = missingDir,
            workspaceMode = WorkspaceMode.READ_WRITE,
            permissionOverrides = AgentPermissionOverrides(
                workspacePath = missingDir,
                workspaceMode = WorkspaceMode.READ_WRITE
            )
        )
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(defaults.copy(sessions = listOf(seededSession))))
        parchmentProcessState.snapshot.value = loadFastInitialAppState().first
        val scene = ImageComposeScene(width = 1280, height = 820, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp() }
            fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flatten)
            fun allNodes() = scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
            fun hasText(target: String) = allNodes().any { n ->
                n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == target }
            }

            repeat(16) {
                Snapshot.sendApplyNotifications()
                scene.render(it * 16_000_000L).close()
                if (hasText(Strings.clearFolder)) return@repeat
                delay(25L)
            }

            assertTrue(hasText(Strings.missingWorkspaceFolderTitle(missingDir)), "Recovery banner must display missing folder path")
            assertTrue(hasText(Strings.chooseFolder), "Recovery banner must offer Choose folder")
            val clearBtn = allNodes().firstOrNull { n ->
                n.config.contains(SemanticsActions.OnClick) &&
                    n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.clearFolder }
            }
            assertNotNull(clearBtn, "Recovery banner must offer clickable Clear folder")
            clearBtn.config.getOrNull(SemanticsActions.OnClick)!!.action!!.invoke()

            repeat(8) {
                Snapshot.sendApplyNotifications()
                scene.render((16 + it) * 16_000_000L).close()
            }
            val updated = parchmentProcessState.snapshot.value.sessions.first()
            assertEquals("", updated.workspacePath)
            assertEquals(WorkspaceMode.OFF, updated.workspaceMode)
            assertEquals("", updated.permissionOverrides?.workspacePath)
            assertEquals(WorkspaceMode.OFF, updated.permissionOverrides?.workspaceMode)
            assertFalse(hasText(Strings.clearFolder), "Recovery banner must disappear after clearing missing folder")
        } finally {
            scene.close()
            parentTmp.toFile().deleteRecursively()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun slashPlanAndBuildCommandsSwitchActiveSessionModeInParchmentApp() = runBlocking<Unit>(Dispatchers.Swing) {
        val wsDir = Files.createTempDirectory("parchment-plan-build-ui")
        val defaults = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
        val seededSession = defaults.sessions.first().copy(
            workspacePath = wsDir.toString(),
            workspaceMode = WorkspaceMode.READ_WRITE,
            permissionOverrides = AgentPermissionOverrides(
                workspacePath = wsDir.toString(),
                workspaceMode = WorkspaceMode.READ_WRITE
            )
        )
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(defaults.copy(sessions = listOf(seededSession))))
        parchmentProcessState.snapshot.value = loadFastInitialAppState().first
        val scene = ImageComposeScene(width = 1280, height = 820, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent { ParchmentApp() }
            fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flatten)
            fun allNodes() = scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
            fun settle(frames: Int = 8) {
                repeat(frames) {
                    Snapshot.sendApplyNotifications()
                    scene.render(it * 16_000_000L).close()
                }
            }
            settle(12)

            // Type "/pl" in the composer (last SetText field, after sidebar search) to open slash suggestions and click "/plan".
            val composer = allNodes().last { it.config.contains(SemanticsActions.SetText) }
            composer.config.getOrNull(SemanticsActions.SetText)!!.action!!.invoke(AnnotatedString("/pl"))
            settle(8)
            val planRow = allNodes().first { n ->
                n.config.contains(SemanticsActions.OnClick) &&
                    n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { "/plan" in it.text }
            }
            planRow.config.getOrNull(SemanticsActions.OnClick)!!.action!!.invoke()
            settle(10)
            assertEquals(WorkspaceMode.READ_ONLY, parchmentProcessState.snapshot.value.sessions.first().workspaceMode)

            // Open slash suggestions again with "/bu" and click "/build" to return to Agent R/W mode.
            val composer2 = allNodes().last { it.config.contains(SemanticsActions.SetText) }
            composer2.config.getOrNull(SemanticsActions.SetText)!!.action!!.invoke(AnnotatedString("/bu"))
            settle(8)
            val buildRow = allNodes().first { n ->
                n.config.contains(SemanticsActions.OnClick) &&
                    n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { "/build" in it.text }
            }
            buildRow.config.getOrNull(SemanticsActions.OnClick)!!.action!!.invoke()
            settle(10)
            assertEquals(WorkspaceMode.READ_WRITE, parchmentProcessState.snapshot.value.sessions.first().workspaceMode)
        } finally {
            scene.close()
            wsDir.toFile().deleteRecursively()
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }
}
