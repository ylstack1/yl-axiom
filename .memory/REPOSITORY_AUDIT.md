# YL Axiom — Repository Audit

Every conclusion below is tied to an exact source path, type/function, and implementation status verified by reading the actual source.

## 1. Workspace structure & Cargo config
- `Cargo.toml` (root): 8 workspace crates, Rust 1.100.0, edition 2024, MIT license. `[workspace.lints.cargo]` denies unused dependencies. Release profile: LTO thin, panic=abort, strip symbols. `release-android` profile: opt-level z, LTO fat, codegen-units 1 for the slim arm64-v8a `.so`.
- Status: Verified complete and functional (cargo build --release -p parchment-desktop passes per TODO.md).

## 2. All Rust crates and responsibilities
See CURRENT_ARCHITECTURE.md. All 8 crates verified by reading source:
- `parchment-model`: pure domain models.
- `parchment-storage`: SQLite WAL + FTS5 + migrator + archive.
- `parchment-protocol`: wire protocols, client, SSE, registry, usage, signatures.
- `parchment-prose`: streaming text buffer, markdown, diff, highlight, latex, svg.
- `parchment-harness`: agent harness (21 tools, path guard, permissions, approval, MCP, SSRF, codecs, runner).
- `parchment-core`: facade + JNI bridge.
- `parchment-desktop`: Windows GPUI shell.
- `parchment-xtask`: dev/CI automation.

## 3. Existing agent runtime and execution loop
- `crates/parchment-harness/src/runner.rs`: `AgentTurnRunner` — multi-step streaming loop, `MAX_TOOL_ROUNDS = 64`, spend limit enforced before every tool round, parallel read-only + subagent batches (max 4), `RunActivity` monotonic timer.
- `crates/parchment-harness/src/tools.rs`: `HarnessExecutor` dispatches all 21 built-in tools + MCP, enforces approval broker + permission gate + permit claim.
- Status: Fully implemented, tested. This is the core agent loop to extend for Axiom Coding.

## 4. Tool definitions, dispatch and execution
- `crates/parchment-harness/src/permissions.rs`: `ToolCatalog` (21 built-in tools), `HarnessToolDefinition`, `AgentPermissionGate`, `ToolAccessScope`, `ToolEffect`.
- `crates/parchment-harness/src/tools.rs`: `HarnessExecutor` — single, parallel read-only, and parallel `agent_delegate` (max 4) paths. `MAX_SHELL_OUTPUT_HALF_BYTES = 16 KiB`. Shell output head/tail spilling.
- `crates/parchment-harness/src/approval.rs`: `ToolApprovalBroker` — run-scoped, one-use permits, exact previews, denial suppression, sync invalidation. `computer_use` never run-wide.
- Status: Fully implemented. Reusable unchanged for Axiom Coding.

## 5. Filesystem and shell implementation
- `crates/parchment-harness/src/path_guard.rs`: `WorkspacePathGuard` — canonicalize ancestor resolution, symlink/junction escape prevention, read caps, 16 MB edit cap, ripgrep `fs_search`, ignored dirs.
- `crates/parchment-harness/src/tools.rs`: `shell_exec` via `tokio::process::Command`, output head/tail spilling at 32 KiB total.
- `mobile/compose-app/src/androidMain/kotlin/io/parchment/ai/platform/AndroidOwnedShell.kt`: Android shell adapter.
- `mobile/compose-app/src/desktopMain/kotlin/io/parchment/ai/platform/DesktopOwnedShell.kt`: desktop shell adapter.
- Status: Implemented. Shell is unsandboxed (known gap per TODO.md).

