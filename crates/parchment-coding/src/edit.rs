//! Structured file editing tools for Axiom Coding.
//!
//! Provides read-only and mutating file operations with path-guard confinement,
//! diff preview, and approval integration. All mutating operations require
//! explicit approval via `ToolApprovalBroker` before execution.

use std::path::{Component, Path, PathBuf};

use parchment_harness::path_guard::WorkspaceError;

use crate::patch::{FileDiff, PatchReview, PatchStatus, PatchStore};

/// Result of a structured file edit.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditResult {
    pub patch_id: String,
    pub path: String,
    pub diff: FileDiff,
    pub bytes_written: u64,
    pub applied: bool,
}

/// A structured file edit request.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEdit {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
    pub require_exact_match: bool,
}

/// Read-only file read with bounds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReadResult {
    pub path: String,
    pub content: String,
    pub lines_read: usize,
    pub total_lines: usize,
    pub truncated: bool,
}

/// File editing engine with path confinement and diff preview.
pub struct FileEditor {
    store: PatchStore,
    workspace_root: PathBuf,
}

impl FileEditor {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            store: PatchStore::new(),
            workspace_root: workspace_root.into(),
        }
    }

    /// Read a file within the workspace with bounds checking.
    pub fn read_file(&self, path: &str) -> Result<FileReadResult, WorkspaceError> {
        let full_path = self.resolve_path(path)?;
        let content = std::fs::read_to_string(&full_path)?;
        let total_lines = content.lines().count();
        let truncated = content.chars().nth(250_000).is_some();
        let content_out: String = if truncated {
            content.chars().take(250_000).collect()
        } else {
            content
        };
        Ok(FileReadResult {
            path: path.to_string(),
            lines_read: content_out.lines().count(),
            content: content_out,
            total_lines,
            truncated,
        })
    }

    /// Compute a diff preview for a proposed edit without applying it.
    pub fn preview_edit(
        &mut self,
        task_id: &str,
        edit: &FileEdit,
    ) -> Result<PatchReview, WorkspaceError> {
        let full_path = self.resolve_path(&edit.path)?;
        let existing = std::fs::read_to_string(&full_path)?;

        if edit.require_exact_match && !existing.contains(&edit.old_text) {
            return Err(WorkspaceError::Invalid("old_text not found in file".into()));
        }

        let new_content = if edit.require_exact_match {
            existing.replace(&edit.old_text, &edit.new_text)
        } else {
            edit.new_text.clone()
        };

        let diff = FileDiff::new(String::new(), edit.path.clone(), &existing, &new_content);
        let patch_id = self.store.register(task_id.to_string(), diff);
        self.store
            .get(&patch_id)
            .cloned()
            .ok_or_else(|| WorkspaceError::Invalid(format!("patch {patch_id} not found")))
    }

    /// Apply a previously approved patch.
    pub fn apply_patch(&mut self, patch_id: &str) -> Result<EditResult, WorkspaceError> {
        let review = self
            .store
            .get(patch_id)
            .ok_or_else(|| WorkspaceError::Invalid(format!("patch {patch_id} not found")))?
            .clone();

        if review.status != PatchStatus::Approved {
            return Err(WorkspaceError::Invalid(
                "patch must be approved before application".into(),
            ));
        }

        let full_path = self.resolve_path(&review.diff.path)?;
        let existing = std::fs::read_to_string(&full_path)?;

        // Apply the diff: reconstruct new content from diff lines
        let new_content = self.apply_diff_lines(&existing, &review.diff.lines)?;
        let bytes_written = new_content.len() as u64;

        std::fs::write(&full_path, &new_content)?;

        self.store
            .applied(patch_id)
            .map_err(|e| WorkspaceError::Invalid(e.to_string()))?;

        Ok(EditResult {
            patch_id: patch_id.to_string(),
            path: review.diff.path.clone(),
            diff: review.diff,
            bytes_written,
            applied: true,
        })
    }

    /// Reconstruct new file content from diff lines.
    fn apply_diff_lines(
        &self,
        old_text: &str,
        diff_lines: &[parchment_prose::diff::DiffLine],
    ) -> Result<String, WorkspaceError> {
        let old_lines: Vec<&str> = old_text.lines().collect();
        let mut new_lines: Vec<String> = Vec::new();
        let mut old_idx: usize = 0;

        for diff_line in diff_lines {
            match diff_line.kind {
                parchment_prose::diff::DiffLineKind::Context => {
                    if let Some(ln) = diff_line.old_line {
                        let idx = ln.saturating_sub(1);
                        if idx < old_lines.len() {
                            new_lines.push(old_lines[idx].to_string());
                            old_idx = idx + 1;
                        } else {
                            new_lines.push(diff_line.text.clone());
                        }
                    } else {
                        new_lines.push(diff_line.text.clone());
                    }
                }
                parchment_prose::diff::DiffLineKind::Added => {
                    new_lines.push(diff_line.text.clone());
                }
                parchment_prose::diff::DiffLineKind::Removed => {
                    if let Some(ln) = diff_line.old_line {
                        old_idx = ln.saturating_sub(1) + 1;
                    }
                }
                parchment_prose::diff::DiffLineKind::Collapsed => {
                    // Collapsed lines are unchanged; skip them
                    if old_idx < old_lines.len() {
                        new_lines.push(old_lines[old_idx].to_string());
                        old_idx += 1;
                    }
                }
            }
        }

        // Append any remaining old lines not covered by diff
        while old_idx < old_lines.len() {
            new_lines.push(old_lines[old_idx].to_string());
            old_idx += 1;
        }

        Ok(new_lines.join("\n"))
    }

    /// Resolve a path within the workspace root with confinement.
    fn resolve_path(&self, path: &str) -> Result<PathBuf, WorkspaceError> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::Invalid("empty path".into()));
        }
        let root_abs = if self.workspace_root.is_absolute() {
            self.workspace_root.clone()
        } else {
            std::env::current_dir()?.join(&self.workspace_root)
        };
        let root_norm = normalize_lexical(&root_abs);
        let root_canon = std::fs::canonicalize(&root_norm).unwrap_or(root_norm);

        let p = Path::new(trimmed);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            root_canon.join(p)
        };
        let lex = normalize_lexical(&joined);
        let canon = std::fs::canonicalize(&lex).unwrap_or(lex);
        if !canon.starts_with(&root_canon) {
            return Err(WorkspaceError::Denied(format!(
                "path {path} escapes workspace root"
            )));
        }
        Ok(canon)
    }

    /// List all patches for a task.
    pub fn list_patches(&self, task_id: &str) -> Vec<&PatchReview> {
        self.store.list_for_task(task_id)
    }

    /// Get pending patches awaiting review.
    pub fn pending_patches(&self) -> Vec<&PatchReview> {
        self.store.pending()
    }
}

