import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// The shared module: all app code for desktop and Android. The Android APK itself (applicationId, signing,
// R8) is built by :androidApp, because AGP 9 no longer lets an application live in a multiplatform module.
plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.androidKmpLibrary)
    alias(libs.plugins.composeMultiplatform)
    alias(libs.plugins.composeCompiler)
    alias(libs.plugins.kotlinSerialization)
    // Dev-only hot reload for desktop UI work: ./gradlew :composeApp:hotRunDesktop (see docs/architecture.md).
    alias(libs.plugins.composeHotReload)
    alias(libs.plugins.ktlint)
}

ktlint {
    outputToConsole.set(true)
}

val manifestVersionProp = providers.fileContents(
    rootProject.layout.projectDirectory.file(".release-please-manifest.json"),
).asText.map { text ->
    Regex(""""\."\s*:\s*"([^"]+)"""").find(text)?.groupValues?.get(1)?.removePrefix("v")?.trim()
        ?.takeIf { it.isNotEmpty() }
        ?: error("Missing root '.' version in .release-please-manifest.json")
}
val appVersionProp = providers.gradleProperty("appVersion")
    .map { it.removePrefix("v").trim() }
    .orElse(manifestVersionProp)
    .zip(manifestVersionProp) { explicit, fallback -> explicit.ifEmpty { fallback } }
val githubRepoProp = providers.gradleProperty("githubRepo")
    .map { it.trim().ifEmpty { "baros4294/parchment" } }
    .orElse("baros4294/parchment")
// Set only by .github/workflows/build-installers.yml. Dev runs and local packages must never install an update over the real app.
val officialReleaseProp = providers.gradleProperty("officialRelease")
    .map { it.trim().toBoolean() }
    .orElse(false)

// Emit a tiny Java resource instead of a generated .kt source file so changing -PappVersion/-PofficialRelease
// never invalidates compileKotlinDesktop, compileDesktopTestKotlinDesktop, or compileAndroidMain in the Gradle build cache.
val generateBuildConfig = tasks.register("generateBuildConfig") {
    val outputDir = layout.buildDirectory.dir("generated/parchmentResources")
    inputs.property("appVersion", appVersionProp)
    inputs.property("githubRepo", githubRepoProp)
    inputs.property("officialRelease", officialReleaseProp)
    outputs.dir(outputDir)
    val ver = appVersionProp
    val repo = githubRepoProp
    val official = officialReleaseProp
    doLast {
        val file = outputDir.get().file("parchment-build.properties").asFile
        file.parentFile.mkdirs()
        file.writeText(
            "appVersion=${ver.get()}\n" +
                "githubRepo=${repo.get()}\n" +
                "officialRelease=${official.get()}\n"
        )
    }
}

kotlin {
    compilerOptions {
        freeCompilerArgs.add("-Xexpect-actual-classes")
    }

    android {
        namespace = "io.parchment.ai"
        compileSdk = libs.versions.android.compileSdk.get().toInt()
        minSdk = libs.versions.android.minSdk.get().toInt()
        // res/ holds the launcher icon, strings and the accessibility service config.
        androidResources { enable = true }
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }

    jvm("desktop") {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_21)
        }
    }

    sourceSets {
        val desktopMain = getByName("desktopMain")
        val androidMain = getByName("androidMain")
        desktopMain.kotlin.srcDir("src/jvmShared/kotlin")

        commonMain {
            resources.srcDir(generateBuildConfig.map { it.outputs.files.singleFile })
            dependencies {
                // Compose Foundation and UI only: no Material.
                implementation(libs.compose.runtime)
                implementation(libs.compose.foundation)
                implementation(libs.compose.ui)

                // Coroutines, Immutable Collections & JSON Serialization
                implementation(libs.kotlinx.collections.immutable)
                implementation(libs.kotlinx.coroutines.core)
                implementation(libs.kotlinx.serialization.json)

                // Ktor HTTP/2 & SSE Streaming Engine
                implementation(libs.ktor.client.core)
                implementation(libs.ktor.client.content.negotiation)
                implementation(libs.ktor.serialization.kotlinx.json)
            }
        }

        androidMain.dependencies {
            implementation(libs.androidx.activity.compose)
            implementation(libs.kotlinx.coroutines.android)
            implementation(libs.ktor.client.okhttp)
        }
        androidMain.kotlin.srcDir("src/jvmShared/kotlin")

        desktopMain.dependencies {
            implementation(compose.desktop.currentOs)
            implementation(libs.kotlinx.coroutines.swing)
            implementation(libs.ktor.client.okhttp)
        }

        commonTest.dependencies {
            implementation(kotlin("test"))
        }
    }
}

