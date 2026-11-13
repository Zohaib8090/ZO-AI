# The Zohara Chat server (`crates/zo-server`)

The same brain as the desktop app, on a server that is always on, behind a Firebase login. First step of
`PLAN-WEB-CLI-SYNC.md`. Built 2026-11-12. It is a **public, multi-user service**: any Google account, or e-mail and password account, may sign in, each person has their own isolated data and their own API key, and only the owner's addresses are admins. Design, threat model, limits and cost: `PUBLIC.md`. Privacy and terms: `PRIVACY.md`, `TERMS.md`.

## What it does

* Starts zo-core's own service (the one the phone talks to) and puts HTTPS in front of it.
* **Device credentials** (replace the old global `ZO_ACCESS_TOKEN`, which no longer exists; nobody copies one): the desktop app and `zo` get theirs by
  signing in, through the browser device flow (`/api/auth/device_start`, `device_poll`, and the approval on `/device`) or e-mail and password
  (`/api/auth/exchange`); see `LOGIN.md`. It is `zd_` plus 32 random bytes (base64url), handed to the device once; only its sha256 is stored
  (Firestore `users/<tenant>/meta/devices` and the index `deviceidx/<sha256>`). Kind `desktop` gets scope `sync`, which may call only `/api/sync/*`; kind `cli`
  gets `full`, which also allows `/api/op`, `/api/send`, `/api/reply` (the `zo` client). A credential is tied to its own tenant, can be revoked
  (Account > signed-in devices, or `/api/auth/signout`), and is never logged. `/api/account/device_create` is gone. At most 10 per person.
* **Login**: every call needs a Firebase ID token or a device credential (`Authorization: Bearer ...`). The Firebase token is checked against
  Google's keys, must be for this Firebase project and a verified account: Google, or e-mail and password with a **verified e-mail** (anonymous, custom, phone and unverified sign-ins are refused). Any such account may register, except addresses in
  `ZO_BLOCKED_EMAILS` and once `ZO_MAX_USERS` (200) is reached. Addresses in `ZO_ADMIN_EMAILS` are **admins** (all tools, Claude Code, SSH
  machines, Bypass); everybody else is **restricted** (web search, page fetch and memory tools only, no MCP servers, provider addresses must be public https, quotas).
* **Tenant**: first 32 hex characters of `sha256("zo-tenant:" + uid)`. Disk `$ZO_TENANT_ROOT/<tenant>/{config,data}`; no raw uid or e-mail in any path.
* **Limits**: 60 requests/minute and 2 concurrent `/api/send` per user, `ZO_MAX_RUNS` (8) answers at once, bodies 1 MB (`/api/op`, `/api/reply`), 14 MB (`/api/send`, it may carry attachments: see PUBLIC.md) and 20 MB (`/api/sync/*`); over the limit: 429 with `Retry-After`.
* **Firestore**: Render's disk is lost on every redeploy, so at start everything is fetched from Firestore and every change is
  sent back within about 3 seconds. One sync manager per tenant (pulled on its first request, idle ones are evicted from disk after 30 minutes with nothing unsynced). Layout under `users/<tenant>`: `chats/{id}`, `memory/{id}`, `files/{name}` (model choice,
  profiles, personalities, projects, ...). A text over 700 KB is cut into several documents.
* **API keys**: bring your own. Each user's keys live in `<their config>/keys.enc`, AES-256-GCM, key = `sha256("zo-keys" || ZO_SERVER_SECRET || tenant)`, and the ciphertext goes to Firestore. There is no shared model key. Without the secret, keys are lost at every restart. The operator holds the secret and so can technically decrypt (the privacy notice says so).
* **Public address** (`ZO_PUBLIC_URL`, else `RENDER_EXTERNAL_URL`; `https://host[:port]`, no path): the base of the approval link `<base>/device` in the sign-in flow. In login mode the server exits with code 2 if neither is set or the value is invalid.
* **Self-ping**: calls its own `/healthz` every 8 minutes (address from `RENDER_EXTERNAL_URL`); only useful on the free plan, which is not suitable for a public service (`PUBLIC.md`).
* **Account page** (web app) and `/api/account/*`: `whoami`, `devices_list`, `device_revoke` (the "signed-in devices"), `export` (chats, memory, personalities as JSON, never keys), `delete` (`{"confirm":"DELETE"}`, removes everything of the user, irreversible). `/privacy` and `/terms` are served as pages; the web app asks for agreement on first use and hides admin-only settings from restricted users. It says plainly that the web version has no access to a person's computer: computer-control tools run only in the person's own desktop app, which syncs their chats.
* `ZO_SERVER_NO_AUTH=1` (local test only, loopback) uses one fixed tenant.
* If Firestore cannot be read at start, the server stops instead of starting empty (it would otherwise overwrite what is saved).

## The web app

`crates/zo-server/web/index.html`, one file, served by the server at `/` (same address as the API, so no extra hosting and no CORS).
Sign-in through Firebase (Google, or e-mail and password; project `zo-ai-144`; the web config in the page is public by design) and the `/device` approval screen for the desktop app and `zo`. Chat with streaming answers,
step approvals and Stop, chat list, delete; Settings page with provider, model, API key, permission mode, personality and the
memory (add, pin, forget). **Firebase must know the server's address**: Authentication > Settings > Authorized domains > add the
Render address (for example `zo-ai-server.onrender.com`), or the Google pop-up refuses.

