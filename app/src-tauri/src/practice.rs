//! Practice mode runner: imports a script, reads questions aloud, feeds the
//! pacer with what the pipeline hears, and writes the review report.
//!
//! The pacing rules live in `meetcue_pipeline::practice::schedule`; this
//! module only connects them to speech synthesis, the hint pipeline, the
//! windows and the files in the `practice` folder.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

use meetcue_core::ipc::AppEvent;
use meetcue_core::settings::{HintLanguage, PanelMode, PlaybackMode};
use meetcue_core::{
    codes, Channel, MeetCueError, MeetCueEvent, PointStatus, PriceTable, UsageKind, UsagePurpose,
    UserAction,
};
use meetcue_pipeline::backend::now_ms;
use meetcue_pipeline::engine::ModelRef;
use meetcue_pipeline::practice::schedule::{Pacer, PacerConfig, PacerInput, PacerOutput};
use meetcue_pipeline::practice::{
    parse, review, PracticeReport, PracticeReportSummary, PracticeScript,
};
use meetcue_pipeline::provider::{complete, LlmChunk, LlmProvider, LlmRequest};
use meetcue_pipeline::text::{self, Lang};
use meetcue_pipeline::HintBackend;
use meetcue_platform::speech::Speaker;

use crate::state::AppState;
use crate::usage::{self, LedgerLine};
use crate::{config, session, windows, APP_EVENT, PIPELINE_EVENT};

/// A practice run in progress.
pub struct PracticeRunner {
    tx: mpsc::UnboundedSender<Msg>,
    paused: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy)]
pub enum Control {
    Pause,
    Resume,
    /// Ctrl+Alt+Space: pause when running, resume when paused.
    Toggle,
    Skip,
    Stop,
    Answered,
}

enum Msg {
    Control(Control),
    /// Speech synthesis finished utterance number `n`.
    SpeechDone(u64),
    FollowUp(Option<String>),
}

/// When no latency probe has measured recognition yet.
const DEFAULT_SIMULATED_DELAY_MS: f64 = 400.0;
/// Microphone level that counts as the user speaking, above the VAD threshold.
const VOICE_MARGIN_DB: f32 = 6.0;

pub fn control(app: &AppHandle, c: Control) {
    let state = app.state::<AppState>();
    let guard = state.practice.lock().unwrap();
    let Some(runner) = guard.as_ref() else { return };
    let c = match c {
        Control::Toggle if runner.paused.load(Ordering::SeqCst) => Control::Resume,
        Control::Toggle => Control::Pause,
        other => other,
    };
    let _ = runner.tx.send(Msg::Control(c));
}

/// Whether a practice run recognises speech: the other side unless its
/// latency is simulated, and the user when their answers are recognised.
pub fn needs_asr(s: &meetcue_core::Settings) -> bool {
    s.practice.playback != PlaybackMode::SimulatedLatency || s.practice.recognize_me
}

pub fn is_active(app: &AppHandle) -> bool {
    app.state::<AppState>().practice.lock().unwrap().is_some()
}

fn err(detail: impl Into<String>) -> MeetCueError {
    MeetCueError::new(codes::PRACTICE_FILE, detail)
}

