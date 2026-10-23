//! Starting, pausing and stopping a meeting session, and forwarding
//! pipeline events to every window.

use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use meetcue_audio::{CaptureConfig, SourceEvent};
use meetcue_core::ipc::AppEvent;
use meetcue_core::settings::EndpointChoice;
use meetcue_core::{
    codes, AudioFrame, Channel, MeetCueError, MeetCueEvent, SessionState, Settings, UserAction,
};
use meetcue_pipeline::backend::SessionConfig;
use meetcue_pipeline::endpoints::EndpointCatalog;
use meetcue_pipeline::engine::{EngineConfig, ModelRef};
use meetcue_pipeline::probe::ProbeReport;
use meetcue_pipeline::provider::AsrOptions;
use meetcue_pipeline::providers::{self, ProviderConfig};
use meetcue_pipeline::{materials, HintBackend};

use crate::state::AppState;
use crate::usage::{self, LedgerLine};
use crate::{tray, APP_EVENT, PIPELINE_EVENT};

/// The host to use for a provider: the user's choice, else the fastest
/// endpoint of the latest probe, else the first known endpoint.
pub fn choose_host(
    provider: &str,
    choice: &EndpointChoice,
    probes: &[ProbeReport],
) -> Option<String> {
    if let EndpointChoice::Host(h) = choice {
        return Some(h.clone());
    }
    let measured = probes.iter().find_map(|r| {
        r.results
            .iter()
            .filter(|e| e.provider == provider && e.error.is_none())
            .filter_map(|e| {
                let score = e
                    .final_result
                    .or(e.first_point)
                    .or(e.first_token)
                    .or(e.tls)
                    .map(|s| s.median_ms)?;
                Some((score, e.host.clone()))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, h)| h)
    });
    measured.or_else(|| EndpointCatalog::builtin().hosts(provider).first().cloned())
}

/// Provider config for one model slot, with its key from the credential store.
pub fn provider_config(
    state: &AppState,
    settings: &Settings,
    provider: &str,
    model: &str,
    choice: &EndpointChoice,
) -> Result<ProviderConfig, MeetCueError> {
    let key = state.credentials.get(provider)?;
    let probes = state.probes.lock().unwrap().clone();
    let host = match provider {
        "bedrock" => settings
            .providers
            .bedrock_region
            .as_deref()
            .and_then(|r| EndpointCatalog::builtin().host_for_region("bedrock", r)),
        "openai-compatible" => None,
        _ => choose_host(provider, choice, &probes),
    };
    Ok(ProviderConfig {
        provider: provider.into(),
        model: model.into(),
        host,
        key,
        region: settings.providers.bedrock_region.clone(),
        base_url: settings.providers.openai_base_url.clone(),
        proxy: settings.providers.proxy.clone(),
    })
}

fn engine_config(settings: &Settings, detect: ModelRef, hints: ModelRef) -> EngineConfig {
    EngineConfig {
        detect,
        hints,
        role: settings.hints.role.clone(),
        materials: materials::summary(&settings.hints.materials, materials::SUMMARY_CHARS),
        hint_language: settings.general.hint_language,
        only_directed_to_me: settings.hints.only_directed_to_me,
        queue_interruptions: settings.hints.queue_interruptions,
        track_coverage: settings.hints.track_coverage,
        early_trigger: settings.hints.early_trigger,
    }
}

fn this_month() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

