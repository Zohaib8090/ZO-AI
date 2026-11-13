//! Tenants: one person's own corner of a shared server. The context is per thread: `scope` runs code as a tenant, `spawn` starts a
//! thread that keeps the tenant of the thread that started it. Everything that reads or writes ZO-AI's files asks `audit::config_dir`
//! / `audit::data_dir`, which answer with the tenant's own folders.
//!
//! With `ZO_MULTI_TENANT=1` the default folders are never used: code that runs without a tenant gets an unusable path (reads find
//! nothing, writes fail) and is treated as restricted. Without that variable nothing changes for the desktop app.

use std::cell::RefCell;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

/// What a restricted tenant may keep on disk (all its folders together).
pub const MAX_DATA_BYTES: u64 = 25 * 1024 * 1024;
/// The longest message a restricted tenant may send.
pub const MAX_MESSAGE_CHARS: usize = 20_000;
/// Tool-loop steps per answer for a restricted tenant.
pub const MAX_STEPS: usize = 8;
/// Seconds one answer may take for a restricted tenant.
pub const MAX_ANSWER_SECS: u64 = 120;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant {
    /// 32 lowercase hex characters.
    pub id: String,
    pub config: PathBuf,
    pub data: PathBuf,
    pub restricted: bool,
}

thread_local! {
    static CURRENT: RefCell<Option<Tenant>> = const { RefCell::new(None) };
}

/// Is this process a multi-tenant server (`ZO_MULTI_TENANT=1`)?
pub fn multi_tenant() -> bool {
    #[cfg(test)]
    if TEST_MULTI.with(|c| c.get()) {
        return true;
    }
    multi_from(std::env::var("ZO_MULTI_TENANT").ok().as_deref())
}

fn multi_from(v: Option<&str>) -> bool {
    v == Some("1")
}

