package io.parchment.ai.platform

import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.text.font.FontFamily
import io.parchment.ai.harness.WorkspaceContextCache
import io.parchment.ai.model.StorageFileCategory
import java.io.File
import java.nio.file.Files
import java.util.concurrent.ConcurrentHashMap
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

class PlatformSharedHelpersTest {
    private fun desktopConfig() = WorkspaceScanConfig(
        missingDirMessage = { "Directory does not exist: $it" },
        notDirMessage = { "Path is not a directory: $it" },
        emptyDirectoryMessage = "(Empty directory)",
        useTreeCache = true,
        canonicalizeRelativize = false,
        allowEmptyPlainGlobFallback = false,
        requirePositiveSearchFileBytes = true,
        maxSearchFileBytes = 2_000_000L,
        maxSearchHits = 150,
        maxSearchLineChars = 200,
        parallelSearch = true,
        filterSearchByRelPath = true
    )

    private fun androidConfig() = WorkspaceScanConfig(
        missingDirMessage = { "Directory not found: $it" },
        notDirMessage = { "Directory not found: $it" },
        emptyDirectoryMessage = "(Empty workspace directory)",
        useTreeCache = false,
        canonicalizeRelativize = true,
        allowEmptyPlainGlobFallback = true,
        requirePositiveSearchFileBytes = false,
        maxSearchFileBytes = 1_499_999L,
        maxSearchHits = 100,
        maxSearchLineChars = 180,
        parallelSearch = false,
        filterSearchByRelPath = false
    )

    @Test
    fun storageInventoryListsCategorizesAndDeletesSafely() {
        val tempRoot = Files.createTempDirectory("parchment-storage-inv-").toFile()
        val otaDir = Files.createTempDirectory("parchment-ota-inv-").toFile()
        try {
            File(tempRoot, "chats/s1.json").apply { parentFile.mkdirs(); writeText("{}") }
            File(tempRoot, "media/att-photo.png").apply { parentFile.mkdirs(); writeText("att") }
            File(tempRoot, "media/gen-art.png").apply { parentFile.mkdirs(); writeText("gen") }
            File(tempRoot, "settings.json").writeText("{}")
            File(tempRoot, "pending.tmp").writeText("tmp")
            File(otaDir, "update.exe").writeText("exe")

            val outsideFile = File(tempRoot.parentFile, "outside-${System.nanoTime()}.txt").apply {
                writeText("keep")
            }
            try {
                val items = AppStorageInventory.listFiles(tempRoot, extraOtaDir = otaDir)
                val byRel = items.associateBy { it.relativePath }
                assertEquals(StorageFileCategory.CHAT_SESSION, byRel["chats/s1.json"]?.category)
                assertEquals(StorageFileCategory.UPLOADED_MEDIA, byRel["media/att-photo.png"]?.category)
                assertEquals(StorageFileCategory.GENERATED_MEDIA, byRel["media/gen-art.png"]?.category)
                assertEquals(StorageFileCategory.SETTINGS_AND_BACKUPS, byRel["settings.json"]?.category)
                assertEquals(StorageFileCategory.OTA_AND_TEMP, byRel["pending.tmp"]?.category)
                assertEquals(StorageFileCategory.OTA_AND_TEMP, byRel["temp/parchment-ota/update.exe"]?.category)

                val deletedCallbacks = mutableListOf<String>()
                assertFalse(
                    AppStorageInventory.deleteFile(outsideFile.absolutePath, tempRoot, otaDir) {
                        deletedCallbacks.add(it)
                    }
                )
                assertTrue(outsideFile.exists())
                assertTrue(deletedCallbacks.isEmpty())

                val targetMedia = File(tempRoot, "media/gen-art.png")
                assertTrue(
                    AppStorageInventory.deleteFile(targetMedia.absolutePath, tempRoot, otaDir) {
                        deletedCallbacks.add(it)
                    }
                )
                assertFalse(targetMedia.exists())
                assertEquals(listOf(targetMedia.canonicalFile.absolutePath), deletedCallbacks)

                val extraTarget = File(otaDir, "update.exe")
                assertTrue(
                    AppStorageInventory.deleteFile(extraTarget.absolutePath, tempRoot, otaDir) {}
                )
                assertFalse(extraTarget.exists())
            } finally {
                outsideFile.delete()
            }
        } finally {
            tempRoot.deleteRecursively()
            otaDir.deleteRecursively()
        }
    }

