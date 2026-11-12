# The ZO-AI server (`crates/zo-server`)

The same brain as the desktop app, on a server that is always on, behind a Firebase login. First step of
`PLAN-WEB-CLI-SYNC.md`. Built 2026-11-12.

## What it does

* Starts zo-core's own service (the one the phone talks to) and puts HTTPS in front of it.
* **Login**: every call needs a Firebase ID token (`Authorization: Bearer ...`). The token is checked against Google's keys, must
  be for this Firebase project, and must belong to an address in `ZO_ALLOWED_EMAILS`.
* **Firestore**: Render's disk is lost on every redeploy, so at start everything is fetched from Firestore and every change is
  sent back within about 3 seconds. Layout under `zoai/owner`: `chats/{id}`, `memory/{id}`, `files/{name}` (model choice,
  profiles, personalities, projects, ...). A text over 700 KB is cut into several documents.
* **API keys**: a server has no keyring, so keys live in a private file (`ZO_AI_KEYS_FILE`) and go to Firestore encrypted with
  `ZO_SERVER_SECRET` (AES-256-GCM). Without that secret, keys are lost at every restart.
* **Self-ping**: calls its own `/healthz` every 8 minutes (address from `RENDER_EXTERNAL_URL`) so the free instance is not put to sleep.
* If Firestore cannot be read at start, the server stops instead of starting empty (it would otherwise overwrite what is saved).

## The web app

`crates/zo-server/web/index.html`, one file, served by the server at `/` (same address as the API, so no extra hosting and no CORS).
Google sign-in through Firebase (project `zo-ai-144`; the web config in the page is public by design). Chat with streaming answers,
step approvals and Stop, chat list, delete; Settings page with provider, model, API key, permission mode, personality and the
memory (add, pin, forget). **Firebase must know the server's address**: Authentication > Settings > Authorized domains > add the
Render address (for example `zo-ai-server.onrender.com`), or the Google pop-up refuses.

## The API

| Call | What |
|---|---|
| `GET /healthz` | `ok`, no login |
| `POST /api/op` | one operation of the service, for example `{"op":"chats"}`, `{"op":"memory"}`; answers JSON |
| `POST /api/send` | `{"text": "...", "chat": "id or empty"}`; the answer streams as one JSON object per line; the first line is `{"event":"stream","id":N}` |
| `POST /api/reply` | `{"stream": N, ...}`: a line for a running answer (stop it, approve a step) |

## Setting it up on Render (owner)

Full steps, free-plan facts and the Claude Code token: `docs/DEPLOY.md`. Short version:

1. Render > New > Blueprint > this repository (`render.yaml`).
2. Fill in `FIREBASE_SERVICE_ACCOUNT` (paste the whole JSON key file) and `ZO_ALLOWED_EMAILS` (your Google address).
   `ZO_SERVER_SECRET` is made by Render. `CLAUDE_CODE_OAUTH_TOKEN` is optional (from `claude setup-token`).
3. In Firebase: Authentication > Google sign-in on; Firestore created. The server uses the service account, so Firestore's
   rules can stay closed to everyone else (`allow read, write: if false;`).

## Try it on your own computer

    ZO_SERVER_NO_AUTH=1 PORT=8799 cargo run -p zo-server      # no login, 127.0.0.1 only, no Firestore

Use a separate `XDG_RUNTIME_DIR`, `XDG_DATA_HOME` and `XDG_CONFIG_HOME` if the desktop app is running, or it will refuse to start
(the app already owns the socket) and would share the app's data.

## Tested / not tested

Tested: unit tests (cutting and joining long texts, the document layout, key encryption), and a local run without login
(`status`, `memory_add`, `set_key` into the keys file, a streamed `send`, CORS preflight).
The web page was opened against the local test server (`/?noauth` skips only the sign-in screen; a real server still refuses calls without a token): it renders, a message reaches the service (answered "No model is chosen yet"), the settings page lists the providers.
The Docker image was built and run with podman (health, web page, status, non-root, `claude`, `ssh`, Claude Code mode reaching `claude` with a made-up token): see `docs/DEPLOY.md`.
Not tested yet: the Google sign-in itself, the login check and Firestore against the real project (needs the owner's Firebase key),
Render itself, Claude Code mode with a real token.

## Not built yet

The desktop app and the phone do not sync with Firestore yet (the server is the only writer), editing a memory note's text in the web app, chat summaries from the web, the `zo` terminal client, SSH machines.
