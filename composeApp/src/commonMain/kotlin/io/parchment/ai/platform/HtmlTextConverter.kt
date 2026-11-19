package io.parchment.ai.platform

internal object HtmlTextConverter {
    private val HTML_STRIP_BLOCK_TAGS_REGEX = Regex("(?is)<(script|style|noscript|svg|head)\\b[^>]*>.*?</\\1>")
    private val HTML_COMMENT_REGEX = Regex("(?is)<!--.*?-->")
    private val HTML_H1_REGEX = Regex("(?is)<h1\\b[^>]*>(.*?)</h1>")
    private val HTML_H2_REGEX = Regex("(?is)<h2\\b[^>]*>(.*?)</h2>")
    private val HTML_H3_REGEX = Regex("(?is)<h3\\b[^>]*>(.*?)</h3>")
    private val HTML_H4_6_REGEX = Regex("(?is)<h[4-6]\\b[^>]*>(.*?)</h[4-6]>")
    private val HTML_LI_REGEX = Regex("(?is)<li\\b[^>]*>(.*?)</li>")
    private val HTML_A_HREF_REGEX = Regex("(?is)<a\\b[^>]*href=[\"']([^\"']+)[\"'][^>]*>(.*?)</a>")
    private val HTML_BR_HR_REGEX = Regex("(?is)<(br|hr)\\s*/?>")
    private val HTML_BLOCK_CLOSE_REGEX = Regex("(?is)</(p|div|section|article|tr|blockquote|pre|ul|ol|table)>")
    private val HTML_ANY_TAG_REGEX = Regex("(?is)<[^>]+>")
    private val HEX_DIGITS = "0123456789ABCDEF"

    private fun hexNibble(ch: Char): Int = when (ch) {
        in '0'..'9' -> ch - '0'
        in 'a'..'f' -> ch - 'a' + 10
        in 'A'..'F' -> ch - 'A' + 10
        else -> -1
    }

    internal fun isHttpWebUrl(url: String): Boolean = url.startsWith("http://", ignoreCase = true) || url.startsWith("https://", ignoreCase = true)

    internal fun encodeWikiTitle(title: String): String {
        val bytes = title.trim().replace(' ', '_').encodeToByteArray()
        val out = StringBuilder(bytes.size)
        for (raw in bytes) {
            val u = raw.toInt() and 0xFF
            val ch = u.toChar()
            val safe = ch in 'A'..'Z' ||
                ch in 'a'..'z' ||
                ch in '0'..'9' ||
                ch == '-' ||
                ch == '_' ||
                ch == '.' ||
                ch == '~' ||
                ch == ':' ||
                ch == '/' ||
                ch == '(' ||
                ch == ')' ||
                ch == '\''
            if (safe) {
                out.append(ch)
            } else {
                out.append('%')
                out.append(HEX_DIGITS[u ushr 4])
                out.append(HEX_DIGITS[u and 0x0F])
            }
        }
        return out.toString()
    }

    internal fun urlDecodeComponent(encoded: String): String {
        val len = encoded.length
        var out = ByteArray(len.coerceAtLeast(16))
        var outIdx = 0
        fun ensure(extra: Int) {
            if (outIdx + extra > out.size) {
                out = out.copyOf(maxOf(out.size * 2, outIdx + extra))
            }
        }
        var i = 0
        while (i < len) {
            val ch = encoded[i]
            if (ch == '+') {
                ensure(1)
                out[outIdx++] = ' '.code.toByte()
                i++
            } else if (ch == '%' && i + 2 < len) {
                val hi = hexNibble(encoded[i + 1])
                val lo = hexNibble(encoded[i + 2])
                if (hi >= 0 && lo >= 0) {
                    ensure(1)
                    out[outIdx++] = ((hi shl 4) or lo).toByte()
                    i += 3
                } else {
                    ensure(1)
                    out[outIdx++] = '%'.code.toByte()
                    i++
                }
            } else if (ch.code < 0x80) {
                ensure(1)
                out[outIdx++] = ch.code.toByte()
                i++
            } else {
                val span = if (ch.isHighSurrogate() && i + 1 < len && encoded[i + 1].isLowSurrogate()) 2 else 1
                val utf8 = encoded.substring(i, i + span).encodeToByteArray()
                ensure(utf8.size)
                utf8.copyInto(out, outIdx)
                outIdx += utf8.size
                i += span
            }
        }
        return out.decodeToString(0, outIdx)
    }

