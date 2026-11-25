# YL Axiom — Axiom Coding Engine Design

## Overview
Axiom Coding is a first-class autonomous software-engineering capability built as a thin `CodingTaskCoordinator` around the existing `AgentTurnRunner` agent loop. It does not replace the working agent infrastructure. Completion is based on verified results (build/test/lint pass), not model claims.

## Module: `crates/parchment-coding/`
Cargo feature-gated optional module. Depends on `parchment-harness`, `parchment-model`, `parchment-storage`, `parchment-core`. No network dependency of its own (uses the shared `ProtocolClient` via `parchment-core`).

## Domain models

### Task
Persistent top-level unit of work.
- `id: String` — opaque task handle.
- `title: String`, `description: String`.
- `state: TaskState` — Pending | Planning | Executing | Reviewing | Verified | Completed | Failed | Blocked | Paused.
- `workspace_path: String`, `repo_url: Option<String>`.
- `created_at_ms`, `updated_at_ms`, `completed_at_ms: Option<i64>`.
- `acceptance_criteria: Vec<AcceptanceCriterion>`.
- `plan: Option<TaskPlan>`.
- `checkpoints: Vec<Checkpoint>`.
- `artifacts: Vec<Artifact>`.
- `subtasks: Vec<Subtask>`.
- `current_subtask_index: usize`.

### TaskPlan
- `goal: String`.
- `subtasks: Vec<Subtask>` with dependency-aware ordering (`depends_on: Vec<String>`).
- `estimated_steps: u32`.

### Subtask
- `id: String`, `title: String`, `description: String`.
- `kind: SubtaskKind` (Explore | WriteTests | Implement | Refactor | Verify).
- `state: SubtaskState` — Pending | InProgress | Verified | Blocked | Skipped | Failed.
- `depends_on: Vec<String>` (subtask IDs).
- `acceptance_criteria: Vec<AcceptanceCriterion>`.
- `completed_at_ms: Option<i64>`.

### AcceptanceCriterion
- `id: String`, `description: String`, `kind: AcceptanceKind` (Build | Test | Lint | Review | FileExists | Custom).
- `command: Option<String>` (shell command to verify).
- `expected: Option<String>`, `verified: bool`, `verified_at_ms: Option<i64>`.

### SubtaskKind (drives the TDD loop)
- `Explore` — read-only investigation, no edits.
- `WriteTests` (Red) — writes failing tests; implementation edits forbidden this turn.
- `Implement` (Green) — makes tests pass; test files read-only this turn.
- `Refactor` — cleanup with the suite green.
- `Verify` — runs the full acceptance suite and records artifacts.

### CodingHook
- `id: String`, `event: HookEvent` (PostToolUse | Stop), `matcher: String` (tool name or file glob).
- `command: String`, `timeout_ms: u64`, `blocking: bool`.
- `created_at_ms`.

### Checkpoint
- `id: String`, `task_id: String`, `subtask_id: Option<String>`.
- `created_at_ms`, `description: String`.
- `snapshot_ref: String` (reference to workspace/file state or storage snapshot).
- `resumable: bool`.

### Artifact
- `id: String`, `task_id: String`, `kind: ArtifactKind` (Patch | Diff | BuildLog | TestResult | ReviewReport | ErrorReport).
- `path: String` or inline content (large outputs spill to disk per existing 64 KiB pattern).
- `bytes: u64`, `created_at_ms`.

### TaskState transitions
Pending → Planning → Executing → Reviewing → Verified → Completed
Any state → Paused (user), Blocked (external), Failed (terminal unless resumed), Completed (terminal).
Resume from Paused/Blocked/Failed reloads latest checkpoint.

