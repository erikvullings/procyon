//! One-shot host-side semantic indexing orchestration.
//!
//! This capability is deliberately invoked explicitly by a trusted host. It
//! enumerates only an already-enrolled root, retains provider locations in the
//! host catalog, and sends the isolated worker opaque scope metadata plus
//! bounded file bytes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::SeekFrom;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use fm_domain::{EntryKind, EntrySummary, GitFileStatus, Location, LocationError, WorkspaceId};
use fm_events::{BackendEventPayload, EventAudience, EventBus};
use fm_semantic_library::{
    ContentFingerprint, EligibilityCandidate, EligibilityEntryKind, EligibilityReason,
    OccurrenceId, OccurrenceScope, RootId,
};
use fm_vcs_status::{GitIgnoreError, ignored_entries};
use fm_vfs::{
    EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities, ProviderRegistry, VfsError,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::time::{Instant, MissedTickBehavior, interval_at, sleep};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::semantic::{
    DocumentId, DocumentIngestion, LibraryId, PdfStreamIngestion, SemanticError,
    SemanticIngestionState, SemanticOperationId, SemanticScope, SemanticService, TenantId,
};
use crate::semantic_library::{
    SemanticAccessContext, SemanticEligibilityReasonCount, SemanticFeedCandidate,
    SemanticIndexingObservation, SemanticLibraryError, SemanticLibraryService,
    SemanticRootAvailability,
};

const PAGE_SIZE: usize = 128;
const MAX_ENTRIES: u64 = 100_000;
const MAX_DIRECTORIES: u64 = 10_000;
const MAX_RECURSION_DEPTH: u16 = 64;
const MAX_FAILURE_DETAILS: usize = 16;
const MAX_FAILURE_DETAIL_CHARS: usize = 512;
pub(crate) const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const INGESTION_POLL_INTERVAL: Duration = Duration::from_millis(25);
// Ingestion without PDF images has a total deadline. For consented PDF
// images this is an idle deadline, renewed only by durable worker progress.
const INGESTION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Outcome of one complete root enumeration and worker feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIndexingReport {
    /// Worker tenants populated by the reconciliation.
    ///
    /// Desktop search uses each attached workspace UUID as its tenant. A
    /// server library retains its authenticated tenant across workspaces.
    pub tenant_ids: Vec<String>,
    /// Active immutable semantic library identity.
    pub library_id: String,
    /// Reconciled enrolled root.
    pub root_id: RootId,
    /// Regular files admitted and recorded in the host catalog.
    pub observed_files: u64,
    /// Workspace-scoped worker occurrences successfully ingested.
    pub ingested_occurrences: u64,
    /// Workspace-scoped occurrences whose worker ingestion job failed.
    pub failed_occurrences: u64,
    /// Sanitized reasons for worker failures, including page-local PDF limits.
    pub failure_details: Vec<String>,
    /// Additional or overlong failure reasons were not retained in this report.
    pub failure_details_truncated: bool,
    /// Completed PDF jobs with non-fatal image/page evidence omissions.
    pub coverage_details: Vec<String>,
    /// Additional or overlong coverage reasons were not retained.
    pub coverage_details_truncated: bool,
    /// Workspace-scoped occurrences intentionally excluded during conversion.
    pub excluded_occurrences: u64,
    /// Actionable, sanitized reasons for conversion-time exclusions.
    pub exclusion_details: Vec<String>,
    /// Source files whose latest completed reconciliation requires OCR.
    pub ocr_required_files: Vec<Location>,
    /// Entries rejected by the curated eligibility policy.
    pub skipped_reason_counts: Vec<SemanticEligibilityReasonCount>,
    /// Files that could not be read; their previous index is retained.
    pub unreadable_files: u64,
    /// Subdirectories that could not be listed; their previous index is
    /// retained.
    pub unreadable_directories: u64,
    /// Newly committed complete reconciliation generation.
    pub reconciliation_generation: u64,
}

/// Typed failures from explicit one-shot semantic indexing.
#[derive(Debug, Error)]
pub enum SemanticIndexingError {
    /// Durable consent/catalog state rejected the operation.
    #[error(transparent)]
    Library(#[from] SemanticLibraryError),
    /// A provider failed while listing or reading the enrolled root.
    #[error(transparent)]
    Provider(#[from] VfsError),
    /// Local path could not be translated for repository ignore evaluation.
    #[error(transparent)]
    Location(#[from] LocationError),
    /// Repository ignore rules could not be evaluated.
    #[error(transparent)]
    GitIgnore(#[from] GitIgnoreError),
    /// The semantic worker rejected ingestion or job polling.
    #[error(transparent)]
    Semantic(#[from] SemanticError),
    /// The requested root is not enrolled.
    #[error("semantic root is not enrolled")]
    RootNotFound,
    /// The root is enrolled but currently unavailable.
    #[error("semantic root is currently unavailable")]
    RootUnavailable,
    /// Ingestion is paused by the durable semantic-library state.
    #[error("semantic indexing is paused")]
    Paused,
    /// A traversal, read, or completion deadline exceeded its hard bound.
    #[error("semantic indexing limit exceeded: {0}")]
    LimitExceeded(&'static str),
    /// A provider returned an invalid paging response.
    #[error("semantic provider returned an invalid directory page")]
    InvalidProviderPage,
    /// The caller cancelled the reconciliation.
    #[error("semantic indexing was cancelled")]
    Cancelled,
}

/// Deep application capability coordinating VFS, catalog, and worker state.
pub struct SemanticIndexingService {
    providers: ProviderRegistry,
    events: EventBus,
    reconciling: AtomicBool,
    scanned_entries: AtomicU64,
    semantic: std::sync::RwLock<SemanticService>,
    ocr_required_files: std::sync::RwLock<BTreeMap<RootId, Vec<Location>>>,
    run_lock: tokio::sync::Mutex<()>,
}

struct ReconciliationGuard<'a>(&'a AtomicBool);

impl Drop for ReconciliationGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl SemanticIndexingService {
    pub(crate) fn new(providers: ProviderRegistry, events: EventBus) -> Self {
        Self {
            providers,
            events,
            reconciling: AtomicBool::new(false),
            scanned_entries: AtomicU64::new(0),
            semantic: std::sync::RwLock::new(SemanticService::unavailable()),
            ocr_required_files: std::sync::RwLock::new(BTreeMap::new()),
            run_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn ocr_required_files(&self, root_id: RootId) -> Vec<Location> {
        self.ocr_required_files
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&root_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn set_semantic(&self, semantic: SemanticService) {
        *self
            .semantic
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = semantic;
    }

    pub(crate) fn is_reconciling(&self) -> bool {
        self.reconciling.load(Ordering::SeqCst)
    }

    /// Enumerates, feeds, waits for, and commits one enrolled root.
    pub(crate) async fn reconcile(
        &self,
        library: Arc<SemanticLibraryService>,
        access: &SemanticAccessContext,
        root_id: RootId,
        cancellation: CancellationToken,
    ) -> Result<SemanticIndexingReport, SemanticIndexingError> {
        let _run = self.run_lock.lock().await;
        self.reconciling.store(true, Ordering::SeqCst);
        let _active = ReconciliationGuard(&self.reconciling);
        self.scanned_entries.store(0, Ordering::Relaxed);
        let job_id = format!("root:{root_id}");
        self.publish_progress(&job_id, "reconciling", 0, 0);
        let work = self.reconcile_root(library, access, root_id, cancellation);
        tokio::pin!(work);
        let mut heartbeat = interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        );
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let result = loop {
            tokio::select! {
                result = &mut work => break result,
                _ = heartbeat.tick() => self.publish_progress(
                    &job_id,
                    "reconciling",
                    self.scanned_entries.load(Ordering::Relaxed),
                    0,
                ),
            }
        };
        self.publish_progress(
            &job_id,
            match &result {
                Ok(report) if report.failed_occurrences > 0 => "failed",
                Ok(_) => "complete",
                Err(SemanticIndexingError::Cancelled) => "cancelled",
                Err(_) => "failed",
            },
            self.scanned_entries.load(Ordering::Relaxed),
            match &result {
                Ok(report) => report.failed_occurrences,
                Err(_) => 1,
            },
        );
        result
    }

    fn publish_progress(&self, job_id: &str, stage: &str, completed: u64, errors: u64) {
        self.events.publish(
            EventAudience::Global,
            BackendEventPayload::SemanticIngestionProgress {
                job_id: job_id.to_owned(),
                stage: stage.to_owned(),
                completed,
                total: 0,
                errors,
            },
        );
    }

    async fn reconcile_root(
        &self,
        library: Arc<SemanticLibraryService>,
        access: &SemanticAccessContext,
        root_id: RootId,
        cancellation: CancellationToken,
    ) -> Result<SemanticIndexingReport, SemanticIndexingError> {
        check_cancelled(&cancellation)?;

        let (context, provider, semantic) =
            self.resolve_root_context(&library, access, root_id).await?;
        let library_id = context.library_id.clone();

        let mut pending = VecDeque::from([(context.location.clone(), 0_u16)]);
        let mut directory_count = 0_u64;
        let mut entry_count = 0_u64;
        let mut observed = BTreeSet::new();
        let mut observed_files = 0_u64;
        let mut ingested_occurrences = 0_u64;
        let mut failed_occurrences = 0_u64;
        let mut failure_details = BTreeSet::new();
        let mut failure_details_truncated = false;
        let mut coverage_details = BTreeSet::new();
        let mut coverage_details_truncated = false;
        let mut excluded_occurrences = 0_u64;
        let mut exclusion_details = BTreeSet::new();
        let mut ocr_required_files = Vec::new();
        let mut skipped = BTreeMap::<EligibilityReason, u64>::new();
        let mut preserved = Vec::new();
        let mut unreadable_files = 0_u64;
        let mut unreadable_directories = 0_u64;

        while let Some((directory, depth)) = pending.pop_front() {
            check_cancelled(&cancellation)?;
            directory_count = directory_count.saturating_add(1);
            if directory_count > MAX_DIRECTORIES {
                return Err(SemanticIndexingError::LimitExceeded("directory count"));
            }
            let mut continuation_token = None;
            let mut seen_tokens = BTreeSet::new();
            loop {
                let listed = provider
                    .list(
                        &directory,
                        ListOptions {
                            page_size: PAGE_SIZE,
                            continuation_token: continuation_token.clone(),
                        },
                        cancellation.child_token(),
                    )
                    .await;
                let mut page = match listed {
                    Ok(page) => page,
                    Err(error) if depth > 0 && is_entry_local_failure(&error) => {
                        tracing::debug!(%error, "semantic reconciliation skipped an unreadable directory");
                        unreadable_directories = unreadable_directories.saturating_add(1);
                        preserved.push(directory.clone());
                        break;
                    }
                    Err(error) => return Err(error.into()),
                };
                if page.entries.len() > PAGE_SIZE {
                    return Err(SemanticIndexingError::InvalidProviderPage);
                }
                annotate_git_ignored(&directory, &mut page.entries).await?;
                for entry in page.entries {
                    check_cancelled(&cancellation)?;
                    entry_count = entry_count.saturating_add(1);
                    self.scanned_entries.store(entry_count, Ordering::Relaxed);
                    if entry_count > MAX_ENTRIES {
                        return Err(SemanticIndexingError::LimitExceeded("entry count"));
                    }
                    let candidate = eligibility_candidate(entry.clone());
                    let plan = library.worker_feed_plan_with_pdf_limit(
                        access,
                        &[SemanticFeedCandidate { root_id, candidate }],
                        context.max_pdf_source_bytes,
                    )?;
                    for count in plan.skipped_reason_counts {
                        *skipped.entry(count.reason).or_insert(0) += count.count;
                    }
                    if plan.eligible_locations.first() != Some(&entry.location) {
                        continue;
                    }
                    match entry.kind {
                        EntryKind::Directory if context.recursive => {
                            let next_depth = depth.saturating_add(1);
                            if next_depth > MAX_RECURSION_DEPTH {
                                return Err(SemanticIndexingError::LimitExceeded(
                                    "recursion depth",
                                ));
                            }
                            pending.push_back((entry.location, next_depth));
                        }
                        EntryKind::Directory | EntryKind::Symlink => {}
                        EntryKind::File => {
                            match self
                                .ingest_entry(EntryIngestRequest {
                                    library: &library,
                                    access,
                                    context: &context,
                                    provider: provider.as_ref(),
                                    entry: &entry,
                                    semantic: &semantic,
                                    cancellation: &cancellation,
                                })
                                .await?
                            {
                                EntryIngestOutcome::Oversized => {
                                    *skipped.entry(EligibilityReason::Oversized).or_insert(0) += 1;
                                }
                                EntryIngestOutcome::Unreadable(error) => {
                                    tracing::debug!(%error, "semantic reconciliation skipped an unreadable file");
                                    unreadable_files = unreadable_files.saturating_add(1);
                                    preserved.push(entry.location.clone());
                                }
                                EntryIngestOutcome::Ingested(report) => {
                                    observed.insert(report.occurrence_id);
                                    observed_files = observed_files.saturating_add(1);
                                    ingested_occurrences =
                                        ingested_occurrences.saturating_add(report.ingested);
                                    failed_occurrences =
                                        failed_occurrences.saturating_add(report.failed);
                                    for detail in report.failure_details {
                                        retain_failure_detail(
                                            &mut failure_details,
                                            &mut failure_details_truncated,
                                            &detail,
                                        );
                                    }
                                    for detail in report.coverage_details {
                                        retain_failure_detail(
                                            &mut coverage_details,
                                            &mut coverage_details_truncated,
                                            &detail,
                                        );
                                    }
                                    excluded_occurrences =
                                        excluded_occurrences.saturating_add(report.excluded);
                                    for detail in report.exclusion_details {
                                        exclusion_details.insert(detail);
                                    }
                                    if report.requires_ocr {
                                        ocr_required_files.push(entry.location.clone());
                                    }
                                }
                            }
                        }
                    }
                }
                if !page.has_more {
                    break;
                }
                let next = page
                    .continuation_token
                    .ok_or(SemanticIndexingError::InvalidProviderPage)?;
                if !seen_tokens.insert(next.clone()) {
                    return Err(SemanticIndexingError::InvalidProviderPage);
                }
                continuation_token = Some(next);
            }
        }

        check_cancelled(&cancellation)?;
        let reconciliation_generation = library
            .complete_reconciliation_preserving(access, root_id, &observed, &preserved, &skipped)?;
        self.ocr_required_files
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(root_id, ocr_required_files.clone());
        let tenant_ids = match access {
            SemanticAccessContext::Host => context
                .workspace_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            SemanticAccessContext::Server(_) => vec![access.tenant_id()?],
            SemanticAccessContext::Anonymous => {
                return Err(SemanticLibraryError::AccessDenied.into());
            }
        };
        Ok(SemanticIndexingReport {
            tenant_ids,
            library_id,
            root_id,
            observed_files,
            ingested_occurrences,
            failed_occurrences,
            failure_details: failure_details.into_iter().collect(),
            failure_details_truncated,
            coverage_details: coverage_details.into_iter().collect(),
            coverage_details_truncated,
            excluded_occurrences,
            exclusion_details: exclusion_details.into_iter().collect(),
            ocr_required_files,
            skipped_reason_counts: skipped
                .into_iter()
                .map(|(reason, count)| SemanticEligibilityReasonCount { reason, count })
                .collect(),
            unreadable_files,
            unreadable_directories,
            reconciliation_generation,
        })
    }

    /// Resolves the shared per-root ingestion context, provider, and semantic
    /// service, mirroring the checks the full reconciliation performs before
    /// enumeration. Both root reconciliation and single-file OCR remediation
    /// use it so they agree on eligibility scope, budgets, and authority.
    async fn resolve_root_context(
        &self,
        library: &SemanticLibraryService,
        access: &SemanticAccessContext,
        root_id: RootId,
    ) -> Result<
        (
            RootIngestContext,
            Arc<dyn FileSystemProvider>,
            SemanticService,
        ),
        SemanticIndexingError,
    > {
        let status = library.status(access)?;
        if status.paused {
            return Err(SemanticIndexingError::Paused);
        }
        let root = status
            .roots
            .into_iter()
            .find(|candidate| candidate.id == root_id.to_string())
            .ok_or(SemanticIndexingError::RootNotFound)?;
        if root.availability != SemanticRootAvailability::Available {
            return Err(SemanticIndexingError::RootUnavailable);
        }
        let library_id = status
            .library
            .ok_or(SemanticLibraryError::Unavailable)?
            .library_id;
        let profile_source_bytes = status
            .resource_profile
            .ok_or(SemanticLibraryError::Unavailable)?
            .budgets
            .max_source_bytes_per_document;
        let max_source_bytes = profile_source_bytes.min(MAX_SOURCE_BYTES);
        let workspace_ids = root
            .workspace_references
            .iter()
            .copied()
            .map(WorkspaceId::from)
            .collect::<Vec<_>>();
        if workspace_ids.is_empty() {
            return Err(SemanticLibraryError::WorkspaceRequired.into());
        }
        let provider = self.providers.resolve(&root.location)?;
        let capabilities = provider.capabilities_for(&root.location)?;
        capabilities.require(ProviderCapabilities::LIST)?;
        capabilities.require(ProviderCapabilities::READ)?;
        let semantic = self
            .semantic
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        semantic.health().await?;
        let max_pdf_source_bytes = semantic.max_pdf_source_bytes().await?;
        if !(MAX_SOURCE_BYTES..=fm_semantic_protocol::MAX_PDF_INGESTION_BYTES)
            .contains(&max_pdf_source_bytes)
        {
            return Err(SemanticError::InvalidRequest(
                "semantic capability advertised an invalid PDF source limit".into(),
            )
            .into());
        }
        Ok((
            RootIngestContext {
                root_id,
                library_id,
                max_source_bytes,
                max_pdf_source_bytes,
                profile_source_bytes,
                workspace_ids,
                location: root.location,
                recursive: root.recursive,
            },
            provider,
            semantic,
        ))
    }

    /// Records, feeds, and awaits ingestion of one already-eligible file.
    ///
    /// This is the bounded single-file ingestion unit shared by enrolled-root
    /// reconciliation and OCR remediation. It never commits a reconciliation
    /// generation, so remediating specific files cannot remove documents that
    /// were merely absent from a partial scope.
    async fn ingest_entry(
        &self,
        request: EntryIngestRequest<'_>,
    ) -> Result<EntryIngestOutcome, SemanticIndexingError> {
        let EntryIngestRequest {
            library,
            access,
            context,
            provider,
            entry,
            semantic,
            cancellation,
        } = request;
        let media_type = media_type(entry).ok_or(SemanticLibraryError::InvalidRequest)?;
        let source = if media_type == "application/pdf"
            && context.max_pdf_source_bytes > MAX_SOURCE_BYTES
            && entry.size.is_none_or(|size| size > MAX_SOURCE_BYTES)
        {
            read_pdf_snapshot(
                provider,
                entry,
                context
                    .profile_source_bytes
                    .min(context.max_pdf_source_bytes),
                cancellation,
            )
            .await
            .map(|source| source.map(SourceBytes::Pdf))
        } else {
            read_bounded(provider, entry, context.max_source_bytes, cancellation)
                .await
                .map(|source| source.map(SourceBytes::Buffered))
        };
        let mut source = match source {
            Ok(Some(source)) => source,
            Ok(None) => return Ok(EntryIngestOutcome::Oversized),
            Err(SemanticIndexingError::Provider(error)) if is_entry_local_failure(&error) => {
                return Ok(EntryIngestOutcome::Unreadable(error));
            }
            Err(error) => return Err(error),
        };
        let fingerprint = ContentFingerprint::new(source.fingerprint())
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        let occurrence_id = library.record_indexing_observation(
            access,
            SemanticIndexingObservation {
                entry_id: entry.id,
                location: entry.location.clone(),
                content_fingerprint: fingerprint.clone(),
                root_id: context.root_id,
                workspace_ids: context.workspace_ids.clone(),
                source_bytes: source.len(),
            },
        )?;

        let feed = library.worker_feed_plan_with_pdf_limit(
            access,
            &[SemanticFeedCandidate {
                root_id: context.root_id,
                candidate: eligibility_candidate(entry.clone()),
            }],
            context.max_pdf_source_bytes,
        )?;
        let pdf_images_consented = feed.pdf_images_consented;
        let decision = feed
            .decisions
            .into_iter()
            .find(|decision| decision.occurrence_id() == occurrence_id)
            .ok_or(SemanticLibraryError::InvalidRequest)?;
        if decision.library_id().to_string() != context.library_id {
            return Err(SemanticLibraryError::IncompatibleLibraryIdentity.into());
        }
        let mut report = EntryIngestReport {
            occurrence_id,
            ingested: 0,
            failed: 0,
            failure_details: Vec::new(),
            coverage_details: Vec::new(),
            excluded: 0,
            exclusion_details: Vec::new(),
            requires_ocr: false,
        };
        for workspace_id in &context.workspace_ids {
            check_cancelled(cancellation)?;
            let tenant_id = match access {
                SemanticAccessContext::Host => workspace_id.to_string(),
                SemanticAccessContext::Server(_) => access.tenant_id()?,
                SemanticAccessContext::Anonymous => {
                    return Err(SemanticLibraryError::AccessDenied.into());
                }
            };
            if matches!(access, SemanticAccessContext::Server(_))
                && decision.tenant_id().as_str() != tenant_id
            {
                return Err(SemanticLibraryError::AccessDenied.into());
            }
            let content = match &mut source {
                SourceBytes::Buffered(bytes) => FeedContent::Buffered(bytes.clone()),
                SourceBytes::Pdf(snapshot) => {
                    snapshot
                        .file
                        .seek(SeekFrom::Start(0))
                        .await
                        .map_err(|error| {
                            SemanticIndexingError::Provider(VfsError::Io {
                                message: error.to_string(),
                            })
                        })?;
                    FeedContent::Pdf {
                        reader: &mut snapshot.file,
                        content_bytes: snapshot.len,
                        content_sha256: snapshot.digest,
                    }
                }
            };
            match ingest_and_wait(
                semantic,
                &decision,
                FeedDocument {
                    tenant_id,
                    title: entry.name.clone(),
                    root_id: context.root_id,
                    workspace_id: *workspace_id,
                    media_type,
                    pdf_images_consented,
                    modified_at_ms: entry
                        .modified_at
                        .map_or(0, |value| value.timestamp_millis()),
                    content,
                },
                cancellation,
            )
            .await?
            {
                IngestionOutcome::Completed(coverage_detail) => {
                    if let Some(detail) = coverage_detail {
                        report.coverage_details.push(detail);
                    }
                    library.confirm_indexed_observation(
                        access,
                        occurrence_id,
                        &fingerprint,
                        OccurrenceScope::new(*workspace_id, context.root_id),
                    )?;
                    report.ingested = report.ingested.saturating_add(1);
                }
                IngestionOutcome::Failed(detail) => {
                    report.failed = report.failed.saturating_add(1);
                    report.failure_details.push(detail);
                }
                IngestionOutcome::Excluded(detail) => {
                    report.excluded = report.excluded.saturating_add(1);
                    report.requires_ocr |= detail.contains("OCRmyPDF");
                    report.exclusion_details.push(detail);
                }
            }
        }
        Ok(EntryIngestOutcome::Ingested(report))
    }

    /// Returns every source file whose latest reconciliation reported it as
    /// requiring OCR, paired with its enrolled root.
    pub(crate) fn all_ocr_required_files(&self) -> Vec<(RootId, Location)> {
        self.ocr_required_files
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .flat_map(|(root_id, locations)| {
                locations
                    .iter()
                    .map(move |location| (*root_id, location.clone()))
            })
            .collect()
    }

    /// Removes one file from the backend-owned OCR-required report after the
    /// file was successfully re-ingested or ceased to be eligible.
    pub(crate) fn clear_ocr_required_file(&self, root_id: RootId, location: &Location) {
        let mut reported = self
            .ocr_required_files
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(locations) = reported.get_mut(&root_id) {
            locations.retain(|candidate| candidate != location);
            if locations.is_empty() {
                reported.remove(&root_id);
            }
        }
    }

    /// Re-ingests one previously text-less file through the OCR-aware worker.
    ///
    /// The file is re-inspected (so a changed file is remediated as it is now),
    /// re-checked against the curated eligibility policy (so unrelated or newly
    /// excluded files are never OCR'd), and fed to the worker. The worker
    /// performs the OCR, reconversion, and ingestion; this returns only the
    /// bounded ingestion outcome for the file.
    pub(crate) async fn remediate_file(
        &self,
        library: &SemanticLibraryService,
        access: &SemanticAccessContext,
        root_id: RootId,
        location: &Location,
        cancellation: &CancellationToken,
    ) -> Result<SingleFileIngestOutcome, SemanticIndexingError> {
        let _run = self.run_lock.lock().await;
        check_cancelled(cancellation)?;
        let (context, provider, semantic) =
            self.resolve_root_context(library, access, root_id).await?;
        // Re-inspect without following links so a changed or replaced file is
        // remediated exactly as it is now, and a directory or symlink target is
        // never opened as a document.
        let mut entry = provider
            .inspect(
                &EntryRef {
                    id: fm_domain::EntryId::new(),
                    location: location.clone(),
                },
                cancellation.child_token(),
            )
            .await?;
        if entry.kind != EntryKind::File {
            return Ok(SingleFileIngestOutcome::Ineligible);
        }
        let parent = entry
            .location
            .parent()?
            .ok_or(SemanticIndexingError::InvalidProviderPage)?;
        annotate_git_ignored(&parent, std::slice::from_mut(&mut entry)).await?;
        // Only remediate files the curated policy would still admit for this
        // root: an unrelated or newly excluded file must never be OCR'd.
        let plan = library.worker_feed_plan_with_pdf_limit(
            access,
            &[SemanticFeedCandidate {
                root_id,
                candidate: eligibility_candidate(entry.clone()),
            }],
            context.max_pdf_source_bytes,
        )?;
        if plan.eligible_locations.first() != Some(&entry.location) {
            return Ok(SingleFileIngestOutcome::Ineligible);
        }
        match self
            .ingest_entry(EntryIngestRequest {
                library,
                access,
                context: &context,
                provider: provider.as_ref(),
                entry: &entry,
                semantic: &semantic,
                cancellation,
            })
            .await?
        {
            EntryIngestOutcome::Oversized => Ok(SingleFileIngestOutcome::Oversized),
            EntryIngestOutcome::Unreadable(error) => Err(error.into()),
            EntryIngestOutcome::Ingested(report) => Ok(SingleFileIngestOutcome::Ingested(report)),
        }
    }
}

async fn annotate_git_ignored(
    directory: &Location,
    entries: &mut [EntrySummary],
) -> Result<(), SemanticIndexingError> {
    if directory.provider_id.as_str() == "local" {
        let ignored = ignored_entries(&directory.to_native_path()?, entries).await?;
        for (entry, ignored) in entries.iter_mut().zip(ignored) {
            if ignored {
                entry.git_status = Some(GitFileStatus::Ignored);
            }
        }
    }
    Ok(())
}

/// Shared per-root ingestion context resolved once for a reconciliation or a
/// remediation pass.
struct RootIngestContext {
    root_id: RootId,
    library_id: String,
    max_source_bytes: u64,
    max_pdf_source_bytes: u64,
    profile_source_bytes: u64,
    workspace_ids: Vec<WorkspaceId>,
    location: Location,
    recursive: bool,
}

struct EntryIngestRequest<'a> {
    library: &'a SemanticLibraryService,
    access: &'a SemanticAccessContext,
    context: &'a RootIngestContext,
    provider: &'a dyn FileSystemProvider,
    entry: &'a EntrySummary,
    semantic: &'a SemanticService,
    cancellation: &'a CancellationToken,
}

/// Aggregated bounded ingestion result for one observed file.
pub(crate) struct EntryIngestReport {
    pub(crate) occurrence_id: OccurrenceId,
    pub(crate) ingested: u64,
    pub(crate) failed: u64,
    pub(crate) failure_details: Vec<String>,
    pub(crate) coverage_details: Vec<String>,
    pub(crate) excluded: u64,
    pub(crate) exclusion_details: Vec<String>,
    pub(crate) requires_ocr: bool,
}

enum EntryIngestOutcome {
    Oversized,
    Unreadable(VfsError),
    Ingested(EntryIngestReport),
}

/// Outcome of remediating one specific file.
pub(crate) enum SingleFileIngestOutcome {
    /// The file was observed and fed to the worker.
    Ingested(EntryIngestReport),
    /// The file exceeded the per-document source budget.
    Oversized,
    /// The file is no longer a policy-eligible regular file for its root.
    Ineligible,
}

fn eligibility_candidate(entry: EntrySummary) -> EligibilityCandidate {
    let mime_type = media_type(&entry).map(str::to_owned);
    let application_or_package_bundle = entry.extension.as_deref().is_some_and(|extension| {
        matches!(extension.to_ascii_lowercase().as_str(), "app" | "bundle")
    });
    EligibilityCandidate {
        location: entry.location,
        kind: match entry.kind {
            EntryKind::File => EligibilityEntryKind::File,
            EntryKind::Directory => EligibilityEntryKind::Directory,
            EntryKind::Symlink => EligibilityEntryKind::Symlink,
        },
        hidden: entry.hidden,
        system: false,
        application_or_package_bundle,
        git_ignored: entry.git_status == Some(GitFileStatus::Ignored),
        mime_type,
        source_bytes: entry.size.unwrap_or(0),
        estimated_extracted_bytes: 0,
        estimated_vector_bytes: 0,
        // One-shot indexing never follows links, even when a provider could
        // resolve them. A missing target makes the curated policy reject it.
        symlink_target: None,
    }
}

fn media_type(entry: &EntrySummary) -> Option<&'static str> {
    if let Some(media_type) = entry.mime_type.as_deref() {
        return match media_type {
            "application/json" => Some("application/json"),
            "application/pdf" => Some("application/pdf"),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
            }
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
            }
            "application/xml" => Some("application/xml"),
            "image/jpeg" => Some("image/jpeg"),
            "image/png" => Some("image/png"),
            "image/webp" => Some("image/webp"),
            "audio/wav" | "audio/x-wav" => Some("audio/wav"),
            "audio/mpeg" => Some("audio/mpeg"),
            "audio/flac" => Some("audio/flac"),
            "audio/aac" => Some("audio/aac"),
            "audio/mp4" => Some("audio/mp4"),
            "video/mp4" => Some("video/mp4"),
            "video/quicktime" => Some("video/quicktime"),
            "text/css" => Some("text/css"),
            "text/csv" => Some("text/csv"),
            "text/html" => Some("text/html"),
            "text/javascript" => Some("text/javascript"),
            "text/markdown" => Some("text/markdown"),
            "text/plain" => Some("text/plain"),
            "text/xml" => Some("text/xml"),
            _ => None,
        };
    }
    match entry
        .extension
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("txt") => Some("text/plain"),
        Some("md" | "markdown") => Some("text/markdown"),
        Some("csv") => Some("text/csv"),
        Some("json") => Some("application/json"),
        Some("xml") => Some("application/xml"),
        Some("html" | "htm") => Some("text/html"),
        Some("css") => Some("text/css"),
        Some("js" | "mjs" | "cjs") => Some("text/javascript"),
        Some("pdf") => Some("application/pdf"),
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("wav") => Some("audio/wav"),
        Some("mp3") => Some("audio/mpeg"),
        Some("flac") => Some("audio/flac"),
        Some("aac") => Some("audio/aac"),
        Some("m4a") => Some("audio/mp4"),
        Some("mp4" | "m4v") => Some("video/mp4"),
        Some("mov") => Some("video/quicktime"),
        Some("docx") => {
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        }
        Some("xlsx") => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        Some("pptx") => {
            Some("application/vnd.openxmlformats-officedocument.presentationml.presentation")
        }
        _ => None,
    }
}

/// Whether a provider failure concerns only the entry being read or listed.
///
/// Such failures (a cloud placeholder timing out, a locked or vanished file)
/// leave the entry's existing index untouched instead of aborting the whole
/// root. Cancellation and provider-wide failures still abort.
fn is_entry_local_failure(error: &VfsError) -> bool {
    matches!(
        error,
        VfsError::NotFound { .. }
            | VfsError::PermissionDenied { .. }
            | VfsError::Locked { .. }
            | VfsError::NotADirectory { .. }
            | VfsError::IsADirectory { .. }
            | VfsError::LinkCycle { .. }
            | VfsError::UnsafeArchiveEntry
            | VfsError::ArchiveResourceLimit { .. }
            | VfsError::Io { .. }
    )
}

async fn read_bounded(
    provider: &dyn fm_vfs::FileSystemProvider,
    entry: &EntrySummary,
    maximum_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<Option<Vec<u8>>, SemanticIndexingError> {
    if entry.size.is_some_and(|size| size > maximum_bytes) {
        return Ok(None);
    }
    let reader = provider
        .open_read(
            &EntryRef {
                id: entry.id,
                location: entry.location.clone(),
            },
            cancellation.child_token(),
        )
        .await?;
    let mut bytes = Vec::new();
    reader
        .take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Ok(None);
    }
    check_cancelled(cancellation)?;
    Ok(Some(bytes))
}

struct PdfSnapshot {
    file: tokio::fs::File,
    len: u64,
    digest: [u8; 32],
}

enum SourceBytes {
    Buffered(Vec<u8>),
    Pdf(PdfSnapshot),
}

impl SourceBytes {
    fn len(&self) -> u64 {
        match self {
            Self::Buffered(bytes) => bytes.len() as u64,
            Self::Pdf(snapshot) => snapshot.len,
        }
    }

    fn fingerprint(&self) -> String {
        match self {
            Self::Buffered(bytes) => sha256_fingerprint(bytes),
            Self::Pdf(snapshot) => sha256_fingerprint_digest(&snapshot.digest),
        }
    }
}

async fn read_pdf_snapshot(
    provider: &dyn FileSystemProvider,
    entry: &EntrySummary,
    maximum_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<Option<PdfSnapshot>, SemanticIndexingError> {
    if entry.size.is_some_and(|size| size > maximum_bytes) {
        return Ok(None);
    }
    let deadline = Instant::now() + fm_semantic_protocol::STREAM_DEADLINE;
    let source = EntryRef {
        id: entry.id,
        location: entry.location.clone(),
    };
    let mut reader = tokio::select! {
        () = cancellation.cancelled() => return Err(SemanticIndexingError::Cancelled),
        () = tokio::time::sleep_until(deadline) =>
            return Err(SemanticIndexingError::LimitExceeded("PDF source streaming deadline")),
        result = provider.open_read(&source, cancellation.child_token()) => result?,
    };
    let tempfile = tempfile::tempfile().map_err(|error| VfsError::Io {
        message: error.to_string(),
    })?;
    let mut file = tokio::fs::File::from_std(tempfile);
    let mut digest = Sha256::new();
    let mut len = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = tokio::select! {
            () = cancellation.cancelled() => return Err(SemanticIndexingError::Cancelled),
            () = tokio::time::sleep_until(deadline) =>
                return Err(SemanticIndexingError::LimitExceeded("PDF source streaming deadline")),
            result = reader.read(&mut buffer) => result,
        }
        .map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
        if read == 0 {
            break;
        }
        len = len
            .checked_add(read as u64)
            .ok_or(SemanticIndexingError::LimitExceeded("PDF source bytes"))?;
        if len > maximum_bytes {
            return Ok(None);
        }
        tokio::select! {
            () = cancellation.cancelled() => return Err(SemanticIndexingError::Cancelled),
            () = tokio::time::sleep_until(deadline) =>
                return Err(SemanticIndexingError::LimitExceeded("PDF source streaming deadline")),
            result = file.write_all(&buffer[..read]) => result,
        }
        .map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
        digest.update(&buffer[..read]);
    }
    check_cancelled(cancellation)?;
    tokio::time::timeout_at(deadline, file.flush())
        .await
        .map_err(|_| SemanticIndexingError::LimitExceeded("PDF source streaming deadline"))?
        .map_err(|error| VfsError::Io {
            message: error.to_string(),
        })?;
    let digest = digest.finalize().into();
    Ok(Some(PdfSnapshot { file, len, digest }))
}

enum FeedContent<'content> {
    Buffered(Vec<u8>),
    Pdf {
        reader: &'content mut tokio::fs::File,
        content_bytes: u64,
        content_sha256: [u8; 32],
    },
}

struct FeedDocument<'content> {
    tenant_id: String,
    title: String,
    root_id: RootId,
    workspace_id: WorkspaceId,
    media_type: &'content str,
    pdf_images_consented: bool,
    modified_at_ms: i64,
    content: FeedContent<'content>,
}

enum IngestionOutcome {
    Completed(Option<String>),
    Failed(String),
    Excluded(String),
}

struct IngestionDeadline {
    deadline: Instant,
    observed_progress: u64,
}

impl IngestionDeadline {
    fn new(now: Instant) -> Self {
        Self {
            deadline: now + INGESTION_TIMEOUT,
            observed_progress: 0,
        }
    }

    fn expired(&mut self, now: Instant, completed_units: u64, pdf_images_consented: bool) -> bool {
        if pdf_images_consented && completed_units > self.observed_progress {
            self.observed_progress = completed_units;
            self.deadline = now + INGESTION_TIMEOUT;
        }
        now >= self.deadline
    }
}

fn retain_failure_detail(details: &mut BTreeSet<String>, truncated: &mut bool, detail: &str) {
    let sanitized = fm_semantic_conversion::sanitize(detail, 0);
    let bounded = sanitized
        .text
        .chars()
        .take(MAX_FAILURE_DETAIL_CHARS)
        .collect::<String>();
    *truncated |= sanitized.removed > 0 || bounded.len() != sanitized.text.len();
    if details.contains(&bounded) {
        return;
    }
    if details.len() == MAX_FAILURE_DETAILS {
        *truncated = true;
        return;
    }
    details.insert(bounded);
}

async fn ingest_and_wait(
    semantic: &SemanticService,
    decision: &fm_semantic_library::WorkerFeedDecision,
    document: FeedDocument<'_>,
    cancellation: &CancellationToken,
) -> Result<IngestionOutcome, SemanticIndexingError> {
    if document.title.len() > 1024 {
        return Err(SemanticIndexingError::LimitExceeded("title bytes"));
    }
    let operation_id = SemanticOperationId::new(Uuid::new_v4().to_string());
    let occurrence_id = decision.occurrence_id().to_string();
    let workspace = document.workspace_id.to_string();
    let scoped_occurrence_id = format!("{occurrence_id}@{workspace}");
    let scope = SemanticScope::new(
        TenantId::new(document.tenant_id),
        LibraryId::new(decision.library_id().to_string()),
    );
    let mut metadata = BTreeMap::from([
        ("occurrence_id".to_owned(), scoped_occurrence_id),
        ("source_id".to_owned(), occurrence_id),
        ("root_id".to_owned(), document.root_id.to_string()),
        ("workspace_id".to_owned(), workspace),
        (
            "modified_at_ms".to_owned(),
            document.modified_at_ms.to_string(),
        ),
    ]);
    metadata.insert("title".to_owned(), document.title);
    let document_id = DocumentId::new(decision.document_id().to_string());
    let pdf_images_consented =
        document.pdf_images_consented && document.media_type == "application/pdf";
    let job_id = match document.content {
        FeedContent::Buffered(content) => {
            semantic
                .ingest(DocumentIngestion {
                    scope: scope.clone(),
                    operation_id: operation_id.clone(),
                    document_id,
                    metadata,
                    media_type: document.media_type.to_owned(),
                    pdf_images_consented,
                    content,
                })
                .await?
        }
        FeedContent::Pdf {
            reader,
            content_bytes,
            content_sha256,
        } => {
            let ingestion = semantic.ingest_pdf_stream(PdfStreamIngestion {
                scope: scope.clone(),
                operation_id: operation_id.clone(),
                document_id,
                metadata,
                pdf_images_consented,
                content_bytes,
                content_sha256,
                reader,
            });
            tokio::select! {
                () = cancellation.cancelled() => return Err(SemanticIndexingError::Cancelled),
                result = ingestion => result?,
            }
        }
    };
    let mut deadline = IngestionDeadline::new(Instant::now());
    loop {
        let job = tokio::select! {
            () = cancellation.cancelled() => {
                semantic.cancel(operation_id).await?;
                return Err(SemanticIndexingError::Cancelled);
            }
            result = semantic.ingestion_job(scope.clone(), job_id.clone()) => result?,
        };
        match job.state {
            SemanticIngestionState::Completed => {
                return Ok(IngestionOutcome::Completed(job.detail));
            }
            SemanticIngestionState::Failed => {
                return Ok(IngestionOutcome::Failed(job.detail.unwrap_or_else(|| {
                    "semantic ingestion failed without a worker reason".to_owned()
                })));
            }
            SemanticIngestionState::Skipped => {
                return Ok(IngestionOutcome::Excluded(job.detail.unwrap_or_else(
                    || "The document was excluded from semantic indexing.".to_owned(),
                )));
            }
            SemanticIngestionState::Cancelled => {
                return Err(SemanticIndexingError::Cancelled);
            }
            SemanticIngestionState::Pending | SemanticIngestionState::Running => {
                if deadline.expired(Instant::now(), job.completed_units, pdf_images_consented) {
                    semantic.cancel(operation_id).await?;
                    return Ok(IngestionOutcome::Failed(
                        "semantic ingestion exceeded the worker deadline".to_owned(),
                    ));
                }
                tokio::select! {
                    () = cancellation.cancelled() => {
                        semantic.cancel(operation_id).await?;
                        return Err(SemanticIndexingError::Cancelled);
                    }
                    () = sleep(INGESTION_POLL_INTERVAL) => {}
                }
            }
        }
    }
}

fn sha256_fingerprint(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    sha256_fingerprint_digest(&digest.into())
}

fn sha256_fingerprint_digest(digest: &[u8; 32]) -> String {
    let mut value = String::with_capacity(7 + digest.len() * 2);
    value.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing to a string cannot fail");
    }
    value
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), SemanticIndexingError> {
    if cancellation.is_cancelled() {
        Err(SemanticIndexingError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod failure_detail_tests {
    use super::*;

    #[test]
    fn bounds_distinct_worker_diagnostics_and_preserves_utf8() {
        let mut details = BTreeSet::new();
        let mut truncated = false;
        for index in 0..=MAX_FAILURE_DETAILS {
            retain_failure_detail(
                &mut details,
                &mut truncated,
                &format!("page {index} failed"),
            );
        }
        assert_eq!(details.len(), MAX_FAILURE_DETAILS);
        assert!(truncated);
        let long = "€".repeat(MAX_FAILURE_DETAIL_CHARS + 1);
        let mut details = BTreeSet::new();
        let mut truncated = false;
        retain_failure_detail(&mut details, &mut truncated, &long);
        assert_eq!(
            details.iter().next().unwrap().chars().count(),
            MAX_FAILURE_DETAIL_CHARS
        );
        assert!(truncated);
        let mut details = BTreeSet::new();
        let mut truncated = false;
        retain_failure_detail(&mut details, &mut truncated, "page 33\u{202e} image 1");
        assert_eq!(details.iter().next().unwrap(), "page 33 image 1");
        assert!(truncated);
    }

    #[test]
    fn pdf_image_heartbeat_renews_idle_deadline_without_renewing_stalled_work() {
        let start = Instant::now();
        let mut images = IngestionDeadline::new(start);
        let later = start + INGESTION_TIMEOUT - Duration::from_secs(1);
        assert!(!images.expired(later, 2, true));
        assert!(!images.expired(start + INGESTION_TIMEOUT, 2, true));
        assert!(images.expired(later + INGESTION_TIMEOUT, 2, true));

        let mut text = IngestionDeadline::new(start);
        assert!(!text.expired(later, 2, false));
        assert!(text.expired(start + INGESTION_TIMEOUT, 3, false));
    }

    #[test]
    fn three_hundred_fifty_four_page_image_job_outlives_five_minutes_but_stalls() {
        let start = Instant::now();
        let mut deadline = IngestionDeadline::new(start);
        for page in 1..=354 {
            assert!(!deadline.expired(start + Duration::from_secs(page * 28), page, true));
        }
        assert!(!deadline.expired(
            start + Duration::from_secs(354 * 28) + INGESTION_TIMEOUT - Duration::from_millis(1),
            354,
            true
        ));
        assert!(deadline.expired(
            start + Duration::from_secs(354 * 28) + INGESTION_TIMEOUT,
            354,
            true
        ));
    }
}
