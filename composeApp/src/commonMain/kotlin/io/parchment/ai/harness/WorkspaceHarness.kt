package io.parchment.ai.harness

import io.parchment.ai.model.AgentPermissionPolicy
import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.ShellAccess
import io.parchment.ai.model.ToolApprovalMode
import io.parchment.ai.model.ToolCallRecord
import io.parchment.ai.model.WorkspaceMode
import io.parchment.ai.platform.PlatformRuntime
import io.parchment.ai.platform.PlatformStorage
import io.parchment.ai.platform.PlatformWorkspace
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * The agent's local tools: workspace files, shell, web, goals and todo lists. Runs in-process (no external
 * command-line bridge) and serves the tool loops of every provider wire.
 */
object WorkspaceHarness {

    internal val json = Json { ignoreUnknownKeys = true; isLenient = true }

    data class ToolDefinition(
        val name: String,
        val description: String,
        val parametersSchema: JsonObject,
        val requiresWritePermission: Boolean,
        val requiresShellPermission: Boolean = false,
        val requiresWorkspaceFolder: Boolean = true
    ) {
        val parametersSchemaString: String by lazy { parametersSchema.toString() }
    }

    data class WorkspaceSkill(val name: String, val description: String, val path: String)

    internal const val MCP_TOOL_PREFIX = WorkspaceToolCatalog.MCP_TOOL_PREFIX

    internal fun isMcpToolName(name: String): Boolean = WorkspaceToolCatalog.isMcpToolName(name)

    internal fun registerExternalToolsProvider(provider: () -> List<ToolDefinition>) =
        WorkspaceToolCatalog.registerExternalToolsProvider(provider)

    fun allTools(): List<ToolDefinition> = WorkspaceToolCatalog.allTools()

    fun invalidateWorkspaceCache(workspaceRoot: String = "") =
        WorkspaceContextCache.invalidate(workspaceRoot)

    /** Reads the `name` and `description` of a SKILL.md, including folded YAML descriptions. */
    internal fun parseSkillHeader(text: String): Pair<String, String>? =
        WorkspaceContextCache.parseSkillHeader(text)

    fun discoverSkills(workspaceRoot: String): List<WorkspaceSkill> =
        WorkspaceContextCache.discoverSkills(workspaceRoot)

    fun listDiscoveredContextFileNames(workspaceRoot: String): List<String> =
        WorkspaceContextCache.listDiscoveredContextFileNames(workspaceRoot)

    fun isParallelSafeReadOnlyTool(toolName: String): Boolean =
        WorkspaceToolCatalog.isParallelSafeReadOnlyTool(toolName)

