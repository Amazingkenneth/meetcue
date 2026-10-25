//! File logging: daily files in `logs/`, kept for seven days. The most recent
//! lines are also kept in memory for the activity log in the settings.
//!
//! Transcripts are only logged when debug logging is on, and error details
//! are redacted where errors are created (`MeetCueError::new`).

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use meetcue_core::ipc::LogLine;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

/// Lines kept in memory.
const KEEP: usize = 1000;

struct Recent {
    next: u64,
    lines: VecDeque<LogLine>,
}

static RECENT: Mutex<Recent> = Mutex::new(Recent {
    next: 1,
    lines: VecDeque::new(),
});

/// Lines logged after `after` (a previous line's `seq`), oldest first.
pub fn recent(after: u64) -> Vec<LogLine> {
    let r = RECENT.lock().unwrap();
    r.lines.iter().filter(|l| l.seq > after).cloned().collect()
}

fn push(text: &str) {
    let mut r = RECENT.lock().unwrap();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let seq = r.next;
        r.next += 1;
        r.lines.push_back(LogLine {
            seq,
            line: line.to_string(),
        });
        if r.lines.len() > KEEP {
            r.lines.pop_front();
        }
    }
}

/// Collects one formatted event and stores it when dropped.
#[derive(Default)]
struct MemoryWriter(Vec<u8>);

impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for MemoryWriter {
    fn drop(&mut self) {
        push(&String::from_utf8_lossy(&self.0));
    }
}

/// Writes panics to the log. A windowed release build has no console, so
/// without this a panic in a command, task or capture thread leaves no trace.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        tracing::error!(
            "panic in thread {}: {info}\n{}",
            thread.name().unwrap_or("unnamed"),
            std::backtrace::Backtrace::force_capture()
        );
        default(info);
    }));
}

pub fn init(logs_dir: &Path, debug: bool) -> Option<WorkerGuard> {
    log_panics();
    let level = if debug { "debug" } else { "info" };
    let filter = EnvFilter::try_from_env("MEETCUE_LOG").unwrap_or_else(|_| {
        EnvFilter::new(format!("{level},meetcue={level},meetcue_pipeline={level}"))
    });
    let memory = MemoryWriter::default;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("meetcue")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir);
    let fmt = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    match appender {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            fmt.with_writer(writer.and(memory)).try_init().ok()?;
            Some(guard)
        }
        // Without a log folder the activity log still works.
        Err(_) => {
            let _ = fmt.with_writer(memory).try_init();
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_latest_lines_in_order() {
        let before = recent(0).last().map_or(0, |l| l.seq);
        push("one\ntwo\n\n");
        let new = recent(before);
        assert_eq!(
            new.iter().map(|l| l.line.as_str()).collect::<Vec<_>>(),
            ["one", "two"]
        );
        assert!(recent(new[1].seq).is_empty());
        for i in 0..KEEP + 5 {
            push(&format!("line {i}"));
        }
        assert_eq!(recent(0).len(), KEEP);
    }
}
