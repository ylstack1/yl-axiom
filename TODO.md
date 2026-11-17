# Parchment TODO

Updated 2026-11-17. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-17 performance, native picker, UI consistency & CI batch: JDK 25 Generational Shenandoah + Compact Headers + build-time Leyden `parchment.aot`, `KissMotionNode` (`Modifier.Node`, 0 hover/press recompositions), `stability_config.conf` + `kotlinx-collections-immutable` 0.5.2, R8 Full Mode + `profileinstaller` 1.4.1, native Win32 COM `IFileOpenDialog`/`IFileSaveDialog` multi-file/save pickers, `ControlHeight` UI alignment across Settings/Providers/chat, and CI caching/sharding across all 8 workflows.
- **Last verified:** 2026-11-17, `verify.py final` (558 tests + 37 UI gallery captures, signed APK, release package & service-loader checks), `verify.py perf` (`desktop_title_ms=1929`, first launch `1916`, `desktop_idle_private_mb=240.1`, `apk_mb=4.4`, `desktop_cds_archives_ok=True`).
- **Next action:** User checks 17.1 on the phone after the release.

## Open tasks

### For the user

- [ ] **13.6 Screen readers:** go through the app with NVDA on Windows and TalkBack on Android (chat, composer, sidebar, settings, Ctrl+K palette) and report anything unlabeled, read in the wrong order, or unreachable.
- [ ] **17.1 Android check:** after the next release, check edge-to-edge system bars in light and dark themes, Ctrl+G on a hardware keyboard, and cold start (`verify.py perf --android-install`). Also: a reply cut off by swiping the app away comes back from its last checkpoint, and Retry now ends a wait (airplane mode during a reply).

### Waiting on a measurement

- [ ] **17.3 Composer TextFieldState:** move the composer to `TextFieldState` in a leaf host. Measured 2026-11-16: keystroke frame p50 6.1 ms, p95 7.6 ms (`KeystrokeFrameProbeTest`); do it when that probe's p95 nears 12 ms or typing lag is reported, and test Android IME and rich-content paste on a phone.

## Known gaps

Not planned; open a task first. No JDK AOT cache (16.6, dropped 2026-11): AppCDS already meets the start budget, and a cache built in CI may not match installed paths. No Android startup profile without a device-generated one (single DEX; PERFORMANCE.md). No `:core:*` module split: `model` is a clean leaf but 2.4% of the code and `protocol` depends on `harness` (2026-11-16). No Android long-press menus: a long press belongs to text selection. `final` runs without the configuration cache (signer validation is not cache-compatible). No CHARX archives; cards over 512 KB of data lose their original copy. No persona profiles. No full-duplex voice. Android MCP is HTTP-only. Shell is not sandboxed.

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
