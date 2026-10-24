//! Decides which audio frames are sent to the recognizer.
//!
//! Voiced frames are always sent. After speech, silence is still sent for
//! `hangover_ms` so the provider can detect the end of the sentence; after
//! that, frames are dropped (saving cost) and a keep-alive is sent once per
//! second instead. While paused nothing is sent but the keep-alives, so the
//! provider doesn't close the connection (Soniox after 40 s, Deepgram after
//! 10 s). When the gate opens, the caller sends the last few dropped frames
//! first ([`SendGate::opened`]) so quiet word onsets aren't cut off.

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
    /// The gate was closed before the last `Send`, i.e. speech just began.
    opened: bool,
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
            opened: false,
        }
    }

    /// Whether the last decision opened the gate after dropped frames.
    pub fn opened(&self) -> bool {
        self.opened
    }

    /// A frame while paused: nothing is sent, but keep-alives continue.
    pub fn paused(&mut self, frame_ms: i64) -> GateDecision {
        self.silence_ms = i64::MAX / 2;
        self.finalized = true;
        self.opened = false;
        self.keepalive(frame_ms)
    }

    fn keepalive(&mut self, frame_ms: i64) -> GateDecision {
        self.since_keepalive_ms += frame_ms;
        if self.since_keepalive_ms >= self.keepalive_ms {
            self.since_keepalive_ms = 0;
            GateDecision::KeepAlive
        } else {
            GateDecision::Drop
        }
    }

    pub fn decide(&mut self, voiced: bool, frame_ms: i64) -> GateDecision {
        self.opened = false;
        if voiced {
            self.opened = self.silence_ms > self.hangover_ms;
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
        self.keepalive(frame_ms)
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

    #[test]
    fn reports_when_speech_reopens_the_gate() {
        let mut g = SendGate::new(800);
        g.decide(true, 20);
        assert!(g.opened(), "first speech opens the gate");
        g.decide(true, 20);
        assert!(!g.opened());
        // A short pause inside the hangover doesn't close the gate.
        for _ in 0..10 {
            g.decide(false, 20);
        }
        g.decide(true, 20);
        assert!(!g.opened());
        for _ in 0..60 {
            g.decide(false, 20);
        }
        g.decide(true, 20);
        assert!(g.opened());
    }

    #[test]
    fn keeps_the_connection_alive_while_paused() {
        let mut g = SendGate::new(800);
        g.decide(true, 20);
        let paused: Vec<_> = (0..250).map(|_| g.paused(20)).collect();
        assert!(!paused.contains(&GateDecision::Send));
        assert_eq!(
            paused
                .iter()
                .filter(|d| **d == GateDecision::KeepAlive)
                .count(),
            5
        );
        // Speech after the pause reopens the gate.
        assert_eq!(g.decide(true, 20), GateDecision::Send);
        assert!(g.opened());
    }
}
