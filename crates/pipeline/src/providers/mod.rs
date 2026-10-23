//! Provider implementations and the factory that builds them from settings.

use std::sync::Arc;

use meetcue_core::{codes, MeetCueError};

use crate::provider::{AsrProvider, LlmProvider};

pub mod mock;

/// Everything needed to construct a provider client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderConfig {
    pub provider: String,
    pub model: String,
    /// Host chosen by the latency probe or the user; `None` for the default.
    pub host: Option<String>,
    pub key: Option<String>,
    /// AWS region (Bedrock).
    pub region: Option<String>,
    /// OpenAI-compatible base URL.
    pub base_url: Option<String>,
    /// Explicit HTTPS proxy.
    pub proxy: Option<String>,
}

pub fn asr(cfg: &ProviderConfig) -> Result<Arc<dyn AsrProvider>, MeetCueError> {
    match cfg.provider.as_str() {
        "mock" => Ok(Arc::new(mock::MockAsr::sdn())),
        other => Err(MeetCueError::new(
            codes::ASR_NOT_CONFIGURED,
            format!("unknown provider {other}"),
        )),
    }
}

pub fn llm(cfg: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, MeetCueError> {
    match cfg.provider.as_str() {
        "mock" => Ok(Arc::new(mock::MockLlm::sdn())),
        other => Err(MeetCueError::new(
            codes::LLM_REQUEST,
            format!("unknown provider {other}"),
        )),
    }
}
