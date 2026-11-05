//! "Sign in with ChatGPT": the browser OAuth flow (authorization code with
//! PKCE) that OpenAI's Codex CLI uses, so a ChatGPT subscription can stand in
//! for an API key. OpenAI does not support this for other apps; it is an
//! experimental, opt-in provider.
//!
//! The flow: listen on `127.0.0.1:1455` (Codex's registered callback port,
//! `1457` as its fallback), open the authorize URL in the browser, wait for
//! the redirect to `/auth/callback`, then trade the code for tokens.

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use ts_rs::TS;

use meetcue_core::{codes, MeetCueError};

use crate::net;

pub const ISSUER: &str = "https://auth.openai.com";
/// Codex CLI's public OAuth client (no secret; PKCE protects the code).
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CALLBACK_PORTS: &[u16] = &[1455, 1457];
const SCOPE: &str = "openid profile email offline_access";
const ORIGINATOR: &str = "codex_cli_rs";

/// What MeetCue keeps in the credential store. Only the refresh token is
/// kept: access tokens are too long for Windows Credential Manager and are
/// fetched again when the app starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub refresh: String,
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

impl Credential {
    pub fn parse(s: &str) -> Result<Self, MeetCueError> {
        serde_json::from_str(s).map_err(|_| {
            MeetCueError::new(codes::LLM_AUTH, "the saved ChatGPT sign-in is unreadable")
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn account_info(&self) -> ChatGptAccount {
        ChatGptAccount {
            email: self.email.clone(),
            plan: self.plan.clone(),
        }
    }
}

/// The signed-in account as the interface shows it. Never holds a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChatGptAccount {
    pub email: Option<String>,
    /// `plus`, `pro`, `team`, … as the token reports it.
    pub plan: Option<String>,
}

/// An access token and when it stops working (Unix seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub token: String,
    pub expires_at: i64,
}

/// The claims of a JWT, without checking its signature (the token came
/// straight from OpenAI over TLS; we only read our own account details).
pub fn jwt_claims(jwt: &str) -> Option<Value> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// When an access token expires; an hour from now if it doesn't say.
pub fn expires_at(access_token: &str, expires_in: Option<i64>, now: i64) -> i64 {
    jwt_claims(access_token)
        .and_then(|c| c["exp"].as_i64())
        .or_else(|| expires_in.map(|s| now + s))
        .unwrap_or(now + 3600)
}

/// The tokens OpenAI returns from its token endpoint.
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

/// Account id, email and plan from the ID token (or, failing that, the
/// access token, which carries the same `https://api.openai.com/auth` claim).
pub fn account_from_tokens(id_token: Option<&str>, access_token: &str) -> Option<Credential> {
    let read = |jwt: &str| -> Option<(String, Option<String>, Option<String>)> {
        let c = jwt_claims(jwt)?;
        let auth = &c["https://api.openai.com/auth"];
        let account = auth["chatgpt_account_id"].as_str()?.to_string();
        let email = c["email"]
            .as_str()
            .or_else(|| c["https://api.openai.com/profile"]["email"].as_str())
            .map(str::to_string);
        let plan = auth["chatgpt_plan_type"].as_str().map(str::to_string);
        Some((account, email, plan))
    };
    let (account, email, plan) = id_token.and_then(read).or_else(|| read(access_token))?;
    Some(Credential {
        refresh: String::new(),
        account,
        email,
        plan,
    })
}

/// Percent-encodes a query or form value.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn dec(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The value of `name` in a query string.
fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (dec(k) == name).then(|| dec(v))
    })
}

fn random_token(bytes: usize) -> String {
    let data: Vec<u8> = (0..bytes).map(|_| rand::random::<u8>()).collect();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// The PKCE S256 challenge for a verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()))
}

pub fn authorize_url(issuer: &str, redirect_uri: &str, challenge: &str, state: &str) -> String {
    let q = form(&[
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", ORIGINATOR),
    ]);
    format!("{}/oauth/authorize?{q}", issuer.trim_end_matches('/'))
}

