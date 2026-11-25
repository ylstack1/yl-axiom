# YL Axiom — Testing Strategy

## Principles (from AGENTS.md)
- Red-Green-Refactor with local fixtures; no live network tests.
- Stop & diagnose after 2 failures; adversarial self-review (hostile inputs + git diff).
- Report only commands actually run. Never mark a test passed without executing it.
- No fake feature implementations; no weakening tests/assertions.

## Rust testing
- `cargo test` per crate (model, storage, protocol, prose, harness, coding, intel, core, desktop, xtask).
- `clippy -D warnings` + `cargo unused-dep lints deny`.
- Existing: 68 workspace + 2 slim android-jni tests. Preserve all.
- New coding engine tests: task state transitions, CRUD, acceptance criteria, verification, checkpoint/resume, error classification/recovery, hook gating, TDD phase transitions, tool-profile permission equivalence — all local fixtures, no network.
- `cargo xtask rust`, `cargo xtask android-so` (slim .so build + 16 KB align check).

## TDD as the verification backbone
- Every coding subtask with testable criteria runs Red-Green-Refactor; the suite is the external source of truth.
- Red turn writes tests only; Green turn cannot modify tests (hook-enforced); Refactor re-runs the full suite.
- A turn may not end while acceptance tests fail (Stop hook).

## Adversarial / security tests
- Path escape, symlink loop, junction escape, archive zip-bomb.
- Prompt-injection payloads embedded in repo files, web pages, MCP results, and skill bodies.
- MCP/skill scope-escalation attempts (including via the discovery meta-tool).
- Shell output secret leakage; env vars must not reach artifacts.
- Tool-profile equivalence: a hidden tool invoked through `call_tool` produces the same approval decision as a direct call.

## Kotlin testing
- Existing: 584 tests + `UiGalleryTest`. Preserve all.
- `ktlintCheck`.
- New: Axiom Coding UI tests (task overview, plan visibility, tool approval, terminal, diffs, verification results, pause/cancel/resume).
- Compose desktop `desktopTest` + `UiGalleryTest` (phone/desktop sizes, light/dark, 200% text).

## Cross-platform
- `cargo xtask affected` for changed Rust crates + Kotlin modules.
- `cargo xtask perf` for budgets (exe <= 35 MB, start <= 0.35 s, idle RAM <= 140 MB; APK <= 7.50 MB; .so <= 1.85 MB, 16 KB aligned).

## Android device validation (when device available)
- `cargo xtask perf --android-install`: install signed release APK, verify v2→SQLite WAL auto-migration, FTS5 search, edge-to-edge bars, cold start, swipe-kill checkpoint recovery, Retry now after airplane mode.
- Screen readers & IME: NVDA/TalkBack/CJK IME (chat, composer, sidebar, settings, Ctrl+K palette).
- Android MCP is HTTP-only — test graceful fallback for stdio-dependent flows.
- Android coding: tree-sitter intel available, LSP absent, restricted shell (or optional fastshell) — verify graceful degradation and that the `.so` budget holds.

## Acceptance criteria for "complete"
- Builds pass (`cargo build --release`, Gradle release).
- Tests pass (executed, not claimed).
- Perf/security budgets met.
- No regressions in Assistant/Chat.

## Status
Proposed. Not yet executed for new coding engine.
