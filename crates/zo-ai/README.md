# zo-ai (the app)

The GTK4 + libadwaita app of ZO-AI. See the [main README](../../README.md) for what ZO-AI is, how to install it and how permissions work.

## Data

Projects are stored as JSON at `$XDG_DATA_HOME/zo-ai/projects.json` (`~/.local/share/zo-ai/projects.json` by default). A damaged file is kept aside as `projects.json.broken` instead of being overwritten.

Each project has:

- **Instructions**: a layer above the system prompt, for that project only.
- **Context**: attached files.
- **Memory**: notes kept for every session.
- **Schedules**: tasks to run later or repeatedly.

The model and provider choice is saved in `~/.config/zo-ai/ai.json`, the permission mode in `~/.config/zo-ai/mode`.

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

- `ZO_AI_SELFTEST=1 zo-ai` prints the widget tree of every view and exits.
- `ZO_AI_START_VIEW=projects|project zo-ai` opens straight to that view.
