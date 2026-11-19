package io.parchment.ai.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.InternalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import com.sun.net.httpserver.HttpServer
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ContextFragment
import io.parchment.ai.model.GeneratedMediaItem
import io.parchment.ai.model.McpServerConfig
import io.parchment.ai.model.MediaKind
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.model.ThemeConfig
import io.parchment.ai.model.TurnTextLayout
import io.parchment.ai.protocol.ContextWindowBreakdown
import io.parchment.ai.strings.Strings
import java.io.ByteArrayInputStream
import java.net.InetSocketAddress
import javax.imageio.ImageIO
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing

@OptIn(ExperimentalComposeUiApi::class, InternalComposeUiApi::class)
class UiBehaviorRegressionTest {
    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun text(node: SemanticsNode): String =
        node.config.getOrNull(SemanticsProperties.Text).orEmpty().joinToString(" ") { it.text }

    private fun description(node: SemanticsNode): String =
        node.config.getOrNull(SemanticsProperties.ContentDescription).orEmpty().joinToString(" ")

    private fun click(node: SemanticsNode) {
        val action = requireNotNull(node.config.getOrNull(SemanticsActions.OnClick)?.action) {
            "Expected OnClick semantics action on node text='${text(node)}' desc='${description(node)}'"
        }
        assertTrue(action.invoke())
    }

    private fun setText(node: SemanticsNode, value: String) {
        val action = requireNotNull(node.config.getOrNull(SemanticsActions.SetText)?.action) {
            "Expected SetText semantics action on node"
        }
        assertTrue(action.invoke(AnnotatedString(value)))
    }

    private fun settle(scene: ImageComposeScene, frameNanos: Long) {
        Snapshot.sendApplyNotifications()
        scene.render(frameNanos).close()
    }

    private fun withUiScene(
        width: Int = 1040,
        height: Int = 780,
        content: @androidx.compose.runtime.Composable ColumnScope.() -> Unit,
        check: suspend (ImageComposeScene) -> Unit
    ) = runBlocking<Unit>(Dispatchers.Swing) {
        val scene = ImageComposeScene(width, height, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate)
        try {
            scene.setContent {
                CompositionLocalProvider(LocalParchmentWindowSize provides ParchmentWindowSize(width.dp, height.dp)) {
                    Column(Modifier.fillMaxSize().background(LocalParchmentPalette.current.canvasBg)) {
                        content()
                    }
                }
            }
            settle(scene, 0L)
            check(scene)
        } finally {
            scene.close()
        }
    }

