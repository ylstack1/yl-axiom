package io.parchment.ai.network

import io.ktor.client.request.prepareGet
import io.ktor.client.statement.bodyAsChannel
import io.ktor.http.isSuccess
import io.parchment.ai.model.ModelPricingSpec
import io.parchment.ai.model.TokenPricing
import io.parchment.ai.model.ContextPricingTier
import io.parchment.ai.model.ChargePricing
import io.parchment.ai.model.legacyTokenPricing
import io.parchment.ai.platform.PlatformRuntime
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

private const val OPENROUTER_MODELS_URL = "https://openrouter.ai/api/v1/models?output_modalities=all"
private const val OPENROUTER_VIDEOS_URL = "https://openrouter.ai/api/v1/videos/models"
private const val VERCEL_MODELS_URL = "https://ai-gateway.vercel.sh/v1/models"
private const val AIMLAPI_MODELS_URL = "https://api.aimlapi.com/models"
private const val LITELLM_PRICES_URL = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
private const val MODELS_DEV_API_URL = "https://models.dev/api.json"

private const val CACHE_TTL_MS = 30 * 60 * 1000L // 30 minutes
private const val FAILED_FETCH_COOLDOWN_MS = 60 * 1000L // 1 minute cooldown when all registries fail
private const val DEFAULT_MODALITY_WAIT_MS = 2_500L

/** Endpoints queried sequentially during a catalog refresh, ordered from highest-detail primary catalogs to gap-fillers. */
internal data class RegistryEndpoints(
    val openRouterModelsUrl: String = OPENROUTER_MODELS_URL,
    val openRouterVideosUrl: String = OPENROUTER_VIDEOS_URL,
    val vercelModelsUrl: String = VERCEL_MODELS_URL,
    val aimlApiModelsUrl: String = AIMLAPI_MODELS_URL,
    val liteLlmPricesUrl: String = LITELLM_PRICES_URL,
    val modelsDevApiUrl: String = MODELS_DEV_API_URL
)

/**
 * Coordinates single-flight background catalog synchronization so bounded send-time waits never cancel
 * multi-megabyte registry downloads mid-flight, publishes partial snapshots incrementally, and backs off
 * briefly when every registry is unreachable.
 */
