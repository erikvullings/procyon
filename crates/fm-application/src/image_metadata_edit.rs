//! In-place image metadata editing as a cancellable operation (task 0274).
//!
//! Each source is read, rewritten by `fm_metadata::apply_image_edit` (lossless: only metadata
//! containers change), written to a private sibling temporary file and atomically swapped in with
//! the provider's commit primitive, the same way the text editor saves. An optional `.orig`
//! backup makes the edit undoable by putting the content back.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryId, EntryKind, Location};
use fm_metadata::{
    Change, EditableField, GpsPosition, ImageEdit, ImageEditError, apply_image_edit,
};
use fm_operations::{
    ConflictResolution, EntryFingerprint, ExecutionError, ExecutionOutcome, Operation,
    OperationEntryError, OperationExecutor, OperationPlan, OperationProgressReporter,
    OperationUndo, PauseToken, PlanItem, UndoAction, UndoPlan,
};
use fm_transport_dto::{ImageMetadataEditDto, StartOperationRequestDto};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry,
    RemoveOptions, WriteOptions,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::operation_planner::fingerprint;

/// Largest image rewritten in memory.
const MAX_EDITABLE_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SOURCES: usize = 5_000;
const MAX_BACKUP_SUFFIX: u32 = 99;

fn change<T>(set: Option<T>, field: EditableField, cleared: &[EditableField]) -> Change<T> {
    match set {
        Some(value) => Change::Set(value),
        None if cleared.contains(&field) => Change::Clear,
        None => Change::Keep,
    }
}

/// Converts the wire edit into the metadata crate's edit, rejecting unknown field ids.
pub(crate) fn image_edit_from_dto(
    dto: &ImageMetadataEditDto,
) -> Result<ImageEdit, ApplicationError> {
    const ALL: [EditableField; 9] = [
        EditableField::DateTaken,
        EditableField::Gps,
        EditableField::Description,
        EditableField::Copyright,
        EditableField::Author,
        EditableField::Orientation,
        EditableField::Title,
        EditableField::Keywords,
        EditableField::Rating,
    ];
    let mut cleared = Vec::new();
    for id in &dto.clear_fields {
        let field = ALL
            .iter()
            .copied()
            .find(|field| field.id() == id)
            .ok_or_else(|| {
                ApplicationError::InvalidRequest(format!("unknown metadata field '{id}'"))
            })?;
        cleared.push(field);
    }
    let date_taken = dto.date_taken.map(|time| fm_metadata::CaptureTime {
        year: time.year,
        month: time.month,
        day: time.day,
        hour: time.hour,
        minute: time.minute,
        second: time.second,
    });
    Ok(ImageEdit {
        remove_all: dto.remove_all,
        remove_location: dto.remove_location,
        date_taken: change(date_taken, EditableField::DateTaken, &cleared),
        shift_date_seconds: dto.shift_date_seconds,
        gps: change(
            dto.gps.map(|gps| GpsPosition {
                latitude: gps.latitude,
                longitude: gps.longitude,
            }),
            EditableField::Gps,
            &cleared,
        ),
        title: change(dto.title.clone(), EditableField::Title, &cleared),
        description: change(
            dto.description.clone(),
            EditableField::Description,
            &cleared,
        ),
        copyright: change(dto.copyright.clone(), EditableField::Copyright, &cleared),
        author: change(dto.author.clone(), EditableField::Author, &cleared),
        keywords: change(dto.keywords.clone(), EditableField::Keywords, &cleared),
        rating: change(dto.rating, EditableField::Rating, &cleared),
        orientation: change(dto.orientation, EditableField::Orientation, &cleared),
    })
}

/// Builds the executor for a validated `editImageMetadata` request.
pub(crate) fn plan_edit_image_metadata(
    providers: &ProviderRegistry,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    let dto = request.image_metadata_edit.as_ref().ok_or_else(|| {
        ApplicationError::InvalidRequest("editImageMetadata requires imageMetadataEdit".into())
    })?;
    if request.sources.is_empty() || request.sources.len() > MAX_SOURCES {
        return Err(ApplicationError::InvalidRequest(format!(
            "editImageMetadata needs between 1 and {MAX_SOURCES} files"
        )));
    }
    let edit = image_edit_from_dto(dto)?;
    let mut sources = Vec::with_capacity(request.sources.len());
    for source in &request.sources {
        let location: Location = source.clone().into();
        providers
            .resolve(&location)?
            .capabilities_for(&location)?
            .require(ProviderCapabilities::WRITE)?;
        sources.push(location);
    }
    Ok(Arc::new(EditImageMetadataExecutor {
        providers: providers.clone(),
        sources,
        edit,
        backup: dto.backup_original,
        edited: Mutex::new(Vec::new()),
        temporary: Mutex::new(None),
    }))
}

struct EditedFile {
    location: Location,
    backup: Option<Location>,
}

