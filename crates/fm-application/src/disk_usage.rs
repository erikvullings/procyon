//! Parallel local disk-usage scanning for the WinDirStat-style treemap (task 0118).

use std::collections::HashSet;
use std::fs;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Mul, MulAssign, Sub, SubAssign};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use fm_checksum::FileIdentity;
use fm_domain::Location;
use fm_events::{
    BackendEventPayload, DiskUsageNodeKindPayload, DiskUsageNodePayload,
    DiskUsageUnreadableEntryPayload, DiskUsageUnreadableReasonPayload, EventAudience, EventBus,
    LocationPayload,
};
use fm_transport_dto::{
    DiskUsageNodeDto, DiskUsageNodeKindDto, DiskUsageUnreadableEntryDto,
    DiskUsageUnreadableReasonDto, ScanDiskUsageRequestDto, ScanDiskUsageResponseDto,
};
use parallel_disk_usage::data_tree::DataTree;
use parallel_disk_usage::get_size::GetSize;
use parallel_disk_usage::os_string_display::OsStringDisplay;
use parallel_disk_usage::size::Size;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use tokio_util::sync::CancellationToken;

use crate::ApplicationError;

const MAX_SCAN_DEPTH: u64 = 12;
/// The UI only renders a few nested levels and can explicitly rescan any collapsed directory.
/// Capping the response depth avoids remapping and serializing millions of already-counted leaf
/// nodes after traversal has finished.
const MAX_RESPONSE_DEPTH: u64 = 4;
const MAX_CHILDREN_PER_DIRECTORY: usize = 2048;
/// Hard cap on total filesystem scan worker threads. Recursive work stealing prevents one large
/// subtree from stranding the other workers while keeping CPU usage bounded independently of
/// directory fan-out, nesting depth, and the host's logical CPU count.
pub(crate) const DISK_USAGE_WORKER_COUNT: usize = 4;
/// Bounds how many unreadable-entry details are retained/reported per scan, so a directory with
/// pervasive permission errors can't make the response (or its progress events) unbounded.
const MAX_UNREADABLE_DETAILS: usize = 500;
const PROGRESS_INTERVALS: [Duration; 5] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];
type ScanTree = DataTree<OsStringDisplay, DiskUsageSize>;
type ChildScanResult = (usize, Result<ScanTree, ApplicationError>);

#[derive(Clone, Copy)]
struct MapNodeOptions {
    is_root: bool,
    expand_root: bool,
    deduplicate_hardlinks: bool,
    remaining_depth: u64,
}

fn disk_usage_thread_pool() -> Result<ThreadPool, ApplicationError> {
    ThreadPoolBuilder::new()
        .num_threads(DISK_USAGE_WORKER_COUNT)
        .thread_name(|index| format!("disk-usage-{index}"))
        .build()
        .map_err(|_| ApplicationError::Internal)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ScannedEntryKind {
    #[default]
    Aggregate,
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DiskUsageSize {
    logical_bytes: u64,
    physical_bytes: u64,
    kind: ScannedEntryKind,
}

impl Add for DiskUsageSize {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            logical_bytes: self.logical_bytes + rhs.logical_bytes,
            physical_bytes: self.physical_bytes + rhs.physical_bytes,
            kind: self.kind,
        }
    }
}

impl AddAssign for DiskUsageSize {
    fn add_assign(&mut self, rhs: Self) {
        self.logical_bytes += rhs.logical_bytes;
        self.physical_bytes += rhs.physical_bytes;
    }
}

impl Sub for DiskUsageSize {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            logical_bytes: self.logical_bytes - rhs.logical_bytes,
            physical_bytes: self.physical_bytes - rhs.physical_bytes,
            kind: self.kind,
        }
    }
}

impl SubAssign for DiskUsageSize {
    fn sub_assign(&mut self, rhs: Self) {
        self.logical_bytes -= rhs.logical_bytes;
        self.physical_bytes -= rhs.physical_bytes;
    }
}

impl Sum for DiskUsageSize {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), Add::add)
    }
}

macro_rules! implement_size_multiplication {
    ($($integer:ty),+ $(,)?) => {
        $(
            impl Mul<$integer> for DiskUsageSize {
                type Output = Self;

                fn mul(self, rhs: $integer) -> Self::Output {
                    let rhs = u64::from(rhs);
                    Self {
                        logical_bytes: self.logical_bytes * rhs,
                        physical_bytes: self.physical_bytes * rhs,
                        kind: self.kind,
                    }
                }
            }

            impl MulAssign<$integer> for DiskUsageSize {
                fn mul_assign(&mut self, rhs: $integer) {
                    let rhs = u64::from(rhs);
                    self.logical_bytes *= rhs;
                    self.physical_bytes *= rhs;
                }
            }
        )+
    };
}

implement_size_multiplication!(u8, u16, u32, u64);

impl Mul<usize> for DiskUsageSize {
    type Output = Self;

    fn mul(self, rhs: usize) -> Self::Output {
        let rhs = u64::try_from(rhs).expect("usize fits into u64 on supported platforms");
        Self {
            logical_bytes: self.logical_bytes * rhs,
            physical_bytes: self.physical_bytes * rhs,
            kind: self.kind,
        }
    }
}

impl MulAssign<usize> for DiskUsageSize {
    fn mul_assign(&mut self, rhs: usize) {
        let rhs = u64::try_from(rhs).expect("usize fits into u64 on supported platforms");
        self.logical_bytes *= rhs;
        self.physical_bytes *= rhs;
    }
}

impl Mul for DiskUsageSize {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            logical_bytes: self.logical_bytes * rhs.logical_bytes,
            physical_bytes: self.physical_bytes * rhs.physical_bytes,
            kind: self.kind,
        }
    }
}

impl Size for DiskUsageSize {
    type Inner = Self;
    type DisplayFormat = ();
    type DisplayOutput = String;

