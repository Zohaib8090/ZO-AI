# The terminal client `zo` (`crates/zo-cli`)

ZO-AI in the terminal. It does no thinking itself: it talks to ZO-AI's service (zo-core's `service.rs`), so it shares chats,
memory, model choice and permission mode with the app and the phone. Small on purpose: `zo-core` (for the socket path),
`serde_json`, `ureq`, `libc`; no TUI crate, plain stdin/stdout.

## Where it connects

* **Local** (default): the Unix socket `$XDG_RUNTIME_DIR/zo-ai/agent.sock`, served by the ZO-AI desktop app (or a local
  `zo-server`). If nothing listens, `zo` says so and shows how to start the app or use `--remote`.
* **Remote**: `--remote https://server` (or `ZO_REMOTE_URL`; `--local` ignores it). Calls `POST /api/op`, `POST /api/send`
  (one JSON object per line; first line `{"event":"stream","id":N}`) and `POST /api/reply` (approve, stop), with
  `Authorization: Bearer <credential>`.
* **Signing in**: run **`zo login`**. It shows a short code (for example `ABCD-EFGH`) and a web address; open it in any browser, sign in with Google
  or e-mail and password, **type the code**, check the device name and approve. `zo` waits, then keeps the sign-in: nobody copies or pastes a token.
  `zo logout` signs out: the server revokes this device and `zo` forgets it. `zo`
  uses the same sign-in as the desktop app on the same computer and the same default server (`https://zo-ai-server.onrender.com`, overridable with
  `ZO_SERVER_URL` or `--remote`). Flow, endpoints and security: `LOGIN.md`.
* **The credential** behind it is a device credential with scope `full` that the server issued to this device (only a hash is stored there; revoke it
  in the web app under Account > signed-in devices, for example when a computer is lost; it only reaches your own data). It sits in the system key store,
  never in a file or an argument. For scripts and CI without a keyring, `--token-file PATH` or the environment variable named by `--token-env`
  (default `ZO_DEVICE_TOKEN`) still work as a fallback; it is **never accepted as an argument value** (`--token abc` is refused: it would show in the
  process list). The old global `ZO_ACCESS_TOKEN` no longer exists. A server running without login (`ZO_SERVER_NO_AUTH=1`) needs none; a 401 says to run `zo login` again.
  On a public server you are **restricted** unless your address is an admin: the assistant has web search, page fetch and memory only, and
  needs your own API key (set it in the web app's Settings). Computer tools run only against your own local app (`zo` without `--remote`).

## Commands

| | |
|---|---|
| `zo "question"` | One answer. The text streams to stdout; tool steps (`▸ tool ...`, `✓ result`) go to stderr, dimmed. Words after `zo` are joined: `zo what is rust`. If the first word is a command name use `zo ask status of the build`. |
| `echo "q" \| zo` | The question comes from stdin. `cat f \| zo --stdin "summarize"` adds piped text to a question on the command line (not read otherwise, so a script that inherits an open stdin does not hang). |
| `zo chat [chat-id]` | Interactive. Slash commands: `/new`, `/chats`, `/open N` (number from `/chats`, or an id), `/model [name]`, `/mode [Manual\|Auto\|Bypass]`, `/memory`, `/stop`, `/help`, `/quit`. Opening a chat shows its last messages. |
| `zo chats`, `zo show <id>` | List and print saved chats. |
| `zo memory`, `zo remember "text"`, `zo forget <id>` | Long-term memory. |
| `zo login`, `zo logout` | Sign in (browser code or e-mail and password) and out; see above. Not needed for the local app. |
| `zo status` | Provider, model, permission mode, version. |
| `zo models [provider]`, `zo use <provider> [model]` | Providers and models; choose one. |
| `zo mode [Manual\|Auto\|Bypass]` | Show or set the permission mode. |

Options: `-c/--chat ID` (continue a chat with a one-shot question), `--mode MODE` (for this question only: the saved mode that the app and the phone share is not changed), `--no-color`, `--help`, `--version`.
`zo ssh ...` is reserved; machines over SSH are not in this client yet.

## Approvals, Ctrl-C, exit codes

* When the AI wants to run something the permission mode asks about, `zo` prints what it is and asks `Allow? [y/N]`. With no
  terminal to ask on (stdin is piped) it answers no and says so; use `--mode Bypass` for unattended runs.
* **Ctrl-C** during an answer sends Stop; a second Ctrl-C quits (exit 130). At the `zo chat` prompt the first Ctrl-C only
  hints, the second quits. Ctrl-D quits. Typing `/stop` while an answer runs also stops it.
* Colours only when the stream is a terminal and `NO_COLOR` is unset (`--no-color` forces them off).
* Exit codes: 0 ok, 1 error (also an `error` event from the service), 2 usage, 130 interrupted.

## Code layout

* `proto.rs`: no I/O. Parses event lines (`Event`), builds requests, `Renderer` (draws events; skips the final `text` when
  the same text already streamed as `delta`s), small helpers. Unit tested.
* `net.rs`: `Target::Local` / `Target::Remote`; `op()` and `send()` giving a `Stream` (events on a channel, `reply()` for
  approve/stop). Dropping a local stream closes the socket, which stops the answer.
* `main.rs`: arguments, commands, `run_turn` (one answer: events, approvals, Ctrl-C), the chat loop. Tests drive `run_turn`
  with a fake stream.

## Tested

Unit tests (`cargo test -p zo-cli`): event parsing, request building, rendering, argument parsing, the token refusal and
token sources, approval y/n/denied, Ctrl-C once and twice, a broken connection.
End to end (2026-11-12), against a sandboxed `zo-server` (`ZO_SERVER_NO_AUTH=1`, its local socket and `--remote`) with a
fake OpenAI-compatible streaming model: no model chosen (clean error, exit 1), a streamed answer, piped question, a tool
call with the approval answered y and n on a pseudo-terminal (the file was only created after y), denied without a terminal,
Ctrl-C once (stopped) and twice (exit 130), the interactive chat with its slash commands, memory, models, mode, error paths.
Not tested: a real Firebase/Render server with a real credential, and the `zo login` flow against a real server (`LOGIN.md`; the client sends `Authorization: Bearer <credential>` as specified), real models, Claude Code mode, Windows/macOS.
