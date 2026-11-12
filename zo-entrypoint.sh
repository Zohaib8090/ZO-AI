#!/bin/sh
# The container's first process. It runs for an instant: it hands the three secrets to zo-server through a pipe (file descriptor 3,
# read once at start, see crates/zo-server/src/hardening.rs) and then REPLACES itself with tini that has NONE of them in its
# environment. Without this, /proc/1/environ (tini, the same user as zo-server) would still show them to the shell tool of an
# admin session. Run with `sh`, so the file needs no execute bit.
set -e
enc() { printf '%s' "$1" | base64 -w0; }
exec 3<<EOF
FIREBASE_SERVICE_ACCOUNT=$(enc "${FIREBASE_SERVICE_ACCOUNT:-}")
ZO_SERVER_SECRET=$(enc "${ZO_SERVER_SECRET:-}")
CLAUDE_CODE_OAUTH_TOKEN=$(enc "${CLAUDE_CODE_OAUTH_TOKEN:-}")
EOF
export ZO_SECRETS_FD=3
exec env -u FIREBASE_SERVICE_ACCOUNT -u ZO_SERVER_SECRET -u CLAUDE_CODE_OAUTH_TOKEN tini -- "$@"
