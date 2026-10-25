//! Duplicate and near-duplicate music finder (task 0265).
//!
//! A scan walks the roots for audio files, finds byte-identical copies with the staged checksum
//! detector (`fm_checksum`, task 0077), reads tags for local files, and then groups files that
//! are probably the same recording: equal normalised artist and title with a playing time within
//! [`DURATION_TOLERANCE_SECONDS`]. Unreadable tags only exclude a file from the "likely" pass.
//!
//! The pure grouping lives in [`group_music`] so it can be tested without a filesystem.

use crate::rejection::{Rejection, reason};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use fm_checksum::{DuplicateCandidate, DuplicateOptions, collect_candidates, find_duplicates};
use fm_domain::Location;
use fm_metadata::{AudioInfo, is_supported_audio_extension, read_audio_info};
use fm_transport_dto::{
    MusicDuplicateConfidenceDto, MusicDuplicateFileDto, MusicDuplicateGroupDto,
    MusicDuplicatePageDto, MusicDuplicateScanPhaseDto, StartMusicDuplicateScanRequestDto,
    StartMusicDuplicateScanResponseDto,
};
use fm_vfs::ProviderRegistry;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;

/// Playing times this close (seconds) count as the same recording.
pub(crate) const DURATION_TOLERANCE_SECONDS: f64 = 3.0;

/// How many files have their tags read at once.
const TAG_CONCURRENCY: usize = 4;

/// An audio file offered to the grouping step.
#[derive(Debug, Clone)]
pub(crate) struct MusicFile {
    pub(crate) location: Location,
    pub(crate) size: u64,
    pub(crate) info: Option<AudioInfo>,
}

/// How a group's files relate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Confidence {
    Exact,
    Likely,
}

/// Files believed to be the same recording.
#[derive(Debug, Clone)]
pub(crate) struct MusicGroup {
    pub(crate) confidence: Confidence,
    /// Indices into the scanned file list; the suggested keeper is `members[keep]`.
    pub(crate) members: Vec<usize>,
    pub(crate) keep: usize,
}

/// Lowercases and collapses punctuation and whitespace, dropping a leading "the".
pub(crate) fn normalise(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut pending_space = false;
    for character in text.chars() {
        if character.is_alphanumeric() {
            if pending_space && !collapsed.is_empty() {
                collapsed.push(' ');
            }
            pending_space = false;
            collapsed.extend(character.to_lowercase());
        } else {
            pending_space = true;
        }
    }
    collapsed
        .strip_prefix("the ")
        .map_or(collapsed.clone(), str::to_owned)
}

fn quality(file: &MusicFile) -> (bool, u32, u64) {
    let info = file.info.as_ref();
    (
        info.is_some_and(|info| info.lossless),
        info.and_then(|info| info.bitrate_kbps).unwrap_or(0),
        file.size,
    )
}

/// Index (into `members`) of the file worth keeping: lossless, then bitrate, then size, then the
/// lexicographically first location for a stable answer.
fn best_member(files: &[MusicFile], members: &[usize]) -> usize {
    members
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            let (a, b) = (&files[**left], &files[**right]);
            quality(a)
                .cmp(&quality(b))
                .then_with(|| b.location.uri.cmp(&a.location.uri))
        })
        .map_or(0, |(position, _)| position)
}

