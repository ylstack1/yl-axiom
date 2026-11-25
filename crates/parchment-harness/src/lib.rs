//! YL Axiom v3 21-tool agent harness, workspace security perimeter, SSRF-pinned public web
//! transport, run-scoped approval broker, MCP client/registry, `.docx`/card codecs, and runner.

#[cfg(feature = "network")]
pub mod approval;
pub mod codecs;
pub mod math_dice;
#[cfg(feature = "network")]
pub mod mcp;
pub mod path_guard;
#[cfg(feature = "network")]
pub mod permissions;
#[cfg(feature = "network")]
pub mod runner;
#[cfg(feature = "network")]
pub mod ssrf;
#[cfg(feature = "network")]
pub mod tools;

#[cfg(feature = "network")]
pub use approval::{
    ToolApprovalBroker, ToolApprovalDecision, ToolApprovalRequest, ToolAuthorization,
    ToolExecutionPermit, UserQuestion,
};
pub use codecs::{
    CardImportResult, encode_rgba_to_png_bytes, export_card_to_png_bytes, extract_docx_text,
    extract_md_skills_from_zip, import_card_or_prompt_bytes,
};
pub use math_dice::{eval_math_expression, roll_dice_expression};
#[cfg(feature = "network")]
pub use mcp::{
    McpClient, McpRegistry, McpServerState, McpServerStatus, McpToolInfo, McpToolResult,
};
pub use path_guard::WorkspacePathGuard;
#[cfg(feature = "network")]
pub use permissions::{
    AgentPermissionGate, HarnessToolDefinition, MCP_SCHEMA_TOOL_NAME, MCP_TOOL_PREFIX,
    ToolAccessScope, ToolCatalog, ToolEffect,
};
#[cfg(feature = "network")]
pub use runner::{AgentTurnRunner, RunActivity, RunProgressEvent, format_elapsed_whole_seconds};
#[cfg(feature = "network")]
pub use ssrf::{PublicWebTransport, convert_html_to_readable_text, is_public_ip};
#[cfg(feature = "network")]
pub use tools::{
    ComputerUseAction, ComputerUseBackend, HarnessContext, HarnessExecutor, SubagentDelegate,
    ToolOutput,
};
