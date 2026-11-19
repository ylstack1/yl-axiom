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
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
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
}

pub fn validate_session_id(id: &str) -> StorageResult<()> {
    let trimmed = id.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.starts_with('.')
        || trimmed.ends_with('.')
        || trimmed.contains(['/', '\\', '\0'])
        || is_windows_device_name(trimmed)
    {
        return Err(StorageError::InvalidId(id.to_string()));
    }
    Ok(())
}

pub(crate) fn is_windows_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    if stem.eq_ignore_ascii_case("CON")
        || stem.eq_ignore_ascii_case("PRN")
        || stem.eq_ignore_ascii_case("AUX")
        || stem.eq_ignore_ascii_case("NUL")
    {
        return true;
    }
    let b = stem.as_bytes();
    b.len() == 4
        && (b[..3].eq_ignore_ascii_case(b"COM") || b[..3].eq_ignore_ascii_case(b"LPT"))
        && b[3].is_ascii_digit()
}

/// Quotes every whitespace-separated token as an FTS5 phrase literal so punctuation like `C++`
/// or `foo-bar` never causes an FTS5 syntax error.
pub fn escape_fts5_query(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len() + 8);
    for token in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push('"');
        for ch in token.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
    }
    (!out.is_empty()).then_some(out)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn map_pinned_stub(row: &rusqlite::Row<'_>) -> rusqlite::Result<PinnedMessageStub> {
    let mut preview: String = row.get(4)?;
    if let Some((cut, _)) = preview.char_indices().nth(180) {
        preview.truncate(cut);
    }
    Ok(PinnedMessageStub {
        chat_id: row.get(0)?,
        chat_title: row.get(1)?,
        message_id: row.get(2)?,
        role: MessageRole::from_db_str(&row.get::<_, String>(3)?),
        preview,
        created_at: row.get(5)?,
    })
}

fn upsert_chat_row(tx: &Transaction<'_>, session: &ChatSession) -> StorageResult<()> {
    let pinned_count = session.messages.iter().filter(|m| m.pinned).count() as i64;
    upsert_chat_row_with_counts(tx, session, session.messages.len() as i64, pinned_count)
}

fn upsert_chat_row_with_counts(
    tx: &Transaction<'_>,
    session: &ChatSession,
    message_count: i64,
    pinned_message_count: i64,
) -> StorageResult<()> {
    let meta = ChatMetaRef {
        user_name: &session.user_name,
        system_prompt: &session.system_prompt,
        active_card: &session.active_card,
        story_summary: &session.story_summary,
        fragments: &session.fragments,
        authors_note: &session.authors_note,
        authors_note_depth: session.authors_note_depth,
        permission_overrides: &session.permission_overrides,
        utility_usage_records: &session.utility_usage_records,
    };
    let meta_json = serde_json::to_string(&meta)?;

    tx.execute(
        "INSERT INTO chats (
             id, title, created_at_ms, updated_at_ms, profile_id, model_override, assistant_id,
             is_pinned, is_title_manually_set, draft_input, draft_prefill, view_mode,
             workspace_path, workspace_mode, message_count, pinned_message_count, meta_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(id) DO UPDATE SET
             title = excluded.title,
             created_at_ms = excluded.created_at_ms,
             updated_at_ms = excluded.updated_at_ms,
             profile_id = excluded.profile_id,
             model_override = excluded.model_override,
             assistant_id = excluded.assistant_id,
             is_pinned = excluded.is_pinned,
             is_title_manually_set = excluded.is_title_manually_set,
             draft_input = excluded.draft_input,
             draft_prefill = excluded.draft_prefill,
             view_mode = excluded.view_mode,
             workspace_path = excluded.workspace_path,
             workspace_mode = excluded.workspace_mode,
             message_count = excluded.message_count,
             pinned_message_count = excluded.pinned_message_count,
             meta_json = excluded.meta_json",
        params![
            &session.id,
            &session.title,
            session.created_at,
            session.updated_at,
            &session.profile_id,
            &session.model_override,
            &session.assistant_id,
            i64::from(session.is_pinned),
            i64::from(session.is_title_manually_set),
            &session.draft_input,
            &session.draft_prefill,
            session.view_mode.as_db_str(),
            &session.workspace_path,
            session.workspace_mode.as_db_str(),
            message_count,
            pinned_message_count,
            meta_json,
        ],
    )?;
    Ok(())
}

