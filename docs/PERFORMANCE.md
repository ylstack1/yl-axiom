# Parchment performance

Parchment is built for low-end hardware first. Measure with `python scripts/verify.py perf`; it builds the desktop package, runs it against 300 synthetic chats in a scratch folder (never your data) and, when a phone is connected, measures the Android app already installed on it (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets

| Measure | Budget | Before (v2.0.70) | Now |
|---|---|---|---|
| Desktop start to chat title, 300 chats | 2.0 s | 1.65 s (3.1 s without class archive) | 1.47 s; 4.3 s once after an update (was 17.0 s) |
| Desktop idle private memory | 250 MB | about 306 MB (dev run) | 271.3 MB, over budget |
| Android APK | 8 MB | 15.5 MB | 4.3 MB |
| Android cold start | 1.5 s | not measured | needs a phone |
| Android idle PSS | 150 MB | not measured | needs a phone |
| Frame time while streaming, p95 | 16 ms desktop, 33 ms low-end Android | not measured | 5.8 ms desktop (p50 3.7, CPU drawing, `FrameTimeProbeTest`); Android needs a phone |

"Now" is the 2026-11-16 (second batch) ProGuard-shrunk release measurement on an i5-1135G7 with 7.7 GB; the first launch follows the installer's `--cds-warmup` (13 s, hidden). Steady start was 1.40 s and memory 265.9 MB earlier that day; the difference is within run-to-run spread but was not A/B tested (down from 1.52 s and 298.1 MB prior to cross-stack optimization, and 289 MB / 275 MB in the earlier 2026-11-11 batch 12 run on JDK 21/25). Only change a budget with the user's agreement, and record why.

## What already keeps start-up and memory down

- Before the first frame the app reads only the settings and the open chat; everything else loads afterwards, one chat at a time, and inactive chats are kept as small stubs.
- The packaged desktop app uses class-data sharing: a base archive in the runtime (dumped with compact object headers) and an app archive written by the installer's hidden warm-up run.
- Bundled fonts are subset: JetBrains Mono without the Nerd Font icons (0.9 MB instead of 9.4 MB).
- Android release builds are shrunk with R8 and resource shrinking; desktop releases with ProGuard (shrinking only).
- The update check waits a few seconds after start-up.
- Chat identity validation reads only the `id` field (`ChatIdentity`), compact state JSON omits defaults/nulls (`@EncodeDefault` preserves schema markers), and model registries stream gzipped feeds through slim DTOs instead of retaining full JSON DOM trees.
- Settings and the open chat load on a background thread while the window or Activity is still being built. Inactive chats come from `chat-index.json` (one stub per chat, checked against the chat file's size and modification time) instead of decoding every chat file.
- The installer runs the app once, hidden and headless (`--cds-warmup`), so the class archive exists at the installed path before the first real launch. The runtime uses compact object headers (smaller objects) with a matching base archive, and Windows calls go straight through the JDK's FFM API instead of JNA and a PowerShell keep-awake helper.
- Typing recomposes only the chat viewport, not the app root. The streaming pacer sleeps when it has nothing left to reveal instead of waking every frame. Long token counts (reasoning footer, context checks, compaction plans) and clipboard image encoding run off the UI thread.
- Android ships a baseline profile, applied to sideloaded installs by profileinstaller, and draws edge to edge.
- Wire requests send static tool schemas ahead of dynamic turns, stale tool outputs compact in batches of 16, long `CodeFence` blocks chunk past 60 lines in ~40-line segments, item callbacks keep identity across stream frames, and orphaned `.tmp`/old OTA files prune after hydration.
