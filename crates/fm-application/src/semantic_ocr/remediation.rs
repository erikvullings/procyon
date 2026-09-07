//! Durable, bounded, cancellable OCR remediation job registry (task 0197).
//!
//! This module owns the *job* lifecycle: enqueuing remediation work, tracking
//! queued/running/completed/failed/cancelled state, persisting it so it
//! survives a restart, and coordinating a single background processor. It does
//! not run OCR or touch the VFS itself: [`FileManagerService`] claims jobs and
//! drives the actual per-file re-ingestion through the shared semantic
//! indexing capability, reporting each file's typed outcome back here.
//!
//! [`FileManagerService`]: crate::FileManagerService

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fm_domain::Location;
use fm_semantic_library::RootId;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{OcrAvailability, OcrExecutableProbe, OcrPolicyStore, OcrUnavailableReason};
use crate::semantic_indexing::EntryIngestReport;

const JOBS_FILE_NAME: &str = "jobs.json";
const MAX_RETAINED_JOBS: usize = 200;
const MAX_ACTIVE_JOBS: usize = 32;
const MAX_FILES_PER_JOB: usize = 100_000;
const AVAILABILITY_CACHE_TTL: Duration = Duration::from_secs(30);

const POST_OCR_NO_TEXT_MARKER: &str = "OCRmyPDF completed, but";
const EXECUTION_FAILURE_MARKERS: &[&str] = &[
    "OCRmyPDF could not be started",
    "OCRmyPDF exceeded its",
    "OCRmyPDF exited unsuccessfully",
    "OCRmyPDF did not produce",
    "OCRmyPDF output exceeded",
];

/// Opaque identifier for one remediation job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OcrRemediationJobId(String);

impl OcrRemediationJobId {
    fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Wraps an existing identifier string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OcrRemediationJobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One file to remediate, paired with its enrolled root.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OcrRemediationTarget {
    /// Stable path-independent enrolled-root identity.
    pub root_id: RootId,
    /// Source file location.
    pub location: Location,
}

/// Which files a remediation request covers.
///
/// Paths supplied by a frontend are never opened directly. The application
/// expands every variant against its backend-owned OCR-required report before
/// enqueuing concrete targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OcrRemediationScope {
    /// One explicitly selected file.
    File(OcrRemediationTarget),
    /// Multiple explicitly selected files.
    SelectedFiles(Vec<OcrRemediationTarget>),
    /// Every currently reported OCR-required file under one enrolled root.
    Root(RootId),
    /// Every currently reported OCR-required file across all roots.
    AllRequired,
}

/// Lifecycle state of one remediation job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OcrRemediationState {
    /// Waiting for the background processor.
    Queued,
    /// Currently remediating files.
    Running,
    /// Every target file was processed.
    Completed,
    /// The job could not run (for example OCR became unavailable).
    Failed,
    /// The job was cancelled before completing.
    Cancelled,
}

impl OcrRemediationState {
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// Typed per-file remediation outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OcrRemediationOutcome {
    /// OCR produced a text layer and the file was reconverted and ingested.
    Succeeded,
    /// OCR ran but the reconverted document still had no searchable text.
    PostOcrNoText {
        /// Sanitized worker diagnostic.
        detail: String,
    },
    /// OCR could not be executed or failed while running.
    ExecutionFailure {
        /// Sanitized worker diagnostic.
        detail: String,
    },
    /// The file was skipped (no longer eligible, oversized, or missing).
    Skipped {
        /// Reason the file was skipped.
        detail: String,
    },
}

/// One file's recorded remediation result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrRemediationFileOutcome {
    /// Enrolled root the file belongs to.
    pub root_id: RootId,
    /// Source file location.
    pub location: Location,
    /// Typed outcome for this file.
    pub outcome: OcrRemediationOutcome,
}

/// Public snapshot of a remediation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrRemediationJob {
    /// Opaque job identifier.
    pub id: OcrRemediationJobId,
    /// Current lifecycle state.
    pub state: OcrRemediationState,
    /// Wall-clock creation time in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Wall-clock last-update time in milliseconds since the Unix epoch.
    pub updated_at_ms: i64,
    /// Number of target files.
    pub total_files: u64,
    /// Recorded per-file outcomes.
    pub files: Vec<OcrRemediationFileOutcome>,
    /// Typed availability rejection when OCR was unavailable at run time.
    pub availability_failure: Option<OcrUnavailableReason>,
}

