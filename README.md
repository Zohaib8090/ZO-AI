# Zohara Chat

The AI assistant built into Zohara OS. It chats with the model you choose and can look at and control your computer through a set of tools, always under your permission settings. Written in Rust with GTK4 and libadwaita.

## What is in the box

| Crate | What it does |
|---|---|
| `crates/zo-core` | The tools (files, shell, screenshots, processes, snapshots, Android and Waydroid), the permission policy, the audit log, model providers and the MCP protocol |
| `crates/zo-mcp` | A stdio MCP server. Any MCP client, such as Claude Code, can control this computer through it |
| `crates/zohara-chat` | The app: chat window, projects, quick popup, approval dialogs and the global shortcut |
| `crates/zo-server` | The public cloud server: web app, Google or e-mail sign-in, per-user isolated data, sync with the desktop app |
| `crates/zo-cli` | `zo`, the terminal client |

## Install

On Zohara OS, install it from Settings, or:

```bash
sudo pacman -S zohara-chat
```

From source:

```bash
./install.sh        # builds a release and installs to ~/.local
zohara-chat --background  # starts it (it also starts after sign-in)
```

`zohara-chat` opens the full window. `zohara-chat --quick` toggles the quick popup, which is also opened by **Ctrl+Super+C** (change it in System Settings → Shortcuts → Zohara Chat). The Copilot key cannot be used for this on Plasma yet: with the usual keyboard layout Plasma reads it as Touchpad Off.

## Choosing a model

Click the model button in the chat to pick a provider and model. API keys are stored in the system keyring (`secret-tool`), never in a plain file.

- **Anthropic (Claude), OpenAI, Google Gemini**
- **OpenAI-compatible hosts**: OpenRouter, DeepSeek, Groq, Mistral, Together, Fireworks, Cerebras, Perplexity, Kimi, z.ai, Qwen, MiniMax, NVIDIA, Hugging Face and others. The address of every provider can be edited.
- **Local models** need no key: Ollama, LM Studio, llama.cpp, vLLM, SGLang, or any custom address.
- **Claude Code (experimental)** uses the `claude` program installed and signed in on your computer. Zohara Chat starts it and connects it to `zo-mcp`; it never reads or stores your Claude login. Claude Code's own file and shell tools are turned off, so every action goes through Zohara Chat's tools and permission modes. Claude Code has to be installed separately.

## Permissions

The mode button in the chat (Auto, Manual or Bypass) applies to the app and to every MCP client.

| Mode | Behaviour |
|---|---|
| Auto | Reads and plain look-commands (`ls`, `df`, …) run. Changes and other commands ask first. |
| Manual | Reads run. Every change and every command asks first. |
| Bypass | Nothing asks. |

A question appears as a dialog in the Zohara Chat app. If the app is not running, anything that needs approval is refused. Every tool call is logged to `~/.local/share/zohara-chat/audit.jsonl`. No password is stored; Waydroid root commands use polkit (`pkexec`).

## Use it from other tools

```bash
claude mcp add zohara -- ~/.local/bin/zo-mcp   # let Claude Code control this computer
```

Any MCP client that can start a stdio server works the same way.

## In the terminal: `zo`

```bash
zo "how much disk is left?"        # one answer, streamed; tool steps appear dimmed on stderr
zo chat                            # an interactive chat (/new /chats /open N /model /mode /memory /stop /quit)
zo memory; zo remember "I use fish"; zo status
zo login                                                       # sign in (a code and a web address; Google or e-mail)
zo --remote https://your-server "question"                      # a zo-server instead of the local app (after zo login)
```

It uses the same chats, memory and model choice as the app. See [docs/CLI.md](docs/CLI.md).

## The public web service

