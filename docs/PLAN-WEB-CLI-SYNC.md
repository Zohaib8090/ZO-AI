# Plan: Zohara Chat everywhere (web, terminal, sync, remote machines)

Written 2026-11-12 from the owner's request. Status 2026-11-12: the web app, the `zo` terminal client (docs/CLI.md), laptop sync (docs/SYNC.md) and SSH machines (docs/SSH.md) are built and merged on main. Still not built: phone sync, `zo ssh`, and the real Render, Firebase and Firestore set-up has not been tested. The text below is the original brief.

> **Update 2026-11-12 (latest): no manual tokens.** People sign in with a Google account or e-mail and password; the desktop app works fully without any login and sends nothing until the person signs in; the web needs one. The desktop app and `zo` get a server-issued, revocable device credential through a browser device flow or an e-mail exchange, and nobody sees or copies it. Where the text below says "device token" or "made on the Account page", read: the sign-in flows in `LOGIN.md`.

> **Update 2026-11-12 (later): the server is public.** The owner decided that every person who uses Zohara OS / Zohara Chat may sign in with
> Google and use the service. The single-owner assumptions in this plan (one allowed account, one shared data folder, one global
> `ZO_ACCESS_TOKEN`, shell and file tools on the server) are **replaced** by a multi-tenant, bring-your-own-key design: per-user tenants and
> encrypted keys, restricted tools for everyone except the admin addresses in `ZO_ADMIN_EMAILS`, per-user device credentials (later by sign-in, see LOGIN.md), limits, an Account
> page with export and delete, `/privacy` and `/terms`. See `PUBLIC.md`. Where the text below says "one allowed account" or "owner only",
> read it as the admin; Render's free plan is no longer a fit for the public service (see `PUBLIC.md`).

## What the owner asked for

1. A **web version** of Zohara Chat, a full clone of the desktop app, with a **login** (Firebase Authentication), usable from anywhere.
2. **Everything in sync**: chats, memory, personalities, projects, settings, between laptop, phone and web.
3. **API keys** entered once work on the web too.
4. **Works with the laptop off**: normal chats must still work from the web.
5. **Claude Code mode** (the experimental provider) reachable from the web, even when the laptop is locked.
6. **SSH control**: give Zohara Chat other machines (for example a cloud VM) to work on.
7. A **terminal version** of Zohara Chat.

## The one decision that shapes everything

"Works with the laptop off" means the brain cannot live only on the laptop. So there are two kinds of work:

