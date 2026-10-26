//! The async driver around [`Engine`]: audio in, events out.
//!
//! One session owns two recognition streams (microphone and system sound),
//! the engine loop, and the model calls the engine asks for. All model calls
//! are cancellable; cancelling drops the HTTP stream so billing stops.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{interval, sleep, sleep_until, timeout, Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use meetcue_core::settings::BudgetSettings;
use meetcue_core::{
    codes, AudioFrame, Channel, MeetCueError, MeetCueEvent, PriceEntry, TimedEvent, UsageKind,
    UsagePurpose, Usd, UserAction,
};

use crate::engine::{Engine, EngineConfig, Input, ModelRef, Output, ReqId};
use crate::gate::{GateDecision, SendGate};
use crate::hints::HintStreamParser;
use crate::provider::{
    AsrOptions, AsrProvider, AsrResult, AsrStream, LlmChunk, LlmProvider, LlmRequest,
};
use crate::timemap::TimeMap;
use crate::usage::UsageRecord;

/// Everything a session needs. Providers are already constructed with their keys.
#[derive(Clone)]
pub struct SessionConfig {
    pub engine: EngineConfig,
    /// `None` disables recognition entirely (practice with injected text only).
    pub asr: Option<Arc<dyn AsrProvider>>,
    pub asr_options: AsrOptions,
    pub asr_model: ModelRef,
    /// Identifies the recognition setup (provider, model, key, endpoint,
    /// options). A running session reconnects recognition when it changes.
    pub asr_signature: String,
    pub recognize_me: bool,
    pub recognize_remote: bool,
    pub detect_llm: Arc<dyn LlmProvider>,
    pub hints_llm: Arc<dyn LlmProvider>,
    pub pricing_overrides: Vec<PriceEntry>,
    pub budget: BudgetSettings,
    /// Spending earlier this month, for the monthly budget.
    pub month_spent_micros: Usd,
    /// Silence still sent after speech so the recognizer can end the sentence.
    pub hangover_ms: i64,
    pub first_token_timeout: Duration,
    pub detect_timeout: Duration,
}

impl SessionConfig {
    pub fn new(
        engine: EngineConfig,
        asr: Option<Arc<dyn AsrProvider>>,
        detect_llm: Arc<dyn LlmProvider>,
        hints_llm: Arc<dyn LlmProvider>,
    ) -> Self {
        Self {
            engine,
            asr,
            asr_options: AsrOptions::default(),
            asr_model: ModelRef::default(),
            asr_signature: String::new(),
            recognize_me: true,
            recognize_remote: true,
            detect_llm,
            hints_llm,
            pricing_overrides: Vec::new(),
            budget: BudgetSettings::default(),
            month_spent_micros: 0,
            hangover_ms: 800,
            first_token_timeout: Duration::from_secs(3),
            detect_timeout: Duration::from_secs(8),
        }
    }
}

/// What a finished session cost.
#[derive(Debug, Clone, Default)]
pub struct SessionSummary {
    pub duration_ms: i64,
    pub total_micros: Usd,
    pub records: Vec<UsageRecord>,
}

/// The whole hint pipeline behind one interface, so a remote implementation
/// can replace the local one without touching the UI.
#[async_trait]
pub trait HintBackend: Send + Sync {
    async fn start(&self, cfg: SessionConfig) -> Result<(), MeetCueError>;
    async fn stop(&self) -> Option<SessionSummary>;
    async fn push_audio(&self, ch: Channel, frame: AudioFrame);
    /// Practice mode: text "heard" from the other side, run through detection.
    async fn inject_remote_text(&self, text: String, ts_ms: i64);
    /// Practice mode: a known question; skips detection.
    async fn inject_question(&self, text: String, ts_ms: i64);
    async fn send(&self, action: UserAction);
    /// Applies new settings to the running session without restarting it:
    /// later model calls use the new models, recognition reconnects if its
    /// setup changed, and calls already running finish on the old ones.
    async fn reconfigure(&self, cfg: SessionConfig);
    fn events(&self) -> broadcast::Receiver<MeetCueEvent>;
    /// Events of the current session so far, for windows opened mid-session.
    fn history(&self) -> Vec<TimedEvent>;
    fn is_running(&self) -> bool;
}

/// Milliseconds on the shared process clock, following tokio's clock so tests
/// can run with paused time.
pub fn now_ms() -> i64 {
    meetcue_core::clock::since_epoch_ms(Instant::now().into_std())
}

const HISTORY_LIMIT: usize = 5_000;

struct Running {
    input: mpsc::UnboundedSender<Input>,
    reconfig: mpsc::UnboundedSender<Reconfig>,
    /// Recognition setup shared with both channels; `None` without recognition.
    asr_setup: Option<watch::Sender<AsrSetup>>,
    asr_signature: String,
    audio: [Option<mpsc::Sender<AudioFrame>>; 2],
    levels: Arc<[AtomicU32; 2]>,
    asr_cancel: CancellationToken,
    loop_cancel: CancellationToken,
    loop_task: JoinHandle<SessionSummary>,
    asr_tasks: Vec<JoinHandle<()>>,
}

pub struct LocalHintBackend {
    events: broadcast::Sender<MeetCueEvent>,
    history: Arc<Mutex<Vec<TimedEvent>>>,
    running: tokio::sync::Mutex<Option<Running>>,
    active: Arc<AtomicBool>,
}

