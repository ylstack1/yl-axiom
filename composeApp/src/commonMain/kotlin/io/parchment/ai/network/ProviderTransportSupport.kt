package io.parchment.ai.network

import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.header
import io.parchment.ai.model.AuthHeaderMode
import io.parchment.ai.model.CUSTOM_DIALECT_ANTHROPIC
import io.parchment.ai.model.CUSTOM_DIALECT_GEMINI
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

private val DISALLOWED_EXTRA_BODY_KEYS = setOf("tools", "tool_choice", "tool_config")

/** Resolves the concrete wire dialect for a profile (Custom delegates to its explicit [ProviderProfile.customDialect]). */
internal fun resolveProviderWireProtocol(profile: ProviderProfile): ProviderProtocol {
    if (!profile.protocol.isCustomWire) return profile.protocol
    return when (profile.customDialect.trim().lowercase()) {
        "gemini_interactions", "gemini" -> ProviderProtocol.GEMINI_INTERACTIONS
        "anthropic_messages", "anthropic" -> ProviderProtocol.ANTHROPIC_MESSAGES
        "openai_responses", "responses" -> ProviderProtocol.OPENAI_RESPONSES
        "openai_chat", "chat" -> ProviderProtocol.OPENAI_RESPONSES
        else -> ProviderProtocol.CUSTOM
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
    if (profile.authHeaderMode == AuthHeaderMode.QUERY_PARAM && profile.apiKey.isNotBlank()) {
        val sep = if (rawUrl.contains("?")) "&" else "?"
        return "$rawUrl${sep}key=${profile.apiKey.trim()}"
    }
    return rawUrl
}

internal fun HttpRequestBuilder.applyProviderAuthAndCustomHeaders(profile: ProviderProfile) {
    val key = profile.apiKey.trim()
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
                val k = trimmed.substringBefore(":").trim()
                val v = trimmed.substringAfter(":").trim()
                if (k.isNotEmpty()) {
                    header(k, v)
                }
            }
        }
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
                CUSTOM_DIALECT_ANTHROPIC -> {
                    header("x-api-key", key)
                    header("anthropic-version", "2023-06-01")
                }
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
    return deepMergeProviderJsonObjects(base, JsonObject(extraObj.filterKeys { it !in DISALLOWED_EXTRA_BODY_KEYS }))
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
