//! Splitting a file into numbered parts and joining them again (task 0257).
//!
//! Both directions are cancellable operation-engine jobs that stream through the VFS provider
//! traits, so a multi-gigabyte file is never buffered and source and target may live on different
//! providers. Output is written through private temporary files and published with
//! `commit_copy`, which refuses to replace an existing entry unless the user chose to overwrite;
//! conflicts use the shared conflict dialog. A split can write a BLAKE3 manifest that a later
//! combine verifies before publishing anything.

use crate::rejection::{Rejection, reason};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryKind, EntrySummary, Location};
use fm_operations::{
    ConflictResolution, EntryFingerprint, ExecutionError, ExecutionOutcome, Operation,
    OperationEntryError, OperationExecutor, OperationPlan, OperationProgressReporter,
    OperationUndo, PauseToken, PlanItem, UndoAction, UndoPlan,
};
use fm_transport_dto::{
    InspectSplitSetRequestDto, InspectSplitSetResponseDto, SplitFileDto, SplitPartDto,
    StartOperationRequestDto,
};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ListOptions, ProviderCapabilities,
    ProviderRegistry, RemoveOptions, WriteOptions,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::image_metadata_edit::{entry_of, io_failure, sibling};
use crate::operation_planner::{conflict_error, copy_name, effective_resolution};
use crate::split_manifest::{
    DetectedSet, Manifest, ManifestPart, detect_set, manifest_name, part_count, part_name,
};

const MAX_SPLIT_SOURCES: usize = 200;
const MAX_COMBINE_SOURCES: usize = 100_000;
/// Parts one split may produce.
const MAX_PARTS: u64 = 10_000;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const IO_CHUNK: usize = 256 * 1024;
const LIST_PAGE_SIZE: usize = 1_000;
const MAX_RENAME_ATTEMPTS: u32 = 999;

/// A set problem as an operation failure that keeps the reason code and parameters.
fn rejection_failure(rejection: Rejection) -> fm_operations::CodedFailure {
    let mut failure = fm_operations::CodedFailure::new(rejection.reason, rejection.message);
    failure.params = rejection.params;
    failure
}

fn cancelled() -> ExecutionError {
    fm_vfs::VfsError::Cancelled.into()
}

/// A failure whose reason the user must see; the engine reports it as an entry error.
fn warn(entry: &EntryRef, message: String, code: &str, name: Option<&str>) -> ExecutionError {
    let mut error = OperationEntryError::new(entry.clone(), message).with_code(code);
    if let Some(name) = name {
        error = error.with_param("name", name);
    }
    ExecutionError::warning(error)
}

fn folder_of(location: &Location) -> Result<Location, ApplicationError> {
    location.parent().ok().flatten().ok_or_else(|| {
        ApplicationError::from(Rejection::new(
            reason::FILE_NEEDS_FOLDER,
            "a file needs a folder",
        ))
    })
}

fn require_write(providers: &ProviderRegistry, folder: &Location) -> Result<(), ApplicationError> {
    providers
        .resolve(folder)?
        .capabilities_for(folder)?
        .require(ProviderCapabilities::WRITE)?;
    Ok(())
}

/// Validates a `splitFile` request and builds its executor.
pub(crate) fn plan_split_file(
    providers: &ProviderRegistry,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    let split: SplitFileDto = request
        .split_file
        .ok_or_else(|| ApplicationError::InvalidRequest("splitFile requires splitFile".into()))?;
    if split.part_size_bytes == 0 {
        return Err(ApplicationError::from(Rejection::new(
            reason::PART_SIZE_TOO_SMALL,
            "the part size must be at least 1 byte",
        )));
    }
    if request.sources.is_empty() || request.sources.len() > MAX_SPLIT_SOURCES {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::SELECTION_SIZE,
                format!("splitFile needs between 1 and {MAX_SPLIT_SOURCES} files"),
            )
            .with("max", MAX_SPLIT_SOURCES),
        ));
    }
    let destination: Option<Location> = request.destination.clone().map(Into::into);
    let mut sources = Vec::with_capacity(request.sources.len());
    for source in &request.sources {
        let location: Location = source.clone().into();
        providers.resolve(&location)?;
        let folder = match &destination {
            Some(folder) => folder.clone(),
            None => folder_of(&location)?,
        };
        require_write(providers, &folder)?;
        sources.push((location, folder));
    }
    Ok(Arc::new(SplitExecutor {
        providers: providers.clone(),
        sources,
        part_size: split.part_size_bytes,
        write_manifest: split.write_manifest,
        state: Mutex::new(SplitState::default()),
    }))
}

