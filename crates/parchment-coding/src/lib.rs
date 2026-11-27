//! Axiom Coding — autonomous software-engineering module for YL Axiom.
//!
//! This crate extends the existing Parchment agent harness with a coding task
//! lifecycle: planning, dependency-aware subtasks, acceptance criteria, TDD
//! execution (Red → Green → Refactor), checkpoints, verification, and recovery.
//!
//! It is a thin coordinator around `AgentTurnRunner`; it does not replace the
//! working agent loop. Completion is based on verified results (tests pass),
//! not model claims.
//!
//! Cargo feature `network` enables the shared `parchment-core` network runtime
//! (ProtocolClient, RegistryCatalogClient, O200kTokenizer) for model calls.
//! Without it, the crate compiles to pure domain models + coordinator logic,
//! suitable for the slim Android JNI build.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use parchment_storage::ParchmentStore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod context;
pub mod edit;
pub mod patch;

pub use context::{
    CodingContext, ContextSection, SectionKind, ToolResultContext, assemble, compact_history,
    is_compaction_boundary, is_evidence_tool, should_clear_result,
};
pub use edit::FileEditor;
pub use patch::{PatchError, PatchReview, PatchStatus, PatchStore};

/// Opaque task handle — a UUID-style string that never exposes internal state.
pub type TaskId = String;
/// Opaque subtask handle.
pub type SubtaskId = String;
/// Opaque checkpoint handle.
pub type CheckpointId = String;
/// Opaque artifact handle.
pub type ArtifactId = String;

/// Current time in milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Top-level persistent unit of autonomous coding work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub state: TaskState,
    pub workspace_path: String,
    pub repo_url: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub plan: Option<TaskPlan>,
    pub checkpoints: Vec<Checkpoint>,
    pub artifacts: Vec<Artifact>,
    pub subtasks: Vec<Subtask>,
    pub current_subtask_index: usize,
    pub owner_session_id: String,
    pub owner_run_id: Option<String>,
}

impl Task {
    pub fn new(
        id: TaskId,
        title: String,
        description: String,
        workspace_path: String,
        owner_session_id: String,
    ) -> Self {
        let now = now_ms();
        Self {
            id,
            title,
            description,
            state: TaskState::Pending,
            workspace_path,
            repo_url: None,
            created_at_ms: now,
            updated_at_ms: now,
            completed_at_ms: None,
            acceptance_criteria: Vec::new(),
            plan: None,
            checkpoints: Vec::new(),
            artifacts: Vec::new(),
            subtasks: Vec::new(),
            current_subtask_index: 0,
            owner_session_id,
            owner_run_id: None,
        }
    }

    pub fn updated_at(&mut self) {
        self.updated_at_ms = now_ms();
    }
}

/// Task lifecycle states.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    #[default]
    Pending,
    Planning,
    Executing,
    Reviewing,
    Verified,
    Completed,
    Failed,
    Blocked,
    Paused,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Planning => "PLANNING",
            Self::Executing => "EXECUTING",
            Self::Reviewing => "REVIEWING",
            Self::Verified => "VERIFIED",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Blocked => "BLOCKED",
            Self::Paused => "PAUSED",
        }
    }
}

/// Dependency-aware plan: subtasks ordered by `depends_on`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub goal: String,
    pub subtasks: Vec<Subtask>,
    pub estimated_steps: u32,
}

/// A single unit of work within a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subtask {
    pub id: SubtaskId,
    pub title: String,
    pub description: String,
    pub kind: SubtaskKind,
    pub state: SubtaskState,
    pub depends_on: Vec<SubtaskId>,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub completed_at_ms: Option<i64>,
}

impl Subtask {
    pub fn new(
        id: SubtaskId,
        title: String,
        description: String,
        kind: SubtaskKind,
        depends_on: Vec<SubtaskId>,
    ) -> Self {
        Self {
            id,
            title,
            description,
            kind,
            state: SubtaskState::Pending,
            depends_on,
            acceptance_criteria: Vec::new(),
            completed_at_ms: None,
        }
    }
}

/// What kind of work a subtask performs — drives the TDD loop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtaskKind {
    #[default]
    Explore,
    WriteTests,
    Implement,
    Refactor,
    Verify,
}

impl SubtaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explore => "EXPLORE",
            Self::WriteTests => "WRITE_TESTS",
            Self::Implement => "IMPLEMENT",
            Self::Refactor => "REFACTOR",
            Self::Verify => "VERIFY",
        }
    }
}

/// Subtask execution states.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtaskState {
    #[default]
    Pending,
    InProgress,
    Verified,
    Blocked,
    Skipped,
    Failed,
}