impl OcrRemediationJob {
    /// Returns how many target files have a recorded outcome.
    #[must_use]
    pub fn processed_files(&self) -> u64 {
        u64::try_from(self.files.len()).unwrap_or(u64::MAX)
    }
}

/// Typed failures from the OCR remediation capability.
#[derive(Debug, Error)]
pub enum SemanticOcrError {
    /// OCR remediation consent is disabled.
    #[error("OCR remediation is disabled; enable it before starting remediation")]
    Disabled,
    /// No supported executable is available in this host.
    #[error("OCR remediation is unavailable")]
    Unavailable(OcrUnavailableReason),
    /// The referenced job does not exist.
    #[error("OCR remediation job was not found")]
    NotFound,
    /// The requested scope resolved to no files.
    #[error("no files currently require OCR for the requested scope")]
    NothingToRemediate,
    /// An explicit file was not in the backend's current OCR-required report.
    #[error("the requested file is not currently reported as requiring OCR")]
    UnreportedTarget,
    /// The bounded active queue is full.
    #[error("the OCR remediation queue already contains {maximum} active jobs")]
    QueueFull {
        /// Maximum queued and running jobs.
        maximum: usize,
    },
    /// A request expanded beyond the per-job target bound.
    #[error("the OCR remediation request exceeds the {maximum}-file limit")]
    TooManyTargets {
        /// Maximum concrete files in one job.
        maximum: usize,
    },
    /// A transport request contained an invalid identifier or shape.
    #[error("the OCR remediation request is invalid")]
    InvalidRequest,
    /// The worker could not be retired after consent changed.
    #[error("the semantic worker could not be restarted after OCR consent changed: {0}")]
    WorkerRestart(String),
    /// Durable consent or job state could not be persisted.
    #[error("OCR remediation state could not be persisted: {0}")]
    Persist(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedJobs {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    jobs: Vec<PersistedJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedJob {
    job: OcrRemediationJob,
    targets: Vec<OcrRemediationTarget>,
}

#[derive(Clone)]
struct JobEntry {
    job: OcrRemediationJob,
    targets: Vec<OcrRemediationTarget>,
    cancellation: CancellationToken,
}

#[derive(Clone, Default)]
struct RegistryState {
    jobs: BTreeMap<String, JobEntry>,
    order: Vec<String>,
    queue: VecDeque<String>,
}

/// A job claimed by the background processor for execution.
pub(crate) struct ClaimedOcrJob {
    pub(crate) id: OcrRemediationJobId,
    pub(crate) targets: Vec<OcrRemediationTarget>,
    pub(crate) cancellation: CancellationToken,
}

/// Durable OCR consent, availability, and a bounded remediation job queue.
///
/// Construction is inert: it only reads any previously persisted jobs. The
/// background processor is started separately by the host.
pub struct SemanticOcrService {
    policy: Arc<OcrPolicyStore>,
    probe: Arc<dyn OcrExecutableProbe>,
    availability: Mutex<Option<(Instant, OcrAvailability)>>,
    path: PathBuf,
    state: Mutex<RegistryState>,
    notify: Notify,
}

impl SemanticOcrService {
    /// Loads any persisted jobs and prepares an inert remediation service.
    ///
    /// `directory` is the injected settings/app-data root under which consent
    /// and jobs are persisted. Persisted jobs are loaded but not yet requeued;
    /// call [`Self::recover`] once to resume interrupted work.
    #[must_use]
    pub fn load(
        directory: impl Into<PathBuf>,
        policy: Arc<OcrPolicyStore>,
        probe: Arc<dyn OcrExecutableProbe>,
    ) -> Self {
        let directory = directory.into();
        let path = directory.join(JOBS_FILE_NAME);
        let mut state = RegistryState::default();
        if let Some(persisted) = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PersistedJobs>(&bytes).ok())
            .filter(|persisted| persisted.schema_version == 1)
        {
            for persisted in persisted.jobs {
                let id = persisted.job.id.0.clone();
                state.order.push(id.clone());
                state.jobs.insert(
                    id,
                    JobEntry {
                        job: persisted.job,
                        targets: persisted.targets,
                        cancellation: CancellationToken::new(),
                    },
                );
            }
        }
        Self {
            policy,
            probe,
            availability: Mutex::new(None),
            path,
            state: Mutex::new(state),
            notify: Notify::new(),
        }
    }

    /// Returns the injected availability probe result.
    #[must_use]
    pub(crate) fn availability(&self) -> OcrAvailability {
        let mut cached = self
            .availability
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((checked_at, availability)) = cached.as_ref()
            && checked_at.elapsed() < AVAILABILITY_CACHE_TTL
        {
            return availability.clone();
        }
        let availability = self.probe.probe();
        *cached = Some((Instant::now(), availability.clone()));
        availability
    }

    /// Re-runs bounded host discovery, bypassing the short UI polling cache.
    #[must_use]
    pub(crate) fn refresh_availability(&self) -> OcrAvailability {
        let availability = self.probe.probe();
        *self
            .availability
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Instant::now(), availability.clone()));
        availability
    }

