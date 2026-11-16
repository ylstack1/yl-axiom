# Parchment Memory

Updated 2026-11-16. Stable contracts only; read the section you need. AGENTS.md owns workflow.

## Standing decisions

- One build and test run per batch; finish every open TODO, then commit and push once (user, 2026-11-11). After a flaky test failure, rerun only that test. Never force-push, overwrite unrelated work or claim unverified completion.
- No automatic local Git snapshots between checkpoints (user, 2026-11-10); don't re-propose them.
- Never optimize by losing history or instructions, weakening permissions, altering opaque signatures or silently enabling server storage.

## Project and build

- scripts/verify.py is the only entry point: saved JAVA_HOME/ANDROID_HOME, version 2.1.0 (CI: VERSION_LINE/VERSION_BASE_RUN), two workers, config cache except final; logs in .parchment-local/verification/.
- Keep org.gradle.daemon=false (memory bloat); Gradle heap 2560 MB.
- PowerShell scripts stay ASCII.
- Never copy builds into %LOCALAPPDATA%/Programs/Parchment; only CI (-PofficialRelease=true) enables OTA. Dev runs use parchment-portable-data.
- AGP 9: composeApp owns code/manifest/res; androidApp builds the APK (id io.parchment.ai, signing, R8, fonts). Keep io.parchment.ai class names (Accessibility binding).
- Android: keep big suspend bodies split into local suspend functions (cancelling a 565-register coroutine crashed).
- Android signing is private-env-only (scripts/load-private-signer.ps1, docs/RELEASE_SIGNING.md); keep key 95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574.
- Packaged desktop: Serial GC (2026-11-16: idle 269 -> 216 MB vs G1), compact headers + AppCDS (`classes_coh.jsa` after jlink, build fails without it; `parchment.jsa` from the installer's headless `--cds-warmup`, whose watchdog terminates without dumping). Win32 calls are FFM.

## Protocols and harness

- AiProviderEngine holds shared state/streamGeneration; each protocol and concern lives in a `network/` extension file.
- Big three: GEMINI_INTERACTIONS, OPENAI_RESPONSES, ANTHROPIC_MESSAGES. CUSTOM needs an explicit dialect and verbatim URL, sends only its declared wire, never falls back; unresolved Custom blocks Send.
- useStatefulChaining and storeServerSide default false; storage, chaining and background run are separate (Gemini background needs storage). keepsProviderState drives `store`, polling, cancel.
- Chains require matching protocol, non-blank normalized model and stamped dialect. Keep chronological call/result batches and closing prose; ToolCallRecord.startsToolStep separates steps.
- Stamp turns via resolveWireProtocol/resolveWireDialect. Native reasoning/reasoning_content replay only for matching dialect/model (else handoff shifts step text). ReplayToolCallIds owns tool IDs.
- Reasoning signatures are opaque (verbatim). Cross-provider handoff keeps 8000 chars (head+tail); user/system/continuity instructions stay uncapped.
- Keep request beginnings stable: send `tools` before `messages`/`input`/`contents`, compact stale tool output in batches of 16, and keep keyword lore in late note.
- Discover models and capabilities from live registries; no hardcoded models. ModelsDevClient runs single-flight IO sync (2.5 s modality wait never cancels download), fills gaps via models.dev, backs off 60 s on empty failures unless bypassed.
- Reasoning HTTP 400 retries `max -> xhigh -> high` or `minimal -> low` and strips Anthropic adaptive `display` before dropping reasoning; Anthropic replay keeps signed thinking blocks.
- AgentPermissionResolver owns defaults/overrides: null inherits; explicit Off, empty path or false revoke. New chats inherit; imports disable local tools, paths, web, Shell.
- Shared dispatch covers disabled, unknown, unadvertised, read-only and parallel paths. Raw provider options can't grant tools. Provider code execution needs approval Never.
- ToolApprovalBroker is run/chat scoped: exact previews, one-use permits, denial suppression, synchronous invalidation, memory-only grants. The shell step breakdown is display only; permits bind the raw arguments.
- Parallel agent_delegate batches (max 4) relay status/usage to the flow coroutine (onFlowCoroutine); helpers never emit and check the run's live spend (externalRunSpendUsd).
- Shell needs READ_WRITE plus explicit FULL_ACCESS and isn't OS-confined; web-tool controls don't limit its network. Cancel the owning worker only.
- WorkspacePathGuard checks real ancestors, rejects escaping links/junctions. PublicWebTransport binds validated public DNS addresses, checks each redirect; provider transport is separate.
- SSE closing without finish/`[DONE]` is interrupted. AcceptedJobException polls with bounded retries, never resubmits; non-retryable 4xx/policy errors throw NoRetryException.
- Utility requests mask inherited tool context, disable tools/goal loops, propagate cancellation.
- computer_use is off by default; each action needs approval (no run-wide grants), pre/post settle, Android off-screen overlay. Screenshots stay out of state (resend newest); claim permits on permit.scope.
- MCP tools are `mcp__<server>__<tool>` need approval; imports drop local commands and disable the rest. Spec 2025-11-25 handshake, fallback 2026-07-28.

## Runs and timing

- Process state keeps chats, jobs, queues and approvals across UI recreation. New runs await draining workers and refresh saved history.
- A streaming reply's text lives in LiveTurns (per variant) between 3.5 s checkpoints; readers of a streaming chat outside the reply use withLiveTurns (completeSessionForAction, export, pause flush).
- RunActivity/RunLeaseRegistry track time and owners: acquire before replacing/queueing, close owned lease only, skip approval waits, subtract tool unions once; Android foreground service heartbeats every 15 s.
- Durations use Android uptime and desktop nanoTime, skipping unseen gaps over 5 s.
- Windows keep-awake: one thread holds SetThreadExecutionState while owners > 0 (dies with it); failures retry after 5 s.
- Device: OnePlus PLZ110 (Android 17/API 37) can cut the network ~2 s after leaving the screen; the stall watch retries on return. Scripted taps first confirm Parchment is in front.

## Usage and cost

- Records use nullable counts, owner/run/purpose ids, cumulative coalescing, TTL pricing. Failed requests stay unknown. Totals count each owned request once, not copied fork charges.
- Model snapshots keep free (0) and unknown (blank) prices, units and tiers. Reported gateway totals replace estimates; missing media/container fees and BYOK stay partial.

## Persistence

- Keep safe legacy migration and portable relative export paths (layout: ARCHITECTURE.md).
- ChatSession.isCompactStub is transient: hydrate pending full payload before disk/gen/export; merge background loads without overwriting edits or deletions.
- Stub ids stay unique. Pinned messages keep real id and preview in stubs. `compactStateJson` omits defaults/nulls (`@EncodeDefault` preserves `profiles`, `assistants`, `providerStorageReset`, `storeServerSide`).
- appDiskWriter keeps the latest dirty revision per chat, settings and deletion, clears only matching successes and retries failures with backoff.
- AtomicFileStorage/SnapshotFileStorage stage, sync and replace atomically (never truncate); keep backups and failed migrations; reject dot IDs/escapes/collisions; close NIO readers before decode.
- Strip attachment base64 only when the local file is readable and nonempty.
- `chat-index.json` caches stubs by chat folder + chat.json (size, mtime) and CHAT_STUB_FORMAT (bump when stubs change); a cache only.
- Tests use owned temp dirs and clean up. `todo_write` writes `chats/<id>/TODO.md` (plus turn `goalTracker`); `agent_memory` uses global `memory/AGENT.md` (never project files).

## Imports, exports and updates

- Imports are untrusted: withImportedPermissions reduces media references to `chats/<id>/media/<name>` or `media/<name>`, drops network font paths, keeps this device's OTA repository.
- ChatArchiveExtractor is atomic (validate, stage, decode, commit new files); importChatFile detects truncation; chat JSON max 256 MiB in memory.
- ChatArchiveWriter streams one chat at a time; desktop replaces only after a complete write, Android removes a partial document.
- Model/import links open only via openWebLink (http/https); openOrPlayMedia refuses executables.
- OTA verifies size + SHA-256 (Windows GitHub digest; Android APK sig) via OtaStagedFile; blocks install while runs are active or queued.
- Limits: attachments 50 MiB, inline text 8 MiB, card/settings 256 MiB, desktop images 100 MP/128 MiB (downscaled past 2560 px). Shell output keeps head+tail. workspaceRead caps 4k chars/line, 10k lines, 250k chars: never rewrite from it.
- Android clipboard images use FileProvider content URIs (ClipData.newUri).

## UI and performance

- designDefaultsApplied applies DESIGN.md display defaults once (built-in high reasoning moves to none). Windows keeps native caption via WindowsChrome, titled with the open chat.
- Foundation primitives and KissVectorIcon/ModelLabSvgIcon; no raw OS emoji. 14sp prose/code baseline; vertical choices and wrapped rows, no horizontal option scrollbars.
- Shared controls expose one labelled target with state and 48dp minimum. Field labels merge into input semantics (contentDescription would replace reader values).
- Palette keeps muted, danger and success ink at 4.5:1 on all surfaces; filled controls use onFillInk.
- Tabs: MRU cap 12, keep the active tab and settings. Notices wrap, dismiss by a 48dp control and keep their timeout.
- Streamed Markdown: a cached split needs text after it, or an open fence breaks. Model text renderers keep depth and size bounds; past one they show plain text.
- Raw wire previews stay at 4096 chars or less; streaming renders the last 12 interleaved steps. Hydrate active and running chats, dehydrate the rest.
- App root never reads draft text; the stream pacer sleeps when caught up.
- Optimize only with measurements or equivalence checks.
