//! Structured patch workflow and diff review for Axiom Coding.
//!
//! Uses the existing `parchment-prose` diff engine (`compute_unified_line_diff`,
//! `parse_diff_fence`) to produce and review unified diffs before file edits are
//! applied. All operations are read-only until explicitly committed.

use parchment_prose::diff::{DiffLine, DiffLineKind, compute_unified_line_diff};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Opaque patch handle.
pub type PatchId = String;

/// A computed diff between old and new file content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub patch_id: PatchId,
    pub path: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub lines: Vec<DiffLine>,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub context_lines: usize,
    pub collapsed_lines: usize,
    pub is_binary: bool,
    pub truncated: bool,
}

impl FileDiff {
    pub fn new(patch_id: PatchId, path: String, old_text: &str, new_text: &str) -> Self {
        let lines = compute_unified_line_diff(old_text, new_text);
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut context = 0usize;
        let mut collapsed = 0usize;
        for line in &lines {
            match line.kind {
                DiffLineKind::Added => added += 1,
                DiffLineKind::Removed => removed += 1,
                DiffLineKind::Context => context += 1,
                DiffLineKind::Collapsed => collapsed += 1,
            }
        }
        let truncated = lines.iter().any(|l| l.text.contains("… unchanged lines …"));
        Self {
            patch_id,
            path,
            old_path: None,
            new_path: None,
            lines,
            added_lines: added,
            removed_lines: removed,
            context_lines: context,
            collapsed_lines: collapsed,
            is_binary: false,
            truncated,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} +{} / -{} / ctx{} / collapsed{}",
            self.path,
            self.added_lines,
            self.removed_lines,
            self.context_lines,
            self.collapsed_lines,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.added_lines == 0 && self.removed_lines == 0
    }
}

/// Errors from patch operations.
#[derive(Debug, Error)]
pub enum PatchError {
    #[error("patch not found: {0}")]
    PatchNotFound(PatchId),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("diff computation failed: {0}")]
    DiffFailed(String),
    #[error("patch application failed: {0}")]
    ApplyFailed(String),
    #[error("invalid patch format: {0}")]
    InvalidFormat(String),
}

/// Patch application status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatchStatus {
    Pending,
    Reviewed,
    Approved,
    Applied,
    Rejected,
    Failed,
}

/// A patch ready for review before application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchReview {
    pub patch_id: PatchId,
    pub task_id: String,
    pub diff: FileDiff,
    pub status: PatchStatus,
    pub created_at_ms: i64,
    pub reviewed_at_ms: Option<i64>,
    pub applied_at_ms: Option<i64>,
    pub reviewer_notes: Option<String>,
}

impl PatchReview {
    pub fn new(patch_id: PatchId, task_id: String, diff: FileDiff) -> Self {
        Self {
            patch_id,
            task_id,
            diff,
            status: PatchStatus::Pending,
            created_at_ms: crate::now_ms(),
            reviewed_at_ms: None,
            applied_at_ms: None,
            reviewer_notes: None,
        }
    }
}

/// In-memory patch store for the coding coordinator.
#[derive(Debug, Clone, Default)]
pub struct PatchStore {
    patches: std::collections::BTreeMap<PatchId, PatchReview>,
}

