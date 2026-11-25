# YL Axiom — Security, Performance, and Testing Strategy

## Security

### Workspace confinement
- `WorkspacePathGuard` (`crates/parchment-harness/src/path_guard.rs`): canonicalize ancestor resolution, symlink/junction escape prevention, recursive symlink skip, read caps (4000 chars/line, 10000 lines, 250000 total), 16 MB edit cap, ignored directories.
- Coding tasks must run within a declared workspace path; `AgentPermissionPolicy.workspace_path` governs. Fail-closed read-only mode when no workspace granted.
- New coding task storage uses the same guarded path model; no writes outside workspace.

### Command execution permissions
- `ShellAccess` (Disabled/FullAccess) in `AgentPermissionPolicy`. Default Disabled.
- `ToolEffect` (`permissions.rs`) determines requires_approval: `computer_use` always; file/state/web/external changes under AskForChanges.
- `ToolApprovalBroker`: run-scoped, one-use permits bound to raw args, `computer_use` never run-wide, sync invalidation.
- No unsandboxed shell on Android (`AndroidOwnedShell` limited); Android MCP HTTP-only (no stdio).

### Sandbox-is-a-tool (coding)
- Shell is started on demand, not per turn, at the narrowest sufficient mode: read-only default, workspace-write when edits are required, network-restricted unless web access is granted.
- Escalation is per task and recorded; a subagent cannot widen it.
- Android reports restricted capability instead of claiming parity.

### Symlink / path traversal
- `WorkspacePathGuard::can_traverse` rejects escapes. `resolve_local_media_candidate` verifies canonicalized path is under root. Archive importer rejects `..`, Windows reserved device names, trailing dots, case-insensitive media collisions.

### SSRF / network
- `PublicWebTransport` (`ssrf.rs`): public IP only, custom DNS resolver (no DNS rebinding TOCTOU), no auto-redirects, max 6 hops, rejects credentials/non-http(s)/scoped IPv6. Independent of provider HTTP transport.

### MCP client (existing) — revision watch
- `crates/parchment-harness/src/mcp.rs` implements handshake `2025-11-25` with fallback `2026-07-28`.
- The `2026-07-28` revision makes the core stateless: it removes protocol-level sessions and the `Mcp-Session-Id` header, removes the `initialize`/`notifications/initialized` handshake, adds a required `server/discover` RPC, replaces the HTTP GET endpoint and `resources/subscribe` with a single `subscriptions/listen` stream, and deprecates Roots, Sampling, Logging and HTTP+SSE.
- Current code already tolerates `2026-07-28` as a fallback, so no immediate change is required. Adopting `2026-07-28` as the primary revision (stateless requests, `server/discover` probe, `subscriptions/listen`) is deferred to a later phase and must be gated on real server behaviour, not on the spec alone.

### Agent Skills (existing) — hardening
- `SKILL.md` frontmatter (`name`, `description`, optional `license`, `compatibility`, `metadata`, `allowed-tools`) matches the current Agent Skills specification; keep the existing zip importer.
- Skills are untrusted input: a skill body must never be able to widen tool scope. `allowed-tools` is treated as a request, intersected with the task's `ToolAccessScope`, never as a grant.
- Progressive disclosure (metadata at startup, body on activation, resources on demand) is already the intended loading model; keep skill bodies bounded.

### A2A (watch list)
- The Agent2Agent protocol (Linux Foundation, v1.0) standardizes agent-to-agent delegation with Agent Cards, tasks and artifacts. It is not adopted now; internal coordination stays on `SubagentDelegate`. Revisit in Phase 8 if multi-agent teams become a requirement.

### 2026 threat model additions
- Treat the model as an untrusted decision-maker: authorization stays in deterministic Rust code (`AgentPermissionGate`, `ToolApprovalBroker`), never in the prompt.
- Prefer narrow tools over open-ended ones (`run_shell(command)`, `fetch_any_url`) for coding work; keep `WorkspacePathGuard` canonicalization on every path.
- Untrusted content (file contents, web pages, MCP results, skill bodies) is data, never instructions; coding prompts must state this and the harness must not widen scope from tool output.
- Log the full action chain (tool name, arguments hash, approval decision, exit code) for coding runs so a bad run is reconstructable.
- Test adversarially: hostile inputs, path escapes, symlink loops, prompt-injection payloads in repo files, and MCP/skill scope-escalation attempts.

### Secrets / API key storage
- API keys stored in `AppStateSnapshot.profiles` (encrypted-at-rest consideration for future; currently plaintext in settings JSON/SQLite — document as known limitation).
- No hardcoded keys in code (AGENTS.md hardcoded reference ban).
- Coding runs must not print secrets: shell output head/tail spilling applies, and env vars are not echoed into artifacts.

### MCP server permissions
- `McpRegistry` names tools `mcp__<server>__<tool>`, handshake versioning, max 3 restarts/10 min, never resend discovery call. Subagent MCP calls inherit parent scope.

