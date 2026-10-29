use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use fm_platform::PlatformAdapter;

use super::replace::{
    OcrOriginalReplacer, OcrOriginalTrash, PlatformOriginalTrash, SourceSnapshot,
    local_original_path,
};
use super::{
    ClaimedOcrJob, OcrAvailability, OcrRemediationFileOutcome, OcrRemediationJob,
    OcrRemediationJobId, OcrRemediationOutcome, OcrRemediationScope, OcrRemediationTarget,
    OcrReplacementOutcome, OcrReplacementSkip, SemanticOcrError, SemanticOcrService,
    SemanticOcrStatus, classify_ingest_report,
};
use crate::semantic::SemanticService;
use crate::semantic_indexing::{
    SemanticIndexingError, SemanticIndexingService, SingleFileIngestOutcome,
};
use crate::semantic_library::{SemanticAccessContext, SemanticLibraryService};

const WORKER_RESTART_GRACE: Duration = Duration::from_secs(5);

/// Deep application coordinator for OCR policy, scope expansion, and jobs.
pub(crate) struct SemanticOcrCoordinator {
    jobs: Arc<SemanticOcrService>,
    indexing: Arc<SemanticIndexingService>,
    trash: Arc<dyn OcrOriginalTrash>,
}

/// What the replace-original step needs, captured before the worker reads
/// the file so a later edit is detected.
struct ReplacementPlan {
    replacer: Arc<OcrOriginalReplacer>,
    original: Result<(PathBuf, SourceSnapshot), OcrReplacementSkip>,
}

impl SemanticOcrCoordinator {
    pub(crate) fn new(
        jobs: Arc<SemanticOcrService>,
        indexing: Arc<SemanticIndexingService>,
        platform: Arc<dyn PlatformAdapter>,
    ) -> Self {
        Self {
            jobs,
            indexing,
            trash: Arc::new(PlatformOriginalTrash(platform)),
        }
    }

    pub(crate) fn with_jobs(&self, jobs: SemanticOcrService) -> Self {
        Self {
            jobs: Arc::new(jobs),
            indexing: Arc::clone(&self.indexing),
            trash: Arc::clone(&self.trash),
        }
    }

    /// Plans the replace-original step for one target, or `None` when the
    /// user has not opted in.
    fn plan_replacement(&self, target: &OcrRemediationTarget) -> Option<ReplacementPlan> {
        if !self.jobs.replace_original() {
            return None;
        }
        let directory = self.jobs.retention_directory()?.to_owned();
        Some(ReplacementPlan {
            replacer: Arc::new(OcrOriginalReplacer::new(directory, Arc::clone(&self.trash))),
            original: local_original_path(&target.location).and_then(|path| {
                let snapshot = SourceSnapshot::capture(&path)?;
                Ok((path, snapshot))
            }),
        })
    }

    /// Verifies and applies the retained OCR output on a blocking thread.
    async fn replace_original(plan: ReplacementPlan) -> OcrReplacementOutcome {
        let ReplacementPlan { replacer, original } = plan;
        match original {
            Err(reason) => OcrReplacementOutcome::Kept { reason },
            Ok((path, snapshot)) => {
                tokio::task::spawn_blocking(move || replacer.replace(&path, snapshot))
                    .await
                    .unwrap_or_else(|error| OcrReplacementOutcome::Kept {
                        reason: OcrReplacementSkip::ReplaceFailed {
                            detail: error.to_string(),
                        },
                    })
            }
        }
    }

    pub(crate) fn status(&self) -> SemanticOcrStatus {
        self.jobs.status(self.reported_targets())
    }

    pub(crate) async fn set_consent(
        &self,
        semantic: &SemanticService,
        enabled: bool,
    ) -> Result<SemanticOcrStatus, SemanticOcrError> {
        let previous = self.jobs.consent();
        let update = self.jobs.set_consent(enabled);
        let changed = previous != self.jobs.consent();
        if changed {
            semantic
                .restart(WORKER_RESTART_GRACE)
                .await
                .map_err(|error| SemanticOcrError::WorkerRestart(error.to_string()))?;
        }
        update?;
        Ok(self.status())
    }

    pub(crate) async fn set_replace_original(
        &self,
        semantic: &SemanticService,
        enabled: bool,
    ) -> Result<SemanticOcrStatus, SemanticOcrError> {
        let changed = self.jobs.set_replace_original(enabled)?;
        if changed {
            // The worker's output-retention directory is fixed at launch.
            semantic
                .restart(WORKER_RESTART_GRACE)
                .await
                .map_err(|error| SemanticOcrError::WorkerRestart(error.to_string()))?;
        }
        Ok(self.status())
    }