    internal fun unwrapDuckDuckGoRedirectUrl(rawUrl: String): String {
        val trimmed = rawUrl.trim().let {
            if (it.startsWith("//")) "https:$it" else it
        }
        if (trimmed.contains("uddg=")) {
            val uddgParam = trimmed.substringAfter("uddg=").substringBefore("&")
            val decoded = urlDecodeComponent(uddgParam).trim()
            if (isHttpWebUrl(decoded)) {
                return decoded
            }
        }
        return trimmed
    }

    /**
     * Converts raw HTML into clean readable Markdown/text while leaving JSON, Markdown, and plain text intact.
     */
    fun convertHtmlToReadableText(raw: String, contentType: String = ""): String {
        val trimmed = raw.trim()
        val looksLikeHtml = contentType.contains("html", ignoreCase = true) ||
            trimmed.startsWith("<!DOCTYPE html", ignoreCase = true) ||
            trimmed.startsWith("<html", ignoreCase = true) ||
            (trimmed.contains("<body", ignoreCase = true) && trimmed.contains("</body>", ignoreCase = true))
        if (!looksLikeHtml) return trimmed

        val stripped = trimmed
            .replace(HTML_STRIP_BLOCK_TAGS_REGEX, "")
            .replace(HTML_COMMENT_REGEX, "")
            .replace(HTML_H1_REGEX, "\n# $1\n")
            .replace(HTML_H2_REGEX, "\n## $1\n")
            .replace(HTML_H3_REGEX, "\n### $1\n")
            .replace(HTML_H4_6_REGEX, "\n#### $1\n")
            .replace(HTML_LI_REGEX, "\n- $1")
            .replace(HTML_A_HREF_REGEX) { m ->
                val rawHref = m.groupValues[1].trim()
                val cleanHref = unwrapDuckDuckGoRedirectUrl(rawHref)
                val label = m.groupValues[2].replace(HTML_ANY_TAG_REGEX, "").trim()
                if (label.isNotEmpty() && isHttpWebUrl(cleanHref)) "[$label]($cleanHref)" else label
            }
            .replace(HTML_BR_HR_REGEX, "\n")
            .replace(HTML_BLOCK_CLOSE_REGEX, "\n")
            .replace(HTML_ANY_TAG_REGEX, "")
        return decodeEntitiesAndNormalizeLines(stripped)
    }

    private fun decodeEntitiesAndNormalizeLines(src: String): String {
        val len = src.length
        val decoded = StringBuilder(len)
        var i = 0
        while (i < len) {
            if (src[i] == '&') {
                when {
                    src.startsWith("&nbsp;", i) -> { decoded.append(' '); i += 6; continue }
                    src.startsWith("&amp;", i) -> { decoded.append('&'); i += 5; continue }
                    src.startsWith("&lt;", i) -> { decoded.append('<'); i += 4; continue }
                    src.startsWith("&gt;", i) -> { decoded.append('>'); i += 4; continue }
                    src.startsWith("&quot;", i) -> { decoded.append('"'); i += 6; continue }
                    src.startsWith("&#39;", i) -> { decoded.append('\''); i += 5; continue }
                    src.startsWith("&#x27;", i) -> { decoded.append('\''); i += 6; continue }
                }
            }
            decoded.append(src[i++])
        }
        val out = StringBuilder(decoded.length)
        var lineStart = 0
        var blankRun = 0
        val dLen = decoded.length
        while (lineStart <= dLen) {
            var lineEnd = lineStart
            while (lineEnd < dLen && decoded[lineEnd] != '\n' && decoded[lineEnd] != '\r') lineEnd++
            var a = lineStart
            var b = lineEnd
            while (a < b && decoded[a].isWhitespace()) a++
            while (b > a && decoded[b - 1].isWhitespace()) b--
            if (a == b) {
                if (out.isNotEmpty()) blankRun++
            } else {
                if (out.isNotEmpty()) {
                    out.append('\n')
                    if (blankRun > 0) out.append('\n')
                }
                blankRun = 0
                out.append(decoded, a, b)
            }
            lineStart = if (lineEnd < dLen && decoded[lineEnd] == '\r' && lineEnd + 1 < dLen && decoded[lineEnd + 1] == '\n') {
                lineEnd + 2
            } else {
                lineEnd + 1
            }
        }
        return out.toString()
    }
}
