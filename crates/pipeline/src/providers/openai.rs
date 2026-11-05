//! Any OpenAI-compatible chat completions API (`<base>/chat/completions`
//! with `stream: true`). Prices are unknown unless the user enters them.
//!
//! Servers and gateways (One API, New API, LiteLLM, OpenRouter, vLLM,
//! Ollama, Azure and others) differ in small ways, so the client accepts a
//! pasted endpoint as the base URL, drops or renames a request field a server
//! rejects and remembers that, and reads replies that arrive unstreamed.

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};

use meetcue_core::{codes, MeetCueError, PriceEntry, PriceUnit};

use crate::net;
use crate::provider::{LlmChunk, LlmProvider, LlmRequest, LlmStream, ModelInfo, Role};
use crate::providers::sse::SseParser;

/// Which optional request fields this server accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compat {
    /// `stream_options.include_usage`, for token usage while streaming.
    pub stream_options: bool,
    /// Send the limit as `max_completion_tokens` instead of `max_tokens`
    /// (OpenAI reasoning models reject the older name).
    pub max_completion_tokens: bool,
    pub temperature: bool,
}

impl Default for Compat {
    fn default() -> Self {
        Self {
            stream_options: true,
            max_completion_tokens: false,
            temperature: true,
        }
    }
}

impl Compat {
    /// The next variant to try after a 400 with this body, if any field is to blame.
    pub fn after_rejection(self, body: &str) -> Option<Compat> {
        let b = body.to_ascii_lowercase();
        let mut next = self;
        if !self.max_completion_tokens && b.contains("max_completion_tokens") {
            next.max_completion_tokens = true;
        } else if self.stream_options
            && (b.contains("stream_options") || b.contains("include_usage"))
        {
            next.stream_options = false;
        } else if self.temperature && b.contains("temperature") {
            next.temperature = false;
        } else {
            return None;
        }
        Some(next)
    }
}

pub struct OpenAiCompatible {
    base: String,
    key: Option<String>,
    proxy: Option<String>,
    azure: bool,
    compat: Mutex<Compat>,
}

impl OpenAiCompatible {
    pub fn new(base_url: &str, key: Option<String>, proxy: Option<String>) -> Self {
        let base = api_base(base_url);
        let azure = host_of(&base).is_some_and(|h| {
            h.ends_with(".openai.azure.com")
                || h.ends_with(".cognitiveservices.azure.com")
                || h.ends_with(".services.ai.azure.com")
        });
        Self {
            base,
            key: key.filter(|k| !k.trim().is_empty()),
            proxy,
            azure,
            compat: Mutex::new(Compat::default()),
        }
    }

    fn auth(&self, b: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            // Azure reads its keys from `api-key`.
            Some(k) if self.azure => b.header("api-key", k),
            Some(k) => b.bearer_auth(k),
            None => b,
        }
    }
}

fn host_of(url: &str) -> Option<&str> {
    url.split("://")
        .nth(1)?
        .split(['/', ':', '?'])
        .next()
        .filter(|h| !h.is_empty())
}

/// The API root from what the user entered: a pasted
/// `.../chat/completions` or `.../models` endpoint is cut back to its root,
/// and a bare `https://host` gets the usual `/v1`. Other paths (such as
/// `/api/v3` or `/v1beta/openai`) are kept as they are.
pub fn api_base(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/');
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(root) = url.strip_suffix(suffix) {
            url = root.trim_end_matches('/');
            break;
        }
    }
    let has_path = url
        .split("://")
        .nth(1)
        .is_some_and(|rest| rest.contains('/'));
    if url.contains("://") && !has_path {
        format!("{url}/v1")
    } else {
        url.to_string()
    }
}

pub fn request_body(req: &LlmRequest) -> Value {
    request_body_for(req, Compat::default())
}

