//! Rebuilds enrolled-root indexes after the active embedding model changes.
//!
//! Activating a different model activates a different embedding space, and the
//! worker gives each model its own empty index. Durable enrolment consent is
//! unaffected by that switch, so every enrolled root has to be fed to the newly
//! active model before search returns anything again. This module owns that
//! opt-in trusted-host orchestration; it is never reachable from an ordinary
//! transport request.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::semantic::SemanticService;
use crate::semantic_indexing::SemanticIndexingService;
use crate::semantic_library::{
    SemanticAccessContext, SemanticLibraryService, SemanticRootAvailability,
};

/// One root that could not be reindexed after the model changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticReindexFailure {
    /// Stable path-independent root identity.
    pub root_id: String,
    /// Actionable diagnostic for the host log.
    pub reason: String,
}

/// Outcome of one post-model-change reindex pass.
///
/// The pass is deliberately best-effort: model activation is already durable
/// when it runs, so a failure is reported rather than rolled back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticModelChangeReindexReport {
    /// Why stopping the previously active worker failed, when it did.
    pub restart_failure: Option<String>,
    /// Roots that completed a full reconciliation pass into the new index.
    pub reindexed_roots: Vec<String>,
    /// Roots skipped because their source is currently unreachable.
    pub unavailable_roots: Vec<String>,
    /// Roots whose reconciliation failed.
    pub failures: Vec<SemanticReindexFailure>,
    /// Total occurrences fed to the newly active model.
    pub ingested_occurrences: u64,
}

impl SemanticModelChangeReindexReport {
    /// Returns whether every step of the pass succeeded.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.restart_failure.is_none() && self.failures.is_empty()
    }
}

/// Stops the worker holding the previous model, then reconciles every
/// available enrolled root into the newly active one.
///
/// The restart happens first and unconditionally: the running worker still
/// serves the previous model, so ingesting into it would populate the index
/// that was just superseded. A restart failure is recorded and the pass
/// continues, because the next operation reconnects anyway.
pub(crate) async fn reindex_after_model_change(
    semantic: &SemanticService,
    indexing: &SemanticIndexingService,
    library: Arc<SemanticLibraryService>,
    access: &SemanticAccessContext,
    grace: Duration,
    cancellation: CancellationToken,
) -> SemanticModelChangeReindexReport {
    let mut report = SemanticModelChangeReindexReport::default();
    let restart_failure = semantic.restart(grace).await.err().map(|error| {
        let message = error.to_string();
        report.restart_failure = Some(message.clone());
        message
    });
    let status = match library.status(access) {
        Ok(status) => status,
        Err(error) => {
            report.failures.push(SemanticReindexFailure {
                root_id: String::new(),
                reason: format!("enrolled roots could not be listed: {error}"),
            });
            return report;
        }
    };
    if let Some(error) = restart_failure {
        let reason = format!(
            "the previous semantic worker could not be stopped; reindex was not started: {error}"
        );
        report.failures.extend(
            status
                .roots
                .into_iter()
                .filter(|root| matches!(root.availability, SemanticRootAvailability::Available))
                .map(|root| SemanticReindexFailure {
                    root_id: root.id,
                    reason: reason.clone(),
                }),
        );
        return report;
    }

    for root in status.roots {
        if cancellation.is_cancelled() {
            report.failures.push(SemanticReindexFailure {
                root_id: root.id,
                reason: "reindex after the model change was cancelled".to_owned(),
            });
            continue;
        }
        if !matches!(root.availability, SemanticRootAvailability::Available) {
            report.unavailable_roots.push(root.id);
            continue;
        }
        let Ok(root_id) = root.id.parse() else {
            report.failures.push(SemanticReindexFailure {
                root_id: root.id,
                reason: "enrolled root identity could not be resolved".to_owned(),
            });
            continue;
        };
        match indexing
            .reconcile(
                Arc::clone(&library),
                access,
                root_id,
                cancellation.child_token(),
            )
            .await
        {
            Ok(pass) => {
                report.ingested_occurrences += pass.ingested_occurrences;
                report.reindexed_roots.push(root.id);
            }
            Err(error) => report.failures.push(SemanticReindexFailure {
                root_id: root.id,
                reason: error.to_string(),
            }),
        }
    }
    report
}
