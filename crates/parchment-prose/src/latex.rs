//! Bounded LaTeX math AST parser and Unicode math renderer (with safe plain-text fallback).
//!
//! Enforces strict size (`MAX_LATEX_BYTES`), recursion depth (`MAX_LATEX_DEPTH`), and node count
//! (`MAX_LATEX_NODES`) bounds so hostile or pathological model output never panics or hangs UI.

use serde::{Deserialize, Serialize};

pub const MAX_LATEX_BYTES: usize = 8 * 1024;
pub const MAX_LATEX_DEPTH: usize = 24;
pub const MAX_LATEX_NODES: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LatexNode {
    Group(Vec<LatexNode>),
    Text(String),
    Symbol(String),
    Fraction {
        numerator: Box<LatexNode>,
        denominator: Box<LatexNode>,
    },
    Sqrt {
        index: Option<Box<LatexNode>>,
        radicand: Box<LatexNode>,
    },
    Script {
        base: Box<LatexNode>,
        sup: Option<Box<LatexNode>>,
        sub: Option<Box<LatexNode>>,
    },
    Matrix {
        env: String,
        rows: Vec<Vec<LatexNode>>,
    },
    PlainFallback(String),
}

pub fn parse_latex_bounded(raw: &str) -> LatexNode {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LatexNode::Text(String::new());
    }
    if trimmed.len() > MAX_LATEX_BYTES {
        return LatexNode::PlainFallback(trimmed[..MAX_LATEX_BYTES].to_string());
    }
    let mut parser = LatexParser {
        src: trimmed,
        pos: 0,
        nodes: 0,
        exceeded: false,
    };
    let ast = parser.parse_seq(0, None);
    if parser.exceeded {
        LatexNode::PlainFallback(trimmed.to_string())
    } else {
        ast
    }
}

/// Renders a parsed or raw LaTeX expression into crisp Unicode math text for GPUI / accessibility.
pub fn render_latex_unicode(raw: &str) -> String {
    let node = parse_latex_bounded(raw);
    let mut out = String::with_capacity(raw.len());
    render_node(&node, &mut out);
    out
}

fn render_node(node: &LatexNode, out: &mut String) {
    match node {
        LatexNode::Group(items) => {
            for item in items {
                render_node(item, out);
            }
        }
        LatexNode::Text(t) | LatexNode::Symbol(t) | LatexNode::PlainFallback(t) => out.push_str(t),
        LatexNode::Fraction {
            numerator,
            denominator,
        } => {
            out.push('(');
            render_node(numerator, out);
            out.push_str(")/(");
            render_node(denominator, out);
            out.push(')');
        }
        LatexNode::Sqrt { index, radicand } => {
            if let Some(idx) = index {
                let mut idx_str = String::new();
                render_node(idx, &mut idx_str);
                out.push_str(&to_superscript(&idx_str));
            }
            out.push_str("√(");
            render_node(radicand, out);
            out.push(')');
        }
        LatexNode::Script { base, sup, sub } => {
            render_node(base, out);
            if let Some(s) = sub {
                let mut sub_str = String::new();
                render_node(s, &mut sub_str);
                out.push_str(&to_subscript(&sub_str));
            }
            if let Some(p) = sup {
                let mut sup_str = String::new();
                render_node(p, &mut sup_str);
                out.push_str(&to_superscript(&sup_str));
            }
        }
        LatexNode::Matrix { env, rows } => {
            let (open, close) = match env.as_str() {
                "pmatrix" => ("(", ")"),
                "bmatrix" => ("[", "]"),
                "Bmatrix" | "cases" => ("{", "}"),
                "vmatrix" => ("|", "|"),
                _ => ("", ""),
            };
            out.push_str(open);
            for (r_idx, row) in rows.iter().enumerate() {
                if r_idx > 0 {
                    out.push_str("; ");
                }
                for (c_idx, cell) in row.iter().enumerate() {
                    if c_idx > 0 {
                        out.push_str(", ");
                    }
                    render_node(cell, out);
                }
            }
            out.push_str(close);
        }
    }
}

struct LatexParser<'a> {
    src: &'a str,
    pos: usize,
    nodes: usize,
    exceeded: bool,
}

