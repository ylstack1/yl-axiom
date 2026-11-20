use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::usage::{
    ChargePricing, ContextPricingTier, TokenMetrics, TokenPricing, legacy_token_pricing,
};

pub const CUSTOM_DIALECT_OPENAI_CHAT: &str = "openai_chat";
pub const CUSTOM_DIALECT_OPENAI_RESPONSES: &str = "openai_responses";
pub const CUSTOM_DIALECT_ANTHROPIC: &str = "anthropic_messages";
pub const CUSTOM_DIALECT_GEMINI: &str = "gemini_interactions";

pub const CUSTOM_DIALECTS: [&str; 4] = [
    CUSTOM_DIALECT_OPENAI_CHAT,
    CUSTOM_DIALECT_OPENAI_RESPONSES,
    CUSTOM_DIALECT_ANTHROPIC,
    CUSTOM_DIALECT_GEMINI,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderProtocol {
    #[serde(rename = "GEMINI_INTERACTIONS")]
    GeminiInteractions,
    #[serde(rename = "OPENAI_RESPONSES", alias = "BYTEDANCE_ARK")]
    OpenAiResponses,
    #[serde(rename = "ANTHROPIC_MESSAGES")]
    AnthropicMessages,
    #[serde(rename = "CUSTOM")]
    Custom,
}

impl ProviderProtocol {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::GeminiInteractions => "Google Gemini API",
            Self::OpenAiResponses => "OpenAI / Universal Compatible API",
            Self::AnthropicMessages => "Anthropic Messages API",
            Self::Custom => "Custom Protocol",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::GeminiInteractions => "https://generativelanguage.googleapis.com",
            Self::OpenAiResponses => "https://api.openai.com",
            Self::AnthropicMessages => "https://api.anthropic.com",
            Self::Custom => "",
        }
    }

    pub fn default_endpoint_path(self) -> &'static str {
        match self {
            Self::GeminiInteractions => "/v1beta/interactions",
            Self::OpenAiResponses => "/v1/responses",
            Self::AnthropicMessages => "/v1/messages",
            Self::Custom => "",
        }
    }

    pub fn is_custom_wire(self) -> bool {
        self == Self::Custom
    }

    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::GeminiInteractions => "GEMINI_INTERACTIONS",
            Self::OpenAiResponses => "OPENAI_RESPONSES",
            Self::AnthropicMessages => "ANTHROPIC_MESSAGES",
            Self::Custom => "CUSTOM",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "GEMINI_INTERACTIONS" => Some(Self::GeminiInteractions),
            "OPENAI_RESPONSES" | "BYTEDANCE_ARK" => Some(Self::OpenAiResponses),
            "ANTHROPIC_MESSAGES" => Some(Self::AnthropicMessages),
            "CUSTOM" => Some(Self::Custom),
            _ => None,
        }
    }
}

pub fn map_protocol_to_dialect(protocol_name: &str) -> &'static str {
    match protocol_name {
        "GEMINI_INTERACTIONS" => CUSTOM_DIALECT_GEMINI,
        "ANTHROPIC_MESSAGES" => CUSTOM_DIALECT_ANTHROPIC,
        "OPENAI_RESPONSES" => CUSTOM_DIALECT_OPENAI_RESPONSES,
        _ => CUSTOM_DIALECT_OPENAI_CHAT,
    }
}

