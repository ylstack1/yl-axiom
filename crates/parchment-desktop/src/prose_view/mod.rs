//! Rich message turn renderer matching `MessageTurnItem.kt`, `MessageTurnFooter.kt`,
//! `MessageTurnReasoningSection.kt`, `MessageTurnMediaAndDrawers.kt`, and `docs/design.md`.
//!
//! Precomputes structured prose blocks, syntax-highlighted warm YL Axiom code cards,
//! tool diff previews, bounded LaTeX/SVG callouts, quiet tool groups, and token/cost
//! metadata once when a row is built or completes streaming so 120Hz render frames stay fast.

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};
use gpui_component::Icon;
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::text::TextView;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{
    GeneratedMediaItem, GoalTrackerState, MediaKind, MessageNode, MessageRole, MessageVariant,
    PreferencesConfig, PromptAttachment, ProviderProtocol, TokenMetrics, ToolCallRecord, ViewMode,
    WebSourceCitation,
};
use parchment_core::prose::{
    DiffLineKind, ProseBlock, TokenKind, compute_unified_line_diff, highlight_code,
    parse_prose_blocks, parse_svg_bounded, render_latex_unicode,
};

use crate::icons::{KissIcon, kiss_icon, model_lab_icon, tool_kiss_icon};
use crate::strings;
use crate::theme::ParchmentPalette;
use crate::ui_kit::{
    SepiaButtonStyle, format_compact_timestamp, format_tokens_short, format_usd_short, h_divider,
    icon_button, rubric_mark_overlay, sepia_avatar_circle, sepia_button, sepia_chip,
    streaming_pulse_dot,
};

mod blocks;
mod chrome;

use blocks::*;
use chrome::*;

const COLLAPSE_LONG_MESSAGE_CHARS: usize = 1800;

/// Actions triggered from inside a rendered message turn.
#[derive(Debug, Clone)]
pub enum MessageAction {
    CopyPlain(usize),
    CopyMarkdown(usize),
    CopyText {
        node_id: String,
        code_block_index: Option<usize>,
        text: String,
    },
    SaveCodeBlock {
        language: String,
        code: String,
    },
    ToggleSpeak(usize),
    StartEdit(usize),
    CancelEdit,
    SaveEdit {
        index: usize,
        regenerate: bool,
        branch: bool,
    },
    Regenerate(usize),
    Continue(usize),
    Translate(usize),
    SaveAsSkill(usize),
    Branch(usize),
    Rewind(usize),
    TogglePin(usize),
    ToggleContextMute(usize),
    SelectVariant {
        index: usize,
        variant_index: usize,
    },
    DeleteCurrentVariant(usize),
    DeleteNode(usize),
    ToggleThinking(String),
    ToggleMoreActions(String),
    ToggleSources(String),
    ToggleRawWire(String),
    ToggleUsageDetails(String),
    ToggleExpandLong(String),
    ToggleShowAllToolSteps(String),
    ToggleQuietToolGroup {
        node_id: String,
        group_key: String,
    },
    ToggleCodeCollapse {
        node_id: String,
        block_index: usize,
    },
    ToggleCodeWrap {
        node_id: String,
        block_index: usize,
    },
    InspectAttachment(PromptAttachment),
    InspectGeneratedMedia(GeneratedMediaItem),
    InspectSvg {
        title: String,
        raw_svg: String,
    },
    OpenUrl(String),
}

pub type MessageActionHandler = Rc<dyn Fn(MessageAction, &mut Window, &mut App)>;

#[derive(Debug, Default, Clone)]
pub struct TurnUiState {
    pub thinking_expanded: Option<bool>,
    pub show_more_actions: bool,
    pub show_sources_drawer: bool,
    pub show_raw_wire_drawer: bool,
    pub show_usage_details: bool,
    pub expanded_long_message: bool,
    pub show_all_tool_steps: bool,
    pub expanded_tool_groups: HashSet<String>,
    pub toggled_code_collapse: HashSet<usize>,
    pub toggled_code_wrap: HashSet<usize>,
    pub copied_kind: Option<&'static str>,
    pub copied_code_block: Option<usize>,
}

#[derive(Clone)]
pub struct DisplayToolCard {
    pub id: String,
    pub tool_name: String,
    pub title: SharedString,
    pub status_meta: SharedString,
    pub is_error: bool,
    pub is_running: bool,
    pub preceding_thinking: SharedString,
    pub preceding_text: SharedString,
    pub diff_lines: Vec<(DiffLineKind, SharedString)>,
    pub result_preview: SharedString,
}

