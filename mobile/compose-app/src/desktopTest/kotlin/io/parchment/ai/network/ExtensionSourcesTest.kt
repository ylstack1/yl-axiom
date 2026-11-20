package io.parchment.ai.network

import io.parchment.ai.model.McpServerConfig
import io.parchment.ai.platform.LocalSkillSourceReader
import kotlinx.coroutines.runBlocking
import java.io.ByteArrayOutputStream
import java.nio.file.Files
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.*

class ExtensionSourcesTest {
    private val page = """{"servers":[
      {"server":{"name":"a/hosted","title":"Hosted","description":"Plain","remotes":[{"type":"streamable-http","url":"https://a.example/mcp"}]}},
      {"server":{"name":"b/token","description":"Needs a key","remotes":[{"type":"streamable-http","url":"https://b.example/mcp","headers":[{"name":"Authorization","isRequired":true,"isSecret":true}]}]}},
      {"server":{"name":"c/custom","remotes":[{"type":"streamable-http","url":"https://c.example/mcp","headers":[{"name":"X-Api-Key","isRequired":true}]}]}},
      {"server":{"name":"d/old","remotes":[{"type":"sse","url":"https://d.example/sse"}]}},
      {"server":{"name":"e/plain-http","remotes":[{"type":"streamable-http","url":"http://e.example/mcp"}]}},
      {"server":{"name":"f/package","packages":[{"registryType":"npm","identifier":"x"}]}},
      {"server":{"name":"a/hosted","remotes":[{"type":"streamable-http","url":"https://a.example/older"}]}}
    ],"metadata":{"nextCursor":"next-page","count":7}}"""

    @Test fun onlyServersThatCanBeConnectedAreListed() {
        val parsed = parseRegistryPage(page)
        assertEquals(listOf("a/hosted", "b/token"), parsed.servers.map { it.name })
        assertFalse(parsed.servers[0].needsToken)
        assertTrue(parsed.servers[1].needsToken)
        assertEquals("next-page", parsed.nextCursor)
        assertEquals(RegistryPage(emptyList(), ""), parseRegistryPage("[]"))
    }

    @Test fun githubLinksResolveToAFolder() {
        assertEquals(GitHubFolder("o", "r", "", ""), parseGitHubFolder("https://github.com/o/r"))
        assertEquals(GitHubFolder("o", "r", "main", "skills/pdf"), parseGitHubFolder("https://github.com/o/r/tree/main/skills/pdf/"))
        assertEquals(GitHubFolder("o", "r", "v1", "skills/pdf"), parseGitHubFolder("https://github.com/o/r/blob/v1/skills/pdf/SKILL.md?plain=1"))
        listOf(
            "https://evil.example/o/r",
            "http://github.com/o/r",
            "https://github.com/o",
            "https://github.com/o/r/issues/1",
            "https://github.com/o/r/tree/main/../x",
            "https://github.com.evil.example/o/r"
        ).forEach { assertNull(parseGitHubFolder(it), it) }
    }

    private val skill = "---\nname: demo\ndescription: Does a thing\n---\nBody"
    private val api = "https://api.github.com/repos/o/r/contents/skills/demo?ref=main"

    private fun entry(name: String, type: String = "file", path: String = "skills/demo/$name", url: String = "https://raw.githubusercontent.com/o/r/main/$name") = """{"name":"$name","type":"$type","path":"$path","download_url":${if (type == "file") "\"$url\"" else "null"}}"""

    private class Sandbox(val responses: Map<String, ByteArray>, val installed: Boolean = false) {
        val written = linkedMapOf<String, String>()
        suspend fun install(folder: GitHubFolder) = installSkillFromGitHub(
            folder,
            { installed },
            { path, text -> written[path] = text; Result.success("ok") },
            { url, _ -> responses[url] ?: error("Unexpected request: $url") }
        )
    }

    private val folder = GitHubFolder("o", "r", "main", "skills/demo")
    private fun raw(name: String) = "https://raw.githubusercontent.com/o/r/main/$name"

    @Test fun aSkillIsCopiedIntoTheWorkspace() = runBlocking<Unit> {
        val sandbox = Sandbox(
            mapOf(
                api to "[${entry("SKILL.md")},${entry("notes.md")},${entry("logo.png")}]".encodeToByteArray(),
                raw("SKILL.md") to skill.encodeToByteArray(),
                raw("notes.md") to "Notes".encodeToByteArray(),
                raw("logo.png") to byteArrayOf(0x89.toByte(), 0, 1, 2)
            )
        )
        assertEquals("Installed demo, without 1 binary file.", sandbox.install(folder).getOrThrow())
        assertEquals(setOf(".parchment/skills/demo/SKILL.md", ".parchment/skills/demo/notes.md"), sandbox.written.keys)
    }

