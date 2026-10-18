//! Broken-link doctor (task 0266): finds dead symbolic links, Windows shortcuts, playlist entries
//! and relative Markdown/HTML links under some roots, and searches for replacement targets.
//!
//! The scan is read-only. It never executes or fetches anything, treats symbolic links as leaves
//! (they are inspected, not entered, so a link can never lead the walk outside the scanned
//! roots), and checks cancellation between every entry. Applying a fix is a separate, explicit
//! operation owned by the operation engine.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use fm_domain::{EntryId, EntryKind, EntrySummary, Location};
use fm_transport_dto::{
    FindLinkReplacementsRequestDto, FindLinkReplacementsResponseDto, LinkDoctorPageDto,
    LinkProblemDto, LinkProblemKindDto, LinkReplacementCandidateDto, StartLinkDoctorScanRequestDto,
    StartLinkDoctorScanResponseDto,
};
use fm_vfs::{EntryRef, ProviderRegistry, VfsError};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::link_extract::{
    ExtractedLink, PlaylistKind, html_links, markdown_links, playlist_links, relative_path_target,
    shortcut_target,
};
use crate::tree_walk::walk;

const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PROBLEMS: usize = 20_000;
const REPLACEMENT_ENTRY_BUDGET: usize = 200_000;
const MAX_REPLACEMENT_CANDIDATES: usize = 25;

#[derive(Debug)]
struct ScanState {
    entries_scanned: u64,
    links_checked: u64,
    problems: Vec<LinkProblemDto>,
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

/// Owns running and finished link-doctor scans.
#[derive(Default)]
pub(crate) struct LinkDoctorCoordinator {
    scans: Mutex<HashMap<Uuid, Arc<ScanHandle>>>,
}

/// `Some(true)`: present; `Some(false)`: definitely missing; `None`: could not be determined
/// (for example permission denied), which is never reported as a problem.
async fn target_exists(
    providers: &ProviderRegistry,
    location: &Location,
    cancellation: &CancellationToken,
) -> Option<bool> {
    let provider = providers.resolve(location).ok()?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    match provider.inspect(&entry, cancellation.clone()).await {
        Ok(summary) if summary.kind == EntryKind::Symlink => {
            match provider.resolve_symlink(&entry, cancellation.clone()).await {
                Ok(_) => Some(true),
                Err(VfsError::NotFound { .. } | VfsError::Io { .. }) => Some(false),
                Err(_) => None,
            }
        }
        Ok(_) => Some(true),
        Err(VfsError::NotFound { .. }) => Some(false),
        Err(_) => None,
    }
}

/// Resolves `relative` (decoded, `/` or `\` separated) against the directory holding `source`.
fn resolve_relative(source: &Location, relative: &str, backslashes: bool) -> Option<Location> {
    let normalised = if backslashes {
        relative.replace('\\', "/")
    } else {
        relative.to_owned()
    };
    if let Some(absolute) = normalised.strip_prefix('/') {
        // Absolute filesystem paths only make sense for local sources.
        if source.provider_id.as_str() != "local" {
            return None;
        }
        return Location::from_native_path(Path::new(&format!("/{absolute}"))).ok();
    }
    if normalised.len() > 1 && normalised.as_bytes()[1] == b':' {
        // A Windows drive path: only checkable where drives exist.
        return if cfg!(windows) && source.provider_id.as_str() == "local" {
            Location::from_native_path(Path::new(&normalised)).ok()
        } else {
            None
        };
    }
    let mut current = source.parent().ok()??;
    for segment in normalised.split('/') {
        match segment {
            "" | "." => {}
            ".." => current = current.parent().ok()??,
            name => current = current.join(name).ok()?,
        }
    }
    Some(current)
}

async fn read_text(
    providers: &ProviderRegistry,
    entry: &EntrySummary,
    cancellation: &CancellationToken,
) -> Option<String> {
    if entry.size.is_some_and(|size| size > MAX_TEXT_BYTES) {
        return None;
    }
    let provider = providers.resolve(&entry.location).ok()?;
    let reference = EntryRef {
        id: entry.id,
        location: entry.location.clone(),
    };
    let reader = provider
        .open_read(&reference, cancellation.clone())
        .await
        .ok()?;
    let mut bytes = Vec::new();
    reader
        .take(MAX_TEXT_BYTES)
        .read_to_end(&mut bytes)
        .await
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_bytes(
    providers: &ProviderRegistry,
    entry: &EntrySummary,
    limit: u64,
    cancellation: &CancellationToken,
) -> Option<Vec<u8>> {
    let provider = providers.resolve(&entry.location).ok()?;
    let reference = EntryRef {
        id: entry.id,
        location: entry.location.clone(),
    };
    let reader = provider
        .open_read(&reference, cancellation.clone())
        .await
        .ok()?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes).await.ok()?;
    Some(bytes)
}

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
}

