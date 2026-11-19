package io.parchment.ai.network

import io.parchment.ai.harness.WorkspaceHarness
import io.parchment.ai.model.McpServerConfig
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformWorkspace
import io.parchment.ai.strings.Strings
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.IO
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withTimeoutOrNull

enum class McpServerState { CONNECTING, READY, FAILED }
data class McpServerStatus(val state: McpServerState, val toolCount: Int = 0, val message: String = "")

/**
 * Live connections to the enabled MCP servers and the tools they offer. A server that stops is started again after
 * a short wait, keeping its tool names, at most [MAX_STARTS_PER_WINDOW] times in [START_WINDOW_MS]; the call that
 * found it stopped is never sent again for the model, because its effect is unknown.
 */
object McpRegistry {
    private const val PREFIX = WorkspaceHarness.MCP_TOOL_PREFIX
    private const val MAX_NAME_CHARS = 64 // The tightest limit among the provider APIs.
    private const val CONNECT_TIMEOUT_MS = 30_000L
    private val RESTART_WAITS_MS = longArrayOf(1_000L, 4_000L, 15_000L)
    private const val MAX_STARTS_PER_WINDOW = 3
    private const val START_WINDOW_MS = 10 * 60_000L

    /** How long a call waits for a restart in progress before saying the server is unavailable. */
    private const val RESTART_CALL_WAIT_MS = 25_000L

    private class Connection(val config: McpServerConfig, val client: McpClient)
    private data class RemoteTool(val definition: WorkspaceHarness.ToolDefinition, val serverId: String, val remoteName: String)

    private val lock = Mutex()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    // Guarded by lock: the servers the last sync asked for, and when each was started recently.
    private var synced: Map<String, McpServerConfig> = emptyMap()
    private val startTimes = HashMap<String, ArrayDeque<Long>>()
    private val restartJobs = MutableStateFlow<Map<String, Job>>(emptyMap())
    private val connectionsState = MutableStateFlow<Map<String, Connection>>(emptyMap())
    private var connections: Map<String, Connection>
        get() = connectionsState.value
        set(value) { connectionsState.value = value }
    private val toolsState = MutableStateFlow<List<RemoteTool>>(emptyList())
    private val statusState = MutableStateFlow<Map<String, McpServerStatus>>(emptyMap())
    val statuses: StateFlow<Map<String, McpServerStatus>> = statusState.asStateFlow()
    private val definitionsState = MutableStateFlow<List<WorkspaceHarness.ToolDefinition>>(emptyList())
    val definitions: StateFlow<List<WorkspaceHarness.ToolDefinition>> = definitionsState.asStateFlow()

    init {
        WorkspaceHarness.registerExternalToolsProvider { definitionsState.value }
    }

    fun isMcpTool(name: String): Boolean = WorkspaceHarness.isMcpToolName(name)

    private fun slug(raw: String) = raw.replace(Regex("[^a-zA-Z0-9_-]"), "_")

    private fun qualifiedName(server: McpServerConfig, tool: String, taken: Set<String>): String {
        val base = "$PREFIX${slug(server.name.ifBlank { server.id }).take(20)}__${slug(tool)}".take(MAX_NAME_CHARS)
        if (base !in taken) return base
        return generateSequence(2) { it + 1 }.map { n ->
            val suffix = "_$n"
            "${base.take((MAX_NAME_CHARS - suffix.length).coerceAtLeast(1))}$suffix"
        }.first { it !in taken }
    }

    private fun setStatus(id: String, status: McpServerStatus?) {
        statusState.value = if (status == null) statusState.value - id else statusState.value + (id to status)
    }

    private suspend fun open(config: McpServerConfig): McpClient {
        val transport = if (config.command.isNotBlank()) {
            ProcessMcpTransport(PlatformWorkspace.startMcpProcess(config.command) ?: error("Local MCP servers need the desktop app."))
        } else {
            HttpMcpTransport(config.url.trim(), config.bearerToken.trim())
        }
        return McpClient(transport)
    }

