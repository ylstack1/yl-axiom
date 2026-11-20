package io.parchment.ai.network

import com.sun.net.httpserver.HttpServer
import io.parchment.ai.harness.*
import io.parchment.ai.model.*
import io.parchment.ai.platform.PlatformWorkspace
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.toList
import kotlinx.serialization.json.*
import java.net.InetSocketAddress
import java.nio.file.Files
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.*

/** Runs the client against real servers: an HTTP one and a separate operating-system process. */
class McpEndToEndTest {
    private class HttpMcp(
        val stateless: Boolean,
        val streamed: Boolean = false,
        val onMethod: ((String) -> Unit)? = null,
        /** Ends the first session at its first tool call (HTTP 404), as a restarted server would. */
        val loseSessionOnCall: Boolean = false,
        val customTools: List<String> = emptyList()
    ) : AutoCloseable {
        @Volatile private var sessionEnded = false
        val requests = Collections.synchronizedList(mutableListOf<Pair<JsonObject, Map<String, String>>>())
        private val executor = Executors.newCachedThreadPool { task -> Thread(task, "mcp-e2e-fixture").apply { isDaemon = true } }
        private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            this.executor = this@HttpMcp.executor
            createContext("/mcp") { exchange ->
                val message = Json.parseToJsonElement(exchange.requestBody.bufferedReader().readText()).jsonObject
                val headers = exchange.requestHeaders.entries.associate { it.key.lowercase() to it.value.first() }
                requests += message to headers
                val method = message.getValue("method").jsonPrimitive.content
                onMethod?.invoke(method)
                if (loseSessionOnCall && method == "tools/call" && headers["mcp-session-id"] == "session-1") {
                    sessionEnded = true
                    exchange.sendResponseHeaders(404, -1)
                    exchange.close()
                    return@createContext
                }
                val id = message["id"]
                val result: JsonElement? = when {
                    method == "initialize" && stateless -> null
                    method == "initialize" -> buildJsonObject { put("protocolVersion", "2025-06-18"); put("capabilities", JsonObject(emptyMap())); put("serverInfo", buildJsonObject { put("name", "fixture") }) }
                    method == "tools/list" && customTools.isNotEmpty() -> buildJsonObject {
                        putJsonArray("tools") { customTools.forEach { add(tool(it)) } }
                    }
                    method == "tools/list" && message["params"]?.jsonObject?.get("cursor") == null -> buildJsonObject {
                        putJsonArray("tools") { add(tool("first one")) }
                        put("nextCursor", "page-2")
                    }
                    method == "tools/list" -> buildJsonObject { putJsonArray("tools") { add(tool("second")) } }
                    method == "tools/call" -> buildJsonObject { putJsonArray("content") { add(buildJsonObject { put("type", "text"); put("text", "http says hi") }) } }
                    else -> null
                }
                val isNotification = id == null
                val reply = when {
                    isNotification -> ""
                    result != null -> buildJsonObject { put("jsonrpc", "2.0"); put("id", id!!); put("result", result) }.toString()
                    else -> buildJsonObject { put("jsonrpc", "2.0"); put("id", id!!); put("error", buildJsonObject { put("code", -32601); put("message", "Method not found") }) }.toString()
                }
                if (!stateless) exchange.responseHeaders.set("Mcp-Session-Id", if (sessionEnded) "session-2" else "session-1")
                val sse = streamed && !isNotification
                exchange.responseHeaders.set("Content-Type", if (sse) "text/event-stream" else "application/json")
                val body = (if (sse) "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\nevent: message\ndata: $reply\n\n" else reply).toByteArray()
                exchange.sendResponseHeaders(if (isNotification) 202 else 200, if (isNotification) -1 else body.size.toLong())
                if (!isNotification) exchange.responseBody.write(body)
                exchange.close()
            }
            start()
        }
        val url = "http://127.0.0.1:${server.address.port}/mcp"
        override fun close() = server.stop(0)
        private fun tool(name: String) = buildJsonObject { put("name", name); put("description", "A tool"); put("inputSchema", buildJsonObject { put("type", "object") }) }
    }

    private val pythonServer = """
import json, sys
calls = 0
for line in sys.stdin:
    msg = json.loads(line)
    print("log line that is not protocol", file=sys.stderr, flush=True)
    method, mid = msg.get("method"), msg.get("id")
    if mid is None:
        continue
    if method == "initialize":
        result = {"protocolVersion": msg["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "py"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}}]}
    elif method == "tools/call":
        calls += 1
        result = {"content": [{"type": "text", "text": "echo:%s:%d" % (msg["params"]["arguments"].get("text"), calls)}]}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "nope"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": result}), flush=True)
    """.trimIndent()

    private fun python(): Boolean = runCatching {
        ProcessBuilder("python", "--version").redirectErrorStream(true).start().waitFor() == 0
    }.getOrDefault(false)

    private suspend fun <T> connected(servers: List<McpServerConfig>, block: suspend () -> T): T {
        try {
            McpRegistry.sync(servers)
            return block()
        } finally {
            withContext(NonCancellable) { McpRegistry.sync(emptyList()) }
        }
    }

    @Test fun handshakeServersGetSessionsPagingAndEventStreams() = runBlocking<Unit> {
        HttpMcp(stateless = false, streamed = true).use { server ->
            connected(listOf(McpServerConfig("a", "Web App", url = server.url, bearerToken = "secret", enabled = true))) {
                assertEquals(McpServerState.READY, McpRegistry.statuses.value.getValue("a").state, McpRegistry.statuses.value.toString())
                val names = McpRegistry.definitions.value.map { it.name }
                assertEquals(listOf("mcp__Web_App__first_one", "mcp__Web_App__second"), names)
                val result = McpRegistry.call(names[0], "{}")
                assertEquals(McpToolResult("http says hi", false), result)
            }
            val initialized = server.requests.first { it.first["method"]?.jsonPrimitive?.content == "notifications/initialized" }.second
            assertEquals("session-1", initialized["mcp-session-id"])
            assertEquals("2025-06-18", initialized["mcp-protocol-version"])
            assertEquals("Bearer secret", initialized["authorization"])
            val call = server.requests.first { it.first["method"]?.jsonPrimitive?.content == "tools/call" }
            assertEquals("first one", call.second["mcp-name"], "The server's own tool name is what it receives")
            assertEquals("first one", call.first["params"]!!.jsonObject["name"]!!.jsonPrimitive.content)
        }
    }

    @Test fun serversWithoutTheHandshakeGetTheVersionOnEveryRequest() = runBlocking<Unit> {
        HttpMcp(stateless = true).use { server ->
            connected(listOf(McpServerConfig("a", "New", url = server.url, enabled = true))) {
                assertEquals(McpServerState.READY, McpRegistry.statuses.value.getValue("a").state, McpRegistry.statuses.value.toString())
                assertFalse(McpRegistry.call("mcp__New__second", "{}").isError)
            }
            assertTrue(server.requests.none { it.first["method"]?.jsonPrimitive?.content == "notifications/initialized" })
            val call = server.requests.first { it.first["method"]?.jsonPrimitive?.content == "tools/call" }
            assertEquals(MCP_STATELESS_VERSION, call.second["mcp-protocol-version"])
            assertEquals(MCP_STATELESS_VERSION, call.first["params"]!!.jsonObject["_meta"]!!.jsonObject["io.modelcontextprotocol/protocolVersion"]!!.jsonPrimitive.content)
        }
    }

    @Test fun aTokenIsNeverSentOverPlainHttpToAnotherMachine() = runBlocking<Unit> {
        connected(listOf(McpServerConfig("a", "Far", url = "http://example.invalid/mcp", bearerToken = "secret", enabled = true))) {
            val status = McpRegistry.statuses.value.getValue("a")
            assertEquals(McpServerState.FAILED, status.state)
            assertTrue("https" in status.message, status.message)
            assertTrue(McpRegistry.definitions.value.isEmpty())
        }
    }

    @Test fun aRealLocalProcessServesToolsUntilItIsRemoved() = runBlocking<Unit> {
        if (!python()) return@runBlocking
        val script = Files.createTempFile("parchment-mcp-", ".py")
        try {
            Files.writeString(script, pythonServer)
            val config = McpServerConfig("py", "Local", command = "python $script", enabled = true)
            connected(listOf(config)) {
                assertEquals(McpServerState.READY, McpRegistry.statuses.value.getValue("py").state, McpRegistry.statuses.value.toString())
                assertEquals(McpToolResult("echo:hi:1", false), McpRegistry.call("mcp__Local__echo", """{"text":"hi"}"""))
                assertEquals(McpToolResult("echo:yo:2", false), McpRegistry.call("mcp__Local__echo", """{"text":"yo"}"""), "One process keeps serving")
                assertTrue(McpRegistry.call("mcp__Local__echo", "[1]").isError)
                McpRegistry.sync(emptyList())
                assertTrue(McpRegistry.definitions.value.isEmpty())
                assertTrue(McpRegistry.call("mcp__Local__echo", "{}").isError, "A removed server cannot be called")
            }
        } finally { Files.deleteIfExists(script) }
    }

    @Test fun aServerThatExitsIsStartedAgainForTheNextCall() = runBlocking<Unit> {
        if (!python()) return@runBlocking
        val marker = Files.createTempFile("parchment-mcp-once-", ".marker").also { Files.delete(it) }
        val script = Files.createTempFile("parchment-mcp-exit-once-", ".py")
        try {
            val exitOnce = "        if not os.path.exists(MARKER):\n            open(MARKER, \"w\").close()\n            sys.exit(3)\n        calls += 1"
            Files.writeString(script, "import os\nMARKER = r\"$marker\"\n" + pythonServer.replace("        calls += 1", exitOnce))
            connected(listOf(McpServerConfig("py", "Restarts", command = "python $script", enabled = true))) {
                val lost = withTimeout(15_000) { McpRegistry.call("mcp__Restarts__echo", """{"text":"lost"}""") }
                assertTrue(lost.isError && "restarted" in lost.text, lost.text)
                assertEquals(listOf("mcp__Restarts__echo"), McpRegistry.definitions.value.map { it.name }, "Its tool stays offered under its name")
                val started = System.nanoTime()
                assertEquals(McpToolResult("echo:back:1", false), withTimeout(15_000) { McpRegistry.call("mcp__Restarts__echo", """{"text":"back"}""") })
                val waitedMs = (System.nanoTime() - started) / 1_000_000L
                assertTrue(waitedMs < 5_000L, "The next call works within 5 s, took $waitedMs ms")
                assertEquals(McpServerState.READY, McpRegistry.statuses.value.getValue("py").state)
            }
        } finally {
            Files.deleteIfExists(script)
            Files.deleteIfExists(marker)
        }
    }

    @Test fun aServerThatKeepsExitingIsStartedThreeTimesThenLeftOff() = runBlocking<Unit> {
        if (!python()) return@runBlocking
        val starts = Files.createTempFile("parchment-mcp-starts-", ".log")
        val script = Files.createTempFile("parchment-mcp-exit-", ".py")
        try {
            val logStart = "import os\nwith open(r\"$starts\", \"a\") as log:\n    log.write(str(os.getpid()) + \"\\n\")\n"
            Files.writeString(script, logStart + pythonServer.replace("        calls += 1", "        sys.exit(3)"))
            connected(listOf(McpServerConfig("py", "Dying", command = "python $script", enabled = true))) {
                repeat(3) { assertTrue(withTimeout(30_000) { McpRegistry.call("mcp__Dying__echo", "{}") }.isError) }
                assertEquals(McpServerState.FAILED, McpRegistry.statuses.value.getValue("py").state)
                assertTrue(McpRegistry.definitions.value.isEmpty(), "A server that keeps stopping is no longer offered")
                assertTrue(McpRegistry.call("mcp__Dying__echo", "{}").isError)
            }
            val pids = Files.readAllLines(starts).filter { it.isNotBlank() }
            assertEquals(3, pids.size, "Started three times: $pids")
            assertTrue(pids.none { ProcessHandle.of(it.trim().toLong()).map { process -> process.isAlive }.orElse(false) }, "No server process is left running")
        } finally {
            Files.deleteIfExists(script)
            Files.deleteIfExists(starts)
        }
    }

    @Test fun aLostHttpSessionIsRenewedAndTheCallSentOnceMore() = runBlocking<Unit> {
        HttpMcp(stateless = false, loseSessionOnCall = true).use { server ->
            connected(listOf(McpServerConfig("a", "Web", url = server.url, enabled = true))) {
                assertEquals(McpToolResult("http says hi", false), McpRegistry.call("mcp__Web__first_one", "{}"))
            }
            val methods = server.requests.map { it.first.getValue("method").jsonPrimitive.content }
            assertEquals(2, methods.count { it == "initialize" }, methods.toString())
            assertEquals(2, methods.count { it == "tools/call" }, "The call the ended session refused, then the one that ran: $methods")
        }
    }

    private class Provider(val toolName: String, val arguments: String = """{"text":"from model"}""") : AutoCloseable {
        val requests = Collections.synchronizedList(mutableListOf<JsonObject>())
        private val count = AtomicInteger()
        private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            executor = java.util.concurrent.Executors.newCachedThreadPool { Thread(it).apply { isDaemon = true } }
            createContext("/") { exchange ->
                requests += Json.parseToJsonElement(exchange.requestBody.bufferedReader().readText()).jsonObject
                val first = count.getAndIncrement() == 0
                val body = buildJsonObject {
                    put("id", "r"); put("status", "completed"); put("model", "fixture-model")
                    putJsonArray("output") {
                        add(
                            if (first) buildJsonObject { put("type", "function_call"); put("call_id", "c1"); put("name", toolName); put("arguments", arguments) }
                            else buildJsonObject { put("type", "message"); put("role", "assistant"); putJsonArray("content") { add(buildJsonObject { put("type", "output_text"); put("text", "Done.") }) } }
                        )
                    }
                }.toString().toByteArray()
                exchange.responseHeaders.set("Content-Type", "application/json")
                exchange.sendResponseHeaders(200, body.size.toLong())
                exchange.responseBody.use { it.write(body) }
            }
            start()
        }
        val profile = ProviderProfile(
            "fixture",
            "Fixture",
            ProviderProtocol.CUSTOM,
            customDialect = CUSTOM_DIALECT_OPENAI_RESPONSES,
            baseUrl = "http://127.0.0.1:${server.address.port}",
            defaultModel = "fixture-model",
            thinkingEffort = "none",
            customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
        )
        override fun close() = server.stop(0)
        fun run(policy: AgentPermissionPolicy) = AiProviderEngine.streamGeneration(
            profile = profile,
            session = ChatSession("chat", "Fixture", 1L, 1L, profile.id, workspacePath = policy.workspacePath, workspaceMode = policy.workspaceMode, permissionOverrides = policy.asOverrides()),
            historyNodes = listOf(MessageNode("user", MessageRole.USER, listOf(MessageVariant("v", content = "Go"))))
        )
        fun advertised(index: Int) = requests[index].getValue("tools").jsonArray.map { it.jsonObject.getValue("name").jsonPrimitive.content }
    }

    private val noWorkspace = AgentPermissionPolicy(workspacePath = "", workspaceMode = WorkspaceMode.OFF)

    @Test fun aModelCallRunsOnlyAfterApprovalAndItsResultReturns() = runBlocking<Unit> {
        HttpMcp(stateless = false).use { mcp ->
            connected(listOf(McpServerConfig("a", "App", url = mcp.url, enabled = true))) {
                Provider("mcp__App__second").use { provider ->
                    val broker = ToolApprovalBroker("run", "chat", { ToolAccessScope(noWorkspace) })
                    val result = async(AgentToolRunContext(broker)) { withTimeout(12_000) { provider.run(noWorkspace).toList() } }
                    val request = withTimeout(8000) { broker.requests.first { it.isNotEmpty() }.single() }
                    assertEquals("mcp__App__second", request.toolName)
                    assertTrue(mcp.requests.none { it.first["method"]?.jsonPrimitive?.content == "tools/call" }, "Nothing runs before approval")
                    assertTrue(broker.respond(request.requestId, ToolApprovalDecision.ALLOW_ONCE))
                    assertIs<StreamUpdate.Completed>(result.await().last())
                    assertTrue("http says hi" in provider.requests[1].toString())
                    broker.close()
                }
            }
        }
    }

    @Test fun aDeniedCallNeverReachesTheServer() = runBlocking<Unit> {
        HttpMcp(stateless = false).use { mcp ->
            connected(listOf(McpServerConfig("a", "App", url = mcp.url, enabled = true))) {
                Provider("mcp__App__second").use { provider ->
                    val broker = ToolApprovalBroker("run", "chat", { ToolAccessScope(noWorkspace) })
                    val result = async(AgentToolRunContext(broker)) { withTimeout(12_000) { provider.run(noWorkspace).toList() } }
                    val request = withTimeout(8000) { broker.requests.first { it.isNotEmpty() }.single() }
                    assertTrue(broker.respond(request.requestId, ToolApprovalDecision.DENY))
                    result.await()
                    assertTrue(mcp.requests.none { it.first["method"]?.jsonPrimitive?.content == "tools/call" })
                    assertTrue("denied" in provider.requests[1].toString())
                    broker.close()
                }
            }
        }
    }

    @Test fun aDisabledToolIsNotOfferedAndCannotBeCalled() = runBlocking<Unit> {
        HttpMcp(stateless = false).use { mcp ->
            connected(listOf(McpServerConfig("a", "App", url = mcp.url, enabled = true))) {
                val blocked = noWorkspace.copy(toolOverrides = mapOf("mcp__App__second" to false))
                Provider("mcp__App__second").use { provider ->
                    val broker = ToolApprovalBroker("run", "chat", { ToolAccessScope(blocked) })
                    withContext(AgentToolRunContext(broker)) { withTimeout(12_000) { provider.run(blocked).toList() } }
                    assertTrue("mcp__App__first_one" in provider.advertised(0))
                    assertFalse("mcp__App__second" in provider.advertised(0))
                    assertTrue(broker.requests.value.isEmpty())
                    assertTrue(mcp.requests.none { it.first["method"]?.jsonPrimitive?.content == "tools/call" })
                    assertTrue("disabled" in provider.requests[1].toString())
                    broker.close()
                }
            }
        }
    }

    @Test fun anImportCannotBringLocalCommandsOrLiveConnections() {
        val imported = AppStateSnapshot(
            preferences = PreferencesConfig(
                mcpServers = listOf(
                    McpServerConfig("local", "Evil", command = "rm -rf /", enabled = true),
                    McpServerConfig("web", "Remote", url = "https://example.com/mcp", enabled = true)
                )
            )
        ).withImportedPermissions()
        assertEquals(listOf("web"), imported.preferences.mcpServers.map { it.id })
        assertFalse(imported.preferences.mcpServers.single().enabled)
    }

    @Test fun workspaceSkillsAreListedThenLoadedOnDemand() = runBlocking<Unit> {
        val root = Files.createTempDirectory("parchment-skill-")
        val folder = root.resolve(".agents/skills/release-notes")
        Files.createDirectories(folder)
        val body = "---\nname: release-notes\ndescription: >\n  Write release notes\n  from the changelog.\n---\nFull instructions live here."
        Files.writeString(folder.resolve("SKILL.md"), body)
        Files.createDirectories(root.resolve(".agents/skills/no-description"))
        Files.writeString(root.resolve(".agents/skills/no-description/SKILL.md"), "---\nname: x\n---\nBody")
        try {
            val policy = AgentPermissionPolicy(workspacePath = root.toString(), workspaceMode = WorkspaceMode.READ_ONLY)
            val skills = WorkspaceHarness.discoverSkills(root.toString())
            assertEquals(listOf("release-notes"), skills.map { it.name })
            assertEquals("Write release notes from the changelog.", skills.single().description)
            Provider("skill_load", """{"name":"release-notes"}""").use { provider ->
                val broker = ToolApprovalBroker("run", "chat", { ToolAccessScope(policy) })
                withContext(AgentToolRunContext(broker)) { withTimeout(12_000) { provider.run(policy).toList() } }
                val first = provider.requests[0].toString()
                assertTrue("Write release notes from the changelog." in first, "The list is offered up front")
                assertFalse("Full instructions live here." in first, "The body loads only on demand")
                assertTrue("Full instructions live here." in provider.requests[1].toString())
                broker.close()
            }
            val unknown = WorkspaceHarness.executeToolCall(root.toString(), WorkspaceMode.READ_ONLY, "c", "skill_load", """{"name":"../../etc"}""")
            assertTrue(unknown.isError, unknown.resultText)
        } finally { root.toFile().deleteRecursively() }
        assertNotNull(PlatformWorkspace.startMcpProcess("exit 0")?.also { it.close() })
    }

    @Test fun handshakeRejectsAuthJsonRpcErrorAndSurfacesHttpErrorBody() = runBlocking<Unit> {
        val authErrServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            createContext("/err-rpc") { ex ->
                ex.requestBody.use { it.readBytes() }
                val bytes = """{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"Invalid credentials"}}""".toByteArray()
                ex.responseHeaders.set("Content-Type", "application/json")
                ex.sendResponseHeaders(200, bytes.size.toLong())
                ex.responseBody.use { it.write(bytes) }
            }
            createContext("/err-http") { ex ->
                ex.requestBody.use { it.readBytes() }
                val bytes = "Gateway token rejected".toByteArray()
                ex.responseHeaders.set("Content-Type", "text/plain")
                ex.sendResponseHeaders(401, bytes.size.toLong())
                ex.responseBody.use { it.write(bytes) }
            }
            start()
        }
        try {
            val base = "http://127.0.0.1:${authErrServer.address.port}"
            val rpcFail = assertFailsWith<McpException> {
                McpClient(HttpMcpTransport("$base/err-rpc", "")).connect()
            }
            assertTrue("Invalid credentials" in rpcFail.message.orEmpty(), rpcFail.message)
            val httpFail = assertFailsWith<McpException> {
                McpClient(HttpMcpTransport("$base/err-http", "")).connect()
            }
            assertTrue("401" in httpFail.message.orEmpty() && "Gateway token rejected" in httpFail.message.orEmpty(), httpFail.message)
        } finally {
            authErrServer.stop(0)
        }
    }

    @Test
    fun unsupportedAndMalformedMcpUrlsFailFast() {
        listOf("", "not-a-url", "ftp://example.com/mcp", "ws://127.0.0.1:8080/mcp").forEach { bad ->
            val err = assertFailsWith<McpException> { HttpMcpTransport(bad, "") }
            assertTrue("http or https" in err.message.orEmpty(), err.message)
        }
        assertFailsWith<McpException> { HttpMcpTransport("https:///mcp", "") }
        assertFailsWith<McpException> { HttpMcpTransport("https://:8080/mcp", "") }
        assertFailsWith<McpException> { HttpMcpTransport("https://[]/mcp", "") }
        assertFailsWith<McpException> { HttpMcpTransport("http://example.com/mcp", "secret-token") }
        HttpMcpTransport("http://[::1]:8080/mcp", "local-token")
    }

    @Test
    fun concurrentConnectAndListToolsCoalesceToSingleRpc() = runBlocking<Unit>(Dispatchers.Default) {
        val initEntered = CountDownLatch(1)
        val allowInit = CountDownLatch(1)
        val listEntered = CountDownLatch(1)
        val allowList = CountDownLatch(1)
        HttpMcp(stateless = false, onMethod = { method ->
            when (method) {
                "initialize" -> {
                    initEntered.countDown()
                    allowInit.await(5, TimeUnit.SECONDS)
                }
                "tools/list" -> {
                    listEntered.countDown()
                    allowList.await(5, TimeUnit.SECONDS)
                }
            }
        }).use { mcp ->
            val client = McpClient(HttpMcpTransport(mcp.url, ""))
            try {
                val connectCalls = List(3) { async { client.connect() } }
                assertTrue(initEntered.await(3, TimeUnit.SECONDS))
                delay(40)
                allowInit.countDown()
                connectCalls.awaitAll()
                client.connect()

                val methodsAfterConnect = mcp.requests.map { it.first.getValue("method").jsonPrimitive.content }
                assertEquals(1, methodsAfterConnect.count { it == "initialize" })
                assertEquals(1, methodsAfterConnect.count { it == "notifications/initialized" })

                val listCalls = List(3) { async { client.listTools() } }
                assertTrue(listEntered.await(3, TimeUnit.SECONDS))
                delay(40)
                allowList.countDown()
                val results = listCalls.awaitAll()
                results.forEach { tools ->
                    assertEquals(listOf("first one", "second"), tools.map { it.name })
                }
                val listRequests = mcp.requests.count { it.first.getValue("method").jsonPrimitive.content == "tools/list" }
                assertEquals(2, listRequests, "Paged tools/list (2 pages) should execute once across 3 concurrent callers")
            } finally {
                client.close()
            }
        }
    }

    @Test
    fun httpMcpSanitizesPaddedTokenAndRefusesRedirects() = runBlocking {
        val redirectedTargetHit = AtomicInteger(0)
        val redirectServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            createContext("/target") { ex ->
                redirectedTargetHit.incrementAndGet()
                ex.sendResponseHeaders(200, -1)
                ex.close()
            }
            createContext("/mcp") { ex ->
                ex.responseHeaders.set("Location", "http://127.0.0.1:${address.port}/target")
                ex.sendResponseHeaders(303, -1)
                ex.close()
            }
            start()
        }
        try {
            val err = assertFailsWith<McpException> {
                McpClient(
                    HttpMcpTransport("  http://127.0.0.1:${redirectServer.address.port}/mcp  ", "  tok\r\n-1  ")
                ).connect()
            }
            assertTrue("303" in err.message.orEmpty(), err.message)
            assertEquals(0, redirectedTargetHit.get(), "MCP transport must not follow redirects with credentials")
        } finally {
            redirectServer.stop(0)
        }

        HttpMcp(stateless = false).use { mcp ->
            val client = McpClient(HttpMcpTransport("  ${mcp.url}  ", "  local\r\n-secret  "))
            try {
                client.connect()
                assertEquals("Bearer local-secret", mcp.requests.first().second["authorization"])
            } finally {
                client.close()
            }
        }
    }

    @Test
    fun http400OnInitializeFallsBackTo2026StatelessWhileTrue404StillFails() = runBlocking<Unit> {
        val strictStateless = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            createContext("/mcp") { ex ->
                val msg = Json.parseToJsonElement(ex.requestBody.bufferedReader().use { it.readText() }).jsonObject
                val method = msg.getValue("method").jsonPrimitive.content
                val protoHeader = ex.requestHeaders.getFirst("MCP-Protocol-Version")
                if (method == "initialize" || protoHeader != MCP_STATELESS_VERSION) {
                    val err = """{"error":"Stateless 2026-07-28 endpoint does not implement initialize"}""".toByteArray()
                    ex.responseHeaders.set("Content-Type", "application/json")
                    ex.sendResponseHeaders(400, err.size.toLong())
                    ex.responseBody.use { it.write(err) }
                } else {
                    val meta = msg["params"]!!.jsonObject["_meta"]!!.jsonObject
                    assertEquals(MCP_STATELESS_VERSION, meta["io.modelcontextprotocol/protocolVersion"]?.jsonPrimitive?.content)
                    assertEquals("Parchment", meta["io.modelcontextprotocol/clientInfo"]?.jsonObject?.get("name")?.jsonPrimitive?.content)
                    val res = buildJsonObject {
                        put("jsonrpc", "2.0")
                        put("id", msg.getValue("id"))
                        putJsonObject("result") {
                            putJsonArray("tools") {
                                add(
                                    buildJsonObject {
                                        put("name", "ping")
                                        put("description", "Ping")
                                        putJsonObject("inputSchema") { put("type", "object") }
                                    }
                                )
                            }
                        }
                    }.toString().toByteArray()
                    ex.responseHeaders.set("Content-Type", "application/json")
                    ex.sendResponseHeaders(200, res.size.toLong())
                    ex.responseBody.use { it.write(res) }
                }
                ex.close()
            }
            createContext("/missing") { ex ->
                ex.requestBody.use { it.readBytes() }
                val err = "Not found".toByteArray()
                ex.sendResponseHeaders(404, err.size.toLong())
                ex.responseBody.use { it.write(err) }
                ex.close()
            }
            start()
        }
        try {
            val base = "http://127.0.0.1:${strictStateless.address.port}"
            connected(listOf(McpServerConfig("s26", "Stateless26", url = "$base/mcp", enabled = true))) {
                assertEquals(McpServerState.READY, McpRegistry.statuses.value.getValue("s26").state)
                assertEquals(listOf("mcp__Stateless26__ping"), McpRegistry.definitions.value.map { it.name })
            }
            connected(listOf(McpServerConfig("bad", "Missing", url = "$base/missing", enabled = true))) {
                assertEquals(McpServerState.FAILED, McpRegistry.statuses.value.getValue("bad").state)
            }
        } finally {
            strictStateless.stop(0)
        }
    }

    @Test fun deferredMcpSchemasExpandOnDemandPastSixToolsAndKeepApproval() = runBlocking<Unit> {
        val eightTools = (1..8).map { "tool_$it" }
        HttpMcp(stateless = false, customTools = eightTools).use { mcp ->
            connected(listOf(McpServerConfig("bulk", "Bulk", url = mcp.url, enabled = true))) {
                assertEquals(8, McpRegistry.definitions.value.size)
                val step = AtomicInteger()
                val requests = Collections.synchronizedList(mutableListOf<JsonObject>())
                val providerServer = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
                    executor = Executors.newCachedThreadPool { Thread(it).apply { isDaemon = true } }
                    createContext("/") { exchange ->
                        requests += Json.parseToJsonElement(exchange.requestBody.bufferedReader().readText()).jsonObject
                        val idx = step.getAndIncrement()
                        val body = buildJsonObject {
                            put("id", "r$idx"); put("status", "completed"); put("model", "fixture-model")
                            putJsonArray("output") {
                                when (idx) {
                                    0 -> add(
                                        buildJsonObject {
                                            put("type", "function_call"); put("call_id", "c1")
                                            put("name", WorkspaceHarness.MCP_SCHEMA_TOOL_NAME)
                                            put("arguments", """{"names":["mcp__Bulk__tool_5"]}""")
                                        }
                                    )
                                    1 -> add(
                                        buildJsonObject {
                                            put("type", "function_call"); put("call_id", "c2")
                                            put("name", "mcp__Bulk__tool_5")
                                            put("arguments", """{}""")
                                        }
                                    )
                                    else -> add(
                                        buildJsonObject {
                                            put("type", "message"); put("role", "assistant")
                                            putJsonArray("content") { add(buildJsonObject { put("type", "output_text"); put("text", "Expanded and ran.") }) }
                                        }
                                    )
                                }
                            }
                        }.toString().toByteArray()
                        exchange.responseHeaders.set("Content-Type", "application/json")
                        exchange.sendResponseHeaders(200, body.size.toLong())
                        exchange.responseBody.use { it.write(body) }
                    }
                    start()
                }
                try {
                    val profile = ProviderProfile(
                        "fixture-bulk",
                        "Fixture",
                        ProviderProtocol.CUSTOM,
                        customDialect = CUSTOM_DIALECT_OPENAI_RESPONSES,
                        baseUrl = "http://127.0.0.1:${providerServer.address.port}",
                        defaultModel = "fixture-model",
                        thinkingEffort = "none",
                        customModelSpecs = mapOf("fixture-model" to ModelPricingSpec("fixture-model"))
                    )
                    val session = ChatSession(
                        "chat-bulk",
                        "Bulk",
                        1L,
                        1L,
                        profile.id,
                        workspacePath = "",
                        workspaceMode = WorkspaceMode.OFF,
                        permissionOverrides = noWorkspace.asOverrides()
                    )
                    val broker = ToolApprovalBroker("run-bulk", "chat-bulk", { ToolAccessScope(noWorkspace) })
                    val run = async(AgentToolRunContext(broker)) {
                        withTimeout(12_000) {
                            AiProviderEngine.streamGeneration(
                                profile = profile,
                                session = session,
                                historyNodes = listOf(MessageNode("u", MessageRole.USER, listOf(MessageVariant("v", content = "Use tool 5"))))
                            ).toList()
                        }
                    }
                    // Step 1 (mcp_schema) runs without blocking for approval; Step 2 (mcp__Bulk__tool_5) still requires approval.
                    val approvalReq = withTimeout(8_000) { broker.requests.first { it.isNotEmpty() }.single() }
                    assertEquals("mcp__Bulk__tool_5", approvalReq.toolName)
                    assertTrue(broker.respond(approvalReq.requestId, ToolApprovalDecision.ALLOW_ONCE))
                    val completed = run.await().filterIsInstance<StreamUpdate.Completed>().single()
                    assertEquals("Expanded and ran.", completed.fullContent)
                    assertEquals(3, requests.size)

                    val req0Tools = requests[0].getValue("tools").jsonArray.map { it.jsonObject.getValue("name").jsonPrimitive.content }
                    assertTrue(WorkspaceHarness.MCP_SCHEMA_TOOL_NAME in req0Tools, "Request 0 must advertise mcp_schema: $req0Tools")
                    assertTrue(req0Tools.none { WorkspaceHarness.isMcpToolName(it) }, "Request 0 must defer all 8 raw MCP schemas: $req0Tools")
                    assertTrue("Deferred MCP Tools (8 available)" in requests[0].toString(), "Request 0 prompt must list deferred MCP catalog")

                    val req1Tools = requests[1].getValue("tools").jsonArray.map { it.jsonObject.getValue("name").jsonPrimitive.content }
                    assertTrue("mcp__Bulk__tool_5" in req1Tools, "Request 1 must advertise expanded mcp__Bulk__tool_5: $req1Tools")
                    assertFalse("mcp__Bulk__tool_1" in req1Tools, "Unrequested MCP tools must remain deferred: $req1Tools")
                    assertTrue("http says hi" in requests[2].toString())
                    broker.close()
                } finally {
                    providerServer.stop(0)
                }
            }
        }
    }
}
