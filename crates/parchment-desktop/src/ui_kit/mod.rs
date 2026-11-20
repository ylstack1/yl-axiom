//! Parchment Design System primitives matching `docs/design.md`, `DesignTokens.kt`,
//! `ParchmentComponents.kt`, and `ParchmentModelPickerAndStats.kt`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Bounds, ClickEvent, DispatchPhase, Div, Element,
    ElementId, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    InteractiveElement as _, IntoElement, LayoutId, Length, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, SharedString, SpringAnimation,
    SpringConfig, SpringState, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
    canvas, div, point, pulsating_between, px,
};
use gpui_component::Icon;
use gpui_component::tooltip::Tooltip;
use gpui_component::{StyledExt as _, h_flex, v_flex};
use parchment_core::model::{ModelPricingSpec, ProviderProfile, ProviderProtocol};

use crate::icons::{KissIcon, assistant_icon_from_symbol, kiss_icon};
use crate::theme::ParchmentPalette;
mod motion;
pub use motion::{
    KissBounce, SPRING_BOUNCE, kiss_bounce, kiss_bounce_subtle, spring_pop_enter,
    spring_slide_drawer, streaming_pulse_dot,
};

/// Attaches a crisp hover tooltip matching Kotlin's dark pill tooltips (`Voice input`, `Attach file`, etc.).
pub fn with_tooltip(el: KissBounce<Stateful<Div>>, tip: &'static str) -> KissBounce<Stateful<Div>> {
    el.map_element(move |inner| {
        inner.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SepiaButtonStyle {
    Primary,
    Secondary,
    Outline,
    Ghost,
    Danger,
}

/// Full-height inset rubric mark (`verticalInset` matching `Modifier.rubricMark`) inside a `relative()` row.
pub fn rubric_mark_overlay(vertical_inset_px: f32, palette: &ParchmentPalette) -> AnyElement {
    div()
        .absolute()
        .left_0()
        .top(px(vertical_inset_px))
        .bottom(px(vertical_inset_px))
        .w(px(2.5))
        .rounded(px(2.0))
        .bg(palette.accent.to_hsla())
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub fn sepia_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<KissIcon>,
    style: SepiaButtonStyle,
    compact: bool,
    disabled: bool,
    palette: &ParchmentPalette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> KissBounce<Stateful<Div>> {
    let el_id: ElementId = id.into();
    let bounce_id = (el_id.clone(), "kb");
    let label_str: SharedString = label.into();
    let has_label = !label_str.is_empty();
    let (bg, fg, border_col, hover_bg, active_bg, hover_fg, hover_border, active_border) =
        match style {
            SepiaButtonStyle::Primary => (
                palette.accent.to_hsla(),
                palette.on_accent_ink.to_hsla(),
                palette.accent.to_hsla(),
                palette.accent.lerp(palette.primary_ink, 0.10).to_hsla(),
                palette.accent.lerp(palette.primary_ink, 0.22).to_hsla(),
                palette.on_accent_ink.to_hsla(),
                palette.accent.lerp(palette.primary_ink, 0.10).to_hsla(),
                palette.accent.lerp(palette.primary_ink, 0.22).to_hsla(),
            ),
            SepiaButtonStyle::Secondary => (
                palette.elevated_bg.to_hsla(),
                palette.primary_ink.to_hsla(),
                palette.border.to_hsla(),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.18 } else { 0.10 }),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.28 } else { 0.18 }),
                palette.accent.to_hsla(),
                palette.accent.with_alpha(0.68),
                palette.accent.to_hsla(),
            ),
            SepiaButtonStyle::Outline => (
                palette.card_bg.to_hsla(),
                palette.primary_ink.to_hsla(),
                palette.border.to_hsla(),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.16 } else { 0.08 }),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.26 } else { 0.16 }),
                palette.accent.to_hsla(),
                palette.accent.with_alpha(0.78),
                palette.accent.to_hsla(),
            ),
            SepiaButtonStyle::Ghost => (
                palette.canvas_bg.with_alpha(0.0),
                palette.narration_ink.to_hsla(),
                palette.canvas_bg.with_alpha(0.0),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.16 } else { 0.08 }),
                palette
                    .accent
                    .with_alpha(if palette.is_dark { 0.26 } else { 0.15 }),
                palette.accent.to_hsla(),
                palette.accent.with_alpha(0.32),
                palette.accent.with_alpha(0.48),
            ),
            SepiaButtonStyle::Danger => (
                palette.danger.to_hsla(),
                palette.on_danger_ink.to_hsla(),
                palette.danger.to_hsla(),
                palette.danger.lerp(palette.primary_ink, 0.10).to_hsla(),
                palette.danger.lerp(palette.primary_ink, 0.22).to_hsla(),
                palette.on_danger_ink.to_hsla(),
                palette.danger.lerp(palette.primary_ink, 0.10).to_hsla(),
                palette.danger.lerp(palette.primary_ink, 0.22).to_hsla(),
            ),
        };
    let height = if compact { px(34.0) } else { px(38.0) };
    let pad_x = if !has_label {
        px(9.0)
    } else if compact {
        px(10.0)
    } else {
        px(14.0)
    };
    let icon_size = if compact { 13.0 } else { 14.0 };

    // Disabled buttons keep their outline and dimmed fill so they still read as buttons (`docs/design.md`).
    let resolved_bg = if disabled {
        match style {
            SepiaButtonStyle::Primary | SepiaButtonStyle::Danger => palette.border.with_alpha(0.55),
            SepiaButtonStyle::Ghost => palette.canvas_bg.with_alpha(0.0),
            _ => palette.elevated_bg.with_alpha(0.45),
        }
    } else {
        bg
    };
    let resolved_border = if disabled {
        if style == SepiaButtonStyle::Ghost {
            palette.canvas_bg.with_alpha(0.0)
        } else {
            palette.border.to_hsla()
        }
    } else {
        border_col
    };
    let resolved_fg = if disabled {
        palette.muted_ink.with_alpha(0.88)
    } else {
        fg
    };

    let mut el = div()
        .id(el_id)
        .h(height)
        .px(pad_x)
        .rounded(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(5.0))
        .flex_shrink_0()
        .border_1()
        .border_color(resolved_border)
        .bg(resolved_bg)
        .text_color(resolved_fg)
        .text_size(px(if compact { 12.5 } else { 13.5 }))
        .font_medium();

    if !disabled {
        el = el
            .cursor_pointer()
            .hover(move |s| {
                s.bg(hover_bg)
                    .border_color(hover_border)
                    .text_color(hover_fg)
            })
            .active(move |s| {
                s.bg(active_bg)
                    .border_color(active_border)
                    .text_color(hover_fg)
            })
            .on_click(on_click);
    }
    if let Some(ic) = icon {
        el = el.child(kiss_icon(ic, icon_size, resolved_fg));
    }
    if has_label {
        el = el.child(label_str);
    }
    motion::kiss_bounce_opt(bounce_id, el, 2.5, !disabled)
}

