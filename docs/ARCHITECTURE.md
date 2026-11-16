# Architecture

Parchment is a Kotlin Multiplatform application built with Compose Multiplatform (Foundation only, no Material). It targets Windows desktop and Android from shared protocol, agent-harness, storage and UI code.

New contributors and AI harnesses should start with [AGENTS.md](../AGENTS.md) for working rules, [CONTRIBUTING.md](../CONTRIBUTING.md) for local setup and verification commands, and [MEMORY.md](../MEMORY.md) for stable system contracts.

## Modules and source sets

Gradle defines two modules sharing one version catalog (`gradle/libs.versions.toml`):

| Module | Role | JVM / SDK target | Packaging |
|---|---|---|---|
| `composeApp` | Multiplatform shared code, UI, desktop `main()` and desktop/common tests | Desktop JVM 21, Android JVM 17 | Desktop distributable via JLink + AppCDS + ProGuard, consumed as KMP library by `androidApp` |
| `androidApp` | Android application shell (`AndroidManifest.xml`, launcher resources, `proguard-rules.pro`) | `minSdk 26`, `compileSdk 37`, `targetSdk 37` | Release APK via R8 full shrinking and resource shrinking |

`composeApp/build.gradle.kts` generates `io.parchment.ai.ParchmentBuildConfig` (build version and platform flags) and wires pinned dependencies from `gradle/libs.versions.toml` (Kotlin `2.4.21`, AGP `9.3.3`, Compose Multiplatform `1.12.2`, Ktor `3.6.1`, Coroutines `1.11.0`, Serialization `1.12.0`, JNA `5.19.1`).

| Source set | Path | Compiles for | Role |
|---|---|---|---|
| `commonMain` | `composeApp/src/commonMain/kotlin/io/parchment/ai/` | Desktop + Android | UI, state, models, codecs, harness, providers, MCP, `expect` platform APIs and shared resources |
| `jvmShared` | `composeApp/src/jvmShared/kotlin/io/parchment/ai/platform/` | Desktop + Android | Shared JVM I/O, atomic storage, archives, bounded caches, SSRF web transport and workspace path/edit helpers |
| `desktopMain` | `composeApp/src/desktopMain/kotlin/io/parchment/ai/` | Windows desktop | Window entry point (`Main.kt`), Windows chrome, power/memory hooks, owned shells, image decode and pickers |
| `androidMain` | `composeApp/src/androidMain/kotlin/io/parchment/ai/` | Android | `MainActivity`, foreground service, accessibility computer-use service, OTA receiver, `FileProvider` and Android `actual`s |
| `commonTest` | `composeApp/src/commonTest/kotlin/io/parchment/ai/` | Desktop (+ common compilation) | Cross-platform unit tests for models, codecs, permissions, usage and persistence |
| `desktopTest` | `composeApp/src/desktopTest/kotlin/io/parchment/ai/` | Desktop JVM | Headless Compose `ImageComposeScene` UI tests, gallery renders, HTTP/SSE protocol suites and platform integration tests |

## Repository layout

Top-level files and tooling outside `composeApp/src`:

- **Build & dependencies:** `build.gradle.kts`, `settings.gradle.kts`, `gradle/libs.versions.toml`, `composeApp/build.gradle.kts`, `androidApp/build.gradle.kts`, `androidApp/proguard-rules.pro`, `androidApp/src/main/AndroidManifest.xml`.
- **Verification & automation scripts:** `scripts/verify.py` (ledger-backed test/gate runner), `scripts/quality.py` (encoding, dead-code, banned-pattern, link and doc-budget checks), `scripts/quality-baseline.json`, `scripts/perf.py` (300-chat startup/frame benchmark), `scripts/test_workflow_tools.py` (Python script tests), `scripts/create-private-signer.ps1`, `scripts/load-private-signer.ps1`.
- **Installers & assets:** `installer/windows-installer.iss`, `installer/parchment-icon.ico`, `installer/release-badge.svg`, `composeApp/src/commonMain/resources/icon/parchment-icon.png`.
- **CI & releases:** `.github/workflows/` (`build-installers.yml`, `release-please.yml`, `quality-signals.yml`, `codeql.yml`, `scorecard.yml`, `dependency-submission.yml`, `dependabot-automerge.yml`, `stale.yml`), `release-please-config.json`, `.release-please-manifest.json`.
- **Docs:** `README.md`, `AGENTS.md` (with harness shims `CLAUDE.md`, `GEMINI.md`, `.github/copilot-instructions.md`), `CONTRIBUTING.md`, `MEMORY.md`, `TODO.md`, `docs/DESIGN.md`, `docs/PERFORMANCE.md`, `docs/RELEASE_SIGNING.md`.

