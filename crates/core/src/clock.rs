//! A single monotonic clock for the whole process.
//!
//! Audio capture callbacks, ASR results and pipeline timers must all be
//! compared on the same time base, otherwise latency statistics and echo
//! filtering drift apart. Every timestamp in MeetCue is "milliseconds since
//! the process epoch" as returned by [`mono_ms`].

use std::sync::OnceLock;
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();

/// The instant all MeetCue timestamps are measured from.
pub fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

/// Milliseconds elapsed since [`epoch`].
pub fn mono_ms() -> i64 {
    since_epoch_ms(Instant::now())
}

/// Converts an [`Instant`] into milliseconds since [`epoch`]. Instants taken
/// before the epoch map to negative values.
pub fn since_epoch_ms(at: Instant) -> i64 {
    let epoch = epoch();
    if at >= epoch {
        (at - epoch).as_millis() as i64
    } else {
        -((epoch - at).as_millis() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic() {
        let a = mono_ms();
        let b = mono_ms();
        assert!(b >= a);
        assert!(since_epoch_ms(epoch()) == 0);
    }
}