/// Icon-only ghost/outline button for crisp toolbar and footer actions.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: KissIcon,
    selected: bool,
    palette: &ParchmentPalette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> KissBounce<Stateful<Div>> {
    let style = if selected {
        SepiaButtonStyle::Outline
    } else {
        SepiaButtonStyle::Ghost
    };
    let mut btn = sepia_button(id, "", Some(icon), style, true, false, palette, on_click);
    if selected {
        btn = btn
            .border_color(palette.accent.with_alpha(0.65))
            .text_color(palette.accent.to_hsla());
    }
    btn
}

/// Selectable rounded chip (`Radius.r8`, `ControlHeight.compact`) matching `SepiaChip` in Kotlin.
pub fn sepia_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: Option<KissIcon>,
    selected: bool,
    palette: &ParchmentPalette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> KissBounce<Stateful<Div>> {
    let el_id: ElementId = id.into();
    let bounce_id = (el_id.clone(), "kb");
    let label_str: SharedString = label.into();
    let has_label = !label_str.is_empty();
    let bg = if selected {
        palette
            .accent
            .with_alpha(if palette.is_dark { 0.22 } else { 0.14 })
    } else {
        palette.card_bg.to_hsla()
    };
    let border_col = if selected {
        palette.accent.to_hsla()
    } else {
        palette.border.with_alpha(0.85)
    };
    let fg = if selected {
        palette.accent.to_hsla()
    } else {
        palette.narration_ink.to_hsla()
    };
    let hover_bg = palette.hover_tint();
    let active_bg = palette.active_tint();
    let accent_hover_border = palette.accent.with_alpha(0.75);
    let accent_fg = palette.accent.to_hsla();

    let mut el = div()
        .id(el_id)
        .h(px(34.0))
        .px(if has_label { px(10.0) } else { px(9.0) })
        .rounded(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(5.0))
        .flex_shrink_0()
        .cursor_pointer()
        .bg(bg)
        .border_1()
        .border_color(border_col)
        .text_color(fg)
        .text_size(px(12.0))
        .hover(move |s| {
            s.bg(hover_bg)
                .border_color(accent_hover_border)
                .text_color(accent_fg)
        })
        .active(move |s| {
            s.bg(active_bg)
                .border_color(accent_fg)
                .text_color(accent_fg)
        })
        .on_click(on_click);
    el = if selected {
        el.font_semibold()
    } else {
        el.font_medium()
    };

    if let Some(ic) = icon {
        el = el.child(kiss_icon(ic, 14.0, fg));
    }
    if has_label {
        el = el.child(label_str);
    }
    kiss_bounce(bounce_id, el)
}

