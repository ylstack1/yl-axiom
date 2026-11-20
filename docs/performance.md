# Parchment performance

Parchment is built for low-end hardware first and 120Hz+ smoothness on modern displays. Measure both targets with `cargo xtask perf`; it builds and benchmarks the standalone native GPUI Windows `.exe` against 300 synthetic chats + an 87.6 KB live streaming code-fence stress test in a scratch folder (never your data), checks Android `libparchment_core.so` & APK size budgets, and measures the installed Android app over `adb` when a phone is connected (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets & measured floors

| Measure | Budget | v2 Baseline (2026-11-18) | v3 Pre-Audit (2026-11-19) | v3 Post-Audit & Refactor (2026-11-20) |
|---|---|---|---|---|
| Windows standalone `.exe` footprint | < 35 MB | 173.5 MB (JLink + Leyden) | 22.61 MB (minimal shell) | **27.82 MB** (0 DLLs; full Kotlin-parity UI, subsetted fonts, `base64 0.23.1` SIMD, `opt-level="z"` UI crate) |
| Desktop start to chat title, 300 chats | < 0.35 s | 0.77 s (`774 ms`) | 0.27 s (`249 ms` win / `271 ms` title; `4.46 ms` core) | **0.27 s** (`244 ms` win / `271 ms` title; **`2.55 ms` core**, -42.8%) |
| Desktop idle private memory (maximized 1080p) | < 140 MB | 194.3 MB (Shenandoah + Skia) | 116.6 MB (windowed minimal UI) | **136.5 MB** (full maximized DirectComposition UMA swapchain + virtualized `uniform_list` sidebar; down from 158.0 MB) |
| Frame time while streaming (200-msg chat, 87.6 KB code fence), p95 | < 4 ms desktop (120Hz), 16 ms Android | 6.8 ms (`max 27.0 ms`, Kotlin) | 0.360 ms CPU p95 (minimal shell) | **0.622 ms CPU p95** (`p50 0.361 ms`, `vsync_p50 16.66 ms`, `vsync_p95 17.42 ms`, `vsync_max 18.43 ms`, 0 dropped frames) |
| 78 KB streaming code-fence prose parse (Rust) | < 5 ms / < 1 MB alloc | 97 ms / 386 MB (Kotlin) | 1.57 ms / 365.0 KB | **1.64 ms / 365.0 KB alloc** (>1,000x less alloc churn) |
| Streaming checkpoint / 1,000-msg save | < 4 ms | 198 ms full / 4.9 ms sidecar | 3.9 ms Kotlin live p95 | **< 1 ms** Rust SQLite WAL upsert; **3.3 ms** Kotlin `CheckpointCostProbeTest` p95 (`5.6 KB` written) |
| Bundled fonts (10 TTF files) | < 2.5 MB | 3.81 MB (`3,808 KB`) | 3.81 MB (`15 ms` read) | **1.95 MB (`1,947 KB`, -48.9%)**, `7 ms` read (OFL-compliant subsetting, full prose/code/math coverage) |
| Android signed Release APK | <= 7.5 MB | 4.5 MB (Kotlin only) | 6.30 MB (with `1.85 MB` `.so`) | **5.39 MB** (**-0.91 MB / -14.4%**; R8 Full Mode + `1.85 MB` 16 KB-aligned `libparchment_core.so`) |
| Kotlin UI streaming & typing probes (`FrameTime` / `Keystroke` / `LiveTurn`) | < 12 ms p95 | 3.9 / — / 7.6 ms | 5.4 / 6.4 / 8.9 ms p95 | **3.7 ms** (`max 7.0 ms`) / **5.0 ms** / **6.8 ms** p95 |
| Android cold start / idle PSS | 0.5 s / < 100 MB | needs a phone | needs a phone | needs a phone |

Measure both v3 desktop and Android with `cargo xtask perf` (`--android-install` to push the local signed APK first). Release profiles live in `Cargo.toml` (`[profile.release]`, `[profile.release.package.parchment-desktop]`, `[profile.release-android]`) and `mobile/android-app/build.gradle.kts` (R8 Full Mode + resource shrinking).

### Verified OS, GPU & framework floors

- **Windows cold window creation (`~240–245 ms`):** `parchment-core` SQLite WAL + 300-chat stub load takes **`2.42 ms`**; the remaining `~240 ms` is Win32 process creation, DirectWrite primary font load, and Direct3D 11 / DirectComposition swapchain initialization.
- **Windows desktop 60Hz VSync cadence (`16.65 ms` p50 / `17.88 ms` max):** `desktop_stream_bench` spends **`0.386 ms` p50 / `0.628 ms` p95** on CPU per frame; wall-clock pacing is purely display DWM VSync with zero dropped frames.
- **Windows maximized idle private RAM (`~134.7 MB`):** On Intel Iris Xe UMA (1920×1080 maximized DirectComposition swapchain + DirectWrite glyph atlas), `~80–90 MB` is GPU/driver backing surfaces and `~45 MB` is GPUI + Rust heap. Virtualizing the 300-chat sidebar via `gpui::uniform_list` and caching per-frame context token breakdowns dropped idle RAM from `158.0 MB` to `134.7 MB` and streaming CPU p95 from `2.997 ms` to `0.628 ms`.

### Historical v2 JVM baseline notes (2026-11-18)

Parchment v2 (JVM Compose Desktop) used a single `DEFLATED` uber-jar, C1-only JIT, Generational Shenandoah, and a Project Leyden AOT cache (`173.5 MB` runtime, `774 ms` startup, `194.3 MB` idle RAM). Parchment v3 replaces the JVM desktop runtime with the standalone Rust GPUI binary (`27.78 MB`, `270 ms` startup, `134.7 MB` maximized idle RAM) while retaining the shared Kotlin Compose Foundation UI for Android (`:composeApp:desktopTest` still runs JVM/Skiko UI probe tests).

## What keeps start-up, frame times, and memory fast

- **Deferred cold initialization (Rust GPUI desktop):** Frame 1 registers only primary UI fonts (`Inter-Regular`, `JetBrainsMono-Regular`); secondary italic/bold/math fonts register after Frame 1, `PublicWebTransport` initializes lazily via `OnceLock`, and background model catalog / MCP sync defers `1,200 ms` past first paint.
- **Virtualized lists & cached hot paths (Rust GPUI + Kotlin):** Both the chat viewport (`MessageScroller` / `LazyColumn`) and 300-chat sidebar (`gpui::uniform_list` / `LazyColumn`) lay out only visible rows; `cached_context_breakdown` caches per-frame token counts via an FNV-1a session/draft fingerprint so 60/120Hz frames perform zero tokenizer scans; `parchment-prose` parses streaming Markdown with cached prefix splits (`1.64 ms` / `365 KB` on 78 KB code fences).
- **Bounded SQLite WAL & workspace storage hygiene:** `parchment-storage` caps WAL growth (`PRAGMA journal_size_limit = 8388608`, `wal_autocheckpoint = 1000`), spills `> 64 KiB` tool outputs out of hot message rows, and runs `reclaim_storage()` (`wal_checkpoint(TRUNCATE)`) on bulk deletes. `cargo xtask disk` / `cargo xtask prune` monitor and reclaim local build/cache bloat.
- **Zero-recomposition Compose Foundation motion (Android):** `KissMotionNode` (`LayoutModifierNode` + `DrawModifierNode`) and `KissStreamingPulseDot` (`graphicsLayer`) animate hover/press/focus/pulse with `0` caller recompositions (`KissMotionZeroRecompositionTest`); `mobile/compose-app/stability-config.conf` + `kotlinx.collections.immutable` keep `LazyColumn` slots skippable.
- **Android release slimming:** R8 Full Mode (`android.enableR8.fullMode=true`), resource shrinking, subsetted OFL fonts (`-1.86 MB`), `MainActivity.onTrimMemory` cache eviction, bundled Baseline + Startup profiles (`androidx.profileinstaller`), and slim 16 KB-aligned `libparchment_core.so` (`1.85 MB`).
- **Prompt-cache & agent harness efficiency:** Tool schemas stay ahead of conversation history for provider prompt-cache hits; historical tool compaction quantizes in 16-step batches; read-only tool calls execute in parallel; `SseHotPathProbeTest` holds `~265k` events/s and `CheckpointCostProbeTest` writes `5.6 KB` (`3.3 ms` p95) on 1,000-message chats.
## CI workflow budgets

Measured against GitHub Actions runs on `ubuntu-latest` / `windows-latest` and enforced via `crates/parchment-xtask/src/ci.rs` (`cargo xtask ci ...`):

| Workflow / Job | Budget (Incremental / PR) | Budget (Cold / Release) | Key invariants |
|---|---|---|---|
| **Build Android APK & Windows EXE** | **`<= 145 s`** | **`<= 230 s`** | Tight path filters (`parchment-protocol` skips Android, `parchment-xtask` skips Windows EXE); `cache-mode` scoping; concurrency cancels superseded branch/PR runs |
| — `preflight` | **`<= 15 s`** | **`<= 15 s`** | Pure-Rust zero-dep `parchment-xtask` (`cache-mode: none`): unit tests + static review + `rustfmt --check` + path planner + uploads prebuilt Linux binary for downstream jobs |
| — `rust-core-check` | **`<= 40 s`** | **`<= 100 s`** | `Swatinem/rust-cache` (`save-if: main`, `cache-bin: false`), `clippy -D warnings`, workspace & slim `android-jni` tests (parallel with Kotlin; no duplicate `rustfmt`) |
| — `test-desktop` (2 shards) | **`<= 75 s`** | **`<= 95 s`** | Reuses prebuilt `parchment-xtask` (no Rust install), lead-overhead-balanced sharding (`--lead-overhead 80000` for shard 0 `ktlintCheck`), `-Pparchment.testForks=3` |
| — `test-windows-native` | **`<= 100 s`** (parallel) | **`<= 110 s`** (parallel) | Parallel Windows-native Kotlin suites (`-Pparchment.testForks=2`), read-only Gradle cache on PRs, skipped on pure Rust/`installer/` edits |
| — `build-android-apk` + parallel `release-sbom` | **`<= 115 s`** | **`<= 200 s`** | Cached `cargo-ndk` + `Swatinem/rust-cache` (`save-if: main`) builds 16 KB-aligned `.so`, stable SDK 37 caches (`android-platform-37-v3`), R8 APK `<= 7.50 MB`, CycloneDX `3.6.1` |
| — `build-windows-exe` | **`<= 115 s`** | **`<= 205 s`** | `Swatinem/rust-cache` (`save-if: main`, `cache-bin: false`), release GPUI build + test + `clippy`, standalone `.exe` `<= 35 MB`, Inno Setup (`lzma2/fast` PR / `lzma2/max` release) + smoke install |
| **Quality signals (PR wall clock)** | **`<= 115 s`** (`0 s` pure Rust) | **`<= 140 s`** | `cache-mode: read`, skips pure Rust PRs, reuses prebuilt `parchment-xtask`, parallel Kover `coverage`, `UiGalleryTest`, and CycloneDX `3.6.1` SBOM validation |
| **CodeQL (wall clock)** | **`<= 165 s`** (`0 s` test-only) | **`<= 185 s`** | `cache-mode: read`, reuses prebuilt `parchment-xtask` for `java-kotlin` manual compile, `rust` (`build-mode: none`), and `actions` |
| **Dependency submission** | **`<= 35 s`** | **`<= 40 s`** | `cache-mode: read`, stable SDK 37 platform cache (`android-platform-37-v3`, `~65 MB`) + static license hashes |
