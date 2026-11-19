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
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ModelPricingSpec
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.ThemeConfig
import io.parchment.ai.model.TokenPricing
import io.parchment.ai.model.WorkspaceMode
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.PlatformWorkspace
import io.parchment.ai.strings.Strings
import io.parchment.ai.ui.ParchmentApp
import io.parchment.ai.ui.PrivacyBadgeTone
import io.parchment.ai.ui.ProvideParchmentTheme
import io.parchment.ai.ui.SepiaSearchableModelPicker
import io.parchment.ai.ui.compactStateJson
import io.parchment.ai.ui.createDefaultAppState
import io.parchment.ai.ui.loadFastInitialAppState
import io.parchment.ai.ui.parchmentProcessState
import io.parchment.ai.ui.primaryModelPrivacyBadge
import io.parchment.ai.ui.resolveModelPrivacyBadges
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import java.io.ByteArrayOutputStream
import java.nio.file.Files
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class Batch6FeaturesTest {

    private fun buildZip(vararg entries: Pair<String, ByteArray>): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            for ((name, bytes) in entries) {
                zip.putNextEntry(ZipEntry(name))
                zip.write(bytes)
                zip.closeEntry()
            }
        }
        return out.toByteArray()
    }

    @Test
    fun docxExtractorParsesParagraphsTablesEntitiesAndBlocksZipBombsAndTraversal() {
        val docXml = """
            <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body>
                <w:p><w:r><w:t>Hello &amp; welcome &#8212; &lt;Parchment&gt;</w:t></w:r></w:p>
                <w:p>
                  <w:r><w:t>Line 1</w:t><w:br/><w:t>Line 2</w:t></w:r>
                  <w:del><w:r><w:delText>DELETED_SECRET</w:delText></w:r></w:del>
                  <w:r><w:instrText>HYPERLINK "https://ignored.example"</w:instrText></w:r>
                </w:p>
                <w:tbl>
                  <w:tr>
                    <w:tc><w:p><w:r><w:t>ColA</w:t></w:r></w:p></w:tc>
                    <w:tc><w:p><w:r><w:t>ColB</w:t></w:r></w:p></w:tc>
                  </w:tr>
                </w:tbl>
              </w:body>
            </w:document>
        """.trimIndent()
        val docxBytes = buildZip("word/document.xml" to docXml.encodeToByteArray())
        val text = PlatformWorkspace.extractDocxText(docxBytes).getOrThrow()

        assertTrue("Hello & welcome — <Parchment>" in text, "Decoded entities missing: $text")
        assertTrue("Line 1\nLine 2" in text, "Line break missing: $text")
        assertTrue("ColA\tColB" in text, "Table columns missing: $text")
        assertFalse("DELETED_SECRET" in text, "Tracked deletion text must be excluded: $text")
        assertFalse("HYPERLINK" in text, "Field instrText must be excluded: $text")

        // Invalid non-zip payload rejected
        assertTrue(PlatformWorkspace.extractDocxText("not a zip".encodeToByteArray()).isFailure)

        // Zip missing word/document.xml rejected
        val missingDoc = buildZip("readme.txt" to "hi".encodeToByteArray())
        assertTrue("word/document.xml" in PlatformWorkspace.extractDocxText(missingDoc).exceptionOrNull()?.message.orEmpty())

        // Zip traversal rejected
        val badZip = buildZip("../evil.xml" to "x".encodeToByteArray(), "word/document.xml" to docXml.encodeToByteArray())
        assertTrue("unsafe" in PlatformWorkspace.extractDocxText(badZip).exceptionOrNull()?.message.orEmpty())

        // Zip bomb (> 16 MiB uncompressed word/document.xml in tiny zip) rejected
        val hugeXml = "<w:document><w:body><w:p><w:r><w:t>" + "A".repeat(17 * 1024 * 1024) + "</w:t></w:r></w:p></w:body></w:document>"
        val bombDocx = buildZip("word/document.xml" to hugeXml.encodeToByteArray())
        assertTrue(bombDocx.size < 200 * 1024, "Expected highly compressed test docx")
        assertTrue("16 MB" in PlatformWorkspace.extractDocxText(bombDocx).exceptionOrNull()?.message.orEmpty())
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun privacyRetentionBadgesReflectLocalityStorageFlagsAndCatalogPricingTruthfully() = runBlocking<Unit>(Dispatchers.Swing) {
        // 1. Local endpoint -> Local (POSITIVE)
        val localProf = ProviderProfile(
            id = "p-local", name = "Ollama", protocol = ProviderProtocol.CUSTOM,
            baseUrl = "http://127.0.0.1:11434/v1", defaultModel = "qwen3:32b", savedModels = listOf("qwen3:32b")
        )
        val localBadges = resolveModelPrivacyBadges(localProf, "qwen3:32b")
        assertEquals(listOf(Strings.privacyBadgeLocal), localBadges.map { it.label })
        assertEquals(PrivacyBadgeTone.POSITIVE, localBadges.single().tone)

        // 2. Stateless vs Stored OpenAI Responses + Paid vs Free vs Unknown pricing
        val statelessPaid = ProviderProfile(
            id = "p-oai", name = "OpenAI", protocol = ProviderProtocol.OPENAI_RESPONSES,
            storeServerSide = false, defaultModel = "gpt-5", savedModels = listOf("gpt-5", "model-unknown"),
            customModelSpecs = mapOf(
                "gpt-5" to ModelPricingSpec("gpt-5", knownPricing = TokenPricing(inputPerM = 2.0, outputPerM = 8.0))
            )
        )
        val paidBadges = resolveModelPrivacyBadges(statelessPaid, "gpt-5")
        assertEquals(listOf(Strings.privacyBadgeNoStore, Strings.privacyBadgePaidApi), paidBadges.map { it.label })

        // Unknown pricing must NEVER falsely show Free Tier
        val unknownBadges = resolveModelPrivacyBadges(statelessPaid, "model-unknown")
        assertEquals(listOf(Strings.privacyBadgeNoStore), unknownBadges.map { it.label })

        val storedFree = ProviderProfile(
            id = "p-free", name = "Gateway", protocol = ProviderProtocol.GEMINI_INTERACTIONS,
            storeServerSide = true, defaultModel = "open/demo:free", savedModels = listOf("open/demo:free"),
            customModelSpecs = mapOf(
                "open/demo:free" to ModelPricingSpec("open/demo:free", knownPricing = TokenPricing(inputPerM = 0.0, outputPerM = 0.0))
            )
        )
        val storedFreeBadges = resolveModelPrivacyBadges(storedFree, "open/demo:free")
        assertEquals(listOf(Strings.privacyBadgeStored, Strings.privacyBadgeFreeTier), storedFreeBadges.map { it.label })
        assertEquals(PrivacyBadgeTone.CAUTION, primaryModelPrivacyBadge(storedFree, "open/demo:free")?.tone)

        // 3. Model picker renders privacy badges cleanly
        val scene = ImageComposeScene(width = 760, height = 560, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            scene.setContent {
                ProvideParchmentTheme(ThemeConfig()) {
                    SepiaSearchableModelPicker(
                        profiles = listOf(localProf, statelessPaid, storedFree),
                        selectedProfileId = localProf.id,
                        selectedModelId = "qwen3:32b",
                        onSelectModel = { _, _ -> }
                    )
                }
            }
            repeat(4) { Snapshot.sendApplyNotifications(); scene.render(it * 16_000_000L).close() }
            fun flatten(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flatten)
            val texts = scene.semanticsOwners.flatMap { flatten(it.unmergedRootSemanticsNode) }
                .flatMap { it.config.getOrNull(SemanticsProperties.Text).orEmpty().map { s -> s.text } }
            assertTrue(Strings.privacyBadgeLocal in texts, "Picker must render Local badge: $texts")
            assertTrue(Strings.privacyBadgeNoStore in texts, "Picker must render No Store badge: $texts")
            assertTrue(Strings.privacyBadgeStored in texts, "Picker must render Stored badge: $texts")
        } finally {
            scene.close()
        }
    }

    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun workspaceCommandsDiscoverExpandAndExplicitSaveAsSkillPersistsManifest() = runBlocking<Unit>(Dispatchers.Swing) {
        val wsDir = Files.createTempDirectory("parchment-batch6-commands-skill")
        try {
            val wsPath = wsDir.toString()
            val cmdsDir = wsDir.resolve(".parchment/commands")
            Files.createDirectories(cmdsDir)
            Files.writeString(
                cmdsDir.resolve("review.md"),
                "---\ndescription: Focused security review\n---\nAudit \$ARGUMENTS for auth flaws."
            )
            Files.writeString(
                cmdsDir.resolve("standup.md"),
                "# Daily Standup\n\nSummarize recent workspace changes."
            )
            // Colliding with built-in /plan must be ignored cleanly.
            Files.writeString(cmdsDir.resolve("plan.md"), "Shadow attempt")

            WorkspaceHarness.invalidateWorkspaceCache(wsPath)
            val cmds = WorkspaceHarness.discoverWorkspaceCommands(wsPath)
            assertEquals(listOf("/review", "/standup"), cmds.map { it.command })
            assertTrue(cmds.first { it.command == "/review" }.takesArgument)
            assertFalse(cmds.first { it.command == "/standup" }.takesArgument)

            assertEquals(
                "Audit src/Auth.kt for auth flaws.",
                WorkspaceHarness.expandWorkspaceCommand(wsPath, "/review src/Auth.kt")
            )
            assertEquals(
                "# Daily Standup\n\nSummarize recent workspace changes.\n\nextra detail",
                WorkspaceHarness.expandWorkspaceCommand(wsPath, "/standup extra detail")
            )

            val (slug, skillMd) = WorkspaceHarness.buildSkillMarkdown("Release Checklist", "# Release Flow\n\nVerify tests first.")
            assertEquals("release-flow", slug)
            val parsed = WorkspaceHarness.parseSkillHeader(skillMd)
            assertNotNull(parsed)
            assertEquals("release-flow", parsed.first)
            assertTrue(parsed.second.isNotBlank())

            // End-to-end UI check: composer "/" popup shows workspace /review command AND Save as skill writes SKILL.md!
            val defaults = createDefaultAppState().copy(autoCheckOtaOnStartup = false)
            val replyNode = MessageNode(
                id = "a-skill-node",
                role = MessageRole.ASSISTANT,
                variants = listOf(MessageVariant(id = "v-skill", content = "# Deploy Checklist\n\nRun verify.py final before tagging."))
            )
            val seeded = defaults.sessions.first().copy(
                title = "Deploy Flow",
                messages = listOf(replyNode),
                workspacePath = wsPath,
                workspaceMode = WorkspaceMode.READ_WRITE,
                permissionOverrides = AgentPermissionOverrides(workspacePath = wsPath, workspaceMode = WorkspaceMode.READ_WRITE)
            )
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(defaults.copy(sessions = listOf(seeded))))
            parchmentProcessState.snapshot.value = loadFastInitialAppState().first

            val scene = ImageComposeScene(width = 1280, height = 840, density = Density(1f), coroutineContext = Dispatchers.Swing)
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

                // 1. Composer slash popup discovers .parchment/commands/review.md
                val composer = allNodes().last { it.config.contains(SemanticsActions.SetText) }
                composer.config.getOrNull(SemanticsActions.SetText)!!.action!!.invoke(AnnotatedString("/rev"))
                repeat(10) {
                    settle(2)
                    if (allNodes().any { n -> n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { "/review" in it.text } }) return@repeat
                    delay(20L)
                }
                assertTrue(
                    allNodes().any { n -> n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { "/review" in it.text } },
                    "Composer slash popup must display .parchment/commands/review.md"
                )

                // 2. Click More actions -> Save as skill -> verifies .parchment/skills/deploy-checklist/SKILL.md created!
                val moreBtn = allNodes().first { n ->
                    n.config.contains(SemanticsActions.OnClick) &&
                        n.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().any { it == Strings.moreActions }
                }
                moreBtn.config.getOrNull(SemanticsActions.OnClick)!!.action!!.invoke()
                settle(8)

                val saveSkillBtn = allNodes().first { n ->
                    n.config.contains(SemanticsActions.OnClick) &&
                        n.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.saveAsSkill }
                }
                saveSkillBtn.config.getOrNull(SemanticsActions.OnClick)!!.action!!.invoke()
                val expectedSkillFile = wsDir.resolve(".parchment/skills/deploy-checklist/SKILL.md")
                repeat(20) {
                    settle(2)
                    if (Files.exists(expectedSkillFile)) return@repeat
                    delay(25L)
                }
                assertTrue(Files.exists(expectedSkillFile), "Save as skill must create .parchment/skills/deploy-checklist/SKILL.md")
                val discoveredSkills = WorkspaceHarness.discoverSkills(wsPath)
                assertTrue(discoveredSkills.any { it.name == "deploy-checklist" }, "Saved skill must be immediately discoverable: $discoveredSkills")
            } finally {
                scene.close()
                PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            }
        } finally {
            wsDir.toFile().deleteRecursively()
        }
    }
}
