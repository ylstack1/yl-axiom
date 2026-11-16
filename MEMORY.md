# Parchment Memory

Updated 2026-11-16. Stable contracts only; read the section you need. AGENTS.md owns workflow, TODO progress.

## Standing decisions

- One build and test run per batch; finish every open TODO, then commit and push once (user, 2026-11-11). After a flaky test failure, rerun only that test. Never force-push, overwrite unrelated work or claim unverified completion.
- No automatic local Git snapshots between checkpoints (user, 2026-11-10); don't re-propose them.
- Scripted phone taps first confirm Parchment is in front.
- Never optimize by losing history or instructions, weakening permissions, altering opaque signatures or silently enabling server storage.

## Project and build

- scripts/verify.py is the only entry point: saved JAVA_HOME/ANDROID_HOME (skips stale inherited JDK 8), version 2.1.0 (CI: VERSION_LINE/VERSION_BASE_RUN), two workers, config cache except final; logs in .parchment-local/verification/.
- Keep org.gradle.daemon=false (memory bloat); Gradle heap 2560 MB.
- PowerShell scripts stay ASCII.
- Never copy builds into %LOCALAPPDATA%/Programs/Parchment; only CI (-PofficialRelease=true) enables OTA. Dev runs use ignored parchment-portable-data.
- AGP 9: composeApp owns code/manifest/res; androidApp builds the APK (id io.parchment.ai, signing, R8, fonts). Keep io.parchment.ai class names (Accessibility binding).
- Android: keep big suspend bodies split into local suspend functions (cancelling a 565-register coroutine aborted).
- Android signing is private-env-only: dot-source scripts/load-private-signer.ps1; keep key 95d9804cfd9c80313440ed97619100c9e9f741c27943f61700fa802d0dbe4574 (legacy rejected, docs/RELEASE_SIGNING.md); blank key password uses store.
- Packaged desktop uses AppCDS: jlink base archive + `$APPDIR/parchment.jsa`, rebuilt after updates.

## Protocols and harness

- AiProviderEngine holds shared state/streamGeneration; each protocol and concern lives in a `network/` extension file.
- Big three: GEMINI_INTERACTIONS, OPENAI_RESPONSES, ANTHROPIC_MESSAGES. CUSTOM needs an explicit dialect and verbatim URL, sends only its declared wire, never falls back; unresolved Custom blocks Send.
- useStatefulChaining and storeServerSide default to false; storage, chaining and background execution are separate (Gemini background needs compatible storage). keepsProviderState drives `store`, polling and cancel.
- Chains require matching protocol, non-blank normalized model and stamped dialect. Keep chronological call/result batches and closing prose; ToolCallRecord.startsToolStep separates steps.
- Stamp turns via resolveWireProtocol/resolveWireDialect (not Custom base). Native reasoning and reasoning_content replay only for matching dialect/model (else handoff shifts step text). ReplayToolCallIds owns tool IDs.
- Reasoning signatures are opaque: preserve verbatim. Cross-provider handoff keeps 8000 chars (head + tail); user/system/continuity instructions stay uncapped.
- Keep request beginnings stable: stale tool output compacts in batches of 8; keyword lore goes in the late note, not the system prompt.
- Discover models and capabilities from live metadata/registries; no hardcoded models or branches. ModelsDevClient (`RegistryCatalogClient`) runs single-flight Dispatchers.IO sync (`2.5 s` modality wait never cancels download), fills gaps via `models.dev`, and backs off `60 s` on empty failures unless bypassed manually.
- Reasoning HTTP 400 retries step `max -> xhigh -> high` or `minimal -> low` and strip Anthropic adaptive `display` before abandoning reasoning; Anthropic tool replay preserves signed thinking blocks without text.
- AgentPermissionResolver owns defaults and overrides: null inherits; explicit Off, empty path or false revoke. New chats inherit; imports disable local tools, paths, web and Shell.
- Shared dispatch covers disabled, unknown, unadvertised, read-only and parallel paths. Raw provider options can't grant tools. Provider-managed code needs approval mode Never.
- ToolApprovalBroker is run/chat scoped: exact previews, one-use permits, denial suppression, synchronous invalidation, memory-only grants.
- Shell needs READ_WRITE plus explicit FULL_ACCESS and isn't OS-confined; web-tool controls don't limit its network. Cancel the owning worker, never kill globally.
- WorkspacePathGuard checks real ancestors and rejects escaping links and junctions. PublicWebTransport binds to validated public DNS addresses and checks each redirect; provider transport is separate.
- SSE closing without finish or `[DONE]` is interrupted. AcceptedJobException polls with bounded transient retries, never resubmits; non-retryable 4xx/policy SSE/JSON errors throw NoRetryException.
- Utility requests mask inherited tool context, disable tools and goal loops, and propagate cancellation.
- computer_use is off by default; each action needs approval (no run-wide grants), pre/post settle, Android off-screen overlay. Keep screenshots out of state (resend newest only); claim permits on permit.scope.
- MCP tools are `mcp__<server>__<tool>` and need approval like file changes; imports drop local commands and disable the rest. Spec 2025-11-25 handshake, fallback 2026-07-28.

