//! One-shot host-side semantic indexing orchestration.
//!
//! This capability is deliberately invoked explicitly by a trusted host. It
//! enumerates only an already-enrolled root, retains provider locations in the
//! host catalog, and sends the isolated worker opaque scope metadata plus
//! bounded file bytes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use fm_domain::{EntryKind, EntrySummary, GitFileStatus, Location, WorkspaceId};
use fm_semantic_library::{
    ContentFingerprint, EligibilityCandidate, EligibilityEntryKind, EligibilityReason,
    OccurrenceId, RootId,
};
use fm_vfs::{
    EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities, ProviderRegistry, VfsError,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::time::{Instant, sleep};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::semantic::{
    DocumentId, DocumentIngestion, LibraryId, SemanticError, SemanticIngestionState,
    SemanticOperationId, SemanticScope, SemanticService, TenantId,
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
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const INGESTION_POLL_INTERVAL: Duration = Duration::from_millis(25);
// Covers conversion, chunking, local CPU embedding, and publication. Durable
// embedding checkpoints make retries resumable; expiry isolates one stranded
// document instead of aborting the enrolled-root reconciliation.
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
    /// Workspace-scoped occurrences intentionally excluded during conversion.
    pub excluded_occurrences: u64,
    /// Actionable, sanitized reasons for conversion-time exclusions.
    pub exclusion_details: Vec<String>,
    /// Source files whose latest completed reconciliation requires OCR.
    pub ocr_required_files: Vec<Location>,
    /// Entries rejected by the curated eligibility policy.
    pub skipped_reason_counts: Vec<SemanticEligibilityReasonCount>,
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
    semantic: std::sync::RwLock<SemanticService>,
    ocr_required_files: std::sync::RwLock<BTreeMap<RootId, Vec<Location>>>,
    run_lock: tokio::sync::Mutex<()>,
}

impl SemanticIndexingService {
    pub(crate) fn new(providers: ProviderRegistry) -> Self {
        Self {
            providers,
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

    /// Enumerates, feeds, waits for, and commits one enrolled root.
    pub(crate) async fn reconcile(
        &self,
        library: Arc<SemanticLibraryService>,
        access: &SemanticAccessContext,
        root_id: RootId,
        cancellation: CancellationToken,
    ) -> Result<SemanticIndexingReport, SemanticIndexingError> {
        let _run = self.run_lock.lock().await;
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
        let mut excluded_occurrences = 0_u64;
        let mut exclusion_details = BTreeSet::new();
        let mut ocr_required_files = Vec::new();
        let mut skipped = BTreeMap::<EligibilityReason, u64>::new();

        while let Some((directory, depth)) = pending.pop_front() {
            check_cancelled(&cancellation)?;
            directory_count = directory_count.saturating_add(1);
            if directory_count > MAX_DIRECTORIES {
                return Err(SemanticIndexingError::LimitExceeded("directory count"));
            }
            let mut continuation_token = None;
            let mut seen_tokens = BTreeSet::new();
            loop {
                let page = provider
                    .list(
                        &directory,
                        ListOptions {
                            page_size: PAGE_SIZE,
                            continuation_token: continuation_token.clone(),
                        },
                        cancellation.child_token(),
                    )
                    .await?;
                if page.entries.len() > PAGE_SIZE {
                    return Err(SemanticIndexingError::InvalidProviderPage);
                }
                for entry in page.entries {
                    check_cancelled(&cancellation)?;
                    entry_count = entry_count.saturating_add(1);
                    if entry_count > MAX_ENTRIES {
                        return Err(SemanticIndexingError::LimitExceeded("entry count"));
                    }
                    let candidate = eligibility_candidate(entry.clone());
                    let plan = library.worker_feed_plan(
                        access,
                        &[SemanticFeedCandidate { root_id, candidate }],
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
                                EntryIngestOutcome::Ingested(report) => {
                                    observed.insert(report.occurrence_id);
                                    observed_files = observed_files.saturating_add(1);
                                    ingested_occurrences =
                                        ingested_occurrences.saturating_add(report.ingested);
                                    failed_occurrences =
                                        failed_occurrences.saturating_add(report.failed);
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
        let reconciliation_generation =
            library.complete_reconciliation(access, root_id, &observed)?;
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
            excluded_occurrences,
            exclusion_details: exclusion_details.into_iter().collect(),
            ocr_required_files,
            skipped_reason_counts: skipped
                .into_iter()
                .map(|(reason, count)| SemanticEligibilityReasonCount { reason, count })
                .collect(),
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
        let max_source_bytes = status
            .resource_profile
            .ok_or(SemanticLibraryError::Unavailable)?
            .budgets
            .max_source_bytes_per_document
            .min(MAX_SOURCE_BYTES);
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
        Ok((
            RootIngestContext {
                root_id,
                library_id,
                max_source_bytes,
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
        let Some(bytes) =
            read_bounded(provider, entry, context.max_source_bytes, cancellation).await?
        else {
            return Ok(EntryIngestOutcome::Oversized);
        };
        let fingerprint = ContentFingerprint::new(sha256_fingerprint(&bytes))
            .map_err(|_| SemanticLibraryError::InvalidRequest)?;
        let occurrence_id = library.record_indexing_observation(
            access,
            SemanticIndexingObservation {
                entry_id: entry.id,
                location: entry.location.clone(),
                content_fingerprint: fingerprint,
                root_id: context.root_id,
                workspace_ids: context.workspace_ids.clone(),
                source_bytes: u64::try_from(bytes.len())
                    .map_err(|_| SemanticIndexingError::LimitExceeded("source bytes"))?,
            },
        )?;

        let feed = library.worker_feed_plan(
            access,
            &[SemanticFeedCandidate {
                root_id: context.root_id,
                candidate: eligibility_candidate(entry.clone()),
            }],
        )?;
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
            excluded: 0,
            exclusion_details: Vec::new(),
            requires_ocr: false,
        };
        for workspace_id in &context.workspace_ids {
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
            match ingest_and_wait(
                semantic,
                &decision,
                FeedDocument {
                    tenant_id,
                    root_id: context.root_id,
                    workspace_id: *workspace_id,
                    media_type: media_type(entry).ok_or(SemanticLibraryError::InvalidRequest)?,
                    modified_at_ms: entry
                        .modified_at
                        .map_or(0, |value| value.timestamp_millis()),
                    content: bytes.clone(),
                },
                cancellation,
            )
            .await?
            {
                IngestionOutcome::Completed => report.ingested = report.ingested.saturating_add(1),
                IngestionOutcome::Failed => report.failed = report.failed.saturating_add(1),
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
        let entry = provider
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
        // Only remediate files the curated policy would still admit for this
        // root: an unrelated or newly excluded file must never be OCR'd.
        let plan = library.worker_feed_plan(
            access,
            &[SemanticFeedCandidate {
                root_id,
                candidate: eligibility_candidate(entry.clone()),
            }],
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
            EntryIngestOutcome::Ingested(report) => Ok(SingleFileIngestOutcome::Ingested(report)),
        }
    }
}

/// Shared per-root ingestion context resolved once for a reconciliation or a
/// remediation pass.
struct RootIngestContext {
    root_id: RootId,
    library_id: String,
    max_source_bytes: u64,
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
    pub(crate) excluded: u64,
    pub(crate) exclusion_details: Vec<String>,
    pub(crate) requires_ocr: bool,
}

enum EntryIngestOutcome {
    Oversized,
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

struct FeedDocument<'content> {
    tenant_id: String,
    root_id: RootId,
    workspace_id: WorkspaceId,
    media_type: &'content str,
    modified_at_ms: i64,
    content: Vec<u8>,
}

enum IngestionOutcome {
    Completed,
    Failed,
    Excluded(String),
}

async fn ingest_and_wait(
    semantic: &SemanticService,
    decision: &fm_semantic_library::WorkerFeedDecision,
    document: FeedDocument<'_>,
    cancellation: &CancellationToken,
) -> Result<IngestionOutcome, SemanticIndexingError> {
    let operation_id = SemanticOperationId::new(Uuid::new_v4().to_string());
    let occurrence_id = decision.occurrence_id().to_string();
    let workspace = document.workspace_id.to_string();
    let scoped_occurrence_id = format!("{occurrence_id}@{workspace}");
    let scope = SemanticScope::new(
        TenantId::new(document.tenant_id),
        LibraryId::new(decision.library_id().to_string()),
    );
    let metadata = BTreeMap::from([
        ("occurrence_id".to_owned(), scoped_occurrence_id),
        ("source_id".to_owned(), occurrence_id),
        ("root_id".to_owned(), document.root_id.to_string()),
        ("workspace_id".to_owned(), workspace),
        (
            "modified_at_ms".to_owned(),
            document.modified_at_ms.to_string(),
        ),
    ]);
    let job_id = semantic
        .ingest(DocumentIngestion {
            scope: scope.clone(),
            operation_id: operation_id.clone(),
            document_id: DocumentId::new(decision.document_id().to_string()),
            metadata,
            media_type: document.media_type.to_owned(),
            content: document.content,
        })
        .await?;
    let deadline = Instant::now() + INGESTION_TIMEOUT;
    loop {
        if Instant::now() >= deadline {
            semantic.cancel(operation_id).await?;
            return Ok(IngestionOutcome::Failed);
        }
        let job = tokio::select! {
            () = cancellation.cancelled() => {
                semantic.cancel(operation_id).await?;
                return Err(SemanticIndexingError::Cancelled);
            }
            result = semantic.ingestion_job(scope.clone(), job_id.clone()) => result?,
        };
        match job.state {
            SemanticIngestionState::Completed => return Ok(IngestionOutcome::Completed),
            SemanticIngestionState::Failed => return Ok(IngestionOutcome::Failed),
            SemanticIngestionState::Skipped => {
                return Ok(IngestionOutcome::Excluded(job.detail.unwrap_or_else(
                    || "The document was excluded from semantic indexing.".to_owned(),
                )));
            }
            SemanticIngestionState::Cancelled => {
                return Err(SemanticIndexingError::Cancelled);
            }
            SemanticIngestionState::Pending | SemanticIngestionState::Running => {
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
