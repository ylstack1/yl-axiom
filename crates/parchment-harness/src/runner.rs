//! Multi-step streaming `AgentTurnRunner` and monotonic `RunActivity` elapsed-time tracker.
//!
//! Contracts:
//! - Monotonic clock (`Instant`) skips system-sleep gaps `> 5 s` and pauses during approval waits.
//! - Displays whole seconds (`30s`, `20m21s`).
//! - Enforces `chat_spend_limit_usd` before every tool round (including live `external_run_spend_usd`).
//! - Runs parallel read-only tool batches and parallel `agent_delegate` batches (max 4).

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parchment_model::{
    ApiUsageRecord, ChatSession, MessageNode, MessageRole, MessageVariant, ModelPricingSpec,
    ProviderProfile, TokenMetrics, ToolCallRecord, WebSourceCitation,
};
use parchment_protocol::usage::spend_toward_limit;
use parchment_protocol::{ProtocolClient, ProtocolError, StreamCallbacks, WireDialect};

use crate::permissions::ToolCatalog;
use crate::tools::{HarnessContext, HarnessExecutor, ToolOutput};

const MAX_TOOL_ROUNDS: usize = 64;
const SLEEP_GAP_THRESHOLD: Duration = Duration::from_secs(5);

/// Monotonic active-run timer that skips sleep gaps `> 5 s` and user approval waits.
#[derive(Debug, Clone)]
pub struct RunActivity {
    accumulated: Duration,
    last_tick: Instant,
    waiting_for_user: bool,
}

impl Default for RunActivity {
    fn default() -> Self {
        Self::new()
    }
}

