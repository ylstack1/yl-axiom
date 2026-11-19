//! Single-flight live model catalog & pricing sync (`ModelsDevClient` / `RegistryCatalogClient`).
//!
//! Discovers models, modalities, and token pricing dynamically from live registries
//! (OpenRouter -> Vercel AI Gateway -> LiteLLM -> `models.dev` gap-filler) with:
//! - Single-flight background `tokio` execution (`2.5 s` bounded modality wait never cancels download).
//! - Incremental snapshot publication as each source completes.
//! - `60 s` cooldown backoff after all-empty failures unless `bypass_failure_cooldown = true`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parchment_model::{ModelPricingSpec, TokenPricing};
use serde_json::Value;
use tokio::sync::{Mutex, watch};

pub const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models?output_modalities=all";
pub const VERCEL_MODELS_URL: &str = "https://ai-gateway.vercel.sh/v1/models";
pub const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
pub const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";

const DEFAULT_CACHE_TTL_MS: i64 = 30 * 60 * 1000;
const DEFAULT_FAILURE_COOLDOWN_MS: i64 = 60 * 1000;
const DEFAULT_MODALITY_WAIT_MS: u64 = 2_500;

#[derive(Debug, Clone)]
pub struct RegistryEndpoints {
    pub openrouter_models_url: String,
    pub vercel_models_url: String,
    pub litellm_prices_url: String,
    pub models_dev_api_url: String,
}

impl Default for RegistryEndpoints {
    fn default() -> Self {
        Self {
            openrouter_models_url: OPENROUTER_MODELS_URL.to_string(),
            vercel_models_url: VERCEL_MODELS_URL.to_string(),
            litellm_prices_url: LITELLM_PRICES_URL.to_string(),
            models_dev_api_url: MODELS_DEV_API_URL.to_string(),
        }
    }
}

struct SyncState {
    in_flight: bool,
    last_fetch_ms: Option<i64>,
    last_failure_ms: Option<i64>,
}

pub struct RegistryCatalogClient {
    http: reqwest::Client,
    endpoints: RegistryEndpoints,
    cache_ttl_ms: i64,
    failure_cooldown_ms: i64,
    modality_wait: Duration,
    catalog: Arc<RwLock<HashMap<String, ModelPricingSpec>>>,
    sync_state: Arc<Mutex<SyncState>>,
    gen_tx: Arc<watch::Sender<u64>>,
    gen_rx: watch::Receiver<u64>,
}

impl RegistryCatalogClient {
    pub fn new(http: reqwest::Client, endpoints: RegistryEndpoints) -> Self {
        Self::with_timings(
            http,
            endpoints,
            DEFAULT_CACHE_TTL_MS,
            DEFAULT_FAILURE_COOLDOWN_MS,
            Duration::from_millis(DEFAULT_MODALITY_WAIT_MS),
        )
    }

    pub fn with_timings(
        http: reqwest::Client,
        endpoints: RegistryEndpoints,
        cache_ttl_ms: i64,
        failure_cooldown_ms: i64,
        modality_wait: Duration,
    ) -> Self {
        let (gen_tx, gen_rx) = watch::channel(0u64);
        Self {
            http,
            endpoints,
            cache_ttl_ms,
            failure_cooldown_ms,
            modality_wait,
            catalog: Arc::new(RwLock::new(HashMap::new())),
            sync_state: Arc::new(Mutex::new(SyncState {
                in_flight: false,
                last_fetch_ms: None,
                last_failure_ms: None,
            })),
            gen_tx: Arc::new(gen_tx),
            gen_rx,
        }
    }

    pub fn get_cached_spec(&self, model_id: &str) -> Option<ModelPricingSpec> {
        let guard = self.catalog.read().ok()?;
        lookup_spec_in_catalog(&guard, model_id).cloned()
    }

    pub fn cached_len(&self) -> usize {
        self.catalog.read().map(|g| g.len()).unwrap_or(0)
    }

