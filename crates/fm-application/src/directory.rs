//! Authoritative directory listing state shared by every transport.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;

use fm_domain::{DirectorySnapshot, EntryId, EntryKind, EntryMetadata, LoadingState, PaneId};
use fm_events::{
    BackendEventPayload, DirectoryDeltaPayload, EntrySummaryPayload, EventAudience, EventBus,
};
use fm_transport_dto::{
    EntryMetadataRequest, ListDirectoryRequest, NavigateRequest, SortDescriptorDto,
    SortDirectionDto,
};
use fm_vcs_status::GitStatusService;
use fm_vfs::{
    ChangeTracking, EntryRef, FileSystemProvider, ListOptions, ProviderChange,
    ProviderChangeStream, ProviderRegistry, VfsError,
};
use futures::{StreamExt, stream};
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApplicationError;

/// Cap on how many matching commits `DirectoryService::git_history` returns
/// for the Alt+Space metadata panel's history section (task 0135) - a long
/// scrolling list stops being useful well before this.
const GIT_HISTORY_RESULT_LIMIT: usize = 50;

/// Cap on how many commits back from `HEAD` `DirectoryService::git_history`
/// scans looking for matches, so a huge, mostly-unrelated history never
/// turns one Alt+Space press into an unbounded walk.
const GIT_HISTORY_SCAN_LIMIT: usize = 2000;

/// A pane not currently in the foreground polls a poll-tracked location this
/// many times less often than an active one (task 0109). Native/delta-API
/// watches are push-based and unaffected by pane activity.
const BACKGROUND_POLL_MULTIPLIER: u32 = 4;

/// Ceiling on how far a run of consecutive poll failures can stretch the
/// interval, so a still-broken connection is still retried at least this
/// often rather than backing off forever.
const MAX_POLL_BACKOFF_MULTIPLIER: u32 = 8;

/// Maximum entries returned per `list()` response. The full directory is always enumerated and
/// sorted first (see `list()` docs); this only bounds response/DOM size for very large
/// directories, it never affects sort correctness across pages.
const LIST_PAGE_SIZE: usize = 256;

struct PaneRequest {
    request_id: Uuid,
    workspace_id: fm_domain::WorkspaceId,
    cancellation: CancellationToken,
    watch_cancellation: CancellationToken,
    revision: u64,
    show_hidden: bool,
    folders_first: bool,
    show_git_status: bool,
    sort: Vec<SortDescriptorDto>,
    snapshot: Option<DirectorySnapshot>,
    /// The complete, filtered, globally-sorted listing for the pane's current directory,
    /// computed once (via [`list_all`]) and then sliced per page — never per-provider-page
    /// sorted, since provider enumeration order is arbitrary (spec: see `list()` docs).
    full_entries: Option<Arc<Vec<fm_domain::EntrySummary>>>,
}

struct SharedWatch {
    sender: broadcast::Sender<ProviderChange>,
    cancellation: CancellationToken,
    references: usize,
    /// Shared by every pane watching this location (task 0109): the most
    /// recent `set_active` call from any of them wins. Only meaningful for
    /// [`ChangeTracking::Poll`]; ignored by native/delta-API watches, which
    /// are push-based and have no cadence to throttle.
    active: Arc<AtomicBool>,
}

#[derive(Default)]
struct WatchHub {
    watches: Mutex<HashMap<fm_domain::Location, SharedWatch>>,
}

/// Lists directories and owns per-pane cancellation and revision state.
#[derive(Clone)]
pub struct DirectoryService {
    providers: ProviderRegistry,
    panes: Arc<Mutex<HashMap<PaneId, PaneRequest>>>,
    watches: Arc<WatchHub>,
    events: EventBus,
    git_status: Arc<GitStatusService>,
}

impl DirectoryService {
    /// Creates a directory service backed by the given provider registry.
    #[must_use]
    pub fn new(providers: ProviderRegistry) -> Self {
        Self::with_event_bus(providers, EventBus::default())
    }

    /// Creates a directory service publishing changes through `events`.
    #[must_use]
    pub fn with_event_bus(providers: ProviderRegistry, events: EventBus) -> Self {
        Self {
            providers,
            panes: Arc::new(Mutex::new(HashMap::new())),
            watches: Arc::new(WatchHub::default()),
            events,
            git_status: Arc::new(GitStatusService::new()),
        }
    }

    /// Lists one page of a directory, publishing it only if it is still the pane's newest
    /// request.
    ///
    /// The entire directory is enumerated and globally sorted once per navigation (mirroring
    /// [`list_all`], used by the watch-triggered refresh path), cached on the pane, and then
    /// sliced into bounded [`LIST_PAGE_SIZE`] pages for the wire — sorting only cannot be done
    /// per provider page, since providers enumerate in arbitrary (e.g. filesystem/inode) order:
    /// that previously surfaced as an initial listing showing only a filesystem-order prefix of
    /// entries, with more (and out-of-order) entries appearing as the pane scrolled. Slicing an
    /// already-fully-sorted cached list keeps that fixed while still bounding per-response size
    /// for very large directories; later pages reuse the cached list rather than re-enumerating
    /// the provider.
    pub async fn list(
        &self,
        request: ListDirectoryRequest,
    ) -> Result<DirectorySnapshot, ApplicationError> {
        let pane_id = PaneId::from(request.pane_id);
        let location: fm_domain::Location = request.location.clone().into();
        let first_page = request.continuation_token.is_none();
        let cancellation = CancellationToken::new();
        let cached_full_entries = {
            let mut panes = self.panes.lock().await;
            let revision = panes.get(&pane_id).map_or(0, |state| state.revision);
            let previous = panes.remove(&pane_id);
            let continuing_same_listing = !first_page
                && previous.as_ref().is_some_and(|state| {
                    state
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.location == location)
                        && state.show_hidden == request.show_hidden
                        && state.folders_first == request.folders_first
                        && state.show_git_status == request.show_git_status
                        && state.sort == request.sort
                });
            let (watch_cancellation, snapshot, full_entries) = if continuing_same_listing {
                let previous = previous.as_ref().expect("checked above");
                (
                    previous.watch_cancellation.clone(),
                    previous.snapshot.clone(),
                    previous.full_entries.clone(),
                )
            } else {
                (CancellationToken::new(), None, None)
            };
            panes.insert(
                pane_id,
                PaneRequest {
                    request_id: request.request_id,
                    workspace_id: request.workspace_id.into(),
                    cancellation: cancellation.clone(),
                    watch_cancellation,
                    revision,
                    show_hidden: request.show_hidden,
                    folders_first: request.folders_first,
                    show_git_status: request.show_git_status,
                    sort: request.sort.clone(),
                    snapshot,
                    full_entries,
                },
            );
            if let Some(previous) = previous {
                previous.cancellation.cancel();
                if !continuing_same_listing {
                    previous.watch_cancellation.cancel();
                }
            }
            panes
                .get(&pane_id)
                .and_then(|state| state.full_entries.clone())
        };

        let provider = self.providers.resolve(&location)?;
        let full_entries = match cached_full_entries {
            Some(cached) => cached,
            None => {
                let mut entries =
                    list_all(provider.clone(), &location, cancellation.clone()).await?;
                annotate_git_status(
                    &self.git_status,
                    &location,
                    &mut entries,
                    false,
                    request.show_git_status,
                )
                .await;
                if !request.show_hidden {
                    entries.retain(|entry| !entry.hidden);
                }
                sort_entries(&mut entries, &request.sort, request.folders_first);
                Arc::new(entries)
            }
        };
        let offset = decode_page_offset(request.continuation_token.as_deref())?;
        if offset > full_entries.len() {
            return Err(ApplicationError::InvalidRequest(
                "continuation token is out of range".to_owned(),
            ));
        }
        let end = full_entries.len().min(offset + LIST_PAGE_SIZE);
        let has_more = end < full_entries.len();

