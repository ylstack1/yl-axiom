use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Saturating sum for non-negative token counts; negative inputs clamp to zero.
pub fn saturating_token_sum(counts: &[i64]) -> i64 {
    counts.iter().fold(0i64, |total, &count| {
        let pos = count.max(0);
        total.saturating_add(pos)
    })
}

pub fn legacy_token_pricing(
    input: f64,
    output: f64,
    cached: f64,
    write: f64,
    reasoning: f64,
) -> TokenPricing {
    let rate = |v: f64| (v.is_finite() && v > 0.0).then_some(v);
    TokenPricing {
        input_per_m: rate(input),
        output_per_m: rate(output),
        cache_read_per_m: rate(cached),
        cache_write_per_m: rate(write),
        reasoning_per_m: rate(reasoning),
        long_cache_write_per_m: None,
    }
}

pub fn parse_price_text(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenMetrics {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub reasoning_tokens: i64,
    pub output_tokens: i64,
    pub ttft_ms: i64,
    pub duration_ms: i64,
    pub tokens_per_sec: f64,
    pub estimated_cost_usd: f64,
    pub cache_creation_input_tokens: i64,
    pub cache_creation_cost_units: f64,
    pub reported_requests: i32,
    pub usage_is_complete: bool,
    pub cost_is_available: bool,
    pub output_includes_unreported_reasoning: bool,
    pub unpriced_cost_components: BTreeSet<String>,
    pub reported_cost_requests: i32,
}

impl TokenMetrics {
    pub fn total_tokens(&self) -> i64 {
        saturating_token_sum(&[self.input_tokens, self.reasoning_tokens, self.output_tokens])
    }
}

/// Cumulative non-token charges; prices are USD per declared unit, never inferred from magnitude.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderChargeUsage {
    pub reported_total_usd: Option<f64>,
    pub units: BTreeMap<String, i64>,
    pub observed_components: BTreeSet<String>,
    pub invalid: bool,
}