internal class RegistryCatalogClient(
    private val endpoints: RegistryEndpoints = RegistryEndpoints(),
    private val modalityWaitTimeoutMs: Long = DEFAULT_MODALITY_WAIT_MS,
    private val failureCooldownMs: Long = FAILED_FETCH_COOLDOWN_MS,
    private val cacheTtlMs: Long = CACHE_TTL_MS,
    private val nowMs: () -> Long = { PlatformRuntime.currentTimeMillis() }
) {
    private val catalogScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val syncMutex = Mutex()
    private val catalogGeneration = MutableStateFlow(0L)
    private var inFlightSync: Deferred<Map<String, ModelPricingSpec>>? = null

    @Volatile
    var cachedCatalog: Map<String, ModelPricingSpec> = emptyMap()
        private set

    @Volatile
    private var lastFetchEpochMs: Long = Long.MIN_VALUE

    @Volatile
    private var lastFailureEpochMs: Long = Long.MIN_VALUE

    fun clearCache() {
        cachedCatalog = emptyMap()
        lastFetchEpochMs = Long.MIN_VALUE
    }

    private fun isCacheFreshLocked(now: Long): Boolean =
        cachedCatalog.isNotEmpty() &&
            lastFetchEpochMs != Long.MIN_VALUE &&
            (now - lastFetchEpochMs) in 0L until cacheTtlMs

    private fun isFailureInCooldownLocked(now: Long): Boolean =
        lastFailureEpochMs != Long.MIN_VALUE &&
            (now - lastFailureEpochMs) in 0L until failureCooldownMs

    private suspend fun getOrStartSync(
        bypassFailureCooldown: Boolean
    ): Pair<Map<String, ModelPricingSpec>, Deferred<Map<String, ModelPricingSpec>>?> = syncMutex.withLock {
        val now = nowMs()
        if (isCacheFreshLocked(now)) {
            return@withLock cachedCatalog to null
        }

        inFlightSync?.takeIf { it.isActive }?.let { active ->
            return@withLock cachedCatalog to active
        }

        if (!bypassFailureCooldown && isFailureInCooldownLocked(now)) {
            return@withLock cachedCatalog to null
        }

        lateinit var created: Deferred<Map<String, ModelPricingSpec>>
        created = catalogScope.async(start = CoroutineStart.LAZY) {
            try {
                runSyncPass()
            } finally {
                withContext(NonCancellable) {
                    syncMutex.withLock {
                        if (inFlightSync === created) {
                            inFlightSync = null
                        }
                    }
                }
            }
        }
        inFlightSync = created
        created.invokeOnCompletion {
            catalogGeneration.update { it + 1L }
        }
        created.start()
        cachedCatalog to created
    }

    private suspend fun <T> syncSource(
        url: String,
        timeoutMs: Long,
        deserializer: DeserializationStrategy<T>,
        catalog: MutableMap<String, ModelPricingSpec>,
        publishIncrementalSnapshot: () -> Unit,
        apply: (T, MutableMap<String, ModelPricingSpec>) -> Unit
    ) {
        val decoded = withTimeoutOrNull(timeoutMs) {
            runCatching {
                AiProviderEngine.httpClient.prepareGet(url).execute { resp ->
                    if (resp.status.isSuccess()) {
                        PlatformRuntime.decodeJsonStream(resp.bodyAsChannel(), ModelsDevClient.catalogJson, deserializer)
                    } else null
                }
            }.getOrNull()
        } ?: return
        runCatching { apply(decoded, catalog) }
        publishIncrementalSnapshot()
    }

    private suspend fun runSyncPass(): Map<String, ModelPricingSpec> {
        val staleCatalog = cachedCatalog
        val catalog = mutableMapOf<String, ModelPricingSpec>()

        fun publishIncrementalSnapshot() {
            if (catalog.isEmpty()) return
            // Publish an immutable overlay snapshot so fast sources become queryable before slower downloads end.
            cachedCatalog = if (staleCatalog.isEmpty()) catalog.toMap() else staleCatalog + catalog
            catalogGeneration.update { it + 1L }
        }

        // Stream sequentially to skip unreferenced metadata fields and avoid retaining multi-MB raw strings in heap.
        syncSource(endpoints.openRouterModelsUrl, 15_000L, OpenRouterCatalogDto.serializer(), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexOpenRouterCatalog(dto, dest, forceVideoOutput = false)
        }
        syncSource(endpoints.openRouterVideosUrl, 12_000L, OpenRouterCatalogDto.serializer(), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexOpenRouterCatalog(dto, dest, forceVideoOutput = true)
        }
        syncSource(endpoints.vercelModelsUrl, 12_000L, VercelCatalogDto.serializer(), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexVercelCatalog(dto, dest)
        }
        syncSource(endpoints.aimlApiModelsUrl, 15_000L, AimlApiCatalogDto.serializer(), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexAimlApiCatalog(dto, dest)
        }
        syncSource(endpoints.liteLlmPricesUrl, 18_000L, MapSerializer(String.serializer(), JsonElement.serializer()), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexLiteLlmCatalog(dto, dest)
        }
        syncSource(endpoints.modelsDevApiUrl, 15_000L, MapSerializer(String.serializer(), ModelsDevProviderDto.serializer()), catalog, ::publishIncrementalSnapshot) { dto, dest ->
            ModelsDevClient.indexModelsDevCatalog(dto, dest)
        }

        val completedAt = nowMs()
        if (catalog.isNotEmpty()) {
            cachedCatalog = catalog.toMap()
            lastFetchEpochMs = completedAt
            lastFailureEpochMs = Long.MIN_VALUE
        } else {
            lastFailureEpochMs = completedAt
        }
        catalogGeneration.update { it + 1L }
        return cachedCatalog
    }

    suspend fun resolveOutputModalities(modelId: String): List<String> {
        ModelsDevClient.lookupSpecInCatalog(cachedCatalog, modelId)
            ?.outputModalities
            ?.takeIf { it.isNotEmpty() }
            ?.let { return it }

        val (readyCatalog, deferred) = getOrStartSync(bypassFailureCooldown = false)
        if (deferred == null) {
            return ModelsDevClient.lookupSpecInCatalog(readyCatalog, modelId)?.outputModalities.orEmpty()
        }

        withTimeoutOrNull(modalityWaitTimeoutMs) {
            catalogGeneration.first {
                ModelsDevClient.lookupSpecInCatalog(cachedCatalog, modelId)?.outputModalities?.isNotEmpty() == true ||
                    deferred.isCompleted
            }
        }
        return ModelsDevClient.lookupSpecInCatalog(cachedCatalog, modelId)?.outputModalities.orEmpty()
    }

    /**
     * Resolves a model spec from the cached or freshly synced catalog. Automatic callers honor the empty-failure
     * cooldown; manual user actions pass [bypassFailureCooldown] = true to retry immediately.
     */
    suspend fun fetchModelSpec(
        modelId: String,
        bypassFailureCooldown: Boolean = false
    ): Result<ModelPricingSpec> = withContext(Dispatchers.IO) {
        try {
            val cleanTarget = modelId.trim().lowercase().removePrefix("models/")
            require(cleanTarget.isNotBlank()) { "Model ID is empty." }

            val (readyCatalog, deferred) = getOrStartSync(bypassFailureCooldown = bypassFailureCooldown)
            val catalog = deferred?.await() ?: readyCatalog
            val matched = ModelsDevClient.lookupSpecInCatalog(catalog, cleanTarget)
                ?: error("Model '$modelId' was not found in the unified model registry.")

            Result.success(matched.copy(modelId = modelId.trim()))
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (t: Throwable) {
            Result.failure(t)
        }
    }
}

/**
 * Multi-Source Unified Model Registry Client.
 *
 * Fetches and merges live model pricing, context window limits, creator/vendor metadata,
 * and input/output modalities (`text`, `image`, `video`, `audio`) from multiple complementary
 * zero-auth public registries:
 * 1. OpenRouter Multimodal API (`https://openrouter.ai/api/v1/models?output_modalities=all`)
 * 2. OpenRouter Video API (`https://openrouter.ai/api/v1/videos/models`)
 * 3. Vercel AI Gateway Catalog (`https://ai-gateway.vercel.sh/v1/models`)
 * 4. AIMLAPI Multimodal & Video Catalog (`https://api.aimlapi.com/models`)
 * 5. LiteLLM Official Registry (`https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`)
 * 6. `models.dev` (`https://models.dev/api.json`) for gap-filling and fallback
 */
@Serializable
internal class OpenRouterArchDto(
    val modality: String? = null,
    @SerialName("input_modalities") val inputModalities: JsonElement? = null,
    @SerialName("output_modalities") val outputModalities: JsonElement? = null
)

@Serializable
internal class OpenRouterTopProviderDto(
    @SerialName("max_completion_tokens") val maxCompletionTokens: JsonElement? = null
)

@Serializable
internal class OpenRouterModelDto(
    val id: String? = null,
    val name: String? = null,
    val architecture: OpenRouterArchDto? = null,
    @SerialName("input_modalities") val inputModalities: JsonElement? = null,
    @SerialName("output_modalities") val outputModalities: JsonElement? = null,
    @SerialName("context_length") val contextLength: JsonElement? = null,
    @SerialName("top_provider") val topProvider: OpenRouterTopProviderDto? = null,
    val pricing: JsonObject? = null
)

@Serializable
internal class OpenRouterCatalogDto(val data: List<OpenRouterModelDto> = emptyList())

@Serializable
internal class VercelModalitiesDto(val input: JsonElement? = null, val output: JsonElement? = null)

@Serializable
internal class VercelModelDto(
    val id: String? = null,
    val name: String? = null,
    @SerialName("owned_by") val ownedBy: String? = null,
    val type: String? = null,
    val modalities: VercelModalitiesDto? = null,
    @SerialName("context_window") val contextWindow: JsonElement? = null,
    @SerialName("max_tokens") val maxTokens: JsonElement? = null,
    val pricing: JsonObject? = null
)

@Serializable
internal class VercelCatalogDto(val data: List<VercelModelDto> = emptyList())

@Serializable
internal class AimlApiInfoDto(
    val name: String? = null,
    val developer: String? = null,
    val contextLength: JsonElement? = null,
    val maxTokens: JsonElement? = null
)

@Serializable
internal class AimlApiModelDto(
    val id: String? = null,
    val type: String? = null,
    val tags: JsonElement? = null,
    val aliases: JsonElement? = null,
    val info: AimlApiInfoDto? = null
)

@Serializable
internal class AimlApiCatalogDto(val data: List<AimlApiModelDto> = emptyList())

@Serializable
internal class ModelsDevLimitDto(val context: JsonElement? = null, val output: JsonElement? = null)

@Serializable
internal class ModelsDevModalitiesDto(val input: JsonElement? = null, val output: JsonElement? = null)

@Serializable
internal class ModelsDevModelDto(
    val id: String? = null,
    val name: String? = null,
    val cost: JsonObject? = null,
    val limit: ModelsDevLimitDto? = null,
    val modalities: ModelsDevModalitiesDto? = null
)

@Serializable
internal class ModelsDevProviderDto(
    val name: String? = null,
    val models: Map<String, ModelsDevModelDto> = emptyMap()
)

object ModelsDevClient {
    private val defaultCatalogClient = RegistryCatalogClient()

    internal val catalogJson = Json {
        ignoreUnknownKeys = true
        isLenient = true
        coerceInputValues = true
    }
    private val CONTEXT_TIER_PATTERN = Regex("context_over_([0-9]+)([km]?)")
    private val LITELLM_CONTEXT_TIER_PATTERN = Regex("(input_cost_per_token|output_cost_per_token|cache_read_input_token_cost|cache_creation_input_token_cost|cache_creation_input_token_cost_above_(?:1|12)hr)_above_([0-9]+)([km]?)_tokens")

    private val TRAILING_SNAPSHOT_REGEX = Regex(
        "-(?:20\\d{6}|\\d{6}|20\\d{2}-\\d{2}-\\d{2}|00\\d|latest|preview)$"
    )
    private val VERSION_DOT_BETWEEN_DIGITS_REGEX = Regex("(?<=\\d)\\.(?=\\d)")
    private val VERSION_DASH_BETWEEN_DIGITS_REGEX = Regex("(?<=\\d)-(?=\\d)")
    private val VERSION_V_PREFIX_REGEX = Regex("(?<=-)v(?=\\d)")
    private val WRAPPER_PREFIX_REGEX = Regex("^(?:doubao-|dola-|dreamina-)")

    private val AGGREGATOR_PROVIDER_NAMES = setOf(
        "openrouter", "vercel", "aimlapi", "litellm", "fal_ai", "fal-ai",
        "together_ai", "fireworks_ai", "deepinfra", "replicate", "bedrock", "azure", "vertex_ai"
    )

    /**
     * Normalizes a model identifier into a canonical vendor-stripped, version-normalized slug
     * so `bytedance-seed/seedream-5-0-flash`, `bytedance/seedream-5.0-flash`, and
     * `volcengine/doubao-seedream-5-0-260128` all converge to `seedream-5-0-flash`.
     */
    internal fun normalizeCanonicalModelSlug(raw: String): String {
        var s = raw.trim().lowercase().removePrefix("models/")
        if (s.contains('/')) {
            s = s.substringAfterLast('/')
        }
        if (s.contains(':')) {
            s = s.substringBefore(':')
        }
        s = WRAPPER_PREFIX_REGEX.replace(s, "")
        s = TRAILING_SNAPSHOT_REGEX.replace(s, "")
        s = VERSION_V_PREFIX_REGEX.replace(s, "")
        s = VERSION_DOT_BETWEEN_DIGITS_REGEX.replace(s, "-")
        return s.trim('-', '_', '.', ' ')
    }

    /**
     * Merges two [ModelPricingSpec] entries for the same model across registries while
     * strictly preserving non-text output modalities (`image`, `video`, `audio`) so a
     * generic `["text"]` fallback never overwrites a media modality.
     */
    internal fun mergeModelSpecs(
        existing: ModelPricingSpec,
        incoming: ModelPricingSpec
    ): ModelPricingSpec {
        val mergedInputs = (existing.inputModalities + incoming.inputModalities)
            .map { it.trim().lowercase() }
            .filter { it.isNotEmpty() }
            .distinct()

        val existingNonTextOutputs = existing.outputModalities.filter { it != "text" }
        val incomingNonTextOutputs = incoming.outputModalities.filter { it != "text" }

        val mergedOutputs = when {
            existingNonTextOutputs.isNotEmpty() && incomingNonTextOutputs.isEmpty() ->
                existing.outputModalities
            incomingNonTextOutputs.isNotEmpty() && existingNonTextOutputs.isEmpty() ->
                incoming.outputModalities
            existingNonTextOutputs.isNotEmpty() && incomingNonTextOutputs.isNotEmpty() -> {
                val combined = (existing.outputModalities + incoming.outputModalities).distinct()
                // Only keep "text" alongside media if at least one source explicitly reported interleaved text+media
                val hasInterleavedText = (existing.outputModalities.contains("text") && existingNonTextOutputs.isNotEmpty()) ||
                    (incoming.outputModalities.contains("text") && incomingNonTextOutputs.isNotEmpty())
                if (hasInterleavedText) combined else combined.filter { it != "text" }
            }
            else -> (existing.outputModalities + incoming.outputModalities).distinct()
        }

        val isPureMedia = mergedOutputs.any { it == "image" || it == "video" || it == "audio" } &&
            !mergedOutputs.contains("text")

        val bestContextWindow = when {
            isPureMedia -> maxOf(
                if (existing.contextWindowTokens == 128000) 0 else existing.contextWindowTokens,
                if (incoming.contextWindowTokens == 128000) 0 else incoming.contextWindowTokens
            )
            existing.contextWindowTokens > 0 && existing.contextWindowTokens != 128000 ->
                existing.contextWindowTokens
            incoming.contextWindowTokens > 0 && incoming.contextWindowTokens != 128000 ->
                incoming.contextWindowTokens
            else -> maxOf(existing.contextWindowTokens, incoming.contextWindowTokens)
        }

        val bestMaxOutput = when {
            (existing.maxOutputTokens ?: 0) > 0 -> existing.maxOutputTokens
            (incoming.maxOutputTokens ?: 0) > 0 -> incoming.maxOutputTokens
            else -> existing.maxOutputTokens ?: incoming.maxOutputTokens
        }

        val bestProviderName = when {
            existing.providerName.isNotBlank() &&
                existing.providerName.lowercase() !in AGGREGATOR_PROVIDER_NAMES -> existing.providerName
            incoming.providerName.isNotBlank() &&
                incoming.providerName.lowercase() !in AGGREGATOR_PROVIDER_NAMES -> incoming.providerName
            existing.providerName.isNotBlank() -> existing.providerName
            else -> incoming.providerName
        }

        val mergedPricing = if (existing.knownPricing != null || incoming.knownPricing != null) {
            (existing.knownPricing ?: legacyTokenPricing(existing.inputPricePerM, existing.outputPricePerM, existing.cacheReadPricePerM,
                existing.cacheWritePricePerM, existing.reasoningPricePerM)).withMissingFrom(incoming.knownPricing
                ?: legacyTokenPricing(incoming.inputPricePerM, incoming.outputPricePerM, incoming.cacheReadPricePerM,
                    incoming.cacheWritePricePerM, incoming.reasoningPricePerM))
        } else null
        return existing.copy(
            displayName = existing.displayName.ifBlank { incoming.displayName },
            providerName = bestProviderName,
            inputPricePerM = mergedPricing?.inputPerM ?: if (existing.inputPricePerM > 0.0) existing.inputPricePerM else incoming.inputPricePerM,
            outputPricePerM = mergedPricing?.outputPerM ?: if (existing.outputPricePerM > 0.0) existing.outputPricePerM else incoming.outputPricePerM,
            cacheReadPricePerM = mergedPricing?.cacheReadPerM ?: if (existing.cacheReadPricePerM > 0.0) existing.cacheReadPricePerM else incoming.cacheReadPricePerM,
            cacheWritePricePerM = mergedPricing?.cacheWritePerM ?: if (existing.cacheWritePricePerM > 0.0) existing.cacheWritePricePerM else incoming.cacheWritePricePerM,
            reasoningPricePerM = mergedPricing?.reasoningPerM ?: if (existing.reasoningPricePerM > 0.0) existing.reasoningPricePerM else incoming.reasoningPricePerM,
            knownPricing = mergedPricing,
            contextPricingTiers = (existing.contextPricingTiers + incoming.contextPricingTiers).distinctBy { it.inputTokensAbove },
            chargePricing = existing.chargePricing.withMissingFrom(incoming.chargePricing),
            contextWindowTokens = bestContextWindow,
            maxOutputTokens = bestMaxOutput,
            outputModalities = mergedOutputs,
            inputModalities = mergedInputs
        )
    }

    internal fun indexModelSpec(
        catalog: MutableMap<String, ModelPricingSpec>,
        rawId: String,
        spec: ModelPricingSpec,
        aliases: List<String> = emptyList()
    ) {
        val cleanId = rawId.trim().lowercase().removePrefix("models/")
        if (cleanId.isEmpty()) return
        val shortKey = cleanId.substringAfterLast('/')
        val canonicalKey = normalizeCanonicalModelSlug(cleanId)
        val dotVariantKey = canonicalKey.replace(VERSION_DASH_BETWEEN_DIGITS_REGEX, ".")

        val keys = buildList {
            add(cleanId)
            if (shortKey.isNotEmpty()) add(shortKey)
            if (canonicalKey.isNotEmpty()) add(canonicalKey)
            if (dotVariantKey.isNotEmpty()) add(dotVariantKey)
            for (alias in aliases) {
                val cleanAlias = alias.trim().lowercase().removePrefix("models/")
                if (cleanAlias.isNotEmpty()) {
                    add(cleanAlias)
                    add(cleanAlias.substringAfterLast('/'))
                    val normAlias = normalizeCanonicalModelSlug(cleanAlias)
                    if (normAlias.isNotEmpty()) add(normAlias)
                }
            }
        }.distinct()

        var merged = spec
        var distinctPrev: ModelPricingSpec? = null
        for (key in keys) {
            val prev = catalog[key]
            if (prev != null && prev !== distinctPrev) {
                merged = mergeModelSpecs(prev, merged)
                distinctPrev = prev
            }
        }
        for (key in keys) {
            catalog[key] = merged
        }
    }

    internal fun lookupSpecInCatalog(
        catalog: Map<String, ModelPricingSpec>,
        modelId: String
    ): ModelPricingSpec? {
        if (catalog.isEmpty()) return null
        val cleanTarget = modelId.trim().lowercase().removePrefix("models/")
        if (cleanTarget.isEmpty()) return null
        val withoutSlash = cleanTarget.substringAfterLast('/')
        val canonical = normalizeCanonicalModelSlug(cleanTarget)
        val dotCanonical = canonical.replace(VERSION_DASH_BETWEEN_DIGITS_REGEX, ".")

        catalog[cleanTarget]?.let { return it }
        catalog[withoutSlash]?.let { return it }
        catalog[canonical]?.let { return it }
        catalog[dotCanonical]?.let { return it }

        // A shortened identifier may resolve only to one model at a delimiter boundary.
        // Never let an unknown longer name inherit metadata from a shorter catalog key.
        val checkWithoutSlash = withoutSlash.length >= 6
        val checkCanonical = canonical.length >= 6 && canonical != withoutSlash
        if (!checkWithoutSlash && !checkCanonical) return null

        var matched: ModelPricingSpec? = null
        var matchedSlug: String? = null
        for ((key, spec) in catalog) {
            val matchesWithout = checkWithoutSlash && key.startsWith(withoutSlash) &&
                isDelimiterChar(key.getOrNull(withoutSlash.length))
            val matchesCanonical = !matchesWithout && checkCanonical && key.startsWith(canonical) &&
                isDelimiterChar(key.getOrNull(canonical.length))
            if (!matchesWithout && !matchesCanonical) continue

            val specSlug = normalizeCanonicalModelSlug(spec.modelId)
            if (matched == null) {
                matched = spec
                matchedSlug = specSlug
            } else if (specSlug != matchedSlug) {
                return null
            }
        }
        return matched
    }

    private fun isDelimiterChar(ch: Char?): Boolean = ch == '-' || ch == '_' || ch == '.'

    /**
     * Returns the cached `outputModalities` (`text`, `image`, `video`, `audio`) for [modelId]
     * if the registry has already been fetched during this session, or an empty list otherwise.
     */
    fun getCachedOutputModalities(modelId: String): List<String> =
        lookupSpecInCatalog(defaultCatalogClient.cachedCatalog, modelId)?.outputModalities.orEmpty()

    /** Releases the bulk multi-thousand-model catalog from heap when the app is minimized or under memory pressure. */
    fun trimMemoryCache() {
        defaultCatalogClient.clearCache()
    }

    internal fun getCachedSpec(modelId: String): ModelPricingSpec? =
        lookupSpecInCatalog(defaultCatalogClient.cachedCatalog, modelId)

    /**
     * Returns the cached `inputModalities` for [modelId] if already loaded.
     */
    fun getCachedInputModalities(modelId: String): List<String> =
        lookupSpecInCatalog(defaultCatalogClient.cachedCatalog, modelId)?.inputModalities.orEmpty()

    /**
     * Returns the cached vendor/creator name for [modelId] if known in the registry.
     */
    fun getCachedVendorSlug(modelId: String): String? =
        lookupSpecInCatalog(defaultCatalogClient.cachedCatalog, modelId)?.providerName?.takeIf { it.isNotBlank() }

    /**
     * Ensures the registry catalog is warming in the background (waiting only up to a short bound
     * without cancelling the in-flight sync) and returns any known output modalities for [modelId].
     */
    suspend fun resolveOutputModalities(modelId: String): List<String> =
        defaultCatalogClient.resolveOutputModalities(modelId)

    suspend fun fetchModelSpec(
        modelId: String,
        bypassFailureCooldown: Boolean = false
    ): Result<ModelPricingSpec> =
        defaultCatalogClient.fetchModelSpec(modelId, bypassFailureCooldown = bypassFailureCooldown)

    internal fun indexOpenRouterCatalog(
        dto: OpenRouterCatalogDto,
        catalog: MutableMap<String, ModelPricingSpec>,
        forceVideoOutput: Boolean = false
    ) {
        for (model in dto.data) {
            val rawId = model.id?.trim().orEmpty()
            if (rawId.isEmpty()) continue
            val name = model.name ?: rawId
            val vendorFromId = rawId.substringBefore('/', "").trim()

            val archObj = model.architecture
            val modalityStr = archObj?.modality?.lowercase().orEmpty()
            val inMods = parseStringList(archObj?.inputModalities ?: model.inputModalities).ifEmpty {
                if (modalityStr.contains("->")) {
                    modalityStr.substringBefore("->").split('+').map { it.trim() }.filter { it.isNotEmpty() }
                } else emptyList()
            }
            val outMods = if (forceVideoOutput) {
                listOf("video")
            } else {
                parseStringList(archObj?.outputModalities ?: model.outputModalities).ifEmpty {
                    when {
                        modalityStr.endsWith("->video") -> listOf("video")
                        modalityStr.endsWith("->image") -> listOf("image")
                        modalityStr.endsWith("->audio") || modalityStr.endsWith("->speech") -> listOf("audio")
                        modalityStr.contains("->") ->
                            modalityStr.substringAfter("->").split('+').map { it.trim() }.filter { it.isNotEmpty() }
                        else -> emptyList()
                    }
                }
            }

            val contextLen = model.contextLength.intCompat() ?: 0
            val maxCompletion = model.topProvider?.maxCompletionTokens.intCompat()
            val pricingObj = model.pricing
            val inputPricePerM = parsePerTokenToPerMillion(pricingObj?.get("prompt"))
            val outputPricePerM = parsePerTokenToPerMillion(pricingObj?.get("completion"))
            val cacheReadPerM = parsePerTokenToPerMillion(pricingObj?.get("input_cache_read"))
            val cacheWritePerM = parsePerTokenToPerMillion(pricingObj?.get("input_cache_write"))
            val reasoningPerM = parsePerTokenToPerMillion(pricingObj?.get("internal_reasoning"))

            val spec = ModelPricingSpec(
                modelId = rawId,
                displayName = name,
                providerName = vendorFromId,
                inputPricePerM = inputPricePerM,
                outputPricePerM = outputPricePerM,
                cacheReadPricePerM = cacheReadPerM,
                cacheWritePricePerM = cacheWritePerM,
                reasoningPricePerM = reasoningPerM,
                knownPricing = pricing(pricingObj, "prompt", "completion", "input_cache_read", "input_cache_write", "internal_reasoning", 1_000_000.0),
                chargePricing = ChargePricing(buildMap {
                    readRate(pricingObj?.get("web_search"))?.let { put("web_search", it) }
                    readRate(pricingObj?.get("request"))?.let { put("request", it) }
                    readRate(pricingObj?.get("audio"))?.let { put("input_audio", it) }
                    readRate(pricingObj?.get("audio_output"))?.let { put("output_audio", it) }
                    readRate(pricingObj?.get("input_audio_cache"))?.let { put("cached_input_audio", it) }
                    readRate(pricingObj?.get("image"))?.let { put("input_image_count", it) }
                    readRate(pricingObj?.get("image_output"))?.let { put("output_image_count", it) }
                }),
                contextWindowTokens = contextLen,
                maxOutputTokens = maxCompletion,
                outputModalities = outMods,
                inputModalities = inMods
            )
            indexModelSpec(catalog, rawId, spec)
        }
    }

    internal fun indexVercelCatalog(
        dto: VercelCatalogDto,
        catalog: MutableMap<String, ModelPricingSpec>
    ) {
        for (model in dto.data) {
            val rawId = model.id?.trim().orEmpty()
            if (rawId.isEmpty()) continue
            val name = model.name ?: rawId
            val ownedBy = model.ownedBy ?: rawId.substringBefore('/', "")
            val typeStr = model.type?.lowercase().orEmpty()

            val modalitiesObj = model.modalities
            val inMods = parseStringList(modalitiesObj?.input)
            val outMods = parseStringList(modalitiesObj?.output).ifEmpty {
                when (typeStr) {
                    "image" -> listOf("image")
                    "video" -> listOf("video")
                    "audio", "speech", "tts" -> listOf("audio")
                    "language", "chat" -> listOf("text")
                    else -> emptyList()
                }
            }

            val contextWin = model.contextWindow.intCompat() ?: 0
            val maxTokens = model.maxTokens.intCompat()
            val pricingObj = model.pricing
            val inputPricePerM = parsePerTokenToPerMillion(pricingObj?.get("input"))
            val outputPricePerM = parsePerTokenToPerMillion(pricingObj?.get("output"))
            val cacheReadPerM = parsePerTokenToPerMillion(pricingObj?.get("input_cache_read"))
            val cacheWritePerM = parsePerTokenToPerMillion(pricingObj?.get("input_cache_write"))

            val spec = ModelPricingSpec(
                modelId = rawId,
                displayName = name,
                providerName = ownedBy,
                inputPricePerM = inputPricePerM,
                outputPricePerM = outputPricePerM,
                cacheReadPricePerM = cacheReadPerM,
                cacheWritePricePerM = cacheWritePerM,
                knownPricing = pricing(pricingObj, "input", "output", "input_cache_read", "input_cache_write", multiplier = 1_000_000.0),
                contextWindowTokens = contextWin,
                maxOutputTokens = maxTokens,
                outputModalities = outMods,
                inputModalities = inMods
            )
            indexModelSpec(catalog, rawId, spec)
        }
    }

    internal fun indexAimlApiCatalog(
        dto: AimlApiCatalogDto,
        catalog: MutableMap<String, ModelPricingSpec>
    ) {
        for (model in dto.data) {
            val rawId = model.id?.trim().orEmpty()
            if (rawId.isEmpty()) continue
            val typeStr = model.type?.lowercase().orEmpty()
            val tags = parseStringList(model.tags)
            val aliases = parseStringList(model.aliases)
            val infoObj = model.info
            val name = infoObj?.name ?: rawId
            val developer = infoObj?.developer ?: rawId.substringBefore('/', "")
            val contextLen = infoObj?.contextLength.intCompat() ?: 0
            val maxTokens = infoObj?.maxTokens.intCompat()

            val outMods = when {
                typeStr.contains("video") || tags.any { it.contains("video") } -> listOf("video")
                typeStr.contains("image") || tags.any { it.contains("image") } -> listOf("image")
                typeStr.contains("tts") || typeStr.contains("speech") ||
                    typeStr.contains("audio") || typeStr.contains("music") -> listOf("audio")
                typeStr.contains("chat") || typeStr.contains("completion") ||
                    typeStr.contains("response") || tags.any { it.contains("chat") } -> listOf("text")
                else -> emptyList()
            }

            val spec = ModelPricingSpec(
                modelId = rawId,
                displayName = name,
                providerName = developer,
                contextWindowTokens = contextLen,
                maxOutputTokens = maxTokens,
                outputModalities = outMods
            )
            indexModelSpec(catalog, rawId, spec, aliases = aliases)
        }
    }

    internal fun indexLiteLlmCatalog(
        root: Map<String, JsonElement>,
        catalog: MutableMap<String, ModelPricingSpec>
    ) {
        for ((modelKey, element) in root) {
            if (modelKey == "sample_spec") continue
            val obj = element as? JsonObject ?: continue
            val mode = obj["mode"]?.jsonPrimitive?.contentOrNull?.lowercase().orEmpty()
            val provider = obj["litellm_provider"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val supportsVision = obj["supports_vision"]?.jsonPrimitive?.booleanOrNull == true
            val supportsAudioIn = obj["supports_audio_input"]?.jsonPrimitive?.booleanOrNull == true
            val supportsAudioOut = obj["supports_audio_output"]?.jsonPrimitive?.booleanOrNull == true
            val supportsPdf = obj["supports_pdf_input"]?.jsonPrimitive?.booleanOrNull == true

            val inMods = buildList {
                add("text")
                if (supportsVision || mode == "image_edit") add("image")
                if (supportsAudioIn || mode == "audio_transcription") add("audio")
                if (supportsPdf) add("pdf")
            }

            val outMods = when (mode) {
                "image_generation", "image_edit" -> listOf("image")
                "video_generation" -> listOf("video")
                "audio_speech" -> listOf("audio")
                "chat", "completion", "responses" ->
                    if (supportsAudioOut) listOf("text", "audio") else listOf("text")
                else -> emptyList()
            }

            val maxInput = obj["max_input_tokens"]?.jsonPrimitive?.intOrNull
                ?: obj["max_tokens"]?.jsonPrimitive?.intOrNull ?: 0
            val maxOutput = obj["max_output_tokens"]?.jsonPrimitive?.intOrNull
                ?: obj["max_tokens"]?.jsonPrimitive?.intOrNull

            val inputCostPerM = parsePerTokenToPerMillion(obj["input_cost_per_token"])
            val outputCostPerM = parsePerTokenToPerMillion(obj["output_cost_per_token"])
            val cacheReadPerM = parsePerTokenToPerMillion(obj["cache_read_input_token_cost"])
            val cacheWritePerM = parsePerTokenToPerMillion(obj["cache_creation_input_token_cost"])

            val spec = ModelPricingSpec(
                modelId = modelKey,
                displayName = modelKey.substringAfterLast('/'),
                providerName = provider,
                inputPricePerM = inputCostPerM,
                outputPricePerM = outputCostPerM,
                cacheReadPricePerM = cacheReadPerM,
                cacheWritePricePerM = cacheWritePerM,
                knownPricing = pricing(obj, "input_cost_per_token", "output_cost_per_token", "cache_read_input_token_cost", "cache_creation_input_token_cost", multiplier = 1_000_000.0),
                contextPricingTiers = liteLlmContextTiers(obj),
                contextWindowTokens = maxInput,
                maxOutputTokens = maxOutput,
                outputModalities = outMods,
                inputModalities = inMods
            )
            indexModelSpec(catalog, modelKey, spec)
        }
    }

    internal fun indexModelsDevCatalog(
        rootObj: Map<String, ModelsDevProviderDto>,
        catalog: MutableMap<String, ModelPricingSpec>
    ) {
        for ((providerKey, providerDto) in rootObj) {
            val providerName = providerDto.name ?: providerKey
            for ((modelKey, modelDto) in providerDto.models) {
                val id = modelDto.id ?: modelKey
                val name = modelDto.name ?: id

                val costObj = modelDto.cost
                val inputCost = readRate(costObj?.get("input")) ?: 0.0
                val outputCost = readRate(costObj?.get("output")) ?: 0.0
                val cacheReadCost = readRate(costObj?.get("cache_read")) ?: 0.0
                val cacheWriteCost = readRate(costObj?.get("cache_write")) ?: 0.0
                val reasoningCost = readRate(costObj?.get("reasoning")) ?: 0.0

                val limitObj = modelDto.limit
                val contextWindow = limitObj?.context.intCompat() ?: 0
                val maxOutput = limitObj?.output.intCompat()

                val modalitiesObj = modelDto.modalities
                val inModalities = parseStringList(modalitiesObj?.input)
                val outModalities = parseStringList(modalitiesObj?.output)

                val spec = ModelPricingSpec(
                    modelId = id,
                    displayName = name,
                    providerName = providerName,
                    inputPricePerM = inputCost,
                    outputPricePerM = outputCost,
                    cacheReadPricePerM = cacheReadCost,
                    cacheWritePricePerM = cacheWriteCost,
                    reasoningPricePerM = reasoningCost,
                    knownPricing = pricing(costObj, "input", "output", "cache_read", "cache_write"),
                    chargePricing = ChargePricing(buildMap {
                        readRate(costObj?.get("input_audio"), 0.000001)?.let { put("input_audio", it) }
                        readRate(costObj?.get("output_audio"), 0.000001)?.let { put("output_audio", it) }
                    }),
                    contextPricingTiers = contextTiers(costObj),
                    contextWindowTokens = contextWindow,
                    maxOutputTokens = maxOutput,
                    outputModalities = outModalities,
                    inputModalities = inModalities
                )
                indexModelSpec(catalog, id, spec, aliases = listOf(modelKey))
            }
        }
    }

    private fun JsonElement?.intCompat(): Int? {
        val primitive = this as? JsonPrimitive ?: return null
        return primitive.intOrNull
            ?: primitive.doubleOrNull?.takeIf { it.isFinite() }?.toInt()
            ?: primitive.contentOrNull?.trim()?.toIntOrNull()
    }

    private fun parseStringList(element: JsonElement?): List<String> {
        val arr = element as? JsonArray ?: return emptyList()
        return arr.mapNotNull { (it as? JsonPrimitive)?.contentOrNull?.trim()?.lowercase() }
            .filter { it.isNotEmpty() }
            .distinct()
    }

    private fun readRate(element: JsonElement?, multiplier: Double = 1.0): Double? {
        val primitive = element as? kotlinx.serialization.json.JsonPrimitive ?: return null
        val raw = primitive.doubleOrNull ?: primitive.contentOrNull?.toDoubleOrNull() ?: return null
        return (raw * multiplier).takeIf { raw >= 0 && it.isFinite() && it >= 0 }
    }

    private fun parsePerTokenToPerMillion(element: JsonElement?): Double = readRate(element, 1_000_000.0) ?: 0.0

    private fun pricing(source: JsonObject?, input: String, output: String, cached: String, write: String,
        reasoning: String = "reasoning", multiplier: Double = 1.0): TokenPricing? = TokenPricing(
        readRate(source?.get(input), multiplier), readRate(source?.get(output), multiplier),
        readRate(source?.get(cached), multiplier), readRate(source?.get(write), multiplier),
        readRate(source?.get(reasoning), multiplier),
        readRate(source?.get("cache_creation_input_token_cost_above_12hr")
            ?: source?.get("cache_creation_input_token_cost_above_1hr")
            ?: source?.get("input_cache_write_1h"), multiplier)
    ).takeIf { it.hasAnyRate }

    private fun contextTiers(cost: JsonObject?): List<ContextPricingTier> = cost?.mapNotNull { (key, value) ->
        val match = CONTEXT_TIER_PATTERN.matchEntire(key) ?: return@mapNotNull null
        val scalar = when (match.groupValues[2]) { "k" -> 1000L; "m" -> 1_000_000L; else -> 1L }
        val count = match.groupValues[1].toLongOrNull()?.takeIf { it <= Long.MAX_VALUE / scalar } ?: return@mapNotNull null
        val rates = pricing(value as? JsonObject, "input", "output", "cache_read", "cache_write") ?: return@mapNotNull null
        ContextPricingTier(count * scalar, rates)
    }.orEmpty()

    private fun liteLlmContextTiers(source: JsonObject): List<ContextPricingTier> {
        val groups = mutableMapOf<Long, MutableMap<String, JsonElement>>()
        source.forEach { (key, value) ->
            val match = LITELLM_CONTEXT_TIER_PATTERN.matchEntire(key) ?: return@forEach
            val scalar = when (match.groupValues[3]) { "k" -> 1000L; "m" -> 1_000_000L; else -> 1L }
            val count = match.groupValues[2].toLongOrNull()?.takeIf { it <= Long.MAX_VALUE / scalar } ?: return@forEach
            groups.getOrPut(count * scalar) { mutableMapOf() }[match.groupValues[1]] = value
        }
        return groups.mapNotNull { (threshold, values) ->
            pricing(JsonObject(values), "input_cost_per_token", "output_cost_per_token", "cache_read_input_token_cost",
                "cache_creation_input_token_cost", multiplier = 1_000_000.0)?.let { ContextPricingTier(threshold, it) }
        }
    }
}
