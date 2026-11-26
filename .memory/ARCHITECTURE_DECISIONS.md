# YL Axiom — Architecture Decisions

## Decision 1: Extend Parchment core; do not rebuild
**Decision:** Build Axiom Coding as an extension of the existing Parchment agent harness (`AgentTurnRunner`, `HarnessExecutor`, `ToolApprovalBroker`, `AgentPermissionGate`, `WorkspacePathGuard`, `McpRegistry`, `SubagentDelegate`), not a replacement.

**Rationale:** The harness is fully implemented and tested (21 tools, approval, permissions, MCP, subagents, path guard, SSRF transport). Rebuilding it would duplicate infrastructure and break working Assistant/Chat behavior.

**Files affected:** `crates/parchment-harness/`, `crates/parchment-core/`, `crates/parchment-model/`, `crates/parchment-storage/`.

**Status:** Proposed.

---

## Decision 2: Shared core vs optional modules
**Decision:** Split the Rust core into:
- **Shared core (always on):** `parchment-model`, `parchment-storage`, `parchment-prose`, `parchment-harness` (agent loop, tools, permissions, approval, path guard, SSRF, codecs, math/dice), `parchment-core` facade. Used by both Assistant/Chat and Axiom Coding on Windows and Android.
- **Optional coding module (`parchment-coding`):** Cargo feature-gated module adding coding task lifecycle, planning, checkpoints, file-editing patch workflow, build/test execution, error recovery, verification, context/artifact management. Enabled where Axiom Coding is needed.
- **Optional network/protocol module (`parchment-protocol`):** Already feature-gated via `parchment-core` `network` flag. Windows desktop uses `network`; Android slim JNI uses `android-jni` (no network in `.so`).

**Rationale:** Clean module boundaries, optional Cargo features, runtime capability registration, independent module testing. Avoids duplicating infrastructure. Keeps Android `.so` slim.

**Files affected:** New `crates/parchment-coding/`; `Cargo.toml` workspace members; `crates/parchment-core/Cargo.toml` features.

**Status:** Proposed.

---

## Decision 3: Thin Coding task coordinator around existing agent loop
**Decision:** Axiom Coding uses a thin `CodingTaskCoordinator` around `AgentTurnRunner`. It owns task lifecycle (`Task` → `TaskPlan` → subtasks → execution → checkpoints → verification → done) and delegates per-turn model interaction to the existing `AgentTurnRunner`. It does not replace the agent loop.

**Rationale:** "Prefer a thin Coding task coordinator around existing agent infrastructure rather than replacing the working agent loop." Completion based on verified results (build/test/lint pass), not model claims.

**Files affected:** `crates/parchment-coding/src/coordinator.rs`, `task.rs`, `plan.rs`, `checkpoint.rs`, `verify.rs`, `recovery.rs`.

**Status:** Proposed.

---

## Decision 4: Coding task lifecycle states
**Decision:** Task states: `Pending` → `Planning` → `Executing` → `Reviewing` → `Verified` → `Completed`/`Failed`/`Blocked`/`Paused`. Persistent `Task` + `TaskPlan` + `Subtask` domain models stored in SQLite (new tables, non-destructive migration alongside existing schema).

**Rationale:** Persistent task state enables checkpoints and resumption. New tables added to existing schema (version bump) without altering existing tables.

**Files affected:** `crates/parchment-coding/src/task.rs`, `crates/parchment-storage/src/schema.rs` (new tables), `crates/parchment-storage/src/migrate_v2.rs` (if v3→v4 needed; note: v3 is the current schema, coding adds v4).

**Status:** Proposed.

---

## Decision 5: Windows and Android native integration strategies
**Windows:** Native Rust GPUI desktop shell (`parchment-desktop`). Native filesystem, process execution (`tokio::process::Command`), PowerShell integration, Git/dev tools/project discovery, cancellation, process management, permissions, native file dialogs (Win32 COM). Shared Rust core via `network` feature.
**Android:** Native Kotlin/Jetpack Compose UI. Shared Rust agent engine via JNI (`parchment-core` `android-jni` slim `.so`). Android-compatible native libs, SAF for filesystem, Android permissions/lifecycle, FGS for background runs, cancellation. Capability discovery + graceful fallback (no pretending Windows-only features exist on Android).
**Shared engine:** Same Rust agent brain, contracts, task lifecycle, orchestration, core logic on both. Platform adapters handle filesystem/shell/process/permissions/dev-tools differences. No Kotlin agent engine duplicate.

