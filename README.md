# ZO-AI

The AI assistant built into Zohara OS. It chats with the model you choose and can look at and control your computer through a set of tools, always under your permission settings. Written in Rust with GTK4 and libadwaita.

## What is in the box

| Crate | What it does |
|---|---|
| `crates/zo-core` | The tools (files, shell, screenshots, processes, snapshots, Android and Waydroid), the permission policy, the audit log, model providers and the MCP protocol |
| `crates/zo-mcp` | A stdio MCP server. Any MCP client, such as Claude Code, can control this computer through it |
| `crates/zo-ai` | The app: chat window, projects, quick popup, approval dialogs and the global shortcut |

## Install

On Zohara OS, install it from Settings, or:

```bash
sudo pacman -S zohara-ai
```

From source:

```bash
./install.sh        # builds a release and installs to ~/.local
zo-ai --background  # starts it (it also starts after sign-in)
```

`zo-ai` opens the full window. `zo-ai --quick` toggles the quick popup, which is also opened by **Ctrl+Super+C** (change it in System Settings → Shortcuts → ZO-AI). The Copilot key cannot be used for this on Plasma yet: with the usual keyboard layout Plasma reads it as Touchpad Off.

## Choosing a model

Click the model button in the chat to pick a provider and model. API keys are stored in the system keyring (`secret-tool`), never in a plain file.

- **Anthropic (Claude), OpenAI, Google Gemini**
- **OpenAI-compatible hosts**: OpenRouter, DeepSeek, Groq, Mistral, Together, Fireworks, Cerebras, Perplexity, Kimi, z.ai, Qwen, MiniMax, NVIDIA, Hugging Face and others. The address of every provider can be edited.
- **Local models** need no key: Ollama, LM Studio, llama.cpp, vLLM, SGLang, or any custom address.
- **Claude Code (experimental)** uses the `claude` program installed and signed in on your computer. ZO-AI starts it and connects it to `zo-mcp`; it never reads or stores your Claude login. Claude Code's own file and shell tools are turned off, so every action goes through ZO-AI's tools and permission modes. Claude Code has to be installed separately.

## Permissions

The mode button in the chat (Auto, Manual or Bypass) applies to the app and to every MCP client.

| Mode | Behaviour |
|---|---|
| Auto | Reads and plain look-commands (`ls`, `df`, …) run. Changes and other commands ask first. |
| Manual | Reads run. Every change and every command asks first. |
| Bypass | Nothing asks. |

A question appears as a dialog in the ZO-AI app. If the app is not running, anything that needs approval is refused. Every tool call is logged to `~/.local/share/zo-ai/audit.jsonl`. No password is stored; Waydroid root commands use polkit (`pkexec`).

## Use it from other tools

```bash
claude mcp add zohara -- ~/.local/bin/zo-mcp   # let Claude Code control this computer
```

Any MCP client that can start a stdio server works the same way.

## Develop

```bash
cargo test -p zo-core          # tools, policy, providers, MCP protocol
cargo build --release
cargo run -p zo-core --example mock-model   # a fake model server, to try the chat without an API key
```

`packaging/PKGBUILD` builds the Arch package, and CI (`.github/workflows`) builds it on every push to `main`. `docs/design/zo-ai-design.html` is the design reference.

## Documentation

* [docs/ZO-AI.md](docs/ZO-AI.md): how the app, the model layer, context and compaction, profiles and storage work.
* [docs/MCP.md](docs/MCP.md): MCP servers, sign-in and the redirect address.

## License

GPL-3.0-or-later.
