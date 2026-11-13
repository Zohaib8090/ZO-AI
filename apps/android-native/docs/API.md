# Zohara Agent server API for the native Android client

Written 2026-11-13 for `apps/android-native`. Everything the app calls, with request and response JSON. Sources: `crates/zo-server/src/main.rs`
(routes, limits, errors), `crates/zo-core/src/service.rs` (`handle()` = the ops, `send()` = the stream), `account.rs`, `loginapi.rs`, `push.rs`,
`link.rs`, and the docs PUBLIC, LOGIN, SERVER, LINK, INTERACTION, NOTIFICATIONS, SYNC. Companion: `docs/ANDROID-NATIVE-PARITY.md` (the feature matrix).

**Status marks used below**

| Mark | Meaning |
|---|---|
| **live** | the example is a real response captured on 2026-11-13 from the built `zo-server` (release binary of `/home/zohaibbaig/Documents/Zohara MCP`, which includes the main 8207dd5 ops) in test mode (`ZO_SERVER_NO_AUTH=1 ZO_NOAUTH_ADMIN=1`, private HOME, a local fake OpenAI-compatible model on 127.0.0.1) |
| **code** | the shape was read from the source (file and function named) but not run (needs Firebase, a network model, a desktop, or an admin-only dependency) |
| **base** | exists in the worktree base `4a488cc` |
| **main** | added in main `8207dd5` (sync2 merge: `model_test`, `settings_get`, `settings_set`, `profile_update`, `base_url` and friends). The app must tolerate `"unknown request: <op>"` (answer: friendly "This server is too old for that.") |
| **contract** | specified in `docs/INTERACTION.md`, not in any code yet (question cards): the app implements it and never fails when the server does not send it |

What was NOT verified: anything against the real Render server, real Firebase tokens, Firestore, FCM, a linked desktop (`/api/link/*`), account
export/delete, push registration, device approval. Those are marked **code**.

## 1. Conventions

* **Base URL**: `https://zohara-agent.duckdns.org` (default; a self-hosted server may differ, `https` only). Paths below are relative to it.
* **Transport**: HTTPS, `POST`, `Content-Type: application/json`, UTF-8. Only `GET /healthz`, `GET /privacy`, `GET /terms` are `GET`.
* **Auth**: every call except the three `GET` pages carries `Authorization: Bearer <Firebase ID token>` (`FirebaseUser.getIdToken(false)`; the SDK
  refreshes it before it expires, about hourly; on a 401 refresh once with `getIdToken(true)` and retry that one call, then sign out). The server
  verifies the token against Google's keys, the Firebase project `zo-ai-144`, and a verified e-mail (Google, or e-mail + password with a verified
  address). The phone never uses the `zd_...` device credentials (those are for the desktop app and `zo`). In test mode (`ZO_SERVER_NO_AUTH=1`) there is no
  token and every call needs `X-ZO-Local: 1` and a `Host` of this computer (debug builds only).
* **Whose data**: the account is the person in the token; the server derives the tenant itself. Never send `tenant`.
* **Access level**: `whoami` says `admin` or `restricted`. Restricted = everybody except the owner (no machines, MCP, schedules, projects, Bypass, Claude
  Code, local providers; only public `https` model addresses; tools are web search, page fetch and memory). The server enforces it; the UI hides it.
* **The error rule**, important: an `/api/op` call that reaches the service always returns HTTP **200**; a failure is `{"ok":false,"error":"<sentence for a person>"}` in
  that 200 body (verified live: unknown op, missing chat, restricted refusals). Treat `status != 2xx` OR `ok == false` as an error and show `error` as is. Success bodies
  carry `"ok":true` next to their fields.
