# YL Axiom — Current Architecture (Verified)

Source of truth: actual repository state at workspace root `/data/user/0/com.androidharness.app/files/github-projects/yl-axiom-6e3015d2`.

## Workspace layout
```
Cargo.toml                      (8 crates, Rust 1.100, edition 2024, MIT)
crates/
  parchment-model/              pure domain models (serde camelCase, v2-compatible)
  parchment-storage/            SQLite WAL + FTS5, schema, migrate_v2, archive codec
  parchment-protocol/           wire protocols, client, SSE, registry, usage, tokenizer, signatures
  parchment-prose/              streaming text buffer, markdown, diff, highlight, latex, svg
  parchment-harness/            agent harness: runner, tools, path_guard, permissions, approval, mcp, ssrf, codecs
  parchment-core/               facade + JNI bridge (network default / android-jni slim)
  parchment-desktop/            native Windows GPUI shell
  parchment-xtask/              dev/CI automation (std-only)
mobile/
  compose-app/                  KMP Compose source: androidMain, commonMain, desktopMain, jvmShared, commonTest
  android-app/                  AGP application shell (id, version, signing, R8, fonts)
```

## Crate responsibilities (verified by reading source)

### parchment-model (`crates/parchment-model/src/`)
Pure domain models and invariants. Serde JSON matches Parchment v2 `kotlinx.serialization` schema field-for-field so v2 `settings.json`, `chats/<id>/chat.json`, `live.json`, and `.parchment` archives round-trip with zero data or signature loss.
- `chat.rs`: `ChatSession`, `MessageNode`, `MessageRole`, `MessageVariant`, `ChatStub`, `PinnedMessageStub`, `ContextFragment`, `WorkspaceMode`, `TodoItem`, `GoalTrackerState`, media, citations, attachments.
- `permissions.rs`: `AgentPermissionPolicy`, `ToolApprovalMode` (AskForChanges/AskEveryTool/Never), `ShellAccess` (Disabled/FullAccess), `AgentPermissionOverrides`, `WorkspaceMode` (Off/ReadOnly/ReadWrite).
- `provider.rs`: `ProviderProfile`, `ProviderProtocol` (GeminiInteractions/OpenAiResponses/AnthropicMessages/Custom), `ModelPricingSpec`, `DefaultModelConfig`, `ProviderStorage`, `CUSTOM_DIALECTS`.
- `settings.rs`: `AppStateSnapshot`, `PreferencesConfig`, `AssistantAgent`, `SystemPromptPreset`, `ThemeConfig`, `DesktopWindowConfig`, `ProviderProfileStorage`.
- `usage.rs`: `TokenMetrics`, `TokenPricing`, `ProviderTokenUsage`, `ApiUsageRecord`, `ProviderChargeUsage`, `ContextPricingTier`, `ChargePricing`, pricing math.
- `import_policy.rs`: web-link check, media segment sanitization, portable imported media path.

### parchment-storage (`crates/parchment-storage/src/`)
- `lib.rs`: `StorageError`, exports.
- `schema.rs`: `SCHEMA_VERSION = 1`, WAL pragmas, `kv_settings`, `chats`, `messages`, `message_variants`, `tool_calls`, `media_items`, `web_citations`, `usage_records`, `messages_fts` FTS5 virtual table + triggers, `INLINE_TOOL_RESULT_LIMIT_BYTES = 64 KiB`.
- `migrate_v2.rs`: Non-destructive, idempotent v2 flat JSON → SQLite migrator. Never deletes/alters v2 files. Folds `live.json` sidecars verifying `(chatLength, chatModifiedMs)` stamp. Strips redundant attachment base64 only when local file readable. `KEY_V2_MIGRATION_MARKER = "v2_migration_v1"`.
- `archive.rs`: Atomic `.parchment`/`.zip`/`.json` chat archive importer/exporter. Zip-bomb guards, path traversal rejection, staging → decode → no-clobber commit. `ChatArchiveLimits` (max entries 20k, total 4 GiB).
- `store/` (`ParchmentStore`): persisted store; `SearchHit`.