/// Groups `files`: `exact` lists byte-identical sets (indices); the rest is matched by tags.
pub(crate) fn group_music(files: &[MusicFile], exact: &[Vec<usize>]) -> Vec<MusicGroup> {
    let mut groups = Vec::new();
    let mut exact_group_of: HashMap<usize, usize> = HashMap::new();
    for (group_index, members) in exact.iter().enumerate() {
        if members.len() < 2 {
            continue;
        }
        for member in members {
            exact_group_of.insert(*member, group_index);
        }
        let mut members = members.clone();
        members.sort_by(|left, right| files[*left].location.uri.cmp(&files[*right].location.uri));
        let keep = best_member(files, &members);
        groups.push(MusicGroup {
            confidence: Confidence::Exact,
            members,
            keep,
        });
    }

    let mut by_key: HashMap<(String, String), Vec<(usize, f64)>> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        let Some(info) = &file.info else { continue };
        let (Some(artist), Some(title), Some(duration)) =
            (&info.artist, &info.title, info.duration)
        else {
            continue;
        };
        let (artist, title) = (normalise(artist), normalise(title));
        if artist.is_empty() || title.is_empty() {
            continue;
        }
        by_key
            .entry((artist, title))
            .or_default()
            .push((index, duration.as_secs_f64()));
    }
    let mut keys: Vec<_> = by_key.into_iter().collect();
    keys.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, mut candidates) in keys {
        candidates.sort_by(|left, right| left.1.total_cmp(&right.1));
        let mut cluster: Vec<usize> = Vec::new();
        let mut last = f64::NEG_INFINITY;
        let flush = |cluster: &mut Vec<usize>, groups: &mut Vec<MusicGroup>| {
            if cluster.len() >= 2 {
                let single_exact_group = cluster
                    .iter()
                    .map(|index| exact_group_of.get(index))
                    .collect::<Vec<_>>();
                let covered = single_exact_group.iter().all(Option::is_some)
                    && single_exact_group.windows(2).all(|pair| pair[0] == pair[1]);
                if !covered {
                    let mut members = std::mem::take(cluster);
                    members.sort_by(|left, right| {
                        files[*left].location.uri.cmp(&files[*right].location.uri)
                    });
                    let keep = best_member(files, &members);
                    groups.push(MusicGroup {
                        confidence: Confidence::Likely,
                        members,
                        keep,
                    });
                }
            }
            cluster.clear();
        };
        for (index, duration) in candidates {
            if !cluster.is_empty() && duration - last > DURATION_TOLERANCE_SECONDS {
                flush(&mut cluster, &mut groups);
            }
            cluster.push(index);
            last = duration;
        }
        flush(&mut cluster, &mut groups);
    }
    groups
}

#[derive(Debug)]
struct ScanState {
    phase: MusicDuplicateScanPhaseDto,
    files_found: u64,
    files_processed: u64,
    unreadable_tags: u64,
    groups: Vec<MusicDuplicateGroupDto>,
    complete: bool,
    cancelled: bool,
}

struct ScanHandle {
    roots: Vec<Location>,
    cancellation: CancellationToken,
    state: Mutex<ScanState>,
}

impl ScanHandle {
    fn update(&self, change: impl FnOnce(&mut ScanState)) {
        change(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// Owns running and finished music duplicate scans.
#[derive(Default)]
pub(crate) struct MusicDuplicateCoordinator {
    scans: Mutex<HashMap<Uuid, Arc<ScanHandle>>>,
}

fn file_dto(file: &MusicFile) -> MusicDuplicateFileDto {
    let info = file.info.as_ref();
    MusicDuplicateFileDto {
        location: file.location.clone().into(),
        size: file.size,
        format: info.map(|info| info.format.clone()),
        lossless: info.is_some_and(|info| info.lossless),
        bitrate_kbps: info.and_then(|info| info.bitrate_kbps),
        duration_seconds: info.and_then(|info| info.duration).map(|d| d.as_secs_f64()),
        artist: info.and_then(|info| info.artist.clone()),
        title: info.and_then(|info| info.title.clone()),
        album: info.and_then(|info| info.album.clone()),
    }
}

fn group_dto(files: &[MusicFile], group: &MusicGroup) -> MusicDuplicateGroupDto {
    let kept = group.members.get(group.keep).copied();
    let reclaimable_bytes = group
        .members
        .iter()
        .filter(|member| Some(**member) != kept)
        .map(|member| files[*member].size)
        .sum();
    MusicDuplicateGroupDto {
        confidence: match group.confidence {
            Confidence::Exact => MusicDuplicateConfidenceDto::Exact,
            Confidence::Likely => MusicDuplicateConfidenceDto::Likely,
        },
        files: group
            .members
            .iter()
            .map(|member| file_dto(&files[*member]))
            .collect(),
        keep_index: u32::try_from(group.keep).unwrap_or(0),
        reclaimable_bytes,
    }
}

fn is_audio(location: &Location) -> bool {
    let name = location.uri.rsplit('/').next().unwrap_or_default();
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| is_supported_audio_extension(extension))
}

async fn run_scan(handle: Arc<ScanHandle>, providers: ProviderRegistry, show_hidden: bool) {
    let cancellation = handle.cancellation.clone();
    let candidates: Vec<DuplicateCandidate> =
        collect_candidates(&providers, &handle.roots, show_hidden, &cancellation)
            .await
            .into_iter()
            .filter(|candidate| is_audio(&candidate.entry.location))
            .collect();
    handle.update(|state| {
        state.files_found = candidates.len() as u64;
        state.phase = MusicDuplicateScanPhaseDto::Hashing;
    });

    let mut files: Vec<MusicFile> = candidates
        .iter()
        .map(|candidate| MusicFile {
            location: candidate.entry.location.clone(),
            size: candidate.size,
            info: None,
        })
        .collect();
    let index_by_uri: HashMap<String, usize> = files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.location.uri.clone(), index))
        .collect();

