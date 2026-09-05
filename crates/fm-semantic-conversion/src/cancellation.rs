//! Cancellation without a runtime dependency.
//!
//! Conversion runs on a blocking thread and must stop promptly when the caller
//! goes away, but this crate deliberately does not depend on Tokio: hosts wrap
//! whatever cancellation primitive they already own (a
//! `tokio_util::sync::CancellationToken`, an `AtomicBool`, a channel) in a
//! [`CancellationSignal`] implementation.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A host-supplied signal that conversion should stop.
///
/// Implementations must be cheap to poll: converters check it in every loop
/// that can grow with the size of the input.
pub trait CancellationSignal: Send + Sync {
    /// Whether the caller has asked for the work to stop.
    fn is_cancelled(&self) -> bool;
}

/// Handle passed into conversion; an absent signal never cancels.
#[derive(Clone, Default)]
pub struct Cancellation {
    signal: Option<Arc<dyn CancellationSignal>>,
}

impl Cancellation {
    /// A handle that never reports cancellation.
    #[must_use]
    pub fn none() -> Self {
        Self { signal: None }
    }

    /// Wraps a host cancellation primitive.
    #[must_use]
    pub fn new(signal: Arc<dyn CancellationSignal>) -> Self {
        Self {
            signal: Some(signal),
        }
    }

    /// Whether the caller has asked for the work to stop.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.signal
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
    }
}

impl std::fmt::Debug for Cancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Cancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// A simple in-process cancellation flag, used by tests and by callers that
/// have no runtime primitive of their own.
#[derive(Clone, Debug, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);

impl CancellationFlag {
    /// Creates a flag that is not yet cancelled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Wraps this flag in a [`Cancellation`] handle.
    #[must_use]
    pub fn handle(&self) -> Cancellation {
        Cancellation::new(Arc::new(self.clone()))
    }
}

impl CancellationSignal for CancellationFlag {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_signal_never_cancels() {
        assert!(!Cancellation::none().is_cancelled());
        assert!(!Cancellation::default().is_cancelled());
    }

    #[test]
    fn a_flag_cancels_every_clone_of_its_handle() {
        let flag = CancellationFlag::new();
        let handle = flag.handle();
        let clone = handle.clone();
        assert!(!handle.is_cancelled());
        flag.cancel();
        assert!(handle.is_cancelled());
        assert!(clone.is_cancelled());
    }
}
