//! Zero-dependency linear-time syntax tokenizer producing theme-semantic `HighlightSpan`s.
//!
//! Avoids bundling multi-megabyte grammar dumps into the Android `.so` or Windows `.exe` while
//! covering Rust, Kotlin, Python, JS/TS, JSON, TOML/YAML, SQL, Bash/PowerShell, HTML/XML, C/C++/Go.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    TypeName,
    String,
    Number,
    Comment,
    Function,
    Operator,
    Punctuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub kind: TokenKind,
}

pub const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

pub fn highlight_code(language: &str, code: &str) -> Vec<HighlightSpan> {
    if code.is_empty() {
        return Vec::new();
    }
    if code.len() > MAX_HIGHLIGHT_BYTES {
        return vec![HighlightSpan {
            range: 0..code.len(),
            kind: TokenKind::Plain,
        }];
    }

    let lang = language.trim();
    let is_lang = |names: &[&str]| names.iter().any(|n| lang.eq_ignore_ascii_case(n));
    let hash_comments = is_lang(&[
        "py",
        "python",
        "sh",
        "bash",
        "zsh",
        "ps1",
        "powershell",
        "pwsh",
        "toml",
        "yaml",
        "yml",
        "rb",
        "ruby",
        "r",
    ]);
    let dash_comments = is_lang(&["sql", "lua", "haskell", "hs"]);
    let slash_comments = !hash_comments && !dash_comments;
    let is_rust = lang.eq_ignore_ascii_case("rust") || lang.eq_ignore_ascii_case("rs");
    let is_json = lang.eq_ignore_ascii_case("json");
    let is_sql = lang.eq_ignore_ascii_case("sql");

    let bytes = code.as_bytes();
    let n = bytes.len();
    let mut spans: Vec<HighlightSpan> = Vec::with_capacity((n / 12).clamp(16, 4096));
    let mut i = 0usize;

    while i < n {
        let b = bytes[i];

        // 1. Line comments
        if (slash_comments && bytes[i..].starts_with(b"//"))
            || (hash_comments && b == b'#')
            || (dash_comments && bytes[i..].starts_with(b"--"))
        {
            let start = i;
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            push_span(&mut spans, start..i, TokenKind::Comment);
            continue;
        }

        // 2. Block comments `/* ... */`
        if slash_comments && bytes[i..].starts_with(b"/*") {
            let start = i;
            i += 2;
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(n);
            push_span(&mut spans, start..i, TokenKind::Comment);
            continue;
        }

        // 3. Quoted strings (`"..."`, `'...'`, `` `...` ``)
        if b == b'"' || b == b'\'' || b == b'`' {
            let quote = b;
            let start = i;
            i += 1;
            let mut escaped = false;
            while i < n {
                let c = bytes[i];
                if escaped {
                    escaped = false;
                } else if c == b'\\' && quote != b'`' {
                    escaped = true;
                } else if c == quote {
                    i += 1;
                    break;
                } else if c == b'\n' && quote == b'\'' && is_rust {
                    // Rust lifetime `'a` rather than multi-line char literal
                    i = start + 1;
                    while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    push_span(&mut spans, start..i, TokenKind::TypeName);
                    break;
                }
                i += 1;
            }
            if spans.last().is_none_or(|s| s.range.start != start) {
                push_span(&mut spans, start..i, TokenKind::String);
            }
            continue;
        }

        // 4. Numbers
        if b.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < n
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            push_span(&mut spans, start..i, TokenKind::Number);
            continue;
        }

        // 5. Identifiers / Keywords / Types / Function calls
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &code[start..i];
            let kind = if is_keyword(is_json, is_sql, word) {
                TokenKind::Keyword
            } else if bytes.get(i) == Some(&b'(') {
                TokenKind::Function
            } else if word.as_bytes()[0].is_ascii_uppercase() && word.len() > 1 {
                TokenKind::TypeName
            } else {
                TokenKind::Plain
            };
            push_span(&mut spans, start..i, kind);
            continue;
        }

        // 6. Operators / punctuation / whitespace
        let start = i;
        let ch_len = code[i..].chars().next().map_or(1, char::len_utf8);
        i += ch_len;
        let kind = if b.is_ascii_whitespace() {
            TokenKind::Plain
        } else if matches!(
            b,
            b'=' | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'<'
                | b'>'
                | b'!'
                | b'&'
                | b'|'
                | b'^'
                | b'?'
                | b':'
        ) {
            TokenKind::Operator
        } else if matches!(
            b,
            b'(' | b')' | b'{' | b'}' | b'[' | b']' | b',' | b';' | b'.'
        ) {
            TokenKind::Punctuation
        } else {
            TokenKind::Plain
        };
        push_span(&mut spans, start..i, kind);
    }

    spans
}

fn push_span(spans: &mut Vec<HighlightSpan>, range: Range<usize>, kind: TokenKind) {
    if let Some(last) = spans.last_mut()
        && last.kind == kind
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }
    spans.push(HighlightSpan { range, kind });
}

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "UPDATE", "DELETE", "CREATE", "TABLE", "INDEX",
    "JOIN", "LEFT", "INNER", "ON", "AS", "AND", "OR", "NOT", "NULL", "PRIMARY", "KEY", "ORDER",
    "BY", "LIMIT", "VALUES", "BEGIN", "COMMIT",
];

fn is_keyword(is_json: bool, is_sql: bool, word: &str) -> bool {
    if is_json {
        return matches!(word, "true" | "false" | "null");
    }
    if is_sql {
        return SQL_KEYWORDS.iter().any(|kw| kw.eq_ignore_ascii_case(word));
    }
    matches!(
        word,
        "fn" | "let"
            | "mut"
            | "pub"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "for"
            | "while"
            | "loop"
            | "if"
            | "else"
            | "match"
            | "return"
            | "break"
            | "continue"
            | "use"
            | "mod"
            | "async"
            | "await"
            | "const"
            | "static"
            | "type"
            | "where"
            | "self"
            | "Self"
            | "super"
            | "crate"
            | "true"
            | "false"
            | "null"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "fun"
            | "val"
            | "var"
            | "class"
            | "object"
            | "interface"
            | "private"
            | "internal"
            | "override"
            | "suspend"
            | "def"
            | "import"
            | "from"
            | "pass"
            | "with"
            | "yield"
            | "lambda"
            | "function"
            | "export"
            | "default"
            | "try"
            | "catch"
            | "throw"
            | "new"
            | "in"
            | "as"
            | "ref"
            | "move"
    )
}