    fn display(self, (): Self::DisplayFormat) -> Self::DisplayOutput {
        format!(
            "{} logical bytes, {} physical bytes",
            self.logical_bytes, self.physical_bytes
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct GetDiskUsageSize;

impl GetSize for GetDiskUsageSize {
    type Size = DiskUsageSize;

    fn get_size(&self, metadata: &fs::Metadata) -> Self::Size {
        DiskUsageSize {
            logical_bytes: metadata.len(),
            physical_bytes: physical_bytes(metadata),
            kind: if metadata.file_type().is_symlink() {
                ScannedEntryKind::Symlink
            } else if metadata.is_dir() {
                ScannedEntryKind::Directory
            } else {
                ScannedEntryKind::File
            },
        }
    }
}

#[cfg(unix)]
fn physical_bytes(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;

    metadata.blocks() * 512
}

#[cfg(not(unix))]
fn physical_bytes(metadata: &fs::Metadata) -> u64 {
    metadata.len()
}

/// Why one filesystem entry could not be included in the scan, sanitized from the raw
/// [`std::io::ErrorKind`] so callers never see OS-specific error strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnreadableReason {
    PermissionDenied,
    Disappeared,
    IoError,
}

impl UnreadableReason {
    fn from_error_kind(kind: std::io::ErrorKind) -> Self {
        match kind {
            std::io::ErrorKind::NotFound => Self::Disappeared,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            _ => Self::IoError,
        }
    }
}

impl From<UnreadableReason> for DiskUsageUnreadableReasonDto {
    fn from(reason: UnreadableReason) -> Self {
        match reason {
            UnreadableReason::PermissionDenied => Self::PermissionDenied,
            UnreadableReason::Disappeared => Self::Disappeared,
            UnreadableReason::IoError => Self::IoError,
        }
    }
}

impl From<UnreadableReason> for DiskUsageUnreadableReasonPayload {
    fn from(reason: UnreadableReason) -> Self {
        match reason {
            UnreadableReason::PermissionDenied => Self::PermissionDenied,
            UnreadableReason::Disappeared => Self::Disappeared,
            UnreadableReason::IoError => Self::IoError,
        }
    }
}

struct UnreadableEntry {
    path: PathBuf,
    reason: UnreadableReason,
}

/// Cumulative count plus a bounded, sorted detail list of entries the scan could not read.
/// The count is retained for compatibility even once the detail list is capped at
/// [`MAX_UNREADABLE_DETAILS`], so a heavily-restricted tree still reports an accurate total.
#[derive(Default)]
struct UnreadableRegistry {
    count: AtomicU64,
    details: Mutex<Vec<UnreadableEntry>>,
}

impl UnreadableRegistry {
    /// Records one unreadable entry. `path` should be the most specific known location: the
    /// entry itself for a metadata or whole-directory read failure, or the parent directory when
    /// an individual `read_dir` entry failed mid-iteration (its own path is not recoverable in
    /// that case).
    fn record(&self, path: &Path, kind: std::io::ErrorKind) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut details = self
            .details
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if details.len() < MAX_UNREADABLE_DETAILS {
            details.push(UnreadableEntry {
                path: path.to_owned(),
                reason: UnreadableReason::from_error_kind(kind),
            });
        }
    }

    fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Bounded, stable-sorted-by-location detail list for the response and progress events.
    /// `LocationDto` has no `Ord`, so entries are compared by `(provider_id, uri)` tuples.
    fn details(&self) -> Vec<DiskUsageUnreadableEntryDto> {
        let details = self
            .details
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut mapped = details
            .iter()
            .filter_map(|entry| {
                let location = Location::from_native_path(&entry.path).ok()?;
                Some(DiskUsageUnreadableEntryDto {
                    location: location.into(),
                    reason: entry.reason.into(),
                })
            })
            .collect::<Vec<_>>();
        mapped.sort_by(|left, right| {
            (
                left.location.provider_id.as_str(),
                left.location.uri.as_str(),
            )
                .cmp(&(
                    right.location.provider_id.as_str(),
                    right.location.uri.as_str(),
                ))
        });
        mapped
    }
}

/// Bounded parallel recursive traversal, replacing
/// `parallel_disk_usage::fs_tree_builder::FsTreeBuilder`. `FsTreeBuilder`'s own `TreeBuilder`
/// forks into Rayon's *global* thread pool, which previously combined with outer std-thread fan-out
/// to produce unbounded nested parallelism. This implementation only runs in the dedicated fixed
/// pool created by [`disk_usage_thread_pool`], but recursively shares work so a single large
/// top-level subtree can use every bounded worker. It retains the same `DataTree` shape and checks
/// `cancellation` at every entry and directory.
///
/// `max_depth` follows `TreeBuilder::from`'s exact arithmetic: it is decremented once per level
/// *before* deciding whether this node's own `children` stay visible, and children are *always*
/// fully traversed to compute correct totals — only the visible `DataTree` structure is capped by
/// depth, never the totals (see `parallel-disk-usage`'s "sizes beyond max depth still count
/// toward total" doc comment on `max_depth`, replicated here so
/// `disk_usage_scan_keeps_sizes_beyond_the_display_depth_cap` continues to hold).
#[allow(clippy::too_many_arguments)]
fn build_tree_parallel(
    path: &Path,
    name: OsStringDisplay,
    max_depth: u64,
    cancellation: &CancellationToken,
    unreadable: &UnreadableRegistry,
    scanned_entries: &AtomicU64,
    seen_hardlinks: &Mutex<HashSet<FileIdentity>>,
) -> Result<ScanTree, ApplicationError> {
    if cancellation.is_cancelled() {
        return Err(ApplicationError::OperationCancelled);
    }

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            unreadable.record(path, error.kind());
            return Ok(DataTree::dir(name, DiskUsageSize::default(), Vec::new()));
        }
    };
    scanned_entries.fetch_add(1, Ordering::Relaxed);
    let mut size = GetDiskUsageSize.get_size(&metadata);
    #[cfg(unix)]
    if size.kind == ScannedEntryKind::File {
        use std::os::unix::fs::MetadataExt;

        // Deduplicate while the inode metadata is already available. The dependency's tree-wide
        // post-pass filters every hardlink path at every retained node, which becomes pathological
        // for multi-million-entry trees.
        if metadata.nlink() > 1
            && !seen_hardlinks
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(FileIdentity {
                    device: metadata.dev(),
                    inode: metadata.ino(),
                })
        {
            size.logical_bytes = 0;
            size.physical_bytes = 0;
        }
    }
    #[cfg(not(unix))]
    {
        if size.kind == ScannedEntryKind::File
            && max_depth == 0
            && FileIdentity::of_path(path).is_some_and(|identity| {
                !seen_hardlinks
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .insert(identity)
            })
        {
            size.logical_bytes = 0;
            size.physical_bytes = 0;
        }
    }

    if size.kind != ScannedEntryKind::Directory {
        return Ok(DataTree::dir(name, size, Vec::new()));
    }

    let mut entry_names = Vec::new();
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries {
                if cancellation.is_cancelled() {
                    return Err(ApplicationError::OperationCancelled);
                }
                match entry {
                    Ok(entry) => entry_names.push(entry.file_name()),
                    // The failing entry's own name is unrecoverable from `io::Error` here, so the
                    // failure is attributed to the directory being iterated instead.
                    Err(error) => unreadable.record(path, error.kind()),
                }
            }
        }
        Err(error) => {
            unreadable.record(path, error.kind());
            return Ok(DataTree::dir(name, size, Vec::new()));
        }
    }
    entry_names.sort_unstable();

    let next_depth = max_depth.saturating_sub(1);
    let children = entry_names
        .into_par_iter()
        .map(|entry_name| {
            ensure_not_cancelled(cancellation)?;
            let child_path = path.join(&entry_name);
            build_tree_parallel(
                &child_path,
                OsStringDisplay::os_string_from(entry_name),
                next_depth,
                cancellation,
                unreadable,
                scanned_entries,
                seen_hardlinks,
            )
        })
        .collect::<Result<Vec<_>, ApplicationError>>()?;

    if next_depth > 0 {
        Ok(DataTree::dir(name, size, children))
    } else {
        let aggregated = children.iter().map(DataTree::size).sum();
        Ok(DataTree::dir(name, size + aggregated, Vec::new()))
    }
}

