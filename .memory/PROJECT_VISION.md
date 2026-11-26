# YL Axiom — Product Vision

## Product identity
- Product name: YL Axiom
- Brand: YL Stack
- Product category: Native AI software-engineering application
- Coding environment: Axiom Coding
- Main navigation: Assistant, Chat, Axiom Coding

## Mission
Build YL Axiom as a professional, native, cross-platform AI software-engineering application. It is not a demo, prototype, simple chat app, or UI-only rebrand.

YL Axiom provides a powerful AI workspace with three first-class modes:

1. **Assistant** — general AI assistance.
2. **Chat** — conversational AI with existing Parchment behavior, tools, sessions, and provider integrations.
3. **Axiom Coding** — autonomous software engineering: coding, planning, execution, testing, debugging, and review.

The Coding environment must be a first-class product capability, not merely another chat screen with a different system prompt.

## Guiding principle
Reuse and improve the existing Parchment foundation as much as technically possible instead of rebuilding working systems from scratch. No blind rewrites. No unnecessary scratch development. No speculative architecture presented as existing functionality.

## Repository origin
YL Axiom is an independent fork of Parchment v3.1.1 (https://github.com/baros4294/parchment), now hosted as the YL Axiom product. The upstream origin is preserved as `ylstack1/yl-axiom`.

## Stable contracts (inherited from Parchment)
- Rust 1.100.0, edition 2024, 8 workspace crates.
- MIT license.
- CamelCase JSON / SQLite schema parity with YL Axiom v2 for migration compatibility.
- Windows: native Rust GPUI desktop shell + Win32 DWM/COM, file dialogs, process management.
- Android: Kotlin Multiplatform / Compose Foundation + shared Rust core via direct zero-JNA JNI (`arm64-v8a` `libparchment_core.so`).
- SQLite WAL + FTS5 storage with non-destructive v2 migration.
- 21 built-in agent tools, MCP client/registry, Skills, subagent delegation (max 4), ToolApprovalBroker, WorkspacePathGuard, SSRF-pinned public web transport.
- AGENTS.md owns rules, rigor, and system contracts; do not weaken them.