#[derive(Clone)]
pub enum DisplayProseBlock {
    Markdown(SharedString),
    CodeFence {
        language: SharedString,
        code: SharedString,
        line_count: usize,
        highlighted_lines: Vec<Vec<(TokenKind, SharedString)>>,
    },
    Latex(SharedString),
    Svg {
        raw_bytes: Vec<u8>,
        raw_svg: SharedString,
        width: f32,
        height: f32,
    },
}

#[derive(Clone)]
pub struct DisplayMessageRow {
    pub node_id: String,
    pub is_user: bool,
    pub content: SharedString,
    pub thinking_summary: SharedString,
    pub trailing_thinking: SharedString,
    pub thinking_signature: SharedString,
    pub thinking_duration_ms: i64,
    pub tool_cards: Vec<DisplayToolCard>,
    pub goal_tracker: Option<GoalTrackerState>,
    pub attachments: Vec<PromptAttachment>,
    pub media_items: Vec<GeneratedMediaItem>,
    pub web_sources: Vec<WebSourceCitation>,
    pub raw_request_preview: SharedString,
    pub raw_response_preview: SharedString,
    pub model_used: SharedString,
    pub provider_protocol: Option<ProviderProtocol>,
    pub created_at: i64,
    pub token_metrics: TokenMetrics,
    pub cost_usd: f64,
    pub estimated_prose_tokens: i64,
    pub variant_index: usize,
    pub variant_count: usize,
    pub included_in_context: bool,
    pub pinned: bool,
    pub is_streaming: bool,
    pub prose_blocks: Vec<DisplayProseBlock>,
}

impl DisplayMessageRow {
    pub fn user(
        node_id: String,
        content: String,
        attachments: Vec<PromptAttachment>,
        now_ms: i64,
    ) -> Self {
        let prose_blocks = precompute_prose_blocks(&content, true);
        let estimated_prose_tokens = estimate_tokens_fast(&content) as i64;
        Self {
            node_id,
            is_user: true,
            content: SharedString::from(content),
            thinking_summary: SharedString::new_static(""),
            trailing_thinking: SharedString::new_static(""),
            thinking_signature: SharedString::new_static(""),
            thinking_duration_ms: 0,
            tool_cards: Vec::new(),
            goal_tracker: None,
            attachments,
            media_items: Vec::new(),
            web_sources: Vec::new(),
            raw_request_preview: SharedString::new_static(""),
            raw_response_preview: SharedString::new_static(""),
            model_used: SharedString::new_static(""),
            provider_protocol: None,
            created_at: now_ms,
            token_metrics: TokenMetrics::default(),
            cost_usd: 0.0,
            estimated_prose_tokens,
            variant_index: 0,
            variant_count: 1,
            included_in_context: true,
            pinned: false,
            is_streaming: false,
            prose_blocks,
        }
    }

    pub fn assistant_placeholder(node_id: String, model_used: SharedString, now_ms: i64) -> Self {
        Self {
            node_id,
            is_user: false,
            content: SharedString::new_static(""),
            thinking_summary: SharedString::new_static(""),
            trailing_thinking: SharedString::new_static(""),
            thinking_signature: SharedString::new_static(""),
            thinking_duration_ms: 0,
            tool_cards: Vec::new(),
            goal_tracker: None,
            attachments: Vec::new(),
            media_items: Vec::new(),
            web_sources: Vec::new(),
            raw_request_preview: SharedString::new_static(""),
            raw_response_preview: SharedString::new_static(""),
            model_used,
            provider_protocol: None,
            created_at: now_ms,
            token_metrics: TokenMetrics::default(),
            cost_usd: 0.0,
            estimated_prose_tokens: 0,
            variant_index: 0,
            variant_count: 1,
            included_in_context: true,
            pinned: false,
            is_streaming: true,
            prose_blocks: Vec::new(),
        }
    }

    pub fn from_node(node: MessageNode) -> Self {
        let is_user = node.role == MessageRole::User;
        let variant_count = node.variants.len().max(1);
        let active_idx = node
            .active_variant_index
            .min(node.variants.len().saturating_sub(1));
        let variant = node
            .variants
            .into_iter()
            .nth(active_idx)
            .unwrap_or_default();
        Self::from_variant(
            node.id,
            is_user,
            node.pinned,
            node.included_in_context,
            active_idx,
            variant_count,
            variant,
        )
    }

