//! Silence padding for providers that end sentences only by hearing silence.
//!
//! Alibaba and Tencent have no "finalize now" message: a sentence ends after
//! the server has heard enough quiet audio. MeetCue's send gate already sends
//! that much silence after speech, so in a meeting nothing extra is needed.
//! When audio stops right after speech (the latency probe, a flush), the
//! stream sends the missing silence itself.

/// Level below which audio counts as quiet, in dBFS (matches the probe).
const QUIET_DB: f32 = -40.0;

/// Tracks how much quiet audio was sent last.
#[derive(Debug, Default, Clone)]
pub struct TrailingQuiet {
    quiet_ms: u64,
}

impl TrailingQuiet {
    pub fn push(&mut self, pcm16k: &[i16]) {
        let ms = pcm16k.len() as u64 / 16;
        if pcm16k.is_empty() {
            return;
        }
        if crate::text_level(pcm16k) < QUIET_DB {
            self.quiet_ms += ms;
        } else {
            self.quiet_ms = 0;
        }
    }

    /// Silence still needed so the server hears `needed_ms` of it.
    pub fn missing(&self, needed_ms: u64) -> u64 {
        needed_ms.saturating_sub(self.quiet_ms)
    }
}

/// `ms` of silence at 16 kHz.
pub fn silence(ms: u64) -> Vec<i16> {
    vec![0; (ms * 16) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_trailing_quiet() {
        let mut q = TrailingQuiet::default();
        assert_eq!(q.missing(700), 700);
        let loud: Vec<i16> = (0..320)
            .map(|i| if i % 2 == 0 { 8000 } else { -8000 })
            .collect();
        q.push(&silence(500));
        q.push(&loud);
        assert_eq!(q.missing(700), 700, "speech resets the count");
        q.push(&silence(300));
        assert_eq!(q.missing(700), 400);
        q.push(&silence(600));
        assert_eq!(q.missing(700), 0);
    }
}