## 6. Provider protocols and model configuration
- `crates/parchment-protocol/src/client.rs`: `ProtocolClient` — 5 dialects, HTTP 400 self-healing, retry/backoff, key rotation/failover.
- `crates/parchment-protocol/src/registry.rs`: live model catalog sync.
- `crates/parchment-model/src/provider.rs`: `ProviderProfile`, `ProviderProtocol` (Gemini/OpenAI/Anthropic/Custom), pricing.
- `crates/parchment-protocol/src/signatures.rs`: opaque reasoning signatures, effort downgrade ladder, 8 KiB handoff.
- Status: Fully implemented. No duplicate provider clients should be created.

## 7. Sessions, storage and database migrations
- `crates/parchment-storage/src/schema.rs`: `SCHEMA_VERSION = 1`, WAL pragmas, tables, FTS5.
- `crates/parchment-storage/src/migrate_v2.rs`: non-destructive v2 migrator.
- `crates/parchment-storage/src/archive.rs`: atomic archive codec.
- `crates/parchment-core/src/lib.rs`: `ParchmentCore` session CRUD (load/save/delete chat, settings, search).
- Status: Fully implemented. Do not break migration compatibility.

## 8. Permissions and approvals
- `crates/parchment-model/src/permissions.rs`: `AgentPermissionPolicy`, `ToolApprovalMode`, `ShellAccess`, `WorkspaceMode`.
- `crates/parchment-harness/src/permissions.rs`: `AgentPermissionGate`, `ToolCatalog`, `ToolAccessScope`, `ToolEffect`.
- `crates/parchment-harness/src/approval.rs`: `ToolApprovalBroker`, `ToolExecutionPermit`.
- `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/harness/AgentPermissionGate.kt`, `ToolApprovalBroker.kt`, `WorkspaceHarness.kt`: Kotlin-side wrappers.
- Status: Fully implemented. Single permission authority — do not duplicate. Any new backend (e.g. optional fastshell) must bridge into this broker, never grant independently.

## 9. MCP and skills
- `crates/parchment-harness/src/mcp.rs`: `McpClient`, `McpRegistry` — handshake 2025-11-25/2026-07-28, Streamable HTTP + stdio, `mcp__<server>__<tool>` naming, max 3 restarts/10 min.
- `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/network/McpClient.kt`, `McpRegistry.kt`, `McpSettings.kt`: Kotlin-side.
- Android MCP is HTTP-only (stdio not available on Android) — known gap.
- `crates/parchment-harness/src/codecs.rs`: `extract_md_skills_from_zip`, skill import.
- Status: Implemented. Reusable unchanged.

## 10. Subagent implementation
- `crates/parchment-harness/src/tools.rs`: `SubagentDelegate`, `MAX_PARALLEL_SUBAGENTS = 4`, parallel `agent_delegate` batches.
- `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/harness/Subagents.kt`: Kotlin-side.
- Status: Implemented. Reusable for Axiom Coding coordination.

## 11. Windows GPUI application
- `crates/parchment-desktop/`: full native Windows GPUI shell, multi-tab streaming, `ActiveRunState`, 3.5 s SQLite checkpoints, one-use `ToolApprovalBroker`, background model/OTA/title/summary channel.
- `crates/parchment-desktop/src/win32.rs`: native COM file dialogs, DWM.
- `mobile/compose-app/src/desktopMain/`: Compose desktop JVM (dev/hot-reload only; production Windows ships the Rust GPUI `.exe`).
- Status: Fully implemented. Reusable unchanged; extend window title + branding.

## 12. Android Kotlin/Compose application
- `mobile/compose-app/src/androidMain/`: `MainActivity`, `LongDecodeForegroundService` (specialUse FGS), `ParchmentAccessibilityService`, `OtaInstallReceiver`, `ParchmentFileProvider`, platform impls.
- `mobile/compose-app/src/commonMain/`: shared Compose UI.
- `mobile/android-app/`: AGP shell, `io.parchment.ai`, signing/R8, bundles `libparchment_core.so`.
- `crates/parchment-core/src/jni_android.rs`: zero-JNA JNI bridge (950+ functions).
- Status: Fully implemented. Reusable unchanged; extend navigation + Axiom Coding UI.

