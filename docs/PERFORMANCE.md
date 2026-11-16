# Parchment performance

Parchment is built for low-end hardware first. Measure with `python scripts/verify.py perf`; it builds the desktop package, runs it against 300 synthetic chats in a scratch folder (never your data) and, when a phone is connected, measures the Android app already installed on it (`--android-install` installs the local release APK first).

## Reference devices

- **Low-end Windows:** 2 cores, 4 GB RAM. **Low-end Android:** API 26, 2 cores, 2 GB RAM (emulator profile).
- **Everyday:** the development PC (i5-1135G7, 7.7 GB) and a OnePlus 15T (Android 17), which the user checks by hand.

Under 4 GB of RAM, or with Android's low-RAM flag, the app runs in low-memory mode: smaller image caches and streaming redrawn at about 30 frames a second instead of every frame.

## Budgets

| Measure | Budget | Before (v2.0.70) | Now |
|---|---|---|---|
| Desktop start to chat title, 300 chats | 2.0 s | 1.65 s (3.1 s without class archive) | 1.46 s; 1.38 s for the first launch after an update (was 17.0 s) |
| Desktop idle private memory | 250 MB | about 306 MB (dev run) | 219.7 MB |
| Android APK | 8 MB | 15.5 MB | 4.4 MB |
| Android cold start | 1.5 s | not measured | needs a phone |
| Android idle PSS | 150 MB | not measured | needs a phone |
| Frame time while streaming, p95 | 16 ms desktop, 33 ms low-end Android | not measured | 7.7 ms for the prose renderer alone (p50 5.0, CPU drawing, `FrameTimeProbeTest`); 7.8 ms in the full app with a 200-message chat (`LiveTurnStreamingTest`); Android needs a phone |

"Now" is the 2026-11-16 (third batch) ProGuard-shrunk release measurement on an i5-1135G7 with 7.7 GB. The update launch follows the installer's hidden `--cds-warmup` (11 s) after a previous session wrote its settings and chat index; a start with chat files but no settings (unusual) still decodes every chat first and takes 4.5 s (`desktop_first_launch_fresh_ms`). Only change a budget with the user's agreement, and record why.

### Measured decisions (2026-11-16)

- **Garbage collector.** Native memory tracking on the packaged app (300 chats, idle): heap 33 MB committed (22 MB used), G1's own structures 50 MB, metaspace 29 MB, code 15 MB, about 110 MB outside the JVM (Skia, GPU, AWT). Serial GC: idle 269 -> 216 MB and start 1.59 -> 1.40 s, pauses under 11 ms while starting and hydrating; a full collection takes about 0.4 s only near the 1 GB ceiling with ~3 million live objects (G1: 0.15 s). Parallel GC saved no memory (264 MB). Turning off CDS archive relocation saved nothing (270.6 vs 269.2 MB).
- **Streaming.** Writing each frame to the reply's live state instead of the app state: full-app frames p50 8.9 -> 5.9 ms, p95 11.1 -> 8.1 ms.
- **Typing.** One composer keystroke in the full app: p50 6.1 ms, p95 7.6 ms (idle frame 1.4 ms, `KeystrokeFrameProbeTest`). Not worth a composer rewrite yet (TODO 17.3).
- **Android startup profile.** The release APK is a single `classes.dex` (4.6 MB) and already ships `baseline.prof`; a startup profile only reorders classes inside that file and is worth it only when generated on a device (a Macrobenchmark `BaselineProfileRule` run, then `src/main/baselineProfiles/startup-prof.txt`).

## What already keeps start-up and memory down

- Before the first frame the app reads only the settings and the open chat; everything else loads afterwards, one chat at a time, and inactive chats are kept as small stubs.
- The packaged desktop app uses class-data sharing: a base archive in the runtime (dumped with compact object headers) and an app archive written by the installer's hidden warm-up run, which renders the real UI in a throwaway folder so the archive covers a normal start.
- Serial GC instead of G1 (see the decisions above); minimizing the window runs a full collection so a finished long turn hands its memory back.
- Bundled fonts are subset: JetBrains Mono without the Nerd Font icons (0.9 MB instead of 9.4 MB).
- Android release builds are shrunk with R8 and resource shrinking; desktop releases with ProGuard (shrinking only).
- The update check waits a few seconds after start-up.
- Chat identity validation reads only the `id` field (`ChatIdentity`), compact state JSON omits defaults/nulls (`@EncodeDefault` preserves schema markers), and model registries stream gzipped feeds through slim DTOs instead of retaining full JSON DOM trees.
- Settings and the open chat load on a background thread while the window or Activity is still being built. Inactive chats come from `chat-index.json` (one stub per chat, checked against the chat file's size and modification time) instead of decoding every chat file.
- The installer runs the app once, hidden and headless (`--cds-warmup`), so the class archive exists at the installed path before the first real launch. The runtime uses compact object headers (smaller objects) with a matching base archive, and Windows calls go straight through the JDK's FFM API instead of JNA and a PowerShell keep-awake helper.
- Typing recomposes only the chat viewport, not the app root. A streaming reply redraws only itself (its live state), and the pacer sleeps when it has nothing left to reveal. Long token counts (context gauge, reasoning footer, context checks, compaction plans) and clipboard image encoding run off the UI thread.
- Android ships a baseline profile, applied to sideloaded installs by profileinstaller, and draws edge to edge.
- Wire requests send static tool schemas ahead of dynamic turns, stale tool outputs compact in batches of 16, long `CodeFence` blocks chunk past 60 lines in ~40-line segments, item callbacks keep identity across stream frames, and orphaned `.tmp`/old OTA files prune after hydration.
