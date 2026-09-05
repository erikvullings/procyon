//! Conversion budgets, the clock abstraction behind deadline checks, and the
//! internal tracker every converter charges against.
//!
//! Budgets fall into two classes, and the distinction is deliberate:
//!
//! * **Hard** budgets (source bytes, expanded archive bytes, archive entries,
//!   nesting depth, item counts, wall-clock deadline) describe input Procyon
//!   refuses to process at all. Exceeding one aborts the conversion with a
//!   typed [`ConversionOutcome::OverBudget`](crate::ConversionOutcome::OverBudget).
//! * **Soft** budgets (unit count, per-unit characters, total output
//!   characters) bound how much *output* one document may produce. Exceeding
//!   one truncates, and the document is marked
//!   [`Completeness::Partial`](crate::Completeness::Partial) with an explicit
//!   [`Omission`](crate::Omission). Truncation is never silent.

use std::time::{Duration, Instant};

use crate::cancellation::Cancellation;

/// Which budget an [`ConversionOutcome::OverBudget`](crate::ConversionOutcome::OverBudget)
/// outcome refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BudgetKind {
    /// Bytes of the source document.
    SourceBytes,
    /// Total uncompressed bytes of an archive/package.
    ExpandedBytes,
    /// Bytes read from one structured package part.
    PartBytes,
    /// Number of entries inside an archive/package.
    ArchiveEntries,
    /// Number of top-level items (pages, slides, sheets).
    Items,
    /// Nesting depth of the parsed structure (XML elements).
    NestingDepth,
    /// Wall-clock conversion deadline.
    Time,
}

impl BudgetKind {
    /// Stable, human-readable name used in messages and tests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceBytes => "source bytes",
            Self::ExpandedBytes => "expanded bytes",
            Self::PartBytes => "part bytes",
            Self::ArchiveEntries => "archive entries",
            Self::Items => "items",
            Self::NestingDepth => "nesting depth",
            Self::Time => "time",
        }
    }
}

impl std::fmt::Display for BudgetKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Per-document limits applied by every baseline converter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionBudgets {
    /// Maximum bytes of source content accepted.
    pub max_source_bytes: u64,
    /// Maximum total uncompressed bytes of a ZIP-based package.
    pub max_expanded_bytes: u64,
    /// Maximum number of entries in a ZIP-based package.
    pub max_archive_entries: u32,
    /// Maximum number of top-level items (PDF pages, slides, sheets).
    pub max_items: u32,
    /// Maximum XML element nesting depth.
    pub max_nesting_depth: u32,
    /// Maximum number of structural units emitted.
    pub max_units: u32,
    /// Maximum characters retained for a single structural unit.
    pub max_unit_chars: u32,
    /// Maximum characters retained across the whole document.
    pub max_output_chars: u64,
    /// Wall-clock deadline for one conversion.
    pub timeout: Duration,
}

impl Default for ConversionBudgets {
    fn default() -> Self {
        Self {
            max_source_bytes: 32 * 1024 * 1024,
            max_expanded_bytes: 128 * 1024 * 1024,
            max_archive_entries: 4_096,
            max_items: 4_096,
            max_nesting_depth: 128,
            max_units: 20_000,
            max_unit_chars: 64 * 1024,
            max_output_chars: 8 * 1024 * 1024,
            timeout: Duration::from_secs(30),
        }
    }
}

/// Monotonic elapsed-time source, so deadline behaviour is testable without
/// sleeping.
pub trait Clock: Send + Sync {
    /// Time elapsed since the clock was created.
    fn elapsed(&self) -> Duration;
}

/// Real monotonic clock.
#[derive(Debug)]
pub struct SystemClock {
    start: Instant,
}

impl SystemClock {
    /// Starts a clock at the current instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Test clock advanced explicitly; conversion never sleeps.
#[derive(Debug, Default)]
pub struct ManualClock {
    elapsed: std::sync::Mutex<Duration>,
}

impl ManualClock {
    /// Creates a clock reading zero elapsed time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances the clock by `delta`.
    pub fn advance(&self, delta: Duration) {
        let mut guard = self.elapsed.lock().unwrap_or_else(|error| {
            self.elapsed.clear_poison();
            error.into_inner()
        });
        *guard = guard.saturating_add(delta);
    }
}

impl Clock for ManualClock {
    fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap_or_else(|error| {
            self.elapsed.clear_poison();
            error.into_inner()
        })
    }
}

