//! Starting, pausing and stopping a meeting session, and forwarding
//! pipeline events to every window.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
use meetcue_pipeline::provider::{AsrOptions, LlmProvider};
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
        "openai-compatible" => None,
        _ => choose_host(provider, choice, &probes),
    };
    Ok(ProviderConfig {
        provider: provider.into(),
        model: model.into(),
        host,
        key,
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

/// The hint model with its key, for practice follow-ups, labels and feedback.
pub fn hint_llm(state: &AppState) -> Result<(Arc<dyn LlmProvider>, ModelRef), MeetCueError> {
    if state.demo {
        let llm = providers::llm(&ProviderConfig {
            provider: "mock".into(),
            ..Default::default()
        })?;
        return Ok((
            llm,
            ModelRef {
                provider: "mock".into(),
                model: "mock".into(),
            },
        ));
    }
    let s = state.settings();
    let p = &s.providers;
    let (Some(provider), Some(model)) = (p.hints.provider.clone(), p.hints.model.clone()) else {
        return Err(MeetCueError::new(
            codes::LLM_MODEL_UNAVAILABLE,
            "no hint model chosen",
        ));
    };
    let llm = providers::llm(&provider_config(
        state,
        &s,
        &provider,
        &model,
        &p.hints.endpoint,
    )?)?;
    Ok((llm, ModelRef { provider, model }))
}

/// Re-reads the system proxy so WebSocket connections follow it like HTTPS
/// requests do. Called before connecting, so changes apply without a restart.
pub fn refresh_system_proxy() {
    meetcue_pipeline::net::set_system_proxy(
        meetcue_platform::proxy::system_proxy().map(|p| (p.server, p.bypass)),
    );
}

/// Builds the session from settings and stored keys.
pub fn build_config(state: &AppState) -> Result<SessionConfig, MeetCueError> {
    build_config_with(state, true)
}

/// Like [`build_config`]; with `need_asr` false a missing recognition setup
/// is allowed and recognition is left out (practice without recognition).
pub fn build_config_with(state: &AppState, need_asr: bool) -> Result<SessionConfig, MeetCueError> {
    refresh_system_proxy();
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
        cfg.asr_signature = "mock".into();
        return Ok(cfg);
    }
    let p = &s.providers;
    let asr_choice = (p.asr.provider.clone(), p.asr.model.clone());
    let (Some(asr_p), Some(asr_m)) = asr_choice else {
        if need_asr {
            return Err(MeetCueError::new(
                codes::ASR_NOT_CONFIGURED,
                "no speech recognition model chosen",
            ));
        }
        return build_without_asr(state, &s);
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
        if !need_asr {
            return build_without_asr(state, &s);
        }
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
        languages: s.audio.languages.clone(),
        hotwords: materials::hotwords(&texts, materials::MAX_HOTWORDS),
        endpoint_silence_ms: s.audio.endpoint_silence_ms,
        proxy: s.providers.proxy.clone(),
    };
    cfg.asr_model = ModelRef {
        provider: asr_p.clone(),
        model: asr_m,
    };
    cfg.asr_signature = asr_signature(&asr_cfg, &cfg.asr_options);
    cfg.hangover_ms = hangover_ms(s.audio.endpoint_silence_ms);
    if asr_p == "deepgram" && !meetcue_pipeline::providers::deepgram::covers_all(&s.audio.languages)
    {
        tracing::warn!(
            "Deepgram can't recognise Chinese and English in one stream; using {} for {:?}",
            meetcue_pipeline::providers::deepgram::language_param(&s.audio.languages),
            s.audio.languages
        );
    }
    cfg.pricing_overrides = p.pricing_overrides.clone();
    cfg.budget = p.budget.clone();
    cfg.month_spent_micros =
        usage::month_total(&usage::read(&state.paths.usage_ledger()), &this_month());
    Ok(cfg)
}

/// Identifies a recognition setup, so a running session reconnects only when
/// something that matters changed. The key is included as a hash, so a new
/// key reconnects too; the signature itself is never logged or stored.
fn asr_signature(cfg: &ProviderConfig, opts: &AsrOptions) -> String {
    use std::hash::{Hash, Hasher};
    let mut key = std::collections::hash_map::DefaultHasher::new();
    cfg.key.hash(&mut key);
    format!(
        "{}|{}|{:?}|{:x}|{:?}|{:?}|{}|{:?}",
        cfg.provider,
        cfg.model,
        cfg.host,
        key.finish(),
        opts.languages,
        opts.hotwords,
        opts.endpoint_silence_ms,
        opts.proxy
    )
}

/// Silence still sent after speech: a little longer than the end-of-sentence
/// silence the provider waits for, so its own endpoint fires first and the
/// forced finalize is only a backstop.
fn hangover_ms(endpoint_silence_ms: u32) -> i64 {
    (i64::from(endpoint_silence_ms) + 300).clamp(800, 3300)
}

/// Applies changed settings or keys to a running meeting or practice session
/// without restarting it. If the new settings can't be used (say a key is
/// missing), the session keeps its current models and the reason is shown.
pub async fn apply_live(app: &AppHandle) {
    // Typing in the role box saves on every key; apply once it settles.
    static LATEST: AtomicU64 = AtomicU64::new(0);
    if !settled(&LATEST, 1500).await {
        return;
    }
    let state = app.state::<AppState>();
    if !state.backend.is_running() || state.demo {
        return;
    }
    let built = if crate::practice::is_active(app) {
        build_config_with(&state, crate::practice::needs_asr(&state.settings()))
    } else {
        build_config(&state)
    };
    match built {
        Ok(cfg) => state.backend.reconfigure(cfg).await,
        Err(e) => {
            tracing::warn!(code = %e.code, "new settings not applied to the running session: {}", e.detail);
            let _ = app.emit(
                PIPELINE_EVENT,
                MeetCueEvent::Error {
                    code: e.code,
                    detail: e.detail,
                },
            );
        }
    }
}

/// Waits `ms`; false if another call with the same counter came meanwhile,
/// so only the last of a burst of changes is applied.
async fn settled(counter: &AtomicU64, ms: u64) -> bool {
    let me = counter.fetch_add(1, Ordering::SeqCst) + 1;
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    counter.load(Ordering::SeqCst) == me
}

/// Restarts capture (and the level preview) with a new microphone or voice
/// threshold, so the change applies during a meeting without restarting it.
pub async fn restart_audio(app: &AppHandle) {
    static LATEST: AtomicU64 = AtomicU64::new(0);
    if !settled(&LATEST, 800).await {
        return;
    }
    let state = app.state::<AppState>();
    let old = state.audio.lock().unwrap().take();
    if let Some(mut old) = old {
        old.stop();
        match start_audio(app) {
            Ok(()) => tracing::info!("audio capture restarted with the new microphone settings"),
            Err(e) => {
                let _ = reported(app, Err(e));
            }
        }
    }
    let previewing = state.preview.lock().unwrap().is_some();
    if previewing {
        let _ = audio_preview(app, true);
    }
}

fn build_without_asr(state: &AppState, s: &Settings) -> Result<SessionConfig, MeetCueError> {
    let (hints_llm, hints) = hint_llm(state)?;
    let p = &s.providers;
    let (detect_llm, detect) = match (p.detect.provider.clone(), p.detect.model.clone()) {
        (Some(provider), Some(model)) => (
            providers::llm(&provider_config(
                state,
                s,
                &provider,
                &model,
                &p.detect.endpoint,
            )?)?,
            ModelRef { provider, model },
        ),
        _ => (hints_llm.clone(), hints.clone()),
    };
    let mut cfg = SessionConfig::new(engine_config(s, detect, hints), None, detect_llm, hints_llm);
    cfg.recognize_me = false;
    cfg.recognize_remote = false;
    cfg.pricing_overrides = p.pricing_overrides.clone();
    cfg.budget = p.budget.clone();
    cfg.month_spent_micros =
        usage::month_total(&usage::read(&state.paths.usage_ledger()), &this_month());
    Ok(cfg)
}

/// Shows a failed start, pause or resume in every window and in the log, so
/// a click is never silently ignored. Returns the result unchanged.
pub fn reported(app: &AppHandle, result: Result<(), MeetCueError>) -> Result<(), MeetCueError> {
    if let Err(e) = &result {
        tracing::warn!(code = %e.code, "could not start or resume: {}", e.detail);
        let _ = app.emit(
            PIPELINE_EVENT,
            MeetCueEvent::Error {
                code: e.code.clone(),
                detail: e.detail.clone(),
            },
        );
    }
    result
}

pub async fn start(app: &AppHandle) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    let cfg = build_config(&state)?;
    tracing::info!(
        asr = %format!("{}/{}", cfg.asr_model.provider, cfg.asr_model.model),
        detect = %format!("{}/{}", cfg.engine.detect.provider, cfg.engine.detect.model),
        hints = %format!("{}/{}", cfg.engine.hints.provider, cfg.engine.hints.model),
        "starting session"
    );
    state.backend.start(cfg).await?;
    if !state.demo {
        if let Err(e) = start_audio(app) {
            // Without audio the session would look started but hear nothing.
            state.backend.stop().await;
            return Err(e);
        }
    }
    tracing::info!("session started");
    Ok(())
}

