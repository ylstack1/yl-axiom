# Changelog

What changed in each release, newest first. From 2.1.7 on, release-please writes these entries from commit messages that follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat:` and `fix:` appear here and set the next version); edit wording in its release PR before merging it.

## [2.2.6](https://github.com/baros4294/parchment/compare/v2.2.5...v2.2.6) (2026-11-16)


### Faster and lighter

* FFM native calls, CDS warm-up, chat stub index, prompt jump and parallel helpers ([b2ecff7](https://github.com/baros4294/parchment/commit/b2ecff7d57fdb8a6716850145736baa759924ad0))
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
