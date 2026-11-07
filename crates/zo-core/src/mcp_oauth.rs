//! Signing in to a remote MCP server that asks for it (the MCP "OAuth" way): find out where the server's sign-in lives, register
//! ZO-AI there, open the person's browser, catch the answer on a local address and keep the tokens in the system keyring.
//! Nothing is stored in a file; the tokens belong to the keyring entry `mcp-<server name>`.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// What is kept in the keyring for a signed-in server.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub access: String,
    pub refresh: Option<String>,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub token_endpoint: String,
    /// The address of the MCP server the tokens are for (sent as `resource`, RFC 8707).
    pub resource: Option<String>,
    /// Unix time at which the access token runs out, when the server said.
    pub expires_at: Option<u64>,
    /// The server takes the client secret in a Basic header rather than in the form.
    pub basic_auth: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Saved {
    pub fn to_json(&self) -> String {
        json!({ "access": self.access, "refresh": self.refresh, "client_id": self.client_id, "client_secret": self.client_secret, "token_endpoint": self.token_endpoint,
            "resource": self.resource, "expires_at": self.expires_at, "basic_auth": self.basic_auth }).to_string()
    }

    /// The access token ends within five minutes (so it is renewed before it is used).
    pub fn nearly_expired(&self) -> bool {
        self.expires_at.is_some_and(|t| t <= now() + 300)
    }

    /// A keyring value is either this JSON or just a plain access token the person pasted.
    pub fn parse(stored: &str) -> Option<Saved> {
        let v: Value = serde_json::from_str(stored).ok()?;
        Some(Saved {
            access: v["access"].as_str()?.to_string(),
            refresh: v["refresh"].as_str().map(str::to_string),
            client_id: v["client_id"].as_str()?.to_string(),
            client_secret: v["client_secret"].as_str().map(str::to_string),
            token_endpoint: v["token_endpoint"].as_str()?.to_string(),
            resource: v["resource"].as_str().map(str::to_string),
            expires_at: v["expires_at"].as_u64(),
            basic_auth: v["basic_auth"].as_bool().unwrap_or(false),
        })
    }
}

/// What the person told ZO-AI about signing in to one server (all optional).
#[derive(Clone, Debug, Default)]
pub struct SignInOptions {
    /// A client id registered for ZO-AI at the server, for servers that do not register apps by themselves.
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    /// A fixed local port for the sign-in to come back to (http://127.0.0.1:PORT/callback).
    pub callback_port: Option<u16>,
    /// Scopes to ask for, instead of the ones the server suggests.
    pub scope: Option<String>,
}

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout(Duration::from_secs(30)).build()
}

// ---------------------------------------------------------------- small pieces (no extra crates)

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        if c.len() > 1 {
            out.push(A[(n >> 6) as usize & 63] as char);
        }
        if c.len() > 2 {
            out.push(A[n as usize & 63] as char);
        }
    }
    out
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    base64url(&buf)
}