/// Circular assistant/user avatar container (`SepiaAvatarCircle` parity).
pub fn sepia_avatar_circle(
    symbol: &str,
    selected: bool,
    size_px: f32,
    palette: &ParchmentPalette,
) -> AnyElement {
    let icon = assistant_icon_from_symbol(symbol);
    let bg = if selected {
        palette.accent.with_alpha(0.14)
    } else {
        palette.elevated_bg.to_hsla()
    };
    let border_col = if selected {
        palette.accent.with_alpha(0.75)
    } else {
        palette.subtle_border()
    };
    let tint = if selected {
        palette.accent.to_hsla()
    } else {
        palette.narration_ink.to_hsla()
    };
    div()
        .w(px(size_px))
        .h(px(size_px))
        .rounded(px(999.0))
        .bg(bg)
        .border_1()
        .border_color(border_col)
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .child(kiss_icon(icon, (size_px * 0.48).round().max(12.0), tint))
        .into_any_element()
}

/// Full-width 48dp+ radio/check row matching `PermissionOption` in `AgentPermissionEditor.kt`.
pub fn permission_option_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    checked: bool,
    is_radio: bool,
    palette: &ParchmentPalette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let sub: SharedString = subtitle.into();
    let icon = if !checked {
        KissIcon::CircleEmpty
    } else if is_radio {
        KissIcon::CircleDot
    } else {
        KissIcon::Check
    };
    let icon_col = if checked {
        palette.accent.to_hsla()
    } else {
        palette.muted_ink.to_hsla()
    };
    let el_id: ElementId = id.into();
    let bounce_id = (el_id.clone(), "kb");
    let hover_bg = palette.hover_tint();
    let active_bg = palette.active_tint();
    kiss_bounce_subtle(
        bounce_id,
        h_flex()
            .id(el_id)
            .w_full()
            .min_h(px(44.0))
            .px(px(6.0))
            .py(px(8.0))
            .rounded(px(8.0))
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .active(move |s| s.bg(active_bg))
            .on_click(on_click)
            .child(
                v_flex()
                    .flex_1()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .text_color(palette.primary_ink.to_hsla())
                            .child(title.into()),
                    )
                    .children((!sub.is_empty()).then(|| {
                        div()
                            .text_size(px(12.0))
                            .text_color(palette.muted_ink.to_hsla())
                            .child(sub)
                    })),
            )
            .child(kiss_icon(icon, 20.0, icon_col)),
    )
    .into_any_element()
}

/// Crisp circular context progress ring SVG (`SepiaContextGaugeRing` parity).
pub fn context_gauge_ring_svg(
    fraction: f32,
    stroke_hex: u32,
    track_hex: u32,
    size_px: f32,
) -> AnyElement {
    let clamped = fraction.clamp(0.02, 1.0);
    let r = 8.5_f32;
    let circ = 2.0 * std::f32::consts::PI * r;
    let dash = (circ * clamped).max(1.2);
    let gap = (circ - dash).max(0.0);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="{r:.2}" stroke="#{track_hex:06X}" stroke-opacity="0.55" stroke-width="2.4"/><circle cx="12" cy="12" r="{r:.2}" stroke="#{stroke_hex:06X}" stroke-width="2.4" stroke-linecap="round" stroke-dasharray="{dash:.2} {gap:.2}" transform="rotate(-90 12 12)"/></svg>"##
    );
    Icon::default()
        .data(svg.as_bytes())
        .size(px(size_px))
        .into_any_element()
}

