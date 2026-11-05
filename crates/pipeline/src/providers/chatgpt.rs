//! Experimental: a ChatGPT subscription as the language model, through the
//! backend that OpenAI's Codex CLI talks to
//! (`chatgpt.com/backend-api/codex/responses`, the Responses API with
//! streaming). OpenAI does not support this for other apps and may change or
//! block it at any time; the user opts in from Settings.
//!
//! Differences from the API: only Codex's own models are offered, every one
//! of them reasons (MeetCue asks for the lowest effort to keep replies
//! quick), and `temperature` and `max_output_tokens` are not accepted.
//! Usage counts against the subscription's limits instead of being billed,
//! so no prices are listed.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};

use meetcue_core::{codes, MeetCueError};

use crate::net;
use crate::provider::{LlmChunk, LlmProvider, LlmRequest, LlmStream, ModelInfo, Role};
use crate::providers::chatgpt_login::{self as login, Access, Credential};
use crate::providers::sse::SseParser;

pub const ID: &str = "chatgpt";
pub const API_BASE: &str = "https://chatgpt.com/backend-api/codex";
/// The Codex version the model list is asked for: models newer than the
/// client are left out of the list.
const CLIENT_VERSION: &str = "0.166.0";
/// Codex's originator, which the backend expects with its OAuth client.
const ORIGINATOR: &str = "codex_cli_rs";
/// Every model on the list reasons; this is the lowest effort it accepts.
const EFFORT: &str = "low";

type Saver = Arc<dyn Fn(&str) + Send + Sync>;

/// Writes a refreshed credential back to the credential store: OpenAI
/// rotates the refresh token, and the old one stops working.
static SAVER: std::sync::Mutex<Option<Saver>> = std::sync::Mutex::new(None);

/// Access tokens live only in memory, one per account. Refreshes run one at
/// a time so two requests never spend the same refresh token.
static SESSIONS: tokio::sync::Mutex<Vec<Session>> = tokio::sync::Mutex::const_new(Vec::new());

#[derive(Debug, Clone)]
struct Session {
    /// The newest credential for this account (its refresh token may be
    /// newer than the one a provider was built with).
    cred: Credential,
    access: Option<Access>,
}

/// Lets the app persist rotated refresh tokens.
pub fn set_saver(f: impl Fn(&str) + Send + Sync + 'static) {
    *SAVER.lock().unwrap() = Some(Arc::new(f));
}

/// Keeps a fresh sign-in's access token, so the first request doesn't refresh.
pub async fn remember(cred: &Credential, access: Access) {
    let mut sessions = SESSIONS.lock().await;
    sessions.retain(|s| s.cred.account != cred.account);
    sessions.push(Session {
        cred: cred.clone(),
        access: Some(access),
    });
}

/// Drops everything kept in memory (on sign-out).
pub async fn forget() {
    SESSIONS.lock().await.clear();
}

/// A valid access token for the account, refreshing it when it has expired
/// (or when `force` is set after the backend refused it).
async fn access_token(
    issuer: &str,
    stored: &Credential,
    proxy: Option<&str>,
    force: bool,
) -> Result<String, MeetCueError> {
    let mut sessions = SESSIONS.lock().await;
    let i = match sessions
        .iter()
        .position(|s| s.cred.account == stored.account)
    {
        Some(i) => i,
        None => {
            sessions.push(Session {
                cred: stored.clone(),
                access: None,
            });
            sessions.len() - 1
        }
    };
    let now = chrono::Utc::now().timestamp();
    if let Some(a) = &sessions[i].access {
        if !force && a.expires_at - 60 > now {
            return Ok(a.token.clone());
        }
    }
    let tokens = login::refresh(issuer, &sessions[i].cred.refresh, proxy)
        .await
        .map_err(|e| {
            MeetCueError::new(
                codes::LLM_AUTH,
                format!("ChatGPT sign-in expired, sign in again: {}", e.detail),
            )
        })?;
    let token = tokens
        .access_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| MeetCueError::new(codes::LLM_AUTH, "no access token returned"))?;
    let session = &mut sessions[i];
    session.access = Some(Access {
        expires_at: login::expires_at(&token, tokens.expires_in, now),
        token: token.clone(),
    });
    if let Some(rt) = tokens.refresh_token.filter(|t| !t.is_empty()) {
        if rt != session.cred.refresh {
            session.cred.refresh = rt;
            let saver = SAVER.lock().unwrap().clone();
            if let Some(save) = saver {
                save(&session.cred.to_json());
            }
        }
    }
    Ok(token)
}

pub struct ChatGpt {
    cred: Credential,
    proxy: Option<String>,
    api: String,
    issuer: String,
}

