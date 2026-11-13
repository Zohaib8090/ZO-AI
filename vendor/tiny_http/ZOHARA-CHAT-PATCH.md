tiny_http 0.12.0 from crates.io, with one change (src/connection.rs): accepted TCP connections get a 20 s read timeout, so a client
that sends nothing cannot hold a server thread for ever. Used through [patch.crates-io] in the workspace Cargo.toml.
