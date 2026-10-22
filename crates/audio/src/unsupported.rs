//! Placeholder for platforms whose capture backend is not implemented yet.
//!
//! macOS will use ScreenCaptureKit (system sound) and Core Audio; Linux will
//! use PipeWire/PulseAudio monitor sources. Until then every call returns
//! `MC-AUD-900` so the rest of the application still builds and runs.

use meetcue_core::{codes, MeetCueError};

use crate::{AudioSource, CaptureConfig, DeviceInfo, Sink};

pub struct UnsupportedSource;

fn unsupported() -> MeetCueError {
    MeetCueError::new(codes::AUD_UNSUPPORTED, std::env::consts::OS)
}

impl AudioSource for UnsupportedSource {
    fn list_microphones(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        Ok(Vec::new())
    }

    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, MeetCueError> {
        Ok(Vec::new())
    }

    fn start(&mut self, _cfg: CaptureConfig, _sink: Sink) -> Result<(), MeetCueError> {
        Err(unsupported())
    }

    fn stop(&mut self) {}

    fn play_test_tone(&self, _freq_hz: f32, _duration_ms: u32) -> Result<(), MeetCueError> {
        Err(unsupported())
    }
}