    pub fn from_variant(
        node_id: String,
        is_user: bool,
        pinned: bool,
        included_in_context: bool,
        variant_index: usize,
        variant_count: usize,
        variant: MessageVariant,
    ) -> Self {
        let prose_blocks = precompute_prose_blocks(&variant.content, !variant.is_streaming);
        let tool_cards = precompute_tool_cards(&variant.tool_calls);
        let estimated_prose_tokens = estimate_tokens_fast(&variant.content) as i64;
        let cost_usd: f64 = if variant.api_usage_records.is_empty() {
            if variant.token_metrics.cost_is_available
                && variant.token_metrics.estimated_cost_usd.is_finite()
            {
                variant.token_metrics.estimated_cost_usd
            } else {
                0.0
            }
        } else {
            variant
                .api_usage_records
                .iter()
                .map(|r| parchment_core::protocol::usage::estimate_request_cost(r).usd)
                .sum()
        };
        Self {
            node_id,
            is_user,
            content: SharedString::from(variant.content),
            thinking_summary: SharedString::from(variant.thinking_summary),
            trailing_thinking: SharedString::from(variant.trailing_thinking),
            thinking_signature: SharedString::from(variant.thinking_signature),
            thinking_duration_ms: variant.thinking_duration_ms,
            tool_cards,
            goal_tracker: variant.goal_tracker,
            attachments: variant.attachments,
            media_items: variant.media_items,
            web_sources: variant.web_sources,
            raw_request_preview: SharedString::from(variant.raw_request_preview),
            raw_response_preview: SharedString::from(variant.raw_response_preview),
            model_used: SharedString::from(variant.model_used),
            provider_protocol: variant.provider_protocol,
            created_at: variant.created_at,
            token_metrics: variant.token_metrics,
            cost_usd,
            estimated_prose_tokens,
            variant_index,
            variant_count,
            included_in_context,
            pinned,
            is_streaming: variant.is_streaming,
            prose_blocks,
        }
    }

    pub fn set_streaming_content(&mut self, text: &str, refresh_blocks: bool) {
        self.content = SharedString::from(text.to_string());
        if refresh_blocks {
            self.prose_blocks = precompute_prose_blocks(text, true);
        }
    }

    pub fn set_streaming_thinking(&mut self, thinking: &str) {
        self.thinking_summary = SharedString::from(thinking.to_string());
    }

    pub fn set_streaming_tools(&mut self, tools: &[ToolCallRecord]) {
        self.tool_cards = precompute_tool_cards(tools);
    }

    pub fn recompute_extras_for_completed(&mut self) {
        self.prose_blocks = precompute_prose_blocks(self.content.as_ref(), true);
        self.estimated_prose_tokens = estimate_tokens_fast(self.content.as_ref()) as i64;
    }
}

fn estimate_tokens_fast(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    (trimmed.chars().count() / 4).max(1)
}

fn count_words_fast(text: &str) -> usize {
    text.split_whitespace().count()
}

fn clip_preview(raw: &str, max_chars: usize) -> SharedString {
    if raw.is_empty() {
        return SharedString::new_static("");
    }
    match raw.char_indices().nth(max_chars) {
        Some((cut, _)) => SharedString::from(format!("{}…", &raw[..cut])),
        None => SharedString::from(raw.to_string()),
    }
}

/// Splits Markdown into prose runs, warm YL Axiom code cards (with precomputed syntax tokens),
/// bounded LaTeX math blocks, and inline SVG diagrams.
fn precompute_prose_blocks(content: &str, allow_rich_blocks: bool) -> Vec<DisplayProseBlock> {
    if content.is_empty() {
        return Vec::new();
    }
    if !allow_rich_blocks
        || (!content.contains("```")
            && !content.contains("~~~")
            && !content.contains("$$")
            && !content.contains("\\[")
            && !content.contains("<svg"))
    {
        return vec![DisplayProseBlock::Markdown(SharedString::from(
            content.to_string(),
        ))];
    }

    let blocks = parse_prose_blocks(content, true);
    let mut out = Vec::with_capacity(blocks.len());
    let mut md_acc = String::new();

    let flush_md = |acc: &mut String, dest: &mut Vec<DisplayProseBlock>| {
        let trimmed = acc.trim();
        if !trimmed.is_empty() {
            dest.push(DisplayProseBlock::Markdown(SharedString::from(
                trimmed.to_string(),
            )));
        }
        acc.clear();
    };

    for block in blocks {
        match block {
            ProseBlock::CodeFence { language, code, .. } => {
                flush_md(&mut md_acc, &mut out);
                let line_count = code.lines().count().max(1);
                let highlighted_lines = precompute_highlighted_lines(&language, &code);
                out.push(DisplayProseBlock::CodeFence {
                    language: SharedString::from(language),
                    code: SharedString::from(code),
                    line_count,
                    highlighted_lines,
                });
            }
            ProseBlock::MathBlock { raw_latex } => {
                flush_md(&mut md_acc, &mut out);
                let rendered = render_latex_unicode(&raw_latex);
                if !rendered.trim().is_empty() {
                    out.push(DisplayProseBlock::Latex(SharedString::from(rendered)));
                }
            }
            ProseBlock::SvgBlock { raw_svg } => {
                flush_md(&mut md_acc, &mut out);
                if let Some(svg_doc) = parse_svg_bounded(&raw_svg) {
                    let safe_svg = sanitize_svg_for_gpui(&raw_svg, svg_doc.width, svg_doc.height);
                    out.push(DisplayProseBlock::Svg {
                        raw_bytes: safe_svg.into_bytes(),
                        raw_svg: SharedString::from(raw_svg),
                        width: svg_doc.width.clamp(80.0, 680.0),
                        height: svg_doc.height.clamp(60.0, 420.0),
                    });
                }
            }
            ProseBlock::Heading { level, text } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                let hashes = "#".repeat(level.clamp(1, 6) as usize);
                md_acc.push_str(&format!("{hashes} {text}"));
            }
            ProseBlock::SceneBreak { .. } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                md_acc.push_str("---");
            }
            ProseBlock::BlockQuote { text } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                for line in text.lines() {
                    md_acc.push_str("> ");
                    md_acc.push_str(line);
                    md_acc.push('\n');
                }
            }
            ProseBlock::ListBlock { ordered, items } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                for (i, item) in items.iter().enumerate() {
                    if ordered {
                        md_acc.push_str(&format!("{}. {item}\n", i + 1));
                    } else {
                        md_acc.push_str(&format!("- {item}\n"));
                    }
                }
            }
            ProseBlock::TableBlock { headers, rows } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                md_acc.push_str(&format!("| {} |\n", headers.join(" | ")));
                let sep: Vec<&str> = headers.iter().map(|_| "---").collect();
                md_acc.push_str(&format!("| {} |\n", sep.join(" | ")));
                for r in rows {
                    md_acc.push_str(&format!("| {} |\n", r.join(" | ")));
                }
            }
            ProseBlock::Paragraph { text } => {
                if !md_acc.is_empty() {
                    md_acc.push_str("\n\n");
                }
                md_acc.push_str(&text);
            }
        }
    }
    flush_md(&mut md_acc, &mut out);
    if out.is_empty() {
        out.push(DisplayProseBlock::Markdown(SharedString::from(
            content.to_string(),
        )));
    }
    out
}

