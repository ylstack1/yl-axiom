//! Multi-dialect token usage normalization, provider charge extraction, TTL cache-write costing,
//! and cumulative per-request spend ledger (`ProviderUsageLedger`).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use parchment_model::{
    ApiUsageRecord, ChargePricing, ContextPricingTier, ModelPricingSpec, ProviderChargeUsage,
    ProviderProfile, ProviderTokenUsage, TokenMetrics, TokenPricing, legacy_token_pricing,
    saturating_token_sum,
};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsageDialect {
    OpenAiResponses,
    OpenAiChat,
    Anthropic,
    GeminiInteractions,
    GeminiContent,
    ImageGeneration,
    VideoGeneration,
}

impl UsageDialect {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "openai_responses",
            Self::OpenAiChat => "openai_chat",
            Self::Anthropic => "anthropic_messages",
            Self::GeminiInteractions => "gemini_interactions",
            Self::GeminiContent => "gemini_generate_content",
            Self::ImageGeneration => "image_generation",
            Self::VideoGeneration => "video_generation",
        }
    }
}

pub fn uses_prompt_completion_usage_fields(
    dialect: UsageDialect,
    usage: Option<&Map<String, Value>>,
) -> bool {
    if dialect == UsageDialect::OpenAiChat {
        return true;
    }
    if matches!(
        dialect,
        UsageDialect::ImageGeneration | UsageDialect::VideoGeneration
    ) && let Some(u) = usage
    {
        return !u.contains_key("input_tokens")
            && !u.contains_key("output_tokens")
            && !u.contains_key("input_tokens_details")
            && !u.contains_key("output_tokens_details")
            && (u.contains_key("prompt_tokens")
                || u.contains_key("completion_tokens")
                || u.contains_key("prompt_tokens_details")
                || u.contains_key("completion_tokens_details"));
    }
    false
}

fn has_absent_or_null(source: Option<&Map<String, Value>>, name: &str) -> bool {
    match source.and_then(|s| s.get(name)) {
        None | Some(Value::Null) => true,
        Some(_) => false,
    }
}

fn read_non_neg_i64(
    source: Option<&Map<String, Value>>,
    name: &str,
    invalid: &mut bool,
) -> Option<i64> {
    let raw = source.and_then(|s| s.get(name))?;
    if raw.is_null() {
        return None;
    }
    match raw.as_i64() {
        Some(v) if v >= 0 => Some(v),
        _ => {
            *invalid = true;
            None
        }
    }
}

fn checked_sum(values: &[i64], invalid: &mut bool) -> i64 {
    let mut total = 0i64;
    for &v in values {
        if v > i64::MAX - total {
            *invalid = true;
            return i64::MAX;
        }
        total += v;
    }
    total
}

