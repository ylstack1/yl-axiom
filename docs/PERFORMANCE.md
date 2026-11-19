# Parchment performance

Parchment is built for low-end hardware first and 120Hz+ smoothness on modern displays. Measure both targets with `cargo xtask perf`; it builds and benchmarks the standalone native GPUI Windows `.exe` against 300 synthetic chats + an 87.6 KB live streaming code-fence stress test in a scratch folder (never your data), checks Android `libparchment_core.so` & APK size budgets, and measures the installed Android app over `adb` when a phone is connected (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets

| Measure | Budget | v2 Baseline (2026-11-18) | v3 Native Rust + GPUI / JNI (2026-11-19) |
|---|---|---|---|
| Windows binary / runtime footprint | < 35 MB | 173.5 MB (JLink + Leyden AOT image) | **22.61 MB** standalone `.exe` (0 DLLs; 7.7x smaller) |
| Desktop start to chat title, 300 chats | < 0.35 s | 0.77 s (`774 ms` cold / `865 ms` warm) | **0.26 s** (`250 ms` window / `261 ms` title; `3.47 ms` core startup; 3.0x faster) |
| Desktop idle private memory | < 130 MB | 194.3 MB (Shenandoah + Skia D3D12) | **116.6 MB** (`~38 MB` heap + `~78 MB` Intel Iris Xe UMA swapchain; 40% lower) |
| Frame time while streaming (200-msg chat, 87 KB code fence), p95 | < 4 ms desktop (120Hz), 16 ms Android | 6.8 ms (`max 27.0 ms`, `LiveTurnStreamingTest`) | **0.360 ms CPU p95** (`p50 0.168 ms`, `vsync_p95 17.34 ms`, `vsync_max 18.09 ms`, `0` dropped frames; 19x faster) |
| 78 KB streaming code-fence prose parse | < 5 ms / < 1 MB alloc | 97 ms / 386 MB alloc churn | **1.55–1.72 ms / 371 KB alloc** (>1,000x less alloc churn, ~60x faster) |
| Streaming checkpoint / 1,000-msg save | < 2 ms | 198 ms / 19.3 MB full JSON (`4.9 ms` sidecar) | **< 1 ms** SQLite WAL incremental upsert (zero-clone borrowed meta serialization) |
| Android signed Release APK | <= 7.5 MB | 4.5 MB (Kotlin only) | **6.30 MB** (R8 Full Mode + `1.85 MB` uncompressed 16 KB-aligned `libparchment_core.so`, `useLegacyPackaging=false`) |
| Android cold start / idle PSS | 0.5 s / < 100 MB | needs a phone | needs a phone |

Measure both v3 desktop and Android with `cargo xtask perf` (`--android-install` to push the local signed APK first). Release profiles live in `Cargo.toml` (`[profile.release]`, `[profile.release-android]`) and `androidApp/build.gradle.kts` (R8 Full Mode + resource shrinking).

### Measured decisions (2026-11-18)

- **Sub-0.85 s startup & sub-195 MB sustained RAM with single `DEFLATED` uber-jar, C1-only JIT, Generational Shenandoah, and Project Leyden AOT cache.** Serial GC kept idle memory at 218.8 MB and startup at 1.46 s, but stopped application and UI render threads during collections (up to ~400 ms under heavy streaming or large attachments). Benchmarking JDK 25 collectors with Compact Object Headers (`-XX:+UseCompactObjectHeaders`) on the 300-chat package:
  - **ZGC (`-XX:+UseZGC`):** 193.4 MB idle RAM, but disables CompressedOops, turning off AOT class linking and pushing 300-chat startup to 2.88 s.
  - **Generational Shenandoah (JEP 521, `-XX:+UseShenandoahGC -XX:ShenandoahGCMode=generational`):** keeps CompressedOops active so `parchment.aot` maps AOT-linked classes, archived heap objects, and the full module graph. Live profiling of a 3-hour `9.1 MB` agent session revealed two hidden regressions in `v2.5.2`: (1) `-XX:SoftMaxHeapSize=16m` + `-XX:ShenandoahUncommitDelay=300` was below the real `51 MB` live heap, causing Shenandoah GC threads to spin at `84.3%` of total app CPU (`108.6%` core usage + `382 s` VM safepoints); removing `SoftMaxHeapSize=16m` and setting `-XX:ShenandoahUncommitDelay=5000 -XX:ShenandoahControlIntervalMin=50 -XX:ShenandoahControlIntervalMax=900 -XX:ShenandoahGuaranteedGCInterval=0` dropped idle GC CPU to `0 ms` (`0.5%` total idle CPU); and (2) uncapped Skia Ganesh D3D12 resource cache let D3D12MA escalate through `32+64+128 MB` permanent `PAGE_WRITECOMBINE` pools (`259.3 MB` GPU / `353 MB` Task Manager RAM); capping `skiko.gpu.resourceCacheLimit` at `48 MiB` (`32 MiB` on `< 4 GiB` PCs) preserves glitch-free 4K/DirectWrite atlases while preventing runaway GPU pool bloat. Pairing single `DEFLATED` `parchment-all.jar`, deferred stub open for `> 512 KB` active chats (`9 MB` cold start `1,340 ms -> 822 ms`), deduplicated/trimmable `ModelsDevClient` catalog specs, C1-only JIT (`-XX:TieredStopAtLevel=1`), and decoupled VSync `GenerationRun` prose reveals (`0 ms` hold up to `32 KB`, `16 ms` = `60 Hz` up to `128 KB`, independent of `thinkingSummary` length) brought 300-chat idle RAM to `193.8 MB` and full-app streaming p95 to `6.8 ms`.
- **True 120Hz VSync SSE streaming, whole-second stopwatch & skippable prefix prose blocks.** `GenerationRun` preserves hardware VSync letter-by-letter reveals (`nextFrameNanos()` per frame) while throttling multi-kilobyte `thinkingSummary`/`trailingThinking` copies via `visibleTextCopyIntervalMs` and advancing whole-second durations (`30s`, `20m21s`) cleanly once per second. `KissStreamingPulseDot` animates smoothly in `graphicsLayer` with `0` caller recompositions (`KissMotionZeroRecompositionTest`), `ProseBlock` is `@Immutable` with skippable `ProseBlockItem` scopes (`7.6 ms` full-app 200-message streaming p95 in `LiveTurnStreamingTest`; `3.9 ms` renderer p95 in `FrameTimeProbeTest`), and `ParchmentSidebar` precomputes lowercase title search keys.
- **Zero-recomposition pointer & focus motion (`Modifier.Node`).** `Modifier.kissClickable` previously read `animateFloatAsState` values in its `@Composable` body, recomposing parent screens across 30+ spring frames on every mouse hover or press. Moving hover lift, press squeeze, and keyboard-only focus rings into `KissMotionNode` (`LayoutModifierNode` `placeWithLayer` + `DrawModifierNode`) reduced caller recompositions during hover/press/focus animations to `0` (`KissMotionZeroRecompositionTest`), and `SepiaTextField`, `SepiaToggleRow`, `SepiaProgressBar`, and `SepiaContextGaugeRing` likewise defer animated color/float reads to draw and graphics-layer lambdas.
- **Strong Skipping stability & LazyColumn slot reuse.** External and model value types are registered in `composeApp/stability_config.conf`, filtered UI lists wrap in `kotlinx.collections.immutable` (`0.5.2`), `KissVectorIcon` caches `Stroke` allocations outside `Canvas`, and all nine `LazyColumn` surfaces supply both `key` and `contentType`.
- **Retry waits, SSE hot path, and streaming checkpoints.** Server `Retry-After` hints are honored (`StreamRecoveryHttpTest`), SSE event allocations dropped `7461 -> 2980` bytes/event (`289k` events/s, `SseHotPathProbeTest`), and 3.5 s streaming checkpoints write only `chats/<id>/live.json` (`p95 4.9 ms` and `5.6 KB` on a 10 MB chat, `CheckpointCostProbeTest`).

## What keeps start-up, frame times, and memory fast

- Before the first frame, `preloadProcessStateInBackground()` reads settings, window bounds, and the open chat off the UI thread; inactive chats load as compact stubs from `chat-index.json` (checked by file size and mtime).
- Packaging builds `$APPDIR/parchment.aot` (JEP 483/514/515) against a multi-chat headless UI run and verifies `Parchment.exe --aot-verify` maps the AOT cache at build time.
- Skiko runs with hardware VSync (`skiko.vsync.enabled=true`) and Swing frame pacing (`skiko.swing.frame.pacing=true`), while Windows file, save, folder, DWM chrome, memory, and keep-awake calls run directly through JDK 25's FFM API (`IFileOpenDialog`, `IFileSaveDialog`, `WindowsNative`).
- Desktop image decoding (`DesktopImageDecoder`) reads dimensions and pixels in a single Skia `Codec` pass and downsamples oversized images before GPU upload.
- Android release builds enable R8 Full Mode (`android.enableR8.fullMode=true` + `proguard-android-optimize.txt`), resource shrinking, edge-to-edge insets, `MainActivity.onTrimMemory` bitmap/token cache eviction, and bundled Baseline + Startup (`startup-prof.txt` DEX layout) profiles applied via `androidx.profileinstaller` (`1.4.1`).
- Typing recomposes only the chat viewport; streaming replies update isolated `LiveTurns` state; token counting and clipboard media encoding run off the UI thread; wire requests keep tool schemas ahead of conversation history for provider prompt-cache hits.
- The in-process agent harness caches prebuilt `JsonObject` tool schemas (`< 8.2 KB` across all 21 tools, ~30% leaner), strips reader line numbers from auto-loaded `AGENTS.md`/`CLAUDE.md`/`SKILL.md` prompts, quantizes historical tool compaction in 16-step batches with symbol/tail receipts, partitions mixed tool steps into parallel read-only `Dispatchers.IO` sub-runs, avoids Win32 `CreateFileW` `toRealPath()` calls on verified regular files during `fs_search`, and runs Windows `shell_exec` on `pwsh` (`-OutputFormat Text`, `PYTHONUNBUFFERED=1`, up to `1800 s`).

## CI workflow budgets

Measured against GitHub Actions runs on `ubuntu-latest` / `windows-latest` and enforced via `crates/parchment-xtask/src/ci.rs` (`cargo xtask ci ...`):

| Workflow / Job | Budget (Incremental / PR) | Budget (Cold / Release) | Key invariants |
|---|---|---|---|
| **Build Android APK & Windows EXE** | **`<= 160 s`** | **`<= 240 s`** | Path-filtered per stack (`rust_core`, `rust_windows`, `desktop_tests`, `windows_tests`, `android`); concurrency cancels superseded branch/PR runs |
| — `preflight` | **`<= 15 s`** | **`<= 15 s`** | Pure-Rust zero-dep `parchment-xtask`: unit tests + static quality review + `rustfmt --check` + path planner |
| — `rust-core-check` | **`<= 45 s`** | **`<= 110 s`** | `Swatinem/rust-cache`, `rustfmt`, `clippy -D warnings`, workspace & slim `android-jni` tests (parallel with Kotlin) |
| — `test-desktop` (2 shards) | **`<= 85 s`** | **`<= 105 s`** | Deterministic `@Test` sharding (`cargo xtask ci shard-tests`), `ktlintCheck` on shard 0, `-Pparchment.testForks=3` |
| — `test-windows-native` | **`<= 105 s`** (parallel) | **`<= 115 s`** (parallel) | Parallel Windows-native Kotlin suites (`-Pparchment.testForks=2`), skipped on pure Rust/`installer/` edits |
| — `build-android-apk` + parallel `release-sbom` | **`<= 120 s`** | **`<= 210 s`** | Cached `cargo-ndk` + `Swatinem/rust-cache` builds 16 KB-aligned `libparchment_core.so`, split Android SDK 37 caches, R8 APK `<= 7.50 MB` |
| — `build-windows-exe` | **`<= 120 s`** | **`<= 210 s`** | `Swatinem/rust-cache`, full Windows `clippy` + `cargo test --workspace`, standalone `parchment-desktop.exe` `<= 35 MB`, Inno Setup (`lzma2/fast` PR / `lzma2/max` release) + silent install smoke test |
| **Quality signals (PR wall clock)** | **`<= 130 s`** | **`<= 150 s`** | Parallel Kover `coverage`, virtual-clock `UiGalleryTest`, and CycloneDX SBOM validation (`cargo xtask ci check-sbom`) |
| **CodeQL (wall clock)** | **`<= 175 s`** (`0 s` test-only) | **`<= 195 s`** | Path-filtered `java-kotlin` (manual parallel compile), `rust` (`build-mode: none`), and `actions` |
| **Dependency submission** | **`<= 40 s`** | **`<= 45 s`** | Platform-only SDK 37 restore (`~65 MB`) + static license hashes |