    /// Returns whether OCR remediation consent is enabled.
    #[must_use]
    pub(crate) fn consent(&self) -> bool {
        self.policy.enabled()
    }

    /// Returns the current availability, consent, reported targets, and jobs.
    #[must_use]
    pub(crate) fn status(
        &self,
        reported_files: Vec<OcrRemediationTarget>,
    ) -> super::SemanticOcrStatus {
        super::SemanticOcrStatus {
            enabled: self.consent(),
            availability: self.availability(),
            reported_files,
            jobs: self.jobs(),
        }
    }

    /// Durably records explicit consent, returning whether the value changed.
    ///
    /// Disabling consent cancels every non-terminal job so no queued or running
    /// remediation can execute OCR after the user opts out.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticOcrError::Persist`] if the durable file cannot be
    /// written.
    pub(crate) fn set_consent(&self, enabled: bool) -> Result<bool, SemanticOcrError> {
        if enabled && let OcrAvailability::Unavailable { reason, .. } = self.refresh_availability()
        {
            return Err(SemanticOcrError::Unavailable(reason));
        }
        let changed = self
            .policy
            .set_enabled(enabled)
            .map_err(|error| SemanticOcrError::Persist(error.to_string()))?;
        if changed && !enabled {
            self.cancel_all_active()?;
        }
        Ok(changed)
    }

    /// Enqueues a remediation job for the resolved target files.
    ///
    /// Consent must be enabled; the caller resolves the scope into a concrete,
    /// deterministic target list first so a restart resumes the same work.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticOcrError::Disabled`] when consent is off,
    /// [`SemanticOcrError::NothingToRemediate`] for an empty target list, or a
    /// persistence failure.
    pub(crate) fn enqueue(
        &self,
        mut targets: Vec<OcrRemediationTarget>,
    ) -> Result<OcrRemediationJobId, SemanticOcrError> {
        if !self.policy.enabled() {
            return Err(SemanticOcrError::Disabled);
        }
        if let OcrAvailability::Unavailable { reason, .. } = self.probe.probe() {
            return Err(SemanticOcrError::Unavailable(reason));
        }
        if targets.is_empty() {
            return Err(SemanticOcrError::NothingToRemediate);
        }
        let mut seen = HashSet::with_capacity(targets.len());
        targets.retain(|target| seen.insert(target.clone()));
        if targets.len() > MAX_FILES_PER_JOB {
            return Err(SemanticOcrError::TooManyTargets {
                maximum: MAX_FILES_PER_JOB,
            });
        }
        let id = OcrRemediationJobId::generate();
        let now = now_ms();
        let job = OcrRemediationJob {
            id: id.clone(),
            state: OcrRemediationState::Queued,
            created_at_ms: now,
            updated_at_ms: now,
            total_files: u64::try_from(targets.len()).unwrap_or(u64::MAX),
            files: Vec::new(),
            availability_failure: None,
        };
        {
            let mut state = self.lock();
            let previous = state.clone();
            let active = state
                .jobs
                .values()
                .filter(|entry| !entry.job.state.is_terminal())
                .count();
            if active >= MAX_ACTIVE_JOBS {
                return Err(SemanticOcrError::QueueFull {
                    maximum: MAX_ACTIVE_JOBS,
                });
            }
            state.order.push(id.0.clone());
            state.queue.push_back(id.0.clone());
            state.jobs.insert(
                id.0.clone(),
                JobEntry {
                    job,
                    targets,
                    cancellation: CancellationToken::new(),
                },
            );
            prune(&mut state);
            if let Err(error) = self.persist(&state) {
                *state = previous;
                return Err(error);
            }
        }
        self.notify.notify_one();
        Ok(id)
    }

