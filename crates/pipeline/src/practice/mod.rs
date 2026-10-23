//! Practice mode: scripts imported from subtitles or question lists, the
//! playback schedule, and the review report.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use meetcue_core::settings::{HintLevel, PlaybackMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PracticeKind {
    Questions,
    Subtitles,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PracticeItem {
    pub id: u32,
    pub text: String,
    pub speaker: Option<String>,
    #[ts(type = "number | null")]
    pub start_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub end_ms: Option<i64>,
    /// Subtitles: a line spoken by the user (hidden and used as reference).
    pub is_me: bool,
    pub is_question: bool,
    pub reference: Option<String>,
    pub category: Option<String>,
    pub followups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PracticeScript {
    pub kind: PracticeKind,
    pub title: String,
    pub items: Vec<PracticeItem>,
    /// Speakers found in subtitles.
    pub speakers: Vec<String>,
    pub me_speaker: Option<String>,
    /// Subtitles without speaker names need question/answer classification.
    pub needs_classification: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FillerCount {
    pub word: String,
    pub count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionReview {
    pub question: String,
    pub answer: String,
    pub reference: Option<String>,
    /// End of the question to the user's first recognised word.
    #[ts(type = "number | null")]
    pub start_latency_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub answer_ms: Option<i64>,
    pub points_done: u32,
    pub points_total: u32,
    pub fillers: Vec<FillerCount>,
    /// Characters per minute (Chinese) or words per minute (English).
    pub pace: Option<f64>,
    pub pace_unit: String,
    pub feedback: Vec<String>,
    pub follow_up: Option<String>,
    pub skipped: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReviewSummary {
    pub questions: u32,
    pub answered: u32,
    pub avg_start_latency_ms: Option<f64>,
    pub avg_coverage: Option<f64>,
    pub total_fillers: u32,
    pub avg_pace: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PracticeReport {
    pub id: String,
    pub title: String,
    pub started_at: String,
    #[ts(type = "number")]
    pub duration_ms: i64,
    pub playback: PlaybackMode,
    pub hint_level: HintLevel,
    /// Simulated recognition delay was used (model latency is still real).
    pub simulated: bool,
    pub questions: Vec<QuestionReview>,
    pub summary: ReviewSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PracticeReportSummary {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub questions: u32,
    pub avg_start_latency_ms: Option<f64>,
    pub avg_coverage: Option<f64>,
}
