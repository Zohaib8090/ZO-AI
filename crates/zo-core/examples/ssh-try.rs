//! Real SSH test through the tool call function: ssh-try <port> <good-key-file> <wrong-key-file>
//! Run it with HOME, XDG_* and ZOHARA_AGENT_KEYS_FILE pointing into a throw-away folder.
use serde_json::json;
use zo_core::store::machines::{self, Machine};

fn show(what: &str, r: Result<String, String>) {
    println!("== {what}\n{}\n", match r { Ok(s) => format!("OK: {s}"), Err(e) => format!("ERR: {e}") });
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let port: u16 = a[1].parse().unwrap();
    let (good, wrong) = (std::fs::read_to_string(&a[2]).unwrap(), std::fs::read_to_string(&a[3]).unwrap());
    let m = |name: &str| Machine { name: name.into(), host: "127.0.0.1".into(), user: "zotest".into(), port, note: "throwaway container".into() };
    machines::add(m("box"), Some(&good)).unwrap();
    machines::add(m("badkey"), Some(&wrong)).unwrap();
    let c = zo_core::call;
    show("zohara_ssh_machines", c("zohara_ssh_machines", &json!({})));
    show("run: id; hostname", c("zohara_ssh_run", &json!({"machine": "box", "command": "id -un; hostname; echo to-stderr >&2; exit 3"})));
    show("mkdir", c("zohara_ssh_run", &json!({"machine": "box", "command": "mkdir -p ~/'my notes'"})));
    show("write_file with nasty path and content", c("zohara_ssh_write_file", &json!({"machine": "box", "path": "~/my notes/it's $(echo pwned).txt", "content": "line one\nit's `x` $(y) \"z\"\n"})));
    show("append", c("zohara_ssh_write_file", &json!({"machine": "box", "path": "~/my notes/it's $(echo pwned).txt", "content": "appended\n", "append": true})));
    show("read_file same path", c("zohara_ssh_read_file", &json!({"machine": "box", "path": "~/my notes/it's $(echo pwned).txt"})));
    show("ls of the home (no 'pwned' file made by the $())", c("zohara_ssh_run", &json!({"machine": "box", "command": "ls -A ~; ls ~/'my notes'"})));
    show("read missing file", c("zohara_ssh_read_file", &json!({"machine": "box", "path": "/nope/missing"})));
    show("big output is cut", c("zohara_ssh_run", &json!({"machine": "box", "command": "head -c 100000 /dev/zero | tr '\\0' x"})).map(|s| format!("{} chars, tail: {:?}", s.len(), &s[s.len().saturating_sub(70)..])));
    let t = std::time::Instant::now();
    show("timeout (sleep 30 with timeout_s 2)", c("zohara_ssh_run", &json!({"machine": "box", "command": "sleep 30", "timeout_s": 2})));
    println!("   took {:.1}s", t.elapsed().as_secs_f32());
    show("process left behind after timeout? (local ssh processes)", std::process::Command::new("sh").arg("-c").arg("pgrep -x ssh | wc -l").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).map_err(|e| e.to_string()));
    show("wrong key", c("zohara_ssh_run", &json!({"machine": "badkey", "command": "id"})));
    show("unknown machine", c("zohara_ssh_run", &json!({"machine": "ghost", "command": "id"})));
    show("service-style test", zo_core::tools::ssh::test("box"));
    let rt = std::env::var("XDG_RUNTIME_DIR").unwrap();
    println!("leftover key files: {:?}", std::fs::read_dir(format!("{rt}/zohara-agent")).map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>()));
    println!("machines.json:\n{}", std::fs::read_to_string(format!("{}/zohara-agent/machines.json", std::env::var("XDG_CONFIG_HOME").unwrap())).unwrap());
}
