//! Rich message turn renderer powered by `parchment-prose` + GPUI selectable `TextView::markdown`.
//!
//! Precomputes tool diff previews, bounded thinking previews, and footer strings once when a row
//! is built or finishes streaming so 120Hz render frames perform zero JSON parsing or LCS diffing.

use gpui::{
    AnyElement, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};
use gpui_component::text::TextView;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{MessageNode, MessageRole, MessageVariant, ToolCallRecord, ViewMode};
use parchment_core::prose::{
    DiffLineKind, ProseBlock, compute_unified_line_diff, parse_prose_blocks, parse_svg_bounded,
    render_latex_unicode,
};

use crate::theme::ParchmentPalette;

#[derive(Clone)]
pub struct DisplayToolCard {
    pub title: SharedString,
    pub status_meta: SharedString,
    pub is_error: bool,
    pub diff_lines: Vec<(DiffLineKind, SharedString)>,
    pub result_preview: SharedString,
}

#[derive(Clone)]
pub struct DisplayMessageRow {
    pub is_user: bool,
    pub content: SharedString,
    pub thinking_preview: SharedString,
    pub tool_cards: Vec<DisplayToolCard>,
    pub footer_text: SharedString,
    pub pinned: bool,
}

impl DisplayMessageRow {
    pub fn user(content: String) -> Self {
        Self {
            is_user: true,
            content: SharedString::from(content),
            thinking_preview: SharedString::new_static(""),
            tool_cards: Vec::new(),
            footer_text: SharedString::new_static(""),
            pinned: false,
        }
    }

    pub fn assistant_placeholder(model_used: SharedString) -> Self {
        let footer_text = build_footer_text(false, model_used.as_ref(), 0, 0, 0, false);
        Self {
            is_user: false,
            content: SharedString::new_static(""),
            thinking_preview: SharedString::new_static(""),
            tool_cards: Vec::new(),
            footer_text,
            pinned: false,
        }
    }

    pub fn from_node(node: MessageNode) -> Self {
        let is_user = node.role == MessageRole::User;
        let pinned = node.pinned;
        let active_idx = node.active_variant_index;
        let variant = node
            .variants
            .into_iter()
            .nth(active_idx)
            .unwrap_or_default();
        Self::from_variant(is_user, pinned, variant)
    }

    pub fn from_variant(is_user: bool, pinned: bool, variant: MessageVariant) -> Self {
        let thinking_preview = clip_preview(&variant.thinking_summary, 600);
        let tool_cards = precompute_tool_cards(&variant.tool_calls);
        let footer_text = build_footer_text(
            is_user,
            &variant.model_used,
            variant.token_metrics.input_tokens,
            variant.token_metrics.output_tokens,
            variant.token_metrics.cached_input_tokens,
            pinned,
        );
        Self {
            is_user,
            content: SharedString::from(variant.content),
            thinking_preview,
            tool_cards,
            footer_text,
            pinned,
        }
    }

    pub fn set_streaming_thinking(&mut self, thinking: &str) {
        self.thinking_preview = clip_preview(thinking, 600);
    }
}

fn clip_preview(raw: &str, max_chars: usize) -> SharedString {
    if raw.is_empty() {
        return SharedString::new_static("");
    }
    match raw.char_indices().nth(max_chars) {
        Some((cut, _)) => SharedString::from(raw[..cut].to_string()),
        None => SharedString::from(raw.to_string()),
    }
}

fn build_footer_text(
    is_user: bool,
    model_used: &str,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    pinned: bool,
) -> SharedString {
    use std::fmt::Write as _;
    let has_model = !is_user && !model_used.is_empty();
    let has_tokens = input_tokens > 0 || output_tokens > 0;
    if !has_model && !has_tokens && !pinned {
        return SharedString::new_static("");
    }
    let mut out = String::with_capacity(64);
    if has_model {
        out.push_str(model_used);
    }
    if has_tokens {
        if !out.is_empty() {
            out.push_str("  ·  ");
        }
        if cached_tokens > 0 {
            let _ = write!(
                out,
                "{input_tokens} in ({cached_tokens} reused) · {output_tokens} out"
            );
        } else {
            let _ = write!(out, "{input_tokens} in · {output_tokens} out");
        }
    }
    if pinned {
        if !out.is_empty() {
            out.push_str("  ·  ");
        }
        out.push_str("Pinned");
    }
    SharedString::from(out)
}

fn precompute_tool_cards(tool_calls: &[ToolCallRecord]) -> Vec<DisplayToolCard> {
    if tool_calls.is_empty() {
        return Vec::new();
    }
    // Render the last 12 interleaved tool steps per stable contract.
    let skip = tool_calls.len().saturating_sub(12);
    tool_calls[skip..]
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
                    val.get("old_text").and_then(|v| v.as_str()),
                    val.get("new_text").and_then(|v| v.as_str()),
                )
            {
                diff_lines = compute_unified_line_diff(old_t, new_t)
                    .into_iter()
                    .take(12)
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
                result_preview = clip_preview(&tc.result_text, 240);
            }
            DisplayToolCard {
                title: SharedString::from(format!("Tool: {}", tc.name)),
                status_meta: SharedString::from(format!("{status_lbl} · {}ms", tc.duration_ms)),
                is_error: tc.is_error,
                diff_lines,
                result_preview,
            }
        })
        .collect()
}

