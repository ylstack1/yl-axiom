# Parchment TODO

Updated 2026-11-15. AGENTS.md owns workflow; MEMORY holds contracts. Task IDs stay stable. Finished tasks are removed: CHANGELOG.md and the git history keep what was done.

## Current status

- **Source:** 2026-11-16: Cross-stack perf & efficiency batch (`ChatIdentity` snapshot checks, `compactStateJson` default/null omission, async Android `onPause` flush & smaller image cache, desktop iconify `AppForeground`, `ModelsDevClient` streaming DTOs, incremental tokenizer prefix, static `tools` wire prefix & 16-step compaction, chunked `CodeFence` + stable item lambdas, stale `.tmp`/OTA pruning, parameterized test waits & `quality.py` binary skip).
- **Last verified:** 2026-11-16, `verify.py affected` (505 desktop tests + Android compile clean, 3m34s), `verify.py perf` (`desktop_title_ms=1403`, `desktop_idle_private_mb=265.9`, `p50_ms=3.7`, `p95_ms=5.8`, `average_shared_prefix=0.908..0.909`, `apk_mb=4.3`), and `unittest` (`test_workflow_tools.py`).
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
- **Anthropic:** platform.claude.com/docs/en/api/messages; /build-with-claude/streaming; /build-with-claude/prompt-caching; /build-with-claude/extended-thinking; /build-with-claude/adaptive-thinking; /build-with-claude/effort; /build-with-claude/thinking-tool-workflows; /about-claude/pricing; /agents-and-tools/tool-use/web-search-tool
- **DeepSeek:** api-docs.deepseek.com/guides/thinking_mode
- **Rates:** models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway/models-and-providers; raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json
- **Windows:** learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate; /nf-winbase-powersetrequest
- **Android:** developer.android.com/develop/background-work/services/fgs/service-types; /fgs/timeout; /training/monitoring-device-state/doze-standby; /tools/adb; /reference/android/system/Os; /develop/ui/views/notifications/notification-permission
- **JVM and processes:** docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/file/Path.html; /java/lang/Process.html; pubs.opengroup.org/onlinepubs/9799919799/functions/kill.html
- **Formats:** modelcontextprotocol.io/specification/latest/server/tools; /2026-07-28/basic/lifecycle; github.com/kwaroran/character-card-spec-v3/blob/main/SPEC_V3.md
- **Security and GitHub:** cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html; docs.github.com/en/rest/releases/assets (asset digest is nullable, observed as `sha256:<hex>`)
- **Build:** kotlinlang.org/docs/gradle-compilation-and-caches.html; docs.gradle.org/8.13/userguide/configuration_cache.html; docs.oracle.com/en/java/javase/21/vm/class-data-sharing.html