        let mut panes = self.panes.lock().await;
        let state = panes
            .get_mut(&pane_id)
            .filter(|state| state.request_id == request.request_id)
            .ok_or(ApplicationError::OperationCancelled)?;
        state.revision += 1;
        state.full_entries = Some(Arc::clone(&full_entries));

        let writable = provider
            .inspect(
                &EntryRef {
                    id: EntryId::new(),
                    location: location.clone(),
                },
                cancellation.clone(),
            )
            .await
            .map(|entry| !entry.read_only)
            .unwrap_or(false);

        let (total_known_size, total_known_file_count) = aggregate_totals(&full_entries);
        let snapshot = DirectorySnapshot {
            pane_id,
            request_id: request.request_id,
            revision: state.revision,
            location: location.clone(),
            writable,
            entries: full_entries[offset..end].to_vec(),
            total_known_entries: Some(full_entries.len() as u64),
            total_known_size: Some(total_known_size),
            total_known_file_count: Some(total_known_file_count),
            has_more,
            continuation_token: has_more.then(|| end.to_string()),
            loading_state: LoadingState::Loaded,
        };
        state.snapshot = Some(snapshot.clone());
        let watch_cancellation = state.watch_cancellation.clone();
        drop(panes);

        if first_page && provider.change_tracking() != ChangeTracking::Unsupported {
            // Spawned rather than awaited inline: acquiring/registering a
            // filesystem watch (e.g. FSEvents on macOS) is unrelated to the
            // listing already computed above and must never delay returning
            // it to the caller. Measured under load, FSEvents setup alone
            // took 20+ seconds regardless of directory size (task 0156) -
            // awaiting it here meant every first-time navigation to a new
            // location paid that cost before the user saw any content.
            // Spawning also fixes a latent correctness bug: `?` on a failed
            // `acquire()` used to fail the whole listing even though a
            // perfectly good snapshot had already been built - a watch
            // failure (e.g. an OS resource limit) should only mean "no live
            // updates for this pane," never "no listing at all."
            let watches = Arc::clone(&self.watches);
            let panes = Arc::clone(&self.panes);
            let events = self.events.clone();
            let git_status = Arc::clone(&self.git_status);
            let watch_location = snapshot.location.clone();
            let workspace_id = request.workspace_id.into();
            let show_hidden = request.show_hidden;
            let folders_first = request.folders_first;
            let show_git_status = request.show_git_status;
            let sort = request.sort;
            tokio::spawn(async move {
                if let Ok(receiver) = watches
                    .acquire(provider.clone(), watch_location.clone())
                    .await
                {
                    spawn_pane_watch(PaneWatch {
                        provider,
                        location: watch_location,
                        workspace_id,
                        pane_id,
                        show_hidden,
                        folders_first,
                        show_git_status,
                        sort,
                        cancellation: watch_cancellation,
                        receiver,
                        panes,
                        watches,
                        events,
                        git_status,
                    });
                }
            });
        }

        Ok(snapshot)
    }

    /// Navigates a pane to a location, cancelling any older pane request.
    ///
    /// The view options (`sort`/`show_hidden`/`folders_first`/`show_git_status`) are carried over
    /// from the navigating tab's current view (the caller is expected to
    /// populate them from its own state) so that pushing a new location -
    /// e.g. via a favourite, breadcrumb, or opening a subfolder - doesn't
    /// silently reset the tab back to default view settings.
    pub async fn navigate(
        &self,
        request: NavigateRequest,
    ) -> Result<DirectorySnapshot, ApplicationError> {
        self.list(ListDirectoryRequest {
            workspace_id: request.workspace_id,
            pane_id: request.pane_id,
            request_id: request.request_id,
            location: request.location,
            continuation_token: None,
            sort: request.sort,
            show_hidden: request.show_hidden,
            folders_first: request.folders_first,
            show_git_status: request.show_git_status,
        })
        .await
    }

    /// Refreshes a pane using the same listing options and cancellation semantics.
    pub async fn refresh(
        &self,
        request: ListDirectoryRequest,
    ) -> Result<DirectorySnapshot, ApplicationError> {
        self.list(request).await
    }

    /// Re-lists every open pane whose directory was affected by an operation.
    ///
    /// Operation engines do not depend on provider watch support, so this emits
    /// a reset delta explicitly after each terminal operation as well as the
    /// normal provider-originated deltas.
    pub async fn refresh_affected(&self, locations: &HashSet<fm_domain::Location>) {
        let refreshes = {
            let panes = self.panes.lock().await;
            panes
                .iter()
                .filter_map(|(pane_id, state)| {
                    let snapshot = state.snapshot.as_ref()?;
                    if !locations.contains(&snapshot.location) {
                        return None;
                    }
                    Some(PaneWatch {
                        provider: self.providers.resolve(&snapshot.location).ok()?,
                        location: snapshot.location.clone(),
                        workspace_id: state.workspace_id,
                        pane_id: *pane_id,
                        show_hidden: state.show_hidden,
                        folders_first: state.folders_first,
                        show_git_status: state.show_git_status,
                        sort: state.sort.clone(),
                        cancellation: state.cancellation.clone(),
                        receiver: broadcast::channel(1).1,
                        panes: Arc::clone(&self.panes),
                        watches: Arc::clone(&self.watches),
                        events: self.events.clone(),
                        git_status: Arc::clone(&self.git_status),
                    })
                })
                .collect::<Vec<_>>()
        };
        for refresh in refreshes {
            let mut entries = match list_all(
                Arc::clone(&refresh.provider),
                &refresh.location,
                refresh.cancellation.clone(),
            )
            .await
            {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            annotate_git_status(
                &self.git_status,
                &refresh.location,
                &mut entries,
                true,
                refresh.show_git_status,
            )
            .await;
            publish_changes(&refresh, ProviderChange::ResetRequired, entries).await;
        }
    }

    /// Fetches detailed metadata from the provider that owns the entry.
    pub async fn metadata(
        &self,
        request: EntryMetadataRequest,
    ) -> Result<EntryMetadata, ApplicationError> {
        let location = request.location.into();
        let provider = self.providers.resolve(&location)?;
        provider
            .metadata(
                &EntryRef {
                    id: request.entry_id.into(),
                    location,
                },
                CancellationToken::new(),
            )
            .await
            .map_err(Into::into)
    }

    /// Lists immediate child directories of `location`, for the directory-tree sidebar (task
    /// 0139). Unlike [`list`](Self::list), this is not bound to a pane: it keeps no
    /// cancellation/revision/watch state in `self.panes`, so expanding a tree node can never
    /// race with or cancel a pane's own in-flight listing for the same location (see
    /// `list_children_does_not_disturb_a_pane_s_own_in_flight_listing` in
    /// `fm-application/tests/directory_tree.rs`).
    pub async fn list_children(
        &self,
        location: &fm_domain::Location,
        show_hidden: bool,
    ) -> Result<Vec<fm_domain::EntrySummary>, ApplicationError> {
        let provider = self.providers.resolve(location)?;
        let mut entries = list_all(provider, location, CancellationToken::new()).await?;
        entries
            .retain(|entry| entry.kind == EntryKind::Directory && (show_hidden || !entry.hidden));
        entries.sort_by_key(|entry| entry.name.to_lowercase());
        Ok(entries)
    }

    /// Fetches a file's git commit history for the Alt+Space metadata panel's history section
    /// (task 0135). Local provider only; returns an empty list for non-local providers, files
    /// outside a git working tree, or files with no commits yet - never an error, since "no
    /// history to show" is a normal, expected outcome, not a failure.
    #[must_use]
    pub async fn git_history(&self, location: &fm_domain::Location) -> Vec<fm_domain::GitLogEntry> {
        if location.provider_id.as_str() != "local" {
            return Vec::new();
        }
        let Ok(path) = location.to_native_path() else {
            return Vec::new();
        };
        self.git_status
            .file_history(&path, GIT_HISTORY_RESULT_LIMIT, GIT_HISTORY_SCAN_LIMIT)
            .await
    }

    /// Marks whether a pane is currently in the foreground, so a
    /// poll-tracked watch on its directory (SFTP, FTP, ...) can poll less
    /// often while backgrounded (task 0109).
    ///
    /// A no-op for a pane with no watch registered at all — e.g. its
    /// provider has no change tracking, or its listing hasn't finished yet
    /// — since there is nothing to throttle; native/delta-API watches are
    /// push-based and unaffected either way. Only an unknown pane id is
    /// rejected.
    pub async fn set_pane_activity(
        &self,
        pane_id: PaneId,
        active: bool,
    ) -> Result<(), ApplicationError> {
        let location = {
            let panes = self.panes.lock().await;
            let state = panes.get(&pane_id).ok_or(ApplicationError::NotFound)?;
            state
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.location.clone())
        };
        if let Some(location) = location {
            self.watches.set_active(&location, active).await;
        }
        Ok(())
    }
}