**Files affected:** `crates/parchment-desktop/`, `mobile/compose-app/src/androidMain/`, `mobile/compose-app/src/commonMain/`, `crates/parchment-core/src/jni_android.rs`, new `crates/parchment-coding/` platform adapters.

**Status:** Proposed.

---

## Decision 6: aacode-rs — ideas only, never code
**Decision:** Do not add `aacode-rs` as a dependency and do not copy its source. It is GPL-3.0-only, which is incompatible with YL Axiom's MIT foundation. Its *architectural ideas* may be reimplemented independently under MIT where the audit shows a real need.

**Borrow-worthy ideas (reimplement, do not copy):**
- Frozen-cutoff context retrieval — pin an immutable prefix boundary so compaction never rewrites earlier turns.
- Large tool-output disk spilling — already present in Parchment (`INLINE_TOOL_RESULT_LIMIT_BYTES`, shell head/tail).
- Shell exit-code classification — richer than pass/fail (permission-denied, not-found, timeout).
- Opaque task handles with per-handle cancellation — Parchment already has `Arc<ParchmentCore>` handles; coding tasks should follow the same opaque-handle pattern.
- Fail-closed read-only mode — Parchment already has `WorkspaceMode::ReadOnly`; make it the coding default.
- Interrupted tool-call history repair — partially present (`ReplayToolCallIds`, interrupted-run handling); extend for coding turns.
- Plan-first mode as an explicit task phase — matches Phase 4 planning.
- Tool profiles (core/auto/web/full) with hidden-but-callable tools via a discovery meta-tool, so the advertised schema set (and prompt-cache prefix) stays stable — strong idea, adopted as Decision 15.

**Explicitly rejected:** its GPL source, its `run_shell`-as-universal-adapter design (YL Axiom has a typed 21-tool catalog, which is safer), and its env-var configuration model (YL Axiom uses typed serde models).

**Status:** Verified decision.

---

## Decision 6b: fastshell — optional Android execution backend, not a core dependency
**Decision:** `fastshell` is Apache-2.0 (permissive, MIT-compatible) and is therefore *usable*, unlike aacode-rs. It is adopted only as an **optional, feature-gated Android execution backend** behind the existing shell abstraction — never as a replacement for the desktop shell and never in the default build.

**Why it is attractive:** it solves the real Android gap (no native Bash) with 180+ built-in commands, pipelines, glob expansion, an embedded RustPython, a virtual filesystem with path-escape prevention, `command_timeout_ms`, `cancel_execution()`, and a permission protocol that returns `exit_code = 100` with `PERMISSION_NEEDED:<resource>` so the *host* decides — which maps cleanly onto YL Axiom's `ToolApprovalBroker`.

**Constraints and conditions:**
- Optional Cargo feature (`android-shell-fastshell`), off by default; the slim `.so` budget (1.85 MB) must be re-measured if enabled.
- Its `DevicePlugin` trait is the integration seam for Android camera/clipboard/location; YL Axiom registers only what it needs.
- On mobile it is single-process (`allow_subprocess = false`), which avoids the Android 12+ phantom-process killer — a genuine reliability win.
- Its permission model must be *bridged* to `ToolApprovalBroker`, not allowed to become a second authority: `exit_code = 100` becomes a `ToolApprovalRequest`, and the broker's decision is the only thing that grants access.
- LGPL note: the repo uses local Apache-2.0 shims for `malachite-*`; a crates.io dependency would pull LGPL crates. If adopted, vendor with the shims or pin a permissive build.
- Reject: its `network_ask_permission` as an independent grant path, and any suggestion that it replaces `WorkspacePathGuard`.

**Status:** Proposed, optional, deferred to Phase 9 (Android parity) and gated on a measured `.so` size check.

---

## Decision 15: Tool profiles with a discovery meta-tool
**Decision:** Advertise a stable *tool profile* (core / auto / web / full) instead of the full catalog on every turn. Additional tools stay callable through a discovery meta-tool (`list_tool_schemas`, `get_tool_schema`, `call_tool`) without changing the advertised schema set, so the prompt-cache prefix stays stable.

**Rationale:** Taken from aacode-rs's design (idea only, reimplemented). Parchment already defers MCP schemas past `DEFERRED_MCP_SCHEMA_THRESHOLD = 6`; this generalizes the same idea to coding tools and protects the cache prefix that `history.rs` is built around.