/// Builds the session from settings and stored keys.
pub fn build_config(state: &AppState) -> Result<SessionConfig, MeetCueError> {
    let s = state.settings();
    if state.demo {
        let mock = ModelRef {
            provider: "mock".into(),
            model: "mock".into(),
        };
        let mut cfg = SessionConfig::new(
            engine_config(&s, mock.clone(), mock.clone()),
            Some(providers::asr(&ProviderConfig {
                provider: "mock".into(),
                ..Default::default()
            })?),
            providers::llm(&ProviderConfig {
                provider: "mock".into(),
                ..Default::default()
            })?,
            providers::llm(&ProviderConfig {
                provider: "mock".into(),
                ..Default::default()
            })?,
        );
        cfg.asr_model = mock;
        return Ok(cfg);
    }
    let p = &s.providers;
    let (Some(asr_p), Some(asr_m)) = (p.asr.provider.clone(), p.asr.model.clone()) else {
        return Err(MeetCueError::new(
            codes::ASR_NOT_CONFIGURED,
            "no speech recognition model chosen",
        ));
    };
    let (Some(hint_p), Some(hint_m)) = (p.hints.provider.clone(), p.hints.model.clone()) else {
        return Err(MeetCueError::new(
            codes::LLM_MODEL_UNAVAILABLE,
            "no hint model chosen",
        ));
    };
    // Detection falls back to the hint model if not chosen separately.
    let (det_p, det_m) = match (p.detect.provider.clone(), p.detect.model.clone()) {
        (Some(a), Some(b)) => (a, b),
        _ => (hint_p.clone(), hint_m.clone()),
    };
    let asr_cfg = provider_config(state, &s, &asr_p, &asr_m, &p.asr.endpoint)?;
    if asr_cfg.key.is_none() {
        return Err(MeetCueError::new(
            codes::ASR_NOT_CONFIGURED,
            format!("no key for {asr_p}"),
        ));
    }
    let asr = providers::asr(&asr_cfg)?;
    let hints_llm = providers::llm(&provider_config(
        state,
        &s,
        &hint_p,
        &hint_m,
        &p.hints.endpoint,
    )?)?;
    let detect_llm = providers::llm(&provider_config(
        state,
        &s,
        &det_p,
        &det_m,
        &p.detect.endpoint,
    )?)?;

    let texts: Vec<&str> = s
        .hints
        .materials
        .iter()
        .map(|m| m.text.as_str())
        .chain([s.hints.role.as_str()])
        .collect();
    let mut cfg = SessionConfig::new(
        engine_config(
            &s,
            ModelRef {
                provider: det_p,
                model: det_m,
            },
            ModelRef {
                provider: hint_p,
                model: hint_m,
            },
        ),
        Some(asr),
        detect_llm,
        hints_llm,
    );
    cfg.asr_options = AsrOptions {
        channel: None,
        model: asr_m.clone(),
        host: asr_cfg.host.clone(),
        languages: vec!["zh".into(), "en".into()],
        hotwords: materials::hotwords(&texts, materials::MAX_HOTWORDS),
        endpoint_silence_ms: s.audio.endpoint_silence_ms,
        proxy: s.providers.proxy.clone(),
    };
    cfg.asr_model = ModelRef {
        provider: asr_p,
        model: asr_m,
    };
    cfg.pricing_overrides = p.pricing_overrides.clone();
    cfg.budget = p.budget.clone();
    cfg.month_spent_micros =
        usage::month_total(&usage::read(&state.paths.usage_ledger()), &this_month());
    Ok(cfg)
}

pub async fn start(app: &AppHandle) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    let cfg = build_config(&state)?;
    state.backend.start(cfg).await?;
    if !state.demo {
        start_audio(app)?;
    }
    Ok(())
}

/// Ctrl+Alt+Space: start when idle, otherwise pause or resume.
pub async fn toggle(app: &AppHandle) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    let running = state.backend.is_running();
    match (running, state.session_state()) {
        (false, _) => start(app).await,
        (true, SessionState::Paused) => {
            state.backend.send(UserAction::Resume).await;
            Ok(())
        }
        (true, _) => {
            state.backend.send(UserAction::Pause).await;
            Ok(())
        }
    }
}

pub async fn stop(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Some(mut audio) = state.audio.lock().unwrap().take() {
        audio.stop();
    }
    state.backend.stop().await;
}

fn start_audio(app: &AppHandle) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    let s = state.settings();
    let (tx, mut rx) = mpsc::unbounded_channel::<SourceEvent>();
    let mut source = meetcue_audio::platform_source();
    source.start(
        CaptureConfig {
            mic_device: s.audio.mic_device.clone(),
            capture_me: true,
            capture_remote: true,
            vad_threshold_db: s.audio.vad_threshold_db,
        },
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
    )?;
    *state.audio.lock().unwrap() = Some(source);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let backend = app.state::<AppState>().backend.clone();
        while let Some(e) = rx.recv().await {
            match e {
                SourceEvent::Frame(f) => backend.push_audio(f.channel, f).await,
                SourceEvent::DeviceChanged(_) => {
                    let _ = app.emit(
                        APP_EVENT,
                        AppEvent::Notice {
                            code: codes::AUD_DEVICE_CHANGED.into(),
                            detail: String::new(),
                        },
                    );
                }
                SourceEvent::Error(ch, err) => {
                    tracing::warn!(channel = ch.as_str(), "audio error: {err}");
                    let _ = app.emit(
                        PIPELINE_EVENT,
                        MeetCueEvent::Error {
                            code: err.code,
                            detail: err.detail,
                        },
                    );
                }
            }
        }
    });
    Ok(())
}