fn insert_message_node(
    tx: &Transaction<'_>,
    chat_id: &str,
    seq: i64,
    node: &MessageNode,
) -> StorageResult<()> {
    let created_at = node.active_variant().map(|v| v.created_at).unwrap_or(0);
    tx.execute(
        "INSERT INTO messages (
             id, chat_id, seq, role, active_variant_index, included_in_context,
             pinned, created_at_ms, active_content
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &node.id,
            chat_id,
            seq,
            node.role.as_db_str(),
            node.active_variant_index as i64,
            i64::from(node.included_in_context),
            i64::from(node.pinned),
            created_at,
            node.active_content(),
        ],
    )?;

    for (v_idx, variant) in node.variants.iter().enumerate() {
        upsert_variant_and_tools(tx, chat_id, &node.id, v_idx as i64, variant)?;
    }
    Ok(())
}

fn upsert_variant_and_tools(
    tx: &Transaction<'_>,
    chat_id: &str,
    message_id: &str,
    variant_index: i64,
    variant: &MessageVariant,
) -> StorageResult<()> {
    let proto_str = variant
        .provider_protocol
        .map(ProviderProtocol::as_db_str)
        .unwrap_or("");
    let meta = VariantMetaRef {
        goal_tracker: &variant.goal_tracker,
        media_items: &variant.media_items,
        attachments: &variant.attachments,
        web_sources: &variant.web_sources,
        token_metrics: &variant.token_metrics,
        api_usage_records: &variant.api_usage_records,
        legacy_usage_owner_session_id: &variant.legacy_usage_owner_session_id,
        raw_request_preview: &variant.raw_request_preview,
        raw_response_preview: &variant.raw_response_preview,
    };
    let meta_json = serde_json::to_string(&meta)?;

    tx.execute(
        "INSERT INTO variants (
             id, message_id, chat_id, variant_index, content, thinking_summary, trailing_thinking,
             thinking_signature, thinking_duration_ms, provider_interaction_id,
             pending_background_interaction_id, is_streaming, provider_protocol,
             provider_wire_dialect, model_used, created_at_ms, meta_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(chat_id, message_id, id) DO UPDATE SET
             variant_index = excluded.variant_index,
             content = excluded.content,
             thinking_summary = excluded.thinking_summary,
             trailing_thinking = excluded.trailing_thinking,
             thinking_signature = excluded.thinking_signature,
             thinking_duration_ms = excluded.thinking_duration_ms,
             provider_interaction_id = excluded.provider_interaction_id,
             pending_background_interaction_id = excluded.pending_background_interaction_id,
             is_streaming = excluded.is_streaming,
             provider_protocol = excluded.provider_protocol,
             provider_wire_dialect = excluded.provider_wire_dialect,
             model_used = excluded.model_used,
             created_at_ms = excluded.created_at_ms,
             meta_json = excluded.meta_json",
        params![
            &variant.id,
            message_id,
            chat_id,
            variant_index,
            &variant.content,
            &variant.thinking_summary,
            &variant.trailing_thinking,
            &variant.thinking_signature,
            variant.thinking_duration_ms,
            &variant.provider_interaction_id,
            &variant.pending_background_interaction_id,
            i64::from(variant.is_streaming),
            proto_str,
            &variant.provider_wire_dialect,
            &variant.model_used,
            variant.created_at,
            meta_json,
        ],
    )?;

    tx.execute(
        "DELETE FROM tool_calls WHERE chat_id = ?1 AND message_id = ?2 AND variant_id = ?3",
        params![chat_id, message_id, &variant.id],
    )?;
    for (seq, tc) in variant.tool_calls.iter().enumerate() {
        let (inline_text, overflow_blob): (&str, Option<&[u8]>) =
            if tc.result_text.len() > INLINE_TOOL_RESULT_LIMIT_BYTES {
                let mut cut = INLINE_TOOL_RESULT_LIMIT_BYTES;
                while cut > 0 && !tc.result_text.is_char_boundary(cut) {
                    cut -= 1;
                }
                (&tc.result_text[..cut], Some(tc.result_text.as_bytes()))
            } else {
                (tc.result_text.as_str(), None)
            };
        tx.execute(
            "INSERT INTO tool_calls (
                 id, variant_id, message_id, chat_id, seq, name, arguments_json,
                 result_text, result_overflow, is_error, is_running, duration_ms,
                 thought_signature, preceding_thinking, preceding_text,
                 message_phases, starts_tool_step
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                &tc.id,
                &variant.id,
                message_id,
                chat_id,
                seq as i64,
                &tc.name,
                &tc.arguments_json,
                inline_text,
                overflow_blob,
                i64::from(tc.is_error),
                i64::from(tc.is_running),
                tc.duration_ms,
                &tc.thought_signature,
                &tc.preceding_thinking,
                &tc.preceding_text,
                i64::from(tc.message_phases),
                i64::from(tc.starts_tool_step),
            ],
        )?;
    }

    for rec in &variant.api_usage_records {
        insert_usage_record(
            tx,
            chat_id,
            message_id,
            &variant.id,
            rec,
            variant.created_at,
        )?;
    }
    Ok(())
}