impl PatchStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new computed diff for review.
    pub fn register(&mut self, task_id: String, mut diff: FileDiff) -> PatchId {
        let id = if diff.patch_id.is_empty() || self.patches.contains_key(&diff.patch_id) {
            let mut seq = self.patches.len() + 1;
            let mut candidate = format!("patch-{}-{seq}", crate::now_ms());
            while self.patches.contains_key(&candidate) {
                seq += 1;
                candidate = format!("patch-{}-{seq}", crate::now_ms());
            }
            candidate
        } else {
            diff.patch_id.clone()
        };
        diff.patch_id = id.clone();
        let review = PatchReview::new(id.clone(), task_id, diff);
        self.patches.insert(id.clone(), review);
        id
    }

    /// Get a patch review by ID.
    pub fn get(&self, patch_id: &str) -> Option<&PatchReview> {
        self.patches.get(patch_id)
    }

    /// Mark a patch as reviewed.
    pub fn review(&mut self, patch_id: &str, notes: Option<String>) -> Result<(), PatchError> {
        let review = self
            .patches
            .get_mut(patch_id)
            .ok_or_else(|| PatchError::PatchNotFound(patch_id.to_string()))?;
        review.status = PatchStatus::Reviewed;
        review.reviewed_at_ms = Some(crate::now_ms());
        review.reviewer_notes = notes;
        Ok(())
    }

    /// Mark a patch as approved for application.
    pub fn approve(&mut self, patch_id: &str) -> Result<(), PatchError> {
        let review = self
            .patches
            .get_mut(patch_id)
            .ok_or_else(|| PatchError::PatchNotFound(patch_id.to_string()))?;
        if review.status != PatchStatus::Reviewed {
            return Err(PatchError::InvalidFormat(
                "patch must be reviewed before approval".into(),
            ));
        }
        review.status = PatchStatus::Approved;
        Ok(())
    }

    /// Mark a patch as applied.
    pub fn applied(&mut self, patch_id: &str) -> Result<(), PatchError> {
        let review = self
            .patches
            .get_mut(patch_id)
            .ok_or_else(|| PatchError::PatchNotFound(patch_id.to_string()))?;
        review.status = PatchStatus::Applied;
        review.applied_at_ms = Some(crate::now_ms());
        Ok(())
    }

    /// Mark a patch as rejected.
    pub fn reject(&mut self, patch_id: &str, notes: Option<String>) -> Result<(), PatchError> {
        let review = self
            .patches
            .get_mut(patch_id)
            .ok_or_else(|| PatchError::PatchNotFound(patch_id.to_string()))?;
        review.status = PatchStatus::Rejected;
        review.reviewed_at_ms = Some(crate::now_ms());
        review.reviewer_notes = notes;
        Ok(())
    }

    /// List all patches for a task.
    pub fn list_for_task(&self, task_id: &str) -> Vec<&PatchReview> {
        self.patches
            .values()
            .filter(|r| r.task_id == task_id)
            .collect()
    }

    /// List all pending patches awaiting review.
    pub fn pending(&self) -> Vec<&PatchReview> {
        self.patches
            .values()
            .filter(|r| r.status == PatchStatus::Pending)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_computes_added_and_removed_lines() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 2 modified\nline 3\nline 4\n";
        let diff = FileDiff::new("p-1".into(), "src/lib.rs".into(), old, new);
        assert!(diff.added_lines > 0);
        assert!(diff.removed_lines > 0);
        assert!(!diff.is_empty());
    }

    #[test]
    fn diff_empty_when_content_unchanged() {
        let text = "same content\n";
        let diff = FileDiff::new("p-2".into(), "src/lib.rs".into(), text, text);
        assert!(diff.is_empty());
    }

    #[test]
    fn patch_review_lifecycle() {
        let mut store = PatchStore::new();
        let diff = FileDiff::new("p-3".into(), "src/main.rs".into(), "old\n", "new\n");
        let id = store.register("t-1".into(), diff);

        assert_eq!(store.get(&id).unwrap().status, PatchStatus::Pending);
        assert!(store.review(&id, Some("looks good".into())).is_ok());
        assert_eq!(store.get(&id).unwrap().status, PatchStatus::Reviewed);
        assert!(store.approve(&id).is_ok());
        assert_eq!(store.get(&id).unwrap().status, PatchStatus::Approved);
        assert!(store.applied(&id).is_ok());
        assert_eq!(store.get(&id).unwrap().status, PatchStatus::Applied);
    }

    #[test]
    fn approve_without_review_fails() {
        let mut store = PatchStore::new();
        let diff = FileDiff::new("p-4".into(), "x.rs".into(), "a\n", "b\n");
        let id = store.register("t-2".into(), diff);
        assert!(store.approve(&id).is_err());
    }

    #[test]
    fn summary_includes_path_and_counts() {
        let diff = FileDiff::new("p-5".into(), "src/lib.rs".into(), "a\nb\n", "a\nc\nd\n");
        let summary = diff.summary();
        assert!(summary.contains("src/lib.rs"));
        assert!(summary.contains("+"));
        assert!(summary.contains("-"));
    }
}
