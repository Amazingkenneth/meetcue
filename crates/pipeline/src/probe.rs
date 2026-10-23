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

// ---- runner ---------------------------------------------------------------

use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use futures::StreamExt;
use meetcue_core::pricing::{estimate_hourly, CostAssumptions, PriceEntry, PriceTable};
use meetcue_core::{codes, MeetCueError, UsageKind, UsagePurpose};

use crate::endpoints::EndpointCatalog;
use crate::hints::{HintItem, HintStreamParser};
use crate::net;
use crate::provider::{AsrOptions, AsrResult, LlmChunk, LlmRequest};
use crate::providers::{self, ProviderConfig};

/// Network samples per endpoint.
const NET_SAMPLES: usize = 5;
/// Recognition runs on the fastest endpoint of each speech provider.
const ASR_RUNS: usize = 3;
/// Model runs after one warm-up call (which fills the prompt cache).
const LLM_RUNS: usize = 3;

pub type Resolver =
    Arc<dyn Fn(&ProbeTarget, Option<&str>) -> Result<ProviderConfig, MeetCueError> + Send + Sync>;
pub type Progress = Arc<dyn Fn(u32, u32, &str) + Send + Sync>;
pub type UsageSink = Arc<dyn Fn(&str, &str, UsagePurpose, UsageKind) + Send + Sync>;

pub struct ProbeEnv {
    /// Provider config (with key) for a target at a host.
    pub resolve: Resolver,
    pub progress: Progress,
    pub on_usage: UsageSink,
    pub assumptions: CostAssumptions,
    pub overrides: Vec<PriceEntry>,
    /// Silence the recognizer waits before ending a sentence.
    pub sentence_wait_ms: f64,
    /// Probe speech: 16 kHz mono PCM, a mixed-language sentence ending in a question.
    pub audio: Arc<Vec<i16>>,
    pub proxy: Option<String>,
}

fn hosts_for(t: &ProbeTarget, env: &ProbeEnv) -> Vec<String> {
    if !t.hosts.is_empty() {
        return t.hosts.clone();
    }
    let known = EndpointCatalog::builtin().hosts(&t.provider);
    if !known.is_empty() {
        return known.to_vec();
    }
    let Ok(cfg) = (env.resolve)(t, None) else {
        return Vec::new();
    };
    cfg.host
        .or_else(|| {
            cfg.base_url
                .as_deref()
                .and_then(|u| u.split("://").nth(1))
                .and_then(|r| r.split(['/', ':']).next())
                .map(String::from)
        })
        .into_iter()
        .collect()
}

async fn time_network(
    host: &str,
    proxy: Option<&str>,
) -> (Option<Stat>, Option<Stat>, Option<Stat>, Option<String>) {
    let proxy = net::proxy_for(host, proxy);
    let tls = match net::tls_config() {
        Ok(c) => tokio_rustls::TlsConnector::from(c),
        Err(e) => return (None, None, None, Some(e.code)),
    };
    let Ok(server_name) = rustls::pki_types::ServerName::try_from(host.to_string()) else {
        return (None, None, None, Some(codes::NET_TLS.into()));
    };
    let (mut dns, mut tcp, mut hs) = (Vec::new(), Vec::new(), Vec::new());
    let mut error = None;
    for _ in 0..NET_SAMPLES {
        if proxy.is_none() {
            let t0 = Instant::now();
            match tokio::time::timeout(net::CONNECT_TIMEOUT, tokio::net::lookup_host((host, 443)))
                .await
            {
                Ok(Ok(_)) => dns.push(t0.elapsed().as_secs_f64() * 1000.0),
                _ => {
                    error = Some(codes::ASR_CONNECT.to_string());
                    break;
                }
            }
        }
        let t1 = Instant::now();
        let stream = match net::tcp(host, 443, proxy.as_ref()).await {
            Ok(s) => s,
            Err(e) => {
                error = Some(e.code);
                break;
            }
        };
        tcp.push(t1.elapsed().as_secs_f64() * 1000.0);
        let t2 = Instant::now();
        match tokio::time::timeout(
            net::CONNECT_TIMEOUT,
            tls.connect(server_name.clone(), stream),
        )
        .await
        {
            Ok(Ok(_)) => hs.push(t2.elapsed().as_secs_f64() * 1000.0),
            _ => {
                error = Some(codes::NET_TLS.into());
                break;
            }
        }
    }
    (
        Stat::from_samples(&dns),
        Stat::from_samples(&tcp),
        Stat::from_samples(&hs),
        error,
    )
}