    @Test
    fun storageInventoryWipeRespectsPreserveRootAndFontCacheDeduplicates() {
        val dirA = Files.createTempDirectory("parchment-wipe-keep-").toFile()
        val dirB = Files.createTempDirectory("parchment-wipe-drop-").toFile()
        try {
            val childA = File(dirA, "child.txt").apply { writeText("a") }
            val childB = File(dirB, "child.txt").apply { writeText("b") }
            assertTrue(AppStorageInventory.wipeDirectory(dirA, preserveRoot = true))
            assertTrue(dirA.exists())
            assertFalse(childA.exists())

            assertTrue(AppStorageInventory.wipeDirectory(dirB, preserveRoot = false))
            assertFalse(dirB.exists())
            assertFalse(childB.exists())

            val fontFile = File(dirA, "dummy.ttf").apply { writeText("font") }
            val fontCache = ConcurrentHashMap<String, FontFamily>()
            var buildCount = 0
            assertNull(AppStorageInventory.loadCachedFontFamily(fontCache, "   ") { FontFamily.SansSerif })
            assertNull(
                AppStorageInventory.loadCachedFontFamily(
                    fontCache,
                    File(dirA, "missing.ttf").absolutePath
                ) { FontFamily.SansSerif }
            )

            val first = AppStorageInventory.loadCachedFontFamily(fontCache, " \"${fontFile.absolutePath}\" ") {
                buildCount++
                FontFamily.Serif
            }
            val second = AppStorageInventory.loadCachedFontFamily(fontCache, fontFile.absolutePath) {
                buildCount++
                FontFamily.Monospace
            }
            assertNotNull(first)
            assertSame(first, second)
            assertEquals(1, buildCount)
        } finally {
            dirA.deleteRecursively()
            dirB.deleteRecursively()
        }
    }