impl ProviderChargeUsage {
    pub fn updated_by(&self, next: &Self) -> Self {
        let mut units = self.units.clone();
        for (k, &v) in &next.units {
            units.insert(k.clone(), v);
        }
        let mut observed = self.observed_components.clone();
        observed.extend(next.observed_components.iter().cloned());
        Self {
            reported_total_usd: next.reported_total_usd.or(self.reported_total_usd),
            units,
            observed_components: observed,
            invalid: self.invalid || next.invalid,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChargePricing {
    pub per_unit_usd: BTreeMap<String, f64>,
}

impl ChargePricing {
    pub fn with_missing_from(&self, other: &Self) -> Self {
        let mut merged = other.per_unit_usd.clone();
        for (k, &v) in &self.per_unit_usd {
            merged.insert(k.clone(), v);
        }
        Self {
            per_unit_usd: merged,
        }
    }
}

/// Nullable counters distinguish unavailable usage from a provider-reported zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderTokenUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub short_cache_creation_input_tokens: Option<i64>,
    pub long_cache_creation_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub output_includes_reasoning: bool,
    pub invalid_counts: bool,
    pub uncached_input_tokens: Option<i64>,
    pub is_final: bool,
    pub charges: ProviderChargeUsage,
}

impl ProviderTokenUsage {
    pub fn is_complete(&self) -> bool {
        let (Some(input), Some(output)) = (self.input_tokens, self.output_tokens) else {
            return false;
        };
        if self.invalid_counts {
            return false;
        }
        let all_non_neg = [
            Some(input),
            Some(output),
            self.cached_input_tokens,
            self.cache_creation_input_tokens,
            self.short_cache_creation_input_tokens,
            self.long_cache_creation_input_tokens,
            self.reasoning_tokens,
            self.uncached_input_tokens,
        ]
        .into_iter()
        .flatten()
        .all(|v| v >= 0);
        if !all_non_neg {
            return false;
        }
        let cached = self.cached_input_tokens.unwrap_or(0);
        let created = self.cache_creation_input_tokens.unwrap_or(0);
        cached <= input
            && created <= input - cached
            && (self.uncached_input_tokens.is_none()
                || (self.cached_input_tokens.is_some()
                    && self.cache_creation_input_tokens.is_some()))
            && (!self.output_includes_reasoning || self.reasoning_tokens.unwrap_or(0) <= output)
    }

    pub fn updated_by(&self, next: &Self) -> Self {
        let mut merged = Self {
            input_tokens: next.input_tokens.or(self.input_tokens),
            cached_input_tokens: next.cached_input_tokens.or(self.cached_input_tokens),
            cache_creation_input_tokens: next
                .cache_creation_input_tokens
                .or(self.cache_creation_input_tokens),
            short_cache_creation_input_tokens: next
                .short_cache_creation_input_tokens
                .or(self.short_cache_creation_input_tokens),
            long_cache_creation_input_tokens: next
                .long_cache_creation_input_tokens
                .or(self.long_cache_creation_input_tokens),
            reasoning_tokens: next.reasoning_tokens.or(self.reasoning_tokens),
            output_tokens: next.output_tokens.or(self.output_tokens),
            output_includes_reasoning: next.output_includes_reasoning,
            invalid_counts: self.invalid_counts || next.invalid_counts,
            uncached_input_tokens: next.uncached_input_tokens.or(self.uncached_input_tokens),
            is_final: self.is_final || next.is_final,
            charges: self.charges.updated_by(&next.charges),
        };
        if let Some(uncached) = merged.uncached_input_tokens {
            let parts = [
                uncached,
                merged.cached_input_tokens.unwrap_or(0),
                merged.cache_creation_input_tokens.unwrap_or(0),
            ];
            let mut sum = 0i64;
            let mut overflow = false;
            for part in parts {
                if part > i64::MAX - sum {
                    overflow = true;
                    sum = i64::MAX;
                    break;
                }
                sum += part;
            }
            merged.input_tokens = Some(sum);
            merged.invalid_counts |= overflow;
        }
        merged
    }

    pub fn separate_reasoning_tokens(&self) -> i64 {
        let raw = self.reasoning_tokens.unwrap_or(0).max(0);
        if self.output_includes_reasoning {
            raw.min(self.output_tokens.unwrap_or(i64::MAX).max(0))
        } else {
            raw
        }
    }

    pub fn non_reasoning_output_tokens(&self) -> i64 {
        let out = self.output_tokens.unwrap_or(0);
        let sub = if self.output_includes_reasoning {
            self.separate_reasoning_tokens()
        } else {
            0
        };
        (out - sub).max(0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenPricing {
    pub input_per_m: Option<f64>,
    pub output_per_m: Option<f64>,
    pub cache_read_per_m: Option<f64>,
    pub cache_write_per_m: Option<f64>,
    pub reasoning_per_m: Option<f64>,
    pub long_cache_write_per_m: Option<f64>,
}

impl TokenPricing {
    pub fn has_any_rate(&self) -> bool {
        self.input_per_m.is_some()
            || self.output_per_m.is_some()
            || self.cache_read_per_m.is_some()
            || self.cache_write_per_m.is_some()
            || self.reasoning_per_m.is_some()
            || self.long_cache_write_per_m.is_some()
    }

    pub fn with_missing_from(&self, other: &Self) -> Self {
        Self {
            input_per_m: self.input_per_m.or(other.input_per_m),
            output_per_m: self.output_per_m.or(other.output_per_m),
            cache_read_per_m: self.cache_read_per_m.or(other.cache_read_per_m),
            cache_write_per_m: self.cache_write_per_m.or(other.cache_write_per_m),
            reasoning_per_m: self.reasoning_per_m.or(other.reasoning_per_m),
            long_cache_write_per_m: self.long_cache_write_per_m.or(other.long_cache_write_per_m),
        }
    }

    /// Token-only estimate; a missing rate returns `None` rather than silently treating it as free.
    pub fn estimate(
        &self,
        metrics: &TokenMetrics,
        short_write_tokens: Option<i64>,
        long_write_tokens: Option<i64>,
    ) -> Option<f64> {
        let input = metrics.input_tokens.max(0);
        let cached = metrics.cached_input_tokens.clamp(0, input);
        let created = metrics.cache_creation_input_tokens.clamp(0, input - cached);
        let ordinary = input - cached - created;

        let cost = |tokens: f64, rate: Option<f64>| -> Option<f64> {
            if tokens == 0.0 {
                return Some(0.0);
            }
            let r = rate?;
            if !tokens.is_finite() || tokens < 0.0 || !r.is_finite() || r < 0.0 {
                return None;
            }
            let c = tokens / 1_000_000.0 * r;
            c.is_finite().then_some(c)
        };

        let created_f = created as f64;
        let write_units = if metrics.cache_creation_cost_units.is_finite()
            && metrics.cache_creation_cost_units >= created_f
        {
            metrics.cache_creation_cost_units.min(created_f * 2.0)
        } else {
            created_f
        };
        let short_write_rate = self
            .cache_write_per_m
            .or(self.input_per_m.map(|v| v * 1.25));
        let write_cost = match (short_write_tokens, long_write_tokens) {
            (Some(s), Some(l)) if s >= 0 && l >= 0 && saturating_token_sum(&[s, l]) == created => {
                let short_cost = cost(s as f64, short_write_rate)?;
                let long_cost = if self.long_cache_write_per_m.is_some() {
                    cost(l as f64, self.long_cache_write_per_m)?
                } else {
                    cost((write_units - s as f64).max(0.0), short_write_rate)?
                };
                Some(short_cost + long_cost)
            }
            _ => cost(write_units, short_write_rate),
        }?;

        let terms = [
            cost(ordinary as f64, self.input_per_m)?,
            cost(cached as f64, self.cache_read_per_m.or(self.input_per_m))?,
            write_cost,
            cost(
                metrics.reasoning_tokens.max(0) as f64,
                self.reasoning_per_m.or(self.output_per_m),
            )?,
            cost(metrics.output_tokens.max(0) as f64, self.output_per_m)?,
        ];
        let sum: f64 = terms.into_iter().sum();
        sum.is_finite().then_some(sum)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPricingTier {
    pub input_tokens_above: i64,
    pub pricing: TokenPricing,
}

fn default_purpose() -> String {
    "chat".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiUsageRecord {
    pub request_id: String,
    #[serde(default)]
    pub response_id: String,
    pub wire_dialect: String,
    pub profile_id: String,
    pub model: String,
    #[serde(default)]
    pub pricing: TokenPricing,
    #[serde(default)]
    pub usage: ProviderTokenUsage,
    #[serde(default)]
    pub owner_session_id: String,
    #[serde(default)]
    pub owner_run_id: String,
    #[serde(default = "default_purpose")]
    pub purpose: String,
    #[serde(default)]
    pub context_pricing_tiers: Vec<ContextPricingTier>,
    #[serde(default)]
    pub charge_pricing: ChargePricing,
}