/// A sign-in waiting for the browser to come back.
pub struct Login {
    /// Open this in the user's browser.
    pub url: String,
    listener: TcpListener,
    verifier: String,
    state: String,
    redirect_uri: String,
    issuer: String,
}

/// Starts a sign-in: binds the callback port and builds the authorize URL.
pub async fn begin() -> Result<Login, MeetCueError> {
    begin_at(ISSUER, CALLBACK_PORTS).await
}

pub async fn begin_at(issuer: &str, ports: &[u16]) -> Result<Login, MeetCueError> {
    let mut last = None;
    for &port in ports {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => {
                let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
                let redirect_uri = format!("http://127.0.0.1:{port}/auth/callback");
                let verifier = random_token(64);
                let state = random_token(32);
                let url = authorize_url(issuer, &redirect_uri, &pkce_challenge(&verifier), &state);
                return Ok(Login {
                    url,
                    listener,
                    verifier,
                    state,
                    redirect_uri,
                    issuer: issuer.trim_end_matches('/').to_string(),
                });
            }
            Err(e) => last = Some(e),
        }
    }
    Err(MeetCueError::new(
        codes::LLM_AUTH,
        format!(
            "the sign-in callback port is in use (is another Codex sign-in open?): {}",
            last.map(|e| e.to_string()).unwrap_or_default()
        ),
    ))
}

const PAGE_OK: &str = "<!doctype html><meta charset=utf-8><title>MeetCue</title>\
<body style=\"font:16px system-ui;margin:3em;text-align:center\">\
<h2>Signed in to MeetCue</h2><p>You can close this tab.</p><p>已登录 MeetCue，可以关闭此页面。</p>";

fn page_err(msg: &str) -> String {
    let safe: String = msg
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | '&' | '"'))
        .take(300)
        .collect();
    format!(
        "<!doctype html><meta charset=utf-8><title>MeetCue</title>\
<body style=\"font:16px system-ui;margin:3em;text-align:center\">\
<h2>Sign-in failed</h2><p>{safe}</p><p>登录失败，请回到 MeetCue 重试。</p>"
    )
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Reads the request line's path, e.g. `/auth/callback?code=…`.
async fn read_path(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .ok()?
            .ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next()?;
    let mut parts = line.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(str::to_string)
}

impl Login {
    /// Waits for the browser's redirect and trades the code for tokens.
    pub async fn finish(self, proxy: Option<&str>) -> Result<(Credential, Access), MeetCueError> {
        loop {
            let (mut stream, _) = self
                .listener
                .accept()
                .await
                .map_err(|e| MeetCueError::new(codes::LLM_AUTH, e.to_string()))?;
            let Some(path) = read_path(&mut stream).await else {
                continue;
            };
            let (route, query) = path.split_once('?').unwrap_or((&path, ""));
            if route != "/auth/callback" {
                respond(&mut stream, "404 Not Found", "").await;
                continue;
            }
            if let Some(err) = query_param(query, "error") {
                let detail = query_param(query, "error_description").unwrap_or(err);
                respond(&mut stream, "200 OK", &page_err(&detail)).await;
                return Err(MeetCueError::new(codes::LLM_AUTH, detail));
            }
            if query_param(query, "state").as_deref() != Some(self.state.as_str()) {
                // Not our sign-in (an old tab, or a forged request): ignore it.
                respond(&mut stream, "400 Bad Request", &page_err("state mismatch")).await;
                continue;
            }
            let Some(code) = query_param(query, "code") else {
                respond(&mut stream, "400 Bad Request", &page_err("no code")).await;
                continue;
            };
            let result = self.exchange(&code, proxy).await;
            match &result {
                Ok(_) => respond(&mut stream, "200 OK", PAGE_OK).await,
                Err(e) => respond(&mut stream, "200 OK", &page_err(&e.detail)).await,
            }
            return result;
        }
    }

