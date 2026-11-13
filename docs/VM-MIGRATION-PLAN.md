# Scheduled Test button, schedules on a VM, migration from other tools, MCP logins to VMs, machines popup (owner's request, 2026-11-14)

Status: specified, not built. Order: after the "updates / Play / greetings / persona / questions" workflow and the memory/schedule plan (docs/MEMORY-AND-SCHEDULE-PLAN.md).

## What exists today (facts, checked in the code)

- **SSH machines** (docs/SSH.md): Settings > Machines; tools `zohara_ssh_machines/read_file/write_file/run`; keys in the key store; known_hosts trust-on-first-use. Works from the desktop and, for the admin, from the cloud server (the SSH tools are not filtered out on the server).
- **Chat with every laptop off**: YES, already possible. The cloud server (Render) runs the whole chat loop with the person's own API key (stored encrypted per person), so the phone or web works while the laptop is off. Limits: (1) only models that work with an API key (Claude Code needs the person's own login token on the server, see docs/TERMINAL-AND-CLAUDE-LOGIN.md); (2) a person who is not the owner gets only web search/fetch and memory tools on the server (no shell, no files, no SSH); the owner/admin also gets SSH to added machines; (3) Render's free plan sleeps after idle time (the self-ping keeps it awake; the first request after a sleep can take ~30 s); (4) the desktop-only tools (screenshots, browser, terminals in the window) do not exist there.
- **MCP import**: `mcp_import` takes pasted JSON config text (Settings > Connected apps). There is no detection of other installed tools and no import from files on disk.
- **Scheduled tasks** run only in the desktop app. There is no "Test" button and no VM target.
- No tool lets the AI add a machine for the person; machines are added by hand in Settings.

## 1. Scheduled: Test button in every client

- A **Test** button on each task (desktop, web, Android) and in the New/Edit form: runs the task once now in test mode and shows the output live. Test mode: the run is a normal chat titled "Test: <task>", the task's own schedule/next-run/last-run are NOT changed, side effects follow the task's permission mode (a Test of a task that would write or send still asks first), and a reminder-type task shows the notification it would send. Result card: what it did, the final answer, tools used, time taken, success/failed.
- Before saving a new task the form also shows "next runs: ..." (the next 3 times) so the time is checked without running anything.
- Service ops: `schedule_test {id | draft}`, streamed like a send.

## 2. Scheduled tasks on a VM

- Task field **Run on**: this computer (default), the cloud server (allowed people only), or an added machine (VM).
- Phase 1 (no install on the VM): the host that owns the task (desktop or cloud server) runs it as today and the agent works on the VM through the SSH tools; shown as "runs from <host> on <VM>". Needs the host to be on.
- Phase 2 (the real answer to "laptop off"): a small headless runner `zohara-agent-runner` for Linux VMs: a "Install runner on <VM>" button copies the binary over SSH (checksum verified), creates a systemd user service + timer, stores its own copy of the task list and the API key reference (the key is copied only after an explicit confirmation that names the VM), runs due tasks with the agent loop (no GUI, approvals default to deny except tools the task was created with), writes results to a local log and reports back to the person's account (notification + chat) when the server is reachable. Status of each VM runner (online, last run, version) is visible in the Machines popup. Uninstall button removes everything.
- Missed runs run once on the next start. Per-person limits on the cloud.

## 3. Migration from other tools (detect and import)

- A **Migrate** page (Settings > Import) with detectors that look for config of: Claude Code CLI (`~/.claude.json`, `~/.claude/settings.json`, project `.mcp.json`, `CLAUDE.md` memory files, custom commands/skills names), Claude Desktop (`claude_desktop_config.json` on Linux/macOS/Windows paths), and other common tools where the format is known (Codex CLI `~/.codex/config.toml`, Gemini CLI `~/.gemini/settings.json`, Cursor `~/.cursor/mcp.json`, VS Code `mcp.json`, Zed settings). Read-only detection; nothing is sent anywhere.
- Shows what it found as a checklist: MCP servers (name, command or URL, which env vars/headers it needs), instructions/memory files (offered as memory notes or a project instruction), model/provider settings where they map. The person picks items; import writes them into Zohara Agent's own config, converting formats, skipping duplicates, never overwriting without a confirm, with a summary and an undo.
- Secrets: values found in those files (API keys in env blocks, headers) are copied into the key store only when the person ticks "include secrets" per server; otherwise the server is imported with empty placeholders and a note. Never logged, never synced unless the person turned on key sync.
- Same detectors run for a VM: "Detect on <VM>" reads the same files there over SSH (read-only) and imports from them.
- Cloud: web/phone cannot read the person's disk; the Migrate page on web/Android offers paste/upload of a config file and "import from my computer" through the desktop link (read-only op with local approval, like the other control ops in docs/FULL-CONTROL.md).

## 4. MCP logins and config to VMs without logging in again

- A person adds a VM; Zohara Agent offers "Copy my connected apps to this VM": it copies the MCP server definitions and, per server and only after confirmation, the credentials already stored here (API keys, tokens) to the VM's runner config over the SSH channel (never through our server), file mode 0600, named in the confirmation ("This copies the Notion and GitHub credentials to vm-1"). Servers that need a runtime (node/npx, python/uvx) are checked on the VM first and the missing piece is reported (optionally installed after approval).
- OAuth-style MCP logins: a refresh token copied to a second machine is a second live copy; where the provider supports it, offer "sign in separately on the VM" with the same device-code flow used for Claude login; otherwise copy with a clear warning, a "Revoke on VM" button (deletes the files there) and a note that revoking at the provider invalidates both. Logins that are bound to a browser session/cookie or to the laptop's keyring cannot be moved: say so per server.
- Keeping them in step: when a server is changed here, a "Sync to VMs" action (manual by default, optional auto) pushes the change; removal removes it on the VM.

## 5. "AI, add a VM for me" (low priority)

- Tool `zohara_ssh_add_machine` (asks first, never in restricted mode): the AI asks for host, user, port through the question card, generates a new ed25519 key pair for Zohara Agent (the private key goes to the key store), shows the public key line and the exact one-line command to put it in `authorized_keys`, then runs Test. If the person gives a one-time password it is used once for `ssh-copy-id` and never stored.
- If a task is running and a needed VM is missing, the agent says so and offers this flow instead of failing.

## 6. Machines popup (no need to open Settings)

- A **Machines** popup/sheet reachable from the chat screen (a small server icon next to the model pill on every client): list of VMs with a status dot (reachable now? runner installed?), Test, Open terminal (desktop: terminal tab in the window; web: the terminal page for the owner/admin; Android: terminal page), Add machine (the wizard above), Install runner, Copy connected apps, Run a task here. Settings > Machines stays as the full page.
- Web/phone: the SSH terminal runs from the cloud server and is admin-only (a shell that starts connections from our server's address must not be offered to everyone); other people use the desktop link to reach their own VMs or the runner status only.

## Security notes for the builder

- Never send private keys or MCP credentials to the cloud server unless the person turned on key sync; VM copies go desktop -> VM directly.
- Every credential copy: explicit named confirmation, audit-log entry without the secret, 0600 files, revoke button.
- SSH from the cloud server: admin only (abuse risk from a shared IP).
- Test mode must never skip approvals.
