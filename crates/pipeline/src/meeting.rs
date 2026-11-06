//! Meeting history: what a meeting left behind once listening stopped.
//!
//! [`record`] rebuilds the questions, their hints and ticks from the events
//! the session emitted, the same way the panel shows them, so the recap
//! matches what the user saw. Nothing here touches the disk; the app saves
//! the record next to the settings.

use std::collections::BTreeMap;

use meetcue_core::{
    Channel, MeetCueEvent, PointStatus, QuestionStatus, SessionState, TimedEvent, TranscriptLine,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One saved meeting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeetingRecord {
    /// A timestamp such as `20261106-091500`, also the file name.
    pub id: String,
    /// RFC 3339 local time the meeting started.
    pub started_at: String,
    #[ts(type = "number")]
    pub duration_ms: i64,
    /// Cost of the meeting's provider calls, in micro-dollars.
    #[ts(type = "number")]
    pub cost_micros: u64,
    pub questions: Vec<MeetingQuestion>,
    /// Everything that was said, when the user chose to keep it.
    pub transcript: Vec<TranscriptLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeetingQuestion {
    /// When it was asked, from the start of the meeting.
    #[ts(type = "number")]
    pub at_ms: i64,
    pub text: String,
    pub short: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub speaker: Option<u32>,
    pub status: QuestionStatus,
    pub points: Vec<MeetingPoint>,
    /// The sample answer.
    pub answer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeetingPoint {
    pub head: String,
    pub rest: String,
    pub done: bool,
}

/// A line in the list of saved meetings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MeetingSummary {
    pub id: String,
    pub started_at: String,
    #[ts(type = "number")]
    pub duration_ms: i64,
    pub questions: u32,
    /// Talking points ticked, out of all points.
    pub points_done: u32,
    pub points_total: u32,
    #[ts(type = "number")]
    pub cost_micros: u64,
    /// The first question's title, to tell meetings apart.
    pub first_question: String,
}

impl MeetingRecord {
    pub fn summary(&self) -> MeetingSummary {
        let points = self.questions.iter().flat_map(|q| &q.points);
        MeetingSummary {
            id: self.id.clone(),
            started_at: self.started_at.clone(),
            duration_ms: self.duration_ms,
            questions: self.questions.len() as u32,
            points_done: points.clone().filter(|p| p.done).count() as u32,
            points_total: points.count() as u32,
            cost_micros: self.cost_micros,
            first_question: self
                .questions
                .first()
                .map(|q| title(q).to_string())
                .unwrap_or_default(),
        }
    }
}

fn title(q: &MeetingQuestion) -> &str {
    if q.short.trim().is_empty() {
        &q.text
    } else {
        &q.short
    }
}

#[derive(Default)]
struct Building {
    version: u32,
    q: Option<MeetingQuestion>,
    t_ms: i64,
    statuses: Vec<PointStatus>,
}

/// Rebuilds a meeting from the events of its session. Returns `None` when
/// nothing worth keeping happened: no question and, with `keep_transcript`,
/// nothing said either.
pub fn record(
    events: &[TimedEvent],
    id: String,
    started_at: String,
    keep_transcript: bool,
) -> Option<MeetingRecord> {
    let mut start: Option<i64> = None;
    let mut first_t: Option<i64> = None;
    let mut last_t = 0i64;
    let mut cost = 0u64;
    let mut questions: BTreeMap<u32, Building> = BTreeMap::new();
    let mut transcript = Vec::new();
    for te in events {
        first_t.get_or_insert(te.t_ms);
        last_t = last_t.max(te.t_ms);
        match &te.event {
            MeetCueEvent::SessionState {
                state: SessionState::Running,
                t_ms,
            } => {
                start.get_or_insert(*t_ms);
            }
            MeetCueEvent::QuestionNew {
                id,
                version,
                text,
                short,
                status,
                t_ms,
                speaker,
                ..
            } => {
                let b = questions.entry(*id).or_default();
                if *version < b.version {
                    continue;
                }
                let keep_hints = *version == b.version;
                let old = b.q.take();
                let (points, answer, old_status) = match old {
                    Some(o) if keep_hints => (o.points, o.answer, Some(o.status)),
                    Some(o) => (Vec::new(), String::new(), Some(o.status)),
                    None => (Vec::new(), String::new(), None),
                };
                if !keep_hints {
                    b.statuses.clear();
                }
                b.version = *version;
                b.t_ms = *t_ms;
                b.q = Some(MeetingQuestion {
                    at_ms: 0,
                    text: text.clone(),
                    short: short.clone(),
                    speaker: *speaker,
                    status: old_status.unwrap_or(*status),
                    points,
                    answer,
                });
            }
            MeetCueEvent::QuestionRemoved { id } => {
                questions.remove(id);
            }
            MeetCueEvent::QuestionPoint {
                id,
                version,
                head,
                rest,
                status,
                ..
            } => {
                if let Some(b) = questions.get_mut(id).filter(|b| b.version == *version) {
                    if let Some(q) = b.q.as_mut() {
                        q.points.push(MeetingPoint {
                            head: head.clone(),
                            rest: rest.clone(),
                            done: *status == PointStatus::Done,
                        });
                        b.statuses.push(*status);
                    }
                }
            }
            MeetCueEvent::AnswerDelta { id, version, text } => {
                if let Some(q) = questions
                    .get_mut(id)
                    .filter(|b| b.version == *version)
                    .and_then(|b| b.q.as_mut())
                {
                    q.answer.push_str(text);
                }
            }
            MeetCueEvent::Coverage { id, statuses } => {
                if let Some(q) = questions.get_mut(id).and_then(|b| b.q.as_mut()) {
                    for (p, s) in q.points.iter_mut().zip(statuses) {
                        p.done = *s == PointStatus::Done;
                    }
                }
            }
            MeetCueEvent::QuestionStatus { id, status } => {
                if let Some(q) = questions.get_mut(id).and_then(|b| b.q.as_mut()) {
                    q.status = *status;
                }
            }
            MeetCueEvent::Usage { session_micros, .. } => cost = cost.max(*session_micros),
            MeetCueEvent::Transcript {
                is_final: true,
                channel,
                text,
                t0_ms,
                t1_ms,
                ..
            } if keep_transcript && !text.trim().is_empty() => transcript.push(TranscriptLine {
                channel: *channel,
                text: text.clone(),
                t0_ms: *t0_ms,
                t1_ms: *t1_ms,
            }),
            _ => {}
        }
    }
    let start = start.unwrap_or(0);
    let questions: Vec<MeetingQuestion> = questions
        .into_values()
        .filter_map(|b| {
            let mut q = b.q?;
            q.at_ms = (b.t_ms - start).max(0);
            q.answer = q.answer.trim().to_string();
            Some(q)
        })
        .collect();
    for l in &mut transcript {
        l.t0_ms = (l.t0_ms - start).max(0);
        l.t1_ms = (l.t1_ms - start).max(0);
    }
    if questions.is_empty() && transcript.is_empty() {
        return None;
    }
    Some(MeetingRecord {
        id,
        started_at,
        duration_ms: (last_t - first_t.unwrap_or(0)).max(0),
        cost_micros: cost,
        questions,
        transcript,
    })
}

fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Renders the meeting as Markdown. `t` translates `meetings.*` and
/// `overlay.*` keys of the interface locale files.
pub fn markdown(m: &MeetingRecord, t: &dyn Fn(&str) -> String) -> String {
    let date: String = m.started_at.replace('T', " ").chars().take(16).collect();
    let mut md = format!("# {} {date}\n\n", t("meetings.title"));
    let s = m.summary();
    md.push_str(&format!(
        "{} {} · {} {} · {} {}/{}\n\n",
        t("meetings.duration"),
        clock(m.duration_ms),
        t("meetings.questions"),
        s.questions,
        t("meetings.covered"),
        s.points_done,
        s.points_total
    ));
    for (i, q) in m.questions.iter().enumerate() {
        md.push_str(&format!("## {}. {}\n\n", i + 1, title(q)));
        md.push_str(&format!("`{}` {}\n\n", clock(q.at_ms), q.text));
        for p in &q.points {
            let mark = if p.done { "x" } else { " " };
            md.push_str(&format!("- [{mark}] **{}** {}\n", p.head, p.rest));
        }
        if !q.points.is_empty() {
            md.push('\n');
        }
        if !q.answer.is_empty() {
            md.push_str(&format!("{}\n\n", t("meetings.sampleAnswer")));
            for l in q.answer.lines() {
                md.push_str(&format!("> {l}\n"));
            }
            md.push('\n');
        }
    }
    if !m.transcript.is_empty() {
        md.push_str(&format!("## {}\n\n", t("meetings.transcript")));
        for l in &m.transcript {
            let who = match l.channel {
                Channel::Me => t("overlay.me"),
                Channel::Remote => t("overlay.them"),
            };
            md.push_str(&format!("`{}` **{who}** {}\n\n", clock(l.t0_ms), l.text));
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t_ms: i64, event: MeetCueEvent) -> TimedEvent {
        TimedEvent { t_ms, event }
    }

    fn question(id: u32, version: u32, text: &str, t_ms: i64) -> MeetCueEvent {
        MeetCueEvent::QuestionNew {
            id,
            version,
            text: text.into(),
            short: format!("Q{id}"),
            tags: vec![],
            sub_questions: vec![],
            status: QuestionStatus::Active,
            t_ms,
            context: vec![],
            speaker: None,
        }
    }

    fn point(id: u32, version: u32, head: &str) -> MeetCueEvent {
        MeetCueEvent::QuestionPoint {
            id,
            version,
            n: 1,
            head: head.into(),
            rest: "more".into(),
            status: PointStatus::Next,
        }
    }

    fn events() -> Vec<TimedEvent> {
        vec![
            at(
                0,
                MeetCueEvent::SessionState {
                    state: SessionState::Running,
                    t_ms: 1_000,
                },
            ),
            at(
                10,
                MeetCueEvent::Transcript {
                    channel: Channel::Remote,
                    utterance: 1,
                    is_final: true,
                    text: "What is a hash map?".into(),
                    t0_ms: 2_000,
                    t1_ms: 4_000,
                },
            ),
            at(20, question(1, 1, "What is a hash map?", 4_000)),
            at(30, point(1, 1, "Old point")),
            // A longer version replaces the hints.
            at(
                40,
                question(1, 2, "What is a hash map, and how does it grow?", 6_000),
            ),
            at(50, point(1, 2, "Buckets")),
            at(
                60,
                MeetCueEvent::AnswerDelta {
                    id: 1,
                    version: 2,
                    text: "It maps keys ".into(),
                },
            ),
            at(
                61,
                MeetCueEvent::AnswerDelta {
                    id: 1,
                    version: 2,
                    text: "to buckets.".into(),
                },
            ),
            at(
                70,
                MeetCueEvent::Coverage {
                    id: 1,
                    statuses: vec![PointStatus::Done],
                },
            ),
            at(
                80,
                MeetCueEvent::QuestionStatus {
                    id: 1,
                    status: QuestionStatus::Answered,
                },
            ),
            at(90, question(2, 1, "Not really a question", 9_000)),
            at(91, MeetCueEvent::QuestionRemoved { id: 2 }),
            at(
                95,
                MeetCueEvent::Usage {
                    provider: "p".into(),
                    model: "m".into(),
                    purpose: meetcue_core::UsagePurpose::Hints,
                    usage: meetcue_core::UsageKind::Audio { ms: 1 },
                    cost_micros: Some(10),
                    session_micros: 1234,
                },
            ),
            at(
                65_000,
                MeetCueEvent::SessionState {
                    state: SessionState::Stopped,
                    t_ms: 66_000,
                },
            ),
        ]
    }

    #[test]
    fn a_meeting_is_rebuilt_from_its_events() {
        let m = record(
            &events(),
            "id".into(),
            "2026-11-06T09:00:00+08:00".into(),
            false,
        )
        .expect("a record");
        assert_eq!(m.questions.len(), 1, "removed questions are left out");
        let q = &m.questions[0];
        assert_eq!(q.text, "What is a hash map, and how does it grow?");
        assert_eq!(q.at_ms, 5_000);
        assert_eq!(q.status, QuestionStatus::Answered);
        assert_eq!(q.points.len(), 1, "points of the older version are dropped");
        assert!(q.points[0].done);
        assert_eq!(q.answer, "It maps keys to buckets.");
        assert_eq!(m.cost_micros, 1234);
        assert_eq!(m.duration_ms, 65_000);
        assert!(
            m.transcript.is_empty(),
            "the transcript is kept only when asked"
        );
        let s = m.summary();
        assert_eq!((s.points_done, s.points_total), (1, 1));
        assert_eq!(s.first_question, "Q1");
    }

    #[test]
    fn the_transcript_is_kept_when_asked() {
        let m = record(&events(), "id".into(), String::new(), true).unwrap();
        assert_eq!(m.transcript.len(), 1);
        assert_eq!(m.transcript[0].t0_ms, 1_000);
    }

    #[test]
    fn an_empty_meeting_is_not_saved() {
        let only_start = &events()[..1];
        assert!(record(only_start, "id".into(), String::new(), true).is_none());
    }

    #[test]
    fn markdown_lists_points_and_ticks() {
        let m = record(&events(), "id".into(), "2026-11-06T09:00:00".into(), true).unwrap();
        let md = markdown(&m, &|k| k.to_string());
        assert!(md.starts_with("# meetings.title 2026-11-06 09:00"));
        assert!(md.contains("## 1. Q1"));
        assert!(md.contains("- [x] **Buckets** more"));
        assert!(md.contains("> It maps keys to buckets."));
        assert!(md.contains("**overlay.them** What is a hash map?"));
    }
}
