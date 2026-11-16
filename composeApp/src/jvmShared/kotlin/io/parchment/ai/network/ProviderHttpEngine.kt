package io.parchment.ai.network

import io.ktor.client.engine.HttpClientEngineConfig
import io.ktor.client.engine.HttpClientEngineFactory
import io.ktor.client.engine.okhttp.OkHttp

/** Both targets use OkHttp; naming it here spares Ktor's ServiceLoader scan and its R8/ProGuard keep rule. */
internal actual val providerHttpEngine: HttpClientEngineFactory<HttpClientEngineConfig> = OkHttp