    /** Makes the live connections match [configs]; a changed server is reconnected. */
    suspend fun sync(configs: List<McpServerConfig>) = lock.withLock {
        val wanted = configs.filter { it.enabled && (it.url.isNotBlank() || it.command.isNotBlank()) }.associateBy { it.id }
        synced = wanted
        // A sync settles every server itself, so a restart planned before it no longer applies.
        restartJobs.value.values.forEach { it.cancel() }
        restartJobs.value = emptyMap()
        startTimes.keys.retainAll(wanted.keys)
        connections.values.filter { wanted[it.config.id] != it.config }.forEach { connection ->
            connections = connections - connection.config.id
            runCatching { connection.client.close() }
            setStatus(connection.config.id, null)
        }
        statusState.value.keys.filter { it !in wanted }.forEach { setStatus(it, null) }
        toolsState.value = toolsState.value.filter { it.serverId in connections }
        publish()
        for (config in wanted.values) {
            if (config.id in connections) continue
            connectServer(config)
        }
    }

    /**
     * Starts [config]'s server and offers its tools, under the names they had if it ran before. Call with [lock] held.
     * A timeout counts as this server's failure; any other cancellation is passed on.
     */
    private suspend fun connectServer(config: McpServerConfig): Boolean {
        startTimes.getOrPut(config.id) { ArrayDeque() }.addLast(PlatformRuntime.monotonicTimeMillis())
        setStatus(config.id, McpServerStatus(McpServerState.CONNECTING))
        var client: McpClient? = null
        try {
            val tools = withTimeout(CONNECT_TIMEOUT_MS) {
                open(config).also { client = it }.let { opened -> opened.connect(); opened.listTools() }
            }
            val others = toolsState.value.filter { it.serverId != config.id }
            val previousNames = toolsState.value.filter { it.serverId == config.id }.associate { it.remoteName to it.definition.name }
            val taken = others.map { it.definition.name }.toMutableSet()
            val added = tools.map { tool ->
                val name = (previousNames[tool.name]?.takeIf { it !in taken } ?: qualifiedName(config, tool.name, taken)).also { taken += it }
                RemoteTool(
                    WorkspaceHarness.ToolDefinition(
                        name = name,
                        // A server's own words are untrusted, so the model is told where they came from.
                        description = "[From MCP server \"${config.name.ifBlank { config.id }}\"] ${tool.description}".take(1024),
                        parametersSchema = tool.inputSchema,
                        requiresWritePermission = false,
                        requiresWorkspaceFolder = false
                    ),
                    config.id,
                    tool.name
                )
            }
            connections = connections + (config.id to Connection(config, checkNotNull(client)))
            toolsState.value = (others + added).sortedBy { it.definition.name }
            publish()
            setStatus(config.id, McpServerStatus(McpServerState.READY, added.size))
            return true
        } catch (cancelled: CancellationException) {
            withContext(NonCancellable) { runCatching { client?.close() } }
            setStatus(config.id, null)
            if (cancelled !is kotlinx.coroutines.TimeoutCancellationException) throw cancelled
            setStatus(config.id, McpServerStatus(McpServerState.FAILED, message = "The server did not respond."))
        } catch (error: Exception) {
            runCatching { client?.close() }
            setStatus(config.id, McpServerStatus(McpServerState.FAILED, message = error.message?.take(200) ?: "Could not connect."))
        }
        return false
    }

    /** Whether [serverId] may be started automatically again: fewer than the allowed starts in the window. */
    private fun mayStartAgain(serverId: String): Boolean {
        val times = startTimes[serverId] ?: return true
        val now = PlatformRuntime.monotonicTimeMillis()
        while (times.isNotEmpty() && now - times.first() > START_WINDOW_MS) times.removeFirst()
        return times.size < MAX_STARTS_PER_WINDOW
    }

