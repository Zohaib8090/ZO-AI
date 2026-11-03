//! ZO-AI core. Everything that touches the operating system lives here, so the app and the
//! MCP server share one set of tools, one permission policy and one audit log.

pub mod ai;
pub mod approval;
pub mod audit;
pub mod exec;
pub mod mcp;
pub mod mcp_client;
pub mod policy;
pub mod store;
pub mod tools;

pub use policy::{Decision, Mode, Risk};
pub use tools::{call, definitions, ToolDef};
