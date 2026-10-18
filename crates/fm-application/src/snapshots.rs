//! Folder snapshots and snapshot comparison (task 0268).
//!
//! A *create* job walks a folder and writes a [`crate::snapshot_format`] file (optionally with
//! SHA-256 checksums) through the destination provider, refusing to overwrite. A *compare* job
//! reads such a file and reports what was added, removed, changed or silently corrupted under a
//! target folder - which may differ from the recorded root, to verify a backup copy. Both are
//! cancellable background jobs polled by the frontend; symbolic links are never followed or
//! recorded.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use fm_checksum::{ChecksumAlgorithm, hash_entry};
use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_transport_dto::{
    SnapshotDiffCountsDto, SnapshotDiffEntryDto, SnapshotDiffKindDto, SnapshotJobKindDto,
    SnapshotJobPageDto, SnapshotJobPhaseDto, StartSnapshotJobRequestDto,
    StartSnapshotJobResponseDto,
};
use fm_vfs::{EntryRef, ProviderRegistry, WriteOptions};
use percent_encoding::percent_decode_str;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::snapshot_format::{
    Comparison, CurrentState, SNAPSHOT_FORMAT_NAME, SNAPSHOT_FORMAT_VERSION, SnapshotEntry,
    SnapshotHeader, compare_entry, parse_snapshot, render_snapshot,
};
use crate::tree_walk::walk;

/// Largest snapshot file a compare job will read.
const MAX_SNAPSHOT_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
struct JobState {
    phase: SnapshotJobPhaseDto,
    entries_found: u64,
    files_processed: u64,
    bytes_hashed: u64,
    has_checksums: bool,
    snapshot: Option<Location>,
    counts: SnapshotDiffCountsDto,
    diffs: Vec<SnapshotDiffEntryDto>,
    complete: bool,
    cancelled: bool,
    error: Option<String>,
}

struct JobHandle {
    kind: SnapshotJobKindDto,
    root: Location,
    cancellation: CancellationToken,
    state: Mutex<JobState>,
}

impl JobHandle {
    fn update(&self, change: impl FnOnce(&mut JobState)) {
        change(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner));
    }

    fn finish(&self, error: Option<String>) {
        let cancelled = self.cancellation.is_cancelled();
        self.update(|state| {
            state.phase = SnapshotJobPhaseDto::Done;
            state.complete = true;
            state.cancelled = cancelled;
            if error.is_some() {
                state.error = error;
            }
        });
    }
}

/// Owns running and finished snapshot jobs.
#[derive(Default)]
pub(crate) struct SnapshotCoordinator {
    jobs: Mutex<HashMap<Uuid, Arc<JobHandle>>>,
}

/// The path of `entry` relative to `root`, decoded, with `/` separators.
fn relative_path(root: &Location, entry: &Location) -> Option<String> {
    let prefix = format!("{}/", root.uri.trim_end_matches('/'));
    let remainder = entry.uri.strip_prefix(&prefix)?;
    Some(
        percent_decode_str(remainder)
            .decode_utf8_lossy()
            .into_owned(),
    )
}

fn modified_ms(entry: &EntrySummary) -> Option<i64> {
    entry.modified_at.map(|time| time.timestamp_millis())
}

async fn hash_file(
    providers: &ProviderRegistry,
    entry: &EntrySummary,
    cancellation: &CancellationToken,
) -> Option<(String, u64)> {
    let reference = EntryRef {
        id: entry.id,
        location: entry.location.clone(),
    };
    let set = hash_entry(
        providers,
        &reference,
        &[ChecksumAlgorithm::Sha256],
        cancellation,
    )
    .await
    .ok()?;
    Some((
        set.get(ChecksumAlgorithm::Sha256)?.to_owned(),
        set.bytes_hashed(),
    ))
}