## Common package and file map

All Kotlin files below sit under `composeApp/src/commonMain/kotlin/io/parchment/ai/`:

| Package | Files & responsibilities |
|---|---|
| `strings` | `Strings.kt` — central UI copy table (`val` for static strings, functions for formatted strings). Add all visible text here, never inline in screens. |
| `model` | `Models.kt` (providers, presets, assistants, worldbook/lorebook entries, chats/branches/turns, persisted `AppState`), `AgentPermissions.kt` (sandbox, path/command grants, approval policies), `ImportedContentPolicy.kt`, `TurnText.kt` (plain/markdown/thinking extraction), `UsageModels.kt`. |
| `protocol` | `CardAndPromptCodec.kt` (SillyTavern character card PNG/JSON, `.parchment` archives, Markdown/JSON export), `O200kTokenizer.kt` (token estimator), `GeminiThoughtSignature.kt` (opaque Gemini thinking-signature preservation). |
| `runtime` | `AppForeground.kt`, `RunActivity.kt` (background run / CPU wake activity accounting). |
| `harness` | `WorkspaceHarness.kt`, `WorkspaceToolCatalog.kt` (21 built-in tools: `fs_list`, `fs_glob`, `fs_read`, `fs_search`, `fs_write`, `fs_edit`, `fs_multi_edit`, `fs_move`, `fs_delete`, `web_search`, `web_fetch`, `calc_eval`, `dice_roll`, `todo_write`, `computer_use`, `agent_delegate`, `skill_load`, `ask_user`, `goal_complete`, `agent_memory`, `shell_exec`, plus `mcp__*`), `AgentPermissionGate.kt`, `ToolApprovalBroker.kt`, `WorkspaceContextCache.kt`, `WorkspaceWebSearch.kt`, `MathAndDiceEvaluator.kt`, `ToolOutputAndGoalSupport.kt`, `ComputerUse.kt`, `InterruptedRuns.kt`. |
| `platform` | Multiplatform contracts and pure-Kotlin helpers: `PlatformRuntime.kt`, `PlatformStorage.kt`, `PlatformWorkspace.kt`, `PlatformPickers.kt`, `PlatformVoice.kt`, `HtmlTextConverter.kt`, `OtaStagedDownload.kt`, `OwnedShellProtocol.kt`, `WorkspaceFuzzyEditEngine.kt`. |

### `network` package

- **Engine & streaming:** `AiProviderEngine.kt` (request dispatch and retry orchestration), `GenerationStreamSession.kt`, `GenerationStreamAccumulator.kt`, `SseStreamReader.kt`.
- **Provider protocol wires:** `OpenAiResponsesProtocol.kt`, `OpenAiChatProtocol.kt`, `AnthropicMessagesProtocol.kt`, `GeminiInteractionsProtocol.kt`, `GeminiInteractionsStreamParser.kt`, `GeminiInteractionsWire.kt`, `GeminiGenerateContentProtocol.kt`.
- **Shared provider support:** `ProviderContextHistory.kt` (history/branch/lorebook assembly), `ProviderReasoning.kt`, `ProviderToolExecution.kt` (tool loop, parallel execution, approvals), `ToolScreenshots.kt`, `ProviderCitations.kt`, `ProviderMediaGeneration.kt`, `ProviderModelDiscovery.kt`, `ModelsDevClient.kt` (single-flight background model catalog & pricing sync), `ProviderTransportSupport.kt`, `ProviderErrorPolicy.kt`, `ProviderUsage.kt`, `UsageCharges.kt`.
- **Extensions & updates:** `McpClient.kt`, `McpRegistry.kt`, `ExtensionSources.kt` (skills & extension discovery), `Subagents.kt` (`agent_delegate` orchestration), `OtaUpdateEngine.kt` (GitHub Release update checks and download staging).

### `ui` package (50 files)

