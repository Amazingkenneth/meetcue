//! Latency probe: measures network, recognition and model latency per
//! provider and endpoint from the user's own network, and estimates the
//! end-to-end delay and hourly cost of each combination.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProbeTarget {
    pub provider: String,
    /// `asr` or `llm`.
    pub kind: String,
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Hosts to test; empty means every known endpoint of the provider.
    #[serde(default)]
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProbeRequest {
    pub targets: Vec<ProbeTarget>,
    /// Optional network label such as "home" or "office".
    #[ts(optional)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Only measure DNS/TCP/TLS (free, works without keys).
    #[serde(default)]
    pub network_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Stat {
    pub median_ms: f64,
    pub max_ms: f64,
    pub samples: u32,
}

impl Stat {
    pub fn from_samples(samples: &[f64]) -> Option<Stat> {
        if samples.is_empty() {
            return None;
        }
        let mut v = samples.to_vec();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = v.len() / 2;
        let median = if v.len() % 2 == 0 {
            (v[mid - 1] + v[mid]) / 2.0
        } else {
            v[mid]
        };
        Some(Stat {
            median_ms: median,
            max_ms: *v.last().unwrap(),
            samples: v.len() as u32,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EndpointProbe {
    pub provider: String,
    pub kind: String,
    pub host: String,
    pub model: Option<String>,
    pub dns: Option<Stat>,
    pub tcp: Option<Stat>,
    pub tls: Option<Stat>,
    /// ASR: start of speech to first partial result.
    pub first_partial: Option<Stat>,
    /// ASR: end of audio to final result.
    pub final_result: Option<Stat>,
    /// LLM: request to first token.
    pub first_token: Option<Stat>,
    /// LLM: request to first complete hint point.
    pub first_point: Option<Stat>,
    /// Error code if the endpoint could not be measured.
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub hourly_micros: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ComboEstimate {
    pub asr_provider: String,
    pub asr_host: String,
    pub asr_model: Option<String>,
    pub llm_provider: String,
    pub llm_host: String,
    pub llm_model: Option<String>,
    /// Sentence-end wait + recognition + detection + first hint point.
    pub end_to_end_ms: f64,
    #[ts(type = "number | null")]
    pub hourly_micros: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProbeReport {
    /// RFC 3339 time of the run.
    pub at: String,
    pub label: Option<String>,
    pub network_only: bool,
    pub results: Vec<EndpointProbe>,
    pub combos: Vec<ComboEstimate>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats() {
        let s = Stat::from_samples(&[30.0, 10.0, 20.0]).unwrap();
        assert_eq!((s.median_ms, s.max_ms, s.samples), (20.0, 30.0, 3));
        let s = Stat::from_samples(&[10.0, 20.0]).unwrap();
        assert_eq!(s.median_ms, 15.0);
        assert!(Stat::from_samples(&[]).is_none());
    }
}