/// Asks for a file and parses it.
pub async fn import(app: &AppHandle) -> Result<Option<PracticeScript>, MeetCueError> {
    let dialog = app
        .dialog()
        .file()
        .add_filter("Practice", &["srt", "vtt", "txt", "md", "csv"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file())
        .await
        .map_err(|e| err(e.to_string()))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let bytes = fs::read(&path).map_err(|e| err(e.to_string()))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    parse::parse_file(&name, &bytes).map(Some)
}

/// Labels subtitle lines without speakers as questions or answers.
pub async fn classify(
    app: &AppHandle,
    mut script: PracticeScript,
) -> Result<PracticeScript, MeetCueError> {
    let state = app.state::<AppState>();
    let (llm, model) = session::hint_llm(&state)?;
    let lines: Vec<(u32, String)> = script
        .items
        .iter()
        .map(|i| (i.id, i.text.clone()))
        .collect();
    let mut questions = Vec::new();
    // Long files are labelled in chunks to keep each reply small.
    for chunk in lines.chunks(60) {
        let req = review::classify_request(&model.model, chunk);
        let text = call(app, llm.as_ref(), &model, req).await?;
        let ids = review::parse_classification(&text)
            .ok_or_else(|| MeetCueError::new(codes::LLM_BAD_OUTPUT, "unreadable classification"))?;
        questions.extend(ids);
    }
    parse::apply_classification(&mut script, &questions);
    Ok(script)
}

/// Runs one model call to completion and records its usage.
async fn call(
    app: &AppHandle,
    llm: &dyn LlmProvider,
    model: &ModelRef,
    req: LlmRequest,
) -> Result<String, MeetCueError> {
    let purpose = req.purpose;
    let (text, usage) = complete(llm, req).await?;
    if let Some(LlmChunk::Usage {
        input,
        output,
        cache_read,
        cache_write,
    }) = usage
    {
        record_usage(
            app,
            model,
            purpose,
            UsageKind::Llm {
                input,
                output,
                cache_read,
                cache_write,
            },
        );
    }
    Ok(text)
}

fn record_usage(app: &AppHandle, model: &ModelRef, purpose: UsagePurpose, usage: UsageKind) {
    let state = app.state::<AppState>();
    if state.demo {
        return;
    }
    let overrides = state.settings().providers.pricing_overrides;
    let micros = PriceTable::current()
        .find(&overrides, &model.provider, &model.model)
        .map(|p| p.cost(&usage));
    let line = LedgerLine {
        at: chrono::Utc::now().to_rfc3339(),
        provider: model.provider.clone(),
        model: model.model.clone(),
        purpose,
        usage,
        micros,
    };
    if let Err(e) = usage::append(&state.paths.usage_ledger(), &line) {
        tracing::warn!("could not record usage: {e}");
    }
}

/// Recognition delay to simulate: the latest probe's median for the chosen
/// speech provider, with ±25% jitter.
fn simulated_delay_ms(state: &AppState) -> u64 {
    let provider = state.settings().providers.asr.provider;
    let median = state
        .probes
        .lock()
        .unwrap()
        .iter()
        .find_map(|r| {
            r.results
                .iter()
                .filter(|e| Some(&e.provider) == provider.as_ref())
                .filter_map(|e| e.final_result.as_ref().map(|s| s.median_ms))
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap_or(DEFAULT_SIMULATED_DELAY_MS);
    let jitter = rand::rng().random_range(0.75..1.25);
    (median * jitter).max(0.0) as u64
}

fn output_language(pref: HintLanguage, question: &str) -> Lang {
    match pref {
        HintLanguage::En => Lang::En,
        HintLanguage::ZhCn => Lang::Zh,
        HintLanguage::FollowQuestion => text::dominant_lang(question),
    }
}

pub async fn start(app: &AppHandle, script: PracticeScript) -> Result<(), MeetCueError> {
    if script.items.is_empty() {
        return Err(err("empty practice script"));
    }
    if is_active(app) {
        control(app, Control::Stop);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let state = app.state::<AppState>();
    // A practice run replaces a meeting session.
    session::stop(app).await;

    let s = state.settings();
    let simulated = s.practice.playback == PlaybackMode::SimulatedLatency;
    let mut cfg = session::build_config_with(&state, needs_asr(&s))?;
    if cfg.asr.is_some() {
        cfg.recognize_remote = !simulated;
        cfg.recognize_me = s.practice.recognize_me;
    }
    let hints_model = cfg.engine.hints.clone();
    let hints_llm = cfg.hints_llm.clone();
    state.backend.start(cfg).await?;
    if !state.demo {
        if let Err(e) = session::start_audio(app) {
            state.backend.stop().await;
            let _ = session::sync_preview(app, false);
            return Err(e);
        }
    }

    let (tx, rx) = mpsc::unbounded_channel();
    let paused = Arc::new(AtomicBool::new(false));
    *state.practice.lock().unwrap() = Some(PracticeRunner {
        tx: tx.clone(),
        paused: paused.clone(),
    });
    let pacer = Pacer::new(
        &script,
        PacerConfig {
            answer_pause_ms: (s.practice.answer_pause_s * 1000.0) as i64,
            nudge_after_ms: (s.practice.nudge_after_s * 1000.0) as i64,
            follow_up_probability: s.practice.follow_up_probability,
            simulated,
            stop_on_speech: s.audio.stop_tts_on_speech,
            seed: rand::rng().random(),
            ..Default::default()
        },
    );
    let ctx = RunCtx {
        app: app.clone(),
        tx,
        rx,
        paused,
        speaker: Speaker::new(),
        utterance: 0,
        hints_llm,
        hints_model,
        script,
        started_at: chrono::Utc::now(),
        started_ms: now_ms(),
    };
    tauri::async_runtime::spawn(run(ctx, pacer));
    let mode = match s.overlay.mode {
        PanelMode::Hidden => PanelMode::Full,
        m => m,
    };
    windows::set_mode(app, mode);
    Ok(())
}

struct RunCtx {
    app: AppHandle,
    tx: mpsc::UnboundedSender<Msg>,
    rx: mpsc::UnboundedReceiver<Msg>,
    paused: Arc<AtomicBool>,
    speaker: Speaker,
    utterance: u64,
    hints_llm: Arc<dyn LlmProvider>,
    hints_model: ModelRef,
    script: PracticeScript,
    started_at: chrono::DateTime<chrono::Utc>,
    started_ms: i64,
}

/// The hint model to use now, so a change in the settings applies to the
/// next follow-up or feedback call; the model the run started with if the
/// new one can't be set up.
fn current_hint_llm(ctx: &RunCtx) -> (Arc<dyn LlmProvider>, ModelRef) {
    session::hint_llm(&ctx.app.state::<AppState>())
        .unwrap_or_else(|_| (ctx.hints_llm.clone(), ctx.hints_model.clone()))
}

async fn run(mut ctx: RunCtx, mut pacer: Pacer) {
    let app = ctx.app.clone();
    let mut events = app.state::<AppState>().backend.events();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let voice_db = app.state::<AppState>().settings().audio.vad_threshold_db + VOICE_MARGIN_DB;

    let outputs = pacer.start(now_ms());
    apply(&mut ctx, outputs).await;
    while !pacer.is_done() {
        let input = tokio::select! {
            msg = ctx.rx.recv() => match msg {
                Some(Msg::Control(c)) => Some(match c {
                    Control::Pause => {
                        ctx.paused.store(true, Ordering::SeqCst);
                        app.state::<AppState>().backend.send(UserAction::Pause).await;
                        PacerInput::Pause
                    }
                    Control::Resume | Control::Toggle => {
                        ctx.paused.store(false, Ordering::SeqCst);
                        app.state::<AppState>().backend.send(UserAction::Resume).await;
                        PacerInput::Resume
                    }
                    Control::Skip => PacerInput::Skip,
                    Control::Stop => PacerInput::Stop,
                    Control::Answered => PacerInput::Answered,
                }),
                Some(Msg::SpeechDone(n)) => (n == ctx.utterance).then_some(PacerInput::SpeechDone),
                Some(Msg::FollowUp(q)) => Some(PacerInput::FollowUp(q)),
                None => Some(PacerInput::Stop),
            },
            ev = events.recv() => match ev {
                Ok(e) => event_input(&e, voice_db),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                // The session ended (budget limit or an error): finish.
                Err(_) => Some(PacerInput::Stop),
            },
            _ = tick.tick() => Some(PacerInput::Tick),
        };
        if let Some(input) = input {
            let stopping = input == PacerInput::Stop;
            let outputs = pacer.handle(input, now_ms());
            apply(&mut ctx, outputs).await;
            if stopping && !pacer.is_done() {
                break;
            }
        }
        if !app.state::<AppState>().backend.is_running() && !pacer.is_done() {
            let outputs = pacer.handle(PacerInput::Stop, now_ms());
            apply(&mut ctx, outputs).await;
        }
    }
    ctx.speaker.stop();
    finish(&ctx, &pacer).await;
}

/// Turns a pipeline event into pacer input.
fn event_input(e: &MeetCueEvent, voice_db: f32) -> Option<PacerInput> {
    match e {
        MeetCueEvent::Transcript {
            channel: Channel::Me,
            text,
            is_final,
            ..
        } => Some(PacerInput::MeText {
            text: text.clone(),
            is_final: *is_final,
        }),
        MeetCueEvent::Levels { me, .. } if *me >= voice_db => Some(PacerInput::MeVoice),
        MeetCueEvent::QuestionNew { id, .. } => Some(PacerInput::QuestionDetected { id: *id }),
        MeetCueEvent::Coverage { id, statuses } => Some(PacerInput::Coverage {
            id: *id,
            done: statuses.iter().filter(|s| **s == PointStatus::Done).count() as u32,
            total: statuses.len() as u32,
        }),
        _ => None,
    }
}

async fn apply(ctx: &mut RunCtx, outputs: Vec<PacerOutput>) {
    let app = ctx.app.clone();
    for o in outputs {
        match o {
            PacerOutput::Speak { text, lang } => {
                ctx.utterance += 1;
                let n = ctx.utterance;
                let speaker = ctx.speaker.clone();
                let tx = ctx.tx.clone();
                let rate = app.state::<AppState>().settings().practice.speech_rate;
                let lang = match lang {
                    Lang::Zh => "zh",
                    Lang::En => "en",
                };
                let app2 = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(e) = speaker.speak(&text, lang, rate) {
                        tracing::warn!("speech synthesis failed: {e}");
                        let _ = app2.emit(
                            PIPELINE_EVENT,
                            MeetCueEvent::Error {
                                code: e.code,
                                detail: e.detail,
                            },
                        );
                    }
                    let _ = tx.send(Msg::SpeechDone(n));
                });
            }
            PacerOutput::StopSpeaking => {
                // Ignore the "done" of the interrupted utterance.
                ctx.utterance += 1;
                ctx.speaker.stop();
            }
            PacerOutput::Inject {
                text,
                known_question,
            } => {
                let delay = simulated_delay_ms(&app.state::<AppState>());
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let backend = app.state::<AppState>().backend.clone();
                    if known_question {
                        backend.inject_question(text, now_ms()).await;
                    } else {
                        backend.inject_remote_text(text, now_ms()).await;
                    }
                });
            }
            PacerOutput::GenerateFollowUp { question, answer } => {
                let (llm, model) = current_hint_llm(ctx);
                let tx = ctx.tx.clone();
                let pref = app.state::<AppState>().settings().general.hint_language;
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let lang = output_language(pref, &question);
                    let req = review::follow_up_request(&model.model, &question, &answer, lang);
                    let reply = tokio::time::timeout(
                        Duration::from_secs(10),
                        call(&app, llm.as_ref(), &model, req),
                    )
                    .await;
                    let q = match reply {
                        Ok(Ok(text)) => review::clean_follow_up(&text),
                        _ => None,
                    };
                    let _ = tx.send(Msg::FollowUp(q));
                });
            }
            PacerOutput::Progress(p) => {
                let _ = app.emit(
                    APP_EVENT,
                    AppEvent::PracticeProgress {
                        index: p.index,
                        total: p.total,
                        phase: p.phase.as_str().into(),
                        question: p.question,
                    },
                );
            }
            PacerOutput::Nudge => {
                let _ = app.emit(
                    APP_EVENT,
                    AppEvent::PracticeProgress {
                        index: 0,
                        total: 0,
                        phase: "nudge".into(),
                        question: String::new(),
                    },
                );
            }
            PacerOutput::Finished => {}
        }
    }
}