### parchment-protocol (`crates/parchment-protocol/src/`)
- `client.rs`: `ProtocolClient` — async streaming executor for all 5 wire dialects, HTTP 400 reasoning self-healing (max→xhigh→high, minimal→low), retry/backoff, primary→legacy fallback (never for CUSTOM), multi-API-key rotation & 429/401/403 failover.
- `error_policy.rs`: `ProviderHttpError`, `ProtocolError`, `RunErrorKind` (ApiKey/Quota/SpendLimit/ConversationLimit/Model), non-retryable statuses, auth/billing stop markers.
- `history.rs`: Prompt-cache-stable history assembly, quantized 16-step stale tool compaction, `ReplayToolCallIds`, inline error notice stripping.
- `registry.rs`: `RegistryCatalogClient` — single-flight model catalog sync (OpenRouter → Vercel → LiteLLM → models.dev gap filler), 2.5 s modality wait, 60 s failure cooldown.
- `signatures.rs`: Opaque reasoning signatures, Franken-signature guards, effort downgrade ladder, 8 KiB reasoning handoff.
- `sse.rs`: `SseFrame`/`SseFramer` zero-copy SSE framer.
- `tokenizer.rs`: `O200kTokenizer` (`o200k_base`).
- `usage.rs`: `UsageDialect`, `ProviderUsageLedger`, per-dialect usage normalization, charge extraction.
- `wire.rs`: `WireDialect`, `ToolDefinition`, `WireRequest`, `StreamEvent`, dialect builders/parsers.

### parchment-prose (`crates/parchment-prose/src/`)
- `buffer.rs`: `StreamingTextBuffer` monotonic incremental buffer.
- `diff.rs`: `DiffLine`, `DiffLineKind`, unified diff compute/parse.
- `highlight.rs`: `HighlightSpan`, `TokenKind`, multi-language syntax highlighting.
- `latex.rs`: bounded LaTeX math AST.
- `markdown.rs`: `IncrementalProseParser`, cached pre-parsed Markdown block/inline AST.
- `svg.rs`: bounded SVG scene-graph parser.

### parchment-harness (`crates/parchment-harness/src/`)
21-tool agent harness, workspace security perimeter, SSRF-pinned public web transport, run-scoped approval broker, MCP client/registry, docx/card codecs, runner.
- `runner.rs`: `AgentTurnRunner` — multi-step streaming agent loop, `MAX_TOOL_ROUNDS = 64`, parallel read-only tool batches, parallel `agent_delegate` batches (max 4), `chat_spend_limit_usd` enforcement before every tool round, `RunActivity` monotonic elapsed timer (skips sleep gaps > 5 s and approval waits).
- `tools.rs`: `HarnessExecutor` — dispatcher for all 21 built-in tools + `mcp_schema` + `mcp__*`. Enforces `ToolApprovalBroker` + `AgentPermissionGate` + `ToolExecutionPermit::claim`. `MAX_SHELL_OUTPUT_HALF_BYTES = 16 KiB`, `MAX_PARALLEL_SUBAGENTS = 4`. Shell output head/tail spilling.
- `path_guard.rs`: `WorkspacePathGuard` — `fs::canonicalize` ancestor resolution, symlink/junction escape prevention, recursive symlink skip, read caps (4000 chars/line, 10000 lines, 250000 total), 16 MB edit file cap, ripgrep-backed `fs_search`, ignored-directory list (.git, build, .gradle, target, node_modules, etc.).
- `permissions.rs`: `AgentPermissionGate`, `HarnessToolDefinition`, `ToolCatalog` (21 built-in tools), `ToolAccessScope`, `ToolEffect`, `is_mcp_tool_name`, `COMPUTER_USE_TOOL_NAME`, `MCP_SCHEMA_TOOL_NAME`, `MCP_TOOL_PREFIX`, `DEFERRED_MCP_SCHEMA_THRESHOLD = 6`.
- `approval.rs`: `ToolApprovalBroker` — run-scoped, exact previews, one-use permits bound to raw args, denial suppression, sync invalidation, `ask_user` prompts. `ToolExecutionPermit` deliberately not serializable. `computer_use` NEVER accepts run-wide grants.
- `mcp.rs`: `McpClient`, `McpRegistry` — handshake 2025-11-25 with fallback 2026-07-28, Streamable HTTP + stdio transport, `mcp__<server>__<tool>` naming (<= 64 chars), max 3 restarts in 10 min, never resend the discovery call.
- `ssrf.rs`: `PublicWebTransport` — SSRF-hardened, public IP only, custom DNS resolver, no auto-redirects, max 6 hops, `is_public_ip`, `convert_html_to_readable_text`.
- `codecs.rs`: `.docx` extractor (XXE-free, zip-bomb bounded), SillyTavern/Parchment PNG + JSON card codec.
- `math_dice.rs`: bounded math expression evaluator, dice roller.

