//! Claude through the Messages API — directly (`api.anthropic.com`) or on
//! Amazon Bedrock's Messages endpoint (`bedrock-mantle.<region>.api.aws`,
//! bearer token in `x-api-key`, `anthropic.`-prefixed model ids).
//!
//! Requests stream over SSE, put the stable system prompt first with a cache
//! breakpoint, and keep thinking as low as each model allows: hints must
//! appear about a second after the question.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};

use meetcue_core::{codes, MeetCueError};

use crate::net;
use crate::provider::{LlmChunk, LlmProvider, LlmRequest, LlmStream, ModelInfo};
use crate::providers::sse::SseParser;

const API_HOST: &str = "api.anthropic.com";
/// Raw HTTP clients pin the API version they were written against.
const API_VERSION: &str = "2026-10-21";
/// Bedrock's examples use the original version; we send no fields it treats differently.
const BEDROCK_API_VERSION: &str = "2023-06-01";
const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Claude,
    Bedrock,
}

pub struct Anthropic {
    flavor: Flavor,
    key: String,
    host: String,
    proxy: Option<String>,
}

impl Anthropic {
    pub fn claude(key: String, host: Option<String>, proxy: Option<String>) -> Self {
        Self {
            flavor: Flavor::Claude,
            key,
            host: host.unwrap_or_else(|| API_HOST.into()),
            proxy,
        }
    }

    /// `host` is `bedrock-mantle.<region>.api.aws`.
    pub fn bedrock(key: String, host: String, proxy: Option<String>) -> Self {
        Self {
            flavor: Flavor::Bedrock,
            key,
            host,
            proxy,
        }
    }

    fn base(&self) -> String {
        match self.flavor {
            Flavor::Claude => format!("https://{}/v1", self.host),
            Flavor::Bedrock => format!("https://{}/anthropic/v1", self.host),
        }
    }

    fn version(&self) -> &'static str {
        match self.flavor {
            Flavor::Claude => API_VERSION,
            Flavor::Bedrock => BEDROCK_API_VERSION,
        }
    }
}

/// How each model family turns thinking down for latency. Returns the
/// `thinking` value (if any), the effort, and whether thinking stays on.
pub fn latency_settings(model: &str) -> (Option<Value>, Option<&'static str>, bool) {
    let m = model.trim_start_matches("anthropic.");
    let has = |s: &str| m.contains(s);
    if has("haiku-4-5") || has("haiku-3") {
        // No thinking unless asked for; effort is not accepted.
        (None, None, false)
    } else if has("sonnet-5-5") {
        // `disabled` is rejected; `between_tools` turns thinking off at effort high or below.
        (Some(json!({"type": "between_tools"})), Some("low"), false)
    } else if has("opus-5-5") || has("fable") || has("mythos") {
        // Thinking cannot be turned off; keep it shallow.
        (None, Some("low"), true)
    } else if has("haiku-5-5") || has("opus-5") || has("sonnet-5") {
        (Some(json!({"type": "disabled"})), Some("low"), false)
    } else if has("opus-4-8") || has("opus-4-7") || has("opus-4-6") || has("sonnet-4-6") {
        // Thinking is off when the parameter is omitted.
        (None, Some("low"), false)
    } else {
        // Unknown (newer) model: send nothing model-specific.
        (None, None, true)
    }
}

/// Builds the request body. `minimal` drops model-specific fields (used to
/// retry when a model rejects them).
pub fn request_body(req: &LlmRequest, flavor: Flavor, minimal: bool) -> Value {
    let (thinking, effort, thinking_on) = if minimal {
        (None, None, true)
    } else {
        latency_settings(&req.model)
    };
    // Leave room for any thinking that cannot be turned off.
    let max_tokens = if thinking_on {
        req.max_tokens.max(4_000)
    } else {
        req.max_tokens
    };
    let mut system = json!({ "type": "text", "text": req.system });
    if req.cache_system {
        system["cache_control"] = json!({ "type": "ephemeral" });
    }
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();
    let mut body = json!({
        "model": req.model,
        "max_tokens": max_tokens,
        "stream": true,
        "messages": messages,
    });
    if !req.system.is_empty() {
        body["system"] = json!([system]);
    }
    if let Some(t) = thinking {
        body["thinking"] = t;
    }
    if let Some(e) = effort {
        body["output_config"] = json!({ "effort": e });
    }
    // Sampling parameters are not sent: current models accept only the defaults.
    let _ = flavor;
    body
}

