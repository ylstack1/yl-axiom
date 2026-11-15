package io.parchment.ai.network

import com.sun.net.httpserver.HttpServer
import io.ktor.utils.io.ByteReadChannel
import io.parchment.ai.model.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.*
import java.net.InetSocketAddress
import java.util.Collections
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.*

class StreamRecoveryHttpTest {
    private data class Reply(
        val body: String,
        val status: Int = 200,
        val contentType: String = "text/event-stream"
    )

    private class Server(private val replies: List<Reply>) : AutoCloseable {
        private val next = AtomicInteger()
        val requests: MutableList<String> = Collections.synchronizedList(mutableListOf())
        val requestPaths: MutableList<String> = Collections.synchronizedList(mutableListOf())
        val requestHeaders: MutableList<Map<String, List<String>>> = Collections.synchronizedList(mutableListOf())
        private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            createContext("/") { exchange ->
                requestPaths.add(exchange.requestURI.rawPath + (exchange.requestURI.rawQuery?.let { "?$it" } ?: ""))
                requestHeaders.add(exchange.requestHeaders.entries.associate { (k, v) -> k.lowercase() to v.toList() })
                requests.add(exchange.requestBody.bufferedReader().use { it.readText() })
                val reply = replies[next.getAndIncrement().coerceAtMost(replies.lastIndex)]
                val bytes = reply.body.toByteArray()
                exchange.responseHeaders.set("Content-Type", reply.contentType)
                exchange.sendResponseHeaders(reply.status, bytes.size.toLong())
                exchange.responseBody.use { it.write(bytes) }
            }
            start()
        }
        val baseUrl get() = "http://127.0.0.1:${server.address.port}"
        override fun close() = server.stop(0)
    }

    private fun sse(vararg events: String) = events.joinToString("") { "data: $it\n\n" }

    private fun profile(server: Server, dialect: String) = ProviderProfile(
        "fixture", "Fixture", ProviderProtocol.CUSTOM, customDialect = dialect, baseUrl = server.baseUrl,
        defaultModel = "fixture-model", thinkingEffort = "none", customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
    )

    private fun updates(profile: ProviderProfile) = AiProviderEngine.streamGeneration(
        profile = profile,
        session = ChatSession("chat", "Fixture", 1L, 1L, profile.id),
        historyNodes = listOf(MessageNode("user", MessageRole.USER, listOf(MessageVariant("variant", content = "Hello"))))
    )

    /** Stops at the first retry notice so a test never sits through the real retry wait. */
    private fun outcome(profile: ProviderProfile): StreamUpdate = runBlocking {
        withTimeout(20_000) {
            updates(profile).first { it is StreamUpdate.Completed || (it is StreamUpdate.Status && "Auto-retrying (" in it.message) }
        }
    }

    private fun assertResumes(update: StreamUpdate, reason: String = "Stream ended before") {
        val status = assertIs<StreamUpdate.Status>(update, "An unfinished stream must not be accepted as the answer")
        assertTrue(reason in status.message, status.message)
    }

    private val chatText = """{"choices":[{"delta":{"content":"partial"}}]}"""
    private val responsesText = """{"type":"response.output_text.delta","delta":"partial"}"""
    private val anthropicText = """{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"partial"}}"""
    private val geminiStatus = """{"event_type":"interaction.status_update","interaction_id":"i"}"""
    private val generateText = """{"candidates":[{"content":{"parts":[{"text":"partial"}]}}]}"""

    private val cutStreams = mapOf(
        CUSTOM_DIALECT_OPENAI_CHAT to sse(chatText),
        CUSTOM_DIALECT_OPENAI_RESPONSES to sse(responsesText),
        CUSTOM_DIALECT_ANTHROPIC to sse(anthropicText),
        CUSTOM_DIALECT_GEMINI to sse(geminiStatus)
    )

    private val finishedStreams = mapOf(
        CUSTOM_DIALECT_OPENAI_CHAT to sse(chatText, """{"choices":[{"delta":{},"finish_reason":"stop"}]}"""),
        CUSTOM_DIALECT_OPENAI_RESPONSES to sse(responsesText, """{"type":"response.completed","response":{"id":"r","status":"completed","output":[]}}"""),
        CUSTOM_DIALECT_ANTHROPIC to sse(anthropicText, """{"type":"message_stop"}"""),
        CUSTOM_DIALECT_GEMINI to sse("""{"event_type":"interaction.complete","interaction":{"id":"i","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"partial"}]}]}}""")
    )

    @Test fun cutStreamsResumeOnEveryCustomWire() {
        for ((dialect, body) in cutStreams) Server(listOf(Reply(body))).use { server ->
            assertResumes(outcome(profile(server, dialect)))
        }
    }

    @Test fun finishedStreamsCompleteOnEveryCustomWire() {
        for ((dialect, body) in finishedStreams) Server(listOf(Reply(body))).use { server ->
            assertEquals("partial", assertIs<StreamUpdate.Completed>(outcome(profile(server, dialect)), dialect).fullContent)
        }
    }

    private fun geminiProfile(server: Server) = ProviderProfile(
        "gemini", "Gemini", ProviderProtocol.GEMINI_INTERACTIONS, baseUrl = server.baseUrl,
        defaultModel = "fixture-model", thinkingEffort = "none", customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
    )

    private fun steppedTurn(content: String = "Step one. Step two. Final answer.", phases: Boolean = false) = MessageNode("reply", MessageRole.ASSISTANT, listOf(MessageVariant(
        "reply-v", content = content, toolCalls = listOf(
            ToolCallRecord("c1", "fs_read", "{}", resultText = "result-a", precedingText = "Step one. ", messagePhases = phases),
            ToolCallRecord("c2", "fs_read", "{}", resultText = "result-b", precedingText = "Step two. ", messagePhases = phases)
        ))))

    private fun requestAfter(turn: MessageNode, profile: ProviderProfile, server: Server): String {
        runBlocking {
            withTimeout(20_000) {
                AiProviderEngine.streamGeneration(profile = profile, session = ChatSession("chat", "Fixture", 1L, 1L, profile.id), historyNodes = listOf(
                    MessageNode("first", MessageRole.USER, listOf(MessageVariant("first-v", content = "Start"))), turn,
                    MessageNode("next", MessageRole.USER, listOf(MessageVariant("next-v", content = "Carry on")))
                )).first { it is StreamUpdate.Completed }
            }
        }
        return server.requests.last()
    }

    private fun assertInOrder(body: String, wire: String) {
        val marks = listOf("Step one.", "result-a", "Step two.", "result-b", "Final answer.").map { body.indexOf(it) }
        assertTrue(marks.none { it < 0 } && marks == marks.sorted(), "$wire replayed the turn out of order: $marks in $body")
    }

    @Test fun textBetweenToolsReplaysInOrderOnEveryWire() {
        for ((dialect, body) in finishedStreams) Server(listOf(Reply(body))).use { server ->
            assertInOrder(requestAfter(steppedTurn(), profile(server, dialect), server), dialect)
        }
        val missing = Reply("""{"error":{"message":"missing"}}""", 404)
        val done = """{"candidates":[{"content":{"parts":[{"text":"partial"}]},"finishReason":"STOP"}]}"""
        Server(listOf(missing, Reply(sse(done)))).use { server ->
            assertInOrder(requestAfter(steppedTurn(), geminiProfile(server), server), "generateContent")
        }
    }

    @Test fun responsesReplayKeepsMessagePhasesOnlyWhenTheProviderSentThem() {
        val body = finishedStreams.getValue(CUSTOM_DIALECT_OPENAI_RESPONSES)
        Server(listOf(Reply(body))).use { server ->
            val sent = requestAfter(steppedTurn(phases = true), profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES), server)
            assertEquals(2, Regex(""""phase":"commentary"""").findAll(sent).count(), sent)
            assertEquals(1, Regex(""""phase":"final_answer"""").findAll(sent).count(), sent)
        }
        Server(listOf(Reply(body))).use { server ->
            assertFalse("phase" in requestAfter(steppedTurn(), profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES), server))
        }
    }

    @Test fun aReplyEditedAfterTheRunIsSentAsWrittenWithoutStaleSteps() {
        Server(listOf(Reply(finishedStreams.getValue(CUSTOM_DIALECT_OPENAI_RESPONSES)))).use { server ->
            val sent = requestAfter(steppedTurn(content = "My own rewrite."), profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES), server)
            assertTrue("My own rewrite." in sent)
            assertFalse("Step one." in sent, sent)
        }
    }

    @Test fun phasesOnStreamedMessagesTravelWithTheToolThatFollows() {
        val added = """{"type":"response.output_item.added","item":{"type":"message","phase":"commentary","content":[]}}"""
        val text = """{"type":"response.output_text.delta","delta":"Checking."}"""
        val call = """{"type":"response.output_item.done","item":{"type":"function_call","id":"f1","call_id":"c1","name":"fs_read","arguments":"{}","status":"completed"}}"""
        val done = """{"type":"response.completed","response":{"id":"r","status":"completed","output":[]}}"""
        Server(listOf(Reply(sse(added, text, call, done)), Reply(finishedStreams.getValue(CUSTOM_DIALECT_OPENAI_RESPONSES)))).use { server ->
            val fixture = profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES)
            val updates = runBlocking { withTimeout(20_000) { updates(fixture).toList() } }
            assertTrue(updates.filterIsInstance<StreamUpdate.ToolCallEvent>().all { it.toolCall.messagePhases })
            assertTrue(""""phase":"commentary"""" in server.requests[1], server.requests[1])
        }
    }

    @Test fun generateContentNeedsAFinishReason() {
        val missing = Reply("""{"error":{"message":"missing"}}""", 404)
        Server(listOf(missing, Reply(sse(generateText)))).use { assertResumes(outcome(geminiProfile(it))) }
        val done = """{"candidates":[{"content":{"parts":[{"text":"partial"}]},"finishReason":"STOP"}]}"""
        Server(listOf(missing, Reply(sse(done)))).use { assertIs<StreamUpdate.Completed>(outcome(geminiProfile(it))) }
    }

    @Test fun doneMarkerAloneFinishesAStream() = Server(listOf(Reply(sse(chatText, "[DONE]")))).use { server ->
        assertIs<StreamUpdate.Completed>(outcome(profile(server, CUSTOM_DIALECT_OPENAI_CHAT)))
        Unit
    }

    @Test fun responsesFailureEventIsAnError() {
        val failed = """{"type":"response.failed","response":{"error":{"message":"server melted"}}}"""
        Server(listOf(Reply(sse(responsesText, failed)))).use { server ->
            assertResumes(outcome(profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES)), "server melted")
        }
    }

    @Test fun customProfilesNeverSwitchWireAfterAFailure() {
        for (dialect in listOf(CUSTOM_DIALECT_OPENAI_RESPONSES, CUSTOM_DIALECT_GEMINI)) {
            Server(listOf(Reply("""{"error":{"message":"missing"}}""", 404))).use { server ->
                runBlocking { withTimeout(20_000) { runCatching { updates(profile(server, dialect)).toList() } } }
                assertEquals(1, server.requests.size, "$dialect must not try another endpoint: ${server.requests}")
            }
        }
    }

    @Test fun anAcceptedVideoJobIsNeverSubmittedTwice() {
        Server(listOf(Reply("""{"id":"task-1","status":"queued"}"""), Reply("down", 500))).use { server ->
            val video = ProviderProfile(
                "video", "Video", ProviderProtocol.OPENAI_RESPONSES, baseUrl = server.baseUrl, endpointPathOverride = "/v1/videos/generations",
                defaultModel = "fixture-model", thinkingEffort = "none", customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
            )
            val updates = runBlocking { withTimeout(40_000) { runCatching { updates(video).toList() } } }
            assertEquals(1, server.requests.count { it.isNotEmpty() }, "The job must be created once")
            assertEquals(6, server.requests.size, "One create and five status checks")
            assertFalse(updates.getOrDefault(emptyList()).any { it is StreamUpdate.Status && "Auto-retrying (" in it.message })
        }
    }

    @Test fun newInstallsKeepNothingOnProviderServers() {
        assertTrue(io.parchment.ai.ui.createDefaultAppState().profiles.none { it.keepsProviderState })
    }

    @Test fun storageIsItsOwnChoiceAndChainingImpliesIt() {
        fun requestFor(change: (ProviderProfile) -> ProviderProfile, dialect: String) =
            Server(listOf(Reply(finishedStreams.getValue(dialect)))).use { server ->
                assertIs<StreamUpdate.Completed>(outcome(change(profile(server, dialect))))
                Json.parseToJsonElement(server.requests.single()).jsonObject
            }
        fun storeOn(dialect: String, change: (ProviderProfile) -> ProviderProfile) = requestFor(change, dialect)["store"]?.jsonPrimitive?.boolean

        assertEquals(false, storeOn(CUSTOM_DIALECT_OPENAI_RESPONSES) { it })
        assertEquals(true, storeOn(CUSTOM_DIALECT_OPENAI_RESPONSES) { it.copy(storeServerSide = true) })
        assertEquals(true, storeOn(CUSTOM_DIALECT_OPENAI_RESPONSES) { it.copy(useStatefulChaining = true) })
        assertEquals(false, storeOn(CUSTOM_DIALECT_GEMINI) { it })
        assertNull(storeOn(CUSTOM_DIALECT_GEMINI) { it.copy(storeServerSide = true) }, "Gemini stores unless told not to")
    }

    @Test fun storedGeminiInteractionsRecoverByPollingWithoutChaining() {
        val poll = """{"id":"i","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"partial"}]}]}"""
        Server(listOf(Reply(sse(geminiStatus)), Reply(poll))).use { server ->
            val completed = assertIs<StreamUpdate.Completed>(outcome(profile(server, CUSTOM_DIALECT_GEMINI).copy(storeServerSide = true)))
            assertEquals("partial", completed.fullContent)
            assertEquals(1, server.requests.count { it.isNotEmpty() }, "Recovery reads the interaction instead of sending it again")
        }
    }

    @Test fun onlyStoredGeminiInteractionsAreCancelled() = runBlocking {
        Server(listOf(Reply("{}"))).use { server ->
            AiProviderEngine.cancelBackgroundInteraction(profile(server, CUSTOM_DIALECT_GEMINI), "i")
            AiProviderEngine.cancelBackgroundInteraction(profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES).copy(storeServerSide = true), "i")
            assertEquals(0, server.requests.size)
            AiProviderEngine.cancelBackgroundInteraction(profile(server, CUSTOM_DIALECT_GEMINI).copy(storeServerSide = true), "i")
            assertEquals(1, server.requests.size)
        }
    }

    @Test fun anAnswerCutOffByTheOutputLimitSaysSoAndTheNoticeIsNotReplayed() {
        val cut = """{"choices":[{"delta":{"content":"partial"},"finish_reason":"length"}]}"""
        Server(listOf(Reply(sse(cut)))).use { server ->
            val completed = assertIs<StreamUpdate.Completed>(outcome(profile(server, CUSTOM_DIALECT_OPENAI_CHAT)))
            assertEquals("partial${AiProviderEngine.OUTPUT_LIMIT_NOTICE}", completed.fullContent)
            assertEquals("partial", AiProviderEngine.stripInlineErrorNotices(completed.fullContent))
        }
    }

    @Test fun aPausedAnthropicTurnContinuesWithItsServerBlocksIntact() {
        val search = """{"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"srv_1","name":"web_search","input":{}}}"""
        val query = """{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"x\"}"}}"""
        val paused = """{"type":"message_delta","delta":{"stop_reason":"pause_turn"}}"""
        val finished = """{"type":"message_stop"}"""
        Server(listOf(Reply(sse(search, query, paused, finished)), Reply(sse(anthropicText, finished)))).use { server ->
            assertEquals("partial", assertIs<StreamUpdate.Completed>(outcome(profile(server, CUSTOM_DIALECT_ANTHROPIC))).fullContent)
            val replayed = Json.parseToJsonElement(server.requests[1]).jsonObject["messages"]!!.jsonArray.last().jsonObject
            assertEquals("assistant", replayed["role"]!!.jsonPrimitive.content)
            val block = replayed["content"]!!.jsonArray.single().jsonObject
            assertEquals("server_tool_use", block["type"]!!.jsonPrimitive.content)
            assertEquals("x", block["input"]!!.jsonObject["query"]!!.jsonPrimitive.content)
        }
    }

    @Test fun anthropicToolStepsMarkTheNewestTurnForCaching() {
        val call = """{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"fs_read","input":{"path":"x"}}}"""
        Server(listOf(Reply(sse(call, """{"type":"message_stop"}""")), Reply(sse(anthropicText, """{"type":"message_stop"}""")))).use { server ->
            assertIs<StreamUpdate.Completed>(outcome(profile(server, CUSTOM_DIALECT_ANTHROPIC)))
            val last = Json.parseToJsonElement(server.requests[1]).jsonObject["messages"]!!.jsonArray.last().jsonObject
            assertNotNull(last["content"]!!.jsonArray.last().jsonObject["cache_control"], "The newest tool result must carry the marker")
        }
    }

    private fun videoProfile(server: Server) = ProviderProfile(
        "video", "Video", ProviderProtocol.OPENAI_RESPONSES, baseUrl = server.baseUrl, endpointPathOverride = "/v1/videos/generations",
        defaultModel = "fixture-model", thinkingEffort = "none", customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
    )

    private fun runVideo(server: Server, resumeTask: String) = runBlocking {
        val video = videoProfile(server)
        withTimeout(30_000) {
            runCatching {
                AiProviderEngine.streamGeneration(
                    profile = video, session = ChatSession("chat", "Fixture", 1L, 1L, video.id),
                    historyNodes = listOf(MessageNode("user", MessageRole.USER, listOf(MessageVariant("variant", content = "A short clip")))),
                    resumeMediaTask = resumeTask
                ).toList()
            }
        }
    }

    @Test fun anAcceptedVideoJobResumesInsteadOfBeingCreatedAgain() {
        val finished = """{"id":"task-1","status":"succeeded","video_url":"http://127.0.0.1:1/x.mp4"}"""
        Server(listOf(Reply(finished))).use { server ->
            runVideo(server, "video-task:0:task-1")
            assertEquals(0, server.requests.count { it.isNotEmpty() }, "Resuming must not create a job")
            assertTrue(server.requests.isNotEmpty(), "It must check the existing job")
        }
    }

    @Test fun aSavedVideoTaskCannotPointAnywhereElse() {
        val created = """{"id":"fresh","status":"succeeded","video_url":"http://127.0.0.1:1/x.mp4"}"""
        listOf("video-task:9:task-1", "video-task:0:../../evil", "https://evil.example/task").forEach { forged ->
            Server(listOf(Reply(created))).use { server ->
                runVideo(server, forged)
                assertEquals(1, server.requests.count { it.isNotEmpty() }, "$forged must be ignored")
            }
        }
    }

    private fun limited(server: Server, before: Double, limit: Double) = runBlocking {
        val priced = ModelPricingSpec("fixture-model", inputPricePerM = 1000.0, outputPricePerM = 1000.0)
        val fixture = profile(server, CUSTOM_DIALECT_OPENAI_CHAT).copy(customModelSpecs = mapOf("fixture-model" to priced))
        withTimeout(20_000) {
            AiProviderEngine.streamGeneration(
                profile = fixture, session = ChatSession("chat", "Fixture", 1L, 1L, fixture.id),
                historyNodes = listOf(MessageNode("user", MessageRole.USER, listOf(MessageVariant("variant", content = "Hello")))),
                spendLimitUsd = limit, spendBeforeRunUsd = before
            ).toList()
        }
    }

    @Test fun aChatAtItsSpendLimitSendsNothing() {
        Server(listOf(Reply(sse(chatText, "[DONE]")))).use { server ->
            val updates = limited(server, before = 1.0, limit = 1.0)
            assertEquals(0, server.requests.size)
            assertTrue("spend limit" in updates.filterIsInstance<StreamUpdate.Error>().single().message)
        }
    }

    @Test fun aRunStopsBetweenStepsOnceItCrossesTheLimit() {
        val step = """{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"fs_read","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1000,"completion_tokens":1000}}"""
        Server(listOf(Reply(sse(step, "[DONE]")))).use { server ->
            val updates = limited(server, before = 0.0, limit = 1.0)
            assertEquals(1, server.requests.size, "The next step must not be sent")
            assertTrue("spend limit" in updates.filterIsInstance<StreamUpdate.Error>().single().message)
        }
    }

    @Test fun chatToolCallsWithoutAnIndexStaySeparate() {
        val first = """{"choices":[{"delta":{"tool_calls":[{"id":"a","function":{"name":"fs_list","arguments":"{\"pa"}}]}}]}"""
        val rest = """{"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"th\":\"x\"}"}}]}}]}"""
        val second = """{"choices":[{"delta":{"tool_calls":[{"id":"b","function":{"name":"fs_read","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"""
        val answer = """{"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"""
        Server(listOf(Reply(sse(first, rest, second)), Reply(sse(answer)))).use { server ->
            val completed = runBlocking { withTimeout(20_000) { updates(profile(server, CUSTOM_DIALECT_OPENAI_CHAT)).toList() } }
                .filterIsInstance<StreamUpdate.Completed>().single()
            assertEquals(listOf("a" to "fs_list", "b" to "fs_read"), completed.toolCalls.map { it.id to it.name })
            assertEquals("""{"path":"x"}""", completed.toolCalls.first().argumentsJson)
        }
    }

    @Test fun jsonSplitAcrossDataLinesIsDeliveredWhole() = runBlocking {
        val seen = mutableListOf<String>()
        AiProviderEngine.readSseOrJsonFallback(
            channel = ByteReadChannel("data: {\"a\":{\"b\":1}\ndata: ,\"c\":\"}\"}\n\n".toByteArray()),
            streamFinished = { true },
            onSseEvent = { _, data -> seen += data },
            onPlainJsonFallback = {}
        )
        assertEquals(1, Json.parseToJsonElement(seen.single().replace("\n", "")).jsonObject["a"]!!.jsonObject["b"]!!.jsonPrimitive.int)
    }

    private fun runUntilTerminalOrRetry(profile: ProviderProfile, history: List<MessageNode>? = null): StreamUpdate = runBlocking {
        withTimeout(20_000) {
            AiProviderEngine.streamGeneration(
                profile = profile,
                session = ChatSession("chat", "Fixture", 1L, 1L, profile.id),
                historyNodes = history ?: listOf(MessageNode("user", MessageRole.USER, listOf(MessageVariant("variant", content = "Hello"))))
            ).first {
                it is StreamUpdate.Completed ||
                    it is StreamUpdate.Error ||
                    (it is StreamUpdate.Status && "Auto-retrying (" in it.message)
            }
        }
    }

    @Test fun geminiInteractionsSendsBackgroundOnlyWithStorageAndStripsOn400() {
        val complete = sse("""{"event_type":"interaction.complete","interaction":{"id":"i1","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"partial"}]}]}}""")
        Server(listOf(Reply(complete))).use { server ->
            val defaultProfile = geminiProfile(server).copy(useBackgroundLongDecode = true)
            assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(defaultProfile))
            val req = Json.parseToJsonElement(server.requests.single()).jsonObject
            assertEquals(false, req["store"]?.jsonPrimitive?.booleanOrNull)
            assertNull(req["background"], "Default private profile must not send background=true when store=false")
        }

        val rejected = Reply("""{"error":{"message":"Invalid field: background is not supported"}}""", status = 400, contentType = "application/json")
        Server(listOf(rejected, Reply(complete))).use { server ->
            val stored = geminiProfile(server).copy(storeServerSide = true, useBackgroundLongDecode = true)
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(stored))
            assertEquals("partial", done.fullContent)
            assertEquals(2, server.requests.size)
            val firstReq = Json.parseToJsonElement(server.requests[0]).jsonObject
            val secondReq = Json.parseToJsonElement(server.requests[1]).jsonObject
            assertNull(firstReq["store"], "Gemini omits store when storage is enabled")
            assertEquals(true, firstReq["background"]?.jsonPrimitive?.booleanOrNull)
            assertNull(secondReq["background"], "HTTP 400 fallback must retry without background=true")
        }
    }

    @Test fun geminiPreTokenFailureDoesNotPollPriorTurnId() {
        val cutWithoutCurrentId = sse("""{"event_type":"interaction.status_update","status":"in_progress"}""")
        Server(listOf(Reply(cutWithoutCurrentId))).use { server ->
            val stored = geminiProfile(server).copy(storeServerSide = true, useStatefulChaining = true)
            val history = listOf(
                MessageNode("u1", MessageRole.USER, listOf(MessageVariant("uv1", content = "Hi"))),
                MessageNode("a1", MessageRole.ASSISTANT, listOf(MessageVariant("av1", content = "Earlier", providerInteractionId = "prior-turn-id"))),
                MessageNode("u2", MessageRole.USER, listOf(MessageVariant("uv2", content = "Next")))
            )
            assertResumes(runUntilTerminalOrRetry(stored, history))
            assertTrue(server.requestPaths.none { "prior-turn-id" in it }, "Must not poll prior turn id: ${server.requestPaths}")
        }
    }

    @Test fun geminiRequiresContinuationHopsWithoutStatefulChaining() {
        val hop1 = sse("""{"event_type":"interaction.complete","interaction":{"id":"i1","status":"requires_continuation","continuation_token":"tok-1","steps":[{"type":"thought","content":[{"type":"thought","summary":[{"type":"text","text":"First step"}]}]}]}}""")
        val hop2 = sse("""{"event_type":"interaction.complete","interaction":{"id":"i2","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"Final part"}]}]}}""")
        Server(listOf(Reply(hop1), Reply(hop2))).use { server ->
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(geminiProfile(server)))
            assertEquals("Final part", done.fullContent)
            assertEquals(2, server.requests.size)
            val req2 = Json.parseToJsonElement(server.requests[1]).jsonObject
            assertEquals("i1", req2["previous_interaction_id"]?.jsonPrimitive?.contentOrNull)
            assertEquals("tok-1", req2["continuation_token"]?.jsonPrimitive?.contentOrNull)
        }
    }

    @Test fun geminiPartialStreamMergesPollSuffixWithoutDuplication() {
        val partialSse = sse(
            """{"event_type":"interaction.start","interaction":{"id":"int-99","status":"in_progress"}}""",
            """{"event_type":"content.delta","index":0,"delta":{"type":"text","text":"Hello "}}"""
        )
        val pollDone = """{"id":"int-99","status":"completed","outputs":[{"type":"text","text":"Hello world"}]}"""
        Server(listOf(Reply(partialSse), Reply(pollDone, contentType = "application/json"))).use { server ->
            val stored = geminiProfile(server).copy(storeServerSide = true, useBackgroundLongDecode = true)
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(stored))
            assertEquals("Hello world", done.fullContent)
            assertTrue(server.requestPaths.any { it.endsWith("/v1beta/interactions/int-99") }, server.requestPaths.toString())
        }
    }

    @Test fun geminiFailedInteractionStatusFailsInsteadOfCompletingEmpty() {
        val failed = sse("""{"event_type":"interaction.complete","interaction":{"id":"i1","status":"failed","error":{"message":"Model execution failed"}}}""")
        Server(listOf(Reply(failed))).use { server ->
            assertResumes(runUntilTerminalOrRetry(geminiProfile(server)), "Model execution failed")
        }
    }

    @Test fun generateContentPlainJsonArrayFallbackAndXhighStepDown() {
        val missing = Reply("""{"error":{"message":"missing"}}""", status = 404, contentType = "application/json")
        val badXhigh = Reply("""{"error":{"message":"Invalid thinkingLevel: xhigh"}}""", status = 400, contentType = "application/json")
        val jsonArrayReply = Reply(
            """[{"candidates":[{"content":{"parts":[{"text":"Array reply"}]},"finishReason":"MAX_TOKENS"}]}]""",
            contentType = "application/json"
        )
        Server(listOf(missing, badXhigh, jsonArrayReply)).use { server ->
            val gemini = geminiProfile(server).copy(thinkingEffort = "xhigh")
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(gemini))
            assertEquals(3, server.requests.size)
            val req1Cfg = Json.parseToJsonElement(server.requests[1]).jsonObject["generationConfig"]!!.jsonObject["thinkingConfig"]!!.jsonObject
            val req2Cfg = Json.parseToJsonElement(server.requests[2]).jsonObject["generationConfig"]!!.jsonObject["thinkingConfig"]!!.jsonObject
            assertEquals("XHIGH", req1Cfg["thinkingLevel"]?.jsonPrimitive?.contentOrNull)
            assertEquals("HIGH", req2Cfg["thinkingLevel"]?.jsonPrimitive?.contentOrNull)
            assertTrue("Array reply" in done.fullContent && done.fullContent != "Array reply", done.fullContent)
        }
    }

    @Test fun openAiResponsesPersistsEncryptedReasoningAndHealsSummary400() {
        val summary400 = Reply("""{"error":{"message":"Unsupported parameter: reasoning.summary"}}""", status = 400, contentType = "application/json")
        val okSse = sse(
            """{"type":"response.output_item.done","item":{"id":"rs_1","type":"reasoning","encrypted_content":"enc_blob_123","summary":[]}}""",
            """{"type":"response.output_text.delta","delta":"Answer"}""",
            """{"type":"response.completed","response":{"id":"r1","status":"completed","output":[]}}"""
        )
        Server(listOf(summary400, Reply(okSse))).use { server ->
            val p = profile(server, CUSTOM_DIALECT_OPENAI_RESPONSES).copy(thinkingEffort = "high")
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(p))
            assertEquals("Answer", done.fullContent)
            assertTrue("enc_blob_123" in done.thinkingSignature, done.thinkingSignature)
            assertEquals(2, server.requests.size)
            val req0Reasoning = Json.parseToJsonElement(server.requests[0]).jsonObject["reasoning"]!!.jsonObject
            val req1Reasoning = Json.parseToJsonElement(server.requests[1]).jsonObject["reasoning"]!!.jsonObject
            assertNotNull(req0Reasoning["summary"])
            assertEquals("high", req1Reasoning["effort"]?.jsonPrimitive?.contentOrNull)
            assertNull(req1Reasoning["summary"])
        }
    }

    @Test fun openAiChatNullFinishReasonIsIncompleteAndStripsHistoricalReasoningOn400() {
        val nullFinish = sse("""{"choices":[{"delta":{"content":"partial"},"finish_reason":null}]}""")
        Server(listOf(Reply(nullFinish))).use { server ->
            assertResumes(outcome(profile(server, CUSTOM_DIALECT_OPENAI_CHAT)))
        }

        val badReasoning = Reply("""{"error":{"message":"Unknown field: reasoning_content"}}""", status = 400, contentType = "application/json")
        val okChat = Reply(finishedStreams.getValue(CUSTOM_DIALECT_OPENAI_CHAT))
        Server(listOf(badReasoning, okChat)).use { server ->
            val turn = MessageNode(
                "reply", MessageRole.ASSISTANT,
                listOf(
                    MessageVariant(
                        "reply-v",
                        content = "Step 1. Done",
                        thinkingSummary = "Plan",
                        toolCalls = listOf(ToolCallRecord("c1", "fs_read", "{}", resultText = "ok", precedingThinking = "Plan", precedingText = "Step 1."))
                    )
                )
            )
            val history = listOf(
                MessageNode("u1", MessageRole.USER, listOf(MessageVariant("uv1", content = "Start"))),
                turn,
                MessageNode("u2", MessageRole.USER, listOf(MessageVariant("uv2", content = "Next")))
            )
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(profile(server, CUSTOM_DIALECT_OPENAI_CHAT), history))
            assertEquals("partial", done.fullContent)
            assertEquals(2, server.requests.size)
            val msgs0 = Json.parseToJsonElement(server.requests[0]).jsonObject["messages"]!!.jsonArray
            val msgs1 = Json.parseToJsonElement(server.requests[1]).jsonObject["messages"]!!.jsonArray
            val assistant0 = msgs0.first { it.jsonObject["role"]?.jsonPrimitive?.contentOrNull == "assistant" }.jsonObject
            val assistant1 = msgs1.first { it.jsonObject["role"]?.jsonPrimitive?.contentOrNull == "assistant" }.jsonObject
            assertEquals("Plan", assistant0["reasoning_content"]?.jsonPrimitive?.contentOrNull)
            assertNotNull(assistant0["tool_calls"])
            assertNull(assistant1["reasoning_content"])
            assertNotNull(assistant1["tool_calls"], "Must strip reasoning_content without folding structured tool_calls")
        }
    }

    @Test fun anthropicThinking400StopsAfterAdaptiveAndBudgetFallbacks() {
        val replies = listOf(
            Reply("""{"error":{"message":"adaptive mode not supported"}}""", status = 400, contentType = "application/json"),
            Reply("""{"error":{"message":"thinking budget_tokens not supported"}}""", status = 400, contentType = "application/json"),
            Reply("""{"error":{"message":"bad request"}}""", status = 400, contentType = "application/json"),
            Reply(finishedStreams.getValue(CUSTOM_DIALECT_ANTHROPIC))
        )
        Server(replies).use { server ->
            val p = profile(server, CUSTOM_DIALECT_ANTHROPIC).copy(thinkingEffort = "high")
            assertIs<StreamUpdate.Error>(runUntilTerminalOrRetry(p))
            assertEquals(3, server.requests.size)
            assertEquals("adaptive", Json.parseToJsonElement(server.requests[0]).jsonObject["thinking"]!!.jsonObject["type"]?.jsonPrimitive?.contentOrNull)
            assertEquals("enabled", Json.parseToJsonElement(server.requests[1]).jsonObject["thinking"]!!.jsonObject["type"]?.jsonPrimitive?.contentOrNull)
            assertNull(Json.parseToJsonElement(server.requests[2]).jsonObject["thinking"])
            assertTrue(server.requestHeaders.all { (it["anthropic-version"]?.size ?: 0) == 1 }, server.requestHeaders.toString())
        }
    }

    @Test fun anthropicDoublePauseTurnMergesAssistantBlocks() {
        val pause1 = sse("""{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Step 1"}}""", """{"type":"message_delta","delta":{"stop_reason":"pause_turn"}}""", """{"type":"message_stop"}""")
        val pause2 = sse("""{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Step 2"}}""", """{"type":"message_delta","delta":{"stop_reason":"pause_turn"}}""", """{"type":"message_stop"}""")
        val end = sse("""{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Done"}}""", """{"type":"message_delta","delta":{"stop_reason":"end_turn"}}""", """{"type":"message_stop"}""")
        Server(listOf(Reply(pause1), Reply(pause2), Reply(end))).use { server ->
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(profile(server, CUSTOM_DIALECT_ANTHROPIC)))
            assertTrue("Done" in done.fullContent, done.fullContent)
            assertEquals(3, server.requests.size)
            val roles = Json.parseToJsonElement(server.requests[2]).jsonObject["messages"]!!.jsonArray
                .map { it.jsonObject["role"]!!.jsonPrimitive.content }
            val hasAdjacentAssistant = roles.zipWithNext().any { (a, b) -> a == "assistant" && b == "assistant" }
            assertFalse(hasAdjacentAssistant, "Consecutive pause_turn rounds must merge into one assistant message: $roles")
        }
    }

    @Test fun customProfileModelDiscoveryUsesResolvedDialectUrlAndSingleAnthropicHeader() = runBlocking {
        Server(listOf(Reply("""{"data":[{"id":"claude-fixture"}]}""", contentType = "application/json"))).use { server ->
            val models = AiProviderEngine.fetchAvailableModels(profile(server, CUSTOM_DIALECT_ANTHROPIC)).getOrThrow()
            assertEquals(listOf("claude-fixture"), models)
            assertTrue(server.requestPaths.single().startsWith("/v1/models"), server.requestPaths.toString())
            assertEquals(1, server.requestHeaders.single()["anthropic-version"]?.size)
        }
        Server(listOf(Reply("""{"models":[{"name":"models/gemini-fixture"}]}""", contentType = "application/json"))).use { server ->
            val models = AiProviderEngine.fetchAvailableModels(profile(server, CUSTOM_DIALECT_GEMINI)).getOrThrow()
            assertEquals(listOf("gemini-fixture"), models)
            assertEquals("/v1beta/models?pageSize=200", server.requestPaths.single())
        }
    }

    @Test fun imageContentPolicy400DoesNotRedirectToVideoEndpoint() {
        val rejected = Reply("""{"error":{"message":"Prompt violates content policy"}}""", status = 400, contentType = "application/json")
        Server(listOf(rejected)).use { server ->
            val imgProfile = ProviderProfile(
                "img", "Img", ProviderProtocol.OPENAI_RESPONSES,
                baseUrl = server.baseUrl,
                endpointPathOverride = "/v1/images/generations",
                defaultModel = "fixture-image-model",
                thinkingEffort = "none",
                useLegacyFallbackEndpoint = false,
                customModelSpecs = mapOf("fixture-image-model" to ModelPricingSpec("fixture-image-model"))
            )
            assertIs<StreamUpdate.Error>(runUntilTerminalOrRetry(imgProfile))
            assertTrue(server.requestPaths.isNotEmpty() && server.requestPaths.all { it == "/v1/images/generations" }, server.requestPaths.toString())
        }
    }

    @Test fun payloadTooLargeHttp413FailsFastWithoutRetryDelay() {
        Server(listOf(Reply("""{"error":{"message":"Request too large"}}""", status = 413, contentType = "application/json"))).use { server ->
            val outcome = runUntilTerminalOrRetry(profile(server, CUSTOM_DIALECT_OPENAI_CHAT))
            assertIs<StreamUpdate.Error>(outcome)
            assertEquals(1, server.requests.size)
        }
    }

    @Test fun sseKeepaliveCommentsDoNotBreakJsonFallbackOrMaskPrematureEof() = runBlocking {
        var jsonFallback = ""
        AiProviderEngine.readSseOrJsonFallback(
            channel = ByteReadChannel(": OPENROUTER PROCESSING\n\n{\"ok\":true}".toByteArray()),
            streamFinished = { true },
            onSseEvent = { _, _ -> fail("Comment plus plain JSON must route to plain fallback") },
            onPlainJsonFallback = { jsonFallback = it }
        )
        assertEquals("{\"ok\":true}", jsonFallback)

        val eofError = runCatching {
            AiProviderEngine.readSseOrJsonFallback(
                channel = ByteReadChannel(": ping\n\n".toByteArray()),
                streamFinished = { false },
                onSseEvent = { _, _ -> },
                onPlainJsonFallback = { fail("SSE comment alone must not invoke plain JSON fallback") }
            )
        }.exceptionOrNull()
        assertNotNull(eofError)
        assertTrue("Stream ended before" in eofError.message.orEmpty())
    }

    @Test fun geminiInteractionsStatusUpdateHandlesIncompleteAndFailedStates() {
        val incompleteStream = sse("""{"event_type":"interaction.status_update","interaction":{"id":"i-inc","status":"incomplete","usage":{"total_input_tokens":10,"total_output_tokens":5},"outputs":[{"type":"text","text":"Cut off"}]}}""")
        Server(listOf(Reply(incompleteStream))).use { server ->
            val completed = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(geminiProfile(server)))
            assertEquals("Cut off${AiProviderEngine.OUTPUT_LIMIT_NOTICE}", completed.fullContent)
            assertTrue(completed.metrics.usageIsComplete)
        }

        val failedStream = sse("""{"event_type":"interaction.failed","interaction":{"id":"i-fail","status":"failed","error":{"message":"upstream failure"}}}""")
        Server(listOf(Reply(failedStream))).use { server ->
            val update = runUntilTerminalOrRetry(geminiProfile(server))
            assertTrue(update is StreamUpdate.Status && "upstream failure" in update.message, "Unexpected update: $update")
        }
    }

    @Test fun geminiChain404FallbackStillCarriesOlderForeignReasoningHandoff() {
        val history = listOf(
            MessageNode("u1", MessageRole.USER, listOf(MessageVariant("uv1", content = "Start"))),
            MessageNode(
                "a1", MessageRole.ASSISTANT,
                listOf(
                    MessageVariant(
                        "av1",
                        content = "Claude answer",
                        thinkingSummary = "Claude private reasoning plan",
                        providerProtocol = ProviderProtocol.ANTHROPIC_MESSAGES,
                        providerWireDialect = CUSTOM_DIALECT_ANTHROPIC,
                        modelUsed = "claude-sonnet"
                    )
                )
            ),
            MessageNode("u2", MessageRole.USER, listOf(MessageVariant("uv2", content = "Switch to Gemini"))),
            MessageNode(
                "a2", MessageRole.ASSISTANT,
                listOf(
                    MessageVariant(
                        "av2",
                        content = "Gemini reply",
                        providerInteractionId = "expired-chain-id",
                        providerProtocol = ProviderProtocol.GEMINI_INTERACTIONS,
                        providerWireDialect = CUSTOM_DIALECT_GEMINI,
                        modelUsed = "fixture-model"
                    )
                )
            ),
            MessageNode("u3", MessageRole.USER, listOf(MessageVariant("uv3", content = "Continue")))
        )
        val expired404 = Reply("""{"error":{"message":"Interaction expired"}}""", status = 404, contentType = "application/json")
        val okSse = sse("""{"event_type":"interaction.complete","interaction":{"id":"i-new","status":"completed","steps":[{"type":"model_output","content":[{"type":"text","text":"Recovered"}]}]}}""")
        Server(listOf(expired404, Reply(okSse))).use { server ->
            val chainedProfile = geminiProfile(server).copy(storeServerSide = true, useStatefulChaining = true)
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(chainedProfile, history))
            assertEquals("Recovered", done.fullContent)
            assertEquals(2, server.requests.size)
            assertTrue(AiProviderEngine.REASONING_HANDOFF_MARKER in server.requests[1], server.requests[1])
            assertTrue("Claude private reasoning plan" in server.requests[1], server.requests[1])
        }
    }

    @Test fun geminiBackgroundPollRetriesTransientHttp502WithoutResubmittingPost() {
        val partialSse = sse(
            """{"event_type":"interaction.start","interaction":{"id":"int-poll-1","status":"in_progress"}}""",
            """{"event_type":"content.delta","index":0,"delta":{"type":"text","text":"Hello "}}"""
        )
        val poll502 = Reply("""{"error":{"message":"Bad Gateway"}}""", status = 502, contentType = "application/json")
        val pollDone = """{"id":"int-poll-1","status":"completed","outputs":[{"type":"text","text":"Hello recovered"}]}"""
        Server(listOf(Reply(partialSse), poll502, Reply(pollDone, contentType = "application/json"))).use { server ->
            val stored = geminiProfile(server).copy(storeServerSide = true, useBackgroundLongDecode = true)
            val done = assertIs<StreamUpdate.Completed>(runUntilTerminalOrRetry(stored))
            assertEquals("Hello recovered", done.fullContent)
            assertEquals(1, server.requestPaths.count { it == "/v1beta/interactions" }, server.requestPaths.toString())
            assertEquals(2, server.requestPaths.count { it == "/v1beta/interactions/int-poll-1" }, server.requestPaths.toString())
        }
    }

    @Test fun geminiBackgroundPollExhaustionStopsWithAcceptedJobInsteadOfResubmitting() {
        val partialSse = sse("""{"event_type":"interaction.start","interaction":{"id":"int-stuck","status":"in_progress"}}""")
        val poll502 = Reply("""{"error":{"message":"Bad Gateway"}}""", status = 502, contentType = "application/json")
        val replies = listOf(Reply(partialSse)) + List(8) { poll502 }
        Server(replies).use { server ->
            val stored = geminiProfile(server).copy(
                storeServerSide = true,
                useBackgroundLongDecode = true,
                useLegacyFallbackEndpoint = true
            )
            val allUpdates = runBlocking { withTimeout(20_000) { updates(stored).toList() } }
            val err = assertIs<StreamUpdate.Error>(allUpdates.last())
            assertTrue("int-stuck" in err.message, err.message)
            assertEquals(1, server.requestPaths.count { it == "/v1beta/interactions" }, server.requestPaths.toString())
            assertEquals(6, server.requestPaths.count { it == "/v1beta/interactions/int-stuck" }, server.requestPaths.toString())
            assertTrue(server.requestPaths.none { "generateContent" in it }, server.requestPaths.toString())
        }
    }

    @Test fun geminiNonRetryableSseStreamErrorFailsFastWithoutPollingOrFallback() {
        val streamInvalid = sse(
            """{"event_type":"interaction.start","interaction":{"id":"int-bad","status":"in_progress"}}""",
            """{"event_type":"error","error":{"code":400,"status":"INVALID_ARGUMENT","message":"Invalid schema"}}"""
        )
        Server(listOf(Reply(streamInvalid))).use { server ->
            val stored = geminiProfile(server).copy(
                storeServerSide = true,
                useBackgroundLongDecode = true,
                useLegacyFallbackEndpoint = true
            )
            val err = assertIs<StreamUpdate.Error>(runUntilTerminalOrRetry(stored))
            assertTrue("Invalid schema" in err.message, err.message)
            assertEquals(listOf("/v1beta/interactions"), server.requestPaths)
        }
    }
}
