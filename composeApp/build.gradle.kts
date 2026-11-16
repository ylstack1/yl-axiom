import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

// The shared module: all app code for desktop and Android. The Android APK itself (applicationId, signing,
// R8) is built by :androidApp, because AGP 9 no longer lets an application live in a multiplatform module.
plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.androidKmpLibrary)
    alias(libs.plugins.composeMultiplatform)
    alias(libs.plugins.composeCompiler)
    alias(libs.plugins.kotlinSerialization)
    // Dev-only hot reload for desktop UI work: ./gradlew :composeApp:hotRunDesktop (see docs/ARCHITECTURE.md).
    alias(libs.plugins.composeHotReload)
}

val appVersionProp = (project.findProperty("appVersion") as? String)?.removePrefix("v")?.trim()
    ?.takeIf { it.isNotEmpty() } ?: "2.1.0"
val appVersionCodeProp = (project.findProperty("appVersionCode") as? String)?.toIntOrNull() ?: 200
val githubRepoProp = (project.findProperty("githubRepo") as? String)?.trim()
    ?.takeIf { it.isNotEmpty() } ?: "baros4294/parchment"
// Set only by .github/workflows/build-installers.yml. Dev runs and local packages must never install an update over the real app.
val officialReleaseProp = (project.findProperty("officialRelease") as? String)?.trim().toBoolean()

