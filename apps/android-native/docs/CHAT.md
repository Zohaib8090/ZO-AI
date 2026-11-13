# Chat module (package `org.zohara.agent.chat`)

Home, chat thread, composer, tool groups, approval and question cards, context sheet. Written 2026-11-13. Parity rows: CHAT-*, COMP-*, PERM-*,
INTER-*, CTX-*, NAV-17..19, NET-07, NET-15, SRV-*, BG-01..03, PHN-15 (see `docs/ANDROID-NATIVE-PARITY.md`).

## Shape

| File | What |
|---|---|
| `ChatItems.kt` | the rows of a thread (`UserItem`, `AiItem`, `ToolGroupItem`, `ErrorItem`, `ApprovalItem`, `QuestionItem` ...) and `ChatUiState` |
| `ChatTranscript.kt` | pure helpers: saved chat to rows (desktop grouping rule), copy text, greeting, model names, context estimate |
| `ChatSession.kt` | one conversation: load, send (NDJSON events), stop, retry, edit, takes, approval and question replies, compact, delete, pin |
| `ChatHub.kt` | all sessions of the app (the home session, one per opened chat); an answer keeps streaming while another page is open |
| `PillsState.kt` | model pill, mode pill (`status`, `profiles`, `mode`, and the same ops on the chosen computer) |
| `QuestionLogic.kt` | `QuestionDraft` (card state, validation per INTERACTION.md) |
| `ChatBackend.kt`, `ChatEnv.kt`, `AppChatEnv.kt` | what the session needs from the server and the app; real and fake implementations |
| `*View.kt`, `ChatPane.kt`, `Sheets.kt`, screens | Compose UI. `ChatContent` is the stateless body the screenshot tests draw |
| `ChatMenu.kt` | the top-bar "More" menu (Context, Copy conversation, Pin, Terminal for admins, Delete) |
| `ChatLinks.kt` | `ChatUriHandler` (links in answers open in a Custom Tab, only http/https) and `AnswerLinks` (the "Copy link" entries of the long-press menu) |

A session lives in the container scope, not in a screen. The home screen has its own session; when the server names the chat (`chat` event) the
hub moves it under its id, `HomeScreen` opens `chat/<id>` and makes a new empty home. `ChatScreen(id)` asks the hub for the session of that id
(it loads the chat only if the hub does not have it yet), so the live rows are never thrown away. A chat that another device changed is read
again on resume unless an answer is running (CHAT-42).

## Hooks other modules fill (`ChatHooks`)

`attachmentChips`, `attachButton`, `runOnPill`, `micButton(appendText, send)`, `readAloud`, `prefill`, and `chatsChanged` (a flow the chat lists
collect: the chat emits it after a send, after the answer, 6 s later for a computer turn, after delete and pin). Attachments themselves come from
`MediaModule.of(container).attachments` (`take()` at send, `restore()` when nothing reached the model); approvals and questions go to
`MediaModule.of(container).notifier` (it checks the foreground and the notification choices); `keep_running` comes from `MediaModule.keepRunning()`.

## Event handling (BG-01..03, CHAT-51)

`stream` -> remember the id; `chat` -> id and title; `delta` -> one live row; `text` is used only when no delta came for that step;
`tool_start` / `tool_done` -> one active group (it survives an approval card between its lines); `approval` -> card, or the "waiting on your
computer" row when `local`; `question` -> card; `usage`; `error` -> error row (+ "Choose a model" when no model is chosen, + link refresh for
`desktop_offline`); `stopped`; `done`; `ping`, unknown events and lines that are not JSON are ignored. A stream that ends without `done` shows the
"connection was lost" sentence and reloads the chat.

## Not in this module

Settings > Model (the pill's "Model and API key..." row opens `settings/model`), the chat lists (they collect `ChatHooks.chatsChanged`), the Terminal
page, attachments UI, voice, Run on control, push registration.

## Second pass (2026-11-13): verification and gaps

* Links in answers go through `ChatUriHandler` (Custom Tab); the answer's long-press menu has Copy, Select text, Share, Copy link (up to 3), Try again.
* `LoadingSkeleton` replaces the spinner while a chat loads (CHAT-45). "Jump to the newest" now scrolls (it only switched following on before).
* The question card keeps its draft over rotation (`QuestionDraft.saver`) and has the hardware keyboard keys of INTER-09: arrows and Enter through focus,
  number keys 1-6 pick an option, Esc skips, an Enter that no row took goes to Next or Send.
* `DraftKeeper` puts the composer draft into the saved state (COMP-18); the draft itself lives in the session (NAV-19).
* Tablets (width >= 840 dp): the Context sheet has "Pin to the side"; the panel (`ContextPanel`, 340 dp, Unpin) sits beside the chat and follows the rows (CTX-08, CTX-11).
  Dragging its edge is not offered on a touch screen.
* A notification for an approval or question is cancelled when the answer goes on without the person's reply (answered on the computer or another device) (INTER-14).
* PERM-04: the transport supports `mode` per send (ZoApiTest); the app does not send it, the saved mode applies (a test pins that).
* CHAT-39 is adapted: nothing is refused. Leaving a chat while it answers is fine, the answer keeps going in its own session (`ChatSessionMoreTest`).

## Tests

* `ChatSessionTest`, `ChatSessionMoreTest`, `PillsStateTest`, `QuestionLogicTest`, `TranscriptTest`: state holders with fakes (no Android).
* `ChatInteractionTest`: Robolectric + Compose; taps, typing, long press, hardware keys, scrolling, semantics, saved-state restore.
* `ChatScreenshotTest`: Roborazzi PNGs in `app/src/test/screenshots/chat_*.png` (dark and light, 320 dp and 411 dp wide, a 1000 dp tablet). The tests use
  `application = Application::class` because `ZoApplication` installs the media/link hooks, which need an AppContainer; the screenshot test puts stand-ins
  into `ChatHooks` instead.
* Several agents build this project at once: give Gradle its own output folder (`-I init.gradle` with `layout.buildDirectory`), or two `testDebugUnitTest`
  runs write the same result files and die with `EOFException`.
