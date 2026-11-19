//! Zero-dependency static quality gates for both Rust and Kotlin stacks.
//!
//! Enforces: UTF-8 without BOM, LF line endings (CRLF for `.bat`/`.iss`), ASCII `.ps1`,
//! Kotlin unused imports / dead code / 3000-line file cap / banned APIs, `AGENTS.md` and
//! `TODO.md` 10,000-char doc budgets, relative Markdown links & `#heading` anchors, and
//! zero tracked signing keys or secret files.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::process::Command;

const MAIN_SETS: &[&str] = &["commonMain", "desktopMain", "androidMain", "jvmShared"];
const TEST_SETS: &[&str] = &["commonTest", "desktopTest"];
const CRLF_SUFFIXES: &[&str] = &[".bat", ".iss"];
const BINARY_SUFFIXES: &[&str] = &[
    ".ttf",
    ".otf",
    ".woff",
    ".woff2",
    ".png",
    ".jpg",
    ".jpeg",
    ".webp",
    ".gif",
    ".ico",
    ".bmp",
    ".mp3",
    ".wav",
    ".ogg",
    ".mp4",
    ".webm",
    ".pdf",
    ".zip",
    ".jar",
    ".apk",
    ".exe",
    ".dll",
    ".so",
    ".dylib",
    ".jks",
    ".keystore",
    ".p12",
    ".pfx",
];
const SECRET_SUFFIXES: &[&str] = &[".jks", ".keystore", ".p12", ".pfx"];
const SECRET_NAMES: &[&str] = &["local.properties", ".env"];
const MAX_KOTLIN_LINES: usize = 3000;
const MAX_REPORTED_PER_RULE: usize = 15;
pub const DOC_BUDGET_CHARS: usize = 10_000;
pub const MAX_STATUS_CHARS: usize = 1400;
const STATUS_HEADING: &str = "## Current status";
const STATUS_FIELDS: &[&str] = &["**Source:**", "**Last verified", "**Next action:**"];

const IMPLICIT_IMPORTS: &[&str] = &[
    "getValue",
    "setValue",
    "provideDelegate",
    "plus",
    "minus",
    "times",
    "div",
    "rem",
    "get",
    "set",
    "invoke",
    "contains",
    "iterator",
    "plusAssign",
    "minusAssign",
    "unaryMinus",
    "unaryPlus",
    "not",
    "compareTo",
    "inc",
    "dec",
];

const KT_MODIFIERS: &[&str] = &[
    "private",
    "internal",
    "public",
    "protected",
    "actual",
    "expect",
    "inline",
    "suspend",
    "operator",
    "infix",
    "tailrec",
    "external",
    "open",
    "abstract",
    "final",
    "const",
    "lateinit",
    "data",
    "sealed",
    "enum",
    "value",
    "annotation",
    "override",
];

const KT_DECL_KINDS: &[&str] = &[
    "fun",
    "val",
    "var",
    "class",
    "object",
    "interface",
    "typealias",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub rule: &'static str,
    pub path: String,
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 {
            write!(
                f,
                "{}:{} [{}] {}",
                self.path, self.line, self.rule, self.message
            )
        } else {
            write!(f, "{} [{}] {}", self.path, self.rule, self.message)
        }
    }
}

