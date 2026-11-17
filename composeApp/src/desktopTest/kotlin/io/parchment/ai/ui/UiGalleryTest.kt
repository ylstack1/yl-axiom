package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import io.parchment.ai.model.*
import io.parchment.ai.network.RetryControl
import io.parchment.ai.network.RetryWait
import io.parchment.ai.network.RunErrorKind
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlinx.serialization.encodeToString
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

/** Renders the real app with sample chats, so design changes can be reviewed at phone and desktop sizes. */
@OptIn(ExperimentalComposeUiApi::class, ExperimentalLayoutApi::class)
class UiGalleryTest {
    private val output = File(
        generateSequence(File(System.getProperty("user.dir"))) { it.parentFile }.first { File(it, "TODO.md").isFile },
        ".parchment-local/verification/gallery"
    ).apply { mkdirs() }

    private data class Viewport(val name: String, val widthDp: Int, val heightDp: Int, val density: Float, val fontScale: Float = 1f)
    private val phone = Viewport("phone", 390, 844, 2f)
    private val desktop = Viewport("desktop", 1360, 860, 1f)
    // 200% text, the largest size Android and Windows accessibility settings offer.
    private val phoneLargeText = phone.copy(name = "phone-text200", fontScale = 2f)
    private val desktopLargeText = desktop.copy(name = "desktop-text200", fontScale = 2f)

    private fun message(id: String, role: MessageRole, variant: MessageVariant) = MessageNode(id, role, listOf(variant))

    private fun sampleMessages(protocol: ProviderProtocol) = listOf(
        message("u1", MessageRole.USER, MessageVariant("vu1", "Why does the export fail on chats with long titles? Please check the writer and fix it.")),
        message("a1", MessageRole.ASSISTANT, MessageVariant(
            id = "va1",
            content = "The export built each file name directly from the chat title, so a long title went past the 255 character limit and the write failed.\n\n" +
                "I changed it to shorten the name and add a short hash, so two similar titles stay distinct:\n\n" +
                "```kotlin\nfun exportName(title: String, id: String): String =\n    title.take(80).trim() + \"-\" + id.takeLast(6) + \".json\"\n```\n\n" +
                "- Titles over 80 characters are shortened\n- Existing exports keep their names\n\nThe export test now covers a 400 character title.",
            thinkingSummary = "The failure only happens on long titles, so the path length is the likely cause. I should read the writer first.",
            thinkingDurationMs = 4200L,
            toolCalls = listOf(
                ToolCallRecord("t1", "fs_read", """{"path":"src/ChatArchiveWriter.kt"}""", "fun write(title: String) { ... }", durationMs = 120L, startsToolStep = true),
                ToolCallRecord("t2", "fs_edit", """{"path":"src/ChatArchiveWriter.kt"}""", "Edited 1 file", durationMs = 80L, startsToolStep = true)
            ),
            providerProtocol = protocol,
            modelUsed = "gemini-3-pro",
            createdAt = PlatformRuntime.currentTimeMillis() - 300_000L
        )),
        message("u2", MessageRole.USER, MessageVariant("vu2", "Great. Does that also cover the Android export?"))
    )