/// Normalizes provider usage JSON across all 5 chat dialects plus image/video generation.
pub fn parse_provider_usage(
    dialect: UsageDialect,
    obj: Option<&Map<String, Value>>,
    is_final: bool,
    response: Option<&Map<String, Value>>,
) -> ProviderTokenUsage {
    let mut invalid = false;
    let mut combined = matches!(
        dialect,
        UsageDialect::OpenAiResponses | UsageDialect::OpenAiChat | UsageDialect::Anthropic
    );
    let charges = parse_provider_charges(dialect, obj, response);
    let Some(obj) = obj else {
        return ProviderTokenUsage {
            output_includes_reasoning: combined,
            is_final,
            charges,
            ..ProviderTokenUsage::default()
        };
    };

    let input: Option<i64>;
    let cached: Option<i64>;
    let mut created: Option<i64>;
    let mut short: Option<i64> = None;
    let mut long: Option<i64> = None;
    let output: Option<i64>;
    let thinking: Option<i64>;
    let mut uncached_input: Option<i64> = None;

    match dialect {
        UsageDialect::OpenAiResponses
        | UsageDialect::OpenAiChat
        | UsageDialect::ImageGeneration
        | UsageDialect::VideoGeneration => {
            let is_chat = dialect == UsageDialect::OpenAiChat;
            let is_media = matches!(
                dialect,
                UsageDialect::ImageGeneration | UsageDialect::VideoGeneration
            );
            let uses_pc = uses_prompt_completion_usage_fields(dialect, Some(obj));
            let in_details = obj
                .get(if uses_pc {
                    "prompt_tokens_details"
                } else {
                    "input_tokens_details"
                })
                .and_then(Value::as_object);
            let out_details = obj
                .get(if uses_pc {
                    "completion_tokens_details"
                } else {
                    "output_tokens_details"
                })
                .and_then(Value::as_object);
            let raw_in = read_non_neg_i64(
                Some(obj),
                if uses_pc {
                    "prompt_tokens"
                } else {
                    "input_tokens"
                },
                &mut invalid,
            );
            output = read_non_neg_i64(
                Some(obj),
                if uses_pc {
                    "completion_tokens"
                } else {
                    "output_tokens"
                },
                &mut invalid,
            );
            let raw_total = if is_media && raw_in.is_none() {
                read_non_neg_i64(Some(obj), "total_tokens", &mut invalid)
            } else {
                None
            };
            input = raw_in.or_else(|| {
                if is_media
                    && output.is_some()
                    && !invalid
                    && has_absent_or_null(Some(obj), "input_tokens")
                    && has_absent_or_null(Some(obj), "prompt_tokens")
                    && (raw_total.is_none() || raw_total == output)
                {
                    Some(0)
                } else {
                    None
                }
            });
            let raw_cached = read_non_neg_i64(in_details, "cached_tokens", &mut invalid);
            let has_media_in_details = in_details.is_some_and(|d| {
                d.contains_key("text_tokens")
                    || d.contains_key("image_tokens")
                    || d.contains_key("audio_tokens")
                    || d.contains_key("video_tokens")
            });
            cached = raw_cached.or_else(|| {
                if input.is_some()
                    && has_absent_or_null(in_details, "cached_tokens")
                    && (is_media || has_media_in_details)
                {
                    Some(0)
                } else {
                    None
                }
            });
            thinking = read_non_neg_i64(out_details, "reasoning_tokens", &mut invalid);
            if is_media && thinking.is_some() {
                combined = true;
            }
            if !is_chat {
                created = read_non_neg_i64(in_details, "cache_write_tokens", &mut invalid);
                let ttl = in_details
                    .and_then(|d| d.get("cache_write_tokens_details"))
                    .and_then(Value::as_object);
                short = read_non_neg_i64(ttl, "ttl_30m_tokens", &mut invalid);
                long = read_non_neg_i64(ttl, "ttl_12h_tokens", &mut invalid);
                if ttl.is_some() && (short.is_some() || long.is_some() || created == Some(0)) {
                    if short.is_none() && has_absent_or_null(ttl, "ttl_30m_tokens") {
                        short = Some(0);
                    }
                    if long.is_none() && has_absent_or_null(ttl, "ttl_12h_tokens") {
                        long = Some(0);
                    }
                }
                if ttl.is_none()
                    && created.is_none()
                    && input.is_some()
                    && has_absent_or_null(in_details, "cache_write_tokens")
                    && has_absent_or_null(in_details, "cache_write_tokens_details")
                {
                    created = Some(0);
                }
            } else {
                created = Some(0);
            }
        }
        UsageDialect::Anthropic => {
            let uncached = read_non_neg_i64(Some(obj), "input_tokens", &mut invalid);
            uncached_input = uncached;
            cached = read_non_neg_i64(Some(obj), "cache_read_input_tokens", &mut invalid);
            created = read_non_neg_i64(Some(obj), "cache_creation_input_tokens", &mut invalid);
            let ttl = obj.get("cache_creation").and_then(Value::as_object);
            short = read_non_neg_i64(ttl, "ephemeral_5m_input_tokens", &mut invalid);
            long = read_non_neg_i64(ttl, "ephemeral_1h_input_tokens", &mut invalid);
            if ttl.is_some() && (short.is_some() || long.is_some() || created == Some(0)) {
                if short.is_none() && has_absent_or_null(ttl, "ephemeral_5m_input_tokens") {
                    short = Some(0);
                }
                if long.is_none() && has_absent_or_null(ttl, "ephemeral_1h_input_tokens") {
                    long = Some(0);
                }
            }
            if created.is_none()
                && let (Some(s), Some(l)) = (short, long)
            {
                created = Some(checked_sum(&[s, l], &mut invalid));
            }
            input = uncached.map(|u| {
                checked_sum(
                    &[u, cached.unwrap_or(0), created.unwrap_or(0)],
                    &mut invalid,
                )
            });
            output = read_non_neg_i64(Some(obj), "output_tokens", &mut invalid);
            thinking = None;
        }
        UsageDialect::GeminiInteractions => {
            input = read_non_neg_i64(Some(obj), "total_input_tokens", &mut invalid);
            let raw_cached = read_non_neg_i64(Some(obj), "total_cached_tokens", &mut invalid);
            cached = raw_cached.or_else(|| {
                if input.is_some() && has_absent_or_null(Some(obj), "total_cached_tokens") {
                    Some(0)
                } else {
                    None
                }
            });
            let raw_out = read_non_neg_i64(Some(obj), "total_output_tokens", &mut invalid);
            output = raw_out.or_else(|| {
                if is_final
                    && input.is_some()
                    && has_absent_or_null(Some(obj), "total_output_tokens")
                {
                    Some(0)
                } else {
                    None
                }
            });
            thinking = read_non_neg_i64(Some(obj), "total_thought_tokens", &mut invalid)
                .or_else(|| read_non_neg_i64(Some(obj), "total_reasoning_tokens", &mut invalid))
                .or_else(|| {
                    if (is_final || raw_out.is_some())
                        && input.is_some()
                        && has_absent_or_null(Some(obj), "total_thought_tokens")
                        && has_absent_or_null(Some(obj), "total_reasoning_tokens")
                    {
                        Some(0)
                    } else {
                        None
                    }
                });
            created = Some(0);
        }
        UsageDialect::GeminiContent => {
            input = read_non_neg_i64(Some(obj), "promptTokenCount", &mut invalid);
            let raw_cached = read_non_neg_i64(Some(obj), "cachedContentTokenCount", &mut invalid);
            cached = raw_cached.or_else(|| {
                if input.is_some() && has_absent_or_null(Some(obj), "cachedContentTokenCount") {
                    Some(0)
                } else {
                    None
                }
            });
            let raw_out = read_non_neg_i64(Some(obj), "candidatesTokenCount", &mut invalid);
            output = raw_out.or_else(|| {
                if is_final
                    && input.is_some()
                    && has_absent_or_null(Some(obj), "candidatesTokenCount")
                {
                    Some(0)
                } else {
                    None
                }
            });
            thinking =
                read_non_neg_i64(Some(obj), "thoughtsTokenCount", &mut invalid).or_else(|| {
                    if (is_final || raw_out.is_some())
                        && input.is_some()
                        && has_absent_or_null(Some(obj), "thoughtsTokenCount")
                    {
                        Some(0)
                    } else {
                        None
                    }
                });
            created = Some(0);
        }
    }

    if created.is_none()
        && let (Some(s), Some(l)) = (short, long)
    {
        created = Some(checked_sum(&[s, l], &mut invalid));
    }
    if let (Some(c), Some(s), Some(l)) = (created, short, long)
        && checked_sum(&[s, l], &mut invalid) != c
    {
        invalid = true;
    }
    if let Some(inp) = input {
        let c = cached.unwrap_or(0);
        if c > inp {
            invalid = true;
        }
        if created.unwrap_or(0) > inp - c.min(inp) {
            invalid = true;
        }
    }
    if combined
        && let (Some(t), Some(o)) = (thinking, output)
        && t > o
    {
        invalid = true;
    }

    ProviderTokenUsage {
        input_tokens: input,
        cached_input_tokens: cached,
        cache_creation_input_tokens: created,
        short_cache_creation_input_tokens: short,
        long_cache_creation_input_tokens: long,
        reasoning_tokens: thinking,
        output_tokens: output,
        output_includes_reasoning: combined,
        invalid_counts: invalid,
        uncached_input_tokens: uncached_input,
        is_final,
        charges,
    }
}