impl WatchHub {
    async fn acquire(
        &self,
        provider: Arc<dyn FileSystemProvider>,
        location: fm_domain::Location,
    ) -> Result<broadcast::Receiver<ProviderChange>, VfsError> {
        let mut watches = self.watches.lock().await;
        if let Some(watch) = watches.get_mut(&location) {
            watch.references += 1;
            return Ok(watch.sender.subscribe());
        }

        let cancellation = CancellationToken::new();
        let active = Arc::new(AtomicBool::new(true));
        let mut stream: ProviderChangeStream = match provider.change_tracking() {
            ChangeTracking::NativeWatch | ChangeTracking::DeltaApi => {
                provider.watch(&location, cancellation.clone()).await?
            }
            ChangeTracking::Poll { interval } => poll_change_stream(
                Arc::clone(&provider),
                location.clone(),
                interval,
                Arc::clone(&active),
                cancellation.clone(),
            ),
            ChangeTracking::Unsupported => {
                return Err(VfsError::UnsupportedCapability {
                    capability: fm_vfs::ProviderCapabilities::WATCH,
                });
            }
        };
        let (sender, receiver) = broadcast::channel(16);
        let forward = sender.clone();
        let source_cancellation = cancellation.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = source_cancellation.cancelled() => break,
                    item = stream.next() => match item {
                        Some(Ok(change)) => { let _ = forward.send(change); }
                        Some(Err(_)) => { let _ = forward.send(ProviderChange::ResetRequired); }
                        None => break,
                    }
                }
            }
        });
        watches.insert(
            location,
            SharedWatch {
                sender,
                cancellation,
                references: 1,
                active,
            },
        );
        Ok(receiver)
    }

    async fn release(&self, location: &fm_domain::Location) {
        let mut watches = self.watches.lock().await;
        let remove = watches.get_mut(location).is_some_and(|watch| {
            watch.references -= 1;
            watch.references == 0
        });
        if remove && let Some(watch) = watches.remove(location) {
            watch.cancellation.cancel();
        }
    }

    /// Adjusts the poll cadence for a poll-tracked location (task 0109); a
    /// no-op when no watch is registered for it (e.g. its provider has no
    /// change tracking, or every pane that watched it has since navigated
    /// away).
    async fn set_active(&self, location: &fm_domain::Location, active: bool) {
        let watches = self.watches.lock().await;
        if let Some(watch) = watches.get(location) {
            watch.active.store(active, AtomicOrdering::Relaxed);
        }
    }

    #[cfg(test)]
    async fn registration_count(&self) -> usize {
        self.watches.lock().await.len()
    }
}

/// Builds a [`ProviderChangeStream`] for a [`ChangeTracking::Poll`] provider
/// by periodically re-listing `location` and comparing it against the
/// previous poll (task 0109), rather than requiring the provider to fake a
/// native [`FileSystemProvider::watch`].
///
/// The very first successful poll only seeds the baseline and never emits:
/// [`DirectoryService::list`] already published that same listing before
/// this watch was acquired, so signalling a change here too would cause an
/// immediate, redundant re-list. Entries are compared by id rather than by
/// list order, since a provider makes no ordering guarantee between calls.
/// A failed poll is swallowed and doubles the wait before the next attempt,
/// capped at [`MAX_POLL_BACKOFF_MULTIPLIER`], rather than tearing down the
/// watch: a transient network hiccup must not force a full directory reset.
/// `active` (shared with every pane watching this location) is read fresh
/// before each sleep, so toggling it takes effect on the very next tick.
fn poll_change_stream(
    provider: Arc<dyn FileSystemProvider>,
    location: fm_domain::Location,
    base_interval: Duration,
    active: Arc<AtomicBool>,
    cancellation: CancellationToken,
) -> ProviderChangeStream {
    struct State {
        provider: Arc<dyn FileSystemProvider>,
        location: fm_domain::Location,
        base_interval: Duration,
        active: Arc<AtomicBool>,
        cancellation: CancellationToken,
        previous: Option<HashMap<EntryId, fm_domain::EntrySummary>>,
        backoff: u32,
    }

    let state = State {
        provider,
        location,
        base_interval,
        active,
        cancellation,
        previous: None,
        backoff: 1,
    };

    Box::pin(stream::unfold(state, |mut state| async move {
        loop {
            let activity_multiplier = if state.active.load(AtomicOrdering::Relaxed) {
                1
            } else {
                BACKGROUND_POLL_MULTIPLIER
            };
            let interval = state.base_interval * (activity_multiplier * state.backoff);
            tokio::select! {
                () = state.cancellation.cancelled() => return None,
                () = tokio::time::sleep(interval) => {}
            }

            match list_all(
                Arc::clone(&state.provider),
                &state.location,
                state.cancellation.clone(),
            )
            .await
            {
                Ok(entries) => {
                    state.backoff = 1;
                    let fingerprint: HashMap<EntryId, fm_domain::EntrySummary> =
                        entries.into_iter().map(|entry| (entry.id, entry)).collect();
                    let is_first_poll = state.previous.is_none();
                    let changed = state.previous.as_ref() != Some(&fingerprint);
                    state.previous = Some(fingerprint);
                    if changed && !is_first_poll {
                        return Some((Ok(ProviderChange::Changed), state));
                    }
                }
                Err(VfsError::Cancelled) => return None,
                Err(_) => {
                    state.backoff = (state.backoff * 2).min(MAX_POLL_BACKOFF_MULTIPLIER);
                }
            }
        }
    }))
}

struct PaneWatch {
    provider: Arc<dyn FileSystemProvider>,
    location: fm_domain::Location,
    workspace_id: fm_domain::WorkspaceId,
    pane_id: PaneId,
    show_hidden: bool,
    folders_first: bool,
    show_git_status: bool,
    sort: Vec<SortDescriptorDto>,
    cancellation: CancellationToken,
    receiver: broadcast::Receiver<ProviderChange>,
    panes: Arc<Mutex<HashMap<PaneId, PaneRequest>>>,
    watches: Arc<WatchHub>,
    events: EventBus,
    git_status: Arc<GitStatusService>,
}

