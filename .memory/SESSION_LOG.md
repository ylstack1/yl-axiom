# YL Axiom — Session Log

## 2026-11-25 (Phase 4 implementation session)

- Implemented Phase 4: TDD Red-Green-Refactor loop + hook gates in `crates/parchment-coding/src/lib.rs`:
  - `SubtaskPhase` enum (None/Red/Green/Refactor) for TDD phase tracking
  - `start_red_turn()` — WriteTests phase, implementation forbidden this turn
  - `start_green_turn()` — Implement phase, test files read-only (hook-enforced)
  - `start_refactor_turn()` — Refactor phase
  - `complete_green_turn()` — verifies all acceptance criteria; Stop hook blocks turn end if tests fail
  - `run_post_tool_use_hook()` — lint/format check after file mutation
  - `run_stop_hook()` — blocks turn end when acceptance tests fail
  - Unit tests: `tdd_red_green_refactor_phases` — Red→Green transitions, Stop hook pass/fail, PostToolUse hook pass/fail
- Committed and pushed commit `008e7dc` to `feat/axiom-coding-foundation`
- Created PR #1: "feat: Axiom Coding foundation — rebrand + coding task module" (targets main)
- CI status: Quality signals PASSED (rustfmt check passes after `add_artifact` signature fix). Preflight Gates (build-installers) fails only on `cargo fmt --all` — rustfmt not installed locally, cannot verify formatting.
- Known issue documented in `.harness/memory/rustfmt-ci-blocker.md`

## Known blockers (local environment)

- `rustfmt` NOT installed locally → `cargo fmt --all` cannot run → CI Preflight Gates formatting check fails
- `cargo check` fails on Android target due to missing `-lunwind` linker library — environment limitation, not code issue
- Fix: run `cargo fmt --all` in environment with rustfmt, commit, re-push; CI will pass

## Next: Phase 4c — VerificationEngine

- Build/test/lint acceptance criteria engine
- ErrorClassifier + RecoveryEngine for bounded recovery
- Completion based on verified results (not model claims)

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
