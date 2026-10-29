//! The session state machine.
//!
//! `Engine` is synchronous and deterministic: it receives [`Input`]s with the
//! current time and returns [`Output`]s (events to emit, model requests to
//! start or cancel). The async driver in [`crate::backend`] performs the I/O.
//! This keeps every rule of the pipeline unit-testable without a network.

use std::collections::{HashMap, VecDeque};

use meetcue_core::settings::{BudgetSettings, HintLanguage};
use meetcue_core::{
    codes, Channel, MeetCueError, MeetCueEvent, PointStatus, PriceEntry, QuestionBrief, QuestionId,
    QuestionStatus, SessionState, TranscriptLine, UsageKind, UsagePurpose, UserAction,
};

use crate::coverage::{self, AnswerText, PointVocab};
use crate::detect::{self, Detection};
use crate::hints::HintItem;
use crate::latency::LatencyMonitor;
use crate::prefilter;
use crate::prompts;
use crate::provider::{AsrResult, LlmRequest};
use crate::queue::{Admission, QuestionQueue};
use crate::text::{dominant_lang, edit_ratio, extends, join_parts, Lang};
use crate::transcript::{format_for_prompt, TranscriptBuffer};
use crate::usage::{BudgetOutcome, CostMeter};

pub type ReqId = u64;

/// Quiet time on a partial result before an early trigger fires.
pub const EARLY_QUIET_MS: i64 = 400;
/// Edit distance above which an early trigger is cancelled and redone.
pub const EARLY_MAX_CHANGE: f64 = 0.30;
/// A remote sentence starting this soon after a question's last words may be
/// the rest of it, cut off by a pause ("…idea A" / "and idea B").
pub const CONTINUATION_MS: i64 = 5_000;
/// Silence after which a fully covered question counts as answered.
pub const ANSWERED_SILENCE_MS: i64 = 4_000;
/// The user speaking this recently means a new question is an interruption.
pub const INTERRUPTION_WINDOW_MS: i64 = 1_500;
/// Maximum hint generations running at once.
pub const MAX_CONCURRENT_HINTS: usize = 2;
/// Detection sees this much transcript.
pub const DETECT_WINDOW_MS: i64 = 60_000;
/// Hint generation sees this much transcript.
pub const HINTS_WINDOW_MS: i64 = 180_000;
/// Echo filter tolerance between the two channels.
pub const ECHO_WINDOW_MS: i64 = 2_000;
/// A sentence with final text but no endpoint is closed after this long.
pub const STALE_UTTERANCE_MS: i64 = 2_000;
/// Minimum spacing of partial transcript events per channel.
const PARTIAL_EMIT_MS: i64 = 100;
const MAX_QUESTIONS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub detect: ModelRef,
    pub hints: ModelRef,
    pub role: String,
    /// Condensed briefing material placed in the cached system prompt.
    pub materials: String,
    pub hint_language: HintLanguage,
    pub only_directed_to_me: bool,
    pub queue_interruptions: bool,
    pub track_coverage: bool,
    pub early_trigger: bool,
    /// Start hint generation together with detection instead of after it,
    /// and keep the result only if detection confirms a question. Saves a
    /// whole model round trip per question; sentences that pass the
    /// pre-filter but aren't questions cost one unused hint call.
    pub speculative_hints: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            detect: ModelRef::default(),
            hints: ModelRef::default(),
            role: String::new(),
            materials: String::new(),
            hint_language: HintLanguage::FollowQuestion,
            only_directed_to_me: false,
            queue_interruptions: true,
            track_coverage: true,
            early_trigger: true,
            speculative_hints: false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Input {
    /// A recognition result with times already mapped to the capture clock.
    Asr {
        channel: Channel,
        result: AsrResult,
    },
    /// A recognition stream of a channel opened, or was lost.
    AsrStatus {
        channel: Channel,
        connected: bool,
    },
    /// Remote text injected by practice mode. `known_question` skips detection.
    RemoteText {
        text: String,
        t_ms: i64,
        known_question: bool,
    },
    DetectDone {
        req: ReqId,
        reply: Result<String, MeetCueError>,
    },
    Hint {
        req: ReqId,
        item: HintItem,
    },
    HintsDone {
        req: ReqId,
    },
    HintsFailed {
        req: ReqId,
        error: MeetCueError,
    },
    Usage {
        provider: String,
        model: String,
        purpose: UsagePurpose,
        usage: UsageKind,
    },
    Action(UserAction),
    Tick,
}

#[derive(Debug, Clone)]
pub enum Output {
    Emit(MeetCueEvent),
    Detect { req: ReqId, request: LlmRequest },
    Hints { req: ReqId, request: LlmRequest },
    Cancel { req: ReqId },
    SetCapturePaused(bool),
    EndSession,
}

#[derive(Debug, Default)]
struct Utterance {
    id: u32,
    finals: String,
    partial: String,
    t0: Option<i64>,
    t1: i64,
    last_change: i64,
    last_emit: i64,
}

impl Utterance {
    fn text(&self) -> String {
        let mut s = self.finals.clone();
        s.push_str(&self.partial);
        s.trim().to_string()
    }

    fn is_empty(&self) -> bool {
        self.finals.trim().is_empty() && self.partial.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    New,
    Redo(QuestionId),
}

#[derive(Debug, Clone)]
struct Candidate {
    text: String,
    speech_end: i64,
    heard_at: i64,
    started_at: i64,
    early: bool,
    target: Target,
    /// More of the question arrived after detection started; detection only
    /// judged the first part.
    joined: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EarlyState {
    Detecting(ReqId),
    Created(QuestionId),
    Dropped,
}

#[derive(Debug, Clone)]
struct EarlyLink {
    utterance: u32,
    snapshot: String,
    state: EarlyState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HintsState {
    Idle,
    Waiting,
    Running(ReqId),
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, Default)]
struct Latency {
    speech_end: i64,
    asr_ms: i64,
    detect_ms: i64,
    hint_start: i64,
    reported: bool,
    early: bool,
}

#[derive(Debug)]
struct Question {
    id: QuestionId,
    version: u32,
    text: String,
    short: String,
    tags: Vec<String>,
    subs: Vec<String>,
    t_ms: i64,
    /// When the latest part of the question ended.
    last_part_end: i64,
    context: Vec<TranscriptLine>,
    vocab: Vec<PointVocab>,
    statuses: Vec<PointStatus>,
    hints: HintsState,
    answer: AnswerText,
    /// Points ticked (true) or unticked (false) by hand, by index.
    manual: HashMap<usize, bool>,
    user_spoke: bool,
    latency: Latency,
}

#[derive(Debug, Clone, Copy)]
enum Req {
    Detect,
    Hints {
        id: QuestionId,
        version: u32,
    },
    /// Hints started alongside the detection request `detect`.
    Speculative {
        detect: ReqId,
    },
}

/// Hint generation started before detection confirmed the question. What it
/// produces is held until detection decides whether to keep it.
#[derive(Debug)]
struct Speculation {
    req: ReqId,
    started_at: i64,
    items: Vec<HintItem>,
    /// Set once the request has ended: `true` if it completed.
    finished: Option<bool>,
}

pub struct Engine {
    cfg: EngineConfig,
    state: SessionState,
    transcript: TranscriptBuffer,
    utterances: [Utterance; 2],
    next_utterance: u32,
    last_me_activity: i64,
    early: Option<EarlyLink>,
    candidates: HashMap<ReqId, Candidate>,
    /// Speculative hint requests, by the detection request they belong to.
    speculative: HashMap<ReqId, Speculation>,
    reqs: HashMap<ReqId, Req>,
    next_req: ReqId,
    questions: Vec<Question>,
    next_question: QuestionId,
    queue: QuestionQueue,
    hints_waiting: VecDeque<QuestionId>,
    hints_running: usize,
    hints_paused: bool,
    meter: CostMeter,
    latency: LatencyMonitor,
}

fn idx(ch: Channel) -> usize {
    match ch {
        Channel::Me => 0,
        Channel::Remote => 1,
    }
}

impl Engine {
    pub fn new(cfg: EngineConfig, meter: CostMeter) -> Self {
        Self {
            cfg,
            state: SessionState::Idle,
            transcript: TranscriptBuffer::default(),
            utterances: Default::default(),
            next_utterance: 1,
            last_me_activity: i64::MIN / 2,
            early: None,
            candidates: HashMap::new(),
            speculative: HashMap::new(),
            reqs: HashMap::new(),
            next_req: 1,
            questions: Vec::new(),
            next_question: 1,
            queue: QuestionQueue::default(),
            hints_waiting: VecDeque::new(),
            hints_running: 0,
            hints_paused: false,
            meter,
            latency: LatencyMonitor::default(),
        }
    }

    pub fn meter(&self) -> &CostMeter {
        &self.meter
    }

