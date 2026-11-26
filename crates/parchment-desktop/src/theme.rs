//! YL Axiom Design System theme palette, all 9 built-in presets, WCAG AA `>= 4.5:1` contrast
//! enforcement (`readable_ink`, `on_fill_ink`), bundled font registration, and `gpui-component`
//! global `Theme` synchronization matching `docs/design.md` and `ParchmentTheme.kt`.

use std::borrow::Cow;

use gpui::{App, Hsla, Rgba, SharedString, px, rgb};
use gpui_component::theme::{Theme, ThemeMode};
use parchment_core::model::ThemeConfig;

pub const READABLE_TEXT_CONTRAST: f32 = 4.5;
const WARM_FILL_INK: u32 = 0xFAF6EB;

/// 8 built-in accent swatches matching `SettingsPreferencesPage.kt`.
pub const ACCENT_SWATCHES: [(&str, &str); 8] = [
    ("#8B5A2B", "Brown"),
    ("#9E4730", "Rust"),
    ("#3B5998", "Blue"),
    ("#1F6F54", "Green"),
    ("#5E4B8B", "Purple"),
    ("#7A5216", "Bronze"),
    ("#8C3823", "Brick"),
    ("#2D3748", "Slate"),
];

pub const TEXT_FONTS: [&str; 4] = ["Inter", "Serif", "LaTeX", "JetBrains Mono"];
pub const CODE_FONTS: [&str; 3] = ["JetBrains Mono", "Inter", "Serif"];

/// Muted categorical chart and storage bar accents matching `DesignTokens.kt` (`CategoryColor`).
pub const CAT_SAGE: Rgb24 = Rgb24::from_hex_u32(0x4A7C59);
pub const CAT_SLATE: Rgb24 = Rgb24::from_hex_u32(0x3D6B8E);
pub const CAT_OCHRE: Rgb24 = Rgb24::from_hex_u32(0x9B5B34);
pub const CAT_BRASS: Rgb24 = Rgb24::from_hex_u32(0xB5892E);

static INTER_REGULAR: &[u8] =
    include_bytes!("../../../mobile/compose-app/src/commonMain/resources/fonts/Inter-Regular.ttf");
static INTER_ITALIC: &[u8] =
    include_bytes!("../../../mobile/compose-app/src/commonMain/resources/fonts/Inter-Italic.ttf");
static JETBRAINS_REGULAR: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/JetBrainsMono-Regular.ttf"
);
static JETBRAINS_ITALIC: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/JetBrainsMono-Italic.ttf"
);
static JETBRAINS_SEMIBOLD: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/JetBrainsMono-SemiBold.ttf"
);
static JETBRAINS_BOLD: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/JetBrainsMono-Bold.ttf"
);
static KATEX_MAIN_REGULAR: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/KaTeX_Main-Regular.ttf"
);
static KATEX_MAIN_BOLD: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/KaTeX_Main-Bold.ttf"
);
static KATEX_MATH_ITALIC: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/KaTeX_Math-Italic.ttf"
);
static NOTO_MATH_REGULAR: &[u8] = include_bytes!(
    "../../../mobile/compose-app/src/commonMain/resources/fonts/NotoSansMath-Regular.ttf"
);

