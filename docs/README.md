# Zohara Agent documentation index

One line per document. Names: the product is **Zohara Agent** (earlier ZO-AI, then Zohara Chat; the root README has the table of old names), the public service is
https://zohara-agent.duckdns.org, the repository is Zohaib8090/zohara-agent. English only. Start a new working session with `HANDOFF-2026-11-13.md`.

## Using and understanding the product

| Document | What it is |
|---|---|
| [ZOHARA-AGENT.md](ZOHARA-AGENT.md) | What the desktop app is and how it works: model layer, context and compaction, profiles, storage, permission modes |
| [CLI.md](CLI.md) | The terminal client `zo`: one-shot questions, interactive chat, memory, `zo login`, `--remote` |
| [MCP.md](MCP.md) | MCP: Zohara Agent as an MCP client (other people's servers) and as an MCP server (`zo-mcp`) |
| [MEMORY.md](MEMORY.md) | The memory (one store for the app, the phone service and the MCP tools) and the memory agent |
| [ACTIVITY.md](ACTIVITY.md) | What is running now (tray icon, Activity page, web menu) and how to stop it |
| [SSH.md](SSH.md) | Running commands on other computers over SSH (machines) |
| [INTERACTION.md](INTERACTION.md) | Question cards and "needs you" notifications on every platform (the contract) |
| [NOTIFICATIONS.md](NOTIFICATIONS.md) | Notifications, the installable web app (PWA) and background push |

## Accounts, sync and the desktop link

| Document | What it is |
|---|---|
| [LOGIN.md](LOGIN.md) | Signing in (desktop, terminal, web): Google or e-mail, the device flow and its security |
| [SYNC.md](SYNC.md) | Laptop to server sync of chats, memory, personalities, profiles |
| [LINK.md](LINK.md) | The desktop link: "My computer" for the web and phone, approvals on the desktop |
| [FULL-CONTROL.md](FULL-CONTROL.md) | Specification: full control of the desktop from the web and the phone (phase 1 built, the rest specified) |
| [TERMINAL-AND-CLAUDE-LOGIN.md](TERMINAL-AND-CLAUDE-LOGIN.md) | Real terminal and one-click Claude Code login (web, desktop, phone) |

## The public server

| Document | What it is |
|---|---|
| [PUBLIC.md](PUBLIC.md) | The server as a public multi-user service: design, threat model, limits, costs, launch checklist |
| [SERVER.md](SERVER.md) | `crates/zo-server`: routes, the web app, what is built and what is not |
| [DEPLOY.md](DEPLOY.md) | Putting the server on Render with Firebase: owner steps, the public address |
| [PRIVACY.md](PRIVACY.md) | The privacy notice served at `/privacy` (template, to be reviewed; placeholders filled from environment settings) |
| [TERMS.md](TERMS.md) | The terms of use served at `/terms` (template, to be reviewed) |
| [DELETE-ACCOUNT.md](DELETE-ACCOUNT.md) | The public account-deletion page served at `/delete-account` (needed by Google Play) |

## Android

| Document | What it is |
|---|---|
| [ANDROID.md](ANDROID.md) | The native Android app (`apps/android-native`), what is open, the package history; the old Capacitor wrapper at the end |
| [ANDROID-NATIVE-PARITY.md](ANDROID-NATIVE-PARITY.md) | The parity matrix: every feature of the desktop and web app, one row each with its test |
| [PLAY-STORE.md](PLAY-STORE.md) | Every step and owner action to publish on Google Play: account, keys, listing, data safety, testing tracks, checklist |
| `../apps/android-native/README.md` | Building, testing, architecture, Firebase steps of the Android app |
| `../apps/android-native/docs/` | API shapes (`API.md`), module notes (`CHAT.md`, `PAGES.md`, `MEDIA-LINK.md`), `check-parity.py` |
| `../apps/android-native/play/` | Store listing text, graphics, data safety, permissions, content rating, screenshot plan |

## Releasing

| Document | What it is |
|---|---|
| [RELEASING.md](RELEASING.md) | How a tag becomes a GitHub Release (package, tarball, APK, AAB, checksums, `latest.json`), versioning, secrets, keystore |

## History and working notes

| Document | What it is |
|---|---|
| [HANDOFF-2026-11-13.md](HANDOFF-2026-11-13.md) | The handoff for a new session: state, rules, decisions, open problems, owner steps (newest section at the end) |
| [PLAN-WEB-CLI-SYNC.md](PLAN-WEB-CLI-SYNC.md) | The first plan for web, terminal, sync and SSH (historical: those parts are built; read the other documents for the current state) |
| [design/zohara-agent-design.html](design/zohara-agent-design.html) | The design reference (look and feel) |
