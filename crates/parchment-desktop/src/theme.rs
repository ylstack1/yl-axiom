//! Parchment Design System theme palette, all 9 built-in presets, and WCAG AA `>= 4.5:1` contrast
//! enforcement (`readable_ink`, `on_fill_ink`), matching `docs/DESIGN.md` and v2 `ParchmentTheme.kt`.

use gpui::{Rgba, rgb};
use parchment_core::model::ThemeConfig;

pub const READABLE_TEXT_CONTRAST: f32 = 4.5;
const WARM_FILL_INK: u32 = 0xFAF6EB;

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

    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let v = (c as f32) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| -> u8 {
            ((a as f32) + ((b as f32) - (a as f32)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
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
    let parsed = match cleaned.len() {
        6 => u32::from_str_radix(cleaned, 16).ok(),
        8 => u32::from_str_radix(&cleaned[2..], 16).ok(),
        _ => None,
    };
    parsed.map(Rgb24::from_hex_u32).unwrap_or(fallback)
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
        let cand = ink.lerp(target, (step as f32) / 20.0);
        if worst(cand) >= READABLE_TEXT_CONTRAST {
            return cand;
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
    pub muted_ink: Rgb24,
    pub accent: Rgb24,
    pub on_accent_ink: Rgb24,
    pub danger: Rgb24,
    pub success: Rgb24,
}

impl ParchmentPalette {
    pub fn from_config(cfg: &ThemeConfig) -> Self {
        let canvas_bg = parse_hex_color(&cfg.canvas_bg_hex, Rgb24::from_hex_u32(0xF4ECD8));
        let sheet_bg = parse_hex_color(&cfg.sheet_bg_hex, Rgb24::from_hex_u32(0xEBE0C5));
        let card_bg = parse_hex_color(&cfg.card_bg_hex, Rgb24::from_hex_u32(0xFAF6EB));
        let elevated_bg = parse_hex_color(&cfg.elevated_bg_hex, Rgb24::from_hex_u32(0xEFE5CC));
        let border = parse_hex_color(&cfg.border_hex, Rgb24::from_hex_u32(0xD5C4A1));
        let primary_ink = parse_hex_color(&cfg.primary_ink_hex, Rgb24::from_hex_u32(0x261C14));
        let surfaces = [canvas_bg, sheet_bg, card_bg, elevated_bg];
        let muted_ink = readable_ink(
            parse_hex_color(&cfg.muted_ink_hex, Rgb24::from_hex_u32(0x826F5C)),
            &surfaces,
        );
        let accent = readable_ink(
            parse_hex_color(&cfg.accent_hex, Rgb24::from_hex_u32(0x8B5A2B)),
            &surfaces,
        );
        let danger = readable_ink(
            parse_hex_color(&cfg.danger_hex, Rgb24::from_hex_u32(0xA62B2B)),
            &surfaces,
        );
        let success = readable_ink(Rgb24::from_hex_u32(0x2E7D32), &surfaces);
        let is_dark = cfg.color_mode.eq_ignore_ascii_case("dark") || canvas_bg.luminance() < 0.2;

        Self {
            is_dark,
            canvas_bg,
            sheet_bg,
            card_bg,
            elevated_bg,
            border,
            primary_ink,
            muted_ink,
            accent,
            on_accent_ink: on_fill_ink(accent),
            danger,
            success,
        }
    }
}

/// All 9 built-in Parchment theme presets (exact hex parity with v2 `ThemeConfig.PRESETS`).
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
            "Parchment Warm Ivory (Light)",
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
        for preset in &presets {
            let pal = ParchmentPalette::from_config(preset);
            let surfaces = [pal.canvas_bg, pal.sheet_bg, pal.card_bg, pal.elevated_bg];
            for surf in surfaces {
                assert!(
                    contrast_ratio(pal.muted_ink, surf) >= READABLE_TEXT_CONTRAST,
                    "preset {} muted_ink failed contrast",
                    preset.name
                );
                assert!(
                    contrast_ratio(pal.danger, surf) >= READABLE_TEXT_CONTRAST,
                    "preset {} danger failed contrast",
                    preset.name
                );
                assert!(
                    contrast_ratio(pal.success, surf) >= READABLE_TEXT_CONTRAST,
                    "preset {} success failed contrast",
                    preset.name
                );
            }
            assert!(
                contrast_ratio(pal.on_accent_ink, pal.accent) >= READABLE_TEXT_CONTRAST,
                "preset {} on_accent_ink failed contrast",
                preset.name
            );
        }
    }
}
