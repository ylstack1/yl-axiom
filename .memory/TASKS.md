# YL Axiom — Task Backlog

Statuses: NOT_STARTED | IN_PROGRESS | BLOCKED | NEEDS_REVIEW | VERIFIED_COMPLETE

## Phase 0 — Repository discovery and product definition (COMPLETE)
- T-001 Inspect repository structure and all source code — VERIFIED_COMPLETE
- T-002 Establish actual current project state — VERIFIED_COMPLETE
- T-003 Audit existing functionality and identify reusable components — VERIFIED_COMPLETE
- T-004 Define target YL Axiom product architecture — VERIFIED_COMPLETE
- T-005 Produce shared-core vs optional-module design — VERIFIED_COMPLETE
- T-006 Define Windows and Android native integration strategies — VERIFIED_COMPLETE
- T-007 Create rebranding plan — VERIFIED_COMPLETE
- T-008 Identify missing autonomous coding capabilities — VERIFIED_COMPLETE
- T-009 Create security, performance, testing strategy — VERIFIED_COMPLETE
- T-010 Write engineering documentation — VERIFIED_COMPLETE
- T-011 Create dependency-aware implementation roadmap — VERIFIED_COMPLETE
- T-012 Create actionable prioritized task backlog — VERIFIED_COMPLETE
- T-013 Identify highest-risk architectural decisions requiring validation — VERIFIED_COMPLETE
- T-014 Produce concise planning report — VERIFIED_COMPLETE
- T-015 Web research of 2026 agent/coding practice (MCP 2026-07-28, Agent Skills, TDD, hooks, context engineering, code intelligence, sandboxing, A2A) — VERIFIED_COMPLETE
- T-016 Update plan with researched best practices — VERIFIED_COMPLETE

## Phase 1 — Product identity, rebranding and application foundation
- T-101 Centralize user-visible strings (strings.rs, Strings.kt) — COMPLETE
- T-102 Rebrand app name "Parchment" → "YL Axiom" in user-visible strings — COMPLETE
- T-103 Rebrand desktop window title — COMPLETE
- T-104 Rebrand Android application label — COMPLETE
- T-105 Rebrand README, docs, installer assets — NOT_STARTED (README/docs still say Parchment)
- T-106 Preserve packageId (io.parchment.ai), JNI symbols, storage IDs, license, history — COMPLETE (constraint task)
- T-107 Verify rebrand via existing tests (no functional regression) — VERIFIED_COMPLETE (CI green on feat/axiom-coding-ui)

## Phase 2 — Architecture boundaries and shared-core cleanup
- T-201 Add parchment-coding Cargo module to workspace — COMPLETE
- T-202 Add coding task tables to SQLite schema (non-destructive migration) — COMPLETE
- T-203 Verify existing harness/tests unchanged — VERIFIED_COMPLETE (22 tests pass on main)

## Phase 3 — Axiom Coding module and independent sessions
- T-301 Define Task, TaskPlan, Subtask domain models — COMPLETE (parchment-coding/lib.rs)
- T-302 Define AcceptanceCriterion, Checkpoint, Artifact domain models — COMPLETE (parchment-coding/lib.rs)
- T-303 Implement persistent CRUD for coding tasks in storage — COMPLETE (parchment-coding/lib.rs)
- T-304 Make coding sessions independent from Chat sessions — NOT_STARTED (coordinator has no callers)
- T-305 Unit tests for task state transitions and CRUD — PARTIAL (task models exist, TDD/verification tests not implemented)

## Phase 4 — Autonomous task orchestration, planning and checkpoints
- T-401 Implement CodingTaskCoordinator (plan → execute → verify → complete) — COMPLETE (run_turn + JNI + Kotlin wiring)
- T-402 Dependency-aware subtask ordering — NOT_STARTED
- T-403 Checkpoints after verified subtask and on pause/block — PARTIAL (create_checkpoint CRUD only, no verified-subtask gating)
- T-404 Resumption from checkpoint — NOT_STARTED
- T-405 Stream progress via RunProgressEvent + coding events — NOT_STARTED
- T-406 Unit tests for coordinator lifecycle — NOT_STARTED

