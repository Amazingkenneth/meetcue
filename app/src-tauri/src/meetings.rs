//! Saved meetings (`meetings/<id>.json`): written when listening stops,
//! listed and shown in Settings › Meetings, exported as Markdown.

use std::fs;
use std::path::PathBuf;

use chrono::{Duration as ChronoDuration, Local};
use meetcue_core::ipc::AppEvent;
use meetcue_core::{codes, MeetCueError};
use meetcue_pipeline::meeting::{self, MeetingRecord, MeetingSummary};
use meetcue_pipeline::HintBackend;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::config;
use crate::state::AppState;

fn err(detail: impl Into<String>) -> MeetCueError {
    MeetCueError::new(codes::SYS_INTERNAL, detail)
}

fn path(state: &AppState, id: &str) -> Option<PathBuf> {
    // Ids are generated timestamps; refuse anything that could leave the folder.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some(state.paths.meetings_dir().join(format!("{id}.json")))
}

/// Saves the session that just ended, if the user keeps meetings and
/// something worth keeping happened. Practice runs have their own reports
/// and demo sessions aren't real meetings, so neither is saved.
pub fn save_finished(app: &AppHandle) {
    let state = app.state::<AppState>();
    let s = state.settings();
    if state.demo || !s.history.save_meetings || crate::practice::is_active(app) {
        return;
    }
    let events = state.backend.history();
    // A stop from the user and the session's own end can both report the
    // same session; save it once.
    static LAST: std::sync::Mutex<(usize, i64)> = std::sync::Mutex::new((0, -1));
    let mark = (events.len(), events.last().map(|e| e.t_ms).unwrap_or(-1));
    {
        let mut last = LAST.lock().unwrap();
        if *last == mark {
            return;
        }
        *last = mark;
    }
    let duration = events.last().map(|e| e.t_ms).unwrap_or(0);
    let started = Local::now() - ChronoDuration::milliseconds(duration);
    let id = started.format("%Y%m%d-%H%M%S").to_string();
    let Some(record) = meeting::record(
        &events,
        id.clone(),
        started.to_rfc3339(),
        s.history.save_transcript,
    ) else {
        return;
    };
    let Some(file) = path(&state, &id) else {
        return;
    };
    let json = serde_json::to_string_pretty(&record).expect("serialize meeting");
    match config::write_atomic(&file, json.as_bytes()) {
        Ok(()) => {
            tracing::info!(questions = record.questions.len(), "meeting saved");
            let _ = app.emit(crate::APP_EVENT, AppEvent::MeetingSaved { id });
        }
        Err(e) => tracing::warn!("could not save the meeting: {e}"),
    }
    prune(&state, s.history.keep_days);
}

/// Deletes meetings older than `keep_days` (0 keeps everything).
fn prune(state: &AppState, keep_days: u32) {
    if keep_days == 0 {
        return;
    }
    let cutoff = (Local::now() - ChronoDuration::days(keep_days as i64))
        .format("%Y%m%d-%H%M%S")
        .to_string();
    for s in list(state) {
        if s.id < cutoff {
            if let Some(p) = path(state, &s.id) {
                let _ = fs::remove_file(p);
            }
        }
    }
}

fn read(state: &AppState, id: &str) -> Result<MeetingRecord, MeetCueError> {
    let file = path(state, id).ok_or_else(|| err("bad meeting id"))?;
    let text = fs::read_to_string(file).map_err(|e| err(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| err(e.to_string()))
}

/// Saved meetings, newest first.
pub fn list(state: &AppState) -> Vec<MeetingSummary> {
    let mut out: Vec<MeetingSummary> = fs::read_dir(state.paths.meetings_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<MeetingRecord>(&s).ok())
        .map(|m| m.summary())
        .collect();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

pub fn get(app: &AppHandle, id: &str) -> Result<MeetingRecord, MeetCueError> {
    read(&app.state::<AppState>(), id)
}

/// Deletes one meeting, or all of them when `id` is empty.
pub fn delete(app: &AppHandle, id: &str) -> Result<(), MeetCueError> {
    let state = app.state::<AppState>();
    if id.is_empty() {
        for s in list(&state) {
            if let Some(p) = path(&state, &s.id) {
                fs::remove_file(p).map_err(|e| err(e.to_string()))?;
            }
        }
        return Ok(());
    }
    let file = path(&state, id).ok_or_else(|| err("bad meeting id"))?;
    fs::remove_file(file).map_err(|e| err(e.to_string()))
}

/// Saves the meeting as Markdown where the user chooses.
pub async fn export(app: &AppHandle, id: &str) -> Result<Option<String>, MeetCueError> {
    let record = get(app, id)?;
    let lang = app.state::<AppState>().language();
    let md = meeting::markdown(&record, &|k| crate::i18n::t(lang, k));
    let dialog = app
        .dialog()
        .file()
        .set_file_name(format!("meeting-{}.md", record.id))
        .add_filter("Markdown", &["md"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file())
        .await
        .map_err(|e| err(e.to_string()))?;
    let Some(file) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    fs::write(&file, md).map_err(|e| err(e.to_string()))?;
    Ok(Some(file.display().to_string()))
}