    let scan = find_duplicates(
        &providers,
        candidates,
        &DuplicateOptions::default(),
        &cancellation,
    )
    .await;
    let exact: Vec<Vec<usize>> = scan
        .groups
        .iter()
        .map(|group| {
            group
                .distinct_files
                .iter()
                .filter_map(|file| index_by_uri.get(&file.entry.location.uri).copied())
                .collect()
        })
        .collect();

    handle.update(|state| {
        state.phase = MusicDuplicateScanPhaseDto::Tagging;
        state.files_processed = 0;
    });
    let paths: Vec<(usize, Option<std::path::PathBuf>)> = files
        .iter()
        .enumerate()
        .map(|(index, file)| (index, file.location.to_native_path().ok()))
        .collect();
    let mut reads = futures::stream::iter(paths)
        .map(|(index, path)| async move {
            let info = match path {
                Some(path) => tokio::task::spawn_blocking(move || read_audio_info(&path).ok())
                    .await
                    .ok()
                    .flatten(),
                None => None,
            };
            (index, info)
        })
        .buffer_unordered(TAG_CONCURRENCY);
    while let Some((index, info)) = reads.next().await {
        if cancellation.is_cancelled() {
            break;
        }
        handle.update(|state| {
            state.files_processed += 1;
            if info.is_none() {
                state.unreadable_tags += 1;
            }
        });
        files[index].info = info;
    }
    drop(reads);

    if cancellation.is_cancelled() {
        handle.update(|state| {
            state.phase = MusicDuplicateScanPhaseDto::Done;
            state.cancelled = true;
            state.complete = true;
        });
        return;
    }
    handle.update(|state| state.phase = MusicDuplicateScanPhaseDto::Grouping);
    let mut groups: Vec<MusicDuplicateGroupDto> = group_music(&files, &exact)
        .iter()
        .map(|group| group_dto(&files, group))
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.reclaimable_bytes));
    handle.update(|state| {
        state.groups = groups;
        state.phase = MusicDuplicateScanPhaseDto::Done;
        state.complete = true;
    });
}

impl MusicDuplicateCoordinator {
    /// Starts a scan on the current Tokio runtime.
    pub(crate) fn start(
        &self,
        providers: ProviderRegistry,
        request: StartMusicDuplicateScanRequestDto,
    ) -> Result<StartMusicDuplicateScanResponseDto, ApplicationError> {
        if request.roots.is_empty() {
            return Err(ApplicationError::from(Rejection::new(
                reason::ROOT_REQUIRED,
                "at least one root must be supplied",
            )));
        }
        let scan_id = Uuid::new_v4();
        let handle = Arc::new(ScanHandle {
            roots: request.roots.into_iter().map(Into::into).collect(),
            cancellation: CancellationToken::new(),
            state: Mutex::new(ScanState {
                phase: MusicDuplicateScanPhaseDto::Listing,
                files_found: 0,
                files_processed: 0,
                unreadable_tags: 0,
                groups: Vec::new(),
                complete: false,
                cancelled: false,
            }),
        });
        self.scans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(scan_id, Arc::clone(&handle));
        tokio::spawn(run_scan(handle, providers, request.show_hidden));
        Ok(StartMusicDuplicateScanResponseDto { scan_id })
    }