composeCompiler {
    stabilityConfigurationFiles.add(project.layout.projectDirectory.file("stability-config.conf"))
    // -Pparchment.composeMetrics=true writes the Compose compiler's stability and skippability reports to
    // build/compose-metrics, for checking that a hot composable (composer, streaming turn) can skip.
    if (providers.gradleProperty("parchment.composeMetrics").orNull == "true") {
        metricsDestination = layout.buildDirectory.dir("compose-metrics")
        reportsDestination = layout.buildDirectory.dir("compose-metrics")
    }
}

// The UI gallery renders every screen to PNG; cargo xtask passes -Pparchment.gallery=true only when it is wanted.
// Parallel test forks stay 1 locally (low-RAM safe) and scale on multi-core CI runners via -Pparchment.testForks=N;
// DesktopPlatformHost gives every Gradle worker JVM its own isolated temp data directory.
val renderGallery = providers.gradleProperty("parchment.gallery").orElse("false")
val testParallelForks = providers.gradleProperty("parchment.testForks")
    .map { (it.trim().toIntOrNull() ?: 1).coerceIn(1, 4) }
    .orElse(1)
tasks.withType<Test>().configureEach {
    systemProperty("parchment.gallery", renderGallery.get())
    maxParallelForks = testParallelForks.get()
}

val desktopJvmArgs = listOf(
    "-Xms8m",
    "-Xmx224m",
    "-Xss512k",
    "-XX:TieredStopAtLevel=1",
    "-XX:CICompilerCount=1",
    "-XX:InitialCodeCacheSize=4m",
    "-XX:ReservedCodeCacheSize=24m",
    "-XX:+UseShenandoahGC",
    "-XX:ShenandoahGCMode=generational",
    "-XX:ParallelGCThreads=2",
    "-XX:ConcGCThreads=1",
    "-XX:+ExplicitGCInvokesConcurrent",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:ShenandoahUncommitDelay=5000",
    "-XX:ShenandoahControlIntervalMin=50",
    "-XX:ShenandoahControlIntervalMax=900",
    "-XX:ShenandoahGuaranteedGCInterval=0",
    "-XX:+UseCompactObjectHeaders",
    "-Dsun.java2d.d3d=false",
    "-Dskiko.rendering.laf.global=false",
    "-Dskiko.vsync.enabled=true",
    "-Dskiko.swing.frame.pacing=true",
    "-Dkotlinx.coroutines.io.parallelism=16",
    // FFM downcalls (title bar, file/folder COM dialogs, memory check, keep-awake: WindowsNative) and Skia call
    // native code; JDK 25 warns without this and later JDKs will block it.
    "--enable-native-access=ALL-UNNAMED"
)

// Desktop JVM target is retained for headless Compose `desktopTest`, `UiGalleryTest`, and dev hot-reload (`hotRunDesktop`);
// Windows production installers ship the standalone native Rust GPUI shell (`crates/parchment-desktop`).
compose.desktop {
    application {
        mainClass = "io.parchment.ai.MainKt"
        jvmArgs(*desktopJvmArgs.toTypedArray())
    }
}
