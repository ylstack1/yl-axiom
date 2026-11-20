package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.ImageComposeScene
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.PreferencesConfig
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import io.parchment.ai.network.RetryWait
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.swing.Swing
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** While a run waits to try again, the composer's status offers Retry now: labelled, a full touch target, and live. */
class RetryWaitUiTest {
    private fun nodes(scene: ImageComposeScene): List<SemanticsNode> {
        fun flatten(node: SemanticsNode): List<SemanticsNode> = listOf(node) + node.children.flatMap(::flatten)
        return scene.semanticsOwners.flatMap { flatten(it.rootSemanticsNode) }
    }

    private fun retryNow(scene: ImageComposeScene): SemanticsNode? = nodes(scene).firstOrNull { node ->
        node.config.getOrNull(SemanticsActions.OnClick) != null &&
            node.config.getOrNull(SemanticsProperties.Text).orEmpty().any { it.text == Strings.retryNow }
    }

    @Test fun retryNowShowsOnlyWhileWaitingAndEndsTheWait() = runBlocking<Unit>(Dispatchers.Swing) {
        val assistant = AssistantAgent(id = "a", name = "Assistant")
        val profile = ProviderProfile(id = "p", name = "Fixture", protocol = ProviderProtocol.OPENAI_RESPONSES, defaultModel = "m")
        val reply = MessageNode("reply", MessageRole.ASSISTANT, listOf(MessageVariant("reply-v", content = "", isStreaming = true)))
        val prompt = MessageNode("prompt", MessageRole.USER, listOf(MessageVariant("prompt-v", content = "Hello")))
        val session = ChatSession("s", "Retry", 1L, 1L, profile.id, assistantId = assistant.id, messages = listOf(prompt, reply))
        val appState = AppStateSnapshot(assistants = listOf(assistant), profiles = listOf(profile), sessions = listOf(session), activeSessionId = session.id)
        val wait = RetryWait(2, 10, 12, "Chat Completions HTTP 503: busy")
        var retryWait by mutableStateOf<RetryWait?>(wait)
        var pressed = 0
        val scene = ImageComposeScene(900, 640, density = Density(1f), coroutineContext = Dispatchers.Swing.immediate)
        try {
            scene.setContent {
                CompositionLocalProvider(LocalParchmentWindowSize provides ParchmentWindowSize(900.dp, 640.dp)) {
                    Column(Modifier.fillMaxSize().background(LocalParchmentPalette.current.canvasBg)) {
                        ManuscriptViewport(
                            appState = appState, activeSession = session, activeAssistant = assistant, activeProfile = profile,
                            preferences = PreferencesConfig(), showSessionInspector = false, onToggleSessionInspector = {},
                            isStreaming = true, isCurrentlyThinkingPhase = false, streamingTargetNodeId = "reply",
                            streamingStatus = Strings.tryingAgainIn(wait.remainingSeconds, wait.attempt, wait.maxAttempts) + "\n" + wait.reason,
                            streamingInteractionId = "", retryWait = retryWait, onRetryNow = { pressed++ },
                            queuedPrompts = emptyList(), onRemoveQueuedPrompt = {}, onSteerQueuedPromptNow = {},
                            readUserPromptInput = { "" }, onUserPromptChange = {}, readAssistantPrefillInput = { "" },
                            onAssistantPrefillChange = {}, showPrefillBox = false, onTogglePrefillBox = {},
                            showComposerToolsDrawer = false, onComposerToolsDrawerChange = {}, isGoalModeEnabled = false,
                            onToggleGoalMode = {}, isLiveVoiceMode = false, isVoiceListening = false, onToggleLiveVoiceMode = {},
                            onTriggerVoiceInput = {}, pendingAttachments = emptyList(), onAddAttachment = {},
                            onPasteClipboardMedia = { false }, onRequestClearCurrentChat = {}, onRemoveAttachment = {},
                            onInspectAttachment = {}, onInspectGeneratedImage = {}, onExportChatMarkdown = {},
                            onOpenProviderSettings = {}, onSend = {}, onSendGuidedDirection = {}, onSteerNow = {},
                            onQueuePrompt = {}, onImpersonateDraft = {}, onAutoSummarizeStory = {}, onCompactContext = {},
                            onRewindToNode = {}, onForkSessionFromNode = {}, onStopStream = {}, onUpdateSession = {},
                            onUpdateProfile = {}, onRegenerateNode = {}, onContinueAssistantNode = {}
                        )
                    }
                }
            }
            var frame = 0L
            fun settle() = repeat(30) {
                Snapshot.sendApplyNotifications()
                frame += 16_000_000L
                scene.render(frame).close()
            }
            settle()
            val button = assertNotNull(retryNow(scene), "Retry now must show while the run waits")
            assertTrue(button.size.height >= 48, "Retry now must be a full touch target: ${button.size}")
            assertTrue(button.boundsInRoot.left > 450f, "Retry now must sit inline on the trailing right side of the status bar: ${button.boundsInRoot}")
            button.config[SemanticsActions.OnClick].action?.invoke()
            assertEquals(1, pressed)
            retryWait = null
            settle()
            assertNull(retryNow(scene), "Without a wait there is nothing to retry")
        } finally {
            scene.close()
        }
    }
}
