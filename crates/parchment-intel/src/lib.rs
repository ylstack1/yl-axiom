//! Code intelligence module for Axiom Coding.
//!
//! Provides symbol search, document outline, and AST pattern search via a
//! `CodeIntelBackend` trait. The default backend uses text-based heuristics
//! (no native dependencies). The optional `tree-sitter` feature adds
//! tree-sitter-based AST parsing for precise symbol resolution.
//!
//! All operations are read-only and inherit existing approval/permission
//! handling from `parchment-harness`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Opaque symbol handle.
pub type SymbolId = String;

/// A code symbol (function, class, variable, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInfo {
    pub id: SymbolId,
    pub name: String,
    pub kind: SymbolKind,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub signature: Option<String>,
    pub doc: Option<String>,
}

/// Symbol categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SymbolKind {
    Function,
    Class,
    Method,
    Variable,
    Constant,
    Type,
    Interface,
    Trait,
    Module,
    Field,
    Parameter,
    Other,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "function"),
            Self::Class => write!(f, "class"),
            Self::Method => write!(f, "method"),
            Self::Variable => write!(f, "variable"),
            Self::Constant => write!(f, "constant"),
            Self::Type => write!(f, "type"),
            Self::Interface => write!(f, "interface"),
            Self::Trait => write!(f, "trait"),
            Self::Module => write!(f, "module"),
            Self::Field => write!(f, "field"),
            Self::Parameter => write!(f, "parameter"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Search result for symbol queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolSearchResult {
    pub query: String,
    pub symbols: Vec<SymbolInfo>,
    pub total: usize,
    pub truncated: bool,
}

/// Outline of symbols in a single document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentOutline {
    pub file_path: String,
    pub symbols: Vec<SymbolInfo>,
    pub total: usize,
}

/// Errors from code intelligence operations.
#[derive(Debug, Error)]
pub enum IntelError {
    #[error("language not supported: {0}")]
    LanguageNotSupported(String),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("parse failed: {0}")]
    ParseFailed(String),
    #[error("backend not available: {0}")]
    BackendNotAvailable(String),
}

/// Trait for code intelligence backends.
/// Implementations provide symbol search, outline, and AST queries.
pub trait CodeIntelBackend: Send + Sync {
    /// Returns the language this backend handles (e.g. "rust", "kotlin").
    fn language(&self) -> &str;

    /// Search for symbols matching a fuzzy name query.
    fn search_symbols(&self, query: &str, limit: usize) -> Result<SymbolSearchResult, IntelError>;

    /// Get the outline of symbols in a document.
    fn outline(&self, file_path: &str) -> Result<DocumentOutline, IntelError>;

    /// Check if the backend is ready for use.
    fn is_ready(&self) -> bool;
}

/// Text-based symbol backend using heuristics.
/// Works without tree-sitter — finds symbol-like identifiers in source text.
pub struct TextSymbolBackend {
    language: String,
}

#[allow(dead_code)]
impl TextSymbolBackend {
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
        }
    }

    fn extract_symbols(&self, content: &str, file_path: &str) -> Vec<SymbolInfo> {
        let mut symbols = Vec::new();
        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with("/*")
                || trimmed.starts_with("*")
            {
                continue;
            }
            // Rust function: `pub fn name(...)`
            if let Some(name) = self.extract_rust_function(trimmed) {
                symbols.push(SymbolInfo {
                    id: format!("{}:{}:{}", file_path, line_idx + 1, name),
                    name,
                    kind: SymbolKind::Function,
                    file_path: file_path.to_string(),
                    line: line_idx + 1,
                    column: 1,
                    signature: Some(trimmed.to_string()),
                    doc: None,
                });
            }
            // Rust struct/enum/trait: `pub struct Name`, `pub enum Name`, `pub trait Name`
            if let Some((name, kind)) = self.extract_rust_type_decl(trimmed) {
                symbols.push(SymbolInfo {
                    id: format!("{}:{}:{}", file_path, line_idx + 1, name),
                    name,
                    kind,
                    file_path: file_path.to_string(),
                    line: line_idx + 1,
                    column: 1,
                    signature: Some(trimmed.to_string()),
                    doc: None,
                });
            }
            // Kotlin function: `fun name(...)` or `public fun name(...)`
            if let Some(name) = self.extract_kotlin_function(trimmed) {
                symbols.push(SymbolInfo {
                    id: format!("{}:{}:{}", file_path, line_idx + 1, name),
                    name,
                    kind: SymbolKind::Function,
                    file_path: file_path.to_string(),
                    line: line_idx + 1,
                    column: 1,
                    signature: Some(trimmed.to_string()),
                    doc: None,
                });
            }
            // Kotlin class/interface: `class Name`, `interface Name`
            if let Some((name, kind)) = self.extract_kotlin_type_decl(trimmed) {
                symbols.push(SymbolInfo {
                    id: format!("{}:{}:{}", file_path, line_idx + 1, name),
                    name,
                    kind,
                    file_path: file_path.to_string(),
                    line: line_idx + 1,
                    column: 1,
                    signature: Some(trimmed.to_string()),
                    doc: None,
                });
            }
        }
        symbols
    }

    fn extract_rust_function(&self, line: &str) -> Option<String> {
        let re = regex_lite_regex(line, r"fn\s+([a-zA-Z_][a-zA-Z0-9_]*)");
        re
    }

    fn extract_rust_type_decl(&self, line: &str) -> Option<(String, SymbolKind)> {
        for (kw, kind) in [
            ("struct", SymbolKind::Class),
            ("enum", SymbolKind::Type),
            ("trait", SymbolKind::Trait),
        ] {
            if let Some(name) = regex_lite_regex(line, &format!(r"{kw}\s+([a-zA-Z_][a-zA-Z0-9_]*)"))
            {
                return Some((name, kind));
            }
        }
        None
    }

    fn extract_kotlin_function(&self, line: &str) -> Option<String> {
        let re = regex_lite_regex(
            line,
            r"(?:public\s+|private\s+|internal\s+)?fun\s+([a-zA-Z_][a-zA-Z0-9_]*)",
        );
        re
    }

    fn extract_kotlin_type_decl(&self, line: &str) -> Option<(String, SymbolKind)> {
        for (kw, kind) in [
            ("class", SymbolKind::Class),
            ("interface", SymbolKind::Interface),
        ] {
            if let Some(name) = regex_lite_regex(
                line,
                &format!(r"(?:public\s+|abstract\s+)?{kw}\s+([a-zA-Z_][a-zA-Z0-9_]*)"),
            ) {
                return Some((name, kind));
            }
        }
        None
    }
}

