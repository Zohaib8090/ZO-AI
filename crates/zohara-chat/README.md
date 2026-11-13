# zohara-chat (the app)

The GTK4 + libadwaita app of Zohara Chat. See the [main README](../../README.md) for what Zohara Chat is, how to install it and how permissions work.

## Data

Projects are stored as JSON at `$XDG_DATA_HOME/zohara-chat/projects.json` (`~/.local/share/zohara-chat/projects.json` by default). A damaged file is kept aside as `projects.json.broken` instead of being overwritten.

Each project has:

- **Instructions**: a layer above the system prompt, for that project only.
- **Context**: attached files.
- **Memory**: notes kept for every session.
- **Schedules**: tasks to run later or repeatedly.

The model and provider choice is saved in `~/.config/zohara-chat/ai.json`, the permission mode in `~/.config/zohara-chat/mode`.

## Source layout

| File | Contents |
|---|---|
| `src/main.rs` | Window, sidebar, projects, workspace and sheets |
| `src/chat.rs` | Sending messages, showing replies and tool runs, the model and key sheet |
| `src/quick.rs` | The quick popup |
| `src/ask.rs` | The approval dialog |
| `src/shortcut.rs` | Registering the global shortcut with KDE |
| `src/model.rs` | Project data and saving |
| `src/style.css` | Colors and layout |

## Debugging

- `ZOHARA_CHAT_SELFTEST=1 zohara-chat` prints the widget tree of every view and exits.
- `ZOHARA_CHAT_START_VIEW=projects|project zohara-chat` opens straight to that view.
