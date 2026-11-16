package io.parchment.ai.network

import io.ktor.client.engine.HttpClientEngineConfig
import io.ktor.client.engine.HttpClientEngineFactory

/** The HTTP engine for provider and MCP traffic, chosen explicitly per platform (see the `jvmShared` actual). */
internal expect val providerHttpEngine: HttpClientEngineFactory<HttpClientEngineConfig>
