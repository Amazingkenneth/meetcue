//! Error codes (`MC-<module>-<nnn>`) and the bilingual catalog in `data/errors.json`.
//!
//! The catalog is the single source for in-app error messages and for the
//! error code reference page of the documentation site.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Every code the application can raise. Each must exist in `errors.json`;
/// a unit test enforces this.
pub mod codes {
    pub const AUD_EXCLUSIVE: &str = "MC-AUD-001";
    pub const AUD_MIC_DENIED: &str = "MC-AUD-002";
    pub const AUD_NO_DEVICE: &str = "MC-AUD-003";
    pub const AUD_DEVICE_CHANGED: &str = "MC-AUD-004";
    pub const AUD_LOOPBACK_SILENT: &str = "MC-AUD-005";
    pub const AUD_SYSTEM_DENIED: &str = "MC-AUD-006";
    pub const AUD_UNSUPPORTED: &str = "MC-AUD-900";
    pub const ASR_AUTH: &str = "MC-ASR-001";
    pub const ASR_CONNECT: &str = "MC-ASR-002";
    pub const ASR_RATE_LIMIT: &str = "MC-ASR-003";
    pub const ASR_PROTOCOL: &str = "MC-ASR-004";
    pub const ASR_NOT_CONFIGURED: &str = "MC-ASR-005";
    pub const LLM_TIMEOUT: &str = "MC-LLM-001";
    pub const LLM_AUTH: &str = "MC-LLM-002";
    pub const LLM_RATE_LIMIT: &str = "MC-LLM-003";
    pub const LLM_MODEL_UNAVAILABLE: &str = "MC-LLM-004";
    pub const LLM_REQUEST: &str = "MC-LLM-005";
    pub const LLM_BAD_OUTPUT: &str = "MC-LLM-006";
    pub const HOTKEY_TAKEN: &str = "MC-HK-001";
    pub const SYS_WEBVIEW2: &str = "MC-SYS-001";
    pub const SYS_CONFIG_DIR: &str = "MC-SYS-002";
    pub const SYS_CREDENTIALS: &str = "MC-SYS-003";
    pub const SYS_UNSUPPORTED: &str = "MC-SYS-004";
    pub const SYS_SETTINGS_SAVE: &str = "MC-SYS-005";
    pub const SYS_INTERNAL: &str = "MC-SYS-006";
    pub const NET_TLS: &str = "MC-NET-001";
    pub const BUDGET_WARN: &str = "MC-BUD-001";
    pub const BUDGET_REACHED: &str = "MC-BUD-002";
    pub const PRACTICE_FILE: &str = "MC-PRC-001";
    pub const PRACTICE_TTS: &str = "MC-PRC-002";
    pub const DIAG_EXPORT: &str = "MC-DIAG-001";
    pub const KNOWLEDGE_FILE: &str = "MC-KB-001";

    pub const ALL: &[&str] = &[
        AUD_EXCLUSIVE,
        AUD_MIC_DENIED,
        AUD_NO_DEVICE,
        AUD_DEVICE_CHANGED,
        AUD_LOOPBACK_SILENT,
        AUD_SYSTEM_DENIED,
        AUD_UNSUPPORTED,
        ASR_AUTH,
        ASR_CONNECT,
        ASR_RATE_LIMIT,
        ASR_PROTOCOL,
        ASR_NOT_CONFIGURED,
        LLM_TIMEOUT,
        LLM_AUTH,
        LLM_RATE_LIMIT,
        LLM_MODEL_UNAVAILABLE,
        LLM_REQUEST,
        LLM_BAD_OUTPUT,
        HOTKEY_TAKEN,
        SYS_WEBVIEW2,
        SYS_CONFIG_DIR,
        SYS_CREDENTIALS,
        SYS_UNSUPPORTED,
        SYS_SETTINGS_SAVE,
        SYS_INTERNAL,
        NET_TLS,
        BUDGET_WARN,
        BUDGET_REACHED,
        PRACTICE_FILE,
        PRACTICE_TTS,
        DIAG_EXPORT,
        KNOWLEDGE_FILE,
    ];
}

/// An error carrying a catalog code plus a technical detail for the logs.
///
/// The detail must never contain API keys or tokens; providers pass response
/// bodies through [`redact`] before building an error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, TS)]
#[error("{code}: {detail}")]
#[ts(export)]
pub struct MeetCueError {
    pub code: String,
    pub detail: String,
}

pub type Result<T, E = MeetCueError> = std::result::Result<T, E>;

impl MeetCueError {
    pub fn new(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            detail: redact(&detail.into()),
        }
    }

    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }

    /// Whether retrying the same request later can succeed.
    pub fn is_transient(&self) -> bool {
        matches!(
            self.code.as_str(),
            codes::ASR_CONNECT
                | codes::ASR_RATE_LIMIT
                | codes::LLM_TIMEOUT
                | codes::LLM_RATE_LIMIT
                | codes::LLM_REQUEST
        )
    }
}

/// Masks anything that looks like a credential: bearer tokens, `key=`/`token=`
/// query parameters, `x-api-key` headers and long opaque strings.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut mask_next = false;
    for word in text.split_inclusive(char::is_whitespace) {
        let trimmed = word.trim_end();
        let tail = &word[trimmed.len()..];
        let lower = trimmed.to_ascii_lowercase();
        if mask_next && (lower == "bearer" || lower == "basic") {
            out.push_str(word);
            continue;
        }
        if mask_next {
            out.push_str("***");
            out.push_str(tail);
            mask_next = false;
            continue;
        }
        if lower == "bearer"
            || lower == "x-api-key:"
            || lower == "authorization:"
            || lower == "token"
        {
            mask_next = true;
            out.push_str(word);
            continue;
        }
        out.push_str(&redact_word(trimmed));
        out.push_str(tail);
    }
    out
}

