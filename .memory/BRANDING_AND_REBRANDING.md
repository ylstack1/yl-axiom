# YL Axiom — Branding and Rebranding Plan

## Identity
- Product name: YL Axiom
- Brand: YL Stack
- Product category: Native AI software-engineering application
- Coding environment: Axiom Coding
- Main navigation: Assistant, Chat, Axiom Coding

## Rebranding categories (do not blindly rename everything)

| Category | Action | Examples |
|---|---|---|
| 1. User-visible branding | Change | App name, window title, Android label, icons, splash, sidebar, settings/About, onboarding, descriptions, empty states, errors, notifications |
| 2. Internal crate/module identifiers | PRESERVE | `parchment-*` crate names, module paths, Rust symbols |
| 3. Historical attribution | PRESERVE | Git history, CHANGELOG, original author credits, third-party notices |
| 4. License / copyright | PRESERVE | MIT LICENSE, copyright headers, third-party-notices |
| 5. Migration-sensitive storage IDs | PRESERVE | `v2_migration_v1` marker, SQLite schema, `settings.json`, `chat.json`, `live.json`, `.parchment` archive IDs |
| 6. External integration identifiers | PRESERVE | `io.parchment.ai` applicationId, file provider authority, MCP server names, OTA repo URL |

## Files to change (user-visible branding)
- `crates/parchment-desktop/src/strings.rs`: `APP_NAME = "Parchment"` → `"YL Axiom"`; `window_title_for_*` functions.
- `mobile/compose-app/src/commonMain/kotlin/io/parchment/ai/strings/Strings.kt`: user-facing strings.
- `mobile/compose-app/src/androidMain/AndroidManifest.xml`: `android:label`, app name.
- `mobile/android-app/build.gradle.kts`: `applicationId` PRESERVE (`io.parchment.ai`); version/release metadata update.
- `README.md`: product name, descriptions, screenshots, download links.
- `docs/*.md`: update product references.
- `installer/`: `windows-installer.iss` app name, `release-badge.svg`, `parchment-icon.ico`.
- `.release-please-manifest.json` / `release-please-config.json`: version/release naming.
- `CHANGELOG.md`: PRESERVE history; add new entries under YL Axiom going forward.

## Files NOT to change (internal / migration / external)
- Crate names (`parchment-core`, `parchment-harness`, etc.) — internal identifiers.
- `io.parchment.ai` applicationId — package compatibility / deep links / storage.
- `ParchmentNativeCore` JNI class name — JNI method symbol linkage (`Java_io_parchment_ai_platform_ParchmentNativeCore_*`).
- `v2_migration_v1` key, SQLite schema, chat/settings JSON schema — migration compatibility.
- `PARCHMENT_APP_VERSION` / `APP_VERSION` const — internal version constant (can add a separate product display version).
- MIT LICENSE, copyright headers, third-party notices — historical/legal.
- Git history, CHANGELOG prior entries — historical attribution.
- MCP server names, OTA repo URL — external integrations.

## Centralized string configuration
- User-visible strings live in `strings.rs` (Rust desktop) and `Strings.kt` (Kotlin). Rebrand by editing these tables, not inline strings. New strings added to these tables.

## Brand assets
- Do not invent final logo assets or permanently lock visual identity during planning. Icons/splash planned but placeholder-ready; assets can be swapped later via centralized resource paths.

## Migration strategy
- Existing Parchment users migrating to YL Axiom: same `io.parchment.ai` packageId, same SQLite schema, same `settings.json`/`chat.json` layout → seamless upgrade, non-destructive.
- App display name changes but storage IDs unchanged → no data loss, no re-migration.

## Rollback safety
- Rebrand changes are string/resource-level; reversible via git. No code logic changes tied to naming.

## Status
Proposed. Not yet implemented.
