//! Provider abstractions for speech recognition and language models.

use async_trait::async_trait;
use futures::stream::BoxStream;
use meetcue_core::{Channel, MeetCueError, UsagePurpose};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Options for opening a streaming recognition session.
#[derive(Debug, Clone, Default)]
pub struct AsrOptions {
    /// The capture channel this stream recognises (for logs and mock replay).
    pub channel: Option<Channel>,
    pub model: String,
    /// Host to connect to; `None` uses the provider default.
    pub host: Option<String>,
    /// Expected languages as ISO codes, e.g. `["zh", "en"]`.
    pub languages: Vec<String>,
    /// Domain terms to boost (at most 50).
    pub hotwords: Vec<String>,
    /// Silence that ends a sentence, when the provider lets us choose.
    pub endpoint_silence_ms: u32,
    /// Explicit proxy URL; `None` uses the system settings.
    pub proxy: Option<String>,
}

/// A recognition result. Times are offsets from the start of the audio sent
/// on this stream, in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub enum AsrResult {
    /// Unstable text of the current sentence (replaces the previous partial).
    Partial {
        text: String,
        t0_ms: i64,
        t1_ms: i64,
    },
    /// Text that will not change any more (appended to the sentence).
    Final {
        text: String,
        t0_ms: i64,
        t1_ms: i64,
    },
    /// The speaker finished the sentence.
    Endpoint {
        t_ms: i64,
    },
    Error(MeetCueError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioUsage {
    /// Audio sent to the provider, which is what it bills.
    pub audio_ms: u64,
}

#[async_trait]
pub trait AsrProvider: Send + Sync {
    /// Stable id: `soniox`, `deepgram`, `mock`, …
    fn id(&self) -> &'static str;
    async fn open(&self, opts: AsrOptions) -> Result<Box<dyn AsrStream>, MeetCueError>;
    /// Checks the key without streaming audio (cheap REST call where possible).
    async fn verify(&self) -> Result<(), MeetCueError> {
        Ok(())
    }
    /// Models offered by the provider, when it has an API for that.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError> {
        Ok(Vec::new())
    }
}

#[async_trait]
pub trait AsrStream: Send {
    /// Sends 16 kHz mono PCM captured at `capture_ts_ms`.
    async fn write(&mut self, pcm16k: &[i16], capture_ts_ms: i64) -> Result<(), MeetCueError>;
    /// The result stream. Can be taken once.
    fn results(&mut self) -> BoxStream<'static, AsrResult>;
    /// Keeps the connection open while no audio is sent.
    async fn keepalive(&mut self) -> Result<(), MeetCueError> {
        Ok(())
    }
    /// Asks the provider to finalize pending text now (sent after trailing silence).
    async fn finalize(&mut self) -> Result<(), MeetCueError> {
        Ok(())
    }
    async fn close(self: Box<Self>) -> Result<AudioUsage, MeetCueError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmRequest {
    pub model: String,
    /// Stable instructions placed first so they can be served from the prompt cache.
    pub system: String,
    pub cache_system: bool,
    pub messages: Vec<LlmMessage>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub purpose: UsagePurpose,
}

impl LlmRequest {
    pub fn new(model: impl Into<String>, purpose: UsagePurpose) -> Self {
        Self {
            model: model.into(),
            system: String::new(),
            cache_system: false,
            messages: Vec::new(),
            max_tokens: 512,
            temperature: None,
            purpose,
        }
    }

    pub fn system(mut self, system: impl Into<String>, cache: bool) -> Self {
        self.system = system.into();
        self.cache_system = cache;
        self
    }

    pub fn user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(LlmMessage {
            role: Role::User,
            content: content.into(),
        });
        self
    }

    pub fn max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = n;
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LlmChunk {
    Text(String),
    Usage {
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

pub type LlmStream = BoxStream<'static, Result<LlmChunk, MeetCueError>>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable id: `anthropic`, `openai-compatible`, `mock`.
    fn id(&self) -> &'static str;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, MeetCueError>;
    async fn stream(&self, req: LlmRequest) -> Result<LlmStream, MeetCueError>;
}

/// Runs a request to completion and returns the full text plus usage.
pub async fn complete(
    llm: &dyn LlmProvider,
    req: LlmRequest,
) -> Result<(String, Option<LlmChunk>), MeetCueError> {
    use futures::StreamExt;
    let mut stream = llm.stream(req).await?;
    let mut text = String::new();
    let mut usage = None;
    while let Some(chunk) = stream.next().await {
        match chunk? {
            LlmChunk::Text(t) => text.push_str(&t),
            u @ LlmChunk::Usage { .. } => usage = Some(u),
        }
    }
    Ok((text, usage))
}