* **Time values**: chat `updated` is **milliseconds** since the epoch. Other timestamps can be seconds or milliseconds (`devices_list.created`, `last_used`, `link/status.last_seen`,
  `device_lookup.requested_at`): normalise with `t > 1e12 ? t : t * 1000` (the web's `secs()`); `push_list.created` is seconds; `claude_status.set_at` is seconds.
* **Text**: messages are Unicode; control characters other than tab and newline are refused in attachments; names are cut to 120 characters.

### Status codes and error bodies

All errors made by the server itself (not by an op) have this shape (`fail_response` in `main.rs`):

```json
{"ok": false, "error": "The request is not JSON.", "retry_after": null}
```

Verified live: bad JSON gives `400` with exactly that body; `{"op":"send"}` on `/api/op` gives `400 {"ok":false,"error":"Use /api/send to send a message."}`; an unknown path gives `404` with the plain text `not found`;
no token gives `401` (test mode: `{"ok":false,"error":"The test mode only answers its own page.","retry_after":null}`; production: `"Sign in first."`).

| Status | When | Body | Client action |
|---|---|---|---|
| 200 | op answered (also failures, see above); `/api/send` stream started | JSON / NDJSON | read `ok` |
| 400 | not JSON, not an object, not text, attachments malformed (`{"ok":false,"error":"The attachments must be a list."}` live), wrong confirm for delete, bad version for accept_terms | error body | show `error` |
| 401 | missing or invalid token ("Sign in first.", "The device token is not right (or it was revoked).") | error body | refresh token once, then sign-in screen + banner "Signed out or not allowed." |
| 403 | `terms_required`; blocked account ("This account is blocked."); wrong credential scope ("This device token may not do that.") | `{"ok":false,"error":"Accept the privacy notice and the terms first.","terms_required":true,"version":"2026-11-12"}` for terms | terms: open the consent screen with `version`; blocked: sign out with the message |
| 404 | unknown path; `/api/reply` for a stream that is not yours (`{"ok":false}` live); unknown device on `device_revoke`; unknown `push_unsubscribe` id; unknown device code | | show or ignore |
| 408 | body did not arrive within 30 s (90 s for `/api/send`, 120 s for sync) | error body | tell the person to retry on a better connection |
| 409 | push: more than 5 devices; `approvals_on_computer` (see section 7); device limit 10 | error body | show |
| 410 | "This account was deleted." | error body | sign out |
| 413 | body too big (1 MB op/reply, 14 MB send, 20 MB sync): "The request is too big." | error body | restore the draft |
| 429 | per-person limits (60 requests/minute; 2 answers at once; model-calling ops count as an answer), too many big sends at once (retry 5), device/code lookups | `{"ok":false,"error":"You are sending requests too fast. Wait a moment.","retry_after":N}` plus header `Retry-After: N` | section 9 |
| 502 | the service socket answered garbage ("Zohara Agent gave no answer.") | `{"ok":false,"error":...}` | show; retry read-only calls once |
| 503 | server full ("Zohara Agent has as many people as it can take right now. Try again later."), busy (`Retry-After: 5`), storage trouble, service not running (`Zohara Agent's service is not running: ...`) | error body | show, retry later |

## 2. Endpoint index

| Method and path | Purpose | Section |
|---|---|---|
| `GET /healthz` | `ok` (plain text), no login | 10 |
| `GET /privacy`, `GET /terms` | legal pages (HTML) | 10 |
| `POST /api/account/whoami` | who am I, limits, terms state | 5 |
| `POST /api/account/accept_terms` | accept the current terms | 5 |
| `POST /api/account/devices_list`, `device_revoke` | signed-in devices | 5 |
| `POST /api/account/export`, `delete` | take data out, delete the account | 5 |
| `POST /api/account/push_config`, `push_subscribe`, `push_unsubscribe`, `push_list` | FCM registration | 5 |
| `POST /api/account/claude_status` | Claude Code token status (admin) | 5 |
| `POST /api/op` | one operation, one answer (chats, models, memory, ...) | 4 |
| `POST /api/send` | send a message, NDJSON stream | 3 |
| `POST /api/reply` | stop / approve / answer on a running stream | 3.5 |
| `POST /api/auth/device_lookup`, `device_approve` | approve a desktop or terminal sign-in from the phone | 6 |
| `POST /api/link/status` | linked computers | 7 |
| `POST /api/sync/manifest` | counts of what is stored (Desktop sync page) | 8 |

## 3. `POST /api/send`: send a message and stream the answer

Body (JSON, up to 14 MB; the only route with a large limit):

| Field | Type | Notes |
|---|---|---|
| `text` | string | up to 20,000 characters for restricted accounts ("That message is too long (at most 20000 characters)."). May be empty when attachments exist or when `edit` is used (the old text is asked again) |
| `chat` | string | id of the chat to continue; empty or unknown starts a new chat (title = first 40 characters) |
| `edit` | integer | optional. Re-ask from user message number `nth` (0-based, counting only user messages): everything from it on is kept as an earlier take and a new take starts. With empty `text` the old text is asked again (the "Try again" button). **live** |
| `attachments` | array | optional, see 3.2 |
| `mode` | `"manual"` / `"auto"` / `"bypass"` | optional, for this question only; the saved mode is not changed. Bypass is refused for restricted accounts |
| `keep_running` | bool | optional. Honoured only if the account has at least one registered push device (section 5); then the answer is finished and saved even if the connection closes, and one push is sent (NOTIFICATIONS.md). Dropped otherwise |
| `target` | `"desktop"` | optional. Run on the person's linked computer instead of the server (section 7). `device` (string id from `link/status`) picks the computer |
| `client` | `"phone"` | optional and undocumented: the Activity page then says "the phone" (`client_source` in `service.rs`). Harmless if ignored |

Response: HTTP 200, `Content-Type: application/x-ndjson`, chunked, one JSON object per line, ends with `{"event":"done"}` (and the connection closes).
Rebuild the stream by buffering bytes until `\n`, trim, skip empty lines, skip lines that do not parse (the web does `catch { continue }`).

If the request itself is bad the answer is not a stream but a normal error: `400 {"ok":false,"error":"The attachments must be a list."}` (**live**), `401`, `403`, `413`, `429`.
Errors that happen inside the service arrive as an `error` event followed by `done` (an empty message gives `{"event":"error","error":"empty message"}` then `done`, **live**).

### 3.1 Event table

| `event` | Fields | Meaning | Status |
|---|---|---|---|
| `stream` | `id` (number); for a computer relay also `target:"desktop"`, `device` (name) | first line; the id is the handle for `/api/reply` | live |
| `chat` | `id`, `title` | the chat this answer is saved in (new or existing) | live |
| `delta` | `text` | a piece of the answer as it is written | live |
| `text` | `text` | the complete answer text of this step; arrives AFTER its deltas and after `usage`: ignore it if deltas already built the text, use it when there were none | live |
| `tool_start` | `name`, `summary` (first line the tool name, then `key: value` lines) | a tool is about to run; the saved chat line is `"▸ <name>  <second line of summary>"` | live |
| `approval` | `id` (number, per stream), `tool`, `summary` (multi-line); from a computer relay also `local` (bool) | the tool needs a yes. Arrives after its `tool_start`. Answer with `/api/reply` (3.5). Server waits 180 s, then counts it as no. With `local:true` (or when the computer does not allow web answers) only the computer can answer | live (cloud), code (`local`) |
| `tool_done` | `name`, `ok` (bool), `output` (up to 1,500 characters) | result of a tool; `name:"vision", ok:false` is the "this model cannot see pictures" notice | live |
| `usage` | `input`, `output` (numbers) | tokens of the last request as the provider counted them | live |
| `question` | `id` (u64), `questions:[{question, header, multi_select, options:[{label, description}]}]` | **contract**: the model asks the person. Answer with `/api/reply` `{"op":"answer",...}` (3.5). Not sent by any server yet | contract |
| `error` | `error` (text); relay errors also `message` | the answer failed; `desktop_offline`, `desktop_busy`, `limit`, `desktop_timeout`, `desktop_lost`, `job_timeout`, `too_much_output`, `desktop_revoked`, `account_deleted` are the relay codes (use `message`) | live (`desktop_offline`) |
| `stopped` | none | the answer was stopped (by `/api/reply stop`, Activity Stop, or a desktop that ignored a stop for 20 s); `done` follows | code |
| `ping` | none | computer relay only: every 15 s of silence; ignore | code |
| `done` | none | the stream is over | live |

Unknown events must be ignored (the web also tolerates `approval_wait` and `waiting_approval`).

Real transcript (**live**, fake model, Bypass; each line is one event):

```json
{"event":"stream","id":1}
{"event":"chat","id":"c1794509115674","title":"hi there"}
{"event":"delta","text":"Hello "}
{"event":"delta","text":"from the "}
{"event":"delta","text":"fake model."}
{"event":"usage","input":12,"output":4}
{"event":"text","text":"Hello from the fake model."}
{"event":"done"}
```

With a tool and an approval (**live**, Manual mode, message "run it"; the second part was received after the app posted the approval to `/api/reply`):

```json
{"event":"stream","id":2}
{"event":"chat","id":"c1794509120577","title":"run it"}
{"event":"usage","input":10,"output":3}
{"event":"tool_start","name":"zohara_shell","summary":"zohara_shell\ncmd: echo hi"}
{"event":"approval","id":1,"summary":"zohara_shell\ncmd: echo hi","tool":"zohara_shell"}
{"event":"tool_done","name":"zohara_shell","ok":true,"output":"hi"}
{"event":"delta","text":"Hello "}
{"event":"delta","text":"from the "}
{"event":"delta","text":"fake model."}
{"event":"usage","input":12,"output":4}
{"event":"text","text":"Hello from the fake model."}
{"event":"done"}
```

Picture sent to a model that cannot see pictures (**live**): the stream carries the notice, the question is still answered, and the saved chat gets an error line:

```json
{"event":"stream","id":6}
{"event":"chat","id":"c1794509140206","title":"look"}
{"event":"tool_done","name":"vision","ok":false,"output":"The model m1 cannot look at pictures, so the picture was left out. Choose a model that can see images (for example Claude, Gemini or GPT-4o) in the model settings."}
{"event":"delta","text":"Hello "}
...
{"event":"done"}
```

Computer offline (**live**, `target:"desktop"` with no computer linked):

```json
{"event":"error","error":"desktop_offline","message":"Your computer is not reachable. Start Zohara Agent on your computer (it can run in the tray) and turn on Settings > Account and sync > Control this computer from the web."}
{"event":"done"}
```

### 3.2 Attachments

```json
{"text": "look", "attachments": [
  {"kind": "image", "mime": "image/png", "name": "a.png", "data_b64": "<standard base64, padded>", "thumb": "data:image/jpeg;base64,..."},
  {"kind": "text", "name": "n.txt", "text": "Attached file: n.txt\nhello"}
]}
```

Rules (`zo_core::attach::parse`, checked on the server and again in the service): at most **4 pictures and 6 files**; a picture is png, jpeg, webp or gif by mime **and** first bytes, strict base64,
<= 3 MB decoded, <= 9 MB for all; a text is <= 200 KB (<= 400 KB for all), no control characters except tab/newline; names lose control characters, `/ \ " < >` and are cut to 120; `thumb` is kept only if it is a
`data:image/(png|jpeg|webp);base64,...` of <= 20,000 characters (the thumbnail is optional). Body <= 14 MB, 90 s to arrive, at most 16 big bodies at once on the whole server. A bad attachment list is a `400` before any stream.
The saved chat keeps only `attach:[{kind,name,size,thumb}]` (live: `{"kind":"image","name":"a.png","size":70}`; the picture itself is never kept).

### 3.3 Editing and asking again (takes)

`{"text":"second try","chat":"<id>","edit":0}` forks user message 0 (**live**): `op chat` then returns the new take with `takes:[{"count":2,"current":1,"nth":0}]`, and
`op switch {id, nth:0, delta:-1}` returns the older take (**live**, section 4). "Try again" = `edit` with the index of the last user message and empty `text`.

### 3.4 Running on the person's computer

`{"text":"...","target":"desktop","device":"<id>"}`: the stream is the desktop's events relayed (`LINK.md`): `stream` (with `target` and `device` name), the desktop's events in order (`ping` every 15 s of silence),
`done`. Chat id/attachments/mode may be sent; the server passes only `text`, `chat`, `attachments`, `mode` to the computer (so `edit` does not work there). Error codes in 3.1. **code**.

### 3.5 `POST /api/reply`: talk to a running answer

Body `{"stream": <id from the stream event>, ...}`; the rest is passed to the answer:

| Body | Effect | Status |
|---|---|---|
| `{"stream":2,"op":"stop"}` | stop the answer (also closing the connection stops it, unless `keep_running`) | code |
| `{"stream":2,"op":"approve","id":1,"yes":true}` | answer an `approval` event; `yes:false` denies | **live**: `{"ok":true}` |
| `{"stream":2,"op":"answer","id":7,"answers":[{"selected":["Label"],"other":null}]}` | answer a `question` event: one entry per question in order; `other` is a string <= 2000 characters or null | contract |
| `{"stream":2,"op":"answer","id":7,"cancelled":true}` | skip the question | contract |

Answer: `{"ok":true}` (200) when the line reached the answer; `{"ok":false}` with **404** when the stream is not the person's, is over, or the write failed (**live** for an unknown id).
For a computer relay the server converts `stop` and `approve` into jobs; an approval is forwarded only if the computer announced `approvals_from_web:true`, otherwise **409** `{"error":"approvals_on_computer"}`. Question answers follow the same relay rule (contract).
Validation the client does before sending a question answer (INTERACTION.md): the id is open, the number of answers equals the number of questions, every selected label was offered, `other` <= 2000 characters, control characters removed.

## 4. `POST /api/op`: operations

Body: `{"op":"<name>", ...fields}`. Answer: the fields below plus `"ok":true`, or `{"ok":false,"error":"..."}` (HTTP 200). Limit: 1 MB. Calls that ask a model take as long as an answer (up to 180 s) and count as an answer for the
limits (2 at once, 8 server-wide): `compact`, `memory_edit`, `personality_generate`, `models`, `model_test`. "R" = refused for restricted accounts (message in the last column).

### 4.1 Status, chats

| Op | Request | Response (live unless noted) |
|---|---|---|
| `status` | `{"op":"status"}` | `{"ok":true,"mode":"Auto","model":"","provider":"","ready":false,"version":"0.1.0"}` (`mode` is exactly `Manual`, `Auto` or `Bypass`; `ready` = a provider is chosen) |
| `chats` | `{"op":"chats"}` | `{"ok":true,"chats":[{"id":"c1794509120577","pinned":true,"title":"run it","updated":1794509131852}]}` newest first, at most 60 |
| `chat` | `{"op":"chat","id":"<id>"}` | `{"ok":true,"id":"...","title":"run it","entries":[{"kind":"msg-user","text":"run it"},{"kind":"msg-tool","text":"▸ zohara_shell  cmd: echo hi"},{"kind":"msg-tool-out","text":"✓ hi"},{"kind":"msg-ai","text":"Hello from the fake model."}],"takes":[{"count":2,"current":0,"nth":0}]}` ; entry kinds `msg-user`, `msg-ai`, `msg-tool`, `msg-tool-out`, `msg-error`; a user entry may carry `"attach":[{"kind":"image","name":"a.png","size":70,"thumb":"data:..."},{"kind":"text","name":"n.txt","size":26}]`; unknown id: `{"ok":false,"error":"that chat is gone"}` |
| `switch` | `{"op":"switch","id":"<id>","nth":0,"delta":-1}` | the same body as `chat` for the chosen take; `{"ok":false,"error":"There is no such take."}` at the ends |
| `delete_chat` | `{"op":"delete_chat","id":"<id>"}` | `{"ok":true}` (also for an unknown id) |
| `delete_all_chats` | `{"op":"delete_all_chats"}` | `{"ok":true,"deleted":3}` |
| `pin_chat` | `{"op":"pin_chat","id":"<id>","on":true}` | `{"ok":true}` |
| `search` | `{"op":"search","query":"fake"}` | `{"ok":true,"hits":[{"id":"...","snippet":"Hello from the fake model.","title":"run it","updated":1794509131852}]}` (at most 50) |
| `compact` | `{"op":"compact","id":"<id>"}` | `{"ok":true}`; errors: `"The conversation is too short to compact."` (live), `"Claude Code keeps and compacts this conversation itself."`; counts as an answer |
| `link_preview` | `{"op":"link_preview","url":"https://example.com/page"}` | `{"ok":true,"title":"...","description":"...","host":"example.com"}` (code); a private address: `{"ok":false,"error":"That address is on this computer or its own network, so it is not read."}` (live); 12 a minute |
| `terminal_run` | `{"op":"terminal_run","cmd":"echo hi"}` | admin: `{"ok":true,"output":"hi"}` (live); R: "Terminal command execution on the cloud server requires an admin account. ..." (code) |
| `mode` | `{"op":"mode"}` or `{"op":"mode","set":"manual"}` | `{"ok":true,"mode":"Manual"}`; accepted `set`: manual, auto, bypass; R for bypass: "Bypass mode is not available to this account." |
| `activity` | `{"op":"activity"}` | `{"ok":true,"now_ms":1794509131887,"clients_seen":[{"last_ms":1794509131884,"source":"web"}],"items":[{"id":3,"kind":"answer","title":"...","detail":"m1","source":"web","chat_id":"c...","started_ms":1794509131000,"can_stop":true}]}`; `kind` is one of answer, claude_code, scheduled, memory_job, command, terminal, sync, client (the item shape is from `Item::to_json`, `items` empty in the live run) |
| `activity_stop` | `{"op":"activity_stop","id":3}` | `{"ok":true,"stopped":3}` ; unknown: `{"ok":false,"error":"That is not running (any more), or it cannot be stopped."}` (live) |
| `sync_status` | `{"op":"sync_status"}` | R: "Sync is not available here." (live); admin on a server with sync: `{"ok":true,"enabled":true,"url":"...","email":"","last_ok":0,"error":"","pulled":0,"pushed":0,"deleted":0,"running":false}` (code) |

### 4.2 Models, keys, profiles (base unless marked main)

| Op | Request | Response |
|---|---|---|
| `providers` | `{"op":"providers"}` | live (main): `{"ok":true,"current":"custom","hidden_providers":["groq"],"providers":[{"id":"anthropic","name":"Anthropic (Claude)","local":false,"has_key":false,"needs_url":false,"default_url":"https://api.anthropic.com/v1","base_url":"https://api.anthropic.com/v1","hidden":false}, ...]}`. Base: only `id`, `name`, `local`, `has_key` and `current`. `Claude Code` is listed (named "Claude Code (your subscription)") only when the server says it is usable (admin with a token). The 27 provider ids: anthropic, openai, gemini, openrouter, deepseek, fireworks, groq, mistral, together, cerebras, perplexity, novita, gmi, stepfun, kimi, zai, qwen, minimax, nvidia, huggingface, ollama, lmstudio, llamacpp, vllm, sglang, claude-code, custom |
| `models` | `{"op":"models","provider":"openai","base_url":"https://my-host.example/v1"}` (both optional; `base_url` is tried, not saved) | live (main): `{"ok":true,"provider":"ollama","models":["m1","m2"],"current":"m1","efforts":["low","medium","high","xhigh","max"],"effort":null,"base_url":"http://127.0.0.1:8798/v1"}`; a failed fetch is still `ok:true` with `"models":[]` and `"error":"A restricted account can only use a model server with a public https address."` (live). Base: no `base_url`, no `error` (a failure is `ok:false`). `current` is "" for a provider that is not the chosen one. Counts as an answer |
| `model_test` | `{"op":"model_test","provider":"ollama","model":"m1","base_url":"..."}` (all optional) | live (main): `{"ok":true,"works":true,"message":"Ollama (this computer) answered with m1: it works."}`; failure `{"ok":true,"works":false,"message":"A restricted account can only use a model server with a public https address."}`; without `model` the model list is tried ("... 2 models ..."); Claude Code is refused (R). Counts as an answer; with a model it sends one small real request (15 s) |
| `select` | `{"op":"select","provider":"custom","model":"m1","effort":"high","base_url":"https://my-host.example/v1"}` (all optional; empty `base_url` resets to the default; empty `effort` clears) | live (main): `{"ok":true,"provider":"custom","model":"m1","effort":null,"base_url":"http://127.0.0.1:9/v1"}`. Base: no `base_url`. Switching provider resets the model and effort. R: private addresses refused ("A restricted account can only use a model server with a public https address.") and Claude Code |
| `set_key` | `{"op":"set_key","provider":"openai","key":"sk-..."}` | `{"ok":true}`; empty key: `{"ok":false,"error":"The key is empty."}` (live). Keys are encrypted on the server; never log them |
| `profiles` | `{"op":"profiles"}` | live (main): `{"ok":true,"active":"p1794509093772","profiles":[{"id":"p1794509093772","name":"Work","provider":"custom","model":"m1","effort":null,"pinned":[],"base_url":"http://127.0.0.1:9/v1","vision":null,"updated":1794509093772}]}`. Base: no `base_url`, `vision`, `updated` |
| `profile_create` | `{"op":"profile_create","name":"Work"}` | live: `{"ok":true,"id":"p1794509093772"}` (saves the current provider, model, address, effort; becomes the active profile); without a provider: `{"ok":false,"error":"Choose a provider first, then save it as a profile."}` (main) |
| `profile_use` | `{"op":"profile_use","id":"p..."}` | `{"ok":true,"provider":"...","model":"...","effort":null,"base_url":"..."}` (main adds `base_url`); unknown: "that profile is gone" (live) |
| `profile_update` | `{"op":"profile_update","id":"p...","base_url":"https://...","effort":"high"}` | main: `{"ok":true}`; bad address `{"ok":false,"error":...}` |
| `profile_rename` | `{"op":"profile_rename","id":"p...","name":"New"}` | `{"ok":true}` |
| `profile_delete` | `{"op":"profile_delete","id":"p..."}` | `{"ok":true}` (also removes the key it kept) |
| `profile_pin` | `{"op":"profile_pin","id":"p...","model":"m1","on":true}` | `{"ok":true}` (the star; `on:false` unpins) |
| `settings_get` | `{"op":"settings_get"}` | live (main): `{"ok":true,"hidden_providers":[],"provider_prefs":{},"updated":0}` |
| `settings_set` | `{"op":"settings_set","hidden_providers":["groq"],"provider_prefs":{"openai":{"order":1}}}` (either optional) | live (main): `{"ok":true,"hidden_providers":["groq"],"provider_prefs":{"openai":{"order":1}},"updated":1794509081430}`; an unknown provider id is an error |

### 4.3 Memory and personalities

| Op | Request | Response |
|---|---|---|
| `memory` | `{"op":"memory"}` | live: `{"ok":true,"enabled":true,"every":3,"items":[{"id":"m1794509148940000","kind":"preference","text":"I prefer short answers","title":"","chat":"","pinned":false,"updated":1794509148940}]}`; kinds person, preference, project, fact, episode (chat summary, `title` set); up to 300 items, pinned first |
| `memory_add` | `{"op":"memory_add","text":"I prefer short answers","kind":"preference"}` (`kind` default `fact`) | live: `{"ok":true,"new":true}`; too short: "That note is too short." |
| `memory_update` | `{"op":"memory_update","id":"m...","text":"..."}` | `{"ok":true}`; "That note is gone or the text is too short." (live) |
| `memory_pin` | `{"op":"memory_pin","id":"m...","on":true}` | `{"ok":true}` |
| `memory_forget` | `{"op":"memory_forget","id":"m..."}` | `{"ok":true}` |
| `memory_enable` | `{"op":"memory_enable","on":true}` | `{"ok":true}` |
| `memory_clear` | `{"op":"memory_clear"}` | `{"ok":true}` |
| `memory_edit` | `{"op":"memory_edit","text":"join my preferences into one note"}` | `{"ok":true,"message":"..."}`; counts as an answer |
| `personalities` | `{"op":"personalities"}` | live: `{"ok":true,"active":"friend","items":[{"id":"friend","name":"Friendly buddy","prompt":"Sound like a warm ...","generated":false,"builtin":true}, ...]}`; built-in ids: friend, engineer, mentor, sharp; own ones `p<ms>`; `active:""` = Plain |
| `personality_use` | `{"op":"personality_use","id":"friend"}` (`""` = Plain) | `{"ok":true}` |
| `personality_add` | `{"op":"personality_add","name":"Calm","prompt":"<4 to 7 sentences>"}` | live: `{"ok":true,"id":"p1794509148954"}`; too little: "A personality needs a name and a few sentences about how to sound." |
| `personality_generate` | `{"op":"personality_generate","description":"A patient teacher ..."}` | `{"ok":true,"id":"p...","name":"...","prompt":"..."}` (code); counts as an answer |
| `personality_remove` | `{"op":"personality_remove","id":"p..."}` | `{"ok":true}` |

### 4.4 Projects, artifacts, schedules, machines, MCP (admin only)

For a restricted account every op in this block except `artifacts` answers `{"ok":false,"error":"<refusal>"}` (live): machines "Machines (SSH) are not available to this account."; `mcp_*` "MCP servers are not available to this account.";
`schedule*` "Scheduled tasks are not available to this account."; `scheduled` and `project*` "Projects and scheduled tasks are not available to this account.".

| Op | Request | Response |
|---|---|---|
| `projects` | `{"op":"projects"}` | live: `{"ok":true,"text":"p1794509093697 \| Demo \| g \| 0 memory notes"}` (one line per project: id, name, goal, "N memory notes", separated by vertical bars; none: `"There are no projects."`) |
| `project_create` | `{"op":"project_create","name":"Demo","goal":"g"}` | live: `{"ok":true,"message":"Created the project \"Demo\" (id p1794509093697). It is on the Projects page."}`; duplicate name: error |
| `artifacts` | `{"op":"artifacts"}` | live: `{"ok":true,"items":[]}`; items `{id, kind ("code"/"screenshot"/"file"), title, path, text (<= 20,000 characters or null), chat, created}`, at most 100 (code) |
| `scheduled` | `{"op":"scheduled"}` | live: `{"ok":true,"tasks":[{"id":"t17945090995370","title":"t","prompt":"t","next_run":1794512699000,"repeat":"once","enabled":true}]}` |
| `schedule_add` | `{"op":"schedule_add","task":"check disk","when":"tomorrow 9am","repeat":"daily","title":"Disk"}` (`repeat`, `title` optional; `repeat` is once, hourly, daily, weekly, monthly or "every 3 days"/"every 90 minutes"/"every 2 months", at least 5 minutes) | live: `{"ok":true,"message":"Scheduled \"t\" for ... (once). It is on the Scheduled page, id t17945090995431."}`. **Known server bug (live)**: `when` is parsed with GNU `date -d`, so `"in 2 hours"` fails with `{"ok":false,"error":"I couldn't read \"in 2 hours\" as a time. Try \"tomorrow 9am\", \"in 2 hours\" or \"2026-11-05 18:30\"."}` while `"now + 1 hour"`, `"tomorrow 9am"`, `"2026-11-20 18:30"` work; the app rewrites a leading "in " to "now + " |
| `schedule_cancel` | `{"op":"schedule_cancel","id":"t..."}` | `{"ok":true}`; unknown: "That task is gone." (live) |
| `machines` | `{"op":"machines"}` | live: `{"ok":true,"machines":[]}`; items `{name, host, user, port, note, has_key}` |
| `machine_add` | `{"op":"machine_add","name":"vm1","host":"203.0.113.7","user":"ubuntu","port":22,"note":"n","key":"-----BEGIN OPENSSH PRIVATE KEY-----\n..."}` | `{"ok":true}`; **the key is required** (live without: `{"ok":false,"error":"Paste the private key that this machine accepts."}`); `port` is a number or a string |
| `machine_test` | `{"op":"machine_test","name":"vm1"}` | `{"ok":true,"message":"..."}` (code) |
| `machine_remove` | `{"op":"machine_remove","name":"vm1"}` | `{"ok":true}`; unknown: "There is no such machine." (live) |
| `mcp_list` | `{"op":"mcp_list"}` | live: `{"ok":true,"servers":[]}`; items `{name, command, enabled}` |
| `mcp_add` | `{"op":"mcp_add","name":"x","address":"https://example.com/mcp","token":"optional"}` | `{"ok":true,"message":"..."}`; tries the server first (live error: "Could not reach the server: ...") |
| `mcp_import` | `{"op":"mcp_import","config":"<JSON of mcpServers>"}` | `{"ok":true,"message":"..."}`; live error for `{}`: "No server was found in that. A server needs a \"command\" (a program) or a \"url\" (a remote server)." |
| `mcp_toggle` | `{"op":"mcp_toggle","name":"x","enabled":false}` | `{"ok":true}`; unknown: "no such server" (live) |
| `mcp_remove` | `{"op":"mcp_remove","name":"x"}` | `{"ok":true}`; unknown: "no server named x" |

### 4.5 Computer relay op

`{"op":"desktop_op","device":"<id>","inner":{"op":"status"}}` -> `{"ok":true,"result":{...the computer's own answer...}}`. Only `status providers profiles profile_use models select mode`, with only the fields
`id`, `provider`, `model`, `effort`, `set` (`auto`|`manual`). Errors: **503** `{"error":"desktop_offline"|"desktop_timeout"|"desktop_lost"|"desktop_error","message":"..."}`, **400** `not_allowed`, **429** `limit`. Waits up to 45 s. **code** (LINK.md).

## 5. `/api/account/*`

All `POST`, Firebase token only (a device credential gets 403). Until the current terms are accepted everything except `whoami`, `accept_terms`, `export`, `delete` answers 403 `terms_required`.

| Call | Request | Response |
|---|---|---|
| `whoami` | `{}` | live (test mode): `{"ok":true,"admin":true,"restricted":false,"email":"","tenant":"cf6bba7f...","limits":{"requests_per_min":60,"concurrent_sends":2},"data_bytes":2224,"terms":{"version":"2026-11-12","accepted":true}}`. A restricted account's `limits` also has `message_chars` 20000, `data_bytes` 26214400, `tool_steps` 8, `answer_secs` 120 (code, `account::limits`) |
| `accept_terms` | `{"version":"2026-11-12"}` (the version from `whoami.terms.version` or from the 403 body) | `{"ok":true,"version":"2026-11-12"}`; wrong version: 400 "That is not the current version of the notice and the terms. Reload the page." (code) |
| `devices_list` | `{}` | `{"ok":true,"devices":[{"id":"...","name":"My laptop","kind":"desktop" or "cli","scope":"sync" or "full","created":1794509000,"last_used":1794509100}]}` (code, `Device::public`) |
| `device_revoke` | `{"id":"<device id>"}` | `{"ok":true}`; unknown: 404 "There is no such device." |
| `export` | `{}` | `{"ok":true,"format":"zo-ai-export-1","exported_at":1794509000,"chats":[...],"memory":[...],"memory_settings":{...},"personalities":[...]}` never keys (code); save as `zo-ai-export-YYYY-MM-DD.json` (drop `ok`) |
| `delete` | `{"confirm":"DELETE"}` | `{"ok":true,"login_removed":true}`; `login_removed:false` means the sign-in record could not be removed (tell the person to ask the operator); wrong confirm: 400 "Send {\"confirm\": \"DELETE\"} to delete the account." (code). Irreversible; also available to blocked accounts; sign out afterwards |
| `push_config` | `{}` | `{"ok":true,"vapid_public_key":"...","fcm":true}`; register FCM only when `fcm` is true (code) |
| `push_subscribe` | `{"kind":"fcm","token":"<FCM token, 20-4096 chars [A-Za-z0-9_:.-]>","label":"Android: Pixel 8","show_title":false}` | `{"ok":true,"id":"<registration id>"}`; same token again replaces its entry and keeps the id; more than 5 devices: 409 (code) |
| `push_unsubscribe` | `{"id":"<registration id>"}` | `{"ok":true}`; other person's id: 404 |
| `push_list` | `{}` | `{"ok":true,"subscriptions":[{"id":"...","kind":"fcm","label":"Android: Pixel 8","created":1794509000}]}` (`created` in seconds; never the token) |
| `claude_status` | `{}` | `{"ok":true,"present":true,"source":"own", "server" or "none","set_at":1794509000,"admin":true}`; show the Claude Code card only when `admin` (code). The phone never sends the token (`/api/sync/claude_token` is for the desktop credential) |

## 6. Approving a desktop or terminal sign-in from the phone

`POST /api/auth/device_lookup` `{"user_code":"ABCD-EFGH"}` (Firebase token, verified e-mail, terms accepted) -> `{"ok":true,"name":"My laptop","kind":"desktop"|"cli","requested_at":1794509000,"ip":"203.0.113.4","same_network":true,"email":"me@example.com"}`;
404 `{"ok":false,"error":"That code is not right, or it has run out. Check the code on your device, or start again there."}` for a code that is unknown, expired (10 minutes) or used; after 5 wrong codes in 10 minutes: 429 "Too many wrong codes. Wait a few minutes before trying again." with `Retry-After` (`locked()` in `loginapi.rs`).
`POST /api/auth/device_approve` `{"user_code":"ABCD-EFGH","approve":true}` -> `{"ok":true}` (`approve:false` denies); 409 at the limit of 10 devices; 410 "That sign-in request ran out. Start again on your device." The code is typed by the person, never read from a link. **code** (LOGIN.md, `loginapi.rs`).

## 7. Linked computers

`POST /api/link/status` `{}` -> `{"ok":true,"devices":[{"id":"...","name":"Zohaib's laptop","online":true,"last_seen":1794509000,"caps":{"claude_code":true,"approvals_from_web":false,"version":"0.1.0","os":"linux"}}]}`.
`online` = polled within 40 s. Show at most 30. **code** (LINK.md). Use it before sending to a computer; poll every 20 s while the composer or the My computers page is open.

## 8. Desktop sync information

`POST /api/sync/manifest` `{}` -> `{"ok":true,"chats":{"<id>":<ms>},"memory":{...},"personalities":{...},"profiles":{...},"settings":{...},"mcp":{...},"keys":{...},"deleted":{"chats:<id>":<ms>},"now":<ms>}`
(main adds the last four kinds and `now`; an older server has only the first three). The app only counts keys and takes the newest time for "Stored for you" (SYNC.md). The other sync calls (`get`, `put`, `delete`) are the desktop's.

## 9. Limits, 429 handling and retry rules

Server limits (PUBLIC.md, `main.rs`): **60 requests per minute per person**, **8 requests at once per person**, **2 answers at once per person** (`/api/send` and the model-calling ops), `ZO_MAX_RUNS` (8) answers at once overall, 400 connections overall (`503` with `Retry-After: 5`), 16 big message bodies at once (`429`, retry 5),
bodies 1 MB (`/api/op`, `/api/reply`), 14 MB (`/api/send`), 20 MB (sync), arriving within 30 s / 90 s / 120 s. Restricted: 20,000 characters per message, 25 MB stored, 8 tool steps and 120 s per answer. `link_preview` 12 a minute. Push: 5 devices, 30 pushes an hour.

| Situation | Rule |
|---|---|
| 429 on any call | read `Retry-After` (header, or `retry_after` in the body). Show "You are going a little fast. Please wait N second(s) and try again." Do not retry a send automatically. For a read-only op triggered by a screen refresh, retry once after `Retry-After` (cap at 10 s) |
| 429 "Answers at the same time" for `/api/send`, `compact`, `memory_edit`, `personality_generate`, `models`, `model_test` | the person's two answer slots are busy (another device, or a kept-running answer); say so, keep the draft |
| 503 | show the error; for read-only ops retry once after `Retry-After` (default 5 s); never auto-retry sends |
| network error before any byte | read-only ops: one retry after 1 s; sends: no retry, restore the draft and attachments |
| stream ends without `done` | not an error to retry: show "The connection was lost..." and reload the chat with `op chat` (the server may still be finishing it) |
| 401 | refresh the token once (`getIdToken(true)`); if it still fails, sign out |
| 403 `terms_required` | consent screen (never an error toast) |
| timeouts | connect 15 s; `/api/op` read 190 s (the service waits up to 180 s); `/api/send` stream: no fixed total, treat 90 s of silence as a lost connection on cloud sends (the cloud stream has no `ping`) |

## 10. Pages

`GET /healthz` -> `200 ok` (live). `GET /privacy`, `GET /terms` -> HTML (live: 200); open them in a Custom Tab. `GET /` and `GET /device` serve the web app (not used). `GET /sw.js`, `/manifest.webmanifest` belong to the web app.

## 11. Notes on what the server does not give the phone

* No paging of chats: `chats` returns the newest 60; `search` covers up to 100 stored chats.
* No op returns the Context breakdown, the project workspace, scheduled-task run state (`last_run`, `last_chat`, run now, on/off) or the model's vision setting for `select`.
* `question` events and `needs_you` pushes are specified but not built (INTERACTION.md); approvals are live. A push for an approval does not exist yet: the "needs you" notification can only be made while a stream is open.
* The desktop's `edit` is not forwarded to a linked computer.
* Keys are never returned by any op (`has_key` only).

## 12. How these examples were checked

1. Read: `service.rs` (`handle`, `send`, `serve_request`, `restricted_refusal`), `main.rs` (`route`, `send`, `fail_response`, limits), `account.rs`, `loginapi.rs`, `push.rs`, the main-branch diff of `service.rs` and `SYNC.md`, and the tests in `tests.rs`.
2. Ran (2026-11-13): the release binary `target/release/zo-server` of the main workspace in test mode on 127.0.0.1:8799 with a private HOME, XDG_* and tenant folders, plus a 40-line fake OpenAI-compatible model (streaming deltas, a `zohara_shell` tool call, `/models`). About 70 ops and 8 sends were issued with `curl`; every example marked **live** is a copy of a real response (ids and times shortened where noted). The server and the fake model were stopped and their folders deleted afterwards.
3. Not run: anything that needs Firebase, Firestore, FCM, a linked desktop, push, the account calls and the device-code calls. Their shapes are **code**, from the functions named, and from the docs; re-check them against the real server when it is reachable.
