//! Cancellable, provider-neutral two-root comparison traversal (spec §16
//! milestone 5, task 0075). Shaped after `fm_search::engine`: results stream
//! into a store in batches rather than being collected into one giant
//! response, so a large tree cannot flood the frontend or blow up backend
//! memory (spec §28).
//!
//! Traversal only ever descends into a directory pair that exists, with the
//! same kind, on both sides; entries that exist on only one side (or whose
//! kind differs) are recorded as a single leaf entry and never descended
//! into. Symbolic links are likewise always leaves. Together these two rules
//! mean a symlink loop cannot cause unbounded recursion without a separate
//! device/inode cycle detector (spec §6, §35; the same discipline
//! `fm_search`, task 0068, already applies for the same reason after tasks
//! 0018/0040 established it) — a defensive `visited` set below still guards
//! against a provider returning a repeated URI through non-symlink means.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fm_domain::{EntryKind, EntrySummary, Location, OperationId};
use fm_events::{
    BackendEventPayload, ComparisonEntryPayload, ComparisonEntrySidePayload,
    ComparisonStatusPayload, EntryKindPayload, EventAudience, EventBus, OperationProgressDetails,
    OperationProgressPayload, OperationStatePayload,
};
use fm_vfs::{EntryRef, ListOptions, ProviderCapabilities, ProviderRegistry};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::model::{
    ComparisonCriteria, ComparisonEntry, ComparisonEntrySide, ComparisonStatus, classify,
};
use crate::path::relative_join;
use crate::store::ComparisonResultsStore;

/// Maximum number of compared entries buffered before a batch is flushed.
const BATCH_SIZE: usize = 500;
/// Maximum time a partial batch is held before being flushed anyway, so a
/// slow comparison still streams progress promptly.
const BATCH_INTERVAL: Duration = Duration::from_millis(100);

/// Errors starting or controlling a comparison.
#[derive(Debug, thiserror::Error)]
pub enum ComparisonError {
    /// A root location cannot be resolved to a registered provider.
    #[error("comparison root cannot be resolved: {0}")]
    InvalidRoot(String),
    /// No comparison is tracked under this id.
    #[error("no comparison is tracked with id {0}")]
    NotFound(Uuid),
}

/// Parameters for a comparison request.
#[derive(Debug, Clone)]
pub struct ComparisonOptions {
    /// How entries are classified.
    pub criteria: ComparisonCriteria,
    /// When false, hidden entries (dotfiles, or the platform hidden
    /// attribute) on either side are excluded from the comparison entirely,
    /// mirroring the pane's "show hidden files" setting.
    pub show_hidden: bool,
    /// When present, the engine emits `operation.*` events so the operation
    /// centre can track and cancel this comparison, matching
    /// `fm_search::SearchOptions::operation_id`.
    pub operation_id: Option<OperationId>,
}

/// Starts and cancels recursive directory comparisons, streaming compared
/// entries over the event bus as they are found.
pub struct ComparisonEngine {
    store: Arc<ComparisonResultsStore>,
    events: EventBus,
    providers: ProviderRegistry,
}

impl ComparisonEngine {
    /// Creates an engine that stores results in `store` and streams batches
    /// over `events`.
    #[must_use]
    pub fn new(
        store: Arc<ComparisonResultsStore>,
        events: EventBus,
        providers: ProviderRegistry,
    ) -> Self {
        Self {
            store,
            events,
            providers,
        }
    }

