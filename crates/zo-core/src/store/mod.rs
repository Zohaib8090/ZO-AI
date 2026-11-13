//! What Zohara Agent keeps on disk: projects, saved chats and scheduled tasks. The app, the chat AI and the MCP server all read and
//! write the same files, so what one makes the others see.

pub mod artifacts;
pub mod chats;
pub mod machines;
pub mod memory;
pub mod personality;
pub mod projects;
pub mod schedules;

/// Tests that point XDG_DATA_HOME at a throw-away folder must not run at the same time.
#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