fn add_charge_count(
    units: &mut BTreeMap<String, i64>,
    invalid: &mut bool,
    source: Option<&Map<String, Value>>,
    field: &str,
    key: &str,
) {
    let Some(raw) = source.and_then(|s| s.get(field)) else {
        return;
    };
    if raw.is_null() {
        return;
    }
    match raw.as_i64() {
        Some(v) if v >= 0 => {
            units.insert(key.to_string(), v);
        }
        _ => *invalid = true,
    }
}

pub fn parse_provider_charges(
    dialect: UsageDialect,
    usage: Option<&Map<String, Value>>,
    response: Option<&Map<String, Value>>,
) -> ProviderChargeUsage {
    let mut units = BTreeMap::new();
    let mut observed = BTreeSet::new();
    let mut invalid = false;

    let raw_cost = if matches!(
        dialect,
        UsageDialect::OpenAiChat
            | UsageDialect::OpenAiResponses
            | UsageDialect::ImageGeneration
            | UsageDialect::VideoGeneration
    ) {
        usage.and_then(|u| u.get("cost")).filter(|v| !v.is_null())
    } else {
        None
    };
    let reported_total_usd = raw_cost
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0);
    if raw_cost.is_some() && reported_total_usd.is_none() {
        invalid = true;
    }

    if usage
        .and_then(|u| u.get("is_byok"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        observed.insert("external provider billing".to_string());
    }

    if let Some(raw_server) = usage
        .and_then(|u| u.get("server_tool_use"))
        .filter(|v| !v.is_null())
    {
        if let Some(server) = raw_server.as_object() {
            add_charge_count(
                &mut units,
                &mut invalid,
                Some(server),
                "web_search_requests",
                "web_search",
            );
            for (k, v) in server {
                if k != "web_search_requests" && !v.is_null() && v.as_i64() != Some(0) {
                    observed.insert("other server tools".to_string());
                }
            }
        } else {
            invalid = true;
        }
    }

    if matches!(
        dialect,
        UsageDialect::OpenAiChat
            | UsageDialect::OpenAiResponses
            | UsageDialect::ImageGeneration
            | UsageDialect::VideoGeneration
    ) {
        let uses_pc = uses_prompt_completion_usage_fields(dialect, usage);
        let in_d = usage
            .and_then(|u| {
                u.get(if uses_pc {
                    "prompt_tokens_details"
                } else {
                    "input_tokens_details"
                })
            })
            .and_then(Value::as_object);
        let out_d = usage
            .and_then(|u| {
                u.get(if uses_pc {
                    "completion_tokens_details"
                } else {
                    "output_tokens_details"
                })
            })
            .and_then(Value::as_object);
        add_charge_count(
            &mut units,
            &mut invalid,
            in_d,
            "audio_tokens",
            "input_audio",
        );
        add_charge_count(
            &mut units,
            &mut invalid,
            in_d,
            "video_tokens",
            "input_video",
        );
        add_charge_count(
            &mut units,
            &mut invalid,
            out_d,
            "audio_tokens",
            "output_audio",
        );
        add_charge_count(
            &mut units,
            &mut invalid,
            out_d,
            "image_tokens",
            "output_image",
        );
    }

    if dialect == UsageDialect::GeminiContent
        && let Some(u) = usage
    {
        for (field, prefix) in [
            ("promptTokensDetails", "input"),
            ("cacheTokensDetails", "cached_input"),
            ("candidatesTokensDetails", "output"),
        ] {
            if let Some(arr) = u.get(field).and_then(Value::as_array) {
                for el in arr {
                    let Some(item) = el.as_object() else {
                        invalid = true;
                        continue;
                    };
                    let modality = item
                        .get("modality")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if modality == "text" || modality.is_empty() {
                        continue;
                    }
                    let key = format!("{prefix}_{modality}");
                    let prev = units.contains_key(&key);
                    add_charge_count(&mut units, &mut invalid, Some(item), "tokenCount", &key);
                    if prev {
                        invalid = true;
                    }
                }
            }
        }
    }

    if let Some(resp) = response {
        let mut inspect_item = |el: &Value| {
            let Some(obj) = el.as_object() else {
                return;
            };
            match obj.get("type").and_then(Value::as_str) {
                Some(
                    "web_search_call"
                    | "google_search_result"
                    | "web_search_result"
                    | "web_search_tool_result",
                ) => {
                    observed.insert("web_search".to_string());
                }
                Some("image_generation_call" | "image") => {
                    observed.insert("output_image".to_string());
                }
                Some("code_interpreter_call") => {
                    observed.insert("code execution".to_string());
                }
                Some("code_execution_result")
                    if !matches!(
                        dialect,
                        UsageDialect::GeminiContent | UsageDialect::GeminiInteractions
                    ) =>
                {
                    observed.insert("code execution".to_string());
                }
                Some("server_tool_use") => {
                    let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
                    if name == "web_search" {
                        observed.insert("web_search".to_string());
                    } else {
                        observed.insert("other server tools".to_string());
                    }
                }
                _ => {}
            }
        };

        for field in ["output", "outputs", "steps", "content"] {
            if let Some(arr) = resp.get(field).and_then(Value::as_array) {
                for el in arr {
                    inspect_item(el);
                }
            }
        }
        for field in ["step", "delta", "content_block", "item"] {
            if let Some(el) = resp.get(field) {
                inspect_item(el);
            }
        }
        if let Some(cands) = resp.get("candidates").and_then(Value::as_array) {
            for cand in cands {
                if let Some(g) = cand.get("groundingMetadata").and_then(Value::as_object) {
                    let has_q = g
                        .get("webSearchQueries")
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty());
                    let has_c = g
                        .get("groundingChunks")
                        .and_then(Value::as_array)
                        .is_some_and(|a| !a.is_empty());
                    if has_q || has_c {
                        observed.insert("web_search".to_string());
                    }
                }
            }
        }
    }

    ProviderChargeUsage {
        reported_total_usd,
        units,
        observed_components: observed,
        invalid,
    }
}

