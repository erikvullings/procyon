use std::time::{Duration, Instant};

use crate::OperationProgress;

/// Coalesces progress changes and calculates an exponentially smoothed rate.
#[derive(Debug)]
pub struct ProgressPublisher {
    minimum_interval: Duration,
    smoothing_factor: f64,
    last_emitted_at: Option<Instant>,
    bytes_at_last_emission: u64,
    completed_bytes: u64,
    smoothed_rate: Option<f64>,
}

impl ProgressPublisher {
    /// Creates a publisher. `smoothing_factor` is clamped to `0..=1`.
    #[must_use]
    pub fn new(minimum_interval: Duration, smoothing_factor: f64) -> Self {
        Self {
            minimum_interval,
            smoothing_factor: smoothing_factor.clamp(0.0, 1.0),
            last_emitted_at: None,
            bytes_at_last_emission: 0,
            completed_bytes: 0,
            smoothed_rate: None,
        }
    }

    /// Records completed bytes and emits at most once per configured interval.
    pub fn record(&mut self, now: Instant, additional_bytes: u64) -> Option<OperationProgress> {
        self.completed_bytes = self.completed_bytes.saturating_add(additional_bytes);
        if let Some(last) = self.last_emitted_at {
            let elapsed = now.saturating_duration_since(last);
            if elapsed < self.minimum_interval {
                return None;
            }
            let delta = self
                .completed_bytes
                .saturating_sub(self.bytes_at_last_emission);
            if !elapsed.is_zero() {
                let instantaneous = delta as f64 / elapsed.as_secs_f64();
                self.smoothed_rate = Some(self.smoothed_rate.map_or(instantaneous, |previous| {
                    self.smoothing_factor
                        .mul_add(instantaneous, (1.0 - self.smoothing_factor) * previous)
                }));
            }
        }
        self.last_emitted_at = Some(now);
        self.bytes_at_last_emission = self.completed_bytes;
        Some(OperationProgress {
            completed_bytes: self.completed_bytes,
            bytes_per_second: self.smoothed_rate.map(|rate| rate.round() as u64),
            ..OperationProgress::default()
        })
    }
}