    /// Starts a detached single-flight background sync if cache is stale and not in failure cooldown.
    /// Returns `true` if a sync is now running (either newly started or already in flight).
    pub async fn ensure_sync_started(&self, bypass_failure_cooldown: bool) -> bool {
        let mut state = self.sync_state.lock().await;
        let now = now_ms();
        let has_data = self.cached_len() > 0;
        if has_data
            && state
                .last_fetch_ms
                .is_some_and(|t| now.saturating_sub(t) < self.cache_ttl_ms)
        {
            return false;
        }
        if state.in_flight {
            return true;
        }
        if !bypass_failure_cooldown
            && state
                .last_failure_ms
                .is_some_and(|t| now.saturating_sub(t) < self.failure_cooldown_ms)
        {
            return false;
        }
        state.in_flight = true;
        drop(state);

        let http = self.http.clone();
        let endpoints = self.endpoints.clone();
        let catalog = self.catalog.clone();
        let sync_state = self.sync_state.clone();
        let gen_tx = self.gen_tx.clone();

        // Detached task so a caller's bounded modality wait timeout NEVER cancels the download.
        tokio::spawn(async move {
            type CatalogIndexer = fn(&Value, &mut HashMap<String, ModelPricingSpec>);
            let mut merged: HashMap<String, ModelPricingSpec> = HashMap::new();
            let sources: [(&str, Duration, CatalogIndexer); 4] = [
                (
                    &endpoints.openrouter_models_url,
                    Duration::from_secs(15),
                    index_openrouter_catalog,
                ),
                (
                    &endpoints.vercel_models_url,
                    Duration::from_secs(12),
                    index_vercel_catalog,
                ),
                (
                    &endpoints.litellm_prices_url,
                    Duration::from_secs(18),
                    index_litellm_catalog,
                ),
                (
                    &endpoints.models_dev_api_url,
                    Duration::from_secs(15),
                    index_models_dev_catalog,
                ),
            ];

            for (url, timeout, indexer) in sources {
                if url.trim().is_empty() {
                    continue;
                }
                if let Ok(Ok(resp)) = tokio::time::timeout(timeout, http.get(url).send()).await
                    && resp.status().is_success()
                    && let Ok(Ok(json)) = tokio::time::timeout(timeout, resp.json::<Value>()).await
                {
                    indexer(&json, &mut merged);
                    if !merged.is_empty()
                        && let Ok(mut guard) = catalog.write()
                    {
                        for (k, v) in &merged {
                            guard.insert(k.clone(), v.clone());
                        }
                        let next_gen = gen_tx.borrow().wrapping_add(1);
                        let _ = gen_tx.send(next_gen);
                    }
                }
            }

            let finished_ms = now_ms();
            let mut st = sync_state.lock().await;
            st.in_flight = false;
            if !merged.is_empty() {
                st.last_fetch_ms = Some(finished_ms);
                st.last_failure_ms = None;
            } else {
                st.last_failure_ms = Some(finished_ms);
            }
            drop(st);
            let next_gen = gen_tx.borrow().wrapping_add(1);
            let _ = gen_tx.send(next_gen);
        });

        true
    }

