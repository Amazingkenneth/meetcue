//! Any OpenAI-compatible chat completions API (`<base>/chat/completions`
//! with `stream: true`). Prices are unknown unless the user enters them.

use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{json, Value};

use meetcue_core::{codes, MeetCueError};

use crate::net;
use crate::provider::{LlmChunk, LlmProvider, LlmRequest, LlmStream, ModelInfo, Role};
use crate::providers::sse::SseParser;

pub struct OpenAiCompatible {
    base: String,
    key: Option<String>,
    proxy: Option<String>,
}

impl OpenAiCompatible {
    pub fn new(base_url: &str, key: Option<String>, proxy: Option<String>) -> Self {
        Self {
            base: base_url.trim().trim_end_matches('/').to_string(),
            key: key.filter(|k| !k.trim().is_empty()),
            proxy,
        }
    }

    fn auth(&self, b: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(k) => b.bearer_auth(k),
            None => b,
        }
    }
}

pub fn request_body(req: &LlmRequest) -> Value {
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
        "max_tokens": req.max_tokens,
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    body
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
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string();
        return Some(Err(MeetCueError::new(codes::LLM_REQUEST, msg)));
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
        let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
        let cached = u
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        out.push(LlmChunk::Usage {
            input: prompt.saturating_sub(cached),
            output: u["completion_tokens"].as_u64().unwrap_or(0),
            cache_read: cached,
            cache_write: 0,
        });
    }
    Some(Ok(out))
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
            return Err(MeetCueError::new(
                net::llm_status_code(status),
                format!("HTTP {status}"),
            ));
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| MeetCueError::new(codes::LLM_REQUEST, e.to_string()))?;
        Ok(v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str())
            .map(|id| ModelInfo {
                id: id.into(),
                display_name: id.into(),
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
        let body = request_body(&req);
        let mut attempt = 0;
        let resp = loop {
            let resp = self
                .auth(client.post(format!("{}/chat/completions", self.base)))
                .json(&body)
                .send()
                .await
                .map_err(|e| net::reqwest_error(&e))?;
            let status = resp.status().as_u16();
            if status == 200 {
                break resp;
            }
            let retry_after = net::retry_after(resp.headers());
            let text = resp.text().await.unwrap_or_default();
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
        assert!(handle_data("[DONE]").is_none());
        assert!(handle_data(r#"{"error":{"message":"bad"}}"#)
            .unwrap()
            .is_err());
    }
}
