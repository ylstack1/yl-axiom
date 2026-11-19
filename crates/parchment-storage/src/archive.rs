//! Atomic `.parchment` / `.zip` / `.json` chat archive importer and streaming exporter.
//!
//! Enforces every v2 `ChatArchiveExtractor` / `ChatArchiveWriter` security contract:
//! - Bounded entries & decompressed bytes (anti-zip-bomb).
//! - Rejects `..` path traversal, Windows reserved device names, trailing dots, case-insensitive
//!   media collisions, and truncated/corrupt ZIPs.
//! - Atomic stage -> decode (through `with_imported_permissions`) -> no-clobber commit.
//! - Atomic streaming ZIP export (`*.tmp` -> `sync_all` -> rename; deletes partial output on error).

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use parchment_model::{ChatSession, sanitize_media_segment};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::migrate_v2::{resolve_local_media_candidate, strip_redundant_attachment_base64};
use crate::store::is_windows_device_name;
use crate::{StorageError, StorageResult};

const MIB: u64 = 1024 * 1024;
const STAGING_PREFIX: &str = ".chat-import-";

#[derive(Debug, Clone, Copy)]
pub struct ChatArchiveLimits {
    pub max_entries: usize,
    pub max_chat_json_bytes: u64,
    pub max_total_chat_json_bytes: u64,
    pub max_media_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for ChatArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 20_000,
            max_chat_json_bytes: 64 * MIB,
            max_total_chat_json_bytes: 256 * MIB,
            max_media_bytes: 1024 * MIB,
            max_total_bytes: 4096 * MIB,
        }
    }
}

struct StagingGuard(PathBuf);

impl Drop for StagingGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Decodes one or more untrusted `ChatSession`s from raw JSON (single object, `{ "sessions": [...] }`,
/// or JSON array) and strips all local execution authority via `with_imported_permissions()`.
pub fn decode_chat_sessions_from_raw_json(raw_json: &str) -> Vec<ChatSession> {
    let trimmed = raw_json.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let patched = trimmed.replace("\"BYTEDANCE_ARK\"", "\"OPENAI_RESPONSES\"");
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&patched) else {
        return Vec::new();
    };
    match val {
        serde_json::Value::Object(ref map) => {
            if map.contains_key("id")
                && map.contains_key("title")
                && map.contains_key("profileId")
                && !map.contains_key("profiles")
                && !map.contains_key("sessions")
            {
                if let Ok(session) = serde_json::from_value::<ChatSession>(val) {
                    return vec![session.with_imported_permissions()];
                }
            } else if let Some(serde_json::Value::Array(items)) = map.get("sessions") {
                return items
                    .iter()
                    .filter_map(|item| serde_json::from_value::<ChatSession>(item.clone()).ok())
                    .map(|s| s.with_imported_permissions())
                    .collect();
            }
            Vec::new()
        }
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value::<ChatSession>(item).ok())
            .map(|s| s.with_imported_permissions())
            .collect(),
        _ => Vec::new(),
    }
}

/// Imports a chosen file (`.zip` / `.parchment` bundle or raw `.json` chat export) atomically.
pub fn import_chat_file_bytes(
    file_name: &str,
    bytes: &[u8],
    storage_root: &Path,
    limits: ChatArchiveLimits,
) -> StorageResult<Vec<ChatSession>> {
    let lower = file_name.to_ascii_lowercase();
    let is_zip = (bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4B)
        || lower.ends_with(".zip")
        || lower.ends_with(".parchment");
    if is_zip {
        extract_chat_archive(bytes, storage_root, limits)
    } else {
        if bytes.len() as u64 > limits.max_total_chat_json_bytes {
            return Err(StorageError::Archive(
                "This chat file is too large to import.".into(),
            ));
        }
        let raw = std::str::from_utf8(bytes)
            .map_err(|_| StorageError::Archive("The chat JSON file is not valid UTF-8.".into()))?;
        let sessions = decode_chat_sessions_from_raw_json(raw);
        if sessions.is_empty() {
            return Err(StorageError::Archive(
                "The file contains no valid chats.".into(),
            ));
        }
        Ok(sessions)
    }
}

