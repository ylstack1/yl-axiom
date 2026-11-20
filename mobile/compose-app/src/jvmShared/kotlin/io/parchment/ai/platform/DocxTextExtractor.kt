package io.parchment.ai.platform

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.util.zip.ZipInputStream

/**
 * Zero-dependency `.docx` text extractor using JDK/Android `ZipInputStream` and an XXE-free
 * streaming tag walker over `word/document.xml` (no DOM parser, 0 KB APK growth).
 *
 * Enforces bounded zip decompression (16 MiB XML ceiling, 8 MiB extracted text cap) to prevent
 * zip bombs while preserving paragraph breaks, line breaks, tabs, and table cells.
 */
internal object DocxTextExtractor {
    private const val MAX_ARCHIVE_BYTES = 25 * 1024 * 1024
    private const val MAX_DOCUMENT_XML_BYTES = 16 * 1024 * 1024
    private const val MAX_EXTRACTED_CHARS = 8 * 1024 * 1024
    private const val MAX_ZIP_ENTRIES = 512

    fun extractText(bytes: ByteArray): Result<String> = runCatching {
        require(bytes.size >= 4 && bytes[0] == 0x50.toByte() && bytes[1] == 0x4B.toByte()) {
            "File is not a valid .docx archive."
        }
        check(bytes.size <= MAX_ARCHIVE_BYTES) { "Document archive exceeds 25 MB limit." }

        var documentXml: ByteArray? = null
        var entriesSeen = 0
        ZipInputStream(ByteArrayInputStream(bytes)).use { zip ->
            val buf = ByteArray(16 * 1024)
            while (true) {
                val entry = zip.nextEntry ?: break
                if (++entriesSeen > MAX_ZIP_ENTRIES) error("Archive has too many entries.")
                val norm = entry.name.replace('\\', '/').trimStart('/')
                if (".." in norm.split('/') || '\u0000' in norm) {
                    error("Archive contains an unsafe entry path.")
                }
                if (!entry.isDirectory && norm.equals("word/document.xml", ignoreCase = true)) {
                    val out = ByteArrayOutputStream()
                    var total = 0
                    while (true) {
                        val read = zip.read(buf)
                        if (read <= 0) break
                        total += read
                        check(total <= MAX_DOCUMENT_XML_BYTES) { "word/document.xml exceeds 16 MB safety limit." }
                        out.write(buf, 0, read)
                    }
                    documentXml = out.toByteArray()
                    zip.closeEntry()
                    break
                }
                zip.closeEntry()
            }
        }
        val xmlBytes = documentXml ?: error("Document archive has no word/document.xml.")
        val xml = xmlBytes.decodeToString()
        val extracted = extractTextFromDocumentXml(xml)
        require(extracted.isNotBlank()) { "No readable text found in word/document.xml." }
        extracted
    }

