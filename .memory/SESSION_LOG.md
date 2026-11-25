# YL Axiom — Session Log

## 2026-11-20 (research + plan update session)
- Web research of current (2026) agent/coding practice, reading primary sources: MCP 2026-07-28 spec + changelog, Agent Skills specification, A2A v1.0 spec, Claude Code subagents docs, Codex CLI sandbox/TDD/hook practice, Kiro code-intelligence model, 2026 context-engineering guidance, 2026 agent-security checklist.
- Inspected `kandada/aacode-rs` (GPL-3.0) and `kandada/fastshell` (Apache-2.0) at the user's suggestion.
- Key findings applied to the plan:
  - MCP 2026-07-28 is stateless (no sessions, no initialize, `server/discover`, `subscriptions/listen`); current code already tolerates it as fallback, so no immediate change — recorded as a watch item.
  - TDD (Red-Green-Refactor) is the dominant 2026 autonomous-coding pattern → new Decision 9, `SubtaskKind`, Phase 4b.
  - Hook-based verification gates (PostToolUse lint, Stop on failing tests) → Decision 10, Phase 4b.
  - Context engineering (stable cached prefix, quantized compaction, tool-result clearing, subagent isolation) → Decision 11, Phase 7 tasks.
  - Code intelligence: tree-sitter first, LSP optional → Decision 12, new `parchment-intel`, Phase 5b.
  - Sandbox-is-a-tool with narrowest-mode defaults → Decision 13.
  - A2A monitored, not adopted → Decision 14.
  - aacode-rs: ideas only, no code (GPL) → Decision 6 expanded with an explicit borrow list.
  - fastshell: Apache-2.0, adopted only as an optional feature-gated Android backend with permission bridging → Decision 6b, Phase 9 tasks T-904..T-907, risks R-5/R-6.
  - Tool profiles + discovery meta-tool (from aacode-rs idea) → Decision 15, with an explicit no-bypass constraint.
- Updated: ARCHITECTURE_DECISIONS (9 new decisions), CODING_ENGINE_DESIGN (TDD loop, hooks, context, intel, borrowed ideas), SECURITY_AND_PERFORMANCE (2026 threat model, MCP/skills hardening, sandbox defaults, TDD testing), ROADMAP (Phase 4b, 5b, fastshell in Phase 9), TASKS (T-015/T-016, Phase 4b, 5b, 9 additions, R-5/R-6), RISKS_AND_BLOCKERS (R-5, R-6), CURRENT_STATE, PRODUCT_REQUIREMENTS, NATIVE_PLATFORM_STRATEGY, REPOSITORY_AUDIT, TESTING_STRATEGY.
- No source code modified; still discovery/planning only.

## 2026-11-20 (initial discovery session)
- Performed deep repository discovery: read all 8 Rust crates (model, storage, protocol, prose, harness, core, desktop, xtask), Android/Kotlin source (compose-app androidMain/commonMain/desktopMain/jvmShared, android-app), build configs (Cargo.toml, build.gradle.kts, settings.gradle.kts, gradle.properties), CI workflows, docs (architecture.md, design.md, performance.md), AGENTS.md, TODO.md, README.md.
- Verified actual source state (not relying on historical audit claims): Parchment v3.1.1, Rust 1.100, edition 2024, MIT license.
- Confirmed existing agent harness: AgentTurnRunner (MAX_TOOL_ROUNDS=64), HarnessExecutor (21 tools, MAX_PARALLEL_SUBAGENTS=4), ToolApprovalBroker, AgentPermissionGate, WorkspacePathGuard, McpRegistry, PublicWebTransport, SubagentDelegate.
- Confirmed storage: SQLite WAL + FTS5, SCHEMA_VERSION=1, non-destructive v2 migrator, archive codec.
- Confirmed platforms: Windows GPUI desktop, Android Compose + JNI (slim 1.85 MB arm64-v8a .so), `io.parchment.ai` packageId.
- Confirmed tests/CI: 68 workspace + 2 slim android-jni Rust tests, 584 Kotlin tests + UiGalleryTest, cargo xtask + GitHub Actions.
- Identified missing autonomous coding capabilities (task lifecycle, planning, acceptance criteria, checkpoints, verification, recovery, artifacts, Axiom Coding UI).
- Created .memory/ knowledge base (15 documents).
- Phase 0 complete. No implementation begun (per discovery-phase mandate).

## 2026-11-20 (research + plan update session)
- Web research of current (2026) agent/coding practice, reading primary sources: MCP 2026-07-28 spec + changelog, Agent Skills specification, A2A v1.0 spec, Claude Code subagents docs, Codex CLI sandbox/TDD/hook practice, Kiro code-intelligence model, 2026 context-engineering guidance, 2026 agent-security checklist.
- Inspected `kandada/aacode-rs` (GPL-3.0) and `kandada/fastshell` (Apache-2.0) at the user's suggestion.
- Key findings applied to the plan:
  - MCP 2026-07-28 is stateless (no sessions, no initialize, `server/discover`, `subscriptions/listen`); current code already tolerates it as fallback, so no immediate change — recorded as a watch item.
  - TDD (Red-Green-Refactor) is the dominant 2026 autonomous-coding pattern → new Decision 9, `SubtaskKind`, Phase 4b.
  - Hook-based verification gates (PostToolUse lint, Stop on failing tests) → Decision 10, Phase 4b.
  - Context engineering (stable cached prefix, quantized compaction, tool-result clearing, subagent isolation) → Decision 11, Phase 7 tasks.
  - Code intelligence: tree-sitter first, LSP optional → Decision 12, new `parchment-intel`, Phase 5b.
  - Sandbox-is-a-tool with narrowest-mode defaults → Decision 13.
  - A2A monitored, not adopted → Decision 14.
  - aacode-rs: ideas only, no code (GPL) → Decision 6 expanded with an explicit borrow list.
  - fastshell: Apache-2.0, adopted only as an optional feature-gated Android backend with permission bridging → Decision 6b, Phase 9 tasks T-904..T-907, risks R-5/R-6.
  - Tool profiles + discovery meta-tool (from aacode-rs idea) → Decision 15, with an explicit no-bypass constraint.
- Updated: ARCHITECTURE_DECISIONS (9 new decisions), CODING_ENGINE_DESIGN (TDD loop, hooks, context, intel, borrowed ideas), SECURITY_AND_PERFORMANCE (2026 threat model, MCP/skills hardening, sandbox defaults, TDD testing), ROADMAP (Phase 4b, 5b, fastshell in Phase 9), TASKS (T-015/T-016, Phase 4b, 5b, 9 additions, R-5/R-6), RISKS_AND_BLOCKERS (R-5, R-6).
- No source code modified; still discovery/planning only.

## Next steps
- Begin Phase 1: rebrand user-visible strings/assets; verify no regression.
- Validate R-4 (rebrand preserves packageId/JNI/storage IDs) before any rename.