    private fun stopOffering(serverId: String, message: String) {
        toolsState.value = toolsState.value.filter { it.serverId != serverId }
        publish()
        setStatus(serverId, McpServerStatus(McpServerState.FAILED, message = message))
    }

    private fun publish() { definitionsState.value = toolsState.value.map { it.definition } }

    internal suspend fun call(name: String, argumentsJson: String): McpToolResult {
        val tool = toolsState.value.find { it.definition.name == name } ?: return McpToolResult("This MCP tool is no longer available.", true)
        val arguments = parseMcpArguments(argumentsJson) ?: return McpToolResult("Tool arguments must be a valid JSON object.", true)
        val client = connections[tool.serverId]?.client ?: awaitRestart(tool.serverId)
            ?: return McpToolResult("This MCP server is no longer connected.", true)
        return try {
            client.callTool(tool.remoteName, arguments)
        } catch (cancelled: CancellationException) {
            if (cancelled is kotlinx.coroutines.TimeoutCancellationException) McpToolResult("The MCP server did not respond in time.", true) else throw cancelled
        } catch (error: Exception) {
            if (client.isAlive) return McpToolResult(error.message?.take(500) ?: "The MCP call failed.", true)
            McpToolResult(
                if (markStopped(tool.serverId, client)) {
                    "The MCP server stopped during this call and is being restarted. Its effect is unknown: check before running it again."
                } else "The MCP server stopped during this call.",
                true
            )
        }
    }

    /** Waits, within a bound, for a restart in progress; the server's new client, or null. */
    private suspend fun awaitRestart(serverId: String): McpClient? {
        val job = restartJobs.value[serverId] ?: return null
        withTimeoutOrNull(RESTART_CALL_WAIT_MS) { job.join() }
        return connections[serverId]?.client
    }

    /**
     * Retires a client that stopped and plans its restart. Its tools stay offered under the same names meanwhile.
     * Returns whether a restart is under way.
     */
    private suspend fun markStopped(serverId: String, client: McpClient): Boolean = lock.withLock {
        if (connections[serverId]?.client !== client) return@withLock serverId in restartJobs.value
        connections = connections - serverId
        runCatching { client.close() }
        val config = synced[serverId]
        if (config == null || !mayStartAgain(serverId)) {
            stopOffering(serverId, Strings.mcpServerKeepsStopping)
            return@withLock false
        }
        setStatus(serverId, McpServerStatus(McpServerState.CONNECTING, message = Strings.mcpServerRestarting))
        // Registered before it starts, so a call arriving meanwhile always finds it.
        val job = scope.launch(start = CoroutineStart.LAZY) {
            try {
                for (wait in RESTART_WAITS_MS) {
                    delay(wait)
                    val settled = lock.withLock {
                        when {
                            synced[serverId] != config || serverId in connections -> true // Changed, removed or back.
                            !mayStartAgain(serverId) -> { stopOffering(serverId, Strings.mcpServerKeepsStopping); true }
                            else -> connectServer(config)
                        }
                    }
                    if (settled) return@launch
                }
                lock.withLock {
                    if (synced[serverId] == config && serverId !in connections) stopOffering(serverId, Strings.mcpServerCouldNotRestart)
                }
            } finally {
                val self = currentCoroutineContext()[Job]
                restartJobs.update { jobs -> if (jobs[serverId] === self) jobs - serverId else jobs }
            }
        }
        restartJobs.update { it + (serverId to job) }
        job.start()
        true
    }

    /** A person's Restart: starts a stopped server now, whatever the automatic limit says. */
    fun restartNow(serverId: String) {
        scope.launch {
            lock.withLock {
                val config = synced[serverId] ?: return@withLock
                if (serverId in connections) return@withLock
                restartJobs.value[serverId]?.cancel()
                startTimes.remove(serverId)
                connectServer(config)
            }
        }
    }
}
