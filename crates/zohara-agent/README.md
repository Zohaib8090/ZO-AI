# zohara-agent (the app)

The GTK4 + libadwaita app of Zohara Agent. See the [main README](../../README.md) for what Zohara Agent is, how to install it and how permissions work.

## Data

Projects are stored as JSON at `$XDG_DATA_HOME/zohara-agent/projects.json` (`~/.local/share/zohara-agent/projects.json` by default). A damaged file is kept aside as `projects.json.broken` instead of being overwritten.

Each project has:

- **Instructions**: a layer above the system prompt, for that project only.
- **Context**: attached files.
- **Memory**: notes kept for every session.
- **Schedules**: tasks to run later or repeatedly.

The model and provider choice is saved in `~/.config/zohara-agent/ai.json`, the permission mode in `~/.config/zohara-agent/mode`.

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

- `ZOHARA_AGENT_SELFTEST=1 zohara-agent` prints the widget tree of every view and exits.
- `ZOHARA_AGENT_START_VIEW=projects|project zohara-agent` opens straight to that view.
