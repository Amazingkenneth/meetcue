//! Public endpoints of each provider (`data/endpoints.json`).
//!
//! No endpoint is preferred over another: the latency probe measures them
//! and "Auto" uses the fastest one.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

pub const ENDPOINTS_JSON: &str = include_str!("../data/endpoints.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEndpoints {
    pub kind: String,
    #[serde(default)]
    pub hosts: Vec<String>,
    /// REST hosts paired with streaming hosts, where they differ.
    #[serde(default)]
    pub api_hosts: BTreeMap<String, String>,
    #[serde(default)]
    pub user_defined: bool,
    /// Model timed by the latency probe when none is chosen.
    #[serde(default)]
    pub probe_model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EndpointCatalog {
    pub providers: BTreeMap<String, ProviderEndpoints>,
}

impl EndpointCatalog {
    pub fn builtin() -> &'static EndpointCatalog {
        static CATALOG: OnceLock<EndpointCatalog> = OnceLock::new();
        CATALOG
            .get_or_init(|| serde_json::from_str(ENDPOINTS_JSON).expect("endpoints.json is valid"))
    }

    pub fn hosts(&self, provider: &str) -> &[String] {
        self.providers
            .get(provider)
            .map(|p| p.hosts.as_slice())
            .unwrap_or_default()
    }

    pub fn probe_model(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider)?.probe_model.as_deref()
    }

    /// The REST host paired with a streaming host (same host if none is listed).
    pub fn api_host<'a>(&'a self, provider: &str, host: &'a str) -> &'a str {
        self.providers
            .get(provider)
            .and_then(|p| p.api_hosts.get(host))
            .map(String::as_str)
            .unwrap_or(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let c = EndpointCatalog::builtin();
        for p in ["soniox", "deepgram", "anthropic"] {
            assert!(!c.hosts(p).is_empty(), "{p}");
            assert!(c.probe_model(p).is_some(), "{p}");
        }
        for h in c.hosts("soniox") {
            assert!(c.api_host("soniox", h).starts_with("api."), "{h}");
        }
    }
}