/// Validates a `combineFiles` request and builds its executor.
pub(crate) fn plan_combine_files(
    providers: &ProviderRegistry,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    if request.sources.is_empty() || request.sources.len() > MAX_COMBINE_SOURCES {
        return Err(ApplicationError::from(
            Rejection::new(
                reason::SELECTION_SIZE,
                format!("combineFiles needs between 1 and {MAX_COMBINE_SOURCES} part files"),
            )
            .with("max", MAX_COMBINE_SOURCES),
        ));
    }
    let selected: Vec<Location> = request.sources.iter().cloned().map(Into::into).collect();
    for location in &selected {
        providers.resolve(location)?;
    }
    let destination: Option<Location> = request.destination.clone().map(Into::into);
    let folder = match &destination {
        Some(folder) => folder.clone(),
        None => folder_of(&selected[0])?,
    };
    require_write(providers, &folder)?;
    if let Some(name) = &request.name
        && folder.join(name).is_err()
    {
        return Err(ApplicationError::from(Rejection::new(
            reason::INVALID_OUTPUT_NAME,
            "the output name is not a valid file name",
        )));
    }
    Ok(Arc::new(CombineExecutor {
        providers: providers.clone(),
        selected,
        folder,
        name: request.name.clone(),
        state: Mutex::new(CombineState::default()),
    }))
}

/* -------------------------------------------------------------------------- */
/*  Shared helpers                                                            */
/* -------------------------------------------------------------------------- */

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

async fn exists(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    cancellation: &CancellationToken,
) -> Option<EntrySummary> {
    provider
        .inspect(&entry_of(location), cancellation.clone())
        .await
        .ok()
}

/// Fingerprint of an output whose BLAKE3 hash is already known, so undo needs no second read.
async fn known_fingerprint(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    hash: &str,
    cancellation: &CancellationToken,
) -> Result<EntryFingerprint, ExecutionError> {
    let summary = provider
        .inspect(&entry_of(location), cancellation.clone())
        .await?;
    Ok(EntryFingerprint {
        entry: EntryRef {
            id: summary.id,
            location: summary.location,
        },
        stable_id: false,
        kind: summary.kind,
        size: summary.size,
        modified_at: summary.modified_at,
        content_hash: Some(hash.to_owned()),
    })
}

async fn remove_quietly(providers: &ProviderRegistry, location: &Location) {
    if let Ok(provider) = providers.resolve(location) {
        let _ = provider
            .remove(
                &entry_of(location),
                RemoveOptions {
                    recursive: false,
                    use_trash: false,
                },
                CancellationToken::new(),
            )
            .await;
    }
}

async fn discard_temporary(providers: &ProviderRegistry, temporary: &Location) {
    if let Ok(provider) = providers.resolve(temporary) {
        let _ = provider
            .discard_copy(temporary, CancellationToken::new())
            .await;
    }
}

/// Writes `bytes` to a private temporary file and publishes it at `destination`.
async fn commit_bytes(
    providers: &ProviderRegistry,
    source: &EntryRef,
    destination: &Location,
    bytes: &[u8],
    overwrite: bool,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let provider = providers.resolve(destination)?;
    let temporary = sibling(destination, &format!(".fm-split-{}.tmp", Uuid::new_v4()))?;
    let outcome = async {
        let mut writer = provider
            .open_write(
                &temporary,
                WriteOptions { overwrite: false },
                cancellation.clone(),
            )
            .await?;
        writer.write_all(bytes).await.map_err(io_failure)?;
        writer.shutdown().await.map_err(io_failure)?;
        provider
            .commit_copy(
                source,
                &temporary,
                destination,
                CopyCommitOptions {
                    overwrite,
                    preserve_metadata: false,
                },
                cancellation.clone(),
            )
            .await?;
        Ok::<(), ExecutionError>(())
    }
    .await;
    if outcome.is_err() {
        discard_temporary(providers, &temporary).await;
    }
    outcome
}

