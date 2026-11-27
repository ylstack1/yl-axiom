//! YL Axiom v3 shared core facade (`parchment-model` + `parchment-storage` + `parchment-prose`
//! + `parchment-harness` + optional `parchment-protocol` network runtime).
//!
//! - Default (`network` feature): native Rust runtime for Windows GPUI desktop.
//! - Slim (`--no-default-features --features android-jni`): zero-TLS-duplication direct JNI `.so`
//!   for Android (SQLite WAL + FTS5 + v2 migrator + native search + `.docx`).

use std::path::Path;
use std::sync::{Arc, Mutex};
#[cfg(feature = "network")]
use std::time::Duration;

use parchment_model::{
    AppStateSnapshot, ChatSession, ChatStub, DesktopWindowConfig, MessageNode, MessageRole,
    MessageVariant, PinnedMessageStub,
};
#[cfg(feature = "network")]
use parchment_protocol::{
    O200kTokenizer, ProtocolClient, ProtocolError, RegistryCatalogClient, RegistryEndpoints,
};
use parchment_storage::{
    ChatArchiveLimits, ParchmentStore, SearchHit, StorageError, V2MigrationReport,
    export_chats_to_zip, import_chat_file_bytes, migrate_v2_directory,
};

pub use parchment_harness as harness;
pub use parchment_model as model;
pub use parchment_model::ChatStub as CoreChatStub;
pub use parchment_prose as prose;
#[cfg(feature = "network")]
pub use parchment_protocol as protocol;
#[cfg(feature = "network")]
pub use parchment_protocol::{SseFrame, SseFramer};
pub use parchment_storage as storage;
pub use parchment_storage::{SearchHit as CoreSearchHit, V2MigrationReport as CoreMigrationReport};

#[cfg(all(feature = "android-jni", feature = "network"))]
mod jni_android;

#[derive(Debug, Clone)]
pub enum CoreError {
    Storage(String),
    Network(String),
    Protocol(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(msg) => write!(f, "storage error: {msg}"),
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<StorageError> for CoreError {
    fn from(err: StorageError) -> Self {
        Self::Storage(err.to_string())
    }
}

#[cfg(feature = "network")]
impl From<ProtocolError> for CoreError {
    fn from(err: ProtocolError) -> Self {
        Self::Protocol(err.to_string())
    }
}

/// Core storage + optional network handle shared by Windows GPUI and Android Kotlin.
pub struct ParchmentCore {
    store: Mutex<ParchmentStore>,
    #[cfg(feature = "network")]
    http: reqwest::Client,
    #[cfg(feature = "network")]
    tokenizer: O200kTokenizer,
    #[cfg(feature = "network")]
    registry: Arc<RegistryCatalogClient>,
    #[cfg(feature = "network")]
    runtime: Arc<tokio::runtime::Runtime>,
}

impl ParchmentCore {
    pub fn open(db_path: impl AsRef<Path>) -> Result<Arc<Self>, CoreError> {
        let store = ParchmentStore::open(db_path.as_ref())?;

        #[cfg(feature = "network")]
        let http = reqwest::Client::builder()
            .use_rustls_tls()
            .http2_adaptive_window(true)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| CoreError::Network(e.to_string()))?;

        #[cfg(feature = "network")]
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| CoreError::Network(e.to_string()))?;

        #[cfg(feature = "network")]
        let registry = Arc::new(RegistryCatalogClient::new(
            http.clone(),
            RegistryEndpoints::default(),
        ));

        Ok(Arc::new(Self {
            store: Mutex::new(store),
            #[cfg(feature = "network")]
            http,
            #[cfg(feature = "network")]
            tokenizer: O200kTokenizer::new(),
            #[cfg(feature = "network")]
            registry,
            #[cfg(feature = "network")]
            runtime: Arc::new(runtime),
        }))
    }

    fn lock_store(&self) -> Result<std::sync::MutexGuard<'_, ParchmentStore>, CoreError> {
        self.store
            .lock()
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    /// Read-only store access for the JNI bridge. Panics are impossible here
    /// because the caller holds no lock; a poisoned mutex yields an empty guard
    /// error which the JNI layer turns into a null/false result.
    #[cfg(feature = "android-jni")]
    pub(crate) fn lock_store_for_jni(&self) -> std::sync::MutexGuard<'_, ParchmentStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Mutable store access for the JNI bridge.
    #[cfg(feature = "android-jni")]
    pub(crate) fn lock_store_for_jni_mut(&self) -> std::sync::MutexGuard<'_, ParchmentStore> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(feature = "network")]
    pub fn protocol_client(&self) -> ProtocolClient {
        ProtocolClient::new(self.http.clone())
    }