### Process isolation / resource limits
- Shell output head/tail spill at 32 KiB total (`MAX_SHELL_OUTPUT_HALF_BYTES = 16 KiB`).
- `MAX_TOOL_ROUNDS = 64`, spend limit per tool round, bounded retries (max 3 transient), max 6 HTTP 400 heals.
- `MAX_PARALLEL_SUBAGENTS = 4`.

### Cancellation propagation
- `AgentTurnRunner` abort, `RunActivity` monotonic timer, `ToolApprovalBroker.invalidate()` sync revocation. Coding checkpoints on cancel/pause.

### Android sandbox
- MANAGE_EXTERNAL_STORAGE, scoped storage, SAF where appropriate. No Termux/PRoot/embedded Linux. No unrestricted desktop subprocess.

### Data migration / corruption recovery
- Non-destructive v2 migrator (never deletes v2 files). Atomic stage→sync→replace for storage writes. `live.json` 30 s full save + redacted crash/last-crash.txt. Archive importer zip-bomb bounded.

### Do not weaken existing security
- All security contracts preserved for Assistant/Chat; Axiom Coding additive and subject to same boundaries.

## Performance

### Streaming
- Stream model output and agent events (`SseFramer`, `StreamingTextBuffer`, `RunProgressEvent`). No blocking native UI threads (desktop background channel; Android coroutines off main thread).

### Bounded concurrency
- Parallel read-only tool batches, parallel subagents max 4, bounded retries/backoff.

### Large outputs
- Artifacts for large outputs (disk spill), not unlimited prompts. `INLINE_TOOL_RESULT_LIMIT_BYTES = 64 KiB`.

### Android
- Optimize memory/CPU: slim `.so` (1.85 MB), R8 shrink, profileinstaller, low-RAM mode (smaller caches, ~30 fps).
- `LongDecodeForegroundService` specialUse FGS for background turns.

### Windows
- Preserve responsiveness: GPUI VSync pacer, measure-first, `uniform_list` virtualized sidebar, 0.27 s cold-start budget, 136.5 MB idle RAM budget.

### Binary size
- Avoid unnecessary native deps. Windows `.exe` <= 35 MB. Android APK <= 7.50 MB. `.so` 16 KB aligned, opt-level z, LTO fat, strip symbols.

## Testing strategy

### TDD as the verification backbone
- Every coding subtask with testable criteria runs Red-Green-Refactor; the suite is the external source of truth.
- Red turn writes tests only; Green turn cannot modify tests (hook-enforced); Refactor re-runs the full suite.
- A turn may not end while acceptance tests fail (Stop hook).

### Rust
- `cargo test` per crate (model, storage, protocol, prose, harness, coding, intel, core, desktop, xtask).
- `clippy -D warnings` + `cargo unused-dep lints deny`.
- Existing: 68 workspace + 2 slim android-jni tests. Preserve all.
- New coding engine tests: task state transitions, CRUD, acceptance criteria, verification, checkpoint/resume, error classification/recovery, hook gating, TDD phase transitions, context clearing policy — all local fixtures, no network.
- `cargo xtask rust`, `cargo xtask android-so` (slim .so build + 16 KB align check).

### Kotlin
- Existing: 584 tests + `UiGalleryTest`. Preserve all.
- `ktlintCheck`.
- New: Axiom Coding UI tests (task overview, plan visibility, tool approval, terminal, diffs, verification results, pause/cancel/resume).
- Compose desktop `desktopTest` + `UiGalleryTest` (phone/desktop sizes, light/dark, 200% text).

### Adversarial / security tests
- Path escape, symlink loop, junction escape, archive zip-bomb, prompt-injection payloads in repo files, MCP/skill scope-escalation attempts, shell output secret leakage.

### Cross-platform
- `cargo xtask affected` for changed Rust crates + Kotlin modules.
- `cargo xtask perf` for budgets (exe <= 35 MB, start <= 0.35 s, idle RAM <= 140 MB; APK <= 7.50 MB; .so <= 1.85 MB, 16 KB aligned).

### Android device validation (when device available)
- `cargo xtask perf --android-install`: install signed release APK, verify v2→SQLite WAL auto-migration, FTS5 search, edge-to-edge bars, cold start, swipe-kill checkpoint recovery, Retry now after airplane mode.
- Screen readers & IME: NVDA/TalkBack/CJK IME (chat, composer, sidebar, settings, Ctrl+K palette).
- Android MCP is HTTP-only — test graceful fallback for stdio-dependent flows.
- Android coding: tree-sitter intel available, LSP absent, restricted shell — verify graceful degradation.

### Acceptance criteria for "complete"
- Builds pass (`cargo build --release`, Gradle release).
- Tests pass (executed, not claimed).
- Perf/security budgets met.
- No regressions in Assistant/Chat.

## Status
Proposed. Not yet executed for new coding engine.
