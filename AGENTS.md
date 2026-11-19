# Parchment Rules & System Contracts

Rules bind every contributor and agent. Persist session state in the repo, never chat history.

## Workflow & Engineering Rigor

Core engineering philosophy: Less code means less to execute, allocate, and maintain.
- Write the smallest, most direct solution that achieves optimal time and space complexity.
- Avoid premature abstraction: no factory classes, redundant interfaces, wrapper functions, or speculative configurability.
- Prefer flat control flow, primitive data structures, and in-place operations over chained higher-order helpers when they reduce allocations.
- Use zero external libraries unless explicitly requested.
- Never write "code golf" one-liners that hurt runtime speed or readability; minimize moving parts, not character count.

1. **Start:** run `cargo xtask status` and do **Next action** (critical/high defects first). Maps: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [docs/DESIGN.md](docs/DESIGN.md), [docs/PERFORMANCE.md](docs/PERFORMANCE.md).
2. **Plan (`/plan`):** read, audit, verify external specs, and update `TODO.md` only (cold-executable handoff: manifests/baseline, subtraction choices, state invariants, hostile/UX edge cases, pre-mortem, phased batches with paths/metrics). Never edit code, other docs, or Git until approved.
3. **Rigor:** verify live official specs/registries; reconcile `Cargo.toml`, Gradle, CI, floors, budgets. Fix root structures (leaf state, non-blocking Frame 1, Rust/FFM/NDK/Foundation); zero stubs, one source of truth, atomic I/O, bounded retries, structured cancel. Follow [docs/DESIGN.md](docs/DESIGN.md) (keyboard-first, 48dp+ merged semantics, 100–200% scale, HiDPI, `>=4.5:1` contrast, reduced motion, hostile text/turn stress). Split cold start from hot paths; kill leaks/bloat; verify release binaries/R8/APK budgets with measurements.
4. **Batches:** state task, files, criteria, and check upfront. `git grep -n` all source sets before/after signature changes. Prove regressions fail before fixing. Stop and diagnose after 2 failed attempts.
5. **Verify:** run `cargo xtask affected` once per batch (`rust`, `ktlint`, `fmt`, `android-so`, `test <filter>`, `android`, `review`, `perf`, or `final` after `. .\scripts\load-private-signer.ps1`). Enable `.githooks/pre-commit` once via `cargo xtask install-hooks`. Never run Gradle directly/in parallel, clean builds, live network tests (use local HTTP/SSE fixtures), or weaken gates/budgets. Desktop `ImageComposeScene` tests run on `Dispatchers.Swing` + `Snapshot.sendApplyNotifications()`.
6. **Finish & release:** pass checks, update `TODO.md` (**Source**/**Last verified**/**Next action**; drop done items), sync docs, clean dead code/scratch files, confirm clean `git status` (no untracked files/secrets), report only checks run. Conventional Commits (`feat:`/`fix:`/`perf:` drive `release-please`/`CHANGELOG.md`); merge PRs via `gh pr merge --rebase`.

## Boundaries

- Ask only for destructive/public actions, missing permissions, or product ambiguity; else take the conservative option and state it.
- One build/test per batch; finish open TODOs then commit/push once (2026-11-11). Rerun only flaky tests.
- No auto Git snapshots (2026-11-10); never force-push, overwrite unrelated work, or claim unrun checks.
- Never lose history/instructions, weaken permissions, alter opaque signatures, or silently enable server storage.
- User tests Android on phone; fix quirks generically (no brand/model notices).
- Use edit tools, not patch scripts (announce large scripts/new gates first).
- Never overwrite `%LOCALAPPDATA%\Programs\Parchment` or kill global `Parchment.exe`. Keep secrets/keys/PII out of repo/logs/requests.
- Compose Foundation + `KissVectorIcon`/`ModelLabSvgIcon` only (no Material/raw emoji).

## Stable Contracts

Updated 2026-11-19. Never break without explicit approval.

### Build & platforms

- `cargo xtask` (`crates/parchment-xtask`, zero-dep Rust) owns verification, static quality gates, CI planning, benchmarks, and Gradle (`2` workers, `org.gradle.daemon=false`, `2560 MB` heap, config cache except `final`, logs `.parchment-local/verification/`). Dev uses `parchment-portable-data`; OTA requires `-PofficialRelease=true`.
- AGP 9: `composeApp` owns code/manifest/res (`ktlintCheck` enforced); `androidApp` owns `io.parchment.ai`, R8, fonts, private signer (`95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574`, [docs/RELEASE_SIGNING.md](docs/RELEASE_SIGNING.md)). Keep `io.parchment.ai` names (Accessibility); split huge Android `suspend` functions (ART 565-reg cancel crash).
- v3 Rust (`Cargo.toml`, 8 crates, `rustfmt.toml`, `clippy -D warnings`): native GPUI Windows `.exe` (`parchment-desktop`, DWM/COM/keep-awake via `windows`), SQLite WAL+FTS5 core (`parchment-core`), slim Android `arm64-v8a` direct-JNI `.so` (`--no-default-features --features android-jni`, `0x4000` 16 KB ELF alignment, uncompressed `useLegacyPackaging=false`, `ParchmentNativeCore` kept in ProGuard), and `parchment-xtask`.

