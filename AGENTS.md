# Parchment Working Rules, Engineering Rigor & System Contracts

These rules bind every contributor, person or agent, in any tool. Nothing may depend on chat history: whatever the next session needs goes in the repository. Align with the codebase, domain, user goal, and live external reality before planning or coding; bring full thoroughness to every task; leave code, docs, and Git cleaner than you found them.

## Start & Mode Discipline

1. Run `python scripts/verify.py status` (shows pending files, last verified check, and `TODO.md` status).
2. Do **Next action**. Read other `TODO.md` or contract sections below only when the task needs them.
3. Follow `TODO.md`'s execution order; confirmed critical/high defects always go first.
4. Maps & design: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (modules/files), [docs/DESIGN.md](docs/DESIGN.md) (look/tokens/wording), [docs/PERFORMANCE.md](docs/PERFORMANCE.md) (speed/RAM/APK budgets).
5. **Plan vs. Execute discipline:** during `/plan` or plan-only requests, only read, search, audit, verify external contracts, and write/update `TODO.md`. Never edit workspace code, docs, or Git state until explicitly approved.
6. **Self-contained `TODO.md` plan handoff:** every roadmap must execute cold without chat history and include: (1) reconciled manifest, target runtimes, and live-verified baseline; (2) first-principles/subtraction decisions and ruled-out alternatives; (3) state ownership and split-brain invariants; (4) worst-case UX and hostile-environment stress cases; (5) pre-mortem failure modes and mitigations; (6) phased batches ordered by impact/effort with exact file paths, symbols, quantitative profiling targets, and post-execution doc/repo hygiene.

## Engineering Rigor & Craft

1. **Ground in live, cutting-edge reality:** treat internal knowledge as stale by default; verify official docs, registries, release notes, and primary specs for fast-moving AI/LLM APIs, UI toolkits, build plugins, JVM/compiler flags, and SDKs instead of guessing or copying undated workarounds. Reconcile local manifests (`gradle/libs.versions.toml`, `build.gradle.kts`, `composeApp/build.gradle.kts`, `androidApp/build.gradle.kts`, CI workflows), runtime floors (`jvmTarget`, `minSdk`), packaging interactions (Leyden AOT + compact headers + ProGuard, AGP R8 + startup profiles), and budgets. Research a changed external contract once and record its source in `TODO.md`.
2. **First principles, ideate, and subtract:** apply subtraction first — before adding abstractions, dependencies, overlays, toggles, or wrapper processes, check whether deleting complexity, collapsing steps, using platform-native primitives (FFM, NDK, Foundation), or reusing caches solves it better. Fix structures (isolate hot state from global snapshots; keep sync work off Frame 1), not symptoms. Surface assumptions, runtime floor changes, and non-reversible tradeoffs upfront. Define behavioral invariants, edge cases, and latency/RAM/size targets before coding.
3. **Zero laziness, clean state, hostile environments:** finish every path end-to-end with zero placeholders, stubs, or partial migrations. Make impossible states unrepresentable with one source of truth, clean derived projections, and explicit flush/merge invariants across cancel, pause, edit, export, and hydration. Assume networks drop mid-stream, processes stall, permissions revoke mid-run, writes fail mid-flush, and inputs arrive out of order — use atomic persistence, bounded retries, and structured cancellation.
4. **Distinctive, stress-tested UI/UX:** tailor hierarchy, density, typography, contrast, and rhythm to [docs/DESIGN.md](docs/DESIGN.md) while cutting clutter and option sprawl. Build immediate, keyboard-first, touch-ergonomic flows. Stress-test long unbroken strings, empty/zero states, huge histories or 100-step tool turns, CRLF/LF payloads, wide tables, missing media, and narrow viewports. Enforce semantics, focus order, 48dp+ touch targets, 100%–200% text scaling, HiDPI/multi-monitor coordinates, `>= 4.5:1` contrast, interruptible motion, and reduced-motion compliance.
5. **Speed, lean resources, longevity:** separate cold start (non-blocking Frame 1 / `onCreate`, off-UI preload of settings/active chat/fonts/engines, compact indices, Leyden AOT, startup profiles) from hot paths (leaf-state isolation for typing/scrolling/streaming, `Dispatchers.Default`/`IO` for regex/tokenization/scanning/compression/I/O, zero-copy buffers). Eliminate unbounded caches, idle frame-clock wakeups, leaked listeners/processes, and redundant child spawns. Guard binary size and runtime RAM. Keep edits surgical and pure/testable without drive-by refactors.
6. **Pre-mortem, profile, verify:** simulate failure on low-end devices and release builds first (AOT/CDS mismatch, ProGuard/R8 stripping, races, stale indices, APK bloat) and mitigate upfront. Audit for deprecated APIs, stale packages, hardcoded model IDs, and unverified third-party signatures. Pair unit/UI regressions with quantitative measurements (`scripts/verify.py perf`: cold start ms, idle private RAM/PSS, p95 frame time, binary/installer size).

