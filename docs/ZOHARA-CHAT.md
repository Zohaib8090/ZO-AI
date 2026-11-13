# Zohara Chat: what it is and how it works

Zohara Chat is the AI built into Zohara OS: a chat app (GTK4 + libadwaita, Rust) that can look at and change the computer through tools, always
under the permission mode you chose. It is also an MCP server (`zo-mcp`) so other programs, such as Claude Code, can use the same tools.

| Crate | What it holds |
|---|---|
| `crates/zo-core` | the tools, the permission policy, the model layer (providers, requests, the tool loop, context and compaction), the MCP client and server, saved data |
| `crates/zo-mcp` | the stdio MCP server binary |
| `crates/zohara-chat` | the app: window, sidebar, chat, settings, quick popup, approval dialogs, global shortcut |

More detail on MCP is in [MCP.md](MCP.md).

## Using it

* **Window:** `zohara-chat` opens it; `zohara-chat --background` only starts the helper that keeps the shortcut, approvals and scheduled tasks alive
  (it starts at sign-in). A tray icon shows what is running and offers Open and Quit ([ACTIVITY.md](ACTIVITY.md)). **Ctrl+Super+C** opens the quick popup. The Copilot key cannot be used: Plasma reads it as Touchpad Off.
* **Sidebar:** New chat, Search, Projects, Artifacts, Scheduled, Activity (what is running now, see [ACTIVITY.md](ACTIVITY.md)), then Pinned and Recents. The button in the corner closes it; with it
  closed, the pointer on the corner button slides it out over the chat. A narrow window closes it by itself.
* **Size:** Ctrl + / Ctrl - / Ctrl 0 change the size of the interface (saved); the first run picks a size that fits the screen.
* **Messages:** under your message, Copy and Edit (edit sends it again from there); under a reply, Copy and Try again. These show
  while the pointer is over the message. A failed or stopped reply has its own Try again.
* **Takes:** asking again or editing keeps what was there as an earlier take. Under the message and its reply, "‹ 2 / 3 ›" switches
  between the takes. They are saved with the chat.
* **Tool runs** are collapsed in one box: teal and "Running a task in the terminal…" while working, grey "Ran 3 commands" when done.
  Click it to see each command and its output.
* **Interface size:** Ctrl + / Ctrl - / Ctrl 0 scale the whole window: text, buttons, icons, spacing, the sidebar and the side panel.
* **Side panel** (right of the chat; drag its edge to change the width, the window grows to make room): **Context** (pin it from the
  Context window with "Pin to the side": it stays and updates after each answer), **Terminal** and **Browser** (buttons in the top bar).
* **Microphone** (next to +): speak, press again, and the text is added to the message box. It uses the speech engine of Zohara
  voice typing (whisper.cpp), on this computer, in the language set in Zohara Settings.
* **Copy the whole conversation:** the button in the top bar (a drag-selection cannot cross from one message to the next).

## Models, providers and profiles

* **Providers:** Anthropic, OpenAI, Gemini, many OpenAI-compatible hosts (OpenRouter, DeepSeek, Groq, Mistral, Together, …), local servers
  (Ollama, LM Studio, llama.cpp, vLLM, SGLang), a custom address, and **Claude Code** (uses the `claude` program you signed in to; Zohara Chat
  never sees that login).
* **Profiles** (Settings > Profiles): saved presets of provider, model, address, effort and picture setting, each with its **own API key**
  in the keyring (`provider@profile`). A profile without a key of its own uses the provider's shared key. **Pinned models** of a profile
  show in the model menu, grouped by profile.
* **Keys** are kept in the system keyring (`secret-tool`), never in a file.
* **Claude models** are read from the Models API (with an Anthropic key) and cached for a day: names, release order, window size and
  which **effort** levels each model has. Without a key, Claude Code's own aliases (`opus`, `sonnet`, `haiku`, always the newest) are
  offered. Nothing in the code lists model versions. Effort is sent to Claude Code (`--effort`) today.
* **Pictures:** screenshots (`zohara_ss`) and picture files (`zohara_view_image`, or a path in your message) are sent to models that can
  see: Anthropic and Gemini always, OpenAI except a few old models, other hosts by the model's name (override in the model settings).
  A refusal drops the pictures and asks again once.

## Context, caching and compaction

* The **Context** button (i) in the top bar shows where the model's window goes: Messages, System tools, MCP tools, System prompt, MCP
  server instructions, Memory and project, the auto-compact buffer and what is free, how much is left until auto-compact, and the
  provider's own count of the last request. Sizes are estimated (about four characters per token); the provider's count is exact.