    pub(crate) fn start(
        &self,
        scope: OcrRemediationScope,
    ) -> Result<OcrRemediationJob, SemanticOcrError> {
        let reported = self.reported_targets();
        let targets = match scope {
            OcrRemediationScope::File(target) => {
                if !reported.contains(&target) {
                    return Err(SemanticOcrError::UnreportedTarget);
                }
                vec![target]
            }
            OcrRemediationScope::SelectedFiles(targets) => {
                if targets.iter().any(|target| !reported.contains(target)) {
                    return Err(SemanticOcrError::UnreportedTarget);
                }
                targets
            }
            OcrRemediationScope::Root(root_id) => reported
                .into_iter()
                .filter(|target| target.root_id == root_id)
                .collect(),
            OcrRemediationScope::AllRequired => reported,
        };
        let id = self.jobs.enqueue(targets)?;
        self.jobs.job(&id).ok_or(SemanticOcrError::NotFound)
    }

    pub(crate) fn cancel(
        &self,
        id: &OcrRemediationJobId,
    ) -> Result<OcrRemediationJob, SemanticOcrError> {
        self.jobs.cancel(id)?;
        self.jobs.job(id).ok_or(SemanticOcrError::NotFound)
    }

    pub(crate) fn recover(&self) -> Result<(), SemanticOcrError> {
        self.jobs.recover()
    }

    pub(crate) async fn claim_next(&self) -> Result<ClaimedOcrJob, SemanticOcrError> {
        self.jobs.claim_next().await
    }

    pub(crate) async fn process(
        &self,
        library: Arc<SemanticLibraryService>,
        claimed: ClaimedOcrJob,
    ) -> Result<(), SemanticOcrError> {
        if let OcrAvailability::Unavailable { reason, .. } = self.jobs.refresh_availability() {
            return self.jobs.fail_unavailable(&claimed.id, reason);
        }
        for target in claimed.targets {
            if claimed.cancellation.is_cancelled() || !self.jobs.consent() {
                break;
            }
            let plan = self.plan_replacement(&target);
            let mut replacement = None;
            let outcome = match self
                .indexing
                .remediate_file(
                    library.as_ref(),
                    &SemanticAccessContext::Host,
                    target.root_id,
                    &target.location,
                    &claimed.cancellation,
                )
                .await
            {
                Ok(SingleFileIngestOutcome::Ingested(report)) => {
                    let outcome = classify_ingest_report(&report);
                    if matches!(outcome, OcrRemediationOutcome::Succeeded) {
                        self.indexing
                            .clear_ocr_required_file(target.root_id, &target.location);
                        if let Some(plan) = plan {
                            let result = Self::replace_original(plan).await;
                            if result == OcrReplacementOutcome::Replaced {
                                // Refresh the index entry for the new file; it
                                // already has a text layer, so no OCR runs.
                                // Best effort: a later reconcile also catches up.
                                let _ = self
                                    .indexing
                                    .remediate_file(
                                        library.as_ref(),
                                        &SemanticAccessContext::Host,
                                        target.root_id,
                                        &target.location,
                                        &claimed.cancellation,
                                    )
                                    .await;
                            }
                            replacement = Some(result);
                        }
                    }
                    outcome
                }
                Ok(SingleFileIngestOutcome::Oversized) => OcrRemediationOutcome::Skipped {
                    detail: "the file now exceeds the semantic source-size limit".to_owned(),
                },
                Ok(SingleFileIngestOutcome::Ineligible) => {
                    self.indexing
                        .clear_ocr_required_file(target.root_id, &target.location);
                    OcrRemediationOutcome::Skipped {
                        detail: "the file is no longer eligible for semantic indexing".to_owned(),
                    }
                }
                Err(SemanticIndexingError::Cancelled) => {
                    claimed.cancellation.cancel();
                    break;
                }
                Err(error) => OcrRemediationOutcome::ExecutionFailure {
                    detail: error.to_string(),
                },
            };
            self.jobs.record_file_outcome(
                &claimed.id,
                OcrRemediationFileOutcome {
                    root_id: target.root_id,
                    location: target.location,
                    outcome,
                    replacement,
                },
            )?;
        }
        self.purge_retained_output();
        self.jobs.finish(&claimed.id, &claimed.cancellation)
    }

    /// Retained output is single-use; whatever a failed or skipped file left
    /// behind is removed when the job ends. Best effort.
    fn purge_retained_output(&self) {
        let Some(directory) = self.jobs.retention_directory() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn reported_targets(&self) -> Vec<OcrRemediationTarget> {
        self.indexing
            .all_ocr_required_files()
            .into_iter()
            .map(|(root_id, location)| OcrRemediationTarget { root_id, location })
            .collect()
    }
}
