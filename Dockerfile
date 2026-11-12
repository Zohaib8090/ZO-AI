# ZO-AI server for Render. Only the server and zo-mcp are built: the desktop app (zo-ai) needs GTK and is not part of this image.
# zo-mcp is the program Claude Code connects to; zo-server looks for it next to itself.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p zo-server -p zo-mcp

# The Node image already has node and npm (Debian's own npm package pulls in about 400 packages).
FROM node:22-bookworm-slim
# ca-certificates: HTTPS to the model providers and Firestore. git, ssh: tools the AI uses. tini: a small init. claude-code: Claude Code mode.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git openssh-client tini \
    && npm install -g @anthropic-ai/claude-code \
    && npm cache clean --force \
    && rm -rf /var/lib/apt/lists/* /root/.npm
# A normal user (not root). Everything the service writes goes under paths this user owns:
# data and config under HOME, and the socket in a short XDG_RUNTIME_DIR (unix socket paths are limited to about 100 characters).
RUN useradd -m -u 10001 zo && mkdir -p /tmp/zo-run && chown zo:zo /tmp/zo-run && chmod 700 /tmp/zo-run
USER zo
ENV HOME=/home/zo \
    XDG_DATA_HOME=/home/zo/.local/share \
    XDG_CONFIG_HOME=/home/zo/.config \
    XDG_RUNTIME_DIR=/tmp/zo-run
RUN mkdir -p /home/zo/.local/share /home/zo/.config /home/zo/.claude
WORKDIR /home/zo
COPY --from=build /src/target/release/zo-server /src/target/release/zo-mcp /usr/local/bin/
COPY zo-entrypoint.sh /usr/local/bin/zo-entrypoint.sh
# Render sets PORT and uses healthCheckPath from render.yaml; this line is for running the image anywhere else.
HEALTHCHECK --interval=60s --timeout=5s --start-period=20s CMD curl -fsS "http://127.0.0.1:${PORT:-8787}/healthz" || exit 1
# tini is process 1: it passes the stop signal on, so a redeploy ends the server at once. The entrypoint script gives the secrets to
# zo-server through a pipe and starts tini WITHOUT them in its environment (/proc/1/environ is readable by the same user).
ENTRYPOINT ["sh", "/usr/local/bin/zo-entrypoint.sh"]
CMD ["zo-server"]
