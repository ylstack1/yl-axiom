package io.parchment.ai.network

import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import java.io.ByteArrayOutputStream
import java.net.InetSocketAddress
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicLong
import java.util.zip.GZIPOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ModelRegistrySyncTest {
    private fun respondJson(
        exchange: com.sun.net.httpserver.HttpExchange,
        status: Int,
        body: String
    ) {
        val bytes = body.toByteArray()
        exchange.responseHeaders.set("Content-Type", "application/json")
        exchange.sendResponseHeaders(status, bytes.size.toLong())
        exchange.responseBody.use { it.write(bytes) }
    }

    @Test
    fun resolveOutputModalitiesTimeoutDoesNotCancelBackgroundWarmupAndGapFillsModelsDev() = runBlocking {
        val liteLlmRelease = CountDownLatch(1)
        val openRouterHits = AtomicInteger(0)
        val modelsDevHits = AtomicInteger(0)

        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.executor = Executors.newCachedThreadPool()
        server.createContext("/openrouter") { ex ->
            openRouterHits.incrementAndGet()
            respondJson(
                ex,
                200,
                """{"data":[{"id":"vendor/fast-image","name":"Fast Image","architecture":{"output_modalities":["image"]}}]}"""
            )
        }
        server.createContext("/openrouter-videos") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/vercel") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/aimlapi") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/litellm") { ex ->
            liteLlmRelease.await(10, TimeUnit.SECONDS)
            respondJson(ex, 200, """{"sample_spec":{}}""")
        }
        server.createContext("/models-dev") { ex ->
            modelsDevHits.incrementAndGet()
            respondJson(
                ex,
                200,
                """{"nichevendor":{"name":"Niche Vendor","models":{"niche/audio-only":{"id":"niche/audio-only","name":"Niche Audio","modalities":{"input":["text"],"output":["audio"]},"limit":{"context":32000}}}}}"""
            )
        }
        server.start()
        try {
            val base = "http://127.0.0.1:${server.address.port}"
            val client = RegistryCatalogClient(
                endpoints = RegistryEndpoints(
                    openRouterModelsUrl = "$base/openrouter",
                    openRouterVideosUrl = "$base/openrouter-videos",
                    vercelModelsUrl = "$base/vercel",
                    aimlApiModelsUrl = "$base/aimlapi",
                    liteLlmPricesUrl = "$base/litellm",
                    modelsDevApiUrl = "$base/models-dev"
                ),
                modalityWaitTimeoutMs = 200L
            )

            val earlyResult = withTimeout(5_000L) {
                client.resolveOutputModalities("niche/audio-only")
            }
            assertEquals(emptyList(), earlyResult, "Bounded modality wait should yield before slow sources finish")
            assertEquals(
                listOf("image"),
                ModelsDevClient.lookupSpecInCatalog(client.cachedCatalog, "vendor/fast-image")?.outputModalities,
                "Incremental snapshot should already expose fast OpenRouter results"
            )

            liteLlmRelease.countDown()
            val fullSpec = withTimeout(5_000L) {
                client.fetchModelSpec("niche/audio-only").getOrThrow()
            }
            assertEquals(listOf("audio"), fullSpec.outputModalities)
            assertEquals(32_000, fullSpec.contextWindowTokens)
            assertEquals(1, openRouterHits.get(), "Timed-out waiter must share single in-flight sync rather than restarting")
            assertEquals(1, modelsDevHits.get(), "models.dev must still fill gaps even after OpenRouter succeeds")
        } finally {
            liteLlmRelease.countDown()
            server.stop(0)
        }
    }

    @Test
    fun resolveOutputModalitiesReturnsEarlyOnceIncrementalSourceIndexesModel() = runBlocking {
        val liteLlmEntered = CountDownLatch(1)
        val liteLlmRelease = CountDownLatch(1)

        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.executor = Executors.newCachedThreadPool()
        server.createContext("/openrouter") { ex ->
            respondJson(
                ex,
                200,
                """{"data":[{"id":"vendor/instant-img","name":"Instant","architecture":{"output_modalities":["image"]}}]}"""
            )
        }
        server.createContext("/openrouter-videos") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/vercel") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/aimlapi") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/litellm") { ex ->
            liteLlmEntered.countDown()
            liteLlmRelease.await(10, TimeUnit.SECONDS)
            respondJson(ex, 200, """{}""")
        }
        server.createContext("/models-dev") { ex -> respondJson(ex, 200, """{}""") }
        server.start()
        try {
            val base = "http://127.0.0.1:${server.address.port}"
            val client = RegistryCatalogClient(
                endpoints = RegistryEndpoints(
                    openRouterModelsUrl = "$base/openrouter",
                    openRouterVideosUrl = "$base/openrouter-videos",
                    vercelModelsUrl = "$base/vercel",
                    aimlApiModelsUrl = "$base/aimlapi",
                    liteLlmPricesUrl = "$base/litellm",
                    modelsDevApiUrl = "$base/models-dev"
                ),
                modalityWaitTimeoutMs = 15_000L
            )

            val modalities = withTimeout(3_000L) {
                client.resolveOutputModalities("vendor/instant-img")
            }
            assertEquals(listOf("image"), modalities)
            assertTrue(liteLlmEntered.await(2, TimeUnit.SECONDS), "Remaining sync should keep running in background")
            liteLlmRelease.countDown()
            withTimeout(3_000L) {
                assertTrue(client.fetchModelSpec("vendor/instant-img").isSuccess)
            }
        } finally {
            liteLlmRelease.countDown()
            server.stop(0)
        }
    }

    @Test
    fun emptyCatalogFailureEnforcesCooldownForAutoCallsAndBypassesForManualRetry() = runBlocking {
        val totalRequests = AtomicInteger(0)
        val healthy = AtomicBoolean(false)
        val clockMs = AtomicLong(1_000L)

        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.executor = Executors.newCachedThreadPool()
        listOf("/openrouter", "/openrouter-videos", "/vercel", "/aimlapi", "/litellm").forEach { path ->
            server.createContext(path) { ex ->
                totalRequests.incrementAndGet()
                respondJson(ex, 503, "{}")
            }
        }
        server.createContext("/models-dev") { ex ->
            totalRequests.incrementAndGet()
            if (!healthy.get()) {
                respondJson(ex, 503, "{}")
            } else {
                respondJson(
                    ex,
                    200,
                    """{"vendor":{"name":"Vendor","models":{"vendor/recovered":{"id":"vendor/recovered","name":"Recovered","modalities":{"input":["text"],"output":["text"]},"limit":{"context":128000}}}}}"""
                )
            }
        }
        server.start()
        try {
            val base = "http://127.0.0.1:${server.address.port}"
            val client = RegistryCatalogClient(
                endpoints = RegistryEndpoints(
                    openRouterModelsUrl = "$base/openrouter",
                    openRouterVideosUrl = "$base/openrouter-videos",
                    vercelModelsUrl = "$base/vercel",
                    aimlApiModelsUrl = "$base/aimlapi",
                    liteLlmPricesUrl = "$base/litellm",
                    modelsDevApiUrl = "$base/models-dev"
                ),
                modalityWaitTimeoutMs = 2_000L,
                failureCooldownMs = 60_000L,
                nowMs = { clockMs.get() }
            )

            assertEquals(emptyList(), withTimeout(5_000L) { client.resolveOutputModalities("vendor/recovered") })
            val afterFirstFailure = totalRequests.get()
            assertEquals(6, afterFirstFailure)

            clockMs.set(10_000L)
            assertEquals(emptyList(), withTimeout(1_000L) { client.resolveOutputModalities("vendor/recovered") })
            assertTrue(withTimeout(1_000L) { client.fetchModelSpec("vendor/recovered").isFailure })
            assertEquals(afterFirstFailure, totalRequests.get(), "Auto calls inside cooldown must not hit failing endpoints again")

            healthy.set(true)
            val manual = withTimeout(5_000L) {
                client.fetchModelSpec("vendor/recovered", bypassFailureCooldown = true).getOrThrow()
            }
            assertEquals(128_000, manual.contextWindowTokens)
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun syncStreamsGzippedCatalogsThroughSlimDtoDecoders() = runBlocking {
        fun respondGzipJson(exchange: com.sun.net.httpserver.HttpExchange, body: String) {
            val gz = ByteArrayOutputStream().use { raw ->
                GZIPOutputStream(raw).use { it.write(body.toByteArray(Charsets.UTF_8)) }
                raw.toByteArray()
            }
            exchange.responseHeaders.set("Content-Type", "application/json")
            exchange.responseHeaders.set("Content-Encoding", "gzip")
            exchange.sendResponseHeaders(200, gz.size.toLong())
            exchange.responseBody.use { it.write(gz) }
        }

        val ignoredBlob = "x".repeat(4096)
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.executor = Executors.newCachedThreadPool()
        server.createContext("/openrouter") { ex ->
            respondGzipJson(
                ex,
                """{"data":[{"id":"vendor/gzip-chat","name":"Gzip Chat","description":"$ignoredBlob","supported_parameters":["temperature","tools"],"context_length":"131072","architecture":{"input_modalities":["text"],"output_modalities":["text"]},"pricing":{"prompt":"0.000001","completion":"0.000002","weird":{}}}]}"""
            )
        }
        server.createContext("/openrouter-videos") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/vercel") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/aimlapi") { ex -> respondJson(ex, 404, "{}") }
        server.createContext("/litellm") { ex -> respondJson(ex, 200, """{"sample_spec":{"max_tokens":"ignored"}}""") }
        server.createContext("/models-dev") { ex ->
            respondGzipJson(
                ex,
                """{"gzipvendor":{"name":"GzipVendor","doc":"$ignoredBlob","models":{"vendor/gzip-audio":{"id":"vendor/gzip-audio","name":"Gzip Audio","release_date":"2026-01-01","modalities":{"input":["text"],"output":["audio"]},"limit":{"context":64000,"output":8192}}}}}"""
            )
        }
        server.start()
        try {
            val base = "http://127.0.0.1:${server.address.port}"
            val client = RegistryCatalogClient(
                endpoints = RegistryEndpoints(
                    openRouterModelsUrl = "$base/openrouter",
                    openRouterVideosUrl = "$base/openrouter-videos",
                    vercelModelsUrl = "$base/vercel",
                    aimlApiModelsUrl = "$base/aimlapi",
                    liteLlmPricesUrl = "$base/litellm",
                    modelsDevApiUrl = "$base/models-dev"
                )
            )
            val chatSpec = withTimeout(5_000L) { client.fetchModelSpec("vendor/gzip-chat").getOrThrow() }
            val audioSpec = withTimeout(5_000L) { client.fetchModelSpec("vendor/gzip-audio").getOrThrow() }
            assertEquals(131_072, chatSpec.contextWindowTokens)
            assertEquals(1.0, chatSpec.inputPricePerM)
            assertEquals(listOf("audio"), audioSpec.outputModalities)
            assertEquals(64_000, audioSpec.contextWindowTokens)
        } finally {
            server.stop(0)
        }
    }
}