/// Registers core UI/code fonts (`Inter-Regular`, `JetBrainsMono-Regular`) needed for Frame 1.
pub fn register_primary_fonts(cx: &App) {
    let fonts = vec![
        Cow::Borrowed(INTER_REGULAR),
        Cow::Borrowed(JETBRAINS_REGULAR),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

/// Registers secondary italic, bold, semibold, and math faces right after Frame 1 opens.
pub fn register_secondary_fonts(cx: &App) {
    let fonts = vec![
        Cow::Borrowed(INTER_ITALIC),
        Cow::Borrowed(JETBRAINS_ITALIC),
        Cow::Borrowed(JETBRAINS_SEMIBOLD),
        Cow::Borrowed(JETBRAINS_BOLD),
        Cow::Borrowed(KATEX_MAIN_REGULAR),
        Cow::Borrowed(KATEX_MAIN_BOLD),
        Cow::Borrowed(KATEX_MATH_ITALIC),
        Cow::Borrowed(NOTO_MATH_REGULAR),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

const MAX_CUSTOM_FONT_BYTES: u64 = 32 * 1024 * 1024;

/// Registers a user-chosen `.ttf`/`.otf` font file with GPUI's text system (bounded to 32 MiB).
pub fn register_custom_font_file(path: &str, cx: &App) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let meta = std::fs::metadata(trimmed).map_err(|e| format!("Could not read font file: {e}"))?;
    if meta.len() > MAX_CUSTOM_FONT_BYTES {
        return Err("Font file exceeds the 32 MiB limit.".into());
    }
    let bytes = std::fs::read(trimmed).map_err(|e| format!("Could not read font file: {e}"))?;
    cx.text_system()
        .add_fonts(vec![Cow::Owned(bytes)])
        .map_err(|e| format!("Could not register font: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb24 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb24 {
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };

    pub const fn from_hex_u32(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as u8,
            g: ((hex >> 8) & 0xFF) as u8,
            b: (hex & 0xFF) as u8,
        }
    }

    pub const fn to_hex_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Win32 `COLORREF` (`0x00BBGGRR`) for `DwmSetWindowAttribute`.
    pub const fn to_colorref(self) -> u32 {
        ((self.b as u32) << 16) | ((self.g as u32) << 8) | (self.r as u32)
    }

    pub fn to_gpui_rgb(self) -> Rgba {
        rgb(self.to_hex_u32())
    }

    pub fn to_hsla(self) -> Hsla {
        self.to_gpui_rgb().into()
    }

    pub fn with_alpha(self, alpha: f32) -> Hsla {
        let mut hsla: Hsla = self.to_gpui_rgb().into();
        hsla.a = alpha.clamp(0.0, 1.0);
        hsla
    }

    pub fn luminance(self) -> f32 {
        fn linear(c: u8) -> f32 {
            let v = (c as f32) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let blend = |a: u8, b: u8| -> u8 {
            ((a as f32) + ((b as f32) - (a as f32)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self {
            r: blend(self.r, other.r),
            g: blend(self.g, other.g),
            b: blend(self.b, other.b),
        }
    }
}

pub fn contrast_ratio(a: Rgb24, b: Rgb24) -> f32 {
    let la = a.luminance();
    let lb = b.luminance();
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub fn parse_hex_color(hex: &str, fallback: Rgb24) -> Rgb24 {
    let cleaned = hex.trim().trim_start_matches('#');
    let hex_slice = match cleaned.len() {
        6 => cleaned,
        8 => &cleaned[2..],
        _ => return fallback,
    };
    u32::from_str_radix(hex_slice, 16)
        .map(Rgb24::from_hex_u32)
        .unwrap_or(fallback)
}

/// Nudges `ink` toward white or black only as far as needed to stay `>= 4.5:1` on every surface.
pub fn readable_ink(ink: Rgb24, surfaces: &[Rgb24]) -> Rgb24 {
    let worst = |c: Rgb24| -> f32 {
        surfaces
            .iter()
            .map(|&s| contrast_ratio(c, s))
            .fold(f32::INFINITY, f32::min)
    };
    if worst(ink) >= READABLE_TEXT_CONTRAST {
        return ink;
    }
    let target = if worst(Rgb24::WHITE) >= worst(Rgb24::BLACK) {
        Rgb24::WHITE
    } else {
        Rgb24::BLACK
    };
    for step in 1..=20 {
        let candidate = ink.lerp(target, (step as f32) / 20.0);
        if worst(candidate) >= READABLE_TEXT_CONTRAST {
            return candidate;
        }
    }
    target
}

/// Readable foreground ink drawn on a solid filled control (`Button`, badge, pill).
pub fn on_fill_ink(fill: Rgb24) -> Rgb24 {
    let warm = Rgb24::from_hex_u32(WARM_FILL_INK);
    if contrast_ratio(warm, fill) >= READABLE_TEXT_CONTRAST {
        warm
    } else if contrast_ratio(Rgb24::WHITE, fill) >= contrast_ratio(Rgb24::BLACK, fill) {
        Rgb24::WHITE
    } else {
        Rgb24::BLACK
    }
}

#[derive(Debug, Clone)]
pub struct ParchmentPalette {
    pub is_dark: bool,
    pub canvas_bg: Rgb24,
    pub sheet_bg: Rgb24,
    pub card_bg: Rgb24,
    pub elevated_bg: Rgb24,
    pub border: Rgb24,
    pub primary_ink: Rgb24,
    pub dialogue_ink: Rgb24,
    pub narration_ink: Rgb24,
    pub muted_ink: Rgb24,
    pub accent: Rgb24,
    pub accent_secondary: Rgb24,
    pub on_accent_ink: Rgb24,
    pub danger: Rgb24,
    pub on_danger_ink: Rgb24,
    pub success: Rgb24,
    pub prose_font_family: SharedString,
    pub code_font_family: SharedString,
    pub prose_font_size_px: f32,
    pub code_font_size_px: f32,
    pub line_height_mult: f32,
    pub paragraph_spacing_px: f32,
    pub max_reading_width_px: f32,
}

impl ParchmentPalette {
    pub fn from_config(cfg: &ThemeConfig) -> Self {
        let canvas_bg = parse_hex_color(&cfg.canvas_bg_hex, Rgb24::from_hex_u32(0xF4ECD8));
        let sheet_bg = parse_hex_color(&cfg.sheet_bg_hex, Rgb24::from_hex_u32(0xEBE0C5));
        let card_bg = parse_hex_color(&cfg.card_bg_hex, Rgb24::from_hex_u32(0xFAF6EB));
        let elevated_bg = parse_hex_color(&cfg.elevated_bg_hex, Rgb24::from_hex_u32(0xEFE5CC));
        let border = parse_hex_color(&cfg.border_hex, Rgb24::from_hex_u32(0xD5C4A1));
        let primary_ink = parse_hex_color(&cfg.primary_ink_hex, Rgb24::from_hex_u32(0x261C14));
        let dialogue_ink = parse_hex_color(&cfg.dialogue_ink_hex, Rgb24::from_hex_u32(0x4A2511));
        let narration_ink = parse_hex_color(&cfg.narration_ink_hex, Rgb24::from_hex_u32(0x5C4938));
        let surfaces = [canvas_bg, sheet_bg, card_bg, elevated_bg];
        let muted_ink = readable_ink(
            parse_hex_color(&cfg.muted_ink_hex, Rgb24::from_hex_u32(0x826F5C)),
            &surfaces,
        );
        let accent = readable_ink(
            parse_hex_color(&cfg.accent_hex, Rgb24::from_hex_u32(0x8B5A2B)),
            &surfaces,
        );
        let accent_secondary = readable_ink(
            parse_hex_color(&cfg.accent_secondary_hex, Rgb24::from_hex_u32(0xB57C2E)),
            &surfaces,
        );
        let danger = readable_ink(
            parse_hex_color(&cfg.danger_hex, Rgb24::from_hex_u32(0xA62B2B)),
            &surfaces,
        );
        let success = readable_ink(Rgb24::from_hex_u32(0x2E7D32), &surfaces);
        let is_dark = cfg.color_mode.eq_ignore_ascii_case("dark") || canvas_bg.luminance() < 0.2;

        let prose_font: SharedString = if !cfg.custom_prose_font_path.trim().is_empty()
            && !cfg.prose_font_family.trim().is_empty()
        {
            SharedString::from(cfg.prose_font_family.clone())
        } else {
            SharedString::from(resolve_prose_font(&cfg.prose_font_family))
        };
        let code_font: SharedString = if !cfg.custom_code_font_path.trim().is_empty()
            && !cfg.code_font_family.trim().is_empty()
        {
            SharedString::from(cfg.code_font_family.clone())
        } else {
            SharedString::from(resolve_code_font(&cfg.code_font_family))
        };
        Self {
            is_dark,
            canvas_bg,
            sheet_bg,
            card_bg,
            elevated_bg,
            border,
            primary_ink,
            dialogue_ink,
            narration_ink,
            muted_ink,
            accent,
            accent_secondary,
            on_accent_ink: on_fill_ink(accent),
            danger,
            on_danger_ink: on_fill_ink(danger),
            success,
            prose_font_family: prose_font,
            code_font_family: code_font,
            prose_font_size_px: cfg.prose_font_size_sp.clamp(8.0, 48.0),
            code_font_size_px: cfg.code_font_size_sp.clamp(8.0, 48.0),
            line_height_mult: cfg.prose_line_height_multiplier.clamp(1.0, 3.0),
            paragraph_spacing_px: cfg.paragraph_spacing_dp.clamp(0.0, 64.0),
            max_reading_width_px: cfg.max_reading_width_dp.clamp(300.0, 2400.0),
        }
    }

    pub fn user_bubble_bg(&self) -> Hsla {
        self.accent
            .with_alpha(if self.is_dark { 0.22 } else { 0.16 })
    }

    pub fn hover_tint(&self) -> Hsla {
        self.accent
            .with_alpha(if self.is_dark { 0.16 } else { 0.08 })
    }

    pub fn active_tint(&self) -> Hsla {
        self.accent
            .with_alpha(if self.is_dark { 0.24 } else { 0.14 })
    }

    pub fn subtle_border(&self) -> Hsla {
        self.border.with_alpha(0.55)
    }
}

fn resolve_prose_font(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "serif" | "georgia" => "Georgia",
        "latex" | "katex" | "katex_main" => "KaTeX_Main",
        "jetbrains mono" | "jetbrainsmono" | "jetbrainsmono nf" | "mono" | "monospace" => {
            "JetBrainsMono NF"
        }
        "segoe ui" => "Segoe UI",
        _ => "Inter",
    }
}

fn resolve_code_font(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "consolas" => "Consolas",
        "inter" | "sans" => "Inter",
        "serif" | "georgia" => "Georgia",
        _ => "JetBrainsMono NF",
    }
}

/// Synchronizes `ParchmentPalette` into `gpui-component`'s global `Theme` so `Input`, `Textarea`,
/// `TextView`, `Switch`, `Scrollbar`, selection highlights, focus rings, and popovers paint
/// seamlessly in Parchment's active theme instead of default cold zinc/blue colors.
pub fn sync_gpui_component_theme(palette: &ParchmentPalette, cx: &mut App) {
    let target_mode = if palette.is_dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    if Theme::global(cx).mode != target_mode {
        Theme::update(cx, |theme| {
            theme.mode = target_mode;
        });
    }

    let canvas = palette.canvas_bg.to_hsla();
    let sheet = palette.sheet_bg.to_hsla();
    let card = palette.card_bg.to_hsla();
    let elev = palette.elevated_bg.to_hsla();
    let border = palette.border.to_hsla();
    let ink = palette.primary_ink.to_hsla();
    let muted = palette.muted_ink.to_hsla();
    let accent = palette.accent.to_hsla();
    let on_accent = palette.on_accent_ink.to_hsla();
    let accent_hover = palette.accent.lerp(palette.primary_ink, 0.12).to_hsla();
    let accent_active = palette.accent.lerp(palette.primary_ink, 0.22).to_hsla();
    let danger = palette.danger.to_hsla();
    let on_danger = palette.on_danger_ink.to_hsla();
    let danger_hover = palette.danger.lerp(palette.primary_ink, 0.12).to_hsla();
    let success = palette.success.to_hsla();
    let hover_bg = palette.hover_tint();
    let active_bg = palette.active_tint();
    let selection = palette
        .accent
        .with_alpha(if palette.is_dark { 0.34 } else { 0.22 });
    let scroll_thumb = palette.muted_ink.with_alpha(0.32);
    let scroll_thumb_hover = palette.muted_ink.with_alpha(0.55);

    Theme::update(cx, |theme| {
        theme.font_family = palette.prose_font_family.clone();
        theme.font_size = px(palette.prose_font_size_px);
        theme.mono_font_family = palette.code_font_family.clone();
        theme.mono_font_size = px(palette.code_font_size_px);
        theme.radius = px(8.0);
        theme.radius_lg = px(12.0);
        theme.focus_ring = true;

        let c = &mut theme.colors;
        c.background = card;
        c.foreground = ink;
        c.border = border;
        c.input = border;
        c.ring = accent;
        c.caret = accent;
        c.selection = selection;

        c.primary = accent;
        c.primary_foreground = on_accent;
        c.primary_hover = accent_hover;
        c.primary_active = accent_active;

        c.secondary = elev;
        c.secondary_foreground = ink;
        c.secondary_hover = hover_bg;
        c.secondary_active = active_bg;

        c.accent = hover_bg;
        c.accent_foreground = ink;

        c.muted = sheet;
        c.muted_foreground = muted;

        c.popover = elev;
        c.popover_foreground = ink;

        c.danger = danger;
        c.danger_foreground = on_danger;
        c.danger_hover = danger_hover;
        c.danger_active = danger_hover;

        c.success = success;
        c.success_foreground = on_accent;

        c.button = card;
        c.button_foreground = ink;
        c.button_hover = hover_bg;
        c.button_active = active_bg;
        c.button_primary = accent;
        c.button_primary_foreground = on_accent;
        c.button_primary_hover = accent_hover;
        c.button_primary_active = accent_active;
        c.button_secondary = elev;
        c.button_secondary_foreground = ink;
        c.button_secondary_hover = hover_bg;
        c.button_secondary_active = active_bg;
        c.button_danger = danger;
        c.button_danger_foreground = on_danger;
        c.button_danger_hover = danger_hover;
        c.button_danger_active = danger_hover;

        c.link = accent;
        c.link_hover = accent_hover;
        c.link_active = accent_active;

        c.sidebar = sheet;
        c.sidebar_foreground = ink;
        c.sidebar_border = border;
        c.sidebar_accent = card;
        c.sidebar_accent_foreground = accent;
        c.sidebar_primary = accent;
        c.sidebar_primary_foreground = on_accent;

        c.list = canvas;
        c.list_even = sheet;
        c.list_head = sheet;
        c.list_hover = hover_bg;
        c.list_active = card;
        c.list_active_border = accent;

        c.table = card;
        c.table_even = elev;
        c.table_head = sheet;
        c.table_head_foreground = ink;
        c.table_hover = hover_bg;
        c.table_row_border = border;

        c.tab_bar = sheet;
        c.tab = sheet;
        c.tab_foreground = muted;
        c.tab_active = card;
        c.tab_active_foreground = ink;

        c.switch = border;
        c.switch_thumb = card;
        c.progress_bar = accent;

        c.scrollbar = palette.canvas_bg.with_alpha(0.0);
        c.scrollbar_thumb = scroll_thumb;
        c.scrollbar_thumb_hover = scroll_thumb_hover;

        c.group_box = card;
        c.group_box_foreground = ink;
        c.title_bar = canvas;
        c.title_bar_border = border;
        c.status_bar = sheet;
        c.status_bar_border = border;
    });
}

/// All 9 built-in YL Axiom theme presets (exact hex parity with v2 `ThemeConfig.PRESETS`).
pub fn theme_presets() -> Vec<ThemeConfig> {
    let make = |name: &str,
                mode: &str,
                canvas: &str,
                sheet: &str,
                card: &str,
                elev: &str,
                border: &str,
                ink: &str,
                dlg: &str,
                narr: &str,
                muted: &str,
                accent: &str,
                accent2: &str,
                danger: &str| {
        ThemeConfig {
            name: name.into(),
            color_mode: mode.into(),
            canvas_bg_hex: canvas.into(),
            sheet_bg_hex: sheet.into(),
            card_bg_hex: card.into(),
            elevated_bg_hex: elev.into(),
            border_hex: border.into(),
            primary_ink_hex: ink.into(),
            dialogue_ink_hex: dlg.into(),
            narration_ink_hex: narr.into(),
            muted_ink_hex: muted.into(),
            accent_hex: accent.into(),
            accent_secondary_hex: accent2.into(),
            danger_hex: danger.into(),
            ..ThemeConfig::default()
        }
    };

    vec![
        ThemeConfig::default(), // 1. Parchment & Espresso Ink (Sepia)
        make(
            "Lapis on Vellum",
            "Light",
            "#EFEEE3",
            "#E6E5D8",
            "#F8F7EF",
            "#E9E8DB",
            "#D4D3C2",
            "#1D2230",
            "#24345F",
            "#4A4F5C",
            "#666A72",
            "#2A4A9E",
            "#8F6A12",
            "#B3261E",
        ),
        make(
            "Lapis at Night",
            "Dark",
            "#12151C",
            "#0E1117",
            "#191D26",
            "#222733",
            "#2C3240",
            "#E6E4D8",
            "#B9C6EC",
            "#A9ACB3",
            "#8A8F99",
            "#8FA6E8",
            "#D4B25A",
            "#F0857A",
        ),
        make(
            "YL Axiom Warm Ivory (Light)",
            "Light",
            "#F8F7F4",
            "#EFECE6",
            "#FFFFFF",
            "#F3F0EA",
            "#E2DDD5",
            "#1E1B18",
            "#4A2E1B",
            "#5A524C",
            "#8A8178",
            "#7A5216",
            "#9E6B24",
            "#A62B2B",
        ),
        make(
            "Aged Oxford Manuscript (Warm Deep Sepia)",
            "Sepia",
            "#EADBC0",
            "#DECBA8",
            "#F3E7D0",
            "#E4D3B5",
            "#C4AE88",
            "#1F160F",
            "#421E0D",
            "#523F2E",
            "#75614D",
            "#8C3823",
            "#9E6B24",
            "#A62B2B",
        ),
        make(
            "Candlelight Study (Dark Warm Sepia)",
            "Dark",
            "#1C1612",
            "#15100D",
            "#251E18",
            "#2E251E",
            "#3D3127",
            "#E8DCC4",
            "#F2C69D",
            "#BFB099",
            "#8C7D6B",
            "#D4724E",
            "#D9A14E",
            "#E57373",
        ),
        make(
            "Midnight OLED Ink (AMOLED Dark)",
            "Dark",
            "#0A0908",
            "#12100E",
            "#181512",
            "#221E1A",
            "#2E2822",
            "#E5DDD0",
            "#EABFA0",
            "#B3A48E",
            "#7A6D5B",
            "#C97A3E",
            "#C49142",
            "#E57373",
        ),
        make(
            "Paper & Graphite (Modern Editorial Light)",
            "Light",
            "#F7F7F5",
            "#EFEFED",
            "#FFFFFF",
            "#F2F2EF",
            "#E4E4E0",
            "#18181B",
            "#27272A",
            "#52525B",
            "#71717A",
            "#4F46E5",
            "#0D9488",
            "#B91C1C",
        ),
        make(
            "Linear Carbon (Modern Editorial Dark)",
            "Dark",
            "#0E0F12",
            "#131519",
            "#17191E",
            "#1F2229",
            "#2A2E37",
            "#ECECEE",
            "#D4D8F0",
            "#A0A6B2",
            "#6E7480",
            "#818CF8",
            "#2DD4BF",
            "#F87171",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_nine_theme_presets_pass_wcag_aa_contrast_on_all_surfaces() {
        let presets = theme_presets();
        assert_eq!(presets.len(), 9);
        for preset in presets {
            let pal = ParchmentPalette::from_config(&preset);
            let surfaces = [pal.canvas_bg, pal.sheet_bg, pal.card_bg, pal.elevated_bg];
            for surf in surfaces {
                assert!(
                    contrast_ratio(pal.primary_ink, surf) >= READABLE_TEXT_CONTRAST,
                    "primary_ink failed AA on {}",
                    preset.name
                );
                assert!(
                    contrast_ratio(pal.muted_ink, surf) >= READABLE_TEXT_CONTRAST,
                    "muted_ink failed AA on {}",
                    preset.name
                );
                assert!(
                    contrast_ratio(pal.accent, surf) >= READABLE_TEXT_CONTRAST,
                    "accent failed AA on {}",
                    preset.name
                );
                assert!(
                    contrast_ratio(pal.danger, surf) >= READABLE_TEXT_CONTRAST,
                    "danger failed AA on {}",
                    preset.name
                );
            }
            assert!(
                contrast_ratio(pal.on_accent_ink, pal.accent) >= READABLE_TEXT_CONTRAST,
                "on_accent_ink failed AA on {}",
                preset.name
            );
        }
    }
}