## Phase 4b — TDD loop and hook gates
- T-410 Define SubtaskKind (Explore/WriteTests/Implement/Refactor/Verify) — COMPLETE (enum only)
- T-411 Implement Red turn (write failing tests, no impl) — NOT_STARTED
- T-412 Implement Green turn (impl only, tests read-only) — NOT_STARTED
- T-413 Implement Refactor turn (suite stays green) — NOT_STARTED
- T-414 CodingHook PostToolUse (lint after file mutation) — NOT_STARTED
- T-415 CodingHook Stop (block turn end on failing tests) — NOT_STARTED
- T-416 Hook config model + storage table — NOT_STARTED
- T-417 Unit tests for TDD phase transitions and hook gating — NOT_STARTED

## Phase 5 — Codebase exploration and editing workflows
- T-501 Structured patch workflow + diff review (parchment-prose diff) — PARTIAL (PatchStore/FileEditor exist, no task integration)
- T-502 Windows platform adapter (native file access, tokio::process) — NOT_STARTED
- T-503 Android platform adapter (SAF, AndroidOwnedShell) — PARTIAL (ParchmentNativeCore JNI exists, no sandboxed shell)
- T-504 Capability discovery + graceful fallback — NOT_STARTED

## Phase 5b — Code intelligence
- T-510 Add parchment-intel crate (tree-sitter) — PARTIAL (CodeIntelBackend trait shell only, no backend)
- T-511 Symbol search tool (read-only) — NOT_STARTED
- T-512 Document outline tool — NOT_STARTED
- T-513 AST pattern search/rewrite tools — NOT_STARTED
- T-514 CodeIntelBackend trait + optional LSP (desktop) — NOT_STARTED
- T-515 Capability discovery (Android tree-sitter only) — NOT_STARTED
- T-516 Tests for intel tools and backend selection — NOT_STARTED

## Phase 6 — Execution, build/test verification and recovery
- T-601 VerificationEngine (build/test/lint acceptance criteria) — IN_PROGRESS (Rust struct exists, shell command execution pending)
- T-602 ErrorClassifier + RecoveryEngine (bounded recovery) — IN_PROGRESS (Rust struct exists, needs integration into run_turn)
- T-603 Completion based on verified results — NOT_STARTED
- T-604 Unit tests for verification and recovery — NOT_STARTED

## Phase 7 — Context retrieval and artifact management
- T-701 Disk-spilled large outputs — NOT_STARTED
- T-702 Artifact storage/retrieval — NOT_STARTED
- T-703 Stable cached prefix + quantized compaction for coding turns — VERIFIED_COMPLETE
- T-704 Tool-result clearing policy — VERIFIED_COMPLETE
- T-705 Subagent context isolation for noisy exploration — NOT_STARTED

## Phase 8 — Subagents and advanced agent workflows
- T-801 Coding subagent coordination — NOT_STARTED
- T-802 Subagent permissions inherit parent scope — NOT_STARTED
- T-803 A2A watch-list review — NOT_STARTED

## Phase 9 — Windows and Android integration and capability parity
- T-901 Windows GPUI Coding UI — NOT_STARTED
- T-902 Android Compose Coding UI — PARTIAL (basic task list screen only, not a coding environment)
- T-903 Native file dialogs — NOT_STARTED
- T-904 Optional android-shell-fastshell backend — NOT_STARTED
- T-905 Bridge fastshell exit_code/PERMISSION_NEEDED — NOT_STARTED
- T-906 Measure .so budget — NOT_STARTED
- T-907 Capability discovery for Android — NOT_STARTED

## Phase 10 — Performance, security, reliability and release readiness
- T-1001 Perf budgets — NOT_STARTED
- T-1002 Security review — NOT_STARTED
- T-1003 Full test suite + CI — PARTIAL (CI green on main, but agent-loop tests missing)
- T-1004 Release build — NOT_STARTED

## Riskiest decisions to validate early
- R-1: SQLite schema extension for coding tasks must be non-destructive and not break v2→v3 migration (validate before Phase 3).
- R-2: CodingTaskCoordinator must not replace or duplicate AgentTurnRunner (validate thin-wrapper design before Phase 4).
- R-3: Android coding execution must gracefully fall back (no Termux/PRoot; stdio MCP unavailable) (validate before Phase 9).
- R-4: Rebrand must not change packageId, JNI symbols, storage IDs, or migration keys (validate before Phase 1).
- R-5: fastshell must stay optional and must not become a second permission authority or break the `.so` size budget (validate before Phase 9).
- R-6: Tool profiles + meta-tool must not become a permission bypass (validate before Phase 4b).
