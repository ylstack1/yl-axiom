//! Pure domain models and invariants for Parchment v3.
//!
//! Serde JSON representations match Parchment v2's `kotlinx.serialization` schema field-for-field
//! so v2 `settings.json`, `chats/<id>/chat.json`, `live.json`, and `.parchment` archives round-trip
//! cleanly with zero data or signature loss.

pub mod chat;
pub mod import_policy;
pub mod permissions;
pub mod provider;
pub mod settings;
pub mod usage;

pub use chat::*;
pub use import_policy::*;
pub use permissions::*;
pub use provider::*;
pub use settings::*;
pub use usage::*;
