use std::sync::Arc;
use std::time::Duration;

use super::{
    ClaimedOcrJob, OcrAvailability, OcrRemediationFileOutcome, OcrRemediationJob,
    OcrRemediationJobId, OcrRemediationOutcome, OcrRemediationScope, OcrRemediationTarget,
    SemanticOcrError, SemanticOcrService, SemanticOcrStatus, classify_ingest_report,
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
}

impl SemanticOcrCoordinator {
    pub(crate) fn new(
        jobs: Arc<SemanticOcrService>,
        indexing: Arc<SemanticIndexingService>,
    ) -> Self {
        Self { jobs, indexing }
    }

    pub(crate) fn with_jobs(&self, jobs: SemanticOcrService) -> Self {
        Self::new(Arc::new(jobs), Arc::clone(&self.indexing))
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
                },
            )?;
        }
        self.jobs.finish(&claimed.id, &claimed.cancellation)
    }

    fn reported_targets(&self) -> Vec<OcrRemediationTarget> {
        self.indexing
            .all_ocr_required_files()
            .into_iter()
            .map(|(root_id, location)| OcrRemediationTarget { root_id, location })
            .collect()
    }
}
