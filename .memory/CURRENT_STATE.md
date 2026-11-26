# YL Axiom — Current State

## Verified on main @ 401f7de (independent local runs)
- `cargo check -p parchment-coding` PASS
- `cargo test -p parchment-coding` PASS (31 passed, 0 failed)
- `cargo clippy -p parchment-coding --all-targets -- -D warnings` PASS (0 warnings)
- `cargo clippy --workspace --exclude parchment-desktop --all-targets -- -D warnings` PASS (verified at 77248f9)
- `cargo fmt --all -- --check` PASS
- CI `Build Android APK` PASS: slim arm64-v8a `.so` (1.86 MB, 16 KB aligned) + debug APK built and uploaded
- Not verified: `parchment-desktop` (GPUI/Win32 cannot build on the Linux VM), Android device runtime, Windows `.exe`

## Verified on main @ 77248f9 (independent local run)
- `cargo check -p parchment-coding` PASS
- `cargo test -p parchment-coding` PASS (22 passed, 0 failed)
- `cargo clippy --workspace --exclude parchment-desktop --all-targets -- -D warnings` PASS (0 warnings)
- `cargo fmt --all -- --check` PASS
- CI `Build Android APK` PASS: slim arm64-v8a `.so` (1.86 MB, 16 KB aligned) + debug APK built and uploaded
- Not verified: `parchment-desktop` (GPUI/Win32 cannot build on the Linux VM), Android device runtime, Windows `.exe`

## What currently works (verified from source)
- Full YL Axiom v3.1.1 codebase: 8 Rust crates + Kotlin Multiplatform/Compose Android app + Windows GPUI desktop.
- Assistant and Chat fully functional: provider protocols (Gemini/OpenAI/Anthropic/Custom), 21 built-in tools, MCP, subagents (max 4), ToolApprovalBroker, WorkspacePathGuard, SSRF transport, Skills, SQLite WAL + FTS5 storage, non-destructive v2 migration, archive import/export.
- Windows GPUI desktop shell (native, multi-tab, streaming, 3.5 s checkpoints, one-use approval broker).
- Android Compose app with shared Rust core via zero-JNA JNI (slim 1.85 MB arm64-v8a .so).
- Build/CI: cargo xtask, Gradle, GitHub Actions (build-installers, codeql, quality-signals, release-please).
- Tests: 68 workspace + 2 slim android-jni Rust tests; 584 Kotlin tests + UiGalleryTest.
- Perf budgets verified: 27.82 MB exe, 0.27 s cold start, 136.5 MB idle RAM, 1.85 MB .so, APK <= 7.50 MB target.

## What should be reused unchanged
- All 8 Rust crates (model, storage, protocol, prose, harness, core, desktop, xtask).
- AgentTurnRunner, HarnessExecutor, ToolApprovalBroker, AgentPermissionGate, WorkspacePathGuard, McpRegistry, SubagentDelegate, PublicWebTransport.
- Provider protocols, model registry, tokenizer, signatures, usage accounting.
- SQLite WAL + FTS5 storage, v2 migrator, archive codec.
- Windows GPUI shell, Android Compose UI, JNI bridge.
- Build system (cargo xtask), CI, tests.

## What needs extension
- Coding task lifecycle (Task, TaskPlan, Subtask with SubtaskKind, AcceptanceCriterion, Checkpoint, Artifact, CodingHook domain models).
- CodingTaskCoordinator around AgentTurnRunner.
- TDD Red-Green-Refactor loop + hook gates (PostToolUse lint, Stop on failing tests).
- VerificationEngine, ErrorClassifier, RecoveryEngine.
- SQLite schema extension (additive, non-destructive).
- Axiom Coding UI (Windows GPUI + Android Compose).
- Platform adapters for coding (file access, shell, process, capability discovery).
- `parchment-intel` (optional): tree-sitter code intelligence, LSP behind a trait.
- Tool profiles + discovery meta-tool (stable prompt-cache prefix).
- Optional `android-shell-fastshell` backend (Apache-2.0) for Android.
- Rebranding (user-visible strings/assets only).

## What is genuinely missing (for autonomous coding)
- Persistent coding task state and sessions independent from Chat.
- Planning and dependency-aware subtasks.
- Acceptance criteria domain model.
- Checkpoint/resume and recovery.
- Structured patch workflow + diff review.
- Build/test/lint execution + verification gate.
- Error classification and bounded recovery.
- Output artifacts and disk-spilled large outputs.
- Axiom Coding workspace UI.
- Code intelligence beyond `fs_search` (tree-sitter symbol/outline/AST; LSP optional).
- Tool profiles to keep the prompt-cache prefix stable.
- A real Android shell story beyond the current limited adapter (optional fastshell backend).
- Browser automation (deferred).