    /// Starts a new cancellable comparison of `left_root` against
    /// `right_root`, publishing batches to `audience` as entries are found.
    ///
    /// # Errors
    ///
    /// Returns [`ComparisonError::InvalidRoot`] if either root cannot be
    /// resolved to a registered provider.
    pub fn start(
        &self,
        comparison_id: Uuid,
        left_root: Location,
        right_root: Location,
        options: ComparisonOptions,
        audience: EventAudience,
    ) -> Result<(), ComparisonError> {
        self.providers
            .resolve(&left_root)
            .map_err(|error| ComparisonError::InvalidRoot(error.to_string()))?;
        self.providers
            .resolve(&right_root)
            .map_err(|error| ComparisonError::InvalidRoot(error.to_string()))?;

        let cancellation = CancellationToken::new();
        self.store.register(
            comparison_id,
            left_root.clone(),
            right_root.clone(),
            options.criteria,
            cancellation.clone(),
        );

        let store = Arc::clone(&self.store);
        let events = self.events.clone();
        let providers = self.providers.clone();
        tokio::spawn(async move {
            run_comparison(
                comparison_id,
                left_root,
                right_root,
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

    /// Requests prompt cancellation of a running comparison.
    ///
    /// # Errors
    ///
    /// Returns [`ComparisonError::NotFound`] if no comparison is tracked
    /// with this id.
    pub fn cancel(&self, comparison_id: Uuid) -> Result<(), ComparisonError> {
        self.store
            .cancel(comparison_id)
            .ok_or(ComparisonError::NotFound(comparison_id))
    }
}

/// One directory pair still waiting to be listed and merged.
struct PendingDir {
    relative_path: String,
    left: Location,
    right: Location,
}

struct ComparisonContext<'a> {
    comparison_id: Uuid,
    store: &'a ComparisonResultsStore,
    events: &'a EventBus,
    audience: &'a EventAudience,
    operation_id: Option<OperationId>,
}

#[allow(clippy::too_many_arguments)]
async fn run_comparison(
    comparison_id: Uuid,
    left_root: Location,
    right_root: Location,
    options: &ComparisonOptions,
    cancellation: &CancellationToken,
    store: &ComparisonResultsStore,
    events: &EventBus,
    audience: &EventAudience,
    providers: &ProviderRegistry,
) {
    let ctx = ComparisonContext {
        comparison_id,
        store,
        events,
        audience,
        operation_id: options.operation_id,
    };
    let mut buffer: Vec<ComparisonEntry> = Vec::with_capacity(BATCH_SIZE);
    let mut warnings_count = 0_u32;
    let mut compared_count: u64 = 0;
    let mut last_flush = Instant::now();
    // Defends against a provider returning a repeated URI through
    // non-symlink means; symlinks themselves are never enqueued below, so a
    // genuine symlink cycle never reaches this set.
    let mut visited: HashSet<(String, String)> = HashSet::new();

    let mut stack = vec![PendingDir {
        relative_path: String::new(),
        left: left_root,
        right: right_root,
    }];

    while let Some(dir) = stack.pop() {
        if cancellation.is_cancelled() {
            break;
        }
        if !visited.insert((dir.left.uri.clone(), dir.right.uri.clone())) {
            continue;
        }

        let Some(left_children) = list_all(providers, &dir.left, cancellation).await else {
            warnings_count += 1;
            continue;
        };
        let Some(right_children) = list_all(providers, &dir.right, cancellation).await else {
            warnings_count += 1;
            continue;
        };

        let left_by_name = index_by_name(left_children, options.show_hidden);
        let right_by_name = index_by_name(right_children, options.show_hidden);

        let mut names: BTreeSet<&str> = BTreeSet::new();
        names.extend(left_by_name.keys().map(String::as_str));
        names.extend(right_by_name.keys().map(String::as_str));

        for name in names {
            if cancellation.is_cancelled() {
                break;
            }
            let relative_path = relative_join(&dir.relative_path, name);
            let left_entry = left_by_name.get(name);
            let right_entry = right_by_name.get(name);

            match (left_entry, right_entry) {
                (Some(left), None) => {
                    compared_count += 1;
                    buffer.push(ComparisonEntry {
                        relative_path,
                        left: Some(side_from(left, None)),
                        right: None,
                        status: ComparisonStatus::OnlyLeft,
                    });
                }
                (None, Some(right)) => {
                    compared_count += 1;
                    buffer.push(ComparisonEntry {
                        relative_path,
                        left: None,
                        right: Some(side_from(right, None)),
                        status: ComparisonStatus::OnlyRight,
                    });
                }
                (Some(left), Some(right)) => {
                    if left.kind == EntryKind::Directory && right.kind == EntryKind::Directory {
                        stack.push(PendingDir {
                            relative_path: relative_path.clone(),
                            left: left.location.clone(),
                            right: right.location.clone(),
                        });
                        compared_count += 1;
                        buffer.push(ComparisonEntry {
                            relative_path,
                            left: Some(side_from(left, None)),
                            right: Some(side_from(right, None)),
                            status: ComparisonStatus::Identical,
                        });
                        continue;
                    }
                    let (left_hash, right_hash) = if options.criteria
                        == ComparisonCriteria::ContentHash
                        && left.kind == EntryKind::File
                        && right.kind == EntryKind::File
                    {
                        let left_hash = hash_entry(providers, left, cancellation).await;
                        let right_hash = hash_entry(providers, right, cancellation).await;
                        if left_hash.is_none() || right_hash.is_none() {
                            warnings_count += 1;
                        }
                        (left_hash, right_hash)
                    } else {
                        (None, None)
                    };
                    let left_side = side_from(left, left_hash);
                    let right_side = side_from(right, right_hash);
                    let status = classify(&left_side, &right_side, options.criteria);
                    compared_count += 1;
                    buffer.push(ComparisonEntry {
                        relative_path,
                        left: Some(left_side),
                        right: Some(right_side),
                        status,
                    });
                }
                (None, None) => unreachable!("name came from at least one side's map"),
            }

            if buffer.len() >= BATCH_SIZE || last_flush.elapsed() >= BATCH_INTERVAL {
                flush(&ctx, &mut buffer, warnings_count, false, compared_count);
                last_flush = Instant::now();
            }
        }
    }

    flush(&ctx, &mut buffer, warnings_count, true, compared_count);

    if let Some(operation_id) = ctx.operation_id {
        let state = if cancellation.is_cancelled() {
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
    // This loop always drains the whole directory, so a small page size only
    // forces extra round trips - for `LocalFileSystemProvider` each round trip
    // re-scans the directory from scratch (no cross-call cursor state), making
    // this O(n^2) in directory size instead of O(n) for large directories. See
    // the identical fix/comment on `fm-application::directory::list_all`
    // (task 0156).
    const FULL_LISTING_PAGE_SIZE: usize = 65_536;
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
                    page_size: FULL_LISTING_PAGE_SIZE,
                    continuation_token,
                },
                cancellation.clone(),
            )
            .await
            .ok()?;
        entries.extend(page.entries);
        if !page.has_more {
            break;
        }
        continuation_token = page.continuation_token;
    }
    Some(entries)
}

fn index_by_name(
    entries: Vec<EntrySummary>,
    show_hidden: bool,
) -> std::collections::HashMap<String, EntrySummary> {
    entries
        .into_iter()
        .filter(|entry| show_hidden || !entry.hidden)
        .map(|entry| (entry.name.clone(), entry))
        .collect()
}

fn side_from(entry: &EntrySummary, content_hash: Option<String>) -> ComparisonEntrySide {
    ComparisonEntrySide {
        kind: entry.kind,
        size: entry.size,
        modified_at: entry.modified_at,
        content_hash,
    }
}

/// Streams a file's content through SHA-256 without loading it into memory.
///
/// Task 0077 replaced this function's hand-rolled hashing loop with the
/// shared `fm_checksum` implementation, as the implementation note on task
/// 0075 anticipated: one chunked, cancellable streaming hasher now backs both
/// the content-hash comparison mode and the checksum/duplicate features, so
/// the two can never disagree about a digest.
///
/// A `None` result means "no digest available" — the entry could not be
/// opened, failed mid-read, or the comparison was cancelled. Classification
/// treats all three the same way, so they are deliberately not distinguished.
async fn hash_entry(
    providers: &ProviderRegistry,
    entry: &EntrySummary,
    cancellation: &CancellationToken,
) -> Option<String> {
    let entry_ref = EntryRef {
        id: entry.id,
        location: entry.location.clone(),
    };
    fm_checksum::hash_entry(
        providers,
        &entry_ref,
        &[fm_checksum::ChecksumAlgorithm::Sha256],
        cancellation,
    )
    .await
    .ok()?
    .get(fm_checksum::ChecksumAlgorithm::Sha256)
    .map(str::to_owned)
}

fn flush(
    ctx: &ComparisonContext<'_>,
    buffer: &mut Vec<ComparisonEntry>,
    warnings_count: u32,
    is_complete: bool,
    compared_count: u64,
) {
    if !buffer.is_empty() || is_complete {
        let batch = std::mem::take(buffer);
        let payload_entries: Vec<ComparisonEntryPayload> =
            batch.iter().map(entry_payload).collect();
        ctx.store.append(ctx.comparison_id, batch, warnings_count);
        // Mark the store complete before publishing, matching
        // `fm_search::engine::flush`'s ordering rationale: a subscriber that
        // reacts to `is_complete: true` by immediately paging the store must
        // never observe stale `has_more: true` from a completion race.
        if is_complete {
            ctx.store.mark_complete(ctx.comparison_id);
        }
        ctx.events.publish(
            ctx.audience.clone(),
            BackendEventPayload::ComparisonResultsBatch {
                comparison_id: ctx.comparison_id,
                entries: payload_entries,
                is_complete,
                warnings_count,
            },
        );
    }

    if let Some(operation_id) = ctx.operation_id {
        ctx.events.publish(
            ctx.audience.clone(),
            BackendEventPayload::OperationProgress {
                progress: OperationProgressPayload {
                    operation_id,
                    progress: OperationProgressDetails {
                        completed_items: compared_count,
                        total_items: None,
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

fn entry_payload(entry: &ComparisonEntry) -> ComparisonEntryPayload {
    ComparisonEntryPayload {
        relative_path: entry.relative_path.clone(),
        left: entry.left.as_ref().map(side_payload),
        right: entry.right.as_ref().map(side_payload),
        status: status_payload(entry.status),
    }
}

fn side_payload(side: &ComparisonEntrySide) -> ComparisonEntrySidePayload {
    ComparisonEntrySidePayload {
        kind: match side.kind {
            EntryKind::File => EntryKindPayload::File,
            EntryKind::Directory => EntryKindPayload::Directory,
            EntryKind::Symlink => EntryKindPayload::Symlink,
        },
        size: side.size,
        modified_at: side.modified_at,
        content_hash: side.content_hash.clone(),
    }
}

const fn status_payload(status: ComparisonStatus) -> ComparisonStatusPayload {
    match status {
        ComparisonStatus::OnlyLeft => ComparisonStatusPayload::OnlyLeft,
        ComparisonStatus::OnlyRight => ComparisonStatusPayload::OnlyRight,
        ComparisonStatus::Newer => ComparisonStatusPayload::Newer,
        ComparisonStatus::Older => ComparisonStatusPayload::Older,
        ComparisonStatus::DifferentSize => ComparisonStatusPayload::DifferentSize,
        ComparisonStatus::Identical => ComparisonStatusPayload::Identical,
        ComparisonStatus::TypeMismatch => ComparisonStatusPayload::TypeMismatch,
    }
}
