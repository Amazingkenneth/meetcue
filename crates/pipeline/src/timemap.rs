//! Maps ASR time offsets back to capture time.
//!
//! Providers report times relative to the audio they received. Silent frames
//! are not sent, so that audio is not continuous in wall-clock time: each
//! written chunk is recorded as (stream offset, capture time) and results are
//! mapped through that table.

#[derive(Debug, Default, Clone)]
pub struct TimeMap {
    /// (stream offset ms, capture ts ms) at the start of each contiguous run.
    anchors: Vec<(i64, i64)>,
    sent_ms: i64,
    last_capture_end: Option<i64>,
    /// Used when no audio has been written (e.g. replayed transcripts).
    fallback_base: i64,
}

impl TimeMap {
    pub fn new(fallback_base: i64) -> Self {
        Self {
            fallback_base,
            ..Default::default()
        }
    }

    /// Records that `duration_ms` of audio captured at `capture_ts` was sent.
    pub fn record(&mut self, capture_ts: i64, duration_ms: i64) {
        let contiguous = self
            .last_capture_end
            .is_some_and(|end| (capture_ts - end).abs() <= 5);
        if !contiguous {
            self.anchors.push((self.sent_ms, capture_ts));
            // Keep memory bounded on very long sessions.
            if self.anchors.len() > 4096 {
                self.anchors.drain(..2048);
            }
        }
        self.sent_ms += duration_ms;
        self.last_capture_end = Some(capture_ts + duration_ms);
    }

    pub fn sent_ms(&self) -> i64 {
        self.sent_ms
    }

    /// Capture time of a stream offset.
    pub fn map(&self, offset_ms: i64) -> i64 {
        let idx = self.anchors.partition_point(|(off, _)| *off <= offset_ms);
        if idx == 0 {
            return match self.anchors.first() {
                Some((off, cap)) => cap - (off - offset_ms),
                None => self.fallback_base + offset_ms,
            };
        }
        let (off, cap) = self.anchors[idx - 1];
        cap + (offset_ms - off)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_across_gaps() {
        let mut m = TimeMap::new(0);
        // 1 s of speech from t=10 000, a 5 s gap, then 1 s more from t=16 000.
        for i in 0..50 {
            m.record(10_000 + i * 20, 20);
        }
        for i in 0..50 {
            m.record(16_000 + i * 20, 20);
        }
        assert_eq!(m.sent_ms(), 2_000);
        assert_eq!(m.map(0), 10_000);
        assert_eq!(m.map(500), 10_500);
        assert_eq!(m.map(1_000), 16_000);
        assert_eq!(m.map(1_500), 16_500);
    }

    #[test]
    fn falls_back_without_audio() {
        let m = TimeMap::new(1_000);
        assert_eq!(m.map(250), 1_250);
    }
}
