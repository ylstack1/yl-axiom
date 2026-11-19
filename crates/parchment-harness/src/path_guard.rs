//! Workspace path boundary guard and ripgrep-backed file operations.
//!
//! Security contracts:
//! - Resolves existing ancestors via `fs::canonicalize` so writes to missing paths can never cross
//!   a symlink or Windows NTFS junction escaping the mounted workspace root.
//! - Recursive operations skip symlinks/reparse points to prevent cycles and out-of-root escapes.
//! - `fs_read` caps `4,000` chars/line, `10,000` lines, and `250,000` total chars.
//! - `fs_search` uses `ignore` + `grep-searcher` + `grep-regex` (ripgrep core engine).

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "ripgrep")]
use globset::{Glob, GlobSetBuilder};
#[cfg(feature = "ripgrep")]
use grep_regex::RegexMatcherBuilder;
#[cfg(feature = "ripgrep")]
use grep_searcher::sinks::UTF8;
#[cfg(feature = "ripgrep")]
use grep_searcher::{BinaryDetection, SearcherBuilder};
#[cfg(feature = "ripgrep")]
use ignore::WalkBuilder;
use thiserror::Error;

pub const MAX_READ_LINE_CHARS: usize = 4_000;
pub const MAX_READ_LINES: usize = 10_000;
pub const MAX_READ_TOTAL_CHARS: usize = 250_000;
pub const MAX_EDIT_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Workspace I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct WorkspacePathGuard;

impl WorkspacePathGuard {
    pub fn is_ignored_directory(name: &str) -> bool {
        matches!(
            name.to_ascii_lowercase().as_str(),
            ".git"
                | "build"
                | ".gradle"
                | "target"
                | "node_modules"
                | ".idea"
                | ".kotlin"
                | "__pycache__"
                | ".venv"
                | "venv"
                | ".cxx"
                | ".externalnativebuild"
        )
    }

