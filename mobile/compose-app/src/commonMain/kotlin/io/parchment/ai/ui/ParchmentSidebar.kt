package io.parchment.ai.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsHoveredAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.parchment.ai.model.AppStateSnapshot
import io.parchment.ai.model.AssistantAgent
import io.parchment.ai.model.ChatSession
import io.parchment.ai.strings.Strings
import kotlinx.collections.immutable.toImmutableList

// Left sidebar: Assistants, Chats and Pinned.
@Composable
internal fun ParchmentSidebar(
    appState: AppStateSnapshot,
    activeAssistant: AssistantAgent,
    activeSession: ChatSession,
    selectedTab: SidebarTab,
    isCompactScreen: Boolean,
    onSelectTab: (SidebarTab) -> Unit,
    modifier: Modifier = Modifier,
    onSelectAssistant: (AssistantAgent) -> Unit,
    onEditAssistantInSettings: (String) -> Unit,
    onCreateNewAssistant: () -> Unit,
    onSelectSession: (String) -> Unit,
    onOpenPinnedReply: (PinnedReply) -> Unit,
    onUnpinReply: (PinnedReply) -> Unit,
    onNewTopic: () -> Unit,
    onTogglePinSession: (String) -> Unit,
    onRegenerateSessionTitle: (String) -> Unit,
    onDeleteSession: (String) -> Unit,
    onDeleteAllTopicsForAssistant: () -> Unit,
    onOpenSettings: () -> Unit
) {
    val p = LocalParchmentPalette.current
    var searchQuery by remember { mutableStateOf("") }

    Column(
        modifier = modifier
            .background(p.sheetBg)
            .padding(Space.s12),
        verticalArrangement = Arrangement.spacedBy(Space.s10)
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(Space.s6),
            verticalAlignment = Alignment.CenterVertically
        ) {
            SepiaTextField(
                value = searchQuery,
                onValueChange = { searchQuery = it },
                placeholder = when (selectedTab) {
                    SidebarTab.ASSISTANTS -> Strings.searchAssistants
                    SidebarTab.PINNED -> Strings.searchPinned
                    SidebarTab.TOPICS -> Strings.searchChats
                },
                minHeight = ControlHeight.compact,
                modifier = Modifier.weight(1f)
            )
            SepiaButton(
                text = "+",
                contentDescription = if (selectedTab == SidebarTab.ASSISTANTS) Strings.newAssistantLabel else Strings.newChat2,
                compact = true,
                style = SepiaButtonStyle.PRIMARY,
                onClick = {
                    if (selectedTab == SidebarTab.ASSISTANTS) onCreateNewAssistant() else onNewTopic()
                }
            )
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(Radius.pill))
                .background(p.cardBg.copy(alpha = 0.7f))
                .border(LineWidth.hairline, p.border.copy(alpha = 0.65f), RoundedCornerShape(Radius.pill))
                .padding(Space.s2),
            horizontalArrangement = Arrangement.spacedBy(Space.s4)
        ) {
            listOf(SidebarTab.ASSISTANTS to Strings.assistants, SidebarTab.TOPICS to Strings.chats, SidebarTab.PINNED to Strings.pinned).forEach { (tab, label) ->
                val isSel = selectedTab == tab
                val tabInteraction = remember { MutableInteractionSource() }
                val tabHovered by tabInteraction.collectIsHoveredAsState()
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .kissClickable(
                            pressScale = 0.96f,
                            hoverScale = 1.01f,
                            interactionSource = tabInteraction,
                            role = Role.Tab,
                            selected = isSel,
                            onClick = { onSelectTab(tab) }
                        )
                        .clip(RoundedCornerShape(Radius.pill))
                        .background(
                            when {
                                isSel -> p.sheetBg
                                tabHovered -> p.accent.copy(alpha = 0.08f)
                                else -> Color.Transparent
                            }
                        )
                        .padding(vertical = Space.s6),
                    contentAlignment = Alignment.Center
                ) {
                    BasicText(
                        text = label,
                        style = p.uiBodyStyle.copy(
                            fontSize = 13.sp,
                            fontWeight = if (isSel) FontWeight.Bold else FontWeight.Medium,
                            color = if (isSel || tabHovered) p.accent else p.narrationInk
                        ),
                        // At large text sizes a long label shortens with an ellipsis instead of breaking mid-word.
                        maxLines = 1,
                        softWrap = false,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(horizontal = Space.s4)
                    )
                }
            }
        }

        if (selectedTab == SidebarTab.PINNED) {
            PinnedRepliesPanel(appState.sessions, activeSession.id, searchQuery, onOpenPinnedReply, onUnpinReply)
        } else if (selectedTab == SidebarTab.ASSISTANTS) {
            val filteredAssistants = remember(appState.assistants, searchQuery) {
                val q = searchQuery.trim()
                val list = if (q.isEmpty()) appState.assistants
                else appState.assistants.filter {
                    it.name.contains(q, ignoreCase = true) || it.systemPrompt.contains(q, ignoreCase = true)
                }
                list.toImmutableList()
            }

            LazyColumn(
                modifier = Modifier.weight(1f).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(Space.s6)
            ) {
                if (filteredAssistants.isEmpty()) {
                    item(key = "empty-assistants", contentType = "empty") {
                        BasicText(
                            text = Strings.noMatchingAssistantsFoundTap,
                            style = p.uiLabelStyle,
                            modifier = Modifier.padding(Space.s8)
                        )
                    }
                }
                itemsIndexed(filteredAssistants, key = { _, a -> a.id }, contentType = { _, _ -> "assistant" }) { _, asst ->
                    val isSel = asst.id == activeAssistant.id
                    val shape = RoundedCornerShape(Radius.r8)
                    val rowInteraction = remember { MutableInteractionSource() }
                    val rowHovered by rowInteraction.collectIsHoveredAsState()
                    val rowPressed by rowInteraction.collectIsPressedAsState()
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .kissClickable(
                                pressScale = 0.98f,
                                hoverScale = 1.01f,
                                interactionSource = rowInteraction,
                                selected = isSel,
                                onClick = { onSelectAssistant(asst) }
                            )
                            .clip(shape)
                            .background(
                                when {
                                    isSel -> p.cardBg
                                    rowPressed -> p.accent.copy(alpha = 0.14f)
                                    rowHovered -> p.accent.copy(alpha = 0.08f)
                                    else -> Color.Transparent
                                }
                            )
                            .then(if (isSel) Modifier.rubricMark(p.accent, verticalInset = Space.s8) else Modifier)
                            .padding(horizontal = Space.s10, vertical = Space.s8),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically
                    ) {
                        Row(
                            modifier = Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(Space.s10),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            SepiaAvatarCircle(
                                symbol = asst.iconEmoji,
                                selected = isSel,
                                sizeDp = 32.dp
                            )
                            BasicText(
                                text = asst.name,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                style = p.uiBodyStyle.copy(
                                    fontSize = 14.sp,
                                    fontWeight = if (isSel) FontWeight.SemiBold else FontWeight.Normal,
                                    color = if (isSel || rowHovered) p.accent else p.primaryInk
                                )
                            )
                        }
                        SepiaButton(
                            text = "⚙",
                            contentDescription = Strings.editAssistant,
                            compact = true,
                            style = SepiaButtonStyle.GHOST,
                            onClick = { onEditAssistantInSettings(asst.id) }
                        )
                    }
                }
            }
        } else {
            val storedChatIds = remember(
                appState.sessions.takeUnless { searchQuery.isBlank() },
                appState.assistants,
                activeAssistant.id,
                searchQuery.isBlank()
            ) {
                if (searchQuery.isBlank()) emptyList() else appState.sessions
                    .filter { it.isCompactStub && (it.assistantId == activeAssistant.id || appState.assistants.none { a -> a.id == it.assistantId }) }
                    .map { it.id }
            }
            val historySearch = rememberHistorySearch(storedChatIds, searchQuery)
            val sessionListSignature = remember(appState.sessions) {
                var h = 1125899906842597L
                for (s in appState.sessions) {
                    h = 31L * h + s.id.hashCode()
                    h = 31L * h + s.assistantId.hashCode()
                    h = 31L * h + s.title.hashCode()
                    h = 31L * h + (if (s.isPinned) 1 else 0)
                    h = 31L * h + s.displayMessageCount
                    h = 31L * h + (s.updatedAt xor (s.updatedAt ushr 32))
                    h = 31L * h + (if (s.isCompactStub) 1 else 0)
                }
                h xor (appState.sessions.size.toLong() shl 32)
            }
            val lowerTitleIndex = remember(sessionListSignature) {
                HashMap<String, String>(appState.sessions.size * 2).apply {
                    for (s in appState.sessions) {
                        put(s.id, s.title.lowercase())
                    }
                }
            }
            val assistantIdsSignature = remember(appState.assistants) {
                appState.assistants.joinToString(",") { it.id }
            }
            val topicsStructuralKey = remember(
                sessionListSignature,
                activeSession.id,
                activeAssistant.id,
                assistantIdsSignature,
                searchQuery,
                historySearch.matchingIds
            ) {
                "$sessionListSignature#${activeSession.id}:${activeAssistant.id}:$assistantIdsSignature:$searchQuery:${historySearch.matchingIds.joinToString(",")}"
            }
            val assistantTopics = remember(topicsStructuralKey) {
                val q = searchQuery.trim()
                val qLower = q.lowercase()
                val forAsst = appState.sessions.filter { it.assistantId == activeAssistant.id || appState.assistants.none { a -> a.id == it.assistantId } }
                val searched = if (q.isEmpty()) forAsst else forAsst.filter { s ->
                    (lowerTitleIndex[s.id]?.contains(qLower) == true) || chatMessagesContain(s, q) || s.id in historySearch.matchingIds
                }
                searched.sortedWith(compareByDescending<ChatSession> { it.isPinned }.thenByDescending { it.updatedAt })
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically
            ) {
                Row(
                    modifier = Modifier.weight(1f).padding(end = Space.s6),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(Space.s6)
                ) {
                    val hdrIcon = remember(activeAssistant.iconEmoji) {
                        parseKissLabelIcons(activeAssistant.iconEmoji).leadingIcon ?: KissIcon.SPARKLES
                    }
                    KissVectorIcon(icon = hdrIcon, tint = p.accent, size = IconSize.md)
                    BasicText(
                        text = if (historySearch.inProgress) Strings.searchingFullHistory else "${activeAssistant.name} (${assistantTopics.size})",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = p.uiLabelStyle.copy(fontWeight = FontWeight.SemiBold)
                    )
                }
                if (assistantTopics.size > 1 || assistantTopics.any { it.messages.isNotEmpty() || it.title != "New Chat" }) {
                    SepiaButton(
                        text = Strings.clearAll,
                        hint = Strings.deleteEveryChatForThis,
                        compact = true,
                        style = SepiaButtonStyle.GHOST,
                        onClick = onDeleteAllTopicsForAssistant
                    )
                }
            }

            var touchActionsChatId by remember { mutableStateOf<String?>(null) }
            LazyColumn(
                modifier = Modifier.weight(1f).fillMaxWidth(),
                verticalArrangement = Arrangement.spacedBy(Space.s2)
            ) {
                itemsIndexed(assistantTopics, key = { _, s -> s.id }, contentType = { _, _ -> "topic" }) { _, sess ->
                    val selected = sess.id == activeSession.id
                    val shape = RoundedCornerShape(Radius.r8)
                    val rowInteraction = remember { MutableInteractionSource() }
                    var rowHasFocus by remember { mutableStateOf(false) }
                    val rowHovered by rowInteraction.collectIsHoveredAsState()
                    val rowPressed by rowInteraction.collectIsPressedAsState()
                    // Right-click (desktop) offers the same actions as the row's buttons.
                    PlatformContextMenu(items = {
                        buildList {
                            add(PlatformMenuItem(if (sess.isPinned) Strings.unpinChat else Strings.pinChat) { onTogglePinSession(sess.id) })
                            if (sess.messages.isNotEmpty()) add(PlatformMenuItem(Strings.regenerateTitle) { onRegenerateSessionTitle(sess.id) })
                            add(PlatformMenuItem(Strings.deleteChat) { onDeleteSession(sess.id) })
                        }
                    }) {
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .onFocusChanged { rowHasFocus = it.hasFocus }
                                .kissClickable(
                                    pressScale = 0.98f,
                                    interactionSource = rowInteraction,
                                    selected = selected,
                                    onClick = { onSelectSession(sess.id) }
                                )
                                .then(if (sess.isPinned) Modifier.semantics { stateDescription = Strings.pinned } else Modifier)
                                .clip(shape)
                                .background(
                                    when {
                                        selected -> p.cardBg
                                        rowPressed -> p.accent.copy(alpha = 0.14f)
                                        rowHovered -> p.accent.copy(alpha = 0.06f)
                                        else -> Color.Transparent
                                    }
                                )
                                .then(if (selected) Modifier.rubricMark(p.accent, verticalInset = 9.dp) else Modifier)
                                .padding(horizontal = Space.s10, vertical = 7.dp),
                            horizontalArrangement = Arrangement.SpaceBetween,
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(Space.s4)
                                ) {
                                    if (sess.isPinned) {
                                        KissVectorIcon(icon = KissIcon.PIN, tint = p.accent, size = IconSize.sm)
                                    }
                                    BasicText(
                                        text = sess.title,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        style = p.uiBodyStyle.copy(
                                            fontSize = 14.sp,
                                            fontWeight = if (selected) FontWeight.Medium else FontWeight.Normal
                                        )
                                    )
                                }
                                val dateLabel = if (appState.preferences.showChatListDates && sess.updatedAt > 0L) {
                                    formatCompactTimestamp(sess.updatedAt)
                                } else ""
                                if (dateLabel.isNotEmpty()) {
                                    BasicText(text = dateLabel, style = p.uiLabelStyle.copy(fontSize = 11.sp))
                                }
                            }
                            // Row actions appear on hover or focus; touch opens them with one explicit button.
                            val showRowActions = if (isCompactScreen) touchActionsChatId == sess.id else rowHovered || rowHasFocus
                            if (isCompactScreen) {
                                SepiaButton(
                                    text = if (showRowActions) "▴" else "⋯",
                                    contentDescription = if (showRowActions) Strings.hideChatActions else Strings.chatActions,
                                    compact = true,
                                    style = SepiaButtonStyle.GHOST,
                                    onClick = { touchActionsChatId = if (showRowActions) null else sess.id }
                                )
                            }
                            if (showRowActions) Row(horizontalArrangement = Arrangement.spacedBy(Space.s2), verticalAlignment = Alignment.CenterVertically) {
                                SepiaButton(
                                    text = "📌",
                                    contentDescription = if (sess.isPinned) Strings.unpinChat else Strings.pinChat,
                                    compact = true,
                                    style = SepiaButtonStyle.GHOST,
                                    onClick = { onTogglePinSession(sess.id) }
                                )
                                if (sess.messages.isNotEmpty()) {
                                    SepiaButton(
                                        text = "✦",
                                        contentDescription = Strings.regenerateTitle,
                                        hint = Strings.writeANewTitleFrom,
                                        compact = true,
                                        style = SepiaButtonStyle.GHOST,
                                        onClick = { onRegenerateSessionTitle(sess.id) }
                                    )
                                }
                                SepiaButton(
                                    text = "🗑",
                                    contentDescription = Strings.deleteChat,
                                    compact = true,
                                    style = SepiaButtonStyle.GHOST,
                                    onClick = { onDeleteSession(sess.id) }
                                )
                            }
                        }
                    }
                }
            }
        }

        if (isCompactScreen) {
            SepiaDivider()

            SepiaButton(
                text = Strings.settings3,
                style = SepiaButtonStyle.GHOST,
                modifier = Modifier.fillMaxWidth(),
                onClick = onOpenSettings
            )
        }
    }
}
