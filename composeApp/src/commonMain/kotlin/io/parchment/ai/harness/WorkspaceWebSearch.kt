package io.parchment.ai.harness

import io.parchment.ai.platform.HtmlTextConverter
import io.parchment.ai.platform.PlatformWorkspace
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Keyless multi-engine web search cascade (`web_search`): DuckDuckGo HTML, Wikipedia Search API,
 * and HackerNews Algolia Search API.
 */
internal object WorkspaceWebSearch {

    private val HTML_ANY_TAG_REGEX = Regex("(?is)<[^>]+>")
    private val DDG_MD_LINK_REGEX = Regex("""\[([^\]]+)\]\((https?://[^\s)]+)\)""")

    private data class SearchResultItem(
        val title: String,
        val url: String,
        val snippet: String
    )

    private fun urlEncodeQuery(raw: String): String {
        val sb = StringBuilder()
        for (b in raw.encodeToByteArray()) {
            val c = b.toInt() and 0xFF
            if ((c in 'a'.code..'z'.code) || (c in 'A'.code..'Z'.code) || (c in '0'.code..'9'.code) ||
                c == '-'.code || c == '_'.code || c == '.'.code
            ) {
                sb.append(c.toChar())
            } else if (c == ' '.code) {
                sb.append('+')
            } else {
                sb.append('%')
                sb.append(c.toString(16).uppercase().padStart(2, '0'))
            }
        }
        return sb.toString()
    }

    private fun parseDuckDuckGoMarkdownResults(markdownText: String, maxResults: Int): List<SearchResultItem> {
        val results = mutableListOf<SearchResultItem>()
        val seenUrls = HashSet<String>()
        val lines = markdownText.lines().map { it.trim() }.filter { it.isNotEmpty() }

        for ((idx, line) in lines.withIndex()) {
            for (match in DDG_MD_LINK_REGEX.findAll(line)) {
                val title = match.groupValues[1].trim()
                val rawUrl = match.groupValues[2].trim()
                val url = HtmlTextConverter.unwrapDuckDuckGoRedirectUrl(rawUrl)
                val lowerUrl = url.lowercase()
                if (!HtmlTextConverter.isHttpWebUrl(url) ||
                    lowerUrl.contains("duckduckgo.com") ||
                    lowerUrl.contains("bing.com/aclick") ||
                    title.length < 3 ||
                    !seenUrls.add(lowerUrl)
                ) {
                    continue
                }
                val nextSnippet = lines.drop(idx + 1).take(3)
                    .firstOrNull { cand ->
                        !cand.startsWith("[") && cand.length > 24 && !cand.startsWith("http")
                    }.orEmpty()
                results.add(
                    SearchResultItem(
                        title = title,
                        url = url,
                        snippet = nextSnippet.take(240)
                    )
                )
                if (results.size >= maxResults) return results
            }
        }
        return results
    }

    private fun parseWikipediaSearchJson(rawJson: String, maxResults: Int): List<SearchResultItem> {
        val root = runCatching { WorkspaceHarness.json.parseToJsonElement(rawJson).jsonObject }.getOrNull() ?: return emptyList()
        val searchArr = (root["query"] as? JsonObject)?.get("search") as? JsonArray ?: return emptyList()
        val results = mutableListOf<SearchResultItem>()
        for (el in searchArr) {
            val obj = el as? JsonObject ?: continue
            val title = obj["title"]?.jsonPrimitive?.contentOrNull?.trim().orEmpty()
            if (title.isEmpty()) continue
            val rawSnippet = obj["snippet"]?.jsonPrimitive?.contentOrNull.orEmpty()
                .replace(HTML_ANY_TAG_REGEX, "")
                .replace("&quot;", "\"")
                .replace("&amp;", "&")
                .trim()
            val wikiSlug = HtmlTextConverter.encodeWikiTitle(title)
            val url = "https://en.wikipedia.org/wiki/$wikiSlug"
            results.add(SearchResultItem(title = "$title — Wikipedia", url = url, snippet = rawSnippet))
            if (results.size >= maxResults) break
        }
        return results
    }