/// Monospace keyboard shortcut keycap pills (`Ctrl + K`), matching `ShortcutKeycapRow`.
pub fn shortcut_keycap_row(
    tokens: &[String],
    highlighted: bool,
    palette: &ParchmentPalette,
) -> AnyElement {
    let mut row = h_flex().items_center().gap(px(4.0));
    for (idx, tok) in tokens.iter().enumerate() {
        if idx > 0 {
            row = row.child(
                div()
                    .text_size(px(11.0))
                    .text_color(palette.muted_ink.to_hsla())
                    .child("+"),
            );
        }
        row = row.child(
            div()
                .px(px(7.0))
                .py(px(2.0))
                .rounded(px(6.0))
                .bg(if highlighted {
                    palette.accent.with_alpha(0.14)
                } else {
                    palette.sheet_bg.to_hsla()
                })
                .border_1()
                .border_color(if highlighted {
                    palette.accent.to_hsla()
                } else {
                    palette.border.to_hsla()
                })
                .font_family(palette.code_font_family.clone())
                .text_size(px(11.5))
                .font_medium()
                .text_color(if highlighted {
                    palette.accent.to_hsla()
                } else {
                    palette.primary_ink.to_hsla()
                })
                .child(tok.clone()),
        );
    }
    row.into_any_element()
}

/// Local-only privacy/retention badge matching `ModelPrivacyBadge` in `ParchmentModelPickerAndStats.kt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyBadgeTone {
    Positive,
    Neutral,
    Caution,
}

#[derive(Debug, Clone)]
pub struct ModelPrivacyBadge {
    pub label: &'static str,
    pub tone: PrivacyBadgeTone,
}

fn is_local_endpoint_url(base_url: &str) -> bool {
    let trimmed = base_url.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return false;
    }
    let after = trimmed
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(&trimmed);
    let host_port = after.split(['/', '?']).next().unwrap_or(after);
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    if matches!(host, "localhost" | "::1" | "0.0.0.0") || host.ends_with(".local") {
        return true;
    }
    if host.starts_with("127.") || host.starts_with("10.") || host.starts_with("192.168.") {
        return true;
    }
    if let Some(rest) = host.strip_prefix("172.")
        && let Some(sec) = rest.split('.').next().and_then(|s| s.parse::<u8>().ok())
        && (16..=31).contains(&sec)
    {
        return true;
    }
    false
}

pub fn resolve_model_privacy_badges(
    profile: &ProviderProfile,
    model_id: &str,
    spec: Option<&ModelPricingSpec>,
) -> Vec<ModelPrivacyBadge> {
    if is_local_endpoint_url(&profile.base_url) {
        return vec![ModelPrivacyBadge {
            label: "Local",
            tone: PrivacyBadgeTone::Positive,
        }];
    }
    let mut out = Vec::new();
    let supports_storage = matches!(
        profile.protocol,
        ProviderProtocol::GeminiInteractions | ProviderProtocol::OpenAiResponses
    ) || profile.keeps_provider_state();
    if supports_storage {
        if profile.keeps_provider_state() {
            out.push(ModelPrivacyBadge {
                label: "Stored",
                tone: PrivacyBadgeTone::Caution,
            });
        } else {
            out.push(ModelPrivacyBadge {
                label: "No Store",
                tone: PrivacyBadgeTone::Positive,
            });
        }
    }
    let kp = spec.and_then(|s| s.known_pricing.as_ref());
    let is_free = model_id.trim().to_ascii_lowercase().ends_with(":free")
        || kp.is_some_and(|p| p.input_per_m == Some(0.0) && p.output_per_m == Some(0.0));
    let is_paid = !is_free
        && spec.is_some_and(|s| {
            kp.and_then(|p| p.input_per_m)
                .unwrap_or(s.input_price_per_m)
                > 0.0
                || kp
                    .and_then(|p| p.output_per_m)
                    .unwrap_or(s.output_price_per_m)
                    > 0.0
        });
    if is_free {
        out.push(ModelPrivacyBadge {
            label: "Free Tier",
            tone: PrivacyBadgeTone::Caution,
        });
    } else if is_paid {
        out.push(ModelPrivacyBadge {
            label: "Paid API",
            tone: PrivacyBadgeTone::Neutral,
        });
    }
    out
}

pub fn primary_model_privacy_badge(
    profile: &ProviderProfile,
    model_id: &str,
    spec: Option<&ModelPricingSpec>,
) -> Option<ModelPrivacyBadge> {
    let badges = resolve_model_privacy_badges(profile, model_id, spec);
    badges
        .iter()
        .find(|b| b.tone == PrivacyBadgeTone::Caution)
        .or_else(|| badges.iter().find(|b| b.tone == PrivacyBadgeTone::Positive))
        .or_else(|| badges.first())
        .cloned()
}

pub fn model_privacy_badge_chip(
    badge: &ModelPrivacyBadge,
    palette: &ParchmentPalette,
) -> AnyElement {
    let tone = match badge.tone {
        PrivacyBadgeTone::Positive => palette.success,
        PrivacyBadgeTone::Caution => palette.danger,
        PrivacyBadgeTone::Neutral => palette.muted_ink,
    };
    let bg_alpha = if badge.tone == PrivacyBadgeTone::Neutral {
        0.10
    } else {
        0.13
    };
    div()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .bg(tone.with_alpha(bg_alpha))
        .border_1()
        .border_color(tone.with_alpha(0.45))
        .text_size(px(10.5))
        .font_medium()
        .text_color(tone.to_hsla())
        .child(badge.label)
        .into_any_element()
}