    #[cfg(feature = "network")]
    pub fn http_client(&self) -> &reqwest::Client {
        &self.http
    }

    #[cfg(feature = "network")]
    pub fn runtime(&self) -> &Arc<tokio::runtime::Runtime> {
        &self.runtime
    }

    #[cfg(feature = "network")]
    pub fn count_tokens(&self, text: &str) -> u32 {
        self.tokenizer.count_tokens(text, true) as u32
    }

    #[cfg(feature = "network")]
    pub fn registry(&self) -> &Arc<RegistryCatalogClient> {
        &self.registry
    }

    #[cfg(feature = "network")]
    pub fn tokenizer(&self) -> &O200kTokenizer {
        &self.tokenizer
    }

    pub fn load_desktop_window(&self) -> Result<DesktopWindowConfig, CoreError> {
        Ok(self.lock_store()?.load_desktop_window()?)
    }

    pub fn save_desktop_window(&self, cfg: &DesktopWindowConfig) -> Result<(), CoreError> {
        Ok(self.lock_store()?.save_desktop_window(cfg)?)
    }

    pub fn wipe_all_data(&self) -> Result<(), CoreError> {
        Ok(self.lock_store()?.wipe_all_data()?)
    }

    pub fn append_message_node(
        &self,
        chat_id: &str,
        node: &MessageNode,
        updated_at_ms: i64,
    ) -> Result<(), CoreError> {
        Ok(self
            .lock_store()?
            .append_message_node(chat_id, node, updated_at_ms)?)
    }

    pub fn upsert_streaming_variant(
        &self,
        chat_id: &str,
        message_id: &str,
        variant_index: usize,
        variant: &MessageVariant,
        updated_at_ms: i64,
    ) -> Result<(), CoreError> {
        Ok(self.lock_store()?.upsert_streaming_variant(
            chat_id,
            message_id,
            variant_index,
            variant,
            updated_at_ms,
        )?)
    }

    pub fn set_message_pinned(
        &self,
        chat_id: &str,
        message_id: &str,
        pinned: bool,
    ) -> Result<(), CoreError> {
        Ok(self
            .lock_store()?
            .set_message_pinned(chat_id, message_id, pinned)?)
    }

    pub fn load_settings_snapshot(&self) -> Result<AppStateSnapshot, CoreError> {
        Ok(self.lock_store()?.load_settings()?)
    }

    pub fn save_settings_snapshot(&self, snapshot: &AppStateSnapshot) -> Result<(), CoreError> {
        Ok(self.lock_store()?.save_settings(snapshot)?)
    }

    pub fn load_full_session(&self, chat_id: &str) -> Result<Option<ChatSession>, CoreError> {
        Ok(self.lock_store()?.load_chat_session(chat_id)?)
    }

    pub fn save_full_session(&self, session: &ChatSession) -> Result<(), CoreError> {
        Ok(self.lock_store()?.upsert_chat_session(session)?)
    }