/// Checks the text links of one file and records the broken ones.
async fn check_links(
    providers: &ProviderRegistry,
    handle: &ScanHandle,
    entry: &EntrySummary,
    kind: LinkProblemKindDto,
    links: Vec<ExtractedLink>,
    backslashes: bool,
) {
    for link in links {
        if handle.cancellation.is_cancelled() {
            return;
        }
        let Some(relative) = relative_path_target(&link.target) else {
            continue;
        };
        let Some(resolved) = resolve_relative(&entry.location, &relative, backslashes) else {
            continue;
        };
        handle.update(|state| state.links_checked += 1);
        if target_exists(providers, &resolved, &handle.cancellation).await == Some(false) {
            record(
                handle,
                LinkProblemDto {
                    kind,
                    source: entry.location.clone().into(),
                    target: link.target,
                    resolved: Some(resolved.into()),
                    line: Some(link.line),
                },
            );
        }
    }
}

fn record(handle: &ScanHandle, problem: LinkProblemDto) {
    handle.update(|state| {
        if state.problems.len() < MAX_PROBLEMS {
            state.problems.push(problem);
        }
    });
}

async fn check_entry(providers: &ProviderRegistry, handle: &ScanHandle, entry: &EntrySummary) {
    let cancellation = &handle.cancellation;
    match entry.kind {
        EntryKind::Symlink => {
            if entry.location.provider_id.as_str() != "local" {
                return;
            }
            handle.update(|state| state.links_checked += 1);
            if target_exists(providers, &entry.location, cancellation).await == Some(false) {
                let target = entry
                    .location
                    .to_native_path()
                    .ok()
                    .and_then(|path| std::fs::read_link(path).ok())
                    .map(|target| target.to_string_lossy().into_owned())
                    .unwrap_or_default();
                record(
                    handle,
                    LinkProblemDto {
                        kind: LinkProblemKindDto::BrokenSymlink,
                        source: entry.location.clone().into(),
                        target,
                        resolved: None,
                        line: None,
                    },
                );
            }
        }
        EntryKind::File => {
            let Some(extension) = extension(&entry.name) else {
                return;
            };
            if extension == "lnk" {
                if let Some(bytes) = read_bytes(providers, entry, 64 * 1024, cancellation).await
                    && let Some(target) = shortcut_target(&bytes)
                    && let Some(resolved) = resolve_relative(&entry.location, &target, true)
                {
                    handle.update(|state| state.links_checked += 1);
                    if target_exists(providers, &resolved, cancellation).await == Some(false) {
                        record(
                            handle,
                            LinkProblemDto {
                                kind: LinkProblemKindDto::BrokenShortcut,
                                source: entry.location.clone().into(),
                                target,
                                resolved: Some(resolved.into()),
                                line: None,
                            },
                        );
                    }
                }
            } else if let Some(kind) = PlaylistKind::from_extension(&extension) {
                if let Some(text) = read_text(providers, entry, cancellation).await {
                    let links = playlist_links(kind, &text);
                    check_links(
                        providers,
                        handle,
                        entry,
                        LinkProblemKindDto::MissingPlaylistEntry,
                        links,
                        true,
                    )
                    .await;
                }
            } else if matches!(extension.as_str(), "md" | "markdown" | "html" | "htm")
                && let Some(text) = read_text(providers, entry, cancellation).await
            {
                let links = if extension.starts_with('h') {
                    html_links(&text)
                } else {
                    markdown_links(&text)
                };
                check_links(
                    providers,
                    handle,
                    entry,
                    LinkProblemKindDto::MissingDocumentLink,
                    links,
                    false,
                )
                .await;
            }
        }
        EntryKind::Directory => {}
    }
}

async fn run_scan(handle: Arc<ScanHandle>, providers: ProviderRegistry, show_hidden: bool) {
    let (entries, _) = walk(
        &providers,
        &handle.roots,
        show_hidden,
        &handle.cancellation,
        usize::MAX,
    )
    .await;
    for entry in &entries {
        if handle.cancellation.is_cancelled() {
            break;
        }
        handle.update(|state| state.entries_scanned += 1);
        check_entry(&providers, &handle, entry).await;
    }
    let cancelled = handle.cancellation.is_cancelled();
    handle.update(|state| {
        state.complete = true;
        state.cancelled = cancelled;
    });
}

