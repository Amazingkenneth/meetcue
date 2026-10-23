//! The practice pacer: decides what to play, when the user's answer is over,
//! and when to move on. A pure state machine driven with explicit times, like
//! the hint engine, so every pacing rule is unit-tested.

use meetcue_core::QuestionId;

use super::{PracticeKind, PracticeScript};
use crate::text::{self, Lang};

#[derive(Debug, Clone)]
pub struct PacerConfig {
    /// Silence after speaking that ends an answer.
    pub answer_pause_ms: i64,
    /// Gentle reminder when the user hasn't started answering.
    pub nudge_after_ms: i64,
    /// Chance of one follow-up question after an answer (question lists).
    pub follow_up_probability: f32,
    /// Questions are not sent to recognition; their text is injected.
    pub simulated: bool,
    /// Stop reading when the user starts to speak.
    pub stop_on_speech: bool,
    /// Longest pause between subtitle lines.
    pub max_gap_ms: i64,
    /// Seed for the follow-up dice, so runs are reproducible in tests.
    pub seed: u64,
}

impl Default for PacerConfig {
    fn default() -> Self {
        Self {
            answer_pause_ms: 3_000,
            nudge_after_ms: 20_000,
            follow_up_probability: 0.3,
            simulated: false,
            stop_on_speech: true,
            max_gap_ms: 2_000,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PacerInput {
    /// Periodic clock tick.
    Tick,
    /// Speech synthesis finished (or was stopped) for the current line.
    SpeechDone,
    /// The user's microphone carries voice (from the level meter).
    MeVoice,
    /// Recognised text from the user's microphone.
    MeText {
        text: String,
        is_final: bool,
    },
    /// The engine detected a question (links hints to the current answer).
    QuestionDetected {
        id: QuestionId,
    },
    /// Coverage of a question's talking points.
    Coverage {
        id: QuestionId,
        done: u32,
        total: u32,
    },
    /// The follow-up the runner generated; `None` if generation failed.
    FollowUp(Option<String>),
    Answered,
    Skip,
    Pause,
    Resume,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PacerOutput {
    /// Read a line aloud.
    Speak {
        text: String,
        lang: Lang,
    },
    StopSpeaking,
    /// Simulated mode: hand the line to the pipeline after a recognition
    /// delay chosen by the runner. `known_question` skips detection.
    Inject {
        text: String,
        known_question: bool,
    },
    /// Ask the model for a follow-up question; answer with `FollowUp`.
    GenerateFollowUp {
        question: String,
        answer: String,
    },
    Progress(Progress),
    /// The user hasn't started answering for a while.
    Nudge,
    Finished,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// 1-based number of the current answer turn.
    pub index: u32,
    pub total: u32,
    pub phase: Phase,
    pub question: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Speaking,
    Waiting,
    Answering,
    FollowUp,
    Paused,
    Done,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Speaking => "speaking",
            Phase::Waiting => "waiting",
            Phase::Answering => "answering",
            Phase::FollowUp => "followUp",
            Phase::Paused => "paused",
            Phase::Done => "done",
        }
    }
}

/// What happened for one answer turn; the review is computed from these.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnswerRecord {
    pub item_id: u32,
    pub question: String,
    pub reference: Option<String>,
    pub is_follow_up: bool,
    /// When the question finished playing.
    pub asked_at_ms: Option<i64>,
    pub first_voice_ms: Option<i64>,
    pub first_word_ms: Option<i64>,
    pub last_voice_ms: Option<i64>,
    pub ended_at_ms: Option<i64>,
    /// Final recognised text of the answer.
    pub answer: String,
    pub skipped: bool,
    pub question_ids: Vec<QuestionId>,
    pub points_done: u32,
    pub points_total: u32,
}

#[derive(Debug, Clone)]
enum Step {
    /// A line read aloud.
    Play {
        text: String,
        known_question: bool,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    },
    /// The user answers (question lists: after each question; subtitles:
    /// the user's own lines).
    Answer {
        item_id: u32,
        question: String,
        reference: Option<String>,
        followups: Vec<String>,
        is_follow_up: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Speaking,
    /// Waiting before the next subtitle line plays.
    Gap {
        until: i64,
    },
    Waiting {
        since: i64,
        nudged: bool,
    },
    Answering,
    FollowUpPending,
    Done,
}

pub struct Pacer {
    cfg: PacerConfig,
    steps: Vec<Step>,
    pos: usize,
    state: State,
    paused: Option<State>,
    records: Vec<AnswerRecord>,
    /// Text of the line being read, to ignore its echo in the microphone.
    speaking_text: String,
    /// Questions detected before the answer turn they belong to opened.
    pending_qids: Vec<QuestionId>,
    total: u32,
    answered_turns: u32,
    rng: u64,
}

impl Pacer {
    pub fn new(script: &PracticeScript, cfg: PacerConfig) -> Self {
        let mut steps = Vec::new();
        match script.kind {
            PracticeKind::Questions => {
                for it in &script.items {
                    steps.push(Step::Play {
                        text: it.text.clone(),
                        known_question: true,
                        start_ms: None,
                        end_ms: None,
                    });
                    steps.push(Step::Answer {
                        item_id: it.id,
                        question: it.text.clone(),
                        reference: it.reference.clone(),
                        followups: it.followups.clone(),
                        is_follow_up: false,
                    });
                }
            }
            PracticeKind::Subtitles => {
                // Lines of the same turn that follow each other are answered once.
                let mut last_question = String::new();
                for it in &script.items {
                    if it.is_me {
                        match steps.last_mut() {
                            Some(Step::Answer { reference, .. }) => {
                                let r = reference.get_or_insert_with(String::new);
                                r.push(' ');
                                r.push_str(&it.text);
                            }
                            _ => steps.push(Step::Answer {
                                item_id: it.id,
                                question: last_question.clone(),
                                reference: Some(it.text.clone()),
                                followups: Vec::new(),
                                is_follow_up: false,
                            }),
                        }
                    } else {
                        last_question = it.text.clone();
                        steps.push(Step::Play {
                            text: it.text.clone(),
                            known_question: false,
                            start_ms: it.start_ms,
                            end_ms: it.end_ms,
                        });
                    }
                }
            }
        }
        let total = steps
            .iter()
            .filter(|s| matches!(s, Step::Answer { .. }))
            .count() as u32;
        Self {
            rng: cfg.seed.max(1),
            cfg,
            steps,
            pos: 0,
            state: State::Idle,
            paused: None,
            records: Vec::new(),
            speaking_text: String::new(),
            pending_qids: Vec::new(),
            total,
            answered_turns: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn records(&self) -> &[AnswerRecord] {
        &self.records
    }

    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Starts with the first step.
    pub fn start(&mut self, now: i64) -> Vec<PacerOutput> {
        let mut out = Vec::new();
        self.enter_step(now, &mut out);
        out
    }

    pub fn handle(&mut self, input: PacerInput, now: i64) -> Vec<PacerOutput> {
        let mut out = Vec::new();
        if self.state == State::Done {
            return out;
        }
        // Links and coverage are recorded even while paused.
        match &input {
            PacerInput::QuestionDetected { id } => {
                let open = matches!(
                    self.paused.unwrap_or(self.state),
                    State::Waiting { .. } | State::Answering
                );
                match self.records.last_mut() {
                    Some(r) if open => {
                        if !r.question_ids.contains(id) {
                            r.question_ids.push(*id);
                        }
                    }
                    _ => self.pending_qids.push(*id),
                }
                return out;
            }
            PacerInput::Coverage { id, done, total } => {
                if let Some(r) = self
                    .records
                    .iter_mut()
                    .rev()
                    .find(|r| r.question_ids.contains(id))
                {
                    r.points_done = *done;
                    r.points_total = *total;
                }
                return out;
            }
            _ => {}
        }
        if let Some(prev) = self.paused {
            match input {
                PacerInput::Resume => {
                    self.paused = None;
                    self.resume(prev, now, &mut out);
                }
                PacerInput::Stop => self.finish(&mut out),
                PacerInput::Skip => {
                    self.paused = None;
                    self.skip(now, &mut out);
                }
                _ => {}
            }
            return out;
        }
        match input {
            PacerInput::Tick => self.tick(now, &mut out),
            PacerInput::SpeechDone => {
                if self.state == State::Speaking {
                    self.after_speech(now, &mut out);
                }
            }
            PacerInput::MeVoice => self.voice(now, None, &mut out),
            PacerInput::MeText { text, is_final } => {
                self.voice(now, Some((text, is_final)), &mut out)
            }
            PacerInput::FollowUp(q) => {
                if self.state == State::FollowUpPending {
                    match q.filter(|q| !q.trim().is_empty()) {
                        Some(q) => self.insert_follow_up(q),
                        None => self.pos += 1,
                    }
                    self.enter_step(now, &mut out);
                }
            }
            PacerInput::Answered => match self.state {
                State::Waiting { .. } | State::Answering => self.end_answer(now, &mut out),
                State::Speaking | State::Gap { .. } => {
                    // Treat as "skip ahead to my answer".
                    if self.state == State::Speaking {
                        out.push(PacerOutput::StopSpeaking);
                    }
                    self.after_speech(now, &mut out);
                }
                _ => {}
            },
            PacerInput::Skip => self.skip(now, &mut out),
            PacerInput::Pause => {
                if self.state == State::Speaking {
                    out.push(PacerOutput::StopSpeaking);
                }
                self.paused = Some(self.state);
                out.push(self.progress(Phase::Paused));
            }
            PacerInput::Resume => {}
            PacerInput::Stop => self.finish(&mut out),
            PacerInput::QuestionDetected { .. } | PacerInput::Coverage { .. } => {}
        }
        out
    }

    fn current_question(&self) -> String {
        match self.steps.get(self.pos) {
            Some(Step::Answer { question, .. }) => question.clone(),
            Some(Step::Play { text, .. }) => text.clone(),
            None => String::new(),
        }
    }

    fn turn_index(&self) -> u32 {
        (self.answered_turns + 1).min(self.total.max(1))
    }

    fn progress(&self, phase: Phase) -> PacerOutput {
        let question = match self.steps.get(self.pos) {
            // Show the question being answered, not the user's own line.
            Some(Step::Answer { question, .. }) => question.clone(),
            _ => self.current_question(),
        };
        PacerOutput::Progress(Progress {
            index: self.turn_index(),
            total: self.total,
            phase,
            question,
        })
    }

    fn enter_step(&mut self, now: i64, out: &mut Vec<PacerOutput>) {
        let Some(step) = self.steps.get(self.pos).cloned() else {
            self.finish(out);
            return;
        };
        match step {
            Step::Play { text, .. } => {
                self.state = State::Speaking;
                self.speaking_text = text.clone();
                out.push(PacerOutput::Speak {
                    lang: text::dominant_lang(&text),
                    text,
                });
                out.push(self.progress(Phase::Speaking));
            }
            Step::Answer {
                item_id,
                question,
                reference,
                is_follow_up,
                ..
            } => {
                self.records.push(AnswerRecord {
                    item_id,
                    question,
                    reference,
                    is_follow_up,
                    asked_at_ms: Some(now),
                    question_ids: std::mem::take(&mut self.pending_qids),
                    ..Default::default()
                });
                self.state = State::Waiting {
                    since: now,
                    nudged: false,
                };
                out.push(self.progress(Phase::Waiting));
            }
        }
    }

    fn after_speech(&mut self, now: i64, out: &mut Vec<PacerOutput>) {
        if let Some(Step::Play {
            text,
            known_question,
            end_ms,
            ..
        }) = self.steps.get(self.pos).cloned()
        {
            if self.cfg.simulated {
                out.push(PacerOutput::Inject {
                    text,
                    known_question,
                });
            }
            self.speaking_text.clear();
            self.pos += 1;
            // Subtitles: keep the original rhythm, with long gaps shortened.
            if let Some(Step::Play {
                start_ms: Some(next),
                ..
            }) = self.steps.get(self.pos)
            {
                let gap = end_ms
                    .map(|e| (next - e).clamp(0, self.cfg.max_gap_ms))
                    .unwrap_or(0);
                if gap > 0 {
                    self.state = State::Gap { until: now + gap };
                    return;
                }
            }
            self.enter_step(now, out);
        }
    }

    fn tick(&mut self, now: i64, out: &mut Vec<PacerOutput>) {
        match self.state {
            State::Gap { until } if now >= until => self.enter_step(now, out),
            State::Waiting { since, nudged } => {
                if !nudged && now - since >= self.cfg.nudge_after_ms {
                    self.state = State::Waiting {
                        since,
                        nudged: true,
                    };
                    out.push(PacerOutput::Nudge);
                }
            }
            State::Answering => {
                let last = self
                    .records
                    .last()
                    .and_then(|r| r.last_voice_ms)
                    .unwrap_or(now);
                if now - last >= self.cfg.answer_pause_ms {
                    self.end_answer(last, out);
                }
            }
            _ => {}
        }
    }

    fn is_echo(&self, text: &str) -> bool {
        if self.speaking_text.is_empty() {
            return false;
        }
        let heard = text::unit_set(text);
        if heard.is_empty() {
            return true;
        }
        text::containment(&heard, &text::unit_set(&self.speaking_text)) >= 0.5
    }

    fn voice(&mut self, now: i64, text: Option<(String, bool)>, out: &mut Vec<PacerOutput>) {
        match self.state {
            State::Speaking => {
                // Only recognised words that aren't the played line's echo
                // count as the user speaking over the question.
                if let Some((t, _)) = &text {
                    if self.cfg.stop_on_speech && !self.is_echo(t) {
                        out.push(PacerOutput::StopSpeaking);
                        self.after_speech(now, out);
                        if matches!(self.state, State::Waiting { .. }) {
                            self.voice(now, text, out);
                        }
                    }
                }
            }
            State::Waiting { .. } | State::Answering => {
                let started = self.state != State::Answering;
                if let Some(r) = self.records.last_mut() {
                    r.first_voice_ms.get_or_insert(now);
                    r.last_voice_ms = Some(now);
                    if let Some((t, is_final)) = text {
                        if !t.trim().is_empty() {
                            r.first_word_ms.get_or_insert(now);
                            if is_final {
                                if !r.answer.is_empty() {
                                    r.answer.push(' ');
                                }
                                r.answer.push_str(t.trim());
                            }
                        }
                    }
                }
                self.state = State::Answering;
                if started {
                    out.push(self.progress(Phase::Answering));
                }
            }
            _ => {}
        }
    }

    fn next_random(&mut self) -> f32 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn end_answer(&mut self, at: i64, out: &mut Vec<PacerOutput>) {
        if let Some(r) = self.records.last_mut() {
            r.ended_at_ms = Some(at);
        }
        self.answered_turns += 1;
        let step = self.steps.get(self.pos).cloned();
        let answer = self
            .records
            .last()
            .map(|r| r.answer.clone())
            .unwrap_or_default();
        if let Some(Step::Answer {
            item_id,
            question,
            followups,
            is_follow_up: false,
            ..
        }) = step
        {
            if self.cfg.follow_up_probability > 0.0
                && self.next_random() < self.cfg.follow_up_probability
            {
                self.total += 1;
                if let Some(f) = followups.first() {
                    self.pos += 1;
                    self.insert_follow_up_at(item_id, f.clone());
                    self.enter_step(at, out);
                    return;
                }
                self.state = State::FollowUpPending;
                out.push(PacerOutput::GenerateFollowUp { question, answer });
                out.push(self.progress(Phase::FollowUp));
                return;
            }
        }
        self.pos += 1;
        self.enter_step(at, out);
    }

    fn insert_follow_up(&mut self, q: String) {
        let item_id = match self.steps.get(self.pos) {
            Some(Step::Answer { item_id, .. }) => *item_id,
            _ => 0,
        };
        self.pos += 1;
        self.insert_follow_up_at(item_id, q);
    }

    /// Inserts "play follow-up, then answer it" at the current position.
    fn insert_follow_up_at(&mut self, item_id: u32, q: String) {
        let at = self.pos.min(self.steps.len());
        self.steps.insert(
            at,
            Step::Answer {
                item_id,
                question: q.clone(),
                reference: None,
                followups: Vec::new(),
                is_follow_up: true,
            },
        );
        self.steps.insert(
            at,
            Step::Play {
                text: q,
                known_question: true,
                start_ms: None,
                end_ms: None,
            },
        );
    }

    fn skip(&mut self, now: i64, out: &mut Vec<PacerOutput>) {
        if self.state == State::Speaking {
            out.push(PacerOutput::StopSpeaking);
        }
        if self.state == State::FollowUpPending {
            // The follow-up was counted; drop it.
            self.total = self.total.saturating_sub(1);
            self.pos += 1;
            self.enter_step(now, out);
            return;
        }
        // Move past the next answer turn, recording it as skipped.
        while let Some(step) = self.steps.get(self.pos).cloned() {
            self.pos += 1;
            if let Step::Answer {
                item_id,
                question,
                reference,
                is_follow_up,
                ..
            } = step
            {
                let open = matches!(self.state, State::Waiting { .. } | State::Answering);
                if open {
                    if let Some(r) = self.records.last_mut() {
                        r.skipped = r.answer.is_empty();
                        r.ended_at_ms = Some(now);
                    }
                } else {
                    self.records.push(AnswerRecord {
                        item_id,
                        question,
                        reference,
                        is_follow_up,
                        skipped: true,
                        ..Default::default()
                    });
                }
                self.answered_turns += 1;
                break;
            }
        }
        self.enter_step(now, out);
    }

    fn resume(&mut self, prev: State, now: i64, out: &mut Vec<PacerOutput>) {
        match prev {
            State::Speaking | State::Idle => self.enter_step(now, out),
            State::Gap { .. } => self.enter_step(now, out),
            State::Waiting { nudged, .. } => {
                self.state = State::Waiting { since: now, nudged };
                out.push(self.progress(Phase::Waiting));
            }
            State::Answering => {
                if let Some(r) = self.records.last_mut() {
                    r.last_voice_ms = Some(now);
                }
                self.state = State::Answering;
                out.push(self.progress(Phase::Answering));
            }
            State::FollowUpPending => {
                self.state = prev;
                out.push(self.progress(Phase::FollowUp));
            }
            State::Done => {}
        }
    }

    fn finish(&mut self, out: &mut Vec<PacerOutput>) {
        if self.state == State::Speaking {
            out.push(PacerOutput::StopSpeaking);
        }
        self.state = State::Done;
        self.paused = None;
        out.push(PacerOutput::Progress(Progress {
            index: self.answered_turns.min(self.total),
            total: self.total,
            phase: Phase::Done,
            question: String::new(),
        }));
        out.push(PacerOutput::Finished);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::parse;

    fn questions(n: usize) -> PracticeScript {
        let text: String = (1..=n).map(|i| format!("Question {i}?\n")).collect();
        parse::parse_file("q.txt", text.as_bytes()).unwrap()
    }

    fn cfg() -> PacerConfig {
        PacerConfig {
            follow_up_probability: 0.0,
            ..Default::default()
        }
    }

    fn speaks(out: &[PacerOutput]) -> Vec<String> {
        out.iter()
            .filter_map(|o| match o {
                PacerOutput::Speak { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn phase(out: &[PacerOutput]) -> Option<Phase> {
        out.iter().rev().find_map(|o| match o {
            PacerOutput::Progress(p) => Some(p.phase),
            _ => None,
        })
    }

    fn text(t: &str) -> PacerInput {
        PacerInput::MeText {
            text: t.into(),
            is_final: true,
        }
    }

    #[test]
    fn asks_waits_and_moves_on_after_a_pause() {
        let mut p = Pacer::new(&questions(2), cfg());
        assert_eq!(p.total(), 2);
        let out = p.start(0);
        assert_eq!(speaks(&out), vec!["Question 1?"]);
        assert_eq!(phase(&out), Some(Phase::Speaking));
        let out = p.handle(PacerInput::SpeechDone, 2_000);
        assert_eq!(phase(&out), Some(Phase::Waiting));
        // Thinking quietly does not end the answer.
        assert!(p.handle(PacerInput::Tick, 6_000).is_empty());
        let out = p.handle(text("first part"), 7_000);
        assert_eq!(phase(&out), Some(Phase::Answering));
        p.handle(PacerInput::MeVoice, 9_000);
        assert!(speaks(&p.handle(PacerInput::Tick, 11_900)).is_empty());
        let out = p.handle(PacerInput::Tick, 12_000);
        assert_eq!(speaks(&out), vec!["Question 2?"]);
        let r = &p.records()[0];
        assert_eq!(r.asked_at_ms, Some(2_000));
        assert_eq!(r.first_word_ms, Some(7_000));
        assert_eq!(r.ended_at_ms, Some(9_000), "answer ends at the last voice");
        assert_eq!(r.answer, "first part");
    }

    #[test]
    fn nudges_once_and_hotkey_ends_answer() {
        let mut p = Pacer::new(&questions(1), cfg());
        p.start(0);
        p.handle(PacerInput::SpeechDone, 1_000);
        assert!(p.handle(PacerInput::Tick, 20_000).is_empty());
        assert_eq!(p.handle(PacerInput::Tick, 21_000), vec![PacerOutput::Nudge]);
        assert!(p.handle(PacerInput::Tick, 50_000).is_empty());
        let out = p.handle(PacerInput::Answered, 51_000);
        assert!(out.contains(&PacerOutput::Finished));
        assert!(p.is_done());
    }

    #[test]
    fn echo_of_the_question_does_not_interrupt_but_real_speech_does() {
        let mut p = Pacer::new(&questions(1), cfg());
        p.start(0);
        assert!(p.handle(text("Question 1"), 500).is_empty(), "echo");
        let out = p.handle(text("I think the answer is"), 800);
        assert!(out.contains(&PacerOutput::StopSpeaking));
        assert_eq!(phase(&out), Some(Phase::Answering));
        assert_eq!(p.records()[0].first_word_ms, Some(800));
    }

    #[test]
    fn interrupt_can_be_turned_off() {
        let mut p = Pacer::new(
            &questions(1),
            PacerConfig {
                stop_on_speech: false,
                ..cfg()
            },
        );
        p.start(0);
        assert!(p.handle(text("I think the answer is"), 800).is_empty());
    }

    #[test]
    fn simulated_mode_injects_after_playback() {
        let mut p = Pacer::new(
            &questions(1),
            PacerConfig {
                simulated: true,
                ..cfg()
            },
        );
        p.start(0);
        let out = p.handle(PacerInput::SpeechDone, 1_000);
        assert_eq!(
            out[0],
            PacerOutput::Inject {
                text: "Question 1?".into(),
                known_question: true
            }
        );
    }

    #[test]
    fn skip_pause_resume_and_stop() {
        let mut p = Pacer::new(&questions(3), cfg());
        p.start(0);
        let out = p.handle(PacerInput::Skip, 100);
        assert!(out.contains(&PacerOutput::StopSpeaking));
        assert_eq!(speaks(&out), vec!["Question 2?"]);
        assert!(p.records()[0].skipped);

        p.handle(PacerInput::SpeechDone, 1_000);
        p.handle(text("answer"), 2_000);
        let out = p.handle(PacerInput::Pause, 2_500);
        assert_eq!(phase(&out), Some(Phase::Paused));
        // Time passes while paused: no answer end.
        assert!(p.handle(PacerInput::Tick, 30_000).is_empty());
        let out = p.handle(PacerInput::Resume, 30_000);
        assert_eq!(phase(&out), Some(Phase::Answering));
        let out = p.handle(PacerInput::Tick, 33_000);
        assert_eq!(speaks(&out), vec!["Question 3?"]);
        assert!(!p.records()[1].skipped);

        let out = p.handle(PacerInput::Stop, 33_100);
        assert!(out.contains(&PacerOutput::StopSpeaking));
        assert!(out.contains(&PacerOutput::Finished));
        assert_eq!(p.records().len(), 2, "the unanswered third is not recorded");
    }

    #[test]
    fn pause_while_speaking_replays_the_line() {
        let mut p = Pacer::new(&questions(1), cfg());
        p.start(0);
        let out = p.handle(PacerInput::Pause, 200);
        assert!(out.contains(&PacerOutput::StopSpeaking));
        // The stopped speech reports done; ignored while paused.
        assert!(p.handle(PacerInput::SpeechDone, 210).is_empty());
        let out = p.handle(PacerInput::Resume, 5_000);
        assert_eq!(speaks(&out), vec!["Question 1?"]);
    }

    #[test]
    fn follow_ups_from_the_file_and_from_the_model() {
        let csv = "question,followups\nWhat is SDN?,Which controllers?\nWhat is BGP?,\n";
        let script = parse::parse_file("q.csv", csv.as_bytes()).unwrap();
        let mut p = Pacer::new(
            &script,
            PacerConfig {
                follow_up_probability: 1.0,
                ..cfg()
            },
        );
        p.start(0);
        p.handle(PacerInput::SpeechDone, 1_000);
        p.handle(text("separation of planes"), 2_000);
        let out = p.handle(PacerInput::Tick, 5_000);
        assert_eq!(speaks(&out), vec!["Which controllers?"]);
        assert_eq!(p.total(), 3);
        p.handle(PacerInput::SpeechDone, 6_000);
        p.handle(text("ONOS"), 7_000);
        // No follow-up of a follow-up.
        let out = p.handle(PacerInput::Tick, 10_000);
        assert_eq!(speaks(&out), vec!["What is BGP?"]);
        p.handle(PacerInput::SpeechDone, 11_000);
        p.handle(text("path vector"), 12_000);
        let out = p.handle(PacerInput::Tick, 15_000);
        assert_eq!(
            out[0],
            PacerOutput::GenerateFollowUp {
                question: "What is BGP?".into(),
                answer: "path vector".into()
            }
        );
        let out = p.handle(
            PacerInput::FollowUp(Some("Why path vector?".into())),
            16_000,
        );
        assert_eq!(speaks(&out), vec!["Why path vector?"]);
        let recs = p.records();
        assert!(recs[1].is_follow_up && recs[1].question == "Which controllers?");

        // The answer to a generated follow-up ends the practice.
        p.handle(PacerInput::SpeechDone, 17_000);
        p.handle(text("loops"), 18_000);
        let out = p.handle(PacerInput::Tick, 21_000);
        assert!(out.contains(&PacerOutput::Finished));
        assert_eq!(p.records().len(), 4);
    }

    #[test]
    fn failed_follow_up_generation_moves_on() {
        let mut p = Pacer::new(
            &questions(2),
            PacerConfig {
                follow_up_probability: 1.0,
                ..cfg()
            },
        );
        p.start(0);
        p.handle(PacerInput::SpeechDone, 1_000);
        p.handle(text("x"), 2_000);
        p.handle(PacerInput::Tick, 5_000);
        let out = p.handle(PacerInput::FollowUp(None), 6_000);
        assert_eq!(speaks(&out), vec!["Question 2?"]);
    }

    #[test]
    fn subtitles_keep_rhythm_and_wait_for_my_lines() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nA: Hello, welcome.\n\n\
                   2\n00:00:03,500 --> 00:00:05,000\nA: Could you introduce yourself?\n\n\
                   3\n00:00:06,000 --> 00:00:09,000\nMe: I'm a network engineer.\n\n\
                   4\n00:00:20,000 --> 00:00:22,000\nA: Why networking?\n\n\
                   5\n00:00:23,000 --> 00:00:25,000\nMe: I like it.\n";
        let mut script = parse::parse_file("s.srt", srt.as_bytes()).unwrap();
        parse::set_me(&mut script, Some("Me"));
        let mut p = Pacer::new(&script, cfg());
        assert_eq!(p.total(), 2);
        let out = p.start(0);
        // A's two cues 500 ms apart are one turn.
        assert_eq!(
            speaks(&out),
            vec!["Hello, welcome. Could you introduce yourself?"]
        );
        let out = p.handle(PacerInput::SpeechDone, 4_000);
        assert_eq!(phase(&out), Some(Phase::Waiting));
        p.handle(text("I build networks"), 5_000);
        let out = p.handle(PacerInput::Tick, 8_000);
        assert_eq!(speaks(&out), vec!["Why networking?"]);
        let r = &p.records()[0];
        assert_eq!(r.reference.as_deref(), Some("I'm a network engineer."));
        assert_eq!(r.question, "Hello, welcome. Could you introduce yourself?");
    }

    #[test]
    fn subtitle_gaps_are_capped() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\nA: One.\n\n\
                   2\n00:00:30,000 --> 00:00:31,000\nB: Two?\n\n\
                   3\n00:00:32,000 --> 00:00:33,000\nMe: Three.\n";
        let mut script = parse::parse_file("s.srt", srt.as_bytes()).unwrap();
        parse::set_me(&mut script, Some("Me"));
        let mut p = Pacer::new(&script, cfg());
        p.start(0);
        assert!(p.handle(PacerInput::SpeechDone, 1_000).is_empty());
        assert!(p.handle(PacerInput::Tick, 2_900).is_empty());
        assert_eq!(speaks(&p.handle(PacerInput::Tick, 3_000)), vec!["Two?"]);
    }

    #[test]
    fn coverage_follows_linked_questions() {
        let mut p = Pacer::new(&questions(1), cfg());
        p.start(0);
        // Detected while the question is still being read.
        p.handle(PacerInput::QuestionDetected { id: 7 }, 500);
        p.handle(PacerInput::SpeechDone, 1_000);
        p.handle(
            PacerInput::Coverage {
                id: 7,
                done: 2,
                total: 4,
            },
            3_000,
        );
        p.handle(
            PacerInput::Coverage {
                id: 9,
                done: 1,
                total: 1,
            },
            3_000,
        );
        let r = &p.records()[0];
        assert_eq!((r.points_done, r.points_total), (2, 4));
    }
}