/// Why a converter stopped early. Converted into a typed outcome by the
/// dispatching converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stop {
    /// The caller cancelled the work.
    Cancelled,
    /// A hard budget was exceeded.
    OverBudget { kind: BudgetKind, limit: u64 },
}

/// Charges hard budgets and polls cancellation. Soft output budgets are
/// applied by the document builder.
pub(crate) struct BudgetTracker<'a> {
    budgets: &'a ConversionBudgets,
    cancellation: &'a Cancellation,
    clock: &'a dyn Clock,
    expanded_bytes: u64,
}

impl<'a> BudgetTracker<'a> {
    pub(crate) fn new(
        budgets: &'a ConversionBudgets,
        cancellation: &'a Cancellation,
        clock: &'a dyn Clock,
    ) -> Self {
        Self {
            budgets,
            cancellation,
            clock,
            expanded_bytes: 0,
        }
    }

    pub(crate) fn budgets(&self) -> &ConversionBudgets {
        self.budgets
    }

    /// Cancellation and deadline checkpoint, called inside every unbounded
    /// loop.
    pub(crate) fn checkpoint(&self) -> Result<(), Stop> {
        if self.cancellation.is_cancelled() {
            return Err(Stop::Cancelled);
        }
        if self.clock.elapsed() > self.budgets.timeout {
            return Err(Stop::OverBudget {
                kind: BudgetKind::Time,
                limit: self.budgets.timeout.as_millis().min(u128::from(u64::MAX)) as u64,
            });
        }
        Ok(())
    }

    pub(crate) fn charge_source_bytes(&self, bytes: u64) -> Result<(), Stop> {
        if bytes > self.budgets.max_source_bytes {
            return Err(Stop::OverBudget {
                kind: BudgetKind::SourceBytes,
                limit: self.budgets.max_source_bytes,
            });
        }
        Ok(())
    }

    pub(crate) fn charge_archive_entries(&self, entries: u64) -> Result<(), Stop> {
        if entries > u64::from(self.budgets.max_archive_entries) {
            return Err(Stop::OverBudget {
                kind: BudgetKind::ArchiveEntries,
                limit: u64::from(self.budgets.max_archive_entries),
            });
        }
        Ok(())
    }

    pub(crate) fn charge_expanded_bytes(&mut self, bytes: u64) -> Result<(), Stop> {
        self.expanded_bytes = self.expanded_bytes.saturating_add(bytes);
        if self.expanded_bytes > self.budgets.max_expanded_bytes {
            return Err(Stop::OverBudget {
                kind: BudgetKind::ExpandedBytes,
                limit: self.budgets.max_expanded_bytes,
            });
        }
        Ok(())
    }

    pub(crate) fn charge_items(&self, items: u64) -> Result<(), Stop> {
        if items > u64::from(self.budgets.max_items) {
            return Err(Stop::OverBudget {
                kind: BudgetKind::Items,
                limit: u64::from(self.budgets.max_items),
            });
        }
        Ok(())
    }

    pub(crate) fn charge_depth(&self, depth: u32) -> Result<(), Stop> {
        if depth > self.budgets.max_nesting_depth {
            return Err(Stop::OverBudget {
                kind: BudgetKind::NestingDepth,
                limit: u64::from(self.budgets.max_nesting_depth),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cancellation::CancellationFlag;

    #[test]
    fn the_deadline_is_checked_without_sleeping() {
        let budgets = ConversionBudgets {
            timeout: Duration::from_millis(10),
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert_eq!(tracker.checkpoint(), Ok(()));
        clock.advance(Duration::from_millis(11));
        assert_eq!(
            tracker.checkpoint(),
            Err(Stop::OverBudget {
                kind: BudgetKind::Time,
                limit: 10
            })
        );
    }

    #[test]
    fn cancellation_is_reported_before_budgets() {
        let budgets = ConversionBudgets::default();
        let flag = CancellationFlag::new();
        let cancellation = flag.handle();
        let clock = ManualClock::new();
        clock.advance(Duration::from_secs(3_600));
        flag.cancel();
        let tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert_eq!(tracker.checkpoint(), Err(Stop::Cancelled));
    }

    #[test]
    fn expanded_bytes_accumulate_across_entries() {
        let budgets = ConversionBudgets {
            max_expanded_bytes: 100,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert_eq!(tracker.charge_expanded_bytes(60), Ok(()));
        assert_eq!(
            tracker.charge_expanded_bytes(60),
            Err(Stop::OverBudget {
                kind: BudgetKind::ExpandedBytes,
                limit: 100
            })
        );
    }
}
