//! LLM wire protocols, zero-copy SSE framing, opaque signatures, retry/error policy,
//! multi-dialect usage/cost accounting, live model registry sync, and `o200k_base` tokenizer.

pub mod client;
pub mod error_policy;
pub mod history;
pub mod registry;
pub mod signatures;
pub mod sse;
pub mod tokenizer;
pub mod usage;
pub mod wire;

pub use client::{ProtocolClient, RoundExecutionResult, StreamCallbacks};
pub use error_policy::{ProtocolError, ProviderHttpError, RunErrorKind};
pub use registry::{RegistryCatalogClient, RegistryEndpoints};
pub use sse::{SseFrame, SseFramer};
pub use tokenizer::{ContextWindowBreakdown, O200kTokenizer};
pub use usage::{ProviderUsageLedger, RequestCostEstimate, UsageDialect};
pub use wire::{StreamEvent, ToolDefinition, WireDialect, WireRequest};