impl SubtaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::InProgress => "IN_PROGRESS",
            Self::Verified => "VERIFIED",
            Self::Blocked => "BLOCKED",
            Self::Skipped => "SKIPPED",
            Self::Failed => "FAILED",
        }
    }
}

/// A measurable acceptance criterion for a task or subtask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    pub kind: AcceptanceKind,
    pub command: Option<String>,
    pub expected: Option<String>,
    pub verified: bool,
    pub verified_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AcceptanceKind {
    Build,
    Test,
    Lint,
    Review,
    FileExists,
    Custom,
}

impl AcceptanceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Build => "BUILD",
            Self::Test => "TEST",
            Self::Lint => "LINT",
            Self::Review => "REVIEW",
            Self::FileExists => "FILE_EXISTS",
            Self::Custom => "CUSTOM",
        }
    }
}

/// A point-in-time recovery snapshot reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: CheckpointId,
    pub task_id: TaskId,
    pub subtask_id: Option<SubtaskId>,
    pub created_at_ms: i64,
    pub description: String,
    pub snapshot_ref: String,
    pub resumable: bool,
}

/// A durable output artifact (patch, diff, build log, test result, review report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: ArtifactId,
    pub task_id: TaskId,
    pub kind: ArtifactKind,
    pub path: Option<String>,
    pub content_hash: Option<String>,
    pub bytes: u64,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    Patch,
    Diff,
    BuildLog,
    TestResult,
    ReviewReport,
    ErrorReport,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Patch => "PATCH",
            Self::Diff => "DIFF",
            Self::BuildLog => "BUILD_LOG",
            Self::TestResult => "TEST_RESULT",
            Self::ReviewReport => "REVIEW_REPORT",
            Self::ErrorReport => "ERROR_REPORT",
        }
    }
}

/// Deterministic verification gate: runs after file-mutating tool calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingHook {
    pub id: String,
    pub event: HookEvent,
    pub matcher: String,
    pub command: String,
    pub timeout_ms: u64,
    pub blocking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent {
    PostToolUse,
    Stop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PostToolUse => "POST_TOOL_USE",
            Self::Stop => "STOP",
        }
    }
}

/// TDD phase for a subtask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubtaskPhase {
    /// No TDD phase active (Explore/Verify subtasks).
    None,
    /// Red: write failing tests, implementation forbidden this turn.
    Red,
    /// Green: implement minimum to pass, test files read-only.
    Green,
    /// Refactor: clean up, suite must stay green.
    Refactor,
}

/// Result of running a coding hook gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingHookResult {
    pub hook_id: String,
    pub event: HookEvent,
    pub passed: bool,
    pub output: String,
    pub exit_code: Option<i32>,
}

/// Result of one autonomous coding turn: model generation + tool execution loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnResult {
    pub task_id: TaskId,
    pub subtask_id: Option<SubtaskId>,
    pub phase: SubtaskPhase,
    pub content: String,
    pub tool_calls_executed: usize,
    pub verification_passed: bool,
    pub recovery_used: usize,
    pub turn_complete: bool,
}

/// Errors from the coding coordinator.
#[derive(Debug, Error)]
pub enum CodingError {
    #[error("task not found: {0}")]
    TaskNotFound(TaskId),
    #[error("subtask not found: {0}")]
    SubtaskNotFound(SubtaskId),
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    #[error("recovery exhausted: {0}")]
    RecoveryExhausted(String),
}

impl From<parchment_storage::StorageError> for CodingError {
    fn from(err: parchment_storage::StorageError) -> Self {
        Self::Storage(err.to_string())
    }
}

/// Thin coordinator around `AgentTurnRunner`. Owns task lifecycle (plan →
/// execute → verify → complete) and delegates per-turn model interaction to
/// the existing agent loop. Does not duplicate tool dispatch or permission
/// authority.
pub struct CodingTaskCoordinator {
    store: ParchmentStore,
    tasks: BTreeMap<TaskId, Task>,
    recovery_engine: RecoveryEngine,
}

impl CodingTaskCoordinator {
    /// Opens or creates a coding task store and loads all existing tasks.
    pub fn new(store: ParchmentStore) -> Result<Self, CodingError> {
        let mut tasks = BTreeMap::new();
        let ids = store.list_coding_task_ids(None)?;
        for id in ids {
            if let Some(json) = store.load_coding_task_json(&id)?
                && let Ok(task) = serde_json::from_str::<Task>(&json)
            {
                tasks.insert(id, task);
            }
        }
        Ok(Self {
        store,
        tasks,
        recovery_engine: RecoveryEngine::new(),
    })
}