**Constraint:** the meta-tool must not become a permission bypass — `call_tool` goes through the same `AgentPermissionGate` + `ToolApprovalBroker` + `ToolExecutionPermit::claim` path as a direct call. "Hidden" means unadvertised, never unregulated.

**Files affected:** `crates/parchment-harness/src/permissions.rs` (profiles), `crates/parchment-coding/src/tools.rs`.

**Status:** Proposed.

---

## Decision 7: Rebranding scope — safe, non-breaking
**Decision:** Rebrand user-visible names (app name, window title, Android label, icons, splash, sidebar, settings/About, onboarding, descriptions, empty states, errors, notifications, docs, README, package metadata, build/release artifacts). Do NOT blindly rename internal crate/module identifiers, historical attribution, license/copyright, migration-sensitive storage IDs, or external integration identifiers. Centralize user-visible strings for easy future changes.

**Rationale:** Avoid breaking package IDs, migrations, storage, deep links, integrations. Distinguish user-visible branding vs internal identifiers vs historical attribution vs license vs migration IDs vs external IDs.

**Files affected:** `mobile/android-app/build.gradle.kts` (applicationId preserved), `mobile/compose-app/src/androidMain/AndroidManifest.xml` (label), `crates/parchment-desktop/src/strings.rs` (APP_NAME), `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/strings/Strings.kt`, `README.md`, docs, installer, `.release-please-manifest.json`.

**Status:** Proposed.

---

## Decision 8: Do not break existing Assistant/Chat
**Decision:** All existing Assistant and Chat behavior, tools, sessions, provider integrations, configuration, storage, and migration compatibility preserved. Axiom Coding is additive.

**Status:** Requirement (from product vision).

---

## Decision 9: TDD as the default Axiom Coding execution loop
**Decision:** Axiom Coding runs a Red-Green-Refactor loop by default for any task with testable acceptance criteria:
1. Red — write failing tests that define required behavior (agent never writes implementation in the same turn).
2. Green — implement minimum code to pass; tests must not be modified.
3. Refactor — clean up with green tests as the safety net; re-run full suite.

Completion is the external test suite passing, not the model asserting success. This is the strongest available verification signal and survives context compaction because the tests are on disk.

**Rationale:** 2026 industry consensus (Codex CLI TDD guides, VS Code agent TDD, agentpatterns.ai) is that TDD is the most reliable autonomous-coding pattern; RLVR-trained models converge in 2-3 iterations against a fixed suite. Parchment already has Red-Green-Refactor in AGENTS.md, so this aligns with existing rules rather than adding a new one.

**Files affected:** `crates/parchment-coding/src/verify.rs`, `coordinator.rs`, `plan.rs` (subtask kinds gain `WriteTests` / `Implement` / `Refactor`).

**Status:** Proposed.

---

## Decision 10: Hook-based verification gates (PostToolUse / Stop)
**Decision:** Add a deterministic `CodingHook` layer with two mandatory gates:
- PostToolUse: after any file-mutating tool (`fs_write`, `fs_edit`, `fs_multi_edit`), run the configured lint/format check for the touched language before the next model turn.
- Stop: a turn may not end while acceptance tests are failing; the runner returns the failure output as the next observation instead of yielding to the user.

Hooks are configuration, not model instructions, so they are a control rather than a suggestion.

**Rationale:** Matches the 2026 Codex CLI hook model and the harness rule "gate the actions you can't undo". Parchment's `HarnessExecutor` already centralizes dispatch, so a post-dispatch hook is a small addition at one point rather than a new subsystem.

**Files affected:** `crates/parchment-coding/src/hooks.rs`, `crates/parchment-harness/src/tools.rs` (single post-dispatch hook point), config in `parchment-model`.

**Status:** Proposed.

---

## Decision 11: Context engineering with a frozen stable prefix and subagent isolation
**Decision:** Coding turns use:
- A cached stable prefix (tools + coding instructions first, unchanged across turns) to maximize prompt-cache hits.
- Quantized compaction (reuse `COMPACTION_BATCH_STEPS = 16` from `history.rs`) instead of truncation.
- Tool-result clearing for stale large outputs, with test/build commands excluded from clearing.
- Subagents for exploration and noisy work so the parent context receives only a condensed result.

