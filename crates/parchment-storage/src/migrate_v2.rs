//! Non-destructive, idempotent migrator from YL Axiom v2 flat JSON layout to v3 SQLite WAL.
//!
//! Reads `settings.json` (or legacy `parchment_state.json`), `chats/<id>/chat.json`,
//! crash-surviving `chats/<id>/live.json` sidecars (verifying `(chatLength, chatModifiedMs)` stamp),
//! and legacy `chats/<id>.json` files. Never deletes or alters any v2 file on disk.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use parchment_model::{AppStateSnapshot, ChatSession, MessageVariant};
use serde::{Deserialize, Serialize};

use crate::StorageResult;
use crate::store::{ParchmentStore, validate_session_id};

const KEY_V2_MIGRATION_MARKER: &str = "v2_migration_v1";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V2MigrationReport {
    pub already_migrated: bool,
    pub settings_migrated: bool,
    pub chats_migrated: usize,
    pub live_sidecars_folded: usize,
    pub chats_skipped: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveSidecarEnvelope {
    chat_length: u64,
    chat_modified_ms: i64,
    turn: LiveTurnPayload,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveTurnPayload {
    node_id: String,
    variant: MessageVariant,
}

/// Migrates a YL Axiom v2 data directory (`%LOCALAPPDATA%\YL-Axiom` or portable folder) into
/// `store` without modifying or deleting any legacy files.
pub fn migrate_v2_directory(
    store: &mut ParchmentStore,
    v2_root: &Path,
    force: bool,
) -> StorageResult<V2MigrationReport> {
    if !force && store.get_kv(KEY_V2_MIGRATION_MARKER)?.is_some() {
        return Ok(V2MigrationReport {
            already_migrated: true,
            ..V2MigrationReport::default()
        });
    }

    let mut report = V2MigrationReport::default();

    // 1. Migrate settings.json (fallback to legacy parchment_state.json).
    let settings_candidates = [
        v2_root.join("settings.json"),
        v2_root.join("parchment_state.json"),
    ];
    let mut legacy_embedded_sessions: Vec<ChatSession> = Vec::new();
    for path in &settings_candidates {
        if path.is_file()
            && let Ok(raw) = fs::read_to_string(path)
        {
            let patched = raw.replace("\"BYTEDANCE_ARK\"", "\"OPENAI_RESPONSES\"");
            if let Ok(snap) = serde_json::from_str::<AppStateSnapshot>(&patched) {
                legacy_embedded_sessions = snap.sessions.clone();
                store.save_settings(&snap)?;
                store.save_desktop_window(&snap.desktop_window)?;
                report.settings_migrated = true;
                break;
            }
        }
    }

    // 2. Migrate per-chat directories (`chats/<id>/chat.json` + optional `live.json`) and flat JSONs.
    let mut imported_ids: HashSet<String> = HashSet::new();
    let chats_dir = v2_root.join("chats");
    if chats_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&chats_dir)?.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                let folder_name = entry.file_name().to_string_lossy().into_owned();
                if validate_session_id(&folder_name).is_err() {
                    report.chats_skipped += 1;
                    continue;
                }
                let chat_file = path.join("chat.json");
                if !chat_file.is_file() {
                    continue;
                }
                match read_and_fold_v2_chat(&chat_file, &path.join("live.json"), v2_root) {
                    Some((session, folded)) => {
                        if validate_session_id(&session.id).is_ok() {
                            imported_ids.insert(session.id.clone());
                            store.upsert_chat_session(&session)?;
                            report.chats_migrated += 1;
                            if folded {
                                report.live_sidecars_folded += 1;
                            }
                        } else {
                            report.chats_skipped += 1;
                        }
                    }
                    None => report.chats_skipped += 1,
                }
            } else if path.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if imported_ids.contains(&stem) || validate_session_id(&stem).is_err() {
                    continue;
                }
                if let Some((session, _)) =
                    read_and_fold_v2_chat(&path, &path.with_extension("nope"), v2_root)
                    && !imported_ids.contains(&session.id)
                    && validate_session_id(&session.id).is_ok()
                {
                    imported_ids.insert(session.id.clone());
                    store.upsert_chat_session(&session)?;
                    report.chats_migrated += 1;
                }
            }
        }
    }

    // 3. Migrate any legacy embedded sessions from parchment_state.json that weren't in chats/.
    for mut session in legacy_embedded_sessions {
        if !imported_ids.contains(&session.id) && validate_session_id(&session.id).is_ok() {
            strip_redundant_attachment_base64(&mut session, v2_root);
            imported_ids.insert(session.id.clone());
            store.upsert_chat_session(&session)?;
            report.chats_migrated += 1;
        }
    }

    let marker = serde_json::to_string(&report)?;
    store.set_kv(KEY_V2_MIGRATION_MARKER, &marker)?;
    Ok(report)
}

fn read_and_fold_v2_chat(
    chat_file: &Path,
    live_file: &Path,
    v2_root: &Path,
) -> Option<(ChatSession, bool)> {
    let meta = fs::metadata(chat_file).ok()?;
    let chat_len = meta.len();
    let chat_mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let raw = fs::read_to_string(chat_file).ok()?;
    let patched = raw.replace("\"BYTEDANCE_ARK\"", "\"OPENAI_RESPONSES\"");
    let mut session: ChatSession = serde_json::from_str(&patched).ok()?;

    let mut folded = false;
    if live_file.is_file()
        && let Ok(sidecar_raw) = fs::read_to_string(live_file)
        && let Ok(sidecar) = serde_json::from_str::<LiveSidecarEnvelope>(&sidecar_raw)
        && sidecar.chat_length == chat_len
        && sidecar.chat_modified_ms == chat_mtime_ms
    {
        for node in &mut session.messages {
            if node.id == sidecar.turn.node_id {
                for existing in &mut node.variants {
                    if existing.id == sidecar.turn.variant.id {
                        *existing = sidecar.turn.variant.clone();
                        folded = true;
                        break;
                    }
                }
                if folded {
                    break;
                }
            }
        }
    }

    strip_redundant_attachment_base64(&mut session, v2_root);
    Some((session, folded))
}

/// Strips attachment `base64_data` ONLY when its local file inside `storage_root` is readable and non-empty.
pub fn strip_redundant_attachment_base64(session: &mut ChatSession, storage_root: &Path) {
    let Ok(canon_root) = fs::canonicalize(storage_root) else {
        return;
    };
    for node in &mut session.messages {
        for variant in &mut node.variants {
            for att in &mut variant.attachments {
                if att.base64_data.is_empty() || att.local_file_path.trim().is_empty() {
                    continue;
                }
                let candidate = resolve_local_media_candidate(&att.local_file_path, &canon_root);
                if let Some(file) = candidate
                    && let Ok(meta) = fs::metadata(&file)
                    && meta.is_file()
                    && meta.len() > 0
                {
                    att.base64_data.clear();
                }
            }
        }
    }
}

pub(crate) fn resolve_local_media_candidate(
    raw_path: &str,
    canon_root: &Path,
) -> Option<std::path::PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        canon_root.join(trimmed)
    };
    let canon = fs::canonicalize(&joined).ok()?;
    if canon.starts_with(canon_root) {
        Some(canon)
    } else {
        None
    }
}

#[allow(dead_code)]
fn _keep_time_import() -> SystemTime {
    SystemTime::now()
}