fn redact_word(word: &str) -> String {
    for marker in [
        "key=",
        "token=",
        "api_key=",
        "apikey=",
        "secret=",
        "signature=",
    ] {
        if let Some(pos) = word.to_ascii_lowercase().find(marker) {
            let start = pos + marker.len();
            let end = word[start..]
                .find(['&', '"', '\'', ',', ';'])
                .map(|e| start + e)
                .unwrap_or(word.len());
            return format!("{}***{}", &word[..start], &word[end..]);
        }
    }
    let core = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    let looks_secret = core.len() >= 32
        && core
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && core.chars().any(|c| c.is_ascii_digit())
        && core.chars().any(|c| c.is_ascii_alphabetic());
    if looks_secret {
        word.replace(core, "***")
    } else {
        word.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Localized {
    pub en: String,
    #[serde(rename = "zh-CN")]
    #[ts(rename = "zh-CN")]
    pub zh_cn: String,
}

impl Localized {
    pub fn get(&self, lang: &str) -> &str {
        if lang.starts_with("zh") {
            &self.zh_cn
        } else {
            &self.en
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorAction {
    pub kind: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ErrorInfo {
    pub code: String,
    pub severity: Severity,
    pub title: Localized,
    pub fix: Localized,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub action: Option<ErrorAction>,
}

impl ErrorInfo {
    /// Anchor of this code on the error code reference page, e.g. `mc-aud-001`.
    pub fn anchor(&self) -> String {
        self.code.to_ascii_lowercase()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorCatalog {
    pub docs_page: String,
    pub errors: Vec<ErrorInfo>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

pub const ERRORS_JSON: &str = include_str!("../data/errors.json");

impl ErrorCatalog {
    pub fn parse(json: &str) -> serde_json::Result<Self> {
        let mut catalog: ErrorCatalog = serde_json::from_str(json)?;
        catalog.index = catalog
            .errors
            .iter()
            .enumerate()
            .map(|(i, e)| (e.code.clone(), i))
            .collect();
        Ok(catalog)
    }

    /// The catalog compiled into the binary.
    pub fn builtin() -> &'static ErrorCatalog {
        static CATALOG: OnceLock<ErrorCatalog> = OnceLock::new();
        CATALOG.get_or_init(|| ErrorCatalog::parse(ERRORS_JSON).expect("errors.json is valid"))
    }

    pub fn get(&self, code: &str) -> Option<&ErrorInfo> {
        self.index.get(code).map(|&i| &self.errors[i])
    }

    /// Documentation URL for a code in the given UI language.
    pub fn docs_url(&self, base_url: &str, code: &str, lang: &str) -> String {
        let base = base_url.trim_end_matches('/');
        let prefix = if lang.starts_with("zh") { "/zh" } else { "" };
        format!(
            "{base}{prefix}/{}#{}",
            self.docs_page,
            code.to_ascii_lowercase()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_in_catalog() {
        let catalog = ErrorCatalog::builtin();
        for code in codes::ALL {
            let info = catalog
                .get(code)
                .unwrap_or_else(|| panic!("{code} missing"));
            assert!(!info.title.en.is_empty() && !info.title.zh_cn.is_empty());
            assert!(!info.fix.en.is_empty() && !info.fix.zh_cn.is_empty());
        }
        assert_eq!(
            catalog.errors.len(),
            codes::ALL.len(),
            "catalog has unused codes"
        );
    }

    #[test]
    fn codes_are_well_formed_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for e in &ErrorCatalog::builtin().errors {
            let parts: Vec<_> = e.code.split('-').collect();
            assert_eq!(parts.len(), 3, "{}", e.code);
            assert_eq!(parts[0], "MC");
            assert_eq!(parts[2].len(), 3);
            assert!(seen.insert(e.code.clone()), "duplicate {}", e.code);
        }
    }

    #[test]
    fn docs_url_per_language() {
        let c = ErrorCatalog::builtin();
        assert_eq!(
            c.docs_url("https://x.github.io/meetcue/", "MC-AUD-001", "zh-CN"),
            "https://x.github.io/meetcue/zh/troubleshooting/error-codes#mc-aud-001"
        );
        assert_eq!(
            c.docs_url("https://x.github.io/meetcue", "MC-AUD-001", "en"),
            "https://x.github.io/meetcue/troubleshooting/error-codes#mc-aud-001"
        );
    }

    #[test]
    fn redacts_secrets() {
        let s = redact("Authorization: Bearer sk-ant-abc123 failed");
        assert!(!s.contains("sk-ant-abc123"), "{s}");
        let s = redact("GET /v1/listen?api_key=abcdef&model=nova-3");
        assert!(s.contains("api_key=***&model=nova-3"), "{s}");
        let s = redact("token 9f8e7d6c5b4a39f8e7d6c5b4a39f8e7d6c5b4a3");
        assert!(!s.contains("9f8e7d6c5b"), "{s}");
        let s = redact("key a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6 is invalid");
        assert!(!s.contains("a1b2c3d4e5f6a1b2"), "{s}");
        assert_eq!(redact("plain message"), "plain message");
    }
}
