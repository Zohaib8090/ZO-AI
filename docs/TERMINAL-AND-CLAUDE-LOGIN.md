# Real terminal + one-click Claude Code login (web, desktop, phone)

Owner's requests (2026-11-13, before compacting the session). Replaces AGY's one-shot `terminal_run` terminal (a command box, not a terminal).

**Status (branch feat/loginterm, sections A and B built; C and the native Android parts are not):** see "What was built" at the end of this file.

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

## What was built (sections A and B), and how to run it

Code: `zo-core/src/pty.rs` (a program on a pseudo-terminal, output kept as raw bytes with sequence numbers), `zo-core/src/claude_login.rs`
(the sign-in flows), `zo-server/src/term.rs` (terminals), `zo-server/src/claude_api.rs` (the sign-in calls), `zo-core/src/link.rs` + `zo-server/src/link.rs`
(a `term` job), `zo-ai/src/claude_ui.rs` (desktop), `zo-server/web/index.html` (the `TERM-BEGIN` and `CLAUDE-LOGIN-BEGIN` sections).
`terminal_run` (the one-shot command op) is gone from the service and the web page.

### A. Claude Code login

* Verified against Claude Code 2.1.322: `claude auth login --claudeai` prints `https://claude.com/cai/oauth/authorize?...` and "Paste code here if
  prompted >"; `claude setup-token` is a full-screen TUI that prints the same kind of link and then the token (`sk-ant-oat01-...`); `claude auth status`
  prints JSON (`loggedIn`, `email`, `planTier`). The programs are run with a 1000-column terminal so that the link is never wrapped. If a later version prints
  something else the flow fails with a plain message and the terminal still works.
* **Desktop (zo-ai)**: Settings > Model > Claude Code and the model sheet show the state (`claude auth status`), with Log in / Log out / Check. Log in opens a sheet:
  the link is a button (opened by the system browser; only sign-in hosts of Anthropic are accepted), the code is pasted into a field and typed into the program,
  success is the program's exit code 0. ZO-AI stores nothing.
* **Server (web)**: `/api/claude/{status,start,poll,code,cancel,logout}`. The server runs `claude setup-token` in a sandbox (empty environment, a private home that is
  deleted afterwards, CPU/file/memory limits, 5 minutes, one flow per person, `ZO_MAX_CLAUDE_LOGINS` flows for the whole server, default 2). The page only receives a
  state, a fixed message and the checked link; the program's screen is never passed on. The token printed at the end is stored encrypted (`claude-code-token`) and
  the program is destroyed. Log out removes the token and the person's `claude-home`.
* **Who may**: `ZO_CLAUDE_CODE_USERS=admins` (default) or `all`. With `all` a restricted person can store a token and use Claude Code on the server, and ONLY with ZO-AI's
  five safe tools: Claude's own tools are off (`--tools ""`), the MCP tools program is started with `ZO_TENANT_RESTRICTED=1` (so it serves the restricted tool set),
  and the operator's fallback token (`CLAUDE_CODE_OAUTH_TOKEN`) is never used for a restricted person. Anthropic's terms do not allow third-party products to offer
  Claude subscription logins: choosing `all` is the operator's decision. Not reviewed by the owner yet: leave it on `admins`.
* **My computer (web)**: Log in opens a terminal on the computer (its owner is asked there first) and types `claude auth login --claudeai`. The guided in-page
  flow through the link (a `claude_login` job) was NOT built: the terminal does the same with less code.

### B. The terminal

* **Transport**: `POST /api/term/open` streams NDJSON (`term`, `out` {seq, b: base64}, `lost`, `ping`, `exit`, `replaced`, `done`); `/api/term/{input,resize,close,list}` are small
  posts. Cloud sessions survive a lost connection: open again with `session` and `from` (the sequence number the page has). The kept output is 1 MiB per shell.
* **Cloud** (`target: "cloud"`): admin only (`ZO_ADMIN_EMAILS`), switch `ZO_TERMINAL=off`. A login shell with an EMPTY environment plus PATH, HOME (outside the people's folders:
  `<ZO_TENANT_ROOT>-terminals/<tenant>`), SHELL, USER, TERM, COLORTERM, LANG. Limits: CPU 4 h, file size 1 GiB, 1024 files, processes = (what the user has now) + 1024,
  writable memory `ZO_TERM_MEM_MB` (2048); 4 per person, `ZO_MAX_TERMINALS` (8) per server; idle 30 minutes, lifetime 8 hours; closing kills the process group AND the whole
  session (jobs of an interactive shell have their own groups). The server marks itself not dumpable, so the shell cannot read its memory, environment or file descriptors through /proc.
  Signal dispositions and the signal mask are reset (a server started as a background job would otherwise pass "ignore SIGINT" on and Ctrl-C would not work).
  Known limits: all people's folders are readable by the shell (same Unix user: the operator is an admin anyway, and the privacy notice says the operator can reach the data);
  a program that starts a session of its own escapes the kill; `/proc/<pid>/environ` of a RUNNING Claude Code child (the token of the person it runs for) is readable by an
  admin's shell. Only admins have this.
* **My computer** (`target: "desktop"`): a `term` job through the link. The desktop asks ITS OWNER on its own screen every time (`Host::ask_local`; no setting lets the web answer; text:
  `TERM_QUESTION`), then runs the person's own login shell in their home on a PTY. Output goes back as link events (base64, at most every 120 ms), typed input comes back in the
  answers to those posts. It closes when the page closes it, the link switch is turned off, the shell ends, or the server cannot be reached for about 20 failed posts. The job carries
  only `rows` and `cols`: there is no command in it. `terminal_run` was never added to the link whitelist. No reconnect for these (the terminal closes with the connection).
* **Page**: xterm.js 6.0.0 from cdnjs with SRI (loaded when the first terminal opens; the CSP allows cdnjs styles for it), tabs (up to 8), fit by measuring the cell size (no addon on
  cdnjs), resize, 5000 lines of scrollback, copy/paste buttons, font size, theme following light/dark, bell, clickable links (also when wrapped over lines), a key row on touch
  screens (Esc Tab Ctrl Alt arrows Home End | ~ / - PgUp PgDn, 44 px), height follows the visual viewport (`--vvh`) so the on-screen keyboard does not cover it, tabs of the cloud
  terminals are picked up again after a reload, automatic reconnect with back-off.

### Not done / not verified (see also the commit message)

Native Android screens, the web layout audit (section C), running the GTK windows (compiled and unit-tested only), the real login with a real code (the flows are tested with stand-in
programs and the real program up to the link), the terminal through the link over a real network, the web page on a real phone.
