package io.parchment.ai.harness

import io.parchment.ai.model.AgentPermissionPolicy
import io.parchment.ai.model.WorkspaceMode
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Built-in workspace tool catalog, external MCP tool registration, and cached provider schema adapters.
 */
internal object WorkspaceToolCatalog {

    const val MCP_TOOL_PREFIX = "mcp__"
    const val MCP_SCHEMA_TOOL_NAME = "mcp_schema"
    const val DEFERRED_MCP_SCHEMA_THRESHOLD = 6

    fun isMcpToolName(name: String): Boolean = name.startsWith(MCP_TOOL_PREFIX)

    internal val MCP_SCHEMA_TOOL = WorkspaceHarness.ToolDefinition(
        name = MCP_SCHEMA_TOOL_NAME,
        description = "Load full JSON parameter schemas and enable deferred MCP tools listed under Deferred MCP Tools.",
        requiresWritePermission = false,
        requiresWorkspaceFolder = false,
        parametersSchema = buildJsonObject {
            put("type", "object")
            putJsonObject("properties") {
                putJsonObject("names") {
                    put("type", "array")
                    putJsonObject("items") { put("type", "string") }
                    put("description", "Exact mcp__... tool names or search keywords to inspect and enable.")
                }
            }
            putJsonArray("required") { add(JsonPrimitive("names")) }
        }
    )

    @kotlin.concurrent.Volatile
    private var externalToolsProvider: () -> List<WorkspaceHarness.ToolDefinition> = { emptyList() }

    fun registerExternalToolsProvider(provider: () -> List<WorkspaceHarness.ToolDefinition>) {
        externalToolsProvider = provider
    }

    private fun strProp(desc: String) = buildJsonObject {
        put("type", "string")
        put("description", desc)
    }

    private fun intProp(desc: String) = buildJsonObject {
        put("type", "integer")
        put("description", desc)
    }

    private fun boolProp(desc: String) = buildJsonObject {
        put("type", "boolean")
        put("description", desc)
    }

