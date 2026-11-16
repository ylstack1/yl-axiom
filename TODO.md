# Parchment TODO

Updated 2026-11-16. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-16 optimization and harness batch: Ctrl+G prompt jump + Message Map prompts filter, parallel `agent_delegate` (max 4, relayed emits), quiet read/search grouping, shell step breakdown, invisible-char key hygiene, `chat-index.json` stubs, off-thread paste/geometry/token counts, idle pacer, CRLF Markdown, FFM instead of JNA/PowerShell, compact headers + `--cds-warmup`, explicit OkHttp, Android edge-to-edge + baseline profile, Ctrl+W/Ctrl+Tab.
- **Last verified:** 2026-11-16, `verify.py affected` (516 desktop tests + Android compile), `verify.py final` (signed APK + package + release checks), `verify.py perf` (`desktop_title_ms=1467`, first launch after update `4311` (was 17.0 s), `desktop_idle_private_mb=271.3`, `apk_mb=4.3`, `desktop_cds_archives_ok=True`).
- **Next action:** User checks 17.1 on the phone after the release; then 17.2 (live turn overlay) with a frame-time A/B.

## Open tasks

### For the user

- [ ] **13.6 Screen readers:** go through the app with NVDA on Windows and TalkBack on Android (chat, composer, sidebar, settings, Ctrl+K palette) and report anything unlabeled, read in the wrong order, or unreachable.
- [ ] **17.1 Android check:** after the next release, check edge-to-edge system bars in light and dark themes, Ctrl+G on a hardware keyboard, and cold start (`verify.py perf --android-install`).

### Deferred from the 2026-11-16 optimization plan

Each needs its own batch with measurements; the plan's reasoning is in the commit history.

- [ ] **17.2 Live turn overlay:** stream deltas through a per-turn overlay instead of patching `AppStateSnapshot` every frame; needs one commit helper for stop, edit, fork, export, compact and pause flushes.
- [ ] **17.3 Composer TextFieldState:** move the composer to `TextFieldState` in a leaf host so typing recomposes only the field (today: the viewport, no longer the app root).
- [ ] **17.4 Android startup profile:** add `startup-prof.txt` for R8 DEX layout once a device-generated profile exists; measure APK size.
- [ ] **17.5 Windows UX:** native folder dialog (IFileDialog via FFM) instead of `JFileChooser`; right-click turn and chat menus; Android long-press bubble actions.
- [ ] **17.6 Build tooling:** Compose Hot Reload and a `:core:*` module split; configuration cache for `final` (it runs without it today).

## Known gaps

Not planned; open a task first. No JDK AOT cache (16.6, dropped 2026-11): AppCDS already meets the start budget, and a cache built in CI may not match installed paths. No CHARX archives; cards over 512 KB of data lose their original copy. No persona profiles. No full-duplex voice. Android MCP is HTTP-only. Shell is not sandboxed.

## Sources

Reuse these; consult a changed external contract once.

- **OpenAI:** developers.openai.com/api/reference/resources/responses/methods/create; /api/reference/resources/chat/subresources/completions/methods/create; /api/docs/guides/reasoning; /api/docs/guides/prompt-caching.md; /api/docs/pricing; /codex/agent-approvals-security; /codex/sandboxing
- **Gemini:** aistudio.google.com/api-reference/interactions-api; /docs/interactions-overview; /docs/background-execution; /docs/thinking; ai.google.dev/api/generate-content; ai.google.dev/gemini-api/docs/generate-content/thought-signatures
- **Anthropic:** platform.claude.com/docs/en/api/messages; /build-with-claude/streaming; /build-with-claude/prompt-caching; /build-with-claude/extended-thinking; /build-with-claude/adaptive-thinking; /build-with-claude/effort; /build-with-claude/thinking-tool-workflows; /about-claude/pricing; /agents-and-tools/tool-use/web-search-tool
- **DeepSeek:** api-docs.deepseek.com/guides/thinking_mode
- **Rates:** models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway/models-and-providers; raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
- **Windows:** learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate; /nf-winbase-powersetrequest
- **Android:** developer.android.com/develop/background-work/services/fgs/service-types; /fgs/timeout; /training/monitoring-device-state/doze-standby; /tools/adb; /reference/android/system/Os; /develop/ui/views/notifications/notification-permission
- **JVM and processes:** docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/file/Path.html; /java/lang/Process.html; pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html
- **Formats:** modelcontextprotocol.io/specification/latest/server/tools; /2026-07-28/basic/lifecycle; github.com/kwaroran/character-card-spec-v3/blob/main/SPEC_V3.md
- **Security and GitHub:** cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html; docs.github.com/en/rest/releases/assets (asset digest is nullable, observed as `sha256:<hex>`)
- **Build:** kotlinlang.org/docs/gradle-compilation-and-caches.html; docs.gradle.org/8.13/userguide/configuration_cache.html; docs.oracle.com/en/java/javase/21/vm/class-data-sharing.html
