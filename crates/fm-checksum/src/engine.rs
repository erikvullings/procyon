//! Cancellable, progress-reporting checksum and duplicate-detection engine
//! jobs (task 0077, spec §16 milestone 5, §17, §18).
//!
//! Shaped after `fm_comparison::ComparisonEngine` and `fm_search::engine`:
//! a job is started with an externally chosen id that doubles as its
//! [`OperationId`], results stream into a store in batches rather than being
//! collected into one giant response, and the generic
//! `/operations/{id}/cancel` route can stop it because the store owns the
//! job's [`CancellationToken`].
//!
//! Availability is gated by [`ProviderCapabilities::CHECKSUM`] (spec §6):
//! every target is checked *before* any work is scheduled, so an unsupported
//! provider is rejected synchronously by the caller's request rather than
//! surfacing later as a stream of per-entry failures.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fm_domain::{EntryKind, EntrySummary, Location, OperationId};
use fm_events::{
    BackendEventPayload, ChecksumEntryPayload, DuplicateGroupPayload, EventAudience, EventBus,
    HardlinkClusterPayload, OperationProgressDetails, OperationProgressPayload,
    OperationStatePayload,
};
use fm_vfs::{EntryRef, ListOptions, ProviderCapabilities, ProviderRegistry};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::duplicates::{
    DuplicateCandidate, DuplicateGroup, DuplicateObserver, DuplicateOptions, DuplicateProgress,
    DuplicateStage, ScanOutcome, find_duplicates_observed,
};
use crate::hash::{ChecksumAlgorithm, hash_entry};
use crate::store::{ChecksumEntryResult, ChecksumResultsStore, DuplicateResultsStore};

/// Maximum number of hashed entries buffered before a batch is flushed.
const BATCH_SIZE: usize = 50;
/// Maximum time a partial batch is held before being flushed anyway, so a
/// slow job still streams progress promptly.
const BATCH_INTERVAL: Duration = Duration::from_millis(100);

/// Errors starting or controlling a checksum or duplicate job.
#[derive(Debug, thiserror::Error)]
pub enum ChecksumEngineError {
    /// A target or root cannot be resolved to a registered provider.
    #[error("checksum target cannot be resolved: {0}")]
    InvalidTarget(String),
    /// The owning provider does not support checksum calculation.
    #[error("provider for `{uri}` does not support checksum calculation")]
    UnsupportedCapability {
        /// URI of the entry whose provider lacks `CHECKSUM`.
        uri: String,
    },
    /// The request named no algorithm, or no target.
    #[error("{0}")]
    EmptyRequest(String),
    /// No job is tracked under this id.
    #[error("no checksum job is tracked with id {0}")]
    NotFound(Uuid),
}

/// One entry a checksum job should hash.
#[derive(Debug, Clone)]
pub struct ChecksumTarget {
    /// The entry to hash.
    pub entry: EntryRef,
    /// Path relative to the selection's common root, used for display and
    /// for the `<digest>  <path>` line of a saved checksum file.
    pub relative_path: String,
    /// Byte size, when the caller already knows it.
    pub size: u64,
}

/// Parameters for a checksum job.
#[derive(Debug, Clone)]
pub struct ChecksumJobOptions {
    /// Algorithms to compute, all in a single pass over each file.
    pub algorithms: Vec<ChecksumAlgorithm>,
    /// When present, the engine emits `operation.*` events so the operation
    /// centre can track and cancel this job.
    pub operation_id: Option<OperationId>,
}

/// Parameters for a duplicate scan.
#[derive(Debug, Clone)]
pub struct DuplicateScanOptions {
    /// Staged-detection tuning.
    pub detection: DuplicateOptions,
    /// Whether hidden entries are included in the scan.
    pub show_hidden: bool,
    /// When present, the engine emits `operation.*` events.
    pub operation_id: Option<OperationId>,
}

/// Starts and cancels checksum jobs and duplicate scans.
pub struct ChecksumEngine {
    checksums: Arc<ChecksumResultsStore>,
    duplicates: Arc<DuplicateResultsStore>,
    events: EventBus,
    providers: ProviderRegistry,
}

impl ChecksumEngine {
    /// Creates an engine writing into `checksums`/`duplicates` and streaming
    /// batches over `events`.
    #[must_use]
    pub fn new(
        checksums: Arc<ChecksumResultsStore>,
        duplicates: Arc<DuplicateResultsStore>,
        events: EventBus,
        providers: ProviderRegistry,
    ) -> Self {
        Self {
            checksums,
            duplicates,
            events,
            providers,
        }
    }