    private val ALL_TOOLS: List<WorkspaceHarness.ToolDefinition> = listOf(
        WorkspaceHarness.ToolDefinition(
            name = "fs_list",
            description = "List directory entries in workspace.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative directory path ('' for root)."))
                    put("recursive", boolProp("List subdirectories recursively."))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_glob",
            description = "Find workspace paths matching a glob (e.g. '**/*.kt', 'docs/*').",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("pattern", strProp("Glob pattern."))
                    put("max_results", intProp("Max results (default 200)."))
                }
                putJsonArray("required") { add(JsonPrimitive("pattern")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_read",
            description = "Read UTF-8 file lines in workspace with line numbers.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative file path."))
                    put("start_line", intProp("1-based start line (default 1)."))
                    put("max_lines", intProp("Max lines to read (default 250)."))
                }
                putJsonArray("required") { add(JsonPrimitive("path")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_search",
            description = "Search text or regex across workspace files (prioritizes code/prose over imports).",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("query", strProp("Literal text or regex pattern."))
                    put("path", strProp("Optional relative directory or single file to scope search."))
                    put("file_glob", strProp("Optional filename/extension filter (e.g. '.kt', 'Test.kt')."))
                    put("max_results", intProp("Max matches to return (default 150)."))
                }
                putJsonArray("required") { add(JsonPrimitive("query")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_write",
            description = "Create or overwrite a UTF-8 file in workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative file path."))
                    put("content", strProp("Full file text."))
                }
                putJsonArray("required") {
                    add(JsonPrimitive("path"))
                    add(JsonPrimitive("content"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_edit",
            description = "Surgical search-and-replace on an existing workspace file.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative file path."))
                    put("old_text", strProp("Existing text block to match."))
                    put("new_text", strProp("Replacement text block."))
                    put("replace_all", boolProp("Replace all matches (default false)."))
                }
                putJsonArray("required") {
                    add(JsonPrimitive("path"))
                    add(JsonPrimitive("old_text"))
                    add(JsonPrimitive("new_text"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_multi_edit",
            description = "Apply multiple search-and-replace hunks to one file atomically.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative file path."))
                    putJsonObject("edits") {
                        put("type", "array")
                        put("description", "Ordered hunks.")
                        putJsonObject("items") {
                            put("type", "object")
                            putJsonObject("properties") {
                                put("old_text", strProp("Existing text block."))
                                put("new_text", strProp("Replacement text block."))
                                put("replace_all", boolProp("Replace all matches (default false)."))
                            }
                            putJsonArray("required") {
                                add(JsonPrimitive("old_text"))
                                add(JsonPrimitive("new_text"))
                            }
                        }
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("path"))
                    add(JsonPrimitive("edits"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_move",
            description = "Move or rename a file or directory in workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("source_path", strProp("Existing relative path."))
                    put("dest_path", strProp("Destination relative path."))
                }
                putJsonArray("required") {
                    add(JsonPrimitive("source_path"))
                    add(JsonPrimitive("dest_path"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_delete",
            description = "Delete a file or directory in workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("path", strProp("Relative path to delete."))
                }
                putJsonArray("required") { add(JsonPrimitive("path")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "web_search",
            description = "Search the web for current docs and facts.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("query", strProp("Search query."))
                    put("max_results", intProp("Max results (default 6)."))
                }
                putJsonArray("required") { add(JsonPrimitive("query")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "web_fetch",
            description = "Fetch an HTTP/HTTPS URL as clean Markdown/text or JSON.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("url", strProp("Full http(s) URL."))
                    put("max_chars", intProp("Max chars (default 24000)."))
                }
                putJsonArray("required") { add(JsonPrimitive("url")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "calc_eval",
            description = "Evaluate a math/scientific expression (+, -, *, /, %, ^, sqrt, trig, ln, log, pi, e).",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("expression", strProp("Math expression."))
                }
                putJsonArray("required") { add(JsonPrimitive("expression")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "dice_roll",
            description = "Roll unbiased dice (e.g. '1d20', '2d6+3', '1d100').",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("expression", strProp("Dice notation."))
                    put("reason", strProp("Optional roll label."))
                }
                putJsonArray("required") { add(JsonPrimitive("expression")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "todo_write",
            description = "Update chat task checklist (never project files). Keep active step 'in_progress'; mark 'completed' once verified.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("goal", strProp("Goal objective and definition of done."))
                    putJsonObject("todos") {
                        put("type", "array")
                        put("description", "Ordered task steps.")
                        putJsonObject("items") {
                            put("type", "object")
                            putJsonObject("properties") {
                                put("id", strProp("Step ID (e.g. '1')."))
                                put("content", strProp("Actionable step and check."))
                                put("status", strProp("'pending', 'in_progress', 'completed', or 'cancelled'."))
                            }
                            putJsonArray("required") {
                                add(JsonPrimitive("id"))
                                add(JsonPrimitive("content"))
                                add(JsonPrimitive("status"))
                            }
                        }
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("todos")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = ComputerUse.TOOL_NAME,
            description = "Control this device (each action requires approval). Start with 'screenshot'; coordinates match latest screenshot.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("action") {
                        put("type", "string")
                        putJsonArray("enum") {
                            listOf("screenshot", "click", "double_click", "right_click", "move", "type", "key", "scroll")
                                .forEach { add(JsonPrimitive(it)) }
                        }
                    }
                    putJsonObject("x") { put("type", "integer") }
                    putJsonObject("y") { put("type", "integer") }
                    put("text", strProp("Text for 'type'."))
                    put("keys", strProp("Key chord (e.g. ctrl+c, enter; Android: back, home, recents, notifications)."))
                    put("amount", intProp("Scroll delta (+ down, - up)."))
                }
                putJsonArray("required") { add(JsonPrimitive("action")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "agent_delegate",
            description = "Delegate a self-contained read/search task to an isolated helper that returns only findings.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("task", strProp("Self-contained read/search task."))
                }
                putJsonArray("required") { add(JsonPrimitive("task")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "skill_load",
            description = "Load full instructions of a skill listed under Available skills.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("name", strProp("Exact skill name."))
                }
                putJsonArray("required") { add(JsonPrimitive("name")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "ask_user",
            description = "Pause to ask the user a genuine decision or missing input (never for routine confirmation).",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("question", strProp("Self-contained question."))
                    putJsonObject("options") {
                        put("type", "array")
                        putJsonObject("items") { put("type", "string") }
                        put("description", "Optional short choices.")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("question")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "goal_complete",
            description = "Mark the active goal complete once all acceptance criteria are verified.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("summary", strProp("Concise summary of completed work."))
                    put("verification", strProp("Checks run to verify completion."))
                }
                putJsonArray("required") { add(JsonPrimitive("summary")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "agent_memory",
            description = "Read or update global memory/AGENT.md across sessions when needed or requested.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("action", strProp("'read', 'append', or 'overwrite'."))
                    put("content", strProp("Notes for 'append' or 'overwrite'."))
                }
                putJsonArray("required") { add(JsonPrimitive("action")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "shell_exec",
            description = "Run a shell command in workspace and return stdout/stderr.",
            requiresWritePermission = true,
            requiresShellPermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    put("command", strProp("Shell command to run."))
                    put("timeout_seconds", intProp("Timeout in seconds (default 30, max 1800)."))
                }
                putJsonArray("required") { add(JsonPrimitive("command")) }
            }
        )
    )

    fun allTools(): List<WorkspaceHarness.ToolDefinition> = ALL_TOOLS + externalToolsProvider()

    fun allowedMcpTools(policy: AgentPermissionPolicy?): List<WorkspaceHarness.ToolDefinition> {
        if (policy != null && !policy.toolsEnabled) return emptyList()
        return externalToolsProvider().filter { tool -> policy == null || policy.allowsTool(tool.name) }
    }

    fun shouldDeferMcpSchemas(policy: AgentPermissionPolicy?): Boolean =
        allowedMcpTools(policy).size > DEFERRED_MCP_SCHEMA_THRESHOLD

    fun deferredMcpToolsCatalogPrompt(
        policy: AgentPermissionPolicy?,
        expandedMcpTools: Set<String> = emptySet()
    ): String {
        val allowed = allowedMcpTools(policy)
        if (allowed.size <= DEFERRED_MCP_SCHEMA_THRESHOLD) return ""
        return buildString {
            appendLine("### Deferred MCP Tools (${allowed.size} available)")
            appendLine("Full MCP parameter schemas are deferred to save context tokens. Call `$MCP_SCHEMA_TOOL_NAME(names=[...])` with exact tool name(s) or keywords to load schemas and enable them before calling:")
            allowed.forEach { tool ->
                val summary = tool.description.lineSequence().firstOrNull()?.trim().orEmpty().take(90)
                val status = if (tool.name in expandedMcpTools) " [loaded]" else ""
                if (summary.isNotEmpty()) {
                    appendLine("- `${tool.name}`$status: $summary")
                } else {
                    appendLine("- `${tool.name}`$status")
                }
            }
        }.trim()
    }

    fun isParallelSafeReadOnlyTool(toolName: String): Boolean = when (toolName.lowercase()) {
        "fs_read", "fs_list", "fs_glob", "fs_search", "skill_load", MCP_SCHEMA_TOOL_NAME,
        "web_search", "web_fetch", "calc_eval", "dice_roll" -> true
        else -> false
    }

    fun activeToolsForMode(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null,
        expandedMcpTools: Set<String>? = null
    ): List<WorkspaceHarness.ToolDefinition> {
        val hasMountedWorkspace = mode != WorkspaceMode.OFF && workspacePath.isNotBlank()
        val externalTools = externalToolsProvider()
        if (!hasMountedWorkspace && !goalModeActive && !enableWebTools && !includeLocalWebSearchTool && externalTools.isEmpty() && policy?.computerAccess != true) {
            return emptyList()
        }
        val deferMcp = expandedMcpTools != null && shouldDeferMcpSchemas(policy)
        val base = allTools().filter { tool ->
            if (policy != null && !policy.allowsTool(tool.name)) return@filter false
            if (isMcpToolName(tool.name) && deferMcp && tool.name !in expandedMcpTools) return@filter false
            if (tool.name == ComputerUse.TOOL_NAME && policy?.computerAccess != true) return@filter false
            if (tool.name in setOf("web_search", "web_fetch") && !enableWebTools) return@filter false
            if (tool.name == "web_search" && !includeLocalWebSearchTool) return@filter false
            if (!hasMountedWorkspace) {
                if (!goalModeActive) {
                    tool.name == "web_search" || tool.name == "web_fetch" || tool.name == "calc_eval" ||
                        tool.name == "dice_roll" || tool.name == "ask_user" || tool.name == "agent_delegate" ||
                        tool.name == "todo_write" || tool.name == "goal_complete" || tool.name == "agent_memory" ||
                        tool.name == ComputerUse.TOOL_NAME || isMcpToolName(tool.name)
                } else {
                    !tool.requiresWorkspaceFolder
                }
            } else {
                val writeOk = mode == WorkspaceMode.READ_WRITE || !tool.requiresWritePermission
                val shellOk = !tool.requiresShellPermission || (mode == WorkspaceMode.READ_WRITE && allowShell)
                writeOk && shellOk
            }
        }
        return if (deferMcp && (policy == null || policy.allowsTool(MCP_SCHEMA_TOOL_NAME))) {
            base + MCP_SCHEMA_TOOL
        } else {
            base
        }
    }

    fun buildFlatFunctionTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null,
        expandedMcpTools: Set<String>? = emptySet()
    ): JsonArray = JsonArray(
        activeToolsForMode(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy, expandedMcpTools)
            .map { it.flatFunctionJson }
    )

    fun buildOpenAiChatCompletionsTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null,
        expandedMcpTools: Set<String>? = emptySet()
    ): JsonArray = JsonArray(
        activeToolsForMode(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy, expandedMcpTools)
            .map { it.openAiChatJson }
    )

    fun buildAnthropicTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null,
        expandedMcpTools: Set<String>? = emptySet()
    ): JsonArray = JsonArray(
        activeToolsForMode(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy, expandedMcpTools)
            .map { it.anthropicJson }
    )
}
