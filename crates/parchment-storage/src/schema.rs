use rusqlite::Connection;

use crate::StorageResult;

pub const SCHEMA_VERSION: i32 = 2;

/// Tool call outputs larger than 64 KiB spill to `result_overflow` so viewport window queries
/// stay fast and lean even across 100-step agent runs with multi-megabyte tool receipts.
pub const INLINE_TOOL_RESULT_LIMIT_BYTES: usize = 64 * 1024;

pub fn configure_and_migrate(conn: &Connection) -> StorageResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;
         PRAGMA temp_store = MEMORY;
         PRAGMA wal_autocheckpoint = 1000;
         PRAGMA journal_size_limit = 8388608;",
    )?;
    let current_version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current_version < 1 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS kv_settings (
                 key TEXT PRIMARY KEY,
                 value_json TEXT NOT NULL,
                 updated_at_ms INTEGER NOT NULL DEFAULT 0
             );

             CREATE TABLE IF NOT EXISTS chats (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 updated_at_ms INTEGER NOT NULL DEFAULT 0,
                 profile_id TEXT NOT NULL DEFAULT '',
                 model_override TEXT NOT NULL DEFAULT '',
                 assistant_id TEXT NOT NULL DEFAULT '',
                 is_pinned INTEGER NOT NULL DEFAULT 0,
                 is_title_manually_set INTEGER NOT NULL DEFAULT 0,
                 draft_input TEXT NOT NULL DEFAULT '',
                 draft_prefill TEXT NOT NULL DEFAULT '',
                 view_mode TEXT NOT NULL DEFAULT 'EDITORIAL_TURNS',
                 workspace_path TEXT NOT NULL DEFAULT '',
                 workspace_mode TEXT NOT NULL DEFAULT 'OFF',
                 message_count INTEGER NOT NULL DEFAULT 0,
                 pinned_message_count INTEGER NOT NULL DEFAULT 0,
                 meta_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_chats_updated
                 ON chats(is_pinned DESC, updated_at_ms DESC);

             CREATE TABLE IF NOT EXISTS messages (
                 id TEXT NOT NULL,
                 chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
                 seq INTEGER NOT NULL,
                 role TEXT NOT NULL,
                 active_variant_index INTEGER NOT NULL DEFAULT 0,
                 included_in_context INTEGER NOT NULL DEFAULT 1,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 active_content TEXT NOT NULL DEFAULT '',
                 PRIMARY KEY (chat_id, id)
             );

             CREATE INDEX IF NOT EXISTS idx_messages_chat_seq
                 ON messages(chat_id, seq);

             CREATE INDEX IF NOT EXISTS idx_messages_pinned
                 ON messages(pinned, created_at_ms DESC) WHERE pinned = 1;

             CREATE TABLE IF NOT EXISTS variants (
                 id TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 chat_id TEXT NOT NULL,
                 variant_index INTEGER NOT NULL,
                 content TEXT NOT NULL DEFAULT '',
                 thinking_summary TEXT NOT NULL DEFAULT '',
                 trailing_thinking TEXT NOT NULL DEFAULT '',
                 thinking_signature TEXT NOT NULL DEFAULT '',
                 thinking_duration_ms INTEGER NOT NULL DEFAULT 0,
                 provider_interaction_id TEXT NOT NULL DEFAULT '',
                 pending_background_interaction_id TEXT NOT NULL DEFAULT '',
                 is_streaming INTEGER NOT NULL DEFAULT 0,
                 provider_protocol TEXT NOT NULL DEFAULT '',
                 provider_wire_dialect TEXT NOT NULL DEFAULT '',
                 model_used TEXT NOT NULL DEFAULT '',
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 meta_json TEXT NOT NULL DEFAULT '{}',
                 PRIMARY KEY (chat_id, message_id, id),
                 FOREIGN KEY (chat_id, message_id) REFERENCES messages(chat_id, id) ON DELETE CASCADE
             );

             CREATE INDEX IF NOT EXISTS idx_variants_message
                 ON variants(chat_id, message_id, variant_index);

             CREATE TABLE IF NOT EXISTS tool_calls (
                 id TEXT NOT NULL,
                 variant_id TEXT NOT NULL,
                 message_id TEXT NOT NULL,
                 chat_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 arguments_json TEXT NOT NULL DEFAULT '',
                 result_text TEXT NOT NULL DEFAULT '',
                 result_overflow BLOB,
                 is_error INTEGER NOT NULL DEFAULT 0,
                 is_running INTEGER NOT NULL DEFAULT 0,
                 duration_ms INTEGER NOT NULL DEFAULT 0,
                 thought_signature TEXT NOT NULL DEFAULT '',
                 preceding_thinking TEXT NOT NULL DEFAULT '',
                 preceding_text TEXT NOT NULL DEFAULT '',
                 message_phases INTEGER NOT NULL DEFAULT 0,
                 starts_tool_step INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (chat_id, message_id, variant_id, seq),
                 FOREIGN KEY (chat_id, message_id, variant_id)
                     REFERENCES variants(chat_id, message_id, id) ON DELETE CASCADE
             );

             CREATE TABLE IF NOT EXISTS usage_records (
                 request_id TEXT NOT NULL,
                 chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
                 message_id TEXT NOT NULL DEFAULT '',
                 variant_id TEXT NOT NULL DEFAULT '',
                 owner_session_id TEXT NOT NULL DEFAULT '',
                 owner_run_id TEXT NOT NULL DEFAULT '',
                 purpose TEXT NOT NULL DEFAULT 'chat',
                 profile_id TEXT NOT NULL DEFAULT '',
                 model TEXT NOT NULL DEFAULT '',
                 wire_dialect TEXT NOT NULL DEFAULT '',
                 input_tokens INTEGER,
                 output_tokens INTEGER,
                 cached_input_tokens INTEGER,
                 reasoning_tokens INTEGER,
                 reported_total_usd REAL,
                 record_json TEXT NOT NULL,
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (chat_id, request_id)
             );

             CREATE INDEX IF NOT EXISTS idx_usage_owner
                 ON usage_records(owner_session_id, request_id);

             CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                 active_content,
                 content='messages',
                 content_rowid='rowid'
             );

             CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                 INSERT INTO messages_fts(rowid, active_content)
                 VALUES (new.rowid, new.active_content);
             END;

             CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                 INSERT INTO messages_fts(messages_fts, rowid, active_content)
                 VALUES ('delete', old.rowid, old.active_content);
             END;

             CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF active_content ON messages BEGIN
                 INSERT INTO messages_fts(messages_fts, rowid, active_content)
                 VALUES ('delete', old.rowid, old.active_content);
                 INSERT INTO messages_fts(rowid, active_content)
                 VALUES (new.rowid, new.active_content);
             END;

             PRAGMA user_version = 1;",
        )?;
    }

    if current_version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS coding_tasks (
                 id TEXT PRIMARY KEY,
                 title TEXT NOT NULL,
                 description TEXT NOT NULL,
                 state TEXT NOT NULL DEFAULT 'PENDING',
                 workspace_path TEXT NOT NULL DEFAULT '',
                 repo_url TEXT NOT NULL DEFAULT '',
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 updated_at_ms INTEGER NOT NULL DEFAULT 0,
                 completed_at_ms INTEGER,
                 task_json TEXT NOT NULL DEFAULT '{}',
                 owner_session_id TEXT NOT NULL DEFAULT '',
                 owner_run_id TEXT NOT NULL DEFAULT ''
             );

             CREATE INDEX IF NOT EXISTS idx_coding_tasks_workspace
                 ON coding_tasks(workspace_path);
             CREATE INDEX IF NOT EXISTS idx_coding_tasks_state
                 ON coding_tasks(state);
             CREATE INDEX IF NOT EXISTS idx_coding_tasks_owner
                 ON coding_tasks(owner_session_id);

             CREATE TABLE IF NOT EXISTS coding_subtasks (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES coding_tasks(id) ON DELETE CASCADE,
                 title TEXT NOT NULL,
                 description TEXT NOT NULL,
                 kind TEXT NOT NULL DEFAULT 'EXPLORE',
                 state TEXT NOT NULL DEFAULT 'PENDING',
                 depends_on_json TEXT NOT NULL DEFAULT '[]',
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 completed_at_ms INTEGER,
                 subtask_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_coding_subtasks_task
                 ON coding_subtasks(task_id);
             CREATE INDEX IF NOT EXISTS idx_coding_subtasks_state
                 ON coding_subtasks(state);

             CREATE TABLE IF NOT EXISTS coding_acceptance_criteria (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES coding_tasks(id) ON DELETE CASCADE,
                 subtask_id TEXT REFERENCES coding_subtasks(id) ON DELETE CASCADE,
                 description TEXT NOT NULL,
                 kind TEXT NOT NULL DEFAULT 'CUSTOM',
                 command TEXT NOT NULL DEFAULT '',
                 expected TEXT NOT NULL DEFAULT '',
                 verified INTEGER NOT NULL DEFAULT 0,
                 verified_at_ms INTEGER,
                 criterion_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_coding_criteria_task
                 ON coding_acceptance_criteria(task_id);

             CREATE TABLE IF NOT EXISTS coding_checkpoints (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES coding_tasks(id) ON DELETE CASCADE,
                 subtask_id TEXT REFERENCES coding_subtasks(id) ON DELETE CASCADE,
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 description TEXT NOT NULL DEFAULT '',
                 snapshot_ref TEXT NOT NULL DEFAULT '',
                 resumable INTEGER NOT NULL DEFAULT 1,
                 checkpoint_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_coding_checkpoints_task
                 ON coding_checkpoints(task_id);

             CREATE TABLE IF NOT EXISTS coding_artifacts (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES coding_tasks(id) ON DELETE CASCADE,
                 kind TEXT NOT NULL DEFAULT 'PATCH',
                 path TEXT NOT NULL DEFAULT '',
                 content_hash TEXT NOT NULL DEFAULT '',
                 bytes INTEGER NOT NULL DEFAULT 0,
                 created_at_ms INTEGER NOT NULL DEFAULT 0,
                 artifact_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_coding_artifacts_task
                 ON coding_artifacts(task_id);

             CREATE TABLE IF NOT EXISTS coding_hooks (
                 id TEXT PRIMARY KEY,
                 task_id TEXT NOT NULL REFERENCES coding_tasks(id) ON DELETE CASCADE,
                 event TEXT NOT NULL DEFAULT 'POST_TOOL_USE',
                 matcher TEXT NOT NULL DEFAULT '',
                 command TEXT NOT NULL DEFAULT '',
                 timeout_ms INTEGER NOT NULL DEFAULT 60000,
                 blocking INTEGER NOT NULL DEFAULT 0,
                 hook_json TEXT NOT NULL DEFAULT '{}'
             );

             CREATE INDEX IF NOT EXISTS idx_coding_hooks_task
                 ON coding_hooks(task_id);

             PRAGMA user_version = 2;",
        )?;
    }

    Ok(())
}