pub fn repo_paths(repo: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("failed to run git ls-files: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files exited with non-zero status".into());
    }
    let mut paths = Vec::new();
    for part in out.stdout.split(|&b| b == 0) {
        if part.is_empty() {
            continue;
        }
        let s = String::from_utf8_lossy(part).into_owned();
        if repo.join(&s).is_file() {
            paths.push(s);
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn load_files(repo: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    for path in repo_paths(repo)? {
        let data = if has_suffix_ci(&path, BINARY_SUFFIXES) {
            Vec::new()
        } else {
            std::fs::read(repo.join(&path)).map_err(|e| format!("failed to read {path}: {e}"))?
        };
        files.insert(path, data);
    }
    Ok(files)
}

pub fn format_problems(problems: &[Problem]) -> Vec<String> {
    let mut shown: HashMap<&'static str, usize> = HashMap::new();
    let mut lines = Vec::new();
    for p in problems {
        let count = shown.entry(p.rule).or_insert(0);
        *count += 1;
        if *count <= MAX_REPORTED_PER_RULE {
            lines.push(p.to_string());
        }
    }
    for (rule, count) in shown {
        if count > MAX_REPORTED_PER_RULE {
            lines.push(format!(
                "[{rule}] {} more not shown",
                count - MAX_REPORTED_PER_RULE
            ));
        }
    }
    lines
}

pub fn check_files(files: &BTreeMap<String, Vec<u8>>) -> Vec<Problem> {
    let mut kotlin_texts: BTreeMap<&str, String> = BTreeMap::new();
    let mut stripped: HashMap<&str, String> = HashMap::new();
    for (path, data) in files {
        if is_kotlin(path) {
            let text = String::from_utf8_lossy(data)
                .replace("\r\n", "\n")
                .trim_start_matches('\u{feff}')
                .to_string();
            stripped.insert(path.as_str(), strip_kotlin_comments(&text));
            kotlin_texts.insert(path.as_str(), text);
        }
    }
    let mut problems = Vec::new();
    check_encoding(files, &mut problems);
    check_unused_imports(&kotlin_texts, &mut problems);
    check_dead_code(&kotlin_texts, &stripped, &mut problems);
    check_file_sizes(&kotlin_texts, &mut problems);
    check_banned(&kotlin_texts, &stripped, &mut problems);
    check_docs(files, &mut problems);
    check_links(files, &mut problems);
    check_secrets(files, &mut problems);
    problems
}

pub fn status_block(todo: &str) -> String {
    let Some((_, rest)) = todo.split_once(STATUS_HEADING) else {
        return String::new();
    };
    rest.split("\n## ").next().unwrap_or("").trim().to_string()
}

fn has_suffix_ci(path: &str, suffixes: &[&str]) -> bool {
    let lower = path.to_ascii_lowercase();
    suffixes.iter().any(|s| lower.ends_with(s))
}

fn source_set(path: &str) -> &str {
    let Some(rest) = path.strip_prefix("composeApp/src/") else {
        return "";
    };
    rest.split('/').next().unwrap_or("")
}

fn is_kotlin(path: &str) -> bool {
    path.ends_with(".kt") && !source_set(path).is_empty()
}

fn check_encoding(files: &BTreeMap<String, Vec<u8>>, out: &mut Vec<Problem>) {
    for (path, data) in files {
        if has_suffix_ci(path, BINARY_SUFFIXES) || data.iter().take(8000).any(|&b| b == 0) {
            continue;
        }
        if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            out.push(Problem {
                rule: "encoding",
                path: path.clone(),
                line: 1,
                message: "Remove the UTF-8 byte-order mark.".into(),
            });
        }
        let crlf = data.windows(2).filter(|w| w == b"\r\n").count();
        let lf = data.iter().filter(|&&b| b == b'\n').count();
        if has_suffix_ci(path, CRLF_SUFFIXES) {
            if crlf != lf {
                out.push(Problem {
                    rule: "encoding",
                    path: path.clone(),
                    line: 0,
                    message: "This file type must use CRLF line endings.".into(),
                });
            }
        } else if crlf > 0 {
            out.push(Problem {
                rule: "encoding",
                path: path.clone(),
                line: 0,
                message: "Use LF line endings.".into(),
            });
        }
        if std::str::from_utf8(data).is_err() {
            out.push(Problem {
                rule: "encoding",
                path: path.clone(),
                line: 0,
                message: "File is not valid UTF-8.".into(),
            });
        }
        if path.ends_with(".ps1") && !data.is_ascii() {
            out.push(Problem {
                rule: "encoding",
                path: path.clone(),
                line: 0,
                message: "PowerShell scripts must be ASCII; Windows PowerShell misreads UTF-8 without a BOM.".into(),
            });
        }
    }
}

/// Removes `//` and `/* ... */` comments while preserving string literals and newline positions.
pub fn strip_kotlin_comments(text: &str) -> String {
    let b = text.as_bytes();
    let n = b.len();
    let mut out = String::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        if b[i..].starts_with(b"\"\"\"") {
            let start = i;
            i += 3;
            while i < n && !b[i..].starts_with(b"\"\"\"") {
                i += 1;
            }
            if i < n {
                i += 3;
            }
            out.push_str(&text[start..i]);
        } else if b[i] == b'"' {
            let start = i;
            i += 1;
            while i < n && b[i] != b'"' && b[i] != b'\n' {
                i += if b[i] == b'\\' && i + 1 < n { 2 } else { 1 };
            }
            if i < n && b[i] == b'"' {
                i += 1;
            }
            out.push_str(&text[start..i]);
        } else if b[i] == b'\'' {
            let start = i;
            i += 1;
            while i < n && b[i] != b'\'' && b[i] != b'\n' && i - start < 10 {
                i += if b[i] == b'\\' && i + 1 < n { 2 } else { 1 };
            }
            if i < n && b[i] == b'\'' {
                i += 1;
            }
            out.push_str(&text[start..i]);
        } else if b[i..].starts_with(b"//") {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i += 2;
            while i + 1 < n && !(b[i] == b'*' && b[i + 1] == b'/') {
                if b[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i = (i + 2).min(n);
        } else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn for_each_word<'a>(text: &'a str, mut f: impl FnMut(&'a str)) {
    let b = text.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    while i < n {
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let start = i;
            i += 1;
            while i < n && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            f(&text[start..i]);
        } else {
            i += 1;
        }
    }
}

fn check_unused_imports(texts: &BTreeMap<&str, String>, out: &mut Vec<Problem>) {
    for (&path, text) in texts {
        let mut body_words: HashSet<&str> = HashSet::new();
        let mut imports: Vec<(usize, &str, &str)> = Vec::new();
        let mut in_header = true;
        let mut in_block_comment = false;
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if in_header {
                if in_block_comment {
                    if trimmed.contains("*/") {
                        in_block_comment = false;
                    }
                    continue;
                }
                if trimmed.starts_with("@file:") {
                    for_each_word(line, |w| {
                        body_words.insert(w);
                    });
                    continue;
                }
                if trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || trimmed.starts_with("package ")
                {
                    continue;
                }
                if trimmed.starts_with("/*") {
                    if !trimmed.contains("*/") {
                        in_block_comment = true;
                    }
                    continue;
                }
                if let Some(rest) = line.strip_prefix("import ") {
                    let rest = rest.trim().trim_end_matches(';').trim();
                    let (name, alias) = match rest.split_once(" as ") {
                        Some((n, a)) => (n.trim(), Some(a.trim())),
                        None => (rest, None),
                    };
                    let symbol = alias.unwrap_or_else(|| name.rsplit('.').next().unwrap_or(name));
                    imports.push((idx + 1, name, symbol));
                    continue;
                }
                in_header = false;
            }
            for_each_word(line, |w| {
                body_words.insert(w);
            });
        }
        for (line_no, name, symbol) in imports {
            if symbol == "*"
                || symbol.starts_with("component")
                || IMPLICIT_IMPORTS.contains(&symbol)
            {
                continue;
            }
            if !body_words.contains(symbol) {
                out.push(Problem {
                    rule: "unused-import",
                    path: path.to_string(),
                    line: line_no,
                    message: format!("Remove unused import {name}."),
                });
            }
        }
    }
}

/// Parses a Kotlin declaration line returning `(has_override, kind, name)`.
fn parse_kt_declaration(line: &str) -> Option<(bool, &'static str, &str)> {
    let mut s = line.trim_start();
    if s.is_empty() {
        return None;
    }
    let mut has_override = false;
    loop {
        // Skip leading annotations like `@Composable` or `@Suppress("...")`
        if s.starts_with('@') {
            let bytes = s.as_bytes();
            let mut i = 1usize;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'(' {
                let mut depth = 1usize;
                i += 1;
                while i < bytes.len() && depth > 0 {
                    match bytes[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            let rest = s[i..].trim_start();
            if rest.len() == s.len() {
                return None;
            }
            s = rest;
            continue;
        }
        let end = s
            .bytes()
            .position(|c| !(c.is_ascii_alphanumeric() || c == b'_'))
            .unwrap_or(s.len());
        if end == 0 {
            return None;
        }
        let word = &s[..end];
        let after = &s[end..];
        if !after.starts_with(|c: char| c.is_ascii_whitespace()) {
            return None;
        }
        if KT_MODIFIERS.contains(&word) {
            if word == "override" {
                has_override = true;
            }
            s = after.trim_start();
            continue;
        }
        let kind = *KT_DECL_KINDS.iter().find(|&&k| k == word)?;
        let mut tail = after.trim_start();
        // Skip optional generic parameter list `<T, U>`
        if tail.starts_with('<') {
            let mut depth = 0usize;
            let mut idx = 0usize;
            for (i, ch) in tail.char_indices() {
                match ch {
                    '<' => depth += 1,
                    '>' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            idx = i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if idx == 0 {
                return None;
            }
            tail = tail[idx..].trim_start();
        }
        // Skip optional receiver type `Foo<Bar>.` before member name
        let bytes = tail.as_bytes();
        let mut i = 0usize;
        let mut depth = 0usize;
        let mut last_dot: Option<usize> = None;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'<' {
                depth += 1;
            } else if c == b'>' {
                depth = depth.saturating_sub(1);
            } else if depth == 0 {
                if c == b'.' {
                    last_dot = Some(i);
                } else if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'?') {
                    break;
                }
            }
            i += 1;
        }
        let name_start = last_dot.map_or(0, |d| d + 1);
        let name_slice = &tail[name_start..];
        let name_end = name_slice
            .bytes()
            .position(|c| !(c.is_ascii_alphanumeric() || c == b'_'))
            .unwrap_or(name_slice.len());
        if name_end == 0 {
            return None;
        }
        let name = &name_slice[..name_end];
        if !name.as_bytes()[0].is_ascii_alphabetic() && name.as_bytes()[0] != b'_' {
            return None;
        }
        return Some((has_override, kind, name));
    }
}

fn check_dead_code(
    texts: &BTreeMap<&str, String>,
    stripped: &HashMap<&str, String>,
    out: &mut Vec<Problem>,
) {
    let mut main_words: HashMap<&str, usize> = HashMap::new();
    let mut test_words: HashMap<&str, usize> = HashMap::new();
    let mut declarations: HashMap<&str, usize> = HashMap::new();
    let mut found: Vec<(&str, usize, bool, &'static str, &str, &str)> = Vec::new();

    for &path in texts.keys() {
        let ss = source_set(path);
        let code = &stripped[path];
        if MAIN_SETS.contains(&ss) {
            for_each_word(code, |w| *main_words.entry(w).or_insert(0) += 1);
            for (idx, line) in code.lines().enumerate() {
                if let Some((has_override, kind, name)) = parse_kt_declaration(line) {
                    *declarations.entry(name).or_insert(0) += 1;
                    found.push((path, idx + 1, has_override, kind, name, line));
                }
            }
        } else if TEST_SETS.contains(&ss) {
            for_each_word(code, |w| *test_words.entry(w).or_insert(0) += 1);
        }
    }

    let mut seen: HashSet<&str> = HashSet::new();
    for (path, line_no, has_override, kind, name, line_text) in found {
        if seen.contains(name) || has_override || name.starts_with('_') || name == "main" {
            continue;
        }
        if matches!(kind, "val" | "var") && line_text.trim_end().ends_with(',') {
            continue;
        }
        let total = main_words.get(name).copied().unwrap_or(0);
        let decls = declarations.get(name).copied().unwrap_or(0);
        if total > decls {
            continue;
        }
        seen.insert(name);
        if test_words.get(name).copied().unwrap_or(0) > 0 {
            out.push(Problem {
                rule: "dead-code",
                path: path.to_string(),
                line: line_no,
                message: format!(
                    "{name} is used only by tests; remove it or make it private to tests."
                ),
            });
        } else {
            out.push(Problem {
                rule: "dead-code",
                path: path.to_string(),
                line: line_no,
                message: format!("{name} is never used."),
            });
        }
    }
}

fn check_file_sizes(texts: &BTreeMap<&str, String>, out: &mut Vec<Problem>) {
    for (&path, text) in texts {
        if !MAIN_SETS.contains(&source_set(path)) {
            continue;
        }
        let lines = text.bytes().filter(|&b| b == b'\n').count();
        if lines > MAX_KOTLIN_LINES {
            out.push(Problem {
                rule: "file-size",
                path: path.to_string(),
                line: 0,
                message: format!(
                    "{lines} lines exceeds the limit of {MAX_KOTLIN_LINES}. Split by responsibility; do not raise the limit."
                ),
            });
        }
    }
}

fn check_banned(
    texts: &BTreeMap<&str, String>,
    stripped: &HashMap<&str, String>,
    out: &mut Vec<Problem>,
) {
    for &path in texts.keys() {
        if !MAIN_SETS.contains(&source_set(path)) {
            continue;
        }
        let code = &stripped[path];
        for (idx, line) in code.lines().enumerate() {
            let line_no = idx + 1;
            if line.contains("androidx.compose.material") {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Compose Foundation only: Material is not allowed.".into(),
                });
            }
            if contains_word(line, "GlobalScope") {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Use an owned coroutine scope, not GlobalScope.".into(),
                });
            }
            if contains_unqualified_call(line, "println(") {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Do not leave console output in app code.".into(),
                });
            }
            if contains_word(line, "FIXME") || contains_unqualified_call(line, "TODO(") {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Track unfinished work in TODO.md.".into(),
                });
            }
            if line.contains(".clickable(enabled = false)") {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Use blockTapThrough(); a disabled clickable merges reader semantics."
                        .into(),
                });
            }
            if contains_raw_profile_protocol(line) {
                out.push(Problem {
                    rule: "banned",
                    path: path.to_string(),
                    line: line_no,
                    message: "Stamp resolveWireProtocol(profile); a Custom profile's stored protocol hides its real wire.".into(),
                });
            }
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn contains_word(line: &str, word: &str) -> bool {
    let b = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(word) {
        let pos = from + rel;
        let end = pos + word.len();
        let left_ok = pos == 0 || !is_ident_byte(b[pos - 1]);
        let right_ok = end == b.len() || !is_ident_byte(b[end]);
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

fn contains_unqualified_call(line: &str, needle: &str) -> bool {
    let b = line.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(needle) {
        let pos = from + rel;
        let left_ok = pos == 0 || !(is_ident_byte(b[pos - 1]) || b[pos - 1] == b'.');
        if left_ok {
            return true;
        }
        from = pos + needle.len();
    }
    false
}

fn contains_raw_profile_protocol(line: &str) -> bool {
    let needle = "providerProtocol = ";
    let mut from = 0usize;
    while let Some(rel) = line[from..].find(needle) {
        let pos = from + rel;
        let left_ok = pos == 0 || !is_ident_byte(line.as_bytes()[pos - 1]);
        let rest = &line[pos + needle.len()..];
        let ident_len = rest.bytes().take_while(|&c| is_ident_byte(c)).count();
        if left_ok && ident_len > 0 && rest[ident_len..].starts_with(".protocol") {
            let after = ident_len + ".protocol".len();
            if after == rest.len() || !is_ident_byte(rest.as_bytes()[after]) {
                return true;
            }
        }
        from = pos + needle.len();
    }
    false
}

fn check_docs(files: &BTreeMap<String, Vec<u8>>, out: &mut Vec<Problem>) {
    for path in ["AGENTS.md", "TODO.md"] {
        match files.get(path) {
            None => out.push(Problem {
                rule: "docs",
                path: path.into(),
                line: 0,
                message: "Required workflow file is missing.".into(),
            }),
            Some(data) => {
                let text = String::from_utf8_lossy(data);
                let chars = text.chars().count();
                if chars > DOC_BUDGET_CHARS {
                    out.push(Problem {
                        rule: "docs",
                        path: path.into(),
                        line: 0,
                        message: format!(
                            "{chars} characters exceeds the {DOC_BUDGET_CHARS} character budget. Condense it; do not raise the budget."
                        ),
                    });
                }
            }
        }
    }
    if let Some(agents) = files.get("AGENTS.md") {
        let text = String::from_utf8_lossy(agents);
        if !text.contains("cargo xtask") {
            out.push(Problem {
                rule: "docs",
                path: "AGENTS.md".into(),
                line: 0,
                message: "Must tell readers to use cargo xtask.".into(),
            });
        }
    }
    let Some(todo_bytes) = files.get("TODO.md") else {
        return;
    };
    let todo = String::from_utf8_lossy(todo_bytes);
    let status = status_block(&todo);
    if status.is_empty() {
        out.push(Problem {
            rule: "docs",
            path: "TODO.md".into(),
            line: 0,
            message: format!("Missing the '{STATUS_HEADING}' section."),
        });
    } else {
        for field in STATUS_FIELDS {
            if !status.contains(field) {
                out.push(Problem {
                    rule: "docs",
                    path: "TODO.md".into(),
                    line: 0,
                    message: format!("Current status must include {field}"),
                });
            }
        }
        let status_chars = status.chars().count();
        if status_chars > MAX_STATUS_CHARS {
            out.push(Problem {
                rule: "docs",
                path: "TODO.md".into(),
                line: 0,
                message: format!(
                    "Current status is {status_chars} characters; keep it under {MAX_STATUS_CHARS}."
                ),
            });
        }
    }
    let mut task_counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in todo.lines() {
        if let Some(id) = parse_task_id(line) {
            *task_counts.entry(id.to_string()).or_insert(0) += 1;
        }
    }
    for (id, count) in task_counts {
        if count > 1 {
            out.push(Problem {
                rule: "docs",
                path: "TODO.md".into(),
                line: 0,
                message: format!("Task {id} appears {count} times."),
            });
        }
    }
}

fn parse_task_id(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("- [ ] **")
        .or_else(|| line.strip_prefix("- [x] **"))?;
    let b = rest.as_bytes();
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'.' {
        return None;
    }
    i += 1;
    let minor_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == minor_start {
        return None;
    }
    if i < b.len() && b[i].is_ascii_lowercase() {
        i += 1;
    }
    if i < b.len() && is_ident_byte(b[i]) {
        return None;
    }
    Some(&rest[..i])
}

fn check_secrets(files: &BTreeMap<String, Vec<u8>>, out: &mut Vec<Problem>) {
    for path in files.keys() {
        let name = path.rsplit('/').next().unwrap_or(path.as_str());
        if has_suffix_ci(path, SECRET_SUFFIXES) || SECRET_NAMES.contains(&name) {
            out.push(Problem {
                rule: "secrets",
                path: path.clone(),
                line: 0,
                message: "Private signing or credential files must never be tracked.".into(),
            });
        }
    }
}

fn strip_code_fences_preserving_lines(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_fence = false;
    for line in md.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            if line.ends_with('\n') {
                out.push('\n');
            }
            continue;
        }
        if in_fence {
            if line.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(line);
        }
    }
    out
}