**Rationale:** 2026 context-engineering guidance converges on stable cached prefix + compaction + tool-result clearing + subagent isolation. Parchment already implements the first, third (16-step quantized compaction) and fourth (`SubagentDelegate`), so this is mostly configuration plus one clearing policy.

**Files affected:** `crates/parchment-protocol/src/history.rs` (coding profile), `crates/parchment-coding/src/context.rs`.

**Status:** Proposed.

---

## Decision 12: Code intelligence via tree-sitter first, LSP as an optional extension point
**Decision:** Add a `parchment-intel` optional module providing tree-sitter-based symbol search, document outline, and AST pattern search/rewrite. LSP (rust-analyzer, typescript-language-server, pyright, gopls, clangd, jdtls, kotlin-language-server) is an extension point behind a `CodeIntelBackend` trait, enabled per-workspace, never required.

Desktop supports both layers. Android supports tree-sitter only (no language servers), reported through capability discovery.

**Rationale:** 2026 practice (Kiro, carv, krait, CodeRLM, tsindex) shows tree-sitter gives most of the token savings without a server, and LSP adds precision where a server exists. This gives real capability now without making LSP a hard dependency or bloating the Android `.so`.

**Files affected:** New `crates/parchment-intel/`, `crates/parchment-coding/src/platform.rs` (backend trait), `crates/parchment-coding/src/tools.rs` (new read-only tools).

**Status:** Proposed. Deferred to Phase 5b (after basic editing works).

---

## Decision 13: Sandbox-is-a-tool execution model
**Decision:** Shell execution is invoked only when the agent needs to run code, under the narrowest mode that works: read-only by default, workspace-write when the task requires edits, and network-restricted unless web access is explicitly granted. Android keeps its current limited shell and reports the restriction rather than pretending parity.

**Rationale:** 2026 harness guidance is explicit that a sandbox should be started on demand rather than wrapping every turn, and that blast radius is what matters. Parchment's `ShellAccess` + `WorkspacePathGuard` + `PublicWebTransport` already provide the primitives; this decision fixes the defaults and the per-task escalation path.

**Files affected:** `crates/parchment-coding/src/platform.rs`, `crates/parchment-model/src/permissions.rs` (coding defaults), `crates/parchment-harness/src/tools.rs`.

**Status:** Proposed.

---

## Decision 14: A2A is monitored, not adopted
**Decision:** Do not implement the Agent2Agent protocol now. Track it (Linux Foundation, v1.0, 150+ organizations) as a future interoperability option for multi-agent teams. Internal coordination stays on the existing `SubagentDelegate` (max 4) with structured result summaries.

**Rationale:** A2A solves cross-vendor agent interop, which is not a YL Axiom requirement yet; adopting it now would add a protocol layer with no consumer. MCP already covers tool/context integration.

**Files affected:** none now; revisit in Phase 8.

**Status:** Proposed (watch list).

---

## Open decisions / alternatives to validate
| Decision | Options | Recommendation |
|---|---|---|
| Coding task storage | New SQLite tables in existing `parchment_storage` vs separate coding DB | New tables in existing store (one DB, non-destructive migration) |
| Plan dependency representation | Adjacency list vs DAG with topological execution | Adjacency list (subtask depends_on IDs), simple topological order |
| Checkpoint granularity | Per-turn vs per-subtask vs per-task | Per-turn + per-subtask checkpoints |
| Large output spill | Reuse 64 KiB `INLINE_TOOL_RESULT_LIMIT_BYTES` pattern | Reuse existing pattern; extend to artifacts |
| Code intelligence backend | tree-sitter only vs LSP only vs both | tree-sitter now, LSP behind trait |
| TDD enforcement | Always vs opt-in per task | Default on when acceptance criteria are testable; opt-out recorded on the task |
| Hook strictness | Warn vs block turn end | Block turn end on failing acceptance tests |
| MCP revision | Keep 2025-11-25 primary vs move to 2026-07-28 | Keep current primary + fallback; add 2026-07-28 as a first-class option in a later phase |
| Sandbox default | Workspace-write vs read-only | Read-only default, escalate per task |
| Android shell backend | Current `AndroidOwnedShell` vs optional fastshell | Keep current default; add fastshell as an optional feature-gated backend in Phase 9, measured against the `.so` budget |
| Tool advertisement | Full catalog vs profiles + meta-tool | Profiles + discovery meta-tool (Decision 15), with the same permission path |