- **Shell, state & runs:** `ParchmentApp.kt`, `ParchmentAppController.kt`, `AgentRunCoordinator.kt`, `ChatRunHosts.kt`, `GenerationRun.kt`, `StorySummaryRun.kt`, `AppShortcuts.kt`, `CommandPalette.kt`.
- **Persistence, usage & search:** `ParchmentStatePersistence.kt`, `ParchmentDiskWriter.kt`, `SessionSpend.kt`, `UtilityUsage.kt`, `PinnedReplies.kt`, `ChatHistorySearch.kt`.
- **Navigation, modals & sidebars:** `ParchmentModalsAndNavigation.kt`, `ParchmentSidebar.kt`, `ChatFilesSidebar.kt`, `AppDialogs.kt`, `FloatingNotification.kt`.
- **Chat viewport, turns & renderers:** `ManuscriptViewport.kt`, `EmptyChatAndNotes.kt`, `MessageTurnItem.kt`, `MessageTurnReasoningSection.kt`, `MessageTurnMediaAndDrawers.kt`, `MessageTurnFooter.kt`, `ComposerControls.kt`, `ProseRenderer.kt`, `LatexMathRenderer.kt`, `SvgSceneRenderer.kt`, `ParchmentDiffViewer.kt`, `FileImages.kt`.
- **Settings, pickers & permissions:** `ParchmentSettingsPages.kt`, `SettingsProvidersPage.kt`, `SettingsAssistantsPage.kt`, `SettingsDefaultModelPage.kt`, `SettingsWorkspacePage.kt`, `SettingsWorldBookPage.kt`, `SettingsPreferencesPage.kt`, `SettingsBackupPage.kt`, `SettingsSearchResults.kt`, `McpSettings.kt`, `AgentPermissionEditor.kt`, `ParchmentModelPickerAndStats.kt`, `ParchmentProfileSettings.kt`.
- **Design system & icons:** `DesignTokens.kt`, `ParchmentTheme.kt`, `ParchmentComponents.kt`, `ParchmentFormComponents.kt`, `ParchmentIcons.kt`, `KissIconGlyphs.kt`.

## Platform implementations

| Source set | Files & responsibilities |
|---|---|
| `jvmShared/platform` | `AppAgentStorage.kt`, `AppStorageInventory.kt`, `AtomicFileStorage.kt`, `BoundedImageCache.kt`, `BoundedTextCapture.kt`, `ChatArchiveExtractor.kt`, `ChatArchiveWriter.kt`, `LocalMediaStore.kt`, `OtaStagedFile.kt`, `PublicWebTransport.kt` (DNS/private-IP SSRF guard), `SnapshotFileStorage.kt`, `StreamingJsonDecoder.kt`, `WorkspaceFileEditor.kt`, `WorkspacePathGuard.kt`, `WorkspaceTextReader.kt`, `WorkspaceTreeScanner.kt`. |
| `desktopMain` | `Main.kt` (window startup, single-instance lock, JNA Windows framing), `WindowKeys.kt`, and `platform/`: `DesktopComputerControl.kt`, `DesktopImageDecoder.kt`, `DesktopMcpProcess.kt`, `DesktopOwnedShell.kt`, `DesktopPlatformHost.kt`, `PlatformPickers.desktop.kt`, `PlatformRuntime.desktop.kt`, `PlatformStorage.desktop.kt`, `PlatformVoice.desktop.kt`, `PlatformWorkspace.desktop.kt`, `WindowsChrome.kt`, `WindowsMemory.kt`, `WindowsPowerLease.kt`. |
| `androidMain` | `MainActivity.kt`, `LongDecodeForegroundService.kt`, `ParchmentAccessibilityService.kt`, `OtaInstallReceiver.kt`, `ParchmentFileProvider.kt`, and `platform/`: `AndroidComputerControl.kt`, `AndroidOwnedShell.kt`, `AndroidPlatformHost.kt`, `PlatformPickers.android.kt`, `PlatformRuntime.android.kt`, `PlatformStorage.android.kt`, `PlatformVoice.android.kt`, `PlatformWorkspace.android.kt`. |

## Core runtime flows

### State ownership and persistence

