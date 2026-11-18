# Parchment TODO

Updated 2026-11-17. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-18 Batch 2A UI & GPU hotfix: removed 8 MiB `skiko.gpu.resourceCacheLimit` Direct3D 12 atlas starvation (fixing tiled glyph/pill corruption), moved `Retry now` inline to the right side of the status header bar, added global + per-fence `wrapCodeBlocks` (`PreferencesConfig` + `ProseRenderer` `"↩"` vector toggle), tightened `TopEditorialBar` desktop vertical rhythm (`84.dp -> ~60.dp`), added wide-desktop Split Diff toggle (`ParchmentDiffViewer.kt`), and skipped empty assistant stubs on Markdown export.
- **Last verified:** 2026-11-18, `verify.py final` (563 tests + 37 UI gallery captures, signed R8 APK, ProGuard/Leyden AOT package) + `verify.py perf` (`desktop_title_ms=979`, `desktop_idle_private_mb=200.0`, `apk_mb=4.4`, `desktop_cds_archives_ok=True`).
- **Next action:** Execute Batch 2B (`18.2`: `< 8 ms` streaming p95 incremental tail-block markdown layout & MRU hydrated tab/search UX).

## Open tasks

### Planned batches

- [ ] **18.2 Streaming `< 8 ms` p95 & instant tab/search UX:** memoize sealed prefix blocks in `ProseRenderer.kt` during live streaming, retain a bounded MRU cache of hydrated `ChatSession` tabs (`3` normal / `1` low-memory), and precompute lowercase sidebar search keys.
- [ ] **18.4 Settings keyboard shortcuts subpage:** add `SettingsCategory.SHORTCUTS` (`SettingsShortcutsPage.kt`), keycap overview, click-to-record rebinding (`PreferencesConfig.customShortcuts`) with conflict check & Reset, send-key toggle, Settings search indexing, and live `handleAppShortcut`/`CommandPalette` reflection.
- [ ] **18.5 Offline local import for Skills & MCP:** add local folder, `SKILL.md`, and `.zip` skill import (`MAX_SKILL_FILES = 64`, `1 MB`/file, `8 MB` total, traversal guard) plus `mcp.json`/`.mcp.json` and desktop command-picker MCP import (`enabled = false` default).
- [ ] **18.6 `/plan` & `/build` modes, stale-read guard, missing-folder recovery & deferred MCP schemas:** add `/plan` (`READ_ONLY`) and `/build` (`READ_WRITE`) switches with `ToolApprovalBroker.invalidate()`, run-scoped `fs_read` revision guard before `fs_edit`/`fs_write`, active-chat missing `workspacePath` recovery prompt, and on-demand MCP schema expansion when `> 6` tools are enabled.
- [ ] **18.7 Privacy retention badges, zero-dep `.docx` extraction & workspace `/` commands:** surface local-only server-storage/training-tier badges from catalog metadata, extract `.docx` `word/document.xml` via `ZipInputStream` (`0 KB` deps), and discover `.parchment/commands/*.md` + explicit Save-as-Skill.

### For the user

- [ ] **13.6 Screen readers:** go through the app with NVDA on Windows and TalkBack on Android (chat, composer, sidebar, settings, Ctrl+K palette) and report anything unlabeled, read in the wrong order, or unreachable.
- [ ] **17.1 Android check:** after the next release, check edge-to-edge system bars in light and dark themes, Ctrl+G on a hardware keyboard, and cold start (`verify.py perf --android-install`). Also: a reply cut off by swiping the app away comes back from its last checkpoint, and Retry now ends a wait (airplane mode during a reply).

### Waiting on a measurement

- [ ] **17.3 Composer TextFieldState:** move the composer to `TextFieldState` in a leaf host. Measured 2026-11-16: keystroke frame p50 6.1 ms, p95 7.6 ms (`KeystrokeFrameProbeTest`); do it when that probe's p95 nears 12 ms or typing lag is reported, and test Android IME and rich-content paste on a phone.

## Known gaps

Not planned; open a task first. No `:core:*` module split: `model` is a clean leaf but 2.4% of the code and `protocol` depends on `harness` (2026-11-16). No Android long-press menus: a long press belongs to text selection. `final` runs without the configuration cache (signer validation is not cache-compatible). No CHARX archives; cards over 512 KB of data lose their original copy. No persona profiles. No full-duplex voice. Android MCP is HTTP-only. Shell is not sandboxed.

## Sources

Reuse these; consult a changed external contract once.

- **OpenAI:** developers.openai.com/api/reference/resources/responses/methods/create; /api/reference/resources/chat/subresources/completions/methods/create; /api/docs/guides/reasoning; /api/docs/guides/prompt-caching.md; /api/docs/pricing; /api/docs/guides/rate-limits; /api/docs/guides/error-codes; /codex/agent-approvals-security; /codex/sandboxing
- **Gemini:** aistudio.google.com/api-reference/interactions-api; /docs/interactions-overview; /docs/background-execution; /docs/thinking; ai.google.dev/api/generate-content; ai.google.dev/gemini-api/docs/generate-content/thought-signatures; /gemini-api/docs/rate-limits (429 `RetryInfo.retryDelay`; a `QuotaFailure` quotaId with PerDay will not clear by retrying)
- **Anthropic:** platform.claude.com/docs/en/api/messages; /api/errors; /api/rate-limits (429 `retry-after`; `enforced_spend_limit_reached` has none); /build-with-claude/streaming; /build-with-claude/prompt-caching; /build-with-claude/extended-thinking; /build-with-claude/adaptive-thinking; /build-with-claude/effort; /build-with-claude/thinking-tool-workflows; /about-claude/pricing; /agents-and-tools/tool-use/web-search-tool
- **DeepSeek:** api-docs.deepseek.com/guides/thinking_mode
- **Rates:** models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway/models-and-providers; raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
- **Windows:** learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate; /nf-winbase-powersetrequest
- **Android:** developer.android.com/develop/background-work/services/fgs/service-types; /fgs/timeout; /training/monitoring-device-state/doze-standby; /tools/adb; /reference/android/system/Os; /develop/ui/views/notifications/notification-permission
- **JVM and processes:** docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/file/Path.html; /java/lang/Process.html; pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html
- **Formats:** modelcontextprotocol.io/specification/latest/server/tools; /2026-07-28/basic/lifecycle; /2025-06-18/basic/transports (404 on a session: start a new one; 2026-07-28 has no sessions); github.com/kwaroran/character-card-spec-v3/blob/main/SPEC_V3.md
- **Security and GitHub:** cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html; docs.github.com/en/rest/releases/assets (asset digest is nullable, observed as `sha256:<hex>`)
- **Build:** kotlinlang.org/docs/gradle-compilation-and-caches.html; docs.gradle.org/8.13/userguide/configuration_cache.html; docs.oracle.com/en/java/javase/21/vm/class-data-sharing.html; release-please README (several changes in one commit: typed lines at the bottom of the message); Ktor 3.6.1 `String.fromHttpToGmtDate()` (ktor-http, common)
