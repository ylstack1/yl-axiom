use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use parchment_model::{
    AgentPermissionOverrides, ApiUsageRecord, AppStateSnapshot, CharacterOrScenarioCard,
    ChatSession, ChatStub, ContextFragment, DesktopWindowConfig, GeneratedMediaItem,
    GoalTrackerState, MessageNode, MessageRole, MessageVariant, PinnedMessageStub,
    PromptAttachment, ProviderProtocol, TokenMetrics, ToolCallRecord, ViewMode, WebSourceCitation,
    WorkspaceMode,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::schema::{INLINE_TOOL_RESULT_LIMIT_BYTES, configure_and_migrate};
use crate::{StorageError, StorageResult};

mod rows;

pub(crate) use rows::is_windows_device_name;
use rows::*;
pub use rows::{escape_fts5_query, validate_session_id};

const KEY_APP_SETTINGS: &str = "app_settings";
const KEY_DESKTOP_WINDOW: &str = "desktop_window";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub chat_id: String,
    pub chat_title: String,
    pub message_id: String,
    pub snippet: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ChatMetaJson {
    user_name: String,
    system_prompt: String,
    active_card: Option<CharacterOrScenarioCard>,
    story_summary: String,
    fragments: Vec<ContextFragment>,
    authors_note: String,
    authors_note_depth: i32,
    permission_overrides: Option<AgentPermissionOverrides>,
    utility_usage_records: Vec<ApiUsageRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatMetaRef<'a> {
    user_name: &'a str,
    system_prompt: &'a str,
    active_card: &'a Option<CharacterOrScenarioCard>,
    story_summary: &'a str,
    fragments: &'a [ContextFragment],
    authors_note: &'a str,
    authors_note_depth: i32,
    permission_overrides: &'a Option<AgentPermissionOverrides>,
    utility_usage_records: &'a [ApiUsageRecord],
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct VariantMetaJson {
    goal_tracker: Option<GoalTrackerState>,
    media_items: Vec<GeneratedMediaItem>,
    attachments: Vec<PromptAttachment>,
    web_sources: Vec<WebSourceCitation>,
    token_metrics: TokenMetrics,
    api_usage_records: Vec<ApiUsageRecord>,
    legacy_usage_owner_session_id: String,
    raw_request_preview: String,
    raw_response_preview: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VariantMetaRef<'a> {
    goal_tracker: &'a Option<GoalTrackerState>,
    media_items: &'a [GeneratedMediaItem],
    attachments: &'a [PromptAttachment],
    web_sources: &'a [WebSourceCitation],
    token_metrics: &'a TokenMetrics,
    api_usage_records: &'a [ApiUsageRecord],
    legacy_usage_owner_session_id: &'a str,
    raw_request_preview: &'a str,
    raw_response_preview: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStateSettingsRef<'a> {
    profiles: &'a [parchment_model::ProviderProfile],
    active_profile_id: &'a str,
    assistants: &'a [parchment_model::AssistantAgent],
    active_assistant_id: &'a str,
    default_models: &'a parchment_model::DefaultModelConfig,
    preferences: &'a parchment_model::PreferencesConfig,
    desktop_window: &'a DesktopWindowConfig,
    sessions: &'a [ChatSession],
    active_session_id: &'a str,
    open_tab_session_ids: &'a [String],
    system_prompt_presets: &'a [parchment_model::SystemPromptPreset],
    saved_cards: &'a [CharacterOrScenarioCard],
    global_world_book: &'a [ContextFragment],
    theme: &'a parchment_model::ThemeConfig,
    ota_github_repo: &'a str,
    auto_check_ota_on_startup: bool,
    provider_storage_reset: bool,
    design_defaults_applied: bool,
}

pub struct ParchmentStore {
    conn: Connection,
}

impl ParchmentStore {
    pub fn open(db_path: &Path) -> StorageResult<Self> {
        if let Some(parent) = db_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(db_path)?;
        configure_and_migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory()?;
        configure_and_migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn get_kv(&self, key: &str) -> StorageResult<Option<String>> {
        let val = self
            .conn
            .query_row(
                "SELECT value_json FROM kv_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(val)
    }

    pub fn set_kv(&mut self, key: &str, value_json: &str) -> StorageResult<()> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO kv_settings (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at_ms = excluded.updated_at_ms",
            params![key, value_json, now],
        )?;
        Ok(())
    }

    /// Upserts a coding task, replacing any existing row with the same id.
    /// `task_json` is the full serialized task as a JSON string.
    pub fn upsert_coding_task(
        &mut self,
        id: &str,
        title: &str,
        description: &str,
        state: &str,
        workspace_path: &str,
        repo_url: &str,
        created_at_ms: i64,
        updated_at_ms: i64,
        completed_at_ms: Option<i64>,
        task_json: &str,
        owner_session_id: &str,
        owner_run_id: Option<&str>,
    ) -> StorageResult<()> {
        self.conn.execute(
            "INSERT INTO coding_tasks (id, title, description, state, workspace_path, repo_url,
                 created_at_ms, updated_at_ms, completed_at_ms, task_json,
                 owner_session_id, owner_run_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title, description = excluded.description,
                 state = excluded.state, workspace_path = excluded.workspace_path,
                 repo_url = excluded.repo_url, updated_at_ms = excluded.updated_at_ms,
                 completed_at_ms = excluded.completed_at_ms, task_json = excluded.task_json,
                 owner_session_id = excluded.owner_session_id,
                 owner_run_id = excluded.owner_run_id",
            params![
                id, title, description, state,
                workspace_path, repo_url,
                created_at_ms, updated_at_ms,
                completed_at_ms,
                task_json,
                owner_session_id, owner_run_id
            ],
        )?;
        Ok(())
    }

    /// Loads a coding task's JSON by id, or None if not found.
    pub fn load_coding_task_json(&self, task_id: &str) -> StorageResult<Option<String>> {
        let row: Option<String> = self.conn.query_row(
            "SELECT task_json FROM coding_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        ).optional()?;
        Ok(row)
    }

    /// Deletes a coding task and all related subtasks, criteria, checkpoints, artifacts, hooks.
    pub fn delete_coding_task(&mut self, task_id: &str) -> StorageResult<bool> {
        let changed = self.conn.execute(
            "DELETE FROM coding_tasks WHERE id = ?1",
            params![task_id],
        )?;
        Ok(changed > 0)
    }

    /// Lists all coding task ids, optionally filtered by workspace path.
    pub fn list_coding_task_ids(&self, workspace_path: Option<&str>) -> StorageResult<Vec<String>> {
        let mut ids: Vec<String> = Vec::new();
        let mut stmt = match workspace_path {
            Some(ws) => self.conn.prepare(
                "SELECT id FROM coding_tasks WHERE workspace_path = ?1 ORDER BY updated_at_ms DESC",
            )?,
            None => self.conn.prepare(
                "SELECT id FROM coding_tasks ORDER BY updated_at_ms DESC",
            )?,
        };
        let rows = match workspace_path {
            Some(ws) => stmt.query_map(params![ws], |row| row.get(0))?,
            None => stmt.query_map([], |row| row.get(0))?,
        };
        for r in rows {
            ids.push(r?);
        }
        Ok(ids)
    }

    /// Loads settings (profiles, assistants, preferences, theme, worldbook, presets, cards).
    /// Sessions live in the normalized `chats` table, so `snapshot.sessions` is always empty here.
    pub fn load_settings(&self) -> StorageResult<AppStateSnapshot> {
        let Some(raw) = self.get_kv(KEY_APP_SETTINGS)? else {
            return Ok(AppStateSnapshot::default());
        };
        let mut snap: AppStateSnapshot = serde_json::from_str(&raw)?;
        snap.sessions.clear();
        Ok(snap)
    }

    /// Persists settings cleanly without cloning or duplicating chat histories into JSON.
    pub fn save_settings(&mut self, snapshot: &AppStateSnapshot) -> StorageResult<()> {
        let json = if snapshot.sessions.is_empty() {
            serde_json::to_string(snapshot)?
        } else {
            let view = AppStateSettingsRef {
                profiles: &snapshot.profiles,
                active_profile_id: &snapshot.active_profile_id,
                assistants: &snapshot.assistants,
                active_assistant_id: &snapshot.active_assistant_id,
                default_models: &snapshot.default_models,
                preferences: &snapshot.preferences,
                desktop_window: &snapshot.desktop_window,
                sessions: &[],
                active_session_id: &snapshot.active_session_id,
                open_tab_session_ids: &snapshot.open_tab_session_ids,
                system_prompt_presets: &snapshot.system_prompt_presets,
                saved_cards: &snapshot.saved_cards,
                global_world_book: &snapshot.global_world_book,
                theme: &snapshot.theme,
                ota_github_repo: &snapshot.ota_github_repo,
                auto_check_ota_on_startup: snapshot.auto_check_ota_on_startup,
                provider_storage_reset: snapshot.provider_storage_reset,
                design_defaults_applied: snapshot.design_defaults_applied,
            };
            serde_json::to_string(&view)?
        };
        self.set_kv(KEY_APP_SETTINGS, &json)
    }

    pub fn load_desktop_window(&self) -> StorageResult<DesktopWindowConfig> {
        match self.get_kv(KEY_DESKTOP_WINDOW)? {
            Some(raw) => Ok(serde_json::from_str(&raw)?),
            None => Ok(DesktopWindowConfig::default()),
        }
    }

    pub fn save_desktop_window(&mut self, cfg: &DesktopWindowConfig) -> StorageResult<()> {
        let json = serde_json::to_string(cfg)?;
        self.set_kv(KEY_DESKTOP_WINDOW, &json)
    }

    /// Instant sidebar query — reads zero message bodies.
    pub fn list_chat_stubs(&self) -> StorageResult<Vec<ChatStub>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, created_at_ms, updated_at_ms, profile_id, assistant_id,
                    is_pinned, message_count, pinned_message_count
             FROM chats
             ORDER BY is_pinned DESC, updated_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ChatStub {
                id: row.get(0)?,
                title: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                profile_id: row.get(4)?,
                assistant_id: row.get(5)?,
                is_pinned: row.get::<_, i64>(6)? != 0,
                message_count: row.get::<_, i64>(7)?.max(0) as u32,
                pinned_message_count: row.get::<_, i64>(8)?.max(0) as u32,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Instant Pinned-tab query using the partial `idx_messages_pinned` index.
    pub fn list_pinned_messages(
        &self,
        chat_id: Option<&str>,
        limit: u32,
    ) -> StorageResult<Vec<PinnedMessageStub>> {
        let mut out = Vec::new();
        if let Some(cid) = chat_id {
            let mut stmt = self.conn.prepare_cached(
                "SELECT m.chat_id, c.title, m.id, m.role, m.active_content, m.created_at_ms
                 FROM messages m
                 JOIN chats c ON c.id = m.chat_id
                 WHERE m.pinned = 1 AND m.chat_id = ?1
                 ORDER BY m.created_at_ms DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![cid, limit], map_pinned_stub)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = self.conn.prepare_cached(
                "SELECT m.chat_id, c.title, m.id, m.role, m.active_content, m.created_at_ms
                 FROM messages m
                 JOIN chats c ON c.id = m.chat_id
                 WHERE m.pinned = 1
                 ORDER BY m.created_at_ms DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], map_pinned_stub)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    /// Atomically inserts or replaces a full `ChatSession` (used by migration, import, and full sync).
    pub fn upsert_chat_session(&mut self, session: &ChatSession) -> StorageResult<()> {
        validate_session_id(&session.id)?;
        let tx = self.conn.transaction()?;
        upsert_chat_row(&tx, session)?;

        // Replace messages/variants/tools/usage cleanly inside the same transaction.
        tx.execute(
            "DELETE FROM messages WHERE chat_id = ?1",
            params![&session.id],
        )?;
        tx.execute(
            "DELETE FROM usage_records WHERE chat_id = ?1",
            params![&session.id],
        )?;

        for (seq, node) in session.messages.iter().enumerate() {
            insert_message_node(&tx, &session.id, seq as i64, node)?;
        }
        for rec in &session.utility_usage_records {
            insert_usage_record(&tx, &session.id, "", "", rec, session.updated_at)?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Updates chat metadata/draft/permissions without touching message rows.
    pub fn upsert_chat_metadata(&mut self, session: &ChatSession) -> StorageResult<()> {
        validate_session_id(&session.id)?;
        let tx = self.conn.transaction()?;
        let existing_counts: Option<(i64, i64)> = tx
            .query_row(
                "SELECT message_count, pinned_message_count FROM chats WHERE id = ?1",
                params![&session.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        upsert_chat_row_with_counts(
            &tx,
            session,
            existing_counts
                .map(|c| c.0)
                .unwrap_or(session.messages.len() as i64),
            existing_counts
                .map(|c| c.1)
                .unwrap_or_else(|| session.messages.iter().filter(|m| m.pinned).count() as i64),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Appends one new turn to a chat in a single O(1) WAL transaction.
    pub fn append_message_node(
        &mut self,
        chat_id: &str,
        node: &MessageNode,
        updated_at_ms: i64,
    ) -> StorageResult<()> {
        validate_session_id(chat_id)?;
        let tx = self.conn.transaction()?;
        let next_seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), -1) + 1 FROM messages WHERE chat_id = ?1",
            params![chat_id],
            |r| r.get(0),
        )?;
        insert_message_node(&tx, chat_id, next_seq, node)?;
        let pin_inc: i64 = if node.pinned { 1 } else { 0 };
        tx.execute(
            "UPDATE chats
             SET updated_at_ms = ?2,
                 message_count = message_count + 1,
                 pinned_message_count = pinned_message_count + ?3
             WHERE id = ?1",
            params![chat_id, updated_at_ms, pin_inc],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Hot-path single-turn streaming checkpoint (`< 1 ms` WAL write).
    ///
    /// Replaces v2's `chats/<id>/live.json` sidecar + 30 s monolithic `chat.json` rewrite:
    /// updates only the streaming variant row, its tool calls, and the message's `active_content`
    /// (which refreshes FTS5 automatically via trigger).
    pub fn upsert_streaming_variant(
        &mut self,
        chat_id: &str,
        message_id: &str,
        variant_index: usize,
        variant: &MessageVariant,
        updated_at_ms: i64,
    ) -> StorageResult<()> {
        validate_session_id(chat_id)?;
        let tx = self.conn.transaction()?;
        upsert_variant_and_tools(&tx, chat_id, message_id, variant_index as i64, variant)?;
        tx.execute(
            "UPDATE messages
             SET active_content = ?3,
                 active_variant_index = ?4
             WHERE chat_id = ?1 AND id = ?2",
            params![chat_id, message_id, &variant.content, variant_index as i64],
        )?;
        tx.execute(
            "UPDATE chats SET updated_at_ms = ?2 WHERE id = ?1",
            params![chat_id, updated_at_ms],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_message_pinned(
        &mut self,
        chat_id: &str,
        message_id: &str,
        pinned: bool,
    ) -> StorageResult<()> {
        let tx = self.conn.transaction()?;
        let changed = tx.execute(
            "UPDATE messages SET pinned = ?3 WHERE chat_id = ?1 AND id = ?2 AND pinned != ?3",
            params![chat_id, message_id, i64::from(pinned)],
        )?;
        if changed > 0 {
            let delta: i64 = if pinned { 1 } else { -1 };
            tx.execute(
                "UPDATE chats SET pinned_message_count = MAX(0, pinned_message_count + ?2) WHERE id = ?1",
                params![chat_id, delta],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Paginated forward window load (`ORDER BY seq ASC LIMIT ? OFFSET ?`).
    pub fn load_messages_window(
        &self,
        chat_id: &str,
        limit: u32,
        offset: u32,
        hydrate_large_tools: bool,
    ) -> StorageResult<Vec<MessageNode>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, seq, role, active_variant_index, included_in_context, pinned
             FROM messages
             WHERE chat_id = ?1
             ORDER BY seq ASC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![chat_id, limit, offset], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                MessageRole::from_db_str(&r.get::<_, String>(2)?),
                r.get::<_, i64>(3)?.max(0) as usize,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, i64>(5)? != 0,
            ))
        })?;
        let mut headers = Vec::new();
        for row in rows {
            headers.push(row?);
        }
        hydrate_message_headers(&self.conn, chat_id, headers, hydrate_large_tools)
    }

    /// Instant tail load of the latest `limit` messages (returned in chronological ASC order).
    /// Opening a 10,000-message chat stays O(limit) in time and RAM.
    pub fn load_latest_messages(
        &self,
        chat_id: &str,
        limit: u32,
        hydrate_large_tools: bool,
    ) -> StorageResult<Vec<MessageNode>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, seq, role, active_variant_index, included_in_context, pinned
             FROM messages
             WHERE chat_id = ?1
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![chat_id, limit], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                MessageRole::from_db_str(&r.get::<_, String>(2)?),
                r.get::<_, i64>(3)?.max(0) as usize,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, i64>(5)? != 0,
            ))
        })?;
        let mut headers = Vec::new();
        for row in rows {
            headers.push(row?);
        }
        headers.reverse();
        hydrate_message_headers(&self.conn, chat_id, headers, hydrate_large_tools)
    }

    /// Lazy-loads the complete output of a spilled (>64 KiB) tool call on drawer expand.
    pub fn load_tool_call_full_result(
        &self,
        chat_id: &str,
        message_id: &str,
        variant_id: &str,
        seq: i64,
    ) -> StorageResult<Option<String>> {
        let row: Option<(String, Option<Vec<u8>>)> = self
            .conn
            .query_row(
                "SELECT result_text, result_overflow
                 FROM tool_calls
                 WHERE chat_id = ?1 AND message_id = ?2 AND variant_id = ?3 AND seq = ?4",
                params![chat_id, message_id, variant_id, seq],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.map(|(inline, overflow)| match overflow {
            Some(bytes) => String::from_utf8_lossy_owned(bytes),
            None => inline,
        }))
    }

    /// Hydrates a complete `ChatSession` (including full tool results) for generation or export.
    pub fn load_chat_session(&self, chat_id: &str) -> StorageResult<Option<ChatSession>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, created_at_ms, updated_at_ms, profile_id, model_override,
                        assistant_id, is_pinned, is_title_manually_set, draft_input, draft_prefill,
                        view_mode, workspace_path, workspace_mode, meta_json
                 FROM chats
                 WHERE id = ?1",
                params![chat_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, i64>(7)? != 0,
                        r.get::<_, i64>(8)? != 0,
                        r.get::<_, String>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, String>(11)?,
                        r.get::<_, String>(12)?,
                        r.get::<_, String>(13)?,
                        r.get::<_, String>(14)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            id,
            title,
            created_at,
            updated_at,
            profile_id,
            model_override,
            assistant_id,
            is_pinned,
            is_title_manually_set,
            draft_input,
            draft_prefill,
            view_mode_str,
            workspace_path,
            workspace_mode_str,
            meta_json,
        )) = row
        else {
            return Ok(None);
        };

        let meta: ChatMetaJson = serde_json::from_str(&meta_json).unwrap_or_default();
        let messages = self.load_messages_window(&id, u32::MAX, 0, true)?;

        Ok(Some(ChatSession {
            id,
            title,
            created_at,
            updated_at,
            profile_id,
            model_override,
            assistant_id,
            is_pinned,
            is_title_manually_set,
            draft_input,
            draft_prefill,
            user_name: if meta.user_name.is_empty() {
                "User".to_string()
            } else {
                meta.user_name
            },
            system_prompt: meta.system_prompt,
            active_card: meta.active_card,
            story_summary: meta.story_summary,
            fragments: meta.fragments,
            authors_note: meta.authors_note,
            authors_note_depth: if meta.authors_note_depth == 0 {
                2
            } else {
                meta.authors_note_depth
            },
            view_mode: ViewMode::from_db_str(&view_mode_str),
            workspace_path,
            workspace_mode: WorkspaceMode::from_db_str(&workspace_mode_str),
            permission_overrides: meta.permission_overrides,
            utility_usage_records: meta.utility_usage_records,
            messages,
        }))
    }

    pub fn delete_chat(&mut self, chat_id: &str) -> StorageResult<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM chats WHERE id = ?1", params![chat_id])?;
        Ok(deleted > 0)
    }

    /// Wipes all chats, messages, usage records, and settings from the local SQLite database.
    pub fn wipe_all_data(&mut self) -> StorageResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM usage_records;
             DELETE FROM tool_calls;
             DELETE FROM message_variants;
             DELETE FROM messages;
             DELETE FROM chats;
             DELETE FROM kv_settings;",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Safe FTS5 full-text search across all chats. User punctuation/operators are quoted as literals.
    pub fn search_messages(&self, raw_query: &str, limit: u32) -> StorageResult<Vec<SearchHit>> {
        let Some(fts_query) = escape_fts5_query(raw_query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT m.chat_id, c.title, m.id, snippet(messages_fts, 0, '[', ']', '...', 16)
             FROM messages_fts
             JOIN messages m ON m.rowid = messages_fts.rowid
             JOIN chats c ON c.id = m.chat_id
             WHERE messages_fts MATCH ?1
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit], |r| {
            Ok(SearchHit {
                chat_id: r.get(0)?,
                chat_title: r.get(1)?,
                message_id: r.get(2)?,
                snippet: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Direct distinct chat-ID FTS5 query (no snippet generation, no `chats` join, single `String`).
    pub fn search_chat_ids_newline(&self, raw_query: &str, limit: u32) -> StorageResult<String> {
        let Some(fts_query) = escape_fts5_query(raw_query) else {
            return Ok(String::new());
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT DISTINCT m.chat_id
             FROM messages_fts
             JOIN messages m ON m.rowid = messages_fts.rowid
             WHERE messages_fts MATCH ?1
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![fts_query, limit])?;
        let mut out = String::new();
        while let Some(row) = rows.next()? {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(row.get_ref(0)?.as_str().map_err(rusqlite::Error::from)?);
        }
        Ok(out)
    }

    /// Deduplicated gateway-reported spend total, counting each owned `request_id` strictly once
    /// even if a chat was forked into multiple copies.
    pub fn sum_owned_reported_usd(&self, owner_session_id: Option<&str>) -> StorageResult<f64> {
        let sum: f64 = match owner_session_id {
            Some(owner) => self.conn.query_row(
                "SELECT COALESCE(SUM(usd), 0.0) FROM (
                     SELECT request_id, MAX(reported_total_usd) AS usd
                     FROM usage_records
                     WHERE owner_session_id = ?1 AND reported_total_usd IS NOT NULL
                     GROUP BY request_id
                 )",
                params![owner],
                |r| r.get(0),
            )?,
            None => self.conn.query_row(
                "SELECT COALESCE(SUM(usd), 0.0) FROM (
                     SELECT request_id, MAX(reported_total_usd) AS usd
                     FROM usage_records
                     WHERE reported_total_usd IS NOT NULL
                     GROUP BY request_id
                 )",
                [],
                |r| r.get(0),
            )?,
        };
        Ok(sum)
    }

    /// Fast deterministic synthetic dataset seeder (300 chats + 200 active messages) for benchmarks.
    pub fn seed_benchmark_dataset(
        &mut self,
        chat_count: u32,
        messages_per_chat: u32,
        active_messages: u32,
    ) -> StorageResult<()> {
        let existing: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chats", [], |r| r.get(0))?;
        if existing >= i64::from(chat_count) {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        let base_reply = "This synthetic reply measures start-up and viewport latency with a realistic amount of prose and **Markdown** formatting. ";
        let long_reply = base_reply.repeat(6);
        let code_reply = "Here is a sample Rust streaming step:\n\n```rust\nfn stream_chunk(buf: &mut String, delta: &str) {\n    buf.push_str(delta);\n}\n```\n\nNotice how the layout keeps constant frame time even across long technical threads.";

        {
            let mut ins_chat = tx.prepare(
                "INSERT OR REPLACE INTO chats (
                    id, title, created_at_ms, updated_at_ms, profile_id, message_count
                 ) VALUES (?1, ?2, ?3, ?4, 'prof-gemini-interactions', ?5)",
            )?;
            let mut ins_msg = tx.prepare(
                "INSERT OR REPLACE INTO messages (
                    id, chat_id, seq, role, active_variant_index, included_in_context, pinned, created_at_ms, active_content
                 ) VALUES (?1, ?2, ?3, ?4, 0, 1, 0, ?5, ?6)",
            )?;
            let mut ins_var = tx.prepare(
                "INSERT OR REPLACE INTO variants (
                    id, message_id, chat_id, variant_index, content, created_at_ms
                 ) VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            )?;

            for c in 0..chat_count {
                let cid = format!("perf-{c:03}");
                let msg_total = if c == 0 {
                    active_messages.max(messages_per_chat)
                } else {
                    messages_per_chat
                };
                let ts = 1_700_000_000_000i64 + i64::from(chat_count - c) * 1000;
                ins_chat.execute(params![&cid, format!("Chat {c}"), ts, ts, msg_total])?;
                for m in 0..msg_total {
                    let role = if m % 2 == 0 { "USER" } else { "ASSISTANT" };
                    let body = if role == "USER" {
                        format!(
                            "Prompt {m}: Explain streaming turn isolation and SQLite WAL checkpoints."
                        )
                    } else if m % 4 == 1 {
                        format!("Turn {m}. {code_reply}")
                    } else {
                        format!("Turn {m}. {long_reply}")
                    };
                    let mid = format!("{cid}-m{m}");
                    let vid = format!("{cid}-v{m}");
                    let mts = ts + i64::from(m);
                    ins_msg.execute(params![&mid, &cid, i64::from(m), role, mts, &body])?;
                    ins_var.execute(params![&vid, &mid, &cid, &body, mts])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Truncates the SQLite WAL file and runs planner optimization to keep disk usage bounded.
    pub fn reclaim_storage(&self) -> StorageResult<()> {
        self.conn.execute_batch(
            "PRAGMA wal_checkpoint(TRUNCATE);
             PRAGMA optimize;",
        )?;
        Ok(())
    }
}
