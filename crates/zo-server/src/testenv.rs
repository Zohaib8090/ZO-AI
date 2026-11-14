//! Tests only: the sandbox a multi-tenant server needs (one for the whole test run, set up once because the environment is shared by
//! all tests): its own folders for the people, its own socket, and zo-core's service running in this process.

use std::path::PathBuf;
use std::sync::Once;

pub const SECRET: &str = "test-secret-not-a-real-one";

fn base() -> PathBuf {
    PathBuf::from(format!("/tmp/claude-1000/zsrv{}", std::process::id()))
}

/// Sets the environment (once) and returns the folder the people's folders live in.
pub fn env() -> PathBuf {
    static ONCE: Once = Once::new();
    let b = base();
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&b);
        for d in ["tenants", "rt", "data", "config", "home"] {
            std::fs::create_dir_all(b.join(d)).unwrap();
        }
        std::env::set_var("HOME", b.join("home"));
        std::env::set_var("XDG_DATA_HOME", b.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", b.join("config"));
        std::env::set_var("XDG_RUNTIME_DIR", b.join("rt"));
        std::env::set_var("ZO_MULTI_TENANT", "1");
        std::env::set_var("ZO_TENANT_ROOT", b.join("tenants"));
        std::env::set_var("ZO_SERVER_SECRET", SECRET);
        // the update check must never reach GitHub from a test: nothing listens on port 9
        std::env::set_var("ZO_UPDATE_BASE_URL", "http://127.0.0.1:9");
    });
    b.join("tenants")
}

/// The environment, and zo-core's service listening in this process.
pub fn service() -> PathBuf {
    static ONCE: Once = Once::new();
    let root = env();
    ONCE.call_once(|| zo_core::service::start().expect("the service starts in the sandbox"));
    root
}