    /// Verifies every location's provider is registered and advertises
    /// [`ProviderCapabilities::CHECKSUM`].
    ///
    /// # Errors
    ///
    /// Returns [`ChecksumEngineError::InvalidTarget`] for an unresolvable
    /// location and [`ChecksumEngineError::UnsupportedCapability`] when the
    /// provider cannot checksum.
    fn require_checksum_capability(&self, location: &Location) -> Result<(), ChecksumEngineError> {
        let provider = self
            .providers
            .resolve(location)
            .map_err(|error| ChecksumEngineError::InvalidTarget(error.to_string()))?;
        let capabilities = provider
            .capabilities_for(location)
            .unwrap_or_else(|_| provider.capabilities());
        if capabilities.contains(ProviderCapabilities::CHECKSUM) {
            Ok(())
        } else {
            Err(ChecksumEngineError::UnsupportedCapability {
                uri: location.uri.clone(),
            })
        }
    }

    /// Starts a cancellable checksum job over `targets`.
    ///
    /// # Errors
    ///
    /// Returns [`ChecksumEngineError::EmptyRequest`] if no algorithm or no
    /// target was supplied, and otherwise as
    /// [`Self::require_checksum_capability`] — the capability gate runs over
    /// every target before any hashing is scheduled.
    pub fn start_checksums(
        &self,
        job_id: Uuid,
        targets: Vec<ChecksumTarget>,
        options: ChecksumJobOptions,
        audience: EventAudience,
    ) -> Result<(), ChecksumEngineError> {
        if options.algorithms.is_empty() {
            return Err(ChecksumEngineError::EmptyRequest(
                "at least one checksum algorithm must be requested".to_owned(),
            ));
        }
        if targets.is_empty() {
            return Err(ChecksumEngineError::EmptyRequest(
                "at least one entry must be selected".to_owned(),
            ));
        }
        for target in &targets {
            self.require_checksum_capability(&target.entry.location)?;
        }

        let cancellation = CancellationToken::new();
        self.checksums.register(
            job_id,
            options.algorithms.clone(),
            targets.len(),
            cancellation.clone(),
        );

        let store = Arc::clone(&self.checksums);
        let events = self.events.clone();
        let providers = self.providers.clone();
        tokio::spawn(async move {
            run_checksum_job(
                job_id,
                targets,
                &options,
                &cancellation,
                &store,
                &events,
                &audience,
                &providers,
            )
            .await;
        });
        Ok(())
    }

    /// Starts a cancellable duplicate scan across `roots`.
    ///
    /// # Errors
    ///
    /// As [`Self::start_checksums`]; the capability gate runs over every root.
    pub fn start_duplicate_scan(
        &self,
        scan_id: Uuid,
        roots: Vec<Location>,
        options: DuplicateScanOptions,
        audience: EventAudience,
    ) -> Result<(), ChecksumEngineError> {
        if roots.is_empty() {
            return Err(ChecksumEngineError::EmptyRequest(
                "at least one root must be supplied".to_owned(),
            ));
        }
        for root in &roots {
            self.require_checksum_capability(root)?;
        }

        let cancellation = CancellationToken::new();
        self.duplicates
            .register(scan_id, roots.clone(), cancellation.clone());

        let store = Arc::clone(&self.duplicates);
        let events = self.events.clone();
        let providers = self.providers.clone();
        tokio::spawn(async move {
            run_duplicate_scan(
                scan_id,
                roots,
                &options,
                &cancellation,
                &store,
                &events,
                &audience,
                &providers,
            )
            .await;
        });
        Ok(())
    }

    /// Requests prompt cancellation of a running checksum job.
    ///
    /// # Errors
    ///
    /// Returns [`ChecksumEngineError::NotFound`] if no job is tracked.
    pub fn cancel_checksums(&self, job_id: Uuid) -> Result<(), ChecksumEngineError> {
        self.checksums
            .cancel(job_id)
            .ok_or(ChecksumEngineError::NotFound(job_id))
    }