    private fun parseHackerNewsAlgoliaJson(rawJson: String, maxResults: Int): List<SearchResultItem> {
        val root = runCatching { WorkspaceHarness.json.parseToJsonElement(rawJson).jsonObject }.getOrNull() ?: return emptyList()
        val hitsArr = root["hits"] as? JsonArray ?: return emptyList()
        val results = mutableListOf<SearchResultItem>()
        for (el in hitsArr) {
            val obj = el as? JsonObject ?: continue
            val title = obj["title"]?.jsonPrimitive?.contentOrNull?.trim().orEmpty()
            if (title.isEmpty()) continue
            val objId = obj["objectID"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val url = obj["url"]?.jsonPrimitive?.contentOrNull?.takeIf { it.startsWith("http") }
                ?: if (objId.isNotEmpty()) "https://news.ycombinator.com/item?id=$objId" else continue
            val points = obj["points"]?.jsonPrimitive?.intOrNull ?: 0
            val comments = obj["num_comments"]?.jsonPrimitive?.intOrNull ?: 0
            results.add(
                SearchResultItem(
                    title = title,
                    url = url,
                    snippet = "Hacker News discussion ($points points, $comments comments)."
                )
            )
            if (results.size >= maxResults) break
        }
        return results
    }

    fun search(query: String, maxResults: Int = 6, isCancelled: (() -> Boolean)? = null): Result<String> = runCatching {
        val cleanQuery = query.trim()
        require(cleanQuery.isNotEmpty()) { "Search query must not be blank." }
        val encoded = urlEncodeQuery(cleanQuery)
        val collected = mutableListOf<SearchResultItem>()
        val seenUrls = HashSet<String>()

        fun addUnique(items: List<SearchResultItem>) {
            for (item in items) {
                val key = item.url.lowercase()
                if (seenUrls.add(key)) {
                    collected.add(item)
                    if (collected.size >= maxResults) return
                }
            }
        }

        // 1. Primary: DuckDuckGo HTML search
        val ddgUrl = "https://html.duckduckgo.com/html/?q=$encoded"
        PlatformWorkspace.fetchUrlText(ddgUrl, maxChars = 48_000, isCancelled = isCancelled).onSuccess { ddgMd ->
            addUnique(parseDuckDuckGoMarkdownResults(ddgMd, maxResults))
        }

        // 2. Fallback / Supplement: Wikipedia Search API
        if (collected.size < maxResults) {
            val wikiUrl = "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch=$encoded&utf8=1&format=json&srlimit=${maxResults.coerceAtMost(5)}"
            PlatformWorkspace.fetchUrlText(wikiUrl, maxChars = 32_000, isCancelled = isCancelled).onSuccess { wikiJson ->
                addUnique(parseWikipediaSearchJson(wikiJson, maxResults - collected.size))
            }
        }

        // 3. Fallback / Supplement: HackerNews Algolia Search API
        if (collected.size < maxResults) {
            val hnUrl = "https://hn.algolia.com/api/v1/search?query=$encoded&tags=story&hitsPerPage=${maxResults.coerceAtMost(5)}"
            PlatformWorkspace.fetchUrlText(hnUrl, maxChars = 32_000, isCancelled = isCancelled).onSuccess { hnJson ->
                addUnique(parseHackerNewsAlgoliaJson(hnJson, maxResults - collected.size))
            }
        }

        if (collected.isEmpty()) {
            "No web search results found for '$cleanQuery'."
        } else {
            buildString {
                appendLine("### Web Search Results for \"$cleanQuery\"")
                collected.forEachIndexed { idx, item ->
                    appendLine("${idx + 1}. **[${item.title}](${item.url})**")
                    appendLine("   URL: ${item.url}")
                    if (item.snippet.isNotBlank()) {
                        appendLine("   Snippet: ${item.snippet}")
                    }
                }
            }.trim()
        }
    }
}