impl<'a> LatexParser<'a> {
    fn bump_node(&mut self) -> bool {
        self.nodes += 1;
        if self.nodes > MAX_LATEX_NODES {
            self.exceeded = true;
            false
        } else {
            true
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_spaces(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn parse_seq(&mut self, depth: usize, stop_char: Option<char>) -> LatexNode {
        if depth > MAX_LATEX_DEPTH || !self.bump_node() {
            self.exceeded = true;
            return LatexNode::Text(String::new());
        }
        let mut items = Vec::new();
        while !self.exceeded && self.pos < self.src.len() {
            if let Some(stop) = stop_char
                && self.peek() == Some(stop)
            {
                self.next_char();
                break;
            }
            if stop_char.is_none() && self.src[self.pos..].starts_with("\\end{") {
                break;
            }
            let Some(atom) = self.parse_atom(depth) else {
                break;
            };
            let with_scripts = self.parse_scripts(atom, depth);
            items.push(with_scripts);
        }
        if items.len() == 1
            && let Some(single) = items.pop()
        {
            single
        } else {
            LatexNode::Group(items)
        }
    }

    fn parse_scripts(&mut self, base: LatexNode, depth: usize) -> LatexNode {
        let mut sup: Option<Box<LatexNode>> = None;
        let mut sub: Option<Box<LatexNode>> = None;
        loop {
            let saved = self.pos;
            self.skip_spaces();
            match self.peek() {
                Some('^') if sup.is_none() => {
                    self.next_char();
                    let arg = self.parse_required_arg(depth + 1);
                    sup = Some(Box::new(arg));
                }
                Some('_') if sub.is_none() => {
                    self.next_char();
                    let arg = self.parse_required_arg(depth + 1);
                    sub = Some(Box::new(arg));
                }
                _ => {
                    self.pos = saved;
                    break;
                }
            }
        }
        if sup.is_some() || sub.is_some() {
            if !self.bump_node() {
                return base;
            }
            LatexNode::Script {
                base: Box::new(base),
                sup,
                sub,
            }
        } else {
            base
        }
    }

    fn parse_required_arg(&mut self, depth: usize) -> LatexNode {
        self.skip_spaces();
        if self.peek() == Some('{') {
            self.next_char();
            self.parse_seq(depth + 1, Some('}'))
        } else {
            self.parse_atom(depth + 1)
                .unwrap_or_else(|| LatexNode::Text(String::new()))
        }
    }

    fn parse_atom(&mut self, depth: usize) -> Option<LatexNode> {
        if depth > MAX_LATEX_DEPTH || !self.bump_node() {
            self.exceeded = true;
            return None;
        }
        let ch = self.peek()?;
        if ch == '{' {
            self.next_char();
            return Some(self.parse_seq(depth + 1, Some('}')));
        }
        if ch == '}' {
            self.next_char();
            return Some(LatexNode::Text(String::new()));
        }
        if ch == '\\' {
            self.next_char();
            return Some(self.parse_command(depth));
        }
        self.next_char();
        Some(LatexNode::Text(ch.to_string()))
    }

    fn parse_command(&mut self, depth: usize) -> LatexNode {
        let start = self.pos;
        if let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                while let Some(nc) = self.peek() {
                    if nc.is_ascii_alphabetic() {
                        self.pos += nc.len_utf8();
                    } else {
                        break;
                    }
                }
            } else {
                self.next_char();
            }
        }
        let cmd = &self.src[start..self.pos];
        match cmd {
            "frac" | "dfrac" | "tfrac" => {
                let num = self.parse_required_arg(depth + 1);
                let den = self.parse_required_arg(depth + 1);
                LatexNode::Fraction {
                    numerator: Box::new(num),
                    denominator: Box::new(den),
                }
            }
            "sqrt" => {
                self.skip_spaces();
                let index = if self.peek() == Some('[') {
                    self.next_char();
                    Some(Box::new(self.parse_seq(depth + 1, Some(']'))))
                } else {
                    None
                };
                let radicand = self.parse_required_arg(depth + 1);
                LatexNode::Sqrt {
                    index,
                    radicand: Box::new(radicand),
                }
            }
            "mathbb" => {
                let inner = self.parse_required_arg(depth + 1);
                let mut raw = String::new();
                render_node(&inner, &mut raw);
                let mapped: String = raw
                    .chars()
                    .map(|c| blackboard_bold(c).unwrap_or(c))
                    .collect();
                LatexNode::Symbol(mapped)
            }
            "text" | "mathrm" | "mathbf" | "mathit" | "operatorname" => {
                self.parse_required_arg(depth + 1)
            }
            "left" | "right" => {
                self.skip_spaces();
                match self.next_char() {
                    Some('.') | None => LatexNode::Text(String::new()),
                    Some('\\') => self.parse_command(depth + 1),
                    Some(c) => LatexNode::Text(c.to_string()),
                }
            }
            "begin" => {
                let env_node = self.parse_required_arg(depth + 1);
                let mut env = String::new();
                render_node(&env_node, &mut env);
                self.parse_environment(env.trim(), depth + 1)
            }
            _ => {
                if let Some(sym) = latex_symbol(cmd) {
                    LatexNode::Symbol(sym.to_string())
                } else {
                    LatexNode::Text(cmd.to_string())
                }
            }
        }
    }

    fn parse_environment(&mut self, env: &str, depth: usize) -> LatexNode {
        let end_tag = format!("\\end{{{env}}}");
        let Some(rel_end) = self.src[self.pos..].find(&end_tag) else {
            return LatexNode::Text(String::new());
        };
        let body = &self.src[self.pos..self.pos + rel_end];
        self.pos += rel_end + end_tag.len();
        let mut rows = Vec::new();
        for row_raw in body.split("\\\\") {
            let trimmed = row_raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let cells = trimmed
                .split('&')
                .map(|cell| {
                    let mut sub = LatexParser {
                        src: cell.trim(),
                        pos: 0,
                        nodes: self.nodes,
                        exceeded: false,
                    };
                    let node = sub.parse_seq(depth + 1, None);
                    self.nodes = sub.nodes;
                    if sub.exceeded {
                        self.exceeded = true;
                    }
                    node
                })
                .collect();
            rows.push(cells);
        }
        LatexNode::Matrix {
            env: env.to_string(),
            rows,
        }
    }
}

fn latex_symbol(cmd: &str) -> Option<&'static str> {
    Some(match cmd {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        "int" => "∫",
        "iint" => "∬",
        "oint" => "∮",
        "sum" => "∑",
        "prod" => "∏",
        "partial" => "∂",
        "nabla" => "∇",
        "infty" => "∞",
        "hbar" => "ℏ",
        "times" => "×",
        "div" => "÷",
        "cdot" => "⋅",
        "pm" => "±",
        "mp" => "∓",
        "leq" | "le" => "≤",
        "geq" | "ge" => "≥",
        "neq" | "ne" => "≠",
        "approx" => "≈",
        "equiv" => "≡",
        "sim" => "∼",
        "propto" => "∝",
        "forall" => "∀",
        "exists" => "∃",
        "in" => "∈",
        "notin" => "∉",
        "subset" => "⊂",
        "subseteq" => "⊆",
        "cup" => "∪",
        "cap" => "∩",
        "emptyset" | "varnothing" => "∅",
        "to" | "rightarrow" => "→",
        "leftarrow" => "←",
        "leftrightarrow" => "↔",
        "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐",
        "Leftrightarrow" | "iff" => "⇔",
        "mapsto" => "↦",
        "dots" | "ldots" => "…",
        "cdots" => "⋯",
        "quad" => "  ",
        "qquad" => "    ",
        "," | ";" | ":" | " " => " ",
        "!" => "",
        _ => return None,
    })
}

