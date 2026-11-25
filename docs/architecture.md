# Architecture

YL Axiom v3 combines a shared pure-Rust core (`crates/*`, SQLite WAL + FTS5, provider protocols, zero-copy prose engine, agent harness), a native Windows GPUI desktop shell (`crates/parchment-desktop`), and the Kotlin Multiplatform / Compose Foundation Android app (`mobile/compose-app` + `mobile/android-app`) backed by the shared Rust core via direct zero-JNA JNI (`arm64-v8a` `libparchment_core.so`).

New contributors and AI harnesses should start with [AGENTS.md](../AGENTS.md) for working rules, engineering rigor, and stable system contracts, and [CONTRIBUTING.md](../.github/CONTRIBUTING.md) for local setup and verification commands.

## Rust workspace (v3 native core & Windows GPUI desktop)

`Cargo.toml` defines 8 workspace crates (Rust `1.100.0`, edition 2024, `[workspace.lints.cargo]` unused-dependency deny + 3-day `.cargo/config.toml` publish-age cooldown):

| Crate | Role |
|---|---|
| `crates/parchment-model` | Exact v2-compatible camelCase serde models (`AppStateSnapshot`, `ChatSession`, `MessageNode`, `MessageVariant`, opaque signatures, transient screenshot skip, `with_imported_permissions`) |
| `crates/parchment-storage` | Normalized SQLite WAL + FTS5 store (`store/{mod,rows}.rs`, `schema.rs`, `> 64 KiB` tool blob spill, bounded 8 MiB WAL limit + `reclaim_storage()`, per-request fork usage deduplication), non-destructive idempotent v2 JSON migrator, atomic `.parchment`/`.zip`/`.json` archive codec |
| `crates/parchment-protocol` | Zero-copy SSE framer, all 5 wire dialects (`wire/{mod,builders,parser}.rs`), `CUSTOM` strict routing, multi-API-key rotation & 429/401/403 failover (`client.rs`), HTTP 400 self-healing, retry policy, `O200kTokenizer`, single-flight `RegistryCatalogClient`, `usage/{mod,ledger}.rs` |
| `crates/parchment-prose` | Monotonic `StreamingTextBuffer`, zero-copy incremental Markdown parser (cached prefix split invariant), multi-language highlighter, bounded LaTeX & sanitized SVG parsers, LCS unified diff |
| `crates/parchment-harness` | Multi-folder `WorkspacePathGuard` (+ desktop `ripgrep` feature / slim Android walker), SSRF-pinned `PublicWebTransport`, 21 built-in tools + `mcp__*`, `AgentPermissionGate`, one-use `ToolApprovalBroker`, `.docx`/PNG card codecs, MCP (`2025-11-25`/`2026-07-28`), `AgentTurnRunner` + `RunActivity` |
| `crates/parchment-core` | Shared facade (`ParchmentCore`): default `network` build for desktop; slim `--no-default-features --features android-jni` `cdylib` (`libparchment_core.so`, `0x4000` 16 KB ELF LOAD alignment) exporting 9 direct-JNI methods for `io.parchment.ai.platform.ParchmentNativeCore` |
| `crates/parchment-desktop` | Native Windows GPUI desktop app (`app/*`, `composer/*`, `overlays/*`, `prose_view/*`, `settings_view/*`, `sidebar.rs`, `vault.rs`, `ui_kit.rs`, `theme.rs`, `icons.rs`, `shortcuts.rs`, `ota.rs`, `win32.rs`): single standalone `.exe`, DWM caption tinting, COM dialogs, keep-awake thread, streaming SHA-256 OTA verification, all 9 WCAG-verified themes, MRU-12 tabs, FTS5 search, `Ctrl+K`/`Ctrl+G` |
| `crates/parchment-xtask` | Zero-dependency std-only Rust automation (`cargo xtask`): dual-stack verification ledger, static quality gates (Rust + Kotlin line caps, hardcoded-ref & secret leak bans), storage cache QC (`disk`/`prune`), CI planner/sharder/SBOM/signer stager, and desktop + Android benchmarks |

## Gradle modules and source sets

Gradle defines two modules sharing one version catalog (`gradle/libs.versions.toml`):