## Coordinator: `CodingTaskCoordinator`
- Creates/loads/persists `Task` in SQLite (new tables, non-destructive migration).
- Drives lifecycle: plan → execute subtasks → verify acceptance → complete.
- Per subtask: invoke `AgentTurnRunner` with coding-optimized system prompt + tool scope (fs_read/fs_edit/fs_multi_edit/shell_exec + web_search if allowed).
- After each tool round, evaluate acceptance criteria via `VerificationEngine`.
- On failure: `ErrorClassifier` classifies error, `RecoveryEngine` attempts bounded recovery (max N attempts), then checkpoint + pause/escalate.
- Checkpoints after each verified subtask and on pause/block.
- Progress events streamed via `RunProgressEvent` (reuse existing) + coding-specific events.

## VerificationEngine
- Runs acceptance-criteria commands (build/test/lint) via `shell_exec`.
- Parses output, classifies pass/fail, records `Artifact` (build log, test result).
- Completion = all acceptance criteria verified. Not based on model saying "done."

## ErrorClassifier + RecoveryEngine
- Classify: compile_error, test_failure, lint_failure, permission_denied, network_error, tool_error, unknown.
- Bounded recovery: retry with corrected command, max N attempts per subtask, then escalate (pause/block + surface to user).
- Never silently swallow errors (per AGENTS.md anti-reward-hacking).

## Context management & artifacts
- Large tool outputs spill to disk (reuse `INLINE_TOOL_RESULT_LIMIT_BYTES = 64 KiB` spill pattern + artifact files).
- Bounded context: `history.rs` compaction (16-step quantized) applies to coding turns.
- Output artifacts stored as files, referenced by `Artifact` records — not stuffed into prompts.

## TDD execution loop (default)
Any subtask with testable acceptance criteria runs Red-Green-Refactor:
1. **Red** — a dedicated turn writes failing tests. The agent must not write implementation in this turn. Checkpoint is taken at the red state.
2. **Green** — a turn implements the minimum to pass. Test files are read-only for this turn (enforced by the hook layer, not by prompt).
3. **Refactor** — a turn cleans up while the suite stays green; full suite re-run at the end.

If the suite cannot be made green within the bounded attempt budget, the subtask is marked Blocked with the last failure artifact attached, and the task pauses for the user.

## Hook-based verification gates
`CodingHook` runs at two deterministic points:
- **PostToolUse** — after any file-mutating tool, run the configured lint/format check for the touched language before the next model turn.
- **Stop** — a turn cannot end while acceptance tests fail; the failure output becomes the next observation.

Hooks are configuration-driven and evaluated in Rust, so they are a control rather than a model suggestion.

