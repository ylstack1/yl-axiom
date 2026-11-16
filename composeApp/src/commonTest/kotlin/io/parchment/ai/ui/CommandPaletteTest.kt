package io.parchment.ai.ui

import io.parchment.ai.model.MessageNode
import io.parchment.ai.model.MessageRole
import io.parchment.ai.model.MessageVariant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class CommandPaletteTest {
    private val commands = listOf("New chat", "Open settings", "Settings: Providers", "Chat: Trip notes for Kyoto", "Toggle sidebar")
        .map { PaletteCommand(it) {} }

    private fun labels(query: String) = filterPaletteCommands(commands, query).map { it.label }

    @Test
    fun anEmptyQueryKeepsEveryCommandInOrder() {
        assertEquals(commands.map { it.label }, labels("  "))
    }

    @Test
    fun everyWordMustMatchInAnyOrder() {
        assertEquals(listOf("Chat: Trip notes for Kyoto"), labels("kyoto trip"))
    }

    @Test
    fun labelsStartingWithTheQueryComeFirst() {
        assertEquals(listOf("Settings: Providers", "Open settings"), labels("settings"))
    }

    @Test
    fun noMatchGivesNothing() {
        assertEquals(emptyList(), labels("zebra"))
    }

    @Test
    fun initialsAndTyposFallBackToInOrderLetters() {
        assertEquals("New chat", labels("nc").first())
        assertEquals(listOf("Chat: Trip notes for Kyoto"), labels("kyto"))
        assertNull(fuzzyMatchScore("Toggle sidebar", "zz"))
        val wordStarts = fuzzyMatchScore("Jump to prompt", "jtp")!!
        val scattered = fuzzyMatchScore("adjust the output", "jtp")!!
        assertTrue(wordStarts > scattered, "word starts outrank scattered letters")
    }

    private fun node(id: String, role: MessageRole, text: String) =
        MessageNode(id = id, role = role, variants = listOf(MessageVariant(id = "v-$id", content = text)))

    @Test
    fun promptJumpListsPromptsNewestFirstAndTheMapCanShowOnlyPrompts() {
        val messages = listOf(
            node("u1", MessageRole.USER, "Plan a\n\ntrip to   Kyoto"),
            node("a1", MessageRole.ASSISTANT, "Here is a Kyoto plan"),
            node("u2", MessageRole.USER, "   "),
            node("u3", MessageRole.USER, "Add temples")
        )
        assertEquals(
            listOf(PromptJumpTarget("u3", 3, "Add temples"), PromptJumpTarget("u1", 1, "Plan a trip to Kyoto")),
            promptJumpTargets(messages)
        )
        assertEquals(listOf(0, 1), filterMessageMapTurns(messages, "kyoto", promptsOnly = false).map { it.first })
        assertEquals(listOf(0), filterMessageMapTurns(messages, "kyoto", promptsOnly = true).map { it.first })
        assertEquals(listOf(0, 2, 3), filterMessageMapTurns(messages, "", promptsOnly = true).map { it.first })
    }
}