/// Reads the next chunk of at most `IO_CHUNK` bytes and never more than `remaining`; `None` at
/// the end of the stream or when `remaining` is zero.
async fn next_chunk<R>(
    reader: &mut R,
    remaining: u64,
    cancellation: &CancellationToken,
) -> Result<Option<Vec<u8>>, ExecutionError>
where
    R: AsyncReadExt + Unpin + ?Sized,
{
    if remaining == 0 {
        return Ok(None);
    }
    if cancellation.is_cancelled() {
        return Err(cancelled());
    }
    let want = usize::try_from(remaining.min(IO_CHUNK as u64)).unwrap_or(IO_CHUNK);
    let mut buffer = vec![0_u8; want];
    let read = reader.read(&mut buffer).await.map_err(io_failure)?;
    if read == 0 {
        return Ok(None);
    }
    buffer.truncate(read);
    Ok(Some(buffer))
}

/// Which of the colliding names was found, if any.
struct Collision {
    existing: EntrySummary,
}

/// Picks the final base name for a set of outputs: asks, skips, overwrites or renames when one of
/// the `probe` names (built from a base name) already exists.
/// Everything `resolve_name` needs to decide about a colliding output name.
struct NameRequest<'a> {
    provider: &'a Arc<dyn FileSystemProvider>,
    folder: &'a Location,
    base: &'a str,
    operation: &'a Operation,
    resolution: Option<ConflictResolution>,
    source: &'a EntrySummary,
    cancellation: &'a CancellationToken,
}

async fn resolve_name<P>(
    request: NameRequest<'_>,
    probe: P,
) -> Result<Option<(String, bool)>, ExecutionError>
where
    P: Fn(&str) -> Vec<String>,
{
    async fn first_collision(
        provider: &Arc<dyn FileSystemProvider>,
        folder: &Location,
        names: Vec<String>,
        cancellation: &CancellationToken,
    ) -> Result<Option<Collision>, ExecutionError> {
        for name in names {
            let location = folder
                .join(&name)
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            if let Some(existing) = exists(provider, &location, cancellation).await {
                return Ok(Some(Collision { existing }));
            }
        }
        Ok(None)
    }

    let NameRequest {
        provider,
        folder,
        base,
        operation,
        resolution,
        source,
        cancellation,
    } = request;
    let Some(collision) = first_collision(provider, folder, probe(base), cancellation).await?
    else {
        return Ok(Some((base.to_owned(), false)));
    };
    match effective_resolution(operation.conflict_policy, resolution) {
        None => Err(conflict_error(source, &collision.existing)),
        Some(ConflictResolution::Skip) => Ok(None),
        Some(ConflictResolution::Overwrite) => {
            if collision.existing.kind != EntryKind::File {
                return Err(ExecutionError::from(fm_operations::CodedFailure::new(
                    fm_operations::codes::failure::FOLDER_REPLACE_BLOCKED,
                    "a folder cannot be replaced by a file",
                )));
            }
            Ok(Some((base.to_owned(), true)))
        }
        Some(ConflictResolution::RenameNew) => {
            for suffix in 1..=MAX_RENAME_ATTEMPTS {
                let candidate = copy_name(base, suffix);
                if first_collision(provider, folder, probe(&candidate), cancellation)
                    .await?
                    .is_none()
                {
                    return Ok(Some((candidate, false)));
                }
            }
            Err(ExecutionError::from(fm_operations::CodedFailure::new(
                fm_operations::codes::failure::NAME_COLLISION_LIMIT,
                "too many files with this name already exist",
            )))
        }
    }
}

/* -------------------------------------------------------------------------- */
/*  Split                                                                     */
/* -------------------------------------------------------------------------- */

#[derive(Default)]
struct SplitState {
    /// Committed outputs (location, BLAKE3 hash), for undo.
    created: Vec<(Location, String)>,
    /// Outputs of the item being written, removed again when it fails or is cancelled.
    in_progress: Vec<Location>,
    /// Private temporary file being written.
    temporary: Option<Location>,
    /// An existing file was replaced, which undo cannot bring back.
    replaced: bool,
}

/// Where and under which name one source file is split.
#[derive(Clone, Copy)]
struct SplitTarget<'a> {
    folder: &'a Location,
    base: &'a str,
    overwrite: bool,
    size: u64,
}

struct SplitExecutor {
    providers: ProviderRegistry,
    /// Source file and the folder its parts go to.
    sources: Vec<(Location, Location)>,
    part_size: u64,
    write_manifest: bool,
    state: Mutex<SplitState>,
}

