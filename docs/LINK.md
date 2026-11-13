# Desktop link: "My computer" for the web app (desktop side)

Written 2026-11-12. Code: `crates/zo-core/src/link.rs` (the loop, job checks, token), `crates/zo-ai/src/settings.rs` (block `link:begin` .. `link:end`:
the two sections of Settings > Account and sync), `crates/zo-ai/src/ask.rs` (`LinkHost`, the approval dialog), `crates/zo-ai/src/tray.rs` ("Web control: on/off").

## Design A: Control this computer from the web

Off by default (`~/.config/zo-ai/link.json`: `enabled`, `approvals_from_web`, `allow_bypass`, `name`, `claude_sent_at`; no secrets in it).
Needs a signed-in app. **With the switch off, or signed out, there is no network traffic for this at all.**

* The thread does `POST /api/link/poll` (Bearer device credential) with `{name, caps:{claude_code, version, os, approvals_from_web}}`. The server waits up to
  25 s and answers `{jobs:[...]}`; the app polls again at once. Failures: 2, 4, 8, ... 60 s; 429 honours Retry-After; 401 waits a minute and says so on the page.
* Job shapes the app understands (everything else is refused and reported as an error event, so the server can free the job):
  * `{id, type:"send", text, chat?, edit?, attachments?, mode?, age_ms?|created_ms?}`
  * `{id, type:"op", op | inner:{op, id?, provider?, model?, effort?, set?}}`, only `status providers profiles profile_use models select mode`
  * `{id, type:"stop", job_id?}`
  * `{id, type:"approval_reply", job_id, approval, yes}`
  * `{id, type:"answer_reply", job_id, question_id, answers|cancelled}`: the answer to a question card (`question` event) of a running web session; only for a card that job showed (docs/INTERACTION.md section 6)
* Jobs are untrusted: id must be `[A-Za-z0-9-_:.]` up to 64; text up to 100 000 characters; attachments at most 20 items / 8 MB; a job whose `age_ms` (or
  `now - created_ms`) is 120 s or more is never run. The request given to the local service is **built from the checked fields**, so a job cannot add a tenant,
  a mode or any other key. Only one web session runs at a time.
* Permission cap: the mode of a web job is what it asked for only if that is as strict as or stricter than the mode saved on the computer (the web cannot loosen it; else the saved mode), and **Bypass becomes Auto** unless "Allow Bypass mode from the
  web" is on. The `mode` op can only make the computer stricter, never looser.
* Approvals: shown on the computer ("A web session wants to: ...", Allow / Deny; the tray asks for attention). The web gets the `approval` event with
  `local:true` and no buttons. With "Answer approvals from the web too" on, the event has `local:false` and `approval_reply` jobs are accepted for that job only.
* Events go back with `POST /api/link/events {job_id, events:[...], done?}`, in batches (250 ms, under 1 MB, 50 MB per job). The answer `{cancel:true}` stops
  the job. `op` jobs answer with one event `{event:"op_result", reply:{...}}` and `done:true`. A job runs for at most 60 minutes.
* The turn is run through this app's own service socket (like the phone), as an Activity item "Web session: <chat>" (source `web-link`) with Stop. The chat is
  saved here and a sync is triggered when the turn ends.
* The tray menu has "Web control: on/off" (click to toggle).

## Design B: Claude Code on the server for the owner

Settings > Account and sync > "Use my Claude subscription on the web (owner only)": **Get sign-in** runs `claude setup-token` in the app's terminal
(Terminal panel), picks `sk-ant-oat...` out of the screen, closes the terminal, and holds the token only in memory (`link::Secret`, wiped on drop; no Debug
or Display). A hidden paste field is the fallback. **Send to my server** = `POST /api/sync/claude_token {token}` (Bearer device credential); **Remove** =
`{delete:true}`. The server accepts this from admin accounts only and says so otherwise. Only the time of sending is remembered (`claude_sent_at`).

---

# Server side (`crates/zo-server/src/link.rs`, `account.rs`, `hardening.rs`; `zo-core/src/ai/claude_code.rs`)

## The protocol (what the server does with the calls above)

