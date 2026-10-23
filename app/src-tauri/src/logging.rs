//! File logging: daily files in `logs/`, kept for seven days.
//!
//! Transcripts are only logged when debug logging is on, and error details
//! are redacted where errors are created (`MeetCueError::new`).

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;

pub fn init(logs_dir: &Path, debug: bool) -> Option<WorkerGuard> {
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("meetcue")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let level = if debug { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("MEETCUE_LOG").unwrap_or_else(|_| {
        EnvFilter::new(format!("{level},meetcue={level},meetcue_pipeline={level}"))
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}