### parchment-core (`crates/parchment-core/src/`)
`ParchmentCore` facade shared by Windows GPUI and Android Kotlin. Two feature modes:
- `default = ["network"]`: full network runtime (reqwest Client, tokio runtime, ProtocolClient, RegistryCatalogClient, O200kTokenizer) for Windows GPUI desktop.
- `--no-default-features --features android-jni`: zero-TLS-duplication slim JNI `.so` for Android (SQLite WAL + FTS5 + v2 migrator + native search + `.docx`).
- `jni_android.rs`: direct zero-JNA JNI bridge to `io.parchment.ai.platform.ParchmentNativeCore`. 950+ generated functions. `ParchmentCore::open`, `load/save_settings_json`, `load/save_chat_json`, `delete_chat`, `search_chat_ids`, `workspace_search_native`, `extract_docx_native`, import/export archive, seed benchmark, count_tokens, etc.

### parchment-desktop (`crates/parchment-desktop/src/`)
Native Windows GPUI shell.
- `main.rs`: entry point, GPUI app, window config, data dir (`./parchment-portable-data` dev / `%LOCALAPPDATA%\ParchmentAI` release), CLI args.
- `app.rs`: `ParchmentDesktopApp` — multi-tab async streaming, `ActiveRunState` with abort/Retry now/Queue/Send now, 3.5 s SQLite checkpoints, one-use `ToolApprovalBroker`, background channel for model fetch/OTA/title/summary/translation.
- `composer.rs`: composer card deck, tools drawer, context popover, thinking popover, slash `/` and `@` autocomplete.
- `prose_view.rs`, `sidebar.rs`, `vault.rs`, `settings_view/`, `overlays/`, `ui_kit/`, `icons.rs`, `theme.rs`, `strings.rs`, `ota.rs`, `shortcuts.rs`, `win32.rs` (native COM file dialogs, DWM).

## Android/Kotlin architecture
- `mobile/compose-app/src/commonMain/`: shared Kotlin UI (Compose Multiplatform) — `ParchmentApp`, `ParchmentAppController`, UI pages, settings, harness wrappers, runtime.
- `mobile/compose-app/src/androidMain/`: Android-specific — `MainActivity`, `LongDecodeForegroundService` (specialUse FGS), `ParchmentAccessibilityService`, `OtaInstallReceiver`, `ParchmentFileProvider`, platform impls (`AndroidPlatformHost`, `AndroidOwnedShell`, `ParchmentNativeCore`, `PlatformStorage`, `PlatformRuntime`, `PlatformPickers`, `PlatformVoice`, `PlatformWorkspace`, `PlatformContextMenu`).
- `mobile/compose-app/src/desktopMain/`: desktop JVM (Compose desktop) — `Main.kt`, `WindowKeys`, platform impls (`DesktopPlatformHost`, `DesktopOwnedShell`, `DesktopMcpProcess`, `DesktopImageDecoder`, `DesktopComputerControl`, Windows chrome/folder dialog/memory/power lease).
- `mobile/compose-app/src/jvmShared/kotlin/`: shared JVM code.
- `mobile/android-app/`: AGP shell — applicationId `io.parchment.ai`, version/signing/R8, bundles fonts + `libparchment_core.so`.

### JNI contract
`ParchmentNativeCore.kt` ↔ `jni_android.rs` via `jni 0.22` (`EnvUnowned::with_env`). Methods: `nativeOpen`, `nativeLoadSettingsJson`, `nativeSaveSettingsJson`, `nativeLoadChatJson`, `nativeSaveChatJson`, `nativeDeleteChat`, `nativeSearchChatIds`, `nativeWorkspaceSearch`, `nativeExtractDocx`. Handle is a raw pointer to `Arc<ParchmentCore>`, cached per storage dir.

### Build/CI
- `cargo xtask` (std-only): status, review, affected, rust, android-so, ktlint, fmt, test, final, perf, disk, prune, pre-commit, ci.
- GitHub Actions: build-installers (Windows `.exe` + Android `.apk`), codeql, quality-signals, dependabot, release-please, scorecard, stale.
- `.github/CONTRIBUTING.md` (referenced): build/lint/test commands.

## Existing tests
Rust: `cargo test` across workspaces (68 workspace + 2 slim android-jni tests per TODO). Kotlin: 584 tests + `UiGalleryTest`. Perf budget verified on v3 post-audit (27.82 MB exe, 0.27 s start, 136.5 MB idle RAM, 1.85 MB `.so`).

## Known gaps (from TODO.md)
- No Android long-press menus (reserved for text selection).
- Android MCP is HTTP-only (no stdio on Android).
- Shell is unsandboxed.
- No persona profiles or full-duplex voice.
- Screen readers & IME verification pending (13.6).
- Android phone check pending (17.1).
