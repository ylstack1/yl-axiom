# Parchment TODO

Updated 2026-11-19. [AGENTS.md](AGENTS.md) owns rules, rigor, and stable contracts. Finished tasks are removed; git history and `CHANGELOG.md` keep history.

## Current status

- **Source:** Parchment v3 rewrite, lean refactor, and dual-stack repo modernization (`v3` branch): 8-crate Rust workspace (native GPUI Windows `.exe`, SQLite WAL+FTS5 core, slim `1.85 MB` 16 KB-aligned `arm64-v8a` JNI `.so`, zero-dep `parchment-xtask` replacing all 17 Python scripts) + Kotlin Mobile (`composeApp`/`androidApp` with `ktlintCheck`), `.githooks/pre-commit`, dual-stack CI + Cargo Dependabot + CodeQL Rust, and cleaned GitHub metadata/branches/releases.
- **Last verified:** 2026-11-19: `cargo xtask final` (private signer): `PASS review` (static gates + `rustfmt`), `PASS rust` (`clippy -D warnings`, 56 workspace + 2 slim `android-jni` tests), `PASS android-so` (`1.85 MB`, 3 `PT_LOAD` 16 KB aligned), `PASS desktop-exe` (`22.61 MB`), `ktlintCheck`, 582 Kotlin tests + UI gallery, signed R8 APK (`6.30 MB`, bundles `libparchment_core.so`).
- **Next action:** Run manual checks below (NVDA/TalkBack/CJK IME and Android phone v3.0.0 check).

## Open tasks

### For the user

- [ ] **13.6 Screen readers & IME:** verify Windows GPUI + Android with NVDA, TalkBack, and CJK IME composition (chat, composer, sidebar, settings, Ctrl+K palette) and report unlabeled or misordered controls.
- [ ] **17.1 Android phone check:** install signed v3 APK (`6.30 MB`), verify v2->SQLite WAL auto-migration, FTS5 search, edge-to-edge bars, Ctrl+G hardware keyboard, cold start (`cargo xtask perf --android-install`), swipe-kill checkpoint recovery, and `Retry now` after airplane mode.

### Waiting on a measurement

- [ ] **17.3 Android Composer TextFieldState:** move Kotlin Android composer to `TextFieldState` leaf host if `KeystrokeFrameProbeTest` p95 nears `12 ms` (measured `7.6 ms`, 2026-11-16) or phone typing lag appears.

## Known gaps

No Android long-press menus (reserved for text selection). `cargo xtask final` disables config cache (signer validation). No CHARX archives; cards `>512 KB` drop raw copy. No persona profiles or full-duplex voice. Android MCP is HTTP-only. Shell is unsandboxed.

## External contract sources

- **OpenAI:** developers.openai.com/api/reference/resources/responses & chat/completions; guides/reasoning, prompt-caching, pricing, rate-limits, error-codes; codex/agent-approvals-security & sandboxing.
- **Gemini:** aistudio.google.com/api-reference/interactions-api, background-execution, thinking; ai.google.dev/gemini-api/docs/generate-content, thought-signatures, rate-limits (`RetryInfo.retryDelay`; `PerDay` `QuotaFailure` never retries).
- **Anthropic:** platform.claude.com/docs/en/api/messages, errors, rate-limits (`retry-after`; `enforced_spend_limit_reached` stops), streaming, prompt-caching, extended/adaptive thinking, effort, thinking-tool-workflows, pricing.
- **DeepSeek & Rates:** api-docs.deepseek.com/guides/thinking_mode; models.dev/api.json; openrouter.ai/api/v1/models; vercel.com/docs/ai-gateway; litellm `model_prices_and_context_window.json`.
- **Platforms & Specs:** Win32 `SetThreadExecutionState`/`PowerSetRequest`; Android FGS/Doze/16 KB ELF (`-Wl,-z,max-page-size=16384`); MCP `2025-11-25` & `2026-07-28` (404 session renew once); Character Card Spec V3; OWASP SSRF; GitHub release asset `sha256:` digest.