pub fn h_divider(palette: &ParchmentPalette) -> AnyElement {
    div()
        .w_full()
        .h(px(1.0))
        .bg(palette.border.with_alpha(0.42))
        .into_any_element()
}

pub fn v_divider(palette: &ParchmentPalette) -> AnyElement {
    div()
        .w(px(1.0))
        .h_full()
        .bg(palette.border.with_alpha(0.42))
        .into_any_element()
}

/// Framed card container for grouped settings controls (`SepiaSectionCard`).
pub fn section_card(title: &str, subtitle: Option<&str>, palette: &ParchmentPalette) -> Div {
    let mut card = v_flex()
        .w_full()
        .p(px(16.0))
        .rounded(px(12.0))
        .bg(palette.card_bg.to_hsla())
        .border_1()
        .border_color(palette.border.with_alpha(0.52))
        .gap(px(10.0));

    if !title.is_empty() {
        let mut hdr = v_flex().pb(px(2.0)).gap(px(2.0)).child(
            div()
                .text_size(px(15.0))
                .font_semibold()
                .text_color(palette.primary_ink.to_hsla())
                .child(title.to_string()),
        );
        if let Some(sub) = subtitle
            && !sub.is_empty()
        {
            hdr = hdr.child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.muted_ink.to_hsla())
                    .child(sub.to_string()),
            );
        }
        card = card.child(hdr);
    }
    card
}

/// Framed card with an optional right-aligned header action element (`SepiaSectionCard` headerAction).
pub fn section_card_with_action(
    title: &str,
    subtitle: Option<&str>,
    action: Option<AnyElement>,
    palette: &ParchmentPalette,
) -> Div {
    let mut card = v_flex()
        .w_full()
        .p(px(16.0))
        .rounded(px(12.0))
        .bg(palette.card_bg.to_hsla())
        .border_1()
        .border_color(palette.border.with_alpha(0.52))
        .gap(px(10.0));

    if !title.is_empty() || action.is_some() {
        let mut left = v_flex().flex_1().gap(px(2.0)).child(
            div()
                .text_size(px(15.0))
                .font_semibold()
                .text_color(palette.primary_ink.to_hsla())
                .child(title.to_string()),
        );
        if let Some(sub) = subtitle
            && !sub.is_empty()
        {
            left = left.child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.muted_ink.to_hsla())
                    .child(sub.to_string()),
            );
        }
        card = card.child(
            h_flex()
                .w_full()
                .pb(px(2.0))
                .justify_between()
                .items_center()
                .gap(px(10.0))
                .child(left)
                .children(action),
        );
    }
    card
}

/// Animated Parchment pill switch track (`SepiaToggleTrack` from `ParchmentFormComponents.kt`).
pub fn sepia_toggle_track(
    id: impl Into<ElementId>,
    checked: bool,
    palette: &ParchmentPalette,
) -> AnyElement {
    let off_bg = palette.border.with_alpha(0.78);
    let on_bg = palette.accent.to_hsla();
    let thumb_col = palette.on_accent_ink.to_hsla();
    div()
        .w(px(42.0))
        .h(px(24.0))
        .rounded(px(999.0))
        .p(px(3.0))
        .flex()
        .items_center()
        .flex_shrink_0()
        .with_spring(
            id,
            SpringAnimation::new(SPRING_BOUNCE).to(checked),
            move |el, phase| {
                let raw_t: f32 = phase.interpolate(0.0_f32, 1.0_f32);
                let t = raw_t.clamp(0.0, 1.0);
                let bg = phase.interpolate_clamped(off_bg, on_bg);
                let stretch: f32 = (t * std::f32::consts::PI).sin() * 3.8;
                let thumb_w: f32 = 18.0 + stretch;
                let shift: f32 = (raw_t * 18.0 - stretch * 0.5).clamp(-1.5, 19.5 - stretch);
                el.bg(bg).child(
                    div()
                        .w(px(thumb_w))
                        .h(px(18.0))
                        .ml(px(shift))
                        .rounded(px(999.0))
                        .bg(thumb_col)
                        .shadow_xs(),
                )
            },
        )
        .into_any_element()
}