fn blackboard_bold(ch: char) -> Option<char> {
    Some(match ch {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        _ => return None,
    })
}

fn to_superscript(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut all_mapped = true;
    for c in s.chars() {
        let mapped = match c {
            '0' => Some('⁰'),
            '1' => Some('¹'),
            '2' => Some('²'),
            '3' => Some('³'),
            '4' => Some('⁴'),
            '5' => Some('⁵'),
            '6' => Some('⁶'),
            '7' => Some('⁷'),
            '8' => Some('⁸'),
            '9' => Some('⁹'),
            '+' => Some('⁺'),
            '-' => Some('⁻'),
            '=' => Some('⁼'),
            '(' => Some('⁽'),
            ')' => Some('⁾'),
            'n' => Some('ⁿ'),
            'i' => Some('ⁱ'),
            _ => None,
        };
        if let Some(m) = mapped {
            out.push(m);
        } else {
            all_mapped = false;
            break;
        }
    }
    if all_mapped && !out.is_empty() {
        out
    } else {
        format!("^({s})")
    }
}

fn to_subscript(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut all_mapped = true;
    for c in s.chars() {
        let mapped = match c {
            '0' => Some('₀'),
            '1' => Some('₁'),
            '2' => Some('₂'),
            '3' => Some('₃'),
            '4' => Some('₄'),
            '5' => Some('₅'),
            '6' => Some('₆'),
            '7' => Some('₇'),
            '8' => Some('₈'),
            '9' => Some('₉'),
            '+' => Some('₊'),
            '-' => Some('₋'),
            '=' => Some('₌'),
            '(' => Some('₍'),
            ')' => Some('₎'),
            _ => None,
        };
        if let Some(m) = mapped {
            out.push(m);
        } else {
            all_mapped = false;
            break;
        }
    }
    if all_mapped && !out.is_empty() {
        out
    } else {
        format!("_({s})")
    }
}
