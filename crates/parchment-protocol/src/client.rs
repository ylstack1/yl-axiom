//! Async streaming request executor combining all 5 wire dialects, HTTP 400 reasoning self-healing,
//! `Retry-After`/backoff retry scheduling, and primary->legacy fallback (never for `CUSTOM`).

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parchment_model::{
    ApiUsageRecord, AuthHeaderMode, ChatSession, GoalTrackerState, MessageNode, MessageRole,
    MessageVariant, ModelPricingSpec, ProviderProfile, TokenMetrics, ToolCallRecord,
};
use tokio::sync::Notify;

use crate::error_policy::{
    ProtocolError, ProviderHttpError, is_context_length_exceeded_error,
    is_transient_or_proxy_timeout_error, parse_retry_after_ms, retry_delay_ms,
    should_fallback_to_legacy_protocol,
};
use crate::signatures::{append_signature_chunk_safe, step_down_reasoning_effort};
use crate::sse::{
    INITIAL_LINE_TIMEOUT, STREAMING_LINE_TIMEOUT, SseFrame, SseStreamConsumer,
    drive_sse_or_json_stream,
};
use crate::usage::ProviderUsageLedger;
use crate::wire::{
    RawWebCitation, StreamEvent, ToolDefinition, WireAdaptOptions, WireDialect, WireStreamParser,
    build_auth_headers_for_key, build_wire_request, resolve_primary_dialect,
    strip_api_version_suffix,
};

const MAX_TRANSIENT_RETRIES: usize = 3;
const MAX_HTTP_400_HEALS: usize = 6;
const MAX_WIRE_PREVIEW_CHARS: usize = 4_096;
static API_KEY_RR: AtomicUsize = AtomicUsize::new(0);
#[derive(Debug, Clone)]
pub struct RoundExecutionResult {
    pub dialect_used: WireDialect,
    pub interaction_id: String,
    pub content: String,
    pub thinking_summary: String,
    pub thinking_signature: String,
    pub thinking_duration_ms: i64,
    pub raw_request_preview: String,
    pub raw_response_preview: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub citations: Vec<RawWebCitation>,
    pub hit_output_limit: bool,
    pub token_metrics: TokenMetrics,
    pub usage_records: Vec<ApiUsageRecord>,
}
pub trait StreamCallbacks {
    fn on_text_delta(&mut self, _delta: &str) {}
    fn on_thinking_delta(&mut self, _delta: &str) {}
    fn on_status(&mut self, _status: &str) {}
    fn on_retry_wait(&mut self, _attempt: usize, _wait_ms: u64, _reason: &str) {}
}

pub struct NoopCallbacks;
impl StreamCallbacks for NoopCallbacks {}

pub struct ProtocolClient {
    http: reqwest::Client,
    initial_line_timeout: Duration,
    streaming_line_timeout: Duration,
    max_retry_wait_override_ms: Option<u64>,
    retry_now: Option<Arc<Notify>>,
}

