# YL Axiom — Risks and Blockers

## Highest-risk architectural decisions (require validation before implementation)

### R-1: SQLite schema extension for coding tasks
**Risk:** Adding coding task tables must be non-destructive and must not break the existing v2→v3 migration or the `SCHEMA_VERSION = 1` contract.
**Validation:** Before Phase 3, design migration to bump schema to v2 additive-only; verify existing migrator still idempotent; test round-trip with existing chat data.
**Mitigation:** Additive tables only; no ALTER of existing tables; version bump documented.

### R-2: CodingTaskCoordinator is a thin wrapper, not a replacement
**Risk:** The coordinator must not duplicate or replace `AgentTurnRunner`, `HarnessExecutor`, `ToolApprovalBroker`, or `AgentPermissionGate`.
**Validation:** Before Phase 4, confirm coordinator delegates per-turn model interaction to `AgentTurnRunner` and only owns task lifecycle/plan/verification.
**Mitigation:** Code review enforces delegation; no duplicate tool dispatch or permission authority.

### R-3: Android coding execution fallback
**Risk:** Android lacks stdio MCP, unsandboxed shell, and unrestricted desktop subprocesses. Coding features depending on these must degrade gracefully.
**Validation:** Before Phase 9, capability discovery reports available features; UI hides/disables unsupported actions; no Termux/PRoot/embedded Linux.
**Mitigation: Platform adapter returns capability set; coding tasks on Android limited to what AndroidOwnedShell + SAF support.

### R-4: Rebrand must not break package compatibility
**Risk:** Renaming packageId, JNI class `ParchmentNativeCore`, storage IDs, or migration keys breaks existing users, deep links, storage, and JNI linkage.
**Validation:** Before Phase 1, confirm `io.parchment.ai` packageId, JNI symbol names, `v2_migration_v1` key, SQLite schema, and `settings.json`/`chat.json` layout are unchanged.
**Mitigation:** Rebrand limited to user-visible strings/assets; internal identifiers preserved.

### R-5: fastshell stays optional and single-authority
**Risk:** The optional Android shell backend could pull LGPL crates (malachite via crates.io), inflate the 1.85 MB `.so` beyond budget, or create a second permission authority through its `PERMISSION_NEEDED` protocol.
**Validation:** Before Phase 9, measure `.so` size with and without the feature; confirm the permission protocol is bridged into `ToolApprovalBroker` only; confirm `WorkspacePathGuard` still governs paths.
**Mitigation:** Feature off by default; vendor with Apache-2.0 shims or pin a permissive build; reject `network_ask_permission` as an independent grant.

### R-6: Tool profiles must not bypass permissions
**Risk:** A discovery meta-tool (`call_tool`) could be used to invoke tools the profile hides, bypassing approval.
**Validation:** Before Phase 4b, prove `call_tool` traverses `AgentPermissionGate` + `ToolApprovalBroker` + `ToolExecutionPermit::claim` identically to a direct call.
**Mitigation:** Single dispatch path in `HarnessExecutor`; "hidden" means unadvertised only.

## Other risks

### Shell unsandboxed (known gap from TODO.md)
- Shell is unsandboxed on desktop. Do not weaken for coding; keep `ShellAccess` default Disabled, `WorkspacePathGuard` enforcement.

### Android MCP HTTP-only (known gap)
- stdio MCP unavailable on Android. Subagents requiring stdio MCP degrade gracefully.

### Secrets stored plaintext
- API keys in settings JSON/SQLite. Known limitation; document; consider encrypted-at-rest in future.

### JNI raw pointer handle
- `ParchmentCore` handle is a raw pointer cached per storage dir. Must remain valid across Kotlin/Rust lifecycle boundaries; test handle lifecycle.

### Build budget (AGENTS.md)
- At most one narrow check per completed batch; no repeated/chained builds. Plan verification carefully.

## Blockers
- None currently. All Phase 0 work complete. Phase 1 can begin.

## Status
Proposed. Risks to be validated as phases proceed.