## Context engineering
- Stable cached prefix first: tools, then coding instructions, then late lore/notes (matches Parchment's existing prompt-cache prefix ordering).
- Quantized compaction (reuse `COMPACTION_BATCH_STEPS = 16`) instead of truncation.
- Tool-result clearing for stale large outputs; build/test commands are excluded from clearing so verification evidence survives.
- Exploration and noisy work goes to subagents, which return a condensed result to the parent.

## Code intelligence (`parchment-intel`, optional)
- **Tree-sitter layer (built-in, no server):** symbol search, document outline, AST pattern search, AST pattern rewrite.
- **LSP layer (optional, per-workspace):** find references, go to definition, hover docs, rename, diagnostics via `CodeIntelBackend` trait.
- Desktop: both layers. Android: tree-sitter only, reported via capability discovery.
- Exposed as read-only tools so they inherit existing approval/permission handling.

## Sandbox-is-a-tool
Shell runs only when code must execute, at the narrowest sufficient mode:
- read-only default,
- workspace-write when the task edits files,
- network-restricted unless web access is explicitly granted.
Android reports its restricted shell instead of claiming parity.

## Subagent coordination
- Reuse existing `SubagentDelegate` + `MAX_PARALLEL_SUBAGENTS = 4`.
- Coding coordinator spawns subagents for independent subtasks (e.g., lint in parallel with test).
- Subagent permissions inherit parent task scope; a subagent cannot expand workspace/shell permissions.
- Each subagent gets a clean context and returns only a structured summary.
- A2A is on the watch list, not implemented.

## Borrowed ideas (reimplemented under MIT, never copied)
From `aacode-rs` (GPL-3.0 — ideas only, no code):
- Frozen-cutoff context retrieval: pin an immutable prefix boundary so compaction never rewrites earlier turns. Adopted in the coding context profile (Phase 7).
- Shell exit-code classification beyond pass/fail (permission-denied, not-found, timeout) — adopted in `recovery.rs`.
- Opaque task handles with per-handle cancellation — coding tasks follow the existing `Arc<ParchmentCore>` handle pattern.
- Fail-closed read-only mode as the coding default (Parchment already has `WorkspaceMode::ReadOnly`).
- Interrupted tool-call history repair extended to coding turns (builds on `ReplayToolCallIds`).
- Plan-first as an explicit task phase (already Phase 4).
- Tool profiles + discovery meta-tool — adopted as Decision 15.
- Already present in Parchment, so no work needed: large tool-output disk spilling, path confinement, dangerous-command rejection, network permission gating.

From `fastshell` (Apache-2.0 — usable, optional):
- Embedded command runtime for Android (180+ commands, pipelines, glob, RustPython) as an **optional** feature-gated backend behind the existing shell abstraction (Decision 6b, Phase 9).
- `exit_code = 100` / `PERMISSION_NEEDED:<resource>` protocol, bridged into `ToolApprovalBroker` so there is exactly one permission authority.
- Virtual filesystem with path-escape prevention, `command_timeout_ms`, `cancel_execution()` — mapped onto `WorkspacePathGuard` and existing cancellation.

Rejected outright:
- Any GPL code from aacode-rs.
- `run_shell`-as-universal-adapter (Parchment's typed 21-tool catalog is safer and already permission-gated).
- A second permission authority from fastshell's `network_ask_permission`.
- Replacing `WorkspacePathGuard` with a foreign VFS.

## Platform adapters
- `CodingPlatform` trait: filesystem, shell, process, git, project discovery, code-intel backend.
- Windows: native file access, `tokio::process::Command`, PowerShell.
- Android: SAF, `AndroidOwnedShell`, graceful fallback (limited dev tools, tree-sitter only).
- Capability discovery: `CodingPlatform::capabilities()` returns available features; UI degrades gracefully.

## UI workspace (Axiom Coding)
- Task overview, plan + subtask visibility, agent activity/execution events.
- Tool approval interface (reuse `ToolApprovalBroker`).
- Terminal/output visibility (reuse streaming).
- Changed files and diffs (reuse `parchment-prose` diff engine).
- Verification results (build/test/lint pass/fail).
- Agent/subagent status.
- Pause, cancel, resume controls.

## What is NOT built now
- No browser automation.
- No multi-agent teams beyond existing subagent delegate; A2A is watch-list only.
- No unnecessary plugin framework.

## Files affected (new)
- `crates/parchment-coding/Cargo.toml`
- `crates/parchment-coding/src/lib.rs`
- `crates/parchment-coding/src/coordinator.rs`
- `crates/parchment-coding/src/task.rs`
- `crates/parchment-coding/src/plan.rs`
- `crates/parchment-coding/src/checkpoint.rs`
- `crates/parchment-coding/src/verify.rs`
- `crates/parchment-coding/src/hooks.rs`
- `crates/parchment-coding/src/context.rs`
- `crates/parchment-coding/src/recovery.rs`
- `crates/parchment-coding/src/platform.rs`
- `crates/parchment-coding/src/error.rs`
- `crates/parchment-intel/` (optional: tree-sitter symbol/outline/AST tools + `CodeIntelBackend` trait)
- `crates/parchment-storage/src/schema.rs` (new tables: tasks, task_plans, subtasks, acceptance_criteria, checkpoints, artifacts, hooks)
- `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/ui/` (Axiom Coding screens — new files)
- `crates/parchment-desktop/src/` (Axiom Coding desktop UI — extend existing)

## Status
Proposed. Not yet implemented.