fn extract_chat_archive(
    zip_bytes: &[u8],
    storage_root: &Path,
    limits: ChatArchiveLimits,
) -> StorageResult<Vec<ChatSession>> {
    fs::create_dir_all(storage_root)?;
    let root = fs::canonicalize(storage_root)?;

    // Clean up any abandoned staging folders from earlier interrupted runs.
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(STAGING_PREFIX))
            {
                let _ = fs::remove_dir_all(&p);
            }
        }
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let staging_dir = root.join(format!("{STAGING_PREFIX}{nonce}"));
    fs::create_dir_all(&staging_dir)?;
    let _guard = StagingGuard(staging_dir.clone());

    let mut archive = ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|_| StorageError::Archive("The archive is damaged or incomplete.".into()))?;

    if archive.len() > limits.max_entries {
        return Err(StorageError::Archive(
            "The archive has too many files.".into(),
        ));
    }

    let mut chat_jsons: Vec<String> = Vec::new();
    let mut staged_media: HashMap<String, PathBuf> = HashMap::new();
    let mut claimed_lower: HashSet<String> = HashSet::new();
    let mut total_bytes: u64 = 0;
    let mut total_json_bytes: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| StorageError::Archive("The archive is damaged or incomplete.".into()))?;
        let raw_name = entry
            .name()
            .map_err(|_| StorageError::Archive("The archive is damaged or incomplete.".into()))?
            .replace('\\', "/");
        let path = raw_name.trim_start_matches('/');
        if path.split('/').any(|seg| seg == "..") {
            return Err(StorageError::Archive(
                "The archive contains an unsafe path.".into(),
            ));
        }
        let lower_path = path.to_ascii_lowercase();
        if entry.is_dir() {
            continue;
        }

        if !path.is_empty() && lower_path.ends_with(".json") {
            let mut out = Vec::new();
            let copied = copy_bounded(
                &mut entry,
                &mut out,
                limits.max_chat_json_bytes,
                &mut total_bytes,
                limits.max_total_bytes,
                "A chat in the archive is too large to import.",
            )?;
            total_json_bytes = total_json_bytes.saturating_add(copied);
            if total_json_bytes > limits.max_total_chat_json_bytes {
                return Err(StorageError::Archive(
                    "The archive is too large to import.".into(),
                ));
            }
            let text = String::from_utf8(out).map_err(|_| {
                StorageError::Archive("A chat JSON entry is not valid UTF-8.".into())
            })?;
            if !text.trim().is_empty() {
                chat_jsons.push(text);
            }
        } else if lower_path.contains("/media/") || lower_path.starts_with("media/") {
            let target = media_target(path)?;
            if !claimed_lower.insert(target.to_ascii_lowercase()) {
                return Err(StorageError::Archive(
                    "The archive contains conflicting media files.".into(),
                ));
            }
            let staged_file = staging_dir.join(format!("media_{}", staged_media.len()));
            let mut file = File::create(&staged_file)?;
            copy_bounded(
                &mut entry,
                &mut file,
                limits.max_media_bytes,
                &mut total_bytes,
                limits.max_total_bytes,
                "A media file in the archive is too large to import.",
            )?;
            staged_media.insert(target, staged_file);
        } else {
            let mut sink = io::sink();
            copy_bounded(
                &mut entry,
                &mut sink,
                limits.max_total_bytes,
                &mut total_bytes,
                limits.max_total_bytes,
                "The archive is too large to import.",
            )?;
        }
    }

    if chat_jsons.is_empty() {
        return Err(StorageError::Archive(
            "The archive contains no chats.".into(),
        ));
    }

    // Decode every chat before touching any media destination.
    let mut sessions = Vec::new();
    for raw in &chat_jsons {
        sessions.extend(decode_chat_sessions_from_raw_json(raw));
    }
    if sessions.is_empty() {
        return Err(StorageError::Archive(
            "The archive contains no valid chats.".into(),
        ));
    }

    // Pre-validate every media destination so conflicting existing files reject all-or-nothing.
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (relative, staged) in &staged_media {
        let target = root.join(relative);
        if !target.starts_with(&root) {
            return Err(StorageError::Archive(
                "The archive contains an unsafe path.".into(),
            ));
        }
        if !target.exists() {
            moves.push((target, staged.clone()));
        } else if target.is_file() && same_file_contents(&target, staged)? {
            // Identical media already present; no move needed.
        } else {
            return Err(StorageError::Archive(
                "The archive would replace a different existing media file, so it was not imported."
                    .into(),
            ));
        }
    }

    let mut moved: Vec<PathBuf> = Vec::new();
    for (target, staged) in moves {
        if let Some(parent) = target.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            for p in &moved {
                let _ = fs::remove_file(p);
            }
            return Err(StorageError::Io(e));
        }
        if let Err(e) = fs::rename(&staged, &target) {
            for p in &moved {
                let _ = fs::remove_file(p);
            }
            return Err(StorageError::Io(e));
        }
        moved.push(target);
    }

    Ok(sessions)
}

fn copy_bounded<R: Read, W: Write>(
    src: &mut R,
    dst: &mut W,
    entry_limit: u64,
    total_bytes: &mut u64,
    total_limit: u64,
    too_large_msg: &str,
) -> StorageResult<u64> {
    let mut buf = [0u8; 8192];
    let mut copied: u64 = 0;
    loop {
        let n = src
            .read(&mut buf)
            .map_err(|_| StorageError::Archive("The archive is damaged or incomplete.".into()))?;
        if n == 0 {
            return Ok(copied);
        }
        copied = copied.saturating_add(n as u64);
        *total_bytes = total_bytes.saturating_add(n as u64);
        if copied > entry_limit {
            return Err(StorageError::Archive(too_large_msg.into()));
        }
        if *total_bytes > total_limit {
            return Err(StorageError::Archive(
                "The archive is too large to import.".into(),
            ));
        }
        dst.write_all(&buf[..n])?;
    }
}