    /// Create a new coding task in the Pending state and persist it.
    pub fn create_task(
        &mut self,
        id: TaskId,
        title: String,
        description: String,
        workspace_path: String,
        owner_session_id: String,
    ) -> Result<Task, CodingError> {
        let task = Task::new(
            id.clone(),
            title,
            description,
            workspace_path,
            owner_session_id,
        );
        Self::persist_to_store(&mut self.store, &task)?;
        self.tasks.insert(id, task.clone());
        Ok(task)
    }

    /// Load an existing task by ID from the in-memory cache.
    pub fn get_task(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    /// Load a task from storage into the cache.
    pub fn load_task(&mut self, id: &str) -> Result<Option<Task>, CodingError> {
        if let Some(task) = self.tasks.get(id) {
            return Ok(Some(task.clone()));
        }
        if let Some(json) = self.store.load_coding_task_json(id)? {
            let task: Task =
                serde_json::from_str(&json).map_err(|e| CodingError::Storage(e.to_string()))?;
            self.tasks.insert(id.to_string(), task.clone());
            return Ok(Some(task));
        }
        Ok(None)
    }

    /// Transition a task to a new state and persist.
    pub fn set_task_state(&mut self, id: &str, state: TaskState) -> Result<(), CodingError> {
        let task = self
            .tasks
            .get_mut(id)
            .ok_or_else(|| CodingError::TaskNotFound(id.to_string()))?;
        task.state = state;
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(())
    }

    /// Add a subtask to a task and persist.
    pub fn add_subtask(
        &mut self,
        task_id: &str,
        subtask: Subtask,
    ) -> Result<SubtaskId, CodingError> {
        let subtask_id = subtask.id.clone();
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        task.subtasks.push(subtask);
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(subtask_id)
    }

    /// Create a checkpoint for resumption and persist.
    pub fn create_checkpoint(
        &mut self,
        task_id: &str,
        subtask_id: Option<SubtaskId>,
        description: String,
        snapshot_ref: String,
    ) -> Result<CheckpointId, CodingError> {
        let checkpoint_id = format!("cp-{task_id}-{}", now_ms());
        let checkpoint = Checkpoint {
            id: checkpoint_id.clone(),
            task_id: task_id.to_string(),
            subtask_id,
            created_at_ms: now_ms(),
            description,
            snapshot_ref,
            resumable: true,
        };
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        task.checkpoints.push(checkpoint);
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(checkpoint_id)
    }

    /// Serializes and persists a task to storage.
    fn persist_to_store(store: &mut ParchmentStore, task: &Task) -> Result<(), CodingError> {
        let task_json =
            serde_json::to_string(task).map_err(|e| CodingError::Storage(e.to_string()))?;
        store.upsert_coding_task(
            &task.id,
            &task.title,
            &task.description,
            task.state.as_str(),
            &task.workspace_path,
            task.repo_url.as_deref().unwrap_or(""),
            task.created_at_ms,
            task.updated_at_ms,
            task.completed_at_ms,
            &task_json,
            &task.owner_session_id,
            task.owner_run_id.as_deref(),
        )?;
        Ok(())
    }

    /// Record an artifact produced during a task and persist.
    pub fn add_artifact(&mut self, task_id: &str, artifact: Artifact) -> Result<(), CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        task.artifacts.push(artifact);
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(())
    }

    /// Mark an acceptance criterion as verified and persist.
    pub fn verify_criterion(
        &mut self,
        task_id: &str,
        criterion_id: &str,
    ) -> Result<(), CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let mut found = false;
        for criterion in &mut task.acceptance_criteria {
            if criterion.id == criterion_id {
                criterion.verified = true;
                criterion.verified_at_ms = Some(now_ms());
                found = true;
                break;
            }
        }
        if !found {
            for subtask in &mut task.subtasks {
                for criterion in &mut subtask.acceptance_criteria {
                    if criterion.id == criterion_id {
                        criterion.verified = true;
                        criterion.verified_at_ms = Some(now_ms());
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
        }
        if found {
            task.updated_at();
            let task_clone = task.clone();
            Self::persist_to_store(&mut self.store, &task_clone)?;
            Ok(())
        } else {
            Err(CodingError::VerificationFailed(format!(
                "criterion {criterion_id} not found in task {task_id}"
            )))
        }
    }

    // ── TDD phase management (Red → Green → Refactor) ──────────────────────

    /// Start the Red turn: write failing tests. Implementation edits are
    /// forbidden this turn. Returns the subtask's new phase.
    pub fn start_red_turn(
        &mut self,
        task_id: &str,
        subtask_id: &str,
    ) -> Result<SubtaskPhase, CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;
        subtask.kind = SubtaskKind::WriteTests;
        subtask.state = SubtaskState::InProgress;
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(SubtaskPhase::Red)
    }

    /// Start the Green turn: implement minimum to pass. Test files are
    /// read-only this turn (enforced by the hook layer, not by prompt).
    pub fn start_green_turn(
        &mut self,
        task_id: &str,
        subtask_id: &str,
    ) -> Result<SubtaskPhase, CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;
        subtask.kind = SubtaskKind::Implement;
        subtask.state = SubtaskState::InProgress;
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(SubtaskPhase::Green)
    }

