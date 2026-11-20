use super::*;

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
