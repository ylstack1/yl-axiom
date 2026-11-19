use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::chat::{CharacterOrScenarioCard, ChatSession, ContextFragment, ViewMode, WorkspaceMode};
use crate::permissions::AgentPermissionPolicy;
use crate::provider::ProviderProfile;

fn default_true() -> bool {
    true
}
fn default_high_effort() -> String {
    "high".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPromptPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub authors_note_default: String,
    #[serde(default)]
    pub suggested_temperature: Option<f64>,
    #[serde(default)]
    pub suggested_top_p: Option<f64>,
    #[serde(default = "default_high_effort")]
    pub suggested_thinking_effort: String,
}

fn default_authors_note_depth() -> i32 {
    2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantAgent {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon_emoji: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub authors_note: String,
    #[serde(default = "default_authors_note_depth")]
    pub authors_note_depth: i32,
    #[serde(default)]
    pub default_profile_id: String,
    #[serde(default)]
    pub default_model_id: String,
    #[serde(default)]
    pub thinking_effort: String,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_output_tokens: Option<i32>,
    #[serde(default)]
    pub context_message_limit: i32,
    #[serde(default)]
    pub active_card: Option<CharacterOrScenarioCard>,
    #[serde(default)]
    pub fragments: Vec<ContextFragment>,
    #[serde(default)]
    pub workspace_path: String,
    #[serde(default)]
    pub workspace_mode: WorkspaceMode,
    #[serde(default)]
    pub permission_defaults: Option<AgentPermissionPolicy>,
    #[serde(default)]
    pub enable_web_search: bool,
}

fn default_title_prompt() -> String {
    "Generate a concise, natural 3 to 6 word title summarizing this conversation. Return ONLY the title text without quotes or trailing punctuation.".to_string()
}
fn default_translation_lang() -> String {
    "English".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModelConfig {
    #[serde(default)]
    pub global_profile_id: String,
    #[serde(default)]
    pub global_model_id: String,
    #[serde(default = "default_true")]
    pub title_follow_global: bool,
    #[serde(default)]
    pub title_profile_id: String,
    #[serde(default)]
    pub title_model_id: String,
    #[serde(default = "default_title_prompt")]
    pub title_prompt_template: String,
    #[serde(default = "default_true")]
    pub summary_follow_global: bool,
    #[serde(default)]
    pub summary_profile_id: String,
    #[serde(default)]
    pub summary_model_id: String,
    #[serde(default = "default_true")]
    pub translation_follow_global: bool,
    #[serde(default)]
    pub translation_profile_id: String,
    #[serde(default)]
    pub translation_model_id: String,
    #[serde(default = "default_translation_lang")]
    pub translation_target_language: String,
}

impl Default for DefaultModelConfig {
    fn default() -> Self {
        Self {
            global_profile_id: String::new(),
            global_model_id: String::new(),
            title_follow_global: true,
            title_profile_id: String::new(),
            title_model_id: String::new(),
            title_prompt_template: default_title_prompt(),
            summary_follow_global: true,
            summary_profile_id: String::new(),
            summary_model_id: String::new(),
            translation_follow_global: true,
            translation_profile_id: String::new(),
            translation_model_id: String::new(),
            translation_target_language: default_translation_lang(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageNavVisibility {
    #[serde(rename = "ALWAYS")]
    Always,
    #[default]
    #[serde(rename = "WHILE_SCROLLING")]
    WhileScrolling,
    #[serde(rename = "HIDDEN")]
    Hidden,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendShortcutMode {
    #[default]
    #[serde(rename = "ENTER")]
    Enter,
    #[serde(rename = "CTRL_ENTER")]
    CtrlEnter,
    #[serde(rename = "SHIFT_ENTER")]
    ShiftEnter,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub command: String,
    pub bearer_token: String,
    pub enabled: bool,
}

fn default_paste_threshold() -> i32 {
    5000
}
fn default_overlay_opacity() -> i32 {
    100
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesConfig {
    #[serde(default)]
    pub default_view_mode: ViewMode,
    #[serde(default = "default_true")]
    pub show_provider_in_model_capsule: bool,
    #[serde(default)]
    pub reduce_motion: bool,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub chat_spend_limit_usd: f64,
    #[serde(default)]
    pub profile_name: String,
    #[serde(default)]
    pub profile_picture_path: String,
    #[serde(default = "default_true")]
    pub show_user_avatar: bool,
    #[serde(default = "default_true")]
    pub show_user_name: bool,
    #[serde(default = "default_true")]
    pub show_user_timestamp: bool,
    #[serde(default = "default_true")]
    pub show_user_message_actions: bool,
    #[serde(default = "default_true")]
    pub show_model_icon: bool,
    #[serde(default = "default_true")]
    pub show_assistant_avatar_in_title_bar: bool,
    #[serde(default = "default_true")]
    pub show_model_name: bool,
    #[serde(default = "default_true")]
    pub show_model_timestamp: bool,
    #[serde(default = "default_true")]
    pub show_provider_after_model_name: bool,
    #[serde(default = "default_true")]
    pub show_token_and_context_stats: bool,
    #[serde(default)]
    pub show_tokens_for_entire_turn: bool,
    #[serde(default = "default_true")]
    pub show_thinking_cards: bool,
    #[serde(default = "default_true")]
    pub show_tool_cards: bool,
    #[serde(default = "default_true")]
    pub show_files_below_replies: bool,
    #[serde(default = "default_true")]
    pub show_reasoning_level_on_button: bool,
    #[serde(default = "default_true")]
    pub inline_dollar_math_rendering: bool,
    #[serde(default = "default_true")]
    pub math_formula_rendering: bool,
    #[serde(default = "default_true")]
    pub render_user_messages_with_markdown: bool,
    #[serde(default = "default_true")]
    pub render_reasoning_with_markdown: bool,
    #[serde(default = "default_true")]
    pub render_assistant_messages_with_markdown: bool,
    #[serde(default)]
    pub auto_collapse_code_blocks: bool,
    #[serde(default)]
    pub wrap_code_blocks: bool,
    #[serde(default = "default_true")]
    pub auto_switch_to_topics: bool,
    #[serde(default = "default_true")]
    pub auto_collapse_thinking: bool,
    #[serde(default)]
    pub collapse_thinking_steps: bool,
    #[serde(default = "default_true")]
    pub show_tool_result_summary: bool,
    #[serde(default)]
    pub collapse_long_messages: bool,
    #[serde(default = "default_true")]
    pub delete_messages_below_when_regenerating: bool,
    #[serde(default)]
    pub delete_previous_message_when_regenerating: bool,
    #[serde(default)]
    pub confirm_before_regenerating: bool,
    #[serde(default)]
    pub confirm_before_deleting_chat: bool,
    #[serde(default = "default_true")]
    pub keep_message_versions_when_forking: bool,
    #[serde(default = "default_true")]
    pub show_chat_list_dates: bool,
    #[serde(default)]
    pub new_chat_when_switching_assistants: bool,
    #[serde(default = "default_true")]
    pub new_chat_after_deleting_topic: bool,
    #[serde(default)]
    pub new_chat_on_launch: bool,
    #[serde(default = "default_true")]
    pub sidebar_open: bool,
    #[serde(default)]
    pub message_nav_buttons: MessageNavVisibility,
    #[serde(default)]
    pub send_shortcut: SendShortcutMode,
    #[serde(default)]
    pub custom_shortcuts: BTreeMap<String, String>,
    #[serde(default)]
    pub paste_long_text_as_file: bool,
    #[serde(default = "default_paste_threshold")]
    pub paste_conversion_threshold_chars: i32,
    #[serde(default = "default_true")]
    pub auto_scroll_to_bottom_on_open: bool,
    #[serde(default = "default_true")]
    pub auto_follow_bottom: bool,
    #[serde(default = "default_true")]
    pub auto_compact_when_near_context_limit: bool,
    #[serde(default = "default_overlay_opacity")]
    pub chat_background_overlay_opacity_percent: i32,
}

impl Default for PreferencesConfig {
    fn default() -> Self {
        Self {
            default_view_mode: ViewMode::EditorialTurns,
            show_provider_in_model_capsule: true,
            reduce_motion: false,
            mcp_servers: Vec::new(),
            chat_spend_limit_usd: 0.0,
            profile_name: String::new(),
            profile_picture_path: String::new(),
            show_user_avatar: true,
            show_user_name: true,
            show_user_timestamp: true,
            show_user_message_actions: true,
            show_model_icon: true,
            show_assistant_avatar_in_title_bar: true,
            show_model_name: true,
            show_model_timestamp: true,
            show_provider_after_model_name: true,
            show_token_and_context_stats: true,
            show_tokens_for_entire_turn: false,
            show_thinking_cards: true,
            show_tool_cards: true,
            show_files_below_replies: true,
            show_reasoning_level_on_button: true,
            inline_dollar_math_rendering: true,
            math_formula_rendering: true,
            render_user_messages_with_markdown: true,
            render_reasoning_with_markdown: true,
            render_assistant_messages_with_markdown: true,
            auto_collapse_code_blocks: false,
            wrap_code_blocks: false,
            auto_switch_to_topics: true,
            auto_collapse_thinking: true,
            collapse_thinking_steps: false,
            show_tool_result_summary: true,
            collapse_long_messages: false,
            delete_messages_below_when_regenerating: true,
            delete_previous_message_when_regenerating: false,
            confirm_before_regenerating: false,
            confirm_before_deleting_chat: false,
            keep_message_versions_when_forking: true,
            show_chat_list_dates: true,
            new_chat_when_switching_assistants: false,
            new_chat_after_deleting_topic: true,
            new_chat_on_launch: false,
            sidebar_open: true,
            message_nav_buttons: MessageNavVisibility::default(),
            send_shortcut: SendShortcutMode::default(),
            custom_shortcuts: BTreeMap::new(),
            paste_long_text_as_file: false,
            paste_conversion_threshold_chars: 5000,
            auto_scroll_to_bottom_on_open: true,
            auto_follow_bottom: true,
            auto_compact_when_near_context_limit: true,
            chat_background_overlay_opacity_percent: 100,
        }
    }
}

fn default_width_dp() -> i32 {
    1400
}
fn default_height_dp() -> i32 {
    900
}
fn default_neg_one() -> i32 {
    -1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWindowConfig {
    #[serde(default = "default_true")]
    pub is_maximized: bool,
    #[serde(default = "default_width_dp")]
    pub width_dp: i32,
    #[serde(default = "default_height_dp")]
    pub height_dp: i32,
    #[serde(default = "default_neg_one")]
    pub x_dp: i32,
    #[serde(default = "default_neg_one")]
    pub y_dp: i32,
}

impl Default for DesktopWindowConfig {
    fn default() -> Self {
        Self {
            is_maximized: true,
            width_dp: 1400,
            height_dp: 900,
            x_dp: -1,
            y_dp: -1,
        }
    }
}

fn default_theme_name() -> String {
    "Parchment & Espresso Ink (Sepia)".to_string()
}
fn default_color_mode() -> String {
    "Sepia".to_string()
}
fn default_canvas() -> String {
    "#F4ECD8".to_string()
}
fn default_sheet() -> String {
    "#EBE0C5".to_string()
}
fn default_card() -> String {
    "#FAF6EB".to_string()
}
fn default_elevated() -> String {
    "#EFE5CC".to_string()
}
fn default_border() -> String {
    "#D5C4A1".to_string()
}
fn default_primary_ink() -> String {
    "#261C14".to_string()
}
fn default_dialogue_ink() -> String {
    "#4A2511".to_string()
}
fn default_narration_ink() -> String {
    "#5C4938".to_string()
}
fn default_muted_ink() -> String {
    "#826F5C".to_string()
}
fn default_accent() -> String {
    "#8B5A2B".to_string()
}
fn default_accent_secondary() -> String {
    "#B57C2E".to_string()
}
fn default_danger() -> String {
    "#A62B2B".to_string()
}
fn default_prose_font() -> String {
    "Inter".to_string()
}
fn default_code_font() -> String {
    "JetBrains Mono".to_string()
}
fn default_font_sp() -> f32 {
    14.0
}
fn default_line_height() -> f32 {
    1.54
}
fn default_para_spacing() -> f32 {
    18.0
}
fn default_reading_width() -> f32 {
    820.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConfig {
    #[serde(default = "default_theme_name")]
    pub name: String,
    #[serde(default = "default_color_mode")]
    pub color_mode: String,
    #[serde(default = "default_canvas")]
    pub canvas_bg_hex: String,
    #[serde(default = "default_sheet")]
    pub sheet_bg_hex: String,
    #[serde(default = "default_card")]
    pub card_bg_hex: String,
    #[serde(default = "default_elevated")]
    pub elevated_bg_hex: String,
    #[serde(default = "default_border")]
    pub border_hex: String,
    #[serde(default = "default_primary_ink")]
    pub primary_ink_hex: String,
    #[serde(default = "default_dialogue_ink")]
    pub dialogue_ink_hex: String,
    #[serde(default = "default_narration_ink")]
    pub narration_ink_hex: String,
    #[serde(default = "default_muted_ink")]
    pub muted_ink_hex: String,
    #[serde(default = "default_accent")]
    pub accent_hex: String,
    #[serde(default = "default_accent_secondary")]
    pub accent_secondary_hex: String,
    #[serde(default = "default_danger")]
    pub danger_hex: String,
    #[serde(default = "default_prose_font")]
    pub prose_font_family: String,
    #[serde(default)]
    pub custom_prose_font_path: String,
    #[serde(default = "default_code_font")]
    pub code_font_family: String,
    #[serde(default)]
    pub custom_code_font_path: String,
    #[serde(default = "default_font_sp")]
    pub prose_font_size_sp: f32,
    #[serde(default = "default_font_sp")]
    pub code_font_size_sp: f32,
    #[serde(default = "default_line_height")]
    pub prose_line_height_multiplier: f32,
    #[serde(default = "default_para_spacing")]
    pub paragraph_spacing_dp: f32,
    #[serde(default)]
    pub first_line_indent_chars: i32,
    #[serde(default = "default_reading_width")]
    pub max_reading_width_dp: f32,
    #[serde(default = "default_true")]
    pub highlight_dialogue_and_narration: bool,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: default_theme_name(),
            color_mode: default_color_mode(),
            canvas_bg_hex: default_canvas(),
            sheet_bg_hex: default_sheet(),
            card_bg_hex: default_card(),
            elevated_bg_hex: default_elevated(),
            border_hex: default_border(),
            primary_ink_hex: default_primary_ink(),
            dialogue_ink_hex: default_dialogue_ink(),
            narration_ink_hex: default_narration_ink(),
            muted_ink_hex: default_muted_ink(),
            accent_hex: default_accent(),
            accent_secondary_hex: default_accent_secondary(),
            danger_hex: default_danger(),
            prose_font_family: default_prose_font(),
            custom_prose_font_path: String::new(),
            code_font_family: default_code_font(),
            custom_code_font_path: String::new(),
            prose_font_size_sp: 14.0,
            code_font_size_sp: 14.0,
            prose_line_height_multiplier: 1.54,
            paragraph_spacing_dp: 18.0,
            first_line_indent_chars: 0,
            max_reading_width_dp: 820.0,
            highlight_dialogue_and_narration: true,
        }
    }
}

pub const DEFAULT_OTA_GITHUB_REPO: &str = "baros4294/parchment";

fn default_ota_repo() -> String {
    DEFAULT_OTA_GITHUB_REPO.to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateSnapshot {
    #[serde(default)]
    pub profiles: Vec<ProviderProfile>,
    #[serde(default)]
    pub active_profile_id: String,
    #[serde(default)]
    pub assistants: Vec<AssistantAgent>,
    #[serde(default)]
    pub active_assistant_id: String,
    #[serde(default)]
    pub default_models: DefaultModelConfig,
    #[serde(default)]
    pub preferences: PreferencesConfig,
    #[serde(default)]
    pub desktop_window: DesktopWindowConfig,
    #[serde(default)]
    pub sessions: Vec<ChatSession>,
    #[serde(default)]
    pub active_session_id: String,
    #[serde(default)]
    pub open_tab_session_ids: Vec<String>,
    #[serde(default)]
    pub system_prompt_presets: Vec<SystemPromptPreset>,
    #[serde(default)]
    pub saved_cards: Vec<CharacterOrScenarioCard>,
    #[serde(default)]
    pub global_world_book: Vec<ContextFragment>,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default = "default_ota_repo")]
    pub ota_github_repo: String,
    #[serde(default = "default_true")]
    pub auto_check_ota_on_startup: bool,
    #[serde(default = "default_true")]
    pub provider_storage_reset: bool,
    #[serde(default)]
    pub design_defaults_applied: bool,
}

impl Default for AppStateSnapshot {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            active_profile_id: String::new(),
            assistants: Vec::new(),
            active_assistant_id: String::new(),
            default_models: DefaultModelConfig::default(),
            preferences: PreferencesConfig::default(),
            desktop_window: DesktopWindowConfig::default(),
            sessions: Vec::new(),
            active_session_id: String::new(),
            open_tab_session_ids: Vec::new(),
            system_prompt_presets: Vec::new(),
            saved_cards: Vec::new(),
            global_world_book: Vec::new(),
            theme: ThemeConfig::default(),
            ota_github_repo: default_ota_repo(),
            auto_check_ota_on_startup: true,
            provider_storage_reset: true,
            design_defaults_applied: false,
        }
    }
}