fn insert_usage_record(
    tx: &Transaction<'_>,
    chat_id: &str,
    message_id: &str,
    variant_id: &str,
    rec: &ApiUsageRecord,
    created_at_ms: i64,
) -> StorageResult<()> {
    let record_json = serde_json::to_string(rec)?;
    tx.execute(
        "INSERT INTO usage_records (
             request_id, chat_id, message_id, variant_id, owner_session_id, owner_run_id,
             purpose, profile_id, model, wire_dialect, input_tokens, output_tokens,
             cached_input_tokens, reasoning_tokens, reported_total_usd, record_json, created_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(chat_id, request_id) DO UPDATE SET
             owner_session_id = excluded.owner_session_id,
             input_tokens = excluded.input_tokens,
             output_tokens = excluded.output_tokens,
             cached_input_tokens = excluded.cached_input_tokens,
             reasoning_tokens = excluded.reasoning_tokens,
             reported_total_usd = excluded.reported_total_usd,
             record_json = excluded.record_json",
        params![
            &rec.request_id,
            chat_id,
            message_id,
            variant_id,
            &rec.owner_session_id,
            &rec.owner_run_id,
            &rec.purpose,
            &rec.profile_id,
            &rec.model,
            &rec.wire_dialect,
            rec.usage.input_tokens,
            rec.usage.output_tokens,
            rec.usage.cached_input_tokens,
            rec.usage.reasoning_tokens,
            rec.usage.charges.reported_total_usd,
            record_json,
            created_at_ms,
        ],
    )?;
    Ok(())
}

type MessageHeader = (String, i64, MessageRole, usize, bool, bool);