pub fn custom_dialect_label(dialect: &str) -> &'static str {
    match dialect {
        CUSTOM_DIALECT_OPENAI_CHAT => "OpenAI Chat Completions",
        CUSTOM_DIALECT_OPENAI_RESPONSES => "OpenAI Responses",
        CUSTOM_DIALECT_ANTHROPIC => "Anthropic Messages",
        CUSTOM_DIALECT_GEMINI => "Gemini Interactions",
        _ => "Unresolved",
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthHeaderMode {
    #[default]
    #[serde(rename = "NATIVE")]
    Native,
    #[serde(rename = "BEARER_ONLY")]
    BearerOnly,
    #[serde(rename = "QUERY_PARAM")]
    QueryParam,
}

fn default_context_window() -> i32 {
    128_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingSpec {
    pub model_id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub input_price_per_m: f64,
    #[serde(default)]
    pub output_price_per_m: f64,
    #[serde(default)]
    pub cache_read_price_per_m: f64,
    #[serde(default)]
    pub cache_write_price_per_m: f64,
    #[serde(default)]
    pub reasoning_price_per_m: f64,
    #[serde(default)]
    pub known_pricing: Option<TokenPricing>,
    #[serde(default)]
    pub context_pricing_tiers: Vec<ContextPricingTier>,
    #[serde(default)]
    pub charge_pricing: ChargePricing,
    #[serde(default = "default_context_window")]
    pub context_window_tokens: i32,
    #[serde(default)]
    pub max_output_tokens: Option<i32>,
    #[serde(default)]
    pub output_modalities: Vec<String>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
}

fn default_true() -> bool {
    true
}
fn default_timeout_minutes() -> i32 {
    60
}
fn default_aspect_ratio() -> String {
    "1:1".to_string()
}
fn default_media_resolution() -> String {
    "1K".to_string()
}
fn default_video_duration() -> i32 {
    5
}
fn default_audio_voice() -> String {
    "alloy".to_string()
}
fn default_audio_speed() -> f64 {
    1.0
}
fn default_thinking_effort() -> String {
    "none".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub endpoint_path_override: String,
    /// Custom wire dialect when `protocol == ProviderProtocol::Custom`. Empty = unresolved (Send blocked).
    #[serde(default)]
    pub custom_dialect: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub extra_api_keys: Vec<String>,
    #[serde(default)]
    pub auth_header_mode: AuthHeaderMode,
    #[serde(default)]
    pub custom_headers_raw: String,
    #[serde(default)]
    pub default_model: String,
    #[serde(default)]
    pub saved_models: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_fetch_models: bool,
    #[serde(default = "default_timeout_minutes")]
    pub timeout_minutes: i32,
    /// Private by default: server-side conversation state is off (`store=false`).
    #[serde(default)]
    pub use_stateful_chaining: bool,
    /// Encoded unconditionally so legacy profiles lacking this key remain distinguishable.
    #[serde(default)]
    pub store_server_side: bool,
    #[serde(default = "default_true")]
    pub use_background_long_decode: bool,
    #[serde(default = "default_true")]
    pub use_legacy_fallback_endpoint: bool,
    #[serde(default)]
    pub enable_web_search: bool,
    #[serde(default)]
    pub enable_code_execution: bool,
    #[serde(default)]
    pub enable_url_context: bool,
    #[serde(default)]
    pub enable_image_generation: bool,
    #[serde(default)]
    pub enable_video_generation: bool,
    /// Legacy import field only; local shell authority belongs to assistant/chat permissions.
    #[serde(default)]
    pub allow_shell_execution: bool,
    #[serde(default = "default_aspect_ratio")]
    pub media_aspect_ratio: String,
    #[serde(default = "default_media_resolution")]
    pub media_resolution: String,
    #[serde(default = "default_video_duration")]
    pub video_duration_seconds: i32,
    #[serde(default = "default_true")]
    pub video_generate_audio: bool,
    #[serde(default = "default_audio_voice")]
    pub audio_voice: String,
    #[serde(default = "default_audio_speed")]
    pub audio_speed: f64,
    #[serde(default = "default_thinking_effort")]
    pub thinking_effort: String,
    #[serde(default)]
    pub thinking_budget_tokens: Option<i32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_output_tokens: Option<i32>,
    #[serde(default = "default_context_window")]
    pub context_window_tokens: i32,
    #[serde(default)]
    pub input_price_per_m: f64,
    #[serde(default)]
    pub output_price_per_m: f64,
    #[serde(default)]
    pub cache_read_price_per_m: f64,
    #[serde(default)]
    pub cache_write_price_per_m: f64,
    #[serde(default)]
    pub reasoning_price_per_m: f64,
    #[serde(default)]
    pub known_pricing: Option<TokenPricing>,
    #[serde(default)]
    pub pricing_model_id: String,
    #[serde(default)]
    pub custom_model_specs: BTreeMap<String, ModelPricingSpec>,
    #[serde(default)]
    pub anthropic_beta_headers: String,
    #[serde(default)]
    pub raw_json_extra_body: String,
}

impl ProviderProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>, protocol: ProviderProtocol) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            protocol,
            enabled: true,
            base_url: protocol.default_base_url().to_string(),
            endpoint_path_override: String::new(),
            custom_dialect: String::new(),
            api_key: String::new(),
            extra_api_keys: Vec::new(),
            auth_header_mode: AuthHeaderMode::Native,
            custom_headers_raw: String::new(),
            default_model: String::new(),
            saved_models: Vec::new(),
            auto_fetch_models: true,
            timeout_minutes: 10,
            use_stateful_chaining: false,
            store_server_side: false,
            use_background_long_decode: true,
            use_legacy_fallback_endpoint: true,
            enable_web_search: false,
            enable_code_execution: false,
            enable_url_context: false,
            enable_image_generation: false,
            enable_video_generation: false,
            allow_shell_execution: false,
            media_aspect_ratio: default_aspect_ratio(),
            media_resolution: default_media_resolution(),
            video_duration_seconds: 8,
            video_generate_audio: true,
            audio_voice: default_audio_voice(),
            audio_speed: 1.0,
            thinking_effort: default_thinking_effort(),
            thinking_budget_tokens: None,
            temperature: None,
            top_p: None,
            max_output_tokens: None,
            context_window_tokens: 128_000,
            input_price_per_m: 0.0,
            output_price_per_m: 0.0,
            cache_read_price_per_m: 0.0,
            cache_write_price_per_m: 0.0,
            reasoning_price_per_m: 0.0,
            known_pricing: None,
            pricing_model_id: String::new(),
            custom_model_specs: BTreeMap::new(),
            anthropic_beta_headers: String::new(),
            raw_json_extra_body: String::new(),
        }
    }

    /// Returns all unique non-empty API keys configured on this provider (primary first, then extras).
    pub fn all_api_keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        let primary = self.api_key.trim();
        if !primary.is_empty() {
            out.push(primary.to_string());
        }
        for extra in &self.extra_api_keys {
            let t = extra.trim();
            if !t.is_empty() && !out.iter().any(|k| k == t) {
                out.push(t.to_string());
            }
        }
        out
    }

    pub fn has_any_api_key(&self) -> bool {
        !self.api_key.trim().is_empty() || self.extra_api_keys.iter().any(|k| !k.trim().is_empty())
    }

    pub fn add_api_key(&mut self, key: String) -> bool {
        let t = key.trim();
        if t.is_empty() {
            return false;
        }
        if self.api_key.trim().is_empty() {
            self.api_key = t.to_string();
            return true;
        }
        if self.api_key.trim() == t || self.extra_api_keys.iter().any(|k| k.trim() == t) {
            return false;
        }
        self.extra_api_keys.push(t.to_string());
        true
    }

    pub fn remove_extra_api_key(&mut self, extra_idx: usize) {
        if extra_idx < self.extra_api_keys.len() {
            self.extra_api_keys.remove(extra_idx);
        }
    }

    pub fn promote_extra_api_key(&mut self, extra_idx: usize) {
        if extra_idx >= self.extra_api_keys.len() {
            return;
        }
        let promoted = self.extra_api_keys.remove(extra_idx);
        let old_primary = self.api_key.trim().to_string();
        self.api_key = promoted;
        if !old_primary.is_empty() {
            self.extra_api_keys.insert(0, old_primary);
        }
    }

    pub fn with_api_key(&self, key: &str) -> Self {
        let mut cloned = self.clone();
        cloned.api_key = key.to_string();
        cloned
    }

    /// Chaining depends on stored responses, so it implies storage; storage alone does not chain.
    pub fn keeps_provider_state(&self) -> bool {
        self.store_server_side || self.use_stateful_chaining
    }
    pub fn effective_base_url(&self) -> &str {
        if self.base_url.trim().is_empty() {
            self.protocol.default_base_url()
        } else {
            self.base_url.as_str()
        }
    }

    pub fn resolve_wire_dialect(&self) -> &'static str {
        match self.protocol {
            ProviderProtocol::GeminiInteractions => CUSTOM_DIALECT_GEMINI,
            ProviderProtocol::OpenAiResponses => CUSTOM_DIALECT_OPENAI_RESPONSES,
            ProviderProtocol::AnthropicMessages => CUSTOM_DIALECT_ANTHROPIC,
            ProviderProtocol::Custom => match self.custom_dialect.as_str() {
                CUSTOM_DIALECT_OPENAI_CHAT => CUSTOM_DIALECT_OPENAI_CHAT,
                CUSTOM_DIALECT_OPENAI_RESPONSES => CUSTOM_DIALECT_OPENAI_RESPONSES,
                CUSTOM_DIALECT_ANTHROPIC => CUSTOM_DIALECT_ANTHROPIC,
                CUSTOM_DIALECT_GEMINI => CUSTOM_DIALECT_GEMINI,
                _ => "",
            },
        }
    }

    pub fn token_pricing(&self, model_id: &str) -> TokenPricing {
        let model = self.custom_model_specs.get(model_id);
        if let Some(known) = model.and_then(|m| m.known_pricing.as_ref()) {
            return known.clone();
        }
        let base = if !self.pricing_model_id.trim().is_empty() && self.pricing_model_id != model_id
        {
            TokenPricing::default()
        } else if let Some(known) = &self.known_pricing {
            known.clone()
        } else {
            legacy_token_pricing(
                self.input_price_per_m,
                self.output_price_per_m,
                self.cache_read_price_per_m,
                self.cache_write_price_per_m,
                self.reasoning_price_per_m,
            )
        };
        match model {
            None => base,
            Some(m) => legacy_token_pricing(
                m.input_price_per_m,
                m.output_price_per_m,
                m.cache_read_price_per_m,
                m.cache_write_price_per_m,
                m.reasoning_price_per_m,
            )
            .with_missing_from(&base),
        }
    }

    pub fn calculate_turn_cost_usd(&self, metrics: &TokenMetrics) -> f64 {
        self.token_pricing(&self.default_model)
            .estimate(metrics, None, None)
            .unwrap_or(0.0)
    }
}
