# Parchment TODO

Updated 2026-11-15. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-16: Protocol/reasoning alignment (preserve OpenAI `max` level, step down `max`/`xhigh`/`minimal` on HTTP 400 before dropping reasoning, strip Anthropic adaptive `display` and step effort before abandoning `adaptive`, preserve omitted-text signed tool replay blocks, MCP `2026-07-28` stateless + `_meta.clientInfo` fallback on init `400/404/405/422`/`-32600`), non-cancelling background model catalog sync (2.5s modality wait, incremental snapshot publishing, `models.dev` gap-filling, 60s empty-failure cooldown with manual bypass), and AWT EDT (`Dispatchers.Swing`) alignment across all 10 `ImageComposeScene` desktop UI suites (resolving the `RectList` concurrency race and removing the `UiGalleryTest` retry suppression).
- **Last verified:** 2026-11-16, `verify.py affected` (499 desktop tests + Android compile clean across code batches, 2m29s), `verify.py test` across all 10 UI suites + gallery (50 tests, 2m22s), and `verify.py review`.
- **Next action:** Re-test Android `computer_use` on device and check `13.6 Screen readers` when the user tests NVDA and TalkBack.

## Open tasks

### For the user

- [ ] **13.6 Screen readers:** go through the app with NVDA on Windows and TalkBack on Android (chat, composer, sidebar, settings, Ctrl+K palette) and report anything unlabeled, read in the wrong order, or unreachable.

## Known gaps

Not planned; open a task first. No JDK AOT cache (16.6, dropped 2026-11): AppCDS already meets the start budget, and a cache built in CI may not match installed paths. No CHARX archives; cards over 512 KB of data lose their original copy. No persona profiles. No full-duplex voice. Android MCP is HTTP-only. Shell is not sandboxed.

## Sources

Reuse these; consult a changed external contract once.

- **OpenAI:** developers.openai.com/api/reference/resources/responses/methods/create; /api/reference/resources/chat/subresources/completions/methods/create; /api/docs/guides/reasoning; /api/docs/guides/prompt-caching.md; /api/docs/pricing; /codex/agent-approvals-security; /codex/sandboxing
- **Gemini:** aistudio.google.com/api-reference/interactions-api; /docs/interactions-overview; /docs/background-execution; /docs/thinking; ai.google.dev/api/generate-content; ai.google.dev/gemini-api/docs/generate-content/thought-signatures
- **Anthropic:** platform.claude.com/docs/en/api/messages; /build-with-claude/streaming; /build-with-claude/prompt-caching; /build-with-claude/extended-thinking; /build-with-claude/thinking-tool-workflows; /about-claude/pricing; /agents-and-tools/tool-use/web-search-tool
- **DeepSeek:** api-docs.deepseek.com/guides/thinking_mode
- **Rates:** models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway/models-and-providers; raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
- **Windows:** learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate; /nf-winbase-powersetrequest
- **Android:** developer.android.com/develop/background-work/services/fgs/service-types; /fgs/timeout; /training/monitoring-device-state/doze-standby; /tools/adb; /reference/android/system/Os; /develop/ui/views/notifications/notification-permission
- **JVM and processes:** docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/file/Path.html; /java/lang/Process.html; pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html
- **Formats:** modelcontextprotocol.io/specification/latest/server/tools; github.com/kwaroran/character-card-spec-v3/blob/main/SPEC_V3.md
- **Security and GitHub:** cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html; docs.github.com/en/rest/releases/assets (asset digest is nullable, observed as `sha256:<hex>`)
- **Build:** kotlinlang.org/docs/gradle-compilation-and-caches.html; docs.gradle.org/8.13/userguide/configuration_cache.html; docs.oracle.com/en/java/javase/21/vm/class-data-sharing.html