fn hydrate_message_headers(
    conn: &Connection,
    chat_id: &str,
    headers: Vec<MessageHeader>,
    hydrate_large_tools: bool,
) -> StorageResult<Vec<MessageNode>> {
    if headers.is_empty() {
        return Ok(Vec::new());
    }

    // Headers come from a contiguous `seq` slice: bound both scans by `[min_seq, max_seq]` so
    // loading the latest 50 turns of a 10,000-turn chat stays strictly O(limit) in time and RAM.
    let min_seq = headers.first().map(|h| h.1).unwrap_or(0);
    let max_seq = headers.last().map(|h| h.1).unwrap_or(min_seq);
    let mut variants_by_msg: HashMap<String, Vec<MessageVariant>> =
        HashMap::with_capacity(headers.len());

    let mut tool_stmt = conn.prepare_cached(if hydrate_large_tools {
        "SELECT tc.message_id, tc.variant_id, tc.id, tc.name, tc.arguments_json, tc.result_text,
                tc.result_overflow, tc.is_error, tc.is_running, tc.duration_ms,
                tc.thought_signature, tc.preceding_thinking, tc.preceding_text,
                tc.message_phases, tc.starts_tool_step
         FROM messages m
         JOIN tool_calls tc ON tc.chat_id = m.chat_id AND tc.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, tc.variant_id, tc.seq ASC"
    } else {
        "SELECT tc.message_id, tc.variant_id, tc.id, tc.name, tc.arguments_json, tc.result_text,
                NULL, tc.is_error, tc.is_running, tc.duration_ms,
                tc.thought_signature, tc.preceding_thinking, tc.preceding_text,
                tc.message_phases, tc.starts_tool_step
         FROM messages m
         JOIN tool_calls tc ON tc.chat_id = m.chat_id AND tc.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, tc.variant_id, tc.seq ASC"
    })?;
    let mut tools_by_msg: HashMap<String, HashMap<String, Vec<ToolCallRecord>>> = HashMap::new();
    let tool_rows = tool_stmt.query_map(params![chat_id, min_seq, max_seq], |r| {
        let msg_id: String = r.get(0)?;
        let var_id: String = r.get(1)?;
        let inline_res: String = r.get(5)?;
        let overflow: Option<Vec<u8>> = r.get(6)?;
        let result_text = match overflow {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => inline_res,
        };
        Ok((
            msg_id,
            var_id,
            ToolCallRecord {
                id: r.get(2)?,
                name: r.get(3)?,
                arguments_json: r.get(4)?,
                result_text,
                is_error: r.get::<_, i64>(7)? != 0,
                is_running: r.get::<_, i64>(8)? != 0,
                duration_ms: r.get(9)?,
                thought_signature: r.get(10)?,
                preceding_thinking: r.get(11)?,
                preceding_text: r.get(12)?,
                screenshot_jpeg_base64: String::new(),
                message_phases: r.get::<_, i64>(13)? != 0,
                starts_tool_step: r.get::<_, i64>(14)? != 0,
            },
        ))
    })?;
    for row in tool_rows {
        let (mid, vid, rec) = row?;
        tools_by_msg
            .entry(mid)
            .or_default()
            .entry(vid)
            .or_default()
            .push(rec);
    }

    let mut var_stmt = conn.prepare_cached(
        "SELECT v.message_id, v.id, v.content, v.thinking_summary, v.trailing_thinking,
                v.thinking_signature, v.thinking_duration_ms, v.provider_interaction_id,
                v.pending_background_interaction_id, v.is_streaming, v.provider_protocol,
                v.provider_wire_dialect, v.model_used, v.created_at_ms, v.meta_json
         FROM messages m
         JOIN variants v ON v.chat_id = m.chat_id AND v.message_id = m.id
         WHERE m.chat_id = ?1 AND m.seq BETWEEN ?2 AND ?3
         ORDER BY m.seq ASC, v.variant_index ASC",
    )?;
    let var_rows = var_stmt.query_map(params![chat_id, min_seq, max_seq], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, i64>(9)? != 0,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
            r.get::<_, String>(12)?,
            r.get::<_, i64>(13)?,
            r.get::<_, String>(14)?,
        ))
    })?;
    for row in var_rows {
        let (
            msg_id,
            id,
            content,
            thinking_summary,
            trailing_thinking,
            thinking_signature,
            thinking_duration_ms,
            provider_interaction_id,
            pending_background_interaction_id,
            is_streaming,
            proto_raw,
            provider_wire_dialect,
            model_used,
            created_at,
            meta_json,
        ) = row?;
        let meta: VariantMetaJson = if meta_json.is_empty() || meta_json == "{}" {
            VariantMetaJson::default()
        } else {
            serde_json::from_str(&meta_json).unwrap_or_default()
        };
        let provider_protocol = ProviderProtocol::from_db_str(&proto_raw);
        let tool_calls = tools_by_msg
            .get_mut(&msg_id)
            .and_then(|by_var| by_var.remove(&id))
            .unwrap_or_default();
        variants_by_msg
            .entry(msg_id)
            .or_default()
            .push(MessageVariant {
                id,
                content,
                thinking_summary,
                trailing_thinking,
                thinking_signature,
                thinking_duration_ms,
                tool_calls,
                goal_tracker: meta.goal_tracker,
                media_items: meta.media_items,
                attachments: meta.attachments,
                web_sources: meta.web_sources,
                provider_interaction_id,
                pending_background_interaction_id,
                is_streaming,
                provider_protocol,
                provider_wire_dialect,
                model_used,
                created_at,
                token_metrics: meta.token_metrics,
                api_usage_records: meta.api_usage_records,
                legacy_usage_owner_session_id: meta.legacy_usage_owner_session_id,
                raw_request_preview: meta.raw_request_preview,
                raw_response_preview: meta.raw_response_preview,
            });
    }

    Ok(headers
        .into_iter()
        .map(
            |(id, _seq, role, active_variant_index, included_in_context, pinned)| {
                let variants = variants_by_msg.remove(&id).unwrap_or_default();
                MessageNode {
                    id,
                    role,
                    variants,
                    active_variant_index,
                    included_in_context,
                    pinned,
                }
            },
        )
        .collect())
}
