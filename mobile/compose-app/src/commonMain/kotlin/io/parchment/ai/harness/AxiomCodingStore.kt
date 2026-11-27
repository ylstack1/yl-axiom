package io.parchment.ai.harness

import kotlinx.serialization.Serializable

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
 * Thin Kotlin access to the coding-task store. Platform-specific actual
 * maps JSON across the JNI boundary to Rust `parchment-coding`. Planning,
 * TDD phases, hooks, verification and recovery all live in Rust, so there
 * is no second agent engine here.
 */
expect object AxiomCodingStore {
    /** Newest-first task summaries for the sidebar/list. */
    fun listTasks(storageDir: java.io.File): List<CodingTaskSummary>

    /**
     * Creates a task with a generated id and returns its summary, or null when
     * the native store rejected the write.
     */
    fun createTask(
        storageDir: java.io.File,
        title: String,
        description: String,
        workspacePath: String,
        ownerSessionId: String
    ): CodingTaskSummary?

    /** Moves a task to a new lifecycle state. */
    fun setState(storageDir: java.io.File, taskId: String, state: String): Boolean

    fun loadTask(storageDir: java.io.File, taskId: String): CodingTaskSummary?

    fun deleteTask(storageDir: java.io.File, taskId: String): Boolean
}