pub fn heading_anchors(markdown: &str) -> HashSet<String> {
    let cleaned = strip_code_fences_preserving_lines(markdown);
    let mut anchors = HashSet::new();
    for line in cleaned.lines() {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &line[hashes..];
        if !rest.starts_with(' ') && !rest.starts_with('\t') {
            continue;
        }
        let title = rest.trim().trim_end_matches('#').trim();
        let mut slug = String::with_capacity(title.len());
        for ch in title.chars() {
            if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                for lc in ch.to_lowercase() {
                    slug.push(lc);
                }
            } else if ch == ' ' {
                slug.push('-');
            }
        }
        anchors.insert(slug);
    }
    anchors
}

fn has_uri_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    let b = scheme.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphabetic()
        && b.iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
}

fn norm_posix_path(base_dir: &str, rel: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    let combined = if base_dir.is_empty() {
        rel.to_string()
    } else {
        format!("{base_dir}/{rel}")
    };
    for comp in combined.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            other => stack.push(other),
        }
    }
    stack.join("/")
}

fn check_links(files: &BTreeMap<String, Vec<u8>>, out: &mut Vec<Problem>) {
    let mut docs: HashMap<&str, String> = HashMap::new();
    let mut anchors_cache: HashMap<&str, HashSet<String>> = HashMap::new();
    for (path, data) in files {
        if path.ends_with(".md") {
            let text = String::from_utf8_lossy(data).into_owned();
            anchors_cache.insert(path.as_str(), heading_anchors(&text));
            docs.insert(path.as_str(), text);
        }
    }
    for (&path, text) in &docs {
        let stripped = strip_code_fences_preserving_lines(text);
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
        for (idx, line) in stripped.lines().enumerate() {
            let line_no = idx + 1;
            let bytes = line.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                if bytes[i] == b'['
                    && let Some(close_br) = line[i + 1..].find(']')
                {
                    let after_br = i + 1 + close_br + 1;
                    if after_br < bytes.len()
                        && bytes[after_br] == b'('
                        && let Some(close_p) = line[after_br + 1..].find(')')
                    {
                        let inner = line[after_br + 1..after_br + 1 + close_p].trim();
                        let target = inner.split_whitespace().next().unwrap_or("");
                        i = after_br + 1 + close_p + 1;
                        if target.is_empty() || has_uri_scheme(target) {
                            continue;
                        }
                        let (file_part, anchor) = match target.split_once('#') {
                            Some((f, a)) => (f, Some(a)),
                            None => (target, None),
                        };
                        let resolved = if file_part.is_empty() {
                            path.to_string()
                        } else {
                            norm_posix_path(parent, file_part)
                        };
                        let dir_prefix = format!("{}/", resolved.trim_end_matches('/'));
                        let exists = files.contains_key(&resolved)
                            || files.keys().any(|k| k.starts_with(&dir_prefix));
                        if !exists {
                            out.push(Problem {
                                rule: "docs",
                                path: path.to_string(),
                                line: line_no,
                                message: format!("Broken link: {target}"),
                            });
                        } else if let Some(anc) = anchor
                            && let Some(valid) = anchors_cache.get(resolved.as_str())
                            && !valid.contains(&anc.to_lowercase())
                        {
                            out.push(Problem {
                                rule: "docs",
                                path: path.to_string(),
                                line: line_no,
                                message: format!("Missing anchor: {target}"),
                            });
                        }
                        continue;
                    }
                }
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_docs() -> BTreeMap<String, Vec<u8>> {
        let mut m = BTreeMap::new();
        m.insert("AGENTS.md".into(), b"Run cargo xtask affected.\n".to_vec());
        m.insert(
            "TODO.md".into(),
            b"## Current status\n- **Source:** a\n- **Last verified:** b\n- **Next action:** c\n\n## Open\n- [ ] **1.1 First:** ok\n".to_vec(),
        );
        m
    }

    #[test]
    fn enforces_encoding_bom_crlf_and_ps1_ascii() {
        let mut files = base_docs();
        files.insert("a.txt".into(), b"\xef\xbb\xbfhello\r\n".to_vec());
        files.insert("run.bat".into(), b"echo hi\n".to_vec());
        files.insert("ok.ps1".into(), "Write-Host '—'\n".as_bytes().to_vec());
        let probs = check_files(&files);
        let enc: Vec<_> = probs.iter().filter(|p| p.rule == "encoding").collect();
        assert_eq!(enc.len(), 4);
    }

    #[test]
    fn enforces_10k_char_doc_budgets_and_unique_tasks() {
        let mut files = base_docs();
        let prefix = "cargo xtask\n";
        let exact = format!("{prefix}{}", "—".repeat(DOC_BUDGET_CHARS - prefix.len()));
        files.insert("AGENTS.md".into(), exact.as_bytes().to_vec());
        assert!(check_files(&files).is_empty());

        let over = format!("{exact}x");
        files.insert("AGENTS.md".into(), over.as_bytes().to_vec());
        assert!(check_files(&files).iter().any(|p| p.rule == "docs"));

        let mut dup = base_docs();
        dup.get_mut("TODO.md")
            .unwrap()
            .extend_from_slice(b"- [x] **1.1 Again:** dup\n");
        assert!(
            check_files(&dup)
                .iter()
                .any(|p| p.message.contains("appears 2 times"))
        );
    }

    #[test]
    fn enforces_kotlin_imports_banned_and_dead_code() {
        let mut files = base_docs();
        files.insert(
            "composeApp/src/commonMain/kotlin/io/parchment/ai/A.kt".into(),
            b"package a\nimport x.Unused\nval v = b(providerProtocol = profile.protocol)\nfun main() { println(v) }\n".to_vec(),
        );
        let probs = check_files(&files);
        assert!(probs.iter().any(|p| p.rule == "unused-import"));
        assert!(
            probs
                .iter()
                .any(|p| p.rule == "banned" && p.message.contains("console output"))
        );
        assert!(
            probs
                .iter()
                .any(|p| p.rule == "banned" && p.message.contains("resolveWireProtocol"))
        );
    }

    #[test]
    fn validates_relative_markdown_links_and_anchors() {
        let mut files = base_docs();
        files.insert(
            "docs/A.md".into(),
            b"# My Heading\n\nSee [ok](../AGENTS.md) and [bad](#nope).\n".to_vec(),
        );
        let probs = check_files(&files);
        assert_eq!(probs.len(), 1);
        assert!(probs[0].message.contains("Missing anchor"));
    }

    #[test]
    fn rejects_tracked_signing_keys() {
        let mut files = base_docs();
        files.insert("release.p12".into(), Vec::new());
        assert!(check_files(&files).iter().any(|p| p.rule == "secrets"));
    }
}
