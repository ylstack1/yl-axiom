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
- T-101 Centralize user-visible strings (strings.rs, Strings.kt) — NOT_STARTED
- T-102 Rebrand app name "Parchment" → "YL Axiom" in user-visible strings — NOT_STARTED
- T-103 Rebrand desktop window title — NOT_STARTED
- T-104 Rebrand Android application label — NOT_STARTED
- T-105 Rebrand README, docs, installer assets — NOT_STARTED
- T-106 Preserve packageId (io.parchment.ai), JNI symbols, storage IDs, license, history — NOT_STARTED (constraint task)
- T-107 Verify rebrand via existing tests (no functional regression) — NOT_STARTED

## Phase 2 — Architecture boundaries and shared-core cleanup
- T-201 Add parchment-coding Cargo module to workspace — NOT_STARTED
- T-202 Add coding task tables to SQLite schema (non-destructive migration) — NOT_STARTED
- T-203 Verify existing harness/tests unchanged — NOT_STARTED

## Phase 3 — Axiom Coding module and independent sessions
- T-301 Define Task, TaskPlan, Subtask domain models — NOT_STARTED
- T-302 Define AcceptanceCriterion, Checkpoint, Artifact domain models — NOT_STARTED
- T-303 Implement persistent CRUD for coding tasks in storage — NOT_STARTED
- T-304 Make coding sessions independent from Chat sessions — NOT_STARTED
- T-305 Unit tests for task state transitions and CRUD — NOT_STARTED

## Phase 4 — Autonomous task orchestration, planning and checkpoints
- T-401 Implement CodingTaskCoordinator (plan → execute → verify → complete) — NOT_STARTED
- T-402 Dependency-aware subtask ordering — NOT_STARTED
- T-403 Checkpoints after verified subtask and on pause/block — NOT_STARTED
- T-404 Resumption from checkpoint — NOT_STARTED
- T-405 Stream progress via RunProgressEvent + coding events — NOT_STARTED
- T-406 Unit tests for coordinator lifecycle — NOT_STARTED

## Phase 4b — TDD loop and hook gates
- T-410 Define SubtaskKind (Explore/WriteTests/Implement/Refactor/Verify) — NOT_STARTED
- T-411 Implement Red turn (write failing tests, no impl) — NOT_STARTED
- T-412 Implement Green turn (impl only, tests read-only) — NOT_STARTED
- T-413 Implement Refactor turn (suite stays green) — NOT_STARTED
- T-414 CodingHook PostToolUse (lint after file mutation) — NOT_STARTED
- T-415 CodingHook Stop (block turn end on failing tests) — NOT_STARTED
- T-416 Hook config model + storage table — NOT_STARTED
- T-417 Unit tests for TDD phase transitions and hook gating — NOT_STARTED

## Phase 5 — Codebase exploration and editing workflows
- T-501 Structured patch workflow + diff review (parchment-prose diff) — NOT_STARTED
- T-502 Windows platform adapter (native file access, tokio::process) — NOT_STARTED
- T-503 Android platform adapter (SAF, AndroidOwnedShell) — NOT_STARTED
- T-504 Capability discovery + graceful fallback — NOT_STARTED

## Phase 5b — Code intelligence
- T-510 Add parchment-intel crate (tree-sitter) — NOT_STARTED
- T-511 Symbol search tool (read-only) — NOT_STARTED
- T-512 Document outline tool — NOT_STARTED
- T-513 AST pattern search/rewrite tools — NOT_STARTED
- T-514 CodeIntelBackend trait + optional LSP (desktop) — NOT_STARTED
- T-515 Capability discovery (Android tree-sitter only) — NOT_STARTED
- T-516 Tests for intel tools and backend selection — NOT_STARTED

## Phase 6 — Execution, build/test verification and recovery
- T-601 VerificationEngine (build/test/lint acceptance criteria) — NOT_STARTED
- T-602 ErrorClassifier + RecoveryEngine (bounded recovery) — NOT_STARTED
- T-603 Completion based on verified results — NOT_STARTED
- T-604 Unit tests for verification and recovery — NOT_STARTED

## Phase 7 — Context retrieval and artifact management
- T-701 Disk-spilled large outputs (reuse 64 KiB spill pattern) — NOT_STARTED
- T-702 Artifact storage/retrieval (patch, diff, build log, test result, review report) — NOT_STARTED
- T-703 Stable cached prefix + quantized compaction for coding turns — NOT_STARTED
- T-704 Tool-result clearing policy (exclude build/test evidence) — NOT_STARTED
- T-705 Subagent context isolation for noisy exploration — NOT_STARTED

## Phase 8 — Subagents and advanced agent workflows
- T-801 Coding subagent coordination (reuse SubagentDelegate, max 4) — NOT_STARTED
- T-802 Subagent permissions inherit parent scope — NOT_STARTED
- T-803 A2A watch-list review (no adoption) — NOT_STARTED

## Phase 9 — Windows and Android integration and capability parity
- T-901 Windows GPUI Coding UI (task overview, plan, tool approval, terminal, diffs, verification, pause/cancel/resume) — NOT_STARTED
- T-902 Android Compose Coding UI — NOT_STARTED
- T-903 Native file dialogs (Windows COM / Android SAF) — NOT_STARTED
- T-904 Optional `android-shell-fastshell` backend behind shell abstraction (Apache-2.0) — NOT_STARTED
- T-905 Bridge fastshell `exit_code = 100` / `PERMISSION_NEEDED` into ToolApprovalBroker — NOT_STARTED
- T-906 Measure `.so` budget with and without fastshell feature — NOT_STARTED
- T-907 Capability discovery for Android (tree-sitter only, restricted shell) — NOT_STARTED

## Phase 10 — Performance, security, reliability and release readiness
- T-1001 Perf budgets (exe, APK, .so, start time, memory) — NOT_STARTED
- T-1002 Security review (confinement, SSRF, approvals, MCP, secrets) — NOT_STARTED
- T-1003 Full test suite + CI — NOT_STARTED
- T-1004 Release build — NOT_STARTED

## Riskiest decisions to validate early
- R-1: SQLite schema extension for coding tasks must be non-destructive and not break v2→v3 migration (validate before Phase 3).
- R-2: CodingTaskCoordinator must not replace or duplicate AgentTurnRunner (validate thin-wrapper design before Phase 4).
- R-3: Android coding execution must gracefully fall back (no Termux/PRoot; stdio MCP unavailable) (validate before Phase 9).
- R-4: Rebrand must not change packageId, JNI symbols, storage IDs, or migration keys (validate before Phase 1).
- R-5: fastshell must stay optional and must not become a second permission authority or break the `.so` size budget (validate before Phase 9).
- R-6: Tool profiles + meta-tool must not become a permission bypass (validate before Phase 4b).
