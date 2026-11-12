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
* Jobs are untrusted: id must be `[A-Za-z0-9-_:.]` up to 64; text up to 100 000 characters; attachments at most 20 items / 8 MB; a job whose `age_ms` (or
  `now - created_ms`) is 120 s or more is never run. The request given to the local service is **built from the checked fields**, so a job cannot add a tenant,
  a mode or any other key. Only one web session runs at a time.
* Permission cap: the mode of a web job is what it asked for (else the mode saved on the computer), but **Bypass becomes Auto** unless "Allow Bypass mode from the
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