/// Standalone clickable animated pill switch (`SepiaToggleSwitch` parity).
pub fn sepia_toggle_switch(
    id: impl Into<ElementId>,
    checked: bool,
    palette: &ParchmentPalette,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let el_id: ElementId = id.into();
    let track_id = (el_id.clone(), "trk");
    let bounce_id = (el_id.clone(), "kb");
    let hover_bg = palette.hover_tint();
    let active_bg = palette.active_tint();
    kiss_bounce(
        bounce_id,
        div()
            .id(el_id)
            .p(px(4.0))
            .rounded(px(999.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .active(move |s| s.bg(active_bg))
            .on_click(move |_, w, cx| {
                let next = !checked;
                on_change(&next, w, cx);
            })
            .child(sepia_toggle_track(track_id, checked, palette)),
    )
    .into_any_element()
}

/// Full-row clickable two-column toggle matching `SepiaToggleRow` (`ParchmentFormComponents.kt`).
pub fn setting_toggle_row(
    id: impl Into<ElementId>,
    title: &str,
    subtitle: &str,
    checked: bool,
    palette: &ParchmentPalette,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let el_id: ElementId = id.into();
    let track_id = (el_id.clone(), "trk");
    let bounce_id = (el_id.clone(), "kb");
    let hover_bg = palette.hover_tint();
    let active_bg = palette.active_tint();
    let accent_col = palette.accent.to_hsla();
    kiss_bounce_subtle(
        bounce_id,
        h_flex()
            .id(el_id)
            .w_full()
            .min_h(px(44.0))
            .px(px(6.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .justify_between()
            .items_center()
            .gap(px(12.0))
            .cursor_pointer()
            .text_color(palette.primary_ink.to_hsla())
            .hover(move |s| s.bg(hover_bg).text_color(accent_col))
            .active(move |s| s.bg(active_bg).text_color(accent_col))
            .on_click(move |_, w, cx| {
                let next = !checked;
                on_change(&next, w, cx);
            })
            .child(
                v_flex()
                    .flex_1()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_medium()
                            .child(title.to_string()),
                    )
                    .children((!subtitle.is_empty()).then(|| {
                        div()
                            .text_size(px(11.5))
                            .text_color(palette.muted_ink.to_hsla())
                            .child(subtitle.to_string())
                    })),
            )
            .child(sepia_toggle_track(track_id, checked, palette)),
    )
    .into_any_element()
}

/// Single-line label-left, controls-right settings row (`SepiaAdaptiveRow` parity).
pub fn adaptive_setting_row(
    label: &str,
    palette: &ParchmentPalette,
    control: impl IntoElement,
) -> AnyElement {
    h_flex()
        .w_full()
        .min_h(px(36.0))
        .px(px(6.0))
        .py(px(4.0))
        .justify_between()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .text_size(px(13.5))
                .font_medium()
                .text_color(palette.primary_ink.to_hsla())
                .child(label.to_string()),
        )
        .child(control)
        .into_any_element()
}

/// Maps pointer X within a slider track to the nearest stop index (`sliderIndexAt` in `ComposerControls.kt`).
pub fn slider_index_at(x: f32, width: f32, inset: f32, step_count: usize) -> usize {
    if step_count <= 1 {
        return 0;
    }
    let usable = (width - inset * 2.0).max(1.0);
    let frac = ((x - inset) / usable).clamp(0.0, 1.0);
    (frac * (step_count - 1) as f32).round() as usize
}

/// Interactive clickable & draggable stepped slider with spring-animated thumb
/// matching `StepSlider` in `ComposerControls.kt` 1:1.
pub fn step_slider(
    id: impl Into<ElementId>,
    step_count: usize,
    selected: Option<usize>,
    palette: &ParchmentPalette,
    on_select: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let el_id: ElementId = id.into();
    let spring_id = (el_id.clone(), "slide-spring");
    let count = step_count.max(2);
    let last = (count - 1) as f32;
    let target_frac = selected
        .map(|ix| (ix.min(count - 1) as f32) / last)
        .unwrap_or(-1.0);

    let bounds_cell: Rc<Cell<Bounds<Pixels>>> = Rc::new(Cell::new(Bounds::default()));
    let bounds_down = bounds_cell.clone();
    let bounds_move = bounds_cell.clone();
    let bounds_paint = bounds_cell;

    let cb = Rc::new(on_select);
    let cb_down = cb.clone();
    let cb_move = cb;

    let track_col = palette.border.lerp(palette.muted_ink, 0.22).to_hsla();
    let accent_col = palette.accent.to_hsla();
    let card_col = palette.card_bg.to_hsla();
    let inset = 11.0_f32;
    let has_selection = selected.is_some();

    let slider_h = px(44.0);
    let bp_pre = bounds_paint.clone();
    let bp_paint = bounds_paint;

    let mut track_layer = div()
        .absolute()
        .left(px(inset))
        .right(px(inset))
        .top_0()
        .bottom_0()
        // 1. Background rail (4px pill)
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(20.0))
                .h(px(4.0))
                .rounded(px(999.0))
                .bg(track_col),
        )
        // 2. Active accent fill + 3. Stop dots + 4. Spring-animated double-ring thumb
        .child(
            div()
                .size_full()
                .relative()
                .with_spring(
                    spring_id,
                    SpringAnimation::new(SPRING_BOUNCE).to(target_frac.max(0.0)),
                    move |el, anim_frac| {
                        let fill_frac = anim_frac.clamp(0.0, 1.0);
                        let thumb_frac = anim_frac.clamp(-0.04, 1.04);
                        let mut inner = el;
                        if has_selection && fill_frac > 0.001 {
                            inner = inner.child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top(px(20.0))
                                    .h(px(4.0))
                                    .w(gpui::relative(fill_frac))
                                    .rounded(px(999.0))
                                    .bg(accent_col),
                            );
                        }
                        if count <= 8 {
                            for i in 0..count {
                                let sf = i as f32 / last;
                                let dot_col = if has_selection && sf <= fill_frac + 0.01 {
                                    accent_col
                                } else {
                                    track_col
                                };
                                inner = inner.child(
                                    div()
                                        .absolute()
                                        .left(gpui::relative(sf))
                                        .ml(px(-3.5))
                                        .top(px(18.5))
                                        .w(px(7.0))
                                        .h(px(7.0))
                                        .rounded(px(999.0))
                                        .bg(dot_col),
                                );
                            }
                        }
                        if has_selection {
                            inner = inner.child(
                                div()
                                    .absolute()
                                    .left(gpui::relative(thumb_frac))
                                    .ml(px(-9.0))
                                    .top(px(13.0))
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .rounded(px(999.0))
                                    .bg(accent_col)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .shadow_xs()
                                    .child(
                                        div()
                                            .w(px(8.0))
                                            .h(px(8.0))
                                            .rounded(px(999.0))
                                            .bg(card_col),
                                    ),
                            );
                        }
                        inner
                    },
                ),
        );

    // Transparent bounds-probe canvas so pointer clicks & drags map X accurately.
    track_layer = track_layer.child(
        div().absolute().inset_0().child(
            canvas(
                move |bounds, _window, _cx| {
                    bp_pre.set(bounds);
                    bounds
                },
                move |bounds, _, _window, _cx| {
                    bp_paint.set(bounds);
                },
            )
            .size_full(),
        ),
    );

    div()
        .id(el_id)
        .relative()
        .w_full()
        .h(slider_h)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
            cx.stop_propagation();
            let b = bounds_down.get();
            let w = f32::from(b.size.width);
            if w > 4.0 {
                let lx = f32::from(ev.position.x - b.origin.x);
                let ix = slider_index_at(lx, w, 0.0, count);
                cb_down(ix, window, cx);
            }
        })
        .on_mouse_move(move |ev, window, cx| {
            if ev.pressed_button == Some(MouseButton::Left) {
                let b = bounds_move.get();
                let w = f32::from(b.size.width);
                if w > 4.0 {
                    let lx = f32::from(ev.position.x - b.origin.x);
                    let ix = slider_index_at(lx, w, 0.0, count);
                    if selected != Some(ix) {
                        cb_move(ix, window, cx);
                    }
                }
            }
        })
        .child(track_layer)
        .into_any_element()
}