    /// Returns a snapshot of one job.
    #[must_use]
    pub(crate) fn job(&self, id: &OcrRemediationJobId) -> Option<OcrRemediationJob> {
        self.lock().jobs.get(&id.0).map(|entry| entry.job.clone())
    }

    /// Returns snapshots of every retained job, newest first.
    #[must_use]
    pub(crate) fn jobs(&self) -> Vec<OcrRemediationJob> {
        let state = self.lock();
        state
            .order
            .iter()
            .rev()
            .filter_map(|id| state.jobs.get(id).map(|entry| entry.job.clone()))
            .collect()
    }

    /// Cancels a queued or running job.
    ///
    /// A queued job is removed from the queue and marked cancelled; a running
    /// job's cancellation token is triggered so the processor finishes it as
    /// cancelled at the next file boundary.
    ///
    /// # Errors
    ///
    /// Returns [`SemanticOcrError::NotFound`] for an unknown job, or a
    /// persistence failure.
    pub(crate) fn cancel(&self, id: &OcrRemediationJobId) -> Result<(), SemanticOcrError> {
        let mut state = self.lock();
        let previous = state.clone();
        let entry = state
            .jobs
            .get_mut(&id.0)
            .ok_or(SemanticOcrError::NotFound)?;
        if entry.job.state.is_terminal() {
            return Ok(());
        }
        entry.cancellation.cancel();
        entry.job.state = OcrRemediationState::Cancelled;
        entry.job.updated_at_ms = now_ms();
        state.queue.retain(|queued| queued != &id.0);
        if let Err(error) = self.persist(&state) {
            *state = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Requeues interrupted work after a restart.
    ///
    /// Any job left `Running` when the process stopped is reset to `Queued`
    /// (its partial file outcomes are cleared so its deterministic target list
    /// is re-run from the start), and every `Queued` job is re-enqueued for the
    /// background processor. Terminal jobs are left untouched as history.
    ///
    /// # Errors
    ///
    /// Returns a persistence failure.
    pub(crate) fn recover(&self) -> Result<(), SemanticOcrError> {
        let mut requeued = false;
        {
            let mut state = self.lock();
            if state.order.is_empty() {
                return Ok(());
            }
            let previous = state.clone();
            let ids: Vec<String> = state.order.clone();
            state.queue.clear();
            for id in ids {
                if let Some(entry) = state.jobs.get_mut(&id) {
                    if !self.policy.enabled() && !entry.job.state.is_terminal() {
                        entry.job.state = OcrRemediationState::Cancelled;
                        entry.job.updated_at_ms = now_ms();
                        entry.cancellation.cancel();
                        continue;
                    }
                    match entry.job.state {
                        OcrRemediationState::Running => {
                            entry.job.state = OcrRemediationState::Queued;
                            entry.job.availability_failure = None;
                            entry.job.updated_at_ms = now_ms();
                            entry.cancellation = CancellationToken::new();
                            state.queue.push_back(id.clone());
                            requeued = true;
                        }
                        OcrRemediationState::Queued => {
                            entry.cancellation = CancellationToken::new();
                            state.queue.push_back(id.clone());
                            requeued = true;
                        }
                        OcrRemediationState::Completed
                        | OcrRemediationState::Failed
                        | OcrRemediationState::Cancelled => {}
                    }
                }
            }
            if let Err(error) = self.persist(&state) {
                *state = previous;
                return Err(error);
            }
        }
        if requeued {
            self.notify.notify_one();
        }
        Ok(())
    }

    /// Waits for and claims the next queued job, marking it running.
    ///
    /// Resolves only once a job is available. A job cancelled while queued is
    /// skipped. The returned cancellation token is triggered if the job is
    /// later cancelled.
    pub(crate) async fn claim_next(&self) -> Result<ClaimedOcrJob, SemanticOcrError> {
        loop {
            let notified = self.notify.notified();
            if let Some(claimed) = self.try_claim()? {
                return Ok(claimed);
            }
            notified.await;
        }
    }

    fn try_claim(&self) -> Result<Option<ClaimedOcrJob>, SemanticOcrError> {
        let mut state = self.lock();
        let previous = state.clone();
        while let Some(id) = state.queue.pop_front() {
            let Some(entry) = state.jobs.get_mut(&id) else {
                continue;
            };
            if entry.job.state != OcrRemediationState::Queued {
                continue;
            }
            let completed = &entry.job.files;
            let targets = entry
                .targets
                .iter()
                .filter(|target| {
                    !completed.iter().any(|outcome| {
                        outcome.root_id == target.root_id && outcome.location == target.location
                    })
                })
                .cloned()
                .collect::<Vec<_>>();
            if targets.is_empty() {
                entry.job.state = OcrRemediationState::Completed;
                entry.job.updated_at_ms = now_ms();
                if let Err(error) = self.persist(&state) {
                    *state = previous;
                    return Err(error);
                }
                continue;
            }
            entry.job.state = OcrRemediationState::Running;
            entry.job.updated_at_ms = now_ms();
            let claimed = ClaimedOcrJob {
                id: OcrRemediationJobId(id.clone()),
                targets,
                cancellation: entry.cancellation.clone(),
            };
            if let Err(error) = self.persist(&state) {
                *state = previous;
                return Err(error);
            }
            return Ok(Some(claimed));
        }
        Ok(None)
    }

    /// Records one file's typed outcome on a running job.
    pub(crate) fn record_file_outcome(
        &self,
        id: &OcrRemediationJobId,
        outcome: OcrRemediationFileOutcome,
    ) -> Result<(), SemanticOcrError> {
        let mut state = self.lock();
        if let Some(entry) = state.jobs.get_mut(&id.0) {
            if entry.job.state == OcrRemediationState::Running {
                entry.job.files.push(outcome);
                entry.job.updated_at_ms = now_ms();
            }
        } else {
            return Err(SemanticOcrError::NotFound);
        }
        self.persist(&state)
    }

    /// Marks a job as failed because OCR was unavailable at run time.
    pub(crate) fn fail_unavailable(
        &self,
        id: &OcrRemediationJobId,
        reason: OcrUnavailableReason,
    ) -> Result<(), SemanticOcrError> {
        let mut state = self.lock();
        let previous = state.clone();
        if let Some(entry) = state.jobs.get_mut(&id.0) {
            entry.job.state = OcrRemediationState::Failed;
            entry.job.availability_failure = Some(reason);
            entry.job.updated_at_ms = now_ms();
        } else {
            return Err(SemanticOcrError::NotFound);
        }
        if let Err(error) = self.persist(&state) {
            *state = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Finishes a job as completed or cancelled based on its cancellation.
    pub(crate) fn finish(
        &self,
        id: &OcrRemediationJobId,
        cancellation: &CancellationToken,
    ) -> Result<(), SemanticOcrError> {
        let mut state = self.lock();
        let previous = state.clone();
        if let Some(entry) = state.jobs.get_mut(&id.0) {
            if entry.job.state.is_terminal() {
                return Ok(());
            }
            entry.job.state = if cancellation.is_cancelled() {
                OcrRemediationState::Cancelled
            } else if entry.job.files.iter().any(|file| {
                matches!(
                    file.outcome,
                    OcrRemediationOutcome::PostOcrNoText { .. }
                        | OcrRemediationOutcome::ExecutionFailure { .. }
                )
            }) {
                OcrRemediationState::Failed
            } else {
                OcrRemediationState::Completed
            };
            entry.job.updated_at_ms = now_ms();
        } else {
            return Err(SemanticOcrError::NotFound);
        }
        if let Err(error) = self.persist(&state) {
            *state = previous;
            return Err(error);
        }
        Ok(())
    }

    fn cancel_all_active(&self) -> Result<(), SemanticOcrError> {
        let mut state = self.lock();
        let previous = state.clone();
        let ids: Vec<String> = state.jobs.keys().cloned().collect();
        for id in ids {
            if let Some(entry) = state.jobs.get_mut(&id)
                && !entry.job.state.is_terminal()
            {
                entry.cancellation.cancel();
                entry.job.state = OcrRemediationState::Cancelled;
                entry.job.updated_at_ms = now_ms();
            }
        }
        state.queue.clear();
        if let Err(error) = self.persist(&state) {
            *state = previous;
            return Err(error);
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegistryState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn persist(&self, state: &RegistryState) -> Result<(), SemanticOcrError> {
        let persisted = PersistedJobs {
            schema_version: 1,
            jobs: state
                .order
                .iter()
                .filter_map(|id| state.jobs.get(id))
                .map(|entry| PersistedJob {
                    job: entry.job.clone(),
                    targets: entry.targets.clone(),
                })
                .collect(),
        };
        persist_jobs(&self.path, &persisted)
            .map_err(|error| SemanticOcrError::Persist(error.to_string()))
    }
}

fn prune(state: &mut RegistryState) {
    while state.order.len() > MAX_RETAINED_JOBS {
        // Drop the oldest terminal job; never evict active work.
        let Some(position) = state.order.iter().position(|id| {
            state
                .jobs
                .get(id)
                .is_some_and(|entry| entry.job.state.is_terminal())
        }) else {
            break;
        };
        let id = state.order.remove(position);
        state.jobs.remove(&id);
    }
}

fn persist_jobs(path: &Path, jobs: &PersistedJobs) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("OCR jobs path has no parent directory"))?;
    let bytes = serde_json::to_vec_pretty(jobs).map_err(std::io::Error::other)?;
    fm_settings::atomic_write(directory, JOBS_FILE_NAME, &bytes)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

/// Classifies one file's bounded ingestion report into a typed OCR outcome.
///
/// The worker emits distinct, stable diagnostics for its OCR paths; this maps
/// them to the four user-visible outcomes without changing the converter.
pub(crate) fn classify_ingest_report(report: &EntryIngestReport) -> OcrRemediationOutcome {
    let joined = report.exclusion_details.join(" \u{2014} ");
    if joined.contains(POST_OCR_NO_TEXT_MARKER) {
        return OcrRemediationOutcome::PostOcrNoText { detail: joined };
    }
    if report.failed > 0
        || EXECUTION_FAILURE_MARKERS
            .iter()
            .any(|marker| joined.contains(marker))
    {
        let detail = if joined.is_empty() {
            "OCR ingestion failed".to_owned()
        } else {
            joined
        };
        return OcrRemediationOutcome::ExecutionFailure { detail };
    }
    if report.excluded > 0 {
        // Excluded for a reason unrelated to OCR execution (or OCR did not run
        // at all); surface the diagnostic as an execution failure so it is not
        // silently reported as success.
        return OcrRemediationOutcome::ExecutionFailure { detail: joined };
    }
    if report.ingested > 0 {
        return OcrRemediationOutcome::Succeeded;
    }
    OcrRemediationOutcome::Skipped {
        detail: "no workspace ingestion occurred".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AvailableProbe;

    impl OcrExecutableProbe for AvailableProbe {
        fn probe(&self) -> OcrAvailability {
            OcrAvailability::Available {
                executable: PathBuf::from("/usr/local/bin/ocrmypdf"),
                version: "16.10.4".to_owned(),
            }
        }
    }

    fn temp_dir() -> tempfile::TempDir {
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-ocr-tests");
        std::fs::create_dir_all(&parent).unwrap();
        tempfile::Builder::new()
            .prefix("ocr-jobs-")
            .tempdir_in(std::fs::canonicalize(parent).unwrap())
            .unwrap()
    }

    fn service(directory: &Path, enabled: bool) -> SemanticOcrService {
        let policy = Arc::new(OcrPolicyStore::load(directory));
        policy.set_enabled(enabled).unwrap();
        SemanticOcrService::load(directory, policy, Arc::new(AvailableProbe))
    }

    fn target(seed: u128) -> OcrRemediationTarget {
        OcrRemediationTarget {
            root_id: RootId::from_uuid(Uuid::from_u128(seed)),
            location: Location::parse(&format!("file:///root/scan-{seed}.pdf")).unwrap(),
        }
    }

    #[test]
    fn enqueue_is_refused_when_consent_is_disabled() {
        let directory = temp_dir();
        let service = service(directory.path(), false);
        assert!(matches!(
            service.enqueue(vec![target(1)]),
            Err(SemanticOcrError::Disabled)
        ));
    }

    #[test]
    fn empty_scope_is_rejected() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        assert!(matches!(
            service.enqueue(Vec::new()),
            Err(SemanticOcrError::NothingToRemediate)
        ));
    }

    #[test]
    fn enqueue_is_refused_when_no_supported_executable_is_available() {
        let directory = temp_dir();
        let policy = Arc::new(OcrPolicyStore::load(directory.path()));
        policy.set_enabled(true).unwrap();
        let service = SemanticOcrService::load(
            directory.path(),
            policy,
            Arc::new(super::super::UnavailableOcrExecutableProbe),
        );

        assert!(matches!(
            service.enqueue(vec![target(1)]),
            Err(SemanticOcrError::Unavailable(
                OcrUnavailableReason::HostUnavailable
            ))
        ));
    }

    #[test]
    fn recovering_an_empty_registry_does_not_create_state() {
        let directory = temp_dir();
        let service = service(directory.path(), false);

        service.recover().unwrap();

        assert!(!directory.path().join(JOBS_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn queued_job_is_claimed_running_and_completed() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        let id = service.enqueue(vec![target(1), target(2)]).unwrap();
        assert_eq!(service.job(&id).unwrap().state, OcrRemediationState::Queued);

        let claimed = service.claim_next().await.unwrap();
        assert_eq!(claimed.id, id);
        assert_eq!(claimed.targets.len(), 2);
        assert_eq!(
            service.job(&id).unwrap().state,
            OcrRemediationState::Running
        );

        service
            .record_file_outcome(
                &id,
                OcrRemediationFileOutcome {
                    root_id: claimed.targets[0].root_id,
                    location: claimed.targets[0].location.clone(),
                    outcome: OcrRemediationOutcome::Succeeded,
                },
            )
            .unwrap();
        service.finish(&id, &claimed.cancellation).unwrap();
        let job = service.job(&id).unwrap();
        assert_eq!(job.state, OcrRemediationState::Completed);
        assert_eq!(job.processed_files(), 1);
    }

    #[tokio::test]
    async fn post_ocr_no_text_marks_the_job_failed_with_a_typed_file_outcome() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        let id = service.enqueue(vec![target(1)]).unwrap();
        let claimed = service.claim_next().await.unwrap();
        service
            .record_file_outcome(
                &id,
                OcrRemediationFileOutcome {
                    root_id: claimed.targets[0].root_id,
                    location: claimed.targets[0].location.clone(),
                    outcome: OcrRemediationOutcome::PostOcrNoText {
                        detail: "no searchable text".to_owned(),
                    },
                },
            )
            .unwrap();
        service.finish(&id, &claimed.cancellation).unwrap();

        assert_eq!(service.job(&id).unwrap().state, OcrRemediationState::Failed);
    }

    #[tokio::test]
    async fn cancelling_a_running_job_finishes_as_cancelled() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        let id = service.enqueue(vec![target(7)]).unwrap();
        let claimed = service.claim_next().await.unwrap();
        service.cancel(&id).unwrap();
        assert!(claimed.cancellation.is_cancelled());
        service.finish(&id, &claimed.cancellation).unwrap();
        assert_eq!(
            service.job(&id).unwrap().state,
            OcrRemediationState::Cancelled
        );
    }

    #[test]
    fn cancelling_a_queued_job_marks_it_cancelled_immediately() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        let id = service.enqueue(vec![target(3)]).unwrap();
        service.cancel(&id).unwrap();
        assert_eq!(
            service.job(&id).unwrap().state,
            OcrRemediationState::Cancelled
        );
    }

    #[test]
    fn disabling_consent_cancels_pending_jobs() {
        let directory = temp_dir();
        let service = service(directory.path(), true);
        let id = service.enqueue(vec![target(4)]).unwrap();
        assert!(service.set_consent(false).unwrap());
        assert_eq!(
            service.job(&id).unwrap().state,
            OcrRemediationState::Cancelled
        );
        assert!(matches!(
            service.enqueue(vec![target(5)]),
            Err(SemanticOcrError::Disabled)
        ));
    }

    #[tokio::test]
    async fn restart_recovery_requeues_interrupted_running_jobs() {
        let directory = temp_dir();
        {
            let service = service(directory.path(), true);
            let id = service.enqueue(vec![target(9), target(10)]).unwrap();
            let claimed = service.claim_next().await.unwrap();
            assert_eq!(claimed.id, id);
            service
                .record_file_outcome(
                    &id,
                    OcrRemediationFileOutcome {
                        root_id: claimed.targets[0].root_id,
                        location: claimed.targets[0].location.clone(),
                        outcome: OcrRemediationOutcome::Succeeded,
                    },
                )
                .unwrap();
            // Simulate a crash: the job is left Running and persisted.
            assert_eq!(
                service.job(&id).unwrap().state,
                OcrRemediationState::Running
            );
        }

        // A fresh process reloads durable state and recovers it.
        let reloaded = service(directory.path(), true);
        let running = reloaded.jobs();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].state, OcrRemediationState::Running);

        reloaded.recover().unwrap();
        let recovered = &reloaded.jobs()[0];
        assert_eq!(recovered.state, OcrRemediationState::Queued);
        assert_eq!(recovered.files.len(), 1);

        // The requeued job resumes only the unfinished deterministic target.
        let claimed = reloaded.claim_next().await.unwrap();
        assert_eq!(claimed.targets, vec![target(10)]);
    }

    #[test]
    fn restart_recovery_cancels_pending_work_when_consent_is_disabled() {
        let directory = temp_dir();
        let id = {
            let service = service(directory.path(), true);
            service.enqueue(vec![target(12)]).unwrap()
        };
        OcrPolicyStore::load(directory.path())
            .set_enabled(false)
            .unwrap();

        let reloaded = service(directory.path(), false);
        reloaded.recover().unwrap();

        assert_eq!(
            reloaded.job(&id).unwrap().state,
            OcrRemediationState::Cancelled
        );
    }

    #[test]
    fn classification_maps_worker_diagnostics_to_typed_outcomes() {
        let succeeded = EntryIngestReport {
            occurrence_id: fm_semantic_library::OccurrenceId::from_uuid(Uuid::from_u128(1)),
            ingested: 1,
            failed: 0,
            excluded: 0,
            exclusion_details: Vec::new(),
            requires_ocr: false,
        };
        assert_eq!(
            classify_ingest_report(&succeeded),
            OcrRemediationOutcome::Succeeded
        );

        let post_ocr = EntryIngestReport {
            occurrence_id: fm_semantic_library::OccurrenceId::from_uuid(Uuid::from_u128(2)),
            ingested: 0,
            failed: 0,
            excluded: 1,
            exclusion_details: vec![
                "OCRmyPDF completed, but deterministic conversion still found no searchable text."
                    .to_owned(),
            ],
            requires_ocr: true,
        };
        assert!(matches!(
            classify_ingest_report(&post_ocr),
            OcrRemediationOutcome::PostOcrNoText { .. }
        ));

        let execution = EntryIngestReport {
            occurrence_id: fm_semantic_library::OccurrenceId::from_uuid(Uuid::from_u128(3)),
            ingested: 0,
            failed: 0,
            excluded: 1,
            exclusion_details: vec!["OCRmyPDF exited unsuccessfully (exit status: 2).".to_owned()],
            requires_ocr: true,
        };
        assert!(matches!(
            classify_ingest_report(&execution),
            OcrRemediationOutcome::ExecutionFailure { .. }
        ));

        let ingestion_failed = EntryIngestReport {
            occurrence_id: fm_semantic_library::OccurrenceId::from_uuid(Uuid::from_u128(4)),
            ingested: 0,
            failed: 1,
            excluded: 0,
            exclusion_details: Vec::new(),
            requires_ocr: false,
        };
        assert!(matches!(
            classify_ingest_report(&ingestion_failed),
            OcrRemediationOutcome::ExecutionFailure { .. }
        ));
    }
}
