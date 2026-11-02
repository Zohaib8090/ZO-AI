# ZO-AI (Rust)

The OS-level AI module for Zohara OS, written in Rust with GTK4 + libadwaita.
This is the native port of the approved HTML design.

## What it is

A dedicated app that lives on the system and gets installed like any other
application. The settings app can later expose it as an installable module —
that listing screen is not built yet.

## Build and install

```bash
cd rust
./install.sh            # builds release, installs to ~/.local
```

Or build alone:

```bash
cargo build --release    # target/release/zo-ai
```

Installed layout:

| Path | Purpose |
|---|---|
| `~/.local/bin/zo-ai` | binary |
| `~/.local/share/applications/zo-ai.desktop` | app-grid entry |
| `~/.local/share/icons/hicolor/scalable/apps/zo-ai.svg` | icon |
| `~/.local/share/metainfo/org.zohara.ZoAi.metainfo.xml` | metadata |

## Data

Projects are stored as JSON at `$XDG_DATA_HOME/zo-ai/projects.json`
(`~/.local/share/zo-ai/projects.json` by default). Nothing is hardcoded — the
UI renders only what is in that file.

Each project owns:

- **Instructions** — a layer above the system prompt, for this project only.
- **Context** — attached files (documents, zips, code, images).
- **Memory** — loaded into the project's working tree on every session.
- **Schedules** — created manually here, or by asking ZO-AI in chat.

## Source layout

| File | Contents |
|---|---|
| `src/main.rs` | UI: sidebar, chat, projects, workspace, popovers, sheets |
| `src/model.rs` | Project/Store types, JSON persistence, relative dates |
| `src/style.css` | Palette and layout (#0B0E14 / #E8ECF1 / #5FD4D4) |

## Verification hooks

- `ZO_AI_SELFTEST=1 zo-ai` — prints the live GTK widget tree for all three
  views and exits. Used to verify structure without a compositor.
- `ZO_AI_START_VIEW=projects|project zo-ai` — opens straight to that view.

## Not done yet

- Settings-app install entry for this module.
- The agent itself: no model calls, tool broker, or scheduler runtime behind
  the UI. Context attachments are recorded but not parsed.