impl RunActivity {
    pub fn new() -> Self {
        Self {
            accumulated: Duration::ZERO,
            last_tick: Instant::now(),
            waiting_for_user: false,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        if !self.waiting_for_user {
            let delta = now.saturating_duration_since(self.last_tick);
            if delta <= SLEEP_GAP_THRESHOLD {
                self.accumulated += delta;
            }
        }
        self.last_tick = now;
    }

    pub fn set_waiting(&mut self, waiting: bool) {
        self.tick();
        self.waiting_for_user = waiting;
    }

    pub fn active_duration(&mut self) -> Duration {
        self.tick();
        self.accumulated
    }
}

/// Formats elapsed duration in Parchment's whole-second display style (`0s`, `30s`, `20m21s`, `1h05m`).
pub fn format_elapsed_whole_seconds(dur: Duration) -> String {
    let total_secs = dur.as_secs();
    if total_secs < 60 {
        format!("{total_secs}s")
    } else if total_secs < 3600 {
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{mins}m{secs:02}s")
    } else {
        let hrs = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        format!("{hrs}h{mins:02}m")
    }
}

#[derive(Debug, Clone)]
pub enum RunProgressEvent {
    Status(String),
    TextDelta(String),
    ThinkingDelta(String),
    RetryWait {
        attempt: usize,
        wait_ms: u64,
        reason: String,
    },
    ToolStarted {
        call_id: String,
        name: String,
        arguments_json: String,
    },
    ToolFinished {
        call_id: String,
        name: String,
        is_error: bool,
        duration_ms: i64,
        result_preview: String,
    },
}

pub struct TurnRunOutcome {
    pub dialect_used: WireDialect,
    pub variant: MessageVariant,
    pub usage_records: Vec<ApiUsageRecord>,
    pub elapsed_ms: i64,
}

pub struct AgentTurnRunner;

impl AgentTurnRunner {
    #[allow(clippy::too_many_arguments)]
    pub async fn run_turn<F>(
        protocol: &ProtocolClient,
        profile: &ProviderProfile,
        model: &str,
        session: &ChatSession,
        history_nodes: &[MessageNode],
        continue_prefix: &str,
        harness_ctx: &HarnessContext,
        owner_run_id: &str,
        chat_spend_limit_usd: f64,
        prior_chat_spend_usd: f64,
        catalog_spec: Option<ModelPricingSpec>,
        mut on_progress: F,
    ) -> Result<TurnRunOutcome, ProtocolError>
    where
        F: FnMut(RunProgressEvent),
    {
        let mut activity = RunActivity::new();
        let wait_ms_before = harness_ctx.broker.total_wait_ms();
        // Borrow `history_nodes` directly on round 0; only materialize an owned Vec if tool rounds run.
        let mut owned_history: Option<Vec<MessageNode>> = None;
        let mut all_usage: Vec<ApiUsageRecord> = Vec::new();
        let mut all_tool_calls: Vec<ToolCallRecord> = Vec::new();
        let mut all_citations: Vec<WebSourceCitation> = Vec::new();
        let mut accumulated_thinking = String::new();
        let mut last_signature = String::new();
        let mut last_interaction_id = String::new();
        let mut final_content = String::new();
        let mut final_dialect = WireDialect::OpenAiResponses;
        let mut accumulated_metrics = TokenMetrics::default();
        let mut has_metrics = false;
        let mut total_thinking_duration_ms: i64 = 0;
        let mut last_raw_request_preview = String::new();
        let mut last_raw_response_preview = String::new();
        for round_idx in 0..MAX_TOOL_ROUNDS {
            activity.tick();
            let live_spend = prior_chat_spend_usd + spend_toward_limit(&all_usage);
            if chat_spend_limit_usd > 0.0 && live_spend >= chat_spend_limit_usd {
                return Err(ProtocolError::SpendLimit(format!(
                    "Chat spend (${live_spend:.4}) reached configured cap (${chat_spend_limit_usd:.4})."
                )));
            }

            let external_tools = match &harness_ctx.mcp {
                Some(reg) => reg.tool_definitions().await,
                None => Vec::new(),
            };
            let active_wire_tools = match harness_ctx.broker.current_scope() {
                Some(mut scope) => {
                    let expanded = harness_ctx.expanded_mcp_tools.lock().await.clone();
                    if !expanded.is_empty() {
                        scope.expanded_mcp_tools = Some(expanded);
                    }
                    ToolCatalog::active_tools(&scope, &external_tools)
                        .into_iter()
                        .map(|t| t.to_wire_tool())
                        .collect()
                }
                None => Vec::new(),
            };

            let goal_snapshot = harness_ctx.goal_state.lock().await.clone();
            let goal_ref = (!goal_snapshot.todos.is_empty()
                || !goal_snapshot.goal_objective.is_empty())
            .then_some(&goal_snapshot);

            let mut cb = ProgressBridge(&mut on_progress);
            let round_history = owned_history.as_deref().unwrap_or(history_nodes);
            let round_prefix = if round_idx == 0 { continue_prefix } else { "" };
            let round = protocol
                .execute_round(
                    profile,
                    model,
                    session,
                    round_history,
                    &active_wire_tools,
                    goal_ref,
                    round_prefix,
                    owner_run_id,
                    catalog_spec.clone(),
                    &mut cb,
                )
                .await?;

            final_dialect = round.dialect_used;
            if !round.interaction_id.is_empty() {
                last_interaction_id = round.interaction_id;
            }
            if !round.thinking_summary.is_empty() {
                if !accumulated_thinking.is_empty() {
                    accumulated_thinking.push('\n');
                }
                accumulated_thinking.push_str(&round.thinking_summary);
            }
            if !round.thinking_signature.is_empty() {
                last_signature = round.thinking_signature;
            }
            total_thinking_duration_ms =
                total_thinking_duration_ms.saturating_add(round.thinking_duration_ms);
            if !round.raw_request_preview.is_empty() {
                last_raw_request_preview = round.raw_request_preview;
            }
            if !round.raw_response_preview.is_empty() {
                last_raw_response_preview = round.raw_response_preview;
            }
            if !has_metrics {
                accumulated_metrics = round.token_metrics;
                has_metrics = true;
            } else {
                let m = &round.token_metrics;
                if m.input_tokens > 0 {
                    accumulated_metrics.input_tokens = m.input_tokens;
                }
                if m.cached_input_tokens > 0 {
                    accumulated_metrics.cached_input_tokens = m.cached_input_tokens;
                }
                accumulated_metrics.output_tokens = accumulated_metrics
                    .output_tokens
                    .saturating_add(m.output_tokens);
                accumulated_metrics.reasoning_tokens = accumulated_metrics
                    .reasoning_tokens
                    .saturating_add(m.reasoning_tokens);
                accumulated_metrics.duration_ms = accumulated_metrics
                    .duration_ms
                    .saturating_add(m.duration_ms);
                accumulated_metrics.reported_requests = accumulated_metrics
                    .reported_requests
                    .saturating_add(m.reported_requests);
                accumulated_metrics.reported_cost_requests = accumulated_metrics
                    .reported_cost_requests
                    .saturating_add(m.reported_cost_requests);
                accumulated_metrics.estimated_cost_usd += m.estimated_cost_usd;
                if accumulated_metrics.ttft_ms == 0 && m.ttft_ms > 0 {
                    accumulated_metrics.ttft_ms = m.ttft_ms;
                }
                let total_gen = accumulated_metrics
                    .output_tokens
                    .saturating_add(accumulated_metrics.reasoning_tokens);
                if accumulated_metrics.duration_ms > 0 && total_gen > 0 {
                    accumulated_metrics.tokens_per_sec =
                        (total_gen as f64) / (accumulated_metrics.duration_ms as f64 / 1000.0);
                }
            }
            all_usage.extend(round.usage_records);
            for c in round.citations {
                if !c.url.is_empty() && !all_citations.iter().any(|e| e.url == c.url) {
                    let domain = url::Url::parse(&c.url)
                        .ok()
                        .and_then(|u| u.host_str().map(str::to_string))
                        .unwrap_or_default();
                    let next_idx = (all_citations.len() + 1) as i32;
                    all_citations.push(WebSourceCitation {
                        index: next_idx,
                        url: c.url,
                        title: c.title,
                        domain,
                        grounded_line_numbers: Vec::new(),
                        grounded_lines: Vec::new(),
                        snippet: c.snippet,
                    });
                }
            }
            if round.tool_calls.is_empty() {
                final_content = round.content;
                break;
            }

            // Execute tool calls (parallel for pure read-only batches or `agent_delegate` batches <= 4).
            let mut step_calls = round.tool_calls;
            let step_text = round.content;
            let step_thinking = round.thinking_summary;
            let all_read_only = step_calls
                .iter()
                .all(|c| ToolCatalog::is_parallel_safe_read_only(&c.name));
            let all_delegates = step_calls.iter().all(|c| c.name == "agent_delegate");

            for c in &step_calls {
                on_progress(RunProgressEvent::ToolStarted {
                    call_id: c.id.clone(),
                    name: c.name.clone(),
                    arguments_json: c.arguments_json.clone(),
                });
            }

            activity.tick();
            let t_tools = Instant::now();
            let outputs: Vec<ToolOutput> = if all_delegates && step_calls.len() > 1 {
                let pairs: Vec<(String, String)> = step_calls
                    .iter()
                    .map(|c| (c.id.clone(), c.arguments_json.clone()))
                    .collect();
                HarnessExecutor::execute_parallel_delegates(harness_ctx, &pairs).await
            } else if all_read_only && step_calls.len() > 1 {
                let futs = step_calls.iter().map(|c| {
                    HarnessExecutor::execute_tool(harness_ctx, &c.id, &c.name, &c.arguments_json)
                });
                futures_util::future::join_all(futs).await
            } else {
                let mut seq_out = Vec::with_capacity(step_calls.len());
                for c in &step_calls {
                    let out = HarnessExecutor::execute_tool(
                        harness_ctx,
                        &c.id,
                        &c.name,
                        &c.arguments_json,
                    )
                    .await;
                    let stopped = out.stopped_by_user;
                    seq_out.push(out);
                    if stopped {
                        break;
                    }
                }
                seq_out
            };
            let step_ms = t_tools.elapsed().as_millis() as i64;
            activity.tick();
            let mut stopped = false;
            for (idx, (call, out)) in step_calls.iter_mut().zip(outputs).enumerate() {
                call.starts_tool_step = idx == 0;
                if idx == 0 {
                    call.preceding_text = step_text.clone();
                    call.preceding_thinking = step_thinking.clone();
                }
                call.result_text = out.text;
                call.is_error = out.is_error;
                call.duration_ms = step_ms;
                call.screenshot_jpeg_base64 = out.screenshot_jpeg_base64;
                if out.stopped_by_user {
                    stopped = true;
                }
                let result_preview: String = call.result_text.chars().take(400).collect();
                on_progress(RunProgressEvent::ToolFinished {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    is_error: call.is_error,
                    duration_ms: call.duration_ms,
                    result_preview,
                });
            }

            all_tool_calls.extend(step_calls.clone());

            // Append synthetic assistant step into owned history so the next LLM round sees tool results.
            owned_history
                .get_or_insert_with(|| history_nodes.to_vec())
                .push(MessageNode {
                    id: format!("tool-round-{round_idx}"),
                    role: MessageRole::Assistant,
                    variants: vec![MessageVariant {
                        id: format!("tool-round-{round_idx}-v0"),
                        content: String::new(),
                        thinking_summary: step_thinking,
                        thinking_signature: last_signature.clone(),
                        tool_calls: step_calls,
                        model_used: model.to_string(),
                        provider_protocol: Some(final_dialect.protocol_family()),
                        provider_wire_dialect: final_dialect.wire_name().to_string(),
                        ..Default::default()
                    }],
                    active_variant_index: 0,
                    included_in_context: true,
                    pinned: false,
                });

            if stopped {
                return Err(ProtocolError::Cancelled);
            }
        }

        let wait_ms_during = harness_ctx
            .broker
            .total_wait_ms()
            .saturating_sub(wait_ms_before);
        let elapsed_ms =
            (activity.active_duration().as_millis() as u64).saturating_sub(wait_ms_during) as i64;
        // Strip transient screenshots before returning persistent `MessageVariant`.
        for tc in &mut all_tool_calls {
            tc.screenshot_jpeg_base64.clear();
        }
        let final_goal = harness_ctx.goal_state.lock().await.clone();
        let goal_tracker = (!final_goal.todos.is_empty() || !final_goal.goal_objective.is_empty())
            .then_some(final_goal);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let variant = MessageVariant {
            id: format!("v-{owner_run_id}"),
            content: final_content,
            thinking_summary: accumulated_thinking,
            thinking_signature: last_signature,
            thinking_duration_ms: total_thinking_duration_ms,
            model_used: model.to_string(),
            created_at: now_ms,
            provider_protocol: Some(profile.protocol),
            provider_wire_dialect: final_dialect.wire_name().to_string(),
            provider_interaction_id: last_interaction_id,
            token_metrics: accumulated_metrics,
            raw_request_preview: last_raw_request_preview,
            raw_response_preview: last_raw_response_preview,
            tool_calls: all_tool_calls,
            goal_tracker,
            web_sources: all_citations,
            api_usage_records: all_usage.clone(),
            ..Default::default()
        };
        Ok(TurnRunOutcome {
            dialect_used: final_dialect,
            variant,
            usage_records: all_usage,
            elapsed_ms,
        })
    }
}

struct ProgressBridge<'a, F: FnMut(RunProgressEvent)>(&'a mut F);

impl<F: FnMut(RunProgressEvent)> StreamCallbacks for ProgressBridge<'_, F> {
    fn on_text_delta(&mut self, delta: &str) {
        (self.0)(RunProgressEvent::TextDelta(delta.to_string()));
    }

    fn on_thinking_delta(&mut self, delta: &str) {
        (self.0)(RunProgressEvent::ThinkingDelta(delta.to_string()));
    }

    fn on_status(&mut self, status: &str) {
        (self.0)(RunProgressEvent::Status(status.to_string()));
    }

    fn on_retry_wait(&mut self, attempt: usize, wait_ms: u64, reason: &str) {
        (self.0)(RunProgressEvent::RetryWait {
            attempt,
            wait_ms,
            reason: reason.to_string(),
        });
    }
}