struct EditImageMetadataExecutor {
    providers: ProviderRegistry,
    sources: Vec<Location>,
    edit: ImageEdit,
    backup: bool,
    edited: Mutex<Vec<EditedFile>>,
    /// Private temporary file of the item being written, removed on cancellation or failure.
    temporary: Mutex<Option<Location>>,
}

pub(crate) fn entry_of(location: &Location) -> EntryRef {
    EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    }
}

pub(crate) fn io_failure(error: std::io::Error) -> ExecutionError {
    ExecutionError::Failed(error.to_string())
}

pub(crate) async fn read_all(
    provider: &Arc<dyn FileSystemProvider>,
    entry: &EntryRef,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, ExecutionError> {
    let mut reader = provider.open_read(entry, cancellation.clone()).await?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.map_err(io_failure)?;
    Ok(bytes)
}

/// Writes `bytes` to a new file; refuses to replace an existing one.
pub(crate) async fn write_new(
    provider: &Arc<dyn FileSystemProvider>,
    destination: &Location,
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let mut writer = provider
        .open_write(
            destination,
            WriteOptions { overwrite: false },
            cancellation.clone(),
        )
        .await?;
    writer.write_all(bytes).await.map_err(io_failure)?;
    writer.shutdown().await.map_err(io_failure)?;
    Ok(())
}

pub(crate) fn sibling(location: &Location, name: &str) -> Result<Location, ExecutionError> {
    location
        .parent()
        .map_err(|error| ExecutionError::Failed(error.to_string()))?
        .ok_or_else(|| ExecutionError::Failed("a filesystem root has no sibling".into()))?
        .join(name)
        .map_err(|error| ExecutionError::Failed(error.to_string()))
}

/// Replaces the content of `entry` with `bytes` through a private sibling temporary file.
pub(crate) async fn replace_content(
    provider: &Arc<dyn FileSystemProvider>,
    entry: &EntryRef,
    bytes: &[u8],
    temporary_slot: &Mutex<Option<Location>>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let temporary = sibling(&entry.location, &format!(".fm-meta-{}.tmp", Uuid::new_v4()))?;
    *temporary_slot
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some(temporary.clone());
    let outcome = async {
        write_new(provider, &temporary, bytes, cancellation).await?;
        provider
            .commit_copy(
                entry,
                &temporary,
                &entry.location,
                CopyCommitOptions {
                    overwrite: true,
                    preserve_metadata: true,
                },
                cancellation.clone(),
            )
            .await?;
        Ok::<(), ExecutionError>(())
    }
    .await;
    if outcome.is_err() {
        let _ = provider
            .discard_copy(&temporary, CancellationToken::new())
            .await;
    }
    *temporary_slot
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = None;
    outcome
}

pub(crate) async fn write_backup(
    provider: &Arc<dyn FileSystemProvider>,
    original: &Location,
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<Location, ExecutionError> {
    let name = original.name().unwrap_or_default();
    for suffix in 1..=MAX_BACKUP_SUFFIX {
        let backup_name = if suffix == 1 {
            format!("{name}.orig")
        } else {
            format!("{name}.orig{suffix}")
        };
        let backup = sibling(original, &backup_name)?;
        match write_new(provider, &backup, bytes, cancellation).await {
            Ok(()) => return Ok(backup),
            Err(ExecutionError::Provider(fm_vfs::VfsError::AlreadyExists { .. })) => {}
            Err(error) => return Err(error),
        }
    }
    Err(ExecutionError::Failed(
        "too many backup copies already exist next to this file".into(),
    ))
}

#[async_trait]
impl OperationExecutor for EditImageMetadataExecutor {
    async fn plan(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationPlan, ExecutionError> {
        let mut items = Vec::with_capacity(self.sources.len());
        for location in &self.sources {
            let provider = self.providers.resolve(location)?;
            let entry = entry_of(location);
            let summary = provider.inspect(&entry, cancellation.clone()).await?;
            if summary.kind != EntryKind::File {
                return Err(ExecutionError::Failed(format!(
                    "{} is not a regular file",
                    summary.name
                )));
            }
            items.push(PlanItem::new(
                EntryRef {
                    id: summary.id,
                    location: summary.location,
                },
                summary.size.unwrap_or(0),
            ));
        }
        Ok(OperationPlan::new(items))
    }

    async fn execute(
        &self,
        _operation: &Operation,
        item: &PlanItem,
        _resolution: Option<ConflictResolution>,
        progress: &dyn OperationProgressReporter,
        _pause: &PauseToken,
        cancellation: &CancellationToken,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let provider = self.providers.resolve(&item.entry.location)?;
        let size = provider
            .file_size(&item.entry, cancellation.clone())
            .await?;
        if size > MAX_EDITABLE_IMAGE_BYTES {
            return Err(ExecutionError::Failed(format!(
                "the image is too large to edit ({size} bytes; limit {MAX_EDITABLE_IMAGE_BYTES})"
            )));
        }
        let original = read_all(&provider, &item.entry, cancellation).await?;
        let edit = self.edit.clone();
        let source = original.clone();
        let edited = tokio::task::spawn_blocking(move || apply_image_edit(&source, &edit))
            .await
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let edited = match edited {
            Ok(bytes) => bytes,
            Err(
                error @ (ImageEditError::UnsupportedFormat | ImageEditError::UnsupportedField(_)),
            ) => {
                progress.report_bytes(size);
                return Ok(ExecutionOutcome::CompletedWithWarnings(vec![{
                    let failure = OperationEntryError::new(
                        item.entry.clone(),
                        format!("Not changed: {error}."),
                    );
                    match &error {
                        ImageEditError::UnsupportedField(field) => failure
                            .with_code(fm_operations::codes::entry_error::IMAGE_FIELD_UNSUPPORTED)
                            .with_param("field", field),
                        _ => failure
                            .with_code(fm_operations::codes::entry_error::IMAGE_FORMAT_UNSUPPORTED),
                    }
                }]));
            }
            Err(error) => return Err(ExecutionError::Failed(error.to_string())),
        };
        if edited == original {
            progress.report_bytes(size);
            return Ok(ExecutionOutcome::Completed);
        }
        let backup = if self.backup {
            Some(write_backup(&provider, &item.entry.location, &original, cancellation).await?)
        } else {
            None
        };
        if let Err(error) = replace_content(
            &provider,
            &item.entry,
            &edited,
            &self.temporary,
            cancellation,
        )
        .await
        {
            if let Some(backup) = &backup {
                // The original is intact; the unused backup would only be clutter.
                let _ = provider
                    .remove(
                        &entry_of(backup),
                        RemoveOptions {
                            recursive: false,
                            use_trash: false,
                        },
                        CancellationToken::new(),
                    )
                    .await;
            }
            return Err(error);
        }
        self.edited
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(EditedFile {
                location: item.entry.location.clone(),
                backup,
            });
        progress.report_bytes(size);
        Ok(ExecutionOutcome::Completed)
    }

    async fn cleanup_partial(&self, _operation: &Operation) -> Result<(), ExecutionError> {
        let temporary = self
            .temporary
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(temporary) = temporary
            && let Ok(provider) = self.providers.resolve(&temporary)
        {
            let _ = provider
                .discard_copy(&temporary, CancellationToken::new())
                .await;
        }
        Ok(())
    }

    async fn undo_evidence(
        &self,
        _operation: &Operation,
        cancellation: &CancellationToken,
    ) -> Result<OperationUndo, ExecutionError> {
        let edited: Vec<(Location, Option<Location>)> = self
            .edited
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .map(|file| (file.location.clone(), file.backup.clone()))
            .collect();
        if edited.is_empty() {
            return Ok(OperationUndo::unavailable("No file was changed."));
        }
        let mut actions = Vec::with_capacity(edited.len());
        for (location, backup) in edited {
            let Some(backup) = backup else {
                return Ok(OperationUndo::unavailable(
                    "Undo needs the backup copy; run the edit with \"keep original\" enabled.",
                ));
            };
            let provider = self.providers.resolve(&location)?;
            actions.push(UndoAction::RestoreContent {
                current: fingerprint(&provider, &location, cancellation).await?,
                backup,
            });
        }
        Ok(OperationUndo::available(UndoPlan { actions }))
    }
}

/// Checks that an edited file is still as the operation left it and its backup is present.
pub(crate) async fn verify_restorable(
    providers: &ProviderRegistry,
    current: &EntryFingerprint,
    backup: &Location,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let provider = providers.resolve(&current.entry.location)?;
    let actual = provider
        .inspect(&current.entry, cancellation.clone())
        .await
        .map_err(|error| match error {
            fm_vfs::VfsError::NotFound { .. } => ExecutionError::Failed(
                "The edited file is missing, so it cannot be restored safely.".into(),
            ),
            error => error.into(),
        })?;
    if actual.kind != current.kind
        || actual.size != current.size
        || actual.modified_at != current.modified_at
    {
        return Err(ExecutionError::Failed(
            "The file changed after it was edited, so it cannot be restored safely.".into(),
        ));
    }
    provider
        .inspect(&entry_of(backup), cancellation.clone())
        .await
        .map_err(|error| match error {
            fm_vfs::VfsError::NotFound { .. } => ExecutionError::Failed(
                "The backup copy is missing, so undo is not possible.".into(),
            ),
            error => error.into(),
        })?;
    Ok(())
}

/// Puts the backup's content back over the edited file and removes the backup.
pub(crate) async fn restore_content(
    providers: &ProviderRegistry,
    current: &EntryFingerprint,
    backup: &Location,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    verify_restorable(providers, current, backup, cancellation).await?;
    let provider = providers.resolve(&current.entry.location)?;
    let backup_entry = entry_of(backup);
    let bytes = read_all(&provider, &backup_entry, cancellation).await?;
    let slot = Mutex::new(None);
    replace_content(&provider, &current.entry, &bytes, &slot, cancellation).await?;
    provider
        .remove(
            &backup_entry,
            RemoveOptions {
                recursive: false,
                use_trash: false,
            },
            cancellation.clone(),
        )
        .await?;
    Ok(())
}
