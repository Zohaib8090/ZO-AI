//! A small driver for trying the sync by hand (use a sandbox: set HOME, XDG_DATA_HOME and XDG_CONFIG_HOME first).
//!
//!   syncctl chat ID TITLE      add or change a chat (its time is now)
//!   syncctl note TEXT          remember a note
//!   syncctl rmchat ID          delete a chat (leaves a tombstone)
//!   syncctl forget ID          forget a note
//!   syncctl list               what is stored here
//!   syncctl sync URL           one sync round with ZOAI_KEY_SYNC_TOKEN as the access token
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
        _ => eprintln!("usage: syncctl chat|note|rmchat|forget|list|sync"),
    }
}