impl SplitExecutor {
    /// Names whose existence makes a split of `parts` parts conflict: the first and last part, the
    /// manifest, and the part after the last (a stale leftover that would corrupt a later join).
    fn probe(&self, parts: u64) -> impl Fn(&str) -> Vec<String> + use<> {
        let manifest = self.write_manifest;
        move |base: &str| {
            let mut names = vec![
                part_name(base, 1),
                part_name(base, u32::try_from(parts).unwrap_or(u32::MAX)),
                part_name(base, u32::try_from(parts + 1).unwrap_or(u32::MAX)),
            ];
            if manifest {
                names.push(manifest_name(base));
            }
            names.dedup();
            names
        }
    }

    async fn write_parts(
        &self,
        item: &PlanItem,
        target: &SplitTarget<'_>,
        progress: &dyn OperationProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<(), ExecutionError> {
        let SplitTarget {
            folder,
            base,
            overwrite,
            size,
        } = *target;
        let source_provider = self.providers.resolve(&item.entry.location)?;
        let folder_provider = self.providers.resolve(folder)?;
        let mut reader = source_provider
            .open_read(&item.entry, cancellation.clone())
            .await?;
        let parts = part_count(size, self.part_size);
        let mut whole = blake3::Hasher::new();
        let mut manifest_parts = Vec::new();
        for index in 1..=parts {
            let name = part_name(
                base,
                u32::try_from(index)
                    .map_err(|_| ExecutionError::Failed("too many parts".into()))?,
            );
            let destination = folder
                .join(&name)
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            let temporary = sibling(&destination, &format!(".fm-split-{}.tmp", Uuid::new_v4()))?;
            lock(&self.state).temporary = Some(temporary.clone());
            let mut writer = folder_provider
                .open_write(
                    &temporary,
                    WriteOptions { overwrite: false },
                    cancellation.clone(),
                )
                .await?;
            let mut part_hash = blake3::Hasher::new();
            let remaining = size.saturating_sub((index - 1) * self.part_size);
            let want = remaining.min(self.part_size);
            let mut written = 0_u64;
            while let Some(chunk) = next_chunk(&mut reader, want - written, cancellation).await? {
                part_hash.update(&chunk);
                whole.update(&chunk);
                writer.write_all(&chunk).await.map_err(io_failure)?;
                progress.report_bytes(chunk.len() as u64);
                written += chunk.len() as u64;
            }
            if written != want {
                return Err(ExecutionError::from(fm_operations::CodedFailure::new(
                    fm_operations::codes::failure::SOURCE_CHANGED_WHILE_SPLITTING,
                    "the source file changed while it was being split",
                )));
            }
            writer.shutdown().await.map_err(io_failure)?;
            drop(writer);
            folder_provider
                .commit_copy(
                    &item.entry,
                    &temporary,
                    &destination,
                    CopyCommitOptions {
                        overwrite,
                        preserve_metadata: false,
                    },
                    cancellation.clone(),
                )
                .await?;
            let hash = part_hash.finalize().to_hex().to_string();
            {
                let mut state = lock(&self.state);
                state.temporary = None;
                state.in_progress.push(destination.clone());
                state.created.push((destination, hash.clone()));
            }
            manifest_parts.push(ManifestPart { name, hash });
        }
        if self.write_manifest {
            let manifest = Manifest {
                name: base.to_owned(),
                size,
                part_size: self.part_size,
                whole_hash: whole.finalize().to_hex().to_string(),
                parts: manifest_parts,
            };
            let text = manifest.render();
            let destination = folder
                .join(&manifest_name(base))
                .map_err(|error| ExecutionError::Failed(error.to_string()))?;
            commit_bytes(
                &self.providers,
                &item.entry,
                &destination,
                text.as_bytes(),
                overwrite,
                cancellation,
            )
            .await?;
            let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
            let mut state = lock(&self.state);
            state.in_progress.push(destination.clone());
            state.created.push((destination, hash));
        }
        Ok(())
    }

    /// Removes what the failed or cancelled item had published, then its temporary file.
    async fn discard_item(&self) {
        let (outputs, temporary) = {
            let mut state = lock(&self.state);
            let outputs = std::mem::take(&mut state.in_progress);
            state
                .created
                .retain(|(location, _)| !outputs.contains(location));
            (outputs, state.temporary.take())
        };
        for location in outputs {
            remove_quietly(&self.providers, &location).await;
        }
        if let Some(temporary) = temporary {
            discard_temporary(&self.providers, &temporary).await;
        }
    }

    /// Deletes parts numbered after `last` that an earlier, larger split left behind.
    async fn remove_stale_parts(&self, folder: &Location, base: &str, last: u64) {
        let Ok(provider) = self.providers.resolve(folder) else {
            return;
        };
        let mut index = last + 1;
        loop {
            let Ok(index_u32) = u32::try_from(index) else {
                return;
            };
            let Ok(location) = folder.join(&part_name(base, index_u32)) else {
                return;
            };
            if exists(&provider, &location, &CancellationToken::new())
                .await
                .is_none()
            {
                return;
            }
            remove_quietly(&self.providers, &location).await;
            index += 1;
        }
    }
}

#[async_trait]
impl OperationExecutor for SplitExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::with_capacity(self.sources.len());
        for (location, _) in &self.sources {
            let provider = self.providers.resolve(location)?;
            let summary = provider
                .inspect(&entry_of(location), cancellation.clone())
                .await?;
            if summary.kind != EntryKind::File {
                return Err(ExecutionError::from(
                    fm_operations::CodedFailure::new(
                        fm_operations::codes::failure::NOT_REGULAR_FILE,
                        format!("{} is not a regular file", summary.name),
                    )
                    .with_param("name", &summary.name),
                ));
            }
            let size = summary.size.unwrap_or(0);
            if part_count(size, self.part_size) > MAX_PARTS {
                return Err(ExecutionError::from(
                    fm_operations::CodedFailure::new(
                        fm_operations::codes::failure::TOO_MANY_PARTS,
                        format!(
                            "{} would need more than {MAX_PARTS} parts; choose a larger part size",
                            summary.name
                        ),
                    )
                    .with_param("name", &summary.name)
                    .with_param("max", MAX_PARTS),
                ));
            }
            items.push(PlanItem::new(
                EntryRef {
                    id: summary.id,
                    location: summary.location,
                },
                size,
            ));
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let folder = self
            .sources
            .iter()
            .find(|(location, _)| *location == item.entry.location)
            .map(|(_, folder)| folder.clone())
            .ok_or_else(|| ExecutionError::Failed("split plan entry is missing".into()))?;
        let source_provider = self.providers.resolve(&item.entry.location)?;
        let source = source_provider
            .inspect(&item.entry, cancellation.clone())
            .await?;
        let size = source.size.unwrap_or(0);
        let parts = part_count(size, self.part_size);
        let folder_provider = self.providers.resolve(&folder)?;
        let Some((base, overwrite)) = resolve_name(
            NameRequest {
                provider: &folder_provider,
                folder: &folder,
                base: &source.name,
                operation,
                resolution,
                source: &source,
                cancellation,
            },
            self.probe(parts),
        )
        .await?
        else {
            return Ok(ExecutionOutcome::Skipped);
        };
        let written = self
            .write_parts(
                item,
                &SplitTarget {
                    folder: &folder,
                    base: &base,
                    overwrite,
                    size,
                },
                progress,
                cancellation,
            )
            .await;
        if let Err(error) = written {
            self.discard_item().await;
            return Err(error);
        }
        lock(&self.state).in_progress.clear();
        if overwrite {
            lock(&self.state).replaced = true;
            self.remove_stale_parts(&folder, &base, parts).await;
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        self.discard_item().await;
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let (created, replaced) = {
            let state = lock(&self.state);
            (state.created.clone(), state.replaced)
        };
        if replaced {
            return Ok(OperationUndo::unavailable(
                fm_operations::codes::undo::REPLACED_EXISTING,
                "Existing files were replaced, which cannot be brought back.",
            ));
        }
        if created.is_empty() {
            return Ok(OperationUndo::unavailable(
                fm_operations::codes::undo::NOTHING_TO_UNDO,
                "No file was created.",
            ));
        }
        let mut entries = Vec::with_capacity(created.len());
        for (location, hash) in created {
            let provider = self.providers.resolve(&location)?;
            entries.push(known_fingerprint(&provider, &location, &hash, cancellation).await?);
        }
        Ok(OperationUndo::available(UndoPlan {
            actions: vec![UndoAction::RemoveCreated { entries }],
        }))
    }
}

/* -------------------------------------------------------------------------- */
/*  Set detection                                                             */
/* -------------------------------------------------------------------------- */

/// A split set that can be joined.
struct LoadedSet {
    base: String,
    /// Parts in order.
    parts: Vec<(Location, EntrySummary)>,
    manifest: Option<Manifest>,
}

impl LoadedSet {
    fn total_size(&self) -> u64 {
        self.parts
            .iter()
            .map(|(_, summary)| summary.size.unwrap_or(0))
            .sum()
    }
}

async fn list_folder(
    provider: &Arc<dyn FileSystemProvider>,
    folder: &Location,
    cancellation: &CancellationToken,
) -> Result<Vec<EntrySummary>, ApplicationError> {
    let mut entries = Vec::new();
    let mut token = None;
    loop {
        let page = provider
            .list(
                folder,
                ListOptions {
                    page_size: LIST_PAGE_SIZE,
                    continuation_token: token.take(),
                },
                cancellation.clone(),
            )
            .await?;
        entries.extend(page.entries);
        if !page.has_more || page.continuation_token.is_none() {
            return Ok(entries);
        }
        token = page.continuation_token;
    }
}

async fn read_manifest(
    provider: &Arc<dyn FileSystemProvider>,
    summary: &EntrySummary,
    cancellation: &CancellationToken,
) -> Result<Manifest, Rejection> {
    let damaged = |detail: String| {
        Rejection::new(reason::SPLIT_MANIFEST_DAMAGED, detail).with("name", &summary.name)
    };
    if summary.size.unwrap_or(0) > MAX_MANIFEST_BYTES {
        return Err(damaged("the manifest is too large".into()));
    }
    let mut reader = provider
        .open_read(&entry_of(&summary.location), cancellation.clone())
        .await
        .map_err(|error| damaged(error.to_string()))?;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| damaged(error.to_string()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| damaged("the manifest is not text".to_owned()))?;
    Manifest::parse(&text).map_err(damaged)
}

/// The set the selection belongs to, or the reason it cannot be joined. `Err(ApplicationError)`
/// is a failure to read the folder at all.
async fn load_set(
    providers: &ProviderRegistry,
    selected: &[Location],
    cancellation: &CancellationToken,
) -> Result<Result<LoadedSet, (Rejection, Option<DetectedSet>)>, ApplicationError> {
    let no_part = || Rejection::new(reason::SPLIT_NO_PART_SELECTED, "no part selected");
    let Some(first) = selected.first() else {
        return Ok(Err((no_part(), None)));
    };
    let folder = folder_of(first)?;
    let mut names = Vec::with_capacity(selected.len());
    for location in selected {
        if folder_of(location)? != folder {
            return Ok(Err((
                Rejection::new(
                    reason::SPLIT_DIFFERENT_FOLDERS,
                    "the selected parts are in different folders",
                ),
                None,
            )));
        }
        names.push(location.name().unwrap_or_default());
    }
    let provider = providers.resolve(&folder)?;
    let entries = list_folder(&provider, &folder, cancellation).await?;
    let Some(set) = detect_set(
        names.iter().map(String::as_str),
        entries.iter().map(|entry| entry.name.as_str()),
    ) else {
        return Ok(Err((no_part(), None)));
    };
    if let Some(problem) = &set.problem {
        return Ok(Err((problem.describe(&set.base), Some(set))));
    }
    let mut by_index: HashMap<u32, &EntrySummary> = HashMap::new();
    for entry in &entries {
        if entry.kind != EntryKind::File {
            continue;
        }
        if let Some((base, index)) = crate::split_manifest::parse_part_name(&entry.name)
            && base == set.base
        {
            let canonical = entry.name == part_name(base, index);
            if canonical || !by_index.contains_key(&index) {
                by_index.insert(index, entry);
            }
        }
    }
    let mut parts = Vec::with_capacity(by_index.len());
    for index in &set.present {
        let Some(entry) = by_index.get(index) else {
            let name = part_name(&set.base, *index);
            return Ok(Err((
                Rejection::new(reason::SPLIT_PART_NOT_FILE, format!("{name} is not a file"))
                    .with("name", name),
                Some(set),
            )));
        };
        parts.push((entry.location.clone(), (*entry).clone()));
    }
    let manifest_file = entries
        .iter()
        .find(|entry| entry.name == manifest_name(&set.base) && entry.kind == EntryKind::File);
    let manifest = match manifest_file {
        None => None,
        Some(summary) => match read_manifest(&provider, summary, cancellation).await {
            Ok(manifest) => Some(manifest),
            Err(rejection) => {
                return Ok(Err((rejection, Some(set))));
            }
        },
    };
    let loaded = LoadedSet {
        base: set.base.clone(),
        parts,
        manifest,
    };
    if let Some(problem) = manifest_mismatch(&loaded) {
        return Ok(Err((problem, Some(set))));
    }
    Ok(Ok(loaded))
}

/// Compares the parts found with what the manifest promises (names and count).
fn manifest_mismatch(set: &LoadedSet) -> Option<Rejection> {
    let manifest = set.manifest.as_ref()?;
    if manifest.parts.len() != set.parts.len() {
        return Some(
            Rejection::new(
                reason::SPLIT_MANIFEST_COUNT,
                format!(
                    "the manifest lists {} parts but {} were found",
                    manifest.parts.len(),
                    set.parts.len()
                ),
            )
            .with("listed", manifest.parts.len())
            .with("found", set.parts.len()),
        );
    }
    for ((_, summary), listed) in set.parts.iter().zip(&manifest.parts) {
        if summary.name != listed.name {
            return Some(
                Rejection::new(
                    reason::SPLIT_MANIFEST_NAME,
                    format!(
                        "the manifest expects {} where {} was found",
                        listed.name, summary.name
                    ),
                )
                .with("expected", &listed.name)
                .with("found", &summary.name),
            );
        }
    }
    None
}

/// Detects the split set a selection of part files belongs to.
pub(crate) async fn inspect_split_set(
    providers: &ProviderRegistry,
    request: InspectSplitSetRequestDto,
) -> Result<InspectSplitSetResponseDto, ApplicationError> {
    if request.locations.is_empty() || request.locations.len() > MAX_COMBINE_SOURCES {
        return Err(ApplicationError::from(Rejection::new(
            reason::SELECT_PART_FILES,
            "select one or more part files",
        )));
    }
    let selected: Vec<Location> = request.locations.into_iter().map(Into::into).collect();
    let cancellation = CancellationToken::new();
    Ok(match load_set(providers, &selected, &cancellation).await? {
        Ok(set) => InspectSplitSetResponseDto {
            output_name: set.base.clone(),
            total_size: set.total_size(),
            has_manifest: set.manifest.is_some(),
            parts: set
                .parts
                .iter()
                .map(|(_, summary)| SplitPartDto {
                    name: summary.name.clone(),
                    size: summary.size.unwrap_or(0),
                })
                .collect(),
            error: None,
            error_code: None,
            error_params: std::collections::BTreeMap::new(),
        },
        Err((rejection, detected)) => InspectSplitSetResponseDto {
            output_name: detected.map(|set| set.base).unwrap_or_default(),
            parts: Vec::new(),
            total_size: 0,
            has_manifest: false,
            error: Some(rejection.message),
            error_code: Some(rejection.reason),
            error_params: rejection.params,
        },
    })
}

/* -------------------------------------------------------------------------- */
/*  Combine                                                                   */
/* -------------------------------------------------------------------------- */

#[derive(Default)]
struct CombineState {
    /// Published output (location, BLAKE3 hash), for undo.
    created: Option<(Location, String)>,
    temporary: Option<Location>,
    replaced: bool,
}

struct CombineExecutor {
    providers: ProviderRegistry,
    selected: Vec<Location>,
    folder: Location,
    name: Option<String>,
    state: Mutex<CombineState>,
}

impl CombineExecutor {
    async fn load(&self, cancellation: &CancellationToken) -> Result<LoadedSet, ExecutionError> {
        load_set(&self.providers, &self.selected, cancellation)
            .await
            .map_err(|error| ExecutionError::Failed(error.to_string()))?
            .map_err(|(rejection, _)| ExecutionError::from(rejection_failure(rejection)))
    }