## Work in Bounded Batches

1. Before editing, state briefly: defect/task, files, acceptance criteria, and check.
2. Search first (`git grep -n`), read only defining code and callers in **every** source set (including recursive/fallback paths), and re-search callers after signature changes before compiling.
3. Group edits sharing behavior and verification; review the whole diff once before building.
4. Add regressions only for real failure modes; prove a test can fail without the fix.
5. Don't grow a batch into a general audit; log unrelated findings in `TODO.md` and continue.
6. After two related failed attempts, stop and diagnose before trying again.

## Verify (`scripts/verify.py`)

- Run `python scripts/verify.py affected` after every batch (runs `scripts/quality.py` + `scripts/test_workflow_tools.py`, all desktop tests if code changed, Android compile when reachable, skipping verified paths). Build/test once per batch, not per edit. UI gallery renders only via `test UiGalleryTest` or `final`.
- Use `test <filters>` while iterating, `android` for Android compile checks, `review` for docs-only changes, and `final` (after `. .\scripts\load-private-signer.ps1`) for Gradle, R8, packaging, or signing changes.
- Failures print compile errors and failed tests; open logs in `.parchment-local/verification/` only if needed.
- Never run Gradle directly or in parallel, clean builds, or weaken gates/baselines/budgets to pass. If a gate is wrong, fix it in the same batch and say why.
- Use local HTTP/SSE fixtures, never live endpoints. Desktop `ImageComposeScene` UI tests run on the AWT EDT (`runBlocking(Dispatchers.Swing)`) and call `Snapshot.sendApplyNotifications()` after state writes before rendering.

## Finish a Batch, Tidy Docs & Release

- A batch finishes only when `python scripts/verify.py affected` passes and `TODO.md` updates **Source**, **Last verified**, and **Next action** (remove finished items; `CHANGELOG.md` and git keep history). Add new stable contracts under **Stable Contracts** below.
- During implementation batches (never plan-only turns), sync repository docs (`README.md`, `docs/ARCHITECTURE.md`, `docs/PERFORMANCE.md`, `AGENTS.md`, `TODO.md`), refine concise "why" comments, delete stale comments/dead code/unused imports/scratch files, and check `git status`/`git diff` for zero untracked artifacts or leaked secrets.
- Report briefly: what changed, what was verified, what wasn't. Never claim a check you did not run.
- **Commits & releases:** Conventional Commits (`feat:`, `fix:`, `perf:` advance `release-please` and `CHANGELOG.md`; `docs:`/`chore:`/`ci:`/`refactor:`/`test:` are hidden). Pushing to `main` does not release; merging the Release Please PR does. Merge feature PRs with `gh pr merge --rebase` (merge commits duplicate release-please entries).

## Token Economy & Boundaries

