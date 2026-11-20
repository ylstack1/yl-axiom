//! Bounded, sanitized W3C SVG scene-graph parser.
//!
//! Enforces strict size (`MAX_SVG_BYTES`), nesting depth (`MAX_SVG_DEPTH`), and element count
//! (`MAX_SVG_ELEMENTS`) caps while stripping unsafe elements (`<script>`, `<foreignObject>`) and
//! external network `href` attributes.

use serde::{Deserialize, Serialize};

pub const MAX_SVG_BYTES: usize = 256 * 1024;
pub const MAX_SVG_DEPTH: usize = 32;
pub const MAX_SVG_ELEMENTS: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SvgColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl SvgColor {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SvgPaint {
    None,
    Solid(SvgColor),
    CurrentColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine2D {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Default for Affine2D {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine2D {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn concat(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SvgStyle {
    pub fill: SvgPaint,
    pub stroke: SvgPaint,
    pub stroke_width: f32,
    pub opacity: f32,
}

impl Default for SvgStyle {
    fn default() -> Self {
        Self {
            fill: SvgPaint::Solid(SvgColor::BLACK),
            stroke: SvgPaint::None,
            stroke_width: 1.0,
            opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SvgShape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rx: f32,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    },
    Polyline {
        points: Vec<(f32, f32)>,
        closed: bool,
    },
    Path {
        d: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SvgDrawItem {
    pub shape: SvgShape,
    pub style: SvgStyle,
    pub transform: Affine2D,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SvgDocument {
    pub min_x: f32,
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
    pub items: Vec<SvgDrawItem>,
    pub truncated: bool,
}

/// Parses raw `<svg>...</svg>` markup into a bounded, sanitized `SvgDocument`.
/// Returns `None` if malformed, over `MAX_SVG_BYTES`, or missing `<svg>`.
pub fn parse_svg_bounded(raw: &str) -> Option<SvgDocument> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SVG_BYTES {
        return None;
    }
    let svg_start = trimmed.find("<svg")?;
    let mut cursor = svg_start;
    let bytes = trimmed.as_bytes();

    let mut min_x = 0.0f32;
    let mut min_y = 0.0f32;
    let mut width = 300.0f32;
    let mut height = 150.0f32;
    let mut items = Vec::new();
    let mut truncated = false;

    let mut style_stack: Vec<SvgStyle> = vec![SvgStyle::default()];
    let mut transform_stack: Vec<Affine2D> = vec![Affine2D::IDENTITY];
    let mut skip_depth: usize = 0;

    while cursor < bytes.len() {
        let Some(rel_lt) = trimmed[cursor..].find('<') else {
            break;
        };
        cursor += rel_lt;
        if trimmed[cursor..].starts_with("<!--") {
            cursor = trimmed[cursor + 4..]
                .find("-->")
                .map(|r| cursor + 4 + r + 3)
                .unwrap_or(bytes.len());
            continue;
        }
        let Some(rel_gt) = trimmed[cursor..].find('>') else {
            break;
        };
        let tag_body = trimmed[cursor + 1..cursor + rel_gt].trim();
        cursor += rel_gt + 1;

        if tag_body.is_empty() || tag_body.starts_with('?') || tag_body.starts_with('!') {
            continue;
        }

        if let Some(closing) = tag_body.strip_prefix('/') {
            let name = closing.split_whitespace().next().unwrap_or("");
            if skip_depth > 0 {
                if matches!(name, "script" | "foreignObject" | "defs" | "style") {
                    skip_depth -= 1;
                }
                continue;
            }
            if matches!(name, "g" | "svg") && style_stack.len() > 1 {
                style_stack.pop();
                transform_stack.pop();
            }
            continue;
        }

        let self_closing = tag_body.ends_with('/');
        let inner = tag_body.trim_end_matches('/').trim();
        let mut parts = inner.splitn(2, char::is_whitespace);
        let tag_name = parts.next().unwrap_or("");
        let attr_str = parts.next().unwrap_or("");

        if matches!(tag_name, "script" | "foreignObject" | "defs" | "style") {
            if !self_closing {
                skip_depth += 1;
            }
            continue;
        }
        if skip_depth > 0 {
            continue;
        }

        let attrs = parse_attrs(attr_str);

        if tag_name == "svg" {
            if let Some(vb) = attr(&attrs, "viewBox") {
                let nums: Vec<f32> = vb
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse::<f32>().ok())
                    .collect();
                if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
                    min_x = nums[0];
                    min_y = nums[1];
                    width = nums[2];
                    height = nums[3];
                }
            }
            if let Some(w) = attr(&attrs, "width").and_then(parse_length)
                && w > 0.0
                && attr(&attrs, "viewBox").is_none()
            {
                width = w;
            }
            if let Some(h) = attr(&attrs, "height").and_then(parse_length)
                && h > 0.0
                && attr(&attrs, "viewBox").is_none()
            {
                height = h;
            }
            let default_style = SvgStyle::default();
            let st = apply_style(style_stack.last().unwrap_or(&default_style), &attrs);
            if !self_closing && style_stack.len() < MAX_SVG_DEPTH {
                style_stack.push(st);
                transform_stack.push(Affine2D::IDENTITY);
            }
            continue;
        }

        if tag_name == "g" {
            if style_stack.len() >= MAX_SVG_DEPTH {
                truncated = true;
                continue;
            }
            let default_style = SvgStyle::default();
            let st = apply_style(style_stack.last().unwrap_or(&default_style), &attrs);
            let base_tr = transform_stack
                .last()
                .copied()
                .unwrap_or(Affine2D::IDENTITY);
            let tr = base_tr.concat(parse_transform(attr(&attrs, "transform").unwrap_or("")));
            if !self_closing {
                style_stack.push(st);
                transform_stack.push(tr);
            }
            continue;
        }

        if items.len() >= MAX_SVG_ELEMENTS {
            truncated = true;
            break;
        }

        let shape = match tag_name {
            "rect" => Some(SvgShape::Rect {
                x: attr_f32(&attrs, "x", 0.0),
                y: attr_f32(&attrs, "y", 0.0),
                width: attr_f32(&attrs, "width", 0.0).max(0.0),
                height: attr_f32(&attrs, "height", 0.0).max(0.0),
                rx: attr_f32(&attrs, "rx", 0.0).max(0.0),
            }),
            "circle" => Some(SvgShape::Circle {
                cx: attr_f32(&attrs, "cx", 0.0),
                cy: attr_f32(&attrs, "cy", 0.0),
                r: attr_f32(&attrs, "r", 0.0).max(0.0),
            }),
            "ellipse" => Some(SvgShape::Ellipse {
                cx: attr_f32(&attrs, "cx", 0.0),
                cy: attr_f32(&attrs, "cy", 0.0),
                rx: attr_f32(&attrs, "rx", 0.0).max(0.0),
                ry: attr_f32(&attrs, "ry", 0.0).max(0.0),
            }),
            "line" => Some(SvgShape::Line {
                x1: attr_f32(&attrs, "x1", 0.0),
                y1: attr_f32(&attrs, "y1", 0.0),
                x2: attr_f32(&attrs, "x2", 0.0),
                y2: attr_f32(&attrs, "y2", 0.0),
            }),
            "polyline" | "polygon" => {
                let pts = parse_points(attr(&attrs, "points").unwrap_or(""));
                (!pts.is_empty()).then_some(SvgShape::Polyline {
                    points: pts,
                    closed: tag_name == "polygon",
                })
            }
            "path" => attr(&attrs, "d")
                .filter(|d| !d.trim().is_empty())
                .map(|d| SvgShape::Path { d: d.to_string() }),
            _ => None,
        };

        if let Some(shape) = shape {
            let default_style = SvgStyle::default();
            let style = apply_style(style_stack.last().unwrap_or(&default_style), &attrs);
            let base_tr = transform_stack
                .last()
                .copied()
                .unwrap_or(Affine2D::IDENTITY);
            let transform =
                base_tr.concat(parse_transform(attr(&attrs, "transform").unwrap_or("")));
            items.push(SvgDrawItem {
                shape,
                style,
                transform,
            });
        }
    }

    Some(SvgDocument {
        min_x,
        min_y,
        width,
        height,
        items,
        truncated,
    })
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let k_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let key = &s[k_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        let val = if quote == b'"' || quote == b'\'' {
            i += 1;
            let v_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let v = &s[v_start..i];
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let v_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            &s[v_start..i]
        };
        if !key.is_empty() && !key.starts_with("on") && key != "href" && key != "xlink:href" {
            out.push((key.to_string(), val.to_string()));
        }
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

fn attr_f32(attrs: &[(String, String)], key: &str, default: f32) -> f32 {
    attr(attrs, key).and_then(parse_length).unwrap_or(default)
}

fn parse_length(raw: &str) -> Option<f32> {
    let trimmed = raw
        .trim()
        .trim_end_matches("px")
        .trim_end_matches("pt")
        .trim();
    trimmed.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_points(raw: &str) -> Vec<(f32, f32)> {
    let nums: Vec<f32> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect()
}

fn apply_style(parent: &SvgStyle, attrs: &[(String, String)]) -> SvgStyle {
    let mut st = parent.clone();
    if let Some(fill) = attr(attrs, "fill") {
        st.fill = parse_paint(fill);
    }
    if let Some(stroke) = attr(attrs, "stroke") {
        st.stroke = parse_paint(stroke);
    }
    if let Some(sw) = attr(attrs, "stroke-width").and_then(parse_length) {
        st.stroke_width = sw.max(0.0);
    }
    if let Some(op) = attr(attrs, "opacity").and_then(|s| s.trim().parse::<f32>().ok()) {
        st.opacity = (st.opacity * op).clamp(0.0, 1.0);
    }
    st
}

fn parse_paint(raw: &str) -> SvgPaint {
    let t = raw.trim().to_ascii_lowercase();
    if t.is_empty() || t == "none" || t == "transparent" {
        return SvgPaint::None;
    }
    if t == "currentcolor" {
        return SvgPaint::CurrentColor;
    }
    if let Some(hex) = t.strip_prefix('#') {
        if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok().map(|v| v * 17);
            let g = u8::from_str_radix(&hex[1..2], 16).ok().map(|v| v * 17);
            let b = u8::from_str_radix(&hex[2..3], 16).ok().map(|v| v * 17);
            if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                return SvgPaint::Solid(SvgColor { r, g, b, a: 255 });
            }
        } else if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok();
            let g = u8::from_str_radix(&hex[2..4], 16).ok();
            let b = u8::from_str_radix(&hex[4..6], 16).ok();
            if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                return SvgPaint::Solid(SvgColor { r, g, b, a: 255 });
            }
        }
    }
    match t.as_str() {
        "black" => SvgPaint::Solid(SvgColor::BLACK),
        "white" => SvgPaint::Solid(SvgColor {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }),
        "red" => SvgPaint::Solid(SvgColor {
            r: 220,
            g: 38,
            b: 38,
            a: 255,
        }),
        "green" => SvgPaint::Solid(SvgColor {
            r: 22,
            g: 163,
            b: 74,
            a: 255,
        }),
        "blue" => SvgPaint::Solid(SvgColor {
            r: 37,
            g: 99,
            b: 235,
            a: 255,
        }),
        _ => SvgPaint::CurrentColor,
    }
}

fn parse_transform(raw: &str) -> Affine2D {
    let mut acc = Affine2D::IDENTITY;
    let mut rest = raw.trim();
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else {
            break;
        };
        let Some(close) = rest[open + 1..].find(')') else {
            break;
        };
        let name = rest[..open].trim();
        let args: Vec<f32> = rest[open + 1..open + 1 + close]
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        let step = match name {
            "translate" if !args.is_empty() => Affine2D {
                tx: args[0],
                ty: args.get(1).copied().unwrap_or(0.0),
                ..Affine2D::IDENTITY
            },
            "scale" if !args.is_empty() => Affine2D {
                a: args[0],
                d: args.get(1).copied().unwrap_or(args[0]),
                ..Affine2D::IDENTITY
            },
            "rotate" if !args.is_empty() => {
                let rad = args[0].to_radians();
                let (s, c) = rad.sin_cos();
                Affine2D {
                    a: c,
                    b: s,
                    c: -s,
                    d: c,
                    tx: 0.0,
                    ty: 0.0,
                }
            }
            "matrix" if args.len() == 6 => Affine2D {
                a: args[0],
                b: args[1],
                c: args[2],
                d: args[3],
                tx: args[4],
                ty: args[5],
            },
            _ => Affine2D::IDENTITY,
        };
        acc = acc.concat(step);
        rest = rest[open + 1 + close + 1..]
            .trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    }
    acc
}