fn spawn_pane_watch(mut watch: PaneWatch) {
    tokio::spawn(async move {
        loop {
            let change = tokio::select! {
                () = watch.cancellation.cancelled() => break,
                received = watch.receiver.recv() => match received {
                    Ok(change) => change,
                    Err(broadcast::error::RecvError::Lagged(_)) => ProviderChange::ResetRequired,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            };
            let mut entries = match list_all(
                Arc::clone(&watch.provider),
                &watch.location,
                watch.cancellation.clone(),
            )
            .await
            {
                Ok(entries) => entries,
                Err(VfsError::Cancelled) => break,
                Err(_) => continue,
            };
            annotate_git_status(
                &watch.git_status,
                &watch.location,
                &mut entries,
                true,
                watch.show_git_status,
            )
            .await;
            publish_changes(&watch, change, entries).await;
        }
        watch.watches.release(&watch.location).await;
    });
}

/// Annotates `entries` with git working-tree status (task 0135), local
/// provider directories only — remote and archive providers, and anything
/// outside a git working tree, are left untouched (`git_status` stays
/// `None`).
///
/// `force_refresh` drops any cached status for `location`'s working tree
/// before recomputing; callers set it on a filesystem-watch-triggered
/// relist, so a real change is never served stale.
///
/// A no-op entirely when `show_git_status` is `false` — most panes never
/// show the git-status column (it's opt-in and hidden by default), so this
/// keeps an ordinary listing free of any `git2` work rather than relying on
/// [`GitStatusService`]'s caching alone to make that work cheap.
async fn annotate_git_status(
    git_status: &GitStatusService,
    location: &fm_domain::Location,
    entries: &mut [fm_domain::EntrySummary],
    force_refresh: bool,
    show_git_status: bool,
) {
    if !show_git_status || location.provider_id.as_str() != "local" {
        return;
    }
    let Ok(dir) = location.to_native_path() else {
        return;
    };
    if force_refresh {
        git_status.invalidate(&dir);
    }
    git_status.annotate(&dir, entries).await;
}

async fn list_all(
    provider: Arc<dyn FileSystemProvider>,
    location: &fm_domain::Location,
    cancellation: CancellationToken,
) -> Result<Vec<fm_domain::EntrySummary>, VfsError> {
    // This loop always drains the whole directory (global sorting requires it —
    // see the doc comment on `DirectoryService::list`), so a small page size
    // only forces extra round trips. For `LocalFileSystemProvider` those round
    // trips are each a full directory re-scan (no cross-call cursor state), so
    // a small page size makes this loop O(n^2) in directory size instead of
    // O(n) — a folder with tens of thousands of entries could take seconds.
    // A large page size makes the common case a single round trip; remote
    // providers with their own real wire-level page caps are unaffected,
    // since they just clamp and keep returning `has_more` normally.
    const FULL_LISTING_PAGE_SIZE: usize = 65_536;
    let mut entries = Vec::new();
    let mut continuation_token = None;
    loop {
        let page = provider
            .list(
                location,
                ListOptions {
                    page_size: FULL_LISTING_PAGE_SIZE,
                    continuation_token,
                },
                cancellation.clone(),
            )
            .await?;
        entries.extend(page.entries);
        if !page.has_more {
            return Ok(entries);
        }
        continuation_token = page.continuation_token;
    }
}

async fn publish_changes(
    watch: &PaneWatch,
    change: ProviderChange,
    mut entries: Vec<fm_domain::EntrySummary>,
) {
    if !watch.show_hidden {
        entries.retain(|entry| !entry.hidden);
    }
    sort_entries(&mut entries, &watch.sort, watch.folders_first);

    let mut panes = watch.panes.lock().await;
    let Some(state) = panes.get_mut(&watch.pane_id).filter(|state| {
        state
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.location == watch.location)
    }) else {
        return;
    };
    let Some(previous) = state.snapshot.clone() else {
        return;
    };
    state.revision += 1;
    let revision = state.revision;
    let (total_known_size, total_known_file_count) = aggregate_totals(&entries);
    let snapshot = DirectorySnapshot {
        revision,
        entries: entries.clone(),
        total_known_entries: Some(entries.len() as u64),
        total_known_size: Some(total_known_size),
        total_known_file_count: Some(total_known_file_count),
        has_more: false,
        continuation_token: None,
        ..previous.clone()
    };
    let mut deltas = deltas_for_change(change, &previous, snapshot.clone(), entries, revision);
    let final_revision = deltas.last().map_or(revision, delta_revision);
    let mut snapshot = snapshot;
    snapshot.revision = final_revision;
    if let [
        DirectoryDeltaPayload::Reset {
            snapshot: reset_snapshot,
        },
    ] = &mut deltas[..]
    {
        reset_snapshot.revision = final_revision;
    }
    state.revision = final_revision;
    state.full_entries = Some(Arc::new(snapshot.entries.clone()));
    state.snapshot = Some(snapshot);
    drop(panes);

    for delta in deltas {
        watch.events.publish(
            EventAudience::Workspace(watch.workspace_id),
            BackendEventPayload::DirectoryDelta {
                pane_id: watch.pane_id,
                delta,
            },
        );
    }
}

fn deltas_for_change(
    change: ProviderChange,
    previous: &DirectorySnapshot,
    snapshot: DirectorySnapshot,
    entries: Vec<fm_domain::EntrySummary>,
    revision: u64,
) -> Vec<DirectoryDeltaPayload> {
    if change == ProviderChange::ResetRequired {
        vec![DirectoryDeltaPayload::Reset {
            snapshot: snapshot.into(),
        }]
    } else {
        diff_entries(&previous.entries, entries, revision)
    }
}

fn diff_entries(
    previous: &[fm_domain::EntrySummary],
    current: Vec<fm_domain::EntrySummary>,
    revision: u64,
) -> Vec<DirectoryDeltaPayload> {
    let previous_by_id: HashMap<_, _> = previous.iter().map(|entry| (entry.id, entry)).collect();
    let current_ids: HashSet<_> = current.iter().map(|entry| entry.id).collect();
    let added: Vec<_> = current
        .iter()
        .filter(|entry| !previous_by_id.contains_key(&entry.id))
        .cloned()
        .map(EntrySummaryPayload::from)
        .collect();
    let updated: Vec<_> = current
        .iter()
        .filter(|entry| {
            previous_by_id
                .get(&entry.id)
                .is_some_and(|old| *old != *entry)
        })
        .cloned()
        .map(EntrySummaryPayload::from)
        .collect();
    let removed: Vec<_> = previous
        .iter()
        .filter(|entry| !current_ids.contains(&entry.id))
        .map(|entry| entry.id)
        .collect();
    let mut deltas = Vec::with_capacity(3);
    let mut next_revision = revision;
    if !added.is_empty() {
        deltas.push(DirectoryDeltaPayload::EntriesAdded {
            revision: next_revision,
            entries: added,
        });
        next_revision += 1;
    }
    if !updated.is_empty() {
        deltas.push(DirectoryDeltaPayload::EntriesUpdated {
            revision: next_revision,
            entries: updated,
        });
        next_revision += 1;
    }
    if !removed.is_empty() {
        deltas.push(DirectoryDeltaPayload::EntriesRemoved {
            revision: next_revision,
            entry_ids: removed,
        });
    }
    deltas
}

fn delta_revision(delta: &DirectoryDeltaPayload) -> u64 {
    match delta {
        DirectoryDeltaPayload::EntriesAdded { revision, .. }
        | DirectoryDeltaPayload::EntriesUpdated { revision, .. }
        | DirectoryDeltaPayload::EntriesRemoved { revision, .. } => *revision,
        DirectoryDeltaPayload::Reset { snapshot } => snapshot.revision,
    }
}

fn sort_entries(
    entries: &mut [fm_domain::EntrySummary],
    sort: &[SortDescriptorDto],
    folders_first: bool,
) {
    entries.sort_by(|left, right| {
        if folders_first {
            let folder_order = matches!(right.kind, EntryKind::Directory)
                .cmp(&matches!(left.kind, EntryKind::Directory));
            if folder_order != Ordering::Equal {
                return folder_order;
            }
        }
        for descriptor in sort {
            let ordering = compare_entry(left, right, &descriptor.column_id);
            if ordering != Ordering::Equal {
                return match descriptor.direction {
                    SortDirectionDto::Ascending => ordering,
                    SortDirectionDto::Descending => ordering.reverse(),
                };
            }
        }
        left.name.cmp(&right.name)
    });
}

fn compare_entry(
    left: &fm_domain::EntrySummary,
    right: &fm_domain::EntrySummary,
    column_id: &str,
) -> Ordering {
    match column_id {
        "core.extension" => left.extension.cmp(&right.extension),
        "core.size" => left.size.cmp(&right.size),
        "core.modified" => left.modified_at.cmp(&right.modified_at),
        _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
    }
}