async fn run_create(
    handle: Arc<JobHandle>,
    providers: ProviderRegistry,
    destination: Location,
    with_checksums: bool,
    show_hidden: bool,
) {
    let cancellation = handle.cancellation.clone();
    handle.update(|state| state.phase = SnapshotJobPhaseDto::Listing);
    let (listed, _) = walk(
        &providers,
        std::slice::from_ref(&handle.root),
        show_hidden,
        &cancellation,
        usize::MAX,
    )
    .await;
    let mut recorded: Vec<(String, EntrySummary)> = listed
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Symlink && entry.location.uri != destination.uri)
        .filter_map(|entry| Some((relative_path(&handle.root, &entry.location)?, entry)))
        .collect();
    recorded.sort_by(|left, right| left.0.cmp(&right.0));
    handle.update(|state| {
        state.entries_found = recorded.len() as u64;
        state.has_checksums = with_checksums;
        state.phase = if with_checksums {
            SnapshotJobPhaseDto::Hashing
        } else {
            SnapshotJobPhaseDto::Writing
        };
    });

    let mut entries = Vec::with_capacity(recorded.len());
    for (path, entry) in &recorded {
        if cancellation.is_cancelled() {
            handle.finish(None);
            return;
        }
        let is_directory = entry.kind == EntryKind::Directory;
        let mut sha256 = None;
        if with_checksums
            && !is_directory
            && let Some((digest, bytes)) = hash_file(&providers, entry, &cancellation).await
        {
            sha256 = Some(digest);
            handle.update(|state| state.bytes_hashed += bytes);
        }
        handle.update(|state| state.files_processed += 1);
        entries.push(SnapshotEntry {
            path: path.clone(),
            kind: if is_directory { "d" } else { "f" }.to_owned(),
            size: (!is_directory).then(|| entry.size.unwrap_or(0)),
            modified_ms: if is_directory {
                None
            } else {
                modified_ms(entry)
            },
            sha256,
        });
    }

    handle.update(|state| state.phase = SnapshotJobPhaseDto::Writing);
    let header = SnapshotHeader {
        format: SNAPSHOT_FORMAT_NAME.to_owned(),
        version: SNAPSHOT_FORMAT_VERSION,
        root: handle.root.uri.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        algorithm: with_checksums.then(|| "sha256".to_owned()),
        entry_count: entries.len() as u64,
    };
    let text = render_snapshot(&header, &entries);
    let outcome = write_snapshot(&providers, &destination, text.as_bytes(), &cancellation).await;
    match outcome {
        Ok(()) => {
            handle.update(|state| state.snapshot = Some(destination));
            handle.finish(None);
        }
        Err(error) => handle.finish(Some(error.to_string())),
    }
}