`zo-server` is a **public, multi-user service**: anyone who uses Zohara OS / Zohara Chat can sign in with Google or e-mail and password on the web, and in the desktop app (optional: it works fully without any login and sends nothing until you sign in) to sync, and in the terminal (`zo login`). No tokens to copy: see [docs/LOGIN.md](docs/LOGIN.md).
Each person has their own isolated data and **brings their own API key**; there is no shared model key. Everyone except the operator's admin
addresses (`ZO_ADMIN_EMAILS`) is **restricted**: web search, page fetch and memory only, with no access to any computer, shell, files or MCP servers
(computer-control tools run only in a person's own desktop app, which syncs their chats). Devices sign in through the browser or with e-mail and password and get their own revocable credential (Account > signed-in devices, which also has Export and Delete). Operator settings: `ZO_ADMIN_EMAILS`, `ZO_PUBLIC_URL`, `ZO_BLOCKED_EMAILS`, `ZO_MAX_USERS`, `ZO_MAX_RUNS`, `ZO_TENANT_ROOT`.
Design, threat model, limits and cost: [docs/PUBLIC.md](docs/PUBLIC.md); setup: [docs/DEPLOY.md](docs/DEPLOY.md), [docs/SERVER.md](docs/SERVER.md).
Texts for the operator to review: [docs/PRIVACY.md](docs/PRIVACY.md), [docs/TERMS.md](docs/TERMS.md) (templates, not legal advice).

## Develop

```bash
cargo test -p zo-core          # tools, policy, providers, MCP protocol
cargo build --release
cargo run -p zo-core --example mock-model   # a fake model server, to try the chat without an API key
```

`packaging/PKGBUILD` builds the Arch package, and CI (`.github/workflows`) builds it on every push to `main`. `docs/design/zohara-chat-design.html` is the design reference.

## Documentation

* [docs/ZOHARA-CHAT.md](docs/ZOHARA-CHAT.md): how the app, the model layer, context and compaction, profiles and storage work.
* [docs/CLI.md](docs/CLI.md): the `zo` terminal client.
* [docs/ACTIVITY.md](docs/ACTIVITY.md): what is running now (tray icon, Activity page, web menu), and how to stop it.
* [docs/LOGIN.md](docs/LOGIN.md): signing in (desktop, terminal, web), the device flow and its security.
* [docs/MCP.md](docs/MCP.md): MCP servers, sign-in and the redirect address.
* [docs/PUBLIC.md](docs/PUBLIC.md), [docs/SERVER.md](docs/SERVER.md), [docs/SYNC.md](docs/SYNC.md), [docs/DEPLOY.md](docs/DEPLOY.md): the public server, sync and deployment.

## License

GPL-3.0-or-later.

## Renamed from ZO-AI

The product used to be called ZO-AI. The terminal command is still `zo`. What changed, and how an existing install carries over
(nothing is deleted: the old places stay, the new version reads them and copies what it needs once):

| Before | Now | Carry-over |
|---|---|---|
| program `zo-ai` | `zohara-chat` | `zo-ai` stays as a thin command: it prints a hint and starts `zohara-chat` |
| package `zohara-ai` | `zohara-chat` | the package `replaces`, `conflicts` with and `provides` `zohara-ai`, so pacman upgrades in place |
| `~/.config/zo-ai/`, `~/.local/share/zo-ai/` | `~/.config/zohara-chat/`, `~/.local/share/zohara-chat/` | copied once on the first start; the old folders are kept (the AI's file tools refuse both) |
| keyring service `zo-ai` | `zohara-chat` | a key found under the old service is copied to the new one when first used; forgetting a key clears both |
| sockets `$XDG_RUNTIME_DIR/zo-ai/agent.sock`, `zo-ai-approve.sock` | `.../zohara-chat/agent.sock`, `zohara-chat-approve.sock` | clients try the new socket, then the old one |
| `ZOAI_KEY_<PROVIDER>`, `ZOAI_CLAUDE_BIN`, `ZO_AI_*` | `ZOHARA_CHAT_KEY_<PROVIDER>`, `ZOHARA_CHAT_CLAUDE_BIN`, `ZOHARA_CHAT_*` | both are read, the new name wins; programs the AI starts get neither |
| app id `org.zohara.ZoAi` | `org.zohara.Chat` | a hidden `org.zohara.ZoAi.desktop` is still installed so panel pins keep working |
| shortcut component `zo-ai` (Plasma) | `zohara-chat` | the keys chosen under the old name are used once |
| autostart `zo-ai-background.desktop` | `zohara-chat-background.desktop` | the person's own entry is carried over (off stays off); the old one is switched off |

Unchanged on purpose: the server address, the Firebase project, the GitHub repository, the export file's format tag
(`zo-ai-export-1`), the web page's storage keys and the JS bridge names (`window.zoaiShare`, `window.zoaiNativeMic`).