val generateBuildConfig = tasks.register("generateBuildConfig") {
    val outputDir = layout.buildDirectory.dir("generated/parchmentConfig")
    inputs.property("appVersion", appVersionProp)
    inputs.property("appVersionCode", appVersionCodeProp)
    inputs.property("githubRepo", githubRepoProp)
    inputs.property("officialRelease", officialReleaseProp)
    outputs.dir(outputDir)
    val ver = appVersionProp
    val code = appVersionCodeProp
    val repo = githubRepoProp
    val official = officialReleaseProp
    doLast {
        val file = outputDir.get().file("io/parchment/ai/ParchmentBuildConfig.kt").asFile
        file.parentFile.mkdirs()
        file.writeText(
            """
            package io.parchment.ai

            object ParchmentBuildConfig {
                const val APP_VERSION = "$ver"
                const val APP_VERSION_CODE = $code
                const val DEFAULT_GITHUB_REPO = "$repo"
                /** True only for builds made by the release workflow; other builds never install updates. */
                const val OFFICIAL_RELEASE = $official
            }
            """.trimIndent()
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
            kotlin.srcDir(generateBuildConfig.map { it.outputs.files.singleFile })
            dependencies {
            // Compose Foundation and UI only: no Material.
            implementation(libs.compose.runtime)
            implementation(libs.compose.foundation)
            implementation(libs.compose.ui)

            // Coroutines & JSON Serialization
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

// -Pparchment.composeMetrics=true writes the Compose compiler's stability and skippability reports to
// build/compose-metrics, for checking that a hot composable (composer, streaming turn) can skip.
if (providers.gradleProperty("parchment.composeMetrics").orNull == "true") {
    composeCompiler {
        metricsDestination = layout.buildDirectory.dir("compose-metrics")
        reportsDestination = layout.buildDirectory.dir("compose-metrics")
    }
}

// The UI gallery renders every screen to PNG; verify.py passes -Pparchment.gallery=true only when it is wanted.
val renderGallery = providers.gradleProperty("parchment.gallery").orElse("false")
tasks.withType<Test>().configureEach {
    systemProperty("parchment.gallery", renderGallery.get())
}

// JDK 25 builds ship without jmods (JEP 493), which is where Compose points ProGuard for the JDK's own classes.
// Extract the java.* modules from the runtime image once instead and hand them to ProGuard as library classes.
val jdkLibraryClasses = tasks.register<Exec>("extractJdkLibraryClasses") {
    val jdk = File(System.getProperty("java.home"))
    val outDir = layout.buildDirectory.dir("jdk-library-classes")
    val classesDir = outDir.map { it.dir("classes").asFile }
    val proFile = outDir.map { it.file("jdk-library.pro").asFile }
    inputs.file(jdk.resolve("lib/modules"))
    outputs.dir(classesDir)
    outputs.file(proFile)
    executable = jdk.resolve("bin/jimage" + if (System.getProperty("os.name").startsWith("Windows")) ".exe" else "").path
    argumentProviders.add(CommandLineArgumentProvider {
        listOf("extract", "--dir", classesDir.get().path,
            "--include", "regex:/(java\\.[a-z.]+|jdk\\.unsupported|jdk\\.crypto\\.ec)/.*\\.class", jdk.resolve("lib/modules").path)
    })
    doFirst { classesDir.get().deleteRecursively() }
    doLast {
        val modules = classesDir.get().listFiles()!!.filter { it.isDirectory }.sortedBy { it.name }
        proFile.get().writeText(modules.joinToString("\n") { "-libraryjars '${it.path.replace('\\', '/')}'(!module-info.class)" } + "\n")
    }
}

compose.desktop {
    application {
        mainClass = "io.parchment.ai.MainKt"

        // Lean JVM: a 32 MB starting heap with a 1 GB ceiling for 100-step tool turns and large attachments.
        // Serial GC, measured 2026-11-16 on the packaged app with 300 chats: idle private memory 269 -> 216 MB and
        // start 1.59 -> 1.40 s against G1, whose own bookkeeping (50 MB) outweighed the 33 MB heap. Pauses while
        // starting and hydrating stayed under 11 ms; a full collection only grows long near the 1 GB ceiling
        // (about 0.4 s with ~3 million live objects). The heap shrinks after full collections (MaxHeapFreeRatio),
        // and minimizing the window runs one (Main.kt), so a finished long turn hands its memory back.
        jvmArgs(
            "-Xms32m",
            "-Xmx1024m",
            "-XX:+UseSerialGC",
            "-XX:+UseStringDeduplication",
            "-XX:MinHeapFreeRatio=10",
            "-XX:MaxHeapFreeRatio=20",
            "-XX:-ShrinkHeapInSteps",
            "-XX:SoftRefLRUPolicyMSPerMB=250",
            // JDK 25 (JEP 519): 8-byte object headers instead of 12, a few percent less heap for the same chats.
            // The base class archive must be dumped with the same flag (classes_coh.jsa, made after jlink below).
            "-XX:+UseCompactObjectHeaders",
            // FFM downcalls (title bar, memory check, keep-awake: WindowsNative) and Skia call native code; JDK 25 warns
            // without this and later JDKs will block it.
            "--enable-native-access=ALL-UNNAMED"
        )

        // Releases (createReleaseDistributable) drop unused code from the jars. Shrinking only: no renaming or
        // optimization passes, which are where ProGuard tends to break Kotlin.
        buildTypes.release.proguard {
            version.set("7.10.0")
            isEnabled.set(true)
            obfuscate.set(false)
            optimize.set(false)
            configurationFiles.from(project.file("compose-desktop.pro"), jdkLibraryClasses.map { it.outputs.files.filter { f -> f.name.endsWith(".pro") } })
        }

        nativeDistributions {
            targetFormats(TargetFormat.Exe, TargetFormat.Msi)
            packageName = "Parchment"
            packageVersion = appVersionProp
            description = "Parchment — Minimal Bleeding-Edge Literary & Agent AI Frontend"
            vendor = "Parchment"

            // Minimal JDK runtime modules for small footprint
            modules(
                "java.base",
                "java.desktop",
                "jdk.crypto.ec",
                "java.naming",
                "jdk.unsupported"
            )

            windows {
                iconFile.set(rootProject.file("installer/parchment-icon.ico"))
                menuGroup = "Parchment"
                shortcut = true
                dirChooser = true
                perUserInstall = true
                upgradeUuid = "8d49c66a-9870-4c97-9499-684f84a8e432"
            }
        }
    }
}

// Faster desktop start: the bundled runtime gets a base class-data-sharing archive, and the packaged app keeps
// its own archive of the classes it loads beside its files (rebuilt automatically after an update).
// Packaged app only; the dev run is unaffected. Measured 2026-11-11: about 3.1 s to 2.0 s with 300 chats.
tasks.withType<org.jetbrains.compose.desktop.application.tasks.AbstractJLinkTask>().configureEach {
    freeArgs.add("--generate-cds-archive")
    // jlink builds the archive with the image's own java.exe, which --strip-native-commands removes first.
    // Compose has no public switch, so turn stripping off here (this fails loudly if Compose renames it)
    // and delete the launchers afterwards: the app starts through Parchment.exe and jvm.dll.
    @Suppress("UNCHECKED_CAST")
    (javaClass.getMethod("getStripNativeCommands\$compose").invoke(this) as Property<Boolean>).set(false)
    val runtimeBin = destinationDir.dir("bin")
    doLast {
        val bin = runtimeBin.get().asFile
        // jlink dumps only the default-header archive (classes.jsa). With -XX:+UseCompactObjectHeaders the JVM looks
        // for classes_coh.jsa and, missing it, silently runs without class sharing, so dump it here and fail loudly.
        val javaExe = listOf("java.exe", "java").map { bin.resolve(it) }.firstOrNull { it.isFile }
            ?: throw GradleException("jlink image has no java launcher to dump classes_coh.jsa with.")
        // Dumped with the app's native-access flag too: a mismatch makes the JVM drop the archived module graph.
        val dump = ProcessBuilder(
            javaExe.path, "-XX:+UseCompactObjectHeaders", "--enable-native-access=ALL-UNNAMED", "-Xshare:dump"
        ).redirectErrorStream(true).start()
        val dumpLog = dump.inputStream.bufferedReader().readText()
        if (dump.waitFor() != 0 || !bin.resolve("server/classes_coh.jsa").isFile) {
            throw GradleException("Dumping classes_coh.jsa failed: " + dumpLog.takeLast(2000))
        }
        bin.listFiles { file -> file.extension.equals("exe", ignoreCase = true) }?.forEach { it.delete() }
        // Unused at runtime: classes.jsa needs default headers and classes_nocoops.jsa needs compressed oops off
        // (heaps over about 32 GB). The app always runs with compact headers and a 1 GB heap.
        bin.resolve("server/classes.jsa").delete()
        bin.resolve("server/classes_nocoops.jsa").delete()
    }
}
tasks.withType<org.jetbrains.compose.desktop.application.tasks.AbstractJPackageTask>().configureEach {
    freeArgs.addAll("--java-options", "-XX:+AutoCreateSharedArchive", "--java-options", "-XX:SharedArchiveFile=\$APPDIR/parchment.jsa")
}
