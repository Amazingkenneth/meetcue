//! Practice mode runner (filled in by the practice milestone).

use tauri::AppHandle;

pub struct PracticeRunner;

#[derive(Debug, Clone, Copy)]
pub enum Control {
    Pause,
    Resume,
    Skip,
    Stop,
    Answered,
}

pub fn control(_app: &AppHandle, _c: Control) {}

use meetcue_core::{codes, MeetCueError};
use meetcue_pipeline::practice::{PracticeReport, PracticeReportSummary, PracticeScript};

fn later() -> MeetCueError {
    MeetCueError::new(codes::SYS_UNSUPPORTED, "practice mode")
}

pub async fn import(_app: &AppHandle) -> Result<Option<PracticeScript>, MeetCueError> {
    Err(later())
}

pub async fn start(_app: &AppHandle, _script: PracticeScript) -> Result<(), MeetCueError> {
    Err(later())
}

pub fn reports(_app: &AppHandle) -> Vec<PracticeReportSummary> {
    Vec::new()
}

pub fn report(_app: &AppHandle, _id: &str) -> Result<PracticeReport, MeetCueError> {
    Err(later())
}

pub async fn export(_app: &AppHandle, _id: &str) -> Result<Option<String>, MeetCueError> {
    Err(later())
}
