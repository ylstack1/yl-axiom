# Parchment TODO

Updated 2026-11-15. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-15: reasoning handoff raised to `8_000` chars (head+tail), per-target dialect/model handoff & chain guards (`providerWireDialect`, step order, overflow/fallback/nudge/signatureless-tool fixes), and bounded transient-resilient Gemini polling with fast-fail non-retryable errors; `v2.1.10` released (`17.1`–`17.6` in CHANGELOG.md).
- **Last verified:** 2026-11-15, `verify.py affected`: 462 desktop tests passed (2m35s); `v2.1.10` `verify.py final` (454 desktop tests + signed Android build 6m16s, `UiGalleryTest` 4/4 1m45s, `perf` 1407 ms title / 269.1 MB idle / 4.3 MB APK).
- **Next action:** Check `13.6 Screen readers` when the user tests NVDA and TalkBack.

## Open tasks

### For the user

- [ ] **13.6 Screen readers:** go through the app with NVDA on Windows and TalkBack on Android (chat, composer, sidebar, settings, Ctrl+K palette) and report anything unlabeled, read in the wrong order, or unreachable.

### Paused

- [ ] **RectList crash in UI tests (paused 2026-11-12):** `AgentPermissionUiTest.substantialSettingsFieldsStackAndTextFits` fails intermittently in CI ("Coverage and UI gallery") with `IllegalArgumentException: LayoutNode N not found in RectList`, thrown from `ImageComposeScene.close()` (`CompositionImpl.dispose` → `LayoutNode.detach` → `RectManager.remove`) after every assertion passed. Traces: runs 43145027121 (PR #8), 43147823103 (PR #9, both retry attempts), 43160848842 (full stack). Not suppressed while the cause is unknown; #9's retry can't help (each attempt ends in the same `close()`) and is reverted in #12. A rerun of 43147823103 (code equal to `main`) passed. Ruled out: `Dispatchers.Unconfined` moving scene work off the test thread (0 of 83,500 scene tasks off-thread; 0 hits in 480 local runs of the class under CPU load). Remaining leads: (1) CI test order: log it, and check whether failures follow `KeyboardShortcutTest` or other tests that render the whole `ParchmentApp` in the same JVM (local runs with and without it: 0 hits each, inconclusive); (2) a Compose Multiplatform release after 1.12.2 that fixes RectList (needs `verify.py final`).

## Known gaps

Not planned; open a task first. Compose 1.12 can fail in its own layout bookkeeping ("not found in RectList"; about half of gallery runs; desktop UI tests retry it once): watch for it in the app and update Compose when fixed. No JDK AOT cache (16.6, dropped 2026-11): AppCDS already meets the start budget, and a cache built in CI may not match installed paths. No CHARX archives; cards over 512 KB of data lose their original copy. No persona profiles. No full-duplex voice. Android MCP is HTTP-only. Shell is not sandboxed.

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
