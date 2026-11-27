# Changelog

What changed in each release, newest first. From 2.1.7 on, release-please writes these entries from commit messages that follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:` and `fix:` appear here and set the next version); edit wording in its release PR before merging it.

## [4.0.0](https://github.com/ylstack1/yl-axiom/compare/v3.1.1...v4.0.0) (2026-11-27)


### ⚠ BREAKING CHANGES

* Windows desktop now ships as a standalone native Rust + GPUI executable and automatically migrates v2 JSON chats into SQLite WAL + FTS5 on first launch.

### New

* adaptive mobile/desktop UI scaling (WindowSizeClass + FlowRow) and permanent zero-data-loss update protection ([05e3269](https://github.com/ylstack1/yl-axiom/commit/05e32695fc6ba937a4d5889d53373d731084c179))
* add Axiom Coding screen reusing existing design system ([2fb87c4](https://github.com/ylstack1/yl-axiom/commit/2fb87c4f974514401d500864b7129bcd99f27894))
* add context engineering to Axiom Coding (Phase 7) ([90abfb5](https://github.com/ylstack1/yl-axiom/commit/90abfb56dab617621b7107dc487df8604e7b05c8))
* add keyboard shortcuts, offline Skill/MCP import, /plan & /build modes, .docx extraction, privacy badges, and slash commands ([855b3c1](https://github.com/ylstack1/yl-axiom/commit/855b3c1de84033e1017550058607ba95f83b842b))
* add multi-tasking tabs, native TTS and voice mode, OTA progress bar, and vector icons ([e20e51b](https://github.com/ylstack1/yl-axiom/commit/e20e51ba7c40ea4caf623dd1d385bfcfe000450c))
* Card Bubbles default, Clear All & delete confirmation dialog, clipboard image/video/file paste & drag-and-drop, first-token seconds telemetry, hardware VSync pacer, and OSS repo polish ([6b069e8](https://github.com/ylstack1/yl-axiom/commit/6b069e82efdfee04c5088aefa311415a57b4df76))
* continuous reasoning/tool stopwatch & Tool Peek, inline code selection fix, omp-style parallel/fuzzy harness, and 120fps streaming perf ([7ab3bc3](https://github.com/ylstack1/yl-axiom/commit/7ab3bc3ccf36e42a3d1488d7d579d6885f192c45))
* custom Lucide-style KISS vector icon pack, fix 120fps pacer MonotonicFrameClock crash, and preserve user assistants 1:1 ([2a22eeb](https://github.com/ylstack1/yl-axiom/commit/2a22eeb71df5e6496e61dce6779ea27f480a8e8f))
* **design:** enforce accentSecondary contrast, unify tokens, fix breakdown colors, and align icon grid ([dc7995d](https://github.com/ylstack1/yl-axiom/commit/dc7995d9c420c232be9d4519c0ae34bb39bd5416))
* **desktop:** native folder picker and right-click menus ([2c6b792](https://github.com/ylstack1/yl-axiom/commit/2c6b792b0d06160ffedf20082b143cb3612fb44d))
* fix Android Gboard EmojiKitchen/GIF/clipboard paste, add built-in zoomable image & text/code viewers, add Android FileProvider for OS PDF/video handoff, harden Windows paste & drag-and-drop, and upgrade dependencies to latest LTS ([283e01d](https://github.com/ylstack1/yl-axiom/commit/283e01d787ffe6f01c04c1afed2da2cbc3c431fb))
* fix GPU atlas glitch, inline retry UI, code auto-wrap, and top-bar polish ([f1e00c4](https://github.com/ylstack1/yl-axiom/commit/f1e00c47006f497f646a9d57e22bfb98b2dc7825))
* Kelivo-style multi-subpage UI, 60/120fps VSYNC streaming, Steer/Queue, Minimap, Markdown/LaTeX reasoning, and OnePlus mid-stream survival ([10e8bf9](https://github.com/ylstack1/yl-axiom/commit/10e8bf9e784a26532ae156ab33ec0a502331887c))
* letter-by-letter VSync pacer, Storage & File Manager, auto-scroll on open, UI redundancy & subtitle cleanup, and 120fps optimizations ([9b05bf2](https://github.com/ylstack1/yl-axiom/commit/9b05bf26d7c370688754f9b185df17d7bcc035d0))
* modernize v3 toolchains, GPUI parity, CI pipelines, and mobile layout ([e6a887c](https://github.com/ylstack1/yl-axiom/commit/e6a887c5d47d2aaeb25d3f7feea88789878899a3))
* **motion,ux:** unify transition specs in DesignTokens, honor LocalReduceMotion everywhere, and add calm loading states ([700c605](https://github.com/ylstack1/yl-axiom/commit/700c6053fcdc4030cf5a8ee90d6bb40476b97c15))
* Parchment v3 native Rust core, Windows GPUI desktop, and Android JNI ([a12791d](https://github.com/ylstack1/yl-axiom/commit/a12791d0d3f2353b982cb9ddeb7e17bcd21fb4fe))
* perfect multi-protocol state machine, omp-inspired 10x60s auto-retry, and deadlock-free shell exec ([f15dde2](https://github.com/ylstack1/yl-axiom/commit/f15dde2a85647372d031d8656fefb657a8be40c7))
* Quiet Folio KISS UI overhaul, spring animations, RAM/streaming optimization, and long-horizon 5-min proxy recovery across all protocols ([c7ab955](https://github.com/ylstack1/yl-axiom/commit/c7ab9553c079341a532760ccf87dda1249bcbacb))
* real new-task dialog and open action, no dead controls ([85fe9e7](https://github.com/ylstack1/yl-axiom/commit/85fe9e7a9c09a2437f1a7d159112a03b5f6aabc8))
* real-time thinking stopwatch, live shell_exec streaming & process kill, mid-stream 85% auto-compaction & resume, and mobile UX overhaul ([b5c7e86](https://github.com/ylstack1/yl-axiom/commit/b5c7e86f2d952ee4b74b0e78b9b4314b674415ef))
* **round3:** per-chat folder + media .zip bundles, nested Markdown/LaTeX engine & O(1) streaming perf ([0a3897b](https://github.com/ylstack1/yl-axiom/commit/0a3897b1c89cf8949c4e864bed95a8f892f2e3cf))
* **storage,workspace,ui:** split settings vs per-chat .json files, 17-tool workspace harness + 1-tap mount, zero-dim viewer, high-contrast frosted Latest pill (v2.0.36) ([c6d0f23](https://github.com/ylstack1/yl-axiom/commit/c6d0f2377ef1a450a0d3fb6111be50fa23a60f9f))
* **ui,diff,icons,settings:** official AI lab SVG icons, Git-style green/red file edit diffs, user-turn regenerate, per-swipe delete, and live provider Base URL editing fix ([0329d20](https://github.com/ylstack1/yl-axiom/commit/0329d20ef61eb162a562901d8aba852c38faa6e8))
* **ui,protocol,harness:** chronological interleaved thought/tool timeline, searchable model picker, universal settings search, and custom KissVectorIcons ([17df013](https://github.com/ylstack1/yl-axiom/commit/17df013a109d9a3f4a938a2f7c4c5c6aa880b4c1))
* **ui,protocol,window:** frameless KISS title bar, 3-layer auto context compaction, mobile vertical settings drill-down, context-rich desktop tabs, and smart legacy fallback ([9e9f991](https://github.com/ylstack1/yl-axiom/commit/9e9f9912aea4f1e0be9ee421eb0ad51ac4be20af))
* **ui:** Retry now during retry waits, and failures you can fix lead to the fix ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **ui:** smart duration formatter (50.5s -&gt; 20m21.2s -&gt; 2h15m -&gt; 1d4h15m) with 1-decimal precision ([8ac01af](https://github.com/ylstack1/yl-axiom/commit/8ac01afd8ba96c9918654d766fe1ab545bb9379f))
* **ui:** tab commands in the palette, arrow keys in the Message Map, context gauge off the UI thread ([97daed9](https://github.com/ylstack1/yl-axiom/commit/97daed9be92ae9d7aeaf3020df04c1b85b7fabb6))
* **ux,a11y:** add slash/at keyboard nav, Escape dismissal, focus rings, and accessible labels ([069e70a](https://github.com/ylstack1/yl-axiom/commit/069e70a40c3297bbe7877250db6d66d45dd8e441))
* **ux,perf:** fix string typos, preserve numeric input drafts, and virtualize modal/sidebar lists ([c5c0b8f](https://github.com/ylstack1/yl-axiom/commit/c5c0b8f10f00b7f22a0a53a4e5cbcbd610b9d3f8))
* **v2.0:** Horizon release - Claude Code and Codex /goal engine with todo_write and Stop-Hook gate, repeated-failure circuit breaker, 3-layer rolling context compaction, /btw /compact /rewind commands, multi-day sleep immunity, smart endpoint auto-fallback, and unified Seedream/Seedance proxy routing ([ca16620](https://github.com/ylstack1/yl-axiom/commit/ca1662053710976bd2c8a046a2204af914660cfe))
* wire Axiom Coding as a top-level mode ([10432d8](https://github.com/ylstack1/yl-axiom/commit/10432d8a262d340d4be7bf937bd2ac27cc1c2b4e))
* wire Axiom Coding to real persisted tasks, no placeholders ([d113759](https://github.com/ylstack1/yl-axiom/commit/d113759e791ce87cc630d322c805f6dd2afd0671))
* Zen-by-default UI overhaul, 5s minimap linger, tactile hover/haptic physics, mobile swipe drawer, and desktop Enter-to-send ([8e7272f](https://github.com/ylstack1/yl-axiom/commit/8e7272fe862ffc94f04ec10e8fca66f4c17b9e11))


### Fixes

* add axiomCodingCreate string, expose ConfirmBackdrop, serialization dep for androidMain ([d608dd7](https://github.com/ylstack1/yl-axiom/commit/d608dd736789be226a328b517ba14730baa4666b))
* add missing parchment-intel crate to main ([f2e0bfa](https://github.com/ylstack1/yl-axiom/commit/f2e0bfa4e3f6021711b9124812cd595264bf2df7))
* always build and upload Android debug APK ([5062bba](https://github.com/ylstack1/yl-axiom/commit/5062bba3ab90874676bea6d05605c24bfca3932d))
* **android,harness:** fix computer_use race/background approvals and isolate session TODO & global AGENT.md ([21e26f2](https://github.com/ylstack1/yl-axiom/commit/21e26f2ff373174b0f60e1600fd7bd8e73a13cc9))
* apply rustfmt to JNI coding bridge ([ea9dbb9](https://github.com/ylstack1/yl-axiom/commit/ea9dbb918efdf89485a73fbf01bf2c1cc8c8b422))
* apply rustfmt to parchment-coding/src/lib.rs ([300bce4](https://github.com/ylstack1/yl-axiom/commit/300bce4e4d1cb97cb62a0b794d2eb12b8a1a3aca))
* **app:** save pending changes and keep a redacted report when an error escapes ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* AxiomCodingStore via PlatformStorage expect/actual; expose ConfirmBackdrop; add axiomCodingCreate string ([4b01dbd](https://github.com/ylstack1/yl-axiom/commit/4b01dbd2929858f756543d0f56ab81e8eee0d7e1))
* ci.rs compare p == *s instead of unstable str_as_str ([cfae697](https://github.com/ylstack1/yl-axiom/commit/cfae697202d91b1326d590af35b1291eda25e7ed))
* ci.rs stable contains for &str at line 163 ([f6986c8](https://github.com/ylstack1/yl-axiom/commit/f6986c8c5a86c1617a4dc2deeaeb28c87e2431ee))
* ci.rs uses stable String == &str instead of unstable str_as_str ([e5f1509](https://github.com/ylstack1/yl-axiom/commit/e5f1509d5c059ba95a632b16afa9842ef6c90115))
* clippy manual_contains; JNI jboolean returns Ok(false as jboolean) ([4bf6a8b](https://github.com/ylstack1/yl-axiom/commit/4bf6a8beaa869ddda2cc1d8be2b7f8a72a6958af))
* desktop PlatformStorage coding-task stubs ([88e0e1e](https://github.com/ylstack1/yl-axiom/commit/88e0e1e0ac8f1dfea795480e79bc0e075e8f3694))
* **desktop:** end a stuck warm-up without writing a partial class archive ([ee5ed10](https://github.com/ylstack1/yl-axiom/commit/ee5ed1010f5247623c5b60326425b661aa705ca7))
* drop unused Axiom Coding strings rejected by dead-code gate ([2e53d51](https://github.com/ylstack1/yl-axiom/commit/2e53d51dea7289583145d53f9b15e5c3c116b497))
* gate JNI store helpers on android-jni + network ([c339a51](https://github.com/ylstack1/yl-axiom/commit/c339a518849c3f4f9fbab2da367a304c8b4371d1))
* **harness:** parallel helpers share one chat spend limit ([2c055be](https://github.com/ylstack1/yl-axiom/commit/2c055bec7b560de801a4fe230d2b9c2b3d4e951c))
* **icons:** normalize SVGO single-bit arc flags before Compose PathParser so Gemini and lab SVG icons render inside 24x24 bounds ([ca69e39](https://github.com/ylstack1/yl-axiom/commit/ca69e3931093ae969ff4acc202e63bdb021ec33d))
* **icons:** use KissIcon.CHAT for chat session items in Storage & File Manager ([bf02a9d](https://github.com/ylstack1/yl-axiom/commit/bf02a9dbadd99f34860d0ebec93558e5fa58d375))
* **icons:** use KissIcon.FILE_TEXT for chat session items in Storage & File Manager ([7bd771c](https://github.com/ylstack1/yl-axiom/commit/7bd771c93baa64ee1f94c50af75fcf75c9a2712e))
* isolate desktop unit test storage and remove wipeAllLocalData from test cleanup so Windows user settings in %LOCALAPPDATA%\ParchmentAI are never wiped ([11ddeda](https://github.com/ylstack1/yl-axiom/commit/11ddedabb0e72d8c23da56cd71486f1fc0d52d79))
* jni save returns bool via map_ok/unwrap_or ([0b194e3](https://github.com/ylstack1/yl-axiom/commit/0b194e32f2285a7efb1fad311ec19bcd3748a1fc))
* ktlint import order; listCodingTaskIds returns List already ([2dc869b](https://github.com/ylstack1/yl-axiom/commit/2dc869bcc2f5b8e1bd0c3df864ea81f940cd0b07))
* ktlint one-liner and import order, nullable split chain ([54caf6a](https://github.com/ylstack1/yl-axiom/commit/54caf6a48711f4bf30bc23ba9e4c32f23f55ec9b))
* make jni and parchment-coding non-optional; gate jni_android on android-jni; fix ci.rs contains type ([88e5b5a](https://github.com/ylstack1/yl-axiom/commit/88e5b5a1b4c633c16712798ca668bcc1a037cac4))
* **mcp:** start a stopped server again with the same tools, and renew a lost HTTP session ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **memory:** fix OOM heap crash on long agent turns, uncap compaction/system prompts & stop Gradle daemon bloat ([1f60adc](https://github.com/ylstack1/yl-axiom/commit/1f60adc7812f5bcb0d94879a7f559fb3ca4efa9b))
* **network,model,ui:** harden cross-provider reasoning handoff and Gemini poll recovery ([83592e4](https://github.com/ylstack1/yl-axiom/commit/83592e4ffe29bcb60e5488c95f5340e30ee32799))
* **network,ui,platform:** fix usage accounting, optimize UI/IO performance, and harden recovery ([f3df995](https://github.com/ylstack1/yl-axiom/commit/f3df99533366bd83667465d7936c849a3e2cb566))
* **network,ui,platform:** harden provider protocols, streaming recovery, persistence, and UI stability ([59e8450](https://github.com/ylstack1/yl-axiom/commit/59e84505b75e0d07ba744da1cc3d36443ff6b652))
* **network,ui,platform:** modularize core files and harden protocols, recovery, and UI ([4c3ba08](https://github.com/ylstack1/yl-axiom/commit/4c3ba088cfa543646c17841886ef5292c8523f70))
* **network:** stop at once on spend caps, billing limits and daily quotas ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **network:** wait as long as the provider asks, otherwise 2 to 60 s, instead of 60 s after every failure ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* optional parchment-coding dep and two ktlint body lines ([fbc5534](https://github.com/ylstack1/yl-axiom/commit/fbc5534c4034a8bce85833566f1a4a04e1b3af44))
* preserve Leyden AOT cache mapping in installed Windows builds ([d03b04f](https://github.com/ylstack1/yl-axiom/commit/d03b04fffd5c6b02d010a79d967c359be7d414b3))
* prevent mobile Send button overflow and ensure main-screen OTA download progress bar renders smoothly ([dc58acd](https://github.com/ylstack1/yl-axiom/commit/dc58acd492bddf5d976fed921f8114fe6f96c11e))
* protocol wire parity, v2 auto-migration, version sync, and desktop polish ([a14affa](https://github.com/ylstack1/yl-axiom/commit/a14affa47abfa75d6f846f9a7bed3229b1a1d9ed))
* **protocol,registry:** harden reasoning fallback, background catalog sync, and EDT UI tests ([08b06e9](https://github.com/ylstack1/yl-axiom/commit/08b06e96333412d81f233705e0cdfaf9c01992ea))
* **protocol:** align reasoning fallback, Anthropic tool replay, and MCP 2026-07-28 ([3fa7f58](https://github.com/ylstack1/yl-axiom/commit/3fa7f582e528783c3a20e939c2f56b915a786c4f))
* **protocol:** keep endpoint URLs credential-free and encode query auth ([f4bb464](https://github.com/ylstack1/yl-axiom/commit/f4bb464d7f0ade493a444626d3068928b83107dc))
* put function body on signature line for ktlint ([0bc76f6](https://github.com/ylstack1/yl-axiom/commit/0bc76f65811e8a76617094e4ce6be11bcc378edd))
* reference jni and parchment-coding under network to satisfy unused_dependencies ([eb26a98](https://github.com/ylstack1/yl-axiom/commit/eb26a98ad57f3e6eb1daa9b93d035c0844220c3c))
* reference jni/parchment-coding types for unused_dependencies; collapse ktlint one-liners ([f1e55a7](https://github.com/ylstack1/yl-axiom/commit/f1e55a7574c0991387e77a46570e6205c9a2c414))
* replace arithmetic in CI shard name so release installer workflow dispatches ([b9d3dc0](https://github.com/ylstack1/yl-axiom/commit/b9d3dc0bde3092b360aa1e236832402319de37ef))
* replace inline unavailable cost text with icons and usage popup ([3e536ee](https://github.com/ylstack1/yl-axiom/commit/3e536ee0e80dfe2e82dfb27e00a2f0493129eaa9))
* resolve all compile errors in parchment-coding and storage ([15e6872](https://github.com/ylstack1/yl-axiom/commit/15e68724932157d185d1c2f4d5e4ae557173ad25))
* resolve blank desktop viewport when opening Settings and prevent OTA file-lock race from overwriting user state ([476ab5f](https://github.com/ylstack1/yl-axiom/commit/476ab5fb193233745f1c875ce2b9528f400e5e80))
* resolve final 2 compile errors in parchment-coding ([bcfbbd8](https://github.com/ylstack1/yl-axiom/commit/bcfbbd835dd46981c3d7fd883cea4a12d1daa131))
* restore coding_* DDL and collapse nested if ([77248f9](https://github.com/ylstack1/yl-axiom/commit/77248f911c17298d1a9843103362b954a89a56e4))
* restore locked hardware VSync SSE streaming and eliminate Shenandoah GC spin loop ([855b3c1](https://github.com/ylstack1/yl-axiom/commit/855b3c1de84033e1017550058607ba95f83b842b))
* restore VSync SSE streaming, smooth pulse dot, whole-second timer, and code wrapping ([6d494fc](https://github.com/ylstack1/yl-axiom/commit/6d494fce762c13309de01f872ec50b68f0c36839))
* run keyboard shortcuts when nothing has focus ([5051e0b](https://github.com/ylstack1/yl-axiom/commit/5051e0ba5bd770d9edf5db3dbb0af584e8ad3982))
* run keyboard shortcuts when nothing has focus ([cf20a2f](https://github.com/ylstack1/yl-axiom/commit/cf20a2f72975f984081ccf3e792e05622e02dda3))
* run the Compose JVM test suite that actually exists ([b920d72](https://github.com/ylstack1/yl-axiom/commit/b920d724f1cd298e00d67b8897650a9973154c4a))
* split newline via regex in listCodingTaskIds ([5a872b9](https://github.com/ylstack1/yl-axiom/commit/5a872b90b211fd41b50d29d007229ebcf960fa35))
* stop passing -Dorg.gradle.jvmargs on the Gradle CLI ([2b48097](https://github.com/ylstack1/yl-axiom/commit/2b4809736396f9c7fa0301de319173194fcc4056))
* **storage:** tie chat-index.json to the stub format ([80b56e1](https://github.com/ylstack1/yl-axiom/commit/80b56e1358cc7d84b710be2b5312a12a103028ab))
* **streams,persistence:** preserve stop phase, join pacer on cancel, and wake urgent disk writes ([6a81f64](https://github.com/ylstack1/yl-axiom/commit/6a81f64f0375387112305c06aba433a80f0382dc))
* support Shift+Enter newline in multi-line inputs and harden workspace tool loops and per-step auto-retry ([2422488](https://github.com/ylstack1/yl-axiom/commit/24224881061e0e3d3e53ab579f41af585ce68ebd))
* **ui,harness:** harden screen-reader semantics, responsive layouts, and transport/computer_use safety ([82f3052](https://github.com/ylstack1/yl-axiom/commit/82f3052e7c0592b5699ebe8f670628f6d526d33b))
* **ui:** add scrim and elevated surface contrast to tab switcher modal ([b1c93f5](https://github.com/ylstack1/yl-axiom/commit/b1c93f5ff121ce4a988cd5f74a37e87adfc4f0c6))
* **ui:** resolve key import and regex constant in ParchmentApp & Modals ([c3217e8](https://github.com/ylstack1/yl-axiom/commit/c3217e877b69e8e9c813c95f2456ee11c508ba19))
* use is_multiple_of to satisfy clippy::manual_is_multiple_of ([401f7de](https://github.com/ylstack1/yl-axiom/commit/401f7deabfdd01b5e8652e13e9180d307087ecf6))
* use Task from parchment-coding in JNI save; jni_android returns Result ([ffbe2fb](https://github.com/ylstack1/yl-axiom/commit/ffbe2fb83beffa990da7ae774cd510d02fd68ac6))
* vertically center model rows and headers in Provider settings FlowRows ([9a3f0b1](https://github.com/ylstack1/yl-axiom/commit/9a3f0b161c106b5fa1606f7799c62f224abdca96))
* **viewport:** pause auto-follow on drag or wheel scroll and reserve async media/peek bounds ([20579e4](https://github.com/ylstack1/yl-axiom/commit/20579e4ffe0a32eaa6e54f8322dc89166ad5ace1))
* wrap jni save closure result in Ok(bool) ([7a6b647](https://github.com/ylstack1/yl-axiom/commit/7a6b647e2d638bdd032605ea9699c5229d4a4ba2))
* wrap jni save closure result in Ok(bool) ([6a37ee4](https://github.com/ylstack1/yl-axiom/commit/6a37ee444c8bbcace7216b018c3ad45900578c82))
* write Android SDK licenses with sudo, tolerate cache restore failures ([cbddc9a](https://github.com/ylstack1/yl-axiom/commit/cbddc9a01d48d7cf584d410ec6828c8db1b69751))


### Faster and lighter

* cap D3D12 GPU cache at 48 MiB, fast-open large active chats, and halve CI workflow runtimes ([855b3c1](https://github.com/ylstack1/yl-axiom/commit/855b3c1de84033e1017550058607ba95f83b842b))
* cut SSE power usage, pack single-JAR AOT startup, and speed streaming ([9a3f0b1](https://github.com/ylstack1/yl-axiom/commit/9a3f0b161c106b5fa1606f7799c62f224abdca96))
* **desktop:** serial GC brings idle memory under budget ([bd58df1](https://github.com/ylstack1/yl-axiom/commit/bd58df1b2bebdf88634ff8c12c76a20e800721a6))
* FFM native calls, CDS warm-up, chat stub index, prompt jump and parallel helpers ([b2ecff7](https://github.com/ylstack1/yl-axiom/commit/b2ecff7d57fdb8a6716850145736baa759924ad0))
* FFM native calls, CDS warm-up, chat stub index, prompt jump and parallel helpers ([a1d6cc5](https://github.com/ylstack1/yl-axiom/commit/a1d6cc5d583ee0f8bf6fcfea6c5e0cbe472338f6))
* Leyden AOT cache, Shenandoah GC, native Win32 file pickers, UI consistency, and sharded CI ([402452b](https://github.com/ylstack1/yl-axiom/commit/402452b91645b7d6d3ca387791735e668b1f4360))
* **memory,io:** bound icon caches, offload UI-thread disk/codec I/O, and fix initial scroll layout race ([55ab542](https://github.com/ylstack1/yl-axiom/commit/55ab54237573cdffcc91e96c6b236d6f126d3a0a))
* **memory,startup:** eliminate dummy stub-node lists, defer startup discovery/TTS, and tighten image cache ([f1b2a34](https://github.com/ylstack1/yl-axiom/commit/f1b2a34d5c2d88cbbd94aa3074a094fba401b434))
* **network,tokenizer:** cache variant/tool BPE counts, coalesce utility/MCP calls, and trim history allocations ([778ea5f](https://github.com/ylstack1/yl-axiom/commit/778ea5f67e9284ddcd3ab531dc64617d83d178fa))
* **network:** stop copying the request on every stream event ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **prose:** flatten inline style builders, eliminate line-split lists, and scan code-fence increments ([0602d6c](https://github.com/ylstack1/yl-axiom/commit/0602d6cca3cff15451a9495ef7bef83d78ccaab4))
* **registry:** warm model catalog in background with models.dev gap-fill and cooldown ([720c509](https://github.com/ylstack1/yl-axiom/commit/720c50925bba9277a09f72094f450da040d73d86))
* **renderers:** cache prepared SVG draw items/paths and coalesce LaTeX math spans ([00c7c90](https://github.com/ylstack1/yl-axiom/commit/00c7c904e99486f9cb407269941b7c65c5d9d064))
* restore C1-only JIT, DEFLATED single-JAR packaging, and sub-195 MB sustained RAM ([6d494fc](https://github.com/ylstack1/yl-axiom/commit/6d494fce762c13309de01f872ec50b68f0c36839))
* **round2:** fix Android CI dexBuilder OOM, stream workspaceRead/fetchUrlText & throttle decode IPC ([af62c48](https://github.com/ylstack1/yl-axiom/commit/af62c48a42b76cd5549a7fa2cd92e223a86498f4))
* **scripts:** skip reading binary asset bytes in quality checks ([001b45c](https://github.com/ylstack1/yl-axiom/commit/001b45cf7cc7f72c9b1149f6d05341ae7eaed44d))
* slash RAM footprint, instant startup, accurate tok/s & O(1) prefix-cached streaming ([ffe28de](https://github.com/ylstack1/yl-axiom/commit/ffe28de1f8f4d45c5949a1188f0044d93a94de44))
* **storage,network,ui:** cross-stack startup, memory, streaming, and cache efficiency ([2972db4](https://github.com/ylstack1/yl-axiom/commit/2972db4d3875e3bad6dd75868a2d970489c804f4))
* **storage,network,ui:** optimize snapshot JSON, DTO streaming, prefix caching, and 120 Hz UI ([c4156e9](https://github.com/ylstack1/yl-axiom/commit/c4156e9a53f70f10e434cc313e9ba7cf4933cc97))
* **storage:** streaming checkpoints save the reply, not the whole chat ([e1d3486](https://github.com/ylstack1/yl-axiom/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* sub-1s cold start, sub-200MB idle RAM, CI cache overhaul, and faster agent harness ([ee45104](https://github.com/ylstack1/yl-axiom/commit/ee45104b2feee9b18c19f9abcbe63f3883b079f5))
* **ui:** memoize sidebarFingerprint per session, stabilize composer callbacks, remove animateContentSize jump, and avoid streaming BPE ([07e3387](https://github.com/ylstack1/yl-axiom/commit/07e3387d71703e0c62c386b15612c49789443c15))
* **ui:** stream replies through a live overlay instead of the app state ([3444296](https://github.com/ylstack1/yl-axiom/commit/3444296dbb89461d2ec51dd6d1284f485b559fa8))


### Dependencies

* bump base64 from 0.22.1 to 0.23.1 ([#53](https://github.com/ylstack1/yl-axiom/issues/53)) ([c5694b7](https://github.com/ylstack1/yl-axiom/commit/c5694b75b4ffae018a2ad64d15f73516ac077c6f))

## [3.1.1](https://github.com/baros4294/parchment/compare/v3.1.0...v3.1.1) (2026-11-20)


### Fixes

* protocol wire parity, v2 auto-migration, version sync, and desktop polish ([a14affa](https://github.com/baros4294/parchment/commit/a14affa47abfa75d6f846f9a7bed3229b1a1d9ed))

## [3.1.0](https://github.com/baros4294/parchment/compare/v3.0.0...v3.1.0) (2026-11-20)


### New

* modernize v3 toolchains, GPUI parity, CI pipelines, and mobile layout ([e6a887c](https://github.com/baros4294/parchment/commit/e6a887c5d47d2aaeb25d3f7feea88789878899a3))


### Fixes

* **protocol:** keep endpoint URLs credential-free and encode query auth ([f4bb464](https://github.com/baros4294/parchment/commit/f4bb464d7f0ade493a444626d3068928b83107dc))


### Dependencies

* bump base64 from 0.22.1 to 0.23.1 ([#53](https://github.com/baros4294/parchment/issues/53)) ([c5694b7](https://github.com/baros4294/parchment/commit/c5694b75b4ffae018a2ad64d15f73516ac077c6f))

## [3.0.0](https://github.com/baros4294/parchment/compare/v2.6.0...v3.0.0) (2026-11-19)


### ⚠ BREAKING CHANGES

* Windows desktop now ships as a standalone native Rust + GPUI executable and automatically migrates v2 JSON chats into SQLite WAL + FTS5 on first launch.

### New

* YL Axiom v3 native Rust core, Windows GPUI desktop, and Android JNI ([a12791d](https://github.com/baros4294/parchment/commit/a12791d0d3f2353b982cb9ddeb7e17bcd21fb4fe))

## [2.6.0](https://github.com/baros4294/parchment/compare/v2.5.2...v2.6.0) (2026-11-19)


### New

* add keyboard shortcuts, offline Skill/MCP import, /plan & /build modes, .docx extraction, privacy badges, and slash commands ([855b3c1](https://github.com/baros4294/parchment/commit/855b3c1de84033e1017550058607ba95f83b842b))


### Fixes

* restore locked hardware VSync SSE streaming and eliminate Shenandoah GC spin loop ([855b3c1](https://github.com/baros4294/parchment/commit/855b3c1de84033e1017550058607ba95f83b842b))


### Faster and lighter

* cap D3D12 GPU cache at 48 MiB, fast-open large active chats, and halve CI workflow runtimes ([855b3c1](https://github.com/baros4294/parchment/commit/855b3c1de84033e1017550058607ba95f83b842b))

## [2.5.2](https://github.com/baros4294/parchment/compare/v2.5.1...v2.5.2) (2026-11-18)


### Fixes

* restore VSync SSE streaming, smooth pulse dot, whole-second timer, and code wrapping ([6d494fc](https://github.com/baros4294/parchment/commit/6d494fce762c13309de01f872ec50b68f0c36839))


### Faster and lighter

* restore C1-only JIT, DEFLATED single-JAR packaging, and sub-195 MB sustained RAM ([6d494fc](https://github.com/baros4294/parchment/commit/6d494fce762c13309de01f872ec50b68f0c36839))

## [2.5.1](https://github.com/baros4294/parchment/compare/v2.5.0...v2.5.1) (2026-11-18)


### Fixes

* vertically center model rows and headers in Provider settings FlowRows ([9a3f0b1](https://github.com/baros4294/parchment/commit/9a3f0b161c106b5fa1606f7799c62f224abdca96))


### Faster and lighter

* cut SSE power usage, pack single-JAR AOT startup, and speed streaming ([9a3f0b1](https://github.com/baros4294/parchment/commit/9a3f0b161c106b5fa1606f7799c62f224abdca96))

## [2.5.0](https://github.com/baros4294/parchment/compare/v2.4.4...v2.5.0) (2026-11-18)


### New

* fix GPU atlas glitch, inline retry UI, code auto-wrap, and top-bar polish ([f1e00c4](https://github.com/baros4294/parchment/commit/f1e00c47006f497f646a9d57e22bfb98b2dc7825))

## [2.4.4](https://github.com/baros4294/parchment/compare/v2.4.3...v2.4.4) (2026-11-18)


### Fixes

* preserve Leyden AOT cache mapping in installed Windows builds ([d03b04f](https://github.com/baros4294/parchment/commit/d03b04fffd5c6b02d010a79d967c359be7d414b3))

## [2.4.3](https://github.com/baros4294/parchment/compare/v2.4.2...v2.4.3) (2026-11-17)


### Faster and lighter

* sub-1s cold start, sub-200MB idle RAM, CI cache overhaul, and faster agent harness ([ee45104](https://github.com/baros4294/parchment/commit/ee45104b2feee9b18c19f9abcbe63f3883b079f5))

## [2.4.2](https://github.com/baros4294/parchment/compare/v2.4.1...v2.4.2) (2026-11-17)


### Fixes

* replace arithmetic in CI shard name so release installer workflow dispatches ([b9d3dc0](https://github.com/baros4294/parchment/commit/b9d3dc0bde3092b360aa1e236832402319de37ef))

## [2.4.1](https://github.com/baros4294/parchment/compare/v2.4.0...v2.4.1) (2026-11-17)


### Faster and lighter

* Leyden AOT cache, Shenandoah GC, native Win32 file pickers, UI consistency, and sharded CI ([402452b](https://github.com/baros4294/parchment/commit/402452b91645b7d6d3ca387791735e668b1f4360))

## [2.4.0](https://github.com/baros4294/parchment/compare/v2.3.0...v2.4.0) (2026-11-16)


### New

* **ui:** Retry now during retry waits, and failures you can fix lead to the fix ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))


### Fixes

* **app:** save pending changes and keep a redacted report when an error escapes ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **mcp:** start a stopped server again with the same tools, and renew a lost HTTP session ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **network:** stop at once on spend caps, billing limits and daily quotas ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **network:** wait as long as the provider asks, otherwise 2 to 60 s, instead of 60 s after every failure ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))


### Faster and lighter

* **network:** stop copying the request on every stream event ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))
* **storage:** streaming checkpoints save the reply, not the whole chat ([e1d3486](https://github.com/baros4294/parchment/commit/e1d34862630f185d557abe8d6f122924b080ae48))

## [2.3.0](https://github.com/baros4294/parchment/compare/v2.2.6...v2.3.0) (2026-11-16)


### New

* **desktop:** native folder picker and right-click menus ([2c6b792](https://github.com/baros4294/parchment/commit/2c6b792b0d06160ffedf20082b143cb3612fb44d))
* **ui:** tab commands in the palette, arrow keys in the Message Map, context gauge off the UI thread ([97daed9](https://github.com/baros4294/parchment/commit/97daed9be92ae9d7aeaf3020df04c1b85b7fabb6))


### Fixes

* **desktop:** end a stuck warm-up without writing a partial class archive ([ee5ed10](https://github.com/baros4294/parchment/commit/ee5ed1010f5247623c5b60326425b661aa705ca7))
* **harness:** parallel helpers share one chat spend limit ([2c055be](https://github.com/baros4294/parchment/commit/2c055bec7b560de801a4fe230d2b9c2b3d4e951c))
* **storage:** tie chat-index.json to the stub format ([80b56e1](https://github.com/baros4294/parchment/commit/80b56e1358cc7d84b710be2b5312a12a103028ab))


### Faster and lighter

* **desktop:** serial GC brings idle memory under budget ([bd58df1](https://github.com/baros4294/parchment/commit/bd58df1b2bebdf88634ff8c12c76a20e800721a6))
* **ui:** stream replies through a live overlay instead of the app state ([3444296](https://github.com/baros4294/parchment/commit/3444296dbb89461d2ec51dd6d1284f485b559fa8))

## [2.2.6](https://github.com/baros4294/parchment/compare/v2.2.5...v2.2.6) (2026-11-16)


### Faster and lighter

* FFM native calls, CDS warm-up, chat stub index, prompt jump and parallel helpers ([a1d6cc5](https://github.com/baros4294/parchment/commit/a1d6cc5d583ee0f8bf6fcfea6c5e0cbe472338f6))

## [2.2.5](https://github.com/baros4294/parchment/compare/v2.2.4...v2.2.5) (2026-11-16)


### Faster and lighter

* **scripts:** skip reading binary asset bytes in quality checks ([001b45c](https://github.com/baros4294/parchment/commit/001b45cf7cc7f72c9b1149f6d05341ae7eaed44d))
* **storage,network,ui:** cross-stack startup, memory, streaming, and cache efficiency ([2972db4](https://github.com/baros4294/parchment/commit/2972db4d3875e3bad6dd75868a2d970489c804f4))
* **storage,network,ui:** optimize snapshot JSON, DTO streaming, prefix caching, and 120 Hz UI ([c4156e9](https://github.com/baros4294/parchment/commit/c4156e9a53f70f10e434cc313e9ba7cf4933cc97))

## [2.2.4](https://github.com/baros4294/parchment/compare/v2.2.3...v2.2.4) (2026-11-16)


### Fixes

* **protocol:** align reasoning fallback, Anthropic tool replay, and MCP 2026-07-28 ([3fa7f58](https://github.com/baros4294/parchment/commit/3fa7f582e528783c3a20e939c2f56b915a786c4f))


### Faster and lighter

* **registry:** warm model catalog in background with models.dev gap-fill and cooldown ([720c509](https://github.com/baros4294/parchment/commit/720c50925bba9277a09f72094f450da040d73d86))

## [2.2.3](https://github.com/baros4294/parchment/compare/v2.2.2...v2.2.3) (2026-11-15)


### Fixes

* **ui,harness:** harden screen-reader semantics, responsive layouts, and transport/computer_use safety ([82f3052](https://github.com/baros4294/parchment/commit/82f3052e7c0592b5699ebe8f670628f6d526d33b))

## [2.2.2](https://github.com/baros4294/parchment/compare/v2.2.1...v2.2.2) (2026-11-15)


### Fixes

* **ui:** add scrim and elevated surface contrast to tab switcher modal ([b1c93f5](https://github.com/baros4294/parchment/commit/b1c93f5ff121ce4a988cd5f74a37e87adfc4f0c6))

## [2.2.1](https://github.com/baros4294/parchment/compare/v2.2.0...v2.2.1) (2026-11-15)


### Fixes

* **android,harness:** fix computer_use race/background approvals and isolate session TODO & global AGENT.md ([21e26f2](https://github.com/baros4294/parchment/commit/21e26f2ff373174b0f60e1600fd7bd8e73a13cc9))

## [2.2.0](https://github.com/baros4294/parchment/compare/v2.1.11...v2.2.0) (2026-11-15)


### New

* **design:** enforce accentSecondary contrast, unify tokens, fix breakdown colors, and align icon grid ([dc7995d](https://github.com/baros4294/parchment/commit/dc7995d9c420c232be9d4519c0ae34bb39bd5416))
* **motion,ux:** unify transition specs in DesignTokens, honor LocalReduceMotion everywhere, and add calm loading states ([700c605](https://github.com/baros4294/parchment/commit/700c6053fcdc4030cf5a8ee90d6bb40476b97c15))
* **ux,a11y:** add slash/at keyboard nav, Escape dismissal, focus rings, and accessible labels ([069e70a](https://github.com/baros4294/parchment/commit/069e70a40c3297bbe7877250db6d66d45dd8e441))
* **ux,perf:** fix string typos, preserve numeric input drafts, and virtualize modal/sidebar lists ([c5c0b8f](https://github.com/baros4294/parchment/commit/c5c0b8f10f00b7f22a0a53a4e5cbcbd610b9d3f8))


### Fixes

* **streams,persistence:** preserve stop phase, join pacer on cancel, and wake urgent disk writes ([6a81f64](https://github.com/baros4294/parchment/commit/6a81f64f0375387112305c06aba433a80f0382dc))
* **viewport:** pause auto-follow on drag or wheel scroll and reserve async media/peek bounds ([20579e4](https://github.com/baros4294/parchment/commit/20579e4ffe0a32eaa6e54f8322dc89166ad5ace1))


### Faster and lighter

* **memory,io:** bound icon caches, offload UI-thread disk/codec I/O, and fix initial scroll layout race ([55ab542](https://github.com/baros4294/parchment/commit/55ab54237573cdffcc91e96c6b236d6f126d3a0a))
* **memory,startup:** eliminate dummy stub-node lists, defer startup discovery/TTS, and tighten image cache ([f1b2a34](https://github.com/baros4294/parchment/commit/f1b2a34d5c2d88cbbd94aa3074a094fba401b434))
* **network,tokenizer:** cache variant/tool BPE counts, coalesce utility/MCP calls, and trim history allocations ([778ea5f](https://github.com/baros4294/parchment/commit/778ea5f67e9284ddcd3ab531dc64617d83d178fa))
* **prose:** flatten inline style builders, eliminate line-split lists, and scan code-fence increments ([0602d6c](https://github.com/baros4294/parchment/commit/0602d6cca3cff15451a9495ef7bef83d78ccaab4))
* **renderers:** cache prepared SVG draw items/paths and coalesce LaTeX math spans ([00c7c90](https://github.com/baros4294/parchment/commit/00c7c904e99486f9cb407269941b7c65c5d9d064))
* **ui:** memoize sidebarFingerprint per session, stabilize composer callbacks, remove animateContentSize jump, and avoid streaming BPE ([07e3387](https://github.com/baros4294/parchment/commit/07e3387d71703e0c62c386b15612c49789443c15))

## [2.1.11](https://github.com/baros4294/parchment/compare/v2.1.10...v2.1.11) (2026-11-15)


### Fixes

* **network,model,ui:** harden cross-provider reasoning handoff and Gemini poll recovery ([83592e4](https://github.com/baros4294/parchment/commit/83592e4ffe29bcb60e5488c95f5340e30ee32799))

## [2.1.10](https://github.com/baros4294/parchment/compare/v2.1.9...v2.1.10) (2026-11-14)


### Fixes

* **network,ui,platform:** fix usage accounting, optimize UI/IO performance, and harden recovery ([f3df995](https://github.com/baros4294/parchment/commit/f3df99533366bd83667465d7936c849a3e2cb566))

## [2.1.9](https://github.com/baros4294/parchment/compare/v2.1.8...v2.1.9) (2026-11-14)


### Fixes

* **network,ui,platform:** harden provider protocols, streaming recovery, persistence, and UI stability ([59e8450](https://github.com/baros4294/parchment/commit/59e84505b75e0d07ba744da1cc3d36443ff6b652))
* **network,ui,platform:** modularize core files and harden protocols, recovery, and UI ([4c3ba08](https://github.com/baros4294/parchment/commit/4c3ba088cfa543646c17841886ef5292c8523f70))

## [2.1.8](https://github.com/baros4294/parchment/compare/v2.1.7...v2.1.8) (2026-11-14)


### Fixes

* replace inline unavailable cost text with icons and usage popup ([3e536ee](https://github.com/baros4294/parchment/commit/3e536ee0e80dfe2e82dfb27e00a2f0493129eaa9))

## [2.1.7](https://github.com/baros4294/parchment/compare/v2.1.6...v2.1.7) (2026-11-12)


### Fixes

* run keyboard shortcuts when nothing has focus ([5051e0b](https://github.com/baros4294/parchment/commit/5051e0ba5bd770d9edf5db3dbb0af584e8ad3982))
* run keyboard shortcuts when nothing has focus ([cf20a2f](https://github.com/baros4294/parchment/commit/cf20a2f72975f984081ccf3e792e05622e02dda3))

## 2.1.6

- **Windows: chats, model lists and update checks work again.** Since 2.1.2 the Windows app's smaller build removed a part Ktor needs for JSON, so every request failed with "Could not initialize class io.ktor.serialization.kotlinx.ExtensionsJvmKt". **On Windows 2.1.2 or 2.1.3, install this version by hand once** (download the installer below and run it; chats and settings are kept): the broken versions can't update themselves. Android was not affected.
- Releases now fail to build if the shrinking step removes any class Java looks up by name.

## 2.1.3

- **Ctrl+K command palette:** find any action, settings page or chat by typing. New shortcuts: Ctrl+N new chat, Ctrl+, settings, Ctrl+B sidebar.
- **Undo for deleted chats:** deleting a chat, or every chat of an assistant, can be undone from the notice for 8 seconds. New installs no longer ask for confirmation first.
- **First run:** without an API key, the empty chat says so and opens provider setup in one click. The models list explains why it's empty and can fetch models right there.
- **Themes:** shown as small previews instead of a list of names.
- **Large text:** at 130% system text and above, the phone composer uses two rows instead of wrapping single controls.
- **Accessibility:** accent-colored text meets 4.5:1 contrast on every theme (the default and Linear Carbon were slightly short).
- **Anthropic prompt caching on long agent runs:** older tool results were shortened one more each step, which made most of every request miss Anthropic's cache. They're now shortened in batches like the other providers: about 69% of each request is reused instead of 21%.
- **Low-end devices:** overlays skip their shadow on devices under 4 GB; Reduce motion also stops the chat's scroll animations.
- Security policy, code of conduct, issue templates for features and questions, and third-party font notices; the fonts' licence (OFL) now ships with both apps.
- Groundwork for translations: interface text is gathered in one table.

## 2.1.2

- Sidebar, settings list and icons grow with the system text size, so labels keep whole words at 200%.
- Disabled buttons keep an outline, so they stay visible on dark themes.
- A streaming reply no longer rebuilds the sidebar's lists every frame.
- Design tokens for spacing, corners, lines, icons and motion; Reduce motion now turns every animation off.
- Windows: release jars shrunk with ProGuard (35 MB to 22 MB); README screenshots; the Android R8 mapping file is attached to releases.

## 2.1.1

- JDK 25, Gradle 9.7, Android Gradle Plugin 9.3, Kotlin 2.4 and Compose 1.12. The Android app is built by its own module.

## 2.1.0

- Release titles show only the version.
- Faster start-up and a smaller app on low-end devices: Android APK from 15.5 MB to 4.2 MB, a smaller Windows installer, and fonts without unused icons.

Earlier releases are listed on the [releases page](https://github.com/baros4294/parchment/releases).
