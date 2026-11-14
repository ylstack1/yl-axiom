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
    private val TRIPLE_NEWLINE_REGEX = Regex("\n{3,}")

    internal fun isHttpWebUrl(url: String): Boolean =
        url.startsWith("http://", ignoreCase = true) || url.startsWith("https://", ignoreCase = true)

    internal fun encodeWikiTitle(title: String): String {
        val bytes = title.trim().replace(' ', '_').encodeToByteArray()
        val out = StringBuilder(bytes.size)
        for (raw in bytes) {
            val u = raw.toInt() and 0xFF
            val ch = u.toChar()
            val safe = ch in 'A'..'Z' || ch in 'a'..'z' || ch in '0'..'9' ||
                ch == '-' || ch == '_' || ch == '.' || ch == '~' ||
                ch == ':' || ch == '/' || ch == '(' || ch == ')' || ch == '\''
            if (safe) {
                out.append(ch)
            } else {
                out.append('%')
                out.append(u.toString(16).uppercase().padStart(2, '0'))
            }
        }
        return out.toString()
    }

    internal fun urlDecodeComponent(encoded: String): String {
        val clean = encoded.replace('+', ' ')
        var out = ByteArray(clean.length.coerceAtLeast(16))
        var outIdx = 0
        fun appendBytes(bytes: ByteArray) {
            if (outIdx + bytes.size > out.size) {
                out = out.copyOf(maxOf(out.size * 2, outIdx + bytes.size))
            }
            bytes.copyInto(out, outIdx)
            outIdx += bytes.size
        }
        var i = 0
        while (i < clean.length) {
            val ch = clean[i]
            if (ch == '%' && i + 2 < clean.length) {
                val hex = clean.substring(i + 1, i + 3).toIntOrNull(16)
                if (hex != null) {
                    if (outIdx >= out.size) out = out.copyOf(out.size * 2)
                    out[outIdx++] = hex.toByte()
                    i += 3
                    continue
                }
            }
            if (ch.isHighSurrogate() && i + 1 < clean.length && clean[i + 1].isLowSurrogate()) {
                appendBytes(clean.substring(i, i + 2).encodeToByteArray())
                i += 2
            } else {
                appendBytes(ch.toString().encodeToByteArray())
                i++
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

        return trimmed
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
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&#x27;", "'")
            .lines()
            .map { it.trim() }
            .joinToString("\n")
            .replace(TRIPLE_NEWLINE_REGEX, "\n\n")
            .trim()
    }
}