    /// New settings during a session: later requests use the new models,
    /// role, materials and options; questions and hints so far are kept.
    pub fn configure(
        &mut self,
        cfg: EngineConfig,
        overrides: Vec<PriceEntry>,
        budget: BudgetSettings,
    ) {
        self.cfg = cfg;
        self.meter.set_rules(overrides, budget);
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn start(&mut self, now: i64) -> Vec<Output> {
        self.state = SessionState::Running;
        for u in &mut self.utterances {
            u.id = self.next_utterance;
            self.next_utterance += 1;
        }
        vec![Output::Emit(MeetCueEvent::SessionState {
            state: self.state,
            t_ms: now,
        })]
    }

    pub fn stop(&mut self, now: i64) -> Vec<Output> {
        let mut out = Vec::new();
        for (req, _) in self.reqs.drain() {
            out.push(Output::Cancel { req });
        }
        self.candidates.clear();
        self.speculative.clear();
        self.state = SessionState::Stopped;
        out.push(Output::Emit(MeetCueEvent::SessionState {
            state: self.state,
            t_ms: now,
        }));
        out
    }

    pub fn handle(&mut self, input: Input, now: i64) -> Vec<Output> {
        let mut out = Vec::new();
        match input {
            Input::Asr { channel, result } => self.on_asr(channel, result, now, &mut out),
            Input::AsrStatus { channel, connected } => {
                out.push(Output::Emit(MeetCueEvent::Recognition {
                    channel,
                    connected,
                }))
            }
            Input::RemoteText {
                text,
                t_ms,
                known_question,
            } => self.on_remote_text(text, t_ms, known_question, now, &mut out),
            Input::DetectDone { req, reply } => self.on_detect_done(req, reply, now, &mut out),
            Input::Hint { req, item } => self.on_hint(req, item, now, &mut out),
            Input::HintsDone { req } => self.on_hints_done(req, now, &mut out),
            Input::HintsFailed { req, error } => self.on_hints_failed(req, error, now, &mut out),
            Input::Usage {
                provider,
                model,
                purpose,
                usage,
            } => self.on_usage(&provider, &model, purpose, usage, now, &mut out),
            Input::Action(action) => self.on_action(action, now, &mut out),
            Input::Tick => self.on_tick(now, &mut out),
        }
        out
    }

    // ---- recognition ------------------------------------------------------

    fn on_asr(&mut self, ch: Channel, result: AsrResult, now: i64, out: &mut Vec<Output>) {
        let u = &mut self.utterances[idx(ch)];
        match result {
            AsrResult::Partial { text, t0_ms, t1_ms } => {
                if u.partial != text {
                    u.partial = text;
                    u.last_change = now;
                }
                u.t0.get_or_insert(t0_ms);
                u.t1 = u.t1.max(t1_ms);
                if ch == Channel::Me && !u.is_empty() {
                    self.last_me_activity = now;
                }
                self.emit_partial(ch, now, false, out);
                self.live_coverage(ch, out);
            }
            AsrResult::Final { text, t0_ms, t1_ms } => {
                u.finals.push_str(&text);
                u.partial.clear();
                u.last_change = now;
                u.t0.get_or_insert(t0_ms);
                u.t1 = u.t1.max(t1_ms);
                if ch == Channel::Me {
                    self.last_me_activity = now;
                }
                self.emit_partial(ch, now, true, out);
                self.live_coverage(ch, out);
            }
            AsrResult::Endpoint { t_ms } => {
                u.t1 = u.t1.max(t_ms);
                self.close_utterance(ch, now, out);
            }
            AsrResult::Error(e) => out.push(Output::Emit(MeetCueEvent::Error {
                code: e.code,
                detail: e.detail,
            })),
        }
    }

    fn live_coverage(&mut self, ch: Channel, out: &mut Vec<Output>) {
        if ch != Channel::Me || !self.cfg.track_coverage {
            return;
        }
        if let Some(id) = self.queue.active() {
            self.update_coverage(id, out);
        }
    }

    fn emit_partial(&mut self, ch: Channel, now: i64, force: bool, out: &mut Vec<Output>) {
        let u = &mut self.utterances[idx(ch)];
        if u.is_empty() || (!force && now - u.last_emit < PARTIAL_EMIT_MS) {
            return;
        }
        u.last_emit = now;
        out.push(Output::Emit(MeetCueEvent::Transcript {
            channel: ch,
            utterance: u.id,
            is_final: false,
            text: u.text(),
            t0_ms: u.t0.unwrap_or(u.t1),
            t1_ms: u.t1,
        }));
    }

    fn close_utterance(&mut self, ch: Channel, now: i64, out: &mut Vec<Output>) {
        let next_id = self.next_utterance;
        let u = &mut self.utterances[idx(ch)];
        if u.is_empty() {
            u.partial.clear();
            u.finals.clear();
            u.t0 = None;
            return;
        }
        let line = TranscriptLine {
            channel: ch,
            text: u.text(),
            t0_ms: u.t0.unwrap_or(u.t1),
            t1_ms: u.t1,
        };
        let utterance = u.id;
        *u = Utterance {
            id: next_id,
            ..Default::default()
        };
        self.next_utterance += 1;
        match ch {
            Channel::Remote => {
                self.transcript.push(line.clone());
                out.push(Output::Emit(transcript_final(&line, utterance)));
                self.on_remote_sentence(line, utterance, now, out);
            }
            Channel::Me => self.on_me_sentence(line, utterance, now, out),
        }
    }

    fn on_remote_text(
        &mut self,
        text: String,
        t_ms: i64,
        known_question: bool,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        let utterance = self.next_utterance;
        self.next_utterance += 1;
        let line = TranscriptLine {
            channel: Channel::Remote,
            text: text.trim().to_string(),
            t0_ms: t_ms,
            t1_ms: t_ms,
        };
        if line.text.is_empty() {
            return;
        }
        self.transcript.push(line.clone());
        out.push(Output::Emit(transcript_final(&line, utterance)));
        if known_question {
            let det = Detection {
                is_question: true,
                directed_to_me: true,
                short: String::new(),
                ..Default::default()
            };
            let cand = Candidate {
                text: line.text.clone(),
                speech_end: t_ms,
                heard_at: now,
                started_at: now,
                early: false,
                target: Target::New,
                joined: false,
            };
            self.accept_detection(cand, det, None, now, out);
        } else {
            self.on_remote_sentence(line, utterance, now, out);
        }
    }

    fn on_remote_sentence(
        &mut self,
        line: TranscriptLine,
        utterance: u32,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        if let Some(early) = self.early.take().filter(|e| e.utterance == utterance) {
            // The speaker went on after the pause that fired the early
            // trigger: answer the whole sentence, not just its start.
            if extends(&early.snapshot, &line.text) {
                match early.state {
                    EarlyState::Detecting(req) => {
                        self.join_candidate(req, line.text.clone(), line.t1_ms, now, out)
                    }
                    EarlyState::Created(id) => {
                        self.extend_question(id, line.text.clone(), line.t1_ms, now, out)
                    }
                    EarlyState::Dropped => {
                        if prefilter::might_be_question(&line.text) {
                            self.start_detect(&line, now, false, Target::New, out);
                        }
                    }
                }
                return;
            }
            let changed = edit_ratio(&early.snapshot, &line.text) > EARLY_MAX_CHANGE;
            match (early.state, changed) {
                (EarlyState::Detecting(req), false) => {
                    if let Some(c) = self.candidates.get_mut(&req) {
                        c.text = line.text.clone();
                    }
                    return;
                }
                (EarlyState::Created(id), false) => {
                    if let Some(q) = self.question_mut(id) {
                        q.text = line.text.clone();
                    }
                    if let Some(ev) = self.question_event(id) {
                        out.push(Output::Emit(ev));
                    }
                    return;
                }
                (EarlyState::Detecting(req), true) => {
                    self.cancel_req(req, now, out);
                }
                (EarlyState::Created(id), true) => {
                    // The sentence changed a lot: re-run detection for the same card.
                    self.cancel_hints(id, out);
                    self.start_detect(&line, now, false, Target::Redo(id), out);
                    return;
                }
                (EarlyState::Dropped, false) => return,
                (EarlyState::Dropped, true) => {}
            }
        }
        if let Some(join) = self.continuation_of(&line) {
            match join {
                Join::Candidate(req, prev) => {
                    let text = join_parts(&prev, &line.text);
                    self.join_candidate(req, text, line.t1_ms, now, out);
                }
                Join::Question(id, prev) => {
                    let text = join_parts(&prev, &line.text);
                    self.extend_question(id, text, line.t1_ms, now, out);
                }
            }
            return;
        }
        // Right after a question, let the model judge sentences the rules
        // would skip: "idea B too" may finish it.
        if prefilter::might_be_question(&line.text) || self.recent_question(&line).is_some() {
            self.start_detect(&line, now, false, Target::New, out);
        }
    }

    /// The open question, or the sentence still being checked, that `line`
    /// carries on, if it starts within [`CONTINUATION_MS`] and is joined to
    /// it by its wording.
    fn continuation_of(&self, line: &TranscriptLine) -> Option<Join> {
        let cand = self
            .candidates
            .iter()
            .filter(|(_, c)| c.target == Target::New && within_join(c.speech_end, line))
            .max_by_key(|(_, c)| c.speech_end)
            .map(|(req, c)| (c.speech_end, Join::Candidate(*req, c.text.clone())));
        let question = self
            .recent_question(line)
            .map(|q| (q.last_part_end, Join::Question(q.id, q.text.clone())));
        let (_, join) = match (cand, question) {
            (Some(c), Some(q)) => Some(if c.0 >= q.0 { c } else { q }),
            (c, q) => c.or(q),
        }?;
        let prev = match &join {
            Join::Candidate(_, t) | Join::Question(_, t) => t,
        };
        prefilter::continues(prev, &line.text).then_some(join)
    }

    /// The newest question, if it is still open and its last part ended
    /// just before `line`.
    fn recent_question(&self, line: &TranscriptLine) -> Option<&Question> {
        self.questions
            .last()
            .filter(|q| self.queue.status(q.id) != Some(QuestionStatus::Answered))
            .filter(|q| within_join(q.last_part_end, line))
    }

    /// Adds the rest of a sentence to one still being checked. Hints written
    /// for the first part alone are dropped.
    fn join_candidate(
        &mut self,
        req: ReqId,
        text: String,
        speech_end: i64,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        if let Some(c) = self.candidates.get_mut(&req) {
            c.text = text;
            c.speech_end = c.speech_end.max(speech_end);
            c.joined = true;
        }
        self.drop_speculation(req, now, out);
    }

    /// Replaces a question with its longer version and rewrites its hints.
    /// Detection already confirmed the start, so it isn't asked again.
    fn extend_question(
        &mut self,
        id: QuestionId,
        text: String,
        speech_end: i64,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        let Some(q) = self.question_mut(id) else {
            return;
        };
        q.version += 1;
        q.text = text;
        // Parts listed for the first half no longer describe the question.
        q.subs.clear();
        q.vocab.clear();
        q.statuses.clear();
        q.manual.clear();
        q.last_part_end = q.last_part_end.max(speech_end);
        q.latency = Latency {
            speech_end,
            asr_ms: (now - speech_end).max(0),
            ..Default::default()
        };
        self.cancel_hints(id, out);
        if let Some(ev) = self.question_event(id) {
            out.push(Output::Emit(ev));
        }
        self.request_hints(id, now, out);
    }

    fn on_me_sentence(
        &mut self,
        line: TranscriptLine,
        utterance: u32,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        // Echo filter: drop microphone sentences that repeat what the other
        // side just said through the speakers.
        let remote_partial = self.utterances[idx(Channel::Remote)].text();
        let nearby: Vec<String> = self
            .transcript
            .remote_near(line.t0_ms, line.t1_ms, ECHO_WINDOW_MS)
            .map(|l| l.text.clone())
            .chain(std::iter::once(remote_partial).filter(|t| !t.is_empty()))
            .collect();
        if coverage::is_echo(&line.text, nearby.iter().map(String::as_str)) {
            tracing::debug!("dropped echo on microphone channel");
            out.push(Output::Emit(MeetCueEvent::Transcript {
                channel: Channel::Me,
                utterance,
                is_final: true,
                text: String::new(),
                t0_ms: line.t0_ms,
                t1_ms: line.t1_ms,
            }));
            return;
        }
        self.last_me_activity = now;
        self.transcript.push(line.clone());
        out.push(Output::Emit(transcript_final(&line, utterance)));
        let Some(active) = self.queue.active() else {
            return;
        };
        let track = self.cfg.track_coverage;
        if let Some(q) = self.question_mut(active) {
            q.user_spoke = true;
            q.answer.push(&line.text);
        }
        if track {
            self.update_coverage(active, out);
        }
    }

    // ---- detection --------------------------------------------------------

    fn start_detect(
        &mut self,
        line: &TranscriptLine,
        now: i64,
        early: bool,
        target: Target,
        out: &mut Vec<Output>,
    ) -> ReqId {
        let req = self.alloc_req(Req::Detect);
        let mut transcript = format_for_prompt(self.transcript.since(now - DETECT_WINDOW_MS));
        if early {
            // The early sentence is not in the buffer yet.
            transcript.push_str("[Them] ");
            transcript.push_str(&line.text);
            transcript.push('\n');
        }
        let lang = self.output_language(&line.text);
        let request = detect::request(&self.cfg.detect.model, &transcript, lang);
        self.candidates.insert(
            req,
            Candidate {
                text: line.text.clone(),
                speech_end: line.t1_ms,
                heard_at: now,
                started_at: now,
                early,
                target,
                joined: false,
            },
        );
        out.push(Output::Detect { req, request });
        // A sentence sent only because it may finish the previous question
        // gets no hints of its own.
        if self.cfg.speculative_hints
            && target == Target::New
            && prefilter::might_be_question(&line.text)
            && !self.hints_paused
            && self.hints_running < MAX_CONCURRENT_HINTS
        {
            let request = self.hints_request(&line.text, lang, now);
            let spec = self.alloc_req(Req::Speculative { detect: req });
            self.hints_running += 1;
            self.speculative.insert(
                req,
                Speculation {
                    req: spec,
                    started_at: now,
                    items: Vec::new(),
                    finished: None,
                },
            );
            out.push(Output::Hints { req: spec, request });
        }
        req
    }

    fn on_detect_done(
        &mut self,
        req: ReqId,
        reply: Result<String, MeetCueError>,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        self.reqs.remove(&req);
        let spec = self.speculative.remove(&req);
        let Some(cand) = self.candidates.remove(&req) else {
            if let Some(spec) = spec {
                self.speculative.insert(req, spec);
                self.drop_speculation(req, now, out);
            }
            return;
        };
        let discard = |e: &mut Self, spec: Option<Speculation>, out: &mut Vec<Output>| {
            if let Some(spec) = spec {
                e.speculative.insert(req, spec);
                e.drop_speculation(req, now, out);
            }
        };
        let det = match reply {
            Ok(text) => match detect::parse(&text) {
                Some(d) => d,
                None => {
                    tracing::warn!("unreadable detection reply; using the rule-based result");
                    Detection {
                        is_question: cand.text.contains(['?', '？']),
                        directed_to_me: true,
                        ..Default::default()
                    }
                }
            },
            Err(e) => {
                discard(self, spec, out);
                out.push(Output::Emit(MeetCueEvent::Error {
                    code: e.code,
                    detail: e.detail,
                }));
                if let Some(link) = self
                    .early
                    .as_mut()
                    .filter(|l| l.state == EarlyState::Detecting(req))
                {
                    link.state = EarlyState::Dropped;
                }
                return;
            }
        };
        // The rest of a question cut off by a pause need not sound like one.
        let continues = det.merged && self.last_open_question().is_some();
        let keep =
            continues || (det.is_question && (det.directed_to_me || !self.cfg.only_directed_to_me));
        if !keep {
            discard(self, spec, out);
            if let Some(link) = self
                .early
                .as_mut()
                .filter(|l| l.state == EarlyState::Detecting(req))
            {
                link.state = EarlyState::Dropped;
            }
            if let Target::Redo(id) = cand.target {
                self.remove_question(id, out);
            }
            return;
        }
        let created = self.accept_detection(cand, det, spec, now, out);
        if let Some(link) = self
            .early
            .as_mut()
            .filter(|l| l.state == EarlyState::Detecting(req))
        {
            link.state = match created {
                Some(id) => EarlyState::Created(id),
                None => EarlyState::Dropped,
            };
        }
    }

    /// Turns a positive detection into a new, merged or updated question.
    fn accept_detection(
        &mut self,
        cand: Candidate,
        det: Detection,
        spec: Option<Speculation>,
        now: i64,
        out: &mut Vec<Output>,
    ) -> Option<QuestionId> {
        let detect_ms = now - cand.started_at;
        let latency = Latency {
            speech_end: cand.speech_end,
            asr_ms: (cand.heard_at - cand.speech_end).max(0),
            detect_ms,
            hint_start: 0,
            reported: false,
            early: cand.early,
        };
        let short = detect::short_title(&det.short, &cand.text);
        // Speculative hints answer the sentence they were started with, which
        // is the whole question unless it merges into an earlier one or more
        // of it arrived since. A compound sentence is fine: the hint prompt
        // covers every part it asks.
        let usable = !det.merged && !cand.joined;
        // Parts detection listed before the rest arrived are incomplete.
        let subs = if cand.joined {
            Vec::new()
        } else {
            det.sub_questions
        };

        // An updated version of an existing card.
        let existing = match cand.target {
            Target::Redo(id) => Some(id),
            Target::New if det.merged => self.last_open_question(),
            Target::New => None,
        };
        if let Some(id) = existing {
            let merged = det.merged && cand.target == Target::New;
            if let Some(q) = self.question_mut(id) {
                q.version += 1;
                if merged {
                    q.text = format!("{} {}", q.text, cand.text);
                } else {
                    q.text = cand.text.clone();
                    q.short = short;
                }
                q.tags = det.tags;
                if !subs.is_empty() {
                    q.subs = subs;
                }
                q.last_part_end = q.last_part_end.max(cand.speech_end);
                q.vocab.clear();
                q.statuses.clear();
                q.manual.clear();
                q.latency = latency;
            }
            self.cancel_hints(id, out);
            if let Some(ev) = self.question_event(id) {
                out.push(Output::Emit(ev));
            }
            self.start_hints(id, spec, usable, now, out);
            return Some(id);
        }

        let id = self.next_question;
        self.next_question += 1;
        let me_busy = !self.utterances[idx(Channel::Me)].is_empty()
            || now - self.last_me_activity < INTERRUPTION_WINDOW_MS;
        let interruption = me_busy || (det.interruption && now - self.last_me_activity < 5_000);
        let active_answered = self
            .queue
            .active()
            .and_then(|a| self.question(a))
            .is_some_and(|q| q.user_spoke);
        let admission = if interruption && !self.cfg.queue_interruptions {
            Admission::Preempt
        } else if active_answered && !interruption {
            Admission::AfterAnswer
        } else {
            Admission::Queue
        };
        let context = self.transcript.last_before(cand.speech_end + 1, 4);
        self.questions.push(Question {
            id,
            version: 1,
            text: cand.text.clone(),
            short,
            tags: det.tags,
            subs,
            t_ms: cand.speech_end,
            last_part_end: cand.speech_end,
            context,
            vocab: Vec::new(),
            statuses: Vec::new(),
            hints: HintsState::Idle,
            answer: AnswerText::default(),
            manual: HashMap::new(),
            user_spoke: false,
            latency,
        });
        let changes = self.queue.add(id, admission);
        if let Some(ev) = self.question_event(id) {
            out.push(Output::Emit(ev));
        }
        self.apply_status_changes(changes, out);
        self.trim_questions();
        self.start_hints(id, spec, usable, now, out);
        Some(id)
    }

    // ---- hints ------------------------------------------------------------

    fn request_hints(&mut self, id: QuestionId, now: i64, out: &mut Vec<Output>) {
        if self.hints_paused {
            if let Some(q) = self.question_mut(id) {
                q.hints = HintsState::Failed;
                let version = q.version;
                out.push(Output::Emit(MeetCueEvent::HintsFailed {
                    id,
                    version,
                    code: codes::BUDGET_REACHED.into(),
                }));
            }
            return;
        }
        if self.hints_running >= MAX_CONCURRENT_HINTS {
            if let Some(q) = self.question_mut(id) {
                q.hints = HintsState::Waiting;
            }
            if self.queue.active() == Some(id) {
                self.hints_waiting.push_front(id);
            } else {
                self.hints_waiting.push_back(id);
            }
            return;
        }
        let Some(q) = self.question(id) else { return };
        let version = q.version;
        let mut question = q.text.clone();
        if q.subs.len() > 1 {
            for (i, s) in q.subs.iter().enumerate() {
                question.push_str(&format!("\n  {}. {}", i + 1, s));
            }
        }
        let lang = self.output_language(&q.text);
        let request = self.hints_request(&question, lang, now);
        let req = self.alloc_req(Req::Hints { id, version });
        self.hints_running += 1;
        if let Some(q) = self.question_mut(id) {
            q.hints = HintsState::Running(req);
            q.latency.hint_start = now;
        }
        out.push(Output::Hints { req, request });
    }

    /// The hint generation request for a question (with its numbered parts).
    fn hints_request(&self, question: &str, lang: &str, now: i64) -> LlmRequest {
        let role = if self.cfg.role.trim().is_empty() {
            "not specified".to_string()
        } else {
            self.cfg.role.clone()
        };
        let materials = if self.cfg.materials.trim().is_empty() {
            "(none)".to_string()
        } else {
            self.cfg.materials.clone()
        };
        let system = prompts::render(
            prompts::HINTS_SYSTEM,
            &[
                ("role", &role),
                ("materials", &materials),
                ("output_language", lang),
            ],
        );
        let transcript = format_for_prompt(self.transcript.since(now - HINTS_WINDOW_MS));
        let user = prompts::render(
            prompts::HINTS_USER,
            &[
                ("transcript", transcript.trim_end()),
                ("question", question),
            ],
        );
        LlmRequest::new(&self.cfg.hints.model, UsagePurpose::Hints)
            .system(system, true)
            .user(user)
            .max_tokens(prompts::HINTS_MAX_TOKENS)
            .temperature(0.3)
    }

    /// Gives a confirmed question its hints: the speculative ones started
    /// with detection when they fit, otherwise a new request.
    fn start_hints(
        &mut self,
        id: QuestionId,
        spec: Option<Speculation>,
        usable: bool,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        let Some(spec) = spec else {
            self.request_hints(id, now, out);
            return;
        };
        let fits = usable && !self.hints_paused && spec.finished != Some(false);
        let Some(version) = self.question(id).map(|q| q.version).filter(|_| fits) else {
            if spec.finished.is_none() {
                self.reqs.remove(&spec.req);
                self.hints_running = self.hints_running.saturating_sub(1);
                out.push(Output::Cancel { req: spec.req });
            }
            self.request_hints(id, now, out);
            return;
        };
        let req = spec.req;
        if spec.finished.is_some() {
            // Ended already: count it as running until its items are replayed.
            self.hints_running += 1;
        }
        self.reqs.insert(req, Req::Hints { id, version });
        if let Some(q) = self.question_mut(id) {
            q.hints = HintsState::Running(req);
            q.latency.hint_start = spec.started_at;
        }
        for item in spec.items {
            self.on_hint(req, item, now, out);
        }
        if spec.finished == Some(true) {
            self.on_hints_done(req, now, out);
        }
    }

    fn on_hint(&mut self, req: ReqId, item: HintItem, now: i64, out: &mut Vec<Output>) {
        if let Some(Req::Speculative { detect }) = self.reqs.get(&req).copied() {
            if let Some(spec) = self.speculative.get_mut(&detect) {
                spec.items.push(item);
            }
            return;
        }
        let Some(Req::Hints { id, version }) = self.reqs.get(&req).copied() else {
            return;
        };
        let is_active = self.queue.active() == Some(id);
        let track = self.cfg.track_coverage;
        let Some(q) = self.question_mut(id) else {
            return;
        };
        if q.version != version {
            return;
        }
        match item {
            HintItem::Point { n, head, rest } => {
                q.vocab.push(PointVocab::new(&head, &rest));
                let status = if q.statuses.contains(&PointStatus::Next) || !is_active {
                    PointStatus::Todo
                } else {
                    PointStatus::Next
                };
                q.statuses.push(status);
                let first = !q.latency.reported;
                q.latency.reported = true;
                let lat = q.latency;
                out.push(Output::Emit(MeetCueEvent::QuestionPoint {
                    id,
                    version,
                    n,
                    head,
                    rest,
                    status,
                }));
                if first {
                    let total = now - lat.speech_end;
                    out.push(Output::Emit(MeetCueEvent::LatencySample {
                        id,
                        asr_ms: lat.asr_ms,
                        detect_ms: lat.detect_ms,
                        generate_ms: now - lat.hint_start,
                        total_ms: total,
                        early: lat.early,
                    }));
                    if let Some(median_ms) = self.latency.add(total) {
                        out.push(Output::Emit(MeetCueEvent::LatencyWarning { median_ms }));
                    }
                }
                if is_active && track {
                    self.update_coverage(id, out);
                }
            }
            HintItem::Answer(text) => out.push(Output::Emit(MeetCueEvent::AnswerDelta {
                id,
                version,
                text,
            })),
        }
    }

    /// A speculative request ended before detection decided: free its slot
    /// and keep what it produced.
    fn speculation_ended(&mut self, req: ReqId, ok: bool, now: i64, out: &mut Vec<Output>) -> bool {
        let Some(Req::Speculative { detect }) = self.reqs.get(&req).copied() else {
            return false;
        };
        self.reqs.remove(&req);
        self.hints_running = self.hints_running.saturating_sub(1);
        if let Some(spec) = self.speculative.get_mut(&detect) {
            spec.finished = Some(ok);
        }
        self.start_waiting_hints(now, out);
        true
    }

    fn on_hints_done(&mut self, req: ReqId, now: i64, out: &mut Vec<Output>) {
        if self.speculation_ended(req, true, now, out) {
            return;
        }
        let Some(Req::Hints { id, version }) = self.reqs.remove(&req) else {
            return;
        };
        self.hints_running = self.hints_running.saturating_sub(1);
        if let Some(q) = self.question_mut(id).filter(|q| q.version == version) {
            q.hints = HintsState::Done;
            out.push(Output::Emit(MeetCueEvent::AnswerDone { id, version }));
        }
        self.start_waiting_hints(now, out);
    }

    fn on_hints_failed(
        &mut self,
        req: ReqId,
        error: MeetCueError,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        if self.speculation_ended(req, false, now, out) {
            // Detection decides; a confirmed question gets a fresh request.
            tracing::debug!("speculative hints failed: {}", error.detail);
            return;
        }
        let Some(Req::Hints { id, version }) = self.reqs.remove(&req) else {
            return;
        };
        self.hints_running = self.hints_running.saturating_sub(1);
        if let Some(q) = self.question_mut(id).filter(|q| q.version == version) {
            q.hints = HintsState::Failed;
            out.push(Output::Emit(MeetCueEvent::HintsFailed {
                id,
                version,
                code: error.code.clone(),
            }));
        }
        out.push(Output::Emit(MeetCueEvent::Error {
            code: error.code,
            detail: error.detail,
        }));
        self.start_waiting_hints(now, out);
    }

    fn start_waiting_hints(&mut self, now: i64, out: &mut Vec<Output>) {
        while self.hints_running < MAX_CONCURRENT_HINTS {
            let Some(id) = self.hints_waiting.pop_front() else {
                break;
            };
            if self
                .question(id)
                .is_some_and(|q| q.hints == HintsState::Waiting)
            {
                self.request_hints(id, now, out);
            }
        }
    }

    fn cancel_hints(&mut self, id: QuestionId, out: &mut Vec<Output>) {
        self.hints_waiting.retain(|q| *q != id);
        let Some(q) = self.question_mut(id) else {
            return;
        };
        if let HintsState::Running(req) = q.hints {
            q.hints = HintsState::Idle;
            self.reqs.remove(&req);
            self.hints_running = self.hints_running.saturating_sub(1);
            out.push(Output::Cancel { req });
        } else {
            q.hints = HintsState::Idle;
        }
    }

    fn cancel_req(&mut self, req: ReqId, now: i64, out: &mut Vec<Output>) {
        self.candidates.remove(&req);
        if self.reqs.remove(&req).is_some() {
            out.push(Output::Cancel { req });
        }
        self.drop_speculation(req, now, out);
    }

    /// Discards the hints started alongside detection request `detect`.
    fn drop_speculation(&mut self, detect: ReqId, now: i64, out: &mut Vec<Output>) {
        let Some(spec) = self.speculative.remove(&detect) else {
            return;
        };
        if spec.finished.is_none() {
            self.reqs.remove(&spec.req);
            self.hints_running = self.hints_running.saturating_sub(1);
            out.push(Output::Cancel { req: spec.req });
            self.start_waiting_hints(now, out);
        }
    }

    // ---- coverage and queue -----------------------------------------------

    /// Recomputes the ticks of a question. The active question also counts the
    /// sentence the user is still saying, so points tick while they talk
    /// rather than only after they pause.
    fn update_coverage(&mut self, id: QuestionId, out: &mut Vec<Output>) {
        let track = self.cfg.track_coverage;
        let live = if track && self.queue.active() == Some(id) {
            self.live_answer()
        } else {
            None
        };
        let Some(q) = self.question_mut(id) else {
            return;
        };
        if q.vocab.is_empty() {
            return;
        }
        let mut answer = if track {
            q.answer.clone()
        } else {
            AnswerText::default()
        };
        if let Some(text) = live {
            answer.push(&text);
        }
        let next = coverage::statuses(&q.vocab, &q.statuses, &answer, &q.manual);
        if next != q.statuses {
            q.statuses = next.clone();
            out.push(Output::Emit(MeetCueEvent::Coverage { id, statuses: next }));
        }
    }

    /// The microphone sentence in progress, unless it is the other side
    /// leaking through the speakers.
    fn live_answer(&self) -> Option<String> {
        let u = &self.utterances[idx(Channel::Me)];
        if u.is_empty() {
            return None;
        }
        let text = u.text();
        let remote_partial = self.utterances[idx(Channel::Remote)].text();
        let nearby = self
            .transcript
            .remote_near(u.t0.unwrap_or(u.t1), u.t1, ECHO_WINDOW_MS)
            .map(|l| l.text.as_str())
            .chain(Some(remote_partial.as_str()).filter(|t| !t.is_empty()));
        (!coverage::is_echo(&text, nearby)).then_some(text)
    }

    fn apply_status_changes(
        &mut self,
        changes: Vec<(QuestionId, QuestionStatus)>,
        out: &mut Vec<Output>,
    ) {
        let track = self.cfg.track_coverage;
        for (id, status) in &changes {
            out.push(Output::Emit(MeetCueEvent::QuestionStatus {
                id: *id,
                status: *status,
            }));
            if *status == QuestionStatus::Active {
                if track {
                    self.update_coverage(*id, out);
                }
                // Make sure the newly active question's hints are first in line.
                if self
                    .question(*id)
                    .is_some_and(|q| q.hints == HintsState::Waiting)
                {
                    self.hints_waiting.retain(|q| q != id);
                    self.hints_waiting.push_front(*id);
                }
            }
        }
        out.push(Output::Emit(self.queue_event()));
    }

    fn finish_active(&mut self, out: &mut Vec<Output>) {
        let changes = self.queue.finish_active();
        if !changes.is_empty() {
            self.apply_status_changes(changes, out);
        }
    }

    fn remove_question(&mut self, id: QuestionId, out: &mut Vec<Output>) {
        self.cancel_hints(id, out);
        self.questions.retain(|q| q.id != id);
        let changes = self.queue.remove(id);
        out.push(Output::Emit(MeetCueEvent::QuestionRemoved { id }));
        self.apply_status_changes(changes, out);
    }

    fn queue_event(&self) -> MeetCueEvent {
        MeetCueEvent::QueueChanged {
            questions: self
                .queue
                .items()
                .iter()
                .filter_map(|(id, status)| {
                    self.question(*id).map(|q| QuestionBrief {
                        id: *id,
                        short: q.short.clone(),
                        status: *status,
                    })
                })
                .collect(),
            active: self.queue.active(),
            focus: self.queue.focus(),
        }
    }

    fn question_event(&self, id: QuestionId) -> Option<MeetCueEvent> {
        let q = self.question(id)?;
        Some(MeetCueEvent::QuestionNew {
            id,
            version: q.version,
            text: q.text.clone(),
            short: q.short.clone(),
            tags: q.tags.clone(),
            sub_questions: q.subs.clone(),
            status: self.queue.status(id).unwrap_or(QuestionStatus::Pending),
            t_ms: q.t_ms,
            context: q.context.clone(),
        })
    }

    fn last_open_question(&self) -> Option<QuestionId> {
        self.queue
            .items()
            .iter()
            .rev()
            .find(|(_, s)| *s != QuestionStatus::Answered)
            .map(|(id, _)| *id)
    }

    fn trim_questions(&mut self) {
        while self.questions.len() > MAX_QUESTIONS {
            let Some(pos) = self
                .questions
                .iter()
                .position(|q| self.queue.status(q.id) == Some(QuestionStatus::Answered))
            else {
                break;
            };
            let id = self.questions.remove(pos).id;
            self.queue.remove(id);
        }
    }

    // ---- actions, ticks, usage --------------------------------------------

    fn on_action(&mut self, action: UserAction, now: i64, out: &mut Vec<Output>) {
        match action {
            UserAction::MarkAnswered => self.finish_active(out),
            UserAction::Focus { id } => {
                if self.queue.set_focus(id) {
                    out.push(Output::Emit(self.queue_event()));
                }
            }
            UserAction::FocusStep { target } => {
                if self.queue.step_focus(target) {
                    out.push(Output::Emit(self.queue_event()));
                }
            }
            UserAction::TogglePoint { id, index } => {
                let index = index as usize;
                let Some(q) = self.question_mut(id) else {
                    return;
                };
                let Some(status) = q.statuses.get(index) else {
                    return;
                };
                let done = *status != PointStatus::Done;
                q.manual.insert(index, done);
                self.update_coverage(id, out);
            }
            UserAction::Pause if self.state == SessionState::Running => {
                self.state = SessionState::Paused;
                out.push(Output::SetCapturePaused(true));
                out.push(Output::Emit(MeetCueEvent::SessionState {
                    state: self.state,
                    t_ms: now,
                }));
            }
            UserAction::Resume if self.state == SessionState::Paused => {
                self.state = SessionState::Running;
                out.push(Output::SetCapturePaused(false));
                out.push(Output::Emit(MeetCueEvent::SessionState {
                    state: self.state,
                    t_ms: now,
                }));
            }
            UserAction::Pause | UserAction::Resume => {}
        }
    }

    fn on_tick(&mut self, now: i64, out: &mut Vec<Output>) {
        // Close sentences whose provider never sent an endpoint.
        for ch in [Channel::Me, Channel::Remote] {
            let u = &self.utterances[idx(ch)];
            if !u.finals.trim().is_empty()
                && u.partial.is_empty()
                && now - u.last_change >= STALE_UTTERANCE_MS
            {
                self.close_utterance(ch, now, out);
            }
        }

        // Early trigger: a partial that looks like a question and has been
        // stable for EARLY_QUIET_MS starts detection before the endpoint.
        let remote = &self.utterances[idx(Channel::Remote)];
        if self.cfg.early_trigger
            && self.state == SessionState::Running
            && !remote.is_empty()
            && now - remote.last_change >= EARLY_QUIET_MS
            && self.early.as_ref().is_none_or(|e| e.utterance != remote.id)
        {
            let text = remote.text();
            let line = TranscriptLine {
                channel: Channel::Remote,
                text: text.clone(),
                t0_ms: remote.t0.unwrap_or(remote.t1),
                t1_ms: remote.t1,
            };
            // The rest of the previous question is joined to it once the
            // sentence ends, not detected as a question of its own.
            if prefilter::might_be_question(&text) && self.continuation_of(&line).is_none() {
                let utterance = remote.id;
                let req = self.start_detect(&line, now, true, Target::New, out);
                self.early = Some(EarlyLink {
                    utterance,
                    snapshot: text,
                    state: EarlyState::Detecting(req),
                });
            }
        }

        // A fully covered question is answered once the user has been quiet a while.
        if let Some(id) = self.queue.active() {
            let me_quiet = self.utterances[idx(Channel::Me)].is_empty()
                && now - self.last_me_activity >= ANSWERED_SILENCE_MS;
            let done = self.question(id).is_some_and(|q| {
                q.user_spoke
                    && !q.statuses.is_empty()
                    && matches!(q.hints, HintsState::Done)
                    && q.statuses.iter().all(|s| *s == PointStatus::Done)
            });
            if done && me_quiet {
                self.finish_active(out);
            }
        }
    }

    fn on_usage(
        &mut self,
        provider: &str,
        model: &str,
        purpose: UsagePurpose,
        usage: UsageKind,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        let (events, outcome) = self.meter.record(provider, model, purpose, usage);
        out.extend(events.into_iter().map(Output::Emit));
        match outcome {
            BudgetOutcome::Continue => {}
            BudgetOutcome::PauseHints => self.hints_paused = true,
            BudgetOutcome::EndSession => {
                out.extend(self.stop(now));
                out.push(Output::EndSession);
            }
        }
    }

    // ---- helpers ----------------------------------------------------------

    fn alloc_req(&mut self, kind: Req) -> ReqId {
        let req = self.next_req;
        self.next_req += 1;
        self.reqs.insert(req, kind);
        req
    }

    fn question(&self, id: QuestionId) -> Option<&Question> {
        self.questions.iter().find(|q| q.id == id)
    }

    fn question_mut(&mut self, id: QuestionId) -> Option<&mut Question> {
        self.questions.iter_mut().find(|q| q.id == id)
    }

    fn output_language(&self, text: &str) -> &'static str {
        match self.cfg.hint_language {
            HintLanguage::ZhCn => Lang::Zh.prompt_name(),
            HintLanguage::En => Lang::En.prompt_name(),
            HintLanguage::FollowQuestion => dominant_lang(text).prompt_name(),
        }
    }
}

/// What a sentence that finishes an earlier one is joined to, with that
/// earlier text.
enum Join {
    Candidate(ReqId, String),
    Question(QuestionId, String),
}

/// Whether `line` starts soon enough after `end` to be its continuation.
fn within_join(end: i64, line: &TranscriptLine) -> bool {
    line.t0_ms - end <= CONTINUATION_MS && end <= line.t1_ms
}

fn transcript_final(line: &TranscriptLine, utterance: u32) -> MeetCueEvent {
    MeetCueEvent::Transcript {
        channel: line.channel,
        utterance,
        is_final: true,
        text: line.text.clone(),
        t0_ms: line.t0_ms,
        t1_ms: line.t1_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meetcue_core::settings::{BudgetAction, BudgetSettings};
    use meetcue_core::FocusTarget;

    /// Drives an engine by hand and collects what it does.
    struct Harness {
        engine: Engine,
        now: i64,
        events: Vec<MeetCueEvent>,
        detects: Vec<(ReqId, LlmRequest)>,
        hints: Vec<(ReqId, LlmRequest)>,
        cancelled: Vec<ReqId>,
        ended: bool,
    }

    impl Harness {
        fn new(cfg: EngineConfig) -> Self {
            Self::with_budget(cfg, BudgetSettings::default())
        }

        fn with_budget(cfg: EngineConfig, budget: BudgetSettings) -> Self {
            let mut h = Self {
                engine: Engine::new(cfg, CostMeter::new(vec![], budget, 0)),
                now: 0,
                events: vec![],
                detects: vec![],
                hints: vec![],
                cancelled: vec![],
                ended: false,
            };
            let out = h.engine.start(0);
            h.absorb(out);
            h
        }

        fn absorb(&mut self, out: Vec<Output>) {
            for o in out {
                match o {
                    Output::Emit(e) => self.events.push(e),
                    Output::Detect { req, request } => self.detects.push((req, request)),
                    Output::Hints { req, request } => self.hints.push((req, request)),
                    Output::Cancel { req } => self.cancelled.push(req),
                    Output::SetCapturePaused(_) => {}
                    Output::EndSession => self.ended = true,
                }
            }
        }

        fn at(&mut self, t: i64) -> &mut Self {
            // Advance in 50 ms ticks like the real driver.
            while self.now + 50 <= t {
                self.now += 50;
                let out = self.engine.handle(Input::Tick, self.now);
                self.absorb(out);
            }
            self.now = t;
            self
        }

        fn input(&mut self, input: Input) -> &mut Self {
            let out = self.engine.handle(input, self.now);
            self.absorb(out);
            self
        }

        /// Speaks a whole sentence on a channel: partial, final, endpoint.
        fn say(&mut self, ch: Channel, t0: i64, t1: i64, text: &str) -> &mut Self {
            self.at(t0);
            let half: String = text.chars().take(text.chars().count() / 2).collect();
            self.input(Input::Asr {
                channel: ch,
                result: AsrResult::Partial {
                    text: half,
                    t0_ms: t0,
                    t1_ms: t0,
                },
            });
            self.at(t1);
            self.input(Input::Asr {
                channel: ch,
                result: AsrResult::Final {
                    text: text.into(),
                    t0_ms: t0,
                    t1_ms: t1,
                },
            });
            self.at(t1 + 100);
            self.input(Input::Asr {
                channel: ch,
                result: AsrResult::Endpoint { t_ms: t1 },
            })
        }

        fn detect_reply(&mut self, json: &str) -> &mut Self {
            let (req, _) = self.detects.last().cloned().expect("a detection request");
            self.input(Input::DetectDone {
                req,
                reply: Ok(json.into()),
            })
        }

        fn hint_points(&mut self, req: ReqId, points: &[(&str, &str)]) -> &mut Self {
            for (i, (head, rest)) in points.iter().enumerate() {
                self.input(Input::Hint {
                    req,
                    item: HintItem::Point {
                        n: i as u32 + 1,
                        head: (*head).into(),
                        rest: (*rest).into(),
                    },
                });
            }
            self.input(Input::HintsDone { req })
        }

        fn statuses(&self) -> Vec<(QuestionId, QuestionStatus)> {
            self.events
                .iter()
                .filter_map(|e| match e {
                    MeetCueEvent::QuestionStatus { id, status } => Some((*id, *status)),
                    _ => None,
                })
                .collect()
        }

        fn last_coverage(&self, qid: QuestionId) -> Option<Vec<PointStatus>> {
            self.events.iter().rev().find_map(|e| match e {
                MeetCueEvent::Coverage { id, statuses } if *id == qid => Some(statuses.clone()),
                _ => None,
            })
        }
    }

    const YES: &str = r#"{"isQuestion":true,"directedToMe":true,"merged":false,"short":"Q","subQuestions":[],"tags":[]}"#;

    fn no_early() -> EngineConfig {
        EngineConfig {
            early_trigger: false,
            ..Default::default()
        }
    }

    #[test]
    fn question_flow_with_coverage_and_silence() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 1000, 3000, "你们是怎么做高可用的？");
        assert_eq!(h.detects.len(), 1);
        let (_, req) = &h.detects[0];
        assert!(req.messages[0]
            .content
            .ends_with("[Them] 你们是怎么做高可用的？\n"));
        h.at(3400).detect_reply(YES);
        assert_eq!(h.statuses(), vec![(1, QuestionStatus::Active)]);
        let hint_req = h.hints[0].0;
        h.at(3700)
            .hint_points(hint_req, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        // Latency: first point at 3700, speech ended at 3000.
        let lat = h.events.iter().find_map(|e| match e {
            MeetCueEvent::LatencySample {
                total_ms,
                detect_ms,
                ..
            } => Some((*total_ms, *detect_ms)),
            _ => None,
        });
        assert_eq!(lat, Some((700, 300)));

        h.say(Channel::Me, 5000, 7000, "我们做了两地双活");
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Done, PointStatus::Next])
        );
        // Not all points said: silence does not finish the question.
        h.at(15_000);
        assert_eq!(h.statuses().len(), 1);
        h.say(Channel::Me, 15_000, 17_000, "核心交换机堆叠，都是双电源");
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Done, PointStatus::Done])
        );
        h.at(17_100 + ANSWERED_SILENCE_MS - 100);
        assert_eq!(h.statuses().len(), 1);
        h.at(17_100 + ANSWERED_SILENCE_MS + 100);
        assert_eq!(h.statuses().last(), Some(&(1, QuestionStatus::Answered)));
    }

    fn asked_and_active(h: &mut Harness) -> ReqId {
        h.say(Channel::Remote, 1000, 3000, "你们是怎么做高可用的？");
        h.at(3400).detect_reply(YES);
        h.hints[0].0
    }

    #[test]
    fn points_tick_while_the_user_is_still_talking() {
        let mut h = Harness::new(no_early());
        let req = asked_and_active(&mut h);
        h.at(3700)
            .hint_points(req, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        // One long sentence with no pause: the first point ticks before it ends.
        h.at(5000).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Partial {
                text: "我们做了两地双活，然后".into(),
                t0_ms: 5000,
                t1_ms: 6000,
            },
        });
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Done, PointStatus::Next])
        );
    }

    #[test]
    fn points_already_said_tick_when_they_arrive() {
        let mut h = Harness::new(no_early());
        let req = asked_and_active(&mut h);
        h.say(Channel::Me, 3500, 4500, "我们做了两地双活");
        h.at(4700)
            .hint_points(req, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        // Ticked as soon as it arrives; the second point then arrives as next.
        assert_eq!(h.last_coverage(1), Some(vec![PointStatus::Done]));
        let second = h.events.iter().rev().find_map(|e| match e {
            MeetCueEvent::QuestionPoint { n: 2, status, .. } => Some(*status),
            _ => None,
        });
        assert_eq!(second, Some(PointStatus::Next));
    }

    #[test]
    fn points_can_be_ticked_by_hand() {
        let mut h = Harness::new(no_early());
        let req = asked_and_active(&mut h);
        h.at(3700)
            .hint_points(req, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        h.input(Input::Action(UserAction::TogglePoint { id: 1, index: 1 }));
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Next, PointStatus::Done])
        );
        // Unticked by hand: saying it again does not tick it back.
        h.input(Input::Action(UserAction::TogglePoint { id: 1, index: 1 }));
        h.say(Channel::Me, 5000, 6000, "核心交换机堆叠，双电源");
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Next, PointStatus::Todo])
        );
        // Out of range does nothing.
        h.input(Input::Action(UserAction::TogglePoint { id: 1, index: 9 }));
    }

    #[test]
    fn non_questions_never_reach_the_model() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "好的，我们开始吧。");
        h.say(Channel::Remote, 2000, 2500, "对吧？");
        h.say(Channel::Me, 3000, 4000, "你能听到我吗？");
        assert!(h.detects.is_empty());
    }

    #[test]
    fn detection_rejects_and_directed_filter() {
        let mut h = Harness::new(EngineConfig {
            only_directed_to_me: true,
            ..no_early()
        });
        h.say(Channel::Remote, 0, 1000, "这个问题大家怎么看？");
        h.detect_reply(r#"{"isQuestion":true,"directedToMe":false}"#);
        h.say(Channel::Remote, 2000, 3000, "为什么呢，我也不知道");
        h.detect_reply(r#"{"isQuestion":false}"#);
        assert!(h.statuses().is_empty());
        assert!(h.hints.is_empty());
    }

    #[test]
    fn interruption_waits_and_new_question_after_answer_takes_over() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "第一个问题是什么？");
        h.detect_reply(YES);
        // The user is mid-answer when the other side cuts in.
        h.at(2000).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Partial {
                text: "我觉得".into(),
                t0_ms: 2000,
                t1_ms: 2000,
            },
        });
        h.say(Channel::Remote, 2100, 2800, "等等，这个会不会有风险？");
        h.detect_reply(YES);
        assert_eq!(h.statuses().last(), Some(&(2, QuestionStatus::Pending)));
        h.input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Final {
                text: "我觉得可以".into(),
                t0_ms: 2000,
                t1_ms: 3500,
            },
        });
        h.at(3600).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Endpoint { t_ms: 3500 },
        });
        // Marking the active question answered promotes the waiting one.
        h.input(Input::Action(UserAction::MarkAnswered));
        assert_eq!(
            h.statuses()[2..].to_vec(),
            vec![(1, QuestionStatus::Answered), (2, QuestionStatus::Active)]
        );
        // The user answers question 2, pauses, then a new question arrives.
        h.say(Channel::Me, 4000, 5000, "风险可控");
        h.say(Channel::Remote, 8000, 9000, "下一个问题，你怎么排期？");
        h.detect_reply(YES);
        assert_eq!(
            h.statuses()[4..].to_vec(),
            vec![(2, QuestionStatus::Answered), (3, QuestionStatus::Active)]
        );
    }

    #[test]
    fn preempt_when_queueing_is_off() {
        let mut h = Harness::new(EngineConfig {
            queue_interruptions: false,
            ..no_early()
        });
        h.say(Channel::Remote, 0, 1000, "第一个问题是什么？");
        h.detect_reply(YES);
        h.at(1500).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Partial {
                text: "我".into(),
                t0_ms: 1500,
                t1_ms: 1500,
            },
        });
        h.say(Channel::Remote, 1600, 2000, "插一句，预算多少？");
        h.detect_reply(YES);
        assert_eq!(
            h.statuses()[1..].to_vec(),
            vec![(1, QuestionStatus::Pending), (2, QuestionStatus::Active)]
        );
    }

    #[test]
    fn merged_question_bumps_version_and_regenerates() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "你们怎么做监控的？");
        h.detect_reply(YES);
        let first = h.hints[0].0;
        h.say(Channel::Remote, 1500, 2500, "我是说告警这块怎么做？");
        h.detect_reply(
            r#"{"isQuestion":true,"directedToMe":true,"merged":true,"short":"监控告警"}"#,
        );
        assert!(h.cancelled.contains(&first));
        assert_eq!(h.hints.len(), 2);
        let versions: Vec<u32> = h
            .events
            .iter()
            .filter_map(|e| match e {
                MeetCueEvent::QuestionNew { id: 1, version, .. } => Some(*version),
                _ => None,
            })
            .collect();
        assert_eq!(versions, [1, 2]);
        // Points from the cancelled request are ignored.
        h.input(Input::Hint {
            req: first,
            item: HintItem::Point {
                n: 1,
                head: "旧".into(),
                rest: String::new(),
            },
        });
        assert!(!h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::QuestionPoint { head, .. } if head == "旧")));
    }

    fn speculative() -> EngineConfig {
        EngineConfig {
            speculative_hints: true,
            ..no_early()
        }
    }

    fn points(h: &Harness) -> usize {
        h.events
            .iter()
            .filter(|e| matches!(e, MeetCueEvent::QuestionPoint { .. }))
            .count()
    }

    #[test]
    fn speculative_hints_start_with_detection_and_wait_for_it() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 1000, 3000, "你们是怎么做高可用的？");
        assert_eq!(h.detects.len(), 1);
        assert_eq!(h.hints.len(), 1, "hints start together with detection");
        let spec = h.hints[0].0;
        // Points that arrive before detection answers are held back.
        h.at(3300)
            .hint_points(spec, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        assert_eq!(points(&h), 0);
        assert!(h.statuses().is_empty());
        h.at(3500).detect_reply(YES);
        assert_eq!(points(&h), 2);
        assert_eq!(h.hints.len(), 1, "no second hint request");
        assert!(h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::AnswerDone { id: 1, .. })));
        let total = h.events.iter().find_map(|e| match e {
            MeetCueEvent::LatencySample { total_ms, .. } => Some(*total_ms),
            _ => None,
        });
        assert_eq!(total, Some(500), "one model round trip instead of two");
    }

    #[test]
    fn speculative_hints_keep_streaming_after_detection() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 1000, 3000, "你们是怎么做高可用的？");
        let spec = h.hints[0].0;
        h.at(3300).detect_reply(YES);
        assert_eq!(h.statuses(), vec![(1, QuestionStatus::Active)]);
        h.at(3600).hint_points(spec, &[("双活", "两地 分担")]);
        assert_eq!(points(&h), 1);
        assert_eq!(h.hints.len(), 1);
    }

    #[test]
    fn speculative_hints_are_cancelled_for_non_questions() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 0, 1000, "为什么呢，我也不知道");
        let spec = h.hints[0].0;
        h.detect_reply(r#"{"isQuestion":false}"#);
        assert!(h.cancelled.contains(&spec));
        h.hint_points(spec, &[("x", "y")]);
        assert_eq!(points(&h), 0);
        assert!(h.statuses().is_empty());
        // The slot is free again for the next question.
        h.say(Channel::Remote, 2000, 3000, "你们是怎么做高可用的？");
        assert_eq!(h.hints.len(), 2);
    }

    #[test]
    fn failed_speculation_is_retried_once_confirmed() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 1000, 3000, "你们是怎么做高可用的？");
        let spec = h.hints[0].0;
        h.input(Input::HintsFailed {
            req: spec,
            error: MeetCueError::new(codes::LLM_TIMEOUT, "slow"),
        });
        h.at(3400).detect_reply(YES);
        assert_eq!(h.hints.len(), 2, "a fresh request after the failure");
        assert!(!h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::HintsFailed { .. })));
    }

    #[test]
    fn early_trigger_keeps_small_changes() {
        let mut h = Harness::new(EngineConfig::default());
        h.at(1000).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Partial {
                text: "控制器挂了还能转发吗".into(),
                t0_ms: 0,
                t1_ms: 1000,
            },
        });
        h.at(1000 + EARLY_QUIET_MS + 50);
        assert_eq!(
            h.detects.len(),
            1,
            "early trigger fired before the endpoint"
        );
        let (_, req) = &h.detects[0];
        assert!(req.messages[0]
            .content
            .ends_with("[Them] 控制器挂了还能转发吗\n"));
        h.detect_reply(YES);
        assert_eq!(h.hints.len(), 1);
        // The final adds only punctuation: same question, no new detection.
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "控制器挂了还能转发吗？".into(),
                t0_ms: 0,
                t1_ms: 1000,
            },
        });
        h.at(1600).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Endpoint { t_ms: 1000 },
        });
        assert_eq!(h.detects.len(), 1);
        assert_eq!(h.hints.len(), 1);
        assert!(h.cancelled.is_empty());
        let early = h.events.iter().find_map(|e| match e {
            MeetCueEvent::QuestionNew { text, .. } => Some(text.clone()),
            _ => None,
        });
        assert!(early.is_some());
    }

    #[test]
    fn early_trigger_redoes_large_changes() {
        let mut h = Harness::new(EngineConfig::default());
        h.at(1000).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Partial {
                text: "你们怎么做".into(),
                t0_ms: 0,
                t1_ms: 1000,
            },
        });
        h.at(1500);
        assert_eq!(h.detects.len(), 1);
        let early_req = h.detects[0].0;
        // The sentence is re-recognised differently before detection returns.
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "那你们公司是如何规划容量的？".into(),
                t0_ms: 0,
                t1_ms: 2500,
            },
        });
        h.at(2600).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Endpoint { t_ms: 2500 },
        });
        assert!(h.cancelled.contains(&early_req), "early request cancelled");
        assert_eq!(
            h.detects.len(),
            2,
            "detection restarted on the final sentence"
        );
        // A late reply for the cancelled request is ignored.
        h.input(Input::DetectDone {
            req: early_req,
            reply: Ok(YES.into()),
        });
        assert!(h.statuses().is_empty());
        h.detect_reply(YES);
        assert_eq!(h.statuses(), vec![(1, QuestionStatus::Active)]);
    }

    #[test]
    fn early_trigger_after_question_created_answers_the_whole_sentence() {
        let mut h = Harness::new(EngineConfig::default());
        h.at(1000).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Partial {
                text: "什么是 SDN".into(),
                t0_ms: 0,
                t1_ms: 1000,
            },
        });
        h.at(1500).detect_reply(YES);
        let first_hints = h.hints[0].0;
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "什么是 SDN，它和传统网络架构的区别和优势在哪里？".into(),
                t0_ms: 0,
                t1_ms: 3000,
            },
        });
        h.at(3100).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Endpoint { t_ms: 3000 },
        });
        assert!(h.cancelled.contains(&first_hints));
        // Detection already confirmed the start: only the hints are redone.
        assert_eq!(h.detects.len(), 1);
        assert_eq!(h.hints.len(), 2);
        assert!(h.hints[1].1.messages[0]
            .content
            .contains("传统网络架构的区别"));
        let versions: Vec<(u32, u32)> = h
            .events
            .iter()
            .filter_map(|e| match e {
                MeetCueEvent::QuestionNew { id, version, .. } => Some((*id, *version)),
                _ => None,
            })
            .collect();
        assert_eq!(versions, [(1, 1), (1, 2)]);
    }

    #[test]
    fn early_trigger_waits_for_the_rest_of_the_sentence() {
        let mut h = Harness::new(EngineConfig {
            speculative_hints: true,
            ..Default::default()
        });
        h.at(1000).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Partial {
                text: "Talk about your thoughts on idea A".into(),
                t0_ms: 0,
                t1_ms: 1000,
            },
        });
        h.at(1500);
        assert_eq!(h.detects.len(), 1);
        let spec = h.hints[0].0;
        // The speaker only paused; the sentence goes on.
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "Talk about your thoughts on idea A and idea B.".into(),
                t0_ms: 0,
                t1_ms: 2500,
            },
        });
        h.at(2600).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Endpoint { t_ms: 2500 },
        });
        assert_eq!(h.detects.len(), 1, "detection is not restarted");
        assert!(
            h.cancelled.contains(&spec),
            "hints for the first half dropped"
        );
        h.detect_reply(YES);
        assert_eq!(h.hints.len(), 2);
        assert!(h.hints[1].1.messages[0]
            .content
            .contains("idea A and idea B"));
    }

    fn texts(h: &Harness) -> Vec<(QuestionId, u32, String)> {
        h.events
            .iter()
            .filter_map(|e| match e {
                MeetCueEvent::QuestionNew {
                    id, version, text, ..
                } => Some((*id, *version, text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn question_split_by_a_pause_is_joined() {
        let mut h = Harness::new(no_early());
        h.say(
            Channel::Remote,
            0,
            2000,
            "Talk about your thoughts on idea A.",
        );
        h.at(2300).detect_reply(YES);
        let first = h.hints[0].0;
        h.say(Channel::Remote, 3500, 4500, "And idea B.");
        assert_eq!(h.detects.len(), 1, "joined without asking the model");
        assert!(h.cancelled.contains(&first));
        assert_eq!(h.hints.len(), 2);
        assert!(h.hints[1].1.messages[0]
            .content
            .contains("Talk about your thoughts on idea A. And idea B."));
        assert_eq!(
            texts(&h).last(),
            Some(&(
                1,
                2,
                "Talk about your thoughts on idea A. And idea B.".into()
            ))
        );
        assert_eq!(h.statuses(), vec![(1, QuestionStatus::Active)]);
    }

    #[test]
    fn rest_of_a_question_joins_it_while_detection_runs() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 0, 2000, "说说你对微服务的理解，");
        let spec = h.hints[0].0;
        h.say(Channel::Remote, 2600, 3500, "以及服务网格适合什么场景");
        assert!(h.cancelled.contains(&spec));
        assert_eq!(h.detects.len(), 1);
        h.detect_reply(
            r#"{"isQuestion":true,"directedToMe":true,"short":"微服务","subQuestions":["微服务是什么","为什么用"]}"#,
        );
        assert_eq!(
            texts(&h),
            vec![(
                1,
                1,
                "说说你对微服务的理解，以及服务网格适合什么场景".into()
            )]
        );
        assert_eq!(h.hints.len(), 2);
        // The parts listed for the first half alone are not kept.
        assert!(!h.hints[1].1.messages[0].content.contains("为什么用"));
    }

    #[test]
    fn fragment_after_a_question_is_checked_by_the_model() {
        let mut h = Harness::new(speculative());
        h.say(Channel::Remote, 0, 2000, "What do you think of idea A?");
        h.at(2300).detect_reply(YES);
        assert_eq!(h.hints.len(), 1);
        h.say(Channel::Remote, 3000, 3800, "Idea B, too.");
        assert_eq!(h.detects.len(), 2, "sent to the model despite the rules");
        assert_eq!(h.hints.len(), 1, "no speculative hints for a fragment");
        h.detect_reply(r#"{"isQuestion":false,"directedToMe":true,"merged":true}"#);
        assert_eq!(h.hints.len(), 2);
        assert_eq!(texts(&h).last().map(|t| (t.0, t.1)), Some((1, 2)));
        // Much later, the same fragment is just a remark.
        h.say(Channel::Remote, 20_000, 20_800, "Idea C, too.");
        assert_eq!(h.detects.len(), 2);
    }

    #[test]
    fn compound_question_keeps_its_speculative_hints() {
        let mut h = Harness::new(speculative());
        h.say(
            Channel::Remote,
            0,
            2000,
            "What are microservices, and when would you avoid them?",
        );
        h.at(2300).detect_reply(
            r#"{"isQuestion":true,"directedToMe":true,"subQuestions":["What are microservices?","When to avoid them?"]}"#,
        );
        assert_eq!(h.hints.len(), 1, "no second round trip");
        assert!(h.cancelled.is_empty());
    }

    #[test]
    fn echo_is_dropped_from_microphone() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 2000, "你们的网络架构是怎么做高可用的？");
        h.detect_reply(YES);
        h.hint_points(h.hints[0].0, &[("高可用", "架构")]);
        h.say(Channel::Me, 300, 2100, "网络架构是怎么做高可用的");
        let me_finals: Vec<_> = h
            .events
            .iter()
            .filter_map(|e| match e {
                MeetCueEvent::Transcript {
                    channel: Channel::Me,
                    is_final: true,
                    text,
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            me_finals,
            [""],
            "the echo is cleared, not shown as the user's speech"
        );
        assert_eq!(h.last_coverage(1), None, "echo must not tick points");
    }

    #[test]
    fn at_most_two_generations_run_and_active_goes_first() {
        let mut h = Harness::new(no_early());
        for (i, q) in ["第一题是什么？", "第二题是什么？", "第三题是什么？"]
            .iter()
            .enumerate()
        {
            let t = i as i64 * 2000;
            h.say(Channel::Remote, t, t + 1000, q);
            h.detect_reply(YES);
        }
        assert_eq!(h.hints.len(), 2, "third generation waits");
        let first = h.hints[0].0;
        h.hint_points(first, &[("a", "")]);
        assert_eq!(h.hints.len(), 3);
    }

    #[test]
    fn hint_failure_is_reported() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "为什么选这个方案？");
        h.detect_reply(YES);
        let req = h.hints[0].0;
        h.input(Input::HintsFailed {
            req,
            error: MeetCueError::new(codes::LLM_TIMEOUT, "slow"),
        });
        assert!(h.events.iter().any(|e| matches!(e, MeetCueEvent::HintsFailed { id: 1, code, .. } if code == codes::LLM_TIMEOUT)));
        assert!(h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::Error { code, .. } if code == codes::LLM_TIMEOUT)));
    }

    #[test]
    fn focus_and_pause() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "第一题是什么？");
        h.detect_reply(YES);
        h.at(1200).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Partial {
                text: "嗯".into(),
                t0_ms: 1200,
                t1_ms: 1200,
            },
        });
        h.say(Channel::Remote, 1300, 1800, "第二题是什么？");
        h.detect_reply(YES);
        h.input(Input::Action(UserAction::FocusStep {
            target: FocusTarget::Next,
        }));
        let focus = h.events.iter().rev().find_map(|e| match e {
            MeetCueEvent::QueueChanged { focus, active, .. } => Some((*focus, *active)),
            _ => None,
        });
        assert_eq!(focus, Some((Some(2), Some(1))));
        h.input(Input::Action(UserAction::Pause));
        assert_eq!(h.engine.state(), SessionState::Paused);
        h.input(Input::Action(UserAction::Resume));
        assert_eq!(h.engine.state(), SessionState::Running);
    }

    #[test]
    fn stale_sentence_is_closed_without_endpoint() {
        let mut h = Harness::new(no_early());
        h.at(0).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "这个怎么部署？".into(),
                t0_ms: 0,
                t1_ms: 900,
            },
        });
        h.at(STALE_UTTERANCE_MS + 100);
        assert_eq!(h.detects.len(), 1);
    }

    #[test]
    fn practice_known_question_skips_detection() {
        let mut h = Harness::new(no_early());
        h.input(Input::RemoteText {
            text: "介绍一下你自己".into(),
            t_ms: 0,
            known_question: true,
        });
        assert!(h.detects.is_empty());
        assert_eq!(h.statuses(), vec![(1, QuestionStatus::Active)]);
        assert_eq!(h.hints.len(), 1);
    }

    #[test]
    fn budget_pause_stops_new_hints() {
        crate::usage::load_published_prices();
        let budget = BudgetSettings {
            session_limit_usd: Some(0.01),
            monthly_limit_usd: None,
            on_limit: BudgetAction::PauseHints,
        };
        let mut h = Harness::with_budget(no_early(), budget);
        h.input(Input::Usage {
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
            purpose: UsagePurpose::Hints,
            usage: UsageKind::Llm {
                input: 200_000,
                output: 0,
                cache_read: 0,
                cache_write: 0,
            },
        });
        assert!(h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::Budget { .. })));
        h.input(Input::RemoteText {
            text: "介绍一下你自己".into(),
            t_ms: 0,
            known_question: true,
        });
        assert!(h.hints.is_empty());
        assert!(h.events.iter().any(
            |e| matches!(e, MeetCueEvent::HintsFailed { code, .. } if code == codes::BUDGET_REACHED)
        ));
    }

    #[test]
    fn budget_end_session() {
        crate::usage::load_published_prices();
        let budget = BudgetSettings {
            session_limit_usd: Some(0.01),
            monthly_limit_usd: None,
            on_limit: BudgetAction::EndSession,
        };
        let mut h = Harness::with_budget(no_early(), budget);
        h.input(Input::Usage {
            provider: "anthropic".into(),
            model: "claude-haiku-5-5".into(),
            purpose: UsagePurpose::Hints,
            usage: UsageKind::Llm {
                input: 200_000,
                output: 0,
                cache_read: 0,
                cache_write: 0,
            },
        });
        assert!(h.ended);
        assert_eq!(h.engine.state(), SessionState::Stopped);
    }

    #[test]
    fn hint_language_follows_question() {
        let mut h = Harness::new(no_early());
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        assert!(h.hints[0].1.system.contains("Write everything in English."));
        let mut h = Harness::new(EngineConfig {
            hint_language: HintLanguage::ZhCn,
            ..no_early()
        });
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        assert!(h.hints[0].1.system.contains("Simplified Chinese"));
    }
}
