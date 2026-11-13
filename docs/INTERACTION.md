# Questions and "needs you" notifications (all platforms)

Written 2026-11-13 from the owner's request: like the Claude app, ZO-AI should (1) tell you with a notification when it needs
a decision (a sensitive action in Manual or Auto mode), and (2) be able to ASK QUESTIONS with ready-made options plus an
"Other" free-text answer. It must work the same on the desktop app, the web app and the native mobile app.
This file is the contract; every client is built against it. Status (2026-11-13): core, server relay, desktop (GTK) and web are built and tested (section 6); the native Android app and Web Push for "needs you" are not.

## 1. The question tool

A new agent tool `zohara_ask_user` (risk: Read, so it never asks for approval itself; allowed for restricted tenants too,
because it only returns what the person typed or chose).

Parameters: `questions`: 1 to 4 items, each
`{question: string (<= 300 chars), header: string (<= 24 chars, shown as a chip), multi_select: bool, options: [{label (<= 60), description (<= 200)}] (2 to 6 options)}`.
The client ALWAYS adds an "Other" choice with a text box; the model must not add one itself.

The tool result given back to the model is plain text: one block per question,
`Q: <question>\nA: <chosen labels joined by ", "> [Other: <text>]`, or `The person declined to answer.`

System prompt rule: use it only when the answer changes what you do and a sensible default does not exist; never for
yes/no confirmations of actions (that is what approvals are for); keep questions short; at most one call per turn.

## 2. Stream events and replies

Server or service to client (one JSON object per line on the `send` stream, same place as the `approval` event):

    {"event":"question","id":<u64>,"questions":[{question,header,multi_select,options:[{label,description}]}]}

Client to server, on the same connection (desktop socket/phone) or `POST /api/reply` (web, mobile native):

    {"op":"answer","id":<u64>,"answers":[{"selected":["label",...],"other":"text or null"}, ...]}   // one entry per question, same order
    {"op":"answer","id":<u64>,"cancelled":true}

Rules for every implementation: validate that `id` is open, that `answers.length` equals the question count, that each
selected label is one of the offered labels, `other` <= 2000 chars; strip control characters; unknown ids are ignored.
Waiting time: 10 minutes (3 minutes when no notification channel exists); on timeout, Stop or disconnect the tool returns
"The person did not answer." and the model carries on without.
Desktop link: the desktop forwards `question` events to the web and accepts `{"op":"answer_reply"}` jobs exactly like
`approval_reply`; a question is never answered by the server on its own.
The activity registry shows a waiting question as "Waiting for your answer".

## 3. Approvals (existing) and notifications

Approval events already exist (`approval` with `id`, `tool`, `summary`; reply `{"op":"approve","id","yes"}`).
New for both approvals and questions: **a notification when the person is not looking**.

| Platform | When | What |
|---|---|---|
| Desktop (GTK) | window not focused or minimized or hidden in the tray | system notification (org.freedesktop.Notifications through GIO): title "ZO-AI needs you", body the summary or the first question; approvals have buttons Allow / Deny; clicking the body shows the window on that chat; the tray icon is NeedsAttention (exists) |
| Web | tab hidden or app closed | in-page notification through the service worker (tag per chat; "Allow"/"Deny" actions are NOT used on the web: the click opens the chat where the card is waiting); Web Push when the tab is closed and `keep_running` is on (generic text, no content unless the person enabled titles) |
| Android (native) | app in background | high-priority notification channel "Needs you"; approvals have Allow / Deny actions handled by a receiver that calls `/api/reply` (needs the signed-in session); questions have an "Answer" action that opens the question screen; delivered by FCM data message when the app is closed |
| Phone daemon (Zohara Link) | existing app | out of scope here |

Privacy: the payload of a push never contains the command, file path or question text unless the person turned on
"Show details in notifications"; the default text is "ZO-AI needs your decision" plus the chat id.

## 4. UI (all platforms, the same behaviour)

* A **question card** appears in the chat where the model asked: the header chip, the question, the options as selectable
  rows (radio for single, checkboxes for multi) each with its description underneath, an "Other" row with a text field,
  and Send / Skip buttons. After sending, the card collapses to "You answered: ...".
* Several questions: shown one after another inside one card with Back / Next (like the Claude app), one Send at the end.
* **Approval card** (existing) gets the same look: what ZO-AI wants to do, Allow / Deny, and a switch "Always allow this kind
  of action in this chat" is NOT added now (policy stays in Settings > Permissions).