    @Test
    fun testConnectionButtonDisablesInFlightAndResetsOnSuccess() {
        val gate = CountDownLatch(1)
        val firstRequestStarted = CountDownLatch(1)
        val requestCount = AtomicInteger(0)
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.executor = Executors.newCachedThreadPool()
        server.createContext("/v1/models") { ex ->
            val callNum = requestCount.incrementAndGet()
            if (callNum == 1) {
                firstRequestStarted.countDown()
                gate.await(5, TimeUnit.SECONDS)
            }
            val body = """{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}""".toByteArray()
            ex.responseHeaders.add("Content-Type", "application/json")
            ex.sendResponseHeaders(200, body.size.toLong())
            ex.responseBody.use { it.write(body) }
        }
        server.start()

        try {
            val baseUrl = "http://127.0.0.1:${server.address.port}/v1"
            val initialProfile = ProviderProfile(
                id = "p-test",
                name = "LocalOpenAI",
                protocol = ProviderProtocol.OPENAI_RESPONSES,
                apiKey = "sk-test-key",
                baseUrl = baseUrl,
                defaultModel = "gpt-4o",
                savedModels = listOf("gpt-4o")
            )
            var state by mutableStateOf(
                AppStateSnapshot(
                    profiles = listOf(initialProfile),
                    activeProfileId = initialProfile.id,
                    assistants = listOf(AssistantAgent(id = "a1", name = "Writer")),
                    activeAssistantId = "a1"
                )
            )
            var session by mutableStateOf(
                ChatSession(id = "s1", title = "Chat", createdAt = 1L, updatedAt = 1L, profileId = initialProfile.id)
            )
            val notices = mutableListOf<String>()

            withUiScene(
                content = {
                    val activeProf = state.profiles.first()
                    ProvidersSubpage(
                        appState = state,
                        activeProfile = activeProf,
                        activeSession = session,
                        onSelectProfile = {},
                        onUpdateProfile = { tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                        onUpdateProfileById = { _, tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                        onAddProfile = {},
                        onDeleteProfile = {},
                        onUpdateSession = { tr -> session = tr(session) },
                        onNotify = { notices += it }
                    )
                }
            ) { scene ->
                val testLabel = parseKissLabelIcons(Strings.test).cleanText
                val testingLabel = parseKissLabelIcons(Strings.testing).cleanText
                var frameNanos = 16_000_000L

                val initialTestBtn = nodes(scene).single { text(it) == testLabel }
                assertFalse(initialTestBtn.config.contains(SemanticsProperties.Disabled))

                click(initialTestBtn)
                assertTrue(firstRequestStarted.await(3, TimeUnit.SECONDS))
                frameNanos += 16_000_000L
                settle(scene, frameNanos)

                val inFlightBtn = nodes(scene).single { text(it) == testingLabel }
                assertTrue(inFlightBtn.config.contains(SemanticsProperties.Disabled), "Button must disable while testing")

                gate.countDown()
                var finished = false
                repeat(120) {
                    frameNanos += 25_000_000L
                    settle(scene, frameNanos)
                    if (notices.isNotEmpty() && nodes(scene).any { text(it) == testLabel }) {
                        finished = true
                        return@repeat
                    }
                    delay(15)
                }
                assertTrue(finished, "Test button must return to enabled '$testLabel' after onSuccess; notices=$notices")
                val restoredBtn = nodes(scene).single { text(it) == testLabel }
                assertFalse(restoredBtn.config.contains(SemanticsProperties.Disabled))

                // Must remain usable for subsequent connection checks.
                click(restoredBtn)
                repeat(120) {
                    frameNanos += 25_000_000L
                    settle(scene, frameNanos)
                    if (notices.size >= 2 && requestCount.get() >= 2) return@repeat
                    delay(15)
                }
                assertEquals(2, requestCount.get(), "Second Test click should trigger another request")
            }
        } finally {
            gate.countDown()
            server.stop(0)
        }
    }

    @Test
    fun apiKeyFieldAllowsEditingWhileMaskedWithoutBulletCorruption() {
        val initialProfile = ProviderProfile(
            id = "p-key",
            name = "KeyProvider",
            protocol = ProviderProtocol.OPENAI_RESPONSES,
            apiKey = "sk-initial-secret",
            defaultModel = "gpt-4o",
            savedModels = listOf("gpt-4o")
        )
        var state by mutableStateOf(
            AppStateSnapshot(
                profiles = listOf(initialProfile),
                activeProfileId = initialProfile.id,
                assistants = listOf(AssistantAgent(id = "a1", name = "Writer")),
                activeAssistantId = "a1"
            )
        )
        var session by mutableStateOf(
            ChatSession(id = "s1", title = "Chat", createdAt = 1L, updatedAt = 1L, profileId = initialProfile.id)
        )

        withUiScene(
            content = {
                ProvidersSubpage(
                    appState = state,
                    activeProfile = state.profiles.first(),
                    activeSession = session,
                    onSelectProfile = {},
                    onUpdateProfile = { tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                    onUpdateProfileById = { _, tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                    onAddProfile = {},
                    onDeleteProfile = {},
                    onUpdateSession = { tr -> session = tr(session) },
                    onNotify = {}
                )
            }
        ) { scene ->
            val maskedInput = nodes(scene).single {
                it.config.contains(SemanticsActions.SetText) && it.config.contains(SemanticsProperties.Password)
            }
            assertEquals("•".repeat("sk-initial-secret".length), maskedInput.config[SemanticsProperties.EditableText].text)

            // Edit directly while masked; PasswordVisualTransformation preserves real underlying state.
            setText(maskedInput, "  sk-updated-masked-key  ")
            settle(scene, 16_000_000L)

            assertEquals("sk-updated-masked-key", state.profiles.first().apiKey)
            assertFalse(state.profiles.first().apiKey.contains('•'))
            val stillMasked = nodes(scene).single {
                it.config.contains(SemanticsActions.SetText) && it.config.contains(SemanticsProperties.Password)
            }
            assertEquals("•".repeat("sk-updated-masked-key".length), stillMasked.config[SemanticsProperties.EditableText].text)

            val showBtn = nodes(scene).single { description(it) == Strings.showApiKey }
            click(showBtn)
            settle(scene, 32_000_000L)

            val revealedInput = nodes(scene).firstOrNull {
                it.config.contains(SemanticsActions.SetText) &&
                    it.config.getOrNull(SemanticsProperties.EditableText)?.text == "sk-updated-masked-key"
            }
            assertNotNull(revealedInput, "Toggling Show API key must reveal the stored plaintext key")
            setText(revealedInput, "sk-final-visible")
            settle(scene, 48_000_000L)
            assertEquals("sk-final-visible", state.profiles.first().apiKey)
        }
    }

    @Test
    fun savedModelsListVirtualizesLargeCatalogInLazyColumn() {
        val manyModels = (0 until 220).map { idx -> "vendor/model-${idx.toString().padStart(3, '0')}" }
        val profile = ProviderProfile(
            id = "p-many",
            name = "LargeCatalog",
            protocol = ProviderProtocol.OPENAI_RESPONSES,
            apiKey = "sk-test",
            defaultModel = manyModels.first(),
            savedModels = manyModels
        )
        var state by mutableStateOf(
            AppStateSnapshot(
                profiles = listOf(profile),
                activeProfileId = profile.id,
                assistants = listOf(AssistantAgent(id = "a1", name = "Writer")),
                activeAssistantId = "a1"
            )
        )
        var session by mutableStateOf(
            ChatSession(id = "s1", title = "Chat", createdAt = 1L, updatedAt = 1L, profileId = profile.id)
        )

        withUiScene(
            content = {
                ProvidersSubpage(
                    appState = state,
                    activeProfile = state.profiles.first(),
                    activeSession = session,
                    onSelectProfile = {},
                    onUpdateProfile = { tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                    onUpdateProfileById = { _, tr -> state = state.copy(profiles = listOf(tr(state.profiles.first()))) },
                    onAddProfile = {},
                    onDeleteProfile = {},
                    onUpdateSession = { tr -> session = tr(session) },
                    onNotify = {}
                )
            }
        ) { scene ->
            val renderedTexts = nodes(scene).map(::text)
            assertTrue(renderedTexts.any { it.contains("vendor/model-000") }, "First visible row must compose")
            assertFalse(
                renderedTexts.any { it.contains("vendor/model-210") },
                "Far-offscreen row should not be composed upfront when virtualized with LazyColumn"
            )
        }
    }

    @Test
    fun desktopBrowserTabBarUpdatesTabTitleWithoutSessionCountChange() {
        val prof = ProviderProfile(id = "p1", name = "OpenAI", protocol = ProviderProtocol.OPENAI_RESPONSES, defaultModel = "gpt-4o")
        val asst = AssistantAgent(id = "a1", name = "Writer")
        var state by mutableStateOf(
            AppStateSnapshot(
                profiles = listOf(prof),
                activeProfileId = prof.id,
                assistants = listOf(asst),
                activeAssistantId = asst.id,
                sessions = listOf(
                    ChatSession(id = "s1", title = "Draft Alpha", createdAt = 1L, updatedAt = 1L, profileId = prof.id, assistantId = asst.id),
                    ChatSession(id = "s2", title = "Draft Beta", createdAt = 2L, updatedAt = 2L, profileId = prof.id, assistantId = asst.id)
                ),
                activeSessionId = "s1",
                openTabSessionIds = listOf("s1", "s2")
            )
        )

        withUiScene(
            content = {
                DesktopBrowserTabBar(
                    appState = state,
                    openTabIds = state.openTabSessionIds,
                    activeSessionId = state.activeSessionId,
                    activeStreamsBySession = emptyMap(),
                    onSelectTab = {},
                    onCloseTab = {},
                    onNewTab = {},
                    onOpenTabOverview = {}
                )
            }
        ) { scene ->
            assertTrue(nodes(scene).any { text(it) == "Draft Alpha" })

            // Rename s1 in place (session count remains 2, activeSessionId & openTabIds unchanged).
            state = state.copy(
                sessions = state.sessions.map { if (it.id == "s1") it.copy(title = "Renamed Alpha") else it }
            )
            settle(scene, 16_000_000L)

            assertTrue(nodes(scene).any { text(it) == "Renamed Alpha" })
            assertNull(nodes(scene).firstOrNull { text(it) == "Draft Alpha" })
        }
    }

    @Test
    fun topEditorialBarKeepsTitleBufferAndFloatsQuickModelPickerWithoutViewportShift() {
        val prof = ProviderProfile(
            id = "p1",
            name = "OpenAI",
            protocol = ProviderProtocol.OPENAI_RESPONSES,
            defaultModel = "gpt-4o",
            savedModels = listOf("gpt-4o", "gpt-4.1", "o3")
        )
        val asst = AssistantAgent(id = "a1", name = "Writer")
        var session by mutableStateOf(
            ChatSession(id = "s1", title = "Initial Title", createdAt = 1L, updatedAt = 1L, profileId = prof.id, assistantId = asst.id)
        )
        var showQuickPopover by mutableStateOf(false)
        var frameNanos = 16_000_000L

        withUiScene(
            width = 960,
            height = 720,
            content = {
                val snap = AppStateSnapshot(
                    profiles = listOf(prof),
                    activeProfileId = prof.id,
                    assistants = listOf(asst),
                    activeAssistantId = asst.id,
                    sessions = listOf(session),
                    activeSessionId = session.id
                )
                TopEditorialBar(
                    appState = snap,
                    activeSession = session,
                    activeAssistant = asst,
                    activeProfile = prof,
                    isCompactScreen = false,
                    showSidebar = false,
                    showSessionInspector = false,
                    showQuickModelPopover = showQuickPopover,
                    openTabCount = 1,
                    backgroundStreamingCount = 0,
                    onOpenMobileTabSwitcher = {},
                    onNewChatTab = {},
                    onToggleSidebar = {},
                    onToggleSessionInspector = {},
                    onToggleQuickModelPopover = { showQuickPopover = !showQuickPopover },
                    onDismissQuickModelPopover = { showQuickPopover = false },
                    onSelectModelQuick = { _, mod ->
                        session = session.copy(modelOverride = mod)
                        showQuickPopover = false
                    },
                    onRenameTopicInline = { newTitle -> session = session.copy(title = newTitle) },
                    onRegenerateTitleAi = {},
                    onOpenSettings = {}
                )
                Box(
                    Modifier
                        .weight(1f)
                        .fillMaxWidth()
                        .semantics { contentDescription = "ViewportAnchor" }
                )
            }
        ) { scene ->
            // 1. Editing title preserves typed buffer if activeSession.title updates externally.
            val titleTrigger = nodes(scene).single { description(it) == Strings.editTitle("Initial Title") }
            click(titleTrigger)
            frameNanos += 16_000_000L
            settle(scene, frameNanos)

            val titleInput = nodes(scene).single { it.config.contains(SemanticsActions.SetText) }
            setText(titleInput, "My Unsaved Draft")
            frameNanos += 16_000_000L
            settle(scene, frameNanos)

            session = session.copy(title = "Background AI Renamed")
            frameNanos += 16_000_000L
            settle(scene, frameNanos)

            val preservedInput = nodes(scene).single { it.config.contains(SemanticsActions.SetText) }
            assertEquals("My Unsaved Draft", preservedInput.config[SemanticsProperties.EditableText].text)

            // Save title cleanly and close editor.
            val saveBtn = nodes(scene).single { text(it) == parseKissLabelIcons(Strings.save).cleanText }
            click(saveBtn)
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            assertEquals("My Unsaved Draft", session.title)

            // 2. Quick model picker floats in Popup without vertically shifting ViewportAnchor.
            val anchorTopBefore = nodes(scene).single { description(it) == "ViewportAnchor" }.boundsInRoot.top
            val modelCapsule = nodes(scene).single { text(it).contains("gpt-4o on OpenAI") }
            click(modelCapsule)
            frameNanos += 16_000_000L
            settle(scene, frameNanos)

            assertTrue(showQuickPopover)
            assertTrue(scene.semanticsOwners.size >= 2, "Quick model picker should render inside a floating Popup")
            assertTrue(nodes(scene).any { text(it) == Strings.chooseAModel })

            val anchorTopAfter = nodes(scene).single { description(it) == "ViewportAnchor" }.boundsInRoot.top
            assertEquals(anchorTopBefore, anchorTopAfter, "Viewport must not shift vertically when quick model picker opens")
        }
    }

    @Test
    fun viewportPausesAutoFollowOnScrollAndAsyncMediaAndLivePeekReserveBounds() {
        val asst = AssistantAgent(id = "a", name = "A", iconEmoji = "📖")
        val prof = ProviderProfile(
            id = "p",
            name = "OpenAI",
            protocol = ProviderProtocol.OPENAI_RESPONSES,
            baseUrl = "https://example.com",
            defaultModel = "gpt-4o"
        )
        val initialMessages = (0..11).map { idx ->
            MessageNode(
                id = "turn-$idx",
                role = if (idx % 2 == 0) MessageRole.USER else MessageRole.ASSISTANT,
                variants = listOf(
                    MessageVariant(
                        id = "v-$idx",
                        content = "Turn $idx line 1\nTurn $idx line 2\nTurn $idx line 3",
                        isStreaming = idx == 11
                    )
                )
            )
        }
        var session by mutableStateOf(
            ChatSession(
                id = "s-scroll",
                title = "Scroll Test",
                createdAt = 1L,
                updatedAt = 1L,
                profileId = prof.id,
                assistantId = asst.id,
                messages = initialMessages
            )
        )
        val appState = AppStateSnapshot(
            assistants = listOf(asst),
            profiles = listOf(prof),
            sessions = listOf(session),
            activeSessionId = session.id
        )
        var frameNanos = 0L

        withUiScene(
            width = 900,
            height = 460,
            content = {
                ManuscriptViewport(
                    appState = appState,
                    activeSession = session,
                    activeAssistant = asst,
                    activeProfile = prof,
                    preferences = PreferencesConfig(),
                    showSessionInspector = false,
                    onToggleSessionInspector = {},
                    isStreaming = true,
                    isCurrentlyThinkingPhase = false,
                    streamingTargetNodeId = "turn-11",
                    streamingStatus = "",
                    streamingInteractionId = "",
                    queuedPrompts = emptyList(),
                    onRemoveQueuedPrompt = {},
                    onSteerQueuedPromptNow = {},
                    readUserPromptInput = { "" },
                    onUserPromptChange = {},
                    readAssistantPrefillInput = { "" },
                    onAssistantPrefillChange = {},
                    showPrefillBox = false,
                    onTogglePrefillBox = {},
                    showComposerToolsDrawer = false,
                    onComposerToolsDrawerChange = {},
                    isGoalModeEnabled = false,
                    onToggleGoalMode = {},
                    isLiveVoiceMode = false,
                    isVoiceListening = false,
                    onToggleLiveVoiceMode = {},
                    onTriggerVoiceInput = {},
                    pendingAttachments = emptyList(),
                    onAddAttachment = {},
                    onPasteClipboardMedia = { false },
                    onRequestClearCurrentChat = {},
                    onRemoveAttachment = {},
                    onInspectAttachment = {},
                    onInspectGeneratedImage = {},
                    onExportChatMarkdown = {},
                    onOpenProviderSettings = {},
                    onSend = {},
                    onSendGuidedDirection = {},
                    onSteerNow = {},
                    onQueuePrompt = {},
                    onImpersonateDraft = {},
                    onAutoSummarizeStory = {},
                    onCompactContext = {},
                    onRewindToNode = {},
                    onForkSessionFromNode = {},
                    onStopStream = {},
                    onUpdateSession = {},
                    onUpdateProfile = {},
                    onRegenerateNode = {},
                    onContinueAssistantNode = {}
                )
            }
        ) { scene ->
            repeat(3) {
                frameNanos += 20_000_000L
                settle(scene, frameNanos)
            }
            assertTrue(nodes(scene).any { text(it).contains("Turn 11") }, "Initial open should follow to bottom turn")

            // Scroll up away from bottom via mouse wheel; streaming must pause auto-follow immediately.
            repeat(8) {
                scene.sendPointerEvent(
                    eventType = PointerEventType.Scroll,
                    position = Offset(400f, 180f),
                    scrollDelta = Offset(0f, -6f),
                    timeMillis = (it + 1) * 16L
                )
                frameNanos += 16_000_000L
                settle(scene, frameNanos)
            }
            val latestPill = nodes(scene).firstOrNull { text(it).contains(Strings.latest) }
            assertNotNull(latestPill, "Latest pill must appear once scrolled away from bottom")

            // Append streaming chunk while paused: Turn 11 update must NOT snap the viewport down.
            val lastTurn = session.messages.last()
            val updatedLast = lastTurn.copy(
                variants = listOf(
                    lastTurn.activeVariant.copy(content = lastTurn.activeVariant.content + "\nNEW_STREAMED_TAIL_MARKER")
                )
            )
            session = session.copy(messages = session.messages.dropLast(1) + updatedLast)
            Snapshot.sendApplyNotifications()
            repeat(4) {
                frameNanos += 64_000_000L
                settle(scene, frameNanos)
            }
            assertNotNull(
                nodes(scene).firstOrNull { text(it).contains(Strings.latest) },
                "Auto-follow must remain paused across streaming updates while scrolled up"
            )

            // Tapping Latest resumes auto-follow and jumps to the bottom marker.
            val clickableLatest = generateSequence(latestPill) { it.parent }
                .first { it.config.contains(SemanticsActions.OnClick) }
            click(clickableLatest)
            repeat(3) {
                frameNanos += 20_000_000L
                settle(scene, frameNanos)
            }
            assertTrue(nodes(scene).any { text(it).contains("NEW_STREAMED_TAIL_MARKER") })
        }

        withUiScene(
            width = 760,
            height = 900,
            content = {
                Box(Modifier.semantics { contentDescription = "AttachmentBoundsHost" }) {
                    TurnAttachmentsSection(
                        visible = true,
                        attachments = listOf(PromptAttachment("att1", "scene.png", "image/png")),
                        onInspectAttachment = {}
                    )
                }
                Box(Modifier.semantics { contentDescription = "GeneratedMediaBoundsHost" }) {
                    TurnGeneratedMediaSection(
                        mediaItems = listOf(
                            GeneratedMediaItem(
                                id = "gen1",
                                kind = MediaKind.IMAGE,
                                mimeType = "image/png",
                                aspectRatio = "16:9"
                            )
                        ),
                        onInspectGeneratedImage = {}
                    )
                }
                Box(Modifier.semantics { contentDescription = "LivePeekHost" }) {
                    MessageTurnReasoningDrawer(
                        nodeId = "n1",
                        activeVariantIndex = 0,
                        activeVar = MessageVariant("v1", content = "", thinkingSummary = "Single short thought"),
                        turnText = TurnTextLayout(beforeTool = emptyList(), finalText = ""),
                        preferences = PreferencesConfig(),
                        enableMd = false,
                        isActivelyThinkingThisNode = true,
                        isReasoningExpanded = false,
                        isLivePeekOnly = true,
                        onTogglePeekOrCollapse = {}
                    )
                }
            }
        ) { scene ->
            val attHost = nodes(scene).single { description(it) == "AttachmentBoundsHost" }
            val genHost = nodes(scene).single { description(it) == "GeneratedMediaBoundsHost" }
            val peekHost = nodes(scene).single { description(it) == "LivePeekHost" }
            assertTrue(attHost.boundsInRoot.height >= 160f, "Attachment placeholder must reserve vertical height (${attHost.boundsInRoot.height})")
            assertTrue(genHost.boundsInRoot.height >= 160f, "Generated media placeholder must reserve aspect height (${genHost.boundsInRoot.height})")
            assertTrue(peekHost.boundsInRoot.height >= 72f, "Live peek drawer must reserve stable minimum height (${peekHost.boundsInRoot.height})")
        }
    }

    @Test
    fun reduceMotionTransitionsAndLoadingPlaceholdersBehaveDeterministically() {
        var expanded by mutableStateOf(false)
        var loading by mutableStateOf(true)
        var observedExpandEnter: EnterTransition? = null
        var observedShrinkExit: ExitTransition? = null
        var observedPopEnter: EnterTransition? = null
        var observedPopExit: ExitTransition? = null
        var observedSideEnter: EnterTransition? = null
        var observedSideExit: ExitTransition? = null
        var observedFadeEnter: EnterTransition? = null
        var observedFadeExit: ExitTransition? = null
        var frameNanos = 0L

        withUiScene(
            width = 760,
            height = 420,
            content = {
                CompositionLocalProvider(LocalReduceMotion provides true) {
                    observedExpandEnter = motionExpandEnter()
                    observedShrinkExit = motionShrinkExit()
                    observedPopEnter = motionPopEnter()
                    observedPopExit = motionPopExit()
                    observedSideEnter = motionSideExpandEnter()
                    observedSideExit = motionSideShrinkExit()
                    observedFadeEnter = motionFadeEnter()
                    observedFadeExit = motionFadeExit()

                    AnimatedVisibility(
                        visible = expanded,
                        enter = motionExpandEnter(),
                        exit = motionShrinkExit()
                    ) {
                        BasicText("ReducedMotionRevealed")
                    }
                    SepiaProgressBar(fraction = 0.45f)
                    ImageViewerFallbackState(isLoading = loading, sourceLabel = "demo.png")
                }
            }
        ) { scene ->
            assertEquals(EnterTransition.None, observedExpandEnter)
            assertEquals(ExitTransition.None, observedShrinkExit)
            assertEquals(EnterTransition.None, observedPopEnter)
            assertEquals(ExitTransition.None, observedPopExit)
            assertEquals(EnterTransition.None, observedSideEnter)
            assertEquals(ExitTransition.None, observedSideExit)
            assertEquals(EnterTransition.None, observedFadeEnter)
            assertEquals(ExitTransition.None, observedFadeExit)

            assertTrue(nodes(scene).any { text(it) == Strings.loading }, "Image viewer must render Loading indicator while decoding")
            assertTrue(nodes(scene).none { text(it) == "ReducedMotionRevealed" })

            expanded = true
            loading = false
            Snapshot.sendApplyNotifications()
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).any { text(it) == "ReducedMotionRevealed" }, "EnterTransition.None must reveal content in 1 frame")
            assertTrue(nodes(scene).any { text(it) == Strings.unableToDecodeImagePreview("demo.png") })

            expanded = false
            Snapshot.sendApplyNotifications()
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).none { text(it) == "ReducedMotionRevealed" }, "ExitTransition.None must finish well before Motion.QUICK (32ms < 110ms)")
        }
    }

    @Test
    fun composerPopupsNavigateByKeyboardAndDismissOnEscape() {
        val prof = ProviderProfile(
            id = "p-kb",
            name = "KB Provider",
            protocol = ProviderProtocol.OPENAI_RESPONSES,
            defaultModel = "gpt-5"
        )
        val asst = AssistantAgent(id = "a-kb", name = "Planner")
        val lore = ContextFragment(id = "lore-alice", title = "Alice", content = "Detective in the archive")
        val session = ChatSession(
            id = "s-kb",
            title = "Keyboard Nav",
            createdAt = 1L,
            updatedAt = 1L,
            profileId = prof.id,
            assistantId = asst.id,
            fragments = listOf(lore)
        )
        val appState = AppStateSnapshot(
            assistants = listOf(asst),
            profiles = listOf(prof),
            sessions = listOf(session),
            activeSessionId = session.id
        )
        var userPromptInput by mutableStateOf("/")
        var compactCalls = 0
        var sendCalls = 0
        var frameNanos = 0L

        fun sendKey(scene: ImageComposeScene, key: Key) {
            scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyDown))
            scene.sendKeyEvent(KeyEvent(key, KeyEventType.KeyUp))
            Snapshot.sendApplyNotifications()
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            repeat(6) {
                frameNanos += 80_000_000L
                settle(scene, frameNanos)
            }
        }

