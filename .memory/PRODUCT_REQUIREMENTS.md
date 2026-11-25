# YL Axiom — Product Requirements

## Scope
YL Axiom provides three modes: Assistant, Chat, and Axiom Coding. The Coding environment is a first-class capability supporting autonomous software engineering.

## Axiom Coding capabilities (target state)
- Autonomous coding tasks with persistent task lifecycle and state.
- Repository understanding and codebase exploration (tree-sitter symbol/outline/AST; LSP optional).
- Multi-step planning with dependency-aware subtasks.
- Test-driven execution by default: Red (write failing tests) → Green (implement) → Refactor.
- File creation, editing, patch management, and diff review.
- Shell and development tool execution (build, test, lint) with hook-based verification gates.
- Error diagnosis and bounded recovery.
- Task checkpoints and resumption.
- Subagents and coordinated workflows (max 4 parallel, consistent with existing harness).
- Code review and verification — completion based on verified results (passing suite), not model claims.
- Context management (stable cached prefix, quantized compaction, tool-result clearing) and output artifacts (disk-spilled large outputs).
- Skills, MCP, and extensible integrations.

## Preservation requirements (Parchment foundation)
- Existing Assistant functionality preserved unchanged.
- Existing Chat behavior, tools, sessions, provider integrations preserved.
- No unnecessary rewrite of working components.
- Existing configuration, storage, user data, migration compatibility preserved.
- Shared infrastructure supports all three modes without duplication.

## Non-goals for this phase
- No Termux, PRoot, or embedded Linux distributions on Android.
- No Node.js/web stack replacing native functionality.
- No unnecessary plugin framework; clean extension points only.
- No blind rename that damages package IDs, migrations, or integrations.
- No hardcoded providers, API keys, URLs, paths, model names, or platform assumptions.
- No GPL code (aacode-rs is reference-only); no second permission authority from any optional backend.

## Architecture constraints
- Shared Rust agent brain, contracts, task lifecycle, orchestration, and core logic across Windows and Android.
- Platform adapters handle filesystem, shell, process execution, permissions, and available dev tools differences.
- Capability discovery and graceful fallback; no pretending every Windows capability exists on Android.
- Stable Rust module boundaries, optional Cargo features, runtime capability registration.
- Versioned interfaces where necessary.

## Native cross-platform requirements
### Windows
- Rust backend, existing GPUI desktop architecture where appropriate.
- Native Windows filesystem access, process execution, PowerShell integration.
- Git, dev tools, project discovery.
- Proper cancellation, process management, permissions.
- Native desktop interaction and file dialogs.

### Android
- Native Kotlin and Jetpack Compose UI.
- Shared Rust agent engine through JNI.
- Android-compatible native libraries and build pipeline.
- Native filesystem integration, Storage Access Framework (SAF) where appropriate.
- Android permissions and lifecycle handling.
- Background task and cancellation behavior.
- Android-compatible execution capabilities: the current limited `AndroidOwnedShell` is the default; an optional Apache-2.0 embedded shell backend (fastshell) may be enabled behind a feature flag, with its permission protocol bridged into the single `ToolApprovalBroker`.

## Rebranding scope
- Application name, desktop window title, Android application label.
- Icons and app assets, splash screen.
- Sidebar and navigation, settings and About screens, onboarding.
- User-facing descriptions, empty states, error messages, notifications.
- Documentation, README, project metadata, package metadata, build/release artifacts.
- Internal product terminology where appropriate.

Distinguish: user-visible branding vs internal crate/module identifiers vs historical attribution vs license/copyright vs migration-sensitive storage IDs vs external integration identifiers.

## Design direction
Professional, minimal, dark-first, modern and polished, excellent information hierarchy, subtle motion, consistent spacing and typography, mobile-first Android, efficient desktop workspace, accessible contrast, readable code. Avoid clutter, unnecessary animations, hacker/gaming aesthetics, superficial dashboards.

## Security, reliability, performance
- Workspace confinement, symlink/path traversal protection, command execution permissions.
- Tool approval boundaries, network/SSRF protection, secrets/API key storage, MCP server permissions.
- Process isolation, resource limits, cancellation propagation, concurrent tool execution, subagent permissions.
- Android sandbox limitations, data migration and corruption recovery.
- Stream model output and agent events, avoid blocking UI threads, bounded concurrency, artifacts for large outputs.
- Optimize Android memory/CPU, preserve Windows responsiveness, avoid unnecessary native deps/binary size.