| Module | Role | JVM / SDK target | Packaging |
|---|---|---|---|
| `:composeApp` (`mobile/compose-app`) | Multiplatform shared code, Android Compose UI, and headless JVM test host (`desktopTest` / `UiGalleryTest`) | Desktop JVM 21 (tests & hot reload), Android JVM 17 | Consumed as KMP library by `:androidApp`; Windows production releases ship native Rust GPUI (`crates/parchment-desktop`) |
| `:androidApp` (`mobile/android-app`) | Android application shell (`AndroidManifest.xml`, launcher resources, `proguard-rules.pro`) | `minSdk 26`, `compileSdk 37`, `targetSdk 37` | Release APK via R8 full shrinking and resource shrinking |

`mobile/compose-app/build.gradle.kts` generates `/parchment-build.properties` read by `io.parchment.ai.ParchmentBuildConfig` (so version bumps do not invalidate Kotlin compilation caches) and wires pinned stable dependencies from `gradle/libs.versions.toml` (JDK `25` LTS, Gradle `9.7.1`, Kotlin `2.4.21`, AGP `9.3.3`, Compose Multiplatform `1.12.2`, Ktor `3.6.1`, Coroutines `1.11.0`, Serialization `1.13.0`, Immutable Collections `0.5.2`, AndroidX Activity Compose `1.13.0`, ProfileInstaller `1.4.1`, ktlint-gradle `14.2.0`). Windows native calls use the JDK's FFM API (`WindowsNative`), not JNA.

| Source set | Path | Compiles for | Role |
|---|---|---|---|
| `commonMain` | `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/` | Desktop + Android | UI, state, models, codecs, harness, providers, MCP, `expect` platform APIs and shared resources |
| `jvmShared` | `mobile/compose-app/src/jvmShared/kotlin/io/parchment/ai/platform/` | Desktop + Android | Shared JVM I/O, atomic storage, archives, bounded caches, SSRF web transport and workspace path/edit helpers |
| `desktopMain` | `mobile/compose-app/src/desktopMain/kotlin/io/parchment/ai/` | Windows desktop | Window entry point (`Main.kt`), Windows chrome, power/memory hooks, owned shells, image decode and pickers |
| `androidMain` | `mobile/compose-app/src/androidMain/kotlin/io/parchment/ai/` | Android | `MainActivity`, foreground service, accessibility computer-use service, OTA receiver, `FileProvider` and Android `actual`s |
| `commonTest` | `mobile/compose-app/src/commonTest/kotlin/io/parchment/ai/` | Desktop (+ common compilation) | Cross-platform unit tests for models, codecs, permissions, usage and persistence |
| `desktopTest` | `mobile/compose-app/src/desktopTest/kotlin/io/parchment/ai/` | Desktop JVM | Headless Compose `ImageComposeScene` UI tests, gallery renders, HTTP/SSE protocol suites and platform integration tests |

## Repository layout

Top-level files and tooling outside `mobile/compose-app/src`:

- **Build & formatting:** `Cargo.toml`, `clippy.toml`, `rustfmt.toml`, `.cargo/config.toml`, `.editorconfig`, `.githooks/pre-commit`, `build.gradle.kts`, `settings.gradle.kts`, `gradle/libs.versions.toml`, `mobile/compose-app/build.gradle.kts`, `mobile/compose-app/stability-config.conf`, `mobile/android-app/build.gradle.kts`, `mobile/android-app/proguard-rules.pro`, `mobile/android-app/src/main/AndroidManifest.xml`.
- **Verification & signing scripts:** `crates/parchment-xtask/` (`cargo xtask` verification, quality gates, storage QC, CI tools, and benchmarks), `scripts/create-private-signer.ps1`, `scripts/load-private-signer.ps1`.
- **Installers & assets:** `installer/windows-installer.iss` (packages standalone `parchment-desktop.exe`), `installer/parchment-icon.ico`, `installer/release-badge.svg`, `mobile/compose-app/src/commonMain/resources/icon/parchment-icon.png`.
- **CI, community & releases:** `.github/workflows/` (`build-installers.yml`, `release-please.yml`, `quality-signals.yml`, `codeql.yml`, `scorecard.yml`, `dependency-submission.yml`, `dependabot-automerge.yml`, `stale.yml`), `.github/scripts/` (`kover.init.gradle.kts`, `sbom.init.gradle.kts`, `ValidateAndroidSigningKey.java`), `.github/{CONTRIBUTING.md,CODE_OF_CONDUCT.md,SECURITY.md,dependabot.yml,pull_request_template.md,ISSUE_TEMPLATE/}`, `release-please-config.json`, `.release-please-manifest.json`.
- **Docs:** `README.md`, `AGENTS.md` (consolidated rules, engineering rigor and stable contracts, with harness shims `CLAUDE.md`, `GEMINI.md`, `.github/copilot-instructions.md`), `TODO.md`, `docs/design.md`, `docs/performance.md`, `docs/release-signing.md`, `docs/third-party-notices.md`.

