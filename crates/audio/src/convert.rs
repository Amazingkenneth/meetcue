//! Converts device audio to 16 kHz mono 16-bit PCM frames.
//!
//! Devices usually deliver 44.1 or 48 kHz stereo 32-bit float. Sending that
//! as 16-bit PCM would be noise to a recognizer, so every chunk is decoded
//! from the device format, mixed down to mono, resampled and cut into 20 ms
//! frames stamped with their capture time.

use meetcue_core::audio::{FRAME_MS, FRAME_SAMPLES, SAMPLE_RATE};
use meetcue_core::{AudioFrame, Channel};
use rubato::{FftFixedInOut, Resampler};

use crate::vad::{level_db, Vad};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    I16,
    /// Packed 24-bit (3 bytes per sample).
    I24,
    /// 32-bit container; 24-bit-in-32 devices also use this.
    I32,
}

impl SampleFormat {
    pub fn bytes(self) -> usize {
        match self {
            SampleFormat::F32 | SampleFormat::I32 => 4,
            SampleFormat::I16 => 2,
            SampleFormat::I24 => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFormat {
    pub rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
}

impl InputFormat {
    pub fn block_align(&self) -> usize {
        self.format.bytes() * self.channels as usize
    }
}

/// Gap after which the frame clock restarts from the capture time.
const RESYNC_MS: i64 = 100;

pub struct FrameConverter {
    channel: Channel,
    fmt: InputFormat,
    resampler: Option<FftFixedInOut<f32>>,
    /// Mono samples at the input rate waiting for a full resampler chunk.
    pending_in: Vec<f32>,
    /// Mono samples at 16 kHz waiting for a full frame.
    pending_out: Vec<f32>,
    /// Capture time of `pending_out[0]`.
    next_ts: Option<i64>,
    /// When capture started; silence is filled from here until the device
    /// delivers its first data.
    started_at: Option<i64>,
    vad: Vad,
}

impl FrameConverter {
    pub fn new(channel: Channel, fmt: InputFormat, vad_threshold_db: f32) -> Self {
        let resampler = if fmt.rate == SAMPLE_RATE {
            None
        } else {
            let chunk = (fmt.rate / 100).max(1) as usize;
            FftFixedInOut::<f32>::new(fmt.rate as usize, SAMPLE_RATE as usize, chunk, 1).ok()
        };
        Self {
            channel,
            fmt,
            resampler,
            pending_in: Vec::new(),
            pending_out: Vec::new(),
            next_ts: None,
            started_at: None,
            vad: Vad::new(vad_threshold_db),
        }
    }

    pub fn format(&self) -> InputFormat {
        self.fmt
    }

    /// Marks when capture started, so [`Self::fill_silence`] keeps time from
    /// then even if the device has delivered nothing yet: loopback capture
    /// stays empty until something plays, and without frames the recognizer
    /// would hear nothing at all and close the stream.
    pub fn start_clock(&mut self, ts: i64) {
        self.started_at = Some(ts);
    }

    /// Time the next emitted frame will carry.
    pub fn next_ts(&self) -> Option<i64> {
        self.next_ts
    }

    /// Decodes interleaved device bytes captured starting at `capture_ts`.
    pub fn push_bytes(&mut self, bytes: &[u8], capture_ts: i64, out: &mut Vec<AudioFrame>) {
        let block = self.fmt.block_align();
        let frames = bytes.len() / block;
        if frames == 0 {
            return;
        }
        self.sync_clock(capture_ts);
        let ch = self.fmt.channels as usize;
        let bps = self.fmt.format.bytes();
        for f in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                let off = f * block + c * bps;
                sum += decode(self.fmt.format, &bytes[off..off + bps]);
            }
            self.pending_in.push(sum / ch as f32);
        }
        self.resample();
        self.emit(out);
    }

    /// Mono f32 samples at the input rate (used by tests and file sources).
    pub fn push_mono(&mut self, samples: &[f32], capture_ts: i64, out: &mut Vec<AudioFrame>) {
        self.sync_clock(capture_ts);
        self.pending_in.extend_from_slice(samples);
        self.resample();
        self.emit(out);
    }

    /// Emits silent frames up to `now` when the device has stopped delivering
    /// data (loopback capture produces nothing while nothing plays). Keeps the
    /// timeline continuous for voice detection and the recognizer.
    pub fn fill_silence(&mut self, now: i64, out: &mut Vec<AudioFrame>) {
        let Some(next) = self.next_ts.or(self.started_at) else {
            return;
        };
        if now - next < RESYNC_MS {
            return;
        }
        // Drop partial data: it is older than the gap being filled.
        self.pending_in.clear();
        self.pending_out.clear();
        let mut ts = next;
        while ts + FRAME_MS as i64 <= now {
            let level = -100.0;
            let voiced = self.vad.process(level);
            out.push(AudioFrame {
                channel: self.channel,
                samples: vec![0; FRAME_SAMPLES],
                capture_ts_ms: ts,
                voiced,
                level_db: level,
            });
            ts += FRAME_MS as i64;
        }
        self.next_ts = Some(ts);
    }

    fn sync_clock(&mut self, capture_ts: i64) {
        // Time already buffered but not yet emitted, in ms at 16 kHz.
        let buffered = (self.pending_out.len() as i64 * 1000) / SAMPLE_RATE as i64
            + (self.pending_in.len() as i64 * 1000) / self.fmt.rate.max(1) as i64;
        match self.next_ts {
            None => self.next_ts = Some(capture_ts),
            Some(next) => {
                let expected = next + buffered;
                if capture_ts - expected > RESYNC_MS {
                    // A real gap: start a new timeline at this chunk.
                    self.pending_in.clear();
                    self.pending_out.clear();
                    self.next_ts = Some(capture_ts);
                }
            }
        }
    }

    fn resample(&mut self) {
        match self.resampler.as_mut() {
            None => self.pending_out.append(&mut self.pending_in),
            Some(r) => loop {
                let need = r.input_frames_next();
                if self.pending_in.len() < need {
                    break;
                }
                let chunk: Vec<f32> = self.pending_in.drain(..need).collect();
                match r.process(&[chunk], None) {
                    Ok(mut res) => self.pending_out.append(&mut res[0]),
                    Err(e) => {
                        tracing::warn!("resampler error: {e}");
                        break;
                    }
                }
            },
        }
    }

    fn emit(&mut self, out: &mut Vec<AudioFrame>) {
        while self.pending_out.len() >= FRAME_SAMPLES {
            let samples: Vec<i16> = self
                .pending_out
                .drain(..FRAME_SAMPLES)
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)
                .collect();
            let level = level_db(&samples);
            let voiced = self.vad.process(level);
            let ts = self.next_ts.unwrap_or(0);
            out.push(AudioFrame {
                channel: self.channel,
                samples,
                capture_ts_ms: ts,
                voiced,
                level_db: level,
            });
            self.next_ts = Some(ts + FRAME_MS as i64);
        }
    }
}

