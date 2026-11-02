//! Try Claude Code mode from the terminal: claude-code-try "your message" [model]
use zo_core::ai::agent::Event;

fn main() {
    let mut args = std::env::args().skip(1);
    let prompt = args.next().expect("give a message");
    let model = args.next().unwrap_or_else(|| "haiku".into());
    let mut session = None;
    let r = zo_core::ai::claude_code::run(&model, &mut session, &prompt, &mut |e| match e {
        Event::Text(t) => println!("AI: {t}"),
        Event::ToolStart { name, .. } => println!("-> {name}"),
        Event::ToolDone { ok, output, .. } => println!("   {} {}", if ok { "ok" } else { "FAILED" }, output.lines().next().unwrap_or("")),
    });
    println!("result: {r:?}  session: {session:?}");
}
