//! A small driver for trying the sync by hand (use a sandbox: set HOME, XDG_DATA_HOME and XDG_CONFIG_HOME first).
//!
//!   syncctl chat ID TITLE      add or change a chat (its time is now)
//!   syncctl note TEXT          remember a note
//!   syncctl rmchat ID          delete a chat (leaves a tombstone)
//!   syncctl forget ID          forget a note
//!   syncctl list               what is stored here
//!   syncctl sync URL           one sync round with ZOHARA_AGENT_KEY_SYNC_TOKEN as the access token
//!
//! The desktop's sign-in, as the integration test drives it (ZO_SERVER_URL says which server; ZOHARA_AGENT_KEYS_FILE is the key store):
//!   syncctl login KIND NAME    the browser device flow: prints "CODE <user code>", waits for the approval, saves the sign-in
//!   syncctl syncnow            one round with the saved sign-in (what the background loop does)
//!   syncctl signout            tells the server to revoke this device and forgets the sign-in
//!   syncctl whoami             the signed-in account, or "not signed in"
//!
//! Profiles, MCP servers, preferences and keys (the sync of those kinds):
//!   syncctl profile NAME PROVIDER MODEL BASE_URL PINS   make a profile (PINS: comma separated models)
//!   syncctl pin ID MODEL | unpin ID MODEL | rmprofile ID | profiles
//!   syncctl mcpadd NAME URL | mcplist
//!   syncctl hidden a,b | hiddenlist                     which providers are hidden in the pickers
//!   syncctl key PROVIDER VALUE | keyis PROVIDER VALUE   save a key / is the saved key this value (prints true/false)
//!   syncctl kinds PROFILES MCP KEYS                     what else is synced (true/false each)
use zo_core::store::{chats, memory};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "chat" => {
            let c = chats::Chat { id: arg(1), title: arg(2), updated: chats::now_millis(), entries: vec![chats::Entry { kind: "msg-user".into(), text: arg(2), ..Default::default() }], ..Default::default() };
            chats::upsert(c);
        }
        "note" => println!("{:?}", memory::remember(&arg(1), "fact", "")),
        "rmchat" => chats::delete(&arg(1)),
        "forget" => println!("{}", memory::forget(&arg(1))),
        "list" => {
            for c in chats::load() {
                println!("chat {} \"{}\" updated {}", c.id, c.title, c.updated);
            }
            for i in memory::load().items {
                println!("note {} \"{}\"", i.id, i.text);
            }
            println!("tombstones {:?}", zo_core::sync::tombstones().keys().collect::<Vec<_>>());
        }
        "sync" => {
            let token = zo_core::sync::token().unwrap_or_default();
            match zo_core::sync::sync_with(&arg(1), &token) {
                Ok((pulled, pushed, deleted)) => println!("synced: pulled {pulled}, pushed {pushed}, deleted here {deleted}"),
                Err(e) => println!("sync failed: {e}"),
            }
        }
        "login" => {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let r = zo_core::login::run_device_flow(&arg(2), &arg(1), |code, _url| {
                println!("CODE {code}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }, &cancel);
            match r.and_then(|l| zo_core::login::save_login(&l).map(|_| l)) {
                Ok(l) => println!("LOGGED_IN {}", l.email),
                Err(e) => println!("LOGIN_FAILED {e}"),
            }
        }
        "syncnow" => match zo_core::sync::sync_now() {
            Ok((pulled, pushed, deleted)) => println!("synced: pulled {pulled}, pushed {pushed}, deleted here {deleted}"),
            Err(e) => println!("sync failed: {e}"),
        },
        "signout" => println!("{:?}", zo_core::login::sign_out()),
        "whoami" => println!("{}", zo_core::login::signed_in().unwrap_or_else(|| "not signed in".into())),
        "profile" => {
            use zo_core::ai::{profiles, Selection};
            let mut sel = Selection { provider: arg(2), model: arg(3), ..Default::default() };
            if !arg(4).is_empty() {
                sel.base_urls.insert(arg(2), arg(4));
            }
            match profiles::create(&arg(1), &sel) {
                Ok(p) => {
                    for m in arg(5).split(',').filter(|m| !m.is_empty()) {
                        profiles::pin(&p.id, m);
                    }
                    println!("{}", p.id);
                }
                Err(e) => println!("failed: {e}"),
            }
        }
        "pin" => zo_core::ai::profiles::pin(&arg(1), &arg(2)),
        "unpin" => zo_core::ai::profiles::unpin(&arg(1), &arg(2)),
        "rmprofile" => zo_core::ai::profiles::delete(&arg(1)),
        "profiles" => {
            for p in zo_core::ai::profiles::load() {
                println!("profile {} {} {} {} base={} pinned={}", p.id, p.name, p.provider, p.model, p.base_url.clone().unwrap_or_default(), p.pinned.join(","));
            }
        }
        "mcpadd" => {
            let mut all = zo_core::mcp_client::load_servers();
            all.push(zo_core::mcp_client::ServerConfig { name: arg(1), command: arg(2), enabled: true, ..Default::default() });
            zo_core::mcp_client::save_servers(&all);
        }
        "mcplist" => {
            for s in zo_core::mcp_client::load_servers() {
                println!("mcp {} {} enabled={}", s.name, s.command, s.enabled);
            }
        }
        "hidden" => println!("{:?}", zo_core::ai::ui_prefs::set_hidden(arg(1).split(',').filter(|h| !h.is_empty()).map(str::to_string).collect()).map(|p| p.hidden_providers)),
        "hiddenlist" => println!("hidden {}", zo_core::ai::ui_prefs::load().hidden_providers.join(",")),
        "key" => println!("{:?}", zo_core::ai::keys::set(&arg(1), &arg(2))),
        "keyis" => println!("{}", zo_core::ai::keys::get(&arg(1)).as_deref() == Some(arg(2).as_str())),
        "kinds" => println!("{:?}", zo_core::sync::set_kinds(arg(1) == "true", arg(2) == "true", arg(3) == "true")),
        _ => eprintln!("usage: syncctl chat|note|rmchat|forget|list|sync|login|syncnow|signout|whoami"),
    }
}
