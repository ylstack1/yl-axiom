# Parchment performance

Parchment is built for low-end hardware first and 120Hz+ smoothness on modern displays. Measure with `python scripts/verify.py perf`; it builds the ProGuard-shrunk desktop release package (including its prebuilt JDK 25 Project Leyden AOT cache), runs it against 300 synthetic chats in a scratch folder (never your data) and, when a phone is connected, measures the Android app already installed on it (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets

| Measure | Budget | Before (v2.0.70) | Now |
|---|---|---|---|
| Desktop start to chat title, 300 chats | 1.0 s | 1.65 s (3.1 s without class archive) | 0.90 s (903 ms); 0.88 s (884 ms) for the first launch after an update (was 17.0 s, 0 s installer wait; 1.12 s unindexed fresh) |
| Desktop idle private memory | < 200 MB | about 306 MB (dev run) | 197.5 MB (Generational Shenandoah, sub-1 ms concurrent GC pauses) |
| Android APK | 5 MB | 15.5 MB | 4.4 MB (R8 Full Mode + resource shrinking + Baseline & Startup Profiles) |
| Android cold start | 0.5 s | not measured | needs a phone |
| Android idle PSS | < 100 MB | not measured | needs a phone |
| Frame time while streaming, p95 | 8 ms desktop (120Hz), 16 ms low-end Android (60Hz) | not measured | 5.4 ms for the prose renderer alone (p50 3.1, CPU drawing, `FrameTimeProbeTest`); 8.4 ms in the full app with a 200-message chat (`LiveTurnStreamingTest`); 0 hover/press recompositions (`KissMotionZeroRecompositionTest`) |

"Now" is the 2026-11-17 ProGuard-shrunk release measurement on an i5-1135G7 with 7.7 GB. Packaging normalizes all packaged file timestamps to an even-second zero-nanosecond UTC epoch before generating `$APPDIR/parchment.aot` (`-XX:AOTCacheOutput` with `-Dskiko.library.path=$APPDIR` and Inno Setup `TimeStampsInUTC=yes`), so installed `.EXE` releases map AOT-linked Compose, Skia, and app classes across every timezone without post-install warmup. Budgets were lowered on 2026-11-17 with the user's agreement to target near-instant startup, memory residency, and 120Hz/60Hz streaming. Only change a budget with the user's agreement, and record why.

### Measured decisions (2026-11-17)

- **Sub-1.0 s startup & sub-200 MB idle RAM with Generational Shenandoah + Project Leyden AOT cache.** Serial GC kept idle memory at 218.8 MB and startup at 1.46 s, but stopped application and UI render threads during collections (up to ~400 ms under heavy streaming or large attachments). Benchmarking JDK 25 collectors with Compact Object Headers (`-XX:+UseCompactObjectHeaders`) on the 300-chat package:
  - **ZGC (`-XX:+UseZGC`):** 193.4 MB idle RAM, but disables CompressedOops, turning off AOT class linking and pushing 300-chat startup to 2.88 s.
  - **Generational Shenandoah (JEP 521, `-XX:+UseShenandoahGC -XX:ShenandoahGCMode=generational`):** keeps CompressedOops active so `parchment.aot` maps AOT-linked classes, archived heap objects, and the full module graph. Passing `-Dskiko.library.path=$APPDIR` during `--cds-warmup` grew `parchment.aot` from `21.7 MB` to `51.7 MB` (archiving all Compose/Skia/UI classes), and pairing Frame-1 window-caption pre-seeding, off-EDT Skia/DirectWrite/font/icon preload, lazy `getData` font loaders, `-Xms8m -Xmx224m -XX:SoftMaxHeapSize=16m -XX:ShenandoahUncommitDelay=300 -XX:+ExplicitGCInvokesConcurrent`, C1-only JIT (`-XX:TieredStopAtLevel=1 -XX:ReservedCodeCacheSize=24m`, cutting `23.5 MB` of C2 compiler arena commit), `-Dsun.java2d.d3d=false`, `-Dskiko.rendering.laf.global=false`, and `-Dskiko.gpu.resourceCacheLimit=8388608` brought 300-chat startup from `1.93 s -> 0.90 s` (`0.88 s` first launch) and idle private RAM from `240.1 MB -> 197.5 MB` while keeping GC pauses sub-millisecond.
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

Measured against GitHub Actions runs on `ubuntu-latest` / `windows-latest` (2026-11-17) and guarded by `.github/scripts/test_ci_changes.py`:

| Workflow / Job | Baseline (2026-11-17) | Budget (Incremental / PR) | Budget (Cold / Release) | Key invariants |
|---|---|---|---|---|
| **Build Android APK & Windows EXE (wall clock)** | `341–365 s` (peak `432 s`) | **`<= 180 s`** | **`<= 240 s`** | `ParchmentBuildConfig` reads `/parchment-build.properties` so version bumps never bust Kotlin compile caches |
| — `preflight` | `16–21 s` | **`<= 12 s`** | **`<= 12 s`** | Quality gates, diff check, and workflow unit tests run concurrently |
| — `test-desktop` (2 shards) | `126–180 s` (`59 s` skew) | **`<= 90 s`** | **`<= 125 s`** | Runtime-cost sharding (`shard_tests.py`, `< 5 s` local skew), `-Xmx5g`, `--no-configuration-cache` |
| — `build-android-apk` | `217–304 s` | **`<= 90 s`** | **`<= 165 s`** | Caches only `platforms/android-37*` & `build-tools/37*` (`~60 MB`, never `1.2 GB` `35*`/`36*`); static license hashes (`0.01 s`); release-only SBOM |
| — `build-windows-exe` | `264–326 s` | **`<= 120 s`** | **`<= 195 s`** | `-Xmx5g`, `LZMANumBlockThreads=4`, `lzma2/fast` on PRs / `lzma2/max` on releases |
| **Quality signals (PR wall clock)** | `433 s` (`sbom` `294 s`, `coverage+gallery` `420 s`) | **`<= 180 s`** | **`<= 210 s`** | Parallel uninstrumented `ui-gallery` (`<= 65 s`, UI-path gated) + Kover `coverage`; read-only SDK restore (`sbom <= 50 s`) |
| **CodeQL (wall clock)** | `513 s` (`183 s` SDK post-cache) | **`<= 270 s`** (`0 s` test-only) | **`<= 300 s`** | Read-only SDK 37 restore, `-Xmx5g`, skips `java-kotlin` on `commonTest`/`desktopTest`-only commits |
| **Dependency submission** | `130 s` (`83 s` SDK post-cache) | **`<= 45 s`** | **`<= 60 s`** | Read-only SDK 37 restore + static license hashes |