        withUiScene(
            width = 900,
            height = 560,
            content = {
                ManuscriptViewport(
                    appState = appState,
                    activeSession = session,
                    activeAssistant = asst,
                    activeProfile = prof,
                    preferences = PreferencesConfig(),
                    showSessionInspector = false,
                    onToggleSessionInspector = {},
                    isStreaming = false,
                    isCurrentlyThinkingPhase = false,
                    streamingTargetNodeId = "",
                    streamingStatus = "",
                    streamingInteractionId = "",
                    queuedPrompts = emptyList(),
                    onRemoveQueuedPrompt = {},
                    onSteerQueuedPromptNow = {},
                    readUserPromptInput = { userPromptInput },
                    onUserPromptChange = { userPromptInput = it },
                    readAssistantPrefillInput = { "" },
                    onAssistantPrefillChange = {},
                    showPrefillBox = false,
                    onTogglePrefillBox = {},
                    showComposerToolsDrawer = false,
                    onComposerToolsDrawerChange = {},
                    isGoalModeEnabled = false,
                    onToggleGoalMode = {},
                    isLiveVoiceMode = false,
                    isVoiceListening = false,
                    onToggleLiveVoiceMode = {},
                    onTriggerVoiceInput = {},
                    pendingAttachments = emptyList(),
                    onAddAttachment = {},
                    onPasteClipboardMedia = { false },
                    onRequestClearCurrentChat = {},
                    onRemoveAttachment = {},
                    onInspectAttachment = {},
                    onInspectGeneratedImage = {},
                    onExportChatMarkdown = {},
                    onOpenProviderSettings = {},
                    onSend = { sendCalls++ },
                    onSendGuidedDirection = {},
                    onSteerNow = {},
                    onQueuePrompt = {},
                    onImpersonateDraft = {},
                    onAutoSummarizeStory = {},
                    onCompactContext = { compactCalls++ },
                    onRewindToNode = {},
                    onForkSessionFromNode = {},
                    onStopStream = {},
                    onUpdateSession = {},
                    onUpdateProfile = {},
                    onRegenerateNode = {},
                    onContinueAssistantNode = {}
                )
            }
        ) { scene ->
            frameNanos += 120_000_000L
            settle(scene, frameNanos)
            val composer = nodes(scene).single { it.config.contains(SemanticsActions.SetText) }
            assertTrue(composer.config[SemanticsActions.RequestFocus].action!!.invoke())
            frameNanos += 16_000_000L
            settle(scene, frameNanos)

            assertTrue(nodes(scene).any { text(it).contains("/compact") }, "Slash popup should list /compact")
            // Index 0 = /goal, 1 = /btw, 2 = /compact -> DirectionDown x 2 + Enter executes /compact without calling onSend.
            sendKey(scene, Key.DirectionDown)
            sendKey(scene, Key.DirectionDown)
            val highlightedSlash = nodes(scene).firstOrNull { text(it).contains("/compact") && it.config.contains(SemanticsActions.OnClick) }
            assertNotNull(highlightedSlash, "Highlighted /compact row must expose click semantics")
            assertEquals(true, highlightedSlash.config[SemanticsProperties.Selected], "DirectionDown x2 should select /compact")
            sendKey(scene, Key.Enter)
            assertEquals(1, compactCalls, "Enter must execute the keyboard-selected slash command")
            assertEquals(0, sendCalls, "Executing a slash suggestion must not trigger onSend")
            assertEquals("", userPromptInput)

            // Typing `/co` opens suggestions; Escape dismisses without erasing text; further edits re-open.
            userPromptInput = "/co"
            Snapshot.sendApplyNotifications()
            frameNanos += 120_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).any { text(it).contains("/compact") })
            sendKey(scene, Key.Escape)
            assertTrue(nodes(scene).none { text(it).contains("/compact") }, "Escape should dismiss slash popup")
            assertEquals("/co", userPromptInput, "Escape must keep typed prefix")

            userPromptInput = "/com"
            Snapshot.sendApplyNotifications()
            frameNanos += 120_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).any { text(it).contains("/compact") }, "Further typing must reopen slash suggestions")

            // `@` mention navigation: index 0 = @file, 1 = @mount, 2 = @Alice -> DirectionDown x 2 + Tab inserts `@Alice `.
            userPromptInput = "Check @"
            Snapshot.sendApplyNotifications()
            frameNanos += 120_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).any { text(it).contains("@Alice") })
            sendKey(scene, Key.DirectionDown)
            sendKey(scene, Key.DirectionDown)
            sendKey(scene, Key.Tab)
            assertEquals("Check @Alice ", userPromptInput, "Tab must complete the keyboard-selected @mention")
        }
    }

    @Test
    fun modalsEscapeAndFormChoicesKeepAccessibleSemantics() {
        var dismissCalls = 0
        var copied by mutableStateOf(false)
        var selectedChoice by mutableStateOf(false)
        var frameNanos = 0L

        withUiScene(
            width = 520,
            height = 420,
            content = {
                Column(Modifier.fillMaxSize()) {
                    SepiaChoiceRow(
                        label = "Strict Wire Mode",
                        selected = selectedChoice,
                        onClick = { selectedChoice = true }
                    )
                    RawWirePreviewBlock(
                        title = "Request JSON",
                        rawText = """{"model":"gpt-5"}""",
                        isCopied = copied,
                        onCopy = { copied = true }
                    )
                    DeleteConfirmDialog(
                        request = DeleteConfirmRequest.ClearCurrentMessages("Session"),
                        askBeforeDeleting = true,
                        onAskBeforeDeletingChange = {},
                        onDismiss = { dismissCalls++ },
                        onConfirm = {}
                    )
                }
            }
        ) { scene ->
            frameNanos += 32_000_000L
            settle(scene, frameNanos)

            val choice = nodes(scene).single {
                it.config.getOrNull(SemanticsProperties.Role) == Role.RadioButton &&
                    text(it).contains("Strict Wire Mode")
            }
            assertEquals(false, choice.config[SemanticsProperties.Selected])
            assertTrue(choice.config[SemanticsActions.RequestFocus].action!!.invoke())
            frameNanos += 32_000_000L
            settle(scene, frameNanos)
            assertEquals(true, nodes(scene).single { text(it).contains("Strict Wire Mode") }.config[SemanticsProperties.Focused])

            val copyBtn = nodes(scene).single { description(it) == Strings.copy }
            click(copyBtn)
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            assertTrue(nodes(scene).any { description(it) == Strings.copied }, "Copied state must update accessibility label")

            val dialogBtn = nodes(scene).last {
                it.config.contains(SemanticsActions.OnClick) && it.config.contains(SemanticsActions.RequestFocus)
            }
            assertTrue(dialogBtn.config[SemanticsActions.RequestFocus].action!!.invoke())
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            scene.sendKeyEvent(KeyEvent(Key.Escape, KeyEventType.KeyDown))
            scene.sendKeyEvent(KeyEvent(Key.Escape, KeyEventType.KeyUp))
            Snapshot.sendApplyNotifications()
            frameNanos += 16_000_000L
            settle(scene, frameNanos)
            assertEquals(1, dismissCalls, "Escape inside dialog must dismiss DeleteConfirmDialog")
        }
    }

    @Test
    fun disabledAndPlaceholderAlphasKeepLegibleContrastAcrossAllPresets() {
        fun compositeOver(fg: Color, bg: Color): Color {
            val a = fg.alpha
            return Color(
                red = fg.red * a + bg.red * (1f - a),
                green = fg.green * a + bg.green * (1f - a),
                blue = fg.blue * a + bg.blue * (1f - a),
                alpha = 1f
            )
        }
        ThemeConfig.PRESETS.forEach { preset ->
            val p = buildPaletteFromConfig(preset)
            val surfaces = listOf(p.canvasBg, p.sheetBg, p.cardBg, p.elevatedBg)
            listOf(Alpha.disabledInk, Alpha.placeholder).forEach { alpha ->
                val ink = p.mutedInk.copy(alpha = alpha)
                surfaces.forEach { bg ->
                    val ratio = contrastRatio(compositeOver(ink, bg), bg)
                    assertTrue(
                        ratio >= 3.0f,
                        "${preset.name}: mutedInk*$alpha over $bg contrast=$ratio (< 3.0)"
                    )
                }
            }
        }
    }

    @Test
    fun contextBreakdownLegendMatchesFreeSpaceColorWithoutCollision() {
        val breakdown = ContextWindowBreakdown(
            systemAndPersonaTokens = 2_000,
            lorebookAndMemoryTokens = 1_500,
            workspaceAndToolsTokens = 800,
            uploadedFilesTokens = 1_200,
            uploadedFilesCount = 2,
            chatHistoryTokens = 4_500,
            activeTurnsCount = 6,
            mutedTurnsCount = 1,
            composerDraftTokens = 300,
            contextLimitTokens = 32_000
        )
        ThemeConfig.PRESETS.forEach { preset ->
            val p = buildPaletteFromConfig(preset)
            val slices = buildContextCategorySlices(breakdown, p)
            val freeSlice = slices.single { it.isFreeSpace }
            val loreSlice = slices.single { it.label == Strings.lorebookStoryMemory }
            assertEquals(p.border.copy(alpha = 0.55f), freeSlice.color)
            assertNotEquals(
                loreSlice.color,
                freeSlice.color,
                "${preset.name}: Free Space swatch must not collide with Lorebook accentSecondary"
            )
        }
        withUiScene(
            width = 480,
            height = 560,
            content = {
                ContextWindowBreakdownPopover(
                    breakdown = breakdown,
                    sessionTotalCostUsd = 0.0,
                    isStreaming = false,
                    canCompact = false,
                    canSummarize = false,
                    onCompactContext = {},
                    onSummarizeStory = {},
                    onOpenPricingSettings = {},
                    onClose = {}
                )
            }
        ) { scene ->
            assertTrue(nodes(scene).any { text(it).contains(Strings.freeSpaceRemaining) })
            assertTrue(nodes(scene).any { text(it).contains(Strings.lorebookStoryMemory) })
        }
    }

    @Test
    fun workspacePagePlacesHeadingBeforeMcpServersAndSidebarSelectsAssistant() {
        val alice = AssistantAgent(id = "a1", name = "Alice")
        val bob = AssistantAgent(id = "a2", name = "Bob")
        val profile = ProviderProfile(id = "p1", name = "OpenAI", protocol = ProviderProtocol.OPENAI_RESPONSES, defaultModel = "gpt-4o")
        val session = ChatSession(id = "s1", title = "Study", createdAt = 1L, updatedAt = 1L, profileId = profile.id, assistantId = bob.id)
        val state = AppStateSnapshot(
            assistants = listOf(alice, bob),
            sessions = listOf(session),
            profiles = listOf(profile),
            activeAssistantId = bob.id,
            activeSessionId = session.id,
            activeProfileId = profile.id
        )
        withUiScene(
            width = 760,
            height = 1800,
            content = {
                WorkspaceSubpage(
                    appState = state,
                    activeSession = session,
                    activeProfile = profile,
                    onUpdateSession = {},
                    onUpdateProfile = {},
                    onUpdatePreferences = {},
                    onNotify = {}
                )
            }
        ) { scene ->
            val headerNode = nodes(scene).first { text(it).contains(Strings.workspaceAndAgentTools) }
            val mcpNode = nodes(scene).first { text(it).contains(Strings.mcpServers) }
            assertTrue(
                headerNode.boundsInRoot.top < mcpNode.boundsInRoot.top,
                "Workspace heading (${headerNode.boundsInRoot.top}) must appear above MCP Servers (${mcpNode.boundsInRoot.top})"
            )
        }
        withUiScene(
            width = 360,
            height = 560,
            content = {
                ParchmentSidebar(
                    appState = state,
                    activeAssistant = bob,
                    activeSession = session,
                    selectedTab = SidebarTab.ASSISTANTS,
                    isCompactScreen = false,
                    onSelectTab = {},
                    onSelectAssistant = {},
                    onEditAssistantInSettings = {},
                    onCreateNewAssistant = {},
                    onSelectSession = {},
                    onOpenPinnedReply = {},
                    onUnpinReply = {},
                    onNewTopic = {},
                    onTogglePinSession = {},
                    onRegenerateSessionTitle = {},
                    onDeleteSession = {},
                    onDeleteAllTopicsForAssistant = {},
                    onOpenSettings = {}
                )
            }
        ) { scene ->
            val bobRow = nodes(scene).single {
                text(it).contains("Bob") && it.config.contains(SemanticsProperties.Selected)
            }
            val aliceRow = nodes(scene).single {
                text(it).contains("Alice") && it.config.contains(SemanticsProperties.Selected)
            }
            assertEquals(true, bobRow.config[SemanticsProperties.Selected])
            assertEquals(false, aliceRow.config[SemanticsProperties.Selected])
        }
    }

    @Test
    fun chatHistoryActiveFormattingAndNumericDraftPreservation() {
        assertEquals("Chat History (4 active)", Strings.chatHistoryActive(4))
        assertEquals("Chat History (4 active, 2 left out)", Strings.chatHistoryActive(4, 2))

        var prefs by mutableStateOf(PreferencesConfig(chatSpendLimitUsd = 0.0))
        withUiScene(
            content = {
                SpendLimitCard(
                    prefs = prefs,
                    onUpdatePreferences = { prefs = it }
                )
            }
        ) { scene ->
            fun inputNode() = nodes(scene).single { it.config.contains(SemanticsActions.SetText) }

            setText(inputNode(), "1")
            settle(scene, 16_000_000L)
            assertEquals(1.0, prefs.chatSpendLimitUsd)
            assertEquals("1", inputNode().config[SemanticsProperties.EditableText].text)

            setText(inputNode(), "1.")
            settle(scene, 32_000_000L)
            assertEquals(1.0, prefs.chatSpendLimitUsd)
            assertEquals("1.", inputNode().config[SemanticsProperties.EditableText].text)

            setText(inputNode(), "1.5")
            settle(scene, 48_000_000L)
            assertEquals(1.5, prefs.chatSpendLimitUsd)
            assertEquals("1.5", inputNode().config[SemanticsProperties.EditableText].text)

            // External state reset still syncs back cleanly.
            prefs = prefs.copy(chatSpendLimitUsd = 0.0)
            settle(scene, 64_000_000L)
            assertEquals("", inputNode().config[SemanticsProperties.EditableText].text)
        }
    }

    @Test
    fun mobileTabSwitcherAndFilesSidebarVirtualizeAndFilterCleanly() {
        val sessions = (0 until 80).map { idx ->
            ChatSession(
                id = "tab-$idx",
                title = "TabSession-${idx.toString().padStart(3, '0')}",
                createdAt = 1L,
                updatedAt = 1L,
                profileId = "p1",
                assistantId = "a1"
            )
        }
        val state = AppStateSnapshot(
            sessions = sessions,
            profiles = listOf(ProviderProfile(id = "p1", name = "P", protocol = ProviderProtocol.OPENAI_RESPONSES)),
            activeProfileId = "p1",
            assistants = listOf(AssistantAgent(id = "a1", name = "Writer")),
            activeAssistantId = "a1"
        )
        withUiScene(
            width = 960,
            height = 480,
            content = {
                MobileTabSwitcherModal(
                    appState = state,
                    openTabIds = sessions.map { it.id },
                    activeSessionId = "tab-0",
                    activeStreamsBySession = emptyMap(),
                    isCompactScreen = false,
                    onSelectTab = {},
                    onCloseTab = {},
                    onNewTab = {},
                    onCloseModal = {}
                )
            }
        ) { scene ->
            val visibleCount = nodes(scene).count { text(it).contains("TabSession-") }
            assertTrue(visibleCount in 1..39, "Expected LazyColumn virtualization, but composed $visibleCount cards")

            val filterInput = nodes(scene).single { it.config.contains(SemanticsActions.SetText) }
            setText(filterInput, "TabSession-075")
            settle(scene, 16_000_000L)
            assertTrue(nodes(scene).any { text(it).contains("TabSession-075") })
        }

        val attachments = (0 until 60).map { idx ->
            PromptAttachment(
                id = "att-$idx",
                fileName = "staged-${idx.toString().padStart(2, '0')}.txt",
                mimeType = "text/plain",
                textContent = "line 1\nline 2"
            )
        }
        withUiScene(
            width = 400,
            height = 420,
            content = {
                ParchmentRightFilesSidebar(
                    activeSession = sessions.first(),
                    pendingAttachments = attachments,
                    onInspectAttachment = {},
                    onInspectGeneratedImage = {},
                    onReattachAttachment = {},
                    onClose = {},
                    modifier = Modifier.fillMaxSize()
                )
            }
        ) { scene ->
            val visibleFiles = nodes(scene).count { text(it).contains("staged-") }
            assertTrue(visibleFiles in 1..30, "Expected LazyColumn virtualization, but composed $visibleFiles files")
            assertTrue(nodes(scene).any { text(it).contains(Strings.staged) })
            assertTrue(nodes(scene).any { text(it).contains(Strings.linesCount(2)) })
        }
    }

    @Test
    fun tabSwitcherModalMatchesCommandPaletteScrimAndSurfaceContrast() {
        val sessions = listOf("tab-1", "tab-2").map { id ->
            ChatSession(id = id, title = "New Chat", createdAt = 1L, updatedAt = 1L, profileId = "p1", assistantId = "a1")
        }
        val state = AppStateSnapshot(
            sessions = sessions,
            profiles = listOf(ProviderProfile(id = "p1", name = "P", protocol = ProviderProtocol.OPENAI_RESPONSES)),
            activeProfileId = "p1",
            assistants = listOf(AssistantAgent(id = "a1", name = "Writer")),
            activeAssistantId = "a1"
        )
        fun renderPng(
            width: Int,
            height: Int,
            theme: ThemeConfig,
            content: @androidx.compose.runtime.Composable () -> Unit,
            inspect: (ImageComposeScene, java.awt.image.BufferedImage) -> Unit
        ) = runBlocking<Unit>(Dispatchers.Swing) {
            val scene = ImageComposeScene(width, height, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate)
            try {
                scene.setContent {
                    CompositionLocalProvider(LocalParchmentWindowSize provides ParchmentWindowSize(width.dp, height.dp)) {
                        ProvideParchmentTheme(theme) {
                            Box(Modifier.fillMaxSize().background(LocalParchmentPalette.current.canvasBg)) {
                                content()
                            }
                        }
                    }
                }
                Snapshot.sendApplyNotifications()
                val png = scene.render(16_000_000L).use { requireNotNull(it.encodeToData()).bytes }
                inspect(scene, ImageIO.read(ByteArrayInputStream(png)))
            } finally {
                scene.close()
            }
        }

        val themes = listOf(ThemeConfig(), ThemeConfig.PRESETS.first { it.name == "Lapis at Night" })
        for (theme in themes) {
            val palette = buildPaletteFromConfig(theme)
            var paletteBackdropRgb = 0
            renderPng(
                width = 960,
                height = 640,
                theme = theme,
                content = { CommandPalette(commands = listOf(PaletteCommand("New chat") {}), onDismiss = {}) }
            ) { _, img ->
                paletteBackdropRgb = img.getRGB(10, 10)
            }
            assertNotEquals(
                palette.canvasBg.toArgb(),
                paletteBackdropRgb,
                "${theme.name}: CommandPalette must darken its canvas backdrop"
            )

            for (compact in listOf(false, true)) {
                val (w, h) = if (compact) 390 to 844 else 960 to 640
                renderPng(
                    width = w,
                    height = h,
                    theme = theme,
                    content = {
                        MobileTabSwitcherModal(
                            appState = state,
                            openTabIds = sessions.map { it.id },
                            activeSessionId = "tab-1",
                            activeStreamsBySession = emptyMap(),
                            isCompactScreen = compact,
                            onSelectTab = {},
                            onCloseTab = {},
                            onNewTab = {},
                            onCloseModal = {}
                        )
                    }
                ) { scene, img ->
                    assertEquals(
                        paletteBackdropRgb,
                        img.getRGB(10, 10),
                        "${theme.name} compact=$compact: tab menu backdrop must match CommandPalette scrim"
                    )
                    val header = nodes(scene).first { text(it).startsWith(Strings.openTabs(2)) }.boundsInRoot
                    val sheetSurfaceRgb = img.getRGB(header.left.toInt() + 4, header.top.toInt() - 8)
                    assertEquals(
                        palette.elevatedBg.toArgb(),
                        sheetSurfaceRgb,
                        "${theme.name} compact=$compact: tab menu sheet must use elevatedBg like CommandPalette"
                    )
                }
            }
        }
    }

    @Test
    fun topBarComfortCodeBlockWrapSplitDiffAndCleanMarkdownExport() {
        // 1. TopEditorialBar desktop height stays comfortable (~54..66dp instead of bloated 84dp)
        val prof = ProviderProfile(id = "p1", name = "Gemini", protocol = ProviderProtocol.GEMINI_INTERACTIONS, defaultModel = "gemini-4-carbon")
        val asst = AssistantAgent(id = "a1", name = "Writer")
        val session = ChatSession(
            id = "s1",
            title = "Current PERFORMANCE.md Stats and Measurements",
            createdAt = 1L,
            updatedAt = 1L,
            profileId = prof.id,
            assistantId = asst.id,
            messages = listOf(
                MessageNode("u1", MessageRole.USER, listOf(MessageVariant("uv1", "Hello"))),
                MessageNode("a-empty", MessageRole.ASSISTANT, listOf(MessageVariant("av-empty", ""))),
                MessageNode("a-real", MessageRole.ASSISTANT, listOf(MessageVariant("av-real", "Done.")))
            )
        )
        val exported = formatSessionMarkdownExport(session, asst)
        assertTrue("### User" in exported && "Done." in exported, exported)
        assertEquals(1, Regex("""^### Writer""", RegexOption.MULTILINE).findAll(exported).count(), "Empty assistant stub must be skipped on export:\n$exported")

        withUiScene(
            width = 1100,
            height = 720,
            content = {
                val snap = AppStateSnapshot(
                    profiles = listOf(prof),
                    activeProfileId = prof.id,
                    assistants = listOf(asst),
                    activeAssistantId = asst.id,
                    sessions = listOf(session),
                    activeSessionId = session.id
                )
                Column {
                    TopEditorialBar(
                        appState = snap,
                        activeSession = session,
                        activeAssistant = asst,
                        activeProfile = prof,
                        isCompactScreen = false,
                        showSidebar = false,
                        showSessionInspector = false,
                        showQuickModelPopover = false,
                        openTabCount = 1,
                        backgroundStreamingCount = 0,
                        onOpenMobileTabSwitcher = {},
                        onNewChatTab = {},
                        onToggleSidebar = {},
                        onToggleSessionInspector = {},
                        onToggleQuickModelPopover = {},
                        onDismissQuickModelPopover = {},
                        onSelectModelQuick = { _, _ -> },
                        onRenameTopicInline = {},
                        onRegenerateTitleAi = {},
                        onOpenSettings = {}
                    )
                    SepiaDivider()
                }
            }
        ) { scene ->
            val settingsBtn = nodes(scene).first { description(it) == Strings.settings }
            assertTrue(settingsBtn.boundsInRoot.bottom <= 68f, "TopEditorialBar should be comfortably compact (<=68dp), got bottom=${settingsBtn.boundsInRoot.bottom}")
        }

        // 2. Code block & diff wrap toggle + global wrapCodeBlocks + Split Diff toggle (asserting real pixel layout wrapping)
        val longCode = "```kotlin\nval CODE_MARKER = \"" + "word ".repeat(60) + "\"\n```\n\n```diff\n+val DIFF_MARKER = \"" + "delta ".repeat(60) + "\"\n```"
        var globalWrap by mutableStateOf(false)
        withUiScene(
            width = 860,
            height = 1200,
            content = {
                Column {
                    LiteraryProseRenderer(text = longCode, wrapCodeBlocks = globalWrap)
                    SepiaFileDiffCard(
                        filePath = "src/App.kt",
                        actionLabel = "Edit",
                        oldText = "val a = 1",
                        newText = "val CARD_MARKER = \"" + "patch ".repeat(60) + "\"",
                        wrapCodeBlocks = globalWrap
                    )
                }
            }
        ) { scene ->
            fun flattenUnmerged(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flattenUnmerged)
            fun nodeWith(marker: String): SemanticsNode =
                scene.semanticsOwners.flatMap { flattenUnmerged(it.unmergedRootSemanticsNode) }.first { text(it).contains(marker) }

            val codeBefore = nodeWith("CODE_MARKER")
            val diffBefore = nodeWith("DIFF_MARKER")
            val cardBefore = nodeWith("CARD_MARKER")
            assertTrue(codeBefore.size.width > 860, "Unwrapped code fence should exceed viewport width, got ${codeBefore.size}")
            assertTrue(diffBefore.size.width > 860, "Unwrapped diff fence should exceed viewport width, got ${diffBefore.size}")
            assertTrue(cardBefore.size.width > 860, "Unwrapped SepiaFileDiffCard line should exceed viewport width, got ${cardBefore.size}")

            val wrapBtns = nodes(scene).filter { description(it) == Strings.wrapCode }
            assertEquals(3, wrapBtns.size, "Expected wrap buttons on standard fence, diff fence, and SepiaFileDiffCard")
            click(wrapBtns[0])
            settle(scene, 32_000_000L)
            val codeAfterClick = nodeWith("CODE_MARKER")
            assertTrue(codeAfterClick.size.width <= 860 && codeAfterClick.size.height > codeBefore.size.height, "Per-block wrap click must wrap code layout, got ${codeAfterClick.size}")

            val splitBtn = nodes(scene).first { description(it) == Strings.splitDiffView }
            click(splitBtn)
            globalWrap = true
            settle(scene, 64_000_000L)
            val diffAfterPref = nodeWith("DIFF_MARKER")
            val cardAfterPref = nodeWith("CARD_MARKER")
            assertTrue(diffAfterPref.size.width <= 860 && diffAfterPref.size.height > diffBefore.size.height, "Global wrapCodeBlocks must wrap ```diff fence, got ${diffAfterPref.size}")
            assertTrue(cardAfterPref.size.width <= 430 && cardAfterPref.size.height > cardBefore.size.height, "Global wrapCodeBlocks must wrap Split Diff SepiaFileDiffCard, got ${cardAfterPref.size}")
        }

        // 3. Provider model list vertically centers model ID beside trailing action buttons
        val modelProfile = ProviderProfile(
            id = "p-align",
            name = "Gemini",
            protocol = ProviderProtocol.GEMINI_INTERACTIONS,
            defaultModel = "gemini-4-carbon",
            savedModels = listOf("gemini-4-hydrogen", "gemini-4-carbon")
        )
        val alignState = AppStateSnapshot(
            profiles = listOf(modelProfile),
            activeProfileId = modelProfile.id,
            sessions = listOf(session.copy(profileId = modelProfile.id)),
            activeSessionId = session.id
        )
        withUiScene(
            width = 1100,
            height = 1400,
            content = {
                ProvidersSubpage(
                    appState = alignState,
                    activeProfile = modelProfile,
                    activeSession = alignState.sessions.single(),
                    onSelectProfile = {},
                    onUpdateProfile = {},
                    onUpdateProfileById = { _, _ -> },
                    onAddProfile = {},
                    onDeleteProfile = {},
                    onUpdateSession = {},
                    onNotify = {}
                )
            }
        ) { scene ->
            fun flattenUnmerged(n: SemanticsNode): List<SemanticsNode> = listOf(n) + n.children.flatMap(::flattenUnmerged)
            val unmerged = scene.semanticsOwners.flatMap { flattenUnmerged(it.unmergedRootSemanticsNode) }
            val carbonText = unmerged.first { text(it) == "gemini-4-carbon" }
            val rowGear = nodes(scene)
                .filter { description(it) == Strings.modelSettings }
                .minByOrNull { kotlin.math.abs(it.boundsInRoot.center.y - carbonText.boundsInRoot.center.y) }
            assertNotNull(rowGear)
            val dy = kotlin.math.abs(carbonText.boundsInRoot.center.y - rowGear.boundsInRoot.center.y)
            assertTrue(dy <= 2.5f, "Model text center.y (${carbonText.boundsInRoot.center.y}) must align with row controls (${rowGear.boundsInRoot.center.y}), dy=$dy")
        }

        // 4. Offline Skill & MCP import controls render cleanly on desktop Workspace settings
        val mcpPrefs = PreferencesConfig(
            mcpServers = listOf(McpServerConfig(id = "local-1", name = "Local Tool", command = "", enabled = false))
        )
        withUiScene(
            width = 960,
            height = 960,
            content = {
                Column {
                    McpServersCard(prefs = mcpPrefs, onUpdatePreferences = {}, onNotify = {})
                    SkillInstallCard(workspacePath = "C:\\tmp\\ws", canWrite = true, onNotify = {})
                }
            }
        ) { scene ->
            val allNodes = nodes(scene)
            assertNotNull(allNodes.firstOrNull { text(it) == Strings.importMcpJson }, "McpServersCard must expose Import JSON")
            assertNotNull(allNodes.firstOrNull { description(it) == Strings.browseLocalCommandDesc }, "Desktop McpServersCard must expose local command Browse button")
            assertNotNull(allNodes.firstOrNull { text(it) == Strings.importFolder }, "SkillInstallCard must expose Import folder")
            assertNotNull(allNodes.firstOrNull { text(it) == Strings.importFileOrZip }, "SkillInstallCard must expose Import file / .zip")
        }
    }
}