pub fn request_body_for(req: &LlmRequest, compat: Compat) -> Value {
    let mut messages = Vec::new();
    if !req.system.is_empty() {
        messages.push(json!({ "role": "system", "content": req.system }));
    }
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        messages.push(json!({ "role": role, "content": m.content }));
    }
    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
    });
    let limit = if compat.max_completion_tokens {
        "max_completion_tokens"
    } else {
        "max_tokens"
    };
    body[limit] = json!(req.max_tokens);
    if compat.stream_options {
        body["stream_options"] = json!({ "include_usage": true });
    }
    if let (Some(t), true) = (req.temperature, compat.temperature) {
        body["temperature"] = json!(t);
    }
    body
}

fn error_message(err: &Value) -> String {
    err.as_str()
        .or_else(|| err.get("message").and_then(Value::as_str))
        .unwrap_or("error")
        .to_string()
}

/// Text and usage from a reply that came back as one JSON object.
pub fn handle_whole(body: &str) -> Result<Vec<LlmChunk>, MeetCueError> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| MeetCueError::new(codes::LLM_BAD_OUTPUT, e.to_string()))?;
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Err(MeetCueError::new(codes::LLM_REQUEST, error_message(err)));
    }
    let mut out = Vec::new();
    if let Some(t) = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
    {
        out.push(LlmChunk::Text(t.to_string()));
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        out.push(usage_chunk(u));
    }
    Ok(out)
}

/// Parses one `data:` payload; `None` marks the end of the stream.
pub fn handle_data(data: &str) -> Option<Result<Vec<LlmChunk>, MeetCueError>> {
    if data.trim() == "[DONE]" {
        return None;
    }
    let v: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => return Some(Err(MeetCueError::new(codes::LLM_BAD_OUTPUT, e.to_string()))),
    };
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Some(Err(MeetCueError::new(
            codes::LLM_REQUEST,
            error_message(err),
        )));
    }
    let mut out = Vec::new();
    if let Some(t) = v
        .pointer("/choices/0/delta/content")
        .and_then(Value::as_str)
    {
        if !t.is_empty() {
            out.push(LlmChunk::Text(t.to_string()));
        }
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        out.push(usage_chunk(u));
    }
    Some(Ok(out))
}

fn usage_chunk(u: &Value) -> LlmChunk {
    let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
    // Servers report cache hits in different places: OpenAI and most
    // others in `prompt_tokens_details`, DeepSeek as
    // `prompt_cache_hit_tokens`, some (such as Kimi) at the top level.
    let cached = [
        "/prompt_tokens_details/cached_tokens",
        "/prompt_cache_hit_tokens",
        "/cached_tokens",
    ]
    .iter()
    .filter_map(|p| u.pointer(p).and_then(Value::as_u64))
    .max()
    .unwrap_or(0)
    .min(prompt);
    LlmChunk::Usage {
        input: prompt.saturating_sub(cached),
        output: u["completion_tokens"].as_u64().unwrap_or(0),
        cache_read: cached,
        cache_write: 0,
    }
}

/// The models a `/models` reply lists: OpenAI's `{"data": [...]}`, a bare
/// array, or `{"models": [...]}`, with entries as objects or plain names.
pub fn model_entries(v: &Value) -> Vec<&Value> {
    let list = v["data"]
        .as_array()
        .or_else(|| v.as_array())
        .or_else(|| v["models"].as_array());
    list.into_iter().flatten().collect()
}

/// The price some servers (OpenRouter and others) put in their model list:
/// `"pricing": {"prompt": "0.000002", "completion": "0.00001"}` in USD per token.
pub fn listed_price(
    m: &Value,
    id: &str,
    name: &str,
    source: &str,
    today: &str,
) -> Option<PriceEntry> {
    let per_token = |k: &str| {
        let v = &m["pricing"][k];
        let n = v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())?;
        (n.is_finite() && n >= 0.0).then_some(n * 1e6)
    };
    Some(PriceEntry {
        provider: "openai-compatible".into(),
        model: id.into(),
        label: name.into(),
        unit: PriceUnit::PerMillionTokens,
        input: Some(per_token("prompt")?),
        output: Some(per_token("completion")?),
        cache_read: per_token("input_cache_read"),
        cache_write: per_token("input_cache_write"),
        audio: None,
        note: None,
        source: source.into(),
        verified: today.into(),
    })
}