## The API

| Call | What |
|---|---|
| `GET /healthz` | `ok`, no login |
| `POST /api/op` | one operation of the service, for example `{"op":"chats"}`, `{"op":"memory"}`; answers JSON |
| `POST /api/send` | `{"text": "...", "chat": "id or empty", "attachments": [...] (optional, see PUBLIC.md)}`; the answer streams as one JSON object per line; the first line is `{"event":"stream","id":N}` |
| `POST /api/auth/device_start`, `/device_poll`, `/device_lookup`, `/device_approve`, `/exchange`, `/signout` | the sign-in of the desktop app and `zo`, see `LOGIN.md`; `GET /device` is the approval page |
| `POST /api/sync/manifest`, `/get`, `/put`, `/delete` | the desktop sync, see `SYNC.md` |
| `POST /api/reply` | `{"stream": N, ...}`: a line for a running answer (stop it, approve a step), also for a job running on the person's computer |
| `POST /api/send` with `"target":"desktop"`, `POST /api/op` with `desktop_op`, `POST /api/link/poll`, `/events`, `/status` | run the message on the person's own computer through the desktop app's long poll; protocol, limits and security in `LINK.md` |
| `POST /api/sync/claude_token` | the owner's desktop sends (or removes) the Claude sign-in token (admin accounts only), stored encrypted; `POST /api/account/claude_status` says whether it is there |

## Setting it up on Render (owner)

Full steps, free-plan facts and the Claude Code token: `docs/DEPLOY.md`. Short version:

1. Render > New > Blueprint > this repository (`render.yaml`).
2. Fill in `FIREBASE_SERVICE_ACCOUNT` (paste the whole JSON key file) and `ZO_ADMIN_EMAILS` (your Google address). After the first deploy set `ZO_PUBLIC_URL` to the service's own address (below; it is the base of the approval link, and without it, or `RENDER_EXTERNAL_URL`, the server refuses to start).
   `ZO_SERVER_SECRET` is made by Render; `ZO_MAX_USERS`, `ZO_MAX_RUNS`, `ZO_TENANT_ROOT` are in the blueprint. `CLAUDE_CODE_OAUTH_TOKEN` is optional (from `claude setup-token`): only a fallback for admins; the normal way is to send the token from the desktop app (see `LINK.md`). The three secrets (`FIREBASE_SERVICE_ACCOUNT`, `ZO_SERVER_SECRET`, `CLAUDE_CODE_OAUTH_TOKEN`) are read at start and removed from the process environment.
3. In Firebase: Authentication > Google **and Email/Password** sign-in on; Firestore created. The server uses the service account, so Firestore's
   rules can stay closed to everyone else (`allow read, write: if false;`).

## Try it on your own computer

    ZO_SERVER_NO_AUTH=1 PORT=8799 cargo run -p zo-server      # no login, 127.0.0.1 only, no Firestore

In this mode there is no CORS, a request needs the header `X-ZO-Local: 1` (the web page sends it) and a `Host` of this computer, and a
request with another `Origin` is refused, so a web page from another site cannot use the server through your browser.
Programs the AI starts (shell, ssh, terminals, other MCP servers) do not get `FIREBASE_SERVICE_ACCOUNT`, `ZO_SERVER_SECRET`,
`CLAUDE_CODE_OAUTH_TOKEN` or `ZOHARA_CHAT_KEY_*`, and the file tools refuse the config folder, `~/.ssh` and `/proc/*/environ`.

Use a separate `XDG_RUNTIME_DIR`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` if the desktop app is running, or it will refuse to start
(the app already owns the socket) and would share the app's data.

## Tested / not tested

Tested: unit tests (cutting and joining long texts, the document layout, key encryption), and a local run without login
(`status`, `memory_add`, `set_key` into the keys file, a streamed `send`, CORS preflight).
The web page was opened against the local test server (`/?noauth` skips only the sign-in screen; a real server still refuses calls without a token): it renders, a message reaches the service (answered "No model is chosen yet"), the settings page lists the providers.
The Docker image was built and run with podman (health, web page, status, non-root, `claude`, `ssh`, Claude Code mode reaching `claude` with a made-up token): see `docs/DEPLOY.md`.
Not tested yet: the Google and e-mail sign-in themselves, the login check and Firestore against the real project (needs the owner's Firebase key),
Render itself, Claude Code mode with a real token.

## Not built yet

- The phone does not sync with the server yet (the laptop does: see SYNC.md). Pins, projects, artifacts, schedules, the model choice and API keys are not synced.
- Editing a memory note's text in the web app, chat summaries from the web, and a sync screen in the web app.
- The web app lacks a few desktop features: project workspace page, pinned projects in the sidebar, Claude Code and API-key rows on the Model page, Add-MCP-server rows, mic, Context and copy-all.
- `zo ssh` in the terminal client (machines work through the AI's tools and Settings > Machines, see SSH.md).
- Not tested against the real Render, Firebase and Firestore: Google sign-in, Firestore storage and Claude Code with a real token.

Built since: the `zo` terminal client (CLI.md), laptop-to-server sync after signing in (SYNC.md, LOGIN.md), SSH machines (SSH.md).
