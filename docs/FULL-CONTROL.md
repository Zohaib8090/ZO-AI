# Full control of the desktop from the web and the phone, everything in sync

Owner's request (2026-11-13): the web version must be able to take FULL control of the person's desktop version (the same
power the desktop app has), the mobile app must have the same, and everything must stay in sync. This file is the
specification. Status: phase 1 built (read-only: system status, file browsing and reading, screenshot; see the end of docs/LINK.md); everything else is specified, not built (the link foundation exists: see docs/LINK.md).

## What exists today

* Desktop link (docs/LINK.md): the desktop app, when "Control this computer from the web" is on, long-polls the server;
  the web (and later the phone) can run a chat turn on it (`target:'desktop'`) with all desktop tools and the person's own
  Claude Code login, and use a short whitelist of settings operations (status, providers, profiles, profile_use, models,
  select, mode). Approvals for changes are asked ON THE DESKTOP. Presence check ("is the desktop app running?") exists.
* Sync (docs/SYNC.md): chats, memory notes, personalities, profiles + pinned models, UI prefs, remote MCP addresses, and
  (opt-in) API keys are kept equal between desktop, web and phone through the server.
* A critical hole was found and fixed on 2026-11-12: a `terminal_run` op in the link whitelist ran shell commands on the
  desktop with NO approval. Never add an op to the link whitelist without the approval path below.

## What "full control" means (the feature list)

From the web page or the native phone app, with "My computer" selected, the person can:

1. **Chat with all tools** (exists): files, shell, screenshots, adb/Waydroid, browser, terminals, schedules, MCP servers, Claude Code.
2. **Terminal**: a live terminal on the desktop (open, type, read, close, several tabs), output streamed back.
3. **Files**: browse folders, read text, download a file (size cap), upload a file to a chosen folder, create/rename/delete.
4. **Screen**: take a screenshot of the desktop and view it; optional live view later (not now).
5. **Apps and system**: list/launch/close applications, see CPU/memory/disks, power actions (lock, sleep; shutdown/reboot only with extra confirmation).
6. **Zohara Chat itself on the desktop**: open/rename/delete chats there, run schedules, the Activity list with Stop (activity op), projects, artifacts, memory, personalities, permission mode (never above Auto without local consent), model/profile choice, MCP servers (add/remove on the desktop), settings pages.
7. **Notifications**: desktop events (answer finished, approval needed, question asked, a scheduled task ran) reach the phone and the web as notifications (docs/INTERACTION.md).
8. **Sync**: everything the person changes on any device appears on the others (existing sync kinds + live ops through the link); chats made on the desktop by a web session are saved on the desktop and synced to the server like any other chat.

## Security model (non-negotiable)

* Default OFF; per desktop; the person turns it on in Settings > Account and sync > Control this computer from the web; a visible indicator (tray "Web control: on") and an immediate kill switch (tray + settings) that also revokes the device's link credential.
* **Every operation is classified** by the same policy code as the agent tools (read / change / shell / system):
  read-only operations may run without asking; anything that changes the computer needs a LOCAL approval dialog on the desktop
  (and a notification), unless the person granted a **time-limited trusted session** (for example 10 minutes, set locally in
  the dialog "Allow web control without asking for 10 minutes", capped, never for shutdown/reboot/delete-outside-home) .
  Approvals from the web are accepted only if the person enabled "Approve from the web" locally (off by default).
* The server is untrusted input: jobs are validated (size, type, op whitelist, age <= 2 minutes, tenant), the whitelist is
  per operation with exact argument schemas, nothing is executed from free-form text except through the agent loop's own policy.
* Every web-initiated action is written to the desktop's audit log and shown in the Activity page with Stop.
* Mode cap: the permission mode for web jobs never exceeds Auto; Bypass only if allowed locally.
* Files: a root allow-list (home folder by default), no access to ~/.ssh, ~/.gnupg, the Zohara Chat config folder, keyring, /proc/*/environ, other users' homes; size caps; no symlink escapes (resolve and re-check).
* Terminal: runs as the desktop user with the scrubbed environment (no Zohara Chat secrets), one session per tab, idle timeout, output size cap, local approval to OPEN a terminal (then typed commands pass through the policy classifier: obviously destructive commands re-ask).
* Rate limits and per-device single active job (existing), poll slot limits (existing).

## Protocol additions (to design in docs/LINK.md when built)

New link job types beyond `send` and `op`: `term_open/term_input/term_close`, `fs_list/fs_read/fs_write/fs_mkdir/fs_rm/fs_move`,
`screenshot`, `app_list/app_launch/app_close`, `system_info`, `power`, with their approval classes, plus streaming of terminal
output as events. The web and the native app use the same `/api/link/*` endpoints through `desktop_op`-style calls.

## Build order

1. Spec the exact op list and argument schemas in docs/LINK.md; classification table; tests that every op has a class.
2. Desktop: local approval dialog with trusted-session grant, audit log, Activity integration, tray kill switch.
3. Terminal + files + screenshot ops (desktop executes, server relays, web and phone UIs).
4. Native Android screens for the same (against the parity matrix: docs/ANDROID-NATIVE-PARITY.md).
5. Security review (RCE lens first), then enable by default only for people who turn it on.
