package io.parchment.ai.network

import io.ktor.client.statement.HttpResponse
import io.ktor.http.fromHttpToGmtDate
import io.parchment.ai.network.AiProviderEngine.StepRetriesExhaustedException
import io.parchment.ai.platform.PlatformRuntime
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlin.math.ceil
import kotlin.random.Random

// How provider errors are summarized, retried or used to fall back.

/**
 * A provider answered with an HTTP error. The message keeps the "<label> HTTP <status>: <body>" shape that saved
 * notices, the 400 self-healing checks and older callers read; [status] and [retryAfterMs] drive the retry policy.
 */
internal class ProviderHttpException(
    label: String,
    val status: Int,
    val body: String,
    /** How long the server asked for before the next attempt (Retry-After, retry-after-ms or Gemini RetryInfo). */
    val retryAfterMs: Long? = null
) : IllegalStateException(if (label.isBlank()) "HTTP $status: $body" else "$label HTTP $status: $body")

internal fun providerHttpError(label: String, response: HttpResponse, body: String): ProviderHttpException =
    ProviderHttpException(
        label, response.status.value, body,
        parseRetryAfterMs({ name -> response.headers[name] }, body, PlatformRuntime.currentTimeMillis())
    )

private const val MAX_HINT_SCAN_CHARS = 64 * 1024
private val RETRY_DELAY_SECONDS = Regex("""(\d+(?:\.\d+)?)s""")

/**
 * The wait a server asked for, in milliseconds: `retry-after-ms`, then `retry-after` as seconds or an HTTP date,
 * then a Gemini `google.rpc.RetryInfo.retryDelay` in the body. Null when it gave none, a date already passed or
 * nothing that parses: the caller then backs off on its own.
 */
internal fun parseRetryAfterMs(header: (String) -> String?, body: String, nowMs: Long): Long? {
    header("retry-after-ms")?.trim()?.toDoubleOrNull()?.let { ms ->
        if (ms > 0.0 && ms.isFinite()) return ceil(ms).toLong()
    }
    header("retry-after")?.trim()?.takeIf { it.isNotEmpty() }?.let { raw ->
        raw.toDoubleOrNull()?.let { seconds ->
            return if (seconds > 0.0 && seconds.isFinite()) ceil(seconds * 1000.0).toLong() else null
        }
        runCatching { raw.fromHttpToGmtDate().timestamp - nowMs }.getOrNull()?.let { ms -> return ms.takeIf { it > 0L } }
    }
    return geminiErrorDetails(body)
        .firstOrNull { it.typeName().endsWith("google.rpc.RetryInfo") }
        ?.let { (it["retryDelay"] as? JsonPrimitive)?.contentOrNull?.trim() }
        ?.let { RETRY_DELAY_SECONDS.matchEntire(it)?.groupValues?.get(1)?.toDoubleOrNull() }
        ?.takeIf { it > 0.0 }
        ?.let { ceil(it * 1000.0).toLong() }
}

/** Gemini's structured error details (`error.details[]`), from an object or a one-element array body. */
private fun geminiErrorDetails(body: String): List<JsonObject> {
    if (body.length > MAX_HINT_SCAN_CHARS || "\"details\"" !in body) return emptyList()
    val root = runCatching { AiProviderEngine.json.parseToJsonElement(body) }.getOrNull() ?: return emptyList()
    val top = root as? JsonObject ?: (root as? JsonArray)?.firstOrNull() as? JsonObject ?: return emptyList()
    val details = ((top["error"] as? JsonObject) ?: top)["details"] as? JsonArray ?: return emptyList()
    return details.mapNotNull { it as? JsonObject }
}

private fun JsonObject.typeName(): String = (this["@type"] as? JsonPrimitive)?.contentOrNull.orEmpty()

/** A Gemini quota that resets once a day (`quotaId` contains PerDay): trying again within minutes cannot work. */
private fun isGeminiDailyQuota(body: String): Boolean = geminiErrorDetails(body)
    .filter { it.typeName().endsWith("google.rpc.QuotaFailure") }
    .flatMap { (it["violations"] as? JsonArray).orEmpty() }
    .any { violation ->
        ((violation as? JsonObject)?.get("quotaId") as? JsonPrimitive)?.contentOrNull.orEmpty().contains("PerDay", ignoreCase = true)
    }