## 13. JNI interfaces and native build config
- `crates/parchment-core/Cargo.toml`: `cdylib` + `rlib`, `network` default feature, `android-jni` slim feature, `jni = 0.22`.
- `mobile/compose-app/build.gradle.kts`: KMP, Compose Multiplatform, Cargo NDK build of 16 KB-aligned `arm64-v8a` `.so`, R8 APK <= 7.50 MB.
- `mobile/android-app/build.gradle.kts`: AGP application, release signing, minify/shrink resources.
- Status: Implemented. Slim `.so` = 1.85 MB, 3 PT_LOAD, 16 KB aligned.

## 14. Feature flags and platform-specific compilation
- `parchment-core`: `default = ["network"]`; `android-jni = ["dep:jni"]`. `#[cfg(feature = "network")]` / `#[cfg(feature = "android-jni")]` gates throughout.
- `parchment-harness`: `#[cfg(feature = "network")]` on network-dependent modules (approval, mcp, permissions, runner, ssrf, tools).
- Status: Implemented. Clean network vs android-jni split already exists.

## 15. Existing tests, CI and build scripts
- Rust: `cargo test` (68 workspace + 2 slim android-jni tests), clippy -D warnings, unused-dep deny.
- Kotlin: 584 tests + `UiGalleryTest`, ktlintCheck.
- CI: build-installers, codeql, quality-signals, dependabot, release-please, scorecard, stale.
- `cargo xtask`: status, review, affected, rust, android-so, ktlint, fmt, test, final, perf, disk, prune, pre-commit, ci.
- Status: Implemented. Perf budgets verified on v3 post-audit.

## 16. README, TODO, architecture docs, conventions
- `README.md`: product description, features, architecture, integrations, license.
- `TODO.md`: current status, open tasks, known gaps, external contract sources.
- `docs/architecture.md`, `docs/design.md`, `docs/performance.md`, `docs/release-signing.md`.
- `AGENTS.md`: engineering rules, system contracts, stable contracts (the authoritative project rules).
- `CHANGELOG.md`, `.release-please-manifest.json`, `release-please-config.json`: automated releases via Conventional Commits.
- `LICENSE`: MIT.
- `clippy.toml`, `rustfmt.toml`, `.editorconfig`, `.gitattributes`, `.gitignore`.
- Status: Implemented. AGENTS.md rules bind all work.

## Reusable components (preserve unchanged)
All of the above are verified implemented and should be preserved. The shared agent harness (runner, tools, permissions, approval, MCP, path guard, SSRF) is the core foundation Axiom Coding extends.

## Genuinely missing for autonomous coding
- Coding task lifecycle and persistent task state (no `Task`/`TaskPlan` domain model).
- Planning and dependency-aware subtasks (no `TaskPlan` with dependencies).
- Acceptance criteria (no acceptance-criteria domain model).
- Iterative agent execution with checkpoints (no checkpoint/recovery).
- TDD phase modelling (no `SubtaskKind` Red/Green/Refactor) and hook gates (no PostToolUse/Stop hooks).
- Repository exploration / code intelligence beyond `fs_search`/`fs_read` (no tree-sitter index, no LSP).
- Structured file editing / patch management (fs_edit exists but no patch workflow).
- Build/test/lint execution loops (shell_exec exists but no build/test orchestration).
- Error classification and bounded recovery (error_policy exists but no coding-specific recovery).
- Final verification gate (no verification domain).
- Context management for large codebases / output artifacts beyond 64 KiB spill.
- Tool profiles to keep the prompt-cache prefix stable (only MCP schema deferral exists).
- Subagent coordination beyond max-4 parallel delegate (no team orchestration).
- A real Android shell story (current `AndroidOwnedShell` is limited; optional Apache-2.0 fastshell backend not yet integrated).
- Axiom Coding UI workspace (task overview, plan visibility, tool approval, terminal, diffs, verification results, pause/cancel/resume).