    /// Streams every part into a temporary file, verifying against the manifest as it goes.
    /// Returns the BLAKE3 hash of the joined content.
    async fn join(
        &self,
        set: &LoadedSet,
        entry: &EntryRef,
        temporary: &Location,
        progress: &dyn OperationProgressReporter,
        cancellation: &CancellationToken,
    ) -> Result<String, ExecutionError> {
        let target_provider = self.providers.resolve(temporary)?;
        let mut writer = target_provider
            .open_write(
                temporary,
                WriteOptions { overwrite: false },
                cancellation.clone(),
            )
            .await?;
        let mut whole = blake3::Hasher::new();
        let mut total = 0_u64;
        for (index, (location, summary)) in set.parts.iter().enumerate() {
            let provider = self.providers.resolve(location)?;
            let mut reader = provider
                .open_read(&entry_of(location), cancellation.clone())
                .await?;
            let mut part_hash = blake3::Hasher::new();
            let expected = summary.size.unwrap_or(0);
            let mut copied = 0_u64;
            while let Some(chunk) = next_chunk(&mut reader, u64::MAX, cancellation).await? {
                part_hash.update(&chunk);
                whole.update(&chunk);
                writer.write_all(&chunk).await.map_err(io_failure)?;
                progress.report_bytes(chunk.len() as u64);
                copied += chunk.len() as u64;
            }
            if copied != expected {
                return Err(warn(
                    entry,
                    format!("{} changed while it was being joined", summary.name),
                    fm_operations::codes::entry_error::PART_CHANGED_WHILE_JOINING,
                    Some(&summary.name),
                ));
            }
            total += copied;
            if let Some(manifest) = &set.manifest
                && part_hash.finalize().to_hex().as_str() != manifest.parts[index].hash
            {
                return Err(warn(
                    entry,
                    format!(
                        "{} does not match the manifest; it is damaged or was changed. Nothing was written.",
                        summary.name
                    ),
                    fm_operations::codes::entry_error::PART_DAMAGED,
                    Some(&summary.name),
                ));
            }
        }
        writer.shutdown().await.map_err(io_failure)?;
        let hash = whole.finalize().to_hex().to_string();
        if let Some(manifest) = &set.manifest
            && (manifest.size != total || manifest.whole_hash != hash)
        {
            return Err(warn(
                entry,
                "The joined file does not match the manifest. Nothing was written.".into(),
                fm_operations::codes::entry_error::JOINED_FILE_MISMATCH,
                None,
            ));
        }
        Ok(hash)
    }
}

#[async_trait]
impl OperationExecutor for CombineExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let set = self.load(cancellation).await?;
        let (first, summary) = &set.parts[0];
        Ok(OperationPlan::new(vec![PlanItem::new(
            EntryRef {
                id: summary.id,
                location: first.clone(),
            },
            set.total_size(),
        )]))
    }

    async fn execute(
        &self,
        operation: &Operation,
        item: &PlanItem,
        resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let set = self.load(cancellation).await?;
        let provider = self.providers.resolve(&self.folder)?;
        let wanted = self.name.clone().unwrap_or_else(|| set.base.clone());
        let source = provider.inspect(&item.entry, cancellation.clone()).await?;
        let Some((name, overwrite)) = resolve_name(
            NameRequest {
                provider: &provider,
                folder: &self.folder,
                base: &wanted,
                operation,
                resolution,
                source: &source,
                cancellation,
            },
            |base| vec![base.to_owned()],
        )
        .await?
        else {
            return Ok(ExecutionOutcome::Skipped);
        };
        let destination = self
            .folder
            .join(&name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let temporary = sibling(&destination, &format!(".fm-combine-{}.tmp", Uuid::new_v4()))?;
        lock(&self.state).temporary = Some(temporary.clone());
        let joined = async {
            let hash = self
                .join(&set, &item.entry, &temporary, progress, cancellation)
                .await?;
            provider
                .commit_copy(
                    &item.entry,
                    &temporary,
                    &destination,
                    CopyCommitOptions {
                        overwrite,
                        preserve_metadata: false,
                    },
                    cancellation.clone(),
                )
                .await?;
            Ok::<String, ExecutionError>(hash)
        }
        .await;
        if joined.is_err() {
            discard_temporary(&self.providers, &temporary).await;
        }
        let hash = joined;
        let mut state = lock(&self.state);
        state.temporary = None;
        let hash = hash?;
        if overwrite {
            state.replaced = true;
        } else {
            state.created = Some((destination, hash));
        }
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        let temporary = lock(&self.state).temporary.take();
        if let Some(temporary) = temporary {
            discard_temporary(&self.providers, &temporary).await;
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let (created, replaced) = {
            let state = lock(&self.state);
            (state.created.clone(), state.replaced)
        };
        if replaced {
            return Ok(OperationUndo::unavailable(
                fm_operations::codes::undo::REPLACED_EXISTING,
                "An existing file was replaced, which cannot be brought back.",
            ));
        }
        let Some((location, hash)) = created else {
            return Ok(OperationUndo::unavailable(
                fm_operations::codes::undo::NOTHING_TO_UNDO,
                "No file was created.",
            ));
        };
        let provider = self.providers.resolve(&location)?;
        let entry = known_fingerprint(&provider, &location, &hash, cancellation).await?;
        Ok(OperationUndo::available(UndoPlan {
            actions: vec![UndoAction::RemoveCreated {
                entries: vec![entry],
            }],
        }))
    }
}
