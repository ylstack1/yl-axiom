# Parchment Rules & System Contracts

Rules bind every contributor and agent. Persist session state in the repo, never chat history.

## Engineering Rigor & Quality Control

Less code = less execution, allocation, maintenance: smallest direct solution, optimal complexity, zero premature abstraction or unrequested libs, flat control flow, primitive data, in-place ops.

- Systems invariants: parse untrusted input once at boundaries into invariant types; make invalid states unrepresentable (match/when); bound loops, recursion, queues, buffers, retries, and I/O; keep zero-copy hot planes off cold control planes.
- Consistency discipline: Red-Green-Refactor (local fixtures; no live network); stop & diagnose after 2 failures; adversarial self-review (hostile inputs + git diff); report only commands run.
- Anti-reward-hacking: never drop features, skip work, weaken tests/assertions, lower stress sizes, loosen budgets, or silence errors (let _ =, empty catch, unchecked unwrap()/!!).
- Hardcoded reference ban: no hardcoded model IDs/capabilities/pricing, device/brand notices, or volatile benchmark/spec literals in code, comments, or README.md; keep tables/benchmarks/profiles in docs/.
- Build & storage restraint (7.7 GB laptop): batch all edits before compiling; NEVER run repeated, chained, or mid-batch builds; use `cargo check -p <crate>` or prebuilt target/debug/parchment-xtask.exe review while iterating; run --release, Gradle, or cargo xtask final at most ONCE at task end (only when finished or requested). Keep caches bounded (cargo xtask disk/prune) and SQLite WAL bounded.

Workflow:
1. Start: cargo xtask status -> Next action. Maps: docs/architecture.md, docs/design.md, docs/performance.md.
2. Plan (/plan): audit + TODO.md only; no code/doc/Git edits until approved.
3. Batches: state task/files/criteria; git grep -n all source sets on signature changes; fix root causes (leaf state, non-blocking Frame 1, atomic I/O, structured cancel); follow docs/design.md (48dp+, 100-200% scale, >=4.5:1, reduced motion).
4. Verify (once per batch): finish all edits first, then run ONE narrowest cargo xtask check (review, affected, `test <filter>`, rust, ktlint, fmt, android-so, perf, disk/prune, or final via `. .\scripts\load-private-signer.ps1`). Never run raw/parallel Gradle, concurrent xtask, clean builds, live network tests, or repeated --release loops. ImageComposeScene tests use Dispatchers.Swing + Snapshot.sendApplyNotifications().
5. Finish: pass checks, sync TODO.md/docs, delete scratch/dead code, clean git status. Conventional Commits; rebase-merge PRs.

## Cross-Platform Parity (Kotlin Mobile + Rust Desktop)

- Kotlin Mobile: mobile/compose-app/src/, mobile/android-app/src/
- Rust Desktop & Core: crates/parchment-{model,storage,protocol,prose,harness,core,desktop}/

On any feature/schema/UI/setting change: update both platforms (keep camelCase JSON/SQLite, defaults, withImportedPermissions, settings, UI copy/layout compatible), justify OS-only skips explicitly (Win32/GPUI vs Android FGS/Accessibility), and verify touched platforms once via cargo xtask.

## Boundaries

