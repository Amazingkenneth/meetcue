//! Events emitted by the pipeline and actions sent to it.
//!
//! The same enum travels over the Tauri event channel today and can be sent
//! as JSON over a network connection later, so every variant is plain data.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type QuestionId = u32;

/// Which side of the conversation an audio frame or transcript belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Channel {
    /// The user, captured from the microphone.
    Me,
    /// Other participants, captured from system sound (loopback).
    Remote,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Me => "me",
            Channel::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum QuestionStatus {
    /// Queued behind the active question.
    Pending,
    /// The question the user is answering now. At most one at a time.
    Active,
    Answered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PointStatus {
    Todo,
    /// The point the user should say next. At most one per question.
    Next,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SessionState {
    Idle,
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TranscriptLine {
    pub channel: Channel,
    pub text: String,
    #[ts(type = "number")]
    pub t0_ms: i64,
    #[ts(type = "number")]
    pub t1_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionBrief {
    pub id: QuestionId,
    pub short: String,
    pub status: QuestionStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum UsageKind {
    Llm {
        #[ts(type = "number")]
        input: u64,
        #[ts(type = "number")]
        output: u64,
        #[ts(type = "number")]
        cache_read: u64,
        #[ts(type = "number")]
        cache_write: u64,
    },
    Audio {
        #[ts(type = "number")]
        ms: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BudgetScope {
    Session,
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BudgetLevel {
    /// 80 % of the limit.
    Warning,
    /// 100 % of the limit; the configured action has been applied.
    Reached,
}

/// What the pipeline used a model call for; drives cost breakdowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum UsagePurpose {
    Asr,
    Detect,
    Hints,
    FollowUp,
    Classify,
    Review,
    Probe,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum MeetCueEvent {
    SessionState {
        state: SessionState,
        #[ts(type = "number")]
        t_ms: i64,
    },
    /// A partial or final transcript line. Partials of the same utterance share `utterance`.
    Transcript {
        channel: Channel,
        utterance: u32,
        is_final: bool,
        text: String,
        #[ts(type = "number")]
        t0_ms: i64,
        #[ts(type = "number")]
        t1_ms: i64,
    },
    /// A new question, or a newer version of an existing one (same id, higher version).
    QuestionNew {
        id: QuestionId,
        version: u32,
        text: String,
        short: String,
        tags: Vec<String>,
        sub_questions: Vec<String>,
        status: QuestionStatus,
        #[ts(type = "number")]
        t_ms: i64,
        /// Transcript lines around the question, for the "context" disclosure.
        context: Vec<TranscriptLine>,
    },
    QuestionRemoved {
        id: QuestionId,
    },
    QuestionPoint {
        id: QuestionId,
        version: u32,
        n: u32,
        head: String,
        rest: String,
        status: PointStatus,
    },
    AnswerDelta {
        id: QuestionId,
        version: u32,
        text: String,
    },
    AnswerDone {
        id: QuestionId,
        version: u32,
    },
    /// Hint generation for this question failed; the UI shows a short notice.
    HintsFailed {
        id: QuestionId,
        version: u32,
        code: String,
    },
    Coverage {
        id: QuestionId,
        statuses: Vec<PointStatus>,
    },
    QuestionStatus {
        id: QuestionId,
        status: QuestionStatus,
    },
    QueueChanged {
        questions: Vec<QuestionBrief>,
        active: Option<QuestionId>,
        focus: Option<QuestionId>,
    },
    /// Time from end of speech to first hint point, split by stage.
    LatencySample {
        id: QuestionId,
        #[ts(type = "number")]
        asr_ms: i64,
        #[ts(type = "number")]
        detect_ms: i64,
        #[ts(type = "number")]
        generate_ms: i64,
        #[ts(type = "number")]
        total_ms: i64,
        early: bool,
    },
    Usage {
        provider: String,
        model: String,
        purpose: UsagePurpose,
        usage: UsageKind,
        /// Cost of this call in micro-dollars, or `None` if the price is unknown.
        #[ts(type = "number | null")]
        cost_micros: Option<u64>,
        #[ts(type = "number")]
        session_micros: u64,
    },
    Budget {
        scope: BudgetScope,
        level: BudgetLevel,
        #[ts(type = "number")]
        spent_micros: u64,
        #[ts(type = "number")]
        limit_micros: u64,
    },
    /// Throttled input levels (dBFS) for the level meters.
    Levels {
        me: f32,
        remote: f32,
    },
    /// The latency monitor recommends running the probe again.
    LatencyWarning {
        #[ts(type = "number")]
        median_ms: i64,
    },
    Error {
        code: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FocusTarget {
    Prev,
    Next,
}

/// Actions from the user (hotkeys, clicks) to the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum UserAction {
    Focus { id: QuestionId },
    FocusStep { target: FocusTarget },
    MarkAnswered,
    Pause,
    Resume,
}

/// An event with the session-relative time it was emitted at; used for
/// recorded demo scripts and diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimedEvent {
    #[ts(type = "number")]
    pub t_ms: i64,
    pub event: MeetCueEvent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_json_shape() {
        let e = MeetCueEvent::QuestionPoint {
            id: 3,
            version: 1,
            n: 2,
            head: "双控制器".into(),
            rest: "主备 心跳".into(),
            status: PointStatus::Next,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "questionPoint");
        assert_eq!(v["status"], "next");
        assert_eq!(v["head"], "双控制器");
        let back: MeetCueEvent = serde_json::from_value(v).unwrap();
        assert_eq!(back, e);

        let e = MeetCueEvent::LatencySample {
            id: 1,
            asr_ms: 300,
            detect_ms: 400,
            generate_ms: 500,
            total_ms: 1200,
            early: false,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["asrMs"], 300);
    }

    #[test]
    fn action_json_shape() {
        let a: UserAction =
            serde_json::from_str(r#"{"type":"focusStep","target":"next"}"#).unwrap();
        assert_eq!(
            a,
            UserAction::FocusStep {
                target: FocusTarget::Next
            }
        );
        let a: UserAction = serde_json::from_str(r#"{"type":"markAnswered"}"#).unwrap();
        assert_eq!(a, UserAction::MarkAnswered);
    }
}
