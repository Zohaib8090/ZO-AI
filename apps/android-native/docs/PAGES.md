# Pages module (org.zohara.chat.pages)

Chats, Search, Projects, Artifacts, Scheduled, Activity, Terminal, the sidebar chat lists and the "Running now" part of the user menu.
Matrix rows: every row with module `pages` in `docs/ANDROID-NATIVE-PARITY.md` (the Done column says which are verified).

## How it is built

| File | What |
|---|---|
| `PagesFormat.kt` | every pure rule and every sentence: relative time (the desktop's `model::relative`), the web's `when()`, the Activity clock, Pinned/Recents split, `projects` text parser, artifact sub line, task lines, kind and source words, elapsed time, transcript for sharing |
| `PagesApi.kt` | `PagesApi` (the ops the pages use; a fake in tests, `ZoPagesApi` over `ZoApi` in the app), `Loadable<T>` (last good value + loading + error) |
| `Holders.kt` | state holders with no Android in them: `ChatsHolder` (list, optimistic delete and pin, transcript), `SearchHolder` (200 ms debounce, stale answers dropped), `ProjectsHolder`, `ArtifactsHolder`, `ScheduledHolder`, `ActivityHolder` (read, Stop, poll), `TerminalHolder` (history, errors, no call on My computer) |
| `PagesHub.kt` | the chat list and the activity list shared by several screens, one per signed-in person (a new person gets a new hub: no list leaks to the next account) |
| `PagesCommon.kt` | `ListPage` (pull to refresh, lazy list, Loading / error / empty), `PageCard`, `ConfirmSheet`, row menus, `RefreshWhileVisible`, `rememberHolder` (ViewModel-owned holder, survives rotation) |
| `ChatActions.kt` | pin, share and delete (with confirm) of a chat row, shared by the Chats page and the sidebar |
| `*Screen.kt` | one stateful screen (collects holders) and one stateless `*Content` (what the screenshot and interaction tests render) per page |

## Decisions

* **Wording**: the desktop's text where the matrix says "same" (empty states of Artifacts, Scheduled, Activity), the web's where the matrix says web
  (Chats: "No chats yet" / "Start one with New."; Search: "Nothing found" / "Try other words."; the delete confirm "The conversation is removed.
  This cannot be undone."). Source words ("the web app", "the phone", "the scheduler") are the desktop's.
* **Chat list refresh** (LIST-04): on entering the page, when the app returns to the foreground, every 60 s while the Chats page is shown, and when
  the drawer is opened. Pull to refresh on every list page.
* **Pagination**: the service lists at most 60 chats and 100 artifacts and has no paging op, so there is nothing to page; the lists are lazy (only
  what is on screen is composed). At 60 chats the page says "Older chats can be found with Search".
* **Row actions** (NAV-11): a trailing "more" button and a long press give the same menu (Pin / Unpin, Share conversation, Delete this chat).
  Swipe actions are not built: the menu does the same job and is reachable with TalkBack.
* **Search** shows the recent chats while the box is empty, searches 200 ms after the last key, shows hits with snippet and relative time.
  The box does not take the keyboard focus by itself (the web's `focus()` has no cheap equivalent here); tap it.
* **Projects** (PROJ-06..11, D7): the desktop's project workspace (instructions, context files, project memory, per-project tasks, chats that
  know the project) lives only in the desktop's own store; the service has no op for any of it. To build it on the phone the server would need
  `project_get` / `project_update` (name, goal, instructions), `project_context_*` (files), `project_memory_*`, a `project` field on `scheduled`
  and `send` (so a chat can carry the project's goal and memory into the system prompt) and a timestamp in `projects`. Until then a project card
  opens a sheet with what the list knows (goal, memory note count) and says that the rest is in the desktop app. Pin stars and sorting (PROJ-04)
  need timestamps and pins that `projects` does not return: n/a.
* **Artifacts**: text opens as code (monospace, scrolls both ways, Copy). A file or screenshot has no bytes the phone can reach, so the sheet shows
  the path and says it is on the server; "Open the chat it came from" opens the chat. "no longer there" cannot be known (the service does not report
  whether the file still exists).
* **Scheduled**: `scheduled` does not return the last run, so a finished one-off task shows "Off · once" and "Done <rel>" cannot be shown. A leading
  "in " in When is rewritten to "now + " by `ZoApi.scheduleAdd` (the server's `date -d` rejects "in 2 hours": reported to the owner). The "Cancel this
  task?" confirm has "Keep it" and "Cancel task" so the two buttons are not both called Cancel.
* **Activity**: the running clock uses the server's own clock difference (`now_ms` - `started_ms`) and counts on locally, so a phone with a wrong
  clock still shows the right time; ticks every second. Read every 3 s while the page or the user menu sheet is shown, every 15 s while the drawer
  is shown (the avatar badge). Clients (`kind: client`) are never in "Running now": they are counted as "N connections open". Connected: this phone,
  the linked computer when Run on is a computer, the web app and command line from `clients_seen`. Background services: the web's static rows.
* **Terminal** (D1): `terminal_run` on the cloud server for admin accounts (a restricted account gets the server's own sentence as an "Error:" line);
  on My computer nothing is sent and the approval sentence is shown. Up/Down history by keys or buttons. The composer pill and the chat menu entry
  that open the screen belong to the chat module and call `LocalNav.current.terminal()` (COMP-12 stays open here for that reason).
* **Restricted accounts**: the drawer hides Projects, Artifacts and Scheduled (foundation). If a restricted account gets there anyway, the server
  refuses and its sentence ("Projects and scheduled tasks are not available to this account.") is shown as the page's error.
* **Share conversation** (LIST-12): the transcript (You / Zohara Chat / Error lines, no tool notes) goes to the Android share sheet as text.

## Tested

* JVM unit tests (`app/src/test/java/org/zohara/chat/pages`): `PagesFormatTest` (wording and rules), `HoldersTest` (every holder against a fake server:
  errors keep the old list, optimistic delete and pin with rollback, debounce and stale search, project create, task add with the own interval,
  Stop, polling period, terminal history), `PagesApiTest` (the real `ZoApi` against MockWebServer with the answers captured live in docs/API.md),
  `ChatActionsTest`, `PagesHubRegistryTest`.
* Robolectric Compose tests (`PagesInteractionTest`, `PagesRefreshTest`): taps, long press, typing, the confirm sheets, the sheets' enabled states,
  the refresh loop and the pull gesture.
* Roborazzi screenshots of every page and sheet in both themes (`app/src/test/screenshots/pages_*.png`).

Not tested: a real device or emulator (forbidden), the live drawer and user menu wiring (compiled, not run), real Firebase, the real service.