| Call | Credential | What |
|---|---|---|
| `POST /api/link/poll` | desktop device credential only | `{name, caps}` -> waits up to 25 s -> `{ok, jobs:[...]}`. One open poll per device: a newer poll ends the older one at once (empty answer). |
| `POST /api/link/events` | desktop device credential only | `{job_id, events:[...], done?}` -> `{ok, cancel}`. `cancel:true`: stop (the page pressed Stop or went away, a limit was hit, the job is over). A job that is not this device's gets `404 {error:"unknown_job", cancel:true}`. |
| `POST /api/link/status` | person signed in (a desktop credential may ask too) | `{ok, devices:[{id, name, online, last_seen, caps}]}`: the person's desktop credentials with what the link knows; `online` = polled within 40 s (or a poll is open); `caps` is cleaned to `{claude_code, approvals_from_web, version, os}`. |
| `POST /api/send` `{target:"desktop", device?, text, chat?, attachments?, mode?}` | person (web or CLI) | NDJSON, like a cloud answer: `{event:"stream", id, target:"desktop", device:<name>}`, then the desktop's events in posting order (`ping` lines every 15 s of silence), then `{event:"done"}`. Without an online desktop (or for a device id that is not the caller's): `{event:"error", error:"desktop_offline", message}` then `{event:"done"}`, nothing queued. Other errors in the same shape: `desktop_busy`, `limit`, `desktop_timeout` (not picked up in 2 minutes: never run), `desktop_lost` (no poll for 2 minutes during a job), `job_timeout` (60 minutes), `too_much_output`, `desktop_revoked`, `account_deleted`; Stop that the desktop did not honour within 20 s ends with `{event:"stopped"}`. |
| `POST /api/op` `{op:"desktop_op", device?, inner:{op, ...}}` | person | Only `status providers profiles profile_use models select mode`, with only the fields `id` / `provider` / `model` / `effort` / `set` (`auto` or `manual`; Bypass is refused). Answer `{ok:true, result:<desktop's reply>}`; errors `503 {error: desktop_offline|desktop_timeout|desktop_lost|desktop_error, message}`, `400 not_allowed`, `429 limit`. Waits up to 45 s; a question nobody picked up is taken back, never run late. Does not count as the one active job. |
| `POST /api/reply` `{stream, op:"stop"}` | person | Queues a `stop` job for the desktop and marks the job cancelled (always allowed: stopping is safe). |
| `POST /api/reply` `{stream, op:"approve", id, yes}` | person | Forwarded as `approval_reply` **only if the desktop announced `approvals_from_web:true`** in its last poll; otherwise `409 {error:"approvals_on_computer"}` and the page shows "Waiting for your approval on your computer". |
| `POST /api/sync/claude_token` | desktop device credential, **admin account only** | see Design B below |
| `POST /api/account/claude_status` | person (Firebase) | `{ok, present, source:"own"|"server"|"none", set_at, admin}` |

Job shapes the server hands out (every job also has `age_secs`, `age_ms`, `expires_in_secs`; the desktop refuses a job of 120 s or more):

* `{id, type:"send", text, chat?, attachments?, mode?}`: only these fields, text up to 100 000 characters, chat id up to 100 characters without control characters, up to 20 attachment objects, `mode` one of auto/manual/bypass (the desktop caps it).
* `{id, type:"op", inner:{op, ...}}`: answered by an event `{event:"op_result", reply:{...}}` (or `result`), or `{event:"error", error}`, with `done:true`.
* `{id, type:"stop", job_id}` and `{id, type:"approval_reply", job_id, approval, approval_id, yes}` (control messages, not jobs).

Events of a send job go to the page as they are, except that the desktop's own `stream` and `done` lines are dropped (the server writes the first and the last line itself).

## Security model