async fn finish(ctx: &RunCtx, pacer: &Pacer) {
    let app = &ctx.app;
    let state = app.state::<AppState>();
    session::stop(app).await;
    let s = state.settings();
    let mut questions = review::reviews(
        pacer.records(),
        &s.practice.fillers_zh,
        &s.practice.fillers_en,
    );
    // Feedback against reference answers, a few at a time.
    let jobs: Vec<usize> = questions
        .iter()
        .enumerate()
        .filter(|(_, q)| q.reference.is_some() && !q.skipped && !q.answer.trim().is_empty())
        .map(|(i, _)| i)
        .collect();
    let (llm, model) = current_hint_llm(ctx);
    for batch in jobs.chunks(3) {
        let calls = batch.iter().map(|&i| {
            let q = &questions[i];
            let lang = output_language(s.general.hint_language, &q.question);
            let (llm, model) = (llm.clone(), model.clone());
            let req = review::feedback_request(
                &model.model,
                &q.question,
                q.reference.as_deref().unwrap_or_default(),
                &q.answer,
                lang,
            );
            async move {
                let reply = tokio::time::timeout(
                    Duration::from_secs(20),
                    call(app, llm.as_ref(), &model, req),
                )
                .await;
                (i, reply)
            }
        });
        for (i, reply) in futures::future::join_all(calls).await {
            match reply {
                Ok(Ok(text)) => questions[i].feedback = review::parse_feedback(&text),
                Ok(Err(e)) => tracing::warn!("review feedback failed: {e}"),
                Err(_) => tracing::warn!("review feedback timed out"),
            }
        }
    }
    let report = PracticeReport {
        id: ctx.started_at.format("%Y%m%d-%H%M%S").to_string(),
        title: ctx.script.title.clone(),
        started_at: ctx.started_at.to_rfc3339(),
        duration_ms: now_ms() - ctx.started_ms,
        playback: s.practice.playback,
        hint_level: s.practice.hint_level,
        simulated: s.practice.playback == PlaybackMode::SimulatedLatency,
        summary: review::summarize(&questions),
        questions,
    };
    *state.practice.lock().unwrap() = None;
    if let Err(e) = save(&state, &report) {
        tracing::warn!("could not save the practice report: {e}");
    }
    let _ = app.emit(
        APP_EVENT,
        AppEvent::PracticeFinished {
            report_id: report.id.clone(),
        },
    );
    if let Err(e) = windows::open_view(app, "review", Some(&report.id)) {
        tracing::warn!("could not open the review: {e}");
    }
}