## Common package and file map

All Kotlin files below sit under `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/`:

| Package | Files & responsibilities |
|---|---|
| `strings` | `Strings.kt` — central UI copy table (`val` for static strings, functions for formatted strings). Add all visible text here, never inline in screens. |
| `model` | `Models.kt` (providers, presets, assistants, worldbook/lorebook entries, chats/branches/turns, persisted `AppState`), `AgentPermissions.kt` (sandbox, path/command grants, approval policies), `ImportedContentPolicy.kt`, `TurnText.kt` (plain/markdown/thinking extraction), `UsageModels.kt`. |
| `protocol` | `CardAndPromptCodec.kt` (SillyTavern character card PNG/JSON, `.parchment` archives, Markdown/JSON export), `O200kTokenizer.kt` (token estimator), `GeminiThoughtSignature.kt` (opaque Gemini thinking-signature preservation). |
| `runtime` | `AppForeground.kt`, `RunActivity.kt` (background run / CPU wake activity accounting). |
| `harness` | `WorkspaceHarness.kt`, `WorkspaceToolCatalog.kt` (21 built-in tools: `fs_list`, `fs_glob`, `fs_read`, `fs_search`, `fs_write`, `fs_edit`, `fs_multi_edit`, `fs_move`, `fs_delete`, `web_search`, `web_fetch`, `calc_eval`, `dice_roll`, `todo_write`, `computer_use`, `agent_delegate`, `skill_load`, `ask_user`, `goal_complete`, `agent_memory`, `shell_exec`, plus `mcp__*`), `AgentPermissionGate.kt`, `ToolApprovalBroker.kt`, `WorkspaceContextCache.kt`, `WorkspaceWebSearch.kt`, `MathAndDiceEvaluator.kt`, `ToolOutputAndGoalSupport.kt`, `ComputerUse.kt`, `InterruptedRuns.kt`. |
| `platform` | Multiplatform contracts and pure-Kotlin helpers: `PlatformRuntime.kt`, `PlatformStorage.kt`, `PlatformWorkspace.kt`, `PlatformPickers.kt`, `PlatformVoice.kt`, `HtmlTextConverter.kt`, `OtaStagedDownload.kt`, `OwnedShellProtocol.kt`, `WorkspaceFuzzyEditEngine.kt`. |

### `network` package

