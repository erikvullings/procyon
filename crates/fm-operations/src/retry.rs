//! Bounded retry with exponential backoff for transfers (task 0301).
//!
//! The policy and the waiting are separate from the scheduler so both are testable without real
//! time: [`RetryPolicy::delay_for`] is a pure function of the attempt number and a jitter value,
//! and the wait itself goes through a [`Sleeper`] that a test replaces with a recorder.

use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

/// How many times, and how patiently, a transient failure is retried.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts including the first; `1` disables retrying.
    pub max_attempts: u32,
    /// Wait before the first retry.
    pub initial_delay: Duration,
    /// Upper bound for any single wait, jitter included.
    pub max_delay: Duration,
    /// Growth factor between consecutive waits.
    pub multiplier: f64,
    /// Fraction of each wait randomised in both directions (`0.2` is plus or minus 20 percent).
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: 0.2,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            max_attempts: 1,
            initial_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
            multiplier: 1.0,
            jitter: 0.0,
        }
    }

    /// Whether another attempt is allowed after `attempt` (1-based) failed.
    #[must_use]
    pub const fn allows_retry_after(&self, attempt: u32) -> bool {
        attempt < self.max_attempts
    }

    /// The wait before retrying after `attempt` (1-based) failed.
    ///
    /// `jitter_unit` is a value in `0.0..=1.0`; `0.5` leaves the base delay unchanged.
    #[must_use]
    pub fn delay_for(&self, attempt: u32, jitter_unit: f64) -> Duration {
        let exponent = i32::try_from(attempt.saturating_sub(1)).unwrap_or(i32::MAX);
        let base = self.initial_delay.as_secs_f64() * self.multiplier.max(1.0).powi(exponent);
        let spread = 1.0 + self.jitter.clamp(0.0, 1.0) * (jitter_unit.clamp(0.0, 1.0) * 2.0 - 1.0);
        let seconds = (base * spread).min(self.max_delay.as_secs_f64()).max(0.0);
        Duration::from_secs_f64(if seconds.is_finite() { seconds } else { 0.0 })
    }
}

/// Waits between attempts. A test supplies a recorder instead of real time.
#[async_trait]
pub trait Sleeper: Send + Sync + 'static {
    /// Waits for `duration`. Returns `false` when `cancellation` fired first, so a cancelled
    /// transfer never sits out its backoff.
    async fn sleep(&self, duration: Duration, cancellation: &CancellationToken) -> bool;
}

/// Real-time [`Sleeper`].
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration, cancellation: &CancellationToken) -> bool {
        tokio::select! {
            () = tokio::time::sleep(duration) => true,
            () = cancellation.cancelled() => false,
        }
    }
}

/// Source of the randomness in each wait.
pub trait JitterSource: Send + Sync + 'static {
    /// A value in `0.0..=1.0`.
    fn unit(&self) -> f64;
}

/// Jitter from the operating system's random identifiers.
#[derive(Debug, Default, Clone, Copy)]
pub struct RandomJitter;

impl JitterSource for RandomJitter {
    #[allow(clippy::cast_precision_loss)]
    fn unit(&self) -> f64 {
        let bits = fm_domain::OperationId::new().into_inner().as_u128() & 0xFFFF_FFFF;
        bits as f64 / f64::from(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(1_000),
            multiplier: 2.0,
            jitter: 0.0,
        }
    }

    #[test]
    fn waits_grow_exponentially_then_stop_at_the_cap() {
        let waits: Vec<_> = (1..=5)
            .map(|attempt| policy().delay_for(attempt, 0.5).as_millis())
            .collect();
        assert_eq!(waits, vec![100, 200, 400, 800, 1_000]);
    }

    #[test]
    fn jitter_spreads_a_wait_but_never_past_the_cap() {
        let jittery = RetryPolicy {
            jitter: 0.5,
            ..policy()
        };
        assert_eq!(jittery.delay_for(1, 0.0).as_millis(), 50);
        assert_eq!(jittery.delay_for(1, 1.0).as_millis(), 150);
        assert_eq!(
            jittery.delay_for(4, 1.0).as_millis(),
            1_000,
            "800 ms plus 50 percent is capped"
        );
    }

    #[test]
    fn attempts_are_bounded_and_a_disabled_policy_never_retries() {
        let policy = policy();
        assert!(policy.allows_retry_after(4));
        assert!(!policy.allows_retry_after(5));
        assert!(!RetryPolicy::disabled().allows_retry_after(1));
    }

    #[test]
    fn nonsense_settings_cannot_produce_a_negative_or_infinite_wait() {
        let silly = RetryPolicy {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(5),
            multiplier: f64::INFINITY,
            jitter: 9.0,
        };
        assert!(silly.delay_for(300, 0.0) <= Duration::from_secs(5));
        assert!(silly.delay_for(1, 7.0) <= Duration::from_secs(5));
    }

    #[test]
    fn random_jitter_stays_in_the_unit_interval() {
        for _ in 0..100 {
            let unit = RandomJitter.unit();
            assert!((0.0..=1.0).contains(&unit));
        }
    }

    #[tokio::test]
    async fn a_cancelled_wait_returns_false_immediately() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let waited = TokioSleeper
            .sleep(Duration::from_secs(3_600), &cancellation)
            .await;
        assert!(!waited);
    }

    #[tokio::test]
    async fn an_uncancelled_wait_completes() {
        let waited = TokioSleeper
            .sleep(Duration::from_millis(1), &CancellationToken::new())
            .await;
        assert!(waited);
    }
}