    /// Requests prompt cancellation of a running duplicate scan.
    ///
    /// # Errors
    ///
    /// Returns [`ChecksumEngineError::NotFound`] if no scan is tracked.
    pub fn cancel_duplicate_scan(&self, scan_id: Uuid) -> Result<(), ChecksumEngineError> {
        self.duplicates
            .cancel(scan_id)
            .ok_or(ChecksumEngineError::NotFound(scan_id))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_checksum_job(
    job_id: Uuid,
    targets: Vec<ChecksumTarget>,
    options: &ChecksumJobOptions,
    cancellation: &CancellationToken,
    store: &ChecksumResultsStore,
    events: &EventBus,
    audience: &EventAudience,
    providers: &ProviderRegistry,
) {
    let total = targets.len() as u64;
    let mut buffer: Vec<ChecksumEntryResult> = Vec::with_capacity(BATCH_SIZE);
    let mut completed: u64 = 0;
    let mut completed_bytes: u64 = 0;
    let mut last_flush = Instant::now();
    let mut cancelled = false;

    for target in targets {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let result = hash_entry(providers, &target.entry, &options.algorithms, cancellation).await;
        let entry = match result {
            Ok(checksums) => {
                completed_bytes += checksums.bytes_hashed();
                ChecksumEntryResult {
                    location: target.entry.location.clone(),
                    relative_path: target.relative_path.clone(),
                    size: checksums.bytes_hashed(),
                    checksums,
                    error: None,
                }
            }
            Err(crate::ChecksumError::Cancelled) => {
                cancelled = true;
                break;
            }
            Err(error) => ChecksumEntryResult {
                location: target.entry.location.clone(),
                relative_path: target.relative_path.clone(),
                size: target.size,
                checksums: crate::ChecksumSet::default(),
                error: Some(error.to_string()),
            },
        };
        buffer.push(entry);
        completed += 1;

        if buffer.len() >= BATCH_SIZE || last_flush.elapsed() >= BATCH_INTERVAL {
            flush_checksums(
                job_id,
                &mut buffer,
                false,
                false,
                store,
                events,
                audience,
                options,
                completed,
                total,
                completed_bytes,
            );
            last_flush = Instant::now();
        }
    }

    flush_checksums(
        job_id,
        &mut buffer,
        true,
        cancelled,
        store,
        events,
        audience,
        options,
        completed,
        total,
        completed_bytes,
    );

    if let Some(operation_id) = options.operation_id {
        let state = if cancelled {
            OperationStatePayload::Cancelled
        } else {
            OperationStatePayload::Completed
        };
        events.publish(
            audience.clone(),
            BackendEventPayload::OperationStateChanged {
                operation_id,
                state,
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_checksums(
    job_id: Uuid,
    buffer: &mut Vec<ChecksumEntryResult>,
    is_complete: bool,
    is_cancelled: bool,
    store: &ChecksumResultsStore,
    events: &EventBus,
    audience: &EventAudience,
    options: &ChecksumJobOptions,
    completed: u64,
    total: u64,
    completed_bytes: u64,
) {
    if buffer.is_empty() && !is_complete {
        return;
    }
    let batch = std::mem::take(buffer);
    let payload_entries: Vec<ChecksumEntryPayload> = batch.iter().map(checksum_payload).collect();
    store.append(job_id, batch);
    // Mark the store complete before publishing, matching
    // `fm_comparison::engine::flush`'s ordering rationale: a subscriber that
    // reacts to `is_complete: true` by immediately paging the store must
    // never observe stale `has_more: true` from a completion race.
    if is_complete {
        store.mark_complete(job_id, is_cancelled);
    }
    events.publish(
        audience.clone(),
        BackendEventPayload::ChecksumResultsBatch {
            job_id,
            entries: payload_entries,
            is_complete,
            is_cancelled,
        },
    );
    if let Some(operation_id) = options.operation_id {
        events.publish(
            audience.clone(),
            BackendEventPayload::OperationProgress {
                progress: OperationProgressPayload {
                    operation_id,
                    progress: OperationProgressDetails {
                        completed_items: completed,
                        total_items: Some(total),
                        completed_bytes,
                        total_bytes: None,
                        current_entry: None,
                        bytes_per_second: None,
                    },
                },
            },
        );
    }
}

fn checksum_payload(entry: &ChecksumEntryResult) -> ChecksumEntryPayload {
    ChecksumEntryPayload {
        location: entry.location.clone().into(),
        relative_path: entry.relative_path.clone(),
        size: entry.size,
        checksums: entry
            .checksums
            .iter()
            .map(|(algorithm, digest)| (algorithm.to_string(), digest.to_owned()))
            .collect(),
        error: entry.error.clone(),
    }
}

/// Publishes an `operation.progress` event for each duplicate-detection stage.
struct StageProgressObserver {
    events: EventBus,
    audience: EventAudience,
    operation_id: Option<OperationId>,
}

impl DuplicateObserver for StageProgressObserver {
    fn on_progress(&self, progress: DuplicateProgress) {
        let Some(operation_id) = self.operation_id else {
            return;
        };
        // Only the hashing stages carry meaningful per-file progress; the
        // size-grouping stage does no I/O and completes instantly.
        if progress.stage == DuplicateStage::GroupBySize {
            return;
        }
        self.events.publish(
            self.audience.clone(),
            BackendEventPayload::OperationProgress {
                progress: OperationProgressPayload {
                    operation_id,
                    progress: OperationProgressDetails {
                        completed_items: progress.files_processed as u64,
                        total_items: Some(progress.files_total as u64),
                        completed_bytes: 0,
                        total_bytes: None,
                        current_entry: None,
                        bytes_per_second: None,
                    },
                },
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_duplicate_scan(
    scan_id: Uuid,
    roots: Vec<Location>,
    options: &DuplicateScanOptions,
    cancellation: &CancellationToken,
    store: &DuplicateResultsStore,
    events: &EventBus,
    audience: &EventAudience,
    providers: &ProviderRegistry,
) {
    let candidates = collect_candidates(providers, &roots, options.show_hidden, cancellation).await;

    let observer = StageProgressObserver {
        events: events.clone(),
        audience: audience.clone(),
        operation_id: options.operation_id,
    };
    let scan = find_duplicates_observed(
        providers,
        candidates,
        &options.detection,
        Some(&observer),
        cancellation,
    )
    .await;

    let cancelled = scan.outcome == ScanOutcome::Cancelled;
    let warnings_count = u32::try_from(scan.warnings.len()).unwrap_or(u32::MAX);
    let payload_groups: Vec<DuplicateGroupPayload> =
        scan.groups.iter().map(duplicate_payload).collect();
    store.finish(scan_id, scan.groups, scan.stats, scan.warnings, cancelled);
    events.publish(
        audience.clone(),
        BackendEventPayload::DuplicateResultsReady {
            scan_id,
            groups: payload_groups,
            is_cancelled: cancelled,
            warnings_count,
        },
    );

    if let Some(operation_id) = options.operation_id {
        let state = if cancelled {
            OperationStatePayload::Cancelled
        } else {
            OperationStatePayload::Completed
        };
        events.publish(
            audience.clone(),
            BackendEventPayload::OperationStateChanged {
                operation_id,
                state,
            },
        );
    }
}

fn duplicate_payload(group: &DuplicateGroup) -> DuplicateGroupPayload {
    DuplicateGroupPayload {
        full_hash: group.full_hash.clone(),
        size: group.size,
        hardlink_clusters: group
            .hardlink_clusters
            .iter()
            .map(|cluster| HardlinkClusterPayload {
                device: cluster.identity.device,
                inode: cluster.identity.inode,
                locations: cluster
                    .files
                    .iter()
                    .map(|file| file.entry.location.clone().into())
                    .collect(),
            })
            .collect(),
        distinct_locations: group
            .distinct_files
            .iter()
            .map(|file| file.entry.location.clone().into())
            .collect(),
        reclaimable_bytes: group.reclaimable_bytes(),
    }
}

/// Walks every root, collecting the files a duplicate scan should consider.
///
/// Directories are descended into; symlinks are always leaves and are never
/// followed, which — together with the `visited` set — is what keeps a
/// symlink loop from causing unbounded recursion, exactly as
/// `fm_comparison::engine` and `fm_search::engine` do (spec §6, §35).
async fn collect_candidates(
    providers: &ProviderRegistry,
    roots: &[Location],
    show_hidden: bool,
    cancellation: &CancellationToken,
) -> Vec<DuplicateCandidate> {
    let mut candidates = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut pending: Vec<Location> = roots.to_vec();

    while let Some(location) = pending.pop() {
        if cancellation.is_cancelled() {
            break;
        }
        if !visited.insert(location.uri.clone()) {
            continue;
        }
        let Some(entries) = list_all(providers, &location, cancellation).await else {
            continue;
        };
        for entry in entries {
            if !show_hidden && entry.hidden {
                continue;
            }
            match entry.kind {
                EntryKind::Directory => pending.push(entry.location),
                EntryKind::File => candidates.push(DuplicateCandidate::new(
                    EntryRef {
                        id: entry.id,
                        location: entry.location,
                    },
                    entry.size.unwrap_or(0),
                )),
                // Symlinks are leaves and are never hashed: following them
                // would double-count their target and could escape the roots.
                EntryKind::Symlink => {}
            }
        }
    }
    candidates
}

/// Lists every page of `location`, or `None` if the provider cannot be
/// resolved, lacks `LIST`, or a page request fails.
async fn list_all(
    providers: &ProviderRegistry,
    location: &Location,
    cancellation: &CancellationToken,
) -> Option<Vec<EntrySummary>> {
    let provider = providers.resolve(location).ok()?;
    let capabilities = provider.capabilities_for(location).ok()?;
    if !capabilities.contains(ProviderCapabilities::LIST) {
        return None;
    }
    let mut entries = Vec::new();
    let mut continuation_token = None;
    loop {
        if cancellation.is_cancelled() {
            return Some(entries);
        }
        let page = provider
            .list(
                location,
                ListOptions {
                    page_size: 500,
                    continuation_token,
                },
                cancellation.clone(),
            )
            .await
            .ok()?;
        entries.extend(page.entries);
        continuation_token = page.continuation_token;
        if continuation_token.is_none() {
            return Some(entries);
        }
    }
}