    private fun seed(theme: ThemeConfig) {
        val base = createDefaultAppState()
        val profile = base.profiles.first()
        val assistant = base.assistants.first()
        fun chat(id: String, title: String, updated: Long, messages: List<MessageNode>) = ChatSession(
            id = id, title = title, createdAt = updated, updatedAt = updated, profileId = profile.id,
            assistantId = assistant.id, messages = messages
        )
        val now = PlatformRuntime.currentTimeMillis()
        val chats = listOf(
            chat("gallery-1", "Fix chat export on long titles", now,
                (0 until conversationCopies).flatMap { n -> sampleMessages(profile.protocol).map { it.copy(id = "${it.id}-$n") } }),
            chat("gallery-2", "Trip notes for Kyoto", now - 3_600_000L, sampleMessages(profile.protocol).take(1)),
            chat("gallery-3", "The lighthouse keeper, chapter four", now - 90_000_000L, sampleMessages(profile.protocol).take(1)),
            chat("gallery-4", "Compare two storage designs", now - 400_000_000L, sampleMessages(profile.protocol).take(1))
        )
        chats.forEach { PlatformStorage.saveChatSessionJson(it.id, compactStateJson.encodeToString(it)) }
        val state = base.copy(
            sessions = emptyList(), activeSessionId = "gallery-1", openTabSessionIds = listOf("gallery-1", "gallery-2"),
            profiles = base.profiles.map { it.copy(apiKey = "sample", defaultModel = "gemini-3-pro") }, theme = theme,
            autoCheckOtaOnStartup = false
        )
        PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(state))
    }

    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun label(node: SemanticsNode) = (
        node.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty() +
            node.config.getOrNull(SemanticsProperties.Text).orEmpty().map { it.text }
        ).joinToString(" ")

    private var conversationCopies = 1
    private var frame = 0L
    private suspend fun settle(scene: ImageComposeScene, frames: Int = 40) = repeat(frames) {
        Snapshot.sendApplyNotifications()
        frame += 16_000_000L
        scene.render(frame).close()
        delay(12)
    }

    /** The active chat waiting to try again after a busy server, as a run shows it. */
    private fun showRetryWait(process: ParchmentProcessState) {
        val chatId = process.snapshot.value.activeSessionId
        val wait = RetryWait(2, 10, 12, "Chat Completions HTTP 503: The server is overloaded. Please try again later.")
        process.runs.attach(
            AgentRunIdentity("gallery-retry", chatId, "", "", "", "", ""),
            SessionStreamRuntime(chatId, statusText = Strings.tryingAgainIn(12, 2, 10) + "\n" + wait.reason,
                retryWait = wait, retryControl = RetryControl()),
            cancelPrevious = false
        )
    }

    /** A run that failed on its API key: the notice leads to the provider's settings. */
    private fun showKeyError(process: ParchmentProcessState) {
        process.runError.value = RunErrorNotice(process.snapshot.value.activeSessionId, RunErrorKind.API_KEY)
        process.notice.value = Strings.runErrorNotice(RunErrorKind.API_KEY)
    }

    private suspend fun capture(
        viewport: Viewport,
        theme: ThemeConfig,
        name: String,
        steps: List<String> = emptyList(),
        setup: (ParchmentProcessState) -> Unit = {}
    ) {
        seed(theme)
        val scene = ImageComposeScene(
            (viewport.widthDp * viewport.density).toInt(), (viewport.heightDp * viewport.density).toInt(),
            density = Density(viewport.density, viewport.fontScale), coroutineContext = Dispatchers.Swing
        )
        val process = parchmentProcessState
        try {
            process.snapshot.value = loadFastInitialAppState().first.copy(theme = theme)
            setup(process)
            scene.setContent { ParchmentApp() }
            settle(scene, 130)
            for (step in steps) {
                val clickable = nodes(scene).filter { it.config.contains(SemanticsActions.OnClick) }
                val target = clickable.firstOrNull { label(it).equals(step, ignoreCase = true) }
                    ?: clickable.firstOrNull { label(it).contains(step, ignoreCase = true) }
                if (target != null) target.config[SemanticsActions.OnClick].action?.invoke()
                else File(output, "${viewport.name}-$name-missing.txt").writeText(step)
                settle(scene)
            }
            File(output, "${viewport.name}-$name.txt").writeText(
                nodes(scene).filter { it.config.contains(SemanticsActions.OnClick) }.joinToString("\n", transform = ::label)
            )
            frame += 16_000_000L
            scene.render(frame).use { File(output, "${viewport.name}-$name.png").writeBytes(requireNotNull(it.encodeToData()).bytes) }
        } finally {
            scene.close()
            // A state set up for one picture must not leak into the next.
            process.runs.streams.value = emptyMap()
            process.runError.value = null
            process.notice.value = ""
        }
    }

    @Test fun yourMessagesSitRightAndRepliesLeft() = runBlocking<Unit>(Dispatchers.Swing) {
        seed(ThemeConfig())
        val scene = ImageComposeScene(desktop.widthDp, desktop.heightDp, density = Density(1f), coroutineContext = Dispatchers.Swing)
        try {
            parchmentProcessState.snapshot.value = loadFastInitialAppState().first
            scene.setContent { ParchmentApp() }
            settle(scene, 130)
            val prompt = nodes(scene).first { "Does that also cover" in label(it) }.boundsInRoot
            val reply = nodes(scene).first { "The export built each file" in label(it) }.boundsInRoot
            assertTrue(prompt.left > reply.left + 100f, "A short message of yours must sit on the right: $prompt against $reply")
            assertTrue(prompt.right >= reply.right, "Your message must reach the right side: $prompt against $reply")
        } finally {
            scene.close()
            (1..4).forEach { PlatformStorage.deleteChatSessionFile("gallery-$it") }
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }

    /** Rendering every screen to PNG is most of the test run's CPU, so it runs only when asked for. */
    private fun assumeGalleryRequested() =
        org.junit.Assume.assumeTrue("Render with: python scripts/verify.py test UiGalleryTest", System.getProperty("parchment.gallery") == "true")

    @Test fun renderIconSheet() {
        assumeGalleryRequested()
        runBlocking<Unit>(Dispatchers.Swing) {
            val icons = KissIcon.entries
            val columns = 12
            val rows = (icons.size + columns - 1) / columns
            val scene = ImageComposeScene(columns * 92, rows * 72, density = Density(2f), coroutineContext = Dispatchers.Swing.immediate)
            try {
                scene.setContent {
                    val palette = buildPaletteFromConfig(ThemeConfig())
                    FlowRow(Modifier.fillMaxSize().background(palette.canvasBg).padding(10.dp), maxItemsInEachRow = columns) {
                        icons.forEach { icon ->
                            Row(Modifier.width(36.dp).height(34.dp), verticalAlignment = Alignment.CenterVertically) {
                                KissVectorIcon(icon, palette.primaryInk, size = 22.dp)
                                Spacer(Modifier.width(2.dp))
                                KissVectorIcon(icon, palette.narrationInk, size = 12.dp)
                            }
                        }
                    }
                }
                settle(scene, 4)
                frame += 16_000_000L
                scene.render(frame).use { File(output, "icons.png").writeBytes(requireNotNull(it.encodeToData()).bytes) }
            } finally { scene.close() }
        }
    }

    @Test fun wheelScrollsTheChatFromThePageMargin() = runBlocking<Unit>(Dispatchers.Swing) {
        conversationCopies = 8
        seed(ThemeConfig())
        val scene = ImageComposeScene(desktop.widthDp, desktop.heightDp, density = Density(1f), coroutineContext = Dispatchers.Swing)
        suspend fun image(): List<Byte> {
            settle(scene)
            frame += 16_000_000L
            return scene.render(frame).use { requireNotNull(it.encodeToData()).bytes.toList() }
        }
        try {
            parchmentProcessState.snapshot.value = loadFastInitialAppState().first
            scene.setContent { ParchmentApp() }
            settle(scene, 130)
            val before = image()
            assertEquals(before, image(), "The page must be still before scrolling")
            val margin = Offset(330f, 400f) // beside the sidebar, left of the reading column
            scene.sendPointerEvent(PointerEventType.Scroll, margin, scrollDelta = Offset(0f, -12f), type = PointerType.Mouse)
            assertNotEquals(before, image(), "The wheel must scroll the chat from the margin")
        } finally {
            scene.close()
            (1..4).forEach { PlatformStorage.deleteChatSessionFile("gallery-$it") }
            PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
        }
    }

    @Test fun renderGallery() {
        assumeGalleryRequested()
        runBlocking<Unit>(Dispatchers.Swing) {
            val night = ThemeConfig.PRESETS.first { it.name == "Lapis at Night" }
            try {
                for (viewport in listOf(phone, desktop)) {
                    val closeDrawer = if (viewport == phone) listOf("Close sidebar") else emptyList()
                    capture(viewport, ThemeConfig(), "chat", closeDrawer)
                    capture(viewport, night, "chat-dark", closeDrawer)
                    capture(viewport, ThemeConfig(), "sidebar")
                    capture(viewport, ThemeConfig(), "settings", listOf("Settings"))
                    capture(viewport, ThemeConfig(), "thought", closeDrawer + listOf("Thought"))
                    capture(viewport, ThemeConfig(), "models", closeDrawer + listOf("gemini-3-pro"))
                    capture(viewport, ThemeConfig(), "notes", closeDrawer + listOf("Chat notes"))
                    capture(viewport, night, "sidebar-dark")
                    capture(viewport, night, "settings-dark", listOf("Settings"))
                    capture(viewport, ThemeConfig(), "retry", closeDrawer, setup = ::showRetryWait)
                    capture(viewport, night, "retry-dark", closeDrawer, setup = ::showRetryWait)
                    capture(viewport, ThemeConfig(), "key-error", closeDrawer, setup = ::showKeyError)
                }
                for (viewport in listOf(phoneLargeText, desktopLargeText)) {
                    val closeDrawer = if (viewport.name.startsWith("phone")) listOf("Close sidebar") else emptyList()
                    capture(viewport, ThemeConfig(), "chat", closeDrawer)
                    capture(viewport, ThemeConfig(), "settings", listOf("Settings"))
                    capture(viewport, ThemeConfig(), "retry", closeDrawer, setup = ::showRetryWait)
                }
                for (page in listOf("Providers", "Assistant", "Workspace", "World Book & Cards", "Storage & Updates")) {
                    capture(desktop, ThemeConfig(), "page-${page.take(5).trim().lowercase()}", listOf("Settings", page))
                }
                capture(phone, ThemeConfig(), "page-provi", listOf("Settings", "Providers"))
                capture(phone, ThemeConfig(), "page-provi-detail", listOf("Settings", "Providers", "Google Gemini"))
                for (viewport in listOf(phone, desktop)) {
                    // On a phone, New chat already closes the sidebar.
                    capture(viewport, ThemeConfig(), "empty", listOf("New chat"))
                }
            } finally {
                (1..4).forEach { PlatformStorage.deleteChatSessionFile("gallery-$it") }
                PlatformStorage.saveAppStateJson(compactStateJson.encodeToString(createDefaultAppState()))
            }
            assertTrue(File(output, "phone-chat.png").isFile)
        }
    }
}