impl LinkDoctorCoordinator {
    /// Starts a scan on the current Tokio runtime.
    pub(crate) fn start(
        &self,
        providers: ProviderRegistry,
        request: StartLinkDoctorScanRequestDto,
    ) -> Result<StartLinkDoctorScanResponseDto, ApplicationError> {
        if request.roots.is_empty() {
            return Err(ApplicationError::InvalidRequest(
                "at least one root must be supplied".to_owned(),
            ));
        }
        let scan_id = Uuid::new_v4();
        let handle = Arc::new(ScanHandle {
            roots: request.roots.into_iter().map(Into::into).collect(),
            cancellation: CancellationToken::new(),
            state: Mutex::new(ScanState {
                entries_scanned: 0,
                links_checked: 0,
                problems: Vec::new(),
                complete: false,
                cancelled: false,
            }),
        });
        self.scans
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(scan_id, Arc::clone(&handle));
        tokio::spawn(run_scan(handle, providers, request.show_hidden));
        Ok(StartLinkDoctorScanResponseDto { scan_id })
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

    /// A bounded page of a scan's problems plus its progress.
    pub(crate) fn page(
        &self,
        scan_id: Uuid,
        offset: u64,
        limit: u16,
    ) -> Result<LinkDoctorPageDto, ApplicationError> {
        let handle = self.handle(scan_id)?;
        let limit = limit.clamp(1, 500);
        let state = handle.state.lock().unwrap_or_else(PoisonError::into_inner);
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(state.problems.len());
        let end = (start + usize::from(limit)).min(state.problems.len());
        Ok(LinkDoctorPageDto {
            scan_id,
            roots: handle.roots.iter().cloned().map(Into::into).collect(),
            entries_scanned: state.entries_scanned,
            links_checked: state.links_checked,
            offset,
            limit,
            total: state.problems.len() as u64,
            problems: state.problems[start..end].to_vec(),
            is_complete: state.complete,
            is_cancelled: state.cancelled,
            has_more: end < state.problems.len(),
        })
    }
}

/// Searches `roots` for files named like a missing target, best matches first.
pub(crate) async fn find_replacements(
    providers: &ProviderRegistry,
    request: FindLinkReplacementsRequestDto,
) -> Result<FindLinkReplacementsResponseDto, ApplicationError> {
    if request.roots.is_empty() || request.target_name.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest(
            "roots and a target name are required".to_owned(),
        ));
    }
    let roots: Vec<Location> = request.roots.into_iter().map(Into::into).collect();
    let wanted = request.target_name.to_lowercase();
    let (entries, truncated) = walk(
        providers,
        &roots,
        request.show_hidden,
        &CancellationToken::new(),
        REPLACEMENT_ENTRY_BUDGET,
    )
    .await;
    let mut candidates: Vec<LinkReplacementCandidateDto> = entries
        .into_iter()
        .filter(|entry| entry.kind != EntryKind::Symlink && entry.name.to_lowercase() == wanted)
        .map(|entry| {
            let size = entry.size.unwrap_or(0);
            LinkReplacementCandidateDto {
                same_size: request
                    .expected_size
                    .is_some_and(|expected| expected == size),
                exact_name: entry.name == request.target_name,
                size,
                location: entry.location.into(),
            }
        })
        .collect();
    candidates.sort_by_key(|candidate| {
        (
            std::cmp::Reverse(candidate.same_size),
            std::cmp::Reverse(candidate.exact_name),
            candidate.location.uri.clone(),
        )
    });
    candidates.truncate(MAX_REPLACEMENT_CANDIDATES);
    Ok(FindLinkReplacementsResponseDto {
        candidates,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_links_resolve_against_the_source_directory() {
        let source = Location::parse("file:///notes/sub/readme.md").expect("location");
        let resolve = |relative: &str| {
            resolve_relative(&source, relative, false).map(|location| location.uri)
        };
        assert_eq!(resolve("a.md").as_deref(), Some("file:///notes/sub/a.md"));
        assert_eq!(
            resolve("../b/c d.md").as_deref(),
            Some("file:///notes/b/c%20d.md")
        );
        assert_eq!(
            resolve("./x/./y.md").as_deref(),
            Some("file:///notes/sub/x/y.md")
        );
    }

    #[test]
    fn climbing_above_the_root_is_unresolvable() {
        let source = Location::parse("file:///readme.md").expect("location");
        assert!(resolve_relative(&source, "../../x.md", false).is_none());
    }

    #[test]
    fn foreign_drive_paths_are_not_checked_off_windows() {
        let source = Location::parse("file:///music/list.m3u").expect("location");
        if !cfg!(windows) {
            assert!(resolve_relative(&source, "C:\\Music\\a.mp3", true).is_none());
        }
    }
}
