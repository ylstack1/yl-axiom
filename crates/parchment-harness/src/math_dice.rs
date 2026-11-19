//! Safe bounded `calc_eval` scientific expression evaluator and `dice_roll` generator.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

const MAX_EXPR_CHARS: usize = 2_048;
const MAX_EXPR_DEPTH: usize = 48;

pub fn eval_math_expression(expr: &str) -> Result<String, String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("Expression cannot be empty.".into());
    }
    if trimmed.len() > MAX_EXPR_CHARS {
        return Err("Expression exceeds maximum length.".into());
    }
    let mut parser = MathParser {
        src: trimmed,
        pos: 0,
    };
    let val = parser.parse_expr(0)?;
    parser.skip_ws();
    if parser.pos < parser.src.len() {
        return Err(format!(
            "Unexpected trailing characters at '{}'.",
            &parser.src[parser.pos..]
        ));
    }
    if !val.is_finite() {
        return Err("Result is not a finite number.".into());
    }
    if (val - val.round()).abs() < 1e-12 && val.abs() < 1e15 {
        Ok(format!("{}", val.round() as i64))
    } else {
        Ok(format!("{val:.10}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string())
    }
}

struct MathParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> MathParser<'a> {
    fn skip_ws(&mut self) {
        while let Some(b) = self.src.as_bytes().get(self.pos) {
            if b.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.src.as_bytes().get(self.pos).copied()
    }

    fn parse_expr(&mut self, depth: usize) -> Result<f64, String> {
        if depth > MAX_EXPR_DEPTH {
            return Err("Expression nesting too deep.".into());
        }
        let mut lhs = self.parse_term(depth + 1)?;
        while let Some(op) = self.peek() {
            if op == b'+' {
                self.pos += 1;
                lhs += self.parse_term(depth + 1)?;
            } else if op == b'-' {
                self.pos += 1;
                lhs -= self.parse_term(depth + 1)?;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_term(&mut self, depth: usize) -> Result<f64, String> {
        let mut lhs = self.parse_power(depth + 1)?;
        while let Some(op) = self.peek() {
            if op == b'*' {
                self.pos += 1;
                lhs *= self.parse_power(depth + 1)?;
            } else if op == b'/' {
                self.pos += 1;
                let rhs = self.parse_power(depth + 1)?;
                if rhs == 0.0 {
                    return Err("Division by zero.".into());
                }
                lhs /= rhs;
            } else if op == b'%' {
                self.pos += 1;
                let rhs = self.parse_power(depth + 1)?;
                if rhs == 0.0 {
                    return Err("Modulo by zero.".into());
                }
                lhs %= rhs;
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_power(&mut self, depth: usize) -> Result<f64, String> {
        let base = self.parse_unary(depth + 1)?;
        if self.peek() == Some(b'^') {
            self.pos += 1;
            let exp = self.parse_power(depth + 1)?;
            Ok(base.powf(exp))
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self, depth: usize) -> Result<f64, String> {
        match self.peek() {
            Some(b'+') => {
                self.pos += 1;
                self.parse_unary(depth + 1)
            }
            Some(b'-') => {
                self.pos += 1;
                Ok(-self.parse_unary(depth + 1)?)
            }
            _ => self.parse_primary(depth + 1),
        }
    }

    fn parse_primary(&mut self, depth: usize) -> Result<f64, String> {
        if depth > MAX_EXPR_DEPTH {
            return Err("Expression nesting too deep.".into());
        }
        match self.peek() {
            Some(b'(') => {
                self.pos += 1;
                let val = self.parse_expr(depth + 1)?;
                if self.peek() != Some(b')') {
                    return Err("Missing closing ')'.".into());
                }
                self.pos += 1;
                Ok(val)
            }
            Some(b) if b.is_ascii_digit() || b == b'.' => {
                let start = self.pos;
                while let Some(c) = self.src.as_bytes().get(self.pos) {
                    if c.is_ascii_digit() || *c == b'.' || *c == b'_' {
                        self.pos += 1;
                    } else if (*c == b'e' || *c == b'E')
                        && self.pos > start
                        && self
                            .src
                            .as_bytes()
                            .get(self.pos + 1)
                            .is_some_and(|n| n.is_ascii_digit() || *n == b'+' || *n == b'-')
                    {
                        self.pos += 2;
                    } else {
                        break;
                    }
                }
                let raw = self.src[start..self.pos].replace('_', "");
                raw.parse::<f64>()
                    .map_err(|_| format!("Invalid number '{raw}'."))
            }
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {
                let start = self.pos;
                while let Some(c) = self.src.as_bytes().get(self.pos) {
                    if c.is_ascii_alphanumeric() || *c == b'_' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                let ident = self.src[start..self.pos].to_ascii_lowercase();
                if self.peek() == Some(b'(') {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(b')') {
                        loop {
                            args.push(self.parse_expr(depth + 1)?);
                            if self.peek() == Some(b',') {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if self.peek() != Some(b')') {
                        return Err(format!("Missing ')' after function '{ident}'."));
                    }
                    self.pos += 1;
                    eval_fn(&ident, &args)
                } else {
                    match ident.as_str() {
                        "pi" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        "tau" => Ok(std::f64::consts::TAU),
                        _ => Err(format!("Unknown identifier '{ident}'.")),
                    }
                }
            }
            _ => Err("Unexpected end of expression.".into()),
        }
    }
}

fn eval_fn(name: &str, args: &[f64]) -> Result<f64, String> {
    let one = || -> Result<f64, String> {
        if args.len() != 1 {
            Err(format!("Function '{name}' expects 1 argument."))
        } else {
            Ok(args[0])
        }
    };
    match name {
        "sqrt" => Ok(one()?.sqrt()),
        "abs" => Ok(one()?.abs()),
        "sin" => Ok(one()?.sin()),
        "cos" => Ok(one()?.cos()),
        "tan" => Ok(one()?.tan()),
        "ln" => Ok(one()?.ln()),
        "log" | "log10" => Ok(one()?.log10()),
        "log2" => Ok(one()?.log2()),
        "exp" => Ok(one()?.exp()),
        "floor" => Ok(one()?.floor()),
        "ceil" => Ok(one()?.ceil()),
        "round" => Ok(one()?.round()),
        "min" if !args.is_empty() => Ok(args.iter().copied().fold(f64::INFINITY, f64::min)),
        "max" if !args.is_empty() => Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "pow" if args.len() == 2 => Ok(args[0].powf(args[1])),
        _ => Err(format!("Unknown function '{name}'.")),
    }
}

/// Evaluates standard tabletop dice notation (`NdS[khK|klK][+/-M]`, e.g. `2d20kh1+5`, `4d6kh3`).
pub fn roll_dice_expression(expr: &str, reason: Option<&str>) -> Result<String, String> {
    let clean: String = expr
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if clean.is_empty() {
        return Err("Dice expression cannot be empty.".into());
    }
    let Some(d_pos) = clean.find('d') else {
        return Err("Dice expression must contain 'd' (e.g. '1d20', '2d6+3').".into());
    };
    let count: usize = if d_pos == 0 {
        1
    } else {
        clean[..d_pos]
            .parse()
            .map_err(|_| "Invalid dice count.".to_string())?
    };
    if count == 0 || count > 1_000 {
        return Err("Dice count must be between 1 and 1000.".into());
    }
    let rest = &clean[d_pos + 1..];
    let sides_end = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if sides_end == 0 {
        return Err("Missing dice sides after 'd'.".into());
    }
    let sides: u64 = rest[..sides_end]
        .parse()
        .map_err(|_| "Invalid dice sides.".to_string())?;
    if !(2..=100_000).contains(&sides) {
        return Err("Dice sides must be between 2 and 100000.".into());
    }

    let mut rem = &rest[sides_end..];
    let mut keep_high: Option<usize> = None;
    let mut keep_low: Option<usize> = None;
    if let Some(after) = rem.strip_prefix("kh") {
        let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        let k: usize = after[..digits]
            .parse()
            .map_err(|_| "Invalid kh count.".to_string())?;
        keep_high = Some(k.clamp(1, count));
        rem = &after[digits..];
    } else if let Some(after) = rem.strip_prefix("kl") {
        let digits = after.bytes().take_while(|b| b.is_ascii_digit()).count();
        let k: usize = after[..digits]
            .parse()
            .map_err(|_| "Invalid kl count.".to_string())?;
        keep_low = Some(k.clamp(1, count));
        rem = &after[digits..];
    }

    let modifier: i64 = if rem.is_empty() {
        0
    } else {
        rem.parse()
            .map_err(|_| format!("Invalid dice modifier '{rem}'."))?
    };

    let mut state = next_entropy_u64();
    let mut rolls = Vec::with_capacity(count);
    for _ in 0..count {
        state = splitmix64(state);
        rolls.push((state % sides) + 1);
    }

    let mut kept = rolls.clone();
    if let Some(k) = keep_high {
        kept.sort_unstable_by(|a, b| b.cmp(a));
        kept.truncate(k);
    } else if let Some(k) = keep_low {
        kept.sort_unstable();
        kept.truncate(k);
    }
    let sum: i64 = kept.iter().map(|&v| v as i64).sum::<i64>() + modifier;
    let label = reason
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|r| format!(" ({r})"))
        .unwrap_or_default();
    Ok(format!(
        "Rolled {expr}{label}: rolls={rolls:?}, kept={kept:?}, modifier={modifier:+} => total={sum}"
    ))
}

fn next_entropy_u64() -> u64 {
    let rs = RandomState::new();
    let mut h = rs.build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15),
    );
    h.finish()
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
