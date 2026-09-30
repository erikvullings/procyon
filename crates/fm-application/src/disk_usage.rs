//! Parallel local disk-usage scanning for the WinDirStat-style treemap (task 0118).

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
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
    BackendEventPayload, DiskUsageCleanupCandidatePayload, DiskUsageCleanupKindPayload,
    DiskUsageTreePayload, DiskUsageUnreadableEntryPayload, DiskUsageUnreadableReasonPayload,
    EventAudience, EventBus, LocationPayload,
};
use fm_transport_dto::{
    DiskUsageCleanupCandidateDto, DiskUsageCleanupKindDto, DiskUsageNodeDto, DiskUsageNodeKindDto,
    DiskUsageTreeDto, DiskUsageUnreadableEntryDto, DiskUsageUnreadableReasonDto,
    ScanDiskUsageRequestDto, ScanDiskUsageResponseDto,
};
use parallel_disk_usage::data_tree::DataTree;
use parallel_disk_usage::get_size::GetSize;
use parallel_disk_usage::os_string_display::OsStringDisplay;
use parallel_disk_usage::size::Size;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use tokio_util::sync::CancellationToken;

use crate::ApplicationError;
use crate::disk_usage_cleanup::{
    CleanupCandidate, CleanupKind, CleanupTreeNode, MIN_CLEANUP_BYTES, find_cleanup_candidates,
};

const MAX_SCAN_DEPTH: u64 = 12;
/// The UI only renders a few nested levels and can explicitly rescan any collapsed directory.
/// Capping the response depth avoids remapping and serializing millions of already-counted leaf
/// nodes after traversal has finished. Five levels in the flat event encoding (task 0233) is
/// still smaller than four levels were as nested JSON on a real `~/Library` scan.
const MAX_RESPONSE_DEPTH: u64 = 5;
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

impl CleanupTreeNode for ScanTree {
    fn entry_name(&self) -> &std::ffi::OsStr {
        self.name().as_os_str()
    }
    fn is_directory(&self) -> bool {
        self.size().kind == ScannedEntryKind::Directory
    }
    fn logical_bytes(&self) -> u64 {
        self.size().logical_bytes
    }
    fn physical_bytes(&self) -> u64 {
        self.size().physical_bytes
    }
    fn child_nodes(&self) -> &[Self] {
        self.children()
    }
}

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
    /// A directory on a different device than the scan root (a mount point). A scan measures one
    /// volume, so it is counted as an entry but never descended.
    OtherVolume,
    /// A directory whose contents live only in the cloud (macOS `SF_DATALESS`). Listing it would
    /// ask the file provider to download it, so it is never descended.
    CloudOnly,
    /// A child whose name this host cannot represent as a location. Its bytes stay in the parent's
    /// total; the entry is recorded against the parent directory.
    UnsupportedName,
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
            UnreadableReason::OtherVolume => Self::OtherVolume,
            UnreadableReason::CloudOnly => Self::CloudOnly,
            UnreadableReason::UnsupportedName => Self::UnsupportedName,
        }
    }
}

