use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;

#[derive(Debug, Default)]
struct PauseState {
    paused: AtomicBool,
    resumed: Notify,
}

/// Cooperative pause signal checked by executors between safe chunks.
#[derive(Debug, Clone, Default)]
pub struct PauseToken {
    state: Arc<PauseState>,
}

impl PauseToken {
    pub(crate) fn pause(&self) {
        self.state.paused.store(true, Ordering::Release);
    }

    pub(crate) fn resume(&self) {
        self.state.paused.store(false, Ordering::Release);
        self.state.resumed.notify_waiters();
    }

    /// Waits while work is paused and returns immediately while it is running.
    pub async fn checkpoint(&self) {
        loop {
            let resumed = self.state.resumed.notified();
            if !self.state.paused.load(Ordering::Acquire) {
                return;
            }
            resumed.await;
        }
    }
}