1. `Main.kt` (desktop) and `MainActivity.kt` (Android) initialize platform hosts (`DesktopPlatformHost` / `AndroidPlatformHost`) and render `ParchmentApp`, which binds Compose snapshot state in `ParchmentAppController` and `ParchmentStatePersistence`.
2. Initial hydration loads metadata and chat stubs first so startup stays fast even with large histories; full chat bodies hydrate on demand and persist incrementally through `ParchmentDiskWriter` backed by `AtomicFileStorage` / `SnapshotFileStorage`.
3. Data roots:
   - **Desktop packaged / Windows user install:** `%LOCALAPPDATA%\ParchmentAI`
   - **Desktop development (`:composeApp:run`):** `composeApp/parchment-portable-data` (override with `--user-dir`)
   - **Android:** `Context.filesDir`
   - **Tests:** isolated temporary directories (`TemporaryFolder` / `createTempDirectory`) so dev or installed user data is never touched.

### Generation, streaming and tool loop

1. Sending or regenerating from `ManuscriptViewport` / `ComposerControls` delegates through `ChatRunHosts` (`GenerationRun` for primary replies, `StorySummaryRun` for memory summaries) and `AgentRunCoordinator` so background chats continue running if the user switches tabs.
2. `AiProviderEngine` selects the wire implementation from provider protocol (`OpenAiResponsesProtocol`, `OpenAiChatProtocol`, `AnthropicMessagesProtocol`, `GeminiInteractionsProtocol`, `GeminiGenerateContentProtocol`), prepares history via `ProviderContextHistory` and `ProviderReasoning` (preserving opaque `GeminiThoughtSignature` blobs), and streams chunks into `GenerationStreamSession` and `GenerationStreamAccumulator`.
3. Tool calls route through `ProviderToolExecution` -> `AgentPermissionGate` & `ToolApprovalBroker` -> built-in `WorkspaceHarness` tools, `Subagents` (`agent_delegate`), `ComputerUse`, or `McpClient` servers (`mcp__*`). Approval prompts and `ask_user` cards suspend safely and resume in-place across retries or interrupted-run recovery (`InterruptedRuns`).
4. Usage tokens and estimated spend flow from `ProviderUsage` and `UsageCharges` into `SessionSpend`, `UtilityUsage` and `MessageTurnFooter`.

## Testing, verification and quality gates

Never run Gradle directly or in parallel; use `python scripts/verify.py`:

| Command | When to run |
|---|---|
| `python scripts/verify.py status` | Start of every session: prints git status, unverified files, last pass and `TODO.md` summary |
| `python scripts/verify.py review` | Docs-only or non-code changes: runs `scripts/quality.py` only |
| `python scripts/verify.py affected` | End of every batch: always runs `quality.py`, runs desktop tests when Kotlin/build inputs changed, adds Android compile when Android-reachable code changed, skips clean targets |
| `python scripts/verify.py test <Filter>` | Fast inner loop for targeted Kotlin test classes or patterns (for example `UiGalleryTest` to render gallery screenshots) |
| `python scripts/verify.py android` | Explicit Android compile check |
| `python scripts/verify.py perf` | Synthetic 300-chat startup and frame-budget check (`scripts/perf.py`) |
| `python scripts/verify.py final` | Release builds (`bundle`, Gradle/R8/signing/packaging edits) with the private signer loaded |
| `python -m unittest scripts/test_workflow_tools.py` | When modifying `scripts/verify.py` or `scripts/quality.py` |

Conventions enforced by tests and quality gates:

