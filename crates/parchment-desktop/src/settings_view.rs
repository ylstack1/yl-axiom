//! Settings categories, provider draft state, and theme preset switcher helpers.

use parchment_core::model::{
    CUSTOM_DIALECT_ANTHROPIC, CUSTOM_DIALECT_GEMINI, CUSTOM_DIALECT_OPENAI_CHAT,
    CUSTOM_DIALECT_OPENAI_RESPONSES, ProviderProfile, ProviderProtocol,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Providers,
    Workspace,
    Appearance,
    Shortcuts,
    BackupAndUpdates,
}

impl SettingsSection {
    pub const ALL: [Self; 5] = [
        Self::Providers,
        Self::Workspace,
        Self::Appearance,
        Self::Shortcuts,
        Self::BackupAndUpdates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Providers => "Providers & models",
            Self::Workspace => "Workspace & permissions",
            Self::Appearance => "Appearance & themes",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::BackupAndUpdates => "Backup & updates",
        }
    }
}

/// Builds a fresh default provider profile directly (no JSON roundtrip, no hardcoded models).
pub fn make_default_profile(protocol: ProviderProtocol) -> ProviderProfile {
    let (id, name) = match protocol {
        ProviderProtocol::OpenAiResponses => ("prof-openai", "OpenAI"),
        ProviderProtocol::AnthropicMessages => ("prof-anthropic", "Anthropic"),
        ProviderProtocol::GeminiInteractions => ("prof-gemini", "Google Gemini"),
        ProviderProtocol::Custom => ("prof-custom", "Custom API"),
    };
    ProviderProfile::new(id, name, protocol)
}

pub fn protocol_plain_label(protocol: ProviderProtocol, custom_dialect: &str) -> &'static str {
    match protocol {
        ProviderProtocol::OpenAiResponses => "OpenAI / Universal",
        ProviderProtocol::AnthropicMessages => "Anthropic Messages",
        ProviderProtocol::GeminiInteractions => "Google Gemini",
        ProviderProtocol::Custom => match custom_dialect {
            CUSTOM_DIALECT_OPENAI_CHAT => "Custom (OpenAI Chat)",
            CUSTOM_DIALECT_OPENAI_RESPONSES => "Custom (OpenAI Responses)",
            CUSTOM_DIALECT_ANTHROPIC => "Custom (Anthropic)",
            CUSTOM_DIALECT_GEMINI => "Custom (Gemini)",
            _ => "Custom (Format required)",
        },
    }
}

pub fn cycle_custom_dialect(current: &str) -> &'static str {
    match current {
        CUSTOM_DIALECT_OPENAI_CHAT => CUSTOM_DIALECT_OPENAI_RESPONSES,
        CUSTOM_DIALECT_OPENAI_RESPONSES => CUSTOM_DIALECT_ANTHROPIC,
        CUSTOM_DIALECT_ANTHROPIC => CUSTOM_DIALECT_GEMINI,
        _ => CUSTOM_DIALECT_OPENAI_CHAT,
    }
}

pub fn cycle_thinking_effort(current: &str) -> &'static str {
    match current {
        "none" | "" => "low",
        "low" => "medium",
        "medium" => "high",
        "high" => "xhigh",
        "xhigh" => "max",
        _ => "none",
    }
}