    async fn exchange(
        &self,
        code: &str,
        proxy: Option<&str>,
    ) -> Result<(Credential, Access), MeetCueError> {
        let client = net::http_client(proxy)?;
        let resp = client
            .post(format!("{}/oauth/token", self.issuer))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form(&[
                ("grant_type", "authorization_code"),
                ("client_id", CLIENT_ID),
                ("code", code),
                ("redirect_uri", &self.redirect_uri),
                ("code_verifier", &self.verifier),
            ]))
            .send()
            .await
            .map_err(|e| net::reqwest_error(&e))?;
        let tokens = read_tokens(resp).await?;
        let access = tokens
            .access_token
            .clone()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| MeetCueError::new(codes::LLM_AUTH, "no access token returned"))?;
        let refresh = tokens
            .refresh_token
            .clone()
            .filter(|t| !t.is_empty())
            .ok_or_else(|| MeetCueError::new(codes::LLM_AUTH, "no refresh token returned"))?;
        let mut cred =
            account_from_tokens(tokens.id_token.as_deref(), &access).ok_or_else(|| {
                MeetCueError::new(
                    codes::LLM_AUTH,
                    "this OpenAI account has no ChatGPT workspace to use",
                )
            })?;
        cred.refresh = refresh;
        let now = chrono::Utc::now().timestamp();
        let expires_at = expires_at(&access, tokens.expires_in, now);
        Ok((
            cred,
            Access {
                token: access,
                expires_at,
            },
        ))
    }
}

pub(crate) async fn read_tokens(resp: reqwest::Response) -> Result<TokenResponse, MeetCueError> {
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    if status != 200 {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error_description"]
                    .as_str()
                    .or_else(|| v["error"]["message"].as_str())
                    .or_else(|| v["error"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(200).collect());
        return Err(MeetCueError::new(
            codes::LLM_AUTH,
            format!("sign-in refused (HTTP {status}): {detail}"),
        ));
    }
    serde_json::from_str(&text).map_err(|e| MeetCueError::new(codes::LLM_AUTH, e.to_string()))
}

/// Trades a refresh token for a new access token (and usually a new refresh token).
pub async fn refresh(
    issuer: &str,
    refresh_token: &str,
    proxy: Option<&str>,
) -> Result<TokenResponse, MeetCueError> {
    let client = net::http_client(proxy)?;
    let resp = client
        .post(format!("{}/oauth/token", issuer.trim_end_matches('/')))
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|e| net::reqwest_error(&e))?;
    read_tokens(resp).await
}

/// Revokes the refresh token on sign-out. Best effort.
pub async fn revoke(refresh_token: &str, proxy: Option<&str>) -> Result<(), MeetCueError> {
    let client = net::http_client(proxy)?;
    client
        .post(format!("{ISSUER}/oauth/revoke"))
        .timeout(Duration::from_secs(10))
        .json(&serde_json::json!({
            "token": refresh_token,
            "token_type_hint": "refresh_token",
            "client_id": CLIENT_ID,
        }))
        .send()
        .await
        .map_err(|e| net::reqwest_error(&e))?;
    Ok(())
}