* Everything is per tenant. A device and a job belong to the tenant of the credential that created them; `events` checks the job's tenant AND device against the credential, `reply` checks the stream's tenant. Another person's device id behaves as "offline"; another person's job id as "unknown". Tests: `nobody_can_reach_another_persons_computers_jobs_or_streams`.
* Scope rules: only a `desktop` credential (scope `sync`) may call `/api/link/poll`, `/api/link/events` and `/api/sync/claude_token`; a web session and a `cli` credential may not (403); a desktop credential still may not call `/api/op`, `/api/send` or `/api/reply`. `/api/link/status` is for a person (and, harmlessly, the desktop).
* The page's message is untrusted for the desktop: only whitelisted fields travel (above), `tenant` and any unknown key are dropped, op names outside the whitelist are refused here (the desktop checks again). The desktop's posts are untrusted for the server: events must be objects, at most 500 per post and 1 MB per request, its name/caps are cleaned of control and direction characters and unknown fields.
* Nothing is stored: presence, queues and jobs are in memory only. A restart loses the queue; a job is never resumed and never run late (2 minute pickup limit, enforced when it is delivered and again when the page's wait ends).
* "Anyone who can sign in to your account on the web can run tools on this computer." The owner of the computer decides: the link is off by default on the desktop, the mode is capped at Auto, approvals are answered on the computer unless the person turned web approvals on.
* Revoking a device (account page, signing out, device limit) or deleting the account ends its jobs at once.

## Limits

| What | Limit |
|---|---|
| Open polls | one per device; 150 on the whole server (more: `503`, retry in 10 s) |
| Poll rate | 20 per minute per device (`429` + Retry-After) |
| Event posts | 600 per minute per device; 500 events and 1 MB per post; 50 MB per job; 8 MB not yet read by the page (then the job is stopped: `too_much_output`) |
| Jobs | 1 active send job per device; 3 per person; 8 waiting per device; 4 settings questions per device; 400 on the server |
| Devices | 10 per person (the same as signed-in devices); 5000 tracked on the server |
| Time | pickup 2 min; a send job at most 60 min; a delivered job whose device made no poll for 2 min is given up; settings question 45 s |
| Message | 100 000 characters; 20 attachments (the request body limit of 1 MB applies) |

The link does not use the person's request allowance (the poll would eat it), has its own limits above, and keeps no `Lease` while it waits (deleting the account is not held up by a poll). Each open poll and each streaming page holds one of the server's 400 connections.

## Design B on the server: Claude Code for the owner only

* `POST /api/sync/claude_token` with the desktop credential of an **admin** account: `{"token": "sk-ant-oat01-..."}` stores, `{"delete": true}` removes (`{"action": "set"|"delete"|"status"}` also work). Anyone else: `403` with a clear message (restricted people sign in to Claude Code on their own computer and use it through My computer). The token must look like one (`sk-ant-`, URL-safe characters, 30 to 400 long); the answer never repeats it.
* Stored in the owner's own encrypted key store (`keys.enc`, AES-256-GCM, key from `ZO_SERVER_SECRET` and the tenant id) under `claude-code-token`, with the time under `claude-code-token-at`. In Firestore only the same ciphertext (`files/keys_enc`) is kept; it is in no export, no sync manifest, no provider list and no log.
* `claude_status` and the provider list: `Claude Code (your subscription)` is listed on the server only for an admin whose token exists. `CLAUDE_CODE_OAUTH_TOKEN` in the server's environment is only a fallback for admins without their own (`source:"server"`), never for restricted people.
* Spawning (`zo-core/src/ai/claude_code.rs`, `command`): on a server `claude` starts with an **empty environment** plus `PATH`, `HOME=<tenant config>/claude-home` (mode 0700, so its sessions are the tenant's alone; a redeploy wipes them and the "session lost" retry starts a new one), `CLAUDE_CODE_OAUTH_TOKEN=<that tenant's token>` and `DISABLE_AUTOUPDATER=1`; its working folder is the tenant's data folder; Claude Code's built-in tools are off (`--tools ""`, only `mcp__zohara__*`), so ZO-AI's permission rules apply and it cannot run a shell in the container. A token Claude turns down gives "Claude rejected the token: send a new one from the desktop (Settings > Account and sync)"; the token is cut out of every error text. Tokens made by `claude setup-token` last about a year: `set_at` is shown on the web page.
* Restricted tenants never get this: `binary()`, `run`, `complete`, the provider list, `select`/`set_key`/`models` for `claude-code` and the token route all refuse them, whatever is stored (tests in `zo-core` and `zo-server`).
* Hardening (`hardening.rs`): at the very start of `main`, before any thread, the server reads `FIREBASE_SERVICE_ACCOUNT`, `ZO_SERVER_SECRET` and `CLAUDE_CODE_OAUTH_TOKEN`, overwrites their values in the process's environment block with zeros (Linux keeps showing the start-up strings in `/proc/<pid>/environ` after `unsetenv`), and removes them. The values then live only in memory (`firestore.rs`, `zo_core::ai::keys::set_server_secret`, `claude_code::set_global_token`). The test starts the test binary as a child with those variables and checks, from inside it, its own environment, a child's environment and `/proc/<ppid>/environ`.
* There is deliberately **no operator switch** that gives other people Claude Code on the server.

## What was and was not tested

Tested (`cargo test -p zo-server -p zo-core`, fake verifier, in-memory Firestore, a fake `claude` script): presence, job lifecycle (send, ordered relay, done, Stop and the 20 s grace, expiry, one job per device, lost device), tenant isolation, scope rules, the offline error, approvals routing, the op whitelist, size/rate/cap limits, account deletion and revocation, the Claude token (admin only, restricted refused, ciphertext only, not in export/sync/provider output, spawn environment and flags, error scrubbing), the environment hardening in a real child process.
Not tested: a real desktop app talking to this server, real Firebase/Firestore/Render (long polls through Render's proxy: the 25 s poll and the 15 s `ping` lines were chosen to stay under typical proxy idle limits), a real `claude setup-token` token or a real Claude login.

## End-to-end test (integration, 2026-11-12)

`crates/zo-server/src/tests.rs` (block `link:e2e`) runs zo-core's real link loop (`examples/linkdesk.rs`: the real ZO-AI service on a socket in its own sandbox,
a scripted "person on the computer" who answers approval dialogs yes then no) against the real server over HTTP, with the `mock-model` example as the model.
Covered: offline error within 1 s, online with caps, B cannot see or reach A's computer, a send with an approval asked on the computer (yes: the tool ran; no: it
did not), the web's approve answered 409, Stop in the middle of an answer, the computer disappears and the next send fails within 1 s, an expired job is never run
later; and the owner's token sent with `zo_core::link::send_claude_token_at` (a guest is refused), run through a fake `claude` with the minimal environment.
Not covered: the GTK app itself, a real `claude`, real Render/Firebase.