* Keyboard (desktop and web): arrow keys and Enter, number keys 1-6 pick an option, Esc skips.
* Accessibility: labels, focus order, screen-reader announcements for "ZO-AI is asking you a question".
* The composer is disabled while a question card is open for that chat (the person answers the card).

## 5. Build order

1. zo-core: tool, event, wait/answer plumbing next to the approval code, service + server + link relay, tests with a fake model.
2. Web: card UI, notification hooks (service worker already exists), reply wiring.
3. Desktop: GTK card in the chat, desktop notification with actions, tray state.
4. Native Android app: built against this file from the start (card screen, notification channel and actions).

## 6. What is built (branch feat/interact) and what is not

Code: `zo-core/src/question.rs` (parse the model's questions, validate answers, the text for the model, the table of open questions, the wait),
`approval.rs` (`Questioner`, `QuestionResult`), `ai/agent.rs` (the tool, one call per answer), `service.rs` (`RemoteQuestioner`, the `answer` op),
`link.rs` (desktop link: `answer_reply` job), `zo-server/src/link.rs` (relay), `zo-ai/src/questioncard.rs` + `ask.rs` + `chat.rs` (GTK card, desktop
notification), `zo-server/web/index.html` (card, notification).

Rules as built (they refine section 2):

* **A client must say it can show a card**: `"can_ask": true` in the `send` request (the web page and the desktop link do). For any other client
  (the phone daemon, `zo`, scheduled tasks, sub-agents) the tool is NOT offered to the model at all, and a call that still arrives is answered
  "There is nobody to ask here". The MCP server (`zo-mcp`) never lists it.
* Result text for the model: answered `Q: ...\nA: a, b [Other: text]`; Skip: `The person declined to answer.`; timeout, Stop, client gone, a second
  call in the same answer or no one to ask: `The person did not answer.` (or an error for the model).
* Validation (`question::validate_answers`, the only place): one answer per question, labels must be among the offered ones, each at most once, one
  choice only for a single-select question, `other` cleaned of control characters and cut to 2000, something chosen for every question. An invalid
  answer is ignored and the question stays open; an unknown id is ignored; the first valid answer closes the question (a second one finds nothing).
* Questions: 1 to 4, texts cleaned of control characters, labels unique and never "Other" (the client adds it).
* Waiting: 10 minutes (`question::WAIT`), checked every 200 ms against Stop, so Stop ends a waiting question at once; the connection closing or
  `detached` ends it too. The 3-minute variant for "no notification channel" is NOT built (the service does not know the client's channels).
* Activity: the item of the answer says "Waiting for your answer" while a question is open (`activity::set_detail_of`).
* Desktop link: the question event goes to the web as it is. The answer comes back as the job `{type:"answer_reply", job_id, question_id, answers|cancelled}`
  (server: `POST /api/reply {stream, op:"answer", id, answers|cancelled}`, 1 to 4 answers, at most 24 KB, numeric id). The desktop forwards it only for a
  running job and only for a card that job showed; the service validates again. It does NOT need "Answer approvals from the web too": a question runs
  nothing, it is the person's own words (the same as typing a message). The desktop does not show its own card for a web session.
* Desktop (GTK): `DeskQuestioner` (chat.rs) + `questioncard.rs`: chip, question, options with descriptions, Other + text, Skip, Back/Next/Send, keys 1 to
  6, Enter, Esc; the card closes itself as "not answered" when the wait ends (Stop, timeout). One line stays in the chat. Notification "ZO-AI needs you"
  (GIO, `ask::needs_you`) when the window is not in front: approvals have Allow / Deny buttons (they answer the open dialog), a question has an Answer
  button; the body click shows the window. The tray asks for attention for approvals and questions.
* Web: card as in section 4; a new card never rebuilds while typing; Send waits for the server's yes (an error keeps the card open); the end of the answer
  closes open cards; the composer is not touched (it is locked while an answer runs). Notification through the service worker/page, tag per chat
  (`zo-needs-<chat>`), generic text unless "Show the chat title" is on; the switch is "ZO-AI needs me" (approvals and questions).
* NOT built: Web Push for a closed tab (a `keep_running` answer cannot take an answer after the tab left: the service refuses approvals and
  questions after `detached`, so a "needs you" push would only lead to a card that cannot be answered; it needs resumable streams first); the native
  Android notification channel and screens; the CLI `zo` does not show cards (it does not say `can_ask`); Claude Code mode has no question tool.

Tests: `cargo test -p zo-core question` and `ask_user`, `-p zo-core link::`, `-p zo-server -- question` (relay and the real link end to end with
the mock model), and in a display container `xvfb-run -a cargo test -p zo-ai -- --ignored the_card` (the GTK card).
