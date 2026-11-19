//! Embedded SQLite WAL + FTS5 persistence, non-destructive v2 migrator, and atomic archive codec.

pub mod archive;
pub mod migrate_v2;
pub mod schema;
pub mod store;

pub use archive::{
    ChatArchiveLimits, decode_chat_sessions_from_raw_json, export_chats_to_zip,
    import_chat_file_bytes,
};
pub use migrate_v2::{V2MigrationReport, migrate_v2_directory};
pub use store::{ParchmentStore, SearchHit};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("archive rejected: {0}")]
    Archive(String),
    #[error("invalid identifier: {0}")]
    InvalidId(String),
}

pub type StorageResult<T> = Result<T, StorageError>;