- **Engine & streaming:** `AiProviderEngine.kt` (request dispatch and retry orchestration), `GenerationStreamSession.kt`, `GenerationStreamAccumulator.kt`, `SseStreamReader.kt`, `RetryControl.kt` (a run's retry waits, shown in the composer and ended early by Retry now).
- **Provider protocol wires:** `OpenAiResponsesProtocol.kt`, `OpenAiChatProtocol.kt`, `AnthropicMessagesProtocol.kt`, `GeminiInteractionsProtocol.kt`, `GeminiInteractionsStreamParser.kt`, `GeminiInteractionsWire.kt`, `GeminiGenerateContentProtocol.kt`.
- **Shared provider support:** `ProviderContextHistory.kt` (history/branch/lorebook assembly), `ProviderReasoning.kt`, `ProviderToolExecution.kt` (tool loop, parallel execution, approvals), `ToolScreenshots.kt`, `ProviderCitations.kt`, `ProviderMediaGeneration.kt`, `ProviderModelDiscovery.kt`, `ModelsDevClient.kt` (single-flight background model catalog & pricing sync), `ProviderTransportSupport.kt`, `ProviderErrorPolicy.kt` (typed HTTP failures, server retry hints, backoff, and which failures a person must fix), `ProviderUsage.kt`, `UsageCharges.kt`.
- **Extensions & updates:** `McpClient.kt`, `McpRegistry.kt`, `ExtensionSources.kt` (skills & extension discovery), `Subagents.kt` (`agent_delegate` orchestration), `OtaUpdateEngine.kt` (GitHub Release update checks and download staging).

### `ui` package (51 files)

- **Shell, state & runs:** `ParchmentApp.kt`, `ParchmentAppController.kt`, `AgentRunCoordinator.kt`, `ChatRunHosts.kt`, `GenerationRun.kt`, `StorySummaryRun.kt`, `AppShortcuts.kt`, `CommandPalette.kt`.
- **Persistence, usage & search:** `ParchmentStatePersistence.kt`, `ParchmentDiskWriter.kt`, `SessionSpend.kt`, `UtilityUsage.kt`, `PinnedReplies.kt`, `ChatHistorySearch.kt`.
- **Navigation, modals & sidebars:** `ParchmentModalsAndNavigation.kt`, `ParchmentSidebar.kt`, `ChatFilesSidebar.kt`, `AppDialogs.kt`, `FloatingNotification.kt`.
- **Chat viewport, turns & renderers:** `ManuscriptViewport.kt`, `EmptyChatAndNotes.kt`, `MessageTurnItem.kt`, `MessageTurnReasoningSection.kt`, `MessageTurnMediaAndDrawers.kt`, `MessageTurnFooter.kt`, `ComposerControls.kt`, `ProseRenderer.kt`, `LatexMathRenderer.kt`, `SvgSceneRenderer.kt`, `ParchmentDiffViewer.kt`, `FileImages.kt`.
- **Settings, pickers & permissions:** `ParchmentSettingsPages.kt`, `SettingsProvidersPage.kt`, `SettingsAssistantsPage.kt`, `SettingsDefaultModelPage.kt`, `SettingsWorkspacePage.kt`, `SettingsWorldBookPage.kt`, `SettingsPreferencesPage.kt`, `SettingsShortcutsPage.kt`, `SettingsBackupPage.kt`, `SettingsSearchResults.kt`, `McpSettings.kt`, `AgentPermissionEditor.kt`, `ParchmentModelPickerAndStats.kt`, `ParchmentProfileSettings.kt`.
- **Design system & icons:** `DesignTokens.kt`, `ParchmentTheme.kt`, `ParchmentComponents.kt`, `ParchmentFormComponents.kt`, `ParchmentIcons.kt`, `KissIconGlyphs.kt`.

## Platform implementations

| Source set | Files & responsibilities |
|---|---|
| `jvmShared/platform` | `AppAgentStorage.kt`, `AppStorageInventory.kt`, `AtomicFileStorage.kt`, `BoundedImageCache.kt`, `BoundedTextCapture.kt`, `ChatArchiveExtractor.kt`, `ChatArchiveWriter.kt`, `CrashRecorder.kt` (uncaught errors: flush, then a redacted `crash/last-crash.txt`), `DocxTextExtractor.kt` (zero-dep `.docx` `word/document.xml` extractor via `ZipInputStream`), `LocalMediaStore.kt`, `LocalSkillSourceReader.kt` (bounded local folder/zip skill import with traversal & zip-bomb guards), `OtaStagedFile.kt`, `PublicWebTransport.kt` (DNS/private-IP SSRF guard), `SnapshotFileStorage.kt`, `StreamingJsonDecoder.kt`, `WorkspaceFileEditor.kt`, `WorkspacePathGuard.kt`, `WorkspaceTextReader.kt`, `WorkspaceTreeScanner.kt`; `jvmShared/network/ProviderHttpEngine.kt` (OkHttp for both targets) and `jvmShared/ui/ProcessStatePreload.kt` (settings and open chat read while the window is built). |
| `desktopMain` | `Main.kt` (window startup, single-instance lock, Windows framing), `CdsWarmup.kt` (installer `--cds-warmup`), `WindowKeys.kt`, and `platform/`: `DesktopComputerControl.kt`, `DesktopImageDecoder.kt`, `DesktopMcpProcess.kt`, `DesktopOwnedShell.kt`, `DesktopPlatformHost.kt`, `PlatformPickers.desktop.kt`, `PlatformRuntime.desktop.kt`, `PlatformStorage.desktop.kt`, `PlatformVoice.desktop.kt`, `PlatformWorkspace.desktop.kt`, `WindowsChrome.kt`, `WindowsFolderDialog.kt` (IFileOpenDialog via FFM COM), `WindowsMemory.kt`, `WindowsNative.kt` (FFM downcalls), `WindowsPowerLease.kt`; `ui/PlatformContextMenu.desktop.kt` (right-click menus). |
| `androidMain` | `MainActivity.kt`, `LongDecodeForegroundService.kt`, `ParchmentAccessibilityService.kt`, `OtaInstallReceiver.kt`, `ParchmentFileProvider.kt`, and `platform/`: `AndroidComputerControl.kt`, `AndroidOwnedShell.kt`, `AndroidPlatformHost.kt`, `PlatformPickers.android.kt`, `PlatformRuntime.android.kt`, `PlatformStorage.android.kt`, `PlatformVoice.android.kt`, `PlatformWorkspace.android.kt`. |

## Core runtime flows

### State ownership and persistence

1. `Main.kt` (desktop) and `MainActivity.kt` (Android) initialize platform hosts (`DesktopPlatformHost` / `AndroidPlatformHost`) and render `ParchmentApp`, which binds Compose snapshot state in `ParchmentAppController` and `ParchmentStatePersistence`.
2. Initial hydration loads metadata and chat stubs first so startup stays fast even with large histories; inactive chats come from `chat-index.json` (stubs checked against each `chat.json`'s size and modification time, a cache only) and full chat bodies hydrate on demand and persist incrementally through `ParchmentDiskWriter` backed by `AtomicFileStorage` / `SnapshotFileStorage`. While a reply streams, its checkpoints save the reply alone to `chats/<id>/live.json`, stamped with the `chat.json` it extends; the whole chat is saved at most every 30 s and when the reply ends (which deletes the sidecar), and loading a chat folds in a sidecar a crash left behind.
3. Data roots:
   - **Desktop packaged / Windows user install:** `%LOCALAPPDATA%\YL-Axiom`
   - **Desktop development (`:composeApp:run`):** `mobile/compose-app/parchment-portable-data` (override with `--user-dir`)
   - **Android:** `Context.filesDir`
   - **Tests:** isolated temporary directories (`TemporaryFolder` / `createTempDirectory`) so dev or installed user data is never touched.

### Generation, streaming and tool loop

1. Sending or regenerating from `ManuscriptViewport` / `ComposerControls` delegates through `ChatRunHosts` (`GenerationRun` for primary replies, `StorySummaryRun` for memory summaries) and `AgentRunCoordinator` so background chats continue running if the user switches tabs.
2. `AiProviderEngine` selects the wire implementation from provider protocol (`OpenAiResponsesProtocol`, `OpenAiChatProtocol`, `AnthropicMessagesProtocol`, `GeminiInteractionsProtocol`, `GeminiGenerateContentProtocol`), prepares history via `ProviderContextHistory` and `ProviderReasoning` (preserving opaque `GeminiThoughtSignature` blobs), and streams chunks into `GenerationStreamSession` and `GenerationStreamAccumulator`.
3. Tool calls route through `ProviderToolExecution` -> `AgentPermissionGate` & `ToolApprovalBroker` -> built-in `WorkspaceHarness` tools, `Subagents` (`agent_delegate`), `ComputerUse`, or `McpClient` servers (`mcp__*`). Approval prompts and `ask_user` cards suspend safely and resume in-place across retries or interrupted-run recovery (`InterruptedRuns`). `McpRegistry` starts a stopped server again (at most three starts in ten minutes) under the same tool names, never resending the call that found it stopped.
4. Failed requests: `ProviderErrorPolicy` decides from the HTTP status first (wording only where the status is ambiguous) whether to retry, how long to wait (the server's own hint, else backoff up to 60 s) and whether a person has to fix something (key, model, conversation limit, spend limit, quota), which the run's notice then offers.
5. Usage tokens and estimated spend flow from `ProviderUsage` and `UsageCharges` into `SessionSpend`, `UtilityUsage` and `MessageTurnFooter`.

## Testing, verification and quality gates

Never run Gradle directly or in parallel; use `cargo xtask` (`crates/parchment-xtask`):

| Command | When to run |
|---|---|
| `cargo xtask status` | Start of every session: prints git status, unverified files, last pass, and `TODO.md` summary |
| `cargo xtask install-hooks` | Once per clone: configures `core.hooksPath = .githooks` so `pre-commit` runs review + `rustfmt` + staged `clippy`/`ktlint` |
| `cargo xtask fmt` | Auto-format both Rust (`cargo fmt --all`) and Kotlin (`ktlintFormat`) |
| `cargo xtask review` | Docs-only or fast review: static quality gates + `git diff --check` + `cargo fmt --all -- --check` |
| `cargo xtask affected` | End of every batch: ledger-backed Rust (`rustfmt`, `clippy -D warnings`, workspace + slim JNI tests) and/or Kotlin (`ktlintCheck`, `:composeApp:desktopTest`, Android compile) |
| `cargo xtask rust` | Full Rust gate across all 8 workspace crates and slim `--no-default-features --features android-jni` |
| `cargo xtask ktlint` | Kotlin formatting and style check (`ktlintCheck` on `:composeApp` and `:androidApp`) |
| `cargo xtask android-so` | Cross-compile slim `arm64-v8a` `libparchment_core.so` (`<= 2.50 MB`) and verify 16 KB (`0x4000`) ELF alignment |
| `cargo xtask test <Filter>` | Fast inner loop for targeted Kotlin tests (for example `UiGalleryTest` to render gallery screenshots) |
| `cargo xtask android` | Explicit Android Kotlin compile check (`:composeApp:compileAndroidMain`) |
| `cargo xtask perf` | Native desktop `.exe` (300-chat startup/RAM/streaming) and Android APK/phone benchmark |
| `cargo xtask disk` / `prune` | Measure workspace build/cache directories against storage budgets and safely prune stale incremental/deps/log bloat |
| `cargo xtask final` | Full release gate (review, Rust, `.so`, desktop `.exe`, `ktlint`, Kotlin tests + gallery, signed APK) with private signer loaded |

For interactive Kotlin Compose UI work (a person, not a check), `./gradlew :composeApp:hotRunDesktop` runs with Compose Hot Reload. Streaming replies are drawn from `ui/LiveTurnOverlay.kt` between checkpoints.

Conventions enforced by tests and quality gates:

- **Search portably:** use `git grep -n` (Windows environments may not have `rg` installed).
- **UI tests:** use Compose Skiko `ImageComposeScene` on the AWT EDT (`runBlocking(Dispatchers.Swing)` with `Dispatchers.Swing` / `Dispatchers.Swing.immediate` so tests serialize with Compose Desktop's internal `SwingDispatcher`), calling `Snapshot.sendApplyNotifications()` after mutating state before rendering. See `desktopTest/kotlin/io/parchment/ai/ui/` (`AgentPermissionUiTest.kt`, `TurnUsageUiTest.kt`, `UiBehaviorRegressionTest.kt`, `KeyboardShortcutTest.kt`, `SharedActionSemanticsUiTest.kt`, `UiGalleryTest.kt`, `PerformanceBaselineTest.kt`).
- **Network tests:** use local HTTP/SSE fixtures (`desktopTest/network/` and `crates/parchment-protocol/tests/`), never live endpoints.
- **Static checks (`cargo xtask review` + `ktlintCheck` + workspace `clippy -D warnings` + `[workspace.lints.cargo]`):** enforces `rustfmt.toml`, `clippy.toml` (`unwrap_used`/`expect_used`/`todo`/`dbg_macro`/`print_stdout` in production code), Cargo 1.100 `unused_dependencies`/`unused_workspace_dependencies`/`missing_lints_inheritance` deny, `.editorconfig`, LF line endings (except `.bat`/`.iss` CRLF), UTF-8 without BOM, ASCII `.ps1` scripts, zero unused imports or dead code, 1,500-line Rust and 3,000-line Kotlin file caps, banned APIs (`androidx.compose.material`, `GlobalScope`, `println(`, `FIXME`, `TODO(`, `.clickable(enabled = false)`), hardcoded model/device reference ban, valid relative Markdown links and `#heading` anchors, no tracked secrets or credential literals, and strict 10,000-char doc budgets (`AGENTS.md`, `TODO.md`).

## Where to change what

| Task | Start here |
|---|---|
| Add or edit visible UI text | `commonMain/.../strings/Strings.kt` (check wording rules in [docs/design.md](design.md)) |
| Change colors, spacing, radii, strokes, motion or overlay scrims | `commonMain/.../ui/DesignTokens.kt`, `ui/ParchmentTheme.kt`, [docs/design.md](design.md) |
| Add a vector icon | `commonMain/.../ui/KissIconGlyphs.kt` and `ui/ParchmentIcons.kt` (24-unit grid) |
| Change chat layout, composer, shortcuts or modals | `ui/ParchmentApp.kt`, `ui/ManuscriptViewport.kt`, `ui/ComposerControls.kt`, `ui/ParchmentModalsAndNavigation.kt`, `ui/CommandPalette.kt`, `ui/AppShortcuts.kt` |
| Change message rendering (prose, code, LaTeX, SVG, diffs, media) | `ui/MessageTurnItem.kt`, `ui/ProseRenderer.kt`, `ui/LatexMathRenderer.kt`, `ui/SvgSceneRenderer.kt`, `ui/ParchmentDiffViewer.kt`, `ui/FileImages.kt` |
| Change agent run lifecycle or background tabs | `ui/ParchmentAppController.kt`, `ui/ChatRunHosts.kt`, `ui/GenerationRun.kt`, `ui/AgentRunCoordinator.kt` |
| Add or modify a provider protocol or stream parser | `network/AiProviderEngine.kt` + matching `*Protocol.kt` (`OpenAi*`, `Anthropic*`, `Gemini*`), `ProviderContextHistory.kt`, `ProviderUsage.kt` |
| Add or modify a built-in agent tool | `harness/WorkspaceToolCatalog.kt`, `harness/WorkspaceHarness.kt`, `network/ProviderToolExecution.kt`, `harness/AgentPermissionGate.kt` |
| Change workspace security, SSRF or atomic file I/O | `jvmShared/platform/` (`WorkspacePathGuard.kt`, `PublicWebTransport.kt`, `AtomicFileStorage.kt`, `SnapshotFileStorage.kt`) + [AGENTS.md](../AGENTS.md#stable-contracts) |
| Change chat persistence, imports or `.parchment` archives | `ui/ParchmentStatePersistence.kt`, `ui/ParchmentDiskWriter.kt`, `protocol/CardAndPromptCodec.kt`, `jvmShared/platform/ChatArchive*.kt` |
| Change Windows desktop chrome, power/memory or owned shell | `desktopMain/WindowKeys.kt`, `desktopMain/Main.kt`, `desktopMain/platform/{WindowsChrome,WindowsMemory,WindowsPowerLease,DesktopOwnedShell,DesktopPlatformHost}.kt` |
| Change Android activity, foreground runs, OTA receiver or computer use | `androidMain/{MainActivity,LongDecodeForegroundService,ParchmentAccessibilityService,OtaInstallReceiver,ParchmentFileProvider}.kt`, `androidMain/platform/*` |

## Releases

Versioning and changelogs are automated with `release-please` (`.github/workflows/release-please.yml`, `release-please-config.json`, `.release-please-manifest.json`). Conventional Commits of type `feat:`, `fix:` and `perf:` advance the release PR and [CHANGELOG.md](../CHANGELOG.md); `docs:`, `chore:`, `ci:`, `refactor:` and `test:` are hidden in `release-please-config.json` and do not open a release PR. Merging the release PR creates the version tag, which triggers `.github/workflows/build-installers.yml` to build the standalone Rust GPUI desktop installer (`.exe`) and signed Android APK (`.apk` bundling `libparchment_core.so`). Signed release setup lives in [docs/release-signing.md](release-signing.md); local verification is covered in [.github/CONTRIBUTING.md#build-lint-and-test](../.github/CONTRIBUTING.md#build-lint-and-test).
