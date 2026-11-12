//! ZO-AI core. Everything that touches the operating system lives here, so the app and the
//! MCP server share one set of tools, one permission policy and one audit log.

pub mod activity;
pub mod ai;
pub mod approval;
pub mod audit;
pub mod exec;
pub mod link;
pub mod login;
pub mod mcp;
pub mod mcp_client;
pub mod mcp_oauth;
pub mod policy;
pub mod service;
pub mod store;
pub mod sync;
pub mod tenant;
pub mod term;
pub mod uibridge;
pub mod voice;
pub mod tools;

pub use policy::{Decision, Mode, Risk};
pub use tools::{call, definitions, ToolDef};