fn precompute_highlighted_lines(language: &str, code: &str) -> Vec<Vec<(TokenKind, SharedString)>> {
    let spans = highlight_code(language, code);
    let mut lines: Vec<Vec<(TokenKind, SharedString)>> = Vec::new();
    let mut cur_line: Vec<(TokenKind, SharedString)> = Vec::new();

    if spans.is_empty() {
        for raw_line in code.lines() {
            lines.push(vec![(
                TokenKind::Plain,
                SharedString::from(raw_line.to_string()),
            )]);
        }
        return lines;
    }

    for sp in spans {
        let Some(slice) = code.get(sp.range) else {
            continue;
        };
        let mut parts = slice.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                cur_line.push((sp.kind, SharedString::from(part.to_string())));
            }
            if parts.peek().is_some() {
                lines.push(std::mem::take(&mut cur_line));
            }
        }
    }
    if !cur_line.is_empty() || lines.is_empty() {
        lines.push(cur_line);
    }
    // Bound huge code fences so a single 5000-line dump never stalls layout.
    if lines.len() > 240 {
        let remaining = lines.len() - 240;
        lines.truncate(240);
        lines.push(vec![(
            TokenKind::Comment,
            SharedString::from(format!(
                "… ({remaining} more lines — copy or save to view all)"
            )),
        )]);
    }
    lines
}

fn sanitize_svg_for_gpui(raw: &str, width: f32, height: f32) -> String {
    let trimmed = raw.trim();
    if trimmed.to_ascii_lowercase().contains("<script")
        || trimmed.to_ascii_lowercase().contains("<foreignobject")
    {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}"></svg>"#)
    } else if !trimmed.contains("xmlns=") {
        trimmed.replacen("<svg", r#"<svg xmlns="http://www.w3.org/2000/svg""#, 1)
    } else {
        trimmed.to_string()
    }
}