/// SHA-256 (FIPS 180-4), for the PKCE challenge and prompt cache keys.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
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
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// An unsigned JWT with the given claims, as the token endpoint would return.
    pub(crate) fn jwt(claims: Value) -> String {
        let e = |v: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v);
        format!(
            "{}.{}.sig",
            e(br#"{"alg":"none"}"#),
            e(claims.to_string().as_bytes())
        )
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            hex(&sha256(long)),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn pkce_follows_rfc_7636() {
        // The example in RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_has_what_openai_expects() {
        let u = authorize_url(ISSUER, "http://127.0.0.1:1455/auth/callback", "ch", "st");
        assert!(u.starts_with("https://auth.openai.com/oauth/authorize?response_type=code&"));
        let q = u.split_once('?').unwrap().1;
        assert_eq!(query_param(q, "client_id").as_deref(), Some(CLIENT_ID));
        assert_eq!(
            query_param(q, "redirect_uri").as_deref(),
            Some("http://127.0.0.1:1455/auth/callback")
        );
        assert_eq!(query_param(q, "scope").as_deref(), Some(SCOPE));
        assert_eq!(
            query_param(q, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert_eq!(query_param(q, "state").as_deref(), Some("st"));
    }

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(query_param("a=1&b=x%20y+z", "b").as_deref(), Some("x y z"));
        assert_eq!(query_param("a=1&b=50%", "b").as_deref(), Some("50%"));
        assert_eq!(query_param("a=1", "b"), None);
    }

    #[test]
    fn reads_the_account_from_the_id_token() {
        let id = jwt(serde_json::json!({
            "email": "me@example.com",
            "https://api.openai.com/auth": {"chatgpt_account_id": "acc-1", "chatgpt_plan_type": "plus"}
        }));
        let access = jwt(serde_json::json!({"exp": 2_000_000_000}));
        let c = account_from_tokens(Some(&id), &access).unwrap();
        assert_eq!(c.account, "acc-1");
        assert_eq!(c.email.as_deref(), Some("me@example.com"));
        assert_eq!(c.plan.as_deref(), Some("plus"));
        assert_eq!(expires_at(&access, None, 0), 2_000_000_000);
        assert_eq!(expires_at("opaque", Some(60), 100), 160);
        assert!(account_from_tokens(None, "opaque").is_none());
    }

    #[test]
    fn credential_round_trips_without_access_tokens() {
        let c = Credential {
            refresh: "rt".into(),
            account: "acc".into(),
            email: None,
            plan: Some("pro".into()),
        };
        let s = c.to_json();
        assert!(!s.contains("access"));
        assert_eq!(Credential::parse(&s).unwrap(), c);
        assert!(Credential::parse("sk-not-json").is_err());
    }

    async fn get(port: u16, path: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    /// A one-shot token endpoint that answers with `body`.
    pub(crate) async fn token_server(body: String) -> (String, tokio::task::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let mut got = Vec::new();
            loop {
                let n = s.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got);
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if rest.len() >= len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&got).into_owned()
        });
        (format!("http://{addr}"), task)
    }

    #[tokio::test]
    async fn sign_in_end_to_end() {
        let id = jwt(serde_json::json!({
            "email": "me@example.com",
            "https://api.openai.com/auth": {"chatgpt_account_id": "acc-9", "chatgpt_plan_type": "pro"}
        }));
        let access = jwt(serde_json::json!({"exp": 2_000_000_000}));
        let body =
            serde_json::json!({"id_token": id, "access_token": access, "refresh_token": "rt-1"});
        let (issuer, server) = token_server(body.to_string()).await;
        let login = begin_at(&issuer, &[0]).await.unwrap();
        let port = login.listener.local_addr().unwrap().port();
        let state = query_param(login.url.split_once('?').unwrap().1, "state").unwrap();
        let finish = tokio::spawn(async move { login.finish(None).await });

        // Stray requests and a wrong state are ignored, not fatal.
        assert!(get(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(get(port, "/auth/callback?code=x&state=wrong")
            .await
            .starts_with("HTTP/1.1 400"));
        let page = get(port, &format!("/auth/callback?code=the-code&state={state}")).await;
        assert!(page.contains("Signed in"), "{page}");

        let (cred, acc) = finish.await.unwrap().unwrap();
        assert_eq!(cred.account, "acc-9");
        assert_eq!(cred.refresh, "rt-1");
        assert_eq!(acc.expires_at, 2_000_000_000);
        let sent = server.await.unwrap();
        assert!(sent.starts_with("POST /oauth/token"));
        assert!(sent.contains("grant_type=authorization_code"));
        assert!(sent.contains("code=the-code"));
        assert!(sent.contains("code_verifier="));
    }

    #[tokio::test]
    async fn a_refused_sign_in_reports_why() {
        let login = begin_at("http://127.0.0.1:9", &[0]).await.unwrap();
        let port = login.listener.local_addr().unwrap().port();
        let finish = tokio::spawn(async move { login.finish(None).await });
        let page = get(
            port,
            "/auth/callback?error=access_denied&error_description=No+%3Cthanks%3E",
        )
        .await;
        assert!(page.contains("No thanks"), "{page}");
        let err = finish.await.unwrap().unwrap_err();
        assert_eq!(err.code, codes::LLM_AUTH);
    }
}