- **Search portably:** use `git grep -n` (Windows environments may not have `rg` installed).
- **UI tests:** use Compose Skiko `ImageComposeScene` on the AWT EDT (`runBlocking(Dispatchers.Swing)` with `Dispatchers.Swing` / `Dispatchers.Swing.immediate` so tests serialize with Compose Desktop's internal `SwingDispatcher`), calling `Snapshot.sendApplyNotifications()` after mutating state before rendering. See `desktopTest/kotlin/io/parchment/ai/ui/` (`AgentPermissionUiTest.kt`, `TurnUsageUiTest.kt`, `UiBehaviorRegressionTest.kt`, `KeyboardShortcutTest.kt`, `SharedActionSemanticsUiTest.kt`, `UiGalleryTest.kt`, `PerformanceBaselineTest.kt`).
- **Network tests:** use local HTTP/SSE fixtures (`desktopTest/network/`, including `StreamRecoveryHttpTest.kt`, `McpEndToEndTest.kt`, `ModelRegistrySyncTest.kt`), never live endpoints.
- **Static checks (`scripts/quality.py`):** enforces LF line endings (except `.bat`/`.iss`), UTF-8 without BOM, ASCII `.ps1` scripts, no unused imports or dead code outside `scripts/quality-baseline.json`, 3000-line limit per main Kotlin file, banned APIs (`androidx.compose.material`, `GlobalScope`, `println(`, `FIXME`, `TODO(`, `.clickable(enabled = false)`), valid relative Markdown links and `#heading` anchors, no tracked secrets, and strict doc byte budgets (`AGENTS.md`, `MEMORY.md`, `TODO.md`).

## Where to change what

| Task | Start here |
|---|---|
| Add or edit visible UI text | `commonMain/.../strings/Strings.kt` (check wording rules in [docs/DESIGN.md](DESIGN.md)) |
| Change colors, spacing, radii, strokes, motion or overlay scrims | `commonMain/.../ui/DesignTokens.kt`, `ui/ParchmentTheme.kt`, [docs/DESIGN.md](DESIGN.md) |
| Add a vector icon | `commonMain/.../ui/KissIconGlyphs.kt` and `ui/ParchmentIcons.kt` (24-unit grid) |
| Change chat layout, composer, shortcuts or modals | `ui/ParchmentApp.kt`, `ui/ManuscriptViewport.kt`, `ui/ComposerControls.kt`, `ui/ParchmentModalsAndNavigation.kt`, `ui/CommandPalette.kt`, `ui/AppShortcuts.kt` |
| Change message rendering (prose, code, LaTeX, SVG, diffs, media) | `ui/MessageTurnItem.kt`, `ui/ProseRenderer.kt`, `ui/LatexMathRenderer.kt`, `ui/SvgSceneRenderer.kt`, `ui/ParchmentDiffViewer.kt`, `ui/FileImages.kt` |
| Change agent run lifecycle or background tabs | `ui/ParchmentAppController.kt`, `ui/ChatRunHosts.kt`, `ui/GenerationRun.kt`, `ui/AgentRunCoordinator.kt` |
| Add or modify a provider protocol or stream parser | `network/AiProviderEngine.kt` + matching `*Protocol.kt` (`OpenAi*`, `Anthropic*`, `Gemini*`), `ProviderContextHistory.kt`, `ProviderUsage.kt` |
| Add or modify a built-in agent tool | `harness/WorkspaceToolCatalog.kt`, `harness/WorkspaceHarness.kt`, `network/ProviderToolExecution.kt`, `harness/AgentPermissionGate.kt` |
| Change workspace security, SSRF or atomic file I/O | `jvmShared/platform/` (`WorkspacePathGuard.kt`, `PublicWebTransport.kt`, `AtomicFileStorage.kt`, `SnapshotFileStorage.kt`) + [MEMORY.md](../MEMORY.md) |
| Change chat persistence, imports or `.parchment` archives | `ui/ParchmentStatePersistence.kt`, `ui/ParchmentDiskWriter.kt`, `protocol/CardAndPromptCodec.kt`, `jvmShared/platform/ChatArchive*.kt` |
| Change Windows desktop chrome, power/memory or owned shell | `desktopMain/WindowKeys.kt`, `desktopMain/Main.kt`, `desktopMain/platform/{WindowsChrome,WindowsMemory,WindowsPowerLease,DesktopOwnedShell,DesktopPlatformHost}.kt` |
| Change Android activity, foreground runs, OTA receiver or computer use | `androidMain/{MainActivity,LongDecodeForegroundService,ParchmentAccessibilityService,OtaInstallReceiver,ParchmentFileProvider}.kt`, `androidMain/platform/*` |

## Releases

Versioning and changelogs are automated with `release-please` (`.github/workflows/release-please.yml`, `release-please-config.json`, `.release-please-manifest.json`). Conventional Commits of type `feat:`, `fix:` and `perf:` advance the release PR and [CHANGELOG.md](../CHANGELOG.md); `docs:`, `chore:`, `ci:`, `refactor:` and `test:` are hidden in `release-please-config.json` and do not open a release PR. Merging the release PR creates the version tag, which triggers `.github/workflows/build-installers.yml` to build signed desktop (`.exe`) and Android (`.apk`) artifacts when secrets are present. Signed release setup lives in [docs/RELEASE_SIGNING.md](RELEASE_SIGNING.md); unsigned local builds are covered in [CONTRIBUTING.md#build-and-test](../CONTRIBUTING.md#build-and-test).
