package io.parchment.ai.network

import io.parchment.ai.harness.*
import io.parchment.ai.model.*
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.*
import kotlin.test.*

class HistoryReplayContractTest {
    private val session = ChatSession(id = "s", title = "t", createdAt = 1L, updatedAt = 2L, profileId = "p")

    private fun user(text: String = "hi", attachments: List<PromptAttachment> = emptyList()) = MessageNode("u-$text", MessageRole.USER, listOf(MessageVariant(id = "vu-$text", content = text, attachments = attachments)))

    private fun history(variant: MessageVariant) = listOf(
        user(),
        MessageNode("a", MessageRole.ASSISTANT, listOf(variant)),
        user("next")
    )

    private fun tool(id: String, thought: String = "", signature: String = "", startsStep: Boolean = false) = ToolCallRecord(
        id = id,
        name = "fs_read",
        argumentsJson = "{}",
        resultText = "ok",
        precedingThinking = thought,
        thoughtSignature = signature,
        startsToolStep = startsStep
    )

    private fun attachment(name: String, mime: String) = PromptAttachment(id = name, fileName = name, mimeType = mime, base64Data = "AAAA")

    private fun JsonElement.field(name: String) = jsonObject[name]?.jsonPrimitive?.content

    private fun chatReasoning(messages: JsonArray) = messages.mapNotNull { it.field("reasoning_content") }

    @Test fun paddedEncryptedReasoningIsReplayedButGluedTokensAreNot() {
        val token = "gAAAAABtoken-value_123=="
        val items = AiProviderEngine.buildOpenAiResponsesInputArray(
            history(
                MessageVariant(
                    id = "v",
                    content = "done",
                    providerProtocol = ProviderProtocol.OPENAI_RESPONSES,
                    toolCalls = listOf(tool("call_1", signature = token))
                )
            ),
            session,
            ""
        )
        assertEquals(token, items.single { it.field("type") == "reasoning" }.field("encrypted_content"))
        assertFalse(AiProviderEngine.isValidOpenAiEncryptedReasoning("gAAAAABone==gAAAAABtwo=="))
    }

    @Test fun chatReasoningStaysWithItsOwnWireFamily() {
        val turn = MessageVariant(
            id = "v",
            content = "closing",
            trailingThinking = "before closing",
            toolCalls = listOf(tool("call_1", thought = "before call"))
        )
        val own = AiProviderEngine.buildOpenAiChatCompletionsMessages(
            history(turn.copy(providerProtocol = ProviderProtocol.OPENAI_RESPONSES)),
            session,
            "",
            ""
        )
        assertEquals(listOf("before call", "before closing"), chatReasoning(own))
        val foreign = AiProviderEngine.buildOpenAiChatCompletionsMessages(
            history(turn.copy(providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES)),
            session,
            "",
            ""
        )
        assertEquals(emptyList(), chatReasoning(foreign))
    }

    @Test fun foldedChatHistoryOmitsReasoningOnPlainTurns() {
        val turn = MessageVariant(id = "v", content = "answer", thinkingSummary = "why", providerProtocol = ProviderProtocol.OPENAI_RESPONSES)
        assertEquals(listOf("why"), chatReasoning(AiProviderEngine.buildOpenAiChatCompletionsMessages(history(turn), session, "", "")))
        val folded = AiProviderEngine.buildOpenAiChatCompletionsMessages(history(turn), session, "", "", foldHistoricalToolsToText = true)
        assertEquals(emptyList(), chatReasoning(folded))
    }

    @Test fun geminiCarriesForeignReasoningOnlyInTheHandoffNote() {
        val handedOff = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(
                MessageVariant(
                    id = "v",
                    content = "answer",
                    thinkingSummary = "foreign thoughts",
                    providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES
                )
            ),
            ProviderProtocol.GEMINI_INTERACTIONS,
            chainingActive = false
        )
        val wire = JsonArray(AiProviderEngine.buildGeminiGenerateContentContents(handedOff, session, "")).toString()
        assertEquals(2, wire.split("foreign thoughts").size, wire)
        assertFalse("Completed internal reasoning" in wire, wire)