fn precompute_tool_cards(tool_calls: &[ToolCallRecord]) -> Vec<DisplayToolCard> {
    if tool_calls.is_empty() {
        return Vec::new();
    }
    tool_calls
        .iter()
        .map(|tc| {
            let status_lbl = if tc.is_running {
                "running"
            } else if tc.is_error {
                "error"
            } else {
                "done"
            };
            let mut diff_lines = Vec::new();
            let mut result_preview = SharedString::new_static("");
            if tc.name == "fs_edit"
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&tc.arguments_json)
                && let (Some(old_t), Some(new_t)) = (
                    val.get("old_text")
                        .or_else(|| val.get("old_string"))
                        .and_then(|v| v.as_str()),
                    val.get("new_text")
                        .or_else(|| val.get("new_string"))
                        .and_then(|v| v.as_str()),
                )
            {
                diff_lines = compute_unified_line_diff(old_t, new_t)
                    .into_iter()
                    .take(16)
                    .map(|dl| {
                        let prefix = match dl.kind {
                            DiffLineKind::Added => "+ ",
                            DiffLineKind::Removed => "- ",
                            DiffLineKind::Context | DiffLineKind::Collapsed => "  ",
                        };
                        (dl.kind, SharedString::from(format!("{prefix}{}", dl.text)))
                    })
                    .collect();
            } else if !tc.result_text.is_empty() {
                result_preview = clip_preview(&tc.result_text, 280);
            }
            DisplayToolCard {
                id: tc.id.clone(),
                tool_name: tc.name.clone(),
                title: SharedString::from(format_tool_title(tc)),
                status_meta: SharedString::from(format!("{status_lbl} · {}ms", tc.duration_ms)),
                is_error: tc.is_error,
                is_running: tc.is_running,
                preceding_thinking: SharedString::from(tc.preceding_thinking.clone()),
                preceding_text: SharedString::from(tc.preceding_text.clone()),
                diff_lines,
                result_preview,
            }
        })
        .collect()
}

fn format_tool_title(tc: &ToolCallRecord) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&tc.arguments_json) {
        for key in ["path", "file_path", "pattern", "query", "url", "command"] {
            if let Some(arg) = val.get(key).and_then(|v| v.as_str())
                && !arg.trim().is_empty()
            {
                let short: String = arg.trim().chars().take(60).collect();
                return format!("{} · {short}", tc.name);
            }
        }
    }
    tc.name.clone()
}

fn is_quiet_lookup_tool(tc: &DisplayToolCard) -> bool {
    if tc.is_running || tc.is_error {
        return false;
    }
    matches!(
        tc.tool_name.to_ascii_lowercase().as_str(),
        "fs_read" | "fs_list" | "fs_glob" | "fs_search" | "web_search" | "web_fetch"
    )
}

fn quiet_tool_group_summary(tools: &[&DisplayToolCard]) -> String {
    let mut read = 0usize;
    let mut list = 0usize;
    let mut search = 0usize;
    let mut web = 0usize;
    for tc in tools {
        match tc.tool_name.to_ascii_lowercase().as_str() {
            "fs_read" => read += 1,
            "fs_list" | "fs_glob" => list += 1,
            "fs_search" => search += 1,
            "web_search" | "web_fetch" => web += 1,
            _ => {}
        }
    }
    let mut parts = Vec::new();
    if read > 0 {
        parts.push(format!("Read {read}"));
    }
    if list > 0 {
        parts.push(format!("listed {list}"));
    }
    if search > 0 {
        parts.push(format!("searched {search}"));
    }
    if web > 0 {
        parts.push(format!("fetched {web}"));
    }
    if parts.is_empty() {
        format!("{} lookup steps", tools.len())
    } else {
        parts.join(" · ")
    }
}

pub struct TurnRenderContext<'a> {
    pub view_mode: ViewMode,
    pub palette: &'a ParchmentPalette,
    pub prefs: &'a PreferencesConfig,
    pub user_name: &'a str,
    pub user_picture_path: &'a str,
    pub assistant_name: &'a str,
    pub provider_name: &'a str,
    pub is_latest_turn: bool,
    pub is_speaking_this_turn: bool,
    pub highlight_dialogue_and_narration: bool,
    pub ui_state: &'a TurnUiState,
    pub is_editing: bool,
    pub edit_input: Option<&'a Entity<TextareaState>>,
    pub on_action: &'a MessageActionHandler,
}