    @Test fun theManifestIsWrittenLastSoAFailedCopyCanBeRetried() = runBlocking<Unit> {
        val responses = mapOf(
            api to "[${entry("SKILL.md")},${entry("notes.md")}]".encodeToByteArray(),
            raw("SKILL.md") to skill.encodeToByteArray(),
            raw("notes.md") to "Notes".encodeToByteArray()
        )
        val order = mutableListOf<String>()
        installSkillFromGitHub(folder, { false }, { path, _ -> order += path; Result.success("ok") }, { url, _ -> responses.getValue(url) }).getOrThrow()
        assertEquals(".parchment/skills/demo/SKILL.md", order.last())
    }

    @Test fun nothingIsWrittenUnlessTheWholeSkillIsValid() = runBlocking<Unit> {
        fun listing(vararg entries: String) = mapOf(api to "[${entries.joinToString(",")}]".encodeToByteArray())
        val cases = listOf(
            Sandbox(listing(entry("notes.md")) + (raw("notes.md") to "x".encodeToByteArray())) to "no SKILL.md",
            Sandbox(listing(entry("SKILL.md")) + (raw("SKILL.md") to "No header".encodeToByteArray())) to "valid header",
            Sandbox(listing(entry("SKILL.md")) + (raw("SKILL.md") to "---\nname: demo\n---\nBody".encodeToByteArray())) to "no description",
            Sandbox(listing(entry("SKILL.md")) + (raw("SKILL.md") to "---\nname: ../../evil\ndescription: x\n---\n".encodeToByteArray())) to "unusable name",
            Sandbox(listing(entry("SKILL.md")) + (raw("SKILL.md") to skill.encodeToByteArray()), installed = true) to "already installed",
            Sandbox(listing(entry("..%2Fx"), entry("SKILL.md"))) to "unsafe",
            Sandbox(listing(entry("../../../AGENTS.md"), entry("SKILL.md"))) to "unsafe",
            Sandbox(listing(entry(".."), entry("SKILL.md"))) to "unsafe",
            Sandbox(listing(entry("SKILL.md", url = "https://evil.example/SKILL.md"))) to "unexpected address",
            Sandbox(mapOf(api to """{"name":"file"}""".encodeToByteArray())) to "not a folder"
        )
        for ((sandbox, reason) in cases) {
            val failure = assertFails(reason) { sandbox.install(folder).getOrThrow() }
            assertTrue(reason in failure.message.orEmpty(), "$reason: ${failure.message}")
            assertTrue(sandbox.written.isEmpty(), reason)
        }
    }