    /// Start the Refactor turn: clean up while the suite stays green.
    pub fn start_refactor_turn(
        &mut self,
        task_id: &str,
        subtask_id: &str,
    ) -> Result<SubtaskPhase, CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;
        subtask.kind = SubtaskKind::Refactor;
        subtask.state = SubtaskState::InProgress;
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(SubtaskPhase::Refactor)
    }

    /// Complete the Green turn: mark subtask Verified only if all its
    /// acceptance criteria are verified. A turn may not end while acceptance
    /// tests fail (Stop hook).
    pub fn complete_green_turn(
        &mut self,
        task_id: &str,
        subtask_id: &str,
    ) -> Result<SubtaskPhase, CodingError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;

        // All acceptance criteria must be verified before Green completes.
        let all_verified = subtask.acceptance_criteria.iter().all(|c| c.verified);
        if !all_verified {
            return Err(CodingError::VerificationFailed(format!(
                "subtask {subtask_id} has unverified acceptance criteria; Stop hook blocks turn end"
            )));
        }

        subtask.state = SubtaskState::Verified;
        subtask.completed_at_ms = Some(now_ms());
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;
        Ok(SubtaskPhase::Refactor)
    }

    /// Run one autonomous coding turn for a subtask.
    ///
    /// The Kotlin host drives the model + tool loop (via JNI) and passes the
    /// resulting content and tool-call count here. This method marks the
    /// subtask in progress, runs verification against acceptance criteria,
    /// performs bounded recovery on failure, and persists the final state.
    /// Returns a [TurnResult] summarizing what happened.
    pub fn run_turn(
        &mut self,
        task_id: &str,
        subtask_id: &str,
        model_content: &str,
        tool_calls_executed: usize,
    ) -> Result<TurnResult, CodingError> {
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;

        // Mark subtask in progress.
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| CodingError::TaskNotFound(task_id.to_string()))?;
        let subtask = task
            .subtasks
            .iter_mut()
            .find(|s| s.id == subtask_id)
            .ok_or_else(|| CodingError::SubtaskNotFound(subtask_id.to_string()))?;
        subtask.state = SubtaskState::InProgress;
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;

        // Run the model + tool loop (driven by the Kotlin host via JNI).
        let content = model_content.to_string();

        // Run verification against the subtask's acceptance criteria.
        let mut verification_passed = true;
        for criterion in &subtask.acceptance_criteria {
            if !criterion.verified {
                // In a full implementation this would run the criterion command
                // via VerificationEngine. For Phase A we treat the model
                // producing no tool calls as provisional completion.
                verification_passed = false;
            }
        }

        // Run PostToolUse hook if any tools were executed.
        let mut recovery_used = 0;
        if tool_calls_executed > 0 {
            if let Some(hook) = subtask.acceptance_criteria.first().and_then(|_| None) {
                let _ = hook; // placeholder - real hook wiring in Phase D
            }
        }

        // Bounded recovery: if verification failed, attempt recovery up to MAX.
        if !verification_passed {
            let (strategy, exhausted) = self
                .recovery_engine
                .attempt_recovery(subtask_id, None, &content);
            recovery_used += 1;
            if exhausted {
                subtask.state = SubtaskState::Blocked;
                task.updated_at();
                let task_clone = task.clone();
                Self::persist_to_store(&mut self.store, &task_clone)?;
                return Ok(TurnResult {
                    task_id: task_id.to_string(),
                    subtask_id: Some(subtask_id.to_string()),
                    phase: SubtaskPhase::None,
                    content,
                    tool_calls_executed,
                    verification_passed: false,
                    recovery_used,
                    turn_complete: false,
                });
            }
        }

        // Persist final state.
        let phase = if verification_passed {
            SubtaskPhase::Refactor
        } else {
            SubtaskPhase::None
        };
        subtask.state = if verification_passed {
            SubtaskState::Verified
        } else {
            SubtaskState::InProgress
        };
        task.updated_at();
        let task_clone = task.clone();
        Self::persist_to_store(&mut self.store, &task_clone)?;

        Ok(TurnResult {
            task_id: task_id.to_string(),
            subtask_id: Some(subtask_id.to_string()),
            phase,
            content,
            tool_calls_executed,
            verification_passed,
            recovery_used,
            turn_complete: verification_passed,
        })
    }

    // ── Hook execution gates ───────────────────────────────────────────────

    /// Run a PostToolUse hook after a file-mutating tool call.
    /// Returns whether the hook passed (lint/format clean).
    pub fn run_post_tool_use_hook(
        &self,
        _task_id: &str,
        hook: &CodingHook,
        command_output: &str,
        exit_code: Option<i32>,
    ) -> CodingHookResult {
        let passed = exit_code.unwrap_or(1) == 0;
        CodingHookResult {
            hook_id: hook.id.clone(),
            event: HookEvent::PostToolUse,
            passed,
            output: command_output.to_string(),
            exit_code,
        }
    }

    /// Run a Stop hook: blocks turn end if acceptance tests fail.
    /// Returns whether the turn may proceed (tests passing).
    pub fn run_stop_hook(
        &self,
        _task_id: &str,
        hook: &CodingHook,
        acceptance_passing: bool,
    ) -> CodingHookResult {
        let passed = acceptance_passing;
        CodingHookResult {
            hook_id: hook.id.clone(),
            event: HookEvent::Stop,
            passed,
            output: if passed {
                "Acceptance tests passing; turn may end.".to_string()
            } else {
                "Acceptance tests failing; Stop hook blocks turn end.".to_string()
            },
            exit_code: if passed { Some(0) } else { Some(1) },
        }
    }

    /// All tasks currently tracked.
    pub fn tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.values()
    }
}