pub(crate) async fn scan_disk_usage(
    events: EventBus,
    request: ScanDiskUsageRequestDto,
    cancellation: CancellationToken,
) -> Result<ScanDiskUsageResponseDto, ApplicationError> {
    let location: Location = request.location.clone().into();
    if location.provider_id.as_str() != "local" {
        return Err(ApplicationError::InvalidRequest(
            "disk-usage analysis currently requires a local location".to_owned(),
        ));
    }
    let root = location
        .to_native_path()
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    let metadata = fs::symlink_metadata(&root).map_err(map_io_error)?;
    if !metadata.is_dir() {
        return Err(ApplicationError::InvalidRequest(
            "disk-usage analysis requires a directory".to_owned(),
        ));
    }

    tokio::task::spawn_blocking(move || scan_local_tree(root, request, events, cancellation))
        .await
        .map_err(|_| ApplicationError::Internal)?
}

fn scan_local_tree(
    root: PathBuf,
    request: ScanDiskUsageRequestDto,
    events: EventBus,
    cancellation: CancellationToken,
) -> Result<ScanDiskUsageResponseDto, ApplicationError> {
    let unreadable = UnreadableRegistry::default();
    let scanned_entries = AtomicU64::new(0);
    let root_size = GetDiskUsageSize.get_size(&fs::symlink_metadata(&root).map_err(map_io_error)?);
    let mut child_paths = Vec::new();
    for entry in fs::read_dir(&root).map_err(map_io_error)? {
        match entry {
            Ok(entry) => child_paths.push(entry.path()),
            // The failing entry's own name is unrecoverable from `io::Error` here, so the
            // failure is attributed to the root being iterated instead.
            Err(error) => unreadable.record(&root, error.kind()),
        }
    }
    child_paths.sort_unstable_by(|left, right| left.file_name().cmp(&right.file_name()));

    let audience = EventAudience::Workspace(request.workspace_id.into());
    if child_paths.is_empty() {
        let response = snapshot_response(
            &root,
            root_size,
            &[],
            request.expand_root,
            &unreadable,
            &scanned_entries,
            &cancellation,
        )?;
        publish_progress(&events, audience, request.scan_id, &response, true);
        return Ok(response);
    }

    let child_count = child_paths.len();
    let (sender, receiver) = mpsc::channel();
    let mut trees = (0..child_count).map(|_| None).collect::<Vec<_>>();
    let pool = disk_usage_thread_pool()?;
    let seen_hardlinks = Mutex::new(HashSet::new());

    std::thread::scope(|thread_scope| -> Result<(), ApplicationError> {
        let scan_sender = sender.clone();
        let unreadable_ref = &unreadable;
        let scanned_entries_ref = &scanned_entries;
        let cancellation_ref = &cancellation;
        let seen_hardlinks_ref = &seen_hardlinks;
        let pool_ref = &pool;
        thread_scope.spawn(move || {
            pool_ref.scope(|rayon_scope| {
                for (index, path) in child_paths.into_iter().enumerate() {
                    let sender = scan_sender.clone();
                    rayon_scope.spawn(move |_| {
                        let name = OsStringDisplay::os_string_from(
                            path.file_name().unwrap_or_else(|| path.as_os_str()),
                        );
                        let result = build_tree_parallel(
                            &path,
                            name,
                            MAX_SCAN_DEPTH.saturating_sub(1),
                            cancellation_ref,
                            unreadable_ref,
                            scanned_entries_ref,
                            seen_hardlinks_ref,
                        );
                        let _ = sender.send((index, result));
                    });
                }
            });
        });
        drop(sender);

        coordinate_progress(
            &receiver,
            &mut trees,
            &root,
            root_size,
            request.expand_root,
            &unreadable,
            &scanned_entries,
            &events,
            audience.clone(),
            request.scan_id,
            &cancellation,
        )
    })?;

    ensure_not_cancelled(&cancellation)?;
    let complete_scan_snapshot = snapshot_response(
        &root,
        root_size,
        &trees,
        request.expand_root,
        &unreadable,
        &scanned_entries,
        &cancellation,
    )?;
    publish_progress(
        &events,
        audience.clone(),
        request.scan_id,
        &complete_scan_snapshot,
        false,
    );
    events.publish(
        audience.clone(),
        BackendEventPayload::DiskUsageFinalizing {
            scan_id: request.scan_id,
            scanned_entries: scanned_entries.load(Ordering::Relaxed),
        },
    );
    let children = trees.into_iter().flatten().collect::<Vec<_>>();
    let tree = DataTree::dir(OsStringDisplay::os_string_from(&root), root_size, children);
    ensure_not_cancelled(&cancellation)?;
    #[cfg(unix)]
    let mut seen_hardlinks = HashSet::new();
    #[cfg(not(unix))]
    let mut seen_hardlinks = seen_hardlinks
        .into_inner()
        .unwrap_or_else(|error| error.into_inner());
    let mut root_node = map_node(
        &tree,
        &root,
        &mut seen_hardlinks,
        MapNodeOptions {
            is_root: true,
            expand_root: request.expand_root,
            deduplicate_hardlinks: cfg!(not(unix)),
            remaining_depth: MAX_RESPONSE_DEPTH,
        },
        &cancellation,
    )?;
    aggregate_excess_children(&mut root_node, &cancellation)?;
    ensure_not_cancelled(&cancellation)?;
    let response = ScanDiskUsageResponseDto {
        root: root_node,
        unreadable_entries: unreadable.count(),
        unreadable: unreadable.details(),
        scanned_entries: scanned_entries.load(Ordering::Relaxed),
    };
    publish_progress(&events, audience, request.scan_id, &response, true);
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn coordinate_progress(
    receiver: &mpsc::Receiver<ChildScanResult>,
    trees: &mut [Option<ScanTree>],
    root: &Path,
    root_size: DiskUsageSize,
    expand_root: bool,
    unreadable: &UnreadableRegistry,
    scanned_entries: &AtomicU64,
    events: &EventBus,
    audience: EventAudience,
    scan_id: uuid::Uuid,
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    let mut completed = 0;
    let mut emitted_tree = false;
    let mut interval_index = 0;
    let mut next_emission = Instant::now() + PROGRESS_INTERVALS[0];
    let mut cached_snapshot = None;
    let mut snapshot_dirty = false;

    while completed < trees.len() {
        if cancellation.is_cancelled() {
            return Err(ApplicationError::OperationCancelled);
        }
        // Always use a bounded wait: a large first subtree can take minutes, and visited-entry
        // progress plus cancellation must remain observable before any subtree completes.
        let received =
            receiver.recv_timeout(next_emission.saturating_duration_since(Instant::now()));
        match received {
            Ok((index, result)) => {
                let tree = result?;
                let useful = tree.size().kind != ScannedEntryKind::Aggregate;
                if useful {
                    trees[index] = Some(tree);
                }
                completed += 1;

                if !emitted_tree && useful {
                    let response = snapshot_response(
                        root,
                        root_size,
                        trees,
                        expand_root,
                        unreadable,
                        scanned_entries,
                        cancellation,
                    )?;
                    publish_progress(events, audience.clone(), scan_id, &response, false);
                    cached_snapshot = Some(response);
                    snapshot_dirty = false;
                    emitted_tree = true;
                    next_emission = Instant::now() + PROGRESS_INTERVALS[0];
                } else if useful {
                    snapshot_dirty = true;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let mut response = match (snapshot_dirty, cached_snapshot.clone()) {
                    (false, Some(response)) => response,
                    _ => snapshot_response(
                        root,
                        root_size,
                        trees,
                        expand_root,
                        unreadable,
                        scanned_entries,
                        cancellation,
                    )?,
                };
                response.unreadable_entries = unreadable.count();
                response.unreadable = unreadable.details();
                response.scanned_entries = scanned_entries.load(Ordering::Relaxed);
                publish_progress(events, audience.clone(), scan_id, &response, false);
                cached_snapshot = Some(response);
                snapshot_dirty = false;
                interval_index = (interval_index + 1).min(PROGRESS_INTERVALS.len() - 1);
                next_emission = Instant::now() + PROGRESS_INTERVALS[interval_index];
            }
            Err(RecvTimeoutError::Disconnected) => {
                if cancellation.is_cancelled() {
                    return Err(ApplicationError::OperationCancelled);
                }
                return Err(ApplicationError::Internal);
            }
        }
    }

    Ok(())
}

fn snapshot_response(
    root: &Path,
    root_size: DiskUsageSize,
    trees: &[Option<ScanTree>],
    expand_root: bool,
    unreadable: &UnreadableRegistry,
    scanned_entries: &AtomicU64,
    cancellation: &CancellationToken,
) -> Result<ScanDiskUsageResponseDto, ApplicationError> {
    let mut seen_hardlinks = HashSet::new();
    let mut children = Vec::new();
    for tree in trees.iter().flatten() {
        ensure_not_cancelled(cancellation)?;
        // Each top-level tree's own `name` is only its basename (`build_tree_parallel` never
        // sees the full path), so its absolute location must be rejoined against `root` here —
        // `map_node`'s `is_root` branch otherwise uses `path` as-is.
        children.push(map_node(
            tree,
            &root.join(tree.name().as_os_str()),
            &mut seen_hardlinks,
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: cfg!(not(unix)),
                remaining_depth: MAX_RESPONSE_DEPTH,
            },
            cancellation,
        )?);
    }
    let logical_bytes = root_size
        .logical_bytes
        .saturating_add(children.iter().map(|child| child.logical_bytes).sum());
    let physical_bytes = root_size
        .physical_bytes
        .saturating_add(children.iter().map(|child| child.physical_bytes).sum());
    let name = root
        .file_name()
        .unwrap_or(root.as_os_str())
        .to_string_lossy()
        .into_owned();
    let collapsed = is_collapsed_name(&name) && !expand_root;
    let mut root_node = DiskUsageNodeDto {
        name,
        location: Location::from_native_path(root)
            .map_err(|_| ApplicationError::Internal)?
            .into(),
        kind: DiskUsageNodeKindDto::Directory,
        logical_bytes,
        physical_bytes,
        collapsed,
        children: if collapsed { Vec::new() } else { children },
    };
    fit_child_totals(&mut root_node);
    aggregate_excess_children(&mut root_node, cancellation)?;
    Ok(ScanDiskUsageResponseDto {
        root: root_node,
        unreadable_entries: unreadable.count(),
        unreadable: unreadable.details(),
        scanned_entries: scanned_entries.load(Ordering::Relaxed),
    })
}

fn publish_progress(
    events: &EventBus,
    audience: EventAudience,
    scan_id: uuid::Uuid,
    response: &ScanDiskUsageResponseDto,
    is_complete: bool,
) {
    events.publish(
        audience,
        BackendEventPayload::DiskUsageProgress {
            scan_id,
            root: event_node(&response.root),
            unreadable_entries: response.unreadable_entries,
            unreadable: response.unreadable.iter().map(event_unreadable).collect(),
            scanned_entries: response.scanned_entries,
            is_complete,
        },
    );
}

fn event_unreadable(entry: &DiskUsageUnreadableEntryDto) -> DiskUsageUnreadableEntryPayload {
    DiskUsageUnreadableEntryPayload {
        location: LocationPayload {
            provider_id: fm_domain::ProviderId::new(entry.location.provider_id.clone()),
            uri: entry.location.uri.clone(),
        },
        reason: match entry.reason {
            DiskUsageUnreadableReasonDto::PermissionDenied => {
                DiskUsageUnreadableReasonPayload::PermissionDenied
            }
            DiskUsageUnreadableReasonDto::Disappeared => {
                DiskUsageUnreadableReasonPayload::Disappeared
            }
            DiskUsageUnreadableReasonDto::IoError => DiskUsageUnreadableReasonPayload::IoError,
        },
    }
}

pub(crate) fn event_node(node: &DiskUsageNodeDto) -> DiskUsageNodePayload {
    DiskUsageNodePayload {
        name: node.name.clone(),
        location: LocationPayload {
            provider_id: fm_domain::ProviderId::new(node.location.provider_id.clone()),
            uri: node.location.uri.clone(),
        },
        kind: match node.kind {
            DiskUsageNodeKindDto::Directory => DiskUsageNodeKindPayload::Directory,
            DiskUsageNodeKindDto::File => DiskUsageNodeKindPayload::File,
            DiskUsageNodeKindDto::Symlink => DiskUsageNodeKindPayload::Symlink,
        },
        logical_bytes: node.logical_bytes,
        physical_bytes: node.physical_bytes,
        collapsed: node.collapsed,
        children: node.children.iter().map(event_node).collect(),
    }
}

fn map_node(
    tree: &DataTree<OsStringDisplay, DiskUsageSize>,
    path: &Path,
    seen_hardlinks: &mut HashSet<FileIdentity>,
    options: MapNodeOptions,
    cancellation: &CancellationToken,
) -> Result<DiskUsageNodeDto, ApplicationError> {
    ensure_not_cancelled(cancellation)?;
    let node_path = if options.is_root {
        path.to_owned()
    } else {
        path.join(tree.name().as_os_str())
    };
    let kind = match tree.size().kind {
        ScannedEntryKind::Directory => DiskUsageNodeKindDto::Directory,
        ScannedEntryKind::File => DiskUsageNodeKindDto::File,
        ScannedEntryKind::Symlink => DiskUsageNodeKindDto::Symlink,
        ScannedEntryKind::Aggregate => return Err(ApplicationError::Internal),
    };
    let name = if options.is_root {
        node_path
            .file_name()
            .unwrap_or_else(|| node_path.as_os_str())
            .to_string_lossy()
            .into_owned()
    } else {
        tree.name().to_string()
    };
    let collapsed = kind == DiskUsageNodeKindDto::Directory
        && ((is_collapsed_name(&name) && !(options.is_root && options.expand_root))
            || options.remaining_depth == 0);
    if collapsed {
        let (logical_bytes, physical_bytes) = if options.deduplicate_hardlinks {
            deduplicate_collapsed_size(tree, &node_path, seen_hardlinks, cancellation)?
        } else {
            (tree.size().logical_bytes, tree.size().physical_bytes)
        };
        return Ok(DiskUsageNodeDto {
            name,
            location: Location::from_native_path(&node_path)
                .map_err(|_| ApplicationError::Internal)?
                .into(),
            kind,
            logical_bytes,
            physical_bytes,
            collapsed: true,
            children: Vec::new(),
        });
    }
    let mut children = Vec::with_capacity(tree.children().len());
    for child in tree.children() {
        if child.size().kind == ScannedEntryKind::Aggregate {
            continue;
        }
        children.push(map_node(
            child,
            &node_path,
            seen_hardlinks,
            MapNodeOptions {
                is_root: false,
                expand_root: false,
                deduplicate_hardlinks: options.deduplicate_hardlinks,
                remaining_depth: options.remaining_depth.saturating_sub(1),
            },
            cancellation,
        )?);
    }
    children.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    let (logical_bytes, physical_bytes) = if kind == DiskUsageNodeKindDto::Directory {
        let raw_children = tree
            .children()
            .iter()
            .map(DataTree::size)
            .sum::<DiskUsageSize>();
        let raw_total = tree.size();
        let logical_total = tree
            .size()
            .logical_bytes
            .saturating_sub(raw_children.logical_bytes)
            .saturating_add(children.iter().map(|child| child.logical_bytes).sum())
            .min(raw_total.logical_bytes);
        let physical_total = raw_total
            .physical_bytes
            .saturating_sub(raw_children.physical_bytes)
            .saturating_add(children.iter().map(|child| child.physical_bytes).sum())
            .min(raw_total.physical_bytes);
        (logical_total, physical_total)
    } else if kind == DiskUsageNodeKindDto::File && options.deduplicate_hardlinks {
        deduplicate_file_sizes(
            &node_path,
            tree.size().logical_bytes,
            tree.size().physical_bytes,
            seen_hardlinks,
        )
    } else {
        (tree.size().logical_bytes, tree.size().physical_bytes)
    };
    let location = Location::from_native_path(&node_path)
        .map_err(|_| ApplicationError::Internal)?
        .into();
    let mut node = DiskUsageNodeDto {
        name,
        location,
        kind,
        logical_bytes,
        physical_bytes,
        collapsed,
        children,
    };
    fit_child_totals(&mut node);
    Ok(node)
}

fn deduplicate_collapsed_size(
    tree: &ScanTree,
    node_path: &Path,
    seen_hardlinks: &mut HashSet<FileIdentity>,
    cancellation: &CancellationToken,
) -> Result<(u64, u64), ApplicationError> {
    ensure_not_cancelled(cancellation)?;
    if tree.size().kind == ScannedEntryKind::File {
        return Ok(deduplicate_file_sizes(
            node_path,
            tree.size().logical_bytes,
            tree.size().physical_bytes,
            seen_hardlinks,
        ));
    }
    if tree.size().kind != ScannedEntryKind::Directory {
        return Ok((tree.size().logical_bytes, tree.size().physical_bytes));
    }

    let raw_children = tree
        .children()
        .iter()
        .map(DataTree::size)
        .sum::<DiskUsageSize>();
    let mut logical_children = 0_u64;
    let mut physical_children = 0_u64;
    for child in tree.children() {
        let child_path = node_path.join(child.name().as_os_str());
        let (logical_bytes, physical_bytes) =
            deduplicate_collapsed_size(child, &child_path, seen_hardlinks, cancellation)?;
        logical_children = logical_children.saturating_add(logical_bytes);
        physical_children = physical_children.saturating_add(physical_bytes);
    }
    Ok((
        tree.size()
            .logical_bytes
            .saturating_sub(raw_children.logical_bytes)
            .saturating_add(logical_children)
            .min(tree.size().logical_bytes),
        tree.size()
            .physical_bytes
            .saturating_sub(raw_children.physical_bytes)
            .saturating_add(physical_children)
            .min(tree.size().physical_bytes),
    ))
}

fn is_collapsed_name(name: &str) -> bool {
    matches!(name, ".git" | ".hg" | ".svn" | "node_modules")
}

fn fit_child_totals(node: &mut DiskUsageNodeDto) {
    let mut logical_overflow = node
        .children
        .iter()
        .map(|child| child.logical_bytes)
        .sum::<u64>()
        .saturating_sub(node.logical_bytes);
    for child in node.children.iter_mut().rev() {
        let reduction = logical_overflow.min(child.logical_bytes);
        child.logical_bytes -= reduction;
        logical_overflow -= reduction;
        fit_logical_children(child);
    }

    let mut physical_overflow = node
        .children
        .iter()
        .map(|child| child.physical_bytes)
        .sum::<u64>()
        .saturating_sub(node.physical_bytes);
    for child in node.children.iter_mut().rev() {
        let reduction = physical_overflow.min(child.physical_bytes);
        child.physical_bytes -= reduction;
        physical_overflow -= reduction;
        fit_physical_children(child);
    }
}

fn fit_logical_children(node: &mut DiskUsageNodeDto) {
    let mut overflow = node
        .children
        .iter()
        .map(|child| child.logical_bytes)
        .sum::<u64>()
        .saturating_sub(node.logical_bytes);
    for child in node.children.iter_mut().rev() {
        let reduction = overflow.min(child.logical_bytes);
        child.logical_bytes -= reduction;
        overflow -= reduction;
        fit_logical_children(child);
    }
}

fn fit_physical_children(node: &mut DiskUsageNodeDto) {
    let mut overflow = node
        .children
        .iter()
        .map(|child| child.physical_bytes)
        .sum::<u64>()
        .saturating_sub(node.physical_bytes);
    for child in node.children.iter_mut().rev() {
        let reduction = overflow.min(child.physical_bytes);
        child.physical_bytes -= reduction;
        overflow -= reduction;
        fit_physical_children(child);
    }
}

fn deduplicate_file_sizes(
    path: &Path,
    logical_bytes: u64,
    physical_bytes: u64,
    seen_hardlinks: &mut HashSet<FileIdentity>,
) -> (u64, u64) {
    let Some(identity) = FileIdentity::of_path(path) else {
        return (logical_bytes, physical_bytes);
    };
    if seen_hardlinks.insert(identity) {
        (logical_bytes, physical_bytes)
    } else {
        (0, 0)
    }
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), ApplicationError> {
    if cancellation.is_cancelled() {
        Err(ApplicationError::OperationCancelled)
    } else {
        Ok(())
    }
}

fn aggregate_excess_children(
    node: &mut DiskUsageNodeDto,
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    ensure_not_cancelled(cancellation)?;
    for child in &mut node.children {
        aggregate_excess_children(child, cancellation)?;
    }
    if node.children.len() <= MAX_CHILDREN_PER_DIRECTORY {
        return Ok(());
    }

    node.children.sort_unstable_by(|left, right| {
        right
            .physical_bytes
            .cmp(&left.physical_bytes)
            .then_with(|| right.logical_bytes.cmp(&left.logical_bytes))
            .then_with(|| left.name.cmp(&right.name))
    });
    let omitted = node.children.split_off(MAX_CHILDREN_PER_DIRECTORY - 1);
    let omitted_count = omitted.len();
    node.children.push(DiskUsageNodeDto {
        name: format!("Small files ({omitted_count})"),
        location: node.location.clone(),
        kind: DiskUsageNodeKindDto::File,
        logical_bytes: omitted.iter().map(|child| child.logical_bytes).sum(),
        physical_bytes: omitted.iter().map(|child| child.physical_bytes).sum(),
        collapsed: false,
        children: Vec::new(),
    });
    node.children
        .sort_unstable_by(|left, right| left.name.cmp(&right.name));
    Ok(())
}

fn map_io_error(error: std::io::Error) -> ApplicationError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ApplicationError::NotFound,
        std::io::ErrorKind::PermissionDenied => ApplicationError::PermissionDenied,
        _ => ApplicationError::Internal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_usage_pool_has_a_fixed_worker_cap() {
        let pool = disk_usage_thread_pool().expect("create disk usage pool");
        assert_eq!(DISK_USAGE_WORKER_COUNT, 4);
        assert_eq!(pool.current_num_threads(), DISK_USAGE_WORKER_COUNT);
    }

    #[cfg(unix)]
    #[test]
    fn build_tree_parallel_deduplicates_hardlinks_during_traversal() {
        let root = tempfile::tempdir().expect("create fixture root");
        let original = root.path().join("original.bin");
        fs::write(&original, [7_u8; 17]).expect("write fixture file");
        fs::hard_link(&original, root.path().join("duplicate.bin")).expect("create hardlink");
        let cancellation = CancellationToken::new();
        let scanned_entries = AtomicU64::new(0);
        let unreadable = UnreadableRegistry::default();
        let seen_hardlinks = Mutex::new(HashSet::new());

        let tree = disk_usage_thread_pool()
            .expect("create disk usage pool")
            .install(|| {
                build_tree_parallel(
                    root.path(),
                    OsStringDisplay::os_string_from(root.path()),
                    MAX_SCAN_DEPTH,
                    &cancellation,
                    &unreadable,
                    &scanned_entries,
                    &seen_hardlinks,
                )
            })
            .expect("scan fixture");
        let file_bytes = tree
            .children()
            .iter()
            .filter(|child| child.size().kind == ScannedEntryKind::File)
            .map(|child| child.size().logical_bytes)
            .sum::<u64>();

        assert_eq!(file_bytes, 17);
    }

    /// `build_tree_parallel` must check `cancellation` at every entry/directory it visits (not
    /// just once at the top), so a scan over a large fixture stops promptly rather than running
    /// to completion once cancelled mid-traversal. A background thread watches
    /// `scanned_entries` and cancels once a small threshold is crossed, well before the fixture's
    /// 20,000 entries could all be visited; the sequential traversal itself has no artificial
    /// delay, but its own bookkeeping (one `symlink_metadata`/`read_dir` syscall per entry) is
    /// slow enough relative to the watcher's tight spin loop that cancellation reliably lands
    /// mid-scan.
    #[test]
    fn build_tree_parallel_cancellation_interrupts_a_large_deterministic_fixture() {
        let root = tempfile::tempdir().expect("create fixture root");
        for file_index in 0..20_000 {
            fs::write(root.path().join(format!("file-{file_index:05}.txt")), b"x")
                .expect("write fixture file");
        }
        let cancellation = CancellationToken::new();
        let scanned_entries = AtomicU64::new(0);
        let unreadable = UnreadableRegistry::default();
        let seen_hardlinks = Mutex::new(HashSet::new());

        let result = std::thread::scope(|scope| {
            let watcher_cancellation = cancellation.clone();
            let watcher_scanned_entries = &scanned_entries;
            scope.spawn(move || {
                while watcher_scanned_entries.load(Ordering::Relaxed) < 50 {
                    std::hint::spin_loop();
                }
                watcher_cancellation.cancel();
            });

            disk_usage_thread_pool()
                .expect("create disk usage pool")
                .install(|| {
                    build_tree_parallel(
                        root.path(),
                        OsStringDisplay::os_string_from(root.path()),
                        MAX_SCAN_DEPTH,
                        &cancellation,
                        &unreadable,
                        &scanned_entries,
                        &seen_hardlinks,
                    )
                })
        });

        assert!(matches!(result, Err(ApplicationError::OperationCancelled)));
        assert!(
            scanned_entries.load(Ordering::Relaxed) < 20_000,
            "expected cancellation to interrupt traversal well before it visited every entry"
        );
    }

    #[test]
    fn unreadable_registry_reports_count_and_sorted_bounded_details() {
        let registry = UnreadableRegistry::default();
        let root = tempfile::tempdir().expect("create fixture root");
        let missing_b = root.path().join("b-missing");
        let missing_a = root.path().join("a-missing");
        // Recorded out of order to prove `details()` stable-sorts by location rather than by
        // insertion order.
        registry.record(&missing_b, std::io::ErrorKind::PermissionDenied);
        registry.record(&missing_a, std::io::ErrorKind::NotFound);

        assert_eq!(registry.count(), 2);
        let details = registry.details();
        assert_eq!(details.len(), 2);
        assert!(details[0].location.uri.ends_with("a-missing"));
        assert_eq!(details[0].reason, DiskUsageUnreadableReasonDto::Disappeared);
        assert!(details[1].location.uri.ends_with("b-missing"));
        assert_eq!(
            details[1].reason,
            DiskUsageUnreadableReasonDto::PermissionDenied
        );
    }

    #[test]
    fn unreadable_registry_caps_details_but_keeps_the_full_count() {
        let registry = UnreadableRegistry::default();
        let root = tempfile::tempdir().expect("create fixture root");
        for index in 0..(MAX_UNREADABLE_DETAILS + 10) {
            registry.record(
                &root.path().join(format!("missing-{index}")),
                std::io::ErrorKind::NotFound,
            );
        }

        assert_eq!(registry.count(), (MAX_UNREADABLE_DETAILS + 10) as u64);
        assert_eq!(registry.details().len(), MAX_UNREADABLE_DETAILS);
    }

    /// Repeated progress snapshots must be able to show `scanned_entries` advancing even while no
    /// additional top-level subtree has completed (i.e. `trees` is unchanged between snapshots) —
    /// this is what lets the UI stop looking stuck on "Updating" for a long-running top-level
    /// subtree.
    #[test]
    fn snapshot_response_scanned_entries_advances_without_a_new_completed_subtree() {
        let root = tempfile::tempdir().expect("create fixture root");
        let unreadable = UnreadableRegistry::default();
        let scanned_entries = AtomicU64::new(3);
        let trees: [Option<ScanTree>; 0] = [];
        let cancellation = CancellationToken::new();

        let first = snapshot_response(
            root.path(),
            DiskUsageSize::default(),
            &trees,
            false,
            &unreadable,
            &scanned_entries,
            &cancellation,
        )
        .expect("first snapshot");
        scanned_entries.fetch_add(5, Ordering::Relaxed);
        let second = snapshot_response(
            root.path(),
            DiskUsageSize::default(),
            &trees,
            false,
            &unreadable,
            &scanned_entries,
            &cancellation,
        )
        .expect("second snapshot");

        assert_eq!(first.scanned_entries, 3);
        assert_eq!(second.scanned_entries, 8);
        assert_eq!(first.root.children.len(), second.root.children.len());
    }

    #[test]
    fn mapping_skips_entries_that_disappear_during_the_scan() {
        let root = tempfile::tempdir().expect("create fixture root");
        let tree = DataTree::dir(
            OsStringDisplay::os_string_from(root.path()),
            DiskUsageSize {
                kind: ScannedEntryKind::Directory,
                ..DiskUsageSize::default()
            },
            vec![DataTree::file(
                OsStringDisplay::os_string_from("gone"),
                DiskUsageSize::default(),
            )],
        );

        let mapped = map_node(
            &tree,
            root.path(),
            &mut HashSet::new(),
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: true,
                remaining_depth: MAX_RESPONSE_DEPTH,
            },
            &CancellationToken::new(),
        )
        .expect("an unreadable child must not fail the scan");

        assert!(mapped.children.is_empty());
    }

    #[test]
    fn mapping_collapses_at_the_response_depth_without_remapping_descendants() {
        let root = tempfile::tempdir().expect("create fixture root");
        let tree = DataTree::dir(
            OsStringDisplay::os_string_from(root.path()),
            DiskUsageSize {
                logical_bytes: 4_096,
                physical_bytes: 4_096,
                kind: ScannedEntryKind::Directory,
            },
            vec![DataTree::file(
                OsStringDisplay::os_string_from("already-counted.bin"),
                DiskUsageSize {
                    logical_bytes: 4_096,
                    physical_bytes: 4_096,
                    kind: ScannedEntryKind::File,
                },
            )],
        );

        let mapped = map_node(
            &tree,
            root.path(),
            &mut HashSet::new(),
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: true,
                remaining_depth: 0,
            },
            &CancellationToken::new(),
        )
        .expect("collapsed mapping");

        assert!(mapped.collapsed);
        assert!(mapped.children.is_empty());
        assert_eq!(mapped.physical_bytes, tree.size().physical_bytes);
    }

    #[test]
    fn collapsed_mapping_still_deduplicates_hidden_hardlinks() {
        let root = tempfile::tempdir().expect("create fixture root");
        let original = root.path().join("original.bin");
        let duplicate = root.path().join("duplicate.bin");
        fs::write(&original, [7_u8; 17]).expect("write fixture file");
        fs::hard_link(&original, &duplicate).expect("create fixture hardlink");
        let file_size = DiskUsageSize {
            logical_bytes: 17,
            physical_bytes: 17,
            kind: ScannedEntryKind::File,
        };
        let tree = DataTree::dir(
            OsStringDisplay::os_string_from(root.path()),
            DiskUsageSize {
                logical_bytes: 0,
                physical_bytes: 0,
                kind: ScannedEntryKind::Directory,
            },
            vec![
                DataTree::file(OsStringDisplay::os_string_from("duplicate.bin"), file_size),
                DataTree::file(OsStringDisplay::os_string_from("original.bin"), file_size),
            ],
        );

        let mapped = map_node(
            &tree,
            root.path(),
            &mut HashSet::new(),
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: true,
                remaining_depth: 0,
            },
            &CancellationToken::new(),
        )
        .expect("collapsed mapping");

        assert!(mapped.collapsed);
        assert!(mapped.children.is_empty());
        assert_eq!(mapped.logical_bytes, 17);
        assert_eq!(mapped.physical_bytes, 17);
    }

    #[test]
    fn progress_cadence_grows_and_caps_at_four_seconds() {
        assert_eq!(
            PROGRESS_INTERVALS,
            [
                Duration::from_millis(250),
                Duration::from_millis(500),
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4),
            ]
        );
    }

    #[test]
    fn mapping_orders_children_recursively_by_name() {
        let directory_size = DiskUsageSize {
            kind: ScannedEntryKind::Directory,
            ..DiskUsageSize::default()
        };
        let root = tempfile::tempdir().expect("create fixture root");
        let tree = DataTree::dir(
            OsStringDisplay::os_string_from(root.path()),
            directory_size,
            vec![
                DataTree::dir(
                    OsStringDisplay::os_string_from("z-directory"),
                    directory_size,
                    vec![
                        DataTree::dir(
                            OsStringDisplay::os_string_from("beta"),
                            directory_size,
                            Vec::new(),
                        ),
                        DataTree::dir(
                            OsStringDisplay::os_string_from("alpha"),
                            directory_size,
                            Vec::new(),
                        ),
                    ],
                ),
                DataTree::dir(
                    OsStringDisplay::os_string_from("a-directory"),
                    directory_size,
                    Vec::new(),
                ),
            ],
        );

        let mapped = map_node(
            &tree,
            root.path(),
            &mut HashSet::new(),
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: true,
                remaining_depth: MAX_RESPONSE_DEPTH,
            },
            &CancellationToken::new(),
        )
        .expect("map tree");

        assert_eq!(
            mapped
                .children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["a-directory", "z-directory"]
        );
        assert_eq!(
            mapped.children[1]
                .children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
    }
}