/// Ctrl+Alt+Space: start when idle, otherwise pause or resume.
pub async fn toggle(app: &AppHandle) -> Result<(), MeetCueError> {
    if crate::practice::is_active(app) {
        crate::practice::control(app, crate::practice::Control::Toggle);
        return Ok(());
    }
    let state = app.state::<AppState>();
    let running = state.backend.is_running();
    match (running, state.session_state()) {
        (false, _) => start(app).await,
        (true, SessionState::Paused) => {
            tracing::info!("resuming");
            state.backend.send(UserAction::Resume).await;
            Ok(())
        }
        (true, _) => {
            tracing::info!("pausing");
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

pub fn start_audio(app: &AppHandle) -> Result<(), MeetCueError> {
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
    tracing::info!("audio capture started");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let backend = app.state::<AppState>().backend.clone();
        // Every 10 s, log whether sound arrives on each side, so the
        // activity log shows when capture works but hears nothing.
        let mut frames = [0u32; 2];
        let mut peak = [-100f32; 2];
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(10));
        tick.tick().await;
        loop {
            let e = tokio::select! {
                e = rx.recv() => match e {
                    Some(e) => e,
                    None => break,
                },
                _ = tick.tick() => {
                    log_audio(&frames, &peak);
                    frames = [0; 2];
                    peak = [-100.0; 2];
                    continue;
                }
            };
            match e {
                SourceEvent::Frame(f) => {
                    let i = usize::from(f.channel == Channel::Remote);
                    frames[i] += 1;
                    peak[i] = peak[i].max(f.level_db);
                    backend.push_audio(f.channel, f).await
                }
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

fn log_audio(frames: &[u32; 2], peak: &[f32; 2]) {
    for (i, side) in ["microphone (me)", "system sound (others)"]
        .iter()
        .enumerate()
    {
        if frames[i] == 0 {
            tracing::warn!("audio: no sound from the {side} in the last 10 s");
        } else {
            tracing::info!("audio: {side} {} frames, peak {:.0} dB", frames[i], peak[i]);
        }
    }
}

/// Logs what the pipeline does, without transcript text unless debug
/// logging is on.
fn log_event(event: &MeetCueEvent) {
    match event {
        MeetCueEvent::SessionState { state, .. } => tracing::info!("session {state:?}"),
        MeetCueEvent::Transcript {
            channel,
            is_final: true,
            text,
            ..
        } => {
            tracing::info!(
                "recognised a sentence from {} ({} characters)",
                channel.as_str(),
                text.chars().count()
            );
            tracing::debug!("{}: {text}", channel.as_str());
        }
        MeetCueEvent::QuestionNew { id, version, .. } => {
            tracing::info!("question {id} detected (version {version})")
        }
        MeetCueEvent::AnswerDone { id, .. } => tracing::info!("hints for question {id} ready"),
        MeetCueEvent::HintsFailed { id, code, .. } => {
            tracing::warn!(code = %code, "hints for question {id} failed")
        }
        MeetCueEvent::LatencySample { id, total_ms, .. } => {
            tracing::info!("question {id}: first hint {total_ms} ms after the question ended")
        }
        MeetCueEvent::Usage {
            provider,
            model,
            cost_micros,
            ..
        } => tracing::debug!("usage {provider}/{model}: {cost_micros:?} micro-USD"),
        MeetCueEvent::Error { code, detail } => tracing::warn!(code = %code, "{detail}"),
        _ => {}
    }
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
            log_event(&event);
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
                    tracing::debug!(
                        target: crate::diagnostics::TRANSCRIPT_TARGET,
                        channel = channel.as_str(),
                        "{text}"
                    );
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

    #[test]
    fn recognition_reconnects_only_when_its_setup_changes() {
        let cfg = ProviderConfig {
            provider: "soniox".into(),
            model: "stt-rt-v5".into(),
            key: Some("k1".into()),
            ..Default::default()
        };
        let opts = AsrOptions {
            languages: vec!["zh".into(), "en".into()],
            endpoint_silence_ms: 700,
            ..Default::default()
        };
        let base = asr_signature(&cfg, &opts);
        assert_eq!(base, asr_signature(&cfg.clone(), &opts.clone()));
        let new_key = ProviderConfig {
            key: Some("k2".into()),
            ..cfg.clone()
        };
        assert_ne!(base, asr_signature(&new_key, &opts), "a new key reconnects");
        assert!(
            !base.contains("k1"),
            "the key itself is not in the signature"
        );
        let english = AsrOptions {
            languages: vec!["en".into()],
            ..opts.clone()
        };
        assert_ne!(base, asr_signature(&cfg, &english));
    }

    #[test]
    fn hangover_follows_the_end_of_sentence_silence() {
        assert_eq!(hangover_ms(700), 1000);
        assert_eq!(hangover_ms(1500), 1800);
        assert_eq!(hangover_ms(100), 800);
        assert_eq!(hangover_ms(10_000), 3300);
    }
}