/// Renders one message turn in either Bubble (`EditorialTurns`) or Page (`Manuscript`) view.
pub fn render_message_row(
    ix: usize,
    msg: &DisplayMessageRow,
    view_mode: ViewMode,
    palette: &ParchmentPalette,
    show_thinking_cards: bool,
    show_tool_cards: bool,
) -> AnyElement {
    let canvas = palette.canvas_bg.to_gpui_rgb();
    let sheet = palette.sheet_bg.to_gpui_rgb();
    let card = palette.card_bg.to_gpui_rgb();
    let elev = palette.elevated_bg.to_gpui_rgb();
    let border = palette.border.to_gpui_rgb();
    let ink = palette.primary_ink.to_gpui_rgb();
    let muted = palette.muted_ink.to_gpui_rgb();
    let accent = palette.accent.to_gpui_rgb();
    let danger = palette.danger.to_gpui_rgb();
    let success = palette.success.to_gpui_rgb();

    // Parse extra math/SVG callouts only if markers appear (fast O(N) precheck keeps streaming 0-overhead).
    let content_str = msg.content.as_ref();
    let mut extras: Vec<AnyElement> = Vec::new();
    if content_str.contains("$$") || content_str.contains("\\[") || content_str.contains("<svg") {
        for (b_idx, block) in parse_prose_blocks(content_str, true)
            .into_iter()
            .enumerate()
        {
            match block {
                ProseBlock::MathBlock { raw_latex } => {
                    extras.push(
                        div()
                            .id(("math-blk", ix * 64 + b_idx))
                            .mt_2()
                            .px_3()
                            .py_2()
                            .rounded(px(8.))
                            .bg(elev)
                            .border_1()
                            .border_color(border)
                            .text_sm()
                            .text_color(ink)
                            .child(render_latex_unicode(&raw_latex))
                            .into_any_element(),
                    );
                }
                ProseBlock::SvgBlock { raw_svg } => {
                    if let Some(svg_doc) = parse_svg_bounded(&raw_svg) {
                        extras.push(
                            div()
                                .id(("svg-blk", ix * 64 + b_idx))
                                .mt_2()
                                .px_3()
                                .py_2()
                                .rounded(px(8.))
                                .bg(elev)
                                .border_1()
                                .border_color(border)
                                .text_xs()
                                .text_color(muted)
                                .child(format!(
                                    "SVG vector scene ({:.0}×{:.0}, {} primitives{})",
                                    svg_doc.width,
                                    svg_doc.height,
                                    svg_doc.items.len(),
                                    if svg_doc.truncated { ", bounded" } else { "" }
                                ))
                                .into_any_element(),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    // Main text body (native GPUI selectable Markdown).
    let body_el = v_flex()
        .w_full()
        .gap_1()
        .child(TextView::markdown(("msg-md", ix), msg.content.clone()).selectable(true))
        .children(extras);

    // Thinking section sits OUTSIDE the bubble (per docs/DESIGN.md Principle #1).
    let thinking_el = if !msg.is_user && show_thinking_cards && !msg.thinking_preview.is_empty() {
        Some(
            v_flex()
                .mb_1()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .bg(elev)
                .border_l_2()
                .border_color(accent)
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(muted)
                        .child("Thinking"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(msg.thinking_preview.clone()),
                ),
        )
    } else {
        None
    };

    // Precomputed tool-call drawers (zero JSON parse or diffing on render).
    let tool_els: Vec<AnyElement> = if !msg.is_user && show_tool_cards && !msg.tool_cards.is_empty()
    {
        msg.tool_cards
            .iter()
            .enumerate()
            .map(|(t_idx, tc)| {
                let status_col = if tc.is_error { danger } else { success };
                let mut card_box = v_flex()
                    .id(("tool-card", ix * 32 + t_idx))
                    .mb_1()
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .bg(elev)
                    .border_1()
                    .border_color(border)
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(ink)
                                    .child(tc.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(status_col)
                                    .child(tc.status_meta.clone()),
                            ),
                    );

                if !tc.diff_lines.is_empty() {
                    for (kind, line) in &tc.diff_lines {
                        let col = match kind {
                            DiffLineKind::Added => success,
                            DiffLineKind::Removed => danger,
                            DiffLineKind::Context | DiffLineKind::Collapsed => muted,
                        };
                        card_box =
                            card_box.child(div().text_xs().text_color(col).child(line.clone()));
                    }
                } else if !tc.result_preview.is_empty() {
                    card_box = card_box.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(tc.result_preview.clone()),
                    );
                }
                card_box.into_any_element()
            })
            .collect()
    } else {
        Vec::new()
    };

    let footer_el = (!msg.footer_text.is_empty()).then(|| {
        div()
            .mt_1()
            .text_xs()
            .text_color(muted)
            .child(msg.footer_text.clone())
    });

    match view_mode {
        ViewMode::Manuscript => {
            // Page view: single continuous sheet; user turns carry left rubric accent mark.
            let mut row = v_flex()
                .w_full()
                .px_6()
                .py_3()
                .bg(canvas)
                .border_b_1()
                .border_color(border);
            if msg.is_user {
                row = row.border_l_2().border_color(accent).bg(sheet);
            }
            row.children(thinking_el)
                .children(tool_els)
                .child(div().text_color(ink).child(body_el))
                .children(footer_el)
                .into_any_element()
        }
        ViewMode::EditorialTurns => {
            // Bubble view: user bubble on the right, assistant on the left, thinking/tools outside.
            let bubble_bg = if msg.is_user { sheet } else { card };
            let inner = v_flex()
                .max_w(px(760.))
                .children(thinking_el)
                .children(tool_els)
                .child(
                    div()
                        .px_4()
                        .py_3()
                        .rounded(px(12.))
                        .bg(bubble_bg)
                        .border_1()
                        .border_color(border)
                        .text_color(ink)
                        .child(body_el),
                )
                .children(footer_el);

            let row = if msg.is_user {
                h_flex().w_full().px_6().py_2().justify_end().child(inner)
            } else {
                h_flex().w_full().px_6().py_2().justify_start().child(inner)
            };
            row.into_any_element()
        }
    }
}
