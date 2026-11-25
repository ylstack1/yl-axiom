# YL Axiom — Implementation Roadmap

## Phases

**Phase 0 — Repository discovery and product definition.** (this document; complete)
- Inspect repo, audit all crates, Android, build/CI. Define product vision, requirements, architecture decisions, rebranding plan, security/perf/testing strategy.

**Phase 1 — Product identity, rebranding and application foundation.**
- Centralize user-visible strings; rebrand app name/window title/Android label/docs/README/installer.
- Preserve packageId, storage IDs, JNI symbols, license, history.
- No functional change; verified via existing tests.

**Phase 2 — Architecture boundaries and shared-core cleanup (only where necessary).**
- Add `parchment-coding` optional Cargo module to workspace.
- Add coding task tables to SQLite schema (non-destructive migration).
- No rewrite of working harness.

**Phase 3 — Axiom Coding module and independent sessions.**
- `Task`, `TaskPlan`, `Subtask`, `AcceptanceCriterion`, `Checkpoint`, `Artifact` domain models.
- Persistent CRUD in storage; coding sessions independent from Chat sessions.

**Phase 4 — Autonomous task orchestration, planning and checkpoints.**
- `CodingTaskCoordinator`: plan → execute → verify → complete lifecycle.
- Checkpoints after each verified subtask and on pause/block; resumption.
- Dependency-aware subtask ordering.

**Phase 4b — TDD loop and hook gates (new).**
- `SubtaskKind` (Explore / WriteTests / Implement / Refactor / Verify) driving Red-Green-Refactor.
- `CodingHook` PostToolUse (lint after file mutation) and Stop (block turn end on failing tests).
- Test files read-only during Green; implementation forbidden during Red.

**Phase 5 — Codebase exploration and editing workflows.**
- Reuse `fs_read`/`fs_edit`/`fs_multi_edit`/`fs_search`; structured patch workflow + diff review (`parchment-prose` diff).
- Platform adapters (Windows native / Android SAF).

**Phase 5b — Code intelligence (new).**
- `parchment-intel`: tree-sitter symbol search, outline, AST pattern search/rewrite (read-only tools).
- `CodeIntelBackend` trait with optional LSP (desktop only); Android tree-sitter only.

**Phase 6 — Execution, build/test verification and recovery.**
- `VerificationEngine`: build/test/lint acceptance criteria.
- `ErrorClassifier` + `RecoveryEngine`: bounded recovery, max attempts, escalate.
- Completion based on verified results.

**Phase 7 — Context retrieval and artifact management.**
- Disk-spilled large outputs (reuse 64 KiB spill pattern).
- Stable cached prefix + quantized compaction + tool-result clearing (build/test evidence excluded).
- Subagent isolation for noisy exploration.
- Artifact storage and retrieval (patch, diff, build log, test result, review report).

**Phase 8 — Subagents and advanced agent workflows.**
- Reuse `SubagentDelegate` (max 4 parallel). Coding subagent coordination for independent subtasks.
- Subagent permissions inherit parent scope.
- A2A monitored, not adopted.

**Phase 9 — Windows and Android integration and capability parity.**
- Windows: native file dialogs, PowerShell, process mgmt, GPUI Coding UI, full code intel.
- Android: Compose Coding UI, SAF, permissions, FGS, graceful fallback, tree-sitter only.
- Optional: `android-shell-fastshell` feature (Apache-2.0, Decision 6b) behind the existing shell abstraction, gated on a measured `.so` budget; its `exit_code = 100` / `PERMISSION_NEEDED` protocol bridged into `ToolApprovalBroker` (never a second authority).
- Capability discovery; no pretending Windows-only features on Android.

**Phase 10 — Performance, security, reliability and release readiness.**
- Perf budgets (exe <= 35 MB, APK <= 7.50 MB, .so 1.85 MB, start <= 0.35 s).
- Security review (confinement, SSRF, approvals, MCP, secrets, prompt injection, sandbox defaults).
- Adversarial test pass, full test suite, CI, release build.

## Dependency notes
- Phase 3 depends on Phase 2 (storage tables).
- Phase 4 depends on Phase 3 (task model).
- Phase 4b depends on Phase 4 (coordinator) and Phase 2 (hook table).
- Phase 5b depends on Phase 5 (editing) and Phase 2 (intel module).
- Phase 6 depends on Phase 4/4b (executor produces results to verify).
- Phases 7-8 are additive to Phase 4-6.
- Phase 9 builds UI on top of Phases 3-8.
- Phase 10 validates all prior phases.

## Status
Phase 0 complete. Phases 1-10 NOT_STARTED (4b and 5b added after 2026 research).