## Runs and timing

- Process state keeps chats, jobs, queues and approvals across UI recreation. New runs await draining workers and refresh saved history.
- RunActivity/RunLeaseRegistry track elapsed time and owners: acquire before replacing/queueing, close owned lease only, skip approval waits, subtract tool unions once; Android foreground service stays active while waiting with 15 s heartbeats.
- Durations use Android uptime and desktop nanoTime, skipping unseen gaps over 5 s.
- Windows power helper: synchronized owners, validated heartbeat, independent restart. Sleep, Modern Standby and battery policy can still suspend runs.
- Device: OnePlus PLZ110 (Android 17/API 37) can cut the network ~2 s after the app leaves the screen; the stall watch retries on return.

## Usage and cost

- Records use wide nullable counts, owner/run/purpose identities, cumulative coalescing and TTL pricing. Failed requests stay unknown. Session totals count each owned request once, excluding copied fork charges.
- Model snapshots keep explicit free (0) and unknown (blank) prices, exact units and tiers. Reported gateway totals replace estimates. Missing per-image/container/video fees and BYOK billing stay marked partial.

## Persistence

- Keep safe legacy migration and portable relative export paths (layout: ARCHITECTURE.md).
- ChatSession.isCompactStub is transient: hydrate pending full payload before disk/gen/export; merge background loads without overwriting edits or deletions.
- Stub message ids stay unique (the chat list keys on them). Pinned messages keep their real id and a preview in stubs, so the Pinned list never loads chats. `pinned` is saved only when true.
- appDiskWriter keeps the latest dirty revision per chat, settings and deletion, clears only matching successes and retries failures with backoff.
- AtomicFileStorage/SnapshotFileStorage stage, sync and replace atomically (never truncate); keep backups and failed migrations; reject dot IDs/escapes/collisions; close NIO readers before decode.
- Strip attachment base64 only when the local file is readable and nonempty.
- Tests use owned temp dirs and clean up. `todo_write` writes per-chat `chats/<id>/TODO.md` (plus turn `goalTracker`); `agent_memory` uses on-demand global `memory/AGENT.md` (never project files).

## Imports, exports and updates

- Imports are untrusted. withImportedPermissions reduces media references to `chats/<id>/media/<name>` or `media/<name>`, drops network font paths and keeps this device's OTA repository.
- ChatArchiveExtractor is atomic (validate, stage, decode, commit new files only); importChatFile detects truncated streams; chat JSON stays in memory (256 MiB max).
- ChatArchiveWriter streams one chat at a time. Desktop exports replace only after a complete write; Android removes a partial document on failure.
- Links from model output or imports open only through openWebLink (http/https). Desktop openOrPlayMedia refuses executables.
- OTA verifies size + SHA-256 (Windows GitHub digest; Android APK sig) via OtaStagedFile; blocks install while runs are active or queued.
- Limits: attachments 50 MiB (pick/drop/paste), inline text 8 MiB, card/settings picks 256 MiB, desktop images 100 MP/128 MiB (downscaled past 2560 px). Shell output keeps head+tail. workspaceRead caps lines at 4k chars, 10k lines, 250k chars: never rewrite from it.
- Android clipboard images use FileProvider content URIs (ClipData.newUri).

## UI and performance

- designDefaultsApplied applies DESIGN.md display defaults once (built-in high reasoning moves to none). Windows keeps native caption via WindowsChrome, titled with the open chat.
- Foundation primitives and KissVectorIcon/ModelLabSvgIcon; no raw OS emoji. 14sp prose/code baseline; vertical choices and wrapped rows, no horizontal option scrollbars.
- Shared controls expose one labelled target with state and 48dp minimum. Field labels merge into input semantics (contentDescription would replace reader values).
- Palette keeps muted, danger and success ink at 4.5:1 on all surfaces; filled controls use onFillInk.
- Tabs: MRU cap 12, keep the active tab and settings. Notices wrap, dismiss by a 48dp control and keep their timeout.
- Streamed Markdown: a cached split needs text after it, or an open fence breaks. Model text renderers keep depth and size bounds; past one they show plain text.
- Raw wire previews stay at 4096 chars or less; streaming renders only the last 12 interleaved steps. Hydrate active and running chats; dehydrate inactive ones.
- Optimize only with measurements or equivalence checks.
