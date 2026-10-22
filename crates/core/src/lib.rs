//! Platform-independent domain model shared by every MeetCue crate.
//!
//! Everything here is plain data: events, settings, error codes and the price
//! table. Types that cross the IPC boundary derive `ts_rs::TS`, and running
//! `cargo test -p meetcue-core` regenerates `app/ui/src/bindings/`.

pub mod audio;
pub mod clock;
pub mod error;
pub mod event;
pub mod ipc;
pub mod pricing;
pub mod settings;

pub use audio::AudioFrame;
pub use error::{codes, ErrorCatalog, ErrorInfo, MeetCueError, Result};
pub use event::*;
pub use pricing::{CostAssumptions, HourlyEstimate, PriceEntry, PriceTable, PriceUnit, Usd};
pub use settings::Settings;

/// Version of the application, shared by every crate in the workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
