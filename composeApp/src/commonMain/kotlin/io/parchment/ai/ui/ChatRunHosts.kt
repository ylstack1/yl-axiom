package io.parchment.ai.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.QueuedPromptItem

internal typealias SessionUpdate = (ChatSession) -> ChatSession
internal typealias TrackedUtility = suspend (String, String, ProviderProfile, String, String, Int?) -> Result<String>

/** State that outlives the window, so a run keeps working when the UI is recreated. */
internal open class AppRunState(val processState: ParchmentProcessState) {
    var appState by processState.snapshot
    var activeStreamsBySession by processState.runs.streams
    var bannerNotification by processState.notice
    var runErrorNotice by processState.runError
    var queuedPromptsBySession by processState.queuedPrompts
}

/** Everything a streaming run may read or change in the app. Anything else is out of its reach. */
internal class GenerationHost(
    processState: ParchmentProcessState,
    val activeAssistant: AssistantAgent,
    val activeProfile: ProviderProfile,
    private val goalMode: () -> Boolean,
    private val liveVoice: () -> Boolean,
    private val queuedDispatcher: () -> ((String, QueuedPromptItem) -> Unit)?,
    private val updateSession: (String, Boolean, SessionUpdate) -> Unit,
    private val updateLiveTurn: (String, LiveTurnRef, SessionUpdate) -> Unit,
    private val updateInMemory: (String, LiveTurnRef, SessionUpdate) -> Unit,
    private val generateTitle: (String, Boolean) -> Unit,
    private val trackedUtility: TrackedUtility
) : AppRunState(processState) {
    val isGoalModeEnabled: Boolean get() = goalMode()
    val isLiveVoiceMode: Boolean get() = liveVoice()
    val dispatchNextQueuedItem: ((String, QueuedPromptItem) -> Unit)? get() = queuedDispatcher()

    fun updateSessionById(sessionId: String, immediate: Boolean = false, transform: SessionUpdate) = updateSession(sessionId, immediate, transform)
    fun updateLiveTurnById(sessionId: String, turn: LiveTurnRef, transform: SessionUpdate) = updateLiveTurn(sessionId, turn, transform)
    fun updateLiveTurnInMemory(sessionId: String, turn: LiveTurnRef, transform: SessionUpdate) = updateInMemory(sessionId, turn, transform)
    fun generateTopicTitle(sessionId: String, forceRegenerate: Boolean = false) = generateTitle(sessionId, forceRegenerate)
    suspend fun generateTrackedUtility(sessionId: String, purpose: String, profile: ProviderProfile, model: String, prompt: String, maxOutputTokens: Int? = null): Result<String> = trackedUtility(sessionId, purpose, profile, model, prompt, maxOutputTokens)
}

/** Everything the story summary may read or change in the app. */
internal class StorySummaryHost(
    processState: ParchmentProcessState,
    val activeProfile: ProviderProfile,
    private val activeSessionProvider: () -> ChatSession,
    private val completeAction: (ChatSession) -> ChatSession?,
    val continueAssistantNodeAt: (Int, String) -> Unit,
    val updateSessionStreamState: (String, (SessionStreamRuntime?) -> SessionStreamRuntime?) -> Unit,
    private val updateSession: (String, Boolean, SessionUpdate) -> Unit,
    private val trackedUtility: TrackedUtility
) : AppRunState(processState) {
    val activeSession: ChatSession get() = activeSessionProvider()
    fun completeSessionForAction(session: ChatSession): ChatSession? = completeAction(session)
    fun updateSessionById(sessionId: String, immediate: Boolean = false, transform: SessionUpdate) = updateSession(sessionId, immediate, transform)
    suspend fun generateTrackedUtility(sessionId: String, purpose: String, profile: ProviderProfile, model: String, prompt: String, maxOutputTokens: Int? = null): Result<String> = trackedUtility(sessionId, purpose, profile, model, prompt, maxOutputTokens)
}