/// `code_challenge` for PKCE (S256).
pub fn challenge(verifier: &str) -> String {
    base64url(&sha256(verifier.as_bytes()))
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn origin(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let scheme = url.split("://").next().unwrap_or("https");
    format!("{scheme}://{}", rest.split('/').next().unwrap_or(rest))
}

/// The address of the page that describes the server's sign-in, from a `WWW-Authenticate: Bearer resource_metadata="…"` header.
pub fn metadata_url(www_authenticate: &str) -> Option<String> {
    let at = www_authenticate.find("resource_metadata=\"")? + "resource_metadata=\"".len();
    let end = www_authenticate[at..].find('"')?;
    Some(www_authenticate[at..at + end].to_string())
}

fn get_json(url: &str) -> Result<Value, String> {
    http().get(url).set("Accept", "application/json").call().map_err(|e| format!("Could not read {url}: {e}"))?.into_json::<Value>().map_err(|e| format!("{url} is not JSON: {e}"))
}

// ---------------------------------------------------------------- the sign-in

/// Signs in to the remote server at `mcp_url`. `open` shows the sign-in page to the person (it opens their browser).
pub fn sign_in(server: &str, mcp_url: &str, opts: &SignInOptions, open: &dyn Fn(&str)) -> Result<(), String> {
    // 1. where does this server say its sign-in is?
    // The address may be the site itself (https://name.example.com) rather than its MCP address (…/mcp): try both.
    let trimmed = mcp_url.trim_end_matches('/');
    let candidates = if trimmed.matches('/').count() == 2 { vec![format!("{trimmed}/mcp"), trimmed.to_string()] } else { vec![trimmed.to_string()] };
    let mut found: Option<(String, String)> = None;
    let mut last = String::from("Could not reach the server.");
    for url in &candidates {
        let probe = http().post(url).set("Accept", "application/json, text/event-stream").send_json(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "zo-ai", "version": "0" } } }));
        match probe {
            Err(ureq::Error::Status(401, r)) => {
                found = Some((url.clone(), r.header("www-authenticate").unwrap_or("").to_string()));
                break;
            }
            Ok(_) => return Err("This server does not ask you to sign in.".into()),
            Err(ureq::Error::Status(code, _)) => last = format!("The server answered with error {code}."),
            Err(e) => last = format!("Could not reach the server: {e}"),
        }
    }
    let (mcp_url, header) = found.ok_or(last)?;
    let mcp_url = mcp_url.as_str();
    let meta_url = metadata_url(&header).unwrap_or_else(|| format!("{}/.well-known/oauth-protected-resource", origin(mcp_url)));
    let resource_meta = get_json(&meta_url).ok();
    let auth_server = resource_meta.as_ref().and_then(|m| m["authorization_servers"][0].as_str().map(str::to_string)).unwrap_or_else(|| origin(mcp_url));
    let issuer = auth_server.trim_end_matches('/').to_string();
    let meta = get_json(&format!("{issuer}/.well-known/oauth-authorization-server")).or_else(|_| get_json(&format!("{issuer}/.well-known/openid-configuration")))?;
    let authorize = meta["authorization_endpoint"].as_str().ok_or("The server's sign-in has no authorization address.")?.to_string();
    let token_endpoint = meta["token_endpoint"].as_str().ok_or("The server's sign-in has no token address.")?.to_string();

    // 2. a local address to catch the answer, and ZO-AI registered as a client
    let listener = TcpListener::bind(("127.0.0.1", opts.callback_port.unwrap_or(0))).map_err(|e| format!("Could not open the local address for the sign-in (port {}): {e}", opts.callback_port.map_or("any".to_string(), |p| p.to_string())))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}/callback", listener.local_addr().map_err(|e| e.to_string())?.port());
    let methods = meta["token_endpoint_auth_methods_supported"].as_array().cloned().unwrap_or_default();
    let has = |m: &str| methods.iter().any(|x| x == m);
    // the server takes the secret in a Basic header only when it lists that method and not the form one
    let basic_auth = has("client_secret_basic") && !has("client_secret_post") && !has("none");
    let (client_id, client_secret) = if let Some(id) = opts.client_id.clone() {
        // a client the person registered by hand: no automatic registration
        (id, opts.client_secret.clone())
    } else {
        let reg = meta["registration_endpoint"].as_str().ok_or_else(|| format!("This server does not register apps by itself. Register ZO-AI there, with the redirect address {redirect} (a fixed port can be set in the server's settings), and enter the client id."))?;
        // a server that lists the methods it takes and does not list "none" gets a client secret instead
        let auth_method = if methods.is_empty() || has("none") { "none" } else if basic_auth { "client_secret_basic" } else { "client_secret_post" };
        let registered: Value = http()
            .post(reg)
            .send_json(json!({ "client_name": "ZO-AI", "redirect_uris": [redirect], "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"], "token_endpoint_auth_method": auth_method }))
            .map_err(|e| format!("The server did not accept ZO-AI: {e}"))?
            .into_json()
            .map_err(|e| e.to_string())?;
        (registered["client_id"].as_str().ok_or("The server gave no client id.")?.to_string(), registered["client_secret"].as_str().map(str::to_string))
    };
    // which permissions to ask for: yours, else what the server's 401 named, else everything it lists; offline_access (a refresh token) when allowed
    let listed = |v: &Value| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let mut scopes: Vec<String> = match (opts.scope.as_deref(), scope_in(&header)) {
        (Some(own), _) if !own.trim().is_empty() => own.split_whitespace().map(str::to_string).collect(),
        (_, Some(from_401)) => from_401.split_whitespace().map(str::to_string).collect(),
        _ => resource_meta.as_ref().map(|m| listed(&m["scopes_supported"])).unwrap_or_default(),
    };
    if listed(&meta["scopes_supported"]).iter().any(|s| s == "offline_access") && !scopes.iter().any(|s| s == "offline_access") {
        scopes.push("offline_access".into());
    }

    // 3. send the person to the sign-in page and wait for them to come back
    let verifier = random_token(48);
    let state = random_token(16);
    let url = format!(
        "{authorize}{}response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&resource={}{}",
        if authorize.contains('?') { "&" } else { "?" },
        encode(&client_id),
        encode(&redirect),
        challenge(&verifier),
        encode(&state),
        encode(mcp_url),
        if scopes.is_empty() { String::new() } else { format!("&scope={}", encode(&scopes.join(" "))) }
    );
    open(&url);
    let code = wait_for_code(&listener, &state, Duration::from_secs(240))?;

    // 4. trade the code for the tokens
    let form = vec![("grant_type", "authorization_code"), ("code", code.as_str()), ("redirect_uri", redirect.as_str()), ("code_verifier", verifier.as_str()), ("resource", mcp_url)];
    let tokens = token_request(&token_endpoint, &client_id, client_secret.as_deref(), basic_auth, &form)?;
    let access = tokens["access_token"].as_str().ok_or("The server answered without a token.")?.to_string();
    let saved = Saved {
        access,
        refresh: tokens["refresh_token"].as_str().map(str::to_string),
        client_id,
        client_secret,
        token_endpoint,
        resource: Some(mcp_url.to_string()),
        expires_at: tokens["expires_in"].as_u64().map(|s| now() + s),
        basic_auth,
    };
    crate::ai::keys::set(&crate::mcp_client::token_id(server), &saved.to_json())
}

