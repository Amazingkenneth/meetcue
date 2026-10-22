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
pub mod practice;
pub mod prefilter;
pub mod probe;
pub mod prompts;
pub mod provider;
pub mod providers;
pub mod queue;
pub mod text;
pub mod timemap;
pub mod transcript;
pub mod usage;

pub use backend::{HintBackend, LocalHintBackend, SessionConfig};
pub use provider::{AsrProvider, AsrStream, LlmProvider};
