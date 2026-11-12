# Real terminal + one-click Claude Code login (web, desktop, phone)

Owner's requests (2026-11-13, before compacting the session). Status: specified, not built. Replaces AGY's one-shot
`terminal_run` terminal (a command box, not a terminal).

## A. One-click Claude Code login

Problem: nowhere in the web version could the owner log in to Claude Code; he wants a direct option in the web version AND the
desktop version that does the login quickly, so that his Claude subscription models can be used from every client.

Entry points (all three open the same flow):
1. Model and API key sheet: the provider "Claude Code" shows a status (Not logged in / Logged in as <account> / Token on server) and a **Log in** button.
2. Settings > Model and profiles > "Claude Code" card (status, Log in, Log out, Test).
3. A hint banner when the person selects Claude Code without being logged in.

Flows (the person only sees steps, never a token):
* **On my computer (desktop app, always available):** the desktop app runs `claude auth login` in an embedded terminal sheet (a real PTY),
  shows the authorization link as a button, waits for the CLI to finish, then checks `claude auth status` and shows the account.
  From the web or phone the same login can be started through the link (a `claude_login` job; local approval on the desktop; the link
  text the CLI prints is relayed so the person can open it on the phone; the code they get is pasted back into the web page and
  written to the CLI's stdin).
* **On the server (only if the operator allows it: `ZO_CLAUDE_CODE_USERS`; always for admins):** a guided in-page flow: the server runs
  `claude setup-token` in a sandboxed PTY (tenant HOME, scrubbed environment, no tools), streams its prompt to the page, shows the
  authorization URL as a button and a field for the code, writes the code to the PTY, captures the long-lived token, stores it in the
  tenant key store (encrypted, id `claude-code-token`), destroys the PTY. One flow per person at a time, 5 minute timeout, global cap,
  token never shown or logged. Verify the exact CLI behaviour against Anthropic's current docs before coding (flags, prompts, token format).
* Status and Log out (delete the token and the tenant `claude-home`) in both places. Per-person tokens follow the privacy/terms texts
  and the Anthropic-terms caveat in docs/HANDOFF-2026-11-13.md (operator decision for `ZO_CLAUDE_CODE_USERS=all`).

## B. A real terminal ("hand me a logged-in terminal I can control everything with")

* **UI (web and native app):** xterm.js on the web (pinned cdnjs version with SRI, fit + web-links add-ons), a native terminal view on
  Android (a maintained terminal emulator library). Tabs, resize, 256 colours, copy/paste, selection, font size, dark/light colours
  matching the app, Ctrl-C and arrow keys, an extra key row on phones (Esc, Tab, Ctrl, Alt, arrows, |, ~, /, -), reconnect to a
  running session after a network drop (session kept 30 minutes idle), scrollback, bell, links clickable.
* **Targets:** (1) **Cloud**: a real login shell (PTY) on the server container as an unprivileged per-tenant user environment: HOME in the
  tenant dir, scrubbed environment (no ZO-AI secrets), PATH with `claude`, `git`, `ssh`; ADMIN ONLY (restricted users never get a shell
  on the shared server); resource limits (setrlimit: processes, memory, file size, CPU), idle timeout, output caps, process group kill,
  audit log. (2) **My computer** (any signed-in person, through the desktop link): a PTY on their own desktop using zo-core's
  existing terminal code (`term.rs`, vt100), opened only after a LOCAL approval (FULL-CONTROL.md), output streamed back as link events,
  input sent as link jobs; closes when the link goes offline or the person presses the kill switch.
* **Transport:** the server is tiny_http (no WebSocket): long-lived streaming response for output frames (NDJSON with base64 payloads)
  plus POST for input/resize/close, with a session id; both through the same auth and rate limits as `/api/send`.
* **Login inside the terminal:** because it is a real shell, `claude auth login`, `gh auth login`, `ssh`, `git` all work; for Claude Code
  also offer the one-click flow (section A).
* **Security:** as in docs/FULL-CONTROL.md; the cloud terminal is the most dangerous feature of the server: review with the RCE lens before enabling; never expose the server's own secrets (they are removed from the process environment at start, keep it that way: test that `env` and `/proc/*/environ` in the terminal show nothing).

## C. Web on Android screens

The web version looked wrong on an Android phone (AGY's attempt at an Android app wrapped it and it did not fit). Two fixes:
1. The native app (apps/android-native, parity matrix) is the real answer; AGY's WebView draft is rejected.
2. Independently, audit the web page in Android Chrome sizes (360x800, 412x915, tablets, landscape, with the on-screen keyboard open, with
   display zoom): AGY's side-panel dock and context sheet must collapse on small screens, the composer must stay visible above the
   keyboard (visualViewport), no horizontal scroll, tap targets >= 44 px, sidebar overlay works, terminal and sheets fit. Fix what breaks.

## Build order

1. Real PTY terminal on the desktop (exists in zo-core) exposed through the link with local approval; web xterm.js UI; cloud terminal for admins.
2. Claude Code login flows (desktop first, then server-guided, then via the link).
3. Native Android terminal and login screens (parity matrix rows).
4. Web mobile layout audit and fixes.
5. Security review (RCE lens) before enabling.
