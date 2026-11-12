//! The session state machine.
//!
//! `Engine` is synchronous and deterministic: it receives [`Input`]s with the
//! current time and returns [`Output`]s (events to emit, model requests to
//! start or cancel). The async driver in [`crate::backend`] performs the I/O.
//! This keeps every rule of the pipeline unit-testable without a network.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use meetcue_core::settings::{BudgetSettings, HintLanguage};
use meetcue_core::{
    codes, Channel, MeetCueError, MeetCueEvent, PointStatus, PriceEntry, QuestionBrief, QuestionId,
    QuestionStatus, RewriteStyle, SessionState, TranscriptLine, UsageKind, UsagePurpose,
    UserAction,
};

use crate::coverage::{self, AnswerText, PointVocab};
use crate::detect::{self, Detection};
use crate::hints::HintItem;
use crate::knowledge::{self, KnowledgeBase};
use crate::latency::LatencyMonitor;
use crate::prefilter;
use crate::prompts;
use crate::provider::{AsrResult, LlmRequest};
use crate::queue::{Admission, QuestionQueue};
use crate::text::{
    dominant_lang, edit_ratio, extends, join_parts, meaningful_len, truncate_chars, Lang,
};
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
/// Lines that set a question up ("如果研发和运维团队的职责重叠，" /
/// "你会如何调整？") are kept with it when they started at most this long
/// before it.
pub const LEAD_IN_MS: i64 = 30_000;
/// At most this many lines are kept as a question's set-up.
const MAX_LEAD_IN_LINES: usize = 5;
/// At most this many short pieces before a question are joined to it.
const MAX_FRAGMENT_LINES: usize = 2;
/// The user saying at least this many letters or characters counts as
/// answering; shorter sounds ("嗯", "OK") are back-channels.
const ANSWER_MIN_CHARS: usize = 4;
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
/// The hint prompt's cache entry lives 5 minutes from its last use; it is
/// refreshed a little before that while the conversation goes on.
pub const HINTS_WARM_MS: i64 = 270_000;
/// Echo filter tolerance between the two channels.
pub const ECHO_WINDOW_MS: i64 = 2_000;
/// A sentence with final text but no endpoint is closed after this long.
pub const STALE_UTTERANCE_MS: i64 = 2_000;
/// Minimum spacing of partial transcript events per channel.
const PARTIAL_EMIT_MS: i64 = 100;
const MAX_QUESTIONS: usize = 50;
/// A question still waiting its turn when a new one arrives this much later
/// is set aside: the conversation has moved on, and leaving it in line would
/// put every later question behind it.
pub const PENDING_STALE_MS: i64 = 120_000;
/// A hint request still running after this long is given up, so a reply that
/// never ends cannot hold a hint slot for the rest of the meeting.
pub const HINTS_GIVE_UP_MS: i64 = 120_000;
/// A sentence this close to one judged in the last [`REPEAT_WINDOW_MS`] is
/// not sent to detection again.
pub const REPEAT_MAX_CHANGE: f64 = 0.2;
pub const REPEAT_WINDOW_MS: i64 = 120_000;
/// Sentences shorter than this are cheap to check and often repeat by
/// chance ("好的。"), so they are always checked.
const REPEAT_MIN_CHARS: usize = 8;
/// The meeting memory lists at most this many earlier questions…
const MEMORY_ITEMS: usize = 12;
/// …in at most this many characters, keeping the newest.
const MEMORY_CHARS: usize = 3_000;
const MEMORY_QUESTION_CHARS: usize = 120;
const MEMORY_ANSWER_CHARS: usize = 300;

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
    /// Passages of the user's files, searched for each question.
    pub knowledge: Arc<KnowledgeBase>,
    /// The user's hint prompt templates; empty uses the built-in ones.
    pub hints_system: String,
    pub hints_user: String,
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
    /// Tell the hint model about earlier questions and the user's answers
    /// once they have left the recent transcript.
    pub meeting_memory: bool,
    /// Skip detection for sentences that repeat one judged just before.
    pub skip_repeated_sentences: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            detect: ModelRef::default(),
            hints: ModelRef::default(),
            role: String::new(),
            materials: String::new(),
            knowledge: Arc::default(),
            hints_system: String::new(),
            hints_user: String::new(),
            hint_language: HintLanguage::FollowQuestion,
            only_directed_to_me: false,
            queue_interruptions: true,
            track_coverage: false,
            early_trigger: true,
            speculative_hints: false,
            meeting_memory: false,
            skip_repeated_sentences: false,
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
    Detect {
        req: ReqId,
        request: LlmRequest,
    },
    Hints {
        req: ReqId,
        request: LlmRequest,
    },
    /// Write the hint prompt's cache without asking for hints, so the next
    /// question starts from the cache.
    WarmHints {
        request: LlmRequest,
    },
    Cancel {
        req: ReqId,
    },
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
    /// Who said it, from speaker labels.
    speaker: Option<u32>,
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
    /// When the first part of the sentence started.
    speech_start: i64,
    speech_end: i64,
    heard_at: i64,
    started_at: i64,
    early: bool,
    target: Target,
    /// More of the question arrived after detection started; detection only
    /// judged the first part.
    joined: bool,
    speaker: Option<u32>,
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
    /// What the speaker said to set the question up, kept in front of its
    /// text when later parts replace it.
    lead_in: String,
    /// The first words of the question said before a pause, part of
    /// `lead_in`, which its title must keep.
    opening: Vec<String>,
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
    /// What the user said while answering, as recognized.
    said: String,
    speaker: Option<u32>,
    /// How the next hint request should change the hints, once.
    rewrite: Option<RewriteStyle>,
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
    /// The sentence the hints answer.
    text: String,
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
    /// Detection replies that came back before the reply for a sentence said
    /// earlier. They wait for it, so questions are created in the order they
    /// were asked and the rest of a split sentence can merge into its start.
    held_detects: Vec<(ReqId, Result<String, MeetCueError>)>,
    /// Speculative hint requests, by the detection request they belong to.
    speculative: HashMap<ReqId, Speculation>,
    reqs: HashMap<ReqId, Req>,
    next_req: ReqId,
    questions: Vec<Question>,
    next_question: QuestionId,
    queue: QuestionQueue,
    hints_waiting: VecDeque<QuestionId>,
    hints_running: usize,
    /// When each running hint request (speculative or not) started.
    hints_started: HashMap<ReqId, i64>,
    hints_paused: bool,
    /// When the hint prompt last reached the provider (a hint request or a
    /// cache warm-up); `None` until the first one.
    hints_prefix_at: Option<i64>,
    meter: CostMeter,
    latency: LatencyMonitor,
    /// Speaker of the sentence being handled, for the question it may become.
    sentence_speaker: Option<u32>,
    /// Sentences detection judged recently, with when, to skip repeats.
    judged: VecDeque<(i64, String)>,
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
            held_detects: Vec::new(),
            speculative: HashMap::new(),
            reqs: HashMap::new(),
            next_req: 1,
            questions: Vec::new(),
            next_question: 1,
            queue: QuestionQueue::default(),
            hints_waiting: VecDeque::new(),
            hints_running: 0,
            hints_started: HashMap::new(),
            hints_paused: false,
            hints_prefix_at: None,
            meter,
            latency: LatencyMonitor::default(),
            sentence_speaker: None,
            judged: VecDeque::new(),
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
        let old_system = self.hints_system(Lang::En.prompt_name());
        let old_model = self.cfg.hints.clone();
        self.cfg = cfg;
        if self.hints_system(Lang::En.prompt_name()) != old_system || self.cfg.hints != old_model {
            // A new prompt or model starts a new cache entry: warm it again.
            self.hints_prefix_at = None;
        }
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
        self.held_detects.clear();
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
            AsrResult::Speaker(n) => {
                u.speaker.get_or_insert(n);
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
        let speaker = u.speaker;
        *u = Utterance {
            id: next_id,
            ..Default::default()
        };
        self.next_utterance += 1;
        match ch {
            Channel::Remote => {
                self.transcript.push(line.clone());
                out.push(Output::Emit(transcript_final(&line, utterance)));
                self.sentence_speaker = speaker;
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
        self.sentence_speaker = None;
        if known_question {
            let det = Detection {
                is_question: true,
                directed_to_me: true,
                short: String::new(),
                ..Default::default()
            };
            let cand = Candidate {
                text: line.text.clone(),
                speech_start: t_ms,
                speech_end: t_ms,
                heard_at: now,
                started_at: now,
                early: false,
                target: Target::New,
                joined: false,
                speaker: None,
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
                        let text = self.with_lead_in(id, &line.text);
                        self.extend_question(id, text, line.t1_ms, now, out)
                    }
                    EarlyState::Dropped => {
                        if self.worth_checking(&line.text, now) {
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
                    // A few words corrected don't change what the hints answer.
                    if let Some(spec) = self.speculative.get_mut(&req) {
                        spec.text = line.text.clone();
                    }
                    return;
                }
                (EarlyState::Created(id), false) => {
                    if let Some(q) = self.question_mut(id) {
                        q.text = join_parts(&q.lead_in, &line.text);
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
        // The model decides by meaning whether an answer is expected, so
        // requests without a question word or mark are not missed.
        if self.worth_checking(&line.text, now) {
            self.start_detect(&line, now, false, Target::New, out);
        }
    }

    /// Whether a sentence needs the detection model: not a back-channel,
    /// and, when repeats are skipped, not a near copy of a sentence judged
    /// in the last two minutes.
    fn worth_checking(&self, text: &str, now: i64) -> bool {
        if !prefilter::worth_checking(text) {
            return false;
        }
        if !self.cfg.skip_repeated_sentences || meaningful_len(text) < REPEAT_MIN_CHARS {
            return true;
        }
        let repeat = self
            .judged
            .iter()
            .filter(|(at, _)| now - at <= REPEAT_WINDOW_MS)
            .any(|(_, t)| edit_ratio(t, text) <= REPEAT_MAX_CHANGE);
        if repeat {
            tracing::debug!("skipped detection for a repeated sentence");
        }
        !repeat
    }

    /// Remembers a sentence detection judged, for [`Self::worth_checking`].
    fn note_judged(&mut self, text: &str, now: i64) {
        while self
            .judged
            .front()
            .is_some_and(|(at, _)| now - at > REPEAT_WINDOW_MS)
        {
            self.judged.pop_front();
        }
        self.judged.push_back((now, text.to_string()));
    }

    /// The open question, or the sentence still being checked, that `line`
    /// carries on, if it starts within [`CONTINUATION_MS`] and is joined to
    /// it by its wording, or simply finishes it with nobody answering in
    /// between (see [`prefilter::finishes`]).
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
        let (end, join) = match (cand, question) {
            (Some(c), Some(q)) => Some(if c.0 >= q.0 { c } else { q }),
            (c, q) => c.or(q),
        }?;
        let prev = match &join {
            Join::Candidate(_, t) | Join::Question(_, t) => t,
        };
        let joins = prefilter::continues(prev, &line.text)
            || (prefilter::finishes(prev, &line.text) && !self.me_answered(end, line.t0_ms));
        joins.then_some(join)
    }

    /// Whether the user said something more than a back-channel between
    /// `from` and `to`, or is saying it now.
    fn me_answered(&self, from: i64, to: i64) -> bool {
        let speaking = &self.utterances[idx(Channel::Me)];
        meaningful_len(&speaking.text()) >= ANSWER_MIN_CHARS
            || self.transcript.since(from).any(|l| {
                l.channel == Channel::Me
                    && l.t0_ms <= to
                    && meaningful_len(&l.text) >= ANSWER_MIN_CHARS
            })
    }

    /// The lines the speaker said just before the sentence starting at
    /// `start` to set it up, oldest first: up to `count` of them, as long as
    /// they follow each other closely, nobody answered in between and none
    /// belongs to an earlier question.
    fn lead_in(&self, start: i64, count: usize) -> Vec<String> {
        let count = count.min(MAX_LEAD_IN_LINES);
        if count == 0 {
            return Vec::new();
        }
        let floor = self.questions.last().map(|q| q.last_part_end);
        let mut parts = Vec::new();
        let mut next_start = start;
        for l in self
            .transcript
            .since(start - LEAD_IN_MS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if l.t0_ms >= start {
                continue;
            }
            if l.channel == Channel::Me {
                if meaningful_len(&l.text) >= ANSWER_MIN_CHARS {
                    break;
                }
                continue;
            }
            if floor.is_some_and(|f| l.t1_ms <= f)
                || next_start - l.t1_ms > CONTINUATION_MS
                || start - l.t0_ms > LEAD_IN_MS
            {
                break;
            }
            parts.push(l.text.clone());
            next_start = l.t0_ms;
            if parts.len() == count {
                break;
            }
        }
        parts.reverse();
        parts
    }

    /// How many of the lines just before the sentence starting at `start`
    /// are the first words of it, cut off by a pause ("上任后。" before
    /// "一年的计划是什么？"). These belong to the question whatever the model
    /// counted as its set-up: they are too short to be checked on their own
    /// and too short to mean anything without the rest. Walks back the same
    /// way as [`Self::lead_in`].
    fn fragments_before(&self, start: i64) -> usize {
        let floor = self.questions.last().map(|q| q.last_part_end);
        let mut count = 0;
        let mut next_start = start;
        for l in self
            .transcript
            .since(start - LEAD_IN_MS)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if l.t0_ms >= start {
                continue;
            }
            if l.channel == Channel::Me {
                if meaningful_len(&l.text) >= ANSWER_MIN_CHARS {
                    break;
                }
                continue;
            }
            if floor.is_some_and(|f| l.t1_ms <= f)
                || next_start - l.t1_ms > CONTINUATION_MS
                || !prefilter::is_fragment(&l.text)
            {
                break;
            }
            count += 1;
            next_start = l.t0_ms;
            if count == MAX_FRAGMENT_LINES {
                break;
            }
        }
        count
    }

    /// `text` with the set-up of question `id` in front of it.
    fn with_lead_in(&self, id: QuestionId, text: &str) -> String {
        match self.question(id) {
            Some(q) => join_parts(&q.lead_in, text),
            None => text.to_string(),
        }
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
        let Some(c) = self.candidates.get_mut(&req) else {
            return;
        };
        c.text = text.clone();
        c.speech_end = c.speech_end.max(speech_end);
        c.joined = true;
        let target = c.target;
        self.drop_speculation(req, now, out);
        // Start on the whole question now rather than once detection is
        // back, so joining the rest costs no extra round trip.
        if target == Target::New && prefilter::might_be_question(&text) {
            let lang = self.output_language(&text);
            self.speculate(req, &text, lang, now, out);
        }
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
        // The title was written for the first part alone.
        q.short = whole_title(q);
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
            q.said = join_parts(&q.said, &line.text);
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
                speech_start: line.t0_ms,
                speech_end: line.t1_ms,
                heard_at: now,
                started_at: now,
                early,
                target,
                joined: false,
                speaker: self.sentence_speaker,
            },
        );
        out.push(Output::Detect { req, request });
        // A sentence sent only because it may finish the previous question
        // gets no hints of its own.
        if target == Target::New && prefilter::might_be_question(&line.text) {
            self.speculate(req, &line.text, lang, now, out);
        }
        req
    }

    /// Starts hints for `text` alongside detection request `detect`, when
    /// speculative hints are on and a slot is free.
    fn speculate(
        &mut self,
        detect: ReqId,
        text: &str,
        lang: &str,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        if !self.cfg.speculative_hints
            || self.hints_paused
            || self.hints_running >= MAX_CONCURRENT_HINTS
        {
            return;
        }
        let request = self.hints_request(text, lang, now);
        let spec = self.alloc_req(Req::Speculative { detect });
        self.hints_running += 1;
        self.hints_started.insert(spec, now);
        self.speculative.insert(
            detect,
            Speculation {
                req: spec,
                text: text.to_string(),
                started_at: now,
                items: Vec::new(),
                finished: None,
            },
        );
        self.hints_prefix_at = Some(now);
        out.push(Output::Hints { req: spec, request });
    }

    fn on_detect_done(
        &mut self,
        req: ReqId,
        reply: Result<String, MeetCueError>,
        now: i64,
        out: &mut Vec<Output>,
    ) {
        if self.earlier_detection_pending(req) {
            self.reqs.remove(&req);
            self.held_detects.push((req, reply));
            return;
        }
        self.apply_detect_done(req, reply, now, out);
        self.release_held_detects(now, out);
    }

    /// Whether a new sentence said before the one `req` checks is still
    /// waiting for its detection reply.
    fn earlier_detection_pending(&self, req: ReqId) -> bool {
        let Some(c) = self
            .candidates
            .get(&req)
            .filter(|c| c.target == Target::New)
        else {
            return false;
        };
        self.candidates
            .iter()
            .any(|(r, o)| *r != req && o.target == Target::New && o.speech_end < c.speech_end)
    }

    /// Applies held detection replies, oldest sentence first, as far as no
    /// earlier sentence is still being checked.
    fn release_held_detects(&mut self, now: i64, out: &mut Vec<Output>) {
        loop {
            let next = self
                .held_detects
                .iter()
                .enumerate()
                .min_by_key(|(_, (r, _))| {
                    self.candidates
                        .get(r)
                        .map(|c| c.speech_end)
                        .unwrap_or(i64::MIN)
                })
                .map(|(i, (r, _))| (i, *r));
            let Some((i, req)) = next else {
                return;
            };
            if self.earlier_detection_pending(req) {
                return;
            }
            let (req, reply) = self.held_detects.remove(i);
            self.apply_detect_done(req, reply, now, out);
        }
    }

    fn apply_detect_done(
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
        // Only a question still waiting for its answer can be continued: once
        // the user has answered, a new question that the model read as more
        // of the old one starts a card of its own.
        let mut det = det;
        self.note_judged(&cand.text, now);
        if det.merged && cand.target == Target::New {
            let still_open = self
                .last_open_question()
                .and_then(|id| self.question(id))
                .is_some_and(|q| !self.me_answered(q.last_part_end, cand.speech_start));
            if !still_open {
                det.merged = false;
            }
        }
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
        let usable = !det.merged && spec.as_ref().is_some_and(|s| s.text == cand.text);
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
                    q.text = join_parts(&q.text, &cand.text);
                    // The model titles the whole question when it merges a
                    // part; without a title, the old one covers only the start.
                    q.short = if det.short.is_empty() {
                        whole_title(q)
                    } else {
                        detect::title_with_fragments(&short, &q.opening)
                    };
                } else {
                    q.text = join_parts(&q.lead_in, &cand.text);
                    q.short = detect::title_with_fragments(&short, &q.opening);
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

        self.set_aside_stale(cand.speech_end, out);
        let id = self.next_question;
        self.next_question += 1;
        let me_busy = !self.utterances[idx(Channel::Me)].is_empty()
            || now - self.last_me_activity < INTERRUPTION_WINDOW_MS;
        let interruption = me_busy || (det.interruption && now - self.last_me_activity < 5_000);
        let active_spoken = self
            .queue
            .active()
            .and_then(|a| self.question(a))
            .map(|q| q.user_spoke);
        let admission = if interruption && !self.cfg.queue_interruptions {
            Admission::Preempt
        } else if active_spoken == Some(true) && !interruption {
            Admission::AfterAnswer
        } else if active_spoken == Some(false) && !interruption {
            // The user never started on the active question, so the one just
            // asked is the one to answer; it must not wait behind it.
            Admission::Supersede
        } else {
            Admission::Queue
        };
        let context = self.transcript.last_before(cand.speech_end + 1, 4);
        // The set-up the speaker gave before asking belongs on the card too.
        // So do the first words of it said before a pause, which the model
        // may not count as set-up.
        let fragments = self.fragments_before(cand.speech_start);
        let parts = self.lead_in(cand.speech_start, (det.lead_in as usize).max(fragments));
        let opening = parts[parts.len() - fragments.min(parts.len())..].to_vec();
        let short = detect::title_with_fragments(&short, &opening);
        let lead_in = parts
            .into_iter()
            .fold(String::new(), |acc, l| join_parts(&acc, &l));
        self.questions.push(Question {
            id,
            version: 1,
            text: join_parts(&lead_in, &cand.text),
            short,
            tags: det.tags,
            subs,
            t_ms: cand.speech_end,
            lead_in,
            opening,
            last_part_end: cand.speech_end,
            context,
            vocab: Vec::new(),
            statuses: Vec::new(),
            hints: HintsState::Idle,
            answer: AnswerText::default(),
            manual: HashMap::new(),
            user_spoke: false,
            said: String::new(),
            speaker: cand.speaker,
            rewrite: None,
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
        let Some(q) = self.question_mut(id) else {
            return;
        };
        let rewrite = q.rewrite.take();
        let version = q.version;
        let mut question = q.text.clone();
        if q.subs.len() > 1 {
            for (i, s) in q.subs.iter().enumerate() {
                question.push_str(&format!("\n  {}. {}", i + 1, s));
            }
        }
        let text = q.text.clone();
        if let Some(style) = rewrite {
            question.push_str("\n\n");
            question.push_str(rewrite_note(style));
        }
        let lang = self.output_language(&text);
        let request = self.hints_request(&question, lang, now);
        let req = self.alloc_req(Req::Hints { id, version });
        self.hints_running += 1;
        self.hints_started.insert(req, now);
        if let Some(q) = self.question_mut(id) {
            q.hints = HintsState::Running(req);
            q.latency.hint_start = now;
        }
        self.hints_prefix_at = Some(now);
        out.push(Output::Hints { req, request });
    }

    /// The hint instructions: the stable, cached part of every hint request.
    /// `lang` only matters to custom instructions that still name
    /// `{output_language}`; the built-in ones leave it to the question message
    /// so one cache entry serves questions in every language.
    fn hints_system(&self, lang: &str) -> String {
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
        prompts::render(
            template(&self.cfg.hints_system, prompts::HINTS_SYSTEM),
            &[
                ("role", &role),
                ("materials", &materials),
                ("output_language", lang),
            ],
        )
    }

    /// The hint generation request for a question (with its numbered parts).
    fn hints_request(&self, question: &str, lang: &str, now: i64) -> LlmRequest {
        let knowledge = knowledge::render(
            &self
                .cfg
                .knowledge
                .search(question, knowledge::RETRIEVED_CHARS),
        );
        let knowledge = if knowledge.is_empty() {
            "(none)".to_string()
        } else {
            knowledge
        };
        let system_template = template(&self.cfg.hints_system, prompts::HINTS_SYSTEM);
        let user_template = template(&self.cfg.hints_user, prompts::HINTS_USER);
        let transcript = format_for_prompt(self.transcript.since(now - HINTS_WINDOW_MS));
        let memory = self.memory(now);
        let mut user = prompts::render(
            user_template,
            &[
                ("knowledge", &knowledge),
                ("memory", if memory.is_empty() { "(none)" } else { &memory }),
                ("transcript", transcript.trim_end()),
                ("question", question),
                ("output_language", lang),
            ],
        );
        if !memory.is_empty() && !user_template.contains("{memory}") {
            // A custom question message from before the memory existed.
            user = format!("{MEMORY_HEADING}\n{memory}\n\n{user}");
        }
        if !system_template.contains("{output_language}")
            && !user_template.contains("{output_language}")
        {
            // A custom question message written before the language moved
            // there: keep telling the model which language to write in.
            user.push_str(&format!("\n\nWrite everything in {lang}."));
        }
        LlmRequest::new(&self.cfg.hints.model, UsagePurpose::Hints)
            .system(self.hints_system(lang), true)
            .user(user)
            .max_tokens(prompts::HINTS_MAX_TOKENS)
            .temperature(0.3)
    }

    /// Earlier questions and what the user answered, one line each, for
    /// questions that have left the transcript the hint model sees. Empty
    /// when the memory is off or there is nothing to recall.
    fn memory(&self, now: i64) -> String {
        if !self.cfg.meeting_memory {
            return String::new();
        }
        let cutoff = now - HINTS_WINDOW_MS;
        let mut lines: Vec<String> = self
            .questions
            .iter()
            .filter(|q| q.last_part_end < cutoff && !q.said.trim().is_empty())
            .rev()
            .take(MEMORY_ITEMS)
            .map(|q| {
                let asked = if q.short.trim().is_empty() {
                    &q.text
                } else {
                    &q.short
                };
                format!(
                    "- Q: {}\n  A: {}",
                    truncate_chars(asked.trim(), MEMORY_QUESTION_CHARS),
                    truncate_chars(q.said.trim(), MEMORY_ANSWER_CHARS)
                )
            })
            .collect();
        let mut total = 0;
        lines.retain(|l| {
            total += l.chars().count() + 1;
            total <= MEMORY_CHARS
        });
        lines.reverse();
        lines.join("\n")
    }

    /// Keeps the hint prompt in the provider's cache: once when the session
    /// starts, and again before the entry expires while people are talking,
    /// so the next question's hints don't wait for the prompt to be read.
    fn warm_hints(&mut self, now: i64, out: &mut Vec<Output>) {
        if self.state != SessionState::Running || self.hints_paused {
            return;
        }
        let due = match self.hints_prefix_at {
            None => true,
            Some(at) => {
                now - at >= HINTS_WARM_MS
                    && self.transcript.since(now - HINTS_WARM_MS).next().is_some()
            }
        };
        if !due {
            return;
        }
        self.hints_prefix_at = Some(now);
        let lang = self.output_language("");
        let request = self.hints_request("", lang, now);
        out.push(Output::WarmHints { request });
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
        let was_candidate = self.candidates.remove(&req).is_some();
        self.held_detects.retain(|(r, _)| *r != req);
        if self.reqs.remove(&req).is_some() {
            out.push(Output::Cancel { req });
        }
        self.drop_speculation(req, now, out);
        if was_candidate {
            self.release_held_detects(now, out);
        }
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

    /// Takes questions that have waited since long before `now` out of the
    /// line, so a question asked now is not shown behind them.
    fn set_aside_stale(&mut self, now: i64, out: &mut Vec<Output>) {
        let stale: Vec<QuestionId> = self
            .queue
            .items()
            .iter()
            .filter(|(_, s)| *s == QuestionStatus::Pending)
            .filter_map(|(id, _)| self.question(*id))
            .filter(|q| now - q.last_part_end > PENDING_STALE_MS)
            .map(|q| q.id)
            .collect();
        if stale.is_empty() {
            return;
        }
        tracing::info!("set aside {} questions that waited too long", stale.len());
        let changes = self.queue.set_aside(&stale);
        self.apply_status_changes(changes, out);
    }

    /// Ends hint requests that have run far longer than any reply takes. The
    /// driver times out stalled replies itself; this keeps the slots free
    /// even if a request is never reported back.
    fn give_up_slow_hints(&mut self, now: i64, out: &mut Vec<Output>) {
        let reqs = &self.reqs;
        self.hints_started.retain(|r, _| reqs.contains_key(r));
        let slow: Vec<ReqId> = self
            .hints_started
            .iter()
            .filter(|(_, started)| now - **started > HINTS_GIVE_UP_MS)
            .map(|(r, _)| *r)
            .collect();
        for req in slow {
            self.hints_started.remove(&req);
            tracing::warn!("hint request {req} gave no result in time; giving up");
            out.push(Output::Cancel { req });
            if self.speculation_ended(req, false, now, out) {
                continue;
            }
            let error = MeetCueError::new(codes::LLM_TIMEOUT, "hint reply took too long");
            self.on_hints_failed(req, error, now, out);
        }
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
            speaker: q.speaker,
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
            UserAction::Rewrite { id, style } => {
                let Some(q) = self.question_mut(id) else {
                    return;
                };
                q.version += 1;
                q.rewrite = Some(style);
                q.vocab.clear();
                q.statuses.clear();
                q.manual.clear();
                q.latency = Latency {
                    speech_end: now,
                    ..Default::default()
                };
                self.cancel_hints(id, out);
                if let Some(ev) = self.question_event(id) {
                    out.push(Output::Emit(ev));
                }
                self.request_hints(id, now, out);
            }
            UserAction::Ask { text } => {
                let text = text.trim().to_string();
                if text.is_empty() || self.state == SessionState::Idle {
                    return;
                }
                let det = Detection {
                    is_question: true,
                    directed_to_me: true,
                    ..Default::default()
                };
                let cand = Candidate {
                    text,
                    speech_start: now,
                    speech_end: now,
                    heard_at: now,
                    started_at: now,
                    early: false,
                    target: Target::New,
                    joined: false,
                    speaker: None,
                };
                self.accept_detection(cand, det, None, now, out);
            }
        }
    }

    fn on_tick(&mut self, now: i64, out: &mut Vec<Output>) {
        self.give_up_slow_hints(now, out);
        self.warm_hints(now, out);

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
            if prefilter::might_be_question(&text)
                && self.worth_checking(&text, now)
                && self.continuation_of(&line).is_none()
            {
                let utterance = remote.id;
                self.sentence_speaker = remote.speaker;
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
/// A title in the speaker's words for a question whose text grew after its
/// title was written: what was asked after the set-up, with the first words
/// said before a pause.
fn whole_title(q: &Question) -> String {
    let asked = q.text.strip_prefix(q.lead_in.as_str()).unwrap_or(&q.text);
    detect::title_with_fragments(&detect::verbatim_title(asked), &q.opening)
}

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

/// Heading of the meeting memory when a custom question message has no
/// `{memory}` placeholder.
const MEMORY_HEADING: &str =
    "Earlier in this meeting (questions and what the user answered, oldest first):";

/// What a rewrite asks of the hint model, added after the question.
fn rewrite_note(style: RewriteStyle) -> &'static str {
    match style {
        RewriteStyle::Shorter => "(The user asked for a shorter version: at most three points, each one short spoken sentence, and a sample answer of two or three sentences.)",
        RewriteStyle::Longer => "(The user asked for more detail: five points with specifics such as numbers, tools or an example, and a fuller sample answer.)",
        RewriteStyle::Again => "(The user asked for a different take: write a fresh set of points from another angle than an obvious first answer.)",
    }
}

/// The user's prompt template, or the built-in one when theirs is empty.
fn template<'a>(custom: &'a str, default: &'a str) -> &'a str {
    if custom.trim().is_empty() {
        default
    } else {
        custom
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
        warms: Vec<LlmRequest>,
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
                warms: vec![],
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
                    Output::WarmHints { request } => self.warms.push(request),
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
            track_coverage: true,
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
            .contains("[Them] 你们是怎么做高可用的？\n\nOutput language:"));
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
    fn points_stay_unticked_when_tracking_is_off() {
        let mut h = Harness::new(EngineConfig {
            track_coverage: false,
            ..no_early()
        });
        let req = asked_and_active(&mut h);
        h.at(3700)
            .hint_points(req, &[("双活", "两地 分担"), ("冗余", "堆叠 双电源")]);
        h.say(Channel::Me, 4000, 5000, "我们做了两地双活");
        assert_eq!(h.last_coverage(1), None, "speech must not tick points");
        // Ticking by hand still works.
        h.input(Input::Action(UserAction::TogglePoint { id: 1, index: 0 }));
        assert_eq!(
            h.last_coverage(1),
            Some(vec![PointStatus::Done, PointStatus::Next])
        );
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
    fn back_channels_and_my_own_speech_never_reach_the_model() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 2000, 2500, "对吧？");
        h.say(Channel::Remote, 3000, 3300, "嗯");
        h.say(Channel::Me, 4000, 5000, "你能听到我吗？");
        assert!(h.detects.is_empty());
    }

    #[test]
    fn the_model_judges_requests_without_question_words() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 2000, "说一下你对微服务的理解");
        assert_eq!(h.detects.len(), 1, "no question word or mark");
        h.detect_reply(YES);
        assert_eq!(question_texts(&h).len(), 1);
        // A plain statement is also sent, and the model turns it down.
        h.say(Channel::Remote, 20_000, 21_000, "好的，我们开始吧。");
        assert_eq!(h.detects.len(), 2);
        h.detect_reply(r#"{"isQuestion":false}"#);
        assert_eq!(question_texts(&h).len(), 1);
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

    /// Asks a question while the user is mid-sentence, so it waits its turn.
    fn interrupt(h: &mut Harness, t: i64, text: &str) {
        h.at(t).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Partial {
                text: "我觉得".into(),
                t0_ms: t,
                t1_ms: t,
            },
        });
        h.say(Channel::Remote, t + 100, t + 800, text);
        h.detect_reply(YES);
        h.input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Final {
                text: "我觉得可以".into(),
                t0_ms: t,
                t1_ms: t + 1500,
            },
        });
        h.at(t + 1600).input(Input::Asr {
            channel: Channel::Me,
            result: AsrResult::Endpoint { t_ms: t + 1500 },
        });
    }

    #[test]
    fn questions_that_waited_too_long_do_not_hold_up_new_ones() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "第一个问题是什么？");
        h.detect_reply(YES);
        interrupt(&mut h, 2000, "等等，这个会不会有风险？");
        assert_eq!(h.engine.queue.status(2), Some(QuestionStatus::Pending));
        // Minutes later the other side moves on to a new question. The one
        // that has waited since is set aside, and the new one is shown.
        h.say(
            Channel::Remote,
            200_000,
            201_000,
            "下一个问题，你怎么排期？",
        );
        h.detect_reply(YES);
        assert_eq!(h.engine.queue.status(2), Some(QuestionStatus::Answered));
        assert_eq!(h.engine.queue.active(), Some(3));
        assert_eq!(h.engine.queue.pending_count(), 0);
    }

    #[test]
    fn a_hint_request_that_never_ends_frees_its_slot() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "第一个问题是什么？");
        h.detect_reply(YES);
        h.say(Channel::Remote, 3000, 4000, "第二个问题是什么？");
        h.detect_reply(YES);
        h.say(Channel::Remote, 6000, 7000, "第三个问题是什么？");
        h.detect_reply(YES);
        // Both slots are taken by replies that never end; the third waits.
        assert_eq!(h.hints.len(), 2);
        let (first, second) = (h.hints[0].0, h.hints[1].0);
        h.at(10_000 + HINTS_GIVE_UP_MS);
        assert!(h.cancelled.contains(&first));
        assert!(h.cancelled.contains(&second));
        assert_eq!(h.hints.len(), 3, "the waiting question gets its hints");
        assert!(h.events.iter().any(|e| matches!(
            e,
            MeetCueEvent::HintsFailed { code, .. } if code == codes::LLM_TIMEOUT
        )));
        let third = h.hints[2].0;
        h.hint_points(third, &[("要点", "细节")]);
        assert!(h
            .events
            .iter()
            .any(|e| matches!(e, MeetCueEvent::AnswerDone { id: 3, .. })));
    }

    #[test]
    fn question_asked_before_the_user_starts_takes_over() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 1000, "第一个问题是什么？");
        h.detect_reply(YES);
        // The other side asks again before the user says anything.
        h.say(Channel::Remote, 3000, 4000, "换个问题，你怎么排期？");
        h.detect_reply(YES);
        assert_eq!(
            h.statuses()[1..].to_vec(),
            vec![(1, QuestionStatus::Answered), (2, QuestionStatus::Active)]
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
            .contains("[Them] 控制器挂了还能转发吗\n\nOutput language:"));
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

    fn titles(h: &Harness) -> Vec<(QuestionId, u32, String)> {
        h.events
            .iter()
            .filter_map(|e| match e {
                MeetCueEvent::QuestionNew {
                    id, version, short, ..
                } => Some((*id, *version, short.clone())),
                _ => None,
            })
            .collect()
    }

    /// "上任后。" said before a pause is too short to be checked on its own;
    /// it stays in front of the question, in its text and in its title, also
    /// when the rest of the question arrives after the early trigger.
    #[test]
    fn first_words_before_a_pause_stay_with_the_question() {
        let mut h = Harness::new(EngineConfig {
            speculative_hints: true,
            ..Default::default()
        });
        h.say(Channel::Remote, 0, 600, "上任后。");
        assert!(h.detects.is_empty(), "too short to check on its own");
        h.at(1500).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Partial {
                text: "一年的计划是什么".into(),
                t0_ms: 1000,
                t1_ms: 1500,
            },
        });
        h.at(2000);
        assert_eq!(h.detects.len(), 1, "early trigger");
        // The model neither counts the piece as set-up nor titles it.
        h.detect_reply(
            r#"{"isQuestion":true,"directedToMe":true,"leadIn":0,"short":"一年的计划"}"#,
        );
        assert_eq!(
            texts(&h).last(),
            Some(&(1, 1, "上任后。一年的计划是什么".into()))
        );
        assert_eq!(titles(&h).last(), Some(&(1, 1, "上任后一年的计划".into())));
        // The speaker goes on after a short pause.
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Final {
                text: "一年的计划是什么，分几步走？".into(),
                t0_ms: 1000,
                t1_ms: 3500,
            },
        });
        h.at(3600).input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Endpoint { t_ms: 3500 },
        });
        assert_eq!(
            texts(&h).last(),
            Some(&(1, 2, "上任后。一年的计划是什么，分几步走？".into()))
        );
        assert_eq!(
            titles(&h).last(),
            Some(&(1, 2, "上任后一年的计划是什么，分几步走".into()))
        );
        assert!(h.hints.last().unwrap().1.messages[0]
            .content
            .contains("上任后。一年的计划是什么，分几步走？"));
    }

    #[test]
    fn acknowledgements_before_a_question_are_left_out() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 800, "好的，谢谢。");
        h.say(Channel::Remote, 1200, 2000, "我们开始吧。");
        h.detect_reply(r#"{"isQuestion":false}"#);
        h.say(Channel::Remote, 2500, 4500, "你为什么离开上一家公司？");
        h.detect_reply(r#"{"isQuestion":true,"directedToMe":true,"short":"离职原因"}"#);
        assert_eq!(
            texts(&h).last(),
            Some(&(1, 1, "你为什么离开上一家公司？".into()))
        );
        assert_eq!(titles(&h).last(), Some(&(1, 1, "离职原因".into())));
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
        // Much later the model sees the same kind of fragment on its own
        // and turns it down as a remark.
        h.say(Channel::Remote, 20_000, 20_800, "Idea C, too.");
        assert_eq!(h.detects.len(), 3);
        assert_eq!(h.hints.len(), 2, "no speculative hints for a fragment");
        h.detect_reply(r#"{"isQuestion":false,"directedToMe":true,"merged":false}"#);
        assert_eq!(texts(&h).last().map(|t| (t.0, t.1)), Some((1, 2)));
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
        let focus = |h: &Harness| {
            h.events.iter().rev().find_map(|e| match e {
                MeetCueEvent::QueueChanged { focus, active, .. } => Some((*focus, *active)),
                _ => None,
            })
        };
        // The newest question is shown while the first one is still active.
        assert_eq!(focus(&h), Some((Some(2), Some(1))));
        h.input(Input::Action(UserAction::FocusStep {
            target: FocusTarget::Prev,
        }));
        assert_eq!(focus(&h), Some((Some(1), Some(1))));
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
    fn hints_use_knowledge_and_custom_prompts() {
        use meetcue_core::settings::Material;
        let notes = [
            "Built a BGP EVPN fabric for three data centers; failover under one second.",
            "Mentored four junior engineers and ran the weekly design review.",
        ]
        .map(|p| format!("{p} {}", "filler ".repeat(500)))
        .join("\n\n");
        let (materials, kb) = knowledge::prepare(&[Material {
            name: "cv.md".into(),
            path: "cv.md".into(),
            text: notes,
        }]);
        let mut h = Harness::new(EngineConfig {
            materials,
            knowledge: Arc::new(kb),
            ..no_early()
        });
        h.input(Input::RemoteText {
            text: "How do you handle data center failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        let user = &h.hints[0].1.messages[0].content;
        assert!(user.contains("[cv.md]\nBuilt a BGP EVPN fabric"), "{user}");
        assert!(!user.contains("Mentored"));

        let mut h = Harness::new(EngineConfig {
            hints_system: "Coach for {role}.".into(),
            hints_user: "Q: {question}\nNotes: {knowledge}".into(),
            role: "SRE".into(),
            ..no_early()
        });
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        assert_eq!(h.hints[0].1.system, "Coach for SRE.");
        assert_eq!(
            h.hints[0].1.messages[0].content,
            "Q: How do you handle failover?\nNotes: (none)\n\nWrite everything in English."
        );
    }

    #[test]
    fn hint_language_follows_question() {
        let mut h = Harness::new(no_early());
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        let en = h.hints[0].1.clone();
        assert!(en.messages[0]
            .content
            .trim_end()
            .ends_with("Write everything in English."));
        let mut h = Harness::new(EngineConfig {
            hint_language: HintLanguage::ZhCn,
            ..no_early()
        });
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: 0,
            known_question: true,
        });
        let zh = &h.hints[0].1;
        assert!(zh.messages[0].content.contains("Simplified Chinese"));
        // One cached system prompt serves both languages.
        assert_eq!(zh.system, en.system);
    }

    #[test]
    fn hint_prompt_is_warmed_at_start_and_kept_warm_while_people_talk() {
        let mut h = Harness::new(no_early());
        h.at(100);
        assert_eq!(h.warms.len(), 1, "warmed when the session starts");
        assert!(h.warms[0].cache_system);
        h.at(HINTS_WARM_MS - 1_000);
        assert_eq!(h.warms.len(), 1);
        // A question uses the prompt, which keeps the entry alive by itself.
        h.input(Input::RemoteText {
            text: "How do you handle failover?".into(),
            t_ms: HINTS_WARM_MS - 1_000,
            known_question: true,
        });
        assert_eq!(h.warms[0].system, h.hints[0].1.system);
        h.at(HINTS_WARM_MS + 1_000);
        assert_eq!(h.warms.len(), 1, "the question refreshed the cache");
        h.at(2 * HINTS_WARM_MS);
        assert_eq!(h.warms.len(), 2, "refreshed while the conversation goes on");
        // Nobody has spoken for a while: let the entry expire.
        h.at(5 * HINTS_WARM_MS);
        assert_eq!(h.warms.len(), 2);
    }

    fn question_texts(h: &Harness) -> Vec<(QuestionId, i64, String)> {
        let mut qs = std::collections::BTreeMap::new();
        for e in &h.events {
            if let MeetCueEvent::QuestionNew { id, t_ms, text, .. } = e {
                qs.insert(*id, (*t_ms, text.clone()));
            }
        }
        qs.into_iter()
            .map(|(id, (t, text))| (id, t, text))
            .collect()
    }

    #[test]
    fn questions_keep_the_order_they_were_asked_in() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 1000, 3000, "What is a hash map?");
        h.say(
            Channel::Remote,
            3600,
            6000,
            "How do you handle on-call incidents?",
        );
        assert_eq!(h.detects.len(), 2, "both sentences are checked at once");
        let (first, _) = h.detects[0].clone();
        let (second, _) = h.detects[1].clone();
        // The reply for the later sentence comes back first.
        h.at(6500).input(Input::DetectDone {
            req: second,
            reply: Ok(YES.into()),
        });
        assert!(
            question_texts(&h).is_empty(),
            "it waits for the earlier one"
        );
        h.at(7000).input(Input::DetectDone {
            req: first,
            reply: Ok(YES.into()),
        });
        assert_eq!(
            question_texts(&h),
            vec![
                (1, 3000, "What is a hash map?".to_string()),
                (2, 6000, "How do you handle on-call incidents?".to_string()),
            ]
        );
    }

    #[test]
    fn rest_of_a_split_question_merges_while_its_start_is_checked() {
        const MERGED: &str = r#"{"isQuestion":true,"directedToMe":true,"merged":true,"short":"Q","subQuestions":[],"tags":[]}"#;
        let mut h = Harness::new(no_early());
        h.say(
            Channel::Remote,
            1000,
            3000,
            "How would you design a zero trust network?",
        );
        h.say(
            Channel::Remote,
            3800,
            6000,
            "For a remote team of fifty people?",
        );
        assert_eq!(h.detects.len(), 2);
        let (first, _) = h.detects[0].clone();
        let (second, _) = h.detects[1].clone();
        h.input(Input::DetectDone {
            req: second,
            reply: Ok(MERGED.into()),
        });
        h.input(Input::DetectDone {
            req: first,
            reply: Ok(YES.into()),
        });
        assert_eq!(
            question_texts(&h),
            vec![(
                1,
                3000,
                "How would you design a zero trust network? For a remote team of fifty people?"
                    .to_string()
            )]
        );
    }

    #[test]
    fn a_held_reply_is_applied_once_the_earlier_check_is_cancelled() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 1000, 3000, "What is a hash map?");
        h.say(
            Channel::Remote,
            3600,
            6000,
            "How do you handle on-call incidents?",
        );
        let (first, _) = h.detects[0].clone();
        let (second, _) = h.detects[1].clone();
        h.input(Input::DetectDone {
            req: second,
            reply: Ok(YES.into()),
        });
        let mut out = Vec::new();
        let now = h.now;
        h.engine.cancel_req(first, now, &mut out);
        h.absorb(out);
        assert_eq!(
            question_texts(&h),
            vec![(1, 6000, "How do you handle on-call incidents?".to_string())]
        );
    }

    fn live() -> EngineConfig {
        EngineConfig {
            speculative_hints: true,
            ..Default::default()
        }
    }

    fn last_text(h: &Harness, id: QuestionId) -> Option<String> {
        texts(h).into_iter().rev().find(|t| t.0 == id).map(|t| t.2)
    }

    /// A question cut into three sentences by the speaker's pauses, right
    /// after the user finished answering the previous one, stays one card.
    #[test]
    fn a_question_split_after_an_answer_stays_together() {
        const MERGED: &str = r#"{"isQuestion":true,"directedToMe":true,"merged":true,"short":"IPv6迁移","subQuestions":[],"tags":[]}"#;
        let mut h = Harness::new(live());
        let first = "业务量翻倍以后，你会怎么调整机房的扩容节奏？";
        h.say(Channel::Remote, 0, 3000, first);
        h.detect_reply(YES);
        h.say(
            Channel::Me,
            5000,
            60_000,
            "我会先看容量趋势，再分批采购，最后留出余量，就这些。",
        );
        // The model takes the start of the new question for more of the old
        // one, which the user has already answered.
        h.say(Channel::Remote, 70_000, 71_500, "如何制定数据中心IP。");
        h.detect_reply(MERGED);
        let detects = h.detects.len();
        h.say(Channel::Remote, 72_500, 74_500, "V6的迁移方案，同时保证。");
        h.say(Channel::Remote, 75_500, 77_000, "存量用户不受影响？");
        assert_eq!(
            h.detects.len(),
            detects,
            "the rest is joined without the model"
        );
        assert_eq!(last_text(&h, 1).as_deref(), Some(first));
        assert_eq!(
            last_text(&h, 2).as_deref(),
            Some("如何制定数据中心IP。V6的迁移方案，同时保证。存量用户不受影响？")
        );
        assert!(last_text(&h, 3).is_none());
        let (_, request) = h.hints.last().unwrap();
        assert!(request.messages[0]
            .content
            .contains("同时保证。存量用户不受影响？"));
    }

    /// The set-up said before the question, and a short tail after a pause,
    /// are both kept on the card.
    #[test]
    fn a_question_keeps_its_set_up_and_its_tail() {
        let mut h = Harness::new(live());
        h.say(Channel::Remote, 0, 2000, "你怎么做备份？");
        h.detect_reply(YES);
        h.say(
            Channel::Me,
            4000,
            30_000,
            "首先要异地备份，然后要定期演练，最后要复盘。",
        );
        h.say(Channel::Remote, 40_000, 41_500, "如果研发和运维团队。");
        h.detect_reply(r#"{"isQuestion":false}"#);
        h.say(Channel::Remote, 42_500, 44_000, "的职责重叠，交付很慢。");
        h.detect_reply(r#"{"isQuestion":false}"#);
        h.say(
            Channel::Remote,
            45_000,
            47_000,
            "我们发现了这些问题，那你会如何。",
        );
        let detects = h.detects.len();
        let spec = h.hints.last().unwrap().0;
        h.say(Channel::Remote, 47_300, 47_800, "调整？");
        assert_eq!(
            h.detects.len(),
            detects,
            "the tail is not checked on its own"
        );
        assert!(
            h.cancelled.contains(&spec),
            "hints for the cut-off part dropped"
        );
        let hints = h.hints.len();
        // The model counts more set-up lines than there are since the answer.
        h.detect_reply(
            r#"{"isQuestion":true,"directedToMe":true,"leadIn":3,"short":"职责重叠如何调整"}"#,
        );
        assert_eq!(
            last_text(&h, 2).as_deref(),
            Some(
                "如果研发和运维团队。的职责重叠，交付很慢。我们发现了这些问题，那你会如何。调整？"
            )
        );
        assert_eq!(
            h.hints.len(),
            hints,
            "hints for the whole question started with the tail"
        );
        assert!(h.hints[hints - 1].1.messages[0]
            .content
            .contains("如何。调整？"));
    }

    #[test]
    fn a_new_question_after_the_answer_is_not_merged_into_the_old_one() {
        const MERGED: &str = r#"{"isQuestion":true,"directedToMe":true,"merged":true,"short":"Q","subQuestions":[],"tags":[]}"#;
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 2000, "What is a hash map?");
        h.detect_reply(YES);
        h.say(
            Channel::Me,
            3000,
            9000,
            "It maps keys to values using a hash function.",
        );
        h.say(
            Channel::Remote,
            10_000,
            12_000,
            "And how do you resize one?",
        );
        h.detect_reply(MERGED);
        assert_eq!(last_text(&h, 1).as_deref(), Some("What is a hash map?"));
        assert_eq!(
            last_text(&h, 2).as_deref(),
            Some("And how do you resize one?")
        );
    }

    fn memory_on() -> EngineConfig {
        EngineConfig {
            early_trigger: false,
            meeting_memory: true,
            ..Default::default()
        }
    }

    #[test]
    fn earlier_answers_are_remembered_after_the_transcript_moves_on() {
        let mut h = Harness::new(memory_on());
        h.say(
            Channel::Remote,
            0,
            2000,
            "How did you migrate the billing database?",
        );
        h.detect_reply(r#"{"isQuestion":true,"directedToMe":true,"short":"Billing migration"}"#);
        h.say(
            Channel::Me,
            3000,
            9000,
            "We copied it table by table with dual writes for two weeks.",
        );
        // Still in the recent transcript: no memory needed.
        h.say(Channel::Remote, 20_000, 22_000, "What is a hash map?");
        h.detect_reply(YES);
        let (_, request) = h.hints.last().unwrap();
        assert!(!request.messages[0].content.contains("A: We copied"));
        // Long after, the hint model still hears what the user said.
        h.say(
            Channel::Remote,
            400_000,
            402_000,
            "Going back to the migration, what would you change?",
        );
        h.detect_reply(YES);
        let (_, request) = h.hints.last().unwrap();
        let user = &request.messages[0].content;
        assert!(user.contains("- Q: Billing migration\n  A: We copied it table by table"));
    }

    #[test]
    fn memory_stays_out_when_switched_off() {
        let mut h = Harness::new(no_early());
        h.say(
            Channel::Remote,
            0,
            2000,
            "How did you migrate the billing database?",
        );
        h.detect_reply(YES);
        h.say(Channel::Me, 3000, 9000, "We copied it table by table.");
        h.say(
            Channel::Remote,
            400_000,
            402_000,
            "What would you change about it?",
        );
        h.detect_reply(YES);
        let (_, request) = h.hints.last().unwrap();
        assert!(!request.messages[0].content.contains("table by table"));
    }

    #[test]
    fn a_repeated_sentence_is_not_checked_twice() {
        let mut h = Harness::new(EngineConfig {
            early_trigger: false,
            skip_repeated_sentences: true,
            ..Default::default()
        });
        h.say(
            Channel::Remote,
            0,
            2000,
            "Let me share my screen for a moment.",
        );
        h.detect_reply(r#"{"isQuestion":false}"#);
        let n = h.detects.len();
        h.say(
            Channel::Remote,
            4000,
            6000,
            "Let me share my screen for a moment",
        );
        assert_eq!(h.detects.len(), n, "repeat skipped");
        h.say(
            Channel::Remote,
            8000,
            10_000,
            "How do you shard a large table?",
        );
        assert_eq!(h.detects.len(), n + 1, "a new sentence is checked");
        // Long after, the same remark is checked again.
        h.say(
            Channel::Remote,
            200_000,
            202_000,
            "Let me share my screen for a moment.",
        );
        assert_eq!(h.detects.len(), n + 2);
    }

    #[test]
    fn repeats_are_checked_when_the_switch_is_off() {
        let mut h = Harness::new(no_early());
        h.say(
            Channel::Remote,
            0,
            2000,
            "Let me share my screen for a moment.",
        );
        h.detect_reply(r#"{"isQuestion":false}"#);
        let n = h.detects.len();
        h.say(
            Channel::Remote,
            4000,
            6000,
            "Let me share my screen for a moment.",
        );
        assert_eq!(h.detects.len(), n + 1);
    }

    #[test]
    fn hints_can_be_rewritten_shorter() {
        let mut h = Harness::new(no_early());
        h.say(Channel::Remote, 0, 2000, "What is a hash map?");
        h.detect_reply(YES);
        let (req, _) = h.hints.last().cloned().unwrap();
        h.hint_points(req, &[("Keys", "map to values")]);
        h.input(Input::Action(UserAction::Rewrite {
            id: 1,
            style: RewriteStyle::Shorter,
        }));
        let (_, request) = h.hints.last().unwrap();
        assert!(request.messages[0].content.contains("shorter version"));
        let version = h.events.iter().rev().find_map(|e| match e {
            MeetCueEvent::QuestionNew { id: 1, version, .. } => Some(*version),
            _ => None,
        });
        assert_eq!(version, Some(2), "a new version clears the old points");
        // The note applies once.
        h.input(Input::Action(UserAction::Rewrite {
            id: 1,
            style: RewriteStyle::Again,
        }));
        let (_, request) = h.hints.last().unwrap();
        assert!(!request.messages[0].content.contains("shorter version"));
        assert!(request.messages[0].content.contains("different take"));
    }

    #[test]
    fn a_typed_question_gets_hints_without_detection() {
        let mut h = Harness::new(no_early());
        let detects = h.detects.len();
        h.input(Input::Action(UserAction::Ask {
            text: "  How would you design a rate limiter? ".into(),
        }));
        assert_eq!(h.detects.len(), detects);
        assert_eq!(
            last_text(&h, 1).as_deref(),
            Some("How would you design a rate limiter?")
        );
        assert!(h.hints.last().unwrap().1.messages[0]
            .content
            .contains("rate limiter"));
        h.input(Input::Action(UserAction::Ask { text: "  ".into() }));
        assert!(last_text(&h, 2).is_none());
    }

    #[test]
    fn the_speaker_label_goes_on_the_question() {
        let mut h = Harness::new(no_early());
        h.at(0);
        h.input(Input::Asr {
            channel: Channel::Remote,
            result: AsrResult::Speaker(2),
        });
        h.say(Channel::Remote, 0, 2000, "What is a hash map?");
        h.detect_reply(YES);
        let speaker = h.events.iter().rev().find_map(|e| match e {
            MeetCueEvent::QuestionNew { speaker, .. } => Some(*speaker),
            _ => None,
        });
        assert_eq!(speaker, Some(Some(2)));
    }
}