pub fn cache_creation_cost_units(record: &ApiUsageRecord) -> f64 {
    let u = &record.usage;
    let total = u.cache_creation_input_tokens.unwrap_or(0).max(0);
    let short = u
        .short_cache_creation_input_tokens
        .unwrap_or(0)
        .clamp(0, total);
    let long = u
        .long_cache_creation_input_tokens
        .unwrap_or(0)
        .clamp(0, total - short);
    let mult = match record.wire_dialect.as_str() {
        "openai_responses" => 2.5 / 1.25,
        "anthropic_messages" => 2.0 / 1.25,
        _ => 1.0,
    };
    (total - long) as f64 + (long as f64) * mult
}

pub fn usage_record_metrics(record: &ApiUsageRecord) -> TokenMetrics {
    TokenMetrics {
        input_tokens: record.usage.input_tokens.unwrap_or(0),
        cached_input_tokens: record.usage.cached_input_tokens.unwrap_or(0),
        cache_creation_input_tokens: record.usage.cache_creation_input_tokens.unwrap_or(0),
        cache_creation_cost_units: cache_creation_cost_units(record),
        output_tokens: record.usage.non_reasoning_output_tokens(),
        reasoning_tokens: record.usage.separate_reasoning_tokens(),
        output_includes_unreported_reasoning: record.usage.output_includes_reasoning
            && record.usage.reasoning_tokens.is_none(),
        ..TokenMetrics::default()
    }
}