async fn write_snapshot(
    providers: &ProviderRegistry,
    destination: &Location,
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<(), ApplicationError> {
    let provider = providers
        .resolve(destination)
        .map_err(ApplicationError::from)?;
    let mut writer = provider
        .open_write(
            destination,
            WriteOptions { overwrite: false },
            cancellation.clone(),
        )
        .await
        .map_err(ApplicationError::from)?;
    writer
        .write_all(bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    writer
        .shutdown()
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    Ok(())
}

async fn read_snapshot_text(
    providers: &ProviderRegistry,
    snapshot: &Location,
    cancellation: &CancellationToken,
) -> Result<String, ApplicationError> {
    let provider = providers
        .resolve(snapshot)
        .map_err(ApplicationError::from)?;
    let reader = provider
        .open_read(
            &EntryRef {
                id: EntryId::new(),
                location: snapshot.clone(),
            },
            cancellation.clone(),
        )
        .await
        .map_err(ApplicationError::from)?;
    let mut bytes = Vec::new();
    reader
        .take(MAX_SNAPSHOT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;
    if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
        return Err(ApplicationError::InvalidRequest(
            "the snapshot file is too large".to_owned(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| ApplicationError::InvalidRequest("the snapshot is not valid UTF-8".to_owned()))
}

fn diff_entry(
    kind: SnapshotDiffKindDto,
    path: &str,
    recorded: Option<&SnapshotEntry>,
    current: Option<&EntrySummary>,
) -> SnapshotDiffEntryDto {
    SnapshotDiffEntryDto {
        kind,
        path: path.to_owned(),
        location: current.map(|entry| entry.location.clone().into()),
        snapshot_size: recorded.and_then(|entry| entry.size),
        current_size: current.and_then(|entry| entry.size),
        snapshot_modified_ms: recorded.and_then(|entry| entry.modified_ms),
        current_modified_ms: current.and_then(modified_ms),
        is_directory: recorded.map_or_else(
            || current.is_some_and(|entry| entry.kind == EntryKind::Directory),
            SnapshotEntry::is_directory,
        ),
    }
}

async fn run_compare(
    handle: Arc<JobHandle>,
    providers: ProviderRegistry,
    snapshot: Location,
    verify_checksums: bool,
    show_hidden: bool,
) {
    let cancellation = handle.cancellation.clone();
    handle.update(|state| state.phase = SnapshotJobPhaseDto::Loading);
    let text = match read_snapshot_text(&providers, &snapshot, &cancellation).await {
        Ok(text) => text,
        Err(error) => return handle.finish(Some(error.to_string())),
    };
    let (header, recorded) = match parse_snapshot(&text) {
        Ok(parsed) => parsed,
        Err(error) => return handle.finish(Some(error.to_string())),
    };
    let has_checksums = header.algorithm.is_some();
    handle.update(|state| {
        state.has_checksums = has_checksums;
        state.entries_found = recorded.len() as u64;
        state.phase = SnapshotJobPhaseDto::Listing;
    });

    let (listed, _) = walk(
        &providers,
        std::slice::from_ref(&handle.root),
        show_hidden,
        &cancellation,
        usize::MAX,
    )
    .await;
    let mut current: HashMap<String, EntrySummary> = listed
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Symlink && entry.location.uri != snapshot.uri)
        .filter_map(|entry| Some((relative_path(&handle.root, &entry.location)?, entry)))
        .collect();

    handle.update(|state| state.phase = SnapshotJobPhaseDto::Hashing);
    let mut diffs: Vec<SnapshotDiffEntryDto> = Vec::new();
    let mut counts = SnapshotDiffCountsDto::default();
    for entry in &recorded {
        if cancellation.is_cancelled() {
            return handle.finish(None);
        }
        let Some(now) = current.remove(&entry.path) else {
            counts.removed += 1;
            diffs.push(diff_entry(
                SnapshotDiffKindDto::Removed,
                &entry.path,
                Some(entry),
                None,
            ));
            continue;
        };
        let state = CurrentState {
            is_directory: now.kind == EntryKind::Directory,
            size: now.size.unwrap_or(0),
            modified_ms: modified_ms(&now),
        };
        let kind = match compare_entry(entry, &state, verify_checksums) {
            Comparison::Unchanged => None,
            Comparison::Changed => Some(SnapshotDiffKindDto::Changed),
            Comparison::NeedsChecksum { time_matches } => {
                let digest = hash_file(&providers, &now, &cancellation).await;
                if let Some((_, bytes)) = &digest {
                    let bytes = *bytes;
                    handle.update(|job| job.bytes_hashed += bytes);
                }
                match (&digest, &entry.sha256) {
                    (Some((actual, _)), Some(expected))
                        if actual.eq_ignore_ascii_case(expected) =>
                    {
                        None
                    }
                    // An unreadable file cannot be shown to match, so it is reported as changed.
                    (None, _) => Some(SnapshotDiffKindDto::Changed),
                    _ if time_matches => Some(SnapshotDiffKindDto::Corrupted),
                    _ => Some(SnapshotDiffKindDto::Changed),
                }
            }
        };
        handle.update(|job| job.files_processed += 1);
        match kind {
            None => counts.unchanged += 1,
            Some(kind) => {
                match kind {
                    SnapshotDiffKindDto::Corrupted => counts.corrupted += 1,
                    _ => counts.changed += 1,
                }
                diffs.push(diff_entry(kind, &entry.path, Some(entry), Some(&now)));
            }
        }
    }
    for (path, now) in &current {
        counts.added += 1;
        diffs.push(diff_entry(
            SnapshotDiffKindDto::Added,
            path,
            None,
            Some(now),
        ));
    }
    let rank = |kind: SnapshotDiffKindDto| match kind {
        SnapshotDiffKindDto::Corrupted => 0,
        SnapshotDiffKindDto::Changed => 1,
        SnapshotDiffKindDto::Removed => 2,
        SnapshotDiffKindDto::Added => 3,
    };
    diffs.sort_by(|left, right| {
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.path.cmp(&right.path))
    });
    handle.update(|state| {
        state.counts = counts;
        state.diffs = diffs;
    });
    handle.finish(None);
}

impl SnapshotCoordinator {
    /// Starts a create or compare job on the current Tokio runtime.
    pub(crate) fn start(
        &self,
        providers: ProviderRegistry,
        request: StartSnapshotJobRequestDto,
    ) -> StartSnapshotJobResponseDto {
        let job_id = Uuid::new_v4();
        let (kind, root) = match &request {
            StartSnapshotJobRequestDto::Create { root, .. } => {
                (SnapshotJobKindDto::Create, root.clone().into())
            }
            StartSnapshotJobRequestDto::Compare { target, .. } => {
                (SnapshotJobKindDto::Compare, target.clone().into())
            }
        };
        let handle = Arc::new(JobHandle {
            kind,
            root,
            cancellation: CancellationToken::new(),
            state: Mutex::new(JobState {
                phase: SnapshotJobPhaseDto::Listing,
                entries_found: 0,
                files_processed: 0,
                bytes_hashed: 0,
                has_checksums: false,
                snapshot: None,
                counts: SnapshotDiffCountsDto::default(),
                diffs: Vec::new(),
                complete: false,
                cancelled: false,
                error: None,
            }),
        });
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(job_id, Arc::clone(&handle));
        match request {
            StartSnapshotJobRequestDto::Create {
                destination,
                with_checksums,
                show_hidden,
                ..
            } => {
                tokio::spawn(run_create(
                    handle,
                    providers,
                    destination.into(),
                    with_checksums,
                    show_hidden,
                ));
            }
            StartSnapshotJobRequestDto::Compare {
                snapshot,
                verify_checksums,
                show_hidden,
                ..
            } => {
                tokio::spawn(run_compare(
                    handle,
                    providers,
                    snapshot.into(),
                    verify_checksums,
                    show_hidden,
                ));
            }
        }
        StartSnapshotJobResponseDto { job_id }
    }

    fn handle(&self, job_id: Uuid) -> Result<Arc<JobHandle>, ApplicationError> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&job_id)
            .cloned()
            .ok_or(ApplicationError::NotFound)
    }

    /// Requests prompt cancellation of a running job.
    pub(crate) fn cancel(&self, job_id: Uuid) -> Result<(), ApplicationError> {
        self.handle(job_id)?.cancellation.cancel();
        Ok(())
    }

    /// A bounded page of a job's differences plus its progress.
    pub(crate) fn page(
        &self,
        job_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<SnapshotJobPageDto, ApplicationError> {
        let handle = self.handle(job_id)?;
        let limit = limit.clamp(1, 500);
        let state = handle.state.lock().unwrap_or_else(PoisonError::into_inner);
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(state.diffs.len());
        let end = (start + usize::from(limit)).min(state.diffs.len());
        Ok(SnapshotJobPageDto {
            job_id,
            kind: handle.kind,
            phase: state.phase,
            entries_found: state.entries_found,
            files_processed: state.files_processed,
            bytes_hashed: state.bytes_hashed,
            has_checksums: state.has_checksums,
            snapshot: state.snapshot.clone().map(Into::into),
            root: handle.root.clone().into(),
            counts: state.counts,
            offset,
            limit,
            total: state.diffs.len() as u64,
            entries: state.diffs[start..end].to_vec(),
            is_complete: state.complete,
            is_cancelled: state.cancelled,
            has_more: end < state.diffs.len(),
            error: state.error.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_decoded_and_scoped_to_the_root() {
        let root = Location::parse("file:///data/My%20Folder/").expect("root");
        let inside = Location::parse("file:///data/My%20Folder/sub/a%20b.txt").expect("inside");
        let outside = Location::parse("file:///data/Other/a.txt").expect("outside");
        assert_eq!(
            relative_path(&root, &inside).as_deref(),
            Some("sub/a b.txt")
        );
        assert_eq!(relative_path(&root, &outside), None);
    }
}