#[async_trait]
impl LlmProvider for OpenAiCompatible {
    fn id(&self) -> &'static str {
        "openai-compatible"
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        let client = net::http_client(self.proxy.as_deref())?;
        let resp = self
            .auth(client.get(format!("{}/models", self.base)))
            .send()
            .await
            .map_err(|e| net::reqwest_error(&e))?;
        let status = resp.status().as_u16();
        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            return Err(MeetCueError::new(
                net::llm_status_code(status),
                format!(
                    "HTTP {status}: {}",
                    text.chars().take(300).collect::<String>()
                ),
            ));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| MeetCueError::new(codes::LLM_REQUEST, e.to_string()))?;
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let source = format!("{}/models", self.base);
        Ok(model_entries(&v)
            .into_iter()
            .filter_map(|m| {
                let id = m
                    .as_str()
                    .or_else(|| m["id"].as_str())
                    .or_else(|| m["model"].as_str())
                    .or_else(|| m["name"].as_str())?;
                let name = m["name"].as_str().unwrap_or(id);
                Some(ModelInfo {
                    id: id.into(),
                    display_name: name.into(),
                    price: listed_price(m, id, name, &source, &today),
                })
            })
            .collect())
    }

    async fn stream(&self, req: LlmRequest) -> Result<LlmStream, MeetCueError> {
        if self.base.is_empty() {
            return Err(MeetCueError::new(
                codes::LLM_REQUEST,
                "no base URL configured",
            ));
        }
        let client = net::http_client(self.proxy.as_deref())?;
        let mut compat = *self.compat.lock().unwrap();
        let mut attempt = 0;
        let mut adjusted = 0;
        let resp = loop {
            let resp = self
                .auth(client.post(format!("{}/chat/completions", self.base)))
                .json(&request_body_for(&req, compat))
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            let status = resp.status().as_u16();
            if status == 200 {
                // Remember what this server accepted, so later requests
                // don't pay for the rejected attempt again.
                *self.compat.lock().unwrap() = compat;
                break resp;
            }
            let retry_after = net::retry_after(resp.headers());
            let text = resp.text().await.unwrap_or_default();
            if matches!(status, 400 | 422) && adjusted < 3 {
                if let Some(next) = compat.after_rejection(&text) {
                    tracing::warn!(model = %req.model, ?next, "server rejected a request field; retrying");
                    compat = next;
                    adjusted += 1;
                    continue;
                }
            }
            if matches!(status, 429 | 500 | 502 | 503) && attempt < 2 {
                tokio::time::sleep(net::backoff(attempt, retry_after).min(Duration::from_secs(5)))
                    .await;
                attempt += 1;
                continue;
            }
            return Err(MeetCueError::new(
                net::llm_status_code(status),
                format!(
                    "HTTP {status}: {}",
                    text.chars().take(300).collect::<String>()
                ),
            ));
        };
        // Some servers ignore `stream` and send the whole reply at once.
        let whole = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.contains("application/json"));
        if whole {
            let text = resp.text().await.map_err(|e| net::reqwest_error(&e))?;
            let chunks = handle_whole(&text)?;
            return Ok(futures::stream::iter(chunks.into_iter().map(Ok)).boxed());
        }
        let (tx, rx) = futures::channel::mpsc::unbounded();
        tokio::spawn(async move {
            let mut parser = SseParser::default();
            let mut bytes = resp.bytes_stream();
            let mut ended = false;
            while !ended {
                // A last event without its blank line still counts.
                let chunk = match bytes.next().await {
                    Some(c) => c.map(|b| b.to_vec()),
                    None => {
                        ended = true;
                        Ok(b"\n\n".to_vec())
                    }
                };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.unbounded_send(Err(net::reqwest_error(&e)));
                        return;
                    }
                };
                for ev in parser.push(&chunk) {
                    match handle_data(&ev.data) {
                        None => return,
                        Some(Ok(chunks)) => {
                            for c in chunks {
                                if tx.unbounded_send(Ok(c)).is_err() {
                                    return;
                                }
                            }
                        }
                        Some(Err(e)) => {
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
    use meetcue_core::UsagePurpose;

    #[test]
    fn body_and_stream() {
        let r = LlmRequest::new("local-model", UsagePurpose::Detect)
            .system("sys", true)
            .user("hi")
            .max_tokens(50)
            .temperature(0.0);
        let b = request_body(&r);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert_eq!(b["temperature"], 0.0);

        let c = handle_data(r#"{"choices":[{"index":0,"delta":{"content":"Hel"}}]}"#)
            .unwrap()
            .unwrap();
        assert_eq!(c, vec![LlmChunk::Text("Hel".into())]);
        let u = handle_data(r#"{"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":60}}}"#).unwrap().unwrap();
        assert_eq!(
            u,
            vec![LlmChunk::Usage {
                input: 40,
                output: 20,
                cache_read: 60,
                cache_write: 0
            }]
        );
        let cached =
            |usage: &str| match handle_data(&format!(r#"{{"choices":[],"usage":{usage}}}"#))
                .unwrap()
                .unwrap()[0]
            {
                LlmChunk::Usage {
                    input, cache_read, ..
                } => (input, cache_read),
                _ => unreachable!(),
            };
        // DeepSeek and top-level fields.
        assert_eq!(
            cached(
                r#"{"prompt_tokens":100,"completion_tokens":5,"prompt_cache_hit_tokens":70,"prompt_cache_miss_tokens":30}"#
            ),
            (30, 70)
        );
        assert_eq!(
            cached(r#"{"prompt_tokens":100,"completion_tokens":5,"cached_tokens":64}"#),
            (36, 64)
        );
        assert!(handle_data("[DONE]").is_none());
        assert!(handle_data(r#"{"error":{"message":"bad"}}"#)
            .unwrap()
            .is_err());
    }

    #[test]
    fn reads_prices_from_the_model_list() {
        let m = serde_json::json!({
            "id": "moonshotai/kimi-k3.1-pro",
            "name": "MoonshotAI: Kimi K3.1 Pro",
            "pricing": {"prompt": "0.000002", "completion": "0.00001", "input_cache_read": "0.0000002"}
        });
        let p = listed_price(
            &m,
            "moonshotai/kimi-k3.1-pro",
            "Kimi",
            "https://h/v1/models",
            "2026-10-24",
        )
        .unwrap();
        assert!((p.input.unwrap() - 2.0).abs() < 1e-9);
        assert!((p.output.unwrap() - 10.0).abs() < 1e-9);
        assert!((p.cache_read.unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(p.cache_write, None);
        let none = serde_json::json!({"id": "x"});
        assert!(listed_price(&none, "x", "x", "s", "d").is_none());
        let bad = serde_json::json!({"pricing": {"prompt": "-1", "completion": "1"}});
        assert!(listed_price(&bad, "x", "x", "s", "d").is_none());
    }

    #[test]
    fn cleans_up_base_urls() {
        assert_eq!(
            api_base("https://gw.example.com"),
            "https://gw.example.com/v1"
        );
        assert_eq!(
            api_base("https://gw.example.com/"),
            "https://gw.example.com/v1"
        );
        assert_eq!(
            api_base("https://gw.example.com/v1/"),
            "https://gw.example.com/v1"
        );
        assert_eq!(
            api_base("https://gw.example.com/v1/chat/completions"),
            "https://gw.example.com/v1"
        );
        assert_eq!(
            api_base("http://localhost:11434/v1/models"),
            "http://localhost:11434/v1"
        );
        assert_eq!(
            api_base("https://ark.cn-beijing.volces.com/api/v3"),
            "https://ark.cn-beijing.volces.com/api/v3"
        );
        assert_eq!(
            api_base("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1"
        );
        let azure = OpenAiCompatible::new("https://me.openai.azure.com/openai/v1", None, None);
        assert!(azure.azure);
        assert!(!OpenAiCompatible::new("https://gw.example.com", None, None).azure);
    }

    #[test]
    fn drops_or_renames_rejected_fields() {
        let c = Compat::default();
        let openai = "Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.";
        let c1 = c.after_rejection(openai).unwrap();
        assert!(c1.max_completion_tokens);
        assert_eq!(c1.after_rejection(openai), None);
        let c2 = c1
            .after_rejection("Unrecognized request argument supplied: stream_options")
            .unwrap();
        assert!(!c2.stream_options);
        let c3 = c2
            .after_rejection("Unsupported value: 'temperature' does not support 0 with this model.")
            .unwrap();
        assert!(!c3.temperature);
        assert_eq!(c.after_rejection("model not found"), None);

        let r = LlmRequest::new("m", UsagePurpose::Detect)
            .user("hi")
            .max_tokens(50)
            .temperature(0.0);
        let b = request_body_for(&r, c3);
        assert_eq!(b["max_completion_tokens"], 50);
        assert!(b.get("max_tokens").is_none());
        assert!(b.get("stream_options").is_none());
        assert!(b.get("temperature").is_none());
    }

    #[test]
    fn reads_other_reply_shapes() {
        let whole = handle_whole(
            r#"{"choices":[{"message":{"role":"assistant","content":"Hi"}}],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
        )
        .unwrap();
        assert_eq!(whole[0], LlmChunk::Text("Hi".into()));
        assert_eq!(whole.len(), 2);
        assert!(handle_whole(r#"{"error":"quota exhausted"}"#).is_err());
        let e = handle_data(r#"{"error":"no channel available"}"#)
            .unwrap()
            .unwrap_err();
        assert!(e.detail.contains("no channel available"));
        assert!(handle_data(r#"{"error":null,"choices":[]}"#)
            .unwrap()
            .is_ok());

        let names = |v: Value| -> Vec<String> {
            model_entries(&v)
                .iter()
                .filter_map(|m| m.as_str().or_else(|| m["id"].as_str()).map(String::from))
                .collect()
        };
        assert_eq!(names(json!({"data": [{"id": "a"}]})), ["a"]);
        assert_eq!(names(json!([{"id": "b"}])), ["b"]);
        assert_eq!(names(json!({"models": ["c"]})), ["c"]);
    }

    /// A one-shot HTTP server: answers each request with the next reply and
    /// hands back the bodies it received.
    async fn serve(replies: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = sock.read(&mut tmp).await.unwrap();
                    buf.extend_from_slice(&tmp[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            seen.push(text[end + 4..].to_string());
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                sock.write_all(reply.as_bytes()).await.unwrap();
                sock.shutdown().await.unwrap();
            }
            seen
        });
        (format!("http://{addr}/v1/chat/completions"), task)
    }

    fn http(status: &str, ctype: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn adapts_to_the_server_and_remembers() {
        let rejected = http(
            "400 Bad Request",
            "application/json",
            r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead."}}"#,
        );
        // The last event has no blank line after it.
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}";
        let streamed = http("200 OK", "text/event-stream", sse);
        let whole = http(
            "200 OK",
            "application/json",
            r#"{"choices":[{"message":{"content":"Again"}}]}"#,
        );
        let (url, server) = serve(vec![rejected, streamed, whole]).await;
        let p = OpenAiCompatible::new(&url, Some("k".into()), None);
        let req = LlmRequest::new("o9", UsagePurpose::Detect).user("hi");

        let first: Vec<_> = p.stream(req.clone()).await.unwrap().collect().await;
        let first: Vec<_> = first.into_iter().map(Result::unwrap).collect();
        assert_eq!(first[0], LlmChunk::Text("Hi".into()));
        assert!(matches!(first[1], LlmChunk::Usage { input: 5, .. }));

        let second: Vec<_> = p.stream(req).await.unwrap().collect().await;
        assert_eq!(second[0].as_ref().unwrap(), &LlmChunk::Text("Again".into()));

        let bodies = server.await.unwrap();
        assert!(bodies[0].contains("\"max_tokens\""));
        assert!(bodies[1].contains("\"max_completion_tokens\""));
        // Learned once, sent straight away the next time.
        assert!(bodies[2].contains("\"max_completion_tokens\""));
    }
}