/// Streams the probe audio in real time and measures the first partial
/// (from the start of speech) and the final result (from the end of audio).
async fn time_asr(cfg: &ProviderConfig, env: &ProbeEnv) -> Result<(f64, f64, u64), MeetCueError> {
    let asr = providers::asr(cfg)?;
    let mut stream = asr
        .open(AsrOptions {
            channel: None,
            model: cfg.model.clone(),
            host: cfg.host.clone(),
            languages: vec!["zh".into(), "en".into()],
            hotwords: Vec::new(),
            endpoint_silence_ms: env.sentence_wait_ms as u32,
            proxy: cfg.proxy.clone(),
        })
        .await?;
    let mut results = stream.results();
    let frame = meetcue_core::audio::FRAME_SAMPLES;
    let speech_start_idx = env
        .audio
        .chunks(frame)
        .position(|c| crate::text_level(c) > -40.0)
        .unwrap_or(0);
    let started = Instant::now();
    let speech_at = started + Duration::from_millis(speech_start_idx as u64 * 20);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(Instant, bool)>();
    let reader = tokio::spawn(async move {
        while let Some(r) = results.next().await {
            let is_final = matches!(r, AsrResult::Endpoint { .. } | AsrResult::Final { .. });
            if let AsrResult::Error(_) = r {
                break;
            }
            if tx.send((Instant::now(), is_final)).is_err() {
                break;
            }
        }
    });
    for (i, chunk) in env.audio.chunks(frame).enumerate() {
        tokio::time::sleep_until(started + Duration::from_millis(i as u64 * 20)).await;
        stream.write(chunk, 0).await?;
    }
    let audio_end = Instant::now();
    stream.finalize().await?;
    let mut first_partial = None;
    let mut final_at = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while let Ok(Some((at, is_final))) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if first_partial.is_none() {
            first_partial = Some(at.saturating_duration_since(speech_at));
        }
        if is_final && at >= audio_end {
            final_at = Some(at - audio_end);
            break;
        }
    }
    reader.abort();
    let usage = stream
        .close()
        .await
        .map(|u| u.audio_ms)
        .unwrap_or(env.audio.len() as u64 / 16);
    match (first_partial, final_at) {
        (Some(p), Some(f)) => Ok((p.as_secs_f64() * 1000.0, f.as_secs_f64() * 1000.0, usage)),
        _ => Err(MeetCueError::new(
            codes::ASR_PROTOCOL,
            "no result for the probe audio",
        )),
    }
}

const PROBE_TRANSCRIPT: &str = "[Them] Thanks for joining. Let's talk about the platform.\n[Me] Sure.\n[Them] How do you keep the service available when a data center goes down?";

/// Measures request → first token and request → first complete talking point.
async fn time_llm(cfg: &ProviderConfig, env: &ProbeEnv) -> Result<(f64, f64), MeetCueError> {
    let llm = providers::llm(cfg)?;
    let system = crate::prompts::render(
        crate::prompts::HINTS_SYSTEM,
        &[
            ("role", "not specified"),
            ("materials", "(none)"),
            ("output_language", "English"),
        ],
    );
    let user = crate::prompts::render(
        crate::prompts::HINTS_USER,
        &[
            ("transcript", PROBE_TRANSCRIPT),
            (
                "question",
                "How do you keep the service available when a data center goes down?",
            ),
        ],
    );
    let req = LlmRequest::new(&cfg.model, UsagePurpose::Probe)
        .system(system, true)
        .user(user)
        .max_tokens(crate::prompts::HINTS_MAX_TOKENS)
        .temperature(0.3);
    let t0 = Instant::now();
    let mut stream = llm.stream(req).await?;
    let mut parser = HintStreamParser::default();
    let mut first_token = None;
    let mut usage = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let result = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Err(_) => {
                break Err(MeetCueError::new(
                    codes::LLM_TIMEOUT,
                    "probe request timed out",
                ))
            }
            Ok(None) => {
                break Err(MeetCueError::new(
                    codes::LLM_BAD_OUTPUT,
                    "no talking point in the reply",
                ))
            }
            Ok(Some(Err(e))) => break Err(e),
            Ok(Some(Ok(LlmChunk::Usage {
                input,
                output,
                cache_read,
                cache_write,
            }))) => {
                usage = Some(UsageKind::Llm {
                    input,
                    output,
                    cache_read,
                    cache_write,
                });
            }
            Ok(Some(Ok(LlmChunk::Text(t)))) => {
                let ft = *first_token.get_or_insert_with(|| t0.elapsed());
                if parser
                    .push(&t)
                    .iter()
                    .any(|i| matches!(i, HintItem::Point { .. }))
                {
                    // Stop here: the rest of the answer is not needed.
                    break Ok((
                        ft.as_secs_f64() * 1000.0,
                        t0.elapsed().as_secs_f64() * 1000.0,
                    ));
                }
            }
        }
    };
    drop(stream);
    if let Some(u) = usage {
        (env.on_usage)(&cfg.provider, &cfg.model, UsagePurpose::Probe, u);
    }
    result
}