    pub fn load_settings_json(&self) -> Result<Option<String>, CoreError> {
        let snap = self.load_settings_snapshot()?;
        if snap.profiles.is_empty()
            && snap.assistants.is_empty()
            && snap.sessions.is_empty()
            && snap.active_session_id.is_empty()
        {
            return Ok(None);
        }
        serde_json::to_string(&snap)
            .map(Some)
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    pub fn save_settings_json(&self, json: &str) -> Result<(), CoreError> {
        let snap: AppStateSnapshot =
            serde_json::from_str(json).map_err(|e| CoreError::Storage(e.to_string()))?;
        self.save_settings_snapshot(&snap)
    }

    pub fn load_chat_json(&self, chat_id: &str) -> Result<Option<String>, CoreError> {
        let Some(session) = self.load_full_session(chat_id)? else {
            return Ok(None);
        };
        serde_json::to_string(&session)
            .map(Some)
            .map_err(|e| CoreError::Storage(e.to_string()))
    }

    pub fn save_chat_json(&self, json: &str) -> Result<(), CoreError> {
        let session: ChatSession =
            serde_json::from_str(json).map_err(|e| CoreError::Storage(e.to_string()))?;
        self.save_full_session(&session)
    }

    pub fn search_chat_ids_newline(&self, query: &str, limit: u32) -> Result<String, CoreError> {
        Ok(self.lock_store()?.search_chat_ids_newline(query, limit)?)
    }

    pub fn workspace_search_native(
        root_path: &str,
        query: &str,
        scope_path: &str,
        file_glob: &str,
        max_results: usize,
    ) -> Result<String, CoreError> {
        let scope = (!scope_path.trim().is_empty()).then_some(scope_path);
        let glob = (!file_glob.trim().is_empty()).then_some(file_glob);
        parchment_harness::WorkspacePathGuard::search_files(
            root_path,
            query,
            scope,
            glob,
            max_results,
        )
        .map_err(|e| CoreError::Storage(e.to_string()))
    }

    pub fn extract_docx_native(bytes: &[u8]) -> Result<String, CoreError> {
        parchment_harness::extract_docx_text(bytes).map_err(CoreError::Protocol)
    }

    pub fn migrate_v2_data_dir(
        &self,
        v2_root_path: impl AsRef<Path>,
        force: bool,
    ) -> Result<V2MigrationReport, CoreError> {
        let mut store = self.lock_store()?;
        Ok(migrate_v2_directory(
            &mut store,
            v2_root_path.as_ref(),
            force,
        )?)
    }

    pub fn list_chat_stubs(&self) -> Result<Vec<ChatStub>, CoreError> {
        Ok(self.lock_store()?.list_chat_stubs()?)
    }

    pub fn list_pinned_message_stubs(
        &self,
        chat_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<PinnedMessageStub>, CoreError> {
        Ok(self.lock_store()?.list_pinned_messages(chat_id, limit)?)
    }

    pub fn load_messages_window(
        &self,
        chat_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MessageNode>, CoreError> {
        Ok(self
            .lock_store()?
            .load_messages_window(chat_id, limit, offset, false)?)
    }

    pub fn load_latest_messages(
        &self,
        chat_id: &str,
        limit: u32,
    ) -> Result<Vec<MessageNode>, CoreError> {
        Ok(self
            .lock_store()?
            .load_latest_messages(chat_id, limit, false)?)
    }

    pub fn append_message(
        &self,
        chat_id: &str,
        message_id: String,
        role: MessageRole,
        content: String,
        created_at_ms: i64,
    ) -> Result<(), CoreError> {
        let node = MessageNode {
            id: message_id.clone(),
            role,
            variants: vec![MessageVariant {
                id: format!("{message_id}-v0"),
                content,
                created_at: created_at_ms,
                ..Default::default()
            }],
            active_variant_index: 0,
            included_in_context: true,
            pinned: false,
        };
        Ok(self
            .lock_store()?
            .append_message_node(chat_id, &node, created_at_ms)?)
    }

    pub fn delete_chat(&self, chat_id: &str) -> Result<bool, CoreError> {
        Ok(self.lock_store()?.delete_chat(chat_id)?)
    }

    pub fn search_messages(&self, query: &str, limit: u32) -> Result<Vec<SearchHit>, CoreError> {
        Ok(self.lock_store()?.search_messages(query, limit)?)
    }

    pub fn seed_benchmark_dataset(
        &self,
        chat_count: u32,
        messages_per_chat: u32,
        active_messages: u32,
    ) -> Result<(), CoreError> {
        Ok(self.lock_store()?.seed_benchmark_dataset(
            chat_count,
            messages_per_chat,
            active_messages,
        )?)
    }

    pub fn import_archive_bytes(
        &self,
        file_name: &str,
        bytes: &[u8],
        storage_root: &Path,
    ) -> Result<Vec<ChatSession>, CoreError> {
        let imported =
            import_chat_file_bytes(file_name, bytes, storage_root, ChatArchiveLimits::default())?;
        let mut store = self.lock_store()?;
        for session in &imported {
            store.upsert_chat_session(session)?;
        }
        Ok(imported)
    }

    pub fn export_sessions_archive(
        &self,
        dest_path: &Path,
        chat_ids: &[String],
        storage_root: &Path,
    ) -> Result<(), CoreError> {
        let store = self.lock_store()?;
        let mut sessions = Vec::with_capacity(chat_ids.len());
        for cid in chat_ids {
            if let Some(s) = store.load_chat_session(cid)? {
                sessions.push(s);
            }
        }
        Ok(export_chats_to_zip(dest_path, &sessions, storage_root)?)
    }
}

/// Generates a deterministic ~87 KB Markdown + Rust code-fence payload for streaming frame benchmarks.
pub fn build_streaming_code_fence_payload() -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(90_000);
    out.push_str("## Streaming Code-Fence Stress Benchmark\n\n");
    out.push_str("Measuring incremental GPUI rendering of a **87 KB** live SSE code fence:\n\n");
    out.push_str("```rust\n");
    for i in 0..950 {
        let _ = writeln!(
            out,
            "pub fn step_{i:04}(acc: &mut String, delta: &str) -> usize {{ acc.push_str(delta); acc.len() }}"
        );
    }
    out.push_str("```\n\nDone streaming all 950 generated functions.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("parchment_core_{label}_{nanos}.db"))
    }

    #[test]
    fn core_facade_roundtrips_300_chats_and_search() {
        let db = temp_db("core300");
        let core = ParchmentCore::open(&db).unwrap();
        core.seed_benchmark_dataset(300, 40, 200).unwrap();

        let stubs = core.list_chat_stubs().unwrap();
        assert_eq!(stubs.len(), 300);
        assert_eq!(stubs[0].id, "perf-000");

        let latest = core.load_latest_messages("perf-000", 50).unwrap();
        assert_eq!(latest.len(), 50);

        core.append_message(
            "perf-000",
            "perf-000-live".into(),
            MessageRole::Assistant,
            "UniqueFacadeToken verified.".into(),
            1_800_000_000_000,
        )
        .unwrap();
        let hits = core.search_messages("UniqueFacadeToken", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].message_id, "perf-000-live");

        drop(core);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(db.with_extension("db-wal"));
        let _ = std::fs::remove_file(db.with_extension("db-shm"));
    }

    #[cfg(feature = "network")]
    #[test]
    fn sse_framer_handles_split_chunks_and_crlf() {
        let mut framer = SseFramer::new();
        let mut out = Vec::new();
        framer
            .push_bytes(b": keepalive\r\nevent: delta\r\nda", &mut out)
            .unwrap();
        assert!(out.is_empty());
        framer
            .push_bytes(b"ta: {\"text\":\"hi\"}\r\n\r\ndata: [DONE]\n\n", &mut out)
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].event_type, "delta");
        assert_eq!(out[0].data, "{\"text\":\"hi\"}");
        assert_eq!(out[1].data, "[DONE]");
    }

    #[test]
    fn android_jni_backing_json_and_fts_facade_roundtrips() {
        let db = temp_db("jni_facade");
        let core = ParchmentCore::open(&db).unwrap();

        assert!(core.load_settings_json().unwrap().is_none());
        let snap_json = r#"{"activeSessionId":"c-jni-1","profiles":[{"id":"p1","name":"Test","protocol":"OPENAI_RESPONSES"}]}"#;
        core.save_settings_json(snap_json).unwrap();
        let loaded_snap = core.load_settings_json().unwrap().unwrap();
        assert!(loaded_snap.contains("c-jni-1"));

        let chat_json = r#"{"id":"c-jni-1","title":"JNI Chat","messages":[{"id":"m1","role":"USER","variants":[{"id":"v1","content":"AndroidFtsUniqueNeedle verified"}]}]}"#;
        core.save_chat_json(chat_json).unwrap();
        let loaded_chat = core.load_chat_json("c-jni-1").unwrap().unwrap();
        assert!(loaded_chat.contains("AndroidFtsUniqueNeedle"));

        let ids = core
            .search_chat_ids_newline("AndroidFtsUniqueNeedle", 10)
            .unwrap();
        assert_eq!(ids, "c-jni-1");

        drop(core);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(db.with_extension("db-wal"));
        let _ = std::fs::remove_file(db.with_extension("db-shm"));
    }
}
