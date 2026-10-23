//! The MeetCue hint pipeline.
//!
//! Platform-independent: it takes 16 kHz PCM frames or injected text and
//! produces [`meetcue_core::MeetCueEvent`]s. Audio capture, windows and
//! hotkeys live in other crates.

pub mod backend;
pub mod coverage;
pub mod detect;
pub mod endpoints;
pub mod engine;
pub mod gate;
pub mod hints;
pub mod latency;
pub mod materials;
pub mod net;
pub mod practice;
pub mod prefilter;
pub mod probe;
pub mod prompts;
pub mod provider;
pub mod providers;
pub mod queue;
pub mod text;
pub mod textio;
pub mod timemap;
pub mod transcript;
pub mod usage;

pub use backend::{HintBackend, LocalHintBackend, SessionConfig};

/// RMS level of a PCM chunk in dBFS (used to find where speech starts).
pub(crate) fn text_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
    let rms = (sum / samples.len() as f64).sqrt() / i16::MAX as f64;
    if rms <= 1e-5 {
        -100.0
    } else {
        (20.0 * rms.log10()) as f32
    }
}
pub use provider::{AsrProvider, AsrStream, LlmProvider};