/// Starts or stops capture without recognition, for the level check.
pub fn audio_preview(app: &AppHandle, on: bool) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    if let Some(mut p) = state.preview.lock().unwrap().take() {
        p.stop();
    }
    if !on || state.backend.is_running() {
        return Ok(());
    }
    let s = state.settings();
    let (tx, mut rx) = mpsc::unbounded_channel::<AudioFrame>();
    let mut source = meetcue_audio::platform_source();
    source.start(
        CaptureConfig {
            mic_device: s.audio.mic_device.clone(),
            capture_me: true,
            capture_remote: true,
            vad_threshold_db: s.audio.vad_threshold_db,
        },
        Box::new(move |e| {
            if let SourceEvent::Frame(f) = e {
                let _ = tx.send(f);
            }
        }),
    )?;
    *state.preview.lock().unwrap() = Some(source);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut levels = [-100f32; 2];
        let mut last = std::time::Instant::now();
        while let Some(f) = rx.recv().await {
            let i = usize::from(f.channel == Channel::Remote);
            levels[i] = levels[i].max(f.level_db);
            if last.elapsed().as_millis() >= 66 {
                let _ = app.emit(
                    PIPELINE_EVENT,
                    MeetCueEvent::Levels {
                        me: levels[0],
                        remote: levels[1],
                    },
                );
                levels = [-100.0; 2];
                last = std::time::Instant::now();
            }
        }
    });
    Ok(())
}

/// Forwards pipeline events to the windows, keeps the session state for the
/// tray, and records usage in the ledger.
pub fn spawn_forwarder(app: &AppHandle) {
    let app = app.clone();
    let mut rx = app.state::<AppState>().backend.events();
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match rx.recv().await {
                Ok(e) => e,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("event forwarder lagged by {n}");
                    continue;
                }
                Err(_) => break,
            };
            let state = app.state::<AppState>();
            match &event {
                MeetCueEvent::SessionState { state: s, .. } => {
                    *state.session.lock().unwrap() = *s;
                    tray::refresh(&app);
                    if *s == SessionState::Stopped {
                        if let Some(mut audio) = state.audio.lock().unwrap().take() {
                            audio.stop();
                        }
                    }
                }
                MeetCueEvent::Usage {
                    provider,
                    model,
                    purpose,
                    usage,
                    cost_micros,
                    ..
                } if !state.demo => {
                    let line = LedgerLine {
                        at: chrono::Utc::now().to_rfc3339(),
                        provider: provider.clone(),
                        model: model.clone(),
                        purpose: *purpose,
                        usage: usage.clone(),
                        micros: *cost_micros,
                    };
                    if let Err(e) = usage::append(&state.paths.usage_ledger(), &line) {
                        tracing::warn!("could not record usage: {e}");
                    }
                }
                MeetCueEvent::Transcript {
                    is_final: true,
                    text,
                    channel,
                    ..
                } if state.settings().diagnostics.debug_logging => {
                    tracing::debug!(channel = channel.as_str(), "transcript: {text}");
                }
                _ => {}
            }
            let _ = app.emit(PIPELINE_EVENT, event);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use meetcue_pipeline::probe::{EndpointProbe, Stat};

    fn stat(ms: f64) -> Option<Stat> {
        Some(Stat {
            median_ms: ms,
            max_ms: ms,
            samples: 3,
        })
    }

    #[test]
    fn host_choice() {
        let report = ProbeReport {
            results: vec![
                EndpointProbe {
                    provider: "soniox".into(),
                    host: "a".into(),
                    final_result: stat(500.0),
                    ..Default::default()
                },
                EndpointProbe {
                    provider: "soniox".into(),
                    host: "b".into(),
                    final_result: stat(300.0),
                    ..Default::default()
                },
                EndpointProbe {
                    provider: "soniox".into(),
                    host: "c".into(),
                    final_result: stat(100.0),
                    error: Some("x".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            choose_host(
                "soniox",
                &EndpointChoice::Auto,
                std::slice::from_ref(&report)
            )
            .as_deref(),
            Some("b")
        );
        assert_eq!(
            choose_host("soniox", &EndpointChoice::Host("z".into()), &[report]).as_deref(),
            Some("z")
        );
        let first = EndpointCatalog::builtin().hosts("deepgram")[0].clone();
        assert_eq!(
            choose_host("deepgram", &EndpointChoice::Auto, &[]),
            Some(first)
        );
    }
}
