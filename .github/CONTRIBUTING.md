# Contributing to YL Axiom

Thanks for helping. This page covers principles, dual-stack setup (Rust Desktop + Kotlin Android), and verification commands. [AGENTS.md](../AGENTS.md) holds the binding rules and stable contracts for people and AI agents alike.

## Principles

1. **Minimal, direct code.** Prefer the smallest flat implementation with optimal complexity and zero unnecessary abstractions or dependencies.
2. **Native UI only.** Windows desktop uses native Rust + GPUI (`crates/parchment-desktop`); Android uses Jetpack Compose Foundation (`compose.runtime`, `compose.foundation`, `compose.ui`). No Electron, WebView, or Material.
3. **Our own icons.** Vector paths on a 24-unit grid (`KissIconGlyphs.kt` / `ParchmentIcons.kt` and GPUI SVG assets), never OS emoji. See [docs/design.md](../docs/design.md).
4. **Nothing hardcoded about models or devices.** Discover capabilities and limits from providers and public registries; blank generation settings are omitted, and `0` means unlimited.
5. **Fits every screen & safe by default.** Verify compact phone widths (`< 600 dp`) and wide desktop windows; require explicit tool approval, revoke tool authority on imports, and never drop user data.

## Setup

### Rust Desktop (Windows GPUI & shared core)

| Tool | Get it | Notes |
|---|---|---|
| Rust (`1.100.0` stable, edition 2024) | [rustup.rs](https://rustup.rs/) | Includes `cargo`, `rustfmt`, `clippy`, Cargo lints, and 3-day publish-age cooldown |
| Windows SDK (`fxc.exe`), `cmake`, `nasm` | Visual Studio Build Tools + NASM | Required for GPUI shaders and `aws-lc-sys` on Windows |

Run the native Windows desktop app with `cargo run --release -p parchment-desktop`.

### Kotlin Mobile (Android + slim Rust JNI `.so`)

| Tool | Get it | Notes |
|---|---|---|
| JDK 25 LTS (Temurin) | [adoptium.net](https://adoptium.net/temurin/releases/?version=25) | Set `JAVA_HOME` and add `bin` to `PATH` |
| Android SDK & NDK | [developer.android.com](https://developer.android.com/studio#command-tools) | Set `ANDROID_HOME` and `ANDROID_NDK_HOME`; install `platform-tools`, `platforms;android-37.0`, `build-tools;37.0.0`, and NDK (local `30.0.16248370`) |
| Android Rust target & `cargo-ndk` | `rustup target add aarch64-linux-android` && `cargo install cargo-ndk --version 4.1.2 --locked` | Builds the 16 KB-aligned `arm64-v8a` `libparchment_core.so` |

Gradle and Kotlin need no separate install (`gradlew` / `gradlew.bat` fetches the pinned wrapper). Enable the local git pre-commit hook once after cloning:

```sh
cargo xtask install-hooks
```

## Find your way

[docs/architecture.md](../docs/architecture.md) maps every Rust crate, Gradle source set, runtime flow, and lookup table. Read [AGENTS.md](../AGENTS.md#stable-contracts) before changing storage, protocols, permissions, or platform code, and search portably with `git grep -n`.

## Build, lint, and test

`cargo xtask` (`crates/parchment-xtask`, zero external dependencies) is the single build, lint, test, and benchmark entry point for both stacks. It prevents concurrent Gradle/Cargo runs and remembers verified file hashes.

| Need | Command |
|---|---|
| Session status, pending checks, and `TODO.md` | `cargo xtask status` |
| Auto-format Rust (`rustfmt`) and Kotlin (`ktlintFormat`) | `cargo xtask fmt` |
| Fast static checks (`rustfmt --check`, encoding, dead code, links, doc budgets) | `cargo xtask review` |
| Verify changed targets (ledger-backed Rust and/or Kotlin + `ktlintCheck`) | `cargo xtask affected` |
| Full Rust gate (`rustfmt`, `clippy -D warnings`, workspace + slim `android-jni` tests) | `cargo xtask rust` |
| Kotlin formatting & lint gate (`ktlintCheck`) | `cargo xtask ktlint` |
| Build & verify 16 KB-aligned Android `arm64-v8a` `libparchment_core.so` | `cargo xtask android-so` |
| Targeted Kotlin JVM/UI tests (e.g. `UiGalleryTest`) | `cargo xtask test '*SessionSpendTest'` |
| Compile Android Kotlin sources | `cargo xtask android` |
| Desktop `.exe` + Android APK/phone benchmarks | `cargo xtask perf` ([docs/performance.md](../docs/performance.md)) |
| Workspace build/cache disk usage & stale artifact cleanup | `cargo xtask disk` / `cargo xtask prune` |
| Full release gate (review, Rust, `.so`, desktop `.exe`, `ktlint`, Kotlin tests, signed APK) | `. .\scripts\load-private-signer.ps1`, then `cargo xtask final` |

Pass `--dry-run` to preview commands. Full Gradle logs live in `.parchment-local/verification/`.

## Submitting a change

- Keep each change focused on one behavior, with a regression test for bug fixes.
- Run `cargo xtask affected` (or `cargo xtask final` for build/packaging/signing edits) and confirm all checks pass.
- Use Conventional Commits (`feat:`, `fix:`, `perf:` drive `release-please` and `CHANGELOG.md`).
- Update [TODO.md](../TODO.md) if you finish or uncover work, and report only checks actually run.

Release signing is covered in [docs/release-signing.md](../docs/release-signing.md).
