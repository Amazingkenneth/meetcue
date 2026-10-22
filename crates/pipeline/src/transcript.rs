//! Rolling transcript of both channels, bounded to the last ten minutes.

use std::collections::VecDeque;

use meetcue_core::{Channel, TranscriptLine};

/// How much history is kept in memory.
pub const RETENTION_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Default)]
pub struct TranscriptBuffer {
    lines: VecDeque<TranscriptLine>,
}

impl TranscriptBuffer {
    pub fn push(&mut self, line: TranscriptLine) {
        let cutoff = line.t1_ms - RETENTION_MS;
        // Keep the buffer sorted by start time even when the two channels
        // deliver slightly out of order.
        let pos = self
            .lines
            .iter()
            .rposition(|l| l.t0_ms <= line.t0_ms)
            .map(|p| p + 1)
            .unwrap_or(0);
        self.lines.insert(pos, line);
        while self.lines.front().is_some_and(|l| l.t1_ms < cutoff) {
            self.lines.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Lines that ended at or after `since_ms`.
    pub fn since(&self, since_ms: i64) -> impl Iterator<Item = &TranscriptLine> {
        self.lines.iter().filter(move |l| l.t1_ms >= since_ms)
    }

    /// The last `n` lines that started before `before_ms`.
    pub fn last_before(&self, before_ms: i64, n: usize) -> Vec<TranscriptLine> {
        let mut out: Vec<_> = self
            .lines
            .iter()
            .rev()
            .filter(|l| l.t0_ms < before_ms)
            .take(n)
            .cloned()
            .collect();
        out.reverse();
        out
    }

    /// Remote lines overlapping `[t0 - margin, t1 + margin]`.
    pub fn remote_near(
        &self,
        t0: i64,
        t1: i64,
        margin: i64,
    ) -> impl Iterator<Item = &TranscriptLine> {
        self.lines.iter().filter(move |l| {
            l.channel == Channel::Remote && l.t1_ms >= t0 - margin && l.t0_ms <= t1 + margin
        })
    }
}

/// Formats lines for a prompt as `[Me] …` / `[Them] …`.
pub fn format_for_prompt<'a>(lines: impl IntoIterator<Item = &'a TranscriptLine>) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(speaker_tag(l.channel));
        out.push(' ');
        out.push_str(l.text.trim());
        out.push('\n');
    }
    out
}

pub fn speaker_tag(channel: Channel) -> &'static str {
    match channel {
        Channel::Me => "[Me]",
        Channel::Remote => "[Them]",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ch: Channel, t0: i64, t1: i64, text: &str) -> TranscriptLine {
        TranscriptLine {
            channel: ch,
            text: text.into(),
            t0_ms: t0,
            t1_ms: t1,
        }
    }

    #[test]
    fn keeps_order_and_trims_old_lines() {
        let mut b = TranscriptBuffer::default();
        b.push(line(Channel::Remote, 0, 1000, "a"));
        b.push(line(Channel::Me, 3000, 4000, "c"));
        b.push(line(Channel::Remote, 2000, 2500, "b"));
        let texts: Vec<_> = b.since(0).map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
        b.push(line(
            Channel::Remote,
            RETENTION_MS + 1500,
            RETENTION_MS + 2000,
            "d",
        ));
        let texts: Vec<_> = b.since(0).map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["b", "c", "d"]);
        assert_eq!(format_for_prompt(b.since(0).take(2)), "[Them] b\n[Me] c\n");
        assert_eq!(b.last_before(5000, 1)[0].text, "c");
        assert_eq!(b.remote_near(1900, 2100, 0).count(), 1);
    }
}
