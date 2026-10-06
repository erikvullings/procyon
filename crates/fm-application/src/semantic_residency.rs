//! Keeps an enabled semantic worker warm between searches (task 0236).
//!
//! The worker drops a connection that sends no frame for the protocol stream
//! deadline and exits shortly after its last connection closes, so without
//! traffic every search after a short pause pays a cold start (model load and
//! index open). While semantic search is enabled and recently used, a
//! periodic health request over the cached connection keeps it resident.

use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::semantic::SemanticService;

/// Timing of the background warm-up and keepalive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SemanticResidencyPolicy {
    /// Interval between health requests; must stay below the protocol's
    /// stream deadline so the worker never closes the idle connection.
    pub(crate) keepalive_interval: Duration,
    /// How long after startup or the last search the worker stays resident.
    pub(crate) residency_window: Duration,
}

impl Default for SemanticResidencyPolicy {
    fn default() -> Self {
        Self {
            keepalive_interval: Duration::from_secs(2 * 60),
            residency_window: Duration::from_secs(4 * 60 * 60),
        }
    }
}

/// Warms the worker immediately, then keeps it resident until `shutdown`.
///
/// `enabled` is consulted before every health request so a library without
/// enrolled roots never starts a worker. Failures are logged and retried on
/// the next tick only.
pub(crate) async fn keep_resident<Enabled, EnabledFuture>(
    semantic: &SemanticService,
    policy: SemanticResidencyPolicy,
    mut enabled: Enabled,
    shutdown: CancellationToken,
) where
    Enabled: FnMut() -> EnabledFuture,
    EnabledFuture: Future<Output = bool>,
{
    // Startup counts as activity so the first window begins now.
    semantic.record_search_activity();
    loop {
        let recently_used = semantic
            .last_search_activity()
            .is_some_and(|last| last.elapsed() < policy.residency_window);
        if recently_used
            && enabled().await
            && let Err(error) = semantic.health().await
        {
            tracing::debug!(%error, "semantic worker keepalive failed");
        }
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = tokio::time::sleep(policy.keepalive_interval) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::semantic::{
        DocumentIngestion, SemanticCapability, SemanticError, SemanticHealth, SemanticIngestionJob,
        SemanticJobId, SemanticOperationId, SemanticProgressEvent, SemanticQuery, SemanticScope,
        SemanticSearchResult,
    };

    #[derive(Default)]
    struct CountingCapability {
        health_calls: AtomicUsize,
        failing: AtomicBool,
    }

    #[async_trait]
    impl SemanticCapability for CountingCapability {
        async fn health(&self) -> Result<SemanticHealth, SemanticError> {
            self.health_calls.fetch_add(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                Err(SemanticError::Unavailable)
            } else {
                Ok(SemanticHealth::Serving)
            }
        }
        async fn ingest(&self, _: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
            Err(SemanticError::Unavailable)
        }
        async fn query(
            &self,
            _: SemanticQuery,
        ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
            Ok(Vec::new())
        }
        async fn ingestion_job(
            &self,
            _: SemanticScope,
            _: SemanticJobId,
        ) -> Result<SemanticIngestionJob, SemanticError> {
            Err(SemanticError::Unavailable)
        }
        async fn events(
            &self,
            _: SemanticScope,
        ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
            Ok(Vec::new())
        }
        async fn cancel(&self, _: SemanticOperationId) -> Result<bool, SemanticError> {
            Ok(false)
        }
        async fn shutdown(&self, _: Duration) -> Result<(), SemanticError> {
            Ok(())
        }
    }

    const POLICY: SemanticResidencyPolicy = SemanticResidencyPolicy {
        keepalive_interval: Duration::from_secs(60),
        residency_window: Duration::from_secs(5 * 60),
    };

    fn spawn_residency(
        capability: &Arc<CountingCapability>,
        enabled: bool,
    ) -> (SemanticService, CancellationToken) {
        let semantic = SemanticService::new(Arc::clone(capability) as Arc<dyn SemanticCapability>);
        let shutdown = CancellationToken::new();
        let task_semantic = semantic.clone();
        let task_shutdown = shutdown.clone();
        tokio::spawn(async move {
            keep_resident(
                &task_semantic,
                POLICY,
                || async move { enabled },
                task_shutdown,
            )
            .await;
        });
        (semantic, shutdown)
    }

    async fn advance(duration: Duration) {
        tokio::time::sleep(duration).await;
        tokio::task::yield_now().await;
    }

    #[tokio::test(start_paused = true)]
    async fn warms_immediately_and_keeps_alive_within_the_window() {
        let capability = Arc::new(CountingCapability::default());
        let (_semantic, shutdown) = spawn_residency(&capability, true);
        advance(Duration::from_millis(1)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 1);
        advance(Duration::from_secs(3 * 60 + 30)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 4);
        shutdown.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn stops_after_the_window_and_resumes_on_search() {
        let capability = Arc::new(CountingCapability::default());
        let (semantic, shutdown) = spawn_residency(&capability, true);
        advance(Duration::from_secs(10 * 60)).await;
        let lapsed = capability.health_calls.load(Ordering::SeqCst);
        assert_eq!(lapsed, 5, "only ticks inside the five-minute window ping");
        advance(Duration::from_secs(10 * 60)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), lapsed);

        advance(Duration::from_secs(30)).await;
        semantic.record_search_activity();
        advance(Duration::from_secs(60)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), lapsed + 1);
        shutdown.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn never_starts_a_worker_while_disabled() {
        let capability = Arc::new(CountingCapability::default());
        let (_semantic, shutdown) = spawn_residency(&capability, false);
        advance(Duration::from_secs(3 * 60)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 0);
        shutdown.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_retrying_after_a_failed_warm_up() {
        let capability = Arc::new(CountingCapability::default());
        capability.failing.store(true, Ordering::SeqCst);
        let (_semantic, shutdown) = spawn_residency(&capability, true);
        advance(Duration::from_millis(1)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 1);
        capability.failing.store(false, Ordering::SeqCst);
        advance(Duration::from_secs(90)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 2);
        shutdown.cancel();
    }

    #[tokio::test(start_paused = true)]
    async fn stops_on_shutdown() {
        let capability = Arc::new(CountingCapability::default());
        let (_semantic, shutdown) = spawn_residency(&capability, true);
        advance(Duration::from_millis(1)).await;
        shutdown.cancel();
        advance(Duration::from_secs(5 * 60)).await;
        assert_eq!(capability.health_calls.load(Ordering::SeqCst), 1);
    }
}
