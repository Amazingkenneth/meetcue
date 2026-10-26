//! Provider implementations and the factory that builds them from settings.

use std::sync::Arc;

use meetcue_core::{codes, MeetCueError};

use crate::provider::{AsrProvider, LlmProvider};

pub mod anthropic;
pub mod deepgram;
pub mod mock;
pub mod openai;
pub mod soniox;
pub mod sse;

/// Everything needed to construct a provider client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderConfig {
    pub provider: String,
    pub model: String,
    /// Host chosen by the latency probe or the user; `None` for the default.
    pub host: Option<String>,
    pub key: Option<String>,
    /// The user's base URL: required for OpenAI-compatible, optional for Anthropic.
    pub base_url: Option<String>,
    /// Explicit HTTPS proxy.
    pub proxy: Option<String>,
}

fn key(cfg: &ProviderConfig, code: &str) -> Result<String, MeetCueError> {
    cfg.key
        .clone()
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| MeetCueError::new(code, format!("no API key for {}", cfg.provider)))
}

pub fn asr(cfg: &ProviderConfig) -> Result<Arc<dyn AsrProvider>, MeetCueError> {
    match cfg.provider.as_str() {
        "mock" => Ok(Arc::new(mock::MockAsr::sdn())),
        "soniox" => Ok(Arc::new(soniox::Soniox::new(
            key(cfg, codes::ASR_NOT_CONFIGURED)?,
            cfg.host.clone(),
            cfg.proxy.clone(),
        ))),
        "deepgram" => Ok(Arc::new(deepgram::Deepgram::new(
            key(cfg, codes::ASR_NOT_CONFIGURED)?,
            cfg.host.clone(),
            cfg.proxy.clone(),
        ))),
        other => Err(MeetCueError::new(
            codes::ASR_NOT_CONFIGURED,
            format!("unknown provider {other}"),
        )),
    }
}

pub fn llm(cfg: &ProviderConfig) -> Result<Arc<dyn LlmProvider>, MeetCueError> {
    match cfg.provider.as_str() {
        "mock" => Ok(Arc::new(mock::MockLlm::sdn())),
        "anthropic" => Ok(Arc::new(anthropic::Anthropic::new(
            key(cfg, codes::LLM_AUTH)?,
            cfg.host.clone(),
            cfg.base_url.clone(),
            cfg.proxy.clone(),
        ))),
        "openai-compatible" => {
            let base = cfg
                .base_url
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .ok_or_else(|| {
                    MeetCueError::new(codes::LLM_REQUEST, "no base URL for openai-compatible")
                })?;
            Ok(Arc::new(openai::OpenAiCompatible::new(
                base,
                cfg.key.clone(),
                cfg.proxy.clone(),
            )))
        }
        other => Err(MeetCueError::new(
            codes::LLM_REQUEST,
            format!("unknown provider {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: &str) -> ProviderConfig {
        ProviderConfig {
            provider: provider.into(),
            key: Some("key".into()),
            base_url: Some("https://host/v1".into()),
            ..Default::default()
        }
    }

    #[test]
    fn builds_every_offered_provider() {
        for p in ["soniox", "deepgram", "mock"] {
            assert_eq!(asr(&cfg(p)).map(|a| a.id()).ok(), Some(p), "{p}");
        }
        for p in ["anthropic", "openai-compatible", "mock"] {
            assert_eq!(llm(&cfg(p)).map(|l| l.id()).ok(), Some(p), "{p}");
        }
    }

    #[test]
    fn missing_key_or_url_is_reported() {
        let no_key = |p: &str| ProviderConfig {
            key: None,
            ..cfg(p)
        };
        assert_eq!(
            asr(&no_key("deepgram")).err().map(|e| e.code),
            Some(codes::ASR_NOT_CONFIGURED.to_string())
        );
        assert_eq!(
            llm(&no_key("anthropic")).err().map(|e| e.code),
            Some(codes::LLM_AUTH.to_string())
        );
        // The key is optional for OpenAI-compatible servers; the URL is not.
        assert!(llm(&no_key("openai-compatible")).is_ok());
        let no_url = ProviderConfig {
            base_url: None,
            ..cfg("openai-compatible")
        };
        assert!(llm(&no_url).is_err());
        assert!(llm(&cfg("bedrock")).is_err());
    }
}
