package io.parchment.ai.ui

import io.parchment.ai.strings.Strings
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.Crossfade
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MonotonicFrameClock
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.PromptAttachment
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.network.fetchAvailableModels
import io.parchment.ai.model.AgentPermissionResolver
import io.parchment.ai.model.withImportedPermissions
import io.parchment.ai.network.AiProviderEngine
import io.parchment.ai.network.ModelsDevClient
import io.parchment.ai.ParchmentBuildConfig
import io.parchment.ai.network.OtaUpdateEngine
import io.parchment.ai.network.OtaUpdateState
import io.parchment.ai.network.RunErrorKind
import io.parchment.ai.platform.PlatformPickers
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.PlatformVoice
import io.parchment.ai.platform.PlatformWorkspace
import io.parchment.ai.protocol.O200kTokenizer
import io.parchment.ai.protocol.CardAndPromptCodec
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlin.io.encoding.ExperimentalEncodingApi


private const val STARTUP_UPDATE_CHECK_DELAY_MS = 8_000L

@OptIn(ExperimentalEncodingApi::class)
@Composable
fun ParchmentApp(
    /**
     * Platform window hook, composed inside the app theme: desktop names the caption after the open chat (or
     * Settings) and themes it; Android matches the system bar icons to the app theme.
     */
    platformWindowChrome: (@Composable (captionTitle: String) -> Unit)? = null,
    /** Desktop window hook; the window sends it key events that nothing focused consumed, so shortcuts always work. */
    shortcutDispatcher: AppShortcutDispatcher? = null
) {
    val uiScope = rememberCoroutineScope()
    val processState = parchmentProcessState
    val app = remember { ParchmentAppController(processState, uiScope) }
    with(app) {
        showComposerToolsDrawerState = remember(appState.activeSessionId) { mutableStateOf(false) }
        DisposableEffect(uiScope) {
            val clock = uiScope.coroutineContext[MonotonicFrameClock]
            processState.attachClock(clock)
            onDispose { processState.detachClock(clock) }
        }

        activeProfile = remember(appState.profiles, appState.activeProfileId) {
            appState.profiles.find { it.id == appState.activeProfileId }
                ?: appState.profiles.first()
        }
        activeSession = remember(appState.sessions, appState.activeSessionId) {
            appState.sessions.find { it.id == appState.activeSessionId }
                ?: appState.sessions.first()
        }
        activeAssistant = remember(appState.assistants, activeSession.assistantId, appState.activeAssistantId) {
            appState.assistants.find { it.id == activeSession.assistantId && it.id.isNotBlank() }
                ?: appState.assistants.find { it.id == appState.activeAssistantId && it.id.isNotBlank() }
                ?: appState.assistants.firstOrNull()
                ?: AssistantAgent(id = "asst-default", name = "Default Assistant", iconEmoji = "💬", systemPrompt = "")
        }

        var startupSettled by remember { mutableStateOf(false) }
        if (startupSettled) {
            McpServerConnections(appState.preferences.mcpServers)
        }

        // Background startup hydration: let the first two frames paint, warm O200kTokenizer in parallel, and hydrate inactive stubs.
        LaunchedEffect(Unit) {
            withFrameNanos { }
            withFrameNanos { }
            startupSettled = true
            launch(Dispatchers.Default) {
                O200kTokenizer.warmUp()
            }
            if (needsBackgroundChatHydration) {
                val beforeLoad = appState
                val keepHydrated = activeStreamsBySession.keys
                val hydrated = withContext(Dispatchers.Default) {
                    loadRemainingHistoricalSessionsDehydrated(beforeLoad, keepHydrated)
                }
                appState = mergeLoadedSessions(appState, beforeLoad, hydrated)
                needsBackgroundChatHydration = false
            }
            launch(Dispatchers.IO) {
                PlatformStorage.pruneStaleTempFiles()
            }
        }

        // Load the chat being switched to in full and shrink inactive ones back to stubs, to keep memory low.
        LaunchedEffect(appState.activeSessionId, tabRebalanceTick) {
            val current = appState
            val target = current.sessions.find { it.id == current.activeSessionId }
            val hadStubTarget = target?.isCompactStub == true
            val keepHydrated = activeStreamsBySession.keys
            val rebalanced = withContext(Dispatchers.Default) {
                ensureActiveSessionHydratedAndDehydrateOthers(current, keepHydrated)
            }
            if (rebalanced !== current) {
                appState = mergeLoadedSessions(appState, current, rebalanced)
            }
            if (hadStubTarget && appState.sessions.find { it.id == appState.activeSessionId }?.isCompactStub == true) {
                bannerNotification = Strings.chatHistoryIsUnavailableRestoreIts
            }
        }

        // Active tab's stream state
        currentTabStream = activeStreamsBySession[activeSession.id]
        isStreaming = currentTabStream != null
        isCurrentlyThinkingPhase = currentTabStream?.isThinkingPhase == true
        streamingTargetNodeId = currentTabStream?.targetNodeId.orEmpty()
        streamingStatus = currentTabStream?.statusText.orEmpty()
        streamingInteractionId = currentTabStream?.interactionId.orEmpty()
        queuedPrompts = queuedPromptsBySession[activeSession.id].orEmpty()

        // Open Browser Tabs list (always includes activeSession.id)
        openTabIds = remember(appState.openTabSessionIds, appState.sessions, activeSession.id) {
            val validSaved = appState.openTabSessionIds.filter { id -> appState.sessions.any { it.id == id } }
            normalizeOpenTabs(validSaved, activeSession.id)
        }

        // Composer text and staged attachments per chat, so switching tabs never moves or loses files.
        val draftSessionId = activeSession.id
        userPromptInputState = remember(draftSessionId) { mutableStateOf(activeSession.draftInput) }
        assistantPrefillInputState = remember(draftSessionId) { mutableStateOf(activeSession.draftPrefill) }
        // The root never reads the draft text, so a keystroke recomposes only the composer's viewport, not the whole
        // app: tracking reads it unobserved, and saving watches it through snapshotFlow instead of an effect key.
        Snapshot.withoutReadObservation {
            syncPendingDraftTracking(draftSessionId, userPromptInputState.value, assistantPrefillInputState.value)
        }
        LaunchedEffect(draftSessionId) {
            val promptState = userPromptInputState
            val prefillState = assistantPrefillInputState
            snapshotFlow { promptState.value to prefillState.value }.collectLatest { (prompt, prefill) ->
                val saved = appState.sessions.find { it.id == draftSessionId }
                if (saved != null && prompt == saved.draftInput && prefill == saved.draftPrefill) return@collectLatest
                delay(300L)
                persistDraftForSession(draftSessionId, prompt, prefill)
            }
        }
        DisposableEffect(draftSessionId) {
            val capturedPromptState = userPromptInputState
            val capturedPrefillState = assistantPrefillInputState
            onDispose {
                persistDraftForSession(draftSessionId, capturedPromptState.value, capturedPrefillState.value)
            }
        }
        pendingAttachments = pendingAttachmentsBySession[draftSessionId].orEmpty()
        LaunchedEffect(Unit) {
            PlatformPickers.registerFileDropCallbacks(
                onDragHover = { hovering -> isDraggingFilesOverWindow = hovering },
                onFilesDropped = { dropped ->
                    uiScope.launch {
                        attachPickedFiles(dropped)
                        if (dropped.none { it.oversized }) {
                            bannerNotification = Strings.attached(plural(dropped.size, "item"), dropped.joinToString { it.fileName })
                        }
                    }
                }
            )
        }

        // One process-wide writer retains all dirty sessions across debounce and UI lifecycle changes.
        LaunchedEffect(Unit) {
            PlatformRuntime.registerStateFlushCallback { blocking ->
                flushPendingDraftToDiskWriter()
                // Save streaming replies as shown, not as of their last checkpoint (up to 3.5 s older).
                LiveTurns.sessionIds().forEach { streamingId ->
                    appState.sessions.find { it.id == streamingId }?.takeUnless { it.isCompactStub }?.let { session ->
                        appDiskWriter.enqueue(sessions = listOf(session.withLiveTurns()), notify = false)
                    }
                }
                if (blocking) appDiskWriter.flushBlocking() else appDiskWriter.flushUrgentlyAsync()
            }
        }
        // An error that escaped the previous run is mentioned once, with its details to copy for a report.
        LaunchedEffect(Unit) {
            val report = withContext(Dispatchers.IO) { runCatching { PlatformRuntime.takeCrashReport() }.getOrNull() }
                ?: return@LaunchedEffect
            pendingCrashReport = report
            bannerNotification = Strings.crashNotice(report.saved)
        }
        LaunchedEffect(Unit) {
            var previousSaveNotice = ""
            appDiskWriter.lastError.collect { message ->
                val notice = message?.takeIf { it.isNotBlank() }?.let { Strings.unableToSaveChanges(it) }.orEmpty()
                if (notice.isNotEmpty()) bannerNotification = notice
                else if (bannerNotification == previousSaveNotice) bannerNotification = ""
                previousSaveNotice = notice
            }
        }

        // Automatically check for OTA updates on startup if enabled; development builds never offer one.
        // The wait keeps the network request and its parsing out of the first seconds of use.
        LaunchedEffect(appState.autoCheckOtaOnStartup, appState.otaGithubRepo) {
            if (appState.autoCheckOtaOnStartup && ParchmentBuildConfig.OFFICIAL_RELEASE) {
                delay(STARTUP_UPDATE_CHECK_DELAY_MS)
                otaState = OtaUpdateState.Checking
                OtaUpdateEngine.checkForUpdates(appState.otaGithubRepo).fold(
                    onSuccess = { rel ->
                        otaState = if (rel.isNewerThanCurrent) {
                            OtaUpdateState.Available(rel)
                        } else {
                            OtaUpdateState.UpToDate(OtaUpdateEngine.currentAppVersion, rel)
                        }
                    },
                    onFailure = {
                        otaState = OtaUpdateState.Idle
                    }
                )
            }
        }

        // Fetch models automatically only when none are saved, and never replace the user's chosen default model.
        LaunchedEffect(activeProfile.id, activeProfile.baseUrl, activeProfile.apiKey, activeProfile.autoFetchModels) {
            val hasCredentialsOrCustomProxy = activeProfile.apiKey.isNotBlank() ||
                activeProfile.baseUrl.trimEnd('/') != activeProfile.protocol.defaultBaseUrl.trimEnd('/')
            if (activeProfile.autoFetchModels && hasCredentialsOrCustomProxy && activeProfile.savedModels.isEmpty()) {
                withFrameNanos { }
                AiProviderEngine.fetchAvailableModels(activeProfile).onSuccess { fetched ->
                    if (fetched.isNotEmpty()) {
                        val existingModel = activeProfile.defaultModel.trim()
                        val mergedList = if (existingModel.isNotEmpty()) (listOf(existingModel) + fetched).distinct() else fetched
                        val chosenDefault = existingModel.ifBlank { pickBestInitialChatModel(mergedList) }
                        updateProfileById(activeProfile.id) {
                            it.copy(
                                savedModels = mergedList,
                                defaultModel = chosenDefault
                            )
                        }
                    }
                }
            }
        }

        // Automatically look up pricing & context window from models.dev when active model changes
        currentEffectiveModel = activeSession.modelOverride.ifBlank { activeProfile.defaultModel }.trim()
        LaunchedEffect(activeProfile.id, currentEffectiveModel) {
            if (currentEffectiveModel.isNotEmpty()) {
                val savedCustom = activeProfile.customModelSpecs[currentEffectiveModel]
                if (savedCustom != null && savedCustom.outputModalities.isNotEmpty()) {
                    updateProfileById(activeProfile.id) {
                        it.copy(
                            contextWindowTokens = savedCustom.contextWindowTokens,
                            maxOutputTokens = savedCustom.maxOutputTokens,
                            inputPricePerM = savedCustom.inputPricePerM,
                            outputPricePerM = savedCustom.outputPricePerM,
                            cacheReadPricePerM = savedCustom.cacheReadPricePerM,
                            cacheWritePricePerM = savedCustom.cacheWritePricePerM,
                            reasoningPricePerM = savedCustom.reasoningPricePerM
                        )
                    }
                } else {
                    if (savedCustom != null) {
                        updateProfileById(activeProfile.id) {
                            it.copy(
                                contextWindowTokens = savedCustom.contextWindowTokens,
                                maxOutputTokens = savedCustom.maxOutputTokens,
                                inputPricePerM = savedCustom.inputPricePerM,
                                outputPricePerM = savedCustom.outputPricePerM,
                                cacheReadPricePerM = savedCustom.cacheReadPricePerM,
                                cacheWritePricePerM = savedCustom.cacheWritePricePerM,
                                reasoningPricePerM = savedCustom.reasoningPricePerM
                            )
                        }
                    }
                    withFrameNanos { }
                    ModelsDevClient.fetchModelSpec(currentEffectiveModel).onSuccess { spec ->
                        updateProfileById(activeProfile.id) {
                            val mergedSpec = if (savedCustom != null) {
                                ModelsDevClient.mergeModelSpecs(savedCustom, spec)
                            } else {
                                spec
                            }
                            it.copy(
                                contextWindowTokens = mergedSpec.contextWindowTokens,
                                inputPricePerM = mergedSpec.inputPricePerM,
                                outputPricePerM = mergedSpec.outputPricePerM,
                                cacheReadPricePerM = mergedSpec.cacheReadPricePerM,
                                cacheWritePricePerM = mergedSpec.cacheWritePricePerM,
                                reasoningPricePerM = mergedSpec.reasoningPricePerM,
                                customModelSpecs = it.customModelSpecs + (currentEffectiveModel to mergedSpec)
                            )
                        }
                    }
                }
            }
        }

        generationHost = remember(activeAssistant, activeProfile) {
            GenerationHost(
                processState, activeAssistant, activeProfile, { isGoalModeEnabled }, { isLiveVoiceMode }, { dispatchNextQueuedItem },
                ::updateSessionById, ::updateLiveTurnById, ::updateLiveTurnInMemory, ::generateTopicTitle
            ) { id, purpose, profile, model, prompt, limit -> generateTrackedUtility(id, purpose, profile, model, prompt, limit) }
        }

        dispatchNextQueuedItem = remember {
            { sessId, nextItem ->
                dispatchPromptItem(
                    sessionId = sessId,
                    text = nextItem.text,
                    prefill = nextItem.prefill,
                    attachments = nextItem.attachments,
                    isGuidedDirection = nextItem.isGuidedDirection
                )
            }
        }

        summaryHost = remember(activeProfile) {
            StorySummaryHost(
                processState, activeProfile, { activeSession }, ::completeSessionForAction, ::continueAssistantNodeAt, ::updateSessionStreamState, ::updateSessionById
            ) { id, purpose, profile, model, prompt, limit -> generateTrackedUtility(id, purpose, profile, model, prompt, limit) }
        }

        LaunchedEffect(Unit) {
            if (appState.preferences.newChatOnLaunch && activeSession.messages.isNotEmpty()) {
                createNewTopicForAssistant(activeAssistant, openInNewTab = true)
            }
        }

        ProvideParchmentTheme(config = appState.theme) {
            val p = LocalParchmentPalette.current

            BoxWithConstraints(
                modifier = Modifier
                    .fillMaxSize()
                    .background(p.sheetBg)
            ) {
                val windowSize = ParchmentWindowSize(widthDp = maxWidth, heightDp = maxHeight)
                val isCompactScreen = maxWidth < 760.dp

                fun openNewChat() {
                    createNewTopicForAssistant(activeAssistant)
                    isSettingsOpen = false
                    if (isCompactScreen) showSidebar = false
                }

                /** Opens Settings at [category], or at its first page; without a category it toggles Settings. */
                fun openSettings(category: SettingsCategory?) {
                    if (category == null && isSettingsOpen) {
                        isSettingsOpen = false
                        return
                    }
                    if (category != null) activeSettingsCategory = category
                    startMobileSettingsAtRoot = category == null
                    isSettingsOpen = true
                    if (isCompactScreen) showSidebar = false
                }

                /** Everything Ctrl+K can do: main actions, every settings page, and each chat by name. */
                fun paletteCommands(): List<PaletteCommand> = buildList {
                    add(PaletteCommand(Strings.newChat2, Strings.ctrlN, ::openNewChat))
                    if (currentTabStream?.retryWait != null) {
                        add(PaletteCommand(Strings.retryNow) { currentTabStream?.retryControl?.retryNow() })
                    }
                    if (!isSettingsOpen && activeSession.messages.any { it.role == MessageRole.USER }) {
                        add(PaletteCommand(Strings.jumpToPromptInChat, Strings.ctrlG) { showPromptJump = true })
                    }
                    add(PaletteCommand(if (isSettingsOpen) Strings.closeSettings else Strings.openSettings, Strings.ctrl) { openSettings(null) })
                    if (!isCompactScreen) add(PaletteCommand(Strings.showOrHideTheSidebar, Strings.ctrlB) { showSidebar = !showSidebar })
                    if (!isCompactScreen && !isSettingsOpen) {
                        add(PaletteCommand(Strings.closeThisTab, Strings.ctrlW) { closeChatTab(appState.activeSessionId) })
                        if (openTabIds.size > 1) {
                            add(PaletteCommand(Strings.nextTab, Strings.ctrlTab) {
                                val here = openTabIds.indexOf(appState.activeSessionId).coerceAtLeast(0)
                                switchToTab(openTabIds[(here + 1) % openTabIds.size])
                            })
                        }
                    }
                    SettingsCategory.entries.forEach { category ->
                        add(PaletteCommand(Strings.settings4(category.label)) { openSettings(category) })
                    }
                    appState.sessions.sortedByDescending { it.updatedAt }.forEach { chat ->
                        add(PaletteCommand(Strings.chat2(chat.title)) {
                            switchToTab(chat.id)
                            isSettingsOpen = false
                            if (isCompactScreen) showSidebar = false
                        })
                    }
                }

                /** Ctrl+G rows: each prompt of the open chat, newest first; choosing one scrolls it to the top. */
                fun promptJumpCommands(): List<PaletteCommand> = promptJumpTargets(activeSession.messages).map { target ->
                    PaletteCommand(Strings.promptJumpRow(target.number, target.preview)) {
                        pendingPinJump = PinnedReply(activeSession.id, activeSession.title, target.nodeId, target.preview)
                    }
                }

                // Register Android System Back Gesture / Back Button Navigation Stack
                LaunchedEffect(
                    showCommandPalette,
                    showPromptJump,
                    activeViewerItem,
                    pendingDeleteConfirmation,
                    pendingRegenerateConfirm,
                    showMobileTabSwitcher,
                    showQuickModelPopover,
                    showRightFilesSidebar,
                    showSessionInspector,
                    showComposerToolsDrawer,
                    showSidebar,
                    isSettingsOpen,
                    isCompactScreen
                ) {
                    PlatformRuntime.registerBackHandler {
                        when {
                            showCommandPalette -> {
                                showCommandPalette = false
                                true
                            }
                            showPromptJump -> {
                                showPromptJump = false
                                true
                            }
                            activeViewerItem != null -> {
                                activeViewerItem = null
                                true
                            }
                            pendingDeleteConfirmation != null -> {
                                pendingDeleteConfirmation = null
                                true
                            }
                            pendingRegenerateConfirm != null -> {
                                pendingRegenerateConfirm = null
                                true
                            }
                            showMobileTabSwitcher -> {
                                showMobileTabSwitcher = false
                                true
                            }
                            showQuickModelPopover -> {
                                showQuickModelPopover = false
                                true
                            }
                            showRightFilesSidebar -> {
                                showRightFilesSidebar = false
                                true
                            }
                            showSessionInspector -> {
                                showSessionInspector = false
                                true
                            }
                            !isSettingsOpen && showComposerToolsDrawer -> {
                                showComposerToolsDrawer = false
                                true
                            }
                            isCompactScreen && showSidebar && !isSettingsOpen -> {
                                showSidebar = false
                                true
                            }
                            isSettingsOpen -> {
                                isSettingsOpen = false
                                true
                            }
                            else -> false
                        }
                    }
                }

                CompositionLocalProvider(
                    LocalParchmentWindowSize provides windowSize,
                    LocalReduceMotion provides appState.preferences.reduceMotion
                ) {
                    var horizontalSwipeDelta by remember { mutableStateOf(0f) }
                    var startedNearLeftEdge by remember { mutableStateOf(false) }

                    // The one shortcut list: the root runs it first while something is focused, and the desktop
                    // window runs it for keys nothing focused consumed (at launch or after a click on empty space).
                    val handleAppShortcut: (KeyEvent) -> Boolean = handle@{ event ->
                        if (event.type != KeyEventType.KeyDown) return@handle false
                        val ctrl = event.isCtrlPressed || event.isMetaPressed
                        when {
                            event.key == Key.Escape && showCommandPalette -> { showCommandPalette = false; true }
                            event.key == Key.Escape && showPromptJump -> { showPromptJump = false; true }
                            event.key == Key.Escape -> PlatformRuntime.handleSystemBackPressed()
                            ctrl && event.key == Key.K -> { showPromptJump = false; showCommandPalette = !showCommandPalette; true }
                            ctrl && event.key == Key.G && !isSettingsOpen -> { showCommandPalette = false; showPromptJump = !showPromptJump; true }
                            ctrl && event.key == Key.N -> { openNewChat(); true }
                            ctrl && event.key == Key.Comma -> { openSettings(null); true }
                            ctrl && event.key == Key.B && !isCompactScreen -> { showSidebar = !showSidebar; true }
                            ctrl && event.key == Key.W && !isCompactScreen && !isSettingsOpen -> { closeChatTab(appState.activeSessionId); true }
                            // Ctrl+Tab / Ctrl+Shift+Tab cycle the open chat tabs, like a browser.
                            ctrl && event.key == Key.Tab && !isCompactScreen && openTabIds.size > 1 -> {
                                val here = openTabIds.indexOf(appState.activeSessionId).coerceAtLeast(0)
                                val step = if (event.isShiftPressed) -1 else 1
                                switchToTab(openTabIds[(here + step + openTabIds.size) % openTabIds.size])
                                true
                            }
                            else -> false
                        }
                    }
                    if (shortcutDispatcher != null) {
                        SideEffect { shortcutDispatcher.handler = handleAppShortcut }
                        DisposableEffect(shortcutDispatcher) { onDispose { shortcutDispatcher.handler = { false } } }
                    }

                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .statusBarsPadding()
                            .navigationBarsPadding()
                            .imePadding()
                            .onPreviewKeyEvent(handleAppShortcut)
                            .background(p.canvasBg)
                            .then(
                                if (isCompactScreen && !isSettingsOpen) {
                                    Modifier.pointerInput(showSidebar, isSettingsOpen) {
                                        detectHorizontalDragGestures(
                                            onDragStart = { startOffset ->
                                                horizontalSwipeDelta = 0f
                                                startedNearLeftEdge = startOffset.x <= 36.dp.toPx()
                                            },
                                            onHorizontalDrag = { _, dragAmount ->
                                                horizontalSwipeDelta += dragAmount
                                            },
                                            onDragEnd = {
                                                if (!showSidebar && startedNearLeftEdge && horizontalSwipeDelta > 52.dp.toPx()) {
                                                    showSidebar = true
                                                } else if (showSidebar && horizontalSwipeDelta < -48.dp.toPx()) {
                                                    showSidebar = false
                                                }
                                                horizontalSwipeDelta = 0f
                                                startedNearLeftEdge = false
                                            }
                                        )
                                    }
                                } else Modifier
                            )
                    ) {
                        if (platformWindowChrome != null) {
                            platformWindowChrome(if (isSettingsOpen) Strings.settings else activeSession.title)
                        }

                        val hasModalOverlay = showCommandPalette || showPromptJump ||
                            showMobileTabSwitcher ||
                            pendingDeleteConfirmation != null ||
                            pendingRegenerateConfirm != null ||
                            activeViewerItem != null
                        val hideMainViewportForDrawer = isCompactScreen &&
                            !isSettingsOpen &&
                            (showSidebar || showRightFilesSidebar)

                        Column(
                            modifier = Modifier
                                .fillMaxWidth()
                                .then(if (hasModalOverlay) Modifier.clearAndSetSemantics {} else Modifier)
                        ) {
                        // 1. Top bar, hidden while Settings is open so there is only one way back to the chat.
                        if (!isSettingsOpen) {
                            val bgStreamingCount = activeStreamsBySession.keys.count { it != activeSession.id }
                            val tailVariant = activeSession.messages.lastOrNull()?.activeVariant
                            val prefixVaultHash = remember(
                                activeSession.messages.takeUnless { tailVariant?.isStreaming == true },
                                activeSession.id,
                                activeSession.messages.size
                            ) {
                                var hash = activeSession.id.hashCode().toLong() * 31L + activeSession.messages.size
                                val prefixEnd = (activeSession.messages.size - 1).coerceAtLeast(0)
                                for (idx in 0 until prefixEnd) {
                                    val v = activeSession.messages[idx].activeVariant
                                    hash = hash * 31L + v.toolCalls.size * 17L + v.mediaItems.size * 31L +
                                        v.webSources.size * 13L + v.attachments.size * 7L + v.content.length
                                }
                                hash
                            }
                            val vaultCountKey = remember(
                                prefixVaultHash,
                                pendingAttachments.size,
                                tailVariant?.toolCalls?.size,
                                tailVariant?.mediaItems?.size,
                                tailVariant?.webSources?.size,
                                tailVariant?.attachments?.size,
                                tailVariant?.isStreaming,
                                tailVariant?.let { if (it.isStreaming) it.content.length / 2048 else it.content.length }
                            ) {
                                var hash = prefixVaultHash * 31L + pendingAttachments.size
                                if (tailVariant != null) {
                                    val lenBucket = if (tailVariant.isStreaming) tailVariant.content.length / 2048 else tailVariant.content.length
                                    hash = hash * 31L + tailVariant.toolCalls.size * 17L + tailVariant.mediaItems.size * 31L +
                                        tailVariant.webSources.size * 13L + tailVariant.attachments.size * 7L + lenBucket +
                                        if (tailVariant.isStreaming) 1L else 0L
                                }
                                hash
                            }
                            val chatVaultCount = remember(vaultCountKey, activeSession.id, pendingAttachments) {
                                countSessionVaultItemsFast(activeSession, pendingAttachments)
                            }
                            TopEditorialBar(
                                appState = appState,
                                activeSession = activeSession,
                                activeAssistant = activeAssistant,
                                activeProfile = activeProfile,
                                isCompactScreen = isCompactScreen,
                                showSidebar = showSidebar,
                                showSessionInspector = showSessionInspector,
                                showRightFilesSidebar = showRightFilesSidebar,
                                chatArtifactsCount = chatVaultCount,
                                showQuickModelPopover = showQuickModelPopover,
                                openTabCount = openTabIds.size,
                                backgroundStreamingCount = bgStreamingCount,
                                onOpenMobileTabSwitcher = { showMobileTabSwitcher = true },
                                onNewChatTab = { createNewTopicForAssistant(activeAssistant, openInNewTab = true) },
                                onToggleSidebar = {
                                    showSidebar = !showSidebar
                                    if (!isCompactScreen) persist(appState.copy(preferences = appState.preferences.copy(sidebarOpen = showSidebar)))
                                },
                                onToggleSessionInspector = { showSessionInspector = !showSessionInspector },
                                onToggleRightFilesSidebar = { showRightFilesSidebar = !showRightFilesSidebar },
                                onToggleQuickModelPopover = { showQuickModelPopover = !showQuickModelPopover },
                                onDismissQuickModelPopover = { showQuickModelPopover = false },
                                onSelectModelQuick = { profId, modId ->
                                    updateProfileById(profId) { it.copy(defaultModel = modId) }
                                    updateActiveSession(immediate = true) { it.copy(profileId = profId, modelOverride = modId) }
                                    persist(appState.copy(activeProfileId = profId), immediateFlush = true)
                                    showQuickModelPopover = false
                                },
                                onRenameTopicInline = { newTitle ->
                                    updateActiveSession(immediate = true) {
                                        it.copy(title = newTitle.ifBlank { Strings.untitled }, isTitleManuallySet = true)
                                    }
                                },
                                onRegenerateTitleAi = { generateTopicTitle(activeSession.id, forceRegenerate = true) },
                                onOpenSettings = { cat ->
                                    if (cat != null) {
                                        activeSettingsCategory = cat
                                        startMobileSettingsAtRoot = false
                                    } else {
                                        startMobileSettingsAtRoot = true
                                    }
                                    isSettingsOpen = true
                                    if (isCompactScreen) showSidebar = false
                                }
                            )

                            SepiaDivider()

                            // Desktop Browser-Style Multi-Tasking Tab Bar
                            if (!isCompactScreen && openTabIds.size > 1) {
                                DesktopBrowserTabBar(
                                    appState = appState,
                                    openTabIds = openTabIds,
                                    activeSessionId = activeSession.id,
                                    activeStreamsBySession = activeStreamsBySession,
                                    onSelectTab = ::switchToTab,
                                    onCloseTab = ::closeChatTab,
                                    onNewTab = { createNewTopicForAssistant(activeAssistant, openInNewTab = true) },
                                    onOpenTabOverview = { showMobileTabSwitcher = true }
                                )
                                SepiaDivider()
                            }
                        }

                        // Top In-App OTA Banner (Available, Downloading Progress Bar, Installing, & Error)
                        when (val currentOta = otaState) {
                            is OtaUpdateState.Available -> {
                                if (!dismissOtaBanner) {
                                    @OptIn(ExperimentalLayoutApi::class)
                                    FlowRow(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .background(p.cardBg)
                                            .padding(horizontal = Space.s16, vertical = 7.dp),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalArrangement = Arrangement.spacedBy(Space.s6)
                                    ) {
                                        val sizeMb = (currentOta.release.assetSizeBytes / (1024.0 * 1024.0) * 10).toInt() / 10.0
                                        Row(
                                            verticalAlignment = Alignment.CenterVertically,
                                            horizontalArrangement = Arrangement.spacedBy(Space.s6),
                                            modifier = Modifier.padding(top = Space.s4, end = Space.s8)
                                        ) {
                                            KissVectorIcon(icon = KissIcon.SPARKLES, tint = p.accent, size = 14.dp)
                                            BasicText(
                                                text = Strings.versionIsReadyMb(currentOta.release.tagName.removePrefix("v"), sizeMb),
                                                style = p.uiBodyStyle.copy(fontWeight = FontWeight.Medium)
                                            )
                                        }
                                        Row(horizontalArrangement = Arrangement.spacedBy(Space.s6)) {
                                            SepiaButton(
                                                text = Strings.update,
                                                compact = true,
                                                style = SepiaButtonStyle.PRIMARY,
                                                onClick = { triggerOtaDownloadAndInstall(currentOta.release) }
                                            )
                                            SepiaButton(
                                                text = "×",
                                                contentDescription = Strings.dismissUpdateNotice,
                                                compact = true,
                                                style = SepiaButtonStyle.GHOST,
                                                onClick = { dismissOtaBanner = true }
                                            )
                                        }
                                    }
                                    SepiaDivider()
                                }
                            }
                            is OtaUpdateState.Downloading -> {
                                Column(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .background(p.cardBg)
                                        .padding(horizontal = Space.s16, vertical = Space.s8),
                                    verticalArrangement = Arrangement.spacedBy(Space.s6)
                                ) {
                                    val dlMb = (currentOta.downloadedBytes / (1024.0 * 1024.0) * 10).toInt() / 10.0
                                    val totMb = (currentOta.totalBytes / (1024.0 * 1024.0) * 10).toInt() / 10.0
                                    Row(
                                        modifier = Modifier.fillMaxWidth(),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalAlignment = Alignment.CenterVertically
                                    ) {
                                        Row(
                                            verticalAlignment = Alignment.CenterVertically,
                                            horizontalArrangement = Arrangement.spacedBy(Space.s6)
                                        ) {
                                            KissVectorIcon(icon = KissIcon.DOWNLOAD, tint = p.accent, size = 14.dp)
                                            BasicText(
                                                text = Strings.downloadingMb(currentOta.release.tagName, dlMb, totMb),
                                                style = p.uiBodyStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
                                            )
                                        }
                                        BasicText(
                                            text = "${currentOta.progressPercent}%",
                                            style = p.uiLabelStyle.copy(
                                                color = p.accent,
                                                fontWeight = FontWeight.Bold,
                                                fontSize = 12.sp,
                                                fontFeatureSettings = "tnum"
                                            )
                                        )
                                    }
                                    SepiaProgressBar(fraction = currentOta.progressPercent / 100f)
                                }
                                SepiaDivider()
                            }
                            is OtaUpdateState.Installing -> {
                                Column(
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .background(p.cardBg)
                                        .padding(horizontal = Space.s16, vertical = Space.s8),
                                    verticalArrangement = Arrangement.spacedBy(Space.s6)
                                ) {
                                    Row(
                                        verticalAlignment = Alignment.CenterVertically,
                                        horizontalArrangement = Arrangement.spacedBy(Space.s8)
                                    ) {
                                        KissStreamingPulseDot(color = p.accent)
                                        BasicText(
                                            text = currentOta.message,
                                            style = p.uiBodyStyle.copy(color = p.accent, fontWeight = FontWeight.SemiBold)
                                        )
                                    }
                                    Box(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .height(4.dp)
                                            .clip(RoundedCornerShape(Radius.pill))
                                            .background(p.accent)
                                    )
                                }
                                SepiaDivider()
                            }
                            is OtaUpdateState.Error -> {
                                if (!dismissOtaBanner) {
                                    @OptIn(ExperimentalLayoutApi::class)
                                    FlowRow(
                                        modifier = Modifier
                                            .fillMaxWidth()
                                            .background(p.cardBg)
                                            .padding(horizontal = Space.s16, vertical = 7.dp),
                                        horizontalArrangement = Arrangement.SpaceBetween,
                                        verticalArrangement = Arrangement.spacedBy(Space.s6)
                                    ) {
                                        BasicText(
                                            text = Strings.updateFailed(currentOta.message),
                                            style = p.uiBodyStyle.copy(color = p.danger, fontWeight = FontWeight.Medium),
                                            modifier = Modifier.weight(1f, fill = false).padding(top = Space.s4, end = Space.s8)
                                        )
                                        Row(horizontalArrangement = Arrangement.spacedBy(Space.s6)) {
                                            SepiaButton(
                                                text = Strings.retry,
                                                compact = true,
                                                style = SepiaButtonStyle.OUTLINE,
                                                onClick = ::triggerOtaCheckManual
                                            )
                                            SepiaButton(
                                                text = "×",
                                                contentDescription = Strings.dismissUpdateNotice,
                                                compact = true,
                                                style = SepiaButtonStyle.GHOST,
                                                onClick = { dismissOtaBanner = true }
                                            )
                                        }
                                    }
                                    SepiaDivider()
                                }
                            }
                            else -> {}
                        }
                        } // Closes top chrome wrapper hidden from screen readers when a modal is open.

                        LaunchedEffect(bannerNotification, isVoiceListening) {
                            if (bannerNotification.isNotBlank() && !isVoiceListening) {
                                val snap = bannerNotification
                                // A notice with an action (Undo, Copy details) stays longer, so there is time to reach it.
                                delay(if (pendingUndo != null || pendingCrashReport != null || runErrorNotice != null) UNDO_NOTICE_MS else NOTICE_MS)
                                if (bannerNotification == snap && !isVoiceListening) {
                                    bannerNotification = ""
                                }
                            }
                            // Once the notice has gone (or shows something else), the deleted chats can't come back.
                            if (bannerNotification.isBlank()) {
                                pendingUndo = null
                                pendingCrashReport = null
                                runErrorNotice = null
                            }
                        }

                        // 2. Main Content Area:
                        // - On Desktop: Side-by-side Sidebar + Chat/Settings
                        // - On phones: the chat or Settings underneath, with the sidebar sliding over 86% of the width.
                        // A streaming reply replaces appState every frame. The sidebar gets the copy it last drew from until
                        // something it shows changes, so its lists and pinned replies aren't rebuilt per frame.
                        val sidebarCache = remember { SidebarFingerprintCache() }
                        val sidebarKey = sidebarFingerprint(appState, sidebarCache)
                        val sidebarAppState = remember(sidebarKey) { appState }
                        val sidebarActiveSession = remember(sidebarKey) { activeSession }
                        val sidebarComposable: @Composable (Modifier) -> Unit = remember(
                            sidebarKey,
                            activeAssistant,
                            sidebarTab,
                            isCompactScreen
                        ) {
                            @Composable { mod ->
                                ParchmentSidebar(
                                appState = sidebarAppState,
                                activeAssistant = activeAssistant,
                                activeSession = sidebarActiveSession,
                                selectedTab = sidebarTab,
                                isCompactScreen = isCompactScreen,
                                onSelectTab = stableCallback { sidebarTab = it },
                                modifier = mod,
                                onSelectAssistant = stableCallback { asst ->
                                    val assistantSessions = appState.sessions.filter { it.assistantId == asst.id }
                                    if (appState.preferences.newChatWhenSwitchingAssistants || assistantSessions.isEmpty()) {
                                        createNewTopicForAssistant(asst)
                                    } else {
                                        val openTabForAsst = assistantSessions.firstOrNull { it.id in openTabIds }
                                        val nextSess = openTabForAsst ?: assistantSessions.maxByOrNull { it.updatedAt } ?: assistantSessions.first()
                                        val updatedTabs = if (nextSess.id in openTabIds) openTabIds else (openTabIds + nextSess.id)
                                        persist(
                                            appState.copy(
                                                activeAssistantId = asst.id,
                                                activeSessionId = nextSess.id,
                                                openTabSessionIds = updatedTabs
                                            ),
                                            immediateFlush = true
                                        )
                                    }
                                    if (appState.preferences.autoSwitchToTopics) {
                                        sidebarTab = SidebarTab.TOPICS
                                    }
                                },
                                onEditAssistantInSettings = stableCallback { asstId ->
                                    persist(appState.copy(activeAssistantId = asstId), immediateFlush = true)
                                    activeSettingsCategory = SettingsCategory.ASSISTANTS
                                    startMobileSettingsAtRoot = false
                                    isSettingsOpen = true
                                    if (isCompactScreen) showSidebar = false
                                },
                                onCreateNewAssistant = stableAction {
                                    val now = PlatformRuntime.currentTimeMillis()
                                    val newAsst = AssistantAgent(id = "asst-$now", name = "New Assistant", iconEmoji = "🤖")
                                    persist(
                                        appState.copy(
                                            assistants = appState.assistants + newAsst,
                                            activeAssistantId = newAsst.id
                                        ),
                                        immediateFlush = true
                                    )
                                    activeSettingsCategory = SettingsCategory.ASSISTANTS
                                    startMobileSettingsAtRoot = false
                                    isSettingsOpen = true
                                    if (isCompactScreen) showSidebar = false
                                },
                                onSelectSession = stableCallback { sessId ->
                                    switchToTab(sessId)
                                    if (isCompactScreen) showSidebar = false
                                },
                                onOpenPinnedReply = stableCallback { pin ->
                                    pendingPinJump = pin
                                    switchToTab(pin.sessionId)
                                    isSettingsOpen = false
                                    if (isCompactScreen) showSidebar = false
                                },
                                onUnpinReply = stableCallback { pin -> togglePinnedReply(pin.sessionId, pin.nodeId) },
                                onNewTopic = stableAction(::openNewChat),
                                onTogglePinSession = stableCallback { sessId ->
                                    updateSessionById(sessId, immediate = true) { it.copy(isPinned = !it.isPinned) }
                                },
                                onRegenerateSessionTitle = stableCallback { sessId ->
                                    generateTopicTitle(sessId, forceRegenerate = true)
                                },
                                onDeleteSession = stableCallback { targetId ->
                                    if (appState.preferences.confirmBeforeDeletingChat) {
                                        val title = appState.sessions.find { it.id == targetId }?.title ?: "Chat"
                                        pendingDeleteConfirmation = DeleteConfirmRequest.SingleSession(targetId, title)
                                    } else {
                                        executeDeleteSingleSession(targetId)
                                    }
                                },
                                onDeleteAllTopicsForAssistant = stableAction {
                                    val matchingCount = appState.sessions.count {
                                        it.assistantId == activeAssistant.id || appState.assistants.none { a -> a.id == it.assistantId }
                                    }
                                    if (appState.preferences.confirmBeforeDeletingChat) {
                                        pendingDeleteConfirmation = DeleteConfirmRequest.ClearAllSessions(
                                            assistantName = activeAssistant.name,
                                            count = matchingCount
                                        )
                                    } else {
                                        executeClearAllTopicsForAssistant()
                                    }
                                },
                                onOpenSettings = stableAction {
                                    startMobileSettingsAtRoot = true
                                    isSettingsOpen = true
                                    if (isCompactScreen) showSidebar = false
                                }
                            )
                            }
                        }

                        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                            Row(
                                modifier = Modifier
                                    .fillMaxSize()
                                    .then(
                                        if (hasModalOverlay || hideMainViewportForDrawer) Modifier.clearAndSetSemantics {}
                                        else Modifier
                                    )
                            ) {
                                AnimatedVisibility(
                                    visible = !isCompactScreen && showSidebar && !isSettingsOpen,
                                    enter = motionSideExpandEnter(),
                                    exit = motionSideShrinkExit()
                                ) {
                                    Row(modifier = Modifier.fillMaxHeight()) {
                                        sidebarComposable(Modifier.textScaledWidth(285.dp).fillMaxHeight())
                                        Box(
                                            modifier = Modifier
                                                .width(LineWidth.hairline)
                                                .fillMaxHeight()
                                                .background(p.border.copy(alpha = 0.7f))
                                        )
                                    }
                                }

                                Crossfade(
                                    targetState = isSettingsOpen,
                                    animationSpec = motionTween(Motion.MEDIUM),
                                    modifier = Modifier.weight(1f).fillMaxHeight()
                                ) { settingsMode ->
                                Column(Modifier.fillMaxSize()) {
                                    ToolApprovalPanel(currentTabStream?.toolApprovalBroker, activeSession.title)
                                    UserQuestionPanel(currentTabStream?.toolApprovalBroker, activeSession.title)
                                    Box(Modifier.weight(1f).fillMaxWidth()) {
                                if (settingsMode) {
                                    ParchmentSettingsScreen(
                                        appState = appState,
                                        activeProfile = activeProfile,
                                        activeSession = activeSession,
                                        initialCategory = activeSettingsCategory,
                                        startAtMobileRoot = startMobileSettingsAtRoot,
                                        otaState = otaState,
                                        onSelectProfile = { id -> persist(appState.copy(activeProfileId = id), immediateFlush = true) },
                                        onUpdateProfile = ::updateActiveProfile,
                                        onUpdateProfileById = ::updateProfileById,
                                        onAddProfile = { protocol ->
                                            val now = PlatformRuntime.currentTimeMillis()
                                            val newProf = ProviderProfile(
                                                id = "prof-$now",
                                                name = Strings.provider(protocol.displayName.substringBefore(" ")),
                                                protocol = protocol
                                            )
                                            persist(
                                                appState.copy(
                                                    profiles = appState.profiles + newProf,
                                                    activeProfileId = newProf.id
                                                ),
                                                immediateFlush = true
                                            )
                                        },
                                        onDeleteProfile = { targetProfId ->
                                            val remaining = appState.profiles.filterNot { it.id == targetProfId }
                                            if (remaining.isNotEmpty()) {
                                                val nextProfId = if (appState.activeProfileId == targetProfId) remaining.first().id else appState.activeProfileId
                                                persist(appState.copy(profiles = remaining, activeProfileId = nextProfId), immediateFlush = true)
                                                bannerNotification = Strings.deletedProviderProfile
                                            } else {
                                                bannerNotification = Strings.atLeastOneProviderProfileIs
                                            }
                                        },
                                        onUpdateAssistants = { updatedList, activeId ->
                                            val safeList = updatedList.ifEmpty {
                                                listOf(AssistantAgent(id = "asst-default", name = "Default Assistant", iconEmoji = "💬"))
                                            }
                                            val safeActive = safeList.find { it.id == activeId }?.id ?: safeList.first().id
                                            persist(appState.copy(assistants = safeList, activeAssistantId = safeActive), immediateFlush = true)
                                        },
                                        onUpdateDefaultModels = { dm ->
                                            persist(appState.copy(defaultModels = dm), immediateFlush = true)
                                        },
                                        onUpdatePreferences = { prefs ->
                                            persist(appState.copy(preferences = prefs), immediateFlush = true)
                                        },
                                        onUpdateTheme = { newTheme ->
                                            persist(appState.copy(theme = newTheme), immediateFlush = true)
                                        },
                                        onUpdateSession = { transform -> updateActiveSession(immediate = true, transform = transform) },
                                        onUpdateGlobalWorldBook = { wb ->
                                            persist(appState.copy(globalWorldBook = wb), immediateFlush = true)
                                        },
                                        onSaveCardToLibrary = { card ->
                                            val updatedCards = listOf(card) + appState.savedCards.filterNot { it.name == card.name }
                                            persist(appState.copy(savedCards = updatedCards), immediateFlush = true)
                                        },
                                        onSavePresetToLibrary = { preset ->
                                            val updatedPresets = listOf(preset) + appState.systemPromptPresets.filterNot { it.id == preset.id }
                                            persist(appState.copy(systemPromptPresets = updatedPresets), immediateFlush = true)
                                        },
                                        onDeletePresetFromLibrary = { presetId ->
                                            persist(appState.copy(systemPromptPresets = appState.systemPromptPresets.filterNot { it.id == presetId }), immediateFlush = true)
                                        },
                                        onUpdateOtaSettings = { repo, autoCheck ->
                                            persist(appState.copy(otaGithubRepo = repo, autoCheckOtaOnStartup = autoCheck), immediateFlush = true)
                                        },
                                        onCheckOtaNow = ::triggerOtaCheckManual,
                                        onInstallOtaRelease = ::triggerOtaDownloadAndInstall,
                                        onRestoreFullBackup = { importedSnapshot ->
                                            val restored = importedSnapshot.withImportedPermissions(appState.otaGithubRepo)
                                            activeStreamsBySession.values.forEach { it.job?.cancel() }
                                            activeStreamsBySession = emptyMap()
                                            queuedPromptsBySession = emptyMap()
                                            pendingAttachmentsBySession = emptyMap()
                                            val existingSessions = appState.sessions
                                            appProcessScope.launch {
                                                val (finalSnap, removedIds) = withContext(Dispatchers.Default) {
                                                    // Importing a settings-only file keeps the chats already on disk.
                                                    val mergedSessions = if (restored.sessions.isEmpty()) {
                                                        existingSessions
                                                    } else {
                                                        val byId = LinkedHashMap<String, ChatSession>()
                                                        for (s in existingSessions) byId[s.id] = s
                                                        for (s in restored.sessions) byId[s.id] = s
                                                        byId.values.toList()
                                                    }
                                                    val snap = sanitizeLoadedSnapshot(restored.copy(sessions = mergedSessions))
                                                    val removed = existingSessions.map { it.id }.toSet() - snap.sessions.map { it.id }.toSet()
                                                    snap to removed
                                                }
                                                withContext(Dispatchers.IO) {
                                                    appDiskWriter.replaceState(finalSnap, removedIds)
                                                    appDiskWriter.flush()
                                                }
                                                appState = finalSnap
                                                bannerNotification = Strings.importedSettingsAssistantsPreserved(plural(finalSnap.sessions.size, "chat"))
                                            }
                                        },
                                        onImportChatSessions = { importedList ->
                                            val defaultProfId = appState.activeProfileId.ifBlank { appState.profiles.first().id }
                                            val defaultAsstId = appState.activeAssistantId.ifBlank { appState.assistants.first().id }
                                            val validProfIds = appState.profiles.map { it.id }.toSet()
                                            val validAsstIds = appState.assistants.map { it.id }.toSet()
                                            appProcessScope.launch {
                                                val (importedChats, addedOrUpdated, firstImportedId) = withContext(Dispatchers.IO) {
                                                    val baseTs = PlatformRuntime.currentTimeMillis()
                                                    var count = 0
                                                    var firstId = ""
                                                    val list = ArrayList<ChatSession>(importedList.size)
                                                    for (incomingChat in importedList) {
                                                        val rawChat = incomingChat.withImportedPermissions()
                                                        val safeId = rawChat.id.ifBlank { "sess-imp-$baseTs-$count" }
                                                        val validProfId = rawChat.profileId.takeIf { it in validProfIds } ?: defaultProfId
                                                        val validAsstId = rawChat.assistantId.takeIf { it in validAsstIds } ?: defaultAsstId
                                                        val normalized = rawChat.copy(
                                                            id = safeId,
                                                            profileId = validProfId,
                                                            assistantId = validAsstId,
                                                            messages = rawChat.messages.map { n ->
                                                                n.copy(variants = n.variants.map { v -> if (v.isStreaming) v.copy(isStreaming = false) else v })
                                                            }
                                                        )
                                                        saveSingleChatSessionToDisk(normalized)
                                                        list.add(normalized)
                                                        if (firstId.isEmpty()) firstId = safeId
                                                        count++
                                                    }
                                                    Triple(list, count, firstId)
                                                }
                                                val byId = LinkedHashMap<String, ChatSession>(appState.sessions.size + importedChats.size)
                                                for (existing in appState.sessions) byId[existing.id] = existing
                                                for (normalized in importedChats) byId[normalized.id] = normalized
                                                val sorted = byId.values.sortedWith(
                                                    compareByDescending<ChatSession> { it.isPinned }.thenByDescending { it.updatedAt }
                                                )
                                                val nextActive = if (firstImportedId.isNotEmpty()) firstImportedId else appState.activeSessionId
                                                val nextTabs = (openTabIds + firstImportedId).filter { it.isNotBlank() }.distinct()
                                                persist(
                                                    appState.copy(
                                                        sessions = sorted,
                                                        activeSessionId = nextActive,
                                                        openTabSessionIds = nextTabs
                                                    ),
                                                    immediateFlush = true
                                                )
                                                bannerNotification = Strings.importedInto(plural(addedOrUpdated, "chat"), PlatformStorage.getChatsStorageDirectoryPath())
                                            }
                                        },
                                        onWipeAllData = {
                                            activeStreamsBySession.values.forEach { it.job?.cancel() }
                                            activeStreamsBySession = emptyMap()
                                            queuedPromptsBySession = emptyMap()
                                            pendingAttachmentsBySession = emptyMap()
                                            PlatformVoice.stopSpeakingAloud()
                                            PlatformVoice.stopVoiceInput()
                                            appProcessScope.launch {
                                                withContext(Dispatchers.IO) { PlatformStorage.wipeAllLocalData() }
                                                appState = createDefaultAppState()
                                                bannerNotification = Strings.allLocalDataWipedCleanly
                                            }
                                        },
                                        onDeleteChatSessionById = ::executeDeleteSingleSession,
                                        onInspectAttachment = ::openAttachmentInViewer,
                                        onNotify = { bannerNotification = it },
                                        onClose = { isSettingsOpen = false }
                                    )
                                } else {
                                    ManuscriptViewport(
                                        appState = appState,
                                        activeSession = activeSession,
                                        activeAssistant = activeAssistant,
                                        activeProfile = activeProfile,
                                        preferences = appState.preferences,
                                        showSessionInspector = showSessionInspector,
                                        onToggleSessionInspector = stableAction { showSessionInspector = !showSessionInspector },
                                        isStreaming = isStreaming,
                                        isCurrentlyThinkingPhase = isCurrentlyThinkingPhase,
                                        streamingTargetNodeId = streamingTargetNodeId,
                                        streamingStatus = streamingStatus,
                                        retryWait = currentTabStream?.retryWait,
                                        onRetryNow = { currentTabStream?.retryControl?.retryNow() },
                                        streamingInteractionId = streamingInteractionId,
                                        queuedPrompts = queuedPrompts,
                                        onRemoveQueuedPrompt = stableCallback { qId ->
                                            val curList = queuedPromptsBySession[activeSession.id].orEmpty()
                                            queuedPromptsBySession = queuedPromptsBySession + (activeSession.id to curList.filterNot { it.id == qId })
                                        },
                                        onSteerQueuedPromptNow = stableCallback { item ->
                                            steerQueuedPromptNow(activeSession.id, item)
                                        },
                                        readUserPromptInput = { userPromptInputState.value },
                                        onUserPromptChange = stableCallback { text -> userPromptInput = text },
                                        readAssistantPrefillInput = { assistantPrefillInputState.value },
                                        onAssistantPrefillChange = stableCallback { pf -> assistantPrefillInput = pf },
                                        showPrefillBox = showPrefillBox,
                                        onTogglePrefillBox = stableAction { showPrefillBox = !showPrefillBox },
                                        showComposerToolsDrawer = showComposerToolsDrawer,
                                        onComposerToolsDrawerChange = stableCallback { showComposerToolsDrawer = it },
                                        isGoalModeEnabled = isGoalModeEnabled,
                                        onToggleGoalMode = stableAction { isGoalModeEnabled = !isGoalModeEnabled },
                                        isLiveVoiceMode = isLiveVoiceMode,
                                        isVoiceListening = isVoiceListening,
                                        onToggleLiveVoiceMode = stableAction {
                                            isLiveVoiceMode = !isLiveVoiceMode
                                            if (!isLiveVoiceMode) {
                                                PlatformVoice.stopSpeakingAloud()
                                            }
                                        },
                                        onTriggerVoiceInput = ::triggerVoiceInput,
                                        pendingAttachments = pendingAttachments,
                                        onAddAttachment = stableAction {
                                            uiScope.launch {
                                                val picked = PlatformPickers.pickFilesForImport(emptyList())
                                                if (picked.isNotEmpty()) {
                                                    attachPickedFiles(picked)
                                                }
                                            }
                                        },
                                        onAttachWorkspaceFile = stableCallback { relPath ->
                                            val targetSessionId = activeSession.id
                                            val wsRoot = AgentPermissionResolver.resolve(activeSession, activeAssistant, activeProfile).workspacePath
                                            if (wsRoot.isNotBlank()) {
                                                uiScope.launch {
                                                    val cleanLines = withContext(Dispatchers.IO) {
                                                        PlatformWorkspace.workspaceRead(wsRoot, relPath, 1, 1500)
                                                            .getOrNull()
                                                            ?.let(::stripWorkspaceReadLineNumbers)
                                                    } ?: return@launch
                                                    val curList = pendingAttachmentsBySession[targetSessionId].orEmpty()
                                                    if (curList.none { it.fileName == relPath }) {
                                                        val att = PromptAttachment(
                                                            id = "att-ws-${PlatformRuntime.currentTimeMillis()}",
                                                            fileName = relPath,
                                                            mimeType = "text/plain",
                                                            textContent = cleanLines
                                                        )
                                                        pendingAttachmentsBySession = pendingAttachmentsBySession +
                                                            (targetSessionId to (curList + att))
                                                    }
                                                }
                                            }
                                        },
                                        onReattachExistingAttachment = stableCallback { existingAtt ->
                                            val curList = pendingAttachmentsBySession[activeSession.id].orEmpty()
                                            if (curList.none { it.fileName == existingAtt.fileName }) {
                                                pendingAttachmentsBySession = pendingAttachmentsBySession +
                                                    (activeSession.id to (curList + existingAtt.copy(id = "att-re-${PlatformRuntime.currentTimeMillis()}")))
                                            }
                                        },
                                        onPasteClipboardMedia = run {
                                            val holder = remember { Array(1) { { false } } }
                                            holder[0] = {
                                                PlatformPickers.pasteClipboardMediaAsync { pasted ->
                                                    attachPickedFiles(pasted)
                                                    if (pasted.none { it.oversized }) {
                                                        bannerNotification = Strings.attached(plural(pasted.size, "clipboard item"), pasted.joinToString { it.fileName })
                                                    }
                                                }
                                            }
                                            remember { { holder[0]() } }
                                        },
                                        onRequestClearCurrentChat = stableAction {
                                            if (appState.preferences.confirmBeforeDeletingChat) {
                                                pendingDeleteConfirmation = DeleteConfirmRequest.ClearCurrentMessages(activeSession.title)
                                            } else {
                                                clearCurrentChatMessages(activeSession.id)
                                            }
                                        },
                                        onRemoveAttachment = stableCallback { attId ->
                                            val curAtts = pendingAttachmentsBySession[activeSession.id].orEmpty()
                                            pendingAttachmentsBySession = pendingAttachmentsBySession +
                                                (activeSession.id to curAtts.filterNot { it.id == attId })
                                        },
                                        onInspectAttachment = ::openAttachmentInViewer,
                                        onInspectGeneratedImage = stableCallback { media ->
                                            activeViewerItem = ActiveMediaViewerItem.ImageViewer(
                                                title = media.revisedPrompt.take(48).ifBlank { "Generated Image" },
                                                mimeType = media.mimeType.ifBlank { "image/png" },
                                                localFilePath = media.localFilePath,
                                                remoteUrl = media.remoteUrl
                                            )
                                        },
                                        onExportChatMarkdown = stableAction {
                                            uiScope.launch {
                                                val md = formatSessionMarkdownExport(activeSession, activeAssistant)
                                                val safeTitle = activeSession.title.replace(Regex("[^a-zA-Z0-9-_]"), "_").ifBlank { "chat" }
                                                val saved = PlatformPickers.saveFileForExport("$safeTitle.md", md.encodeToByteArray())
                                                if (saved != null) bannerNotification = Strings.exportedMarkdownTo(saved)
                                            }
                                        },
                                        onExportChatJson = stableAction {
                                            uiScope.launch {
                                                val hasLocalMedia = activeSession.messages.any { n ->
                                                    n.variants.any { v ->
                                                        v.mediaItems.any { it.localFilePath.isNotBlank() } ||
                                                            v.attachments.any { it.localFilePath.isNotBlank() }
                                                    }
                                                }
                                                if (hasLocalMedia) {
                                                    val zipName = buildExportZipFileNameForChat(activeSession)
                                                    val saved = runCatching { exportChatSessionsBundle(zipName, listOf(activeSession)) }.getOrElse { error ->
                                                        bannerNotification = Strings.chatExportFailed(error.message)
                                                        return@launch
                                                    }
                                                    if (saved != null) bannerNotification = Strings.exportedTheChatWithItsMedia(saved)
                                                } else {
                                                    val fileName = buildExportFileNameForChat(activeSession)
                                                    val bytes = CardAndPromptCodec.prettyJson.encodeToString(ChatSession.serializer(), activeSession).encodeToByteArray()
                                                    val saved = PlatformPickers.saveFileForExport(fileName, bytes)
                                                    if (saved != null) bannerNotification = Strings.exportedTheChatTo(saved)
                                                }
                                            }
                                        },
                                        onOpenProviderSettings = stableAction {
                                            activeSettingsCategory = SettingsCategory.PROVIDERS
                                            startMobileSettingsAtRoot = false
                                            isSettingsOpen = true
                                        },
                                        onSend = ::sendUserTurn,
                                        onSendGuidedDirection = ::sendGuidedDirection,
                                        onSteerNow = ::triggerSteerNow,
                                        onQueuePrompt = ::enqueueFollowUpPrompt,
                                        onImpersonateDraft = ::triggerImpersonateDraft,
                                        onAutoSummarizeStory = stableAction { triggerAutoSummarizeStory(autoMuteOlderTurns = false) },
                                        onCompactContext = stableAction { triggerAutoSummarizeStory(autoMuteOlderTurns = true) },
                                        onRewindToNode = stableCallback { nodeIndex ->
                                            val sessId = activeSession.id
                                            stopSessionStream(sessId)
                                            val total = activeSession.messages.size
                                            if (total > 0) {
                                                if (nodeIndex < total - 1) {
                                                    val keepCount = (nodeIndex + 1).coerceIn(1, total)
                                                    val dropped = total - keepCount
                                                    updateActiveSession(immediate = true) { sess ->
                                                        sess.copy(messages = sess.messages.take(keepCount))
                                                    }
                                                    bannerNotification = Strings.rewoundChatToTurnRemoved(nodeIndex + 1, plural(dropped, "later turn"))
                                                } else {
                                                    // Rewind on the last message removes that turn (or turn pair), so it always does something.
                                                    val dropCount = if (total >= 2 && activeSession.messages.last().role == MessageRole.ASSISTANT) 2 else 1
                                                    updateActiveSession(immediate = true) { sess ->
                                                        sess.copy(messages = sess.messages.dropLast(dropCount))
                                                    }
                                                    bannerNotification = Strings.rewoundLast(plural(dropCount, "turn"))
                                                }
                                            }
                                        },
                                        onForkSessionFromNode = stableCallback { nodeIndex ->
                                            val now = PlatformRuntime.currentTimeMillis()
                                            val keepVersions = appState.preferences.keepMessageVersionsWhenForking
                                            val sourceSession = activeSession.withSpendProvenance()
                                            val slicedMessages = sourceSession.messages
                                                .subList(0, (nodeIndex + 1).coerceAtMost(activeSession.messages.size))
                                                .map { n ->
                                                    if (keepVersions) n else n.copy(variants = listOf(n.activeVariant), activeVariantIndex = 0)
                                                }
                                            val forkedSession = sourceSession.copy(
                                                id = "sess-fork-$now",
                                                title = Strings.branch2(activeSession.title, nodeIndex + 1),
                                                isTitleManuallySet = true,
                                                createdAt = now,
                                                updatedAt = now,
                                                messages = slicedMessages
                                            )
                                            persist(
                                                appState.copy(
                                                    sessions = listOf(forkedSession) + appState.sessions,
                                                    activeSessionId = forkedSession.id,
                                                    openTabSessionIds = (openTabIds + forkedSession.id).distinct()
                                                ),
                                                immediateFlush = true
                                            )
                                            bannerNotification = Strings.startedANewBranchAtMessage(nodeIndex + 1)
                                        },
                                        onStopStream = stableAction {
                                            stopSessionStream(activeSession.id)
                                        },
                                        onUpdateSession = stableCallback { transform -> updateActiveSession(immediate = true, transform = transform) },
                                        onUpdateProfile = ::updateActiveProfile,
                                        onRegenerateNode = stableCallback { nodeIndex ->
                                            if (appState.preferences.confirmBeforeRegenerating) {
                                                pendingRegenerateConfirm = RegenerateConfirmRequest(activeSession.id, nodeIndex)
                                            } else {
                                                executeRegenerateFromNode(nodeIndex, activeSession.id)
                                            }
                                        },
                                        onTranslateNode = stableCallback { nodeIndex ->
                                            triggerTranslateNode(nodeIndex)
                                        },
                                        onContinueAssistantNode = stableCallback { idx -> continueAssistantNodeAt(idx, activeSession.id) },
                                        jumpToNodeId = pendingPinJump?.takeIf { it.sessionId == activeSession.id }?.nodeId.orEmpty(),
                                        onJumpHandled = stableAction { pendingPinJump = null },
                                        onTogglePinnedReply = stableCallback { nodeId -> togglePinnedReply(activeSession.id, nodeId) }
                                    )
                                    }
                                }
                                }
                            }

                            AnimatedVisibility(
                                visible = !isCompactScreen && showRightFilesSidebar && !isSettingsOpen,
                                enter = motionSideExpandEnter(),
                                exit = motionSideShrinkExit()
                            ) {
                                Row(modifier = Modifier.fillMaxHeight()) {
                                    Box(
                                        modifier = Modifier
                                            .width(LineWidth.hairline)
                                            .fillMaxHeight()
                                            .background(p.border.copy(alpha = 0.7f))
                                    )
                                    ParchmentRightFilesSidebar(
                                        activeSession = activeSession,
                                        pendingAttachments = pendingAttachments,
                                        onInspectAttachment = ::openAttachmentInViewer,
                                        onInspectGeneratedImage = { media ->
                                            activeViewerItem = ActiveMediaViewerItem.ImageViewer(
                                                title = media.revisedPrompt.take(48).ifBlank { "Generated Image" },
                                                mimeType = media.mimeType.ifBlank { "image/png" },
                                                localFilePath = media.localFilePath,
                                                remoteUrl = media.remoteUrl
                                            )
                                        },
                                        onReattachAttachment = { existingAtt ->
                                            val curList = pendingAttachmentsBySession[activeSession.id].orEmpty()
                                            if (curList.none { it.fileName == existingAtt.fileName }) {
                                                pendingAttachmentsBySession = pendingAttachmentsBySession +
                                                    (activeSession.id to (curList + existingAtt.copy(id = "att-re-${PlatformRuntime.currentTimeMillis()}")))
                                                bannerNotification = Strings.reAttachedToPrompt(existingAtt.fileName)
                                            }
                                        },
                                        onClose = { showRightFilesSidebar = false },
                                        modifier = Modifier.width(300.dp).fillMaxHeight()
                                    )
                                }
                            }
                            } // Closes Row(modifier = Modifier.fillMaxSize()) so Crossfade(Modifier.weight(1f)) gets 100% width!

                            // Mobile 86%-Width Slide-Over Sidebar Drawer + Dimmed Tap-to-Dismiss Backdrop Scrim
                            if (isCompactScreen) {
                                Row(modifier = Modifier.fillMaxSize()) {
                                    AnimatedVisibility(
                                        visible = showSidebar && !isSettingsOpen,
                                        enter = motionFadeEnter(Motion.STANDARD),
                                        exit = motionFadeExit(Motion.STANDARD)
                                    ) {
                                        Box(
                                            modifier = Modifier
                                                .fillMaxSize()
                                                .background(Scrim.drawer)
                                                .dismissOnOutsideTap { showSidebar = false }
                                        )
                                    }
                                }

                                Row(
                                    modifier = Modifier
                                        .fillMaxHeight()
                                        .then(
                                            if (hasModalOverlay || !showSidebar || isSettingsOpen) Modifier.clearAndSetSemantics {}
                                            else Modifier
                                        )
                                ) {
                                    AnimatedVisibility(
                                        visible = showSidebar && !isSettingsOpen,
                                        enter = motionSideExpandEnter(),
                                        exit = motionSideShrinkExit()
                                    ) {
                                        Row(modifier = Modifier.fillMaxHeight()) {
                                            sidebarComposable(
                                                Modifier
                                                    .fillMaxWidth(0.86f)
                                                    .fillMaxHeight()
                                                    .blockTapThrough()
                                            )
                                            Box(
                                                modifier = Modifier
                                                    .width(1.dp)
                                                    .fillMaxHeight()
                                                    .background(p.border)
                                            )
                                        }
                                    }
                                }

                                // Mobile 86%-Width Right Slide-Over Files & Sources Drawer
                                Row(modifier = Modifier.fillMaxSize()) {
                                    AnimatedVisibility(
                                        visible = showRightFilesSidebar && !isSettingsOpen,
                                        enter = motionFadeEnter(Motion.STANDARD),
                                        exit = motionFadeExit(Motion.STANDARD)
                                    ) {
                                        Box(
                                            modifier = Modifier
                                                .fillMaxSize()
                                                .background(Scrim.drawer)
                                                .dismissOnOutsideTap { showRightFilesSidebar = false }
                                        )
                                    }
                                }

                                Row(
                                    modifier = Modifier
                                        .fillMaxSize()
                                        .then(
                                            if (hasModalOverlay || !showRightFilesSidebar || isSettingsOpen) Modifier.clearAndSetSemantics {}
                                            else Modifier
                                        ),
                                    horizontalArrangement = Arrangement.End
                                ) {
                                    AnimatedVisibility(
                                        visible = showRightFilesSidebar && !isSettingsOpen,
                                        enter = motionSideExpandEnter(),
                                        exit = motionSideShrinkExit()
                                    ) {
                                        Row(modifier = Modifier.fillMaxHeight()) {
                                            Box(
                                                modifier = Modifier
                                                    .width(1.dp)
                                                    .fillMaxHeight()
                                                    .background(p.border)
                                            )
                                            ParchmentRightFilesSidebar(
                                                activeSession = activeSession,
                                                pendingAttachments = pendingAttachments,
                                                onInspectAttachment = ::openAttachmentInViewer,
                                                onInspectGeneratedImage = { media ->
                                                    activeViewerItem = ActiveMediaViewerItem.ImageViewer(
                                                        title = media.revisedPrompt.take(48).ifBlank { Strings.generatedImageTitle },
                                                        mimeType = media.mimeType.ifBlank { "image/png" },
                                                        localFilePath = media.localFilePath,
                                                        remoteUrl = media.remoteUrl
                                                    )
                                                },
                                                onReattachAttachment = { existingAtt ->
                                                    val curList = pendingAttachmentsBySession[activeSession.id].orEmpty()
                                                    if (curList.none { it.fileName == existingAtt.fileName }) {
                                                        pendingAttachmentsBySession = pendingAttachmentsBySession +
                                                            (activeSession.id to (curList + existingAtt.copy(id = "att-re-${PlatformRuntime.currentTimeMillis()}")))
                                                        bannerNotification = Strings.reAttachedToPrompt(existingAtt.fileName)
                                                    }
                                                },
                                                onClose = { showRightFilesSidebar = false },
                                                modifier = Modifier
                                                    .fillMaxWidth(0.86f)
                                                    .fillMaxHeight()
                                                    .blockTapThrough()
                                            )
                                        }
                                    }
                                }
                            }

                            if (isDraggingFilesOverWindow) FileDropOverlay()

                            val confirmReq = pendingDeleteConfirmation
                            if (confirmReq != null) {
                                DeleteConfirmDialog(
                                    request = confirmReq,
                                    askBeforeDeleting = appState.preferences.confirmBeforeDeletingChat,
                                    onAskBeforeDeletingChange = { checked ->
                                        persist(appState.copy(preferences = appState.preferences.copy(confirmBeforeDeletingChat = checked)), immediateFlush = true)
                                    },
                                    onDismiss = { pendingDeleteConfirmation = null },
                                    onConfirm = {
                                        pendingDeleteConfirmation = null
                                        when (confirmReq) {
                                            is DeleteConfirmRequest.SingleSession -> executeDeleteSingleSession(confirmReq.sessionId)
                                            is DeleteConfirmRequest.ClearAllSessions -> executeClearAllTopicsForAssistant()
                                            is DeleteConfirmRequest.ClearCurrentMessages -> clearCurrentChatMessages(activeSession.id)
                                        }
                                    }
                                )
                            }
                            val regenReq = pendingRegenerateConfirm
                            if (regenReq != null) {
                                RegenerateConfirmDialog(
                                    replacesReply = appState.preferences.deletePreviousMessageWhenRegenerating,
                                    askBeforeRegenerating = appState.preferences.confirmBeforeRegenerating,
                                    onAskBeforeRegeneratingChange = { checked ->
                                        persist(appState.copy(preferences = appState.preferences.copy(confirmBeforeRegenerating = checked)), immediateFlush = true)
                                    },
                                    onDismiss = { pendingRegenerateConfirm = null },
                                    onConfirm = {
                                        pendingRegenerateConfirm = null
                                        executeRegenerateFromNode(regenReq.nodeIndex, regenReq.sessionId)
                                    }
                                )
                            }

                            // Built-In Zoomable Image & Text/Code Attachment Viewer Modal
                            val viewerItem = activeViewerItem
                            if (viewerItem != null) {
                                ParchmentAttachmentViewerModal(
                                    item = viewerItem,
                                    wrapCodeBlocks = appState.preferences.wrapCodeBlocks,
                                    onNotify = { bannerNotification = it },
                                    onClose = { activeViewerItem = null }
                                )
                            }

                            // Grid of all open tabs, on phones and desktop.
                            if (showMobileTabSwitcher) {
                                MobileTabSwitcherModal(
                                    appState = appState,
                                    openTabIds = openTabIds,
                                    activeSessionId = activeSession.id,
                                    activeStreamsBySession = activeStreamsBySession,
                                    isCompactScreen = isCompactScreen,
                                    onSelectTab = ::switchToTab,
                                    onCloseTab = ::closeChatTab,
                                    onCloseEmptyTabs = ::closeEmptyNewChatTabs,
                                    onNewTab = {
                                        createNewTopicForAssistant(activeAssistant, openInNewTab = true)
                                        showMobileTabSwitcher = false
                                    },
                                    onCloseModal = { showMobileTabSwitcher = false }
                                )
                            }
                            if (showCommandPalette) {
                                CommandPalette(paletteCommands(), onDismiss = { showCommandPalette = false })
                            }
                            if (showPromptJump) {
                                CommandPalette(
                                    promptJumpCommands(),
                                    onDismiss = { showPromptJump = false },
                                    placeholder = Strings.jumpToPromptPlaceholder
                                )
                            }
                            val noticeClipboard = LocalClipboardManager.current
                            val crashReport = pendingCrashReport?.takeIf { bannerNotification == Strings.crashNotice(it.saved) }
                            // A failed run's fixable cause leads to its fix; fixes that act on a chat need it open.
                            val runError = runErrorNotice?.takeIf { bannerNotification == Strings.runErrorNotice(it.kind) }
                            val runErrorFix: (() -> Unit)? = runError?.let { notice ->
                                val inThisChat = notice.sessionId == activeSession.id
                                when (notice.kind) {
                                    RunErrorKind.API_KEY -> ({ openSettings(SettingsCategory.PROVIDERS) })
                                    RunErrorKind.SPEND_LIMIT -> ({ openSettings(SettingsCategory.PREFERENCES) })
                                    RunErrorKind.MODEL -> if (inThisChat) ({ showQuickModelPopover = true }) else null
                                    RunErrorKind.CONVERSATION_LIMIT -> if (inThisChat) ({ triggerAutoSummarizeStory(autoMuteOlderTurns = true) }) else null
                                    RunErrorKind.QUOTA -> null
                                }
                            }
                            FloatingNotification(
                                message = bannerNotification,
                                onDismiss = { bannerNotification = "" },
                                actionLabel = when {
                                    pendingUndo != null -> Strings.undo
                                    crashReport != null -> Strings.copyDetails
                                    runError != null && runErrorFix != null -> Strings.runErrorAction(runError.kind)
                                    else -> null
                                },
                                onAction = {
                                    when {
                                        pendingUndo != null -> undoDelete()
                                        crashReport != null -> noticeClipboard.setText(AnnotatedString(crashReport.text))
                                        runErrorFix != null -> {
                                            bannerNotification = ""
                                            runErrorFix()
                                        }
                                    }
                                },
                                modifier = Modifier.align(Alignment.TopCenter).padding(horizontal = Space.s12, vertical = Space.s12)
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * Changes only when something the sidebar displays changes, so a streaming reply doesn't redraw it. Per chat:
 * what the sidebar shows, leaving out reply text on purpose (it changes every frame). Allocation-free key.
 */
internal class SidebarFingerprintCache(
    var lastAppState: AppStateSnapshot? = null,
    internal var lastKey: SidebarFingerprintKey? = null,
    val entries: HashMap<String, CachedSidebarSession> = HashMap()
)

internal class CachedSidebarSession(
    val session: ChatSession,
    val messagesRef: List<io.parchment.ai.model.MessageNode>,
    val structuralHash1: Long,
    val structuralHash2: Long,
    val pinnedHash1: Long,
    val pinnedHash2: Long
)

internal fun sidebarFingerprint(
    state: AppStateSnapshot,
    cache: SidebarFingerprintCache? = null
): Any {
    if (cache != null && cache.lastAppState === state) {
        cache.lastKey?.let { return it }
    }
    var h1 = 0x123456789ABCDEF0L
    var h2 = -0x5432109876543210L
    val sessions = state.sessions
    for (i in sessions.indices) {
        val s = sessions[i]
        val cached = cache?.entries?.get(s.id)
        val sH1: Long
        val sH2: Long
        val pH1: Long
        val pH2: Long
        if (cached != null && cached.session === s) {
            sH1 = cached.structuralHash1
            sH2 = cached.structuralHash2
            pH1 = cached.pinnedHash1
            pH2 = cached.pinnedHash2
        } else {
            val idHash = (s.id.hashCode().toLong() shl 32) or (s.id.length.toLong() and 0xffffffffL)
            val titleHash = (s.title.hashCode().toLong() shl 32) or (s.title.length.toLong() and 0xffffffffL)
            val asstHash = (s.assistantId.hashCode().toLong() shl 32) or (s.assistantId.length.toLong() and 0xffffffffL)
            val flags = (if (s.isPinned) 1L else 0L) or (if (s.isCompactStub) 2L else 0L)
            val msgCount = s.displayMessageCount.toLong()
            var m1 = mixSidebar64(0x243F6A8885A308D3L, idHash)
            m1 = mixSidebar64(m1, titleHash)
            m1 = mixSidebar64(m1, s.updatedAt)
            m1 = mixSidebar64(m1, asstHash)
            sH1 = mixSidebar64(m1, (msgCount shl 2) or flags)
            var m2 = mixSidebar64(-0x50A2B9C3D4E5F617L, s.updatedAt xor idHash)
            m2 = mixSidebar64(m2, titleHash xor (asstHash.rotateLeft(13)))
            sH2 = mixSidebar64(m2, (flags shl 32) or msgCount)
            if (cached != null && cached.messagesRef === s.messages) {
                pH1 = cached.pinnedHash1
                pH2 = cached.pinnedHash2
            } else {
                var ph1 = 0x13198A2E03707344L
                var ph2 = -0x540F2A11C7D0B539L
                val msgs = s.messages
                for (j in msgs.indices) {
                    val node = msgs[j]
                    if (node.pinned) {
                        val nodeHash = (node.id.hashCode().toLong() shl 32) or ((j + 1).toLong() shl 16) or (node.id.length.toLong() and 0xffffL)
                        ph1 = mixSidebar64(ph1, nodeHash)
                        ph2 = mixSidebar64(ph2, nodeHash.rotateLeft(19))
                    }
                }
                pH1 = ph1
                pH2 = ph2
            }
            cache?.entries?.put(s.id, CachedSidebarSession(s, s.messages, sH1, sH2, pH1, pH2))
        }
        h1 = mixSidebar64(h1, sH1)
        h1 = mixSidebar64(h1, pH1)
        h2 = mixSidebar64(h2, sH2 xor ((i + 1).toLong() shl 40))
        h2 = mixSidebar64(h2, pH2)
    }
    if (cache != null && cache.entries.size > sessions.size + 64) {
        val activeIds = HashSet<String>(sessions.size)
        for (i in sessions.indices) activeIds.add(sessions[i].id)
        cache.entries.keys.retainAll(activeIds)
    }
    val key = SidebarFingerprintKey(
        sessionsHash1 = h1,
        sessionsHash2 = h2,
        sessionCount = sessions.size,
        assistants = state.assistants,
        preferences = state.preferences,
        activeSessionId = state.activeSessionId
    )
    if (cache != null) {
        cache.lastAppState = state
        cache.lastKey = key
    }
    return key
}

private inline fun mixSidebar64(h: Long, value: Long): Long {
    var z = h xor (value + -0x61c8864680b583ebL + (h shl 6) + (h ushr 2))
    z = (z xor (z ushr 30)) * -0x40a7b892e31b1a47L
    return z xor (z ushr 27)
}

internal class SidebarFingerprintKey(
    private val sessionsHash1: Long,
    private val sessionsHash2: Long,
    private val sessionCount: Int,
    private val assistants: List<AssistantAgent>,
    private val preferences: io.parchment.ai.model.PreferencesConfig,
    private val activeSessionId: String
) {
    override fun equals(other: Any?): Boolean {
        if (this === other) return true
        if (other !is SidebarFingerprintKey) return false
        return sessionsHash1 == other.sessionsHash1 &&
            sessionsHash2 == other.sessionsHash2 &&
            sessionCount == other.sessionCount &&
            activeSessionId == other.activeSessionId &&
            (assistants === other.assistants || assistants == other.assistants) &&
            (preferences === other.preferences || preferences == other.preferences)
    }

    override fun hashCode(): Int {
        var result = (sessionsHash1 xor (sessionsHash1 ushr 32)).toInt()
        result = 31 * result + (sessionsHash2 xor (sessionsHash2 ushr 32)).toInt()
        result = 31 * result + sessionCount
        result = 31 * result + activeSessionId.hashCode()
        result = 31 * result + assistants.hashCode()
        result = 31 * result + preferences.hashCode()
        return result
    }
}

/** Chats removed by the last delete, retaining in-memory data or cold stub JSON so Undo can restore them without main-thread serialization. */
internal class DeletedChatEntry(
    val listed: ChatSession,
    val fullSession: ChatSession? = null,
    val diskJson: String? = null
)

internal class DeletedChats(
    val entries: List<DeletedChatEntry>,
    val activeSessionId: String,
    val openTabIds: List<String>
)

private const val NOTICE_MS = 5_000L
private const val UNDO_NOTICE_MS = 8_000L

internal fun formatSessionMarkdownExport(session: ChatSession, activeAssistant: AssistantAgent): String = buildString {
    appendLine("# ${session.title}")
    appendLine()
    for (n in session.messages) {
        val v = n.activeVariant
        if (
            n.role == MessageRole.ASSISTANT &&
            v.content.isBlank() &&
            v.thinkingSummary.isBlank() &&
            v.toolCalls.isEmpty() &&
            v.attachments.isEmpty() &&
            v.mediaItems.isEmpty()
        ) {
            continue
        }
        val roleHeader = if (n.role == MessageRole.ASSISTANT) {
            session.activeCard?.name?.ifBlank { v.modelUsed.ifBlank { activeAssistant.name } } ?: activeAssistant.name
        } else {
            session.userName.ifBlank { Strings.user }
        }
        val muteTag = if (!n.includedInContext) Strings.muted else ""
        appendLine("### $roleHeader$muteTag")
        if (v.thinkingSummary.isNotBlank()) {
            appendLine("<details><summary>Reasoning</summary>")
            appendLine()
            appendLine(v.thinkingSummary)
            appendLine("</details>")
            appendLine()
        }
        if (v.toolCalls.isNotEmpty()) {
            appendLine("<details><summary>Tools Executed (${v.toolCalls.size})</summary>")
            appendLine()
            for (tc in v.toolCalls) {
                appendLine("- **`${tc.name}`**: `${tc.argumentsJson}`")
                if (tc.resultText.isNotBlank()) {
                    appendLine("  ```")
                    appendLine("  ${tc.resultText.take(1200)}")
                    appendLine("  ```")
                }
            }
            appendLine("</details>")
            appendLine()
        }
        if (v.content.isNotBlank()) {
            appendLine(v.content)
            appendLine()
        }
        if (v.attachments.isNotEmpty()) {
            appendLine(Strings.attachments(v.attachments.joinToString { it.fileName }))
            appendLine()
        }
        if (v.mediaItems.isNotEmpty()) {
            appendLine(Strings.generatedMedia(v.mediaItems.joinToString { it.localFilePath.ifBlank { it.remoteUrl } }))
            appendLine()
        }
        appendLine("---")
        appendLine()
    }
}
