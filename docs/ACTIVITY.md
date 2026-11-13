# Activity: what is running now

Zohara Chat keeps one list of everything that is running right now, and shows it in four places: the **tray icon**, the **Activity page**
of the desktop app, the **web app** (sidebar footer menu) and the service operation `activity` (used by the phone and the web).
Each place can also **stop** things that can be stopped.

## What is listed

| Kind | What it is | Can be stopped | Where it is registered |
|---|---|---|---|
| `answer` | a chat answer being written (desktop, phone, web, CLI) | yes (the same Stop as the chat's Stop button) | service `send`, desktop `chat.rs` |
| `claude_code` | an answer by the Claude Code provider | yes | service `send`, desktop `chat.rs` |
| `scheduled` | a scheduled task that is running | yes | `zohara-chat/src/scheduler.rs` |
| `memory_job` | the memory agent updating or editing memory | yes (where the job has a cancel flag) | `ai/memory_agent.rs` |
| `command` | a long shell command or an SSH command started by the model; the title is the command, cut to one line | yes: the process is killed | `tools/system.rs`, `tools/ssh.rs` |
| `terminal` | an open terminal | yes: Stop closes it | read from `term::list` when the list is asked for |
| `sync` | the sync, with its last status; only while signed in | no, display only | read from the sync status when asked for |
| `answer` / `claude_code`, source `web-link` | a web session that runs on this computer through the desktop link (title "Web session: ..."); Stop ends it | yes | service `send` with `client: web-link` (`zo-core/src/link.rs`, docs/LINK.md) |
| `client` | a phone, web or CLI client with an open connection to the service | no, display only | service connection handling |

Every item has `id`, `kind`, `title`, `detail`, `source` (`desktop`, `phone`, `web`, `cli`, `scheduler`, `background`), `chat_id` (when
it belongs to a chat), `started_ms` and `can_stop`. Terminals and the sync are built when the list is asked for, so they can never be
stale; they have `started_ms` 0 (no running time is shown).

Clients: requests are short, so they are not listed as items. The registry remembers when each kind of client last talked to the
service (`clients_seen`; the `client` field of a request says `phone`, `web` or `cli`; without it the source is `web` on a
multi-tenant server and `phone` otherwise). The Activity page shows "Connected" when the last request was under 30 seconds ago.

Stop: pressing Stop on an answer also stops the shell command it is running. Commands run in a group of their own, and Stop or the
time limit kills the whole group, so what a shell started does not go on in the background.

Quit: asks when chats, tasks, commands or terminals are open (not for the sync or the short memory job). Quit anyway stops the work,
waits up to 3 seconds for answers to save, then closes the terminals and removes the sockets.

Tray presence: the icon counts as present only when the watcher took it and a tray that draws icons is registered (the Plasma
watcher exists even without a tray applet). The check is repeated every 30 seconds. In `--background` mode the window is shown when
no tray is present after 30 seconds, so the app is never invisible. "Start with the computer" reads the package's entry in
`/etc/xdg/autostart` too; turning it off writes a user entry with `Hidden=true`.

## How it works

`zo_core::activity` (`crates/zo-core/src/activity.rs`) is one global registry.

* `start(kind, title, detail, source, chat_id, cancel)` returns a `Guard`. The item is removed when the guard is dropped: on normal
  end, early return, error, panic or Stop. Nothing can leak a "running" row.
* `list()`, `stop(id)`, `counts()`, `busy_count()`, `clients_seen()`, `to_json()` read and act on it.
* `stop(id)` sets the item's cancel flag (or closes the terminal) and returns false when there is no such item for the caller, or it
  cannot be stopped. The agent loop checks the flag on every round, so a model that streams without a pause can be stopped too.

### Service operations

* `activity` returns `{items, clients_seen, now_ms}`.
* `activity_stop` with `{id}` returns `{stopped: id}`, or an error ("That is not running (any more), or it cannot be stopped.").

Both are available to restricted tenants too, scoped to themselves. Adding them to the phone daemon's whitelist is done in the
phone repository, not here.

### Tenants (the public server)

On a multi-tenant server a user sees and stops only their own items. A caller without a tenant sees nothing. Restricted users can only
have `answer`, `memory_job` and `client` items (they have no commands, terminals, schedules or Claude Code, see
[PUBLIC.md](PUBLIC.md)); terminals and the sync belong to the person at the computer and are never shown to a tenant. On the desktop
(no tenants) everything is visible.

## Desktop tray

The tray icon is a StatusNotifierItem (what KDE Plasma uses) with the Zohara Chat mark. This part is implemented in `crates/zohara-chat`
(`tray.rs`); the description is the agreed design:

* Menu: **Open Zohara Chat**; the **Running now** list (each item as one line such as "Chat: title, 2 min", with a Stop action, or "Nothing is
  running"); **Activity...** (opens the Activity page); **Quit Zohara Chat**. Left click shows or hides the window. The tooltip shows how many
  things are running, and the icon asks for attention while an approval is waiting.
* **Quit** while answers or commands are running first raises the window and asks "Quit anyway?" instead of killing them silently.
* If there is no StatusNotifierWatcher on the session bus (no tray host), the tray does nothing and the app behaves as before; it
  looks again every 30 seconds, quietly.
* In `--background` mode the tray is the only visible part; Open Zohara Chat creates the window.

### Settings

* **Keep running in the tray when the window is closed**: on by default when a tray host exists. Without one, closing the window quits,
  so the app is never left running invisibly with no tray.
* **Start with the computer**: writes or removes `~/.config/autostart/zohara-chat-background.desktop`.

## Activity page (desktop)

Sidebar item **Activity** (after Scheduled):

* **Running now**: each item with its running time (updating every second) and Stop / Open chat. "Nothing is running" when empty.
* **Connected**: phone and web/CLI clients, MCP servers, sync (signed-in e-mail, Sync now), open terminals.
* **Background services**: the Zohara Chat service socket, the scheduler, the memory agent, the sync loop.

The page refreshes every 2 seconds, only while it is visible.

## Web app

Click your name in the sidebar footer: a menu shows **Running now** (polled every 3 seconds while the menu is open; every 15 seconds
for a small badge on the avatar), a Stop button per item, then **Settings** and **Sign out**. It uses the operations `activity` and
`activity_stop`.

## Not verified

The registry and the service operations have unit and integration tests (including that Stop really ends a streaming answer and that
tenants cannot see each other's items). The tray, the Activity page and the web menu are described here as designed; check the code and
its test notes for what was exercised.
