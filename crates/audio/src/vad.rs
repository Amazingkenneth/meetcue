//! Energy-based voice activity detection.

/// RMS level of 16-bit samples in dBFS; −100 for digital silence.
pub fn level_db(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / i16::MAX as f64;
    if rms <= 1e-5 {
        -100.0
    } else {
        (20.0 * rms.log10()) as f32
    }
}

/// Marks a frame as voiced when it is above the threshold, and keeps the
/// next few frames voiced so word endings are not clipped.
#[derive(Debug, Clone)]
pub struct Vad {
    threshold_db: f32,
    hangover_frames: u32,
    remaining: u32,
}

impl Vad {
    pub fn new(threshold_db: f32) -> Self {
        Self {
            threshold_db,
            hangover_frames: 10, // 200 ms
            remaining: 0,
        }
    }

    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db;
    }

    pub fn process(&mut self, level_db: f32) -> bool {
        if level_db >= self.threshold_db {
            self.remaining = self.hangover_frames;
            true
        } else if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level_db(&[0; 320]), -100.0);
        let full: Vec<i16> = (0..320)
            .map(|i| if i % 2 == 0 { i16::MAX } else { -i16::MAX })
            .collect();
        assert!(level_db(&full).abs() < 0.1);
        let quiet = vec![33i16; 320]; // about -60 dBFS
        assert!((level_db(&quiet) + 60.0).abs() < 1.0);
    }

    #[test]
    fn hangover() {
        let mut v = Vad::new(-40.0);
        assert!(!v.process(-60.0));
        assert!(v.process(-20.0));
        let tail: Vec<bool> = (0..12).map(|_| v.process(-60.0)).collect();
        assert_eq!(tail.iter().filter(|&&b| b).count(), 10);
        assert!(!tail[11]);
    }
}