fn price(env: &ProbeEnv, provider: &str, model: Option<&str>) -> Option<PriceEntry> {
    let model = model?;
    PriceTable::builtin()
        .find(&env.overrides, provider, model)
        .cloned()
}

fn hourly(env: &ProbeEnv, kind: &str, provider: &str, model: Option<&str>) -> Option<u64> {
    let p = price(env, provider, model)?;
    let e = if kind == "asr" {
        estimate_hourly(Some(&p), None, None, &env.assumptions)
    } else {
        estimate_hourly(None, Some(&p), Some(&p), &env.assumptions)
    };
    Some(e.total)
}

/// Runs the probe. Network timing works without keys; recognition and model
/// timing use the configured keys and incur a small cost.
pub async fn run(req: &ProbeRequest, env: ProbeEnv) -> ProbeReport {
    let total = req.targets.len() as u32;
    let mut results = Vec::new();
    for (i, t) in req.targets.iter().enumerate() {
        (env.progress)(i as u32, total, &t.provider);
        let hosts = hosts_for(t, &env);
        let mut endpoint_results = Vec::new();
        for host in &hosts {
            let (dns, tcp, tls, error) = time_network(host, env.proxy.as_deref()).await;
            endpoint_results.push(EndpointProbe {
                provider: t.provider.clone(),
                kind: t.kind.clone(),
                host: host.clone(),
                model: t.model.clone(),
                dns,
                tcp,
                tls,
                error,
                hourly_micros: hourly(&env, &t.kind, &t.provider, t.model.as_deref()),
                ..Default::default()
            });
        }
        if !req.network_only {
            // Full measurement on the fastest reachable endpoint only.
            let best = endpoint_results
                .iter_mut()
                .filter(|r| r.error.is_none())
                .min_by(|a, b| {
                    let ka = a.tls.map(|s| s.median_ms).unwrap_or(f64::MAX);
                    let kb = b.tls.map(|s| s.median_ms).unwrap_or(f64::MAX);
                    ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(best) = best {
                match (env.resolve)(t, Some(&best.host)) {
                    Err(e) => best.error = Some(e.code),
                    Ok(cfg) if cfg.model.is_empty() => {
                        best.error = Some(codes::LLM_MODEL_UNAVAILABLE.into())
                    }
                    Ok(cfg) => {
                        best.model = Some(cfg.model.clone());
                        best.hourly_micros = hourly(&env, &t.kind, &t.provider, Some(&cfg.model));
                        if t.kind == "asr" {
                            let (mut partial, mut fin) = (Vec::new(), Vec::new());
                            for _ in 0..ASR_RUNS {
                                match time_asr(&cfg, &env).await {
                                    Ok((p, f, ms)) => {
                                        partial.push(p);
                                        fin.push(f);
                                        (env.on_usage)(
                                            &cfg.provider,
                                            &cfg.model,
                                            UsagePurpose::Probe,
                                            UsageKind::Audio { ms },
                                        );
                                    }
                                    Err(e) => {
                                        best.error = Some(e.code);
                                        break;
                                    }
                                }
                            }
                            best.first_partial = Stat::from_samples(&partial);
                            best.final_result = Stat::from_samples(&fin);
                        } else {
                            let (mut tok, mut point) = (Vec::new(), Vec::new());
                            for run in 0..=LLM_RUNS {
                                match time_llm(&cfg, &env).await {
                                    // The first call only warms the prompt cache.
                                    Ok(_) if run == 0 => {}
                                    Ok((a, b)) => {
                                        tok.push(a);
                                        point.push(b);
                                    }
                                    Err(e) => {
                                        best.error = Some(e.code);
                                        break;
                                    }
                                }
                            }
                            best.first_token = Stat::from_samples(&tok);
                            best.first_point = Stat::from_samples(&point);
                        }
                    }
                }
            }
        }
        results.extend(endpoint_results);
    }
    (env.progress)(total, total, "");
    let combos = combos(&results, env.sentence_wait_ms);
    ProbeReport {
        at: chrono::Utc::now().to_rfc3339(),
        label: req.label.clone(),
        network_only: req.network_only,
        results,
        combos,
    }
}

/// Every measured recognition endpoint combined with every measured model.
pub fn combos(results: &[EndpointProbe], sentence_wait_ms: f64) -> Vec<ComboEstimate> {
    let asr: Vec<_> = results
        .iter()
        .filter(|r| r.kind == "asr" && r.final_result.is_some())
        .collect();
    let llm: Vec<_> = results
        .iter()
        .filter(|r| r.kind == "llm" && r.first_point.is_some())
        .collect();
    let mut out = Vec::new();
    for a in &asr {
        for l in &llm {
            // Detection is a short, non-streamed call: about one first-token time.
            let detect = l.first_token.map(|s| s.median_ms).unwrap_or(0.0);
            out.push(ComboEstimate {
                asr_provider: a.provider.clone(),
                asr_host: a.host.clone(),
                asr_model: a.model.clone(),
                llm_provider: l.provider.clone(),
                llm_host: l.host.clone(),
                llm_model: l.model.clone(),
                end_to_end_ms: sentence_wait_ms
                    + a.final_result.unwrap().median_ms
                    + detect
                    + l.first_point.unwrap().median_ms,
                hourly_micros: match (a.hourly_micros, l.hourly_micros) {
                    (Some(x), Some(y)) => Some(x + y),
                    _ => None,
                },
            });
        }
    }
    out.sort_by(|a, b| {
        a.end_to_end_ms
            .partial_cmp(&b.end_to_end_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod run_tests {
    use super::*;

    fn stat(ms: f64) -> Option<Stat> {
        Stat::from_samples(&[ms])
    }

    #[test]
    fn combos_add_up_and_sort() {
        let results = vec![
            EndpointProbe {
                provider: "soniox".into(),
                kind: "asr".into(),
                host: "a".into(),
                final_result: stat(300.0),
                hourly_micros: Some(100),
                ..Default::default()
            },
            EndpointProbe {
                provider: "soniox".into(),
                kind: "asr".into(),
                host: "b".into(),
                final_result: stat(200.0),
                hourly_micros: Some(100),
                ..Default::default()
            },
            EndpointProbe {
                provider: "soniox".into(),
                kind: "asr".into(),
                host: "c".into(),
                ..Default::default()
            },
            EndpointProbe {
                provider: "anthropic".into(),
                kind: "llm".into(),
                host: "x".into(),
                first_token: stat(300.0),
                first_point: stat(500.0),
                hourly_micros: Some(10),
                ..Default::default()
            },
        ];
        let c = combos(&results, 600.0);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].asr_host, "b");
        assert_eq!(c[0].end_to_end_ms, 600.0 + 200.0 + 300.0 + 500.0);
        assert_eq!(c[0].hourly_micros, Some(110));
    }

    fn env() -> ProbeEnv {
        let audio: Vec<i16> = (0..16_000 * 2)
            .map(|i| {
                if (4_000..28_000).contains(&i) {
                    ((i % 50) as i16 - 25) * 400
                } else {
                    0
                }
            })
            .collect();
        ProbeEnv {
            resolve: Arc::new(|t, host| {
                Ok(ProviderConfig {
                    provider: "mock".into(),
                    model: t.model.clone().unwrap_or("mock".into()),
                    host: host.map(String::from),
                    ..Default::default()
                })
            }),
            progress: Arc::new(|_, _, _| {}),
            on_usage: Arc::new(|_, _, _, _| {}),
            assumptions: CostAssumptions::default(),
            overrides: vec![],
            sentence_wait_ms: 700.0,
            audio: Arc::new(audio),
            proxy: None,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn measures_mock_recognition() {
        let env = env();
        let cfg = ProviderConfig {
            provider: "mock".into(),
            model: "mock".into(),
            ..Default::default()
        };
        let (partial, fin, audio_ms) = time_asr(&cfg, &env).await.unwrap();
        // Speech starts 250 ms in; the mock answers on the first voiced frame.
        assert!(partial < 50.0, "{partial}");
        assert!(fin < 50.0, "{fin}");
        assert_eq!(audio_ms, 2_000);
    }

    #[tokio::test(start_paused = true)]
    async fn measures_mock_model() {
        let env = env();
        let cfg = ProviderConfig {
            provider: "mock".into(),
            model: "mock".into(),
            ..Default::default()
        };
        let (first_token, first_point) = time_llm(&cfg, &env).await.unwrap();
        assert!((280.0..400.0).contains(&first_token), "{first_token}");
        assert!(first_point >= first_token);
    }

    #[tokio::test]
    async fn network_only_with_no_targets() {
        let req = ProbeRequest {
            targets: vec![],
            label: Some("home".into()),
            network_only: true,
        };
        let r = run(&req, env()).await;
        assert_eq!(r.label.as_deref(), Some("home"));
        assert!(r.results.is_empty() && r.combos.is_empty());
    }
}
