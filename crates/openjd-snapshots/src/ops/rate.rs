// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

use std::collections::VecDeque;

const RATE_WINDOW_SECONDS: f64 = 12.0;

/// Sliding window rate calculator.
///
/// Tracks (timestamp, cumulative_bytes) samples and computes the rate
/// as the slope over the most recent RATE_WINDOW_SECONDS window.
pub(crate) struct SlidingWindowRate {
    history: VecDeque<(f64, u64)>,
}

impl SlidingWindowRate {
    pub fn new() -> Self {
        let mut history = VecDeque::new();
        history.push_back((0.0, 0));
        Self { history }
    }

    /// Record a sample and return the current rate in bytes/second.
    pub fn update(&mut self, timestamp: f64, cumulative_bytes: u64) -> f64 {
        self.history.push_back((timestamp, cumulative_bytes));

        // Trim entries outside the window, but keep the oldest boundary entry.
        while self.history.len() > 2 && timestamp - self.history[1].0 > RATE_WINDOW_SECONDS {
            self.history.pop_front();
        }

        let (oldest_time, oldest_bytes) = self.history[0];
        let time_delta = timestamp - oldest_time;
        if time_delta > 0.0 {
            (cumulative_bytes - oldest_bytes) as f64 / time_delta
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_initial_zero() {
        let mut r = SlidingWindowRate::new();
        assert_eq!(r.update(0.0, 0), 0.0);
    }

    #[test]
    fn rate_constant_throughput() {
        let mut r = SlidingWindowRate::new();
        // 1000 bytes/sec for 5 seconds
        for i in 1..=5 {
            let rate = r.update(i as f64, i * 1000);
            let expected = 1000.0;
            assert!(
                (rate - expected).abs() < 1.0,
                "at t={i}: rate={rate}, expected={expected}"
            );
        }
    }

    #[test]
    fn rate_window_expiry() {
        let mut r = SlidingWindowRate::new();
        // Push data at t=1 (1000 bytes)
        r.update(1.0, 1000);
        // Push data at t=14 (2000 bytes) — t=1 is outside the 12s window from t=14
        let rate = r.update(14.0, 2000);
        // Window should have trimmed t=0 entry; oldest is t=1
        // rate = (2000-1000)/(14-1) ≈ 76.9
        assert!((rate - 1000.0 / 13.0).abs() < 1.0, "rate={rate}");
    }

    #[test]
    fn rate_zero_time_delta() {
        let mut r = SlidingWindowRate::new();
        assert_eq!(r.update(0.0, 1000), 0.0);
    }

    #[test]
    fn rate_burst() {
        let mut r = SlidingWindowRate::new();
        // Burst: 10000 bytes at t=1
        let r1 = r.update(1.0, 10000);
        assert!(r1 > 0.0);
        // Pause: no new bytes at t=5
        let r2 = r.update(5.0, 10000);
        // Rate should decrease since no new bytes over longer window
        assert!(r2 < r1, "rate should decrease: r1={r1}, r2={r2}");
    }
}