- Ask only for destructive/public actions, missing permissions, or ambiguity; else choose conservatively and state it.
- Strict build budget: at most ONE narrow check per completed batch (never chain or repeat heavy Rust/Gradle/--release builds); no auto Git snapshots; never force-push, overwrite unrelated work, or claim unrun checks.
- Never lose history/instructions, weaken permissions, alter opaque signatures, or silently enable server storage.
- Fix Android quirks generically (no brand/model notices). Use edit tools, not patch scripts.
- Never overwrite %LOCALAPPDATA%\Programs\Parchment or kill global Parchment.exe. Keep secrets/keys/PII out of repo/logs/requests.
- Compose Foundation / GPUI + KissVectorIcon/KissIcon/ModelLabSvgIcon only (no Material/raw emoji).
- Keep AGENTS.md and TODO.md plain: minimal markdown (#, ##, lists, rare backticks only for escaping; no bold, ###+, links, tables, or code fences) and no em dashes (enforced by cargo xtask review).

## Stable Contracts

Updated 2026-11-20. Never break without approval.

Build & platforms:
- cargo xtask (crates/parchment-xtask, std-only Rust) owns verification, static gates, storage QC (disk/prune), CI planning, benchmarks, Gradle (2 workers, no daemon, 2560 MB heap, config cache except final, logs .parchment-local/verification/). Dev uses parchment-portable-data; OTA requires -PofficialRelease=true.
- AGP 9: :composeApp (mobile/compose-app) owns code/manifest/res (ktlintCheck); :androidApp (mobile/android-app) owns io.parchment.ai, R8, fonts, private signer (95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574, docs/release-signing.md). Keep io.parchment.ai names (Accessibility); split huge Android suspend functions (ART 565-reg cancel crash).
- v3 Rust (Rust 1.100, Cargo.toml, 8 crates, rustfmt.toml, clippy -D warnings + workspace Cargo unused-dep lints + 3-day publish-age cooldown): native GPUI Windows .exe (parchment-desktop, DWM/COM/keep-awake via windows), SQLite WAL+FTS5 core (parchment-core), slim arm64-v8a JNI .so (--no-default-features --features android-jni, 0x4000 16 KB align, useLegacyPackaging=false, ParchmentNativeCore kept in ProGuard + startup-prof.txt), and parchment-xtask.

Protocols & harness:
- Dialects: GEMINI_INTERACTIONS, OPENAI_RESPONSES, ANTHROPIC_MESSAGES. CUSTOM requires explicit dialect + verbatim URL, never falls back, blocks Send if unresolved.
- useStatefulChaining/storeServerSide default false (Gemini background needs storage); keepsProviderState drives store/poll/cancel.
- Chains require matching protocol, normalized model, stamped dialect (resolveWireProtocol/resolveWireDialect), chronological call/result + closing prose (startsToolStep), ReplayToolCallIds.
- Signatures opaque verbatim; native reasoning replay only on matching dialect/model (else 8k head+tail handoff; instructions uncapped). Prompt-cache prefix: tools first, 16-step compaction, late lore.
- Live registry model discovery (ModelsDevClient single-flight 2.5 s wait never cancels download; models.dev gap-fill; 60 s failure backoff); no hardcoded models.
- HTTP 400 self-heal: max->xhigh->high, minimal->low, strip Anthropic adaptive display before dropping reasoning; keep signed thinking blocks.
- AgentPermissionResolver: null inherits; Off/empty/false revokes; imports revoke local/paths/web/shell. Shared dispatch enforces disabled/unknown/unadvertised/read-only/parallel; raw options can't grant tools; provider code-exec requires Never.
- ToolApprovalBroker: run/chat scoped, exact previews, one-use raw-arg permits, denial suppression, sync invalidation, memory-only. Parallel agent_delegate (max 4) relays status/usage + live spend check.
- Shell needs READ_WRITE+FULL_ACCESS, unsandboxed, cancels owning worker only. WorkspacePathGuard canonicalizes ancestors, blocks escaping links/junctions. PublicWebTransport pins public DNS IPs + validates redirects.
- SSE EOF without finish/[DONE] is interrupted. Retries obey HTTP status + hints (1-300 s), else backoff 60 s; spend/billing/daily quota/AcceptedJobException/NoRetryException stop. Utility calls disable tools/goals.
- computer_use: off by default, per-action approval (no run-wide), settle, Android off-screen overlay, transient screenshots.
- MCP (`mcp__<server>__<tool>`): approval required; imports drop local cmds & disable rest; 2025-11-25->2026-07-28; restart (3/10 min, same names, never resend failed call); renew lost HTTP session once.

Runs, usage, persistence & UI:
- Process state survives UI recreation; new runs await draining workers. LiveTurns holds stream text between 3.5 s checkpoints (withLiveTurns); RetryControl supports Retry now.
- RunActivity/RunLeaseRegistry: acquire before replacing/queueing, close owned lease only, skip approval waits, Android 15 s FGS heartbeat; uptime/nanoTime skips >5 s gaps (whole seconds); Windows keep-awake 1 thread (5 s retry); Android screen-off stall retries on return.
- Usage: nullable counts, owner/run/purpose IDs, cumulative coalescing, TTL pricing, unknown on error, deduplicated per owned request across forks; free 0 vs unknown blank; gateway totals override estimates.
- Persistence: non-destructive v2->SQLite WAL+FTS5 migration folding exact (length, mtime) live.json; transient isCompactStub hydration/merge; compactStateJson (@EncodeDefault on profiles/assistants/providerStorageReset/storeServerSide); appDiskWriter retries; live.json (30 s full save) + redacted crash/last-crash.txt; atomic stage/sync/replace (AtomicFileStorage/SnapshotFileStorage, Windows retry); strip base64 only when local file readable; chat-index.json (CHAT_STUB_FORMAT); todo_write -> `chats/<id>/TODO.md`, agent_memory -> memory/AGENT.md.
- Imports/exports/OTA: untrusted imports (withImportedPermissions, relative media, drop network fonts, keep OTA repo); atomic ChatArchiveExtractor / streaming ChatArchiveWriter; openWebLink (http/https), openOrPlayMedia blocks executables; OtaStagedFile verifies size+SHA-256/APK sig, blocks during active/queued runs.
- Limits: 50 MiB attach, 8 MiB inline text, 256 MiB card/chat JSON, 100 MP/128 MiB/2560 px desktop images, shell head+tail, workspaceRead 4k/10k/250k chars.
- UI/perf: designDefaultsApplied, 14sp baseline, vertical/wrapped options, 48dp+ merged semantics (no contentDescription on inputs), >=4.5:1 contrast (onFillInk), MRU-12 tabs, wrapping notices (48dp dismiss), cached Markdown split + @Immutable ProseBlock + bounded renderers, 4096-char wire preview, last 12 stream steps, dehydrate inactive chats, root never reads draft text, VSync pacer, measure first.