    private fun buildZip(vararg entries: Pair<String, ByteArray>): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            for ((name, data) in entries) {
                zip.putNextEntry(ZipEntry(name))
                if (!name.endsWith("/")) zip.write(data)
                zip.closeEntry()
            }
        }
        return out.toByteArray()
    }

    @Test fun localFolderSkillInstallsSkipsBinaryAndIgnoresGitMetadata() = runBlocking<Unit> {
        val tmp = Files.createTempDirectory("parchment-local-skill-ok")
        try {
            val skillDir = tmp.resolve("offline-demo")
            Files.createDirectories(skillDir.resolve("references"))
            Files.createDirectories(skillDir.resolve("scripts"))
            Files.createDirectories(skillDir.resolve(".git"))
            Files.writeString(skillDir.resolve("SKILL.md"), skill)
            Files.writeString(skillDir.resolve("references/guide.md"), "# Guide")
            Files.writeString(skillDir.resolve("scripts/helper.py"), "print('inert')")
            Files.write(skillDir.resolve("logo.png"), byteArrayOf(0x89.toByte(), 0x50, 0x4E, 0x47, 0x00))
            Files.writeString(skillDir.resolve(".git/config"), "[core]")
            Files.writeString(skillDir.resolve(".DS_Store"), "junk")

            val bundle = LocalSkillSourceReader.readFolder(skillDir.toString()).getOrThrow()
            val written = linkedMapOf<String, String>()
            val order = mutableListOf<String>()
            val msg = installSkillFromLocalBundle(
                bundle = bundle,
                exists = { it in written },
                write = { path, text -> order += path; written[path] = text; Result.success("ok") }
            ).getOrThrow()

            assertEquals("Installed demo, without 1 binary file.", msg)
            assertEquals(
                setOf(
                    ".parchment/skills/demo/SKILL.md",
                    ".parchment/skills/demo/references/guide.md",
                    ".parchment/skills/demo/scripts/helper.py"
                ),
                written.keys
            )
            assertEquals(".parchment/skills/demo/SKILL.md", order.last(), "SKILL.md must be written last")

            // Re-installing into the same workspace must never overwrite an existing skill.
            val dup = assertFails {
                installSkillFromLocalBundle(bundle, exists = { it in written }, write = { _, _ -> Result.success("ok") }).getOrThrow()
            }
            assertTrue("already installed" in dup.message.orEmpty(), dup.message)
        } finally {
            tmp.toFile().deleteRecursively()
        }
    }

    @Test fun localFolderSkillEnforcesCountSizeDepthAndFilenameGuards() {
        val tmp = Files.createTempDirectory("parchment-local-skill-guards")
        try {
            // 1. > 64 files rejected
            val manyDir = tmp.resolve("too-many")
            Files.createDirectories(manyDir)
            Files.writeString(manyDir.resolve("SKILL.md"), skill)
            for (i in 1..65) Files.writeString(manyDir.resolve("f$i.txt"), "x")
            assertTrue("too many files" in LocalSkillSourceReader.readFolder(manyDir.toString()).exceptionOrNull()?.message.orEmpty())

            // 2. > 1 MB single file rejected
            val bigDir = tmp.resolve("too-big")
            Files.createDirectories(bigDir)
            Files.writeString(bigDir.resolve("SKILL.md"), skill)
            Files.write(bigDir.resolve("huge.txt"), ByteArray(1024 * 1024 + 16) { 'a'.code.toByte() })
            assertTrue("larger than 1 MB" in LocalSkillSourceReader.readFolder(bigDir.toString()).exceptionOrNull()?.message.orEmpty())

            // 3. Depth > 5 rejected
            val deepDir = tmp.resolve("too-deep")
            val nested = deepDir.resolve("a/b/c/d/e/f")
            Files.createDirectories(nested)
            Files.writeString(deepDir.resolve("SKILL.md"), skill)
            Files.writeString(nested.resolve("leaf.txt"), "deep")
            assertTrue("nested too deeply" in LocalSkillSourceReader.readFolder(deepDir.toString()).exceptionOrNull()?.message.orEmpty())

            // 4. Unsafe filename rejected
            val unsafeDir = tmp.resolve("unsafe-name")
            Files.createDirectories(unsafeDir)
            Files.writeString(unsafeDir.resolve("SKILL.md"), skill)
            Files.writeString(unsafeDir.resolve("bad\$name.txt"), "x")
            assertTrue("unsafe file name" in LocalSkillSourceReader.readFolder(unsafeDir.toString()).exceptionOrNull()?.message.orEmpty())
        } finally {
            tmp.toFile().deleteRecursively()
        }
    }

    @Test fun localZipSkillUnwrapsRootFolderAndBlocksTraversalAndZipBombs() = runBlocking<Unit> {
        val manifestNoName = "---\ndescription: Packed offline skill\n---\nBody"
        val wrappedZip = buildZip(
            "packed-skill/" to ByteArray(0),
            "packed-skill/SKILL.md" to manifestNoName.encodeToByteArray(),
            "packed-skill/notes.md" to "Hello".encodeToByteArray(),
            "__MACOSX/packed-skill/._SKILL.md" to byteArrayOf(0, 1, 2),
            "packed-skill/.DS_Store" to byteArrayOf(0, 1, 2)
        )
        assertTrue(isZipPayload("bundle.zip", wrappedZip))
        assertTrue(isZipPayload("no-ext-upload", wrappedZip))
        assertFalse(isZipPayload("SKILL.md", skill.encodeToByteArray()))

        val bundle = LocalSkillSourceReader.readZip("upload.zip", wrappedZip).getOrThrow()
        val written = linkedMapOf<String, String>()
        val msg = installSkillFromLocalBundle(
            bundle = bundle,
            exists = { false },
            write = { path, text -> written[path] = text; Result.success("ok") }
        ).getOrThrow()
        assertEquals("Installed packed-skill.", msg)
        assertEquals(setOf(".parchment/skills/packed-skill/SKILL.md", ".parchment/skills/packed-skill/notes.md"), written.keys)

        // Path traversal rejected
        val traversalZip = buildZip(
            "../escape.txt" to "pwn".encodeToByteArray(),
            "SKILL.md" to skill.encodeToByteArray()
        )
        assertTrue("unsafe file name" in LocalSkillSourceReader.readZip("bad.zip", traversalZip).exceptionOrNull()?.message.orEmpty())

        // Zip bomb (2 MB of highly compressible bytes inside a tiny zip) rejected by bounded decompression
        val bombZip = buildZip(
            "SKILL.md" to skill.encodeToByteArray(),
            "bomb.txt" to ByteArray(2 * 1024 * 1024) { '0'.code.toByte() }
        )
        assertTrue(bombZip.size < 32 * 1024, "Expected highly compressed test zip")
        assertTrue("larger than 1 MB" in LocalSkillSourceReader.readZip("bomb.zip", bombZip).exceptionOrNull()?.message.orEmpty())
    }

    @Test fun standaloneSkillFileInstallsCleanlyAndRejectsBinary() = runBlocking<Unit> {
        val written = linkedMapOf<String, String>()
        val ok = installSkillFromSingleFile(
            fileName = "SKILL.md",
            bytes = skill.encodeToByteArray(),
            exists = { false },
            write = { path, text -> written[path] = text; Result.success("ok") }
        ).getOrThrow()
        assertEquals("Installed demo.", ok)
        assertEquals(setOf(".parchment/skills/demo/SKILL.md"), written.keys)

        val binaryErr = installSkillFromSingleFile(
            fileName = "SKILL.md",
            bytes = byteArrayOf(0x00, 0x01, 0x02),
            exists = { false },
            write = { _, _ -> Result.success("ok") }
        ).exceptionOrNull()
        assertNotNull(binaryErr)
        assertTrue("UTF-8" in binaryErr.message.orEmpty(), binaryErr.message)
    }

    @Test fun mcpConfigJsonParsesStandardFormatsDeduplicatesAndEnforcesSafety() {
        val claudeJson = """
            {
              "mcpServers": {
                "filesystem": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\My Docs\\Project"]
                },
                "local-http": {
                  "url": "http://127.0.0.1:8080/mcp",
                  "headers": {
                    "Authorization": "Bearer secret-tok-42"
                  }
                },
                "bad-scheme": {
                  "url": "javascript:alert(1)"
                },
                "multiline-inject": {
                  "command": "node server.js\ncalc.exe"
                }
              }
            }
        """.trimIndent()

        // Desktop: imports both local stdio command and HTTP server, all disabled by default; rejects unsafe entries.
        val desktop = parseMcpConfigJson(claudeJson, supportsLocalCommands = true).getOrThrow()
        assertEquals(2, desktop.servers.size)
        assertEquals(0, desktop.skippedLocalCommandCount)
        assertEquals(2, desktop.skippedInvalidCount)
        assertTrue(desktop.servers.all { !it.enabled }, "Imported MCP servers must always default to enabled = false")

        val fsServer = desktop.servers.first { it.name == "filesystem" }
        assertEquals("""npx -y @modelcontextprotocol/server-filesystem "C:\My Docs\Project"""", fsServer.command)
        val httpServer = desktop.servers.first { it.name == "local-http" }
        assertEquals("http://127.0.0.1:8080/mcp", httpServer.url)
        assertEquals("secret-tok-42", httpServer.bearerToken)

        // Android (supportsLocalCommands = false): drops local command cleanly, keeps HTTP endpoint disabled.
        val android = parseMcpConfigJson(claudeJson, supportsLocalCommands = false).getOrThrow()
        assertEquals(1, android.servers.size)
        assertEquals("local-http", android.servers.single().name)
        assertFalse(android.servers.single().enabled)
        assertEquals(1, android.skippedLocalCommandCount)

        // VS Code `servers` map format also parses cleanly.
        val vsCodeJson = """{"servers":{"docs":{"type":"http","url":"https://docs.example.com/mcp"}}}"""
        val vsCode = parseMcpConfigJson(vsCodeJson, supportsLocalCommands = false).getOrThrow()
        assertEquals(1, vsCode.servers.size)
        assertEquals("docs", vsCode.servers.single().name)
        assertEquals("https://docs.example.com/mcp", vsCode.servers.single().url)
        assertFalse(vsCode.servers.single().enabled)

        // Merge deduplicates existing URLs and commands.
        val existing = listOf(McpServerConfig(id = "existing-1", name = "Existing HTTP", url = "http://127.0.0.1:8080/mcp", enabled = true))
        val (merged, addedCount) = mergeImportedMcpServers(existing, desktop.servers, idPrefix = "imp")
        assertEquals(1, addedCount)
        assertEquals(2, merged.size)
        assertTrue(merged.first().enabled, "Existing enabled server must stay untouched")
        assertFalse(merged.last().enabled, "Newly merged server must start disabled")

        // Desktop file picker command formatting quotes paths with spaces and prefixes script runtimes cleanly.
        assertEquals("""python "C:\My Scripts\mcp_server.py"""", formatPickedMcpCommand("""C:\My Scripts\mcp_server.py"""))
        assertEquals("""node "C:\My Tools\server.mjs"""", formatPickedMcpCommand("""C:\My Tools\server.mjs"""))
        assertEquals(""""C:\Program Files\MCP\server.exe"""", formatPickedMcpCommand("""C:\Program Files\MCP\server.exe"""))
        assertEquals("""/usr/local/bin/my-mcp""", formatPickedMcpCommand("""/usr/local/bin/my-mcp"""))
    }
}
