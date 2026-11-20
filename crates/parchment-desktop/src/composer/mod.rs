//! Centered composer card deck matching `ManuscriptViewport.kt`, `ComposerControls.kt`,
//! `ParchmentModalsAndNavigation.kt` (`ContextWindowBreakdownPopover`), `desktop-chat.png`,
//! and `desktop-retry.png`:
//! - In-card Retry Wait banner (`Trying again in Ns`, monospace reason, Outline `Retry now`)
//! - Queued follow-up prompts banner (`Send now` / remove)
//! - Collapsible Tools & Permissions drawer (48px-grade folder/shell/web/computer/approval/goal/prefill)
//! - Pending attachment pills + file picker + Voice dictation mic + Modality chips (Image/Video/Audio)
//! - Slash (`/`) command (built-in + `.parchment/commands/*.md`) & `@` mention autocomplete popups
//! - Borderless `Textarea`, Step-stop Thinking Popover (`Off..Max` + `Adaptive`/`Auto` + custom),
//!   7-Category Proportional Stacked Bar Context Breakdown Popover with `SepiaContextGaugeRing`,
//!   and idle `Send` vs streaming `Stop` / `Queue` / `Send now` controls.

use std::rc::Rc;

use crate::icons::{KissIcon, kiss_icon};
use crate::strings;
use crate::theme::{CAT_BRASS, CAT_OCHRE, CAT_SAGE, CAT_SLATE, ParchmentPalette};
use crate::ui_kit::{
    SepiaButtonStyle, aspect_tile, context_gauge_ring_svg, field_label, format_tokens_short,
    format_usd_short, h_divider, icon_button, permission_option_row, sepia_button, sepia_chip,
    spring_pop_enter, step_slider, streaming_pulse_dot, with_tooltip,
};
use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, MouseButton,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, deferred, div, px,
};
use gpui_component::input::{Input, InputState, Textarea, TextareaState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{
    AgentPermissionPolicy, ApiUsageRecord, PromptAttachment, ProviderProfile, ProviderProtocol,
    ShellAccess, TokenMetrics, ToolApprovalMode, WorkspaceMode,
};

mod popovers;

use popovers::*;

#[derive(Debug, Clone)]
pub struct QueuedPrompt {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct RetryBannerInfo {
    pub headline: String,
    pub detail: String,
    pub collapsed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ContextBreakdownInfo {
    pub system_and_persona_tokens: i64,
    pub lorebook_and_memory_tokens: i64,
    pub workspace_and_tools_tokens: i64,
    pub uploaded_files_tokens: i64,
    pub uploaded_files_count: usize,
    pub chat_history_tokens: i64,
    pub active_turns_count: usize,
    pub muted_turns_count: usize,
    pub composer_draft_tokens: i64,
    pub context_limit_tokens: i64,
    pub utility_requests: usize,
    pub utility_cost_usd: f64,
}

impl ContextBreakdownInfo {
    pub fn total_used_tokens(&self) -> i64 {
        self.system_and_persona_tokens
            + self.lorebook_and_memory_tokens
            + self.workspace_and_tools_tokens
            + self.uploaded_files_tokens
            + self.chat_history_tokens
            + self.composer_draft_tokens
    }

    pub fn free_space_tokens(&self) -> i64 {
        if self.context_limit_tokens <= 0 {
            0
        } else {
            (self.context_limit_tokens - self.total_used_tokens()).max(0)
        }
    }
}

#[derive(Debug, Clone)]
pub enum ComposerAction {
    Send,
    Stop,
    RetryNow,
    ToggleRetryCollapse,
    QueuePrompt,
    SteerNow,
    RemoveQueuedPrompt(String),
    SteerQueuedPrompt(String),
    AttachFiles,
    InspectAttachment(PromptAttachment),
    RemoveAttachment(String),
    ToggleVoiceInput,
    ToggleToolsDrawer,
    ToggleThinkingPopover,
    SetThinkingEffort(String),
    ToggleModalityPopover,
    SetImageAspect(String),
    SetImageResolution(String),
    SetVideoAspect(String),
    SetVideoDuration(i32),
    ToggleVideoAudio,
    SetAudioVoice(String),
    SetAudioSpeed(f64),
    ToggleContextPopover,
    ToggleBackgroundUsage,
    OpenPricingSettings,
    ToggleGoalMode,
    TogglePrefillBox,
    ToggleToolsEnabled,
    ResetPermissionOverrides,
    PickWorkspaceFolder,
    RemoveWorkspaceFolder(String),
    ClearWorkspaceFolder,
    SetWorkspaceMode(WorkspaceMode),
    SetShellAccess(ShellAccess),
    ToggleWebAccess,
    ToggleComputerAccess,
    SetApprovalMode(ToolApprovalMode),
    ToggleProviderWebSearch,
    ToggleProviderCodeExec,
    ToggleProviderUrlContext,
    ToggleInlineToolRegistry,
    ToggleBuiltinTool(String, bool),
    DraftMyReply,
    GuideNextReply,
    RunSlashCommand(String),
    InsertAtMention(String),
    CompactContext,
    SummarizeChat,
}

pub type ComposerActionHandler = Rc<dyn Fn(ComposerAction, &mut Window, &mut App)>;

pub struct ComposerRenderProps<'a> {
    pub palette: &'a ParchmentPalette,
    pub composer_input: &'a Entity<TextareaState>,
    pub prefill_input: &'a Entity<TextareaState>,
    pub custom_thinking_input: &'a Entity<InputState>,
    pub draft_text: &'a str,
    pub active_model_id: &'a str,
    pub profile: &'a ProviderProfile,
    pub policy: &'a AgentPermissionPolicy,
    pub has_chat_overrides: bool,
    pub is_streaming: bool,
    pub is_drafting_reply: bool,
    pub is_voice_listening: bool,
    pub live_status_banner: Option<&'a str>,
    pub retry_banner: Option<&'a RetryBannerInfo>,
    pub show_tools_drawer: bool,
    pub show_inline_tool_registry: bool,
    pub show_thinking_popover: bool,
    pub show_modality_popover: bool,
    pub show_context_popover: bool,
    pub show_background_usage: bool,
    pub show_prefill_box: bool,
    pub goal_mode_enabled: bool,
    pub workspace_folder_missing: bool,
    pub show_token_stats: bool,
    pub show_reasoning_on_button: bool,
    pub pending_attachments: &'a [PromptAttachment],
    pub queued_prompts: &'a [QueuedPrompt],
    pub workspace_files: &'a [String],
    pub custom_slash_commands: &'a [(String, String)],
    pub breakdown: &'a ContextBreakdownInfo,
    pub utility_records: &'a [ApiUsageRecord],
    pub session_cost_usd: f64,
    pub has_messages: bool,
    pub on_action: &'a ComposerActionHandler,
}

const SLASH_COMMANDS: [(&str, &str); 7] = [
    ("/plan", "Switch workspace to Read-only planning mode"),
    ("/build", "Switch workspace to Read & write building mode"),
    (
        "/compact",
        "Shorten the conversation by summarizing older turns",
    ),
    ("/summarize", "Update the chat's remembered summary"),
    ("/continue", "Continue the latest assistant reply"),
    ("/rewind", "Remove the most recent turn"),
    ("/clear", "Clear all messages in this chat"),
];

const THINKING_STOPS: [(&str, &str); 6] = [
    ("Off", "none"),
    ("Low", "low"),
    ("Medium", "medium"),
    ("High", "high"),
    ("Extra", "xhigh"),
    ("Max", "max"),
];

const IMAGE_ASPECTS: [&str; 8] = ["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "21:9"];
const IMAGE_RES: [&str; 3] = ["1K", "2K", "4K"];
const VIDEO_ASPECTS: [&str; 6] = ["16:9", "9:16", "1:1", "4:3", "3:4", "21:9"];
const VIDEO_RES: [&str; 3] = ["720p", "1080p", "4K"];
const MIN_VIDEO_SECONDS: i32 = 4;
const MAX_VIDEO_SECONDS: i32 = 15;
const AUDIO_VOICES: [&str; 11] = [
    "alloy", "nova", "echo", "onyx", "shimmer", "coral", "sage", "Puck", "Kore", "Fenrir", "Aoede",
];
const AUDIO_SPEEDS: [f64; 7] = [0.5, 0.75, 0.85, 1.0, 1.25, 1.5, 2.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveModalityKind {
    Text,
    Image,
    Video,
    Audio,
}

fn infer_modality(model_id: &str) -> ActiveModalityKind {
    let low = model_id.trim().to_ascii_lowercase();
    if low.contains("veo")
        || low.contains("sora")
        || low.contains("kling")
        || low.contains("runway")
    {
        ActiveModalityKind::Video
    } else if low.contains("imagen")
        || low.contains("dall-e")
        || low.contains("gpt-image")
        || low.contains("flux")
        || low.contains("image-gen")
    {
        ActiveModalityKind::Image
    } else if low.contains("tts") || low.ends_with("-audio") || low.contains("speech") {
        ActiveModalityKind::Audio
    } else {
        ActiveModalityKind::Text
    }
}

pub fn render_composer_deck(props: ComposerRenderProps<'_>) -> AnyElement {
    let p = props.palette;
    let card = p.card_bg.to_hsla();
    let elev = p.elevated_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let hover_bg = p.hover_tint();

    let has_input = !props.draft_text.trim().is_empty() || !props.pending_attachments.is_empty();
    let modality = infer_modality(props.active_model_id);

    // 0. Retry wait banner rendered INSIDE the top of the composer card (`desktop-retry.png`)
    let retry_in_card = props.retry_banner.map(|rb| {
        let h_retry = props.on_action.clone();
        let h_col = props.on_action.clone();
        v_flex()
            .w_full()
            .p(px(10.0))
            .rounded(px(8.0))
            .bg(elev)
            .border_1()
            .border_color(p.accent.with_alpha(0.55))
            .gap(px(6.0))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        h_flex()
                            .flex_1()
                            .overflow_hidden()
                            .items_center()
                            .gap(px(6.0))
                            .child(streaming_pulse_dot("comp-retry-pulse", accent))
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(rb.headline.clone()),
                            ),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(sepia_button(
                                "comp-retry-now",
                                strings::RETRY_NOW,
                                Some(KissIcon::Refresh),
                                SepiaButtonStyle::Outline,
                                true,
                                false,
                                p,
                                move |_, w, cx| h_retry(ComposerAction::RetryNow, w, cx),
                            ))
                            .child(icon_button(
                                "comp-retry-col",
                                if rb.collapsed {
                                    KissIcon::ChevronDown
                                } else {
                                    KissIcon::ChevronUp
                                },
                                false,
                                p,
                                move |_, w, cx| h_col(ComposerAction::ToggleRetryCollapse, w, cx),
                            )),
                    ),
            )
            .children((!rb.collapsed && !rb.detail.is_empty()).then(|| {
                div()
                    .font_family(p.code_font_family.clone())
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child(rb.detail.clone())
            }))
    });

    // 0b. Live in-composer status banner (`Streaming.mp4` & `Comparision.mp4 02:34`:
    //     `● Thinking (model · 40 words · 1s)…`, `● Drafting User's turn…`, fallback notices)
    let live_status_in_card = props
        .retry_banner
        .is_none()
        .then_some(props.live_status_banner)
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .map(|status_text| {
            h_flex()
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(8.0))
                .bg(elev)
                .border_1()
                .border_color(p.accent.with_alpha(0.45))
                .items_center()
                .gap(px(7.0))
                .child(streaming_pulse_dot("comp-live-status-pulse", accent))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(12.5))
                        .font_medium()
                        .text_color(ink)
                        .child(status_text.to_string()),
                )
        });

    // 0c. Floating Voice Input Listening Pill above composer (`Comparision.mp4 01:49.5`)
    let voice_listening_pill = props.is_voice_listening.then(|| {
        let h_stop_mic = props.on_action.clone();
        let pill = h_flex()
            .id("comp-voice-pill")
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(999.0))
            .bg(elev)
            .border_1()
            .border_color(p.accent.with_alpha(0.65))
            .shadow_md()
            .items_center()
            .gap(px(10.0))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(streaming_pulse_dot("comp-mic-pulse", accent))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_semibold()
                            .text_color(ink)
                            .child("Listening…"),
                    ),
            )
            .child(sepia_button(
                "comp-voice-stop",
                "Stop",
                Some(KissIcon::Stop),
                SepiaButtonStyle::Danger,
                true,
                false,
                p,
                move |_, w, cx| h_stop_mic(ComposerAction::ToggleVoiceInput, w, cx),
            ));
        div()
            .w_full()
            .pb(px(8.0))
            .flex()
            .justify_center()
            .child(spring_pop_enter("voice-pill-anim", pill))
    });

    // 1. Queued prompts list
    let queued_panel = (!props.queued_prompts.is_empty()).then(|| {
        v_flex()
            .w_full()
            .p(px(8.0))
            .rounded(px(8.0))
            .bg(elev)
            .border_1()
            .border_color(border)
            .gap(px(5.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .font_semibold()
                    .text_color(muted)
                    .child(format!(
                        "Queued follow-ups ({})",
                        props.queued_prompts.len()
                    )),
            )
            .children(props.queued_prompts.iter().enumerate().map(|(ix, q)| {
                let id_steer = q.id.clone();
                let id_rem = q.id.clone();
                let h_steer = props.on_action.clone();
                let h_rem = props.on_action.clone();
                h_flex()
                    .id(("queued-item", ix))
                    .w_full()
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(card)
                    .justify_between()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.0))
                            .text_color(ink)
                            .child(q.text.clone()),
                    )
                    .child(
                        h_flex()
                            .gap(px(4.0))
                            .child(sepia_button(
                                ("q-steer", ix),
                                strings::SEND_NOW,
                                None,
                                SepiaButtonStyle::Secondary,
                                true,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_steer(
                                        ComposerAction::SteerQueuedPrompt(id_steer.clone()),
                                        w,
                                        cx,
                                    );
                                },
                            ))
                            .child(icon_button(
                                ("q-rem", ix),
                                KissIcon::Close,
                                false,
                                p,
                                move |_, w, cx| {
                                    h_rem(
                                        ComposerAction::RemoveQueuedPrompt(id_rem.clone()),
                                        w,
                                        cx,
                                    );
                                },
                            )),
                    )
            }))
    });

    // 2. Vertical Tools & Permissions Drawer (`AgentPermissionEditor.kt` 1:1 vertical list)
    let tools_drawer = render_tools_drawer(&props, has_input);

    // 3. Pending attachments strip (click to inspect in viewer, × to remove)
    let attachments_row = (!props.pending_attachments.is_empty()).then(|| {
        h_flex().w_full().flex_wrap().gap(px(6.0)).children(
            props
                .pending_attachments
                .iter()
                .enumerate()
                .map(|(ix, att)| {
                    let id = att.id.clone();
                    let att_open = att.clone();
                    let h_open = props.on_action.clone();
                    let h_rm = props.on_action.clone();
                    h_flex()
                        .id(("comp-att", ix))
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(8.0))
                        .bg(elev)
                        .border_1()
                        .border_color(p.accent.with_alpha(0.55))
                        .items_center()
                        .gap(px(6.0))
                        .cursor_pointer()
                        .on_click(move |_, w, cx| {
                            h_open(ComposerAction::InspectAttachment(att_open.clone()), w, cx)
                        })
                        .child(kiss_icon(KissIcon::Paperclip, 12.0, accent))
                        .child(
                            div()
                                .max_w(px(180.0))
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(ink)
                                .child(att.file_name.clone()),
                        )
                        .child(icon_button(
                            ("comp-att-rm", ix),
                            KissIcon::Close,
                            false,
                            p,
                            move |_, w, cx| {
                                cx.stop_propagation();
                                h_rm(ComposerAction::RemoveAttachment(id.clone()), w, cx);
                            },
                        ))
                }),
        )
    });

    // 4. Optional Assistant Prefill input
    let prefill_row = props.show_prefill_box.then(|| {
        v_flex()
            .w_full()
            .gap(px(4.0))
            .child(field_label(strings::PREFILL_LABEL, p))
            .child(Textarea::new(props.prefill_input).h(px(44.0)))
    });

    // 5. Slash `/` command suggestions popup (built-in + workspace `.parchment/commands/*.md`)
    let trimmed_start = props.draft_text.trim_start();
    let slash_popup = if trimmed_start.starts_with('/')
        && !trimmed_start.contains('\n')
        && !trimmed_start.contains(' ')
    {
        let q = trimmed_start.to_ascii_lowercase();
        let mut matches: Vec<(String, String)> = SLASH_COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(&q))
            .map(|(c, d)| ((*c).to_string(), (*d).to_string()))
            .collect();
        for (cmd, desc) in props.custom_slash_commands {
            if cmd.to_ascii_lowercase().starts_with(&q) {
                matches.push((cmd.clone(), desc.clone()));
            }
        }
        (!matches.is_empty()).then(|| {
            v_flex()
                .w_full()
                .p(px(6.0))
                .rounded(px(10.0))
                .bg(elev)
                .border_1()
                .border_color(border)
                .gap(px(3.0))
                .children(
                    matches
                        .into_iter()
                        .take(10)
                        .enumerate()
                        .map(|(ix, (cmd, desc))| {
                            let h = props.on_action.clone();
                            let c = cmd.clone();
                            h_flex()
                                .id(("slash-opt", ix))
                                .w_full()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded(px(7.0))
                                .bg(card)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .justify_between()
                                .items_center()
                                .gap(px(8.0))
                                .on_click(move |_, w, cx| {
                                    h(ComposerAction::RunSlashCommand(c.clone()), w, cx)
                                })
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .font_family(p.code_font_family.clone())
                                                .text_size(px(12.0))
                                                .font_semibold()
                                                .text_color(accent)
                                                .child(cmd),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(narration)
                                                .child(desc),
                                        ),
                                )
                                .child(kiss_icon(KissIcon::SendEnter, 11.0, muted))
                        }),
                )
        })
    } else {
        None
    };

    // 6. `@` mention suggestions popup
    let at_popup = active_at_query(props.draft_text).and_then(|q| {
        let q_low = q.to_ascii_lowercase();
        let mut items: Vec<(String, String, String)> = Vec::new();
        if q_low.is_empty() || "file".contains(&q_low) || "attach".contains(&q_low) {
            items.push((
                "@file".into(),
                "Attach a local file".into(),
                "__attach__".into(),
            ));
        }
        if q_low.is_empty() || "folder".contains(&q_low) || "mount".contains(&q_low) {
            items.push((
                "@folder".into(),
                "Choose a workspace folder".into(),
                "__mount__".into(),
            ));
        }
        for wf in props
            .workspace_files
            .iter()
            .filter(|f| q_low.is_empty() || f.to_ascii_lowercase().contains(&q_low))
            .take(8)
        {
            items.push((format!("@{wf}"), "Workspace file".into(), format!("@{wf} ")));
        }
        (!items.is_empty()).then(|| {
            v_flex()
                .w_full()
                .p(px(6.0))
                .rounded(px(10.0))
                .bg(elev)
                .border_1()
                .border_color(border)
                .gap(px(3.0))
                .children(items.into_iter().enumerate().map(|(ix, (lbl, desc, val))| {
                    let h = props.on_action.clone();
                    h_flex()
                        .id(("at-opt", ix))
                        .w_full()
                        .px(px(10.0))
                        .py(px(6.0))
                        .rounded(px(7.0))
                        .bg(card)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .justify_between()
                        .items_center()
                        .gap(px(8.0))
                        .on_click(move |_, w, cx| {
                            h(ComposerAction::InsertAtMention(val.clone()), w, cx)
                        })
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .font_family(p.code_font_family.clone())
                                        .text_size(px(12.0))
                                        .font_semibold()
                                        .text_color(accent)
                                        .child(lbl),
                                )
                                .child(div().text_size(px(12.0)).text_color(muted).child(desc)),
                        )
                        .child(kiss_icon(KissIcon::Plus, 11.0, muted))
                }))
        })
    });

    // 7. Interactive Draggable StepSlider Thinking Popover (`ComposerControls.kt` 1:1)
    let raw_effort = props.profile.thinking_effort.trim();
    let effort_display = match raw_effort.to_ascii_lowercase().as_str() {
        "" | "none" => "Off",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra",
        "max" => "Max",
        "adaptive" => "Adaptive",
        "auto" => "Automatic",
        _ => raw_effort,
    };

    let thinking_popover = render_thinking_popover(&props, raw_effort, effort_display);

    // 7b. Modality Popover (`ComposerControls.kt`: AspectTile, Duration StepSlider, Speed StepSlider)
    let modality_popover = render_modality_popover(&props, modality);

    // 8. 7-Category Proportional Stacked Bar `ContextWindowBreakdownPopover` (`ParchmentModalsAndNavigation.kt`)
    let bd = props.breakdown;
    let total_used = bd.total_used_tokens();
    let is_unlimited = bd.context_limit_tokens <= 0;
    let safe_limit = if is_unlimited {
        total_used.max(128_000)
    } else {
        bd.context_limit_tokens
    };
    let ratio = (total_used as f32 / safe_limit.max(1) as f32).clamp(0.0, 1.0);

    let context_popover = render_context_popover(&props, total_used, is_unlimited, safe_limit);

    // 9. Bottom toolbar row (`ComposerToolbar` in `ComposerControls.kt`)
    let thinking_chip_label = if props.show_reasoning_on_button {
        effort_display.to_string()
    } else {
        String::new()
    };
    let thinking_active = !matches!(raw_effort.to_ascii_lowercase().as_str(), "" | "none");
    let has_active_tools = props.policy.workspace_mode != WorkspaceMode::Off
        || props.policy.shell_access == ShellAccess::FullAccess
        || props.policy.web_access
        || props.policy.computer_access
        || props.profile.enable_web_search
        || props.profile.enable_code_execution
        || props.profile.enable_url_context
        || props.goal_mode_enabled
        || props.show_prefill_box;

    let h_att = props.on_action.clone();
    let h_mic = props.on_action.clone();
    let h_mod = props.on_action.clone();
    let h_thk = props.on_action.clone();
    let h_tools = props.on_action.clone();
    let h_ctx = props.on_action.clone();
    let h_send = props.on_action.clone();
    let h_stop = props.on_action.clone();
    let h_queue = props.on_action.clone();
    let h_steer = props.on_action.clone();

    let mut left_controls = h_flex()
        .items_center()
        .flex_wrap()
        .gap(px(6.0))
        .child(with_tooltip(
            sepia_chip(
                "comp-attach-btn",
                "",
                Some(KissIcon::Paperclip),
                !props.pending_attachments.is_empty(),
                p,
                move |_, w, cx| h_att(ComposerAction::AttachFiles, w, cx),
            ),
            "Attach file",
        ))
        .child(with_tooltip(
            sepia_chip(
                "comp-mic-btn",
                "",
                Some(KissIcon::Mic),
                props.is_voice_listening,
                p,
                move |_, w, cx| h_mic(ComposerAction::ToggleVoiceInput, w, cx),
            ),
            if props.is_voice_listening {
                "Stop voice input"
            } else {
                "Voice input"
            },
        ));
    if modality != ActiveModalityKind::Text {
        let (mod_icon, mod_label) = match modality {
            ActiveModalityKind::Image => (
                KissIcon::Image,
                format!(
                    "{} {}",
                    props.profile.media_aspect_ratio, props.profile.media_resolution
                ),
            ),
            ActiveModalityKind::Video => (
                KissIcon::Video,
                format!(
                    "{} {}s",
                    props.profile.media_aspect_ratio, props.profile.video_duration_seconds
                ),
            ),
            ActiveModalityKind::Audio => (
                KissIcon::Audio,
                format!(
                    "{} {:.1}x",
                    props.profile.audio_voice, props.profile.audio_speed
                ),
            ),
            ActiveModalityKind::Text => (KissIcon::Sparkles, String::new()),
        };
        left_controls = left_controls.child(sepia_chip(
            "comp-modality-chip",
            mod_label,
            Some(mod_icon),
            props.show_modality_popover,
            p,
            move |_, w, cx| h_mod(ComposerAction::ToggleModalityPopover, w, cx),
        ));
    }

    left_controls = left_controls
        .child(sepia_chip(
            "comp-thinking-chip",
            thinking_chip_label,
            Some(KissIcon::Brain),
            thinking_active || props.show_thinking_popover,
            p,
            move |_, w, cx| h_thk(ComposerAction::ToggleThinkingPopover, w, cx),
        ))
        .child(sepia_chip(
            "comp-tools-chip",
            if has_active_tools {
                "Tools •"
            } else {
                strings::TOOLS
            },
            Some(KissIcon::Wrench),
            props.show_tools_drawer || has_active_tools,
            p,
            move |_, w, cx| h_tools(ComposerAction::ToggleToolsDrawer, w, cx),
        ));

    let mut right_controls = h_flex().items_center().gap(px(6.0));
    if props.show_token_stats {
        let pct_int = (ratio * 100.0).round() as i32;
        let lim_lbl = if is_unlimited {
            "∞".to_string()
        } else {
            format_tokens_short(bd.context_limit_tokens)
        };
        let cost = format_usd_short(props.session_cost_usd);
        let ctx_lbl = if cost.is_empty() {
            format!("{}/{lim_lbl} ({pct_int}%)", format_tokens_short(total_used))
        } else {
            format!(
                "{}/{lim_lbl} ({pct_int}%) • {cost}",
                format_tokens_short(total_used)
            )
        };
        let stroke_hex = if pct_int >= 85 {
            p.danger.to_hex_u32()
        } else if pct_int >= 65 {
            p.accent_secondary.to_hex_u32()
        } else {
            p.accent.to_hex_u32()
        };
        let track_hex = p.border.to_hex_u32();
        let hover_gauge = p.hover_tint();
        right_controls = right_controls.child(
            h_flex()
                .id("comp-ctx-gauge")
                .h(px(34.0))
                .px(px(9.0))
                .rounded(px(8.0))
                .items_center()
                .gap(px(6.0))
                .cursor_pointer()
                .border_1()
                .border_color(if props.show_context_popover {
                    p.accent.to_hsla()
                } else {
                    p.border.with_alpha(0.85)
                })
                .bg(if props.show_context_popover {
                    p.accent.with_alpha(0.14)
                } else {
                    card
                })
                .hover(move |s| s.bg(hover_gauge))
                .on_click(move |_, w, cx| h_ctx(ComposerAction::ToggleContextPopover, w, cx))
                .child(context_gauge_ring_svg(ratio, stroke_hex, track_hex, 15.0))
                .child(
                    div()
                        .font_family(p.code_font_family.clone())
                        .text_size(px(11.0))
                        .text_color(if pct_int >= 85 {
                            p.danger.to_hsla()
                        } else {
                            narration
                        })
                        .child(ctx_lbl),
                ),
        );
    }

    if props.is_streaming || props.is_drafting_reply {
        right_controls = right_controls
            .child(sepia_button(
                "comp-stop-btn",
                strings::STOP,
                Some(KissIcon::Stop),
                SepiaButtonStyle::Danger,
                true,
                false,
                p,
                move |_, w, cx| h_stop(ComposerAction::Stop, w, cx),
            ))
            .child(sepia_button(
                "comp-queue-btn",
                strings::QUEUE,
                Some(KissIcon::Clock),
                SepiaButtonStyle::Outline,
                true,
                !has_input,
                p,
                move |_, w, cx| h_queue(ComposerAction::QueuePrompt, w, cx),
            ))
            .child(sepia_button(
                "comp-steer-btn",
                strings::SEND_NOW,
                Some(KissIcon::Zap),
                SepiaButtonStyle::Primary,
                true,
                !has_input,
                p,
                move |_, w, cx| h_steer(ComposerAction::SteerNow, w, cx),
            ));
    } else {
        right_controls = right_controls.child(sepia_button(
            "comp-send-btn",
            strings::SEND,
            Some(KissIcon::SendEnter),
            SepiaButtonStyle::Primary,
            true,
            !has_input,
            p,
            move |_, w, cx| h_send(ComposerAction::Send, w, cx),
        ));
    }

    div()
        .w_full()
        .px(px(20.0))
        .pb(px(14.0))
        .pt(px(4.0))
        .child(
            div()
                .relative()
                .w_full()
                .max_w(px(p.max_reading_width_px))
                .mx_auto()
                .children(voice_listening_pill)
                .child(
                    v_flex()
                        .w_full()
                        .px(px(12.0))
                        .py(px(10.0))
                        .rounded(px(12.0))
                        .bg(card)
                        .border_1()
                        .border_color(if props.is_streaming || props.is_drafting_reply {
                            p.accent.with_alpha(0.70)
                        } else {
                            p.border.with_alpha(0.68)
                        })
                        .gap(px(8.0))
                        .children(retry_in_card)
                        .children(live_status_in_card)
                        .children(queued_panel)
                        .children(props.workspace_folder_missing.then(|| {
                            let h_p = props.on_action.clone();
                            let h_c = props.on_action.clone();
                            h_flex()
                                .w_full()
                                .px(px(10.0))
                                .py(px(7.0))
                                .rounded(px(8.0))
                                .bg(p.danger.with_alpha(0.10))
                                .border_1()
                                .border_color(p.danger.with_alpha(0.55))
                                .justify_between()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(kiss_icon(
                                            KissIcon::Folder,
                                            14.0,
                                            p.danger.to_hsla(),
                                        ))
                                        .child(
                                            div()
                                                .flex_1()
                                                .truncate()
                                                .text_size(px(12.0))
                                                .font_semibold()
                                                .text_color(ink)
                                                .child(format!(
                                                    "Workspace folder not found: {}",
                                                    props.policy.workspace_path
                                                )),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .gap(px(6.0))
                                        .flex_shrink_0()
                                        .child(sepia_button(
                                            "ws-miss-pick",
                                            strings::CHOOSE_FOLDER,
                                            None,
                                            SepiaButtonStyle::Primary,
                                            true,
                                            false,
                                            p,
                                            move |_, w, cx| {
                                                h_p(ComposerAction::PickWorkspaceFolder, w, cx)
                                            },
                                        ))
                                        .child(sepia_button(
                                            "ws-miss-clr",
                                            strings::CLEAR_FOLDER,
                                            None,
                                            SepiaButtonStyle::Outline,
                                            true,
                                            false,
                                            p,
                                            move |_, w, cx| {
                                                h_c(ComposerAction::ClearWorkspaceFolder, w, cx)
                                            },
                                        )),
                                )
                        }))
                        .children(attachments_row)
                        .children(prefill_row)
                        .children(slash_popup)
                        .children(at_popup)
                        .child({
                            let est_lines: usize = props
                                .draft_text
                                .split('\n')
                                .map(|line| (line.chars().count() / 78) + 1)
                                .sum::<usize>()
                                .max(1);
                            let composer_h = (32.0 + (est_lines as f32) * 20.0).clamp(54.0, 200.0);
                            Textarea::new(props.composer_input)
                                .appearance(false)
                                .bordered(false)
                                .h(px(composer_h))
                                .aria_label("Message composer")
                        })
                        .child(h_divider(p))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .gap(px(8.0))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(left_controls)
                                .child(right_controls),
                        ),
                )
                .children(tools_drawer)
                .children(context_popover)
                .children(thinking_popover)
                .children(modality_popover),
        )
        .into_any_element()
}

fn active_at_query(input: &str) -> Option<&str> {
    let idx = input.rfind('@')?;
    if idx > 0 && !input[..idx].chars().last()?.is_whitespace() {
        return None;
    }
    let after = &input[idx + 1..];
    if after.contains(' ') || after.contains('\n') {
        None
    } else {
        Some(after)
    }
}