    /// Resolves output modalities for `model_id`, waiting at most `modality_wait` (`2.5 s`)
    /// without cancelling the underlying background catalog download.
    pub async fn resolve_output_modalities(&self, model_id: &str) -> Vec<String> {
        if let Some(spec) = self.get_cached_spec(model_id)
            && !spec.output_modalities.is_empty()
        {
            return spec.output_modalities;
        }
        if !self.ensure_sync_started(false).await {
            return self
                .get_cached_spec(model_id)
                .map(|s| s.output_modalities)
                .unwrap_or_default();
        }

        let mut rx = self.gen_rx.clone();
        let _ = tokio::time::timeout(self.modality_wait, async {
            loop {
                if let Some(spec) = self.get_cached_spec(model_id)
                    && !spec.output_modalities.is_empty()
                {
                    return;
                }
                let still_running = self.sync_state.lock().await.in_flight;
                if !still_running {
                    return;
                }
                if rx.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;

        self.get_cached_spec(model_id)
            .map(|s| s.output_modalities)
            .unwrap_or_default()
    }
}

pub fn lookup_spec_in_catalog<'a>(
    catalog: &'a HashMap<String, ModelPricingSpec>,
    model_id: &str,
) -> Option<&'a ModelPricingSpec> {
    let raw = model_id.trim();
    if raw.is_empty() {
        return None;
    }
    let clean = raw
        .strip_prefix("models/")
        .unwrap_or(raw)
        .to_ascii_lowercase();
    if let Some(hit) = catalog.get(&clean) {
        return Some(hit);
    }
    if let Some((_, short)) = clean.rsplit_once('/')
        && let Some(hit) = catalog.get(short)
    {
        return Some(hit);
    }
    None
}

fn insert_spec_if_missing(dest: &mut HashMap<String, ModelPricingSpec>, spec: ModelPricingSpec) {
    let key = spec.model_id.trim().to_ascii_lowercase();
    if key.is_empty() {
        return;
    }
    let short = key.rsplit_once('/').map(|(_, s)| s.to_string());
    dest.entry(key).or_insert_with(|| spec.clone());
    if let Some(s) = short {
        dest.entry(s).or_insert(spec);
    }
}

fn parse_per_token_to_per_m(val: Option<&Value>) -> Option<f64> {
    let per_tok = match val? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (per_tok.is_finite() && per_tok >= 0.0).then_some(per_tok * 1_000_000.0)
}

fn parse_direct_per_m(val: Option<&Value>) -> Option<f64> {
    let v = match val? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (v.is_finite() && v >= 0.0).then_some(v)
}

pub fn index_openrouter_catalog(json: &Value, dest: &mut HashMap<String, ModelPricingSpec>) {
    let Some(data) = json.get("data").and_then(Value::as_array) else {
        return;
    };
    for item in data {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let pricing = item.get("pricing").and_then(Value::as_object);
        let input_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("prompt")));
        let output_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("completion")));
        let read_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("input_cache_read")));
        let write_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("input_cache_write")));
        let reasoning_m =
            parse_per_token_to_per_m(pricing.and_then(|p| p.get("internal_reasoning")));
        let known = TokenPricing {
            input_per_m: input_m,
            output_per_m: output_m,
            cache_read_per_m: read_m,
            cache_write_per_m: write_m,
            reasoning_per_m: reasoning_m,
            long_cache_write_per_m: None,
        };
        let arch = item.get("architecture").and_then(Value::as_object);
        let input_mods = string_array(arch.and_then(|a| a.get("input_modalities")));
        let output_mods = string_array(arch.and_then(|a| a.get("output_modalities")));
        let ctx = item
            .get("context_length")
            .and_then(Value::as_i64)
            .unwrap_or(128_000) as i32;
        let max_out = item
            .get("top_provider")
            .and_then(|tp| tp.get("max_completion_tokens"))
            .and_then(Value::as_i64)
            .map(|v| v as i32);

        insert_spec_if_missing(
            dest,
            ModelPricingSpec {
                model_id: id.to_string(),
                display_name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                provider_name: "openrouter".to_string(),
                input_price_per_m: input_m.unwrap_or(0.0),
                output_price_per_m: output_m.unwrap_or(0.0),
                cache_read_price_per_m: read_m.unwrap_or(0.0),
                cache_write_price_per_m: write_m.unwrap_or(0.0),
                reasoning_price_per_m: reasoning_m.unwrap_or(0.0),
                known_pricing: known.has_any_rate().then_some(known),
                context_pricing_tiers: Vec::new(),
                charge_pricing: Default::default(),
                context_window_tokens: ctx,
                max_output_tokens: max_out,
                output_modalities: output_mods,
                input_modalities: input_mods,
            },
        );
    }
}

pub fn index_vercel_catalog(json: &Value, dest: &mut HashMap<String, ModelPricingSpec>) {
    let Some(data) = json.get("data").and_then(Value::as_array) else {
        return;
    };
    for item in data {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        let pricing = item.get("pricing").and_then(Value::as_object);
        let input_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("input")));
        let output_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("output")));
        let read_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("input_cache_read")));
        let write_m = parse_per_token_to_per_m(pricing.and_then(|p| p.get("input_cache_write")));
        let known = TokenPricing {
            input_per_m: input_m,
            output_per_m: output_m,
            cache_read_per_m: read_m,
            cache_write_per_m: write_m,
            reasoning_per_m: None,
            long_cache_write_per_m: None,
        };
        insert_spec_if_missing(
            dest,
            ModelPricingSpec {
                model_id: id.to_string(),
                display_name: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                provider_name: "vercel".to_string(),
                input_price_per_m: input_m.unwrap_or(0.0),
                output_price_per_m: output_m.unwrap_or(0.0),
                cache_read_price_per_m: read_m.unwrap_or(0.0),
                cache_write_price_per_m: write_m.unwrap_or(0.0),
                reasoning_price_per_m: 0.0,
                known_pricing: known.has_any_rate().then_some(known),
                context_pricing_tiers: Vec::new(),
                charge_pricing: Default::default(),
                context_window_tokens: item
                    .get("context_window")
                    .and_then(Value::as_i64)
                    .unwrap_or(128_000) as i32,
                max_output_tokens: item
                    .get("max_tokens")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32),
                output_modalities: Vec::new(),
                input_modalities: Vec::new(),
            },
        );
    }
}