impl From<UnreadableReason> for DiskUsageUnreadableReasonPayload {
    fn from(reason: UnreadableReason) -> Self {
        match reason {
            UnreadableReason::PermissionDenied => Self::PermissionDenied,
            UnreadableReason::Disappeared => Self::Disappeared,
            UnreadableReason::IoError => Self::IoError,
            UnreadableReason::OtherVolume => Self::OtherVolume,
            UnreadableReason::CloudOnly => Self::CloudOnly,
            UnreadableReason::UnsupportedName => Self::UnsupportedName,
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
        self.record_reason(path, UnreadableReason::from_error_kind(kind));
    }

    fn record_reason(&self, path: &Path, reason: UnreadableReason) {
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut details = self
            .details
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if details.len() < MAX_UNREADABLE_DETAILS {
            details.push(UnreadableEntry {
                path: path.to_owned(),
                reason,
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

/// macOS `SF_DATALESS`: the directory's contents live in the cloud (iCloud Drive, File Provider).
/// Other platforms report no BSD flags, so the check never matches there.
const SF_DATALESS: u32 = 0x4000_0000;

/// Where a scan stops descending, fixed once from the scan root.
#[derive(Debug, Clone, Copy, Default)]
struct ScanBoundary {
    /// Device of the scan root; directories on any other device are not descended.
    root_device: Option<u64>,
}

impl ScanBoundary {
    fn for_root(metadata: &fs::Metadata) -> Self {
        Self {
            root_device: device_of(metadata),
        }
    }

    /// Why a directory must not be descended, if it crosses the scan's boundary.
    fn skip_reason(self, info: &EntryInfo) -> Option<UnreadableReason> {
        boundary_skip_reason(self.root_device, info.device, info.bsd_flags)
    }
}

/// What the scanner needs to know about one entry, from `lstat` or a bulk directory listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EntryInfo {
    size: DiskUsageSize,
    device: Option<u64>,
    bsd_flags: u32,
    /// `(device, inode)` of a file with more than one hard link, on Unix.
    shared_identity: Option<FileIdentity>,
}

impl EntryInfo {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        let size = GetDiskUsageSize.get_size(metadata);
        #[cfg(unix)]
        let shared_identity = {
            use std::os::unix::fs::MetadataExt;

            (size.kind == ScannedEntryKind::File && metadata.nlink() > 1).then(|| FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        };
        #[cfg(not(unix))]
        let shared_identity = None;
        Self {
            size,
            device: device_of(metadata),
            bsd_flags: bsd_flags_of(metadata),
            shared_identity,
        }
    }

    #[cfg(target_os = "macos")]
    fn from_bulk(attributes: &fm_vfs_local::bulk_listing::BulkAttributes) -> Self {
        use fm_vfs_local::bulk_listing::BulkEntryKind;

        let kind = match attributes.kind {
            BulkEntryKind::Directory => ScannedEntryKind::Directory,
            BulkEntryKind::Symlink => ScannedEntryKind::Symlink,
            BulkEntryKind::Other => ScannedEntryKind::File,
        };
        Self {
            size: DiskUsageSize {
                logical_bytes: attributes.logical_bytes,
                physical_bytes: attributes.physical_bytes,
                kind,
            },
            device: Some(attributes.device),
            bsd_flags: attributes.bsd_flags,
            shared_identity: (kind == ScannedEntryKind::File && attributes.link_count > 1)
                .then_some(FileIdentity {
                    device: attributes.device,
                    inode: attributes.file_id,
                }),
        }
    }
}

/// One directory entry; `info` is `None` when the listing didn't provide metadata and the entry
/// must be `lstat`ed.
struct ListedEntry {
    name: OsString,
    info: Option<EntryInfo>,
}

/// How directories are listed. macOS uses `getattrlistbulk`, which returns each batch of entries
/// with their metadata and so avoids one `lstat` per entry; elsewhere, and as the reference
/// implementation, `read_dir` is followed by `lstat` per entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectoryLister {
    #[cfg_attr(
        all(target_os = "macos", not(test)),
        expect(
            dead_code,
            reason = "macOS lists with Bulk; Portable is its parity reference"
        )
    )]
    Portable,
    #[cfg(target_os = "macos")]
    Bulk,
}

impl DirectoryLister {
    const fn native() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::Bulk
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::Portable
        }
    }

    /// Lists `path`. Failures of individual entries are recorded against `path`, because the
    /// entry's own name can't be recovered from them.
    fn list(self, path: &Path, unreadable: &UnreadableRegistry) -> io::Result<Vec<ListedEntry>> {
        match self {
            Self::Portable => {
                let mut entries = Vec::new();
                for entry in fs::read_dir(path)? {
                    match entry {
                        Ok(entry) => entries.push(ListedEntry {
                            name: entry.file_name(),
                            info: None,
                        }),
                        Err(error) => unreadable.record(path, error.kind()),
                    }
                }
                Ok(entries)
            }
            #[cfg(target_os = "macos")]
            Self::Bulk => {
                let listing = fm_vfs_local::bulk_listing::list_directory_bulk(path)?;
                for kind in listing.failures {
                    unreadable.record(path, kind);
                }
                Ok(listing
                    .entries
                    .into_iter()
                    .map(|entry| ListedEntry {
                        name: entry.name,
                        // A mount point's listed attributes describe the covered directory, so
                        // it is `lstat`ed like the portable path to see the mounted volume.
                        info: entry
                            .attributes
                            .filter(|attributes| !attributes.mount_point)
                            .map(|attributes| EntryInfo::from_bulk(&attributes)),
                    })
                    .collect())
            }
        }
    }
}

