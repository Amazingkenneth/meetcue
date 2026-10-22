//! Audio capture for MeetCue.
//!
//! Two channels are captured: the microphone ("me") and system sound via
//! loopback ("remote"). Whatever the device delivers is converted to 16 kHz
//! mono 16-bit PCM in 20 ms frames with a voice activity flag and a level.
//!
//! The conversion, voice activity detection and silence filling are
//! platform-independent and unit-tested; only device access is per platform.

pub mod convert;
pub mod vad;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod windows;

use meetcue_core::{AudioFrame, Channel, MeetCueError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// Bluetooth headsets switch to a low-quality call profile when their
    /// microphone is opened; the settings page warns about it.
    pub is_bluetooth: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaptureConfig {
    /// Microphone device id; `None` follows the system default.
    pub mic_device: Option<String>,
    pub capture_me: bool,
    pub capture_remote: bool,
    pub vad_threshold_db: f32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            mic_device: None,
            capture_me: true,
            capture_remote: true,
            vad_threshold_db: -45.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SourceEvent {
    Frame(AudioFrame),
    /// The device of a channel changed (default device switched, headset
    /// plugged in or out) and capture was restarted.
    DeviceChanged(Channel),
    Error(Channel, MeetCueError),
}

/// Receives frames and events from the capture threads. Must return quickly.
pub type Sink = Box<dyn Fn(SourceEvent) + Send + Sync>;

pub trait AudioSource: Send {
    fn list_microphones(&self) -> Result<Vec<DeviceInfo>, MeetCueError>;
    fn list_outputs(&self) -> Result<Vec<DeviceInfo>, MeetCueError>;
    fn start(&mut self, cfg: CaptureConfig, sink: Sink) -> Result<(), MeetCueError>;
    fn stop(&mut self);
    /// Plays a short test tone on the default output device (for the self-test).
    fn play_test_tone(&self, freq_hz: f32, duration_ms: u32) -> Result<(), MeetCueError>;
}

/// The capture backend for the current platform.
pub fn platform_source() -> Box<dyn AudioSource> {
    #[cfg(windows)]
    {
        Box::new(windows::WasapiSource::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(unsupported::UnsupportedSource)
    }
}

/// Whether a device name looks like a Bluetooth headset.
pub fn looks_bluetooth(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "bluetooth",
        "hands-free",
        "handsfree",
        "免提",
        "airpods",
        " bt ",
    ]
    .iter()
    .any(|k| n.contains(k))
        || n.ends_with(" bt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_names() {
        assert!(looks_bluetooth("Headset (WH-1000XM4 Hands-Free AG Audio)"));
        assert!(looks_bluetooth("耳机 (Bluetooth Headset)"));
        assert!(!looks_bluetooth("Microphone Array (Realtek(R) Audio)"));
    }
}