    @Test
    fun boundedImageCacheCachesHitsEvictsByCountAndBytesAndSupportsRemoval() {
        val tempRoot = Files.createTempDirectory("parchment-img-cache-").toFile()
        try {
            val file1 = File(tempRoot, "one.png").apply { writeText("1") }
            val file2 = File(tempRoot, "two.png").apply { writeText("2") }
            val file3 = File(tempRoot, "three.png").apply { writeText("3") }
            val resolvedMap = mapOf(
                "one.png" to file1,
                "two.png" to file2,
                "three.png" to file3,
                file1.absolutePath to file1,
                file2.absolutePath to file2,
                file3.absolutePath to file3
            )
            var decodeCalls = 0
            // Each 1024x512 RGBA image = 2 MiB; limit = 3 images / 4 MiB
            val cache = BoundedImageCache { 3 to (4L * 1024L * 1024L) }

            assertNull(cache.getOrLoad("   ", { resolvedMap[it] }, { ImageBitmap(16, 16) }))
            assertNull(cache.getOrLoad("missing.png", { resolvedMap[it] }, { ImageBitmap(16, 16) }))
            assertNull(cache.getOrLoad("one.png", { resolvedMap[it] }, { null }))

            val bmp1 = cache.getOrLoad(file1.absolutePath, { resolvedMap[it] }) {
                decodeCalls++
                ImageBitmap(1024, 512)
            }
            assertNotNull(bmp1)
            assertSame(
                bmp1,
                cache.getOrLoad(file1.absolutePath, { resolvedMap[it] }) {
                    decodeCalls++
                    ImageBitmap(16, 16)
                }
            )
            assertEquals(1, decodeCalls)

            cache.getOrLoad(file2.absolutePath, { resolvedMap[it] }) {
                decodeCalls++
                ImageBitmap(1024, 512)
            }
            // Adding a 3rd 2 MiB bitmap exceeds 4 MiB total, evicting eldest (file1)
            cache.getOrLoad(file3.absolutePath, { resolvedMap[it] }) {
                decodeCalls++
                ImageBitmap(1024, 512)
            }
            assertNull(cache.get(file1.absolutePath))
            assertNotNull(cache.get(file2.absolutePath))
            assertNotNull(cache.get(file3.absolutePath))

            cache.remove(file2.absolutePath)
            assertNull(cache.get(file2.absolutePath))
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun workspaceTreeScannerListsGlobsSearchesAndInvalidatesCache() {
        val root = Files.createTempDirectory("parchment-scanner-").toFile()
        try {
            File(root, "src/App.kt").apply {
                parentFile.mkdirs()
                writeText("fun greet() = \"hello world\"\n")
            }
            File(root, "notes/readme.md").apply {
                parentFile.mkdirs()
                writeText("other content\n")
            }

            val desktopScanner = WorkspaceTreeScanner(desktopConfig())
            val flat = desktopScanner.list(root.absolutePath, "", recursive = false).getOrThrow()
            assertTrue(flat.contains("notes/"))
            assertTrue(flat.contains("src/"))

            val globbed = desktopScanner.glob(root.absolutePath, "**/*.kt", maxResults = 10).getOrThrow()
            assertTrue(globbed.contains("src/App.kt"))
            assertFalse(globbed.contains("readme.md"))

            val hit = desktopScanner.search(root.absolutePath, "hello\\s+world", "*.kt").getOrThrow()
            assertTrue(hit.contains("src/App.kt:1: fun greet()"))

            val beforeInvalidate = desktopScanner.list(root.absolutePath, "", recursive = true).getOrThrow()
            assertTrue(beforeInvalidate.contains("src/App.kt"))

            File(root, "src/Added.kt").writeText("fun added() = true\n")
            // Cached tree still active until invalidated
            val stillCached = desktopScanner.list(root.absolutePath, "", recursive = true).getOrThrow()
            assertFalse(stillCached.contains("src/Added.kt"))

            desktopScanner.invalidate()
            val afterInvalidate = desktopScanner.list(root.absolutePath, "", recursive = true).getOrThrow()
            assertTrue(afterInvalidate.contains("src/Added.kt"))

            val missingDesktop = desktopScanner.list(root.absolutePath, "nope", recursive = false)
            assertTrue(missingDesktop.exceptionOrNull()?.message?.contains("Directory does not exist: nope") == true)

            val androidScanner = WorkspaceTreeScanner(androidConfig())
            val missingAndroid = androidScanner.list(root.absolutePath, "nope", recursive = false)
            assertTrue(missingAndroid.exceptionOrNull()?.message?.contains("Directory not found: nope") == true)
            val androidSearch = androidScanner.search(root.absolutePath, "added", "*.kt").getOrThrow()
            assertTrue(androidSearch.contains("src/Added.kt:1:"))
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun desktopOwnedShellExecutesCapturesOutputAndRespectsPreCancellation() {
        val root = Files.createTempDirectory("parchment-shell-").toFile()
        try {
            val cancelled = DesktopOwnedShell.execute(
                workspaceRoot = root.absolutePath,
                command = "echo never",
                timeoutSeconds = 10,
                onOutputDelta = null,
                isCancelled = { true }
            )
            assertTrue(cancelled.isFailure)
            assertTrue(cancelled.exceptionOrNull()?.message?.contains("cancelled before shell startup") == true)

            val deltas = mutableListOf<String>()
            val ok = DesktopOwnedShell.execute(
                workspaceRoot = root.absolutePath,
                command = "echo parchment-shell-ok",
                timeoutSeconds = 15,
                onOutputDelta = { deltas.add(it) },
                isCancelled = { false }
            ).getOrThrow()
            assertTrue(ok.contains("Exit Code: 0"))
            assertTrue(ok.contains("parchment-shell-ok"))
            assertTrue(deltas.any { it.contains("parchment-shell-ok") })
        } finally {
            root.deleteRecursively()
        }
    }

    @Test
    fun globAndSearchHandleWildcardsAndUtf8UrlHelpers() {
        val root = Files.createTempDirectory("parchment-glob-utf8-").toFile()
        try {
            File(root, "src/App.kt").apply { parentFile.mkdirs(); writeText("fun greet() = \"hello\"\n") }
            File(root, "build.gradle.kts").writeText("fun greet() = \"kts\"\n")

            val desktopScanner = WorkspaceTreeScanner(desktopConfig())
            val androidScanner = WorkspaceTreeScanner(androidConfig())
            val desktopGlob = desktopScanner.glob(root.absolutePath, "*.kt", maxResults = 10).getOrThrow()
            val androidGlob = androidScanner.glob(root.absolutePath, "*.kt", maxResults = 10).getOrThrow()
            assertTrue("src/App.kt" in desktopGlob)
            assertFalse("build.gradle.kts" in desktopGlob)
            assertFalse("build.gradle.kts" in androidGlob)

            val search = desktopScanner.search(root.absolutePath, "hello", "**/*.kt").getOrThrow()
            assertTrue("src/App.kt:1:" in search, search)
            assertFalse("build.gradle.kts" in search, search)

            val utf8Cmd = if (System.getProperty("os.name").orEmpty().startsWith("Windows", true)) {
                "Write-Output 'café — 世界'"
            } else {
                "printf 'café — 世界\\n'"
            }
            val utf8Shell = DesktopOwnedShell.execute(root.absolutePath, utf8Cmd, 15, null) { false }.getOrThrow()
            assertTrue("café — 世界" in utf8Shell, utf8Shell)
        } finally {
            root.deleteRecursively()
        }

        assertEquals("café 😀+?", HtmlTextConverter.urlDecodeComponent("café%20😀%2B?"))
        assertEquals("What_If%3F_100%25_%231", HtmlTextConverter.encodeWikiTitle("What If? 100% #1"))
        assertEquals(
            "HTTPS://example.com/a?b=1",
            HtmlTextConverter.unwrapDuckDuckGoRedirectUrl("//duckduckgo.com/l/?uddg=HTTPS%3A%2F%2Fexample.com%2Fa%3Fb%3D1")
        )
        val md = HtmlTextConverter.convertHtmlToReadableText(
            "<html><body><a href=\"httpx://bad\">Bad</a> <a href=\"https://ok.test\">Good</a></body></html>"
        )
        assertFalse("httpx://bad" in md, md)
        assertTrue("[Good](https://ok.test)" in md, md)
    }

    @Test
    fun boundedImageCacheUsesTrueLruAndNormalizesAliases() {
        val tempRoot = Files.createTempDirectory("parchment-img-lru-").toFile()
        try {
            val file1 = File(tempRoot, "one.png").apply { writeText("1") }
            val file2 = File(tempRoot, "two.png").apply { writeText("2") }
            val file3 = File(tempRoot, "three.png").apply { writeText("3") }
            val unnormalizedOne = File(tempRoot, "sub/../one.png").absolutePath
            val resolver = { key: String ->
                when (key) {
                    file1.absolutePath, unnormalizedOne, "portable://one.png" -> file1
                    file2.absolutePath, "portable://two.png" -> file2
                    file3.absolutePath -> file3
                    else -> null
                }
            }
            var decodeCalls = 0
            // Each 1024x512 RGBA image = 2 MiB; limit = 2 images / 4 MiB
            val cache = BoundedImageCache { 2 to (4L * 1024L * 1024L) }

            val bmp1 = cache.getOrLoad(file1.absolutePath, resolver) { decodeCalls++; ImageBitmap(1024, 512) }
            val bmp2 = cache.getOrLoad(file2.absolutePath, resolver) { decodeCalls++; ImageBitmap(1024, 512) }
            assertNotNull(bmp1)
            assertNotNull(bmp2)
            assertEquals(2, decodeCalls)

            // Access file1 through both an unnormalized path and a portable alias without re-decoding
            assertSame(bmp1, cache.get(unnormalizedOne))
            assertSame(bmp1, cache.getOrLoad("portable://one.png", resolver) { decodeCalls++; ImageBitmap(16, 16) })
            assertEquals(2, decodeCalls)

            // Loading file3 must evict file2 (LRU) while keeping file1 (MRU)
            val bmp3 = cache.getOrLoad(file3.absolutePath, resolver) { decodeCalls++; ImageBitmap(1024, 512) }
            assertNotNull(bmp3)
            assertSame(bmp1, cache.get(file1.absolutePath))
            assertSame(bmp1, cache.get("portable://one.png"))
            assertNull(cache.get(file2.absolutePath))

            // Removing via alias evicts canonical entry and frees bytes for reloading
            cache.remove("portable://one.png")
            assertNull(cache.get(file1.absolutePath))
            assertNull(cache.get("portable://one.png"))
            assertNotNull(cache.get(file3.absolutePath))
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun bundledFontKeysCanonicalizeAndLazyPaletteMathResolves() {
        assertEquals("latex", AppStorageInventory.canonicalBundledFontKey("LaTeX"))
        assertEquals("latex", AppStorageInventory.canonicalBundledFontKey("  katex "))
        assertEquals("latex", AppStorageInventory.canonicalBundledFontKey("math"))
        assertEquals("jetbrains_mono", AppStorageInventory.canonicalBundledFontKey("JetBrainsMono Nerd"))
        assertEquals("inter", AppStorageInventory.canonicalBundledFontKey("Inter"))
        assertEquals("", AppStorageInventory.canonicalBundledFontKey("Unknown Font"))

        val f1 = PlatformPickers.loadBundledFontFamily("LaTeX")
        val f2 = PlatformPickers.loadBundledFontFamily("katex")
        if (f1 != null) {
            assertSame(f1, f2)
        }

        val palette = io.parchment.ai.ui.buildPaletteFromConfig(io.parchment.ai.model.ThemeConfig())
        assertEquals(palette.mathFontFamily, palette.mathStyle.fontFamily)
    }

    @Test
    fun snapshotStorageFastHeaderCheckRejectsNonTopLevelSettingsKeysAndPreservesSaveChat() {
        val tempRoot = Files.createTempDirectory("parchment-snap-fast-").toFile()
        try {
            val storage = SnapshotFileStorage(directory = { tempRoot })
            listOf(
                "{}",
                """{"nested":{"profiles":[]}}""",
                """[{"profiles":[]}]""",
                """{"note":"profiles","other":"sessions"}"""
            ).forEach { invalid ->
                val result = kotlin.runCatching { storage.saveSettings(invalid) }
                assertTrue(result.isFailure, "Expected rejection for $invalid")
            }

            // Minimal valid snapshot with legacy dialect migration passes and round-trips
            val validMinimal = """{"profiles":[{"id":"p1","name":"Custom","protocol":"BYTEDANCE_ARK","defaultModel":"m1"}]}"""
            storage.saveSettings(validMinimal)
            val loaded = storage.loadSettings()
            assertNotNull(loaded)
            assertTrue("p1" in loaded)

            // saveChat single existing decode preserves chat, prevents id collision, and backs up corrupt chat JSON
            val chatOne = """{"id":"c1","title":"Hello","createdAt":1,"updatedAt":1,"profileId":"p1","messages":[]}"""
            storage.saveChat("c1", chatOne)
            val chatOneUpdated = """{"id":"c1","title":"Updated","createdAt":1,"updatedAt":2,"profileId":"p1","messages":[]}"""
            storage.saveChat("c1", chatOneUpdated)
            assertTrue("Updated" in storage.loadChat("c1").orEmpty())
            assertTrue(File(tempRoot, "chats/c1/chat.prev.json").isFile)

            val wrongId = """{"id":"c2","title":"Other","createdAt":1,"updatedAt":1,"profileId":"p1","messages":[]}"""
            assertTrue(kotlin.runCatching { storage.saveChat("c1", wrongId) }.isFailure)

            // Corrupt existing chat.json is backed up before overwriting with valid chatOneUpdated
            val chatFile = File(tempRoot, "chats/c1/chat.json")
            chatFile.writeText("{corrupt")
            storage.saveChat("c1", chatOneUpdated)
            assertTrue(File(tempRoot, "chats/c1/chat.corrupt.json").isFile)
            assertTrue("Updated" in storage.loadChat("c1").orEmpty())
        } finally {
            tempRoot.deleteRecursively()
        }
    }

    @Test
    fun workspaceScannerCapsHitsEarlyAndContextCacheEvictsLeastRecentlyUsedWorkspace() {
        val scanRoot = Files.createTempDirectory("parchment-scan-cap-").toFile()
        val roots = (1..9).map { Files.createTempDirectory("parchment-ctx-lru-$it-").toFile() }
        try {
            val manyLines = (1..100).joinToString("\n") { "line $it contains needle" }
            File(scanRoot, "big.txt").writeText(manyLines)
            val scanner = WorkspaceTreeScanner(desktopConfig().copy(maxSearchHits = 2))
            val searchResult = scanner.search(scanRoot.absolutePath, "needle", "").getOrThrow()
            assertEquals(2, searchResult.lines().size, "Expected hit cap to stop at 2 lines: $searchResult")
            val globLines = scanner.glob(scanRoot.absolutePath, "*.txt", 10).getOrThrow().lines()
            assertEquals(1, globLines.size)
            assertTrue(globLines.single().startsWith("big.txt ("), globLines.single())

            WorkspaceContextCache.invalidate()
            roots.forEachIndexed { idx, dir -> File(dir, "AGENTS.md").writeText("workspace-v1-${idx + 1}") }
            val firstRoot = roots.first()
            assertTrue("workspace-v1-1" in WorkspaceContextCache.discoverContextFiles(firstRoot.absolutePath))

            // Edit firstRoot on disk while still cached: still returns v1 before eviction
            File(firstRoot, "AGENTS.md").writeText("workspace-v2-1")
            assertTrue("workspace-v1-1" in WorkspaceContextCache.discoverContextFiles(firstRoot.absolutePath))

            // Populate 8 more distinct workspaces so firstRoot is evicted from the 8-entry LRU cache
            roots.drop(1).forEach { dir ->
                WorkspaceContextCache.discoverContextFiles(dir.absolutePath)
            }
            assertTrue(
                "workspace-v2-1" in WorkspaceContextCache.discoverContextFiles(firstRoot.absolutePath),
                "Expected evicted first workspace to reload updated AGENTS.md without manual invalidation"
            )
        } finally {
            WorkspaceContextCache.invalidate()
            scanRoot.deleteRecursively()
            roots.forEach { it.deleteRecursively() }
        }
    }
}