## Recommended architecture
- Shared core (parchment-model/storage/prose/harness/core) + optional `parchment-coding` module (Cargo feature-gated) + optional `parchment-intel`.
- Thin CodingTaskCoordinator around AgentTurnRunner; no duplication of agent loop.
- TDD Red-Green-Refactor default with hook-based verification gates; completion = passing suite.
- Context engineering: stable cached prefix, quantized compaction, tool-result clearing, subagent isolation.
- Tool profiles + discovery meta-tool for a stable cache prefix, same permission path.
- Same Rust agent brain on Windows (network feature) and Android (android-jni JNI).
- Platform adapters for filesystem/shell/process/permissions/dev-tools; capability discovery + graceful fallback.
- Optional Apache-2.0 fastshell backend on Android, bridged to the single permission authority.
- Rebrand user-visible only; preserve packageId, JNI symbols, storage IDs, license, history.

## Major conflicts and risks
- R-1: SQLite schema extension must be non-destructive (validate before Phase 3).
- R-2: Coordinator must not duplicate AgentTurnRunner (validate before Phase 4).
- R-3: Android coding fallback (no Termux/PRoot; stdio MCP unavailable) (validate before Phase 9).
- R-4: Rebrand must not change packageId/JNI symbols/storage IDs/migration keys (validate before Phase 1).
- R-5: fastshell optional, no second permission authority, `.so` budget held (validate before Phase 9).
- R-6: Tool profiles/meta-tool must not bypass permissions (validate before Phase 4b).
- Shell unsandboxed; Android MCP HTTP-only; secrets plaintext (known limitations).

## Windows/Android compatibility assessment
- Windows: full native capability (filesystem, process, PowerShell, Git, COM dialogs, LSP-capable). Compatible.
- Android: native Compose + JNI, limited shell (optionally fastshell), SAF, HTTP-only MCP, specialUse FGS, tree-sitter intel only. Graceful fallback required for Windows-only features. Compatible with capability discovery.
- Shared engine: same Rust logic on both; platform differences handled by adapters.

## Rebranding plan
- User-visible: app name → "YL Axiom", window title, Android label, icons, splash, sidebar, settings/About, onboarding, descriptions, empty states, errors, notifications, docs, README, installer.
- Preserved: crate names, `io.parchment.ai` packageId, `ParchmentNativeCore` JNI class, migration keys, SQLite schema, license, copyright, history, external integration IDs.
- Centralized strings in strings.rs / Strings.kt for easy future changes.

## First implementation milestone (Phase 1)
- Rebrand user-visible strings/assets; verify no functional regression via existing tests; preserve packageId, JNI symbols, storage IDs, license, history.

## Research basis (2026 sources consulted)
- MCP specification 2026-07-28 + changelog (stateless core, `server/discover`, `subscriptions/listen`).
- Agent Skills specification (SKILL.md frontmatter, progressive disclosure).
- A2A protocol v1.0 specification (watch list).
- Claude Code subagents docs (context isolation, tool/permission scoping).
- Codex CLI TDD + hooks + sandbox practice (Red-Green-Refactor, PostToolUse/Stop gates).
- Kiro code-intelligence model (tree-sitter built-in, LSP optional).
- 2026 context-engineering guidance (cached prefix, compaction, tool-result clearing, subagents).
- 2026 agent-security checklist (authorization outside the model, least privilege, adversarial testing).
- `kandada/aacode-rs` (GPL-3.0, ideas only) and `kandada/fastshell` (Apache-2.0, optional Android backend).

## Links to planning documents
- .memory/PROJECT_VISION.md
- .memory/PRODUCT_REQUIREMENTS.md
- .memory/CURRENT_ARCHITECTURE.md
- .memory/REPOSITORY_AUDIT.md
- .memory/ARCHITECTURE_DECISIONS.md
- .memory/NATIVE_PLATFORM_STRATEGY.md
- .memory/CODING_ENGINE_DESIGN.md
- .memory/SECURITY_AND_PERFORMANCE.md
- .memory/BRANDING_AND_REBRANDING.md
- .memory/ROADMAP.md
- .memory/TASKS.md
- .memory/TESTING_STRATEGY.md
- .memory/RISKS_AND_BLOCKERS.md
- .memory/CURRENT_STATE.md
- .memory/SESSION_LOG.md

## Change log
- 2026-11-20 (initial): full discovery, 15 planning documents, Phase 0 complete.
- 2026-11-20 (research update): added Decisions 9-15 (TDD, hooks, context engineering, code intelligence, sandbox-is-a-tool, A2A watch, tool profiles), Decision 6 expanded (aacode-rs ideas list), Decision 6b (fastshell optional Android backend), Phase 4b/5b, tasks T-015/T-016 + Phase 4b/5b/9 additions, risks R-5/R-6, 2026 threat model and research basis.