fn decode(format: SampleFormat, b: &[u8]) -> f32 {
    match format {
        SampleFormat::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        SampleFormat::I16 => i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0,
        SampleFormat::I24 => {
            let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
            v as f32 / 8_388_608.0
        }
        SampleFormat::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f32, ms: u32, amp: f32) -> Vec<f32> {
        let n = (rate as u64 * ms as u64 / 1000) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn interleave_f32(mono: &[f32], channels: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for s in mono {
            for _ in 0..channels {
                out.extend_from_slice(&s.to_le_bytes());
            }
        }
        out
    }

    /// Dominant frequency by zero crossings.
    fn freq_of(samples: &[i16]) -> f32 {
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] < 0) != (w[1] < 0))
            .count();
        crossings as f32 / 2.0 / (samples.len() as f32 / SAMPLE_RATE as f32)
    }

    fn convert(fmt: InputFormat, bytes: &[u8]) -> Vec<AudioFrame> {
        let mut c = FrameConverter::new(Channel::Remote, fmt, -45.0);
        let mut out = Vec::new();
        // Feed in 10 ms device packets like WASAPI does.
        let packet = (fmt.rate as usize / 100) * fmt.block_align();
        for (i, chunk) in bytes.chunks(packet).enumerate() {
            c.push_bytes(chunk, 1000 + i as i64 * 10, &mut out);
        }
        out
    }

    #[test]
    fn stereo_float_48k_becomes_16k_mono_frames() {
        let fmt = InputFormat {
            rate: 48_000,
            channels: 2,
            format: SampleFormat::F32,
        };
        let bytes = interleave_f32(&sine(48_000, 440.0, 1000, 0.5), 2);
        let frames = convert(fmt, &bytes);
        // One second of audio minus resampler latency.
        assert!((45..=50).contains(&frames.len()), "{} frames", frames.len());
        assert!(frames.iter().all(|f| f.samples.len() == FRAME_SAMPLES));
        let all: Vec<i16> = frames[5..].iter().flat_map(|f| f.samples.clone()).collect();
        let f = freq_of(&all);
        assert!((f - 440.0).abs() < 15.0, "frequency {f}");
        // About -9 dBFS for a 0.5 sine.
        assert!(frames[10].level_db > -12.0 && frames[10].level_db < -6.0);
        assert!(frames[10].voiced);
        // Timestamps advance 20 ms per frame from the first capture time.
        assert_eq!(frames[0].capture_ts_ms, 1000);
        assert_eq!(frames[3].capture_ts_ms, 1060);
    }

    #[test]
    fn stereo_float_44k1_resamples() {
        let fmt = InputFormat {
            rate: 44_100,
            channels: 2,
            format: SampleFormat::F32,
        };
        let bytes = interleave_f32(&sine(44_100, 1000.0, 1000, 0.5), 2);
        let frames = convert(fmt, &bytes);
        assert!((45..=50).contains(&frames.len()), "{} frames", frames.len());
        let all: Vec<i16> = frames[5..].iter().flat_map(|f| f.samples.clone()).collect();
        let f = freq_of(&all);
        assert!((f - 1000.0).abs() < 25.0, "frequency {f}");
    }

    #[test]
    fn integer_formats_decode() {
        let mono = sine(16_000, 300.0, 200, 0.25);
        for format in [SampleFormat::I16, SampleFormat::I24, SampleFormat::I32] {
            let mut bytes = Vec::new();
            for s in &mono {
                match format {
                    SampleFormat::I16 => {
                        bytes.extend_from_slice(&((s * 32_767.0) as i16).to_le_bytes())
                    }
                    SampleFormat::I24 => {
                        bytes.extend_from_slice(&(((s * 8_388_607.0) as i32).to_le_bytes())[..3])
                    }
                    SampleFormat::I32 => {
                        bytes.extend_from_slice(&((s * 2_147_483_647.0) as i32).to_le_bytes())
                    }
                    SampleFormat::F32 => unreachable!(),
                }
            }
            let frames = convert(
                InputFormat {
                    rate: 16_000,
                    channels: 1,
                    format,
                },
                &bytes,
            );
            assert_eq!(frames.len(), 10, "{format:?}");
            let peak = frames
                .iter()
                .flat_map(|f| f.samples.iter())
                .map(|s| s.abs())
                .max()
                .unwrap();
            assert!(
                (peak as f32 / 32_767.0 - 0.25).abs() < 0.01,
                "{format:?} peak {peak}"
            );
        }
    }

    #[test]
    fn mixes_down_by_averaging() {
        // Left full scale, right silent: mono at half amplitude.
        let mut bytes = Vec::new();
        for s in sine(16_000, 200.0, 100, 1.0) {
            bytes.extend_from_slice(&s.to_le_bytes());
            bytes.extend_from_slice(&0f32.to_le_bytes());
        }
        let frames = convert(
            InputFormat {
                rate: 16_000,
                channels: 2,
                format: SampleFormat::F32,
            },
            &bytes,
        );
        let peak = frames
            .iter()
            .flat_map(|f| f.samples.iter())
            .map(|s| s.abs())
            .max()
            .unwrap();
        assert!((peak as f32 / 32_767.0 - 0.5).abs() < 0.01);
    }

    #[test]
    fn silence_is_filled_when_loopback_stops() {
        let fmt = InputFormat {
            rate: 16_000,
            channels: 1,
            format: SampleFormat::F32,
        };
        let mut c = FrameConverter::new(Channel::Remote, fmt, -45.0);
        let mut out = Vec::new();
        c.push_mono(&vec![0.2; 3200], 0, &mut out); // 200 ms of data
        assert_eq!(out.len(), 10);
        out.clear();
        c.fill_silence(250, &mut out);
        assert!(out.is_empty(), "no fill before 100 ms without data");
        c.fill_silence(500, &mut out);
        assert_eq!(out.len(), 15);
        assert_eq!(out[0].capture_ts_ms, 200);
        assert!(out.iter().all(|f| f.samples.iter().all(|&s| s == 0)));
        assert!(!out.last().unwrap().voiced);
        // Data resumes later: the clock restarts at its capture time.
        out.clear();
        c.push_mono(&vec![0.2; 320], 900, &mut out);
        assert_eq!(out[0].capture_ts_ms, 900);
    }

    #[test]
    fn silence_is_filled_from_the_start_before_any_data() {
        let fmt = InputFormat {
            rate: 48_000,
            channels: 2,
            format: SampleFormat::F32,
        };
        let mut c = FrameConverter::new(Channel::Remote, fmt, -45.0);
        let mut out = Vec::new();
        c.fill_silence(1000, &mut out);
        assert!(out.is_empty(), "no clock without a start time");
        c.start_clock(1000);
        c.fill_silence(1050, &mut out);
        assert!(out.is_empty());
        c.fill_silence(2000, &mut out);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].capture_ts_ms, 1000);
        assert!(out.iter().all(|f| !f.voiced && f.level_db == -100.0));
        // The first real data continues the timeline.
        out.clear();
        c.push_bytes(
            &interleave_f32(&sine(48_000, 440.0, 100, 0.5), 2),
            2000,
            &mut out,
        );
        assert_eq!(out.first().map(|f| f.capture_ts_ms), Some(2000));
    }

    #[test]
    fn small_jitter_keeps_the_clock() {
        let fmt = InputFormat {
            rate: 16_000,
            channels: 1,
            format: SampleFormat::F32,
        };
        let mut c = FrameConverter::new(Channel::Me, fmt, -45.0);
        let mut out = Vec::new();
        c.push_mono(&vec![0.1; 160], 0, &mut out);
        c.push_mono(&vec![0.1; 160], 13, &mut out); // 3 ms late
        c.push_mono(&vec![0.1; 320], 20, &mut out);
        let ts: Vec<i64> = out.iter().map(|f| f.capture_ts_ms).collect();
        assert_eq!(ts, [0, 20]);
    }
}