impl ProtocolClient {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            initial_line_timeout: INITIAL_LINE_TIMEOUT,
            streaming_line_timeout: STREAMING_LINE_TIMEOUT,
            max_retry_wait_override_ms: None,
            retry_now: None,
        }
    }

    /// Attaches a `Notify` handle that wakes an active retry backoff immediately (`Retry now`).
    pub fn with_retry_notify(mut self, notify: Arc<Notify>) -> Self {
        self.retry_now = Some(notify);
        self
    }

    /// Used by deterministic local fixture tests so retries run in milliseconds instead of minutes.
    pub fn with_test_timings(
        http: reqwest::Client,
        line_timeout: Duration,
        max_retry_wait_ms: u64,
    ) -> Self {
        Self {
            http,
            initial_line_timeout: line_timeout,
            streaming_line_timeout: line_timeout,
            max_retry_wait_override_ms: Some(max_retry_wait_ms),
            retry_now: None,
        }
    }

    async fn sleep_retry(&self, wait_ms: u64) {
        let dur = Duration::from_millis(wait_ms);
        if let Some(notify) = &self.retry_now {
            tokio::select! {
                _ = tokio::time::sleep(dur) => {}
                _ = notify.notified() => {}
            }
        } else {
            tokio::time::sleep(dur).await;
        }
    }

    /// Dynamically fetches available model IDs from the provider or custom proxy endpoint.
    /// Supports OpenAI `/v1/models`, Google `/v1beta/models`, and Anthropic `/v1/models`.
    pub async fn fetch_available_models(
        &self,
        profile: &ProviderProfile,
    ) -> Result<Vec<String>, ProtocolError> {
        let dialect = resolve_primary_dialect(profile).unwrap_or(WireDialect::OpenAiResponses);
        let base = profile
            .effective_base_url()
            .trim()
            .trim_end_matches('#')
            .trim_end_matches('/')
            .to_string();
        if base.is_empty() {
            return Err(ProtocolError::NoRetry(
                "Provider Server URL is empty.".into(),
            ));
        }
        let lower = base.to_ascii_lowercase();
        let candidates: Vec<String> = if lower.ends_with("/models") {
            vec![base.clone()]
        } else if let Some(root) = strip_api_version_suffix(&base) {
            let primary = format!("{base}/models");
            let fallback = format!("{root}/v1/models");
            if primary == fallback {
                vec![primary]
            } else {
                vec![primary, fallback]
            }
        } else {
            match dialect {
                WireDialect::GeminiInteractions | WireDialect::GeminiGenerateContent => vec![
                    format!("{base}/v1beta/models?pageSize=200"),
                    format!("{base}/v1/models"),
                    format!("{base}/models"),
                ],
                WireDialect::AnthropicMessages => vec![
                    format!("{base}/v1/models?limit=100"),
                    format!("{base}/v1/models"),
                    format!("{base}/models"),
                ],
                _ => vec![
                    format!("{base}/v1/models"),
                    format!("{base}/api/v3/models"),
                    format!("{base}/models"),
                ],
            }
        };

        let keys = {
            let k = profile.all_api_keys();
            if k.is_empty() { vec![String::new()] } else { k }
        };
        let mut last_err = String::new();

        for key in &keys {
            let trimmed_key = key.trim();
            let headers = build_auth_headers_for_key(profile, trimmed_key, dialect);
            let mut key_auth_or_rate_limited = false;
            for base_url in &candidates {
                let mut req = self.http.get(base_url);
                for (k, v) in &headers {
                    if !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("accept")
                    {
                        req = req.header(k.as_str(), v.as_str());
                    }
                }
                req = req.header("Accept", "application/json");
                let mut built = match req.build() {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = e.to_string();
                        continue;
                    }
                };
                if profile.auth_header_mode == AuthHeaderMode::QueryParam && !trimmed_key.is_empty()
                {
                    built
                        .url_mut()
                        .query_pairs_mut()
                        .append_pair("key", trimmed_key);
                }
                match self.http.execute(built).await {
                    Ok(resp) => {
                        let status = resp.status().as_u16();
                        let body = resp.text().await.unwrap_or_default();
                        if !(200..300).contains(&status) {
                            last_err = format!(
                                "HTTP {status}: {}",
                                body.chars().take(240).collect::<String>()
                            );
                            if matches!(status, 401 | 403 | 429) {
                                key_auth_or_rate_limited = true;
                                break;
                            }
                            continue;
                        }
                        let parsed = parse_models_list_json(&body);
                        if !parsed.is_empty() {
                            return Ok(parsed);
                        }
                    }
                    Err(e) => {
                        last_err = e.to_string();
                    }
                }
            }
            if !key_auth_or_rate_limited {
                break;
            }
        }
        if last_err.is_empty() {
            Err(ProtocolError::NoRetry(
                "Endpoint returned an empty model list.".into(),
            ))
        } else {
            Err(ProtocolError::Transport(last_err))
        }
    }

    /// Executes a single-shot utility completion (title, summary, translation) with tools and goals disabled.
    pub async fn run_utility_prompt(
        &self,
        profile: &ProviderProfile,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        purpose: &str,
    ) -> Result<(String, Vec<ApiUsageRecord>), ProtocolError> {
        let mut util_profile = profile.clone();
        util_profile.enable_web_search = false;
        util_profile.enable_code_execution = false;
        util_profile.enable_url_context = false;
        util_profile.use_stateful_chaining = false;
        util_profile.store_server_side = false;
        util_profile.thinking_effort = "none".into();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut session = ChatSession::new(format!("util-{purpose}-{now}"), purpose, now);
        session.system_prompt = system_prompt.to_string();
        let nodes = vec![MessageNode {
            id: format!("util-u-{now}"),
            role: MessageRole::User,
            variants: vec![MessageVariant {
                id: format!("util-u-{now}-v0"),
                content: user_prompt.to_string(),
                created_at: now,
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        }];
        let mut cb = NoopCallbacks;
        let res = self
            .execute_round(
                &util_profile,
                model,
                &session,
                &nodes,
                &[],
                None,
                "",
                &format!("util-{purpose}-{now}"),
                None,
                &mut cb,
            )
            .await?;
        let mut usage = res.usage_records;
        for u in &mut usage {
            u.purpose = purpose.to_string();
        }
        Ok((res.content.trim().to_string(), usage))
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_round<C: StreamCallbacks>(
        &self,
        profile: &ProviderProfile,
        model: &str,
        session: &ChatSession,
        history_nodes: &[MessageNode],
        tools: &[ToolDefinition],
        goal_state: Option<&GoalTrackerState>,
        continue_prefix: &str,
        owner_run_id: &str,
        catalog_spec: Option<ModelPricingSpec>,
        callbacks: &mut C,
    ) -> Result<RoundExecutionResult, ProtocolError> {
        let primary = resolve_primary_dialect(profile)?;
        let mut ledger = ProviderUsageLedger::new(
            profile.clone(),
            model.to_string(),
            session.id.clone(),
            owner_run_id.to_string(),
            "chat".to_string(),
            catalog_spec,
        );

        let primary_res = self
            .execute_on_dialect(
                profile,
                primary,
                model,
                session,
                history_nodes,
                tools,
                goal_state,
                continue_prefix,
                &mut ledger,
                callbacks,
            )
            .await;

        match primary_res {
            Ok(ok) => Ok(ok),
            Err(err) => {
                // CUSTOM protocol NEVER falls back to another dialect.
                if !profile.protocol.is_custom_wire()
                    && let Some(fallback) = primary.legacy_fallback()
                    && should_fallback_to_legacy_protocol(
                        &err,
                        profile.use_legacy_fallback_endpoint,
                    )
                {
                    callbacks.on_status(&format!(
                        "Primary endpoint failed; falling back to {}",
                        fallback.wire_name()
                    ));
                    return self
                        .execute_on_dialect(
                            profile,
                            fallback,
                            model,
                            session,
                            history_nodes,
                            tools,
                            goal_state,
                            continue_prefix,
                            &mut ledger,
                            callbacks,
                        )
                        .await;
                }
                Err(err)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_on_dialect<C: StreamCallbacks>(
        &self,
        profile: &ProviderProfile,
        dialect: WireDialect,
        model: &str,
        session: &ChatSession,
        history_nodes: &[MessageNode],
        tools: &[ToolDefinition],
        goal_state: Option<&GoalTrackerState>,
        continue_prefix: &str,
        ledger: &mut ProviderUsageLedger,
        callbacks: &mut C,
    ) -> Result<RoundExecutionResult, ProtocolError> {
        let mut adapt = WireAdaptOptions::default();
        let mut working_nodes: Cow<'_, [MessageNode]> = Cow::Borrowed(history_nodes);
        let mut compacted_overflow = false;
        let mut transient_attempts = 0usize;
        let mut heal_attempts = 0usize;
        let keys = {
            let k = profile.all_api_keys();
            if k.is_empty() { vec![String::new()] } else { k }
        };
        let mut key_idx = if keys.len() > 1 {
            API_KEY_RR.fetch_add(1, Ordering::Relaxed) % keys.len()
        } else {
            0
        };
        let mut key_failovers = 0usize;
        let started = Instant::now();

        loop {
            let active_key = keys[key_idx].trim();
            let wire_req = build_wire_request(
                profile,
                dialect,
                model,
                session,
                working_nodes.as_ref(),
                tools,
                goal_state,
                continue_prefix,
                &adapt,
            )?;
            let raw_request_preview = truncate_preview(&wire_req.body_json);
            let headers = build_auth_headers_for_key(profile, active_key, dialect);
            let usage_req_id = ledger.begin(dialect.usage_dialect());
            let mut req_builder = self.http.post(&wire_req.url).body(wire_req.body_json);
            for (k, v) in &headers {
                req_builder = req_builder.header(k.as_str(), v.as_str());
            }
            let mut built_req = req_builder
                .build()
                .map_err(|e| ProtocolError::NoRetry(e.to_string()))?;
            if profile.auth_header_mode == AuthHeaderMode::QueryParam && !active_key.is_empty() {
                built_req
                    .url_mut()
                    .query_pairs_mut()
                    .append_pair("key", active_key);
            }

            let response = match self.http.execute(built_req).await {
                Ok(resp) => resp,
                Err(e) => {
                    let err = ProtocolError::Transport(e.to_string());
                    if is_transient_or_proxy_timeout_error(&err)
                        && transient_attempts < MAX_TRANSIENT_RETRIES
                    {
                        transient_attempts += 1;
                        let wait_ms =
                            self.capped_wait(retry_delay_ms(transient_attempts, &err, 0.5, None));
                        callbacks.on_retry_wait(transient_attempts, wait_ms, &err.to_string());
                        self.sleep_retry(wait_ms).await;
                        continue;
                    }
                    return Err(err);
                }
            };

            let status = response.status().as_u16();
            if !(200..300).contains(&status) {
                let headers_map = response.headers().clone();
                let body = response.text().await.unwrap_or_default();
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let retry_after_ms = parse_retry_after_ms(
                    |name| headers_map.get(name).and_then(|v| v.to_str().ok()),
                    &body,
                    now_ms,
                );
                // HTTP 400 reasoning & parameter self-healing ladder (before failing or falling back).
                if status == 400
                    && heal_attempts < MAX_HTTP_400_HEALS
                    && self.try_heal_http_400(profile, dialect, tools, &body, &mut adapt)
                {
                    heal_attempts += 1;
                    callbacks.on_status("Retrying with adjusted provider request parameters...");
                    continue;
                }

                let http_err = ProtocolError::Http(ProviderHttpError {
                    label: dialect.wire_name().to_string(),
                    status,
                    body,
                    retry_after_ms,
                });

                if !compacted_overflow && is_context_length_exceeded_error(&http_err) {
                    let compacted = crate::history::compact_history_nodes_on_overflow(
                        working_nodes.as_ref(),
                        now_ms,
                    );
                    if compacted.len() != working_nodes.len() {
                        working_nodes = Cow::Owned(compacted);
                        compacted_overflow = true;
                        callbacks
                            .on_status("Shortening older turns after hitting conversation limit…");
                        continue;
                    }
                }
                if matches!(status, 401 | 403 | 429)
                    && keys.len() > 1
                    && key_failovers < keys.len() - 1
                {
                    let prev_num = key_idx + 1;
                    key_idx = (key_idx + 1) % keys.len();
                    let next_num = key_idx + 1;
                    key_failovers += 1;
                    callbacks.on_status(&format!(
                        "API key #{prev_num} hit HTTP {status} — switching to key #{next_num} of {}...",
                        keys.len()
                    ));
                    continue;
                }

                if is_transient_or_proxy_timeout_error(&http_err)
                    && transient_attempts < MAX_TRANSIENT_RETRIES
                {
                    transient_attempts += 1;
                    let wait_ms =
                        self.capped_wait(retry_delay_ms(transient_attempts, &http_err, 0.5, None));
                    callbacks.on_retry_wait(transient_attempts, wait_ms, &http_err.to_string());
                    self.sleep_retry(wait_ms).await;
                    continue;
                }

                return Err(http_err);
            }

            // Stream SSE / plain-JSON response body.
            let mut consumer = RoundStreamConsumer {
                parser: WireStreamParser::new(dialect),
                events_buf: Vec::with_capacity(16),
                interaction_id: String::new(),
                content: String::new(),
                thinking_summary: String::new(),
                thinking_signature: String::new(),
                thinking_started: None,
                thinking_duration_ms: 0,
                raw_response_preview: String::new(),
                raw_response_chars: 0,
                tool_calls: Vec::new(),
                citations: Vec::new(),
                hit_output_limit: false,
                first_token_ms: None,
                started,
                usage_req_id: &usage_req_id,
                ledger,
                callbacks,
            };

            let stream_res = drive_sse_or_json_stream(
                response.bytes_stream(),
                self.initial_line_timeout,
                self.streaming_line_timeout,
                &mut consumer,
            )
            .await;

            let RoundStreamConsumer {
                interaction_id,
                content,
                thinking_summary,
                thinking_signature,
                thinking_started,
                mut thinking_duration_ms,
                raw_response_preview,
                tool_calls,
                citations,
                hit_output_limit,
                first_token_ms,
                ..
            } = consumer;
            if let Some(t0) = thinking_started {
                thinking_duration_ms =
                    thinking_duration_ms.saturating_add(t0.elapsed().as_millis() as i64);
            }

            match stream_res {
                Ok(()) => {
                    let duration_ms = started.elapsed().as_millis() as i64;
                    let token_metrics = ledger
                        .aggregate_metrics(first_token_ms.unwrap_or(duration_ms), duration_ms);
                    return Ok(RoundExecutionResult {
                        dialect_used: dialect,
                        interaction_id,
                        content,
                        thinking_summary,
                        thinking_signature,
                        thinking_duration_ms,
                        raw_request_preview,
                        raw_response_preview,
                        tool_calls,
                        citations,
                        hit_output_limit,
                        token_metrics,
                        usage_records: ledger.records(),
                    });
                }
                Err(err) => {
                    // Only retry automatically if nothing was streamed to the caller yet.
                    let can_retry = content.is_empty()
                        && tool_calls.is_empty()
                        && is_transient_or_proxy_timeout_error(&err)
                        && transient_attempts < MAX_TRANSIENT_RETRIES;
                    if can_retry {
                        transient_attempts += 1;
                        let wait_ms =
                            self.capped_wait(retry_delay_ms(transient_attempts, &err, 0.5, None));
                        callbacks.on_retry_wait(transient_attempts, wait_ms, &err.to_string());
                        self.sleep_retry(wait_ms).await;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    fn try_heal_http_400(
        &self,
        profile: &ProviderProfile,
        dialect: WireDialect,
        tools: &[ToolDefinition],
        body: &str,
        adapt: &mut WireAdaptOptions,
    ) -> bool {
        let lower = body.to_ascii_lowercase();
        let current_eff = adapt
            .reasoning_effort_override
            .as_deref()
            .unwrap_or(profile.thinking_effort.as_str());

        // 1. Anthropic adaptive `display` stripped BEFORE dropping reasoning (stable contract!).
        if dialect == WireDialect::AnthropicMessages
            && !adapt.omit_anthropic_adaptive_display
            && !adapt.omit_reasoning
            && (lower.contains("display") || lower.contains("extra inputs are not permitted"))
        {
            adapt.omit_anthropic_adaptive_display = true;
            return true;
        }

        // 2. Gemini thinkingLevel -> thinkingBudget fallback for older models/proxies.
        if dialect == WireDialect::GeminiGenerateContent
            && !adapt.use_gemini_thinking_budget
            && !adapt.omit_reasoning
            && (lower.contains("thinkinglevel") || lower.contains("thinking_level"))
        {
            adapt.use_gemini_thinking_budget = true;
            return true;
        }

        // 3. Reasoning effort downgrade ladder (`max -> xhigh -> high`, `minimal -> low`).
        if !adapt.omit_reasoning
            && let Some(next_effort) = step_down_reasoning_effort(current_eff, &lower)
        {
            adapt.reasoning_effort_override = Some(next_effort.to_string());
            return true;
        }

        // 4. OpenAI Chat `stream_options` unsupported by third-party proxies.
        if dialect == WireDialect::OpenAiChat
            && !adapt.omit_stream_options
            && lower.contains("stream_options")
        {
            adapt.omit_stream_options = true;
            return true;
        }

        // 5. OpenAI Chat max_completion_tokens vs max_tokens parameter mismatch.
        if dialect == WireDialect::OpenAiChat && adapt.use_max_tokens_key.is_none() {
            if lower.contains("max_completion_tokens") {
                adapt.use_max_tokens_key = Some(true);
                return true;
            }
            if lower.contains("max_tokens") {
                adapt.use_max_tokens_key = Some(false);
                return true;
            }
        }

        // 6. Strip mixed provider built-in tools if they collide with function declarations.
        let has_builtin = profile.enable_web_search
            || profile.enable_code_execution
            || profile.enable_url_context;
        if has_builtin
            && !adapt.omit_builtin_tools
            && !tools.is_empty()
            && (lower.contains("tool")
                || lower.contains("google_search")
                || lower.contains("code_execution")
                || lower.contains("url_context")
                || lower.contains("web_search"))
        {
            adapt.omit_builtin_tools = true;
            return true;
        }

        // 7. Strip unsupported temperature/top_p when reasoning is active or endpoint rejects sampling.
        if !adapt.strip_sampling && (lower.contains("temperature") || lower.contains("top_p")) {
            adapt.strip_sampling = true;
            return true;
        }

        // 8. Last resort: drop reasoning if endpoint rejects reasoning/thinking fields outright.
        if !adapt.omit_reasoning && (lower.contains("reasoning") || lower.contains("thinking")) {
            adapt.omit_reasoning = true;
            return true;
        }

        false
    }

    fn capped_wait(&self, ms: u64) -> u64 {
        match self.max_retry_wait_override_ms {
            Some(cap) => ms.min(cap),
            None => ms,
        }
    }
}

fn truncate_preview(raw: &str) -> String {
    raw.chars().take(MAX_WIRE_PREVIEW_CHARS).collect()
}

struct RoundStreamConsumer<'a, C: StreamCallbacks> {
    parser: WireStreamParser,
    events_buf: Vec<StreamEvent>,
    interaction_id: String,
    content: String,
    thinking_summary: String,
    thinking_signature: String,
    thinking_started: Option<Instant>,
    thinking_duration_ms: i64,
    raw_response_preview: String,
    raw_response_chars: usize,
    tool_calls: Vec<ToolCallRecord>,
    citations: Vec<RawWebCitation>,
    hit_output_limit: bool,
    first_token_ms: Option<i64>,
    started: Instant,
    usage_req_id: &'a str,
    ledger: &'a mut ProviderUsageLedger,
    callbacks: &'a mut C,
}

impl<C: StreamCallbacks> RoundStreamConsumer<'_, C> {
    fn append_response_preview(&mut self, chunk: &str) {
        if self.raw_response_chars >= MAX_WIRE_PREVIEW_CHARS || chunk.is_empty() {
            return;
        }
        if !self.raw_response_preview.is_empty() && self.raw_response_chars < MAX_WIRE_PREVIEW_CHARS
        {
            self.raw_response_preview.push('\n');
            self.raw_response_chars += 1;
        }
        let rem = MAX_WIRE_PREVIEW_CHARS.saturating_sub(self.raw_response_chars);
        for ch in chunk.chars().take(rem) {
            self.raw_response_preview.push(ch);
            self.raw_response_chars += 1;
        }
    }

    fn drain_events(&mut self) {
        let freeze = |started: &mut Option<Instant>, total_ms: &mut i64| {
            if let Some(t0) = started.take() {
                *total_ms = total_ms.saturating_add(t0.elapsed().as_millis() as i64);
            }
        };
        for ev in self.events_buf.drain(..) {
            match ev {
                StreamEvent::Status(s) => self.callbacks.on_status(&s),
                StreamEvent::InteractionId(id) => self.interaction_id = id,
                StreamEvent::ThinkingDelta(d) => {
                    if self.first_token_ms.is_none() {
                        self.first_token_ms = Some(self.started.elapsed().as_millis() as i64);
                    }
                    if self.thinking_started.is_none() {
                        self.thinking_started = Some(Instant::now());
                    }
                    self.thinking_summary.push_str(&d);
                    self.callbacks.on_thinking_delta(&d);
                }
                StreamEvent::ThinkingSignature(s) => {
                    append_signature_chunk_safe(&mut self.thinking_signature, &s);
                }
                StreamEvent::TextDelta(d) => {
                    freeze(&mut self.thinking_started, &mut self.thinking_duration_ms);
                    if self.first_token_ms.is_none() {
                        self.first_token_ms = Some(self.started.elapsed().as_millis() as i64);
                    }
                    self.content.push_str(&d);
                    self.callbacks.on_text_delta(&d);
                }
                StreamEvent::ToolCall {
                    id,
                    name,
                    arguments_json,
                    thought_signature,
                } => {
                    freeze(&mut self.thinking_started, &mut self.thinking_duration_ms);
                    let starts_tool_step = self.tool_calls.is_empty();
                    self.tool_calls.push(ToolCallRecord {
                        id,
                        name,
                        arguments_json,
                        thought_signature,
                        starts_tool_step,
                        ..Default::default()
                    });
                }
                StreamEvent::Citation(c) => {
                    if !self.citations.iter().any(|existing| existing.url == c.url) {
                        self.citations.push(c);
                    }
                }
                StreamEvent::Usage { response_id, usage } => {
                    self.ledger.update(self.usage_req_id, &response_id, &usage);
                }
                StreamEvent::OutputLimit => self.hit_output_limit = true,
                StreamEvent::Done => {
                    freeze(&mut self.thinking_started, &mut self.thinking_duration_ms);
                }
            }
        }
    }
}

impl<C: StreamCallbacks> SseStreamConsumer for RoundStreamConsumer<'_, C> {
    fn is_finished(&self) -> bool {
        self.parser.is_finished()
    }

    fn on_frame(&mut self, frame: &SseFrame) -> Result<(), ProtocolError> {
        self.append_response_preview(&frame.data);
        self.events_buf.clear();
        self.parser
            .consume_sse(&frame.event_type, &frame.data, &mut self.events_buf)?;
        self.drain_events();
        Ok(())
    }

    fn on_plain_json(&mut self, body: &str) -> Result<(), ProtocolError> {
        self.append_response_preview(body);
        self.events_buf.clear();
        self.parser.consume_sse("", body, &mut self.events_buf)?;
        self.parser.flush_pending_tools(&mut self.events_buf);
        self.drain_events();
        Ok(())
    }
}

/// Parses OpenAI (`data[].id`), Gemini (`models[].name`), Anthropic (`data[].id`), or plain array JSON.
pub fn parse_models_list_json(body: &str) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut push_id = |raw: &str| {
        let clean = raw.trim().trim_prefix("models/");
        if !clean.is_empty() && !out.iter().any(|existing: &String| existing == clean) {
            out.push(clean.to_string());
        }
    };
    if let Some(arr) = val.get("data").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                push_id(id);
            } else if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                push_id(name);
            }
        }
    }
    if let Some(arr) = val.get("models").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                push_id(name);
            } else if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                push_id(id);
            }
        }
    }
    if let Some(arr) = val.as_array() {
        for item in arr {
            if let Some(s) = item.as_str() {
                push_id(s);
            } else if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                push_id(id);
            }
        }
    }
    out
}
