//! Automatic gain for quiet voices on the system sound channel.
//!
//! Meeting apps often play a soft-spoken participant 20 dB or more below a
//! normal voice. Recognizers miss such speech or turn it into fragments, so
//! the remote channel is brought up towards a normal speaking level before
//! it is sent. The gain follows the level of voiced frames only, never
//! attenuates, is capped so background noise isn't blown up, and is lowered
//! for any frame whose peaks would clip.

/// Speech level the gain aims for, in dBFS RMS.
pub const TARGET_DB: f32 = -22.0;
/// Most the gain may raise a frame, in dB (about 16×).
pub const MAX_GAIN_DB: f32 = 24.0;
/// Highest sample magnitude after gain, as a fraction of full scale.
const PEAK_LIMIT: f32 = 0.9;
/// How fast the speech level follows louder and quieter frames.
const ATTACK: f32 = 0.4;
const RELEASE: f32 = 0.08;

#[derive(Debug, Clone, Default)]
pub struct Agc {
    /// Tracked speech level in dBFS; `None` until the first voiced frame.
    speech_db: Option<f32>,
    /// Linear gain applied at the end of the previous frame.
    gain: f32,
}

impl Agc {
    pub fn new() -> Self {
        Self {
            speech_db: None,
            gain: 1.0,
        }
    }

    /// Current gain in dB.
    pub fn gain_db(&self) -> f32 {
        20.0 * self.gain.max(1e-6).log10()
    }

    /// Raises `samples` (mono, −1…1) in place. `level_db` is the frame's
    /// level before gain and `voiced` the voice detector's verdict for it.
    pub fn process(&mut self, samples: &mut [f32], level_db: f32, voiced: bool) {
        // Only frames that carry speech move the gain; hangover frames and
        // silence keep it, so the noise between words is not chased.
        if voiced && level_db > -90.0 {
            let env = match self.speech_db {
                None => level_db,
                Some(prev) => {
                    let k = if level_db > prev { ATTACK } else { RELEASE };
                    prev + (level_db - prev) * k
                }
            };
            self.speech_db = Some(env);
        }
        let want_db = self
            .speech_db
            .map_or(0.0, |env| (TARGET_DB - env).clamp(0.0, MAX_GAIN_DB));
        let mut target = 10f32.powf(want_db / 20.0);
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        if peak > 0.0 {
            target = target.min((PEAK_LIMIT / peak).max(1.0));
        }
        // Ramp up across the frame so gain changes don't click; drop at once
        // when the frame would otherwise clip.
        let start = self.gain.min(target);
        let n = samples.len().max(1) as f32;
        for (i, s) in samples.iter_mut().enumerate() {
            let g = start + (target - start) * (i as f32 + 1.0) / n;
            *s = (*s * g).clamp(-1.0, 1.0);
        }
        self.gain = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms_db(s: &[f32]) -> f32 {
        let sum: f32 = s.iter().map(|x| x * x).sum();
        20.0 * (sum / s.len() as f32).sqrt().log10()
    }

    fn tone(amp: f32) -> Vec<f32> {
        (0..320)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 16_000.0).sin())
            .collect()
    }

    #[test]
    fn quiet_speech_is_raised_towards_the_target() {
        let mut agc = Agc::new();
        let mut last = Vec::new();
        for _ in 0..50 {
            let mut f = tone(0.01); // about −43 dBFS
            let level = rms_db(&f);
            agc.process(&mut f, level, true);
            last = f;
        }
        let out = rms_db(&last);
        assert!((out - TARGET_DB).abs() < 2.0, "level after gain: {out}");
    }

    #[test]
    fn gain_is_capped_and_never_attenuates() {
        let mut agc = Agc::new();
        for _ in 0..50 {
            let mut f = tone(0.0005); // about −69 dBFS
            let level = rms_db(&f);
            agc.process(&mut f, level, true);
        }
        assert!(agc.gain_db() <= MAX_GAIN_DB + 0.01);
        let mut loud = Agc::new();
        let mut f = tone(0.5);
        let before = f.clone();
        let level = rms_db(&f);
        loud.process(&mut f, level, true);
        assert_eq!(f, before);
    }

    #[test]
    fn peaks_never_clip() {
        let mut agc = Agc::new();
        for _ in 0..20 {
            let mut f = tone(0.01);
            let level = rms_db(&f);
            agc.process(&mut f, level, true);
        }
        // A sudden loud frame right after quiet speech.
        let mut f = tone(0.8);
        let level = rms_db(&f);
        agc.process(&mut f, level, true);
        assert!(f.iter().all(|s| s.abs() <= PEAK_LIMIT + 1e-4));
    }

    #[test]
    fn silence_does_not_move_the_gain() {
        let mut agc = Agc::new();
        for _ in 0..10 {
            let mut f = vec![0.0001; 320];
            agc.process(&mut f, -80.0, false);
        }
        assert!(agc.gain_db().abs() < 0.01);
    }
}
