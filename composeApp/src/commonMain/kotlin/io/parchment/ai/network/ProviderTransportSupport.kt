package io.parchment.ai.network

import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.header
import io.ktor.http.encodeURLParameter
import io.parchment.ai.model.AuthHeaderMode
import io.parchment.ai.model.CUSTOM_DIALECT_ANTHROPIC
import io.parchment.ai.model.CUSTOM_DIALECT_GEMINI
import io.parchment.ai.model.CUSTOM_DIALECT_OPENAI_CHAT
import io.parchment.ai.model.CUSTOM_DIALECT_OPENAI_RESPONSES
import io.parchment.ai.model.MessageVariant
import io.parchment.ai.model.ProviderProfile
import io.parchment.ai.model.ProviderProtocol
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject

// Endpoint resolution, auth header wiring, and extra-body JSON merging for provider HTTP requests.

private val CONCRETE_ENDPOINT_TAILS = listOf(
    "/responses",
    "/chat/completions",
    "/completions",
    "/messages",
    "/interactions",
    ":generatecontent",
    ":streamgeneratecontent",
    ":predict",
    ":predictlongrunning",
    "/images/generations",
    "/images/edits",
    "/videos/generations",
    "/video/generations",
    "/contents/generations/tasks",
    "/audio/speech",
    "/audio/transcriptions",
    "/generate",
    "/invoke",
    "/chat"
)

private val VERSION_SUFFIX_REGEX = Regex(
    "/(?:api/)?(?:paas/)?v\\d+(?:beta\\d*|alpha\\d*)?$",
    RegexOption.IGNORE_CASE
)

private val DISALLOWED_EXTRA_BODY_KEYS = setOf(
    "tools",
    "tool_choice",
    "toolchoice",
    "tool_config",
    "toolconfig",
    "built_in_tools",
    "builtintools",
    "mcp_servers",
    "mcpservers",
    "functions",
    "function_call",
    "functioncall"
)

/**
 * Removes characters a paste can smuggle into a credential or header: zero-width spaces and joiners, BOMs
 * and other format characters, CR/LF and other controls, then outer whitespace (including no-break spaces).
 * They are invisible in the key field yet make the provider reject the request.
 */
internal fun stripInvisibleCredentialChars(raw: String): String =
    raw.filterNot { it.category == CharCategory.FORMAT || it.category == CharCategory.CONTROL }.trim()

private fun sanitizeHeaderToken(raw: String): String = stripInvisibleCredentialChars(raw)

/** Resolves the concrete wire dialect for a profile (Custom delegates to its explicit [ProviderProfile.customDialect]). */
internal fun resolveProviderWireProtocol(profile: ProviderProfile): ProviderProtocol =
    when (resolveProviderWireDialect(profile)) {
        CUSTOM_DIALECT_GEMINI -> ProviderProtocol.GEMINI_INTERACTIONS
        CUSTOM_DIALECT_ANTHROPIC -> ProviderProtocol.ANTHROPIC_MESSAGES
        CUSTOM_DIALECT_OPENAI_RESPONSES, CUSTOM_DIALECT_OPENAI_CHAT -> ProviderProtocol.OPENAI_RESPONSES
        else -> ProviderProtocol.CUSTOM
    }

/** Resolves the concrete wire dialect (`openai_chat`, `openai_responses`, `anthropic_messages`, `gemini_interactions`). */
internal fun resolveProviderWireDialect(profile: ProviderProfile): String {
    if (profile.protocol.isCustomWire) {
        return when (profile.customDialect.trim().lowercase()) {
            "gemini_interactions", "gemini" -> CUSTOM_DIALECT_GEMINI
            "anthropic_messages", "anthropic" -> CUSTOM_DIALECT_ANTHROPIC
            "openai_responses", "responses" -> CUSTOM_DIALECT_OPENAI_RESPONSES
            "openai_chat", "chat" -> CUSTOM_DIALECT_OPENAI_CHAT
            else -> ""
        }
    }
    return when (profile.protocol) {
        ProviderProtocol.GEMINI_INTERACTIONS -> CUSTOM_DIALECT_GEMINI
        ProviderProtocol.OPENAI_RESPONSES -> CUSTOM_DIALECT_OPENAI_RESPONSES
        ProviderProtocol.ANTHROPIC_MESSAGES -> CUSTOM_DIALECT_ANTHROPIC
        ProviderProtocol.CUSTOM -> ""
    }
}

