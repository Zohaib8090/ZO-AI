# Zohara AI (ZO-AI)

The AI built into Zohara OS. All Rust, no Python.

| Crate | What |
|---|---|
| `crates/zo-core` | The tools that control the machine (files, shell, screenshots, processes, snapshots, Android/Waydroid), the permission policy, the audit log, the approval socket and the MCP protocol |
| `crates/zo-mcp` | Stdio MCP server: any MCP client (Claude Code, …) can control this computer through it |
| `crates/zo-ai` | The app (GTK4 + libadwaita): full window, quick popup, approval dialogs, global shortcut |

## Use

```bash
./install.sh                                  # build and install to ~/.local
zo-ai --background                            # start it (autostart does this after sign-in)
claude mcp add zohara -- ~/.local/bin/zo-mcp  # let Claude Code control this computer
```

- **Copilot key** or **Ctrl+Super+C** opens the quick popup (change it in System Settings > Shortcuts > ZO-AI). Press again or Esc to close.
- `zo-ai` opens the full window; `zo-ai --quick` toggles the popup.

## Permissions

The mode pill in the app (Auto / Manual / Bypass) is saved to `~/.config/zo-ai/mode` and applies to the app and to every MCP client.

- **Auto**: reads and plain look-commands (`ls`, `df`, …) run; changes and other commands ask.
- **Manual**: reads run; every change and every command asks.
- **Bypass**: nothing asks.

Asking = a dialog in the ZO-AI app. If the app is not running, a tool that needs approval is refused. Everything is logged to `~/.local/share/zo-ai/audit.jsonl`.
No password is stored anywhere; Waydroid root commands use polkit (`pkexec`).

## Develop

```bash
cargo test -p zo-core       # tools, policy, MCP protocol
cargo build --release
```

`packaging/PKGBUILD` is the Arch package (`zohara-ai`) for the Zohara repo, so Settings can offer it as an installable module.
`docs/design/zo-ai-design.html` is the original design reference.