/// Cumulative usage tracked across `message_start` and `message_delta`.
#[derive(Debug, Default, Clone, Copy)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl Usage {
    fn update(&mut self, u: &Value) {
        let get = |k: &str| u.get(k).and_then(Value::as_u64);
        if let Some(v) = get("input_tokens") {
            self.input = v;
        }
        if let Some(v) = get("output_tokens") {
            self.output = v;
        }
        if let Some(v) = get("cache_read_input_tokens") {
            self.cache_read = v;
        }
        if let Some(v) = get("cache_creation_input_tokens") {
            self.cache_write = v;
        }
    }

    fn chunk(&self) -> LlmChunk {
        LlmChunk::Usage {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

/// Parses one SSE event of the Messages stream into chunks; `Err` ends the stream.
fn handle_event(name: &str, data: &str, usage: &mut Usage) -> Result<Vec<LlmChunk>, MeetCueError> {
    let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
    let kind = v.get("type").and_then(Value::as_str).unwrap_or(name);
    match kind {
        "message_start" => {
            if let Some(u) = v.pointer("/message/usage") {
                usage.update(u);
            }
            Ok(vec![usage.chunk()])
        }
        "content_block_delta" => match v.pointer("/delta/type").and_then(Value::as_str) {
            Some("text_delta") => Ok(v
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(|t| vec![LlmChunk::Text(t.to_string())])
                .unwrap_or_default()),
            // Thinking and other deltas are not shown.
            _ => Ok(Vec::new()),
        },
        "message_delta" => {
            if let Some(u) = v.get("usage") {
                usage.update(u);
            }
            if v.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err(MeetCueError::new(
                    codes::LLM_BAD_OUTPUT,
                    "the model declined this request",
                ));
            }
            Ok(vec![usage.chunk()])
        }
        "error" => {
            let etype = v
                .pointer("/error/type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let msg = v
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("");
            let code = match etype {
                "overloaded_error" | "rate_limit_error" => codes::LLM_RATE_LIMIT,
                "authentication_error" | "permission_error" => codes::LLM_AUTH,
                "not_found_error" => codes::LLM_MODEL_UNAVAILABLE,
                _ => codes::LLM_REQUEST,
            };
            Err(MeetCueError::new(code, format!("{etype}: {msg}")))
        }
        _ => Ok(Vec::new()),
    }
}

/// Whether a 400 is about a field this model does not accept.
fn rejects_option(body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    ["thinking", "effort", "output_config", "between_tools"]
        .iter()
        .any(|k| b.contains(k))
}

#[async_trait]
impl LlmProvider for Anthropic {
    fn id(&self) -> &'static str {
        match self.flavor {
            Flavor::Claude => "anthropic",
            Flavor::Bedrock => "bedrock",
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        if self.flavor == Flavor::Bedrock {
            // Bedrock's Messages endpoint has no model list; validate the key
            // with a one-token request and offer the known models.
            let req = LlmRequest::new(
                "anthropic.claude-haiku-5-5",
                meetcue_core::UsagePurpose::Probe,
            )
            .user("ping")
            .max_tokens(1);
            let mut s = self.stream(req).await?;
            while let Some(c) = s.next().await {
                c?;
            }
            return Ok(BEDROCK_MODELS
                .iter()
                .map(|(id, name)| ModelInfo {
                    id: id.to_string(),
                    display_name: name.to_string(),
                })
                .collect());
        }
        let client = net::http_client(self.proxy.as_deref())?;
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..10 {
            let mut url = format!("{}/models?limit=1000", self.base());
            if let Some(a) = &after {
                url.push_str(&format!("&after_id={a}"));
            }
            let resp = client
                .get(url)
                .header("x-api-key", &self.key)
                .header("anthropic-version", self.version())
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            let status = resp.status().as_u16();
            if status != 200 {
                let body = resp.text().await.unwrap_or_default();
                return Err(MeetCueError::new(
                    net::llm_status_code(status),
                    format!("HTTP {status}: {body}"),
                ));
            }
            let v: Value = resp
                .json()
                .await
                .map_err(|e| MeetCueError::new(codes::LLM_REQUEST, e.to_string()))?;
            for m in v["data"].as_array().into_iter().flatten() {
                if let Some(id) = m["id"].as_str() {
                    out.push(ModelInfo {
                        id: id.into(),
                        display_name: m["display_name"].as_str().unwrap_or(id).into(),
                    });
                }
            }
            if v["has_more"].as_bool() != Some(true) {
                break;
            }
            after = v["last_id"].as_str().map(String::from);
        }
        Ok(out)
    }

    async fn stream(&self, req: LlmRequest) -> Result<LlmStream, MeetCueError> {
        let client = net::http_client(self.proxy.as_deref())?;
        let url = format!("{}/messages", self.base());
        let mut minimal = false;
        let mut attempt = 0;
        let resp = loop {
            let body = request_body(&req, self.flavor, minimal);
            let sent = client
                .post(&url)
                .header("x-api-key", &self.key)
                .header("anthropic-version", self.version())
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;
            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    let err = net::reqwest_error(&e);
                    if attempt + 1 < MAX_ATTEMPTS && !e.is_timeout() {
                        tokio::time::sleep(net::backoff(attempt, None)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let retry_after = net::retry_after(resp.headers());
            let text = resp.text().await.unwrap_or_default();
            if status == 400 && !minimal && rejects_option(&text) {
                tracing::warn!(model = %req.model, "model rejected latency options; retrying without them");
                minimal = true;
                continue;
            }
            let retryable = matches!(status, 429 | 500 | 502 | 503 | 529);
            if retryable && attempt + 1 < MAX_ATTEMPTS {
                tokio::time::sleep(net::backoff(attempt, retry_after).min(Duration::from_secs(5)))
                    .await;
                attempt += 1;
                continue;
            }
            return Err(MeetCueError::new(
                net::llm_status_code(status),
                format!(
                    "HTTP {status}: {}",
                    text.chars().take(400).collect::<String>()
                ),
            ));
        };

        let (tx, rx) = futures::channel::mpsc::unbounded();
        tokio::spawn(async move {
            let mut parser = SseParser::default();
            let mut usage = Usage::default();
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
                    match handle_event(&ev.name, &ev.data, &mut usage) {
                        Ok(chunks) => {
                            for c in chunks {
                                if tx.unbounded_send(Ok(c)).is_err() {
                                    // The receiver was dropped (cancelled): stop reading.
                                    return;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.unbounded_send(Err(e));
                            return;
                        }
                    }
                    if ev.name == "message_stop" {
                        return;
                    }
                }
            }
        });
        Ok(rx.boxed())
    }
}

/// Models offered on Bedrock's Messages endpoint (it has no model list API).
pub const BEDROCK_MODELS: &[(&str, &str)] = &[
    ("anthropic.claude-haiku-5-5", "Claude Haiku 5.5"),
    ("anthropic.claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("anthropic.claude-opus-5-5", "Claude Opus 5.5"),
    ("anthropic.claude-haiku-4-5", "Claude Haiku 4.5"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use meetcue_core::UsagePurpose;

    fn req(model: &str) -> LlmRequest {
        LlmRequest::new(model, UsagePurpose::Hints)
            .system("stable instructions", true)
            .user("transcript and question")
            .max_tokens(700)
            .temperature(0.3)
    }

    #[test]
    fn body_has_cached_system_and_no_sampling() {
        let b = request_body(&req("claude-haiku-5-5"), Flavor::Claude, false);
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["stream"], true);
        assert_eq!(b["thinking"]["type"], "disabled");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["max_tokens"], 700);
        assert!(
            b.get("temperature").is_none(),
            "sampling parameters are rejected"
        );
        assert_eq!(b["messages"][0]["role"], "user");
    }

    #[test]
    fn per_model_latency_settings() {
        let b = request_body(&req("claude-sonnet-5-5"), Flavor::Claude, false);
        assert_eq!(b["thinking"]["type"], "between_tools");
        let b = request_body(&req("claude-opus-5-5"), Flavor::Claude, false);
        assert!(b.get("thinking").is_none());
        assert_eq!(
            b["max_tokens"], 4000,
            "room for thinking that cannot be turned off"
        );
        let b = request_body(&req("anthropic.claude-haiku-5-5"), Flavor::Bedrock, false);
        assert_eq!(b["thinking"]["type"], "disabled");
        let b = request_body(&req("claude-haiku-4-5"), Flavor::Claude, false);
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
        let b = request_body(&req("claude-future-9"), Flavor::Claude, false);
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
        let b = request_body(&req("claude-haiku-5-5"), Flavor::Claude, true);
        assert!(b.get("thinking").is_none() && b.get("output_config").is_none());
    }

    #[test]
    fn parses_stream_events() {
        let mut u = Usage::default();
        let start = r#"{"type":"message_start","message":{"id":"msg_1","usage":{"input_tokens":120,"cache_read_input_tokens":800,"cache_creation_input_tokens":0,"output_tokens":1}}}"#;
        assert_eq!(
            handle_event("message_start", start, &mut u).unwrap(),
            vec![LlmChunk::Usage {
                input: 120,
                output: 1,
                cache_read: 800,
                cache_write: 0
            }]
        );
        let delta = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"{\"n\":1"}}"#;
        assert_eq!(
            handle_event("content_block_delta", delta, &mut u).unwrap(),
            vec![LlmChunk::Text("{\"n\":1".into())]
        );
        let thinking = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#;
        assert!(handle_event("content_block_delta", thinking, &mut u)
            .unwrap()
            .is_empty());
        let end = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":240}}"#;
        assert_eq!(
            handle_event("message_delta", end, &mut u).unwrap(),
            vec![LlmChunk::Usage {
                input: 120,
                output: 240,
                cache_read: 800,
                cache_write: 0
            }]
        );
        assert!(handle_event("ping", "{\"type\":\"ping\"}", &mut u)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn stream_errors_map_to_codes() {
        let mut u = Usage::default();
        let e = handle_event(
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            &mut u,
        )
        .unwrap_err();
        assert_eq!(e.code, codes::LLM_RATE_LIMIT);
        assert!(e.is_transient());
        let e = handle_event(
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"refusal"}}"#,
            &mut u,
        )
        .unwrap_err();
        assert_eq!(e.code, codes::LLM_BAD_OUTPUT);
        assert_eq!(net::llm_status_code(529), codes::LLM_RATE_LIMIT);
        assert_eq!(net::llm_status_code(401), codes::LLM_AUTH);
        assert!(rejects_option(
            r#"{"error":{"message":"thinking.type: between_tools is not supported"}}"#
        ));
    }
}
