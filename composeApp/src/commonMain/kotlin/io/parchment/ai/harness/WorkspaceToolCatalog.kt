package io.parchment.ai.harness

import io.parchment.ai.model.AgentPermissionPolicy
import io.parchment.ai.model.WorkspaceMode
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Built-in workspace tool catalog, external MCP tool registration, and provider-specific schema adapters.
 */
internal object WorkspaceToolCatalog {

    const val MCP_TOOL_PREFIX = "mcp__"

    fun isMcpToolName(name: String): Boolean = name.startsWith(MCP_TOOL_PREFIX)

    @kotlin.concurrent.Volatile
    private var externalToolsProvider: () -> List<WorkspaceHarness.ToolDefinition> = { emptyList() }

    fun registerExternalToolsProvider(provider: () -> List<WorkspaceHarness.ToolDefinition>) {
        externalToolsProvider = provider
    }

    private val ALL_TOOLS: List<WorkspaceHarness.ToolDefinition> = listOf(
        WorkspaceHarness.ToolDefinition(
            name = "fs_list",
            description = "List files and directories inside the mounted local workspace folder.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative directory path inside workspace (empty string for root).")
                    }
                    putJsonObject("recursive") {
                        put("type", "boolean")
                        put("description", "Whether to list subdirectories recursively.")
                    }
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_glob",
            description = "Find files and directories matching a glob pattern (e.g. '*.kt', '**/*.md', 'src/**/Test*') inside the mounted workspace.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("pattern") {
                        put("type", "string")
                        put("description", "Glob pattern to match (e.g. '*.kt', '**/*.md', 'docs/*').")
                    }
                    putJsonObject("max_results") {
                        put("type", "integer")
                        put("description", "Maximum number of matched paths to return (default 200).")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("pattern")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_read",
            description = "Read the UTF-8 text contents of a file inside the mounted local workspace with line numbers.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative path to the file inside the workspace.")
                    }
                    putJsonObject("start_line") {
                        put("type", "integer")
                        put("description", "1-based starting line number (default 1).")
                    }
                    putJsonObject("max_lines") {
                        put("type", "integer")
                        put("description", "Maximum number of lines to read (default 250).")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("path")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_search",
            description = "Search for literal text or patterns across files in the mounted workspace (manuscript chapters, notes, or source code).",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("query") {
                        put("type", "string")
                        put("description", "Text or regex pattern to search for.")
                    }
                    putJsonObject("file_glob") {
                        put("type", "string")
                        put("description", "Optional file extension or substring filter (e.g. '.md', '.kt').")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("query")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_write",
            description = "Create a new file or overwrite an existing file inside the mounted workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative file path inside the workspace.")
                    }
                    putJsonObject("content") {
                        put("type", "string")
                        put("description", "Full text content to write to the file.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("path"))
                    add(JsonPrimitive("content"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_edit",
            description = "Perform a surgical search-and-replace edit on an existing file in the mounted workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative file path inside the workspace.")
                    }
                    putJsonObject("old_text") {
                        put("type", "string")
                        put("description", "Exact existing text block to find.")
                    }
                    putJsonObject("new_text") {
                        put("type", "string")
                        put("description", "Replacement text block.")
                    }
                    putJsonObject("replace_all") {
                        put("type", "boolean")
                        put("description", "Replace every occurrence if true (default false).")
                    }
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
            description = "Apply multiple search-and-replace edits to a single file atomically in one call. If any edit fails to match, all edits are rolled back.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative file path inside the workspace.")
                    }
                    putJsonObject("edits") {
                        put("type", "array")
                        put("description", "Ordered list of search-and-replace hunks to apply atomically.")
                        putJsonObject("items") {
                            put("type", "object")
                            putJsonObject("properties") {
                                putJsonObject("old_text") {
                                    put("type", "string")
                                    put("description", "Exact existing text block to find.")
                                }
                                putJsonObject("new_text") {
                                    put("type", "string")
                                    put("description", "Replacement text block.")
                                }
                                putJsonObject("replace_all") {
                                    put("type", "boolean")
                                    put("description", "Replace every occurrence if true (default false).")
                                }
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
            description = "Move or rename a file or directory inside the mounted workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("source_path") {
                        put("type", "string")
                        put("description", "Existing relative path inside the workspace.")
                    }
                    putJsonObject("dest_path") {
                        put("type", "string")
                        put("description", "Destination relative path inside the workspace.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("source_path"))
                    add(JsonPrimitive("dest_path"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "fs_delete",
            description = "Delete a file or directory inside the mounted workspace.",
            requiresWritePermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("path") {
                        put("type", "string")
                        put("description", "Relative path to delete inside the workspace.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("path"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "web_search",
            description = "Search the live internet for up-to-date information, documentation, news, and facts using a free multi-engine search cascade (DuckDuckGo, Wikipedia, HackerNews), returning titles, URLs, and snippets.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("query") {
                        put("type", "string")
                        put("description", "Search query keywords or question.")
                    }
                    putJsonObject("max_results") {
                        put("type", "integer")
                        put("description", "Maximum number of search results to return (default 6).")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("query"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "web_fetch",
            description = "Fetch the contents of any HTTP/HTTPS URL and extract clean readable Markdown/text or JSON.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("url") {
                        put("type", "string")
                        put("description", "Full http:// or https:// URL to fetch.")
                    }
                    putJsonObject("max_chars") {
                        put("type", "integer")
                        put("description", "Maximum characters to return (default 24000).")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("url"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "calc_eval",
            description = "Deterministically evaluate a mathematical or scientific expression (supports +, -, *, /, %, ^, parentheses, sqrt, sin, cos, tan, ln, log, abs, round, pi, e).",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("expression") {
                        put("type", "string")
                        put("description", "Mathematical expression to evaluate, e.g. '(1450 * 1.08^5) / sqrt(16)'.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("expression"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "dice_roll",
            description = "Roll unbiased random dice using standard notation (e.g. '1d20', '2d6+3', '4d6', '1d100') for roleplay checks or story branching outcomes.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("expression") {
                        put("type", "string")
                        put("description", "Dice notation such as '1d20', '2d6+3', or '1d100'.")
                    }
                    putJsonObject("reason") {
                        put("type", "string")
                        put("description", "Optional label or reason for the roll.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("expression"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "todo_write",
            description = "Create or update the structured task checklist for multi-step or long-horizon goals. Always keep one current step 'in_progress' and mark steps 'completed' as you verify them.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("goal") {
                        put("type", "string")
                        put("description", "Overarching goal objective and definition of done.")
                    }
                    putJsonObject("todos") {
                        put("type", "array")
                        put("description", "Ordered list of verifiable task steps.")
                        putJsonObject("items") {
                            put("type", "object")
                            putJsonObject("properties") {
                                putJsonObject("id") {
                                    put("type", "string")
                                    put("description", "Step ID such as '1', '2', '3'.")
                                }
                                putJsonObject("content") {
                                    put("type", "string")
                                    put("description", "Specific actionable step and verification check.")
                                }
                                putJsonObject("status") {
                                    put("type", "string")
                                    put("description", "'pending', 'in_progress', 'completed', or 'cancelled'.")
                                }
                            }
                            putJsonArray("required") {
                                add(JsonPrimitive("id"))
                                add(JsonPrimitive("content"))
                                add(JsonPrimitive("status"))
                            }
                        }
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("todos"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = ComputerUse.TOOL_NAME,
            description = "See and control this device. The user approves every action. Start with action 'screenshot'; each other action returns a new screenshot. Coordinates are pixels in the latest screenshot.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("action") {
                        put("type", "string")
                        putJsonArray("enum") { listOf("screenshot", "click", "double_click", "right_click", "move", "type", "key", "scroll").forEach { add(JsonPrimitive(it)) } }
                    }
                    putJsonObject("x") { put("type", "integer") }
                    putJsonObject("y") { put("type", "integer") }
                    putJsonObject("text") { put("type", "string"); put("description", "Text to enter, for 'type'.") }
                    putJsonObject("keys") { put("type", "string"); put("description", "For 'key', such as ctrl+c or enter. On Android: back, home or recents.") }
                    putJsonObject("amount") { put("type", "integer"); put("description", "For 'scroll': positive scrolls down, negative up.") }
                }
                putJsonArray("required") { add(JsonPrimitive("action")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "agent_delegate",
            description = "Hand a self-contained task to a helper with its own separate context, such as searching a large codebase. It can read and search but cannot change anything, and it returns only its findings, which keeps this conversation short. Give it everything it needs in the task, because it cannot see this conversation.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("task") {
                        put("type", "string")
                        put("description", "What the helper should find out, written so it makes sense on its own.")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("task")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "skill_load",
            description = "Load the full instructions of a skill listed under Available skills. Use it when a skill's description fits the task.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("name") {
                        put("type", "string")
                        put("description", "The skill's name, exactly as listed.")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("name")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "ask_user",
            description = "Pause and ask the user a question only they can answer, such as a decision between real options or missing information. Do not use it for confirmation of work you can check yourself.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("question") {
                        put("type", "string")
                        put("description", "The question, written so it makes sense on its own.")
                    }
                    putJsonObject("options") {
                        put("type", "array")
                        putJsonObject("items") { put("type", "string") }
                        put("description", "Optional short answers to offer. The user can always type their own.")
                    }
                }
                putJsonArray("required") { add(JsonPrimitive("question")) }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "goal_complete",
            description = "Signal that all steps and acceptance criteria of the active goal have been verified and completed.",
            requiresWritePermission = false,
            requiresWorkspaceFolder = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("summary") {
                        put("type", "string")
                        put("description", "Concise summary of all completed work.")
                    }
                    putJsonObject("verification") {
                        put("type", "string")
                        put("description", "Verification checks performed to confirm the goal is 100% done.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("summary"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "agent_memory",
            description = "Read or update persistent project/story memory (PARCHMENT_MEMORY.md in workspace) across sessions.",
            requiresWritePermission = false,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("action") {
                        put("type", "string")
                        put("description", "'read', 'append', or 'overwrite'.")
                    }
                    putJsonObject("content") {
                        put("type", "string")
                        put("description", "Memory notes to append or overwrite when action is 'append' or 'overwrite'.")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("action"))
                }
            }
        ),
        WorkspaceHarness.ToolDefinition(
            name = "shell_exec",
            description = "Execute a shell command inside the mounted workspace directory and return stdout/stderr.",
            requiresWritePermission = true,
            requiresShellPermission = true,
            parametersSchema = buildJsonObject {
                put("type", "object")
                putJsonObject("properties") {
                    putJsonObject("command") {
                        put("type", "string")
                        put("description", "Shell command to run in the workspace directory.")
                    }
                    putJsonObject("timeout_seconds") {
                        put("type", "integer")
                        put("description", "Timeout in seconds (default 30).")
                    }
                }
                putJsonArray("required") {
                    add(JsonPrimitive("command"))
                }
            }
        )
    )

    fun allTools(): List<WorkspaceHarness.ToolDefinition> = ALL_TOOLS + externalToolsProvider()

    fun isParallelSafeReadOnlyTool(toolName: String): Boolean = when (toolName.lowercase()) {
        "fs_read", "fs_list", "fs_glob", "fs_search",
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
        policy: AgentPermissionPolicy? = null
    ): List<WorkspaceHarness.ToolDefinition> {
        val hasMountedWorkspace = mode != WorkspaceMode.OFF && workspacePath.isNotBlank()
        if (!hasMountedWorkspace && !goalModeActive && !enableWebTools && !includeLocalWebSearchTool && externalToolsProvider().isEmpty() && policy?.computerAccess != true) {
            return emptyList()
        }
        return allTools().filter { tool ->
            if (policy != null && !policy.allowsTool(tool.name)) return@filter false
            if (tool.name == ComputerUse.TOOL_NAME && policy?.computerAccess != true) return@filter false
            if (tool.name in setOf("web_search", "web_fetch") && !enableWebTools) return@filter false
            if (tool.name == "web_search" && !includeLocalWebSearchTool) return@filter false
            if (!hasMountedWorkspace) {
                if (!goalModeActive) {
                    tool.name == "web_search" || tool.name == "web_fetch" || tool.name == "calc_eval" ||
                        tool.name == "dice_roll" || tool.name == "ask_user" || tool.name == "agent_delegate" ||
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
    }

    private inline fun mapActiveTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean,
        goalModeActive: Boolean,
        enableWebTools: Boolean,
        includeLocalWebSearchTool: Boolean,
        policy: AgentPermissionPolicy?,
        crossinline encode: JsonObjectBuilder.(WorkspaceHarness.ToolDefinition) -> Unit
    ): JsonArray = buildJsonArray {
        for (tool in activeToolsForMode(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy)) {
            add(buildJsonObject { encode(tool) })
        }
    }

    fun buildFlatFunctionTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = mapActiveTools(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy) { tool ->
        put("type", "function")
        put("name", tool.name)
        put("description", tool.description)
        put("parameters", tool.parametersSchema)
    }

    fun buildOpenAiChatCompletionsTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = mapActiveTools(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy) { tool ->
        put("type", "function")
        putJsonObject("function") {
            put("name", tool.name)
            put("description", tool.description)
            put("parameters", tool.parametersSchema)
        }
    }

    fun buildAnthropicTools(
        mode: WorkspaceMode,
        workspacePath: String,
        allowShell: Boolean = false,
        goalModeActive: Boolean = false,
        enableWebTools: Boolean = false,
        includeLocalWebSearchTool: Boolean = enableWebTools,
        policy: AgentPermissionPolicy? = null
    ): JsonArray = mapActiveTools(mode, workspacePath, allowShell, goalModeActive, enableWebTools, includeLocalWebSearchTool, policy) { tool ->
        put("name", tool.name)
        put("description", tool.description)
        put("input_schema", tool.parametersSchema)
    }
}