fn wait_for_code(listener: &TcpListener, state: &str, wait: Duration) -> Result<String, String> {
    let end = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let query = req.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").split('?').nth(1).unwrap_or("").to_string();
                let get = |k: &str| query.split('&').find_map(|p| p.strip_prefix(&format!("{k}=")).map(decode));
                let page = |text: &str| format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<html><body style=\"font-family:sans-serif;background:#191919;color:#ececec;text-align:center;padding-top:20vh\"><h2>{text}</h2><p>You can close this tab and go back to ZO-AI.</p></body></html>");
                if let Some(err) = get("error") {
                    let _ = stream.write_all(page("Sign-in was not completed.").as_bytes());
                    return Err(format!("The sign-in was refused: {err}"));
                }
                match (get("code"), get("state")) {
                    (Some(code), Some(s)) if s == state => {
                        let _ = stream.write_all(page("Signed in.").as_bytes());
                        return Ok(code);
                    }
                    _ => {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > end {
                    return Err("Nobody signed in within a few minutes, so nothing was saved.".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The `scope="…"` of a `WWW-Authenticate` header.
pub fn scope_in(www_authenticate: &str) -> Option<String> {
    let at = www_authenticate.find("scope=\"")? + "scope=\"".len();
    let end = www_authenticate[at..].find('"')?;
    Some(www_authenticate[at..at + end].to_string()).filter(|s| !s.is_empty())
}

/// One request to the token address, with the client's credentials sent the way the server wants them.
fn token_request(endpoint: &str, client_id: &str, client_secret: Option<&str>, basic: bool, form: &[(&str, &str)]) -> Result<Value, String> {
    let mut fields: Vec<(&str, &str)> = form.to_vec();
    let mut req = http().post(endpoint);
    match (client_secret, basic) {
        (Some(secret), true) => {
            let pair = format!("{}:{}", encode(client_id), encode(secret));
            req = req.set("Authorization", &format!("Basic {}", base64_std(pair.as_bytes())));
        }
        (Some(secret), false) => {
            fields.push(("client_id", client_id));
            fields.push(("client_secret", secret));
        }
        (None, _) => fields.push(("client_id", client_id)),
    }
    match req.send_form(&fields) {
        Ok(r) => r.into_json::<Value>().map_err(|e| e.to_string()),
        Err(ureq::Error::Status(code, r)) => {
            let body: Value = r.into_json().unwrap_or(Value::Null);
            let why = body["error_description"].as_str().or_else(|| body["error"].as_str()).unwrap_or("no reason given");
            Err(if body["error"] == "invalid_grant" { format!("The sign-in is no longer valid ({why}). Sign in again.") } else { format!("The server refused the token request ({code}: {why}).") })
        }
        Err(e) => Err(format!("Could not reach the token address: {e}")),
    }
}

fn base64_std(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Trades the refresh token for a new access token; returns the updated saved record (already stored in the keyring).
pub fn refresh(server: &str, saved: &Saved) -> Result<Saved, String> {
    let rt = saved.refresh.as_deref().ok_or("There is no refresh token; sign in again.")?;
    let mut form = vec![("grant_type", "refresh_token"), ("refresh_token", rt)];
    if let Some(r) = saved.resource.as_deref() {
        form.push(("resource", r));
    }
    let tokens = token_request(&saved.token_endpoint, &saved.client_id, saved.client_secret.as_deref(), saved.basic_auth, &form)?;
    let mut next = saved.clone();
    next.access = tokens["access_token"].as_str().ok_or("The server answered without a token.")?.to_string();
    if let Some(r) = tokens["refresh_token"].as_str() {
        next.refresh = Some(r.to_string());
    }
    next.expires_at = tokens["expires_in"].as_u64().map(|s| now() + s);
    crate::ai::keys::set(&crate::mcp_client::token_id(server), &next.to_json())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_and_the_pkce_challenge_match_the_published_examples() {
        let hex: String = sha256(b"abc").iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long: String = sha256(&[b'a'; 1000]).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(long, "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
        // RFC 7636, appendix B
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn addresses_and_headers_are_read_the_way_the_standard_says() {
        assert_eq!(metadata_url("Bearer error=\"invalid_token\", resource_metadata=\"https://a.b/.well-known/x\""), Some("https://a.b/.well-known/x".into()));
        assert_eq!(metadata_url("Bearer"), None);
        assert_eq!(origin("https://a.b/mcp"), "https://a.b");
        assert_eq!(encode("http://127.0.0.1:1/callback"), "http%3A%2F%2F127.0.0.1%3A1%2Fcallback");
        assert_eq!(decode("a%20b%2Fc+d"), "a b/c d");
    }

    #[test]
    fn the_scope_of_a_challenge_and_the_basic_header_are_read_right() {
        assert_eq!(scope_in("Bearer error=\"insufficient_scope\", scope=\"read write\", resource_metadata=\"x\""), Some("read write".into()));
        assert_eq!(scope_in("Bearer realm=\"x\""), None);
        assert_eq!(base64_std(b"user:pass"), "dXNlcjpwYXNz");
        let near = Saved { access: "a".into(), refresh: None, client_id: "c".into(), client_secret: None, token_endpoint: "t".into(), resource: None, expires_at: Some(now() + 100), basic_auth: false };
        assert!(near.nearly_expired() && !Saved { expires_at: Some(now() + 4000), ..near.clone() }.nearly_expired() && !Saved { expires_at: None, ..near }.nearly_expired());
    }

    #[test]
    fn what_is_kept_in_the_keyring_round_trips_and_a_plain_token_is_not_mistaken_for_it() {
        let s = Saved { access: "a".into(), refresh: Some("r".into()), client_id: "c".into(), client_secret: None, token_endpoint: "https://x/t".into(), resource: Some("https://x/mcp".into()), expires_at: Some(5), basic_auth: true };
        assert_eq!(Saved::parse(&s.to_json()), Some(s));
        assert_eq!(Saved::parse("sk-plain-token"), None);
    }

    #[test]
    fn the_local_address_catches_the_code_and_checks_the_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=abc%2F1&state=wrong HTTP/1.1\r\n\r\n").unwrap();
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=abc%2F1&state=good HTTP/1.1\r\n\r\n").unwrap();
        });
        assert_eq!(wait_for_code(&listener, "good", Duration::from_secs(5)).unwrap(), "abc/1");
        t.join().unwrap();
    }
}