### Protocols & harness

- Dialects: `GEMINI_INTERACTIONS`, `OPENAI_RESPONSES`, `ANTHROPIC_MESSAGES`. `CUSTOM` requires explicit dialect + verbatim URL, never falls back, blocks Send if unresolved.
- `useStatefulChaining`/`storeServerSide` default `false` (Gemini background needs storage); `keepsProviderState` drives store/poll/cancel.
- Chains require matching protocol, normalized model, stamped dialect (`resolveWireProtocol`/`resolveWireDialect`), chronological call/result + closing prose (`startsToolStep`), `ReplayToolCallIds`.
- Signatures opaque verbatim; native reasoning replay only on matching dialect/model (else 8k head+tail handoff; instructions uncapped). Prompt-cache prefix: `tools` first, 16-step compaction, late lore.
- Live registry model discovery (`ModelsDevClient` single-flight `2.5 s` wait never cancels download; `models.dev` gap-fill; `60 s` failure backoff); no hardcoded models.
- HTTP 400 self-heal: `max->xhigh->high`, `minimal->low`, strip Anthropic adaptive `display` before dropping reasoning; keep signed thinking blocks.
- `AgentPermissionResolver`: `null` inherits; `Off`/empty/`false` revokes; imports revoke local/paths/web/shell. Shared dispatch enforces disabled/unknown/unadvertised/read-only/parallel; raw options can't grant tools; provider code-exec requires `Never`.
- `ToolApprovalBroker`: run/chat scoped, exact previews, one-use raw-arg permits, denial suppression, sync invalidation, memory-only. Parallel `agent_delegate` (max 4) relays status/usage + live spend check.
- Shell needs `READ_WRITE`+`FULL_ACCESS`, unsandboxed, cancels owning worker only. `WorkspacePathGuard` canonicalizes ancestors, blocks escaping links/junctions. `PublicWebTransport` pins public DNS IPs + validates redirects.
- SSE EOF without finish/`[DONE]` is interrupted. Retries obey HTTP status + hints (`1–300 s`), else backoff `60 s`; spend/billing/daily quota/`AcceptedJobException`/`NoRetryException` stop. Utility calls disable tools/goals.
- `computer_use`: off by default, per-action approval (no run-wide), settle, Android off-screen overlay, transient screenshots.
- MCP (`mcp__<server>__<tool>`): approval required; imports drop local cmds & disable rest; `2025-11-25`->`2026-07-28`; restart (`3`/`10 min`, same names, never resend failed call); renew lost HTTP session once.

### Runs, usage, persistence & UI

- Process state survives UI recreation; new runs await draining workers. `LiveTurns` holds stream text between `3.5 s` checkpoints (`withLiveTurns`); `RetryControl` supports `Retry now`.
- `RunActivity`/`RunLeaseRegistry`: acquire before replacing/queueing, close owned lease only, skip approval waits, Android `15 s` FGS heartbeat; uptime/`nanoTime` skips `>5 s` gaps (whole seconds); Windows keep-awake 1 thread (`5 s` retry); Android screen-off stall retries on return.
- Usage: nullable counts, owner/run/purpose IDs, cumulative coalescing, TTL pricing, unknown on error, deduplicated per owned request across forks; free `0` vs unknown blank; gateway totals override estimates.
- Persistence: non-destructive v2->SQLite WAL+FTS5 migration folding exact `(length, mtime)` `live.json`; transient `isCompactStub` hydration/merge; `compactStateJson` (`@EncodeDefault` on `profiles`/`assistants`/`providerStorageReset`/`storeServerSide`); `appDiskWriter` retries; `live.json` (`30 s` full save) + redacted `crash/last-crash.txt`; atomic stage/sync/replace (`AtomicFileStorage`/`SnapshotFileStorage`, Windows retry); strip base64 only when local file readable; `chat-index.json` (`CHAT_STUB_FORMAT`); `todo_write` -> `chats/<id>/TODO.md`, `agent_memory` -> `memory/AGENT.md`.
- Imports/exports/OTA: untrusted imports (`withImportedPermissions`, relative media, drop network fonts, keep OTA repo); atomic `ChatArchiveExtractor` / streaming `ChatArchiveWriter`; `openWebLink` (`http`/`https`), `openOrPlayMedia` blocks executables; `OtaStagedFile` verifies size+SHA-256/APK sig, blocks during active/queued runs.
- Limits: `50 MiB` attach, `8 MiB` inline text, `256 MiB` card/chat JSON, `100 MP`/`128 MiB`/`2560 px` desktop images, shell head+tail, `workspaceRead` `4k`/`10k`/`250k` chars.
- UI/perf: `designDefaultsApplied`, `14sp` baseline, vertical/wrapped options, `48dp+` merged semantics (no `contentDescription` on inputs), `>=4.5:1` contrast (`onFillInk`), MRU-12 tabs, wrapping notices (`48dp` dismiss), cached Markdown split + `@Immutable` `ProseBlock` + bounded renderers, `4096`-char wire preview, last 12 stream steps, dehydrate inactive chats, root never reads draft text, VSync pacer, measure first.