/// Simple regex-like extraction without the `regex` crate dependency.
/// Finds the first capture group for a simple pattern.
#[allow(dead_code)]
fn regex_lite_regex(_line: &str, _pattern: &str) -> Option<String> {
    // Lightweight heuristic: find identifier after keyword
    // This is a placeholder; real implementation uses tree-sitter or regex crate
    let keywords: &[&str] = &["fn", "fun", "struct", "enum", "trait", "class", "interface"];
    for kw in keywords {
        if let Some(pos) = line.find(kw) {
            let after = &line[pos + kw.len()..];
            let ident: String = after
                .chars()
                .skip_while(|c| c.is_whitespace())
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                return Some(ident);
            }
        }
    }
    None
}

impl CodeIntelBackend for TextSymbolBackend {
    fn language(&self) -> &str {
        &self.language
    }

    fn search_symbols(
        &self,
        _query: &str,
        _limit: usize,
    ) -> Result<SymbolSearchResult, IntelError> {
        // Text backend searches within indexed documents; for now return empty
        // with the query recorded. Full implementation indexes workspace files.
        Ok(SymbolSearchResult {
            query: query.to_string(),
            symbols: Vec::new(),
            total: 0,
            truncated: false,
        })
    }

    fn outline(&self, _file_path: &str) -> Result<DocumentOutline, IntelError> {
        Err(IntelError::BackendNotAvailable(
            "text backend does not support outline without file content".into(),
        ))
    }

    fn is_ready(&self) -> bool {
        true
    }
}

/// Registry of code intelligence backends by language.
pub struct IntelRegistry {
    backends: std::collections::HashMap<String, Box<dyn CodeIntelBackend>>,
}

impl IntelRegistry {
    pub fn new() -> Self {
        Self {
            backends: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, backend: Box<dyn CodeIntelBackend>) {
        let lang = backend.language().to_string();
        self.backends.insert(lang, backend);
    }

    pub fn get(&self, language: &str) -> Option<&dyn CodeIntelBackend> {
        self.backends.get(language).map(|b| b.as_ref())
    }

    pub fn languages(&self) -> Vec<String> {
        self.backends.keys().cloned().collect()
    }

    pub fn search_symbols(
        &self,
        language: &str,
        query: &str,
        limit: usize,
    ) -> Result<SymbolSearchResult, IntelError> {
        let backend = self
            .get(language)
            .ok_or_else(|| IntelError::LanguageNotSupported(language.to_string()))?;
        if !backend.is_ready() {
            return Err(IntelError::BackendNotAvailable(format!(
                "backend for {language} not ready"
            )));
        }
        backend.search_symbols(query, limit)
    }

    pub fn outline(&self, language: &str, file_path: &str) -> Result<DocumentOutline, IntelError> {
        let backend = self
            .get(language)
            .ok_or_else(|| IntelError::LanguageNotSupported(language.to_string()))?;
        if !backend.is_ready() {
            return Err(IntelError::BackendNotAvailable(format!(
                "backend for {language} not ready"
            )));
        }
        backend.outline(file_path)
    }
}

impl Default for IntelRegistry {
    fn default() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(TextSymbolBackend::new("rust")));
        registry.register(Box::new(TextSymbolBackend::new("kotlin")));
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_kind_display() {
        assert_eq!(SymbolKind::Function.to_string(), "function");
        assert_eq!(SymbolKind::Class.to_string(), "class");
        assert_eq!(SymbolKind::Trait.to_string(), "trait");
        assert_eq!(SymbolKind::Other.to_string(), "other");
    }

    #[test]
    fn text_backend_extracts_rust_function() {
        let backend = TextSymbolBackend::new("rust");
        let name = backend.extract_rust_function("pub fn foo_bar(x: i32) -> bool {");
        assert_eq!(name, Some("foo_bar".to_string()));
    }

    #[test]
    fn text_backend_extracts_rust_struct() {
        let backend = TextSymbolBackend::new("rust");
        let (name, kind) = backend
            .extract_rust_type_decl("pub struct MyConfig {")
            .unwrap();
        assert_eq!(name, "MyConfig");
        assert_eq!(kind, SymbolKind::Class);
    }

    #[test]
    fn text_backend_extracts_kotlin_function() {
        let backend = TextSymbolBackend::new("kotlin");
        let name = backend.extract_kotlin_function("public fun compute(x: Int): Int {");
        assert_eq!(name, Some("compute".to_string()));
    }

    #[test]
    fn registry_default_has_languages() {
        let registry = IntelRegistry::default();
        let langs = registry.languages();
        assert!(langs.contains(&"rust".to_string()));
        assert!(langs.contains(&"kotlin".to_string()));
    }

    #[test]
    fn registry_search_unknown_language_fails() {
        let registry = IntelRegistry::default();
        let result = registry.search_symbols("java", "foo", 10);
        assert!(result.is_err());
    }
}