/// Aspect-ratio preview tile with proportional outline (`AspectTile` in `ComposerControls.kt`).
pub fn aspect_tile(
    id: impl Into<ElementId>,
    ratio: &str,
    selected: bool,
    palette: &ParchmentPalette,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (rw, rh) = ratio
        .split_once(':')
        .and_then(|(a, b)| Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?)))
        .filter(|(a, b)| *a > 0.0 && *b > 0.0)
        .unwrap_or((1.0, 1.0));
    let scale = 28.0 / rw.max(rh);
    let box_w = (rw * scale).round().max(10.0);
    let box_h = (rh * scale).round().max(10.0);
    let stroke_col = if selected {
        palette.accent.to_hsla()
    } else {
        palette.muted_ink.to_hsla()
    };
    let el_id: ElementId = id.into();
    let bounce_id = (el_id.clone(), "kb");
    let hover_bg = palette.hover_tint();
    kiss_bounce(
        bounce_id,
        v_flex()
            .id(el_id)
            .px(px(7.0))
            .py(px(5.0))
            .rounded(px(8.0))
            .items_center()
            .gap(px(4.0))
            .cursor_pointer()
            .bg(if selected {
                palette.accent.with_alpha(0.14)
            } else {
                palette.canvas_bg.with_alpha(0.0)
            })
            .hover(move |s| s.bg(hover_bg))
            .on_click(on_click)
            .child(
                div()
                    .w(px(32.0))
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(box_w))
                            .h(px(box_h))
                            .rounded(px(3.0))
                            .border_1()
                            .border_color(stroke_col),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .font_medium()
                    .text_color(if selected {
                        palette.accent.to_hsla()
                    } else {
                        palette.primary_ink.to_hsla()
                    })
                    .child(ratio.to_string()),
            ),
    )
    .into_any_element()
}