impl ChatGpt {
    /// `credential` is the JSON kept in the credential store.
    pub fn new(credential: &str, proxy: Option<String>) -> Result<Self, MeetCueError> {
        Ok(Self {
            cred: Credential::parse(credential)?,
            proxy,
            api: API_BASE.into(),
            issuer: login::ISSUER.into(),
        })
    }

    #[cfg(test)]
    fn at(mut self, api: &str, issuer: &str) -> Self {
        self.api = api.into();
        self.issuer = issuer.into();
        self
    }

    /// Sends a request with the account's headers; a refused token is
    /// refreshed once, and busy or failing servers are retried twice.
    async fn send(
        &self,
        build: impl Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, MeetCueError> {
        let client = net::http_client(self.proxy.as_deref())?;
        let mut forced = false;
        let mut attempt = 0;
        loop {
            let token =
                access_token(&self.issuer, &self.cred, self.proxy.as_deref(), forced).await?;
            let resp = build(&client)
                .bearer_auth(&token)
                .header("chatgpt-account-id", &self.cred.account)
                .header("originator", ORIGINATOR)
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            let status = resp.status().as_u16();
            if status == 200 {
                return Ok(resp);
            }
            if status == 401 && !forced {
                forced = true;
                continue;
            }
            let retry_after = net::retry_after(resp.headers());
            let text = resp.text().await.unwrap_or_default();
            if matches!(status, 500 | 502 | 503) && attempt < 2 {
                tokio::time::sleep(net::backoff(attempt, retry_after).min(Duration::from_secs(5)))
                    .await;
                attempt += 1;
                continue;
            }
            return Err(MeetCueError::new(
                net::llm_status_code(status),
                format!("HTTP {status}: {}", error_detail(&text)),
            ));
        }
    }
}

/// The backend's error message, or the start of the body.
fn error_detail(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["detail"].as_str())
                .or_else(|| v["detail"]["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(300).collect())
}

fn text_item(role: &str, kind: &str, text: &str) -> Value {
    json!({
        "type": "message",
        "role": role,
        "content": [{ "type": kind, "text": text }],
    })
}

pub fn request_body(req: &LlmRequest) -> Value {
    let mut input = Vec::new();
    // Codex sends its instructions as the first developer message.
    if !req.system.is_empty() {
        input.push(text_item("developer", "input_text", &req.system));
    }
    for m in &req.messages {
        input.push(match m.role {
            Role::User => text_item("user", "input_text", &m.content),
            Role::Assistant => text_item("assistant", "output_text", &m.content),
        });
    }
    let mut body = json!({
        "model": req.model,
        "stream": true,
        "input": input,
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "reasoning": { "effort": EFFORT },
        "store": false,
        "include": [],
    });
    if req.cache_system && !req.system.is_empty() {
        // The same instructions get the same key, so they're served from cache.
        let digest = login::sha256(req.system.as_bytes());
        let key: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        body["prompt_cache_key"] = json!(format!("meetcue-{key}"));
    }
    body
}

/// What one streamed event adds, and whether the reply is complete.
pub fn handle_event(data: &str) -> Result<(Vec<LlmChunk>, bool), MeetCueError> {
    let v: Value = serde_json::from_str(data)
        .map_err(|e| MeetCueError::new(codes::LLM_BAD_OUTPUT, e.to_string()))?;
    let kind = v["type"].as_str().unwrap_or_default();
    match kind {
        "response.output_text.delta" => {
            let t = v["delta"].as_str().unwrap_or_default();
            Ok((
                (!t.is_empty())
                    .then(|| LlmChunk::Text(t.to_string()))
                    .into_iter()
                    .collect(),
                false,
            ))
        }
        "response.completed" | "response.incomplete" | "response.done" => {
            let u = &v["response"]["usage"];
            let mut out = Vec::new();
            if u.is_object() {
                let input = u["input_tokens"].as_u64().unwrap_or(0);
                let cached = u
                    .pointer("/input_tokens_details/cached_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(input);
                out.push(LlmChunk::Usage {
                    input: input - cached,
                    output: u["output_tokens"].as_u64().unwrap_or(0),
                    cache_read: cached,
                    cache_write: 0,
                });
            }
            Ok((out, true))
        }
        "response.failed" | "error" => {
            let err = if kind == "error" {
                &v
            } else {
                &v["response"]["error"]
            };
            let message = err["message"]
                .as_str()
                .or_else(|| err["error"]["message"].as_str())
                .unwrap_or("the reply failed")
                .to_string();
            let code = err["code"]
                .as_str()
                .or_else(|| err["error"]["code"].as_str())
                .or_else(|| err["type"].as_str())
                .unwrap_or_default();
            let mc = if code.contains("rate_limit") || code.contains("usage_limit") {
                codes::LLM_RATE_LIMIT
            } else {
                codes::LLM_REQUEST
            };
            Err(MeetCueError::new(mc, message))
        }
        _ => Ok((Vec::new(), false)),
    }
}

/// The models Codex lists for the account, in the order it lists them.
pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let mut models: Vec<(i64, ModelInfo)> = v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"].as_str().unwrap_or("list") == "list")
        .filter_map(|m| {
            let id = m["slug"].as_str()?;
            Some((
                m["priority"].as_i64().unwrap_or(i64::MAX),
                ModelInfo {
                    id: id.into(),
                    display_name: m["display_name"].as_str().unwrap_or(id).into(),
                    price: None,
                },
            ))
        })
        .collect();
    models.sort_by_key(|(p, _)| *p);
    models.into_iter().map(|(_, m)| m).collect()
}

#[async_trait]
impl LlmProvider for ChatGpt {
    fn id(&self) -> &'static str {
        ID
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        let url = format!("{}/models?client_version={CLIENT_VERSION}", self.api);
        let resp = self.send(|c| c.get(&url)).await?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| MeetCueError::new(codes::LLM_REQUEST, e.to_string()))?;
        Ok(parse_models(&v))
    }