fn report_path(state: &AppState, id: &str) -> Option<PathBuf> {
    // Ids are generated timestamps; refuse anything that could leave the folder.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(state.paths.practice_dir().join(format!("{id}.json")))
}

fn save(state: &AppState, report: &PracticeReport) -> Result<(), MeetCueError> {
    let path = report_path(state, &report.id).ok_or_else(|| err("bad report id"))?;
    let json = serde_json::to_string_pretty(report).map_err(|e| err(e.to_string()))?;
    config::write_atomic(&path, json.as_bytes()).map_err(|e| err(e.to_string()))
}

/// Saved reports, newest first.
pub fn reports(app: &AppHandle) -> Vec<PracticeReportSummary> {
    let state = app.state::<AppState>();
    let mut out: Vec<PracticeReportSummary> = fs::read_dir(state.paths.practice_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<PracticeReport>(&s).ok())
        .map(|r| PracticeReportSummary {
            id: r.id,
            title: r.title,
            started_at: r.started_at,
            questions: r.summary.questions,
            avg_start_latency_ms: r.summary.avg_start_latency_ms,
            avg_coverage: r.summary.avg_coverage,
        })
        .collect();
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    out
}

pub fn report(app: &AppHandle, id: &str) -> Result<PracticeReport, MeetCueError> {
    let state = app.state::<AppState>();
    let path = report_path(&state, id).ok_or_else(|| err("bad report id"))?;
    let text = fs::read_to_string(path).map_err(|e| err(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| err(e.to_string()))
}

/// Saves the report as Markdown where the user chooses.
pub async fn export(app: &AppHandle, id: &str) -> Result<Option<String>, MeetCueError> {
    let report = report(app, id)?;
    let lang = app.state::<AppState>().language();
    let md = review::markdown(&report, &|k| crate::i18n::t(lang, k));
    let safe: String = report
        .title
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let dialog = app
        .dialog()
        .file()
        .set_file_name(format!("{safe}-{}.md", report.id))
        .add_filter("Markdown", &["md"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| err(e.to_string()))?;
    let Some(path) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    fs::write(&path, md).map_err(|e| err(e.to_string()))?;
    Ok(Some(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_and_coverage_events_become_input() {
        let lvl = MeetCueEvent::Levels {
            me: -30.0,
            remote: -90.0,
        };
        assert_eq!(event_input(&lvl, -39.0), Some(PacerInput::MeVoice));
        assert_eq!(event_input(&lvl, -20.0), None);
        let cov = MeetCueEvent::Coverage {
            id: 3,
            statuses: vec![PointStatus::Done, PointStatus::Next, PointStatus::Done],
        };
        assert_eq!(
            event_input(&cov, 0.0),
            Some(PacerInput::Coverage {
                id: 3,
                done: 2,
                total: 3
            })
        );
        let remote = MeetCueEvent::Transcript {
            channel: Channel::Remote,
            utterance: 1,
            is_final: true,
            text: "x".into(),
            t0_ms: 0,
            t1_ms: 0,
        };
        assert_eq!(event_input(&remote, 0.0), None);
    }

    #[test]
    fn language_preference() {
        assert_eq!(
            output_language(HintLanguage::FollowQuestion, "什么是 SDN？"),
            Lang::Zh
        );
        assert_eq!(output_language(HintLanguage::En, "什么是 SDN？"), Lang::En);
    }
}
