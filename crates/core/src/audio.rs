use crate::event::Channel;

/// Samples per second of every frame handed to the pipeline.
pub const SAMPLE_RATE: u32 = 16_000;
/// Frame length in milliseconds.
pub const FRAME_MS: u32 = 20;
/// Samples in one frame (16 kHz × 20 ms).
pub const FRAME_SAMPLES: usize = (SAMPLE_RATE * FRAME_MS / 1000) as usize;

/// One 20 ms block of 16 kHz mono 16-bit PCM from either capture channel.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    pub channel: Channel,
    pub samples: Vec<i16>,
    /// Capture time of the first sample, in [`crate::clock`] milliseconds.
    pub capture_ts_ms: i64,
    /// Voice activity decision for this frame.
    pub voiced: bool,
    /// RMS level in dBFS (−100 for digital silence).
    pub level_db: f32,
}

impl AudioFrame {
    /// A silent frame, used to fill gaps when loopback capture stops delivering data.
    pub fn silence(channel: Channel, capture_ts_ms: i64) -> Self {
        Self {
            channel,
            samples: vec![0; FRAME_SAMPLES],
            capture_ts_ms,
            voiced: false,
            level_db: -100.0,
        }
    }

    pub fn duration_ms(&self) -> i64 {
        (self.samples.len() as i64 * 1000) / SAMPLE_RATE as i64
    }
}