pub fn record_pricing(record: &ApiUsageRecord) -> TokenPricing {
    let Some(input) = record.usage.input_tokens else {
        return record.pricing.clone();
    };
    let tier = record
        .context_pricing_tiers
        .iter()
        .filter(|t| t.input_tokens_above >= 0 && input > t.input_tokens_above)
        .max_by_key(|t| t.input_tokens_above);
    match tier {
        Some(t) => t.pricing.with_missing_from(&record.pricing),
        None => record.pricing.clone(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestCostEstimate {
    pub usd: f64,
    pub unavailable: BTreeSet<String>,
    pub reported: bool,
}

/// Reported gateway totals cover tokens and fees; never add a second estimate on top of them.
pub fn estimate_request_cost(record: &ApiUsageRecord) -> RequestCostEstimate {
    let charges = &record.usage.charges;
    if let Some(total) = charges.reported_total_usd
        && total.is_finite()
        && total >= 0.0
        && !charges.invalid
    {
        let unavailable: BTreeSet<String> = charges
            .observed_components
            .iter()
            .filter(|c| c.as_str() == "external provider billing")
            .cloned()
            .collect();
        return RequestCostEstimate {
            usd: total,
            unavailable,
            reported: true,
        };
    }

    let mut missing = BTreeSet::new();
    if charges.invalid {
        missing.insert("invalid fee usage".to_string());
    }
    let usage = &record.usage;
    let mut part = usage_record_metrics(record);
    let mut priced_units = charges.units.clone();

    let label = |key: &str| -> String {
        match key {
            "input_audio" => "audio input".to_string(),
            "input_image_count" => "image input".to_string(),
            "cached_input_audio" => "cached audio input".to_string(),
            "output_audio" => "audio output".to_string(),
            "output_image" | "output_image_count" => "image output".to_string(),
            "web_search" => "native web search".to_string(),
            other => other.replace('_', " "),
        }
    };

    let audio_in = charges.units.get("input_audio").copied().unwrap_or(0);
    let audio_cached = charges.units.get("cached_input_audio").copied();
    let audio_out = charges.units.get("output_audio").copied().unwrap_or(0);
    let image_out = charges.units.get("output_image").copied().unwrap_or(0);
    if charges.units.values().any(|&v| v < 0)
        || audio_in > part.input_tokens
        || audio_cached.is_some_and(|ac| ac > audio_in)
        || audio_out > part.output_tokens
        || image_out > part.output_tokens - audio_out.min(part.output_tokens)
    {
        return RequestCostEstimate {
            usd: 0.0,
            unavailable: BTreeSet::from(["invalid media usage".to_string()]),
            reported: false,
        };
    }

    let price_usage_known = usage.is_complete()
        && (part.input_tokens == 0
            || (usage.cached_input_tokens.is_some()
                && usage.cache_creation_input_tokens.is_some()));
    let ttl_known = part.cache_creation_input_tokens == 0
        || (usage.short_cache_creation_input_tokens.is_some()
            && usage.long_cache_creation_input_tokens.is_some()
            && saturating_token_sum(&[
                usage.short_cache_creation_input_tokens.unwrap_or(0),
                usage.long_cache_creation_input_tokens.unwrap_or(0),
            ]) == part.cache_creation_input_tokens);

    if audio_in > 0 {
        if part.cache_creation_input_tokens > 0
            || (part.cached_input_tokens > 0 && audio_cached.is_none())
            || audio_cached.is_some_and(|ac| {
                ac > part.cached_input_tokens
                    || audio_in - ac > part.input_tokens - part.cached_input_tokens
            })
        {
            missing.insert("audio input/cache split".to_string());
            part.input_tokens = 0;
            part.cached_input_tokens = 0;
            part.cache_creation_input_tokens = 0;
            part.cache_creation_cost_units = 0.0;
            priced_units.remove("input_audio");
            priced_units.remove("cached_input_audio");
        } else {
            let ac = audio_cached.unwrap_or(0);
            part.input_tokens -= audio_in;
            part.cached_input_tokens -= ac;
            priced_units.insert("input_audio".to_string(), audio_in - ac);
        }
    }
    part.output_tokens -= audio_out + image_out;

    let token_cost = if price_usage_known && ttl_known {
        record_pricing(record).estimate(
            &part,
            usage.short_cache_creation_input_tokens,
            usage.long_cache_creation_input_tokens,
        )
    } else {
        None
    };
    if token_cost.is_none() {
        missing.insert("token usage/prices".to_string());
    }

    if record.charge_pricing.per_unit_usd.contains_key("request") {
        if usage.is_final && usage.is_complete() {
            priced_units.insert("request".to_string(), 1);
        } else {
            missing.insert("request fee".to_string());
        }
    }

    let mut total = token_cost.unwrap_or(0.0);
    for (key, count) in &priced_units {
        if *count == 0 {
            continue;
        }
        let cost = record
            .charge_pricing
            .per_unit_usd
            .get(key)
            .copied()
            .filter(|r| r.is_finite() && *r >= 0.0)
            .map(|r| r * (*count as f64))
            .filter(|c| c.is_finite());
        match cost {
            Some(c) => total += c,
            None => {
                missing.insert(label(key));
            }
        }
    }

    for key in &charges.observed_components {
        let covered_by_per_image = key == "output_image"
            && !charges.units.contains_key("output_image")
            && charges.units.contains_key("output_image_count")
            && part.output_tokens == 0
            && record
                .charge_pricing
                .per_unit_usd
                .contains_key("output_image_count");
        if covered_by_per_image {
            continue;
        }
        if !charges.units.contains_key(key)
            || (key == "output_image" && !record.charge_pricing.per_unit_usd.contains_key(key))
        {
            missing.insert(label(key));
        }
    }

    if !total.is_finite() {
        missing.insert("cost overflow".to_string());
        return RequestCostEstimate {
            usd: 0.0,
            unavailable: missing,
            reported: false,
        };
    }
    RequestCostEstimate {
        usd: total,
        unavailable: missing,
        reported: false,
    }
}

/// Computes lower-bound spend toward a chat spend cap (never stops a run on an overestimate).
pub fn spend_toward_limit(records: &[ApiUsageRecord]) -> f64 {
    let total: f64 = records
        .iter()
        .map(|rec| {
            let split_missing = rec.usage.is_complete()
                && (rec.usage.cached_input_tokens.is_none()
                    || rec.usage.cache_creation_input_tokens.is_none());
            let est = estimate_request_cost(rec);
            if !split_missing || est.reported {
                est.usd
            } else {
                let mut conservative = rec.clone();
                conservative.usage.cached_input_tokens = conservative
                    .usage
                    .cached_input_tokens
                    .or(conservative.usage.input_tokens);
                conservative.usage.cache_creation_input_tokens =
                    conservative.usage.cache_creation_input_tokens.or(Some(0));
                estimate_request_cost(&conservative).usd
            }
        })
        .sum();
    if total.is_finite() && total >= 0.0 {
        total
    } else {
        0.0
    }
}

pub fn resolve_request_pricing(
    profile: &ProviderProfile,
    model: &str,
    catalog_spec: Option<&ModelPricingSpec>,
) -> (TokenPricing, Vec<ContextPricingTier>, ChargePricing) {
    let explicit = profile.custom_model_specs.get(model);
    let provider_override = profile.known_pricing.is_some()
        && (profile.pricing_model_id.trim().is_empty() || profile.pricing_model_id == model);
    let base = profile.token_pricing(model);
    let pricing = if explicit.and_then(|e| e.known_pricing.as_ref()).is_some() || provider_override
    {
        base
    } else {
        let known = catalog_spec
            .and_then(|c| c.known_pricing.clone())
            .or_else(|| {
                catalog_spec.map(|c| {
                    legacy_token_pricing(
                        c.input_price_per_m,
                        c.output_price_per_m,
                        c.cache_read_price_per_m,
                        c.cache_write_price_per_m,
                        c.reasoning_price_per_m,
                    )
                })
            })
            .unwrap_or_default();
        base.with_missing_from(&known)
    };
    let tiers = if explicit.and_then(|e| e.known_pricing.as_ref()).is_some()
        || explicit.is_some_and(|e| !e.context_pricing_tiers.is_empty())
    {
        explicit
            .map(|e| e.context_pricing_tiers.clone())
            .unwrap_or_default()
    } else if provider_override {
        Vec::new()
    } else {
        catalog_spec
            .map(|c| c.context_pricing_tiers.clone())
            .unwrap_or_default()
    };
    let charges = explicit
        .filter(|e| !e.charge_pricing.per_unit_usd.is_empty())
        .map(|e| e.charge_pricing.clone())
        .or_else(|| catalog_spec.map(|c| c.charge_pricing.clone()))
        .unwrap_or_default();
    (pricing, tiers, charges)
}

pub struct ProviderUsageLedger {
    profile: ProviderProfile,
    model: String,
    owner_session_id: String,
    owner_run_id: String,
    purpose: String,
    catalog_spec: Option<ModelPricingSpec>,
    records: BTreeMap<String, ApiUsageRecord>,
    prefix: String,
    seq: u64,
}

impl ProviderUsageLedger {
    pub fn new(
        profile: ProviderProfile,
        model: String,
        owner_session_id: String,
        owner_run_id: String,
        purpose: String,
        catalog_spec: Option<ModelPricingSpec>,
    ) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            profile,
            model,
            owner_session_id,
            owner_run_id,
            purpose,
            catalog_spec,
            records: BTreeMap::new(),
            prefix: format!("usage-{nanos:x}"),
            seq: 0,
        }
    }

    pub fn begin(&mut self, dialect: UsageDialect) -> String {
        self.seq += 1;
        let req_id = format!("{}-{}", self.prefix, self.seq);
        let (pricing, tiers, charge_pricing) =
            resolve_request_pricing(&self.profile, &self.model, self.catalog_spec.as_ref());
        let record = ApiUsageRecord {
            request_id: req_id.clone(),
            response_id: String::new(),
            wire_dialect: dialect.wire_name().to_string(),
            profile_id: self.profile.id.clone(),
            model: self.model.clone(),
            pricing,
            usage: parse_provider_usage(dialect, None, false, None),
            owner_session_id: self.owner_session_id.clone(),
            owner_run_id: self.owner_run_id.clone(),
            purpose: self.purpose.clone(),
            context_pricing_tiers: tiers,
            charge_pricing,
        };
        self.records.insert(req_id.clone(), record);
        req_id
    }

    pub fn update(
        &mut self,
        request_id: &str,
        response_id: &str,
        usage: &ProviderTokenUsage,
    ) -> Option<&ApiUsageRecord> {
        let rec = self.records.get_mut(request_id)?;
        if !response_id.is_empty() {
            rec.response_id = response_id.to_string();
        }
        rec.usage = rec.usage.updated_by(usage);
        Some(rec)
    }

    pub fn records(&self) -> Vec<ApiUsageRecord> {
        self.records.values().cloned().collect()
    }

    pub fn aggregate_metrics(&self, ttft_ms: i64, duration_ms: i64) -> TokenMetrics {
        let mut agg = TokenMetrics {
            ttft_ms,
            duration_ms,
            usage_is_complete: !self.records.is_empty(),
            cost_is_available: !self.records.is_empty(),
            ..TokenMetrics::default()
        };
        for rec in self.records.values() {
            if !rec.usage.is_complete() {
                agg.usage_is_complete = false;
            }
            let part = usage_record_metrics(rec);
            agg.input_tokens = saturating_token_sum(&[agg.input_tokens, part.input_tokens]);
            agg.cached_input_tokens =
                saturating_token_sum(&[agg.cached_input_tokens, part.cached_input_tokens]);
            agg.cache_creation_input_tokens = saturating_token_sum(&[
                agg.cache_creation_input_tokens,
                part.cache_creation_input_tokens,
            ]);
            agg.reasoning_tokens =
                saturating_token_sum(&[agg.reasoning_tokens, part.reasoning_tokens]);
            agg.output_tokens = saturating_token_sum(&[agg.output_tokens, part.output_tokens]);
            agg.cache_creation_cost_units += part.cache_creation_cost_units;
            agg.output_includes_unreported_reasoning |= part.output_includes_unreported_reasoning;
            agg.reported_requests += 1;

            let est = estimate_request_cost(rec);
            agg.estimated_cost_usd += est.usd;
            if est.reported {
                agg.reported_cost_requests += 1;
            }
            if !est.unavailable.is_empty() {
                agg.cost_is_available = false;
                agg.unpriced_cost_components.extend(est.unavailable);
            }
        }
        if duration_ms > 0 {
            let generated = saturating_token_sum(&[agg.reasoning_tokens, agg.output_tokens]);
            agg.tokens_per_sec = generated as f64 / (duration_ms as f64 / 1000.0);
        }
        agg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_anthropic_ttl_and_openrouter_reported_cost() {
        let anth_json: Value = serde_json::json!({
            "input_tokens": 1000,
            "cache_read_input_tokens": 2000,
            "cache_creation_input_tokens": 500,
            "cache_creation": { "ephemeral_5m_input_tokens": 300, "ephemeral_1h_input_tokens": 200 },
            "output_tokens": 400
        });
        let u = parse_provider_usage(UsageDialect::Anthropic, anth_json.as_object(), true, None);
        assert!(u.is_complete());
        assert_eq!(u.input_tokens, Some(3500));
        assert_eq!(u.short_cache_creation_input_tokens, Some(300));
        assert_eq!(u.long_cache_creation_input_tokens, Some(200));

        let or_json: Value = serde_json::json!({
            "prompt_tokens": 500,
            "completion_tokens": 200,
            "cost": 0.0042,
            "is_byok": true
        });
        let u2 = parse_provider_usage(UsageDialect::OpenAiChat, or_json.as_object(), true, None);
        assert_eq!(u2.charges.reported_total_usd, Some(0.0042));
        assert!(
            u2.charges
                .observed_components
                .contains("external provider billing")
        );
    }
}
