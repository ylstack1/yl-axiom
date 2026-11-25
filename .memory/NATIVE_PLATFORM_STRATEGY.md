# YL Axiom — Native Platform Strategy

## Windows Desktop

### Backend
- Native Rust GPUI desktop shell: `crates/parchment-desktop/` (already implemented).
- Entry: `crates/parchment-desktop/src/main.rs`, GPUI app, window config, data dir.
- Shared Rust core via `parchment-core` `network` feature (full reqwest + tokio + ProtocolClient + RegistryCatalogClient).

### Native capabilities
- Filesystem: native Windows file access via `WorkspacePathGuard` + `tokio::fs`, canonicalize ancestor resolution, symlink/junction escape prevention.
- Process execution: `tokio::process::Command` (shell_exec), PowerShell integration via shell adapter.
- Git/dev tools/project discovery: shell-based, workspace-confinement respected.
- Cancellation: `AgentTurnRunner` abort, spend limits, timeout support.
- Process management: child process lifecycle, kill on cancel.
- Permissions: `AgentPermissionGate` + `ToolApprovalBroker` + `WorkspacePathGuard`.
- Native desktop interaction/file dialogs: Win32 COM `IFileOpenDialog`/`IFileSaveDialog` (`crates/parchment-desktop/src/win32.rs`), DWM caption tinting.

### Window title / branding
- `crates/parchment-desktop/src/strings.rs`: `APP_NAME = "Parchment"`, `window_title_for_chat`, `window_title_for_settings`, `window_title_for_stream`. Rebrand target: "YL Axiom".

### Production artifact
- Standalone native Rust GPUI Windows `.exe` (<= 35 MB budget), Inno Setup installer (`installer/windows-installer.iss`), private signer (`95d9804c...`).

### Compose desktop (dev only)
- `mobile/compose-app/src/desktopMain/`: Compose desktop JVM for dev/hot-reload (`hotRunDesktop`). Production Windows ships the Rust GPUI `.exe`, not Compose desktop.

## Android

### UI
- Native Kotlin and Jetpack Compose: `mobile/compose-app/src/commonMain/` (shared Compose UI) + `mobile/compose-app/src/androidMain/` (Android-specific).
- `MainActivity.kt`: edge-to-edge, activity result launchers, permissions, speech recognition, accessibility.

### Shared Rust engine via JNI
- `crates/parchment-core/src/jni_android.rs`: direct zero-JNA JNI bridge (`jni 0.22`, `EnvUnowned::with_env`), 950+ generated functions.
- `mobile/compose-app/src/androidMain/kotlin/io/parchment/ai/platform/ParchmentNativeCore.kt`: Kotlin side, handle caching per storage dir.
- Slim `arm64-v8a` `libparchment_core.so` (1.85 MB, 3 PT_LOAD, 16 KB aligned), built with `android-jni` feature (no network/TLS duplication in `.so`).

### Native capabilities
- Filesystem: native Android file access, Storage Access Framework (SAF) where appropriate, `PlatformStorage.android.kt`.
- Permissions: Android manifest permissions (INTERNET, WAKE_LOCK, ACCESS_WIFI_STATE, CHANGE_WIFI_STATE, REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, FOREGROUND_SERVICE, FOREGROUND_SERVICE_SPECIAL_USE, POST_NOTIFICATIONS, REQUEST_INSTALL_PACKAGES, RECORD_AUDIO, MANAGE_EXTERNAL_STORAGE, READ/WRITE_EXTERNAL_STORAGE maxSdkVersion).
- Lifecycle: `ComponentActivity`, `OnBackPressedCallback`, `LifecycleObserver`.
- Background: `LongDecodeForegroundService` (specialUse FGS, not dataSync — Android 15+ 6-hour cap), wake lock + wifi lock.
- Accessibility: `ParchmentAccessibilityService` (off until user enables; for approved agent actions / computer_use).
- Execution: Android-compatible only. `AndroidOwnedShell.kt` is the default. No Termux, PRoot, embedded Linux. No unrestricted desktop subprocess. Capability discovery + graceful fallback.
- Optional execution backend: `fastshell` (Apache-2.0) may be enabled behind a Cargo feature for 180+ built-in commands, pipelines, glob and embedded Python. It is single-process on mobile (`allow_subprocess = false`), which avoids the Android 12+ phantom-process killer. Its `exit_code = 100` / `PERMISSION_NEEDED:<resource>` result is bridged into `ToolApprovalBroker` — it never grants access on its own, and `WorkspacePathGuard` still governs paths. Must be re-measured against the 1.85 MB `.so` budget before enabling.

### Application shell
- `mobile/android-app/`: AGP application, applicationId `io.parchment.ai` (PRESERVE — do not rename), version/signing/R8, bundles fonts + `libparchment_core.so`, R8 APK <= 7.50 MB budget.

### Capability discovery & graceful fallback
- Android does NOT expose every Windows capability. Platform adapters report available capabilities; UI degrades gracefully (e.g., no stdio MCP on Android — HTTP-only, known gap).
- Do not pretend Android has unrestricted desktop subprocess capabilities.

## Shared engine
- Same Rust agent brain, contracts, task lifecycle, orchestration, core logic on both platforms (`parchment-harness` + `parchment-coding` extension).
- Platform adapters handle filesystem/shell/process/permissions/dev-tools differences.
- No Kotlin agent engine duplicate.

## Platform matrix

| Capability | Windows | Android | Notes |
|---|---|---|---|
| Native UI | GPUI (Rust) | Compose (Kotlin) | Separate shells |
| Shared agent core | Yes (network) | Yes (JNI, android-jni) | Same Rust logic |
| Filesystem access | Native Win32 | Native Android + SAF | PathGuard both |
| Process execution | `tokio::process` + PowerShell | `AndroidOwnedShell` (limited); optional fastshell | Fallback on Android; one permission authority |
| Code intelligence | tree-sitter + optional LSP | tree-sitter only | Capability-gated |
| File dialogs | Native COM dialogs | SAF / system picker | Platform-native |
| Background runs | Process lifetime | `specialUse` FGS | Android 15+ |
| Accessibility | Native | `ParchmentAccessibilityService` | Opt-in |
| MCP stdio | Yes | HTTP-only | Known Android gap |
| App ID | — | `io.parchment.ai` | PRESERVE |

## Risks
- Android shell unsandboxed (known gap) — do not weaken for coding.
- Android MCP HTTP-only — coding subagents needing stdio MCP must degrade gracefully.
- JNI handle is a raw pointer — must remain valid across Kotlin/Rust lifecycle boundaries.
- Optional fastshell backend: LGPL risk via crates.io `malachite-*` (repo uses local Apache-2.0 shims), `.so` size inflation, and permission-authority duplication — all three must be checked before enabling.
- LSP unavailable on Android — tree-sitter only, reported through capability discovery.