impl Default for FileEditor {
    fn default() -> Self {
        Self {
            store: PatchStore::new(),
            workspace_root: std::env::temp_dir(),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_result_contains_diff_summary() {
        let store = PatchStore::new();
        let _editor = FileEditor::new("/tmp");
        let diff = FileDiff::new("p-1".into(), "src/lib.rs".into(), "a\nb\n", "a\nc\n");
        let _result = EditResult {
            patch_id: diff.patch_id.clone(),
            path: "src/lib.rs".into(),
            diff,
            bytes_written: 10,
            applied: true,
        };
        // Just verify types compile
        let _store_empty = store.pending();
        assert!(_store_empty.is_empty());
    }

    #[test]
    fn path_confinement_rejects_escape() {
        let editor = FileEditor::new("/tmp/workspace");
        let result = editor.resolve_path("../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn path_confinement_allows_inside() {
        let editor = FileEditor::new("/tmp/workspace");
        let result = editor.resolve_path("src/lib.rs");
        assert!(result.is_ok());
    }

    #[test]
    fn diff_line_counts_match_edits() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 2 modified\nline 3\n";
        let diff = FileDiff::new("p-6".into(), "x.rs".into(), old, new);
        assert!(diff.removed_lines >= 1);
        assert!(diff.added_lines >= 1);
    }

    #[test]
    fn patch_store_pending_list() {
        let mut store = PatchStore::new();
        let diff1 = FileDiff::new("p-7".into(), "a.rs".into(), "old\n", "new\n");
        let diff2 = FileDiff::new("p-8".into(), "b.rs".into(), "old\n", "new\n");
        store.register("t-1".into(), diff1);
        store.register("t-1".into(), diff2);
        let pending = store.pending();
        assert_eq!(pending.len(), 2);
    }
}
