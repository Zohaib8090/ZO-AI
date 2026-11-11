# ZO-AI server (crates/zo-server) for Render. Only the server is built: the desktop app needs GTK and is not part of this image.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p zo-server

FROM debian:bookworm-slim
# ca-certificates: HTTPS to the model providers and Firestore. git, ssh: tools the AI uses. nodejs + claude-code: Claude Code mode.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl git openssh-client nodejs npm \
    && npm install -g @anthropic-ai/claude-code \
    && rm -rf /var/lib/apt/lists/* /root/.npm
RUN useradd -m zo
USER zo
ENV HOME=/home/zo
WORKDIR /home/zo
COPY --from=build /src/target/release/zo-server /usr/local/bin/zo-server
CMD ["zo-server"]