/// Renders one message turn in either Bubble (`EditorialTurns`) or Page (`Manuscript`) view,
/// centered inside `palette.max_reading_width_px` with full Kotlin turn chrome & actions.
pub fn render_message_row(
    ix: usize,
    msg: &DisplayMessageRow,
    ctx: &TurnRenderContext<'_>,
) -> AnyElement {
    let p = ctx.palette;
    let prefs = ctx.prefs;
    let on_action = ctx.on_action;

    let card = p.card_bg.to_hsla();
    let elev = p.elevated_bg.to_hsla();
    let border = p.subtle_border();
    let ink = p.primary_ink.to_hsla();
    let narration = p.narration_ink.to_hsla();
    let muted = p.muted_ink.to_hsla();
    let accent = p.accent.to_hsla();
    let accent2 = p.accent_secondary.to_hsla();
    let danger = p.danger.to_hsla();

    let show_bubbles = ctx.view_mode == ViewMode::EditorialTurns;
    let is_sent_bubble = show_bubbles && msg.is_user;
    let turn_group = SharedString::from(format!("turn-grp-{ix}"));

    let has_thinking = prefs.show_thinking_cards
        && (!msg.thinking_summary.is_empty()
            || !msg.trailing_thinking.is_empty()
            || msg
                .tool_cards
                .iter()
                .any(|t| !t.preceding_thinking.is_empty()));
    let has_tools = prefs.show_tool_cards && !msg.tool_cards.is_empty();
    let is_live_peek_only = msg.is_streaming
        && ctx.ui_state.thinking_expanded == Some(false)
        && (has_thinking || has_tools);
    let thinking_expanded = match ctx.ui_state.thinking_expanded {
        Some(v) => v,
        None => {
            (msg.is_streaming && msg.content.is_empty())
                || (!prefs.auto_collapse_thinking && (has_thinking || has_tools))
        }
    };
    // ---- 1. Turn Header (outside the bubble!) ----
    let header_el = render_turn_header(
        ix,
        msg,
        ctx,
        has_thinking,
        has_tools,
        thinking_expanded,
        narration,
        muted,
        accent2,
        danger,
    );

    // ---- 2. Collapsible Thinking & Tool Timeline OR Live Peek (outside the bubble!) ----
    let reasoning_drawer = if !msg.is_user
        && (thinking_expanded || is_live_peek_only)
        && (has_thinking || has_tools)
    {
        Some(crate::ui_kit::spring_pop_enter(
            ("think-anim", ix),
            render_reasoning_drawer(ix, msg, ctx, has_thinking, has_tools, is_live_peek_only),
        ))
    } else {
        None
    };

    // ---- 3. Goal Tracker Card (if active on this turn) ----
    let goal_card = msg.goal_tracker.as_ref().and_then(|gt| {
        if gt.goal_objective.is_empty() && gt.todos.is_empty() {
            return None;
        }
        let mut box_el = v_flex()
            .w_full()
            .mb(px(6.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(8.0))
            .bg(elev)
            .border_1()
            .border_color(p.accent.with_alpha(0.4))
            .gap(px(4.0))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(kiss_icon(KissIcon::Target, 13.0, accent))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_semibold()
                            .text_color(ink)
                            .child(if gt.goal_objective.is_empty() {
                                "Goal checklist".to_string()
                            } else {
                                gt.goal_objective.clone()
                            }),
                    ),
            );
        for todo in &gt.todos {
            let done = todo.status.eq_ignore_ascii_case("completed")
                || todo.status.eq_ignore_ascii_case("done");
            let in_prog = todo.status.eq_ignore_ascii_case("in_progress");
            let mark = if done {
                "[x]"
            } else if in_prog {
                "[~]"
            } else {
                "[ ]"
            };
            box_el = box_el.child(
                div()
                    .text_size(px(12.0))
                    .text_color(if done { muted } else { ink })
                    .child(format!("{mark} {}", todo.content)),
            );
        }
        Some(box_el)
    });

    // ---- 4. Main Prose Body OR Inline Message Editor ----
    let main_body: AnyElement = if ctx.is_editing
        && let Some(edit_state) = ctx.edit_input
    {
        let h_cancel = on_action.clone();
        let h_save = on_action.clone();
        let h_branch = on_action.clone();
        let h_regen = on_action.clone();
        v_flex()
            .w_full()
            .gap(px(8.0))
            .child(
                Textarea::new(edit_state)
                    .h(px(120.0))
                    .aria_label("Edit message"),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(6.0))
                    .children(msg.is_user.then(|| {
                        let as_branch = !prefs.delete_previous_message_when_regenerating;
                        sepia_button(
                            ("edit-save-regen", ix),
                            strings::SAVE_AND_REGENERATE,
                            Some(KissIcon::Refresh),
                            SepiaButtonStyle::Primary,
                            true,
                            false,
                            p,
                            move |_, w, cx| {
                                h_regen(
                                    MessageAction::SaveEdit {
                                        index: ix,
                                        regenerate: true,
                                        branch: as_branch,
                                    },
                                    w,
                                    cx,
                                );
                            },
                        )
                    }))
                    .child(sepia_button(
                        ("edit-save", ix),
                        strings::SAVE,
                        Some(KissIcon::Check),
                        if msg.is_user {
                            SepiaButtonStyle::Outline
                        } else {
                            SepiaButtonStyle::Primary
                        },
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_save(
                                MessageAction::SaveEdit {
                                    index: ix,
                                    regenerate: false,
                                    branch: false,
                                },
                                w,
                                cx,
                            );
                        },
                    ))
                    .child(sepia_button(
                        ("edit-save-branch", ix),
                        "Save as new version",
                        Some(KissIcon::GitBranch),
                        SepiaButtonStyle::Secondary,
                        true,
                        false,
                        p,
                        move |_, w, cx| {
                            h_branch(
                                MessageAction::SaveEdit {
                                    index: ix,
                                    regenerate: false,
                                    branch: true,
                                },
                                w,
                                cx,
                            );
                        },
                    ))
                    .child(sepia_button(
                        ("edit-cancel", ix),
                        strings::CANCEL,
                        None,
                        SepiaButtonStyle::Ghost,
                        true,
                        false,
                        p,
                        move |_, w, cx| h_cancel(MessageAction::CancelEdit, w, cx),
                    )),
            )
            .into_any_element()
    } else {
        render_turn_body(ix, msg, ctx)
    };

    // ---- 5. Action Footer & Token Stats (outside the bubble!) ----
    let footer_el = render_turn_footer(ix, msg, ctx, is_sent_bubble, &turn_group, muted, accent);

    // ---- 6. Optional Web Sources Drawer ----
    let sources_drawer = if !msg.is_user
        && ctx.ui_state.show_sources_drawer
        && !msg.web_sources.is_empty()
    {
        let h_close_src = on_action.clone();
        let nid_src = msg.node_id.clone();
        Some(
            v_flex()
                .w_full()
                .mt(px(6.0))
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(elev)
                .border_1()
                .border_color(border)
                .gap(px(6.0))
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(kiss_icon(KissIcon::Globe, 13.0, accent))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_semibold()
                                        .text_color(accent)
                                        .child(format!(
                                            "Grounded web sources ({})",
                                            msg.web_sources.len()
                                        )),
                                ),
                        )
                        .child(icon_button(
                            ("web-src-close", ix),
                            KissIcon::Close,
                            false,
                            p,
                            move |_, w, cx| {
                                h_close_src(MessageAction::ToggleSources(nid_src.clone()), w, cx)
                            },
                        )),
                )
                .child(h_divider(p))
                .children(msg.web_sources.iter().enumerate().map(|(s_ix, src)| {
                    let url = src.url.clone();
                    let handler = on_action.clone();
                    let label = if src.title.is_empty() {
                        if src.domain.is_empty() {
                            src.url.clone()
                        } else {
                            src.domain.clone()
                        }
                    } else {
                        src.title.clone()
                    };
                    let first_line = src.grounded_lines.first().cloned().unwrap_or_default();
                    v_flex()
                        .id(("web-src", ix * 32 + s_ix))
                        .w_full()
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(8.0))
                        .bg(card)
                        .border_1()
                        .border_color(border)
                        .gap(px(4.0))
                        .cursor_pointer()
                        .on_click(move |_, w, cx| {
                            handler(MessageAction::OpenUrl(url.clone()), w, cx)
                        })
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .gap(px(8.0))
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .overflow_hidden()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .px(px(5.0))
                                                .py(px(1.5))
                                                .rounded(px(4.0))
                                                .bg(p.accent.with_alpha(0.16))
                                                .font_family(p.code_font_family.clone())
                                                .text_size(px(11.0))
                                                .font_semibold()
                                                .text_color(accent)
                                                .child(format!("[{}]", src.index)),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .overflow_hidden()
                                                .child(
                                                    div()
                                                        .text_size(px(12.5))
                                                        .font_semibold()
                                                        .text_color(ink)
                                                        .truncate()
                                                        .child(label),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(muted)
                                                        .truncate()
                                                        .child(if src.domain.is_empty() {
                                                            src.url.clone()
                                                        } else {
                                                            src.domain.clone()
                                                        }),
                                                ),
                                        ),
                                )
                                .child(kiss_icon(KissIcon::ExternalLink, 12.0, muted)),
                        )
                        .children((!first_line.is_empty()).then(|| {
                            div()
                                .text_size(px(11.0))
                                .text_color(narration)
                                .truncate()
                                .child(format!("\"{first_line}\""))
                        }))
                })),
        )
    } else {
        None
    };

    // ---- 7. Optional Raw Wire Drawer (`MessageTurnMediaAndDrawers.kt:654-731`) ----
    let raw_wire_drawer = if ctx.ui_state.show_raw_wire_drawer
        && (!msg.raw_request_preview.is_empty() || !msg.raw_response_preview.is_empty())
    {
        let h_copy_req = on_action.clone();
        let h_copy_resp = on_action.clone();
        let nid_req = msg.node_id.clone();
        let nid_resp = msg.node_id.clone();
        let req_full = msg.raw_request_preview.to_string();
        let resp_full = msg.raw_response_preview.to_string();
        Some(
            v_flex()
                .w_full()
                .mt(px(6.0))
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(elev)
                .border_1()
                .border_color(border)
                .font_family(p.code_font_family.clone())
                .text_size(px(11.0))
                .text_color(narration)
                .gap(px(6.0))
                .children((!msg.raw_request_preview.is_empty()).then(|| {
                    v_flex()
                        .gap(px(4.0))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(div().font_semibold().text_color(ink).child("Request sent"))
                                .child(icon_button(
                                    ("raw-copy-req", ix),
                                    KissIcon::Copy,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_copy_req(
                                            MessageAction::CopyText {
                                                node_id: nid_req.clone(),
                                                code_block_index: None,
                                                text: req_full.clone(),
                                            },
                                            w,
                                            cx,
                                        )
                                    },
                                )),
                        )
                        .child(div().child(clip_preview(msg.raw_request_preview.as_ref(), 4000)))
                }))
                .children((!msg.raw_response_preview.is_empty()).then(|| {
                    v_flex()
                        .gap(px(4.0))
                        .child(h_divider(p))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .items_center()
                                .child(
                                    div()
                                        .font_semibold()
                                        .text_color(ink)
                                        .child("Latest response"),
                                )
                                .child(icon_button(
                                    ("raw-copy-resp", ix),
                                    KissIcon::Copy,
                                    false,
                                    p,
                                    move |_, w, cx| {
                                        h_copy_resp(
                                            MessageAction::CopyText {
                                                node_id: nid_resp.clone(),
                                                code_block_index: None,
                                                text: resp_full.clone(),
                                            },
                                            w,
                                            cx,
                                        )
                                    },
                                )),
                        )
                        .child(div().child(clip_preview(msg.raw_response_preview.as_ref(), 4000)))
                })),
        )
    } else {
        None
    };

    let reading_w = px(p.max_reading_width_px);

    match ctx.view_mode {
        ViewMode::Manuscript => {
            let body_wrap = if msg.is_user {
                div()
                    .relative()
                    .w_full()
                    .pl(px(14.0))
                    .py(px(2.0))
                    .child(rubric_mark_overlay(2.0, p))
                    .child(main_body)
            } else {
                div().w_full().child(main_body)
            };

            let mut col = v_flex()
                .group(turn_group)
                .w_full()
                .max_w(reading_w)
                .mx_auto()
                .py(px(10.0))
                .gap(px(6.0))
                .children(header_el)
                .children(reasoning_drawer)
                .children(goal_card)
                .child(body_wrap)
                .children(footer_el)
                .children(sources_drawer)
                .children(raw_wire_drawer)
                .child(div().pt(px(6.0)).child(h_divider(p)));

            if !msg.included_in_context {
                col = col.opacity(0.60);
            }
            div().w_full().px(px(16.0)).child(col).into_any_element()
        }
        ViewMode::EditorialTurns => {
            let shrink_bubble = !msg.is_user
                && !ctx.is_editing
                && !msg.is_streaming
                && msg.attachments.is_empty()
                && msg.media_items.is_empty()
                && msg.content.chars().count() < 72
                && !msg.content.contains("```")
                && !msg.content.contains('\n');

            let bubble_box = if msg.is_user {
                div()
                    .px(px(13.0))
                    .py(px(9.0))
                    .rounded_tl(px(16.0))
                    .rounded_tr(px(16.0))
                    .rounded_bl(px(16.0))
                    .rounded_br(px(4.0))
                    .bg(p.user_bubble_bg())
                    .child(main_body)
            } else {
                let mut b = div()
                    .px(px(13.0))
                    .py(px(9.0))
                    .rounded_tl(px(16.0))
                    .rounded_tr(px(16.0))
                    .rounded_br(px(16.0))
                    .rounded_bl(px(4.0))
                    .bg(card)
                    .border_1()
                    .border_color(border)
                    .child(main_body);
                if !shrink_bubble {
                    b = b.w_full();
                }
                b
            };

            let max_bubble_w = if msg.is_user {
                px((p.max_reading_width_px * 0.84).min(690.0))
            } else {
                px((p.max_reading_width_px * 0.94).min(770.0))
            };

            let mut turn_col = v_flex()
                .max_w(max_bubble_w)
                .gap(px(4.0))
                .children(header_el)
                .children(reasoning_drawer)
                .children(goal_card)
                .child(bubble_box)
                .children(footer_el)
                .children(sources_drawer)
                .children(raw_wire_drawer);

            if is_sent_bubble {
                turn_col = turn_col.items_end();
            } else if shrink_bubble {
                turn_col = turn_col.items_start();
            } else {
                turn_col = turn_col.w_full().items_start();
            }
            if !msg.included_in_context {
                turn_col = turn_col.opacity(0.60);
            }

            let outer = h_flex()
                .group(turn_group)
                .w_full()
                .max_w(reading_w)
                .mx_auto()
                .py(px(3.0))
                .child(turn_col);

            let aligned = if is_sent_bubble {
                outer.justify_end()
            } else {
                outer.justify_start()
            };
            div()
                .w_full()
                .px(px(16.0))
                .child(aligned)
                .into_any_element()
        }
    }
}