    fn handle(&self, scan_id: Uuid) -> Result<Arc<ScanHandle>, ApplicationError> {
        self.scans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&scan_id)
            .cloned()
            .ok_or(ApplicationError::NotFound)
    }

    /// Requests prompt cancellation of a running scan.
    pub(crate) fn cancel(&self, scan_id: Uuid) -> Result<(), ApplicationError> {
        self.handle(scan_id)?.cancellation.cancel();
        Ok(())
    }

    /// A bounded page of a scan's groups plus its progress.
    pub(crate) fn page(
        &self,
        scan_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<MusicDuplicatePageDto, ApplicationError> {
        let handle = self.handle(scan_id)?;
        let limit = limit.clamp(1, 500);
        let state = handle.state.lock().unwrap_or_else(PoisonError::into_inner);
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(state.groups.len());
        let end = (start + usize::from(limit)).min(state.groups.len());
        Ok(MusicDuplicatePageDto {
            scan_id,
            roots: handle.roots.iter().cloned().map(Into::into).collect(),
            phase: state.phase,
            files_found: state.files_found,
            files_processed: state.files_processed,
            unreadable_tags: state.unreadable_tags,
            offset,
            limit,
            total: state.groups.len() as u64,
            groups: state.groups[start..end].to_vec(),
            is_complete: state.complete,
            is_cancelled: state.cancelled,
            has_more: end < state.groups.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn file(
        uri: &str,
        size: u64,
        artist: &str,
        title: &str,
        seconds: u64,
        lossless: bool,
    ) -> MusicFile {
        MusicFile {
            location: Location::parse(&format!("file:///music/{uri}")).expect("location"),
            size,
            info: Some(AudioInfo {
                artist: Some(artist.to_owned()),
                title: Some(title.to_owned()),
                duration: Some(Duration::from_secs(seconds)),
                bitrate_kbps: Some(if lossless { 900 } else { 192 }),
                format: if lossless { "flac" } else { "mpeg" }.to_owned(),
                lossless,
                ..AudioInfo::default()
            }),
        }
    }

    #[test]
    fn normalisation_ignores_case_punctuation_and_a_leading_the() {
        assert_eq!(normalise("The  Beatles"), "beatles");
        assert_eq!(normalise("Hey, Jude!"), "hey jude");
        assert_eq!(normalise("  "), "");
    }

    #[test]
    fn same_song_in_two_formats_is_a_likely_group_preferring_lossless() {
        let files = vec![
            file("a.mp3", 4_000_000, "The Beatles", "Hey Jude", 431, false),
            file("a.flac", 30_000_000, "Beatles", "hey jude", 432, true),
            file("other.mp3", 3_000_000, "Beatles", "Let It Be", 243, false),
        ];

        let groups = group_music(&files, &[]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].confidence, Confidence::Likely);
        assert_eq!(groups[0].members, vec![1, 0]);
        assert_eq!(
            files[groups[0].members[groups[0].keep]].location.uri,
            "file:///music/a.flac"
        );
    }

    #[test]
    fn durations_beyond_the_tolerance_are_not_grouped() {
        let files = vec![
            file("a.mp3", 1, "A", "Song", 200, false),
            file("b.mp3", 1, "A", "Song", 260, false),
        ];
        assert!(group_music(&files, &[]).is_empty());
    }

    #[test]
    fn exact_groups_are_reported_and_not_repeated_as_likely() {
        let files = vec![
            file("a.mp3", 5, "A", "Song", 200, false),
            file("copy.mp3", 5, "A", "Song", 200, false),
        ];

        let groups = group_music(&files, &[vec![0, 1]]);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].confidence, Confidence::Exact);
    }

    #[test]
    fn a_likely_group_that_also_holds_a_different_encode_is_kept() {
        let files = vec![
            file("a.mp3", 5, "A", "Song", 200, false),
            file("copy.mp3", 5, "A", "Song", 200, false),
            file("a.flac", 50, "A", "Song", 200, true),
        ];

        let groups = group_music(&files, &[vec![0, 1]]);

        assert_eq!(groups.len(), 2);
        assert!(
            groups
                .iter()
                .any(|group| group.confidence == Confidence::Likely && group.members.len() == 3)
        );
    }

    #[test]
    fn files_without_readable_tags_never_form_likely_groups() {
        let mut files = vec![
            file("a.mp3", 1, "A", "Song", 200, false),
            file("b.mp3", 1, "A", "Song", 200, false),
        ];
        files[1].info = None;
        assert!(group_music(&files, &[]).is_empty());
    }

    #[test]
    fn audio_detection_uses_the_extension() {
        assert!(is_audio(&Location::parse("file:///a/b.MP3").expect("loc")));
        assert!(!is_audio(&Location::parse("file:///a/b.txt").expect("loc")));
    }
}