* **Caching:** Anthropic requests ask for automatic caching (`cache_control`), so a long chat re-reads its start at a fraction of the
  price. OpenAI, Gemini and most hosts cache a repeated start by themselves; Zohara Chat keeps its start stable. Token usage, including the
  cached part, is read from every provider.
* **Compaction:** at 85% of the window, before the next question, the older part of the conversation is summarized by the same model and
  replaced by that summary; the last two messages and their answers stay as they are. **Compact session** in the Context panel does it
  on demand. In Claude Code mode Claude Code does its own caching and compaction.

## Terminals, browser and sub-agents

* **Terminals:** real shells with colours, cursor and scrollback, several at once (tabs; "+" opens one, x closes it). You can type
  in them, and the AI can too: `zohara_term_open`, `_run` (types a command, waits for the output to settle, returns the screen),
  `_send` (text or a key such as Ctrl+C), `_read`, `_list`, `_close`. The AI opening one shows the panel by itself. Copy and paste
  are Ctrl+Shift+C / Ctrl+Shift+V. They run as you; Auto mode asks before running anything but a plain look command.
  Code: `zo-core/src/term.rs` (pseudo-terminal and the vt100 emulator, shared by the window and the tools), drawing in `zohara-chat/src/dock.rs`.
* **Browser:** WebKitGTK inside the panel (address bar, back, forward, reload). The AI uses `zohara_browser_open`, `_read` (text of
  the page plus numbered links, buttons and fields), `_click`, `_type`, `_scroll`, `_back`, `_screenshot` and `_js`. Reading, opening
  and scrolling run on their own; clicking, typing and JavaScript ask in Auto mode.
* **Sub-agents:** `zohara_ai_subagent` runs a second Zohara Chat on one job with a fresh conversation and the same tools and permissions;
  only its final report comes back. One level deep (a sub-agent cannot start sub-agents); several jobs are several calls, one after
  the other.
* These tools exist only inside the app (not in `zo-mcp`, so Claude Code mode does not have them).

## From the phone (Zohara Link > ZoAI)

`zohara-chat` also serves a private socket (`$XDG_RUNTIME_DIR/zohara-chat/agent.sock`, code in `zo-core/src/service.rs`) that `zohara-linkd` carries to the
phone app's ZoAI tab: chat, history, model, ask-before-acting mode and MCP servers, with Zohara Chat's questions answered on the phone. Chats
are the same saved chats as in the window. Details, permission and security: `docs/ZOAI.md` in the zohara-link repository.

## Tools the model has

Files (read, write, list, search, mkdir, rm), shell, screenshots and pictures, system info, processes, snapshots, Android (adb) and
Waydroid (including starting it), Zohara Chat's own projects, memory, scheduled tasks, saved chats and MCP servers, and the tools of every
MCP server you added (read-only ones run on their own in Auto mode, the rest ask).

**Permission modes:** Auto (reads and look-commands run, changes ask), Manual (reads run, everything else asks), Bypass (nothing asks).
The mode applies to the app and to every MCP client. Every call is logged to `audit.jsonl`.

**Scheduled tasks:** once, hourly, daily, weekly, monthly, or your own interval ("every 3 days", "every 90 minutes", "every 2 months";
at least 5 minutes). Made on the Scheduled page, in a project, or by asking in a chat.

## Where things are stored

| What | Where |
|---|---|
| Chats (with their takes), projects, scheduled tasks, artifacts, action log, model list cache | `~/.local/share/zohara-chat/` |
| Chosen model, profiles, MCP servers (readable only by you), permission mode, size, browser for sign-in links | `~/.config/zohara-chat/` |
| API keys, MCP sign-in tokens, profile keys | the system keyring |

## Build, install, test

```bash
cargo test                      # unit tests of all three crates
./install.sh                    # builds a release and installs to ~/.local (launcher org.zohara.Chat.desktop)
```

`ZOHARA_CHAT_START_VIEW=projects|project|settings|add-server|models`, `ZOHARA_CHAT_OPEN_CHAT=<id>`, `ZOHARA_CHAT_SHOW=context`, `ZOHARA_CHAT_SIDEBAR=closed` and
`ZOHARA_CHAT_DEBUG=1` are switches for checking the app by hand; run with `XDG_DATA_HOME` / `XDG_CONFIG_HOME` pointed at a scratch folder to
leave your own data alone. `packaging/PKGBUILD` builds the Arch package.

## Not done yet

* Sending effort to API providers (only Claude Code gets it); asking OpenRouter/OpenAI/Gemini which levels a model has.
* Several browser tabs, downloads and saved logins in the built-in browser; sub-agents that run at the same time; local models and pictures; selecting text across messages (a toolkit limit); resources and prompts of MCP servers in the interface.
* The old HTTP+SSE MCP transport, Client ID Metadata Documents, sampling and elicitation (see MCP.md).
