package io.parchment.ai.harness

import io.parchment.ai.model.GoalTrackerState
import io.parchment.ai.model.TodoItem
import io.parchment.ai.model.ToolCallRecord
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Tool output sanitization/size-valve compression, rolling history compaction, and goal/todo state parsing.
 */
internal object ToolOutputAndGoalSupport {

    private val ANSI_ESCAPE_REGEX = Regex("""\u001B\[[0-?]*[ -/]*[@-~]""")
    private val PS_CLIXML_HEADER_REGEX = Regex("""(?m)^#<\s*CLIXML\s*$""")
    private val PS_CLIXML_ERROR_LINE_REGEX = Regex("""<S S="Error">(.*?)</S>""")
    private val PS_CLIXML_BLOCK_REGEX = Regex("""(?s)\s*<Objs\b.*?</Objs>""")
    private val EXCESS_BLANK_LINES_REGEX = Regex("""\n{4,}""")
    private val CODE_SYMBOL_REGEX = Regex(
        """^\s*(?:\d+:\s*)?(?:(?:internal|public|private|protected|override|suspend|inline|actual|expect|data|sealed|abstract|open)\s+)*(?:fun|class|interface|object|def)\s+([A-Za-z_]\w*)"""
    )
    internal const val COMPACTION_BATCH = 16

    private fun extractCliXmlErrors(block: String): String {
        val parts = PS_CLIXML_ERROR_LINE_REGEX.findAll(block)
            .map { match ->
                match.groupValues[1]
                    .replace("_x000D__x000A_", "\n")
                    .replace("_x000A_", "\n")
                    .replace("_x0009_", "\t")
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
            }
            .joinToString("")
            .trim()
        return if (parts.isEmpty()) "" else "\n$parts"
    }

    fun compressWithSizeValve(toolName: String, rawOutput: String): String {
        if (rawOutput.isEmpty()) return rawOutput
        val cleaned = rawOutput
            .replace(ANSI_ESCAPE_REGEX, "")
            .replace(PS_CLIXML_HEADER_REGEX, "")
            .replace(PS_CLIXML_BLOCK_REGEX) { match -> extractCliXmlErrors(match.value) }
            .replace("\r\n", "\n")
            .replace(EXCESS_BLANK_LINES_REGEX, "\n\n\n")
            .trimEnd()
        if (toolName.equals("fs_read", ignoreCase = true)) {
            return cleaned
        }
        val lines = cleaned.lines()
        val (headKeep, tailKeep) = when (toolName.lowercase()) {
            "shell_exec" -> 90 to 90
            "fs_search" -> 110 to 50
            "fs_list", "fs_glob" -> 120 to 60
            else -> 160 to 80
        }
        val maxTotalLines = headKeep + tailKeep + 15
        if (lines.size <= maxTotalLines) {
            return cleaned
        }
        val omittedCount = lines.size - headKeep - tailKeep
        val headPart = lines.take(headKeep).joinToString("\n")
        val tailPart = lines.takeLast(tailKeep).joinToString("\n")
        return "$headPart\n… [$omittedCount middle lines omitted by size valve — refine query or use fs_read with start_line/max_lines] …\n$tailPart"
    }

    fun compactStaleOutput(
        stepIndex: Int,
        totalSteps: Int,
        toolName: String,
        rawOutput: String,
        keepRecentCount: Int = 8,
        minCharsToCompact: Int = 1_100
    ): String {
        // A boundary sliding every step would rewrite earlier text and miss the provider's cache each time.
        val boundary = if (keepRecentCount > 0) ((totalSteps - keepRecentCount) / COMPACTION_BATCH) * COMPACTION_BATCH else totalSteps
        if (stepIndex >= boundary || rawOutput.length <= minCharsToCompact) {
            return rawOutput
        }
        val lines = rawOutput.lines()
        val firstLine = lines.firstOrNull()?.take(140).orEmpty()
        val lineCount = lines.size
        val extraHint = when (toolName.lowercase()) {
            "fs_read" -> {
                val symbols = lines.asSequence()
                    .mapNotNull { CODE_SYMBOL_REGEX.find(it)?.groupValues?.getOrNull(1) }
                    .distinct()
                    .take(5)
                    .toList()
                if (symbols.isNotEmpty()) " symbols=${symbols.joinToString(",")};" else ""
            }
            "shell_exec" -> {
                val tailLine = lines.asReversed().firstOrNull { it.isNotBlank() && it != firstLine }?.trim()?.take(100)
                if (!tailLine.isNullOrEmpty()) " tail=\"$tailLine\";" else ""
            }
            else -> ""
        }
        return "[Compacted step #$stepIndex ($toolName): ${rawOutput.length} chars ($lineCount lines) — \"$firstLine…\";$extraHint Re-run tool if exact lines are needed.]"
    }

    fun parseTodoItems(args: JsonObject): List<TodoItem> {
        val todosArr = args["todos"] as? JsonArray ?: JsonArray(emptyList())
        return todosArr.mapNotNull { el ->
            val obj = el as? JsonObject ?: return@mapNotNull null
            val id = obj["id"]?.jsonPrimitive?.contentOrNull?.ifBlank { "•" } ?: "•"
            val content = obj["content"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val status = obj["status"]?.jsonPrimitive?.contentOrNull?.lowercase() ?: "pending"
            TodoItem(id = id, content = content, status = status)
        }
    }

    fun applyGoalToolUpdate(current: GoalTrackerState?, tc: ToolCallRecord): GoalTrackerState? {
        if (tc.isError) return current
        val args = runCatching {
            WorkspaceHarness.json.parseToJsonElement(tc.argumentsJson.ifBlank { "{}" }).jsonObject
        }.getOrElse { return current }

        return when (tc.name) {
            "todo_write" -> {
                val goal = args["goal"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }
                    ?: current?.goalObjective.orEmpty()
                val parsed = parseTodoItems(args)
                val allDone = parsed.isNotEmpty() &&
                    parsed.all {
                        it.status == "completed" || it.status == "cancelled"
                    }
                GoalTrackerState(
                    goalObjective = goal,
                    todos = parsed,
                    isCompleted = allDone,
                    verificationSummary = current?.verificationSummary.orEmpty()
                )
            }
            "goal_complete" -> {
                val summary = args["summary"]?.jsonPrimitive?.contentOrNull.orEmpty()
                val verification = args["verification"]?.jsonPrimitive?.contentOrNull.orEmpty()
                val base = current ?: GoalTrackerState()
                base.copy(
                    todos = base.todos.map {
                        if (it.status == "in_progress" || it.status == "pending") it.copy(status = "completed") else it
                    },
                    isCompleted = true,
                    verificationSummary = listOf(summary, verification).filter { it.isNotBlank() }.joinToString(" — ")
                )
            }
            else -> current
        }
    }
}