- Prefer `python scripts/verify.py status` over reading whole docs; read line ranges and batch reads. Don't reread just-edited files, restate plans, or paste logs. Reuse/extend `scripts/`.
- **Ask only when:** an action is destructive, public, or lacks permission, or code cannot settle a genuine user decision; otherwise take the conservative option and state it.
- **Standing boundaries:**
  - One build/test run per batch; finish every open TODO, then commit and push once (user, 2026-11-11). On flaky failure, rerun only that test.
  - No automatic local Git snapshots between checkpoints (user, 2026-11-10); don't re-propose them. Never force-push, overwrite unrelated work, or claim unverified completion.
  - Never optimize by losing history/instructions, weakening permissions, altering opaque signatures, or silently enabling server storage.
  - The user tests Android on their own phone; don't run phone checks. Fix device quirks generically with no brand/model notices in the app.
  - Use editing tools, not patch scripts (announce any large mechanical script or new gate/limit first).
  - Preserve user data, permissions, opaque signatures, explicit model choices, and the installed Windows app (`%LOCALAPPDATA%\Programs\Parchment` — never overwrite it or kill global `Parchment.exe`).
  - Use Compose Foundation and existing icons (`KissVectorIcon`/`ModelLabSvgIcon`). Check **Stable Contracts** below before changing storage, protocol, or platform code.
  - Keep secrets, keys, and personal data out of the repository, logs, and outside requests.

## Stable Contracts

Updated 2026-11-18. Stable contracts only; never break these without explicit approval.

### Project & build

- `scripts/verify.py` is the only entry point: saved `JAVA_HOME`/`ANDROID_HOME`, version `2.1.0` (CI: `VERSION_LINE`/`VERSION_BASE_RUN`), two workers, config cache except `final`; logs in `.parchment-local/verification/`.
- Keep `org.gradle.daemon=false` (memory bloat); Gradle heap `2560 MB`.
- Never copy builds into `%LOCALAPPDATA%/Programs/Parchment`; only CI (`-PofficialRelease=true`) enables OTA. Dev runs use `parchment-portable-data`.
- AGP 9: `composeApp` owns code/manifest/res; `androidApp` builds the APK (`io.parchment.ai`, signing, R8, fonts). Keep `io.parchment.ai` class names (Accessibility binding).
- Android: split large `suspend` bodies into local `suspend` functions (cancelling a 565-register coroutine crashed ART).
- Android signing is private-env-only (`scripts/load-private-signer.ps1`, [docs/RELEASE_SIGNING.md](docs/RELEASE_SIGNING.md)); keep key `95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574`.
- Packaged desktop: Generational Shenandoah (`5000 ms` uncommit, `ControlIntervalMin=50`/`Max=900`, `GuaranteedGCInterval=0`, no low `SoftMaxHeapSize`), C1-only JIT (`-XX:TieredStopAtLevel=1`), compact headers, `48 MiB` Skia GPU cache cap (`32 MiB` on `< 4 GiB` PCs), single `DEFLATED` `$APPDIR/parchment-all.jar` (merging `META-INF/services/*`), and build-time Leyden `$APPDIR/parchment.aot` (`--aot-verify`, `-Dskiko.library.path=$APPDIR`). Pin packaged file mtimes to even-second zero-ns UTC before `-XX:AOTCacheOutput` so Inno Setup (`TimeStampsInUTC=yes`) preserves exact NTFS FILETIMEs. Win32 Chrome, COM `IFileOpenDialog`/`IFileSaveDialog`, memory, and power use FFM.

### Protocols & harness

