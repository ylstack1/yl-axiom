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

/// Canonical Parchment application version resolved at build time from
/// `PARCHMENT_APP_VERSION` (CI override) or `.release-please-manifest.json`.
pub const APP_VERSION: &str = env!("PARCHMENT_APP_VERSION");

#[cfg(test)]
mod version_tests {
    use super::APP_VERSION;

    #[test]
    fn app_version_is_valid_semver() {
        let core = APP_VERSION.split('-').next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(parts.len(), 3, "unexpected APP_VERSION: {APP_VERSION}");
        for p in parts {
            assert!(
                p.parse::<u32>().is_ok(),
                "non-numeric part in {APP_VERSION}"
            );
        }
    }
}