private val HTTP_STATUS_IN_MESSAGE = Regex("""\bhttp ([45]\d\d)\b""")

/** The typed HTTP failure behind [err], looking through the step-retry wrapper. */
private fun Throwable.providerHttp(): ProviderHttpException? =
    ((this as? StepRetriesExhaustedException)?.cause ?: this) as? ProviderHttpException

/** The failure's HTTP status: typed when the engine threw it, read from an older "HTTP 502: ..." message otherwise. */
private fun httpStatusOf(err: Throwable): Int? = err.providerHttp()?.status
    ?: HTTP_STATUS_IN_MESSAGE.find((err.message ?: "").lowercase())?.groupValues?.get(1)?.toIntOrNull()

/** Client errors that sending the same request again cannot fix. 400 is judged by its wording instead. */
private val NON_RETRYABLE_STATUSES = setOf(401, 402, 403, 404, 405, 406, 410, 411, 413, 414, 415, 416, 417, 422, 451, 501)

// Account states that stop every request until a person acts: retrying or switching endpoints cannot help.
private val AUTH_STOP_MARKERS = listOf("invalid_api_key", "invalid api key", "authentication_error", "permission_denied")
private val BILLING_STOP_MARKERS = listOf("billing_not_active", "insufficient_quota", "billing_hard_limit_reached", "enforced_spend_limit_reached")

// Waits between attempts when the server gave no hint: quick for a dropped connection, gentler for a busy server.
private val CONNECTION_BACKOFF_MS = longArrayOf(2_000L, 4_000L, 8_000L, 16_000L, 30_000L, 60_000L)
private val SERVER_BACKOFF_MS = longArrayOf(5_000L, 10_000L, 20_000L, 40_000L, 60_000L)
private const val MIN_SERVER_HINT_MS = 1_000L
private const val MAX_SERVER_HINT_MS = 300_000L

/**
 * How long to wait before attempt [attempt] (1-based) after [error]. A server's own hint wins: clamped to 1 to
 * 300 s and never shortened, plus up to 10% jitter. Otherwise backoff with ±20% jitter, at most
 * [AiProviderEngine.RETRY_WAIT_SECONDS]. [fixedMs] replaces the backoff (the legacy-endpoint probe's short wait)
 * but still yields to a longer server hint. [random] is a parameter so tests can pin the jitter.
 */
internal fun retryDelayMs(attempt: Int, error: Throwable, random: Random = Random.Default, fixedMs: Long? = null): Long {
    val hintMs = error.providerHttp()?.retryAfterMs?.let { hint ->
        val base = hint.coerceIn(MIN_SERVER_HINT_MS, MAX_SERVER_HINT_MS)
        base + (base * random.nextDouble(0.0, 0.1)).toLong()
    }
    if (fixedMs != null) return maxOf(fixedMs, hintMs ?: 0L)
    if (hintMs != null) return hintMs
    val schedule = if (httpStatusOf(error) == null) CONNECTION_BACKOFF_MS else SERVER_BACKOFF_MS
    val base = schedule[(attempt - 1).coerceIn(0, schedule.lastIndex)]
    return (base * random.nextDouble(0.8, 1.2)).toLong().coerceAtMost(AiProviderEngine.RETRY_WAIT_SECONDS * 1000L)
}

/**
 * Unwraps raw JSON API error payloads (such as `{"error":{"code":"invalid_request","message":"..."}}`)
 * into clean, human-readable summaries without raw JSON braces or mid-word truncation.
 */