- Big three: `GEMINI_INTERACTIONS`, `OPENAI_RESPONSES`, `ANTHROPIC_MESSAGES`. `CUSTOM` requires explicit dialect and verbatim URL, sends only its declared wire, never falls back; unresolved `CUSTOM` blocks Send.
- `useStatefulChaining` and `storeServerSide` default `false`; storage, chaining, and background run are separate (Gemini background needs storage). `keepsProviderState` drives `store`, polling, cancel.
- Chains need matching protocol, non-blank normalized model, and stamped dialect. Keep chronological call/result batches and closing prose; `ToolCallRecord.startsToolStep` separates steps.
- Stamp turns via `resolveWireProtocol`/`resolveWireDialect`. Native reasoning/`reasoning_content` replay only for matching dialect/model (else handoff shifts step text). `ReplayToolCallIds` owns tool IDs.
- Reasoning signatures are opaque (verbatim). Cross-provider handoff keeps 8,000 chars (head+tail); user/system/continuity instructions stay uncapped.
- Keep prompt-cache prefixes stable: send `tools` before `messages`/`input`/`contents`, compact stale tool output in batches of 16, and keep keyword lore in a late note.
- Discover models/capabilities from live registries, never hardcoded. `ModelsDevClient`: single-flight IO sync (`2.5 s` modality wait never cancels download), `models.dev` fills gaps, `60 s` backoff after empty failures unless bypassed.
- Reasoning HTTP 400 retries `max -> xhigh -> high` or `minimal -> low` and strips Anthropic adaptive `display` before dropping reasoning; Anthropic replay keeps signed thinking blocks.
- `AgentPermissionResolver` owns defaults/overrides: `null` inherits; explicit `Off`, empty path, or `false` revokes. New chats inherit; imports disable local tools, paths, web, Shell.
- Shared dispatch covers disabled, unknown, unadvertised, read-only, and parallel paths. Raw provider options can't grant tools. Provider code execution needs approval `Never`.
- `ToolApprovalBroker` (run/chat scoped): exact previews, one-use permits, denial suppression, synchronous invalidation, memory-only grants; permits bind raw arguments (shell step breakdown is display only).
- Parallel `agent_delegate` batches (max 4) relay status/usage to the flow coroutine (`onFlowCoroutine`); helpers never emit and check live spend (`externalRunSpendUsd`).
- Shell needs `READ_WRITE` + explicit `FULL_ACCESS` and isn't OS-confined; web controls don't limit its network. Cancel only the owning worker.
- `WorkspacePathGuard` checks real ancestors and rejects escaping links/junctions. `PublicWebTransport` binds validated public DNS addresses and checks every redirect; provider transport is separate.
- SSE closing without finish/`[DONE]` is interrupted. Retries follow `ProviderHttpException` status and server hints (`1–300 s`), else back off to `60 s`; spend caps, billing, and daily quotas never retry. `AcceptedJobException` never resubmits; `NoRetryException` stops.
- Utility requests mask inherited tool context, disable tools/goal loops, and propagate cancellation.
- `computer_use` is off by default; every action needs approval (no run-wide grants), pre/post settle, and Android off-screen overlay. Screenshots stay out of state (resend newest); claim permits on `permit.scope`.
- MCP tools are `mcp__<server>__<tool>` and need approval; imports drop local commands and disable the rest. Spec `2025-11-25` handshake, fallback `2026-07-28`. Stopped servers restart (`3` starts/`10 min`, same names), never resending the call; lost HTTP sessions renew once.

### Runs, timing, usage & cost

- Process state keeps chats, jobs, queues, and approvals across UI recreation. New runs await draining workers and refresh saved history.
- Streaming text lives in `LiveTurns` (per variant) between `3.5 s` checkpoints; external readers use `withLiveTurns` (`completeSessionForAction`, export, pause flush). `RetryControl` shows retry waits; `Retry now` ends them.
- `RunActivity`/`RunLeaseRegistry` track time/owners: acquire before replacing/queueing, close owned lease only, skip approval waits, subtract tool unions once; Android foreground service heartbeats every `15 s`.
- Durations use Android uptime / desktop `nanoTime` (skipping gaps `> 5 s`) and display whole seconds (`30s`, `20m21s`). Windows keep-awake: one thread holds `SetThreadExecutionState` while `owners > 0`; failures retry after `5 s`.
- Device quirk: OnePlus PLZ110 (Android 17) can drop network `~2 s` after leaving screen; stall watch retries on return.
- Usage records use nullable counts, owner/run/purpose IDs, cumulative coalescing, TTL pricing; failed requests stay unknown; totals count each owned request once, not copied fork charges.
- Model snapshots keep free (`0`) and unknown (blank) prices, units, tiers. Reported gateway totals replace estimates; missing media/container fees and BYOK stay partial.

