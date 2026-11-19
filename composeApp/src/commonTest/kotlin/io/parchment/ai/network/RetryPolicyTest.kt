package io.parchment.ai.network

import io.ktor.http.toHttpDate
import io.ktor.util.date.GMTDate
import kotlin.random.Random
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RetryPolicyTest {
    private val now = 1_700_000_000_000L

    /** Jitter pinned to the low or high end of its range. */
    private val lowest = object : Random() { override fun nextBits(bitCount: Int) = 0 }
    private val highest = object : Random() { override fun nextBits(bitCount: Int) = -1 ushr (32 - bitCount) }

    private fun hint(headers: Map<String, String> = emptyMap(), body: String = "") = parseRetryAfterMs({ name -> headers[name] }, body, now)

    private fun geminiError(vararg details: String) = """{"error":{"code":429,"message":"You exceeded your current quota.","status":"RESOURCE_EXHAUSTED","details":[${details.joinToString(",")}]}}"""

    private fun quota(id: String) = """{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[{"quotaMetric":"m","quotaId":"$id"}]}"""

    private val retryInfo = """{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"15.002899939s"}"""

    @Test fun serverHintsAreReadFromHeadersAndGeminiBodies() {
        assertEquals(2_000L, hint(mapOf("retry-after" to "2")))
        assertEquals(1_500L, hint(mapOf("retry-after" to " 1.5 ")))
        assertEquals(251L, hint(mapOf("retry-after-ms" to "250.4", "retry-after" to "9")), "Milliseconds win over seconds")
        assertEquals(30_000L, hint(mapOf("retry-after" to GMTDate(now + 30_000L).toHttpDate())))
        assertEquals(15_003L, hint(body = geminiError(quota("GenerateRequestsPerMinutePerProjectPerModel"), retryInfo)))
        assertEquals(15_003L, hint(body = "[${geminiError(retryInfo)}]"), "Gemini can wrap the error in an array")
    }

    @Test fun unusableHintsMeanBackingOffOnOurOwn() {
        assertNull(hint(mapOf("retry-after" to GMTDate(now - 30_000L).toHttpDate())), "A date already passed")
        assertNull(hint(mapOf("retry-after" to "soon")))
        assertNull(hint(mapOf("retry-after" to "0")))
        assertNull(hint(mapOf("retry-after-ms" to "NaN")))
        assertNull(hint(body = """{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"later"}]}}"""))
        assertNull(hint(body = "<html>Bad gateway</html>"))
    }

    @Test fun serverHintsAreClampedAndNeverShortened() {
        fun limited(retryAfterMs: Long) = ProviderHttpException("Chat Completions", 429, "{}", retryAfterMs)
        assertEquals(2_000L, retryDelayMs(1, limited(2_000L), lowest))
        assertTrue(retryDelayMs(1, limited(2_000L), highest) in 2_000L..2_200L)
        assertEquals(1_000L, retryDelayMs(1, limited(10L), lowest), "Under a second waits a second")
        assertEquals(300_000L, retryDelayMs(1, limited(3_600_000L), lowest), "An hour is capped at five minutes")
        val wrapped = AiProviderEngine.StepRetriesExhaustedException("Retries exhausted", limited(7_000L))
        assertEquals(7_000L, retryDelayMs(1, wrapped, lowest))
        assertEquals(3_000L, retryDelayMs(1, ProviderHttpException("X", 503, "{}"), lowest, fixedMs = 3_000L), "The legacy probe keeps its wait")
        assertEquals(20_000L, retryDelayMs(1, limited(20_000L), lowest, fixedMs = 3_000L), "but yields to a longer hint")
    }

    @Test fun backoffIsQuickForDroppedConnectionsAndGentlerForBusyServers() {
        val dropped = IllegalStateException("Stream ended before the provider finished its response.")
        assertEquals(1_600L, retryDelayMs(1, dropped, lowest))
        assertTrue(retryDelayMs(1, dropped, highest) <= 2_400L)
        val busy = ProviderHttpException("Anthropic Messages", 529, """{"type":"error","error":{"type":"overloaded_error"}}""")
        val waits = (1..5).map { attempt -> retryDelayMs(attempt, busy, lowest) to retryDelayMs(attempt, busy, highest) }
        assertEquals(4_000L, waits.first().first)
        waits.zipWithNext().forEach { (before, after) ->
            assertTrue(after.first >= before.second, "Five busy replies must wait longer each time: $waits")
        }
        (1..20).forEach { attempt ->
            assertTrue(retryDelayMs(attempt, busy, highest) <= 60_000L)
            assertTrue(retryDelayMs(attempt, dropped, highest) <= 60_000L)
        }
        assertEquals(4_000L, retryDelayMs(1, RuntimeException("HTTP 502: Bad Gateway"), lowest), "Older messages still read as server errors")
    }

    @Test fun theStatusDecidesBeforeTheWording() {
        fun http(status: Int, body: String = "{}") = ProviderHttpException("OpenAI Responses", status, body)
        listOf(408, 409, 425, 429, 500, 502, 503, 504, 524, 529).forEach { status ->
            assertTrue(AiProviderEngine.isTransientOrProxyTimeoutError(http(status)), "HTTP $status")
        }
        listOf(401, 402, 403, 404, 405, 410, 413, 422, 451, 501).forEach { status ->
            assertFalse(AiProviderEngine.isTransientOrProxyTimeoutError(http(status)), "HTTP $status")
        }
        assertTrue(AiProviderEngine.isEndpointUnsupportedForAutoFallback(http(404)))
        assertFalse(AiProviderEngine.isEndpointUnsupportedForAutoFallback(http(400, """{"error":{"message":"model not found"}}""")))
        assertFalse(AiProviderEngine.isEndpointUnsupportedForAutoFallback(http(502, "upstream not found")))
        assertTrue(AiProviderEngine.isTransientOrProxyTimeoutError(http(502, "upstream not found")))
        assertTrue(AiProviderEngine.isEndpointUnsupportedForAutoFallback(http(400, "Unrecognized request URL (POST /v1/responses)")))
        assertTrue(AiProviderEngine.isEndpointUnsupportedForAutoFallback(RuntimeException("Model not found")), "Untyped errors keep the wording check")
    }

    @Test fun spendCapsBillingAndDailyQuotasStopAtOnce() {
        val spendCap = ProviderHttpException(
            "Anthropic Messages",
            429,
            """{"type":"error","error":{"type":"rate_limit_error","message":"Spend limit reached.","details":{"error_code":"enforced_spend_limit_reached"}}}"""
        )
        assertTrue(AiProviderEngine.isHardAuthOrQuotaError(spendCap))
        assertFalse(AiProviderEngine.isTransientOrProxyTimeoutError(spendCap))
        val daily = ProviderHttpException("Gemini Interactions", 429, geminiError(quota("GenerateRequestsPerDayPerProjectPerModel-FreeTier"), retryInfo))
        assertTrue(AiProviderEngine.isHardAuthOrQuotaError(daily))
        assertFalse(AiProviderEngine.isTransientOrProxyTimeoutError(daily))
        val perMinute = ProviderHttpException("Gemini Interactions", 429, geminiError(quota("GenerateRequestsPerMinutePerProjectPerModel-FreeTier"), retryInfo))
        assertFalse(AiProviderEngine.isHardAuthOrQuotaError(perMinute))
        assertTrue(AiProviderEngine.isTransientOrProxyTimeoutError(perMinute))
        assertFalse(AiProviderEngine.shouldFallbackToLegacyProtocol(daily, useLegacyFallbackEndpoint = true))
    }

    @Test fun failuresThePersonCanFixAreNamed() {
        fun http(status: Int, body: String = "{}") = ProviderHttpException("OpenAI Responses", status, body)
        assertEquals(RunErrorKind.API_KEY, AiProviderEngine.runErrorKindOf(http(401, """{"error":{"code":"invalid_api_key"}}""")))
        assertEquals(RunErrorKind.API_KEY, AiProviderEngine.runErrorKindOf(http(403)))
        assertEquals(RunErrorKind.QUOTA, AiProviderEngine.runErrorKindOf(http(429, """{"error":{"code":"insufficient_quota"}}""")))
        assertEquals(RunErrorKind.QUOTA, AiProviderEngine.runErrorKindOf(http(402)))
        assertEquals(RunErrorKind.MODEL, AiProviderEngine.runErrorKindOf(http(404, """{"error":{"message":"The model `x` does not exist"}}""")))
        assertNull(AiProviderEngine.runErrorKindOf(http(404, "Not Found")), "A missing endpoint is not the model's fault")
        assertEquals(RunErrorKind.CONVERSATION_LIMIT, AiProviderEngine.runErrorKindOf(http(400, """{"error":{"code":"context_length_exceeded"}}""")))
        assertEquals(RunErrorKind.SPEND_LIMIT, AiProviderEngine.runErrorKindOf(AiProviderEngine.SpendLimitException(1.0)))
        assertNull(AiProviderEngine.runErrorKindOf(http(503)), "A busy server is worth retrying, not a person's fix")
    }

    @Test fun theMessageKeepsItsHistoricalShape() {
        assertEquals("Chat Completions HTTP 429: {}", ProviderHttpException("Chat Completions", 429, "{}").message)
        assertEquals("HTTP 500: down", ProviderHttpException("", 500, "down").message)
    }
}
