package io.parchment.ai

import java.util.Properties

actual object ParchmentBuildConfig {
    private val props: Properties by lazy {
        Properties().apply {
            val stream = ParchmentBuildConfig::class.java.getResourceAsStream("/parchment-build.properties")
                ?: Thread.currentThread().contextClassLoader?.getResourceAsStream("parchment-build.properties")
            stream?.use { load(it.reader(Charsets.UTF_8)) }
        }
    }

    actual val APP_VERSION: String by lazy {
        props.getProperty("appVersion")?.trim()?.takeIf { it.isNotEmpty() } ?: "2.1.0"
    }

    actual val DEFAULT_GITHUB_REPO: String by lazy {
        props.getProperty("githubRepo")?.trim()?.takeIf { it.isNotEmpty() } ?: "baros4294/parchment"
    }

    actual val OFFICIAL_RELEASE: Boolean by lazy {
        props.getProperty("officialRelease")?.trim()?.toBoolean() ?: false
    }
}
