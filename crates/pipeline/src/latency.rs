//! Live latency monitoring during a session.

use std::collections::VecDeque;

/// Questions slower than this count towards a "probe again" suggestion.
pub const SLOW_MS: i64 = 2_500;
/// Consecutive slow questions before suggesting a new probe.
pub const SLOW_STREAK: usize = 3;

#[derive(Debug, Default)]
pub struct LatencyMonitor {
    window: VecDeque<i64>,
    slow_streak: usize,
}

impl LatencyMonitor {
    /// Adds a total latency sample; returns `Some(median)` when the user should
    /// be told to re-run the latency probe.
    pub fn add(&mut self, total_ms: i64) -> Option<i64> {
        self.window.push_back(total_ms);
        if self.window.len() > 20 {
            self.window.pop_front();
        }
        if total_ms > SLOW_MS {
            self.slow_streak += 1;
        } else {
            self.slow_streak = 0;
        }
        if self.slow_streak == SLOW_STREAK {
            Some(self.median().unwrap_or(total_ms))
        } else {
            None
        }
    }

    pub fn median(&self) -> Option<i64> {
        median(self.window.iter().copied().collect())
    }
}

pub fn median(mut values: Vec<i64>) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2
    } else {
        values[mid]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_after_three_slow_questions() {
        let mut m = LatencyMonitor::default();
        assert_eq!(m.add(3000), None);
        assert_eq!(m.add(3000), None);
        assert_eq!(m.add(1000), None);
        assert_eq!(m.add(2600), None);
        assert_eq!(m.add(2700), None);
        assert!(m.add(2800).is_some());
        assert_eq!(m.add(2900), None, "warn once per streak");
        assert_eq!(median(vec![3, 1, 2]), Some(2));
        assert_eq!(median(vec![4, 1, 2, 3]), Some(2));
    }
}
