package io.parchment.ai

/** Build metadata loaded from `/parchment-build.properties` so version bumps never invalidate Kotlin compilation caches. */
expect object ParchmentBuildConfig {
    val APP_VERSION: String
    val DEFAULT_GITHUB_REPO: String
    /** True only for builds made by the release workflow; other builds never install updates. */
    val OFFICIAL_RELEASE: Boolean
}
