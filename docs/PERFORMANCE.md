# Parchment performance

Parchment is built for low-end hardware first and 120Hz+ smoothness on modern displays. Measure with `python scripts/verify.py perf`; it builds the ProGuard-shrunk desktop release package (including its prebuilt JDK 25 Project Leyden AOT cache), runs it against 300 synthetic chats in a scratch folder (never your data) and, when a phone is connected, measures the Android app already installed on it (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets

| Measure | Budget | Before (v2.0.70) | Now |
|---|---|---|---|
| Desktop start to chat title, 300 chats | 2.0 s | 1.65 s (3.1 s without class archive) | 1.93 s; 1.92 s for the first launch after an update (was 17.0 s, 0 s installer wait) |
| Desktop idle private memory | 250 MB | about 306 MB (dev run) | 240.1 MB (Generational Shenandoah, sub-1 ms concurrent GC pauses) |
| Android APK | 8 MB | 15.5 MB | 4.4 MB (R8 Full Mode + resource shrinking + Baseline Profile) |
| Android cold start | 1.5 s | not measured | needs a phone |
| Android idle PSS | 150 MB | not measured | needs a phone |
| Frame time while streaming, p95 | 16 ms desktop (8.3 ms target at 120Hz), 33 ms low-end Android | not measured | 5.4 ms for the prose renderer alone (p50 3.1, CPU drawing, `FrameTimeProbeTest`); 8.4 ms in the full app with a 200-message chat (`LiveTurnStreamingTest`); 0 hover/press recompositions (`KissMotionZeroRecompositionTest`) |

"Now" is the 2026-11-17 ProGuard-shrunk release measurement on an i5-1135G7 with 7.7 GB. Packaging generates `$APPDIR/parchment.aot` at build time (`-XX:AOTCacheOutput`), so updates and fresh installs start immediately with AOT-linked classes, the full module graph, and JEP 515 method profiles without running a post-install warmup on the user's machine. Only change a budget with the user's agreement, and record why.

### Measured decisions (2026-11-17)

- **Sub-1 ms concurrent GC + Project Leyden AOT cache.** Serial GC kept idle memory at 218.8 MB and startup at 1.46 s, but stopped application and UI render threads during collections (up to ~400 ms under heavy streaming or large attachments). Benchmarking JDK 25 collectors with Compact Object Headers (`-XX:+UseCompactObjectHeaders`) on the 300-chat package:
  - **ZGC (`-XX:+UseZGC`):** 193.4 MB idle RAM, but disables CompressedOops, turning off AOT class linking and pushing 300-chat startup to 2.88 s.
  - **Generational Shenandoah (JEP 521, `-XX:+UseShenandoahGC -XX:ShenandoahGCMode=generational`):** keeps CompressedOops active so `parchment.aot` maps AOT-linked classes, archived heap objects, the full module graph, and JEP 515 JIT method profiles (`1.93 s` cold start, `1.92 s` first launch). Pairing `-XX:SoftMaxHeapSize=48m` and `-XX:ShenandoahUncommitDelay=1000` with a post-hydration concurrent sweep brings idle private RAM from `346.8 MB` (5-minute default uncommit) down to `240.1 MB` while keeping GC pauses sub-millisecond for 120Hz/240Hz frame pacing.
- **Zero-recomposition pointer & focus motion (`Modifier.Node`).** `Modifier.kissClickable` previously read `animateFloatAsState` values in its `@Composable` body, recomposing parent screens across 30+ spring frames on every mouse hover or press. Moving hover lift, press squeeze, and keyboard-only focus rings into `KissMotionNode` (`LayoutModifierNode` `placeWithLayer` + `DrawModifierNode`) reduced caller recompositions during hover/press/focus animations to `0` (`KissMotionZeroRecompositionTest`), and `SepiaTextField`, `SepiaToggleRow`, `SepiaProgressBar`, and `SepiaContextGaugeRing` likewise defer animated color/float reads to draw and graphics-layer lambdas.
- **Strong Skipping stability & LazyColumn slot reuse.** External and model value types are registered in `composeApp/stability_config.conf`, filtered UI lists wrap in `kotlinx.collections.immutable` (`0.5.2`), `KissVectorIcon` caches `Stroke` allocations outside `Canvas`, and all nine `LazyColumn` surfaces supply both `key` and `contentType`.
- **Retry waits, SSE hot path, and streaming checkpoints.** Server `Retry-After` hints are honored (`StreamRecoveryHttpTest`), SSE event allocations dropped `7461 -> 2980` bytes/event (`289k` events/s, `SseHotPathProbeTest`), and 3.5 s streaming checkpoints write only `chats/<id>/live.json` (`p95 4.9 ms` and `5.6 KB` on a 10 MB chat, `CheckpointCostProbeTest`).

## What keeps start-up, frame times, and memory fast

- Before the first frame, `preloadProcessStateInBackground()` reads settings, window bounds, and the open chat off the UI thread; inactive chats load as compact stubs from `chat-index.json` (checked by file size and mtime).
- Packaging builds `$APPDIR/parchment.aot` (JEP 483/514/515) against a multi-chat headless UI run and verifies `Parchment.exe --aot-verify` maps the AOT cache at build time.
- Skiko runs with hardware VSync (`skiko.vsync.enabled=true`) and Swing frame pacing (`skiko.swing.frame.pacing=true`), while Windows file, save, folder, DWM chrome, memory, and keep-awake calls run directly through JDK 25's FFM API (`IFileOpenDialog`, `IFileSaveDialog`, `WindowsNative`).
- Desktop image decoding (`DesktopImageDecoder`) reads dimensions and pixels in a single Skia `Codec` pass and downsamples oversized images before GPU upload.
- Android release builds enable R8 Full Mode (`android.enableR8.fullMode=true` + `proguard-android-optimize.txt`), resource shrinking, edge-to-edge insets, and a bundled Baseline Profile applied via `androidx.profileinstaller` (`1.4.1`).
- Typing recomposes only the chat viewport; streaming replies update isolated `LiveTurns` state; token counting and clipboard media encoding run off the UI thread; wire requests keep tool schemas ahead of conversation history for provider prompt-cache hits.