        val withTools = JsonArray(
            AiProviderEngine.buildGeminiGenerateContentContents(
                history(
                    MessageVariant(
                        id = "v",
                        content = "answer",
                        providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
                        toolCalls = listOf(tool("call_1", thought = "foreign step"))
                    )
                ),
                session,
                ""
            )
        ).toString()
        assertFalse("foreign step" in withTools, withTools)
    }

    @Test fun toolIdsFitEveryWireAndStayPairedAndUnique() {
        val longId = "x".repeat(64)
        val turn = MessageVariant(
            id = "v",
            content = "done",
            toolCalls = listOf(
                tool("functions.fs_read:0"),
                tool(""),
                tool(""),
                tool(longId)
            )
        )
        val anthropic = AiProviderEngine.buildAnthropicBaseMessages(history(turn), session, "").flatMap { it.second }
        val useIds = anthropic.filter { it.field("type") == "tool_use" }.map { it.field("id").orEmpty() }
        val resultIds = anthropic.filter { it.field("type") == "tool_result" }.map { it.field("tool_use_id").orEmpty() }
        assertEquals(useIds, resultIds)
        assertEquals(4, useIds.toSet().size)
        assertTrue(useIds.all { Regex("^[a-zA-Z0-9_-]{1,64}$").matches(it) }, useIds.toString())
        assertEquals(longId, useIds.last(), "IDs that already fit are left alone")

        val chat = AiProviderEngine.buildOpenAiChatCompletionsMessages(history(turn), session, "", "")
        val chatIds = chat.flatMap { it.jsonObject["tool_calls"]?.jsonArray.orEmpty() }.map { it.field("id").orEmpty() }
        assertEquals(chat.mapNotNull { it.field("tool_call_id") }, chatIds)
        assertTrue(chatIds.all { it.length <= 40 }, chatIds.toString())
        assertEquals(
            chatIds,
            AiProviderEngine.buildOpenAiChatCompletionsMessages(history(turn), session, "", "")
                .flatMap { it.jsonObject["tool_calls"]?.jsonArray.orEmpty() }.map { it.field("id").orEmpty() },
            "IDs stay stable across requests"
        )
    }

    @Test fun sequentialStepsWithoutThinkingStayApartFromParallelCalls() {
        val turn = MessageVariant(
            id = "v",
            content = "done",
            toolCalls = listOf(
                tool("a", startsStep = true),
                tool("b"),
                tool("c", startsStep = true)
            )
        )
        val roles = AiProviderEngine.buildOpenAiChatCompletionsMessages(history(turn), session, "", "").map { it.field("role") }
        assertEquals(listOf("user", "assistant", "tool", "tool", "assistant", "tool", "assistant", "user"), roles)
    }

    @Test fun onlyTheFirstCallOfAStepIsMarked() {
        val seen = mutableListOf<ToolCallRecord>()
        runBlocking {
            AiProviderEngine.executeWorkspaceToolBatchWithLiveProgress(
                workspaceRoot = "", mode = WorkspaceMode.OFF, allowShell = false,
                accessScope = ToolAccessScope(AgentPermissionPolicy()),
                calls = listOf(Triple("a", "missing_tool", "{}"), Triple("b", "missing_tool", "{}")),
                circuitBreaker = WorkspaceHarness.LoopCircuitBreaker(),
                onStatus = {}, onToolCall = { seen += it }, onGoalProgress = {}
            )
        }
        assertEquals(mapOf("a" to true, "b" to false), seen.associate { it.id to it.startsToolStep })
    }

    @Test fun attachmentsUseTheTypeEachWireAccepts() {
        val nodes = listOf(
            user(
                attachments = listOf(
                    attachment("a.pdf", "application/pdf"),
                    attachment("b.mp4", "video/mp4"),
                    attachment("c.png", "image/png")
                )
            )
        )
        val anthropic = AiProviderEngine.buildAnthropicBaseMessages(nodes, session, "").single().second
        assertEquals(listOf("document", "text", "image", "text"), anthropic.map { it.field("type") })
        assertTrue(anthropic[1].field("text").orEmpty().contains("b.mp4"))

        val chat = AiProviderEngine.buildOpenAiChatCompletionsMessages(nodes, session, "", "").single().jsonObject["content"]!!.jsonArray
        assertEquals(listOf("text", "file", "text", "image_url"), chat.map { it.field("type") })

        val responses = AiProviderEngine.buildOpenAiResponsesInputArray(nodes, session, "").single().jsonObject["content"]!!.jsonArray
        assertEquals(listOf("input_text", "input_file", "input_text", "input_image"), responses.map { it.field("type") })
    }

    @Test fun chainsAreReusedOnlyByTheirOwnProtocolAndModel() {
        val previous = MessageVariant(
            id = "v",
            content = "x",
            providerInteractionId = "resp_1",
            providerProtocol = ProviderProtocol.OPENAI_RESPONSES,
            modelUsed = "model-a"
        )
        assertEquals("resp_1", AiProviderEngine.reusableProviderChainId(previous, ProviderProtocol.OPENAI_RESPONSES, "model-a"))
        assertEquals("", AiProviderEngine.reusableProviderChainId(previous, ProviderProtocol.OPENAI_RESPONSES, "model-b"))
        assertEquals("", AiProviderEngine.reusableProviderChainId(previous, ProviderProtocol.GEMINI_INTERACTIONS, "model-a"))
    }

    @Test fun interactionsReplayKeepsCallAndResultToolNamesAlignedWhenBlank() {
        val turn = MessageVariant(id = "v", content = "done", toolCalls = listOf(ToolCallRecord(id = "c1", name = "", argumentsJson = "{}", resultText = "ok")))
        val built = AiProviderEngine.buildGeminiInteractionsTurnsArray(history(turn), session, "").map { it.jsonObject }
        val callName = built.first { it.field("type") == "function_call" }.field("name")
        val resultName = built.first { it.field("type") == "function_result" }.field("name")
        assertEquals("tool", callName)
        assertEquals(callName, resultName)
    }

    @Test fun modelRegistrySurvivesUnexpectedNonPrimitiveTags() {
        val catalog = mutableMapOf<String, ModelPricingSpec>()
        ModelsDevClient.indexAimlApiCatalog(
            ModelsDevClient.catalogJson.decodeFromString(
                AimlApiCatalogDto.serializer(),
                """{"data":[{"id":"acme/video-gen","type":"video","tags":[{"nested":1},"video"]},{"id":"acme/good-chat","type":"chat","tags":["chat"]}]}"""
            ),
            catalog
        )
        assertNotNull(catalog["good-chat"])
        assertEquals(listOf("video"), catalog["video-gen"]?.outputModalities)
    }

    @Test fun reasoningHandoffRaisesCapTo8kWithHeadAndTail() {
        assertEquals(8_000, AiProviderEngine.MAX_REASONING_HANDOFF_CHARS)
        val head = "OPENING_GOAL: " + "a".repeat(2_500)
        val middle = "m".repeat(6_000)
        val tail = "b".repeat(2_500) + " :FINAL_CONCLUSION"
        val longSummary = "$head$middle$tail"

        val handedOff = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(
                MessageVariant(
                    id = "v",
                    content = "answer",
                    thinkingSummary = longSummary,
                    providerProtocol = ProviderProtocol.GEMINI_INTERACTIONS
                )
            ),
            ProviderProtocol.ANTHROPIC_MESSAGES,
            chainingActive = false
        )
        val content = handedOff[1].activeVariant.content
        val clippedSummary = content
            .removePrefix("${AiProviderEngine.REASONING_HANDOFF_MARKER}\n")
            .removeSuffix("\n\nanswer")
        assertTrue(clippedSummary.length in 7_500..AiProviderEngine.MAX_REASONING_HANDOFF_CHARS)
        assertTrue(clippedSummary.startsWith("OPENING_GOAL: "), clippedSummary.take(40))
        assertTrue(clippedSummary.contains("[...middle reasoning omitted...]"))
        assertTrue(clippedSummary.endsWith(":FINAL_CONCLUSION"), clippedSummary.takeLast(40))

        val midSized = "m".repeat(5_000)
        val midHandedOff = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(
                MessageVariant(
                    id = "v2",
                    content = "ok",
                    thinkingSummary = midSized,
                    providerProtocol = ProviderProtocol.GEMINI_INTERACTIONS
                )
            ),
            ProviderProtocol.ANTHROPIC_MESSAGES,
            chainingActive = false
        )
        assertTrue(midHandedOff[1].activeVariant.content.contains(midSized))
    }

    @Test fun crossProviderHandoffPreservesToolStepOrderAndAvoidsDoubleReasoning() {
        val responsesToolTurn = MessageVariant(
            id = "v-resp",
            content = "Reading config. Done.",
            thinkingSummary = "Check config first.",
            trailingThinking = "Now wrap up.",
            thinkingSignature = "gAAAAABenc_token==",
            toolCalls = listOf(ToolCallRecord("c1", "fs_read", "{}", resultText = "cfg", precedingText = "Reading config. ")),
            providerProtocol = ProviderProtocol.OPENAI_RESPONSES,
            providerWireDialect = CUSTOM_DIALECT_OPENAI_RESPONSES,
            modelUsed = "gpt-5"
        )
        val forChat = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(responsesToolTurn),
            currentProtocol = ProviderProtocol.OPENAI_RESPONSES,
            chainingActive = false,
            currentWireDialect = CUSTOM_DIALECT_OPENAI_CHAT,
            currentModel = "deepseek-r1"
        )
        val chatMessages = AiProviderEngine.buildOpenAiChatCompletionsMessages(
            activeNodes = forChat,
            session = session,
            systemInstruction = "",
            continuePrefix = "",
            currentModel = "deepseek-r1"
        )
        val firstAsst = chatMessages.first { it.field("role") == "assistant" }.jsonObject
        val firstContent = firstAsst["content"]?.jsonPrimitive?.content.orEmpty()
        assertTrue(firstContent.startsWith(AiProviderEngine.REASONING_HANDOFF_MARKER), firstContent)
        assertTrue(firstContent.contains("Check config first."), firstContent)
        assertTrue(firstContent.endsWith("Reading config. "), firstContent)
        assertNotNull(firstAsst["tool_calls"])
        assertEquals(emptyList(), chatReasoning(chatMessages), "Foreign Responses turn must not duplicate reasoning_content")

        val finalAsst = chatMessages.last { it.field("role") == "assistant" }.jsonObject
        assertEquals("Done.", finalAsst["content"]?.jsonPrimitive?.content)

        // Reverse direction: Custom OpenAI Chat -> OpenAI Responses also hands off text & drops foreign-model blobs.
        val chatTurn = responsesToolTurn.copy(
            providerWireDialect = CUSTOM_DIALECT_OPENAI_CHAT,
            thinkingSignature = "",
            modelUsed = "gpt-chat"
        )
        val forResponses = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(chatTurn),
            currentProtocol = ProviderProtocol.OPENAI_RESPONSES,
            chainingActive = false,
            currentWireDialect = CUSTOM_DIALECT_OPENAI_RESPONSES,
            currentModel = "gpt-5"
        )
        assertTrue(forResponses[1].activeVariant.content.contains(AiProviderEngine.REASONING_HANDOFF_MARKER))
    }

    @Test fun foldedHistoryAndNudgePassDoNotDuplicateReasoning() {
        val foreignTurn = MessageVariant(
            id = "v-anth",
            content = "Done.",
            thinkingSummary = "Think deeply first.",
            toolCalls = listOf(tool("c1")),
            providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
            providerWireDialect = CUSTOM_DIALECT_ANTHROPIC
        )
        val handedOff = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(foreignTurn),
            ProviderProtocol.OPENAI_RESPONSES,
            chainingActive = false,
            currentWireDialect = CUSTOM_DIALECT_OPENAI_CHAT
        )
        val foldedWire = AiProviderEngine.buildOpenAiChatCompletionsMessages(
            activeNodes = handedOff,
            session = session,
            systemInstruction = "",
            continuePrefix = "",
            foldHistoricalToolsToText = true
        ).toString()
        assertTrue(AiProviderEngine.REASONING_HANDOFF_MARKER in foldedWire, foldedWire)
        assertFalse("[Completed reasoning:" in foldedWire, foldedWire)

        val (nudgeNodes, _) = AiProviderEngine.buildEffectiveHistoryAndPrefix(
            baseHistoryNodes = listOf(user()),
            continuePrefix = "",
            thinkingText = "Pure thinking narrative",
            signatureText = "sig_gem_123",
            executedTools = emptyList(),
            generatedMedia = emptyList(),
            protocol = ProviderProtocol.GEMINI_INTERACTIONS,
            model = "gemini-pro",
            passTag = "nudge-2",
            forceVisibleThoughtSummary = true,
            wireDialect = CUSTOM_DIALECT_GEMINI
        )
        val geminiWire = AiProviderEngine.buildGeminiInteractionsTurnsArray(nudgeNodes, session, "").map { it.jsonObject }
        assertEquals(0, geminiWire.count { it.field("type") == "thought" }, geminiWire.toString())
        assertTrue(
            geminiWire.any { step ->
                step["content"]?.jsonArray?.any { part ->
                    part.field("text") == "[Completed internal reasoning:\nPure thinking narrative]"
                } == true
            },
            geminiWire.toString()
        )
    }

    @Test fun sameProtocolMissingSignatureToolTurnAndBlankModelChainGuards() {
        val blankModelChain = MessageVariant(
            id = "v-blank",
            content = "x",
            providerInteractionId = "int_blank",
            providerProtocol = ProviderProtocol.GEMINI_INTERACTIONS,
            providerWireDialect = CUSTOM_DIALECT_GEMINI,
            modelUsed = ""
        )
        assertEquals(
            "",
            AiProviderEngine.reusableProviderChainId(blankModelChain, ProviderProtocol.GEMINI_INTERACTIONS, "gemini-pro", CUSTOM_DIALECT_GEMINI)
        )

        val unsignedAnthropicTool = MessageVariant(
            id = "v-unsig",
            content = "Result ok.",
            thinkingSummary = "Reasoned before tool without signature.",
            thinkingSignature = "",
            toolCalls = listOf(tool("c1")),
            providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
            providerWireDialect = CUSTOM_DIALECT_ANTHROPIC,
            modelUsed = "claude-sonnet"
        )
        val repaired = AiProviderEngine.applyCrossProviderReasoningHandoff(
            history(unsignedAnthropicTool),
            currentProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
            chainingActive = false,
            currentWireDialect = CUSTOM_DIALECT_ANTHROPIC,
            currentModel = "claude-sonnet"
        )
        assertTrue(repaired[1].activeVariant.content.contains("Reasoned before tool without signature."))
    }

    @Test fun multiBatchAnthropicToolTurnKeepsSignedThinkingWhenTextIsOmitted() {
        val turn = MessageVariant(
            id = "v-omitted",
            content = "Done.",
            providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
            providerWireDialect = CUSTOM_DIALECT_ANTHROPIC,
            modelUsed = "claude-opus",
            toolCalls = listOf(
                ToolCallRecord("c1", "fs_read", "{}", resultText = "a", thoughtSignature = "sig_batch_1", startsToolStep = true),
                ToolCallRecord("c2", "fs_read", "{}", resultText = "b", thoughtSignature = "sig_batch_2", startsToolStep = true)
            )
        )
        val blocks = AiProviderEngine.buildAnthropicBaseMessages(
            history(turn),
            session,
            "",
            currentModel = "claude-opus"
        ).flatMap { it.second }
        val sigs = blocks.filter { it.field("type") == "thinking" }.map { it.field("signature") }
        assertEquals(listOf("sig_batch_1", "sig_batch_2"), sigs)
    }
}