/**
 * Returns the concrete wire dialect that produced this variant, or `""` when ambiguous
 * (older saves stored both OpenAI Chat and OpenAI Responses under `OPENAI_RESPONSES`).
 */
internal fun MessageVariant.effectiveWireDialect(): String {
    if (providerWireDialect.isNotBlank()) return providerWireDialect.trim().lowercase()
    return when (providerProtocol) {
        ProviderProtocol.GEMINI_INTERACTIONS -> CUSTOM_DIALECT_GEMINI
        ProviderProtocol.ANTHROPIC_MESSAGES -> CUSTOM_DIALECT_ANTHROPIC
        ProviderProtocol.OPENAI_RESPONSES, ProviderProtocol.CUSTOM, null -> ""
    }
}

/** True when the custom profile has no dialect chosen yet — Send must be blocked. */
internal fun isProviderCustomDialectUnresolved(profile: ProviderProfile): Boolean =
    profile.protocol.isCustomWire && resolveProviderWireProtocol(profile) == ProviderProtocol.CUSTOM

internal fun resolveProviderSmartEndpointUrl(
    profile: ProviderProfile,
    defaultPathWithV1: String,
    defaultPathWithoutVersion: String = defaultPathWithV1
        .removePrefix("/v1beta")
        .removePrefix("/v1")
): String {
    val rawBase = profile.baseUrl.trim()
    val rawOverride = profile.endpointPathOverride.trim()

    // 1. Custom providers: the URL is sent exactly as entered, never rebuilt.
    if (profile.protocol.isCustomWire) {
        if (rawOverride.startsWith("http://", ignoreCase = true) || rawOverride.startsWith("https://", ignoreCase = true)) {
            return rawOverride.trimEnd('#')
        }
        if (rawOverride.isNotEmpty() && rawOverride != "/" && rawOverride != "#") {
            val cleanOverride = "/" + rawOverride.trimEnd('#').trimStart('/')
            return if (rawBase.trimEnd('/').endsWith(cleanOverride, ignoreCase = true)) {
                rawBase.trimEnd('#')
            } else {
                rawBase.trimEnd('/') + cleanOverride
            }
        }
        return rawBase.trimEnd('#').trimEnd('/')
    }

    // 1b. Trailing `#` opt-out on managed protocols.
    if (rawBase.endsWith("#") || rawOverride == "/" || rawOverride == "#") {
        return rawBase.trimEnd('#').trimEnd('/')
    }

    val base = rawBase.trimEnd('/')

    // 2. Explicit endpointPathOverride
    if (rawOverride.isNotEmpty()) {
        if (rawOverride.startsWith("http://", ignoreCase = true) ||
            rawOverride.startsWith("https://", ignoreCase = true)
        ) {
            return rawOverride.trimEnd('#')
        }
        val cleanOverride = "/" + rawOverride.trimEnd('#').trimStart('/')
        return if (base.endsWith(cleanOverride, ignoreCase = true)) base else "$base$cleanOverride"
    }

    // 3. A base URL that already ends in an endpoint path is used as is, with no path appended.
    val lowerBase = base.lowercase()
    if (CONCRETE_ENDPOINT_TAILS.any { lowerBase.endsWith(it) || lowerBase.contains("$it?") }) {
        return base
    }

    // 4. Version-agnostic root path resolution (/v1, /v2, /v3, /v4, /v1beta, /api/v3, /api/paas/v4)
    val hasVersionSuffix = VERSION_SUFFIX_REGEX.containsMatchIn(base)
    val normalizedNoVer = "/" + defaultPathWithoutVersion.trimStart('/')
    val normalizedWithVer = "/" + defaultPathWithV1.trimStart('/')
    return if (hasVersionSuffix) "$base$normalizedNoVer" else "$base$normalizedWithVer"
}

internal fun buildProviderUrlWithAuthQuery(profile: ProviderProfile, rawUrl: String): String {
    val key = sanitizeHeaderToken(profile.apiKey)
    if (profile.authHeaderMode == AuthHeaderMode.QUERY_PARAM && key.isNotEmpty()) {
        val sep = if (rawUrl.contains("?")) "&" else "?"
        return "$rawUrl${sep}key=${key.encodeURLParameter()}"
    }
    return rawUrl
}