pub fn index_litellm_catalog(json: &Value, dest: &mut HashMap<String, ModelPricingSpec>) {
    let Some(map) = json.as_object() else {
        return;
    };
    for (id, entry) in map {
        if id == "sample_spec" {
            continue;
        }
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let input_m = parse_per_token_to_per_m(obj.get("input_cost_per_token"));
        let output_m = parse_per_token_to_per_m(obj.get("output_cost_per_token"));
        let read_m = parse_per_token_to_per_m(obj.get("cache_read_input_token_cost"));
        let write_m = parse_per_token_to_per_m(obj.get("cache_creation_input_token_cost"));
        let known = TokenPricing {
            input_per_m: input_m,
            output_per_m: output_m,
            cache_read_per_m: read_m,
            cache_write_per_m: write_m,
            reasoning_per_m: None,
            long_cache_write_per_m: None,
        };
        insert_spec_if_missing(
            dest,
            ModelPricingSpec {
                model_id: id.clone(),
                display_name: String::new(),
                provider_name: "litellm".to_string(),
                input_price_per_m: input_m.unwrap_or(0.0),
                output_price_per_m: output_m.unwrap_or(0.0),
                cache_read_price_per_m: read_m.unwrap_or(0.0),
                cache_write_price_per_m: write_m.unwrap_or(0.0),
                reasoning_price_per_m: 0.0,
                known_pricing: known.has_any_rate().then_some(known),
                context_pricing_tiers: Vec::new(),
                charge_pricing: Default::default(),
                context_window_tokens: obj
                    .get("max_input_tokens")
                    .or_else(|| obj.get("max_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(128_000) as i32,
                max_output_tokens: obj
                    .get("max_output_tokens")
                    .and_then(Value::as_i64)
                    .map(|v| v as i32),
                output_modalities: Vec::new(),
                input_modalities: Vec::new(),
            },
        );
    }
}

/// Gap-filler: `models.dev/api.json` (`{ "<provider>": { "models": { "<id>": { cost, limit, modalities } } } }`).
pub fn index_models_dev_catalog(json: &Value, dest: &mut HashMap<String, ModelPricingSpec>) {
    let Some(providers) = json.as_object() else {
        return;
    };
    for (prov_name, prov_val) in providers {
        let Some(models) = prov_val.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model_id, m_val) in models {
            let cost = m_val.get("cost").and_then(Value::as_object);
            let input_m = parse_direct_per_m(cost.and_then(|c| c.get("input")));
            let output_m = parse_direct_per_m(cost.and_then(|c| c.get("output")));
            let read_m = parse_direct_per_m(cost.and_then(|c| c.get("cache_read")));
            let write_m = parse_direct_per_m(cost.and_then(|c| c.get("cache_write")));
            let reasoning_m = parse_direct_per_m(cost.and_then(|c| c.get("reasoning")));
            let known = TokenPricing {
                input_per_m: input_m,
                output_per_m: output_m,
                cache_read_per_m: read_m,
                cache_write_per_m: write_m,
                reasoning_per_m: reasoning_m,
                long_cache_write_per_m: None,
            };
            let limit = m_val.get("limit").and_then(Value::as_object);
            let mods = m_val.get("modalities").and_then(Value::as_object);
            insert_spec_if_missing(
                dest,
                ModelPricingSpec {
                    model_id: model_id.clone(),
                    display_name: m_val
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    provider_name: prov_name.clone(),
                    input_price_per_m: input_m.unwrap_or(0.0),
                    output_price_per_m: output_m.unwrap_or(0.0),
                    cache_read_price_per_m: read_m.unwrap_or(0.0),
                    cache_write_price_per_m: write_m.unwrap_or(0.0),
                    reasoning_price_per_m: reasoning_m.unwrap_or(0.0),
                    known_pricing: known.has_any_rate().then_some(known),
                    context_pricing_tiers: Vec::new(),
                    charge_pricing: Default::default(),
                    context_window_tokens: limit
                        .and_then(|l| l.get("context"))
                        .and_then(Value::as_i64)
                        .unwrap_or(128_000) as i32,
                    max_output_tokens: limit
                        .and_then(|l| l.get("output"))
                        .and_then(Value::as_i64)
                        .map(|v| v as i32),
                    output_modalities: string_array(mods.and_then(|m| m.get("output"))),
                    input_modalities: string_array(mods.and_then(|m| m.get("input"))),
                },
            );
        }
    }
}

fn string_array(val: Option<&Value>) -> Vec<String> {
    val.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