### Persistence, imports, exports & updates

- Keep safe legacy migration and portable relative export paths (layout: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)).
- `ChatSession.isCompactStub` is transient: hydrate full payload before disk/gen/export; merge background loads without overwriting edits or deletions.
- Stub IDs stay unique; pinned messages keep real ID and preview in stubs. `compactStateJson` omits defaults/nulls (`@EncodeDefault` preserves `profiles`, `assistants`, `providerStorageReset`, `storeServerSide`).
- `appDiskWriter` keeps latest dirty revision per chat/settings/deletion, clears only matching successes, retries failures with backoff.
- Checkpoints write `chats/<id>/live.json` (reply only, stamped with `chat.json`; full saves `30 s` apart delete it; loads fold a match). Uncaught errors flush (`3 s`) and leave redacted `crash/last-crash.txt`.
- `AtomicFileStorage`/`SnapshotFileStorage` stage, sync, replace atomically (never truncate), keep backups/failed migrations, reject dot IDs/escapes/collisions, close NIO readers before decode, and use sub-ms `LockSupport.parkNanos` retries on early Windows `AccessDeniedException`.
- Strip attachment base64 only when local file is readable and nonempty.
- `chat-index.json` caches stubs by folder + `chat.json` (size, mtime) and `CHAT_STUB_FORMAT` (bump when stubs change); cache only.
- Tests use owned temp dirs and clean up. `todo_write` writes `chats/<id>/TODO.md` (+ turn `goalTracker`); `agent_memory` uses global `memory/AGENT.md` (never project files).
- Imports are untrusted: `withImportedPermissions` restricts media refs to `chats/<id>/media/<name>` or `media/<name>`, drops network font paths, keeps local OTA repo.
- `ChatArchiveExtractor` is atomic (validate/stage/decode/commit); `importChatFile` detects truncation; chat JSON max `256 MiB` in RAM. `ChatArchiveWriter` streams one chat at a time (desktop replaces after full write; Android deletes partial output).
- Model/import links open only via `openWebLink` (`http`/`https`); `openOrPlayMedia` refuses executables.
- OTA verifies size + SHA-256 (Windows digest; Android APK sig) via `OtaStagedFile`; blocks install while runs are active/queued.
- Limits: attachments `50 MiB`, inline text `8 MiB`, card/settings `256 MiB`, desktop images `100 MP`/`128 MiB` (downscaled past `2560 px`). Shell keeps head+tail. `workspaceRead` caps `4k` chars/line, `10k` lines, `250k` chars (never rewrite from it).

### UI & performance

- `designDefaultsApplied` applies [docs/DESIGN.md](docs/DESIGN.md) defaults once (built-in high reasoning -> none).
- Foundation primitives + `KissVectorIcon`/`ModelLabSvgIcon`; no raw OS emoji. `14sp` prose/code baseline; vertical choices and wrapped rows, no horizontal option scrollbars.
- Shared controls expose one labelled `48dp+` target with state. Field labels merge into input semantics (`contentDescription` clobbers reader values).
- Palette keeps muted/danger/success ink `>= 4.5:1` on all surfaces; filled controls use `onFillInk`.
- Tabs: MRU cap 12, keep active tab and settings. Notices wrap, dismiss via `48dp` control, keep timeout.
- Streamed Markdown: cached split needs text after it or open fence breaks; `ProseBlock` is `@Immutable` with skippable `ProseBlockItem` scopes. Model text renderers enforce depth/size bounds, falling back to plain text.
- Raw wire previews cap at `4096` chars; streaming renders the last 12 interleaved steps. Hydrate active and running chats, dehydrate the rest.
- App root never reads draft text; the stream pacer runs hardware-VSync-locked while revealing and sleeps when caught up.
- Optimize only with measurements or equivalence checks.