fn media_target(path: &str) -> StorageResult<String> {
    let relative = if path.len() >= 6 && path[..6].eq_ignore_ascii_case("chats/") {
        &path[6..]
    } else {
        path
    };
    let parts: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(StorageError::Archive(
            "The archive contains an unsafe path.".into(),
        ));
    }
    let name = safe_segment(parts[parts.len() - 1])?;
    if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("media") {
        let chat_seg = safe_segment(parts[0])?;
        Ok(format!("chats/{chat_seg}/media/{name}"))
    } else {
        Ok(format!("media/{name}"))
    }
}

fn safe_segment(raw: &str) -> StorageResult<String> {
    let seg = sanitize_media_segment(raw);
    if seg.is_empty() || seg == "." || seg.ends_with('.') || is_windows_device_name(&seg) {
        return Err(StorageError::Archive(
            "The archive contains an unsafe path.".into(),
        ));
    }
    Ok(seg)
}

fn same_file_contents(a: &Path, b: &Path) -> io::Result<bool> {
    let ma = fs::metadata(a)?;
    let mb = fs::metadata(b)?;
    if ma.len() != mb.len() {
        return Ok(false);
    }
    let mut fa = File::open(a)?;
    let mut fb = File::open(b)?;
    let mut ba = [0u8; 8192];
    let mut bb = [0u8; 8192];
    loop {
        let na = fa.read(&mut ba)?;
        let nb = fb.read(&mut bb)?;
        if na != nb || ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

/// Streams chats one at a time into a portable `.parchment` / `.zip` archive atomically.
/// Replaces `dest_path` only after the full archive flushes and syncs; deletes `.tmp` on any error.
pub fn export_chats_to_zip(
    dest_path: &Path,
    chats: &[ChatSession],
    storage_root: &Path,
) -> StorageResult<()> {
    if let Some(parent) = dest_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = dest_path.with_extension("zip.tmp");
    let res = write_chats_zip_inner(&tmp_path, chats, storage_root);
    match res {
        Ok(()) => {
            if dest_path.exists() {
                let _ = fs::remove_file(dest_path);
            }
            fs::rename(&tmp_path, dest_path)?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn write_chats_zip_inner(
    tmp_path: &Path,
    chats: &[ChatSession],
    storage_root: &Path,
) -> StorageResult<()> {
    let canon_root = fs::canonicalize(storage_root).ok();
    let file = File::create(tmp_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut seen: HashSet<String> = HashSet::new();

    for raw_chat in chats {
        let mut portable = raw_chat.clone();
        if let Some(root) = &canon_root {
            strip_redundant_attachment_base64(&mut portable, root);
        }
        let safe_id = {
            let s = sanitize_media_segment(&portable.id);
            if s.is_empty() {
                "session".to_string()
            } else {
                s
            }
        };

        let mut media_to_copy: Vec<(String, PathBuf)> = Vec::new();
        for node in &mut portable.messages {
            for variant in &mut node.variants {
                for att in &mut variant.attachments {
                    rewrite_portable_media(
                        &mut att.local_file_path,
                        &safe_id,
                        canon_root.as_deref(),
                        &mut media_to_copy,
                    );
                }
                for media in &mut variant.media_items {
                    rewrite_portable_media(
                        &mut media.local_file_path,
                        &safe_id,
                        canon_root.as_deref(),
                        &mut media_to_copy,
                    );
                }
            }
        }

        let json_entry = format!("chats/{safe_id}/chat.json");
        if seen.insert(json_entry.clone()) {
            let bytes = serde_json::to_vec_pretty(&portable)?;
            zip.start_file(&json_entry, options)
                .map_err(|e| StorageError::Archive(e.to_string()))?;
            zip.write_all(&bytes)?;
        }

        for (archive_path, disk_path) in media_to_copy {
            if seen.insert(archive_path.clone()) {
                zip.start_file(&archive_path, options)
                    .map_err(|e| StorageError::Archive(e.to_string()))?;
                let mut src = File::open(&disk_path)?;
                io::copy(&mut src, &mut zip)?;
            }
        }
    }

    let file = zip
        .finish()
        .map_err(|e| StorageError::Archive(e.to_string()))?;
    file.sync_all()?;
    Ok(())
}

fn rewrite_portable_media(
    local_path: &mut String,
    safe_chat_id: &str,
    canon_root: Option<&Path>,
    out_files: &mut Vec<(String, PathBuf)>,
) {
    if local_path.trim().is_empty() {
        return;
    }
    let normalized = local_path.replace('\\', "/");
    let raw_name = normalized.rsplit('/').next().unwrap_or("");
    let safe_name = sanitize_media_segment(raw_name);
    if safe_name.is_empty() || safe_name == "." || safe_name.ends_with('.') {
        return;
    }
    let portable_rel = format!("chats/{safe_chat_id}/media/{safe_name}");
    if let Some(root) = canon_root
        && let Some(disk_file) = resolve_local_media_candidate(local_path, root)
        && disk_file.is_file()
    {
        out_files.push((portable_rel.clone(), disk_file));
    }
    *local_path = portable_rel;
}