internal fun AiProviderEngine.extractCleanApiErrorSummary(raw: String): String {
    val trimmed = raw.trim()
    if (trimmed.isEmpty()) return ""
    val braceIdx = trimmed.indexOf('{')
    val bracketIdx = trimmed.indexOf('[')
    val jsonStart = when {
        braceIdx >= 0 && bracketIdx >= 0 -> minOf(braceIdx, bracketIdx)
        braceIdx >= 0 -> braceIdx
        bracketIdx >= 0 -> bracketIdx
        else -> -1
    }
    if (jsonStart >= 0) {
        val prefix = trimmed.substring(0, jsonStart).trim().removeSuffix(":").trim()
        val jsonCandidate = trimmed.substring(jsonStart).trim()
        val parsedSummary = runCatching {
            val rootEl = json.parseToJsonElement(jsonCandidate)
            val rootObj = when (rootEl) {
                is JsonObject -> rootEl
                is JsonArray -> rootEl.firstOrNull() as? JsonObject
                else -> null
            }
            if (rootObj != null) {
                val errObj = (rootObj["error"] as? JsonObject) ?: rootObj
                val code = errObj["code"]?.jsonPrimitive?.contentOrNull
                    ?: errObj["status"]?.jsonPrimitive?.contentOrNull
                    ?: errObj["type"]?.jsonPrimitive?.contentOrNull
                val msg = errObj["message"]?.jsonPrimitive?.contentOrNull
                    ?: errObj["detail"]?.jsonPrimitive?.contentOrNull
                    ?: (rootObj["error"] as? JsonPrimitive)?.contentOrNull
                if (!msg.isNullOrBlank()) {
                    val cleanMsg = msg.replace(Regex("\\s+"), " ").trim()
                    if (prefix.isNotEmpty()) {
                        val codeTag = if (!code.isNullOrBlank() && !prefix.contains(code, ignoreCase = true)) {
                            " ($code)"
                        } else ""
                        "$prefix$codeTag: $cleanMsg"
                    } else {
                        val codeTag = if (!code.isNullOrBlank()) "[$code] " else ""
                        "$codeTag$cleanMsg"
                    }
                } else null
            } else null
        }.getOrNull()
        if (!parsedSummary.isNullOrBlank()) {
            return parsedSummary
        }
    }
    return trimmed.replace(Regex("\\s+"), " ").trim()
}

/**
 * Authentication, permission, spend-cap, billing and daily-quota stops: they need a person, so nothing retries or
 * switches endpoints on them.
 */
internal fun AiProviderEngine.isHardAuthOrQuotaError(err: Throwable): Boolean {
    val status = httpStatusOf(err)
    if (status == 401 || status == 403) return true
    val msg = (err.message ?: err.toString()).lowercase()
    if (AUTH_STOP_MARKERS.any { it in msg } || BILLING_STOP_MARKERS.any { it in msg }) return true
    return status == 429 && isGeminiDailyQuota(err.providerHttp()?.body.orEmpty())
}

/**
 * The failure a person can fix from the app, if [err] is one: what its notice offers instead of a retry that cannot
 * help. A key the provider refuses is checked before quota, since it is the person's to change in Settings.
 */
internal fun AiProviderEngine.runErrorKindOf(err: Throwable): RunErrorKind? {
    val actual = (err as? StepRetriesExhaustedException)?.cause ?: err
    if (actual is AiProviderEngine.SpendLimitException) return RunErrorKind.SPEND_LIMIT
    val status = httpStatusOf(actual)
    val msg = (actual.message ?: "").lowercase()
    return when {
        isContextLengthExceededError(actual) -> RunErrorKind.CONVERSATION_LIMIT
        status == 401 || status == 403 || AUTH_STOP_MARKERS.any { it in msg } -> RunErrorKind.API_KEY
        status == 402 || isHardAuthOrQuotaError(actual) -> RunErrorKind.QUOTA
        status == 404 && "model" in msg -> RunErrorKind.MODEL
        else -> null
    }
}

/**
 * Failures worth sending again: dropped connections, cut streams and timeouts, 408/409/425/429, 5xx and 529
 * (Cloudflare 520-527 included), and a 400 that says it is a rate limit or overload. The HTTP status decides
 * first; the wording only where the status is ambiguous.
 */
