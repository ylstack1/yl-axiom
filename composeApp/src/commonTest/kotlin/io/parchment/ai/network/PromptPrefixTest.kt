package io.parchment.ai.network

import io.parchment.ai.model.*
import kotlin.test.*

class PromptPrefixTest {
    private fun message(text: String) = MessageNode(text, MessageRole.USER, listOf(MessageVariant(text, content = text)))

    private fun chat(vararg messages: String) = ChatSession(
        "c",
        "t",
        1L,
        1L,
        "p",
        messages = messages.map(::message),
        fragments = listOf(
            ContextFragment("1", "Dragons", "Dragons breathe frost.", keywords = listOf("dragon"), mode = FragmentActivationMode.KEYWORD),
            ContextFragment("2", "Setting", "It always rains.", mode = FragmentActivationMode.PINNED)
        )
    )

    @Test fun aKeywordMatchLeavesTheSystemPromptUnchanged() {
        val quiet = AiProviderEngine.buildCompiledSystemPrompt(chat("hello"))
        val matched = AiProviderEngine.buildCompiledSystemPrompt(chat("a dragon appears"))
        assertEquals(quiet, matched)
        assertTrue("It always rains." in matched, "Pinned knowledge stays at the top")
    }

    @Test fun aKeywordMatchStillReachesTheModelNearTheEnd() {
        val session = chat("a dragon appears")
        val turns = AiProviderEngine.injectAuthorsNoteIntoHistory(session.messages, session)
        assertTrue("Dragons breathe frost." in turns.last().second)
        val quiet = chat("hello")
        assertFalse("Dragons breathe frost." in AiProviderEngine.injectAuthorsNoteIntoHistory(quiet.messages, quiet).last().second)
    }

    @Test fun macrosExpandInKeywordFragmentsAuthorsNoteAndPostHistory() {
        val card = CharacterOrScenarioCard(name = "Mira", postHistoryInstructions = "Keep {{char}} focused on {{user}}.")
        val base = chat("a dragon appears").copy(
            userName = "Arthur",
            activeCard = card,
            authorsNote = "{{user}} trusts {{char}}.",
            fragments = listOf(ContextFragment("1", "Lore", "{{char}} remembers {{user}} near the dragon.", keywords = listOf("dragon"), mode = FragmentActivationMode.KEYWORD))
        )
        val joined = AiProviderEngine.injectAuthorsNoteIntoHistory(base.messages, base).joinToString("\n") { it.second }
        assertTrue("Mira remembers Arthur near the dragon." in joined)
        assertTrue("Keep Mira focused on Arthur." in joined)
        assertTrue("Arthur trusts Mira." in joined)
        assertFalse("{{char}}" in joined)
        assertFalse("{{user}}" in joined)
    }
}
