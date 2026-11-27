package io.parchment.ai.harness

import io.parchment.ai.platform.PlatformStorage
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File

/**
 * A coding task as the UI sees it. This is a view model over the Rust
 * `parchment-coding` task record; the engine and its rules stay in Rust.
 */
@Serializable
data class CodingTaskSummary(
    val id: String,
    val title: String,
    val description: String = "",
    val state: String = "PENDING",
    val workspacePath: String = "",
    val createdAtMs: Long = 0L,
    val updatedAtMs: Long = 0L,
    val subtaskCount: Int = 0,
    val verifiedCount: Int = 0
)

/**
 * Thin Kotlin access to the coding-task store.
 *
 * JSON encoding/decoding lives here (commonMain, kotlinx.serialization). The
 * native JNI calls are delegated to `PlatformStorage` (expect/actual), whose
 * Android actual talks to Rust `parchment-core` via `ParchmentNativeCore`.
 * Planning, TDD phases, hooks, verification and recovery all live in Rust.
 */
object AxiomCodingStore {
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        prettyPrint = false
    }

    /** Newest-first task summaries for the sidebar/list. */
    fun listTasks(storageDir: File): List<CodingTaskSummary> {
        val ids = PlatformStorage.listCodingTaskIds(storageDir)
        if (ids.isEmpty()) return emptyList()
        return ids.mapNotNull { id ->
            val raw = PlatformStorage.loadCodingTaskJson(storageDir, id) ?: return@mapNotNull null
            runCatching { json.decodeFromString<CodingTaskSummary>(raw) }.getOrNull()
        }
    }

    /**
     * Creates a task with a generated id and returns its summary, or null when
     * the native store rejected the write.
     */
    fun createTask(
        storageDir: File,
        title: String,
        description: String,
        workspacePath: String,
        ownerSessionId: String
    ): CodingTaskSummary? {
        val now = System.currentTimeMillis()
        val id = "task-$now"
        val task = CodingTaskSummary(
            id = id,
            title = title,
            description = description,
            state = "PENDING",
            workspacePath = workspacePath,
            createdAtMs = now,
            updatedAtMs = now
        )
        val raw = runCatching { json.encodeToString(task) }.getOrNull() ?: return null
        val saved = PlatformStorage.saveCodingTaskJson(storageDir, raw)
        return if (saved) task else null
    }

    /** Moves a task to a new lifecycle state. */
    fun setState(storageDir: File, taskId: String, state: String): Boolean {
        val existing = loadTask(storageDir, taskId) ?: return false
        val updated = existing.copy(state = state, updatedAtMs = System.currentTimeMillis())
        val raw = runCatching { json.encodeToString(updated) }.getOrNull() ?: return false
        return PlatformStorage.saveCodingTaskJson(storageDir, raw)
    }

    fun loadTask(storageDir: File, taskId: String): CodingTaskSummary? {
        val raw = PlatformStorage.loadCodingTaskJson(storageDir, taskId) ?: return null
        return runCatching { json.decodeFromString<CodingTaskSummary>(raw) }.getOrNull()
    }

    fun deleteTask(storageDir: File, taskId: String): Boolean = PlatformStorage.deleteCodingTask(storageDir, taskId)
}
