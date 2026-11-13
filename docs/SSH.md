# SSH machines

Zohara Chat can run commands on other computers (a cloud VM, a server at home) over SSH, read and write files there, and start
Claude Code on them.

## Add a machine

Settings > Machines > Add machine. Give it:

- a **name** (letters, digits, `.` `_` `-`; this is what the AI uses),
- the **address** (a host name or IPv4 address), **user** and **port** (22 by default),
- the **private key** text (from `-----BEGIN` to `-----END`; "Paste key" fills it from the clipboard). The key must have no
  passphrase (Zohara Chat cannot type one). Make one for Zohara Chat with `ssh-keygen -t ed25519 -N ""` and put the `.pub` file in the
  machine's `~/.ssh/authorized_keys`.

"Test" on a machine runs `echo ok` there and shows the answer or the error. The same exists as service ops: `machines`,
`machine_add`, `machine_remove`, `machine_test`.

## What the AI may do

| Tool | What | Asks first? |
| --- | --- | --- |
| `zohara_ssh_machines` | list machines | no |
| `zohara_ssh_read_file` | read a file (first 100 KB) | no |
| `zohara_ssh_write_file` | write or append a file (1 MB at most) | yes, unless Bypass |
| `zohara_ssh_run` | run a command (timeout up to 600 s) | yes, unless Bypass |

`zohara_ssh_run` is classified like `zohara_shell`; the "safe read-only command" shortcut of Auto mode does not apply to it, so
it always asks outside Bypass. Reading a file never asks, like `zohara_fs_read`, so a machine's files are readable by the AI
without a prompt: add only machines you accept that for. To start Claude Code on a machine (it must be installed there) the AI
runs `claude -p "<task>"` with a long `timeout_s`.

Output of a command is cut at 20 KB (a read file at 100 KB), with a note. There is no stop button for a running remote command
yet: it ends when it finishes or at its timeout (the whole local ssh process group is then killed).

## Where things are kept

- `~/.config/zohara-chat/machines.json`: name, host, user, port, note. No key.
- The private key: the key store (`ai::keys`, id `ssh-<name>`): the system keyring, or `ZOHARA_CHAT_KEYS_FILE` on a server. No
  operation returns it; they only say `has_key`.
- `~/.config/zohara-chat/known_hosts`: host keys, trusted on first use (`StrictHostKeyChecking=accept-new`); a changed host key is
  refused. Zohara Chat does not read or write the person's own `~/.ssh/known_hosts`.

## Security notes

- Name, host and user are checked against `[A-Za-z0-9._-]`, must not start with `-` or `.`, and are length-limited; the port is
  1..65535. So nothing can become an ssh option (no `-oProxyCommand=...`), and there is no whitespace, quote or shell character.
  IPv6 addresses are not supported for that reason.
- `ssh` is started with an argument list, never through a local shell. The destination follows `--`; the remote command is the
  last single argument. It is the only text a (remote) shell reads, and paths are shell-quoted by a tested function.
- ssh options: `-F /dev/null` (ignore the person's ssh config), `BatchMode=yes`, `ConnectTimeout=15`, `IdentitiesOnly=yes`,
  `IdentityAgent=none`, `-i <temporary key file>`, `-T`.
- The temporary key file is created `0600` in `$XDG_RUNTIME_DIR/zohara-chat` (a `0700` folder; `/tmp/zohara-chat` if there is none) and is
  removed as soon as the process ends, also on timeout or error. The key text is never in an argument or an environment variable
  and never in the log (the audit log records tool arguments, which never contain a key).
- A remote command can of course do anything the remote user can. Use a user with only the rights the job needs.