pub fn field_label(text: &str, palette: &ParchmentPalette) -> AnyElement {
    div()
        .text_size(px(12.0))
        .font_medium()
        .text_color(palette.narration_ink.to_hsla())
        .child(text.to_string())
        .into_any_element()
}

/// Status pill (`On` green / `Off` amber) matching `SettingsProvidersPage.kt`.
pub fn status_pill(on: bool, palette: &ParchmentPalette) -> AnyElement {
    let col: Hsla = if on {
        palette.success.to_hsla()
    } else {
        palette.accent_secondary.to_hsla()
    };
    let bg = if on {
        palette.success.with_alpha(0.14)
    } else {
        palette.accent_secondary.with_alpha(0.14)
    };
    div()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .bg(bg)
        .text_size(px(11.0))
        .font_semibold()
        .text_color(col)
        .child(if on { "On" } else { "Off" })
        .into_any_element()
}

/// Compact human-readable timestamp (`Just now`, `5m ago`, `3h ago`, `2d ago`).
pub fn format_compact_timestamp(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return String::new();
    }
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff_s = ((now_ms - ts_ms) / 1000).max(0);
    if diff_s < 60 {
        "Just now".into()
    } else if diff_s < 3600 {
        format!("{}m ago", diff_s / 60)
    } else if diff_s < 86_400 {
        format!("{}h ago", diff_s / 3600)
    } else if diff_s < 86_400 * 30 {
        format!("{}d ago", diff_s / 86_400)
    } else {
        format!("{}mo ago", diff_s / (86_400 * 30))
    }
}

pub fn format_tokens_short(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        let m = (tokens / 100_000) as f64 / 10.0;
        if (m - m.trunc()).abs() < 1e-6 {
            format!("{}M", m as i64)
        } else {
            format!("{m:.1}M")
        }
    } else if tokens >= 10_000 {
        format!("{}k", tokens / 1000)
    } else if tokens >= 1_000 {
        format!("{:.1}k", (tokens / 100) as f64 / 10.0)
    } else {
        tokens.to_string()
    }
}

pub fn format_usd_short(usd: f64) -> String {
    if !usd.is_finite() || usd <= 0.0 {
        return String::new();
    }
    if usd < 0.0001 {
        "<$0.0001".into()
    } else if usd < 0.01 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

pub fn format_bytes_compact(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// Strips common Markdown formatting markers when copying plain text or speaking aloud.
pub fn strip_markdown_to_plain_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            continue;
        }
        let cleaned = trimmed.trim_start_matches('#').trim_start();
        let cleaned = cleaned
            .replace("**", "")
            .replace("__", "")
            .replace("~~", "")
            .replace('`', "");
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&cleaned);
    }
    out
}

/// Fuzzy match score ported from `CommandPalette.kt`: returns `Some(score)` when every char of
/// `query` appears in `text` in order (case-insensitive), rewarding consecutive & word-start hits.
pub fn fuzzy_match_score(text: &str, query: &str) -> Option<i32> {
    let q_chars: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if q_chars.is_empty() {
        return Some(0);
    }
    let t_chars: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut qi = 0;
    let mut score = 0i32;
    let mut prev_match: Option<usize> = None;
    for (ti, &ch) in t_chars.iter().enumerate() {
        if ch == q_chars[qi] {
            score += 10;
            if prev_match == Some(ti.saturating_sub(1)) {
                score += 15;
            }
            if ti == 0 || matches!(t_chars[ti - 1], ' ' | '-' | '_' | '/' | ':') {
                score += 12;
            }
            prev_match = Some(ti);
            qi += 1;
            if qi == q_chars.len() {
                return Some(score - (t_chars.len() as i32) / 8);
            }
        }
    }
    None
}