| Kind | Needs | Where it runs |
|---|---|---|
| Chat with an API model (OpenRouter, Gemini, ...), memory, web search, personalities | only keys and data | **a cloud server**, always on |
| Tools that touch a computer (files, shell, screenshots, Claude Code with the owner's sign-in) | a real machine | **an agent on that machine** (the laptop, or a VM over SSH) |

A locked laptop still runs its agent, so tools keep working while it is locked. A laptop that is off or asleep cannot run tools; chat still works, and the web says "laptop offline" for tool work, or uses a VM instead.

## Proposed architecture

```
 web app ─┐
 phone  ──┼──> Zohara Chat cloud server (zo-core, always on) ──> model APIs
 terminal ┘        │  Firebase Auth checks who you are
                   │  Firestore keeps chats, memory, settings (sync)
                   └──> machine agents: laptop (outbound link), VMs (SSH)
```

* **zo-core stays the single brain.** It is already a library with a socket service (`service.rs`). The cloud server is the same code
  behind HTTPS + WebSocket instead of a Unix socket. No second implementation of the agent loop.
* **Cloud server**: `zo-server` (new crate, Rust, axum). Host: Render (the owner already has the admin site there) or an Oracle VM
  (free, always on, and can also be the SSH target). Recommendation: the Oracle VM, because Render's free tier sleeps.
* **Login**: Firebase Authentication (Google sign-in). The server verifies the Firebase ID token on every request. (Original plan: one allowed
  account. Now: any Google account, isolated per tenant; the owner's addresses are admins.)
* **Sync**: Firestore, one document tree per user: `chats/`, `memory/`, `personalities/`, `projects/`, `settings`. The desktop app
  keeps its local files and syncs both ways (local first, so it works offline). Conflicts: newest edit wins per chat and per note.
* **API keys**: stored on the server encrypted (a key held in the server's environment, never in Firestore in plain text). The
  desktop keeps using the system keyring and uploads a key only when the owner turns on "use on the web" for it.
* **Laptop agent**: the laptop opens an outbound connection to the server (no port forwarding, works behind any router). The
  existing `zohara-linkd` whitelist idea is reused: the server can only call the listed operations. Runs as a user service, so a
  locked screen does not stop it.
* **Claude Code mode from the web**: the server passes the message to the laptop agent, which runs `claude -p` there with the
  owner's own sign-in, and streams the answer back. The sign-in never leaves the laptop. Needs the laptop on (locked is fine).
* **SSH machines**: a "Machines" page: add host, user and key; tools get a `machine` choice (laptop, vm-1, ...). The shell and
  file tools run over SSH on that machine. The key is stored like an API key.
* **Web app**: a small single-page app (TypeScript, no heavy framework needed) that copies the desktop layout: sidebar, chat,
  projects, settings, memory. Hosted on Firebase Hosting. Talks only to `zo-server`.
* **Terminal version**: `zo` (new small crate using zo-core): `zo "question"`, `zo chat` for an interactive session, same
  memory and chats. It talks to the local service when the app is running, or to the cloud server with `zo --remote`.

## Order of work

1. `zo` terminal client against the local socket (small, proves the client protocol).
2. `zo-server`: the service over HTTPS/WebSocket + Firebase token check. Chat with API models only.
3. Firestore sync for chats and memory (desktop both ways, server reads and writes).
4. Web app: login, chat, chat list, memory and personality pages.
5. Laptop agent link, then Claude Code mode and tools from the web.
6. SSH machines.
7. Phone app switches from the laptop-only link to the server (keeps the direct link as a fallback on the same Wi-Fi).

## What the owner must decide or do

* Where the server runs (Oracle VM recommended; Render possible).
* Create the Firebase project and turn on Google sign-in (the owner's account; I cannot create accounts).
* Agree that API keys are stored encrypted on the server (needed for chat with the laptop off).
* Whether Claude Code mode may also run on the cloud VM with its own sign-in (then it works with the laptop off too).

## Risks

* A server that holds keys and can run commands on the laptop is a high-value target: one allowed account, token check on every
  call, operation whitelist, an audit log, and a kill switch in the desktop app ("disconnect from the web").
* Sync conflicts when two devices write the same chat at once: keep it simple (newest wins) and show both takes as branches.
* Cost: Firestore and the model APIs are pay-per-use; the Oracle VM is free.

## Decisions from the owner (2026-11-12)

* **Public service (later the same day)**: any Google account, bring your own API key, restricted tools for non-admins, device credentials instead of a global token (issued by sign-in, `LOGIN.md`). `PUBLIC.md`.

* **Server: Render**, with a self-ping so the free instance does not sleep. Render's free tier has no disk that survives a
  redeploy, so everything that must last lives in Firestore, never on the server's disk.
* **Firebase project exists.** Needed from the owner: the web app config (public), the service account key (secret: goes into
  Render's environment by the owner, never into the repo or a chat), Google sign-in turned on, Firestore created.
* **Claude Code mode on the server and on VMs** (admin only, never for other users): the server (and any VM added over SSH) runs `claude` itself, signed in with a
  long-lived token the owner makes once with `claude setup-token` and stores as the secret `CLAUDE_CODE_OAUTH_TOKEN`. So
  Claude Code mode works from the web with the laptop off. For the owner's own use only.
* **SSH machines**: the owner adds a host, user and key; Zohara Chat's shell and file tools, and Claude Code, can then run there.
