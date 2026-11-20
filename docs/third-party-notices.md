# Third-party notices

Parchment is MIT licensed (see [LICENSE](../LICENSE)). It bundles these fonts, each under the [SIL Open Font License 1.1](https://openfontlicense.org) (OFL). The copyright lines are taken from the font files themselves. The full licence text ships with both apps beside the fonts, in [`fonts/OFL.txt`](../mobile/compose-app/src/commonMain/resources/fonts/OFL.txt). The fonts may be used, bundled and redistributed with the app; they may not be sold on their own.

| Font | Files | Copyright |
|---|---|---|
| Inter (subset, variable weight) | `Inter-Regular.ttf`, `Inter-Italic.ttf` | Copyright 2016 The Inter Project Authors ([rsms/inter](https://github.com/rsms/inter)) |
| JetBrains Mono (subset, programming ligatures kept) | `JetBrainsMono-*.ttf` | Copyright 2020 The JetBrains Mono Project Authors ([JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono)) |
| KaTeX fonts (unmodified) | `KaTeX_Main-*.ttf`, `KaTeX_Math-Italic.ttf` | Copyright (c) 2009-2010 Design Science, Inc.; Copyright (c) 2014-2018 Khan Academy |
| Noto Sans Math (subset) | `NotoSansMath-Regular.ttf` | Copyright 2022 Google LLC |

The Inter, JetBrains Mono, and Noto Sans Math files are subsets of the originals (none declare a Reserved Font Name under the OFL); the KaTeX fonts are unmodified.

Libraries the app is built with (Rust, GPUI, SQLite, Kotlin, Compose Multiplatform, Ktor, kotlinx, OkHttp) keep their own licences, listed in each project's repository.
