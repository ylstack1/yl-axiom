# YL Axiom TODO

Updated 2026-11-20. AGENTS.md owns rules, rigor, and stable contracts. Finished tasks are removed; git history and CHANGELOG.md keep history.

## Current status

- Source: YL Axiom v3 toolchain, dependency, code idiom, and CI modernization (Rust 1.100.0 + Cargo lints/cooldown, base64 0.23.1, zip 9.8.1, Kotlin 2.4.21/Compose 1.12.2/AGP 9.3.3/Gradle 9.7.1, CycloneDX 3.6.1, Scorecard v2.6.0, GitHub Actions cache-mode + prebuilt xtask reuse + lead-overhead sharding).
- Last verified: 2026-11-20: cargo xtask fmt, affected, android-so, cargo build --release -p parchment-desktop, test UiGalleryTest, perf: PASS review, PASS rust (clippy -D warnings, Cargo unused-dep lints deny, 68 workspace + 2 slim android-jni tests), PASS android-so (1.85 MB, 3 PT_LOAD 16 KB aligned), desktop-exe (27.82 MB, 244 ms window / 271 ms title / 2.55 ms core / 0.622 ms stream CPU p95 / 16.66 ms VSync / 136.5 MB idle RAM), ktlintCheck, 584 Kotlin tests + UiGalleryTest, compileAndroidMain; existing signed R8 APK 5.39 MB (private signer env vars not loaded).
- Next action: Load private signer (. .\scripts\load-private-signer.ps1) and run cargo xtask final for a fresh signed APK if publishing a release; otherwise review diff, commit, and run manual user checks (13.6, 17.1).

## Open tasks

For the user:
- [ ] 13.6 Screen readers & IME: verify Windows GPUI + Android with NVDA, TalkBack, and CJK IME composition (chat, composer, sidebar, settings, Ctrl+K palette) and report unlabeled or misordered controls.
- [ ] 17.1 Android phone check: install signed v3 APK, verify v2->SQLite WAL auto-migration, FTS5 search, edge-to-edge bars, Ctrl+G hardware keyboard, cold start (cargo xtask perf --android-install), swipe-kill checkpoint recovery, and Retry now after airplane mode.

Waiting on a measurement:
- [ ] 17.3 Android Composer TextFieldState: move Kotlin Android composer to TextFieldState leaf host if KeystrokeFrameProbeTest p95 nears 12 ms or phone typing lag appears.

## Known gaps

No Android long-press menus (reserved for text selection). cargo xtask final disables config cache (signer validation). No CHARX archives; cards >512 KB drop raw copy. No persona profiles or full-duplex voice. Android MCP is HTTP-only. Shell is unsandboxed.

## External contract sources

- OpenAI: developers.openai.com/api/reference/resources/responses & chat/completions; guides/reasoning, prompt-caching, pricing, rate-limits, error-codes; codex/agent-approvals-security & sandboxing.
- Gemini: aistudio.google.com/api-reference/interactions-api, background-execution, thinking; ai.google.dev/gemini-api/docs/generate-content, thought-signatures, rate-limits (RetryInfo.retryDelay; PerDay QuotaFailure never retries).
- Anthropic: platform.claude.com/docs/en/api/messages, errors, rate-limits (retry-after; enforced_spend_limit_reached stops), streaming, prompt-caching, extended/adaptive thinking, effort, thinking-tool-workflows, pricing.
- DeepSeek & Rates: api-docs.deepseek.com/guides/thinking_mode; models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway; litellm model_prices_and_context_window.json.
- Platforms & Specs: Win32 SetThreadExecutionState/PowerSetRequest; Android FGS/Doze/16 KB ELF (-Wl,-z,max-page-size=16384); MCP 2025-11-25 & 2026-07-28 (404 session renew once); Character Card Spec V3; OWASP SSRF; GitHub release asset sha256: digest.
