//! A small driver for trying the sync by hand (use a sandbox: set HOME, XDG_DATA_HOME and XDG_CONFIG_HOME first).
//!
//!   syncctl chat ID TITLE      add or change a chat (its time is now)
//!   syncctl note TEXT          remember a note
//!   syncctl rmchat ID          delete a chat (leaves a tombstone)
//!   syncctl forget ID          forget a note
//!   syncctl list               what is stored here
//!   syncctl sync URL           one sync round with ZOAI_KEY_SYNC_TOKEN as the access token
//!
//! The desktop's sign-in, as the integration test drives it (ZO_SERVER_URL says which server; ZO_AI_KEYS_FILE is the key store):
//!   syncctl login KIND NAME    the browser device flow: prints "CODE <user code>", waits for the approval, saves the sign-in
//!   syncctl syncnow            one round with the saved sign-in (what the background loop does)
//!   syncctl signout            tells the server to revoke this device and forgets the sign-in
//!   syncctl whoami             the signed-in account, or "not signed in"
use zo_core::store::{chats, memory};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    match arg(0).as_str() {
        "chat" => {
            let c = chats::Chat { id: arg(1), title: arg(2), updated: chats::now_millis(), entries: vec![chats::Entry { kind: "msg-user".into(), text: arg(2) }], ..Default::default() };
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
        _ => eprintln!("usage: syncctl chat|note|rmchat|forget|list|sync|login|syncnow|signout|whoami"),
    }
}