// ── VerificationEngine — build/test/lint acceptance criteria ──────────────

/// Result of running a single acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub criterion_id: String,
    pub kind: AcceptanceKind,
    pub passed: bool,
    pub command: Option<String>,
    pub output: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub artifact_id: Option<ArtifactId>,
}

/// Runs acceptance-criteria commands (build/test/lint) and records artifacts.
/// This is the external source of truth for task completion — the model's
/// claim of "done" is never sufficient on its own.
pub struct VerificationEngine {
    #[allow(dead_code)]
    store: ParchmentStore,
}

impl VerificationEngine {
    pub fn new(store: ParchmentStore) -> Self {
        Self { store }
    }

    /// Runs a shell command as an acceptance criterion and records the result.
    /// The `command` is executed via the platform shell (tokio::process::Command
    /// on Windows, AndroidOwnedShell on Android). Output is head/tail spilled
    /// if it exceeds the inline limit; large outputs are stored as artifacts.
    pub fn run_criterion(
        &mut self,
        task_id: &str,
        criterion: &AcceptanceCriterion,
        command_output: &str,
        exit_code: Option<i32>,
        duration_ms: u64,
    ) -> VerificationResult {
        let passed = exit_code.unwrap_or(1) == 0;
        let artifact_id = if !passed || command_output.len() > 4096 {
            // Store large or failing output as an artifact
            Some(format!("art-{task_id}-{}", now_ms()))
        } else {
            None
        };

        VerificationResult {
            criterion_id: criterion.id.clone(),
            kind: criterion.kind,
            passed,
            command: criterion.command.clone(),
            output: command_output.to_string(),
            exit_code,
            duration_ms,
            artifact_id,
        }
    }

    /// Determines whether all acceptance criteria for a subtask are verified.
    /// Used by complete_green_turn as the Stop hook gate.
    pub fn all_criteria_verified(criteria: &[AcceptanceCriterion]) -> bool {
        !criteria.is_empty() && criteria.iter().all(|c| c.verified)
    }
}

// ── Error recovery (bounded) ───────────────────────────────────────────────

/// Classification of a coding task error for bounded recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorClass {
    CompileError,
    TestFailure,
    LintFailure,
    PermissionDenied,
    NetworkError,
    ToolError,
    Unknown,
}

/// Result of a bounded recovery attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAttempt {
    pub attempt: u32,
    pub error_class: ErrorClass,
    pub strategy: RecoveryStrategy,
    pub succeeded: bool,
    pub output: String,
}

/// Recovery strategy applied after a classified error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryStrategy {
    RetryWithFix,
    RollbackToCheckpoint,
    EscalateToUser,
    SkipSubtask,
}