    fun activeToolsForMode(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): List<ToolDefinition> = WorkspaceToolCatalog.activeToolsForMode(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    /**
     * Auto-discovers read-only AGENTS.md, CLAUDE.md, and PARCHMENT.md in the workspace root.
     * Cached in memory with a 10s TTL (and invalidated immediately on workspace writes) so UI frame rendering
     * and `o200k_base` context breakdown never block on synchronous disk I/O.
     */
    fun discoverWorkspaceContextFiles(workspaceRoot: String): String =
        WorkspaceContextCache.discoverContextFiles(workspaceRoot)

    /** Auto-discovers Parchment's global `memory/AGENT.md` when created in application storage. */
    fun discoverGlobalAgentMemory(appStorageRoot: String = ""): String =
        WorkspaceContextCache.discoverGlobalMemory(appStorageRoot)

    /**
     * Formats active workspace & utility tools for Google Gemini Interactions API (`/v1beta/interactions`)
     * and OpenAI Responses API (`/v1/responses`), which share the flat `{"type":"function","name":...,"parameters":...}` schema.
     */
    fun buildFlatFunctionTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = WorkspaceToolCatalog.buildFlatFunctionTools(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    fun buildGeminiInteractionsTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = WorkspaceToolCatalog.buildFlatFunctionTools(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    fun buildOpenAiResponsesTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = WorkspaceToolCatalog.buildFlatFunctionTools(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    /**
     * Formats active workspace & utility tools for OpenAI `/v1/chat/completions`.
     */
    fun buildOpenAiChatCompletionsTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = WorkspaceToolCatalog.buildOpenAiChatCompletionsTools(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    /**
     * Formats active workspace & utility tools for Anthropic Messages API (`/v1/messages`).
     */
    fun buildAnthropicTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = WorkspaceToolCatalog.buildAnthropicTools(
        mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy
    )

    private fun parseArgs(argumentsJson: String): JsonObject =
        runCatching { json.parseToJsonElement(argumentsJson.ifBlank { "{}" }).jsonObject }
            .getOrElse { JsonObject(emptyMap()) }

    private inline fun requireWriteMode(
        mode: WorkspaceMode,
        action: String,
        block: () -> Result<String>
    ): Result<String> = if (mode != WorkspaceMode.READ_WRITE) {
        Result.failure(IllegalStateException("Workspace is in Read-Only mode. Switch to Agent R/W mode $action."))
    } else {
        block()
    }

    /**
     * Formats a concise, present-participle intent description for live UI status telemetry
     * while a workspace or goal tool is executing.
     */
    fun describeToolCallIntent(toolName: String, argumentsJson: String): String {
        val args = parseArgs(argumentsJson)
        fun strArg(key: String): String = args[key]?.jsonPrimitive?.contentOrNull?.trim().orEmpty()

        return when (toolName) {
            "fs_read" -> {
                val path = strArg("path").ifBlank { "file" }
                val start = args["start_line"]?.jsonPrimitive?.intOrNull ?: 1
                val max = args["max_lines"]?.jsonPrimitive?.intOrNull ?: 250
                "Reading $path (lines $start..${start + max - 1})"
            }
            "fs_edit" -> "Editing ${strArg("path").ifBlank { "file" }}"
            "fs_multi_edit" -> {
                val count = (args["edits"] as? JsonArray)?.size ?: 0
                "Multi-editing ${strArg("path").ifBlank { "file" }} ($count hunks)"
            }
            "fs_write" -> "Writing ${strArg("path").ifBlank { "file" }}"
            "fs_move" -> "Moving ${strArg("source_path")} -> ${strArg("dest_path")}"
            "fs_delete" -> "Deleting ${strArg("path")}"
            "fs_list" -> "Listing workspace ${strArg("path").ifBlank { "root" }}"
            "fs_glob" -> "Matching files with glob '${strArg("pattern")}'"
            "fs_search" -> "Searching workspace for '${strArg("query")}'"
            "web_search" -> "Searching the web for '${strArg("query")}'"
            "web_fetch" -> "Fetching URL ${strArg("url")}"
            "calc_eval" -> "Calculating ${strArg("expression")}"
            "shell_exec" -> {
                val cmd = strArg("command").trim()
                if (cmd.isNotEmpty()) "Running shell_exec\n$ $cmd" else "Running shell_exec"
            }
            "todo_write" -> "Updating chat task checklist"
            "goal_complete" -> "Verifying & completing goal"
            "ask_user" -> "Waiting for your answer"
            "skill_load" -> "Loading skill ${strArg("name")}"
            "agent_delegate" -> "Helper working: ${strArg("task").take(80)}"
            ComputerUse.TOOL_NAME -> when (strArg("action")) {
                "screenshot" -> "Looking at the screen"
                "click", "double_click", "right_click" -> "${strArg("action").replace('_', ' ').replaceFirstChar { it.uppercase() }} at ${strArg("x")}, ${strArg("y")}"
                "type" -> "Typing: ${strArg("text").take(60)}"
                "key" -> "Pressing ${strArg("keys")}"
                else -> "Using the computer"
            }
            "agent_memory" -> "Accessing global memory/AGENT.md (${strArg("action").ifBlank { "read" }})"
            "dice_roll" -> "Rolling ${strArg("expression").ifBlank { "1d20" }}"
            else -> "Executing $toolName"
        }
    }

    /**
     * Executes a single tool call requested by the model inside the sandboxed workspace directory
     * (or executes workspace-independent goal/dice tools even when no folder is mounted).
     */
    fun executeToolCall(
        workspaceRoot: String,
        mode: WorkspaceMode,
        callId: String,
        toolName: String,
        argumentsJson: String,
        onOutputDelta: ((String) -> Unit)? = null,
        allowShell: Boolean = false,
        enableWebTools: Boolean = false,
        goalModeActive: Boolean = false,
        accessScope: ToolAccessScope? = null,
        permit: ToolExecutionPermit? = null,
        sessionId: String = "",
        appStorageRoot: String = "",
        isCancelled: () -> Boolean = { false }
    ): ToolCallRecord {
        val toolStartMs = PlatformRuntime.monotonicTimeMillis()
        val scope = accessScope ?: ToolAccessScope(AgentPermissionPolicy(
            workspacePath = workspaceRoot, workspaceMode = mode,
            shellAccess = if (allowShell) ShellAccess.FULL_ACCESS else ShellAccess.DISABLED,
            webAccess = enableWebTools, approvalMode = ToolApprovalMode.NEVER
        ), goalModeActive)
        val denial = when {
            isCancelled() -> "The owning run was cancelled before tool execution."
            scope.policy.workspacePath != workspaceRoot || scope.policy.workspaceMode != mode || scope.policy.allowShell != allowShell -> "Tool dispatch does not match the current permission scope."
            else -> AgentPermissionGate.denial(scope, toolName, argumentsJson)
                ?: if (permit != null) permit.claim(scope, toolName, argumentsJson)
                else if (AgentPermissionGate.effect(scope, toolName, argumentsJson).requiresApproval) "Tool approval is required before execution."
                else null
        }
        if (denial != null) {
            return ToolCallRecord(
                id = callId,
                name = toolName,
                argumentsJson = argumentsJson,
                resultText = "Error: $denial",
                isError = true
            )
        }

        val args = parseArgs(argumentsJson)
        fun strArg(key: String, default: String = ""): String =
            args[key]?.jsonPrimitive?.contentOrNull ?: default

        val result: Result<String> = when (toolName) {
            "todo_write" -> runCatching {
                val goal = strArg("goal", "")
                val parsed = ToolOutputAndGoalSupport.parseTodoItems(args)
                val done = parsed.count { it.status == "completed" }
                val active = parsed.count { it.status == "in_progress" }
                val pending = parsed.count { it.status == "pending" }
                val mdSummary = buildString {
                    if (goal.isNotBlank()) appendLine("# Goal: $goal")
                    parsed.forEach { t ->
                        val mark = when (t.status) {
                            "completed" -> "[x]"
                            "in_progress" -> "[/]"
                            "cancelled" -> "[-]"
                            else -> "[ ]"
                        }
                        appendLine("- $mark (${t.id}) ${t.content}")
                    }
                }.trim()
                if (sessionId.isNotBlank()) {
                    PlatformStorage.writeSessionTodoMarkdown(sessionId, mdSummary, appStorageRoot).getOrThrow()
                }
                "Updated Chat Todo Checklist ($done/${parsed.size} completed, $active in_progress, $pending pending).\n$mdSummary"
            }
            "skill_load" -> discoverSkills(workspaceRoot).find { it.name == strArg("name") }
                ?.let { PlatformWorkspace.workspaceRead(workspaceRoot, it.path, startLine = 1, maxLines = WorkspaceContextCache.SKILL_MAX_LINES) }
                ?: Result.failure(IllegalArgumentException("No skill named '${strArg("name")}' is available in this workspace."))
            ComputerUse.TOOL_NAME -> Result.failure(IllegalStateException("Computer control is not available in this run."))
            "agent_delegate" -> Result.failure(IllegalStateException("Helpers are not available in this run."))
            "ask_user" -> Result.failure(IllegalStateException("No one is available to answer in this run. Decide yourself and state your assumption."))
            "goal_complete" -> runCatching {
                val summary = strArg("summary", "Goal completed.")
                val verification = strArg("verification", "Verified all acceptance criteria.")
                "Goal marked 100% COMPLETE.\nSummary: $summary\nVerification: $verification"
            }
            "fs_list" -> PlatformWorkspace.workspaceList(
                workspaceRoot,
                strArg("path", ""),
                args["recursive"]?.jsonPrimitive?.booleanOrNull ?: false
            )
            "fs_glob" -> PlatformWorkspace.workspaceGlob(
                workspaceRoot,
                strArg("pattern"),
                args["max_results"]?.jsonPrimitive?.intOrNull ?: 200
            )
            "fs_read" -> PlatformWorkspace.workspaceRead(
                workspaceRoot,
                strArg("path"),
                args["start_line"]?.jsonPrimitive?.intOrNull ?: 1,
                args["max_lines"]?.jsonPrimitive?.intOrNull ?: 250
            )
            "fs_search" -> PlatformWorkspace.workspaceSearch(
                workspaceRoot,
                strArg("query"),
                strArg("file_glob", "")
            )
            "fs_write" -> requireWriteMode(mode, "to allow writing files") {
                PlatformWorkspace.workspaceWrite(workspaceRoot, strArg("path"), strArg("content"))
            }
            "fs_edit" -> requireWriteMode(mode, "to allow editing files") {
                PlatformWorkspace.workspaceEdit(
                    workspaceRoot,
                    strArg("path"),
                    strArg("old_text"),
                    strArg("new_text"),
                    args["replace_all"]?.jsonPrimitive?.booleanOrNull ?: false
                )
            }
            "fs_multi_edit" -> requireWriteMode(mode, "to allow editing files") {
                val editsArr = args["edits"] as? JsonArray ?: JsonArray(emptyList())
                val parsedEdits = editsArr.mapNotNull { el ->
                    val obj = el as? JsonObject ?: return@mapNotNull null
                    val oldTxt = obj["old_text"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
                    val newTxt = obj["new_text"]?.jsonPrimitive?.contentOrNull ?: ""
                    val repAll = obj["replace_all"]?.jsonPrimitive?.booleanOrNull ?: false
                    Triple(oldTxt, newTxt, repAll)
                }
                PlatformWorkspace.workspaceMultiEdit(workspaceRoot, strArg("path"), parsedEdits)
            }
            "fs_move" -> requireWriteMode(mode, "to allow moving files") {
                PlatformWorkspace.workspaceMove(workspaceRoot, strArg("source_path"), strArg("dest_path"))
            }
            "fs_delete" -> requireWriteMode(mode, "to allow deleting files") {
                PlatformWorkspace.workspaceDelete(workspaceRoot, strArg("path"))
            }
            "web_search" -> {
                val query = strArg("query")
                val maxRes = (args["max_results"]?.jsonPrimitive?.intOrNull ?: 6).coerceIn(1, 15)
                executeFreeWebSearch(query, maxRes, isCancelled)
            }
            "web_fetch" -> PlatformWorkspace.fetchUrlText(
                strArg("url"),
                args["max_chars"]?.jsonPrimitive?.intOrNull ?: 24000,
                isCancelled
            )
            "calc_eval" -> runCatching { evaluateMathExpression(strArg("expression")) }
            "dice_roll" -> runCatching { MathAndDiceEvaluator.evaluateDice(strArg("expression", "1d20"), strArg("reason", "")) }
            "agent_memory" -> {
                val action = strArg("action", "read").lowercase()
                val content = strArg("content", "")
                when (action) {
                    "read" -> PlatformStorage.readGlobalAgentMemory(maxLines = 500, storageRootOverride = appStorageRoot)
                        .recover { "(memory/AGENT.md does not exist yet.)" }
                    "append" -> {
                        if (mode == WorkspaceMode.READ_ONLY) {
                            Result.failure(IllegalStateException("Workspace is in Read-Only mode. Memory writes are disabled."))
                        } else if (PlatformStorage.hasGlobalAgentMemory(appStorageRoot)) {
                            val storageRoot = appStorageRoot.ifBlank { PlatformStorage.getAppStorageDirectoryPath() }
                            PlatformWorkspace.workspaceAppend(storageRoot, "memory/AGENT.md", "\n\n$content")
                        } else {
                            PlatformStorage.writeGlobalAgentMemory(content, storageRootOverride = appStorageRoot)
                        }
                    }
                    "overwrite" -> {
                        if (mode == WorkspaceMode.READ_ONLY) {
                            Result.failure(IllegalStateException("Workspace is in Read-Only mode. Memory writes are disabled."))
                        } else {
                            PlatformStorage.writeGlobalAgentMemory(content, storageRootOverride = appStorageRoot)
                        }
                    }
                    else -> Result.failure(IllegalArgumentException("Unknown agent_memory action '$action'."))
                }
            }
            "shell_exec" -> {
                if (!allowShell) {
                    Result.failure(IllegalStateException("Shell execution is disabled. Enable it explicitly before running commands."))
                } else {
                    requireWriteMode(mode, "to execute shell commands") {
                        PlatformWorkspace.workspaceShellExec(
                            workspaceRoot,
                            strArg("command"),
                            args["timeout_seconds"]?.jsonPrimitive?.intOrNull ?: 30,
                            onOutputDelta,
                            isCancelled
                        )
                    }
                }
            }
            else -> Result.failure(IllegalArgumentException("Unknown workspace tool '$toolName'."))
        }

        if (toolName in listOf("fs_write", "fs_edit", "fs_multi_edit", "fs_move", "fs_delete", "agent_memory", "shell_exec")) {
            invalidateWorkspaceCache(workspaceRoot)
        }

        val elapsedToolMs = (PlatformRuntime.monotonicTimeMillis() - toolStartMs).coerceAtLeast(1L)
        return result.fold(
            onSuccess = { output ->
                val shapedOutput = compressToolOutputWithSizeValve(toolName, output)
                ToolCallRecord(
                    id = callId,
                    name = toolName,
                    argumentsJson = argumentsJson,
                    resultText = shapedOutput,
                    isError = false,
                    durationMs = elapsedToolMs
                )
            },
            onFailure = { err ->
                ToolCallRecord(
                    id = callId,
                    name = toolName,
                    argumentsJson = argumentsJson,
                    resultText = "Tool Error (${err::class.simpleName}): ${err.message}",
                    isError = true,
                    durationMs = elapsedToolMs
                )
            }
        )
    }

    /**
     * Tidies tool output before it reaches the model:
     * - Drops ANSI escape codes, PowerShell progress XML (`#< CLIXML`) and runs of blank lines.
     * - Keeps only the head and tail of long `shell_exec`, `fs_search`, `fs_list` and `fs_glob` output, so one
     *   noisy command can't flood the context. Line ranges asked for with `fs_read` are never cut.
     */
    fun compressToolOutputWithSizeValve(toolName: String, rawOutput: String): String =
        ToolOutputAndGoalSupport.compressWithSizeValve(toolName, rawOutput)

    /**
     * Runs a keyless web search across these engines in turn:
     * 1. DuckDuckGo HTML (`https://html.duckduckgo.com/html/?q=...`)
     * 2. Wikipedia OpenSearch / Query API (`https://en.wikipedia.org/w/api.php?...`)
     * 3. HackerNews Algolia Search API (`https://hn.algolia.com/api/v1/search?query=...`)
     */
    fun executeFreeWebSearch(query: String, maxResults: Int = 6, isCancelled: (() -> Boolean)? = null): Result<String> =
        WorkspaceWebSearch.search(query, maxResults, isCancelled)

    /**
     * Pure-Kotlin deterministic recursive-descent mathematical evaluator for `calc_eval`.
     * Supports +, -, *, /, %, ^, parentheses, pi, e, sqrt, cbrt, abs, round, floor, ceil, sin, cos, tan, asin, acos, atan, ln, log, log10, log2, exp.
     */
    fun evaluateMathExpression(expression: String): String =
        MathAndDiceEvaluator.evaluateMath(expression)

    /**
     * Parses `todo_write` or `goal_complete` tool calls into an updated `GoalTrackerState`.
     */
    fun applyGoalToolUpdate(current: GoalTrackerState?, tc: ToolCallRecord): GoalTrackerState? =
        ToolOutputAndGoalSupport.applyGoalToolUpdate(current, tc)

    /**
     * 3-Layer Rolling Context Compactor.
     * Keeps the most recent `keepRecentCount` tool outputs verbatim, while compacting older verbose
     * tool outputs (>850 chars) into concise 1-line receipts so multi-day runs never overflow context.
     */
    fun compactStaleToolOutputText(
        stepIndex: Int,
        totalSteps: Int,
        toolName: String,
        rawOutput: String,
        keepRecentCount: Int = 8
    ): String = ToolOutputAndGoalSupport.compactStaleOutput(stepIndex, totalSteps, toolName, rawOutput, keepRecentCount)

    /**
     * Repeated-Failure Circuit Breaker & Stop-Hook Work-Check Gate.
     * - Replaces the arbitrary 64-step ceiling with intelligent progress vs. stagnation tracking.
     * - Allows unlimited productive steps over hours/days/weeks while halting repeated identical error loops.
     */
    class LoopCircuitBreaker(initialGoal: GoalTrackerState? = null) {
        var goalState: GoalTrackerState? = initialGoal
            private set

        private var lastFingerprint: String = ""
        private var lastToolName: String = ""
        var consecutiveIdenticalCalls: Int = 0
            private set
        var consecutiveTotalErrors: Int = 0
            private set
        var consecutiveUnproductiveStopHookNudges: Int = 0
            private set

        fun onToolExecuted(tc: ToolCallRecord): ToolCallRecord {
            val fp = "${tc.name}:${tc.argumentsJson.trim().hashCode()}"
            if (fp == lastFingerprint) {
                consecutiveIdenticalCalls++
            } else {
                lastFingerprint = fp
                lastToolName = tc.name
                consecutiveIdenticalCalls = 1
            }

            if (tc.isError) {
                consecutiveTotalErrors++
            } else {
                consecutiveTotalErrors = 0
                consecutiveUnproductiveStopHookNudges = 0
            }

            val updatedGoal = applyGoalToolUpdate(goalState, tc)
            if (updatedGoal != goalState) {
                goalState = updatedGoal
                consecutiveUnproductiveStopHookNudges = 0
            }

            // Inject diagnostic pivot warning if the model repeats the same failing call twice
            val shouldWarn = (consecutiveIdenticalCalls == 2 && tc.isError) ||
                (consecutiveIdenticalCalls == 3 && tc.name != "fs_read" && tc.name != "todo_write")
            return if (shouldWarn) {
                tc.copy(
                    resultText = tc.resultText +
                        "\n\n[HARNESS CIRCUIT BREAKER WARNING]: You have called '${tc.name}' with identical arguments $consecutiveIdenticalCalls times in a row" +
                        (if (tc.isError) " and it failed each time." else ".") +
                        " Do NOT repeat this exact call again. Diagnose the root cause, inspect the target file/state, or try a different strategy."
                )
            } else {
                tc
            }
        }

        fun checkTripped(): String? {
            if (consecutiveIdenticalCalls >= 3 && consecutiveTotalErrors >= 2) {
                return "Circuit Breaker halted loop: '$lastToolName' failed $consecutiveIdenticalCalls times in a row with identical arguments."
            }
            if (consecutiveTotalErrors >= 6) {
                return "Circuit Breaker halted loop: 6 consecutive tool errors occurred without a single successful step."
            }
            if (consecutiveUnproductiveStopHookNudges >= 3) {
                return "Circuit Breaker halted loop: Work-check nudged 3 times without further tool or todo progress."
            }
            return null
        }

        /**
         * Decides whether a turn that ended without tool calls is really finished.
         * - With no `/goal` or todo list active (ordinary chat or writing), returns `null` and the turn ends.
         * - With unfinished goal or todo items, and the circuit breaker not tripped, returns a prompt that asks
         *   the model to carry on until they are done.
         */
        fun evaluateStopHookPrompt(): String? {
            val gs = goalState ?: return null
            if (!gs.hasPendingOrInProgress) return null
            if (consecutiveUnproductiveStopHookNudges >= 3) return null

            consecutiveUnproductiveStopHookNudges++
            val incompleteItems = gs.todos.filter {
                val s = it.status.lowercase()
                s == "pending" || s == "in_progress"
            }
            return if (incompleteItems.isNotEmpty()) {
                val remainingList = incompleteItems.joinToString("\n") {
                    "  - [${it.status.uppercase()}] (${it.id}) ${it.content}"
                }
                "[Harness Work-Check — Goal Incomplete (${gs.completedCount}/${gs.todos.size} done)]:\n" +
                    "You paused before completing all items in your checklist:\n$remainingList\n\n" +
                    "Continue executing the next incomplete step now. Verify your work (e.g. inspect modified files or run checks) before marking each step `completed` with `todo_write`, and call `goal_complete` only once 100% of acceptance criteria are verified."
            } else {
                "[Harness Work-Check — Goal Active]:\n" +
                    "Objective: \"${gs.goalObjective}\"\n" +
                    "Break this goal into verifiable steps using `todo_write` (mark step 1 `in_progress`), execute and verify each step thoroughly, and call `goal_complete` when 100% finished."
            }
        }
    }
}