impl Default for LocalHintBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalHintBackend {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(2048);
        Self {
            events,
            history: Arc::new(Mutex::new(Vec::new())),
            running: tokio::sync::Mutex::new(None),
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn ch_index(ch: Channel) -> usize {
    match ch {
        Channel::Me => 0,
        Channel::Remote => 1,
    }
}

#[async_trait]
impl HintBackend for LocalHintBackend {
    async fn start(&self, cfg: SessionConfig) -> Result<(), MeetCueError> {
        if let Some(old) = self.running.lock().await.take() {
            shutdown(old).await;
        }
        self.history.lock().unwrap().clear();
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let asr_cancel = CancellationToken::new();
        let loop_cancel = CancellationToken::new();
        let paused = Arc::new(AtomicBool::new(false));
        let levels: Arc<[AtomicU32; 2]> = Arc::new([
            AtomicU32::new((-100f32).to_bits()),
            AtomicU32::new((-100f32).to_bits()),
        ]);

        let mut audio: [Option<mpsc::Sender<AudioFrame>>; 2] = [None, None];
        let mut asr_tasks = Vec::new();
        let mut asr_setup = None;
        if let Some(asr) = cfg.asr.clone() {
            let (setup_tx, setup_rx) = watch::channel(AsrSetup {
                provider: asr,
                options: cfg.asr_options.clone(),
                model: cfg.asr_model.clone(),
                hangover_ms: cfg.hangover_ms,
            });
            asr_setup = Some(setup_tx);
            for (ch, enabled) in [
                (Channel::Me, cfg.recognize_me),
                (Channel::Remote, cfg.recognize_remote),
            ] {
                if !enabled {
                    continue;
                }
                let (tx, rx) = mpsc::channel(500);
                audio[ch_index(ch)] = Some(tx);
                asr_tasks.push(tokio::spawn(asr_channel(AsrChannelCtx {
                    channel: ch,
                    setup: setup_rx.clone(),
                    frames: rx,
                    input: input_tx.clone(),
                    paused: paused.clone(),
                    cancel: asr_cancel.clone(),
                })));
            }
        }

        let meter = crate::usage::CostMeter::new(
            cfg.pricing_overrides.clone(),
            cfg.budget.clone(),
            cfg.month_spent_micros,
        );
        let engine = Engine::new(cfg.engine.clone(), meter);
        let (reconfig_tx, reconfig_rx) = mpsc::unbounded_channel();
        let ctx = LoopCtx {
            events: self.events.clone(),
            history: self.history.clone(),
            input_tx: input_tx.clone(),
            detect_llm: cfg.detect_llm.clone(),
            hints_llm: cfg.hints_llm.clone(),
            detect_model: cfg.engine.detect.clone(),
            hints_model: cfg.engine.hints.clone(),
            first_token_timeout: cfg.first_token_timeout,
            detect_timeout: cfg.detect_timeout,
            paused,
            levels: levels.clone(),
            asr_cancel: asr_cancel.clone(),
            loop_cancel: loop_cancel.clone(),
            active: self.active.clone(),
        };
        self.active.store(true, Ordering::SeqCst);
        let loop_task = tokio::spawn(run_loop(engine, input_rx, reconfig_rx, ctx));
        *self.running.lock().await = Some(Running {
            input: input_tx,
            reconfig: reconfig_tx,
            asr_setup,
            asr_signature: cfg.asr_signature.clone(),
            audio,
            levels,
            asr_cancel,
            loop_cancel,
            loop_task,
            asr_tasks,
        });
        Ok(())
    }

    async fn stop(&self) -> Option<SessionSummary> {
        let running = self.running.lock().await.take()?;
        Some(shutdown(running).await)
    }

    async fn push_audio(&self, ch: Channel, frame: AudioFrame) {
        let guard = self.running.lock().await;
        let Some(r) = guard.as_ref() else { return };
        let level = &r.levels[ch_index(ch)];
        let prev = f32::from_bits(level.load(Ordering::Relaxed));
        level.store(prev.max(frame.level_db).to_bits(), Ordering::Relaxed);
        if let Some(tx) = &r.audio[ch_index(ch)] {
            // Never block the capture path: drop the frame if the queue is full.
            if tx.try_send(frame).is_err() {
                tracing::warn!(channel = ch.as_str(), "audio queue full, frame dropped");
            }
        }
    }

    async fn inject_remote_text(&self, text: String, ts_ms: i64) {
        self.input(Input::RemoteText {
            text,
            t_ms: ts_ms,
            known_question: false,
        })
        .await;
    }

    async fn inject_question(&self, text: String, ts_ms: i64) {
        self.input(Input::RemoteText {
            text,
            t_ms: ts_ms,
            known_question: true,
        })
        .await;
    }

    async fn reconfigure(&self, cfg: SessionConfig) {
        let mut guard = self.running.lock().await;
        let Some(r) = guard.as_mut() else {
            return;
        };
        let _ = r.reconfig.send(Reconfig {
            detect_llm: cfg.detect_llm.clone(),
            hints_llm: cfg.hints_llm.clone(),
            engine: cfg.engine.clone(),
            overrides: cfg.pricing_overrides.clone(),
            budget: cfg.budget.clone(),
        });
        // Recognition reconnects only when its setup really changed; a
        // session started without recognition keeps running without it.
        if let (Some(setup), Some(asr)) = (r.asr_setup.as_ref(), cfg.asr.clone()) {
            if cfg.asr_signature != r.asr_signature {
                r.asr_signature = cfg.asr_signature.clone();
                setup.send_replace(AsrSetup {
                    provider: asr,
                    options: cfg.asr_options.clone(),
                    model: cfg.asr_model.clone(),
                    hangover_ms: cfg.hangover_ms,
                });
            }
        }
    }

    async fn send(&self, action: UserAction) {
        self.input(Input::Action(action)).await;
    }

    fn events(&self) -> broadcast::Receiver<MeetCueEvent> {
        self.events.subscribe()
    }

    fn history(&self) -> Vec<TimedEvent> {
        self.history.lock().unwrap().clone()
    }

    fn is_running(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}

impl LocalHintBackend {
    async fn input(&self, input: Input) {
        if let Some(r) = self.running.lock().await.as_ref() {
            let _ = r.input.send(input);
        }
    }
}

async fn shutdown(r: Running) -> SessionSummary {
    // Close recognition first so its final usage reaches the engine.
    r.asr_cancel.cancel();
    for mut t in r.asr_tasks {
        // Every network step of a channel is bounded; this only guards
        // against a provider that ignores those bounds.
        if timeout(Duration::from_secs(8), &mut t).await.is_err() {
            tracing::warn!("a recognition channel did not stop in time");
            t.abort();
        }
    }
    r.loop_cancel.cancel();
    r.loop_task.await.unwrap_or_default()
}

struct LoopCtx {
    events: broadcast::Sender<MeetCueEvent>,
    history: Arc<Mutex<Vec<TimedEvent>>>,
    input_tx: mpsc::UnboundedSender<Input>,
    detect_llm: Arc<dyn LlmProvider>,
    hints_llm: Arc<dyn LlmProvider>,
    detect_model: ModelRef,
    hints_model: ModelRef,
    first_token_timeout: Duration,
    detect_timeout: Duration,
    paused: Arc<AtomicBool>,
    levels: Arc<[AtomicU32; 2]>,
    asr_cancel: CancellationToken,
    loop_cancel: CancellationToken,
    active: Arc<AtomicBool>,
}

impl LoopCtx {
    fn emit(&self, start: i64, event: MeetCueEvent) {
        let keep = !matches!(
            event,
            MeetCueEvent::Levels { .. }
                | MeetCueEvent::Transcript {
                    is_final: false,
                    ..
                }
        );
        if keep {
            let mut h = self.history.lock().unwrap();
            if h.len() < HISTORY_LIMIT {
                h.push(TimedEvent {
                    t_ms: now_ms() - start,
                    event: event.clone(),
                });
            }
        }
        let _ = self.events.send(event);
    }
}

/// New models and settings for a running session.
struct Reconfig {
    detect_llm: Arc<dyn LlmProvider>,
    hints_llm: Arc<dyn LlmProvider>,
    engine: EngineConfig,
    overrides: Vec<PriceEntry>,
    budget: BudgetSettings,
}

async fn run_loop(
    mut engine: Engine,
    mut rx: mpsc::UnboundedReceiver<Input>,
    mut reconfig: mpsc::UnboundedReceiver<Reconfig>,
    mut ctx: LoopCtx,
) -> SessionSummary {
    let start = now_ms();
    let mut tasks: HashMap<ReqId, CancellationToken> = HashMap::new();
    let mut tick = interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut level_tick = interval(Duration::from_millis(66));
    level_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // `None` until the first emit, so the meters always get a first value.
    let mut last_levels: Option<[f32; 2]> = None;

    let mut end = false;
    let outputs = engine.start(now_ms());
    execute(&ctx, start, outputs, &mut tasks, &mut end);
    while !end {
        let outputs = tokio::select! {
            biased;
            msg = rx.recv() => match msg {
                Some(input) => engine.handle(input, now_ms()),
                None => break,
            },
            _ = ctx.loop_cancel.cancelled() => {
                // Process what is already queued (e.g. final usage) before stopping.
                let mut outs = Vec::new();
                while let Ok(input) = rx.try_recv() {
                    outs.extend(engine.handle(input, now_ms()));
                }
                execute(&ctx, start, outs, &mut tasks, &mut end);
                break;
            }
            Some(r) = reconfig.recv() => {
                // Clients and model ids change together, so every request
                // goes to the provider of the model it names. Calls already
                // running keep their own client and finish normally.
                let changed = ctx.detect_model != r.engine.detect || ctx.hints_model != r.engine.hints;
                ctx.detect_llm = r.detect_llm;
                ctx.hints_llm = r.hints_llm;
                ctx.detect_model = r.engine.detect.clone();
                ctx.hints_model = r.engine.hints.clone();
                if changed {
                    tracing::info!(
                        "models switched: detection {}/{}, hints {}/{}",
                        ctx.detect_model.provider,
                        ctx.detect_model.model,
                        ctx.hints_model.provider,
                        ctx.hints_model.model
                    );
                }
                engine.configure(r.engine, r.overrides, r.budget);
                continue;
            }
            _ = tick.tick() => engine.handle(Input::Tick, now_ms()),
            _ = level_tick.tick() => {
                let me = f32::from_bits(ctx.levels[0].swap((-100f32).to_bits(), Ordering::Relaxed));
                let remote = f32::from_bits(ctx.levels[1].swap((-100f32).to_bits(), Ordering::Relaxed));
                let changed = last_levels
                    .is_none_or(|[m, r]| (me - m).abs() >= 1.0 || (remote - r).abs() >= 1.0);
                if changed {
                    last_levels = Some([me, remote]);
                    ctx.emit(start, MeetCueEvent::Levels { me, remote });
                }
                continue;
            }
        };
        execute(&ctx, start, outputs, &mut tasks, &mut end);
    }
    for (_, t) in tasks.drain() {
        t.cancel();
    }
    // Not running any more before `Stopped` goes out, so a listener that
    // stops capture on it can tell it from a session started since.
    ctx.active.store(false, Ordering::SeqCst);
    if engine.state() != meetcue_core::SessionState::Stopped {
        let outputs = engine.stop(now_ms());
        execute(&ctx, start, outputs, &mut tasks, &mut end);
    }
    ctx.asr_cancel.cancel();
    SessionSummary {
        duration_ms: now_ms() - start,
        total_micros: engine.meter().session_total(),
        records: engine.meter().records().to_vec(),
    }
}

fn execute(
    ctx: &LoopCtx,
    start: i64,
    outputs: Vec<Output>,
    tasks: &mut HashMap<ReqId, CancellationToken>,
    end: &mut bool,
) {
    tasks.retain(|_, t| !t.is_cancelled());
    // The session ends with these outputs (budget reached): not running any
    // more before their `Stopped` goes out, as at the end of `run_loop`.
    if outputs.iter().any(|o| matches!(o, Output::EndSession)) {
        ctx.active.store(false, Ordering::SeqCst);
    }
    for o in outputs {
        match o {
            Output::Emit(ev) => ctx.emit(start, ev),
            Output::Detect { req, request } => {
                let token = CancellationToken::new();
                tasks.insert(req, token.clone());
                tokio::spawn(detect_task(DetectTask {
                    req,
                    llm: ctx.detect_llm.clone(),
                    model: ctx.detect_model.clone(),
                    request,
                    token,
                    input: ctx.input_tx.clone(),
                    timeout: ctx.detect_timeout,
                }));
            }
            Output::Hints { req, request } => {
                let token = CancellationToken::new();
                tasks.insert(req, token.clone());
                tokio::spawn(hints_task(HintsTask {
                    req,
                    llm: ctx.hints_llm.clone(),
                    model: ctx.hints_model.clone(),
                    request,
                    token,
                    input: ctx.input_tx.clone(),
                    first_token_timeout: ctx.first_token_timeout,
                }));
            }
            Output::Cancel { req } => {
                if let Some(t) = tasks.remove(&req) {
                    t.cancel();
                }
            }
            Output::SetCapturePaused(p) => ctx.paused.store(p, Ordering::SeqCst),
            Output::EndSession => {
                ctx.asr_cancel.cancel();
                *end = true;
            }
        }
    }
}

fn usage_input(model: &ModelRef, purpose: UsagePurpose, chunk: &LlmChunk) -> Option<Input> {
    match chunk {
        LlmChunk::Usage {
            input,
            output,
            cache_read,
            cache_write,
        } => Some(Input::Usage {
            provider: model.provider.clone(),
            model: model.model.clone(),
            purpose,
            usage: UsageKind::Llm {
                input: *input,
                output: *output,
                cache_read: *cache_read,
                cache_write: *cache_write,
            },
        }),
        LlmChunk::Text(_) => None,
    }
}

struct DetectTask {
    req: ReqId,
    llm: Arc<dyn LlmProvider>,
    model: ModelRef,
    request: LlmRequest,
    token: CancellationToken,
    input: mpsc::UnboundedSender<Input>,
    timeout: Duration,
}

async fn detect_task(t: DetectTask) {
    let purpose = t.request.purpose;
    let deadline = Instant::now() + t.timeout;
    let mut usage: Option<LlmChunk> = None;
    let report_usage = |usage: &Option<LlmChunk>| {
        if let Some(i) = usage
            .as_ref()
            .and_then(|u| usage_input(&t.model, purpose, u))
        {
            let _ = t.input.send(i);
        }
    };
    let opened = tokio::select! {
        _ = t.token.cancelled() => return,
        _ = sleep_until(deadline) => Err(MeetCueError::new(codes::LLM_TIMEOUT, "question detection timed out")),
        r = t.llm.stream(t.request.clone()) => r,
    };
    let mut stream = match opened {
        Ok(s) => s,
        Err(e) => {
            let _ = t.input.send(Input::DetectDone {
                req: t.req,
                reply: Err(e),
            });
            return;
        }
    };
    let mut text = String::new();
    let reply = loop {
        tokio::select! {
            _ = t.token.cancelled() => {
                // Dropping the stream closes the connection; record what was billed.
                report_usage(&usage);
                return;
            }
            _ = sleep_until(deadline) => {
                break Err(MeetCueError::new(codes::LLM_TIMEOUT, "question detection timed out"));
            }
            item = stream.next() => match item {
                Some(Ok(LlmChunk::Text(s))) => text.push_str(&s),
                Some(Ok(u @ LlmChunk::Usage { .. })) => usage = Some(u),
                Some(Err(e)) => break Err(e),
                None => break Ok(text),
            }
        }
    };
    drop(stream);
    report_usage(&usage);
    let _ = t.input.send(Input::DetectDone { req: t.req, reply });
}

struct HintsTask {
    req: ReqId,
    llm: Arc<dyn LlmProvider>,
    model: ModelRef,
    request: LlmRequest,
    token: CancellationToken,
    input: mpsc::UnboundedSender<Input>,
    first_token_timeout: Duration,
}

/// Streams hints; if no text arrives within the first-token timeout the
/// request is cancelled and retried once.
async fn hints_task(t: HintsTask) {
    let purpose = t.request.purpose;
    let send = |i: Input| {
        let _ = t.input.send(i);
    };
    let report_usage = |usage: &Option<LlmChunk>| {
        if let Some(i) = usage
            .as_ref()
            .and_then(|u| usage_input(&t.model, purpose, u))
        {
            let _ = t.input.send(i);
        }
    };
    let mut last_error = MeetCueError::new(codes::LLM_TIMEOUT, "no hint text before the timeout");
    for attempt in 0..2 {
        let deadline = Instant::now() + t.first_token_timeout;
        let opened = tokio::select! {
            _ = t.token.cancelled() => return,
            _ = sleep_until(deadline) => Err(MeetCueError::new(codes::LLM_TIMEOUT, "hint request timed out")),
            r = t.llm.stream(t.request.clone()) => r,
        };
        let mut stream = match opened {
            Ok(s) => s,
            Err(e) => {
                let retry = attempt == 0 && e.is_transient();
                last_error = e;
                if retry {
                    continue;
                }
                break;
            }
        };
        let mut parser = HintStreamParser::default();
        let mut got_text = false;
        let mut usage: Option<LlmChunk> = None;
        loop {
            tokio::select! {
                _ = t.token.cancelled() => {
                    report_usage(&usage);
                    return;
                }
                _ = sleep_until(deadline), if !got_text => {
                    report_usage(&usage);
                    last_error = MeetCueError::new(codes::LLM_TIMEOUT, "no hint text before the timeout");
                    break;
                }
                item = stream.next() => match item {
                    Some(Ok(LlmChunk::Text(s))) => {
                        got_text = true;
                        for item in parser.push(&s) {
                            send(Input::Hint { req: t.req, item });
                        }
                    }
                    Some(Ok(u @ LlmChunk::Usage { .. })) => usage = Some(u),
                    Some(Err(e)) => {
                        report_usage(&usage);
                        if got_text {
                            for item in parser.finish() {
                                send(Input::Hint { req: t.req, item });
                            }
                            send(Input::HintsFailed { req: t.req, error: e });
                            return;
                        }
                        last_error = e;
                        break;
                    }
                    None => {
                        for item in parser.finish() {
                            send(Input::Hint { req: t.req, item });
                        }
                        report_usage(&usage);
                        if parser.points() == 0 {
                            send(Input::HintsFailed {
                                req: t.req,
                                error: MeetCueError::new(codes::LLM_BAD_OUTPUT, "reply contained no hint points"),
                            });
                        } else {
                            send(Input::HintsDone { req: t.req });
                        }
                        return;
                    }
                }
            }
        }
        if !last_error.is_transient() {
            break;
        }
    }
    send(Input::HintsFailed {
        req: t.req,
        error: last_error,
    });
}

/// The recognition provider and options; replaced live when settings change.
#[derive(Clone)]
struct AsrSetup {
    provider: Arc<dyn AsrProvider>,
    options: AsrOptions,
    model: ModelRef,
    hangover_ms: i64,
}

struct AsrChannelCtx {
    channel: Channel,
    setup: watch::Receiver<AsrSetup>,
    frames: mpsc::Receiver<AudioFrame>,
    input: mpsc::UnboundedSender<Input>,
    paused: Arc<AtomicBool>,
    cancel: CancellationToken,
}

struct OpenStream {
    stream: Box<dyn AsrStream>,
    map: Arc<Mutex<TimeMap>>,
    reader: JoinHandle<()>,
}

/// Longest wait for a stream to open: connection, TLS, WebSocket handshake
/// and the provider's start message.
const OPEN_TIMEOUT: Duration = Duration::from_secs(25);
/// Longest wait for one write, keep-alive or finalize. A send that takes
/// longer means the connection stalled, and it is treated as lost.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest wait for a stream to close when it is replaced or the session ends.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
/// A keep-alive goes out when nothing was sent for this long, whether or not
/// frames arrive: system sound capture delivers none while nothing plays,
/// and providers end idle streams (Deepgram after 10 s, Soniox after 20 s).
const IDLE_KEEPALIVE: Duration = Duration::from_secs(3);
/// How often a channel checks its stream, independent of audio.
const HEALTH_EVERY: Duration = Duration::from_secs(1);
/// A stream that ends sooner than this after opening counts towards the
/// reconnect backoff.
const STABLE_AFTER: Duration = Duration::from_secs(10);

fn timed_out(what: &str) -> MeetCueError {
    MeetCueError::new(codes::ASR_CONNECT, format!("{what} timed out"))
}

/// Runs one send to the provider with [`SEND_TIMEOUT`].
async fn bounded(
    what: &str,
    send: impl std::future::Future<Output = Result<(), MeetCueError>>,
) -> Result<(), MeetCueError> {
    timeout(SEND_TIMEOUT, send)
        .await
        .unwrap_or_else(|_| Err(timed_out(what)))
}

async fn open_stream(
    channel: Channel,
    input: mpsc::UnboundedSender<Input>,
    setup: AsrSetup,
) -> Result<OpenStream, MeetCueError> {
    let mut options = setup.options.clone();
    options.channel = Some(channel);
    let mut last = None;
    for attempt in 0..2 {
        if attempt > 0 {
            sleep(Duration::from_millis(500)).await;
        }
        let opened = timeout(OPEN_TIMEOUT, setup.provider.open(options.clone()))
            .await
            .unwrap_or_else(|_| Err(timed_out("opening the recognition stream")));
        match opened {
            Ok(mut stream) => {
                let map = Arc::new(Mutex::new(TimeMap::new(now_ms())));
                let results = stream.results();
                let reader = tokio::spawn(read_results(results, channel, map.clone(), input));
                return Ok(OpenStream {
                    stream,
                    map,
                    reader,
                });
            }
            Err(e) if e.is_transient() => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| MeetCueError::new(codes::ASR_CONNECT, "could not open stream")))
}

async fn read_results(
    mut results: futures::stream::BoxStream<'static, AsrResult>,
    channel: Channel,
    map: Arc<Mutex<TimeMap>>,
    input: mpsc::UnboundedSender<Input>,
) {
    while let Some(r) = results.next().await {
        let mapped = {
            let m = map.lock().unwrap();
            match r {
                AsrResult::Partial { text, t0_ms, t1_ms } => AsrResult::Partial {
                    text,
                    t0_ms: m.map(t0_ms),
                    t1_ms: m.map(t1_ms),
                },
                AsrResult::Final { text, t0_ms, t1_ms } => AsrResult::Final {
                    text,
                    t0_ms: m.map(t0_ms),
                    t1_ms: m.map(t1_ms),
                },
                AsrResult::Endpoint { t_ms } => AsrResult::Endpoint { t_ms: m.map(t_ms) },
                e @ AsrResult::Error(_) => e,
            }
        };
        if input
            .send(Input::Asr {
                channel,
                result: mapped,
            })
            .is_err()
        {
            break;
        }
    }
}

fn audio_usage(model: &ModelRef, ms: u64) -> Input {
    Input::Usage {
        provider: model.provider.clone(),
        model: model.model.clone(),
        purpose: UsagePurpose::Asr,
        usage: UsageKind::Audio { ms },
    }
}

/// Audio kept from before speech starts and sent when the gate opens, so
/// quiet word onsets below the voice threshold reach the recognizer.
const PREROLL_MS: i64 = 300;

/// Waits between attempts to reopen a lost recognition stream.
const RECONNECT_BACKOFF_MS: [u64; 5] = [1_000, 2_000, 5_000, 10_000, 30_000];

/// A lost stream being reopened.
struct Outage {
    /// Failed attempts so far.
    attempts: usize,
    /// When the next attempt is due.
    next: Instant,
    /// The failure was already shown; later attempts are only logged.
    reported: bool,
}

impl Outage {
    fn now() -> Self {
        Self {
            attempts: 0,
            next: Instant::now(),
            reported: false,
        }
    }

    /// The outage after one more failed attempt.
    fn failed(self) -> Self {
        let wait = RECONNECT_BACKOFF_MS[self.attempts.min(RECONNECT_BACKOFF_MS.len() - 1)];
        Self {
            attempts: self.attempts + 1,
            next: Instant::now() + Duration::from_millis(wait),
            reported: true,
        }
    }
}

/// Closes a replaced or lost stream in the background, so the channel goes
/// on at once while the stream's last results still reach the engine.
/// `flush` asks the provider to finalize pending text first.
fn retire(open: OpenStream, flush: bool) {
    tokio::spawn(async move {
        let OpenStream {
            mut stream, reader, ..
        } = open;
        if flush {
            let _ = bounded("finalize", stream.finalize()).await;
        }
        let _ = timeout(CLOSE_TIMEOUT, stream.close()).await;
        let _ = timeout(Duration::from_millis(500), reader).await;
    });
}

type Opening = JoinHandle<Result<OpenStream, MeetCueError>>;

/// Resolves when the stream being opened is ready; never without one.
async fn opened(
    opening: &mut Option<Opening>,
) -> Result<Result<OpenStream, MeetCueError>, tokio::task::JoinError> {
    match opening {
        Some(task) => task.await,
        None => std::future::pending().await,
    }
}

/// The connection of one channel. Opening runs in the background, so audio,
/// settings changes and stopping are handled while a slow or unreachable
/// provider is retried.
struct Link {
    channel: Channel,
    input: mpsc::UnboundedSender<Input>,
    current: Option<OpenStream>,
    opening: Option<Opening>,
    outage: Option<Outage>,
    /// Last successful send of audio, keep-alive or finalize.
    last_sent: Instant,
    /// Last status told to the engine.
    connected: Option<bool>,
    /// Nothing was written to the current stream yet: the audio kept while
    /// it opened leads the first write.
    fresh: bool,
    /// When the current stream opened.
    opened_at: Instant,
    /// Streams in a row that ended soon after opening.
    short_lived: usize,
}

impl Link {
    fn new(channel: Channel, input: mpsc::UnboundedSender<Input>) -> Self {
        Self {
            channel,
            input,
            current: None,
            opening: None,
            outage: None,
            last_sent: Instant::now(),
            connected: None,
            fresh: false,
            opened_at: Instant::now(),
            short_lived: 0,
        }
    }

    fn report(&self, e: MeetCueError) {
        let _ = self.input.send(Input::Asr {
            channel: self.channel,
            result: AsrResult::Error(e),
        });
    }

    fn set_connected(&mut self, connected: bool) {
        if self.connected != Some(connected) {
            self.connected = Some(connected);
            let _ = self.input.send(Input::AsrStatus {
                channel: self.channel,
                connected,
            });
        }
    }

    /// Starts opening a stream, replacing an attempt in progress.
    fn open(&mut self, setup: &AsrSetup) {
        if let Some(task) = self.opening.take() {
            task.abort();
        }
        self.opening = Some(tokio::spawn(open_stream(
            self.channel,
            self.input.clone(),
            setup.clone(),
        )));
    }

    /// Switches to a new setup: the old stream is flushed and closed in the
    /// background while the new one opens.
    fn switch(&mut self, setup: &AsrSetup) {
        if let Some(old) = self.current.take() {
            retire(old, true);
        }
        // New settings may be what fixes a rejected stream: start afresh.
        self.outage = None;
        self.short_lived = 0;
        self.open(setup);
    }

    fn on_opened(
        &mut self,
        result: Result<Result<OpenStream, MeetCueError>, tokio::task::JoinError>,
    ) {
        self.opening = None;
        let result = match result {
            Ok(r) => r,
            // Replaced by a newer attempt.
            Err(e) if e.is_cancelled() => return,
            Err(e) => Err(MeetCueError::new(codes::ASR_CONNECT, e.to_string())),
        };
        let ch = self.channel.as_str();
        match result {
            Ok(open) => {
                if self.outage.take().is_some_and(|o| o.attempts > 0) {
                    tracing::info!(channel = ch, "recognition reconnected");
                }
                self.current = Some(open);
                self.last_sent = Instant::now();
                self.opened_at = Instant::now();
                self.fresh = true;
                self.set_connected(true);
            }
            Err(e) => {
                let outage = self.outage.take().unwrap_or_else(Outage::now);
                // One message per outage, not one per attempt.
                if outage.reported {
                    tracing::warn!(
                        channel = ch,
                        "reconnect attempt {} failed: {e}",
                        outage.attempts + 1
                    );
                } else {
                    tracing::warn!(channel = ch, "could not open recognition: {e}");
                    self.report(e);
                }
                self.outage = Some(outage.failed());
                self.set_connected(false);
            }
        }
    }

    /// The stream failed or the provider ended it: close it and reopen.
    /// A stream that ran for a while is reopened at once. When streams keep
    /// ending soon after opening (the provider rejects a setting, or every
    /// connection is cut), the wait grows, so the provider isn't hammered
    /// and its error isn't repeated many times a second.
    fn lost(&mut self, setup: &AsrSetup) {
        if let Some(old) = self.current.take() {
            retire(old, false);
        }
        self.set_connected(false);
        if self.opened_at.elapsed() >= STABLE_AFTER {
            self.short_lived = 0;
        } else {
            self.short_lived += 1;
        }
        if self.short_lived <= 1 {
            self.outage = Some(Outage::now());
            self.open(setup);
        } else {
            let wait =
                RECONNECT_BACKOFF_MS[(self.short_lived - 2).min(RECONNECT_BACKOFF_MS.len() - 1)];
            tracing::warn!(
                channel = self.channel.as_str(),
                "recognition stream ended right after opening again; retrying in {} s",
                wait / 1000
            );
            self.outage = Some(Outage {
                attempts: self.short_lived - 1,
                next: Instant::now() + Duration::from_millis(wait),
                reported: false,
            });
        }
    }

    /// Records the outcome of a send; a failed send reconnects.
    fn sent(&mut self, result: Result<(), MeetCueError>, setup: &AsrSetup) {
        match result {
            Ok(()) => self.last_sent = Instant::now(),
            Err(e) => {
                tracing::warn!(channel = self.channel.as_str(), error = %e, "recognition stream failed; reconnecting");
                self.lost(setup);
            }
        }
    }

    /// Reopens the stream if the provider ended it (idle close, session
    /// limit, error). True if it did.
    fn reopen_if_ended(&mut self, setup: &AsrSetup) -> bool {
        if !self
            .current
            .as_ref()
            .is_some_and(|o| o.reader.is_finished())
        {
            return false;
        }
        tracing::warn!(
            channel = self.channel.as_str(),
            "recognition stream ended by the provider; reconnecting"
        );
        self.lost(setup);
        true
    }

    /// Runs every [`HEALTH_EVERY`]: notices a stream the provider ended,
    /// keeps an idle stream alive and retries a lost one when due.
    async fn check(&mut self, setup: &AsrSetup) {
        if self.reopen_if_ended(setup) {
            return;
        }
        if let Some(open) = self.current.as_mut() {
            if self.last_sent.elapsed() >= IDLE_KEEPALIVE {
                let r = bounded("keep-alive", open.stream.keepalive()).await;
                self.sent(r, setup);
            }
            return;
        }
        let due = self
            .outage
            .as_ref()
            .is_some_and(|o| Instant::now() >= o.next);
        if self.opening.is_none() && due {
            self.open(setup);
        }
    }

    /// Ends the connection; returns the provider's count of audio sent.
    async fn close(mut self) -> Option<u64> {
        if let Some(task) = self.opening.take() {
            task.abort();
        }
        let open = self.current.take()?;
        let closed = timeout(CLOSE_TIMEOUT, open.stream.close()).await;
        let _ = timeout(Duration::from_millis(500), open.reader).await;
        match closed {
            Ok(Ok(u)) => Some(u.audio_ms),
            _ => None,
        }
    }
}

/// One recognition channel: gates frames and writes them; keeps the
/// connection alive while paused or silent; reconnects whenever the stream
/// is lost (network, provider session limits) and when the recognition
/// settings change during the session.
async fn asr_channel(mut ctx: AsrChannelCtx) {
    let mut setup = ctx.setup.borrow_and_update().clone();
    let mut link = Link::new(ctx.channel, ctx.input.clone());
    link.open(&setup);
    let mut gate = SendGate::new(setup.hangover_ms);
    let mut preroll: VecDeque<AudioFrame> = VecDeque::new();
    let mut billed_ms: u64 = 0;
    let mut sent_total: u64 = 0;
    let mut usage_tick = interval(Duration::from_secs(60));
    usage_tick.tick().await;
    let mut health = interval(HEALTH_EVERY);
    health.set_missed_tick_behavior(MissedTickBehavior::Delay);
    health.tick().await;

    loop {
        tokio::select! {
            _ = ctx.cancel.cancelled() => break,
            r = opened(&mut link.opening) => link.on_opened(r),
            Ok(()) = ctx.setup.changed() => {
                let next = ctx.setup.borrow_and_update().clone();
                // Bill what the old model recognised before switching.
                if sent_total > billed_ms {
                    let _ = ctx.input.send(audio_usage(&setup.model, sent_total - billed_ms));
                    billed_ms = sent_total;
                }
                if next.hangover_ms != setup.hangover_ms {
                    gate = SendGate::new(next.hangover_ms);
                }
                setup = next;
                tracing::info!(
                    channel = ctx.channel.as_str(),
                    "recognition switched to {}/{}",
                    setup.model.provider,
                    setup.model.model
                );
                link.switch(&setup);
            }
            _ = health.tick() => link.check(&setup).await,
            _ = usage_tick.tick() => {
                if sent_total > billed_ms {
                    let _ = ctx.input.send(audio_usage(&setup.model, sent_total - billed_ms));
                    billed_ms = sent_total;
                }
            }
            frame = ctx.frames.recv() => {
                let Some(frame) = frame else { break };
                let dur = frame.duration_ms();
                let paused = ctx.paused.load(Ordering::Relaxed);
                link.reopen_if_ended(&setup);
                let decision = if paused {
                    // Audio from before the pause must not lead the next sentence.
                    preroll.clear();
                    gate.paused(dur)
                } else {
                    gate.decide(frame.voiced, dur)
                };
                let Some(open) = link.current.as_mut() else {
                    if !paused {
                        push_preroll(&mut preroll, frame);
                    }
                    continue;
                };
                let result = match decision {
                    GateDecision::Drop | GateDecision::KeepAlive => {
                        if !paused {
                            push_preroll(&mut preroll, frame);
                        }
                        if decision == GateDecision::KeepAlive {
                            Some(bounded("keep-alive", open.stream.keepalive()).await)
                        } else {
                            None
                        }
                    }
                    d @ (GateDecision::Send | GateDecision::SendAndFinalize) => {
                        let lead: Vec<AudioFrame> = if gate.opened() || link.fresh {
                            preroll.drain(..).collect()
                        } else {
                            preroll.clear();
                            Vec::new()
                        };
                        let mut r = Ok(());
                        for f in lead.iter().chain(std::iter::once(&frame)) {
                            let fd = f.duration_ms();
                            open.map.lock().unwrap().record(f.capture_ts_ms, fd);
                            sent_total += fd as u64;
                            r = bounded("sending audio", open.stream.write(&f.samples, f.capture_ts_ms)).await;
                            if r.is_err() {
                                break;
                            }
                        }
                        if r.is_ok() && d == GateDecision::SendAndFinalize {
                            r = bounded("finalize", open.stream.finalize()).await;
                        }
                        Some(r)
                    }
                };
                if let Some(r) = result {
                    if r.is_ok() && matches!(decision, GateDecision::Send | GateDecision::SendAndFinalize) {
                        link.fresh = false;
                    }
                    link.sent(r, &setup);
                }
            }
        }
    }
    if let Some(ms) = link.close().await {
        sent_total = sent_total.max(ms);
    }
    if sent_total > billed_ms {
        let _ = ctx
            .input
            .send(audio_usage(&setup.model, sent_total - billed_ms));
    }
    ctx.frames.close();
}

/// Keeps the last [`PREROLL_MS`] of frames that were not sent.
fn push_preroll(preroll: &mut VecDeque<AudioFrame>, frame: AudioFrame) {
    preroll.push_back(frame);
    let mut kept: i64 = preroll.iter().map(AudioFrame::duration_ms).sum();
    while kept > PREROLL_MS {
        match preroll.pop_front() {
            Some(f) => kept -= f.duration_ms(),
            None => break,
        }
    }
}