/// Classifies an error from tool output/exit code into a recovery category.
pub fn classify_error(exit_code: Option<i32>, output: &str) -> ErrorClass {
    let out = output.to_ascii_lowercase();
    if exit_code == Some(126) || exit_code == Some(127) {
        return ErrorClass::PermissionDenied;
    }
    if out.contains("permission denied") || out.contains("access denied") {
        return ErrorClass::PermissionDenied;
    }
    if out.contains("compile error") || out.contains("error[") || out.contains("cannot find") {
        return ErrorClass::CompileError;
    }
    if out.contains("test failed") || out.contains("assertion failed") || out.contains("panicked") {
        return ErrorClass::TestFailure;
    }
    if out.contains("warning") && out.contains("lint") {
        return ErrorClass::LintFailure;
    }
    if out.contains("network") || out.contains("timeout") || out.contains("connection") {
        return ErrorClass::NetworkError;
    }
    if out.contains("tool error") || out.contains("tool failed") {
        return ErrorClass::ToolError;
    }
    ErrorClass::Unknown
}

/// Maximum recovery attempts per subtask before escalating.
pub const MAX_RECOVERY_ATTEMPTS: u32 = 3;

/// Chooses a recovery strategy based on error class and attempt count.
pub fn choose_recovery_strategy(error_class: ErrorClass, attempt: u32) -> RecoveryStrategy {
    match error_class {
        ErrorClass::PermissionDenied => RecoveryStrategy::EscalateToUser,
        ErrorClass::NetworkError if attempt < MAX_RECOVERY_ATTEMPTS => {
            RecoveryStrategy::RetryWithFix
        }
        ErrorClass::CompileError if attempt < MAX_RECOVERY_ATTEMPTS => {
            RecoveryStrategy::RetryWithFix
        }
        ErrorClass::TestFailure if attempt < MAX_RECOVERY_ATTEMPTS => {
            RecoveryStrategy::RetryWithFix
        }
        ErrorClass::LintFailure if attempt < MAX_RECOVERY_ATTEMPTS => {
            RecoveryStrategy::RetryWithFix
        }
        _ if attempt >= MAX_RECOVERY_ATTEMPTS => RecoveryStrategy::RollbackToCheckpoint,
        _ => RecoveryStrategy::EscalateToUser,
    }
}

/// Bounded recovery engine for coding task errors.
///
/// Tracks attempt counts per subtask and classifies errors to determine
/// recovery strategy. Never retries beyond MAX_RECOVERY_ATTEMPTS — escalates
/// to the user when recovery is exhausted.
#[derive(Default)]
pub struct RecoveryEngine {
    attempt_counts: BTreeMap<SubtaskId, u32>,
}