/// Pure boundary decision: cloud-only contents first (never trigger a download), then mounts.
fn boundary_skip_reason(
    root_device: Option<u64>,
    device: Option<u64>,
    bsd_flags: u32,
) -> Option<UnreadableReason> {
    if bsd_flags & SF_DATALESS != 0 {
        return Some(UnreadableReason::CloudOnly);
    }
    match (root_device, device) {
        (Some(root), Some(device)) if root != device => Some(UnreadableReason::OtherVolume),
        _ => None,
    }
}

#[cfg(unix)]
fn device_of(metadata: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;

    Some(metadata.dev())
}

/// Windows mount points are junctions, which `symlink_metadata` already reports as symlinks and
/// the scan never follows.
#[cfg(not(unix))]
fn device_of(_metadata: &fs::Metadata) -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
fn bsd_flags_of(metadata: &fs::Metadata) -> u32 {
    use std::os::macos::fs::MetadataExt;

    metadata.st_flags()
}

#[cfg(not(target_os = "macos"))]
fn bsd_flags_of(_metadata: &fs::Metadata) -> u32 {
    0
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
    listed_info: Option<EntryInfo>,
    max_depth: u64,
    cancellation: &CancellationToken,
    unreadable: &UnreadableRegistry,
    scanned_entries: &AtomicU64,
    seen_hardlinks: &Mutex<HashSet<FileIdentity>>,
    boundary: ScanBoundary,
    lister: DirectoryLister,
) -> Result<ScanTree, ApplicationError> {
    if cancellation.is_cancelled() {
        return Err(ApplicationError::OperationCancelled);
    }

    let info = match listed_info {
        Some(info) => info,
        None => match fs::symlink_metadata(path) {
            Ok(metadata) => EntryInfo::from_metadata(&metadata),
            Err(error) => {
                unreadable.record(path, error.kind());
                return Ok(DataTree::dir(name, DiskUsageSize::default(), Vec::new()));
            }
        },
    };
    scanned_entries.fetch_add(1, Ordering::Relaxed);
    let mut size = info.size;
    // Deduplicate while the inode identity is already available. The dependency's tree-wide
    // post-pass filters every hardlink path at every retained node, which becomes pathological
    // for multi-million-entry trees.
    if let Some(identity) = info.shared_identity
        && !seen_hardlinks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(identity)
    {
        size.logical_bytes = 0;
        size.physical_bytes = 0;
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
    if let Some(reason) = boundary.skip_reason(&info) {
        unreadable.record_reason(path, reason);
        return Ok(DataTree::dir(name, size, Vec::new()));
    }

    let mut entries = match lister.list(path, unreadable) {
        Ok(entries) => entries,
        Err(error) => {
            unreadable.record(path, error.kind());
            return Ok(DataTree::dir(name, size, Vec::new()));
        }
    };
    ensure_not_cancelled(cancellation)?;
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));

    let next_depth = max_depth.saturating_sub(1);
    let children = entries
        .into_par_iter()
        .map(|entry| {
            ensure_not_cancelled(cancellation)?;
            let child_path = path.join(&entry.name);
            build_tree_parallel(
                &child_path,
                OsStringDisplay::os_string_from(entry.name),
                entry.info,
                next_depth,
                cancellation,
                unreadable,
                scanned_entries,
                seen_hardlinks,
                boundary,
                lister,
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
    let root_metadata = fs::symlink_metadata(&root).map_err(map_io_error)?;
    let root_info = EntryInfo::from_metadata(&root_metadata);
    let root_size = root_info.size;
    let boundary = ScanBoundary::for_root(&root_metadata);
    let lister = DirectoryLister::native();
    // The root is on its own device by definition; only its cloud-only state can stop the scan.
    let mut child_entries = if boundary.skip_reason(&root_info).is_some() {
        unreadable.record_reason(&root, UnreadableReason::CloudOnly);
        Vec::new()
    } else {
        lister.list(&root, &unreadable).map_err(map_io_error)?
    };
    child_entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));

    let audience = EventAudience::Workspace(request.workspace_id.into());
    if child_entries.is_empty() {
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

    let child_count = child_entries.len();
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
        let root_ref = root.as_path();
        thread_scope.spawn(move || {
            pool_ref.scope(|rayon_scope| {
                for (index, entry) in child_entries.into_iter().enumerate() {
                    let sender = scan_sender.clone();
                    let path = root_ref.join(&entry.name);
                    rayon_scope.spawn(move |_| {
                        let result = build_tree_parallel(
                            &path,
                            OsStringDisplay::os_string_from(entry.name),
                            entry.info,
                            MAX_SCAN_DEPTH.saturating_sub(1),
                            cancellation_ref,
                            unreadable_ref,
                            scanned_entries_ref,
                            seen_hardlinks_ref,
                            boundary,
                            lister,
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
    let cleanup_candidates =
        find_cleanup_candidates(&tree, &root, MIN_CLEANUP_BYTES, &cancellation)?
            .iter()
            .filter_map(cleanup_candidate_dto)
            .collect::<Vec<_>>();
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
        Some(&unreadable),
        &cancellation,
    )?
    .ok_or(ApplicationError::Internal)?;
    aggregate_excess_children(&mut root_node, &cancellation)?;
    ensure_not_cancelled(&cancellation)?;
    let response = ScanDiskUsageResponseDto {
        root: root_node,
        unreadable_entries: unreadable.count(),
        unreadable: unreadable.details(),
        scanned_entries: scanned_entries.load(Ordering::Relaxed),
        cleanup_candidates,
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
    let mut unsupported = DiskUsageSize::default();
    for tree in trees.iter().flatten() {
        ensure_not_cancelled(cancellation)?;
        // Each top-level tree's own `name` is only its basename (`build_tree_parallel` never
        // sees the full path), so its absolute location must be rejoined against `root` here —
        // `map_node`'s `is_root` branch otherwise uses `path` as-is.
        // Snapshots repeat every progress tick, so only the final mapping records skips.
        match map_node(
            tree,
            &root.join(tree.name().as_os_str()),
            &mut seen_hardlinks,
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: cfg!(not(unix)),
                remaining_depth: MAX_RESPONSE_DEPTH,
            },
            None,
            cancellation,
        )? {
            Some(child) => children.push(child),
            None => unsupported += tree.size(),
        }
    }
    let logical_bytes = root_size
        .logical_bytes
        .saturating_add(unsupported.logical_bytes)
        .saturating_add(children.iter().map(|child| child.logical_bytes).sum());
    let physical_bytes = root_size
        .physical_bytes
        .saturating_add(unsupported.physical_bytes)
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
        cleanup_candidates: Vec::new(),
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
            tree: event_tree(&response.root),
            unreadable_entries: response.unreadable_entries,
            unreadable: response.unreadable.iter().map(event_unreadable).collect(),
            scanned_entries: response.scanned_entries,
            cleanup_candidates: response
                .cleanup_candidates
                .iter()
                .map(event_cleanup_candidate)
                .collect(),
            is_complete,
        },
    );
}

/// `None` for a candidate this host cannot represent as a location; it is simply not suggested.
fn cleanup_candidate_dto(candidate: &CleanupCandidate) -> Option<DiskUsageCleanupCandidateDto> {
    Some(DiskUsageCleanupCandidateDto {
        location: Location::from_native_path(&candidate.path).ok()?.into(),
        kind: match candidate.kind {
            CleanupKind::NodeModules => DiskUsageCleanupKindDto::NodeModules,
            CleanupKind::PythonVirtualEnvironment => {
                DiskUsageCleanupKindDto::PythonVirtualEnvironment
            }
            CleanupKind::RustBuildOutput => DiskUsageCleanupKindDto::RustBuildOutput,
            CleanupKind::NextBuildOutput => DiskUsageCleanupKindDto::NextBuildOutput,
            CleanupKind::XcodeDerivedData => DiskUsageCleanupKindDto::XcodeDerivedData,
            CleanupKind::XcodeDeviceSupport => DiskUsageCleanupKindDto::XcodeDeviceSupport,
            CleanupKind::ApplicationCaches => DiskUsageCleanupKindDto::ApplicationCaches,
            CleanupKind::ToolCache => DiskUsageCleanupKindDto::ToolCache,
        },
        logical_bytes: candidate.logical_bytes,
        physical_bytes: candidate.physical_bytes,
    })
}

fn event_cleanup_candidate(
    candidate: &DiskUsageCleanupCandidateDto,
) -> DiskUsageCleanupCandidatePayload {
    DiskUsageCleanupCandidatePayload {
        location: LocationPayload {
            provider_id: fm_domain::ProviderId::new(candidate.location.provider_id.clone()),
            uri: candidate.location.uri.clone(),
        },
        kind: match candidate.kind {
            DiskUsageCleanupKindDto::NodeModules => DiskUsageCleanupKindPayload::NodeModules,
            DiskUsageCleanupKindDto::PythonVirtualEnvironment => {
                DiskUsageCleanupKindPayload::PythonVirtualEnvironment
            }
            DiskUsageCleanupKindDto::RustBuildOutput => {
                DiskUsageCleanupKindPayload::RustBuildOutput
            }
            DiskUsageCleanupKindDto::NextBuildOutput => {
                DiskUsageCleanupKindPayload::NextBuildOutput
            }
            DiskUsageCleanupKindDto::XcodeDerivedData => {
                DiskUsageCleanupKindPayload::XcodeDerivedData
            }
            DiskUsageCleanupKindDto::XcodeDeviceSupport => {
                DiskUsageCleanupKindPayload::XcodeDeviceSupport
            }
            DiskUsageCleanupKindDto::ApplicationCaches => {
                DiskUsageCleanupKindPayload::ApplicationCaches
            }
            DiskUsageCleanupKindDto::ToolCache => DiskUsageCleanupKindPayload::ToolCache,
        },
        logical_bytes: candidate.logical_bytes,
        physical_bytes: candidate.physical_bytes,
    }
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
            DiskUsageUnreadableReasonDto::OtherVolume => {
                DiskUsageUnreadableReasonPayload::OtherVolume
            }
            DiskUsageUnreadableReasonDto::CloudOnly => DiskUsageUnreadableReasonPayload::CloudOnly,
            DiskUsageUnreadableReasonDto::UnsupportedName => {
                DiskUsageUnreadableReasonPayload::UnsupportedName
            }
        },
    }
}

pub(crate) fn event_tree(root: &DiskUsageNodeDto) -> DiskUsageTreePayload {
    let tree = DiskUsageTreeDto::from_root(root);
    DiskUsageTreePayload {
        provider_id: tree.provider_id,
        root_uri: tree.root_uri,
        parents: tree.parents,
        names: tree.names,
        flags: tree.flags,
        logical_bytes: tree.logical_bytes,
        physical_bytes: tree.physical_bytes,
        uri_overrides: tree.uri_overrides,
    }
}

/// Decodes a progress-event tree back into nested nodes, for asserting on emitted events.
#[cfg(test)]
pub(crate) fn node_from_event_tree(tree: DiskUsageTreePayload) -> DiskUsageNodeDto {
    DiskUsageTreeDto {
        provider_id: tree.provider_id,
        root_uri: tree.root_uri,
        parents: tree.parents,
        names: tree.names,
        flags: tree.flags,
        logical_bytes: tree.logical_bytes,
        physical_bytes: tree.physical_bytes,
        uri_overrides: tree.uri_overrides,
    }
    .to_root()
    .expect("progress events carry a consistent tree")
}

/// Maps one scanned subtree to its DTO. Returns `None` for an entry this host cannot represent as
/// a location, recording it against its parent in `unsupported` when given; callers keep its bytes
/// in the parent's total so one odd name never fails the whole scan.
fn map_node(
    tree: &DataTree<OsStringDisplay, DiskUsageSize>,
    path: &Path,
    seen_hardlinks: &mut HashSet<FileIdentity>,
    options: MapNodeOptions,
    unsupported: Option<&UnreadableRegistry>,
    cancellation: &CancellationToken,
) -> Result<Option<DiskUsageNodeDto>, ApplicationError> {
    ensure_not_cancelled(cancellation)?;
    let node_path = if options.is_root {
        path.to_owned()
    } else {
        path.join(tree.name().as_os_str())
    };
    let Ok(location) = Location::from_native_path(&node_path) else {
        if let Some(registry) = unsupported {
            registry.record_reason(
                node_path.parent().unwrap_or(&node_path),
                UnreadableReason::UnsupportedName,
            );
        }
        return Ok(None);
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
        return Ok(Some(DiskUsageNodeDto {
            name,
            location: location.into(),
            kind,
            logical_bytes,
            physical_bytes,
            collapsed: true,
            children: Vec::new(),
        }));
    }
    let mut children = Vec::with_capacity(tree.children().len());
    let mut unmapped = DiskUsageSize::default();
    for child in tree.children() {
        if child.size().kind == ScannedEntryKind::Aggregate {
            continue;
        }
        let mapped = map_node(
            child,
            &node_path,
            seen_hardlinks,
            MapNodeOptions {
                is_root: false,
                expand_root: false,
                deduplicate_hardlinks: options.deduplicate_hardlinks,
                remaining_depth: options.remaining_depth.saturating_sub(1),
            },
            unsupported,
            cancellation,
        )?;
        match mapped {
            Some(mapped) => children.push(mapped),
            None => unmapped += child.size(),
        }
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
            .saturating_add(unmapped.logical_bytes)
            .saturating_add(children.iter().map(|child| child.logical_bytes).sum())
            .min(raw_total.logical_bytes);
        let physical_total = raw_total
            .physical_bytes
            .saturating_sub(raw_children.physical_bytes)
            .saturating_add(unmapped.physical_bytes)
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
    let mut node = DiskUsageNodeDto {
        name,
        location: location.into(),
        kind,
        logical_bytes,
        physical_bytes,
        collapsed,
        children,
    };
    fit_child_totals(&mut node);
    Ok(Some(node))
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

    #[test]
    fn boundary_skips_cloud_only_directories_before_checking_devices() {
        assert_eq!(
            boundary_skip_reason(Some(1), Some(1), SF_DATALESS),
            Some(UnreadableReason::CloudOnly)
        );
        assert_eq!(
            boundary_skip_reason(Some(1), Some(2), SF_DATALESS),
            Some(UnreadableReason::CloudOnly)
        );
    }

    #[test]
    fn boundary_skips_directories_on_another_device() {
        assert_eq!(
            boundary_skip_reason(Some(1), Some(2), 0),
            Some(UnreadableReason::OtherVolume)
        );
        assert_eq!(boundary_skip_reason(Some(1), Some(1), 0), None);
        assert_eq!(boundary_skip_reason(None, Some(2), 0), None);
        assert_eq!(boundary_skip_reason(Some(1), None, 0), None);
    }

    #[test]
    fn scan_of_one_volume_reports_no_boundary_skips() {
        let root = tempfile::tempdir().expect("create fixture root");
        fs::create_dir(root.path().join("nested")).expect("create nested directory");
        fs::write(root.path().join("nested/file.bin"), [1_u8; 9]).expect("write fixture file");
        let root_metadata = fs::symlink_metadata(root.path()).expect("root metadata");
        let unreadable = UnreadableRegistry::default();

        let tree = disk_usage_thread_pool()
            .expect("create disk usage pool")
            .install(|| {
                build_tree_parallel(
                    root.path(),
                    OsStringDisplay::os_string_from(root.path()),
                    None,
                    MAX_SCAN_DEPTH,
                    &CancellationToken::new(),
                    &unreadable,
                    &AtomicU64::new(0),
                    &Mutex::new(HashSet::new()),
                    ScanBoundary::for_root(&root_metadata),
                    DirectoryLister::native(),
                )
            })
            .expect("scan fixture");

        assert_eq!(unreadable.count(), 0);
        assert_eq!(tree.children().len(), 1);
        assert_eq!(tree.children()[0].children().len(), 1);
    }

    #[test]
    fn cleanup_candidates_are_found_in_the_scanned_tree() {
        let root = tempfile::tempdir().expect("create fixture root");
        let project = root.path().join("app");
        fs::create_dir_all(project.join("target/debug")).expect("create target");
        fs::write(project.join("Cargo.toml"), b"[package]").expect("write manifest");
        fs::write(project.join("target/debug/app"), [1_u8; 4096]).expect("write build output");
        fs::create_dir(root.path().join("target")).expect("create unrelated target");
        let root_metadata = fs::symlink_metadata(root.path()).expect("root metadata");
        let cancellation = CancellationToken::new();

        let tree = disk_usage_thread_pool()
            .expect("create disk usage pool")
            .install(|| {
                build_tree_parallel(
                    root.path(),
                    OsStringDisplay::os_string_from(root.path()),
                    None,
                    MAX_SCAN_DEPTH,
                    &cancellation,
                    &UnreadableRegistry::default(),
                    &AtomicU64::new(0),
                    &Mutex::new(HashSet::new()),
                    ScanBoundary::for_root(&root_metadata),
                    DirectoryLister::native(),
                )
            })
            .expect("scan fixture");
        let candidates =
            find_cleanup_candidates(&tree, root.path(), 1, &cancellation).expect("find candidates");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, project.join("target"));
        assert_eq!(candidates[0].kind, CleanupKind::RustBuildOutput);
        assert!(candidates[0].logical_bytes >= 4096);
        let dto = cleanup_candidate_dto(&candidates[0]).expect("map candidate");
        assert_eq!(dto.kind, DiskUsageCleanupKindDto::RustBuildOutput);
        assert!(dto.location.uri.ends_with("/app/target"));
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
                    None,
                    MAX_SCAN_DEPTH,
                    &cancellation,
                    &unreadable,
                    &scanned_entries,
                    &seen_hardlinks,
                    ScanBoundary::default(),
                    DirectoryLister::native(),
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

    #[cfg(target_os = "macos")]
    fn flatten_scan(tree: &ScanTree, prefix: &str, out: &mut Vec<(String, DiskUsageSize)>) {
        let path = format!("{prefix}/{}", tree.name());
        out.push((path.clone(), tree.size()));
        for child in tree.children() {
            flatten_scan(child, &path, out);
        }
    }

    #[cfg(target_os = "macos")]
    fn scan_with(root: &Path, lister: DirectoryLister) -> (Vec<(String, DiskUsageSize)>, u64) {
        let cancellation = CancellationToken::new();
        let scanned_entries = AtomicU64::new(0);
        let unreadable = UnreadableRegistry::default();
        let seen_hardlinks = Mutex::new(HashSet::new());
        let tree = disk_usage_thread_pool()
            .expect("create disk usage pool")
            .install(|| {
                build_tree_parallel(
                    root,
                    OsStringDisplay::os_string_from("root"),
                    None,
                    MAX_SCAN_DEPTH,
                    &cancellation,
                    &unreadable,
                    &scanned_entries,
                    &seen_hardlinks,
                    ScanBoundary::default(),
                    lister,
                )
            })
            .expect("scan fixture");
        let mut flat = Vec::new();
        flatten_scan(&tree, "", &mut flat);
        flat.sort();
        (flat, unreadable.count())
    }

    /// Gives the hardlink pair their sizes in a fixed order, since which one wins is racy.
    #[cfg(target_os = "macos")]
    fn normalise_hardlinks(
        (mut flat, unreadable): (Vec<(String, DiskUsageSize)>, u64),
    ) -> (Vec<(String, DiskUsageSize)>, u64) {
        let is_link =
            |path: &str| path.ends_with("/linked.bin") || path.ends_with("/linked-copy.bin");
        let mut sizes = flat
            .iter()
            .filter(|(path, _)| is_link(path))
            .map(|(_, size)| *size)
            .collect::<Vec<_>>();
        sizes.sort();
        let mut sizes = sizes.into_iter();
        for (path, size) in &mut flat {
            if is_link(path) {
                *size = sizes.next().expect("same count");
            }
        }
        (flat, unreadable)
    }

    /// The `getattrlistbulk` lister must produce exactly the tree the portable
    /// `read_dir` + `lstat` lister does: same entries, kinds, logical and physical bytes,
    /// hardlinks counted once, and the same unreadable count.
    #[cfg(target_os = "macos")]
    #[test]
    fn bulk_lister_matches_the_portable_lister() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = tempfile::tempdir().expect("create fixture root");
        let base = root.path();
        fs::create_dir_all(base.join("a/b/c")).expect("nested dirs");
        fs::create_dir(base.join("empty")).expect("empty dir");
        fs::write(base.join("a/small.txt"), b"hi").expect("small file");
        fs::write(base.join("a/b/medium.bin"), vec![1_u8; 70_000]).expect("medium file");
        fs::write(base.join("a/b/c/zero"), b"").expect("empty file");
        fs::write(base.join("a/b/c/ünïcødé ☃.txt"), vec![2_u8; 5_000]).expect("unicode file");
        // Same parent, so parallel traversal order only decides which of the two leaves counts.
        fs::write(base.join("a/linked.bin"), vec![3_u8; 9_000]).expect("hardlink source");
        fs::hard_link(base.join("a/linked.bin"), base.join("a/linked-copy.bin")).expect("hardlink");
        let forked = base.join("a/forked.txt");
        fs::write(&forked, b"hello").expect("forked file");
        let status = std::process::Command::new("xattr")
            .args(["-wx", "com.apple.ResourceFork", &"00".repeat(3_000)])
            .arg(&forked)
            .status()
            .expect("run xattr");
        assert!(status.success(), "write resource fork");
        symlink("a/small.txt", base.join("to-small")).expect("symlink");
        symlink("a", base.join("to-dir")).expect("dir symlink");
        let locked = base.join("locked");
        fs::create_dir(&locked).expect("locked dir");
        fs::write(locked.join("hidden"), b"secret").expect("locked file");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("chmod 000");

        let portable = normalise_hardlinks(scan_with(base, DirectoryLister::Portable));
        let bulk = normalise_hardlinks(scan_with(base, DirectoryLister::Bulk));
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("restore mode");

        assert_eq!(bulk, portable);
        assert!(
            portable
                .0
                .iter()
                .any(|(path, _)| path.ends_with("ünïcødé ☃.txt"))
        );
        let hardlinked = portable
            .0
            .iter()
            .filter(|(path, _)| path.ends_with("linked.bin") || path.ends_with("linked-copy.bin"))
            .map(|(_, size)| size.logical_bytes)
            .sum::<u64>();
        assert_eq!(hardlinked, 9_000);
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
                        None,
                        MAX_SCAN_DEPTH,
                        &cancellation,
                        &unreadable,
                        &scanned_entries,
                        &seen_hardlinks,
                        ScanBoundary::default(),
                        DirectoryLister::native(),
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
            None,
            &CancellationToken::new(),
        )
        .expect("an unreadable child must not fail the scan")
        .expect("representable fixture");

        assert!(mapped.children.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn mapping_skips_and_counts_unrepresentable_names_but_keeps_their_bytes() {
        let root = tempfile::tempdir().expect("create fixture root");
        let directory = |name: &str, children| {
            DataTree::dir(
                OsStringDisplay::os_string_from(name),
                DiskUsageSize {
                    kind: ScannedEntryKind::Directory,
                    ..DiskUsageSize::default()
                },
                children,
            )
        };
        let file = |name: &str, bytes: u64| {
            DataTree::file(
                OsStringDisplay::os_string_from(name),
                DiskUsageSize {
                    logical_bytes: bytes,
                    physical_bytes: bytes,
                    kind: ScannedEntryKind::File,
                },
            )
        };
        // A backslash is legal in a Unix file name but can never form a location segment.
        let tree = DataTree::dir(
            OsStringDisplay::os_string_from(root.path()),
            DiskUsageSize {
                kind: ScannedEntryKind::Directory,
                ..DiskUsageSize::default()
            },
            vec![directory(
                "words",
                vec![file("plain.yaml", 10), file("back\\slash.yaml", 30)],
            )],
        );
        let registry = UnreadableRegistry::default();

        let mapped = map_node(
            &tree,
            root.path(),
            &mut HashSet::new(),
            MapNodeOptions {
                is_root: true,
                expand_root: false,
                deduplicate_hardlinks: false,
                remaining_depth: MAX_RESPONSE_DEPTH,
            },
            Some(&registry),
            &CancellationToken::new(),
        )
        .expect("an unrepresentable name must not fail the scan")
        .expect("representable root");

        let words = &mapped.children[0];
        assert_eq!(words.children.len(), 1);
        assert_eq!(words.children[0].name, "plain.yaml");
        assert_eq!(words.logical_bytes, 40);
        assert_eq!(mapped.physical_bytes, 40);
        assert_eq!(registry.count(), 1);
        let details = registry.details();
        assert!(details[0].location.uri.ends_with("/words"));
        assert_eq!(
            details[0].reason,
            DiskUsageUnreadableReasonDto::UnsupportedName
        );
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
            None,
            &CancellationToken::new(),
        )
        .expect("collapsed mapping")
        .expect("representable fixture");

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
            None,
            &CancellationToken::new(),
        )
        .expect("collapsed mapping")
        .expect("representable fixture");

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
            None,
            &CancellationToken::new(),
        )
        .expect("map tree")
        .expect("representable fixture");

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