#[cfg(test)]
thread_local! {
    static TEST_MULTI: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TEST_SECS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Tests only: this thread behaves as in a server started with ZO_MULTI_TENANT=1 (the environment itself is shared by all tests).
#[cfg(test)]
pub(crate) fn test_multi(on: bool) {
    TEST_MULTI.with(|c| c.set(on));
}

/// Tests only: a shorter answer limit on this thread.
#[cfg(test)]
pub(crate) fn test_answer_secs(n: Option<u64>) {
    TEST_SECS.with(|c| c.set(n));
}

/// How long one answer of a restricted tenant may take.
pub fn max_answer() -> std::time::Duration {
    #[cfg(test)]
    if let Some(n) = TEST_SECS.with(|c| c.get()) {
        return std::time::Duration::from_secs(n);
    }
    std::time::Duration::from_secs(MAX_ANSWER_SECS)
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Tenant {
    /// The tenant's folders under `$ZO_TENANT_ROOT/<id>/{config,data}` (mode 0700, made when missing).
    pub fn open(id: &str, restricted: bool) -> Result<Tenant, String> {
        let root = std::env::var_os("ZO_TENANT_ROOT").filter(|r| !r.is_empty()).ok_or("ZO_TENANT_ROOT is not set.")?;
        Tenant::open_in(Path::new(&root), id, restricted)
    }

    pub fn open_in(root: &Path, id: &str, restricted: bool) -> Result<Tenant, String> {
        if !valid_id(id) {
            return Err("That tenant id is not valid.".into());
        }
        let base = root.join(id);
        let (config, data) = (base.join("config"), base.join("data"));
        for d in [&config, &data] {
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(d).map_err(|e| format!("Cannot make the tenant folder: {e}"))?;
        }
        Ok(Tenant { id: id.to_string(), config, data, restricted })
    }
}

struct Restore(Option<Tenant>);

impl Drop for Restore {
    fn drop(&mut self) {
        let prev = self.0.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
    }
}

/// Runs `f` as this tenant on this thread; the previous context comes back afterwards, even after a panic.
pub fn scope<T>(t: &Tenant, f: impl FnOnce() -> T) -> T {
    let prev = CURRENT.with(|c| c.borrow_mut().replace(t.clone()));
    let _restore = Restore(prev);
    f()
}

pub fn current() -> Option<Tenant> {
    if let Some(t) = CURRENT.with(|c| c.borrow().clone()) {
        return Some(t);
    }
    // a program the server started for one tenant (the tools program of Claude Code mode) is told who it works for; it is never
    // restricted, because a restricted tenant cannot start one
    if multi_tenant() {
        static FROM_ENV: std::sync::OnceLock<Option<Tenant>> = std::sync::OnceLock::new();
        return FROM_ENV.get_or_init(|| std::env::var("ZO_TENANT_ID").ok().filter(|i| valid_id(i)).and_then(|id| Tenant::open(&id, false).ok())).clone();
    }
    None
}

/// Must this code run with the small set of safe tools? True for a restricted tenant, and (fail closed) for code that has no
/// tenant at all in a multi-tenant server.
pub fn is_restricted() -> bool {
    match current() {
        Some(t) => t.restricted,
        None => multi_tenant(),
    }
}

/// Starts a thread that works for the same tenant as the thread that calls this.
pub fn spawn<F, T>(f: F) -> std::thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match current() {
        Some(t) => std::thread::spawn(move || scope(&t, f)),
        None => std::thread::spawn(f),
    }
}

/// Where code without a tenant lands in a multi-tenant server: below a file, so nothing can be read or made there.
pub(crate) fn unusable() -> PathBuf {
    PathBuf::from("/dev/null/zo-no-tenant")
}

fn tree_bytes(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => tree_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// What the current tenant keeps on disk, in bytes (config and data together). Zero without a tenant.
pub fn used_bytes() -> u64 {
    current().map(|t| tree_bytes(&t.config) + tree_bytes(&t.data)).unwrap_or(0)
}

pub const FULL_MSG: &str = "Your storage is full (25 MB). Delete some chats or memory notes and try again.";

/// May a restricted tenant replace `file` with `new_len` bytes? Always yes for everyone else.
pub fn fits(file: &Path, new_len: usize) -> bool {
    let Some(t) = current().filter(|t| t.restricted) else { return true };
    let old = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    let used = tree_bytes(&t.config) + tree_bytes(&t.data);
    used.saturating_sub(old) + new_len as u64 <= MAX_DATA_BYTES
}

/// A refusal text when a restricted tenant has no room left for anything new.
pub fn check_room() -> Result<(), String> {
    match current().filter(|t| t.restricted) {
        Some(_) if used_bytes() >= MAX_DATA_BYTES => Err(FULL_MSG.into()),
        _ => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use serde_json::Value;
    use std::io::{Read, Write};

    pub const SECRET: &str = "test-server-secret-not-a-real-one";

    /// Sets the variables a multi-tenant server has (once for the whole test run; the tests that use them hold TEST_ENV_LOCK).
    pub fn env_once() -> PathBuf {
        static ONCE: std::sync::Once = std::sync::Once::new();
        let root = std::env::temp_dir().join(format!("zo-tenant-root-{}", std::process::id()));
        ONCE.call_once(|| {
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            std::env::set_var("ZO_TENANT_ROOT", &root);
            std::env::set_var("ZO_SERVER_SECRET", SECRET);
        });
        root
    }

    /// A fresh tenant that nothing else uses.
    pub fn fresh(restricted: bool) -> Tenant {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(1);
        let id = format!("{:016x}{:016x}", std::process::id() as u64, N.fetch_add(1, Ordering::SeqCst) + 0xabc0_0000);
        Tenant::open_in(&env_once(), &id, restricted).unwrap()
    }

    pub fn chat_reply(text: &str) -> Value {
        serde_json::json!({"choices": [{"message": {"content": text}}]})
    }

    pub fn tool_reply(calls: &[(&str, Value)]) -> Value {
        let calls: Vec<Value> = calls
            .iter()
            .enumerate()
            .map(|(i, (n, a))| serde_json::json!({"id": format!("t{i}"), "type": "function", "function": {"name": n, "arguments": a.to_string()}}))
            .collect();
        serde_json::json!({"choices": [{"message": {"content": null, "tool_calls": calls}}]})
    }

    /// A fake OpenAI-compatible server: answers the given bodies in order, then hands back every request body it saw.
    pub fn fake_model(answers: Vec<Value>) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                let (head_end, len) = loop {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        return seen;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
                        let len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while buf.len() < head_end + len {
                    let n = s.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                }
                seen.push(serde_json::from_slice(&buf[head_end..buf.len().min(head_end + len)]).unwrap_or(Value::Null));
                let body = body.to_string();
                let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            }
            seen
        });
        (url, h)
    }

    /// Makes the tenant use the fake model server as its "custom" provider, with its own key.
    pub fn use_model(t: &Tenant, url: &str) {
        crate::ai::agent::test_allow_local(&t.id);
        scope(t, || {
            let mut sel = crate::ai::Selection { provider: "custom".into(), model: "m".into(), ..Default::default() };
            sel.base_urls.insert("custom".into(), url.to_string());
            sel.save();
            crate::ai::keys::set("custom", "sk-test-bring-your-own-0001").unwrap();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::ai::agent::{self, Config, Event};
    use crate::approval::{Deny, NoQuestions};
    use crate::policy::Mode;
    use crate::store::{chats, memory, personality, TEST_ENV_LOCK};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        let g = TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        env_once();
        g
    }

    fn from_sel(sel: &crate::ai::Selection) -> Result<(), String> {
        Config::from_selection(sel, "s").map(|_| ())
    }

    fn chat(id: &str, text: &str) -> chats::Chat {
        chats::Chat { id: id.into(), title: text.into(), updated: chats::now_millis(), ..Default::default() }
    }

    #[test]
    fn only_a_real_tenant_id_opens_and_the_folders_are_private() {
        let _g = lock();
        for bad in ["", "abc", "../../etc/passwd", &"A".repeat(32), &"g".repeat(32), &"a".repeat(33), &format!("{}/..", "a".repeat(29))] {
            assert!(Tenant::open(bad, true).is_err(), "{bad}");
        }
        let t = Tenant::open(&"ab12".repeat(8), true).unwrap();
        for d in [&t.config, &t.data] {
            let mode = std::fs::metadata(d).unwrap().permissions().mode_bits();
            assert_eq!(mode & 0o777, 0o700, "{d:?}");
        }
        assert!(multi_from(Some("1")) && !multi_from(Some("0")) && !multi_from(None));
    }

    trait ModeBits {
        fn mode_bits(&self) -> u32;
    }
    impl ModeBits for std::fs::Permissions {
        fn mode_bits(&self) -> u32 {
            use std::os::unix::fs::PermissionsExt;
            self.mode()
        }
    }

    #[test]
    fn what_one_tenant_writes_another_cannot_read() {
        let _g = lock();
        let (a, b) = (fresh(true), fresh(true));
        scope(&a, || {
            chats::upsert(chat("c1", "secret chat of A"));
            memory::remember("The person A likes green tea a lot", "preference", "");
            personality::add("Pirate A", "Talk like a pirate, always.", false).unwrap();
            crate::ai::Selection { provider: "openai".into(), model: "a-model".into(), ..Default::default() }.save();
            crate::ai::profiles::create("A profile", &crate::ai::Selection { provider: "openai".into(), model: "x".into(), ..Default::default() }).unwrap();
            crate::ai::keys::set("openai", "sk-A-only-key-1234567890").unwrap();
            crate::store::artifacts::add_code("rust", "fn a() {}", None);
            crate::audit::log("zohara_memory_save", &json!({"text": "A's words"}), "ok");
            assert_eq!(chats::load().len(), 1);
        });
        scope(&b, || {
            assert!(chats::load().is_empty());
            assert!(memory::load().items.is_empty());
            assert!(!personality::load().items.iter().any(|p| p.name == "Pirate A"));
            assert!(crate::ai::Selection::load().provider.is_empty());
            assert!(crate::ai::profiles::load().is_empty());
            assert_eq!(crate::ai::keys::get("openai"), None);
            assert!(crate::store::artifacts::load().is_empty());
            assert!(memory::briefing("green tea").is_empty() || !memory::briefing("green tea").contains("green tea"));
            assert!(std::fs::read_dir(&b.data).unwrap().next().is_none() || !b.data.join("chats.json").exists());
        });
        // and the other way round: the files are in A's folders only
        assert!(a.data.join("chats.json").exists() && !b.data.join("chats.json").exists());
        assert!(!b.config.join("keys.enc").exists() && a.config.join("keys.enc").exists());
        // the log of a restricted person keeps no words
        let log = std::fs::read_to_string(a.data.join("audit.jsonl")).unwrap();
        assert!(log.contains("zohara_memory_save") && !log.contains("A's words"), "{log}");
    }

    #[test]
    fn the_tenant_follows_threads_started_with_spawn_and_not_plain_ones() {
        let _g = lock();
        let a = fresh(true);
        scope(&a, || {
            spawn(|| chats::upsert(chat("from-spawn", "written by a spawned thread"))).join().unwrap();
            let plain = std::thread::spawn(|| (current().is_some(), crate::audit::data_dir())).join().unwrap();
            assert!(!plain.0, "a plain thread has no tenant");
            // nested spawn keeps it too
            spawn(|| spawn(|| memory::remember("Nested spawn note about the person", "fact", "")).join().unwrap()).join().unwrap();
        });
        scope(&a, || {
            assert_eq!(chats::load().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["from-spawn"]);
            assert_eq!(memory::load().items.len(), 1);
        });
        assert!(current().is_none(), "the scope ended");
        // the context is restored after a panic too
        let r = std::panic::catch_unwind(|| scope(&a, || panic!("boom")));
        assert!(r.is_err() && current().is_none());
    }

    #[test]
    fn code_without_a_tenant_on_a_multi_tenant_server_gets_nothing_and_is_restricted() {
        let _g = lock();
        let owner = std::env::temp_dir().join(format!("zo-owner-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&owner);
        std::fs::create_dir_all(&owner).unwrap();
        let old = (std::env::var_os("XDG_DATA_HOME"), std::env::var_os("XDG_CONFIG_HOME"));
        std::env::set_var("XDG_DATA_HOME", owner.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", owner.join("config"));
        let a = fresh(true);
        let result = std::thread::spawn(move || {
            test_multi(true);
            let dirs = (crate::audit::config_dir(), crate::audit::data_dir());
            chats::upsert(chat("leak", "must not be saved"));
            memory::remember("This must not be saved anywhere", "fact", "");
            crate::ai::Selection { provider: "openai".into(), model: "m".into(), ..Default::default() }.save();
            crate::policy::Mode::Bypass.save();
            let key = crate::ai::keys::set("openai", "sk-nowhere");
            let loaded = (chats::load().len(), memory::load().items.len(), crate::ai::Selection::load().provider, crate::ai::keys::get("openai"));
            let tool = crate::tools::call("zohara_fs_write", &json!({"path": "/tmp/zo-no-tenant-must-not-exist", "content": "x"}));
            let shell = crate::tools::call("zohara_shell", &json!({"cmd": "true"}));
            (dirs, loaded, key.is_err(), tool.is_err() && shell.is_err(), is_restricted(), crate::tools::active().len())
        })
        .join()
        .unwrap();
        let (dirs, loaded, key_err, tools_err, restricted, n_tools) = result;
        assert!(dirs.0.starts_with("/dev/null") && dirs.1.starts_with("/dev/null"), "{dirs:?}");
        assert_eq!((loaded.0, loaded.1, loaded.2.as_str(), loaded.3), (0, 0, "", None));
        assert!(key_err && tools_err && restricted && n_tools == crate::tools::RESTRICTED_TOOLS.len());
        assert!(!std::path::Path::new("/tmp/zo-no-tenant-must-not-exist").exists());
        assert!(!owner.join("data").exists() && !owner.join("config").exists(), "the default folders were not touched");
        assert!(!a.data.join("chats.json").exists());
        match old.0 {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old.1 {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&owner);
    }

    #[test]
    fn keys_are_ciphertext_and_only_open_with_the_right_secret_and_tenant() {
        let _g = lock();
        let (a, b) = (fresh(true), fresh(true));
        let plain_key = "sk-live-PLAINTEXT-MARKER-9876543210";
        scope(&a, || crate::ai::keys::set("anthropic", plain_key).unwrap());
        let bytes = std::fs::read(a.config.join(crate::ai::keys::ENC_FILE)).unwrap();
        let hay = String::from_utf8_lossy(&bytes);
        assert!(!hay.contains("PLAINTEXT-MARKER") && !hay.contains("anthropic") && !hay.contains("sk-live"), "the file is ciphertext");
        assert!(bytes.len() > 12 + 16);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(a.config.join("keys.enc")).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(scope(&a, || crate::ai::keys::get("anthropic")).as_deref(), Some(plain_key));
        // the same bytes in B's folder do not open for B, a wrong secret fails, and so does a damaged file
        assert!(crate::ai::keys::decrypt_map(testkit::SECRET, &a.id, &bytes).unwrap().contains_key("anthropic"));
        assert!(crate::ai::keys::decrypt_map("another secret", &a.id, &bytes).is_err());
        assert!(crate::ai::keys::decrypt_map(testkit::SECRET, &b.id, &bytes).is_err());
        let mut bad = bytes.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(crate::ai::keys::decrypt_map(testkit::SECRET, &a.id, &bad).is_err());
        std::fs::write(b.config.join("keys.enc"), &bytes).unwrap();
        assert_eq!(scope(&b, || crate::ai::keys::get("anthropic")), None);
        assert!(scope(&b, || crate::ai::keys::set("openai", "x1")).is_err(), "an unreadable key file is not overwritten");
        assert_eq!(std::fs::read(b.config.join("keys.enc")).unwrap(), bytes);
        // clearing works, and an environment key is never used for a tenant
        scope(&a, || crate::ai::keys::clear("anthropic"));
        assert_eq!(scope(&a, || crate::ai::keys::get("anthropic")), None);
        std::env::set_var("ZOAI_KEY_OPENAI", "sk-from-the-environment");
        let from_env = scope(&a, || crate::ai::keys::get("openai"));
        let on_multi = std::thread::spawn(|| {
            test_multi(true);
            crate::ai::keys::get("openai")
        })
        .join()
        .unwrap();
        std::env::remove_var("ZOAI_KEY_OPENAI");
        assert_eq!((from_env, on_multi), (None, None));
    }

    #[test]
    fn a_restricted_tenant_has_only_its_allowed_tools_and_everything_else_is_refused() {
        let _g = lock();
        let t = fresh(true);
        let admin = fresh(false);
        let mut names: Vec<&str> = crate::tools::definitions().iter().chain(crate::tools::app_definitions().iter()).map(|d| d.name).collect();
        names.extend(["zohara_browser_open", "zohara_does_not_exist", "", "ZOHARA_SHELL", "zohara_shell ", "mcp__x__y", "../zohara_fs_read"]);
        scope(&t, || {
            let mut listed: Vec<&str> = crate::tools::active().iter().map(|d| d.name).collect();
            listed.sort();
            let mut want = crate::tools::RESTRICTED_TOOLS.to_vec();
            want.sort();
            assert_eq!(listed, want);
            assert_eq!(crate::ai::wire::builtin_specs().len(), crate::tools::RESTRICTED_TOOLS.len());
            for n in names.iter().filter(|n| !crate::tools::allowed_for_restricted(n)) {
                let e = crate::tools::call(n, &json!({"path": "/etc/passwd", "cmd": "id", "query": "x"})).expect_err(n);
                assert!(e.contains("not available"), "{n}: {e}");
                // the same through the permission layer, even in Bypass
                assert!(crate::mcp::execute(n, &json!({}), Mode::Bypass, &Deny).is_err(), "{n}");
            }
            // the allowed ones run, and do not ask
            let r = crate::mcp::execute("zohara_memory_save", &json!({"text": "The person lives in Lahore"}), Mode::Manual, &Deny).unwrap();
            assert!(r.contains("Remembered"), "{r}");
            assert!(crate::mcp::execute("zohara_memory_search", &json!({"query": "Lahore"}), Mode::Manual, &Deny).unwrap().contains("Lahore"));
            assert!(crate::mcp::execute("zohara_memory_forget", &json!({"what": "Lahore"}), Mode::Manual, &Deny).unwrap().contains("Forgot"));
            crate::tools::begin_answer(); // a new answer: the notes read above no longer close the web
            assert!(crate::tools::call("zohara_web_search", &json!({"query": "  "})).unwrap_err().contains("empty"), "reaches the real tool, which refuses an empty search");
            assert!(crate::tools::call("zohara_web_fetch", &json!({"url": "http://127.0.0.1:1/"})).is_err());
            assert!(!crate::term::enabled() && !crate::uibridge::available());
            assert!(crate::uibridge::call("zohara_browser_open", &json!({})).is_err());
            assert!(crate::mcp_client::Connection::start(&crate::mcp_client::ServerConfig { name: "x".into(), command: "true".into(), enabled: true, ..Default::default() }).is_err());
            assert!(crate::mcp_client::External::start().tools().is_empty());
            assert!(crate::ai::claude_code::binary().is_none());
        });
        scope(&admin, || {
            assert!(crate::tools::active().iter().any(|d| d.name == "zohara_shell"));
            assert!(crate::tools::call("zohara_shell", &json!({"cmd": "echo admin"})).unwrap().contains("admin"));
        });
    }

    #[test]
    fn claude_code_and_local_or_odd_model_servers_are_refused_for_a_restricted_tenant() {
        let _g = lock();
        let t = fresh(true);
        scope(&t, || {
            let sel = crate::ai::Selection { provider: "claude-code".into(), model: "sonnet".into(), ..Default::default() };
            assert!(from_sel(&sel).unwrap_err().contains("Claude Code"));
            let mut s = None;
            assert!(crate::ai::claude_code::run_in_mode(None, "sonnet", None, &mut s, "hi", "", &std::sync::Arc::new(AtomicBool::new(false)), &mut |_| {}).is_err());
            assert!(crate::ai::claude_code::complete("haiku", "s", "p", &std::sync::Arc::new(AtomicBool::new(false))).is_err());
            for url in ["http://127.0.0.1:11434/v1", "https://127.0.0.1/v1", "https://localhost/v1", "https://10.0.0.5/v1", "https://169.254.169.254/latest", "https://0x7f.1/v1", "https://[::1]/v1", "http://api.openai.com/v1", "https://user:pw@api.openai.com/v1", "https://api.openai.com@127.0.0.1/v1", "https://127.0.0.1\\@api.openai.com/v1", "https://127.1/v1", "https://2130706433/v1", "https://[::ffff:7f00:1]/v1", "https://[::ffff:127.0.0.1]/v1", "https://192.168.1.1/v1", "https://100.64.0.1/v1", "https://intranet/v1", "https://x.internal/v1", "https://nas.local/v1", "ftp://api.openai.com/v1", "", "https://", "https:///v1"] {
                let mut sel = crate::ai::Selection { provider: "custom".into(), model: "m".into(), ..Default::default() };
                sel.base_urls.insert("custom".into(), url.into());
                crate::ai::keys::set("custom", "k").unwrap();
                assert!(from_sel(&sel).is_err(), "{url}");
                assert!(crate::tools::web::public_https_url(url).is_err(), "{url}");
            }
            // a local provider keeps its address on the same machine: refused as well
            let sel = crate::ai::Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
            assert!(from_sel(&sel).is_err());
            assert!(agent::list_models(crate::ai::providers::find("custom").unwrap(), "http://127.0.0.1:1/v1", None).is_err());
            assert!(agent::detect_local().is_empty());
            for ok in ["https://api.openai.com/v1", "https://openrouter.ai/api/v1", "https://example.com:8443/v1/", "https://93.184.216.34/v1", "HTTPS://API.OPENAI.COM/v1"] {
                assert!(crate::tools::web::public_https_url(ok).is_ok(), "{ok}");
            }
            let mut sel = crate::ai::Selection { provider: "openai".into(), model: "gpt".into(), ..Default::default() };
            crate::ai::keys::set("openai", "k").unwrap();
            assert!(from_sel(&sel).is_ok());
            sel.base_urls.insert("openai".into(), "http://127.0.0.1:9/v1".into());
            assert!(from_sel(&sel).is_err(), "a changed address is checked too");
        });
        // the desktop (no tenant, no multi-tenant mode) keeps its local providers
        assert!(!is_restricted());
    }

    #[test]
    fn the_memory_agent_runs_in_the_tenant_of_the_chat_and_one_tenant_cannot_block_another() {
        let _g = lock();
        let (a, b) = (fresh(false), fresh(false));
        let answer = r#"{"add":[{"text":"The person is building a boat","kind":"project"}],"update":[],"delete":[],"summary":"Talked about boats."}"#;
        let (url, asked) = fake_model(vec![chat_reply(answer)]);
        scope(&a, || {
            let mut sel = crate::ai::Selection { provider: "ollama".into(), model: "m".into(), ..Default::default() };
            sel.base_urls.insert("ollama".into(), url.clone());
            sel.save();
        });
        let e = |k: &str, t: &str| chats::Entry { kind: k.into(), text: t.into(), ..Default::default() };
        let es = vec![e("msg-user", "one"), e("msg-ai", "a"), e("msg-user", "two"), e("msg-ai", "a"), e("msg-user", "I am building a boat"), e("msg-ai", "nice")];
        // A's job is busy: B's still starts (its own key), A's second one does not
        assert!(crate::ai::memory_agent::__begin_for_test(&a.id));
        assert!(!crate::ai::memory_agent::__begin_for_test(&a.id));
        assert!(crate::ai::memory_agent::__begin_for_test(&b.id));
        crate::ai::memory_agent::__end_for_test(&a.id);
        crate::ai::memory_agent::__end_for_test(&b.id);
        scope(&a, || crate::ai::memory_agent::after_turn("c1", "Boats", &es));
        let wait = |t: &Tenant| {
            for _ in 0..200 {
                if !scope(t, crate::ai::memory_agent::is_running) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        };
        wait(&a);
        assert!(asked.join().unwrap()[0].to_string().contains("I am building a boat"));
        // what the spawned thread wrote is in A's folder, not in B's and not in the default one
        assert!(scope(&a, || memory::load()).items.iter().any(|i| i.text.contains("boat")));
        assert!(scope(&b, || memory::load()).items.is_empty());
        assert!(a.data.join("memory.json").exists() && !b.data.join("memory.json").exists());
        assert!(!crate::ai::memory_agent::any_running());
    }

    #[test]
    fn a_restricted_tenant_cannot_fill_the_disk_past_25_mb() {
        let _g = lock();
        let t = fresh(true);
        let other = fresh(false);
        scope(&t, || {
            chats::upsert(chat("ok", "fits"));
            assert_eq!(chats::load().len(), 1);
            std::fs::write(t.data.join("junk.bin"), vec![0u8; (MAX_DATA_BYTES - 1000) as usize]).unwrap();
            // a write that would go over is dropped, what is there stays
            let mut big = chat("big", "x");
            big.entries = vec![chats::Entry { kind: "msg-user".into(), text: "y".repeat(5000), ..Default::default() }];
            chats::upsert(big);
            assert_eq!(chats::load().len(), 1, "the chat that does not fit was not written");
            std::fs::write(t.data.join("junk.bin"), vec![0u8; MAX_DATA_BYTES as usize]).unwrap();
            let e = crate::tools::call("zohara_memory_save", &json!({"text": "The person likes tea"})).unwrap_err();
            assert!(e.contains("storage is full"), "{e}");
            assert!(crate::ai::keys::set("openai", "k").unwrap_err().contains("storage is full"));
            assert!(check_room().is_err() && !fits(&t.data.join("x"), 10));
            assert!(memory::load().items.is_empty());
        });
        // someone who is not restricted has no such limit
        scope(&other, || {
            std::fs::write(other.data.join("junk.bin"), vec![0u8; (MAX_DATA_BYTES + 10) as usize]).unwrap();
            assert!(check_room().is_ok() && fits(&other.data.join("x"), 1 << 20));
        });
    }

    fn cfg_for(url: &str) -> Config<'static> {
        Config { provider: crate::ai::providers::find("custom").unwrap(), base_url: url.into(), model: "m".into(), key: Some("k".into()), system: agent::SYSTEM_PROMPT.into(), vision: true }
    }

    #[test]
    fn a_restricted_answer_stops_after_8_steps_and_is_told_it_has_no_computer() {
        let _g = lock();
        let t = fresh(true);
        crate::ai::agent::test_allow_local(&t.id);
        let call = tool_reply(&[("zohara_web_search", json!({"query": ""}))]);
        let (url, seen) = fake_model(vec![call; 8]);
        let mut hist = Vec::new();
        let r = scope(&t, || agent::run(&cfg_for(&url), &mut hist, "look something up", Mode::Bypass, &Deny, &NoQuestions, &AtomicBool::new(false), &mut |_| {}));
        let e = r.unwrap_err();
        assert!(e.contains("8 steps"), "{e}");
        let seen = seen.join().unwrap();
        assert_eq!(seen.len(), 8);
        let system = seen[0]["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains("no access to the person's computer") && !system.contains("Waydroid") && !system.contains("zohara_ss"), "{system}");
        let tools: Vec<&str> = seen[0]["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
        // (zohara_ask_user is left out: nobody can be asked by this run)
        assert_eq!(tools.len(), crate::tools::RESTRICTED_TOOLS.len() - 1, "{tools:?}");
        assert!(tools.iter().all(|n| crate::tools::RESTRICTED_TOOLS.contains(n)));
        // not restricted: the old limit of 15 and the full prompt
        let admin = fresh(false);
        let (url, seen) = fake_model(vec![tool_reply(&[("zohara_system", json!({}))]); 15]);
        let mut hist = Vec::new();
        let mut c = cfg_for(&url);
        c.provider = crate::ai::providers::find("ollama").unwrap();
        let e = scope(&admin, || agent::run(&c, &mut hist, "x", Mode::Bypass, &Deny, &NoQuestions, &AtomicBool::new(false), &mut |_| {})).unwrap_err();
        assert!(e.contains("15 steps"), "{e}");
        assert!(seen.join().unwrap()[0]["messages"][0]["content"].as_str().unwrap().contains("Waydroid"));
    }

    #[test]
    fn a_restricted_message_may_not_be_too_long_and_pictures_are_never_read_from_the_disk() {
        let _g = lock();
        let t = fresh(true);
        crate::ai::agent::test_allow_local(&t.id);
        let long = "x".repeat(MAX_MESSAGE_CHARS + 1);
        let e = scope(&t, || agent::run(&cfg_for("http://127.0.0.1:1/v1"), &mut Vec::new(), &long, Mode::Auto, &Deny, &NoQuestions, &AtomicBool::new(false), &mut |_| {})).unwrap_err();
        assert!(e.contains("too long"), "{e}");
        // a picture path in the message is sent as text only
        let png = std::env::temp_dir().join(format!("zo-secret-pic-{}.png", std::process::id()));
        std::fs::write(&png, b"\x89PNG not really").unwrap();
        let (url, seen) = fake_model(vec![chat_reply("ok")]);
        scope(&t, || agent::run(&cfg_for(&url), &mut Vec::new(), &format!("look at {}", png.display()), Mode::Auto, &Deny, &NoQuestions, &AtomicBool::new(false), &mut |_| {})).unwrap();
        let body = seen.join().unwrap()[0].to_string();
        assert!(!body.contains("image_url") && !body.contains("base64"), "{body}");
        let _ = std::fs::remove_file(png);
    }

    #[test]
    fn a_restricted_answer_that_takes_too_long_is_stopped() {
        let _g = lock();
        let t = fresh(true);
        crate::ai::agent::test_allow_local(&t.id);
        // a server that takes the request and never answers
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", l.local_addr().unwrap());
        let hold = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(4));
            drop(s);
        });
        test_answer_secs(Some(1));
        let started = std::time::Instant::now();
        let e = scope(&t, || agent::run(&cfg_for(&url), &mut Vec::new(), "hi", Mode::Auto, &Deny, &NoQuestions, &AtomicBool::new(false), &mut |_: Event| {})).unwrap_err();
        test_answer_secs(None);
        assert!(e.contains("longer than 1 seconds"), "{e}");
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        hold.join().unwrap();
    }
}
