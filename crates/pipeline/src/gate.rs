//! Decides which audio frames are sent to the recognizer.
//!
//! Voiced frames are always sent. After speech, silence is still sent for
//! `hangover_ms` so the provider can detect the end of the sentence; after
//! that, frames are dropped (saving cost) and a keep-alive is sent once per
//! second instead.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateDecision {
    Send,
    /// Silence reached the hangover limit: ask the provider to finalize.
    SendAndFinalize,
    Drop,
    KeepAlive,
}

#[derive(Debug)]
pub struct SendGate {
    hangover_ms: i64,
    keepalive_ms: i64,
    silence_ms: i64,
    since_keepalive_ms: i64,
    finalized: bool,
}

impl SendGate {
    pub fn new(hangover_ms: i64) -> Self {
        Self {
            hangover_ms,
            keepalive_ms: 1000,
            // Start closed so leading silence is not billed.
            silence_ms: i64::MAX / 2,
            since_keepalive_ms: 0,
            finalized: true,
        }
    }

    pub fn decide(&mut self, voiced: bool, frame_ms: i64) -> GateDecision {
        if voiced {
            self.silence_ms = 0;
            self.finalized = false;
            self.since_keepalive_ms = 0;
            return GateDecision::Send;
        }
        self.silence_ms = self.silence_ms.saturating_add(frame_ms);
        if self.silence_ms <= self.hangover_ms {
            if self.silence_ms + frame_ms > self.hangover_ms && !self.finalized {
                self.finalized = true;
                return GateDecision::SendAndFinalize;
            }
            return GateDecision::Send;
        }
        self.since_keepalive_ms += frame_ms;
        if self.since_keepalive_ms >= self.keepalive_ms {
            self.since_keepalive_ms = 0;
            GateDecision::KeepAlive
        } else {
            GateDecision::Drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_speech_and_hangover_then_keeps_alive() {
        let mut g = SendGate::new(800);
        assert_eq!(g.decide(false, 20), GateDecision::Drop);
        assert_eq!(g.decide(true, 20), GateDecision::Send);
        let decisions: Vec<_> = (0..40).map(|_| g.decide(false, 20)).collect();
        assert!(decisions[..39].iter().all(|d| *d == GateDecision::Send));
        assert_eq!(decisions[39], GateDecision::SendAndFinalize);
        let after: Vec<_> = (0..100).map(|_| g.decide(false, 20)).collect();
        assert_eq!(
            after
                .iter()
                .filter(|d| **d == GateDecision::KeepAlive)
                .count(),
            2
        );
        assert!(!after.contains(&GateDecision::Send));
        assert_eq!(g.decide(true, 20), GateDecision::Send);
    }
}