internal fun HttpRequestBuilder.applyProviderAuthAndCustomHeaders(profile: ProviderProfile) {
    val key = sanitizeHeaderToken(profile.apiKey)
    if (key.isNotEmpty()) {
        when (profile.authHeaderMode) {
            AuthHeaderMode.BEARER_ONLY -> header("Authorization", "Bearer $key")
            AuthHeaderMode.QUERY_PARAM -> Unit // Already appended to URL query string
            AuthHeaderMode.NATIVE -> applyNativeAuthHeaders(profile, key)
        }
    }

    if (profile.customHeadersRaw.isNotBlank()) {
        profile.customHeadersRaw.lines().forEach { line ->
            val trimmed = line.trim()
            if (trimmed.isNotEmpty() && !trimmed.startsWith("#") && trimmed.contains(":")) {
                val k = sanitizeHeaderToken(trimmed.substringBefore(":"))
                val v = sanitizeHeaderToken(trimmed.substringAfter(":"))
                if (k.isNotEmpty() && k.none { it.isWhitespace() }) {
                    header(k, v)
                }
            }
        }
    }
}

internal fun ProviderProfile.hasCustomHeader(headerName: String): Boolean {
    if (customHeadersRaw.isBlank()) return false
    return customHeadersRaw.lines().any { line ->
        val trimmed = line.trim()
        trimmed.isNotEmpty() && !trimmed.startsWith("#") && trimmed.contains(":") &&
            trimmed.substringBefore(":").trim().equals(headerName, ignoreCase = true)
    }
}

internal fun HttpRequestBuilder.applyAnthropicVersionHeader(profile: ProviderProfile) {
    if (!profile.hasCustomHeader("anthropic-version")) {
        header("anthropic-version", "2023-06-01")
    }
}

private fun HttpRequestBuilder.applyNativeAuthHeaders(profile: ProviderProfile, key: String) {
    when (profile.protocol) {
        ProviderProtocol.GEMINI_INTERACTIONS -> {
            header("x-goog-api-key", key)
            // Custom proxies (e.g. api.zai.is, NewAPI, LiteLLM) using sk-* keys often also check Bearer
            if (!profile.baseUrl.contains("googleapis.com", ignoreCase = true) || key.startsWith("sk-")) {
                header("Authorization", "Bearer $key")
            }
        }
        ProviderProtocol.OPENAI_RESPONSES -> header("Authorization", "Bearer $key")
        ProviderProtocol.ANTHROPIC_MESSAGES -> {
            header("x-api-key", key)
            if (!profile.baseUrl.contains("anthropic.com", ignoreCase = true)) {
                header("Authorization", "Bearer $key")
            }
        }
        ProviderProtocol.CUSTOM -> {
            when (profile.customDialect) {
                CUSTOM_DIALECT_GEMINI -> {
                    header("x-goog-api-key", key)
                    if (key.startsWith("sk-")) header("Authorization", "Bearer $key")
                }
                CUSTOM_DIALECT_ANTHROPIC -> header("x-api-key", key)
                else -> header("Authorization", "Bearer $key")
            }
        }
    }
}

internal fun mergeProviderRawJsonExtraBody(base: JsonObject, rawExtraJson: String, json: Json): JsonObject {
    val trimmed = rawExtraJson.trim()
    if (trimmed.isEmpty()) return base
    val extraObj = runCatching { json.parseToJsonElement(trimmed).jsonObject }.getOrNull() ?: return base
    // Raw provider options cannot grant tools or replace the harness's execution authority.
    return deepMergeProviderJsonObjects(
        base,
        JsonObject(extraObj.filterKeys { it.lowercase() !in DISALLOWED_EXTRA_BODY_KEYS })
    )
}

internal fun deepMergeProviderJsonObjects(target: JsonObject, overrideObj: JsonObject): JsonObject {
    val merged = target.toMutableMap()
    for ((k, v) in overrideObj) {
        val existing = merged[k]
        merged[k] = if (existing is JsonObject && v is JsonObject) {
            deepMergeProviderJsonObjects(existing, v)
        } else {
            v
        }
    }
    return JsonObject(merged)
}
