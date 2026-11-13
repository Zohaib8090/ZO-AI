//! zo-mcp: the Zohara OS tools as a stdio MCP server. Point any MCP client at this binary.
//! Risky tools ask in the Zohara Agent app (see zo-core::approval); with the app closed they are refused.

use std::io::{stdin, stdout};
use zo_core::{approval::AppApprover, mcp};

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("zo-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    mcp::serve(stdin().lock(), stdout().lock(), &AppApprover);
}