    internal fun extractTextFromDocumentXml(xml: String): String {
        val out = StringBuilder(minOf(xml.length / 2, 64 * 1024))
        val paragraph = StringBuilder()
        var inTextRun = false
        var skipDepth = 0
        var inTableCell = false
        var cellHasContent = false
        var i = 0
        val n = xml.length

        fun flushParagraph() {
            val trimmed = paragraph.toString().trimEnd(' ', '\t')
            if (trimmed.isNotEmpty()) {
                if (out.isNotEmpty() && out.last() != '\n') out.append('\n')
                out.append(trimmed)
                out.append('\n')
            } else if (out.isNotEmpty() && !out.endsWith("\n\n")) {
                out.append('\n')
            }
            paragraph.clear()
        }

        while (i < n && out.length + paragraph.length < MAX_EXTRACTED_CHARS) {
            val ch = xml[i]
            if (ch == '<') {
                // Skip XML comments and CDATA cleanly
                if (xml.startsWith("<!--", i)) {
                    val end = xml.indexOf("-->", i + 4)
                    i = if (end < 0) n else end + 3
                    continue
                }
                if (xml.startsWith("<![CDATA[", i)) {
                    val end = xml.indexOf("]]>", i + 9)
                    val endIdx = if (end < 0) n else end
                    if (inTextRun && skipDepth == 0) {
                        paragraph.append(xml, i + 9, endIdx)
                        if (inTableCell) cellHasContent = true
                    }
                    i = if (end < 0) n else end + 3
                    continue
                }
                val close = xml.indexOf('>', i + 1)
                if (close < 0) break
                val rawTag = xml.substring(i + 1, close).trim()
                i = close + 1
                if (rawTag.isEmpty() || rawTag.startsWith("?") || rawTag.startsWith("!")) continue

                val isEnd = rawTag.startsWith("/")
                val isSelfClosing = !isEnd && rawTag.endsWith("/")
                val nameBody = rawTag
                    .removePrefix("/")
                    .removeSuffix("/")
                    .trim()
                    .substringBefore(' ')
                    .substringBefore('\t')
                    .substringBefore('\n')
                    .substringBefore('\r')
                val localName = nameBody.substringAfter(':', nameBody).lowercase()

                if (localName == "deltext" || localName == "instrtext" || localName == "fallback") {
                    if (!isSelfClosing) {
                        if (isEnd) skipDepth = (skipDepth - 1).coerceAtLeast(0) else skipDepth++
                    }
                    continue
                }
                if (skipDepth > 0) continue

                when (localName) {
                    "t" -> if (!isSelfClosing) inTextRun = !isEnd
                    "tab" -> if (!isEnd) {
                        paragraph.append('\t')
                        if (inTableCell) cellHasContent = true
                    }
                    "br", "cr" -> if (!isEnd) paragraph.append('\n')
                    "tc" -> {
                        if (!isEnd && !isSelfClosing) {
                            inTableCell = true
                            cellHasContent = false
                        } else if (isEnd) {
                            if (cellHasContent) paragraph.append('\t')
                            inTableCell = false
                            cellHasContent = false
                        }
                    }
                    "tr" -> if (isEnd) flushParagraph()
                    "p" -> if (!isEnd && !isSelfClosing && inTableCell && cellHasContent) {
                        paragraph.append(' ')
                    } else if (isEnd && !inTableCell) {
                        flushParagraph()
                    }
                }
            } else {
                val nextTag = xml.indexOf('<', i).let { if (it < 0) n else it }
                if (inTextRun && skipDepth == 0) {
                    val segment = decodeXmlEntities(xml.substring(i, nextTag))
                    if (segment.isNotEmpty()) {
                        paragraph.append(segment)
                        if (inTableCell) cellHasContent = true
                    }
                }
                i = nextTag
            }
        }
        if (paragraph.isNotEmpty()) flushParagraph()
        return out.toString()
            .replace(Regex("\n{3,}"), "\n\n")
            .trim()
            .take(MAX_EXTRACTED_CHARS)
    }

    internal fun decodeXmlEntities(raw: String): String {
        if ('&' !in raw) return raw
        val sb = StringBuilder(raw.length)
        var i = 0
        val n = raw.length
        while (i < n) {
            if (raw[i] == '&') {
                val semi = raw.indexOf(';', i + 1)
                if (semi > i + 1 && semi - i <= 12) {
                    val entity = raw.substring(i + 1, semi)
                    val decoded: String? = when (entity) {
                        "amp" -> "&"
                        "lt" -> "<"
                        "gt" -> ">"
                        "quot" -> "\""
                        "apos" -> "'"
                        else -> if (entity.startsWith("#x", ignoreCase = true)) {
                            entity.substring(2).toIntOrNull(16)?.let { code ->
                                if (code in 0x09..0x10FFFF) String(Character.toChars(code)) else null
                            }
                        } else if (entity.startsWith("#")) {
                            entity.substring(1).toIntOrNull(10)?.let { code ->
                                if (code in 0x09..0x10FFFF) String(Character.toChars(code)) else null
                            }
                        } else null
                    }
                    if (decoded != null) {
                        sb.append(decoded)
                        i = semi + 1
                        continue
                    }
                }
            }
            sb.append(raw[i])
            i++
        }
        return sb.toString()
    }
}
