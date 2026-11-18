import java.util.concurrent.TimeUnit
import java.util.jar.JarEntry
import java.util.jar.JarFile
import java.util.jar.JarOutputStream
import java.util.zip.CRC32
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

val appVersionProp = providers.gradleProperty("appVersion")
    .map { it.removePrefix("v").trim().ifEmpty { "2.1.0" } }
    .orElse("2.1.0")
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
    stabilityConfigurationFiles.add(project.layout.projectDirectory.file("stability_config.conf"))
    // -Pparchment.composeMetrics=true writes the Compose compiler's stability and skippability reports to
    // build/compose-metrics, for checking that a hot composable (composer, streaming turn) can skip.
    if (providers.gradleProperty("parchment.composeMetrics").orNull == "true") {
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

val desktopJvmArgs = listOf(
    "-Xms8m",
    "-Xmx224m",
    "-Xss512k",
    "-XX:CICompilerCount=2",
    "-XX:Tier4InvocationThreshold=40000",
    "-XX:Tier4MinInvocationThreshold=20000",
    "-XX:Tier4CompileThreshold=50000",
    "-XX:InitialCodeCacheSize=6m",
    "-XX:ReservedCodeCacheSize=32m",
    "-XX:+UseShenandoahGC",
    "-XX:ShenandoahGCMode=generational",
    "-XX:ParallelGCThreads=2",
    "-XX:ConcGCThreads=1",
    "-XX:SoftMaxHeapSize=16m",
    "-XX:+ExplicitGCInvokesConcurrent",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:ShenandoahUncommitDelay=300",
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

compose.desktop {
    application {
        mainClass = "io.parchment.ai.MainKt"

        // Lean, pause-free JDK 25 runtime shared identically with Project Leyden AOT training below.
        jvmArgs(*desktopJvmArgs.toTypedArray())

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
            packageVersion = appVersionProp.get()
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

// Ahead-of-Time cache (Project Leyden JEP 483/514/515): jlink keeps java.exe temporarily so jpackage's doLast can run
// a headless startup + UI training pass (-XX:AOTCacheOutput=$APPDIR/parchment.aot), baking parsed classes and JIT
// method profiles directly into the shipped package before stripping CLI launchers and verifying -Xshare:on.
tasks.withType<org.jetbrains.compose.desktop.application.tasks.AbstractJLinkTask>().configureEach {
    @Suppress("UNCHECKED_CAST")
    (javaClass.getMethod("getStripNativeCommands\$compose").invoke(this) as Property<Boolean>).set(false)
}
tasks.withType<org.jetbrains.compose.desktop.application.tasks.AbstractJPackageTask>().configureEach {
    freeArgs.addAll("--java-options", "-XX:AOTCache=\$APPDIR/parchment.aot")
    val destDir = destinationDir
    val aotJvmArgs = ArrayList(desktopJvmArgs)
    doLast {
        val appRoot = destDir.get().asFile.resolve("Parchment")
        val launcherExe = appRoot.resolve("Parchment.exe")
        if (launcherExe.isFile) {
            val appDir = appRoot.resolve("app")
            val cfgFile = appDir.resolve("Parchment.cfg")
            val cfgLines = cfgFile.readLines()
            val mainJar = cfgLines.firstOrNull { it.startsWith("app.mainjar=") }?.substringAfter("app.mainjar=")?.trim().orEmpty()
            val extraJars = cfgLines.filter { it.startsWith("app.classpath=") }.map { it.substringAfter("app.classpath=").trim() }
            val jarFiles = (listOf(mainJar) + extraJars)
                .filter { it.isNotEmpty() }
                .map { entry ->
                    val rel = entry.replace("\$APPDIR\\", "").replace("\$APPDIR/", "")
                    appDir.resolve(rel)
                }
                .filter { it.isFile }
            val normalizedEpochMs = 1_700_000_000_000L
            val uberJar = appDir.resolve("parchment-all.jar")
            if (jarFiles.size > 1 || (jarFiles.size == 1 && jarFiles.single() != uberJar)) {
                // Repack all ~80 application & dependency jars into a single uncompressed (STORED) parchment-all.jar so
                // cold Explorer launches open 1 contiguous file through NTFS/Defender instead of 80 separate ZIP tables.
                val seenEntries = HashSet<String>(8192)
                val mergedServices = LinkedHashMap<String, LinkedHashSet<String>>()
                fun writeStoredEntry(jos: JarOutputStream, name: String, bytes: ByteArray) {
                    val crc = CRC32().apply { update(bytes) }.value
                    val entry = JarEntry(name).apply {
                        method = JarEntry.STORED
                        size = bytes.size.toLong()
                        compressedSize = bytes.size.toLong()
                        this.crc = crc
                        time = normalizedEpochMs
                    }
                    jos.putNextEntry(entry)
                    jos.write(bytes)
                    jos.closeEntry()
                }
                uberJar.outputStream().buffered(262_144).use { bos ->
                    JarOutputStream(bos).use { jos ->
                        writeStoredEntry(jos, "META-INF/MANIFEST.MF", "Manifest-Version: 1.0\r\n\r\n".toByteArray(Charsets.UTF_8))
                        seenEntries.add("META-INF/MANIFEST.MF")
                        for (jf in jarFiles) {
                            JarFile(jf).use { jar ->
                                val entries = jar.entries()
                                while (entries.hasMoreElements()) {
                                    val e = entries.nextElement()
                                    if (e.isDirectory) continue
                                    val name = e.name.trimStart('/')
                                    if (name.isEmpty() || name == "module-info.class" || name.endsWith("/module-info.class")) continue
                                    if (name.startsWith("META-INF/")) {
                                        val upper = name.uppercase()
                                        if (upper == "META-INF/MANIFEST.MF" ||
                                            upper.endsWith(".SF") ||
                                            upper.endsWith(".DSA") ||
                                            upper.endsWith(".RSA") ||
                                            upper.substringAfter("META-INF/").startsWith("SIG-")
                                        ) {
                                            continue
                                        }
                                        if (name.startsWith("META-INF/services/") && name.length > "META-INF/services/".length) {
                                            val lines = jar.getInputStream(e).bufferedReader(Charsets.UTF_8).use { it.readLines() }
                                            val bucket = mergedServices.getOrPut(name) { LinkedHashSet() }
                                            for (line in lines) {
                                                val trimmed = line.trim()
                                                if (trimmed.isNotEmpty() && !trimmed.startsWith("#")) bucket.add(trimmed)
                                            }
                                            continue
                                        }
                                    }
                                    if (seenEntries.add(name)) {
                                        val bytes = jar.getInputStream(e).use { it.readBytes() }
                                        writeStoredEntry(jos, name, bytes)
                                    }
                                }
                            }
                        }
                        for ((svcName, impls) in mergedServices) {
                            if (impls.isNotEmpty() && seenEntries.add(svcName)) {
                                val content = (impls.joinToString("\n") + "\n").toByteArray(Charsets.UTF_8)
                                writeStoredEntry(jos, svcName, content)
                            }
                        }
                    }
                }
                val sep = if (cfgLines.any { it.contains("\$APPDIR\\") }) "\\" else "/"
                val rewrittenCfg = buildList {
                    var insertedCp = false
                    for (line in cfgLines) {
                        if (line.startsWith("app.mainjar=") || line.startsWith("app.classpath=")) {
                            if (!insertedCp) {
                                add("app.classpath=\$APPDIR${sep}parchment-all.jar")
                                insertedCp = true
                            }
                        } else {
                            add(line)
                            if (line.trim() == "[Application]" && !insertedCp) {
                                add("app.classpath=\$APPDIR${sep}parchment-all.jar")
                                insertedCp = true
                            }
                        }
                    }
                }
                cfgFile.writeText(rewrittenCfg.joinToString("\n") + "\n")
                for (jf in jarFiles) {
                    if (jf != uberJar) jf.delete()
                }
            }
            val orderedCp = uberJar.takeIf { it.isFile }?.absolutePath
                ?: jarFiles.joinToString(File.pathSeparator) { it.absolutePath }
            val runtimeBin = appRoot.resolve("runtime/bin")
            val javaExe = listOf("java.exe", "java").map { runtimeBin.resolve(it) }.firstOrNull { it.isFile }
                ?: throw GradleException("jlink image has no java launcher to build parchment.aot.")
            val aotFile = appDir.resolve("parchment.aot")
            aotFile.delete()
            // Inno Setup strips sub-second NTFS FILETIME ticks, while HotSpot's Windows AOT path-table validation checks
            // exact timestamps (`timestamp has changed` -> unmaps parchment.aot). Pin packaged files to an even-second
            // zero-nanosecond UTC epoch BEFORE building parchment.aot so installed files match bit-for-bit everywhere.
            appRoot.walkTopDown().filter { it.isFile }.forEach { file ->
                if (!file.setLastModified(normalizedEpochMs)) {
                    throw GradleException("Failed to normalize timestamp on ${file.absolutePath}")
                }
            }
            val trainLogFile = File.createTempFile("parchment-aot-train-", ".log")
            val train = ProcessBuilder(
                listOf(
                    javaExe.absolutePath,
                    "-Dskiko.library.path=${appDir.absolutePath}",
                    "-Djava.library.path=${appDir.absolutePath}"
                ) +
                    aotJvmArgs +
                    listOf(
                        "-XX:AOTCacheOutput=${aotFile.absolutePath}",
                        "-cp",
                        orderedCp,
                        "io.parchment.ai.MainKt",
                        "--cds-warmup"
                    )
            ).redirectErrorStream(true).redirectOutput(trainLogFile).start()
            val trainDone = train.waitFor(90, TimeUnit.SECONDS)
            if (!trainDone) train.destroyForcibly()
            val trainLog = runCatching { trainLogFile.readText() }.getOrDefault("")
            trainLogFile.delete()
            if (!trainDone || train.exitValue() != 0 || !aotFile.isFile) {
                throw GradleException("Building parchment.aot failed:\n" + trainLog.takeLast(3000))
            }
            runtimeBin.listFiles { file -> file.extension.equals("exe", ignoreCase = true) }?.forEach { it.delete() }
            runtimeBin.resolve("server").listFiles { file -> file.extension.equals("jsa", ignoreCase = true) }?.forEach { it.delete() }
            aotFile.setLastModified(normalizedEpochMs)
            val verifyLogFile = File.createTempFile("parchment-aot-verify-", ".log")
            val verifyPb = ProcessBuilder(launcherExe.absolutePath, "--aot-verify")
                .redirectErrorStream(true)
                .redirectOutput(verifyLogFile)
            verifyPb.environment()["JAVA_TOOL_OPTIONS"] = "-Xlog:aot*=info"
            val verifyProc = verifyPb.start()
            val verifyDone = verifyProc.waitFor(30, TimeUnit.SECONDS)
            if (!verifyDone) verifyProc.destroyForcibly()
            val verifyLog = runCatching { verifyLogFile.readText() }.getOrDefault("")
            verifyLogFile.delete()
            if (
                !verifyDone ||
                verifyProc.exitValue() != 0 ||
                "Using AOT-linked classes: true" !in verifyLog ||
                "Unable to map shared spaces" in verifyLog
            ) {
                throw GradleException("Parchment.exe failed to map AOT-linked parchment.aot:\n" + verifyLog.takeLast(2500))
            }
        }
    }
}