internal fun AiProviderEngine.isTransientOrProxyTimeoutError(err: Throwable): Boolean {
    if (err is kotlinx.coroutines.CancellationException || err is AiProviderEngine.NoRetryException) return false
    if (isHardAuthOrQuotaError(err)) return false
    // Unsupported endpoints (404/405/501) fall back to the other protocol at once instead of waiting.
    if (isEndpointUnsupportedForAutoFallback(err)) return false
    val msg = (err.message ?: err.toString()).lowercase()
    if (msg.contains("request_too_large") || msg.contains("payload too large") || msg.contains("entity too large")) return false
    return when (val status = httpStatusOf(err)) {
        // A 400 is a client mistake unless its text says rate limit, overload or timeout.
        400 -> !(msg.contains("invalid_request") || msg.contains("invalid_argument") || msg.contains("bad_request")) && (
            msg.contains("rate limit") || msg.contains("rate_limit") || msg.contains("ratelimit") ||
                msg.contains("too many requests") || msg.contains("resource_exhausted") || msg.contains("overload") ||
                msg.contains("timeout") || msg.contains("busy") || msg.contains("capacity") || msg.contains("try again")
            )
        null -> true // Network, socket, EOF and cut-stream failures.
        else -> status !in NON_RETRYABLE_STATUSES
    }
}

/**
 * Detects when a proxy or server does not implement `/v1beta/interactions` or `/v1/responses`
 * so `streamGeneration` can automatically fall back to `:streamGenerateContent` or `/v1/chat/completions`.
 * For a typed HTTP error the status decides; a body that merely mentions "not found" (a missing model on a 400,
 * an upstream on a 502) is not an endpoint the server lacks.
 */
internal fun AiProviderEngine.isEndpointUnsupportedForAutoFallback(err: Throwable): Boolean {
    val status = httpStatusOf(err)
    if (status == 404 || status == 405 || status == 501) return true
    val msg = (err.message ?: err.toString()).lowercase()
    val endpointWording = msg.contains("unknown endpoint") ||
        msg.contains("unrecognized request url") ||
        msg.contains("unsupported path")
    val typed = err.providerHttp() ?: return endpointWording || msg.contains("not found")
    return endpointWording && typed.status in 400..499
}

/**
 * Determines whether a failure on the primary protocol (`/v1beta/interactions` or `/v1/responses`)
 * should fall back to the legacy endpoint (`:streamGenerateContent` or `/v1/chat/completions`).
 * - Never falls back on `CancellationException` or hard auth/quota errors (HTTP 401/403).
 * - Always falls back on unsupported endpoint errors (404/405/501).
 * - When `useLegacyFallbackEndpoint` is enabled, falls back on any remaining error after the
 *   primary protocol's self-healing and fast probe retry budget (`LEGACY_FALLBACK_PRIMARY_MAX_RETRIES`) are exhausted.
 */
internal fun AiProviderEngine.shouldFallbackToLegacyProtocol(
    err: Throwable,
    useLegacyFallbackEndpoint: Boolean
): Boolean {
    if (err is kotlinx.coroutines.CancellationException || err is AiProviderEngine.NoRetryException) return false
    val actual = if (err is StepRetriesExhaustedException && err.cause != null) err.cause!! else err
    if (isHardAuthOrQuotaError(actual)) return false
    if (isEndpointUnsupportedForAutoFallback(actual)) return true
    return useLegacyFallbackEndpoint
}

/**
 * Detects HTTP 400 / 413 context-window / token-limit overflow errors across Gemini, OpenAI, Anthropic, and proxies.
 */
internal fun AiProviderEngine.isContextLengthExceededError(err: Throwable): Boolean {
    val msg = (err.message ?: err.toString()).lowercase()
    return msg.contains("context_length_exceeded") ||
        msg.contains("maximum context length") ||
        msg.contains("prompt is too long") ||
        (msg.contains("input token count") && msg.contains("exceed")) ||
        msg.contains("too many tokens") ||
        msg.contains("exceeds the maximum number of tokens") ||
        msg.contains("request too large") ||
        msg.contains("http 413")
}