    async fn stream(&self, req: LlmRequest) -> Result<LlmStream, MeetCueError> {
        let body = request_body(&req);
        let url = format!("{}/responses", self.api);
        let resp = self
            .send(|c| {
                c.post(&url)
                    .header("Accept", "text/event-stream")
                    .json(&body)
            })
            .await?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        tokio::spawn(async move {
            let mut parser = SseParser::default();
            let mut bytes = resp.bytes_stream();
            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.unbounded_send(Err(net::reqwest_error(&e)));
                        return;
                    }
                };
                for ev in parser.push(&chunk) {
                    if ev.data.trim().is_empty() || ev.data.trim() == "[DONE]" {
                        continue;
                    }
                    match handle_event(&ev.data) {
                        Ok((chunks, done)) => {
                            for c in chunks {
                                if tx.unbounded_send(Ok(c)).is_err() {
                                    return;
                                }
                            }
                            if done {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.unbounded_send(Err(e));
                            return;
                        }
                    }
                }
            }
        });
        Ok(rx.boxed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::chatgpt_login::tests::{jwt, token_server};
    use meetcue_core::UsagePurpose;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn body_is_what_codex_sends() {
        let mut r = LlmRequest::new("gpt-6-luna", UsagePurpose::Hints)
            .system("sys", true)
            .user("hi")
            .max_tokens(50)
            .temperature(0.0);
        r.messages.push(crate::provider::LlmMessage {
            role: Role::Assistant,
            content: "earlier".into(),
        });
        let b = request_body(&r);
        assert_eq!(b["model"], "gpt-6-luna");
        assert_eq!(b["store"], false);
        assert_eq!(b["stream"], true);
        assert_eq!(b["reasoning"]["effort"], "low");
        assert_eq!(b["input"][0]["role"], "developer");
        assert_eq!(b["input"][0]["content"][0]["text"], "sys");
        assert_eq!(b["input"][1]["content"][0]["type"], "input_text");
        assert_eq!(b["input"][2]["content"][0]["type"], "output_text");
        // The backend refuses these.
        assert!(b.get("temperature").is_none());
        assert!(b.get("max_output_tokens").is_none());
        let key = b["prompt_cache_key"].as_str().unwrap();
        assert_eq!(key, request_body(&r)["prompt_cache_key"]);
        let other = LlmRequest::new("m", UsagePurpose::Hints).system("other", true);
        assert_ne!(request_body(&other)["prompt_cache_key"], key);
    }

    #[test]
    fn reads_the_stream() {
        let (c, done) =
            handle_event(r#"{"type":"response.output_text.delta","delta":"Hel"}"#).unwrap();
        assert_eq!((c, done), (vec![LlmChunk::Text("Hel".into())], false));
        let (c, done) =
            handle_event(r#"{"type":"response.reasoning_summary_text.delta","delta":"thinking"}"#)
                .unwrap();
        assert!(c.is_empty() && !done, "reasoning is not shown");
        let (c, done) = handle_event(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":60},"output_tokens":20}}}"#,
        )
        .unwrap();
        assert!(done);
        assert_eq!(
            c,
            vec![LlmChunk::Usage {
                input: 40,
                output: 20,
                cache_read: 60,
                cache_write: 0
            }]
        );
        let e = handle_event(r#"{"type":"response.failed","response":{"error":{"code":"usage_limit_reached","message":"You've hit your usage limit"}}}"#).unwrap_err();
        assert_eq!(e.code, codes::LLM_RATE_LIMIT);
        assert!(e.detail.contains("usage limit"));
        let e = handle_event(r#"{"type":"error","message":"bad model"}"#).unwrap_err();
        assert_eq!(e.code, codes::LLM_REQUEST);
    }

    #[test]
    fn lists_only_visible_models_in_order() {
        let v = json!({"models": [
            {"slug": "b", "display_name": "B", "visibility": "list", "priority": 2},
            {"slug": "hidden", "display_name": "H", "visibility": "hide", "priority": 0},
            {"slug": "a", "display_name": "A", "visibility": "list", "priority": 1},
        ]});
        let ids: Vec<_> = parse_models(&v).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(parse_models(&json!({})).is_empty());
    }

    #[test]
    fn needs_a_saved_sign_in() {
        assert!(ChatGpt::new("not json", None).is_err());
        let c = Credential {
            refresh: "rt".into(),
            account: "a".into(),
            email: None,
            plan: None,
        };
        assert!(ChatGpt::new(&c.to_json(), None).is_ok());
    }

    /// A backend that answers each request with the next canned reply and
    /// returns the requests it saw.
    async fn backend(replies: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = vec![0u8; 16384];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).to_string();
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
                seen.push(String::from_utf8_lossy(&got).to_string());
                s.write_all(reply.as_bytes()).await.unwrap();
                let _ = s.shutdown().await;
            }
            seen
        });
        (format!("http://{addr}"), task)
    }

    fn http(status: &str, ctype: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn refreshes_saves_the_new_token_and_streams() {
        let saved = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let s2 = saved.clone();
        set_saver(move |s| s2.lock().unwrap().push(s.to_string()));

        let access = jwt(json!({"exp": 4_000_000_000i64}));
        let (issuer, auth) =
            token_server(json!({"access_token": access, "refresh_token": "rt-new"}).to_string())
                .await;
        let sse = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\n\n\
event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n";
        let (api, seen) = backend(vec![http("200 OK", "text/event-stream", sse)]).await;

        let cred = Credential {
            refresh: "rt-old".into(),
            account: "acc-stream".into(),
            email: None,
            plan: None,
        };
        let llm = ChatGpt::new(&cred.to_json(), None)
            .unwrap()
            .at(&api, &issuer);
        let req = LlmRequest::new("gpt-6-luna", UsagePurpose::Hints).user("q");
        let (text, usage) = crate::provider::complete(&llm, req).await.unwrap();
        assert_eq!(text, "Hi");
        assert!(matches!(
            usage,
            Some(LlmChunk::Usage {
                input: 5,
                output: 1,
                ..
            })
        ));

        let refresh = auth.await.unwrap();
        assert!(
            refresh.contains("\"refresh_token\":\"rt-old\""),
            "{refresh}"
        );
        let req = &seen.await.unwrap()[0];
        assert!(req.starts_with("POST /responses"));
        let lower = req.to_ascii_lowercase();
        assert!(lower.contains(&format!(
            "authorization: bearer {}",
            access.to_ascii_lowercase()
        )));
        assert!(lower.contains("chatgpt-account-id: acc-stream"));
        let saved = saved.lock().unwrap();
        assert!(saved
            .iter()
            .any(|s| s.contains("rt-new") && s.contains("acc-stream")));
    }

    #[tokio::test]
    async fn a_refused_token_is_refreshed_once() {
        let cred = Credential {
            refresh: "rt".into(),
            account: "acc-401".into(),
            email: None,
            plan: None,
        };
        // A token that looks valid, but the backend has revoked it.
        remember(
            &cred,
            Access {
                token: "stale".into(),
                expires_at: 4_000_000_000,
            },
        )
        .await;
        let fresh = jwt(json!({"exp": 4_000_000_000i64}));
        let (issuer, _auth) = token_server(json!({"access_token": fresh}).to_string()).await;
        let models = json!({"models": [{"slug": "gpt-6-luna", "display_name": "GPT-6-Luna", "visibility": "list"}]});
        let (api, seen) = backend(vec![
            http(
                "401 Unauthorized",
                "application/json",
                r#"{"detail":"expired"}"#,
            ),
            http("200 OK", "application/json", &models.to_string()),
        ])
        .await;
        let llm = ChatGpt::new(&cred.to_json(), None)
            .unwrap()
            .at(&api, &issuer);
        let list = llm.list_models().await.unwrap();
        assert_eq!(list[0].id, "gpt-6-luna");
        let seen = seen.await.unwrap();
        assert!(seen[0].contains("Bearer stale"));
        assert!(seen[1].contains(&format!("Bearer {fresh}")));
        assert!(seen[1].contains("client_version="));
    }
}