    pub fn root(workspace_root: &str) -> Result<PathBuf, WorkspaceError> {
        let trimmed = workspace_root.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::Denied(
                "No workspace directory is mounted.".into(),
            ));
        }
        let abs = make_absolute_normalized(Path::new(trimmed))?;
        let real = real_path_with_missing_suffix(&abs)?;
        if let Ok(meta) = fs::symlink_metadata(&real)
            && !meta.is_dir()
        {
            return Err(WorkspaceError::Denied(
                "Workspace root is not a directory.".into(),
            ));
        }
        Ok(real)
    }

    /// Resolves `relative_path` strictly inside `workspace_root`, checking canonicalized real
    /// ancestors so neither `..` traversal nor symlinks/junctions can escape the root.
    pub fn resolve(workspace_root: &str, relative_path: &str) -> Result<PathBuf, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let cleaned = relative_path.trim();
        if cleaned.starts_with('/')
            || cleaned.starts_with('\\')
            || cleaned.contains(':')
            || cleaned.contains('\0')
        {
            return Err(WorkspaceError::Denied(
                "Access denied: Use a relative workspace path.".into(),
            ));
        }

        let rel = if cleaned.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(cleaned)
        };
        if rel.is_absolute() {
            return Err(WorkspaceError::Denied(
                "Access denied: Absolute paths are not allowed.".into(),
            ));
        }

        let target = normalize_lexical(&root.join(&rel));
        if !target.starts_with(&root) {
            return Err(WorkspaceError::Denied(format!(
                "Access denied: Path '{relative_path}' escapes the mounted workspace."
            )));
        }

        let real_target = real_path_with_missing_suffix(&target)?;
        if !real_target.starts_with(&root) {
            return Err(WorkspaceError::Denied(
                "Access denied: A link or junction escapes the mounted workspace.".into(),
            ));
        }

        Ok(real_target)
    }

    pub fn can_traverse(root: &Path, target: &Path) -> bool {
        let Ok(meta) = fs::symlink_metadata(target) else {
            return false;
        };
        if meta.file_type().is_symlink() {
            return false;
        }
        let Ok(real) = fs::canonicalize(target) else {
            return false;
        };
        if !real.starts_with(root) {
            return false;
        }
        // Reject NTFS junctions (on Windows, a junction has is_dir()=true and canonical path differs
        // from lexical target when pointing elsewhere, or file_type().is_symlink() on modern Rust).
        if let Ok(abs) = make_absolute_normalized(target)
            && abs != root
            && strip_win_verbatim(&real) != strip_win_verbatim(&abs)
        {
            return false;
        }
        true
    }

    pub fn require_unlinked_tree(root: &Path, target: &Path) -> Result<(), WorkspaceError> {
        if !Self::can_traverse(root, target) {
            return Err(WorkspaceError::Denied(
                "Access denied: Linked workspace targets cannot be moved or recursively deleted."
                    .into(),
            ));
        }
        if target.is_dir() {
            for entry in fs::read_dir(target)? {
                let entry = entry?;
                Self::require_unlinked_tree(root, &entry.path())?;
            }
        }
        Ok(())
    }

    pub fn list_dir(
        workspace_root: &str,
        relative_path: &str,
        recursive: bool,
        max_entries: usize,
    ) -> Result<String, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let dir = Self::resolve(workspace_root, relative_path)?;
        if !dir.is_dir() {
            return Err(WorkspaceError::Invalid(format!(
                "Path '{}' is not a directory.",
                relative_path
            )));
        }
        let cap = max_entries.clamp(1, 2_000);
        let mut lines = Vec::new();
        if !recursive {
            let mut entries: Vec<_> = fs::read_dir(&dir)?.filter_map(Result::ok).collect();
            entries.sort_by_key(|e| e.file_name());
            for entry in entries {
                if lines.len() >= cap {
                    lines.push(format!("... (truncated at {cap} entries)"));
                    break;
                }
                let p = entry.path();
                if !Self::can_traverse(&root, &p) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if p.is_dir() && Self::is_ignored_directory(&name) {
                    continue;
                }
                let suffix = if p.is_dir() { "/" } else { "" };
                lines.push(format!("{name}{suffix}"));
            }
        } else {
            #[cfg(feature = "ripgrep")]
            {
                let walker = WalkBuilder::new(&dir)
                    .follow_links(false)
                    .hidden(false)
                    .git_ignore(true)
                    .filter_entry(|e| {
                        if e.depth() == 0 {
                            return true;
                        }
                        let name = e.file_name().to_string_lossy();
                        !(e.file_type().is_some_and(|ft| ft.is_dir())
                            && Self::is_ignored_directory(&name))
                    })
                    .build();
                for entry in walker.flatten() {
                    if entry.depth() == 0 {
                        continue;
                    }
                    let p = entry.path();
                    if !Self::can_traverse(&root, p) {
                        continue;
                    }
                    if lines.len() >= cap {
                        lines.push(format!("... (truncated at {cap} entries)"));
                        break;
                    }
                    if let Ok(rel) = p.strip_prefix(&root) {
                        let suffix = if p.is_dir() { "/" } else { "" };
                        lines.push(format!(
                            "{}{}",
                            rel.to_string_lossy().replace('\\', "/"),
                            suffix
                        ));
                    }
                }
            }
            #[cfg(not(feature = "ripgrep"))]
            {
                let mut collected = Vec::new();
                collect_unlinked_entries(&root, &dir, cap + 1, &mut collected);
                for p in collected {
                    if lines.len() >= cap {
                        lines.push(format!("... (truncated at {cap} entries)"));
                        break;
                    }
                    if let Ok(rel) = p.strip_prefix(&root) {
                        let suffix = if p.is_dir() { "/" } else { "" };
                        lines.push(format!(
                            "{}{}",
                            rel.to_string_lossy().replace('\\', "/"),
                            suffix
                        ));
                    }
                }
            }
        }
        Ok(lines.join("\n"))
    }

    pub fn glob_files(
        workspace_root: &str,
        pattern: &str,
        max_results: usize,
    ) -> Result<String, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::Invalid(
                "Glob pattern cannot be empty.".into(),
            ));
        }
        let cap = max_results.clamp(1, 2_000);
        let mut matches = Vec::new();
        #[cfg(feature = "ripgrep")]
        {
            let mut builder = GlobSetBuilder::new();
            let glob = Glob::new(trimmed)
                .map_err(|e| WorkspaceError::Invalid(format!("Invalid glob pattern: {e}")))?;
            builder.add(glob);
            let set = builder
                .build()
                .map_err(|e| WorkspaceError::Invalid(format!("Invalid glob set: {e}")))?;
            let walker = WalkBuilder::new(&root)
                .follow_links(false)
                .hidden(false)
                .git_ignore(true)
                .filter_entry(|e| {
                    if e.depth() == 0 {
                        return true;
                    }
                    let name = e.file_name().to_string_lossy();
                    !(e.file_type().is_some_and(|ft| ft.is_dir())
                        && Self::is_ignored_directory(&name))
                })
                .build();
            for entry in walker.flatten() {
                if entry.depth() == 0 {
                    continue;
                }
                let p = entry.path();
                if !Self::can_traverse(&root, p) {
                    continue;
                }
                if let Ok(rel) = p.strip_prefix(&root) {
                    let norm = rel.to_string_lossy().replace('\\', "/");
                    if set.is_match(&norm) {
                        matches.push(norm);
                        if matches.len() >= cap {
                            break;
                        }
                    }
                }
            }
        }
        #[cfg(not(feature = "ripgrep"))]
        {
            let mut collected = Vec::new();
            collect_unlinked_entries(&root, &root, 20_000, &mut collected);
            for p in collected {
                if let Ok(rel) = p.strip_prefix(&root) {
                    let norm = rel.to_string_lossy().replace('\\', "/");
                    if simple_glob_match(trimmed, &norm) {
                        matches.push(norm);
                        if matches.len() >= cap {
                            break;
                        }
                    }
                }
            }
        }
        matches.sort();
        Ok(matches.join("\n"))
    }

    /// Reads UTF-8 lines with strict caps (`4k` chars/line, `10k` lines, `250k` total chars).
    pub fn read_file_lines(
        workspace_root: &str,
        relative_path: &str,
        start_line: usize,
        max_lines: usize,
    ) -> Result<String, WorkspaceError> {
        let target = Self::resolve(workspace_root, relative_path)?;
        let meta = fs::metadata(&target)?;
        if !meta.is_file() {
            return Err(WorkspaceError::Invalid(format!(
                "Path '{relative_path}' is not a regular file."
            )));
        }
        let bytes = fs::read(&target)?;
        let text = String::from_utf8_lossy(&bytes);
        let start = start_line.max(1);
        let limit = max_lines.clamp(1, MAX_READ_LINES);
        let mut out = String::new();
        let mut total_chars = 0usize;
        let mut emitted = 0usize;
        let mut truncated = false;

        for (idx, raw_line) in text.lines().enumerate() {
            let line_no = idx + 1;
            if line_no < start {
                continue;
            }
            if emitted >= limit {
                truncated = true;
                break;
            }
            let clipped: String = if raw_line.chars().count() > MAX_READ_LINE_CHARS {
                let mut s: String = raw_line.chars().take(MAX_READ_LINE_CHARS).collect();
                s.push_str("...[line truncated]");
                s
            } else {
                raw_line.to_string()
            };
            let formatted = format!("{line_no}: {clipped}\n");
            let add_chars = formatted.chars().count();
            if total_chars + add_chars > MAX_READ_TOTAL_CHARS {
                truncated = true;
                break;
            }
            out.push_str(&formatted);
            total_chars += add_chars;
            emitted += 1;
        }
        if truncated {
            out.push_str("... [output truncated by workspaceRead safety limits; never rewrite a whole file from truncated reads]\n");
        }
        Ok(out)
    }

    /// Ripgrep-powered (`ignore` + `grep-searcher` + `grep-regex`) workspace search.
    pub fn search_files(
        workspace_root: &str,
        query: &str,
        scope_path: Option<&str>,
        file_glob: Option<&str>,
        max_results: usize,
    ) -> Result<String, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let search_root = match scope_path.map(str::trim).filter(|s| !s.is_empty()) {
            Some(rel) => Self::resolve(workspace_root, rel)?,
            None => root.clone(),
        };
        let q = query.trim();
        if q.is_empty() {
            return Err(WorkspaceError::Invalid(
                "Search query cannot be empty.".into(),
            ));
        }
        let cap = max_results.clamp(1, 500);
        let filter_glob = file_glob.map(str::trim).filter(|s| !s.is_empty());
        let mut results: Vec<String> = Vec::new();

        #[cfg(feature = "ripgrep")]
        {
            let matcher = RegexMatcherBuilder::new()
                .case_smart(true)
                .build(q)
                .or_else(|_| RegexMatcherBuilder::new().fixed_strings(true).build(q))
                .map_err(|e| WorkspaceError::Invalid(format!("Invalid search pattern: {e}")))?;

            let mut searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(b'\x00'))
                .line_number(true)
                .build();

            let walker = WalkBuilder::new(&search_root)
                .follow_links(false)
                .hidden(false)
                .git_ignore(true)
                .filter_entry(|e| {
                    if e.depth() == 0 {
                        return true;
                    }
                    let name = e.file_name().to_string_lossy();
                    !(e.file_type().is_some_and(|ft| ft.is_dir())
                        && Self::is_ignored_directory(&name))
                })
                .build();

            for entry in walker.flatten() {
                if results.len() >= cap {
                    break;
                }
                let p = entry.path();
                if !p.is_file() || !Self::can_traverse(&root, p) {
                    continue;
                }
                let rel_str = p
                    .strip_prefix(&root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| p.to_string_lossy().into_owned());
                if let Some(fg) = filter_glob
                    && !rel_str.contains(fg)
                    && !rel_str.ends_with(fg.trim_start_matches('*'))
                {
                    continue;
                }
                let remaining = cap - results.len();
                let mut file_hits = 0usize;
                let _ = searcher.search_path(
                    &matcher,
                    p,
                    UTF8(|lnum, line| {
                        let trimmed: String = line
                            .trim_end_matches(&['\r', '\n'][..])
                            .chars()
                            .take(300)
                            .collect();
                        results.push(format!("{rel_str}:{lnum}: {trimmed}"));
                        file_hits += 1;
                        Ok(file_hits < remaining)
                    }),
                );
            }
        }

        #[cfg(not(feature = "ripgrep"))]
        {
            let case_sensitive = q.chars().any(|c| c.is_uppercase());
            let needle_lower = q.to_lowercase();
            let mut entries = Vec::new();
            collect_unlinked_entries(&root, &search_root, 20_000, &mut entries);
            for p in entries {
                if results.len() >= cap {
                    break;
                }
                if !p.is_file() {
                    continue;
                }
                let Ok(meta) = fs::metadata(&p) else {
                    continue;
                };
                if meta.len() > 1_500_000 {
                    continue;
                }
                let rel_str = p
                    .strip_prefix(&root)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| p.to_string_lossy().into_owned());
                if let Some(fg) = filter_glob
                    && !simple_glob_match(fg, &rel_str)
                    && !rel_str.contains(fg)
                    && !rel_str.ends_with(fg.trim_start_matches('*'))
                {
                    continue;
                }
                let Ok(bytes) = fs::read(&p) else {
                    continue;
                };
                if bytes.iter().take(8_192).any(|&b| b == 0) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                for (idx, line) in text.lines().enumerate() {
                    let matched = if case_sensitive {
                        line.contains(q)
                    } else {
                        line.to_lowercase().contains(&needle_lower)
                    };
                    if matched {
                        let trimmed: String = line
                            .trim_end_matches(&['\r', '\n'][..])
                            .chars()
                            .take(300)
                            .collect();
                        results.push(format!("{rel_str}:{}: {trimmed}", idx + 1));
                        if results.len() >= cap {
                            break;
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok("No matches found.".into())
        } else {
            Ok(results.join("\n"))
        }
    }

    pub fn write_file_atomic(
        workspace_root: &str,
        relative_path: &str,
        content: &str,
    ) -> Result<String, WorkspaceError> {
        let target = Self::resolve(workspace_root, relative_path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // Re-verify after creating parent directories.
        let verified = Self::resolve(workspace_root, relative_path)?;
        let tmp = verified.with_extension(format!("parchment_tmp_{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(content.as_bytes())?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &verified)?;
        Ok(format!(
            "Wrote {} bytes to '{}'.",
            content.len(),
            relative_path
        ))
    }

    pub fn edit_file(
        workspace_root: &str,
        relative_path: &str,
        edits: &[(&str, &str, bool)],
    ) -> Result<String, WorkspaceError> {
        let target = Self::resolve(workspace_root, relative_path)?;
        let meta = fs::metadata(&target)?;
        if meta.len() > MAX_EDIT_FILE_BYTES {
            return Err(WorkspaceError::Invalid(
                "File exceeds 16 MiB edit cap.".into(),
            ));
        }
        let original = fs::read_to_string(&target)?;
        let uses_crlf = original.contains("\r\n");
        let mut working = original.replace("\r\n", "\n");
        let mut total_replacements = 0usize;

        for (idx, (old_raw, new_raw, replace_all)) in edits.iter().enumerate() {
            let old = old_raw.replace("\r\n", "\n");
            let new = new_raw.replace("\r\n", "\n");
            if old.is_empty() {
                return Err(WorkspaceError::Invalid(format!(
                    "Edit hunk #{} has empty old_text.",
                    idx + 1
                )));
            }
            let count = working.matches(&old).count();
            if count == 0 {
                return Err(WorkspaceError::Invalid(format!(
                    "Edit hunk #{} old_text not found in '{}'.",
                    idx + 1,
                    relative_path
                )));
            }
            if count > 1 && !*replace_all {
                return Err(WorkspaceError::Invalid(format!(
                    "Edit hunk #{} matched {count} times in '{}'; provide more context or set replace_all=true.",
                    idx + 1,
                    relative_path
                )));
            }
            if *replace_all {
                working = working.replace(&old, &new);
                total_replacements += count;
            } else {
                working = working.replacen(&old, &new, 1);
                total_replacements += 1;
            }
        }

        let final_text = if uses_crlf {
            working.replace('\n', "\r\n")
        } else {
            working
        };
        Self::write_file_atomic(workspace_root, relative_path, &final_text)?;
        Ok(format!(
            "Applied {total_replacements} replacement(s) in '{relative_path}'."
        ))
    }

    pub fn move_path(
        workspace_root: &str,
        source_rel: &str,
        dest_rel: &str,
    ) -> Result<String, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let src = Self::resolve(workspace_root, source_rel)?;
        let dst = Self::resolve(workspace_root, dest_rel)?;
        Self::require_unlinked_tree(&root, &src)?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dst)?;
        Ok(format!("Moved '{source_rel}' -> '{dest_rel}'."))
    }

    pub fn delete_path(
        workspace_root: &str,
        relative_path: &str,
    ) -> Result<String, WorkspaceError> {
        let root = Self::root(workspace_root)?;
        let target = Self::resolve(workspace_root, relative_path)?;
        if target == root {
            return Err(WorkspaceError::Denied(
                "Access denied: Cannot delete the workspace root directory.".into(),
            ));
        }
        Self::require_unlinked_tree(&root, &target)?;
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else {
            fs::remove_file(&target)?;
        }
        Ok(format!("Deleted '{relative_path}'."))
    }
}

fn make_absolute_normalized(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(normalize_lexical(&joined))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn real_path_with_missing_suffix(target: &Path) -> Result<PathBuf, WorkspaceError> {
    let mut ancestor = target.to_path_buf();
    let mut missing_parts: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if fs::symlink_metadata(&ancestor).is_ok() {
            let mut real = fs::canonicalize(&ancestor)?;
            for part in missing_parts.into_iter().rev() {
                real.push(part);
            }
            return Ok(normalize_lexical(&real));
        }
        let Some(name) = ancestor.file_name() else {
            return Err(WorkspaceError::Denied(
                "Access denied: No valid workspace ancestor.".into(),
            ));
        };
        missing_parts.push(name.to_os_string());
        if !ancestor.pop() {
            return Err(WorkspaceError::Denied(
                "Access denied: No valid workspace ancestor.".into(),
            ));
        }
    }
}

fn strip_win_verbatim(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

#[cfg(not(feature = "ripgrep"))]
fn collect_unlinked_entries(root: &Path, dir: &Path, max_entries: usize, out: &mut Vec<PathBuf>) {
    if out.len() >= max_entries || !WorkspacePathGuard::can_traverse(root, dir) {
        return;
    }
    let Ok(read_dir) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read_dir.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= max_entries {
            break;
        }
        let p = entry.path();
        if !WorkspacePathGuard::can_traverse(root, &p) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = p.is_dir();
        if is_dir && WorkspacePathGuard::is_ignored_directory(&name) {
            continue;
        }
        out.push(p.clone());
        if is_dir {
            collect_unlinked_entries(root, &p, max_entries, out);
        }
    }
}

#[cfg(not(feature = "ripgrep"))]
fn simple_glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_p: Option<usize> = None;
    let mut star_t = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            while pi + 1 < p.len() && p[pi + 1] == '*' {
                pi += 1;
            }
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            star_t += 1;
            ti = star_t;
            pi = sp + 1;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