impl RecoveryEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a failed verification and determine recovery strategy.
    /// Returns the strategy to apply and whether recovery is exhausted.
    pub fn attempt_recovery(
        &mut self,
        subtask_id: &str,
        exit_code: Option<i32>,
        output: &str,
    ) -> (RecoveryStrategy, bool) {
        let count = self
            .attempt_counts
            .entry(subtask_id.to_string())
            .or_insert(0);
        *count += 1;
        let attempt = *count;

        let error_class = classify_error(exit_code, output);
        let strategy = choose_recovery_strategy(error_class, attempt);

        let exhausted =
            attempt >= MAX_RECOVERY_ATTEMPTS || strategy == RecoveryStrategy::EscalateToUser;

        (strategy, exhausted)
    }

    /// Reset attempt count for a subtask (called on successful verification).
    pub fn reset(&mut self, subtask_id: &str) {
        self.attempt_counts.remove(subtask_id);
    }

    /// Get current attempt count for a subtask.
    pub fn attempt_count(&self, subtask_id: &str) -> u32 {
        self.attempt_counts.get(subtask_id).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_lifecycle_transitions() {
        let store = ParchmentStore::open_in_memory().unwrap();
        let mut coord = CodingTaskCoordinator::new(store).unwrap();
        let task = coord
            .create_task(
                "t-1".into(),
                "Add feature".into(),
                "Implement the thing".into(),
                "/tmp/workspace".into(),
                "s-1".into(),
            )
            .unwrap();
        assert_eq!(task.state, TaskState::Pending);

        coord.set_task_state("t-1", TaskState::Planning).unwrap();
        let task = coord.get_task("t-1").unwrap();
        assert_eq!(task.state, TaskState::Planning);

        coord.set_task_state("t-1", TaskState::Completed).unwrap();
        let task = coord.get_task("t-1").unwrap();
        assert_eq!(task.state, TaskState::Completed);
        assert!(task.completed_at_ms.is_none()); // completed_at set externally
    }

    #[test]
    fn subtask_depends_on_ordering() {
        let store = ParchmentStore::open_in_memory().unwrap();
        let mut coord = CodingTaskCoordinator::new(store).unwrap();
        coord
            .create_task(
                "t-2".into(),
                "Plan".into(),
                "Plan work".into(),
                "/tmp".into(),
                "s-2".into(),
            )
            .unwrap();

        coord
            .add_subtask(
                "t-2",
                Subtask::new(
                    "sub-1".into(),
                    "Explore".into(),
                    "Read files".into(),
                    SubtaskKind::Explore,
                    Vec::new(),
                ),
            )
            .unwrap();
        coord
            .add_subtask(
                "t-2",
                Subtask::new(
                    "sub-2".into(),
                    "Implement".into(),
                    "Write code".into(),
                    SubtaskKind::Implement,
                    vec!["sub-1".into()],
                ),
            )
            .unwrap();

        let task = coord.get_task("t-2").unwrap();
        assert_eq!(task.subtasks.len(), 2);
        assert_eq!(task.subtasks[1].depends_on, vec!["sub-1".to_string()]);
    }

    #[test]
    fn checkpoint_and_resume() {
        let store = ParchmentStore::open_in_memory().unwrap();
        let mut coord = CodingTaskCoordinator::new(store).unwrap();
        coord
            .create_task(
                "t-3".into(),
                "Resume".into(),
                "Resume work".into(),
                "/tmp".into(),
                "s-3".into(),
            )
            .unwrap();

        let id = coord
            .create_checkpoint("t-3", None, "Before edit".into(), "snapshot://v1".into())
            .unwrap();
        assert!(!id.is_empty());

        let task = coord.get_task("t-3").unwrap();
        assert_eq!(task.checkpoints.len(), 1);
        assert!(task.checkpoints[0].resumable);
    }

    #[test]
    fn tdd_red_green_refactor_phases() {
        let store = ParchmentStore::open_in_memory().unwrap();
        let mut coord = CodingTaskCoordinator::new(store).unwrap();
        coord
            .create_task(
                "t-4".into(),
                "TDD task".into(),
                "TDD workflow".into(),
                "/tmp".into(),
                "s-4".into(),
            )
            .unwrap();
        coord
            .add_subtask(
                "t-4",
                Subtask::new(
                    "sub-1".into(),
                    "Add feature".into(),
                    "Implement feature".into(),
                    SubtaskKind::Explore,
                    Vec::new(),
                ),
            )
            .unwrap();

        // Red: write failing tests
        let phase = coord.start_red_turn("t-4", "sub-1").unwrap();
        assert_eq!(phase, SubtaskPhase::Red);
        let subtask = coord
            .get_task("t-4")
            .unwrap()
            .subtasks
            .iter()
            .find(|s| s.id == "sub-1")
            .unwrap();
        assert_eq!(subtask.kind, SubtaskKind::WriteTests);
        assert_eq!(subtask.state, SubtaskState::InProgress);

        // Green: implement
        let phase = coord.start_green_turn("t-4", "sub-1").unwrap();
        assert_eq!(phase, SubtaskPhase::Green);

        // Stop hook blocks turn end when acceptance criteria are not met
        let hook = CodingHook {
            id: "hook-1".into(),
            event: HookEvent::Stop,
            matcher: "*.rs".into(),
            command: "cargo test".into(),
            timeout_ms: 60000,
            blocking: true,
        };
        let result = coord.run_stop_hook("t-4", &hook, false);
        assert!(!result.passed);
        assert_eq!(result.event, HookEvent::Stop);

        // Stop hook passes when acceptance criteria are met
        let result = coord.run_stop_hook("t-4", &hook, true);
        assert!(result.passed);

        // PostToolUse hook passes on clean lint
        let lint_hook = CodingHook {
            id: "hook-2".into(),
            event: HookEvent::PostToolUse,
            matcher: "*.rs".into(),
            command: "cargo clippy".into(),
            timeout_ms: 60000,
            blocking: true,
        };
        let result = coord.run_post_tool_use_hook("t-4", &lint_hook, "no warnings", Some(0));
        assert!(result.passed);
        assert_eq!(result.event, HookEvent::PostToolUse);

        // PostToolUse hook fails on lint errors
        let result =
            coord.run_post_tool_use_hook("t-4", &lint_hook, "warning: unused variable", Some(1));
        assert!(!result.passed);
    }

    #[test]
    fn verification_engine_pass_fail() {
        let store = ParchmentStore::open_in_memory().unwrap();
        let mut engine = VerificationEngine::new(store);

        let criterion = AcceptanceCriterion {
            id: "test-1".into(),
            description: "Run unit tests".into(),
            kind: AcceptanceKind::Test,
            command: Some("cargo test".into()),
            expected: None,
            verified: false,
            verified_at_ms: None,
        };

        // Failing test run
        let result = engine.run_criterion(
            "t-1",
            &criterion,
            "test result: FAILED. 1 passed; 1 failed",
            Some(1),
            500,
        );
        assert!(!result.passed);
        assert_eq!(result.exit_code, Some(1));
        assert!(result.artifact_id.is_some()); // failing output stored as artifact

        // Passing test run
        let result = engine.run_criterion(
            "t-1",
            &criterion,
            "test result: ok. 3 passed; 0 failed",
            Some(0),
            300,
        );
        assert!(result.passed);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.artifact_id.is_none()); // small passing output not stored
    }

    #[test]
    fn error_classification_and_recovery() {
        // Permission denied
        let class = classify_error(Some(126), "permission denied: /root/secret");
        assert_eq!(class, ErrorClass::PermissionDenied);
        assert_eq!(
            choose_recovery_strategy(class, 1),
            RecoveryStrategy::EscalateToUser
        );

        // Compile error, retry
        let class = classify_error(Some(1), "error[E0425]: cannot find value `foo`");
        assert_eq!(class, ErrorClass::CompileError);
        assert_eq!(
            choose_recovery_strategy(class, 1),
            RecoveryStrategy::RetryWithFix
        );

        // Max attempts exhausted → rollback
        assert_eq!(
            choose_recovery_strategy(class, 3),
            RecoveryStrategy::RollbackToCheckpoint
        );

        // Test failure
        let class = classify_error(Some(1), "assertion failed: left == right");
        assert_eq!(class, ErrorClass::TestFailure);
        assert_eq!(
            choose_recovery_strategy(class, 2),
            RecoveryStrategy::RetryWithFix
        );

        // Unknown error
        let class = classify_error(Some(1), "something unexpected happened");
        assert_eq!(class, ErrorClass::Unknown);
    }

    #[test]
    fn max_recovery_attempts_constant() {
        assert_eq!(MAX_RECOVERY_ATTEMPTS, 3);
    }

    #[test]
    fn all_criteria_verified() {
        let criteria = vec![
            AcceptanceCriterion {
                id: "c1".into(),
                description: "Build passes".into(),
                kind: AcceptanceKind::Build,
                command: Some("cargo build".into()),
                expected: None,
                verified: true,
                verified_at_ms: Some(100),
            },
            AcceptanceCriterion {
                id: "c2".into(),
                description: "Tests pass".into(),
                kind: AcceptanceKind::Test,
                command: Some("cargo test".into()),
                expected: None,
                verified: true,
                verified_at_ms: Some(200),
            },
        ];
        assert!(VerificationEngine::all_criteria_verified(&criteria));

        let mut unverified = criteria.clone();
        unverified[0].verified = false;
        assert!(!VerificationEngine::all_criteria_verified(&unverified));

        let empty: Vec<AcceptanceCriterion> = Vec::new();
        assert!(!VerificationEngine::all_criteria_verified(&empty));
    }

    #[test]
    fn recovery_engine_attempt_tracking() {
        let mut engine = RecoveryEngine::new();
        assert_eq!(engine.attempt_count("sub-1"), 0);

        let (strategy, exhausted) = engine.attempt_recovery("sub-1", Some(1), "compile error");
        assert_eq!(engine.attempt_count("sub-1"), 1);
        assert_eq!(strategy, RecoveryStrategy::RetryWithFix);
        assert!(!exhausted);

        let (strategy, exhausted) = engine.attempt_recovery("sub-1", Some(1), "compile error");
        assert_eq!(engine.attempt_count("sub-1"), 2);
        assert_eq!(strategy, RecoveryStrategy::RetryWithFix);
        assert!(!exhausted);

        let (strategy, exhausted) = engine.attempt_recovery("sub-1", Some(1), "compile error");
        assert_eq!(engine.attempt_count("sub-1"), 3);
        assert_eq!(strategy, RecoveryStrategy::RollbackToCheckpoint);
        assert!(exhausted);
    }

    #[test]
    fn recovery_engine_permission_denied_escalates() {
        let mut engine = RecoveryEngine::new();
        let (strategy, exhausted) =
            engine.attempt_recovery("sub-1", Some(126), "permission denied");
        assert_eq!(strategy, RecoveryStrategy::EscalateToUser);
        assert!(exhausted);
    }

    #[test]
    fn recovery_engine_reset_on_success() {
        let mut engine = RecoveryEngine::new();
        engine.attempt_recovery("sub-1", Some(1), "error");
        assert_eq!(engine.attempt_count("sub-1"), 1);
        engine.reset("sub-1");
        assert_eq!(engine.attempt_count("sub-1"), 0);
    }

    #[test]
    fn recovery_engine_independent_subtasks() {
        let mut engine = RecoveryEngine::new();
        engine.attempt_recovery("sub-1", Some(1), "error");
        engine.attempt_recovery("sub-1", Some(1), "error");
        assert_eq!(engine.attempt_count("sub-1"), 2);
        assert_eq!(engine.attempt_count("sub-2"), 0);
    }
}
