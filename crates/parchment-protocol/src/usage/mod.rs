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

mod ledger;

pub use ledger::*;

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
