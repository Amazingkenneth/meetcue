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
    for t in r.asr_tasks {
        let _ = t.await;
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
    let mut last_levels = [f32::NAN; 2];

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
                if (me - last_levels[0]).abs() >= 1.0 || (remote - last_levels[1]).abs() >= 1.0 {
                    last_levels = [me, remote];
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
    if engine.state() != meetcue_core::SessionState::Stopped {
        let outputs = engine.stop(now_ms());
        execute(&ctx, start, outputs, &mut tasks, &mut end);
    }
    ctx.asr_cancel.cancel();
    ctx.active.store(false, Ordering::SeqCst);
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

async fn open_stream(ctx: &AsrChannelCtx, setup: &AsrSetup) -> Result<OpenStream, MeetCueError> {
    let mut options = setup.options.clone();
    options.channel = Some(ctx.channel);
    let mut last = None;
    for attempt in 0..2 {
        if attempt > 0 {
            sleep(Duration::from_millis(500)).await;
        }
        match setup.provider.open(options.clone()).await {
            Ok(mut stream) => {
                let map = Arc::new(Mutex::new(TimeMap::new(now_ms())));
                let results = stream.results();
                let reader = tokio::spawn(read_results(
                    results,
                    ctx.channel,
                    map.clone(),
                    ctx.input.clone(),
                ));
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

/// When the next attempt to reopen a lost stream is due.
struct Outage {
    attempts: usize,
    next: Instant,
}

impl Outage {
    fn after(attempts: usize) -> Self {
        let wait = RECONNECT_BACKOFF_MS[attempts.min(RECONNECT_BACKOFF_MS.len() - 1)];
        Self {
            attempts,
            next: Instant::now() + Duration::from_millis(wait),
        }
    }
}

async fn close_stream(open: OpenStream) {
    let _ = open.stream.close().await;
    // Give the reader a moment to deliver the last results.
    let _ = timeout(Duration::from_millis(500), open.reader).await;
}

/// One recognition channel: gates frames and writes them; keeps the
/// connection alive while paused; reconnects whenever the stream is lost
/// (network, provider session limits) and when the recognition settings
/// change during the session.
async fn asr_channel(mut ctx: AsrChannelCtx) {
    let report = |ctx: &AsrChannelCtx, e: MeetCueError| {
        let _ = ctx.input.send(Input::Asr {
            channel: ctx.channel,
            result: AsrResult::Error(e),
        });
    };
    let mut setup = ctx.setup.borrow_and_update().clone();
    let mut current = None;
    let mut outage = None;
    match open_stream(&ctx, &setup).await {
        Ok(s) => current = Some(s),
        Err(e) => {
            report(&ctx, e);
            outage = Some(Outage::after(0));
        }
    }
    let mut gate = SendGate::new(setup.hangover_ms);
    let mut preroll: VecDeque<AudioFrame> = VecDeque::new();
    let mut billed_ms: u64 = 0;
    let mut sent_total: u64 = 0;
    let mut usage_tick = interval(Duration::from_secs(60));
    usage_tick.tick().await;

    loop {
        tokio::select! {
            _ = ctx.cancel.cancelled() => break,
            Ok(()) = ctx.setup.changed() => {
                let next = ctx.setup.borrow_and_update().clone();
                // Bill what the old model recognised before switching.
                if sent_total > billed_ms {
                    let _ = ctx.input.send(audio_usage(&setup.model, sent_total - billed_ms));
                    billed_ms = sent_total;
                }
                if let Some(mut old) = current.take() {
                    // Flush and close the old stream in the background: its
                    // last words still reach the engine while the new stream
                    // opens right away.
                    tokio::spawn(async move {
                        let _ = old.stream.finalize().await;
                        close_stream(old).await;
                    });
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
                match open_stream(&ctx, &setup).await {
                    Ok(s) => {
                        current = Some(s);
                        outage = None;
                    }
                    Err(e) => {
                        report(&ctx, e);
                        outage = Some(Outage::after(0));
                    }
                }
            }
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

                // The provider ended the stream (idle close, session limit,
                // error): reopen it rather than waiting for a write to fail.
                if current.as_ref().is_some_and(|o| o.reader.is_finished()) {
                    tracing::warn!(channel = ctx.channel.as_str(), "recognition stream closed by the provider; reconnecting");
                    if let Some(old) = current.take() {
                        close_stream(old).await;
                    }
                    outage = Some(Outage { attempts: 0, next: Instant::now() });
                }
                if current.is_none() {
                    if let Some(o) = outage.as_ref().filter(|o| Instant::now() >= o.next) {
                        let attempts = o.attempts + 1;
                        match open_stream(&ctx, &setup).await {
                            Ok(s) => {
                                tracing::info!(channel = ctx.channel.as_str(), "recognition reconnected");
                                current = Some(s);
                                outage = None;
                            }
                            Err(e) => {
                                // One message per outage, not one per attempt.
                                if attempts == 1 {
                                    report(&ctx, e);
                                } else {
                                    tracing::warn!(channel = ctx.channel.as_str(), "reconnect attempt {attempts} failed: {e}");
                                }
                                outage = Some(Outage::after(attempts));
                            }
                        }
                    }
                }

                let decision = if paused {
                    // Audio from before the pause must not lead the next sentence.
                    preroll.clear();
                    gate.paused(dur)
                } else {
                    gate.decide(frame.voiced, dur)
                };
                let Some(open) = current.as_mut() else {
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
                            open.stream.keepalive().await
                        } else {
                            Ok(())
                        }
                    }
                    d @ (GateDecision::Send | GateDecision::SendAndFinalize) => {
                        let lead: Vec<AudioFrame> = if gate.opened() {
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
                            r = open.stream.write(&f.samples, f.capture_ts_ms).await;
                            if r.is_err() {
                                break;
                            }
                        }
                        if r.is_ok() && d == GateDecision::SendAndFinalize {
                            open.stream.finalize().await
                        } else {
                            r
                        }
                    }
                };
                if let Err(e) = result {
                    tracing::warn!(channel = ctx.channel.as_str(), error = %e, "recognition stream failed; reconnecting");
                    if let Some(old) = current.take() {
                        old.reader.abort();
                        let _ = old.stream.close().await;
                    }
                    match open_stream(&ctx, &setup).await {
                        Ok(s) => {
                            tracing::info!(channel = ctx.channel.as_str(), "recognition reconnected");
                            current = Some(s);
                        }
                        Err(e) => {
                            report(&ctx, e);
                            outage = Some(Outage::after(1));
                        }
                    }
                }
            }
        }
    }
    if let Some(open) = current.take() {
        let closed = open.stream.close().await;
        let _ = timeout(Duration::from_millis(500), open.reader).await;
        if let Ok(u) = closed {
            sent_total = sent_total.max(u.audio_ms);
        }
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