/// Sums the byte size and counts the files/symlinks (directories excluded) across every entry
/// already resident in memory - no extra provider I/O, since callers only ever have a fully
/// enumerated entry list on hand (`full_entries`) by the time they need these totals.
fn aggregate_totals(entries: &[fm_domain::EntrySummary]) -> (u64, u64) {
    entries
        .iter()
        .filter(|entry| !matches!(entry.kind, EntryKind::Directory))
        .fold((0u64, 0u64), |(size, count), entry| {
            (size + entry.size.unwrap_or(0), count + 1)
        })
}

/// Decodes `list()`'s continuation token: a plain index offset into the pane's cached, fully
/// sorted entry list (opaque to callers, but simple since it addresses an in-memory `Vec` rather
/// than a provider-specific cursor).
fn decode_page_offset(token: Option<&str>) -> Result<usize, ApplicationError> {
    match token {
        None => Ok(0),
        Some(raw) => raw
            .parse()
            .map_err(|_| ApplicationError::InvalidRequest("invalid continuation token".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    use async_trait::async_trait;
    use fm_domain::{EntryMetadata, Location, ProviderId};
    use fm_events::{SessionId, SubscriptionEvent};
    use fm_transport_dto::LocationDto;
    use fm_vfs::{
        ChangeTracking, DirectoryPage, FileSystemProvider, ProviderCapabilities,
        ProviderChangeStream, ProviderReadStream, ProviderWriteStream, RemoveOptions, VfsError,
        WriteOptions,
    };
    use tokio::sync::Notify;

    use super::*;

    /// Watch acquisition is spawned rather than awaited by `DirectoryService::list` (task 0156 -
    /// awaiting it inline blocked every first-time navigation on filesystem-watch setup), so
    /// tests can no longer assert `registration_count()` synchronously right after `list()`
    /// returns; the registration may still be in flight on a spawned task. Poll with a generous
    /// timeout instead - the same pattern `repeated_navigation_releases_superseded_watch_registrations`
    /// already used for its own (pre-existing, unrelated) async settling.
    async fn wait_for_registration_count(service: &DirectoryService, expected: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if service.watches.registration_count().await == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("registration_count never reached {expected}"));
    }

    struct LateProvider {
        calls: AtomicUsize,
        first_started: Notify,
        release_first: Notify,
    }

    impl LateProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                first_started: Notify::new(),
                release_first: Notify::new(),
            }
        }
    }

    #[async_trait]
    impl FileSystemProvider for LateProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("late")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::LIST
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<DirectoryPage, VfsError> {
            if self.calls.fetch_add(1, AtomicOrdering::SeqCst) == 0 {
                self.first_started.notify_one();
                self.release_first.notified().await;
            }
            Ok(DirectoryPage {
                entries: Vec::new(),
                total_known_entries: Some(0),
                has_more: false,
                continuation_token: None,
            })
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<EntryMetadata, VfsError> {
            Err(unsupported())
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), VfsError> {
            Err(unsupported())
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<ProviderReadStream, VfsError> {
            Err(unsupported())
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<ProviderWriteStream, VfsError> {
            Err(unsupported())
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<ProviderChangeStream, VfsError> {
            Err(unsupported())
        }
    }

    fn unsupported() -> VfsError {
        VfsError::UnsupportedCapability {
            capability: ProviderCapabilities::LIST,
        }
    }

    fn request(pane_id: PaneId, request_id: Uuid) -> ListDirectoryRequest {
        ListDirectoryRequest {
            workspace_id: Uuid::new_v4(),
            pane_id: pane_id.into(),
            request_id,
            location: LocationDto {
                provider_id: "late".to_owned(),
                uri: "late:///directory".to_owned(),
            },
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: true,
            folders_first: false,
            show_git_status: true,
        }
    }

    /// A provider with `ChangeTracking::Poll` tracking whose successive
    /// `list()` responses are scripted, for exercising the generic poll
    /// loop (task 0109). Every call is timestamped so tests can assert on
    /// cadence, and the final scripted response repeats indefinitely once
    /// exhausted so a test never needs to predict exactly how many polls a
    /// downstream consumer (e.g. `spawn_pane_watch`'s own re-list) makes.
    struct PollingProvider {
        id: ProviderId,
        tracking_interval: Duration,
        responses: Vec<PollOutcome>,
        cursor: AtomicUsize,
        call_times: Mutex<Vec<std::time::Instant>>,
    }

    #[derive(Clone)]
    enum PollOutcome {
        Ok(Vec<fm_domain::EntrySummary>),
        Err,
    }

    impl PollingProvider {
        fn new(id: ProviderId, tracking_interval: Duration, responses: Vec<PollOutcome>) -> Self {
            Self {
                id,
                tracking_interval,
                responses,
                cursor: AtomicUsize::new(0),
                call_times: Mutex::new(Vec::new()),
            }
        }

        async fn call_count(&self) -> usize {
            self.call_times.lock().await.len()
        }

        async fn call_times_snapshot(&self) -> Vec<std::time::Instant> {
            self.call_times.lock().await.clone()
        }
    }

    #[async_trait]
    impl FileSystemProvider for PollingProvider {
        fn id(&self) -> ProviderId {
            self.id.clone()
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::LIST
        }

        fn change_tracking(&self) -> ChangeTracking {
            ChangeTracking::Poll {
                interval: self.tracking_interval,
            }
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<DirectoryPage, VfsError> {
            self.call_times.lock().await.push(std::time::Instant::now());
            let index = self.cursor.fetch_add(1, AtomicOrdering::SeqCst);
            let outcome = self
                .responses
                .get(index)
                .or_else(|| self.responses.last())
                .cloned()
                .expect("PollingProvider needs at least one scripted response");
            match outcome {
                PollOutcome::Ok(entries) => Ok(DirectoryPage {
                    entries,
                    total_known_entries: Some(0),
                    has_more: false,
                    continuation_token: None,
                }),
                PollOutcome::Err => Err(VfsError::Io {
                    message: "simulated poll failure".to_owned(),
                }),
            }
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<EntryMetadata, VfsError> {
            Err(unsupported())
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), VfsError> {
            Err(unsupported())
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<ProviderReadStream, VfsError> {
            Err(unsupported())
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<ProviderWriteStream, VfsError> {
            Err(unsupported())
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<ProviderChangeStream, VfsError> {
            Err(unsupported())
        }
    }

    /// A provider with no change tracking at all (`ChangeTracking::Unsupported`),
    /// used to prove such providers are never watched or polled.
    struct UntrackedProvider;

    #[async_trait]
    impl FileSystemProvider for UntrackedProvider {
        fn id(&self) -> ProviderId {
            ProviderId::new("untracked")
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::LIST
        }

        async fn list(
            &self,
            _location: &Location,
            _options: ListOptions,
            _cancellation: CancellationToken,
        ) -> Result<DirectoryPage, VfsError> {
            Ok(DirectoryPage {
                entries: Vec::new(),
                total_known_entries: Some(0),
                has_more: false,
                continuation_token: None,
            })
        }

        async fn metadata(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<EntryMetadata, VfsError> {
            Err(unsupported())
        }

        async fn create_directory(
            &self,
            _location: &Location,
            _name: &str,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn rename(
            &self,
            _source: &EntryRef,
            _destination: &Location,
            _cancellation: CancellationToken,
        ) -> Result<EntryRef, VfsError> {
            Err(unsupported())
        }

        async fn remove(
            &self,
            _entry: &EntryRef,
            _options: RemoveOptions,
            _cancellation: CancellationToken,
        ) -> Result<(), VfsError> {
            Err(unsupported())
        }

        async fn open_read(
            &self,
            _entry: &EntryRef,
            _cancellation: CancellationToken,
        ) -> Result<ProviderReadStream, VfsError> {
            Err(unsupported())
        }

        async fn open_write(
            &self,
            _destination: &Location,
            _options: WriteOptions,
            _cancellation: CancellationToken,
        ) -> Result<ProviderWriteStream, VfsError> {
            Err(unsupported())
        }

        async fn watch(
            &self,
            _location: &Location,
            _cancellation: CancellationToken,
        ) -> Result<ProviderChangeStream, VfsError> {
            Err(unsupported())
        }
    }

    fn poll_entry(name: &str) -> fm_domain::EntrySummary {
        fm_domain::EntrySummary {
            id: fm_domain::EntryId::new(),
            location: Location::new(ProviderId::new("poll"), format!("poll:///dir/{name}")),
            name: name.to_owned(),
            kind: EntryKind::File,
            size: Some(0),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: None,
            mime_type: None,
            icon_key: None,
            metadata_revision: 0,
            git_status: None,
        }
    }

    fn poll_request(pane_id: PaneId) -> ListDirectoryRequest {
        let mut request = request(pane_id, Uuid::new_v4());
        request.location = LocationDto {
            provider_id: "poll".to_owned(),
            uri: "poll:///dir".to_owned(),
        };
        request
    }

    async fn wait_for_call_count(provider: &PollingProvider, target: usize) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if provider.call_count().await >= target {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .expect("provider must reach the expected call count in time");
    }

    #[tokio::test]
    async fn a_poll_tracked_provider_change_produces_a_directory_delta_event() {
        let entry_a = poll_entry("a.txt");
        let entry_b = poll_entry("b.txt");
        let provider = Arc::new(PollingProvider::new(
            ProviderId::new("poll"),
            Duration::from_millis(20),
            vec![
                PollOutcome::Ok(vec![entry_a.clone()]),
                PollOutcome::Ok(vec![entry_a.clone()]),
                PollOutcome::Ok(vec![entry_a.clone(), entry_b.clone()]),
            ],
        ));
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        let events = EventBus::default();
        let service = DirectoryService::with_event_bus(providers, events.clone());
        let workspace_id = Uuid::new_v4();
        let mut subscription = events.subscribe_all_workspaces(SessionId::new("test"), None);

        let mut list_request = poll_request(PaneId::new());
        list_request.workspace_id = workspace_id;
        let snapshot = service
            .list(list_request)
            .await
            .expect("initial listing must succeed");
        assert_eq!(snapshot.entries.len(), 1);

        let delta = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                match subscription
                    .recv()
                    .await
                    .expect("subscription must stay open")
                {
                    SubscriptionEvent::Event(envelope) => {
                        if let BackendEventPayload::DirectoryDelta { delta, .. } = envelope.payload
                        {
                            return delta;
                        }
                    }
                    SubscriptionEvent::Gap { .. } => {}
                }
            }
        })
        .await
        .expect("a delta must be published once the poller observes the provider's change");

        match delta {
            DirectoryDeltaPayload::EntriesAdded { entries, .. } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].name, "b.txt");
            }
            other => panic!("expected an EntriesAdded delta, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unchanged_polls_do_not_publish_directory_delta_events() {
        let entry_a = poll_entry("a.txt");
        let provider = Arc::new(PollingProvider::new(
            ProviderId::new("poll"),
            Duration::from_millis(15),
            vec![PollOutcome::Ok(vec![entry_a.clone()])],
        ));
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        let events = EventBus::default();
        let service = DirectoryService::with_event_bus(providers, events.clone());
        let mut subscription = events.subscribe_all_workspaces(SessionId::new("test"), None);

        service
            .list(poll_request(PaneId::new()))
            .await
            .expect("initial listing must succeed");

        wait_for_call_count(&provider, 6).await;

        let outcome = tokio::time::timeout(Duration::from_millis(50), subscription.recv()).await;
        assert!(
            outcome.is_err(),
            "an unchanged poll must not publish a directory delta event"
        );
    }

    #[tokio::test]
    async fn poll_failures_back_off_with_increasing_delay_before_retrying() {
        let entry_a = poll_entry("a.txt");
        let interval = Duration::from_millis(20);
        let provider = Arc::new(PollingProvider::new(
            ProviderId::new("poll"),
            interval,
            vec![
                PollOutcome::Ok(vec![entry_a.clone()]), // DirectoryService::list()'s own initial listing
                PollOutcome::Ok(vec![entry_a.clone()]), // poll tick 1: baseline
                PollOutcome::Err,                       // poll tick 2
                PollOutcome::Err,                       // poll tick 3
                PollOutcome::Err,                       // poll tick 4
                PollOutcome::Ok(vec![entry_a.clone()]), // poll tick 5: recovers
            ],
        ));
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        let service = DirectoryService::new(providers);

        service
            .list(poll_request(PaneId::new()))
            .await
            .expect("initial listing must succeed");

        wait_for_call_count(&provider, 6).await;

        let calls = provider.call_times_snapshot().await;
        let gaps: Vec<Duration> = calls.windows(2).map(|pair| pair[1] - pair[0]).collect();
        // gaps[0], gaps[1]: poll ticks 1 and 2, both at the base interval (no failure yet).
        // gaps[2]: after tick 2 failed, backoff x2.
        // gaps[3]: after tick 3 failed, backoff x4.
        // gaps[4]: after tick 4 failed, backoff x8.
        assert!(
            gaps[3].as_secs_f64() > gaps[2].as_secs_f64() * 1.3,
            "backoff must grow after a second consecutive failure: {gaps:?}"
        );
        assert!(
            gaps[4].as_secs_f64() > gaps[3].as_secs_f64() * 1.3,
            "backoff must grow after a third consecutive failure: {gaps:?}"
        );
    }

    #[tokio::test]
    async fn navigating_away_stops_the_poll_loop_for_a_poll_tracked_provider() {
        let entry_a = poll_entry("a.txt");
        let provider = Arc::new(PollingProvider::new(
            ProviderId::new("poll"),
            Duration::from_millis(10),
            vec![PollOutcome::Ok(vec![entry_a.clone()])],
        ));
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        providers.register(Arc::new(UntrackedProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();

        service
            .list(poll_request(pane_id))
            .await
            .expect("initial listing must succeed");
        wait_for_call_count(&provider, 3).await;

        let mut other_request = request(pane_id, Uuid::new_v4());
        other_request.location = LocationDto {
            provider_id: "untracked".to_owned(),
            uri: "untracked:///directory".to_owned(),
        };
        service
            .list(other_request)
            .await
            .expect("navigating away must succeed");

        let count_at_navigate = provider.call_count().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let count_after_wait = provider.call_count().await;

        assert!(
            count_after_wait <= count_at_navigate + 1,
            "the poll loop must stop once the pane navigates away: {count_at_navigate} -> {count_after_wait}"
        );
        assert_eq!(service.watches.registration_count().await, 0);
    }

    #[tokio::test]
    async fn a_provider_with_unsupported_change_tracking_is_never_watched() {
        let provider = Arc::new(UntrackedProvider);
        let mut providers = ProviderRegistry::new();
        providers.register(provider);
        let service = DirectoryService::new(providers);

        let mut list_request = request(PaneId::new(), Uuid::new_v4());
        list_request.location = LocationDto {
            provider_id: "untracked".to_owned(),
            uri: "untracked:///directory".to_owned(),
        };
        service
            .list(list_request)
            .await
            .expect("listing must succeed");

        assert_eq!(service.watches.registration_count().await, 0);
    }

    #[tokio::test]
    async fn set_pane_activity_reduces_poll_frequency_for_a_backgrounded_pane() {
        let entry_a = poll_entry("a.txt");
        let interval = Duration::from_millis(20);
        let provider = Arc::new(PollingProvider::new(
            ProviderId::new("poll"),
            interval,
            vec![PollOutcome::Ok(vec![entry_a.clone()])],
        ));
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();

        service
            .list(poll_request(pane_id))
            .await
            .expect("initial listing must succeed");

        wait_for_call_count(&provider, 2).await;
        let before = provider.call_times_snapshot().await;
        let active_gap = *before.last().expect("at least 2 calls") - before[before.len() - 2];

        service
            .set_pane_activity(pane_id, false)
            .await
            .expect("pane must exist");

        // The poll tick already sleeping when `set_pane_activity` lands may have started that
        // sleep with the stale (still-active) cadence, so wait for a *second* tick past the
        // toggle: only its sleep is guaranteed to have read the freshly stored `false`.
        let count_before_background = provider.call_count().await;
        wait_for_call_count(&provider, count_before_background + 2).await;
        let after = provider.call_times_snapshot().await;
        let background_gap = *after.last().expect("at least 2 calls") - after[after.len() - 2];

        assert!(
            background_gap.as_secs_f64() > active_gap.as_secs_f64() * 2.0,
            "a backgrounded pane's poll gap ({background_gap:?}) must be meaningfully longer \
             than the active gap ({active_gap:?})"
        );
    }

    #[tokio::test]
    async fn set_pane_activity_reports_not_found_for_an_unknown_pane() {
        let service = DirectoryService::new(ProviderRegistry::new());

        let error = service
            .set_pane_activity(PaneId::new(), false)
            .await
            .expect_err("an unknown pane must be rejected");

        assert_eq!(error, ApplicationError::NotFound);
    }

    #[tokio::test]
    async fn a_late_superseded_response_is_discarded() {
        let provider = Arc::new(LateProvider::new());
        let mut providers = ProviderRegistry::new();
        providers.register(provider.clone());
        let service = Arc::new(DirectoryService::new(providers));
        let pane_id = PaneId::new();
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();

        let first_service = service.clone();
        let first =
            tokio::spawn(async move { first_service.list(request(pane_id, first_id)).await });
        provider.first_started.notified().await;

        let second = service
            .list(request(pane_id, second_id))
            .await
            .expect("newest request must be published");
        provider.release_first.notify_one();
        let first = first
            .await
            .expect("task must join")
            .expect_err("superseded response must be discarded");

        assert_eq!(second.request_id, second_id);
        assert_eq!(second.revision, 1);
        assert_eq!(first, ApplicationError::OperationCancelled);
    }

    #[tokio::test]
    async fn repeated_navigation_releases_superseded_watch_registrations() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();

        for index in 0..100 {
            let path = root.path().join(format!("directory-{index}"));
            std::fs::create_dir(&path).expect("create watched directory");
            let location = Location::from_native_path(&path).expect("local location");
            let mut request = request(pane_id, Uuid::new_v4());
            request.location = LocationDto::from(location);
            service.list(request).await.expect("navigate and watch");
        }

        wait_for_registration_count(&service, 1).await;
    }

    #[tokio::test]
    async fn listing_a_directory_larger_than_one_page_paginates_a_globally_sorted_cache() {
        let root = tempfile::tempdir().expect("temporary directory");
        for index in 0..257 {
            std::fs::write(root.path().join(format!("entry-{index:03}")), b"")
                .expect("create paged entry");
        }
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();
        let workspace_id = Uuid::new_v4();
        let location =
            LocationDto::from(Location::from_native_path(root.path()).expect("local location"));

        let mut first_request = request(pane_id, Uuid::new_v4());
        first_request.workspace_id = workspace_id;
        first_request.location = location.clone();
        let first = service.list(first_request).await.expect("first page");

        assert!(first.has_more);
        assert_eq!(first.total_known_entries, Some(257));
        assert_eq!(first.total_known_size, Some(0));
        assert_eq!(first.total_known_file_count, Some(257));
        assert_eq!(first.entries.len(), 256);
        wait_for_registration_count(&service, 1).await;

        let mut second_request = request(pane_id, Uuid::new_v4());
        second_request.workspace_id = workspace_id;
        second_request.location = location;
        second_request.continuation_token = first.continuation_token;
        let second = service.list(second_request).await.expect("second page");

        assert!(!second.has_more);
        assert_eq!(second.continuation_token, None);
        assert_eq!(second.total_known_entries, Some(257));
        assert_eq!(second.total_known_size, Some(0));
        assert_eq!(second.total_known_file_count, Some(257));
        assert_eq!(second.entries.len(), 1);
        // The two pages must be a contiguous slice of one globally sorted list: no gaps,
        // no duplicates, and page 2 continues immediately after page 1 in sort order.
        assert!(first.entries.last().unwrap().name < second.entries[0].name);
        let mut names: Vec<_> = first
            .entries
            .iter()
            .chain(second.entries.iter())
            .map(|entry| entry.name.clone())
            .collect();
        let unique_count = {
            names.sort();
            names.dedup();
            names.len()
        };
        assert_eq!(unique_count, 257);
        wait_for_registration_count(&service, 1).await;
    }

    #[tokio::test]
    async fn listing_a_large_directory_completes_in_roughly_linear_time() {
        // Regression test for task 0156 ("slow directory navigation"), which turned
        // out to be three separate bugs, all exercised end-to-end by this one test
        // (a narrower test at just the provider level, like `fm-vfs-local`'s
        // `returns_the_first_page_of_a_hundred_thousand_entry_directory`, would miss
        // all three — each only manifests through `DirectoryService::list`'s full
        // call chain):
        // 1. `list_all` drained the whole directory via repeated provider `list()`
        //    calls, and `LocalFileSystemProvider::list` re-opened `read_dir` from
        //    scratch and re-iterated past `offset` entries on every call - O(n^2)
        //    round trips. Fixed by requesting a page size large enough to cover the
        //    common case in one round trip.
        // 2. Even in one round trip, `LocalFileSystemProvider::list` awaited a
        //    separate `tokio::fs` call (each its own blocking-thread-pool round
        //    trip) per directory entry - tens of thousands of sequential async hops
        //    for a large directory. Fixed by batching the whole page into one
        //    `spawn_blocking` call using plain `std::fs`.
        // 3. The dominant cost, found only once (1) and (2) no longer masked it:
        //    `DirectoryService::list` awaited filesystem-watch registration
        //    (FSEvents on macOS) before returning the listing at all - 20+ seconds
        //    in testing, and NOT proportional to directory size (reproduced with a
        //    single-entry directory too). Fixed by spawning watch acquisition
        //    instead of awaiting it inline, since the listing is already complete
        //    and useful before a live-update watch exists for it.
        // A generous ceiling here is far above the sub-second time the fixed code takes locally,
        // but would reliably catch any of the three regressing - a reintroduced O(n^2) scan or an
        // awaited watch registration both cost tens of seconds at this scale, not a small
        // multiple. 3s (the original value) reliably failed on GitHub's Windows CI runners
        // specifically (5-6s consistently across independent runs, vs. comfortable sub-second
        // margins on macOS/Linux CI) - NTFS/Defender per-file overhead for 20,000 tiny files, not
        // a code regression. 10s keeps two orders of magnitude of margin below what a real
        // regression would cost while accommodating that platform gap.
        let root = tempfile::tempdir().expect("temporary directory");
        for index in 0..20_000 {
            std::fs::write(root.path().join(format!("entry-{index:05}")), b"")
                .expect("create fixture entry");
        }
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let mut request = request(PaneId::new(), Uuid::new_v4());
        request.workspace_id = Uuid::new_v4();
        request.location =
            LocationDto::from(Location::from_native_path(root.path()).expect("local location"));

        let started = std::time::Instant::now();
        let snapshot = service.list(request).await.expect("list large directory");

        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "listing 20,000 entries took {:?} - likely a reintroduced O(n^2) scan",
            started.elapsed()
        );
        assert_eq!(snapshot.total_known_entries, Some(20_000));
    }

    #[tokio::test]
    async fn navigate_carries_over_the_requested_view_options_instead_of_hardcoded_defaults() {
        // Regression test: `navigate` (used for every "push" navigation - favourites,
        // breadcrumbs, opening a subfolder - not just brand new tabs) used to always request
        // `show_hidden: false, folders_first: true, sort: []` regardless of what the caller
        // asked for, silently resetting a tab's view every time it navigated to a new location.
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("visible.txt"), b"").expect("create visible entry");
        std::fs::write(root.path().join(".hidden.txt"), b"").expect("create hidden entry");
        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();
        let location =
            LocationDto::from(Location::from_native_path(root.path()).expect("local location"));

        let snapshot = service
            .navigate(NavigateRequest {
                workspace_id: Uuid::new_v4(),
                pane_id: pane_id.into(),
                request_id: Uuid::new_v4(),
                location,
                sort: Vec::new(),
                show_hidden: true,
                folders_first: false,
                show_git_status: false,
            })
            .await
            .expect("navigate must succeed");

        assert_eq!(snapshot.entries.len(), 2, "hidden entry must be included");
        assert!(snapshot.entries.iter().any(|entry| entry.hidden));
    }

    fn sample_entry(kind: EntryKind, size: Option<u64>) -> fm_domain::EntrySummary {
        fm_domain::EntrySummary {
            id: fm_domain::EntryId::new(),
            location: Location::new(ProviderId::new("late"), "late:///directory/entry"),
            name: "entry".to_owned(),
            kind,
            size,
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: None,
            mime_type: None,
            icon_key: None,
            metadata_revision: 0,
            git_status: None,
        }
    }

    #[test]
    fn aggregate_totals_of_an_empty_directory_is_zero() {
        assert_eq!(aggregate_totals(&[]), (0, 0));
    }

    #[test]
    fn aggregate_totals_excludes_directories_and_treats_missing_size_as_zero() {
        let entries = vec![
            sample_entry(EntryKind::File, Some(1_024)),
            sample_entry(EntryKind::Directory, Some(4_096)),
            sample_entry(EntryKind::Symlink, Some(8)),
            sample_entry(EntryKind::File, None),
        ];

        // 2 files + 1 symlink = 3 non-directory entries; the directory's size is never summed,
        // and a file with an unknown size contributes 0 rather than panicking or being skipped.
        assert_eq!(aggregate_totals(&entries), (1_024 + 8, 3));
    }

    #[test]
    fn ten_thousand_added_entries_are_one_batched_delta() {
        let entries = (0..10_000)
            .map(|index| fm_domain::EntrySummary {
                id: fm_domain::EntryId::new(),
                location: Location::new(
                    ProviderId::new("late"),
                    format!("late:///directory/{index}"),
                ),
                name: format!("entry-{index}"),
                kind: EntryKind::File,
                size: Some(0),
                modified_at: None,
                created_at: None,
                hidden: false,
                read_only: false,
                extension: None,
                mime_type: None,
                icon_key: None,
                metadata_revision: 0,
                git_status: None,
            })
            .collect();

        let deltas = diff_entries(&[], entries, 2);

        assert_eq!(deltas.len(), 1);
        assert!(matches!(
            &deltas[0],
            DirectoryDeltaPayload::EntriesAdded { revision: 2, entries }
                if entries.len() == 10_000
        ));
    }

    #[test]
    fn dropped_provider_events_force_a_fresh_snapshot_reset() {
        let pane_id = PaneId::new();
        let location = Location::new(ProviderId::new("late"), "late:///directory");
        let previous = DirectorySnapshot {
            pane_id,
            request_id: Uuid::new_v4(),
            revision: 1,
            location: location.clone(),
            writable: false,
            entries: Vec::new(),
            total_known_entries: Some(0),
            total_known_size: Some(0),
            total_known_file_count: Some(0),
            has_more: false,
            continuation_token: None,
            loading_state: LoadingState::Loaded,
        };
        let fresh = DirectorySnapshot {
            revision: 2,
            ..previous.clone()
        };

        let deltas = deltas_for_change(
            ProviderChange::ResetRequired,
            &previous,
            fresh,
            Vec::new(),
            2,
        );

        assert!(matches!(
            &deltas[..],
            [DirectoryDeltaPayload::Reset { snapshot }]
                if snapshot.pane_id == pane_id && snapshot.revision == 2
        ));
    }

    fn init_git_repo(root: &std::path::Path) {
        let repo = git2::Repository::init(root).expect("init repo");
        let mut config = repo.config().expect("repo config");
        config.set_str("user.name", "Test").expect("set name");
        config
            .set_str("user.email", "test@example.com")
            .expect("set email");
        let mut index = repo.index().expect("index");
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("stage all");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let signature = repo.signature().expect("signature");
        repo.commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .expect("commit");
    }

    #[tokio::test]
    async fn listing_a_git_working_tree_annotates_entries_with_git_status() {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("tracked.txt"), b"a").expect("write tracked file");
        init_git_repo(root.path());
        std::fs::write(root.path().join("tracked.txt"), b"changed").expect("modify tracked file");
        std::fs::write(root.path().join("new.txt"), b"new").expect("write untracked file");

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();
        let location =
            LocationDto::from(Location::from_native_path(root.path()).expect("local location"));
        let mut req = request(pane_id, Uuid::new_v4());
        req.location = location;

        let snapshot = service.list(req).await.expect("list git working tree");

        let tracked = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == "tracked.txt")
            .expect("tracked entry present");
        assert_eq!(tracked.git_status, Some(fm_domain::GitFileStatus::Modified));
        let untracked = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == "new.txt")
            .expect("untracked entry present");
        assert_eq!(
            untracked.git_status,
            Some(fm_domain::GitFileStatus::Untracked)
        );
    }

    #[tokio::test]
    async fn listing_a_non_git_directory_leaves_git_status_unset() {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("plain.txt"), b"a").expect("write plain file");

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();
        let location =
            LocationDto::from(Location::from_native_path(root.path()).expect("local location"));
        let mut req = request(pane_id, Uuid::new_v4());
        req.location = location;

        let snapshot = service.list(req).await.expect("list non-git directory");

        assert!(
            snapshot
                .entries
                .iter()
                .all(|entry| entry.git_status.is_none())
        );
    }

    #[tokio::test]
    async fn refreshing_affected_locations_reflects_a_new_git_status() {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("tracked.txt"), b"a").expect("write tracked file");
        init_git_repo(root.path());

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let pane_id = PaneId::new();
        let location = Location::from_native_path(root.path()).expect("local location");
        let mut req = request(pane_id, Uuid::new_v4());
        req.location = LocationDto::from(location.clone());

        let snapshot = service.list(req).await.expect("initial listing");
        let tracked = snapshot
            .entries
            .iter()
            .find(|entry| entry.name == "tracked.txt")
            .expect("tracked entry present");
        assert_eq!(tracked.git_status, Some(fm_domain::GitFileStatus::Clean));

        std::fs::write(root.path().join("tracked.txt"), b"changed")
            .expect("modify tracked file after listing");

        let mut affected = HashSet::new();
        affected.insert(location);
        service.refresh_affected(&affected).await;

        let refreshed = {
            let panes = service.panes.lock().await;
            panes
                .get(&pane_id)
                .and_then(|state| state.snapshot.clone())
                .expect("refreshed snapshot present")
        };
        let tracked = refreshed
            .entries
            .iter()
            .find(|entry| entry.name == "tracked.txt")
            .expect("tracked entry present after refresh");
        assert_eq!(tracked.git_status, Some(fm_domain::GitFileStatus::Modified));
    }

    #[tokio::test]
    async fn git_history_returns_commits_touching_a_tracked_file() {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("tracked.txt"), b"a").expect("write tracked file");
        init_git_repo(root.path());

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let location =
            Location::from_native_path(&root.path().join("tracked.txt")).expect("local location");

        let history = service.git_history(&location).await;

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].summary, "initial");
        assert_eq!(history[0].author_name, "Test");
    }

    #[tokio::test]
    async fn git_history_of_a_non_git_file_is_empty() {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("plain.txt"), b"a").expect("write plain file");

        let mut providers = ProviderRegistry::new();
        providers.register(Arc::new(fm_vfs_local::LocalFileSystemProvider));
        let service = DirectoryService::new(providers);
        let location =
            Location::from_native_path(&root.path().join("plain.txt")).expect("local location");

        assert!(service.git_history(&location).await.is_empty());
    }

    #[tokio::test]
    async fn git_history_of_a_non_local_provider_is_empty() {
        let providers = ProviderRegistry::new();
        let service = DirectoryService::new(providers);
        let location = Location::new(ProviderId::new("sftp"), "sftp://host/tracked.txt");

        assert!(service.git_history(&location).await.is_empty());
    }
}
