//! Converting geographic files into other formats (tasks 0281, 0282).
//!
//! The conversion itself lives in `fm-geo`; this module gives it the operation lifecycle: it runs
//! as a cancellable job, writes a new file next to the source (or into a chosen folder) through a
//! private temporary file, resolves name conflicts like copy does, and can be undone because the
//! output is a new entry.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryKind, Location};
use fm_geo::{Converted, Target, conversion_limits, convert, read_document};
use fm_operations::{
    ConflictResolution, ExecutionError, ExecutionOutcome, Operation, OperationEntryError,
    OperationExecutor, OperationPlan, OperationProgressReporter, OperationUndo, PauseToken,
    PlanItem, UndoAction, UndoPlan,
};
use fm_transport_dto::{
    ConvertGeoTextRequestDto, ConvertGeoTextResponseDto, GeoConversionReportDto,
    PreviewGeoConversionRequestDto, PreviewGeoConversionResponseDto, StartOperationRequestDto,
};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::geo_files::MAX_GEO_FILE_BYTES;
use crate::image_metadata_edit::{entry_of, read_all, sibling, write_new};
use crate::operation_planner::{conflict_error, copy_name, effective_resolution, fingerprint};

const MAX_SOURCES: usize = 200;
const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;

fn target_of(id: &str, csv_wkt: bool) -> Result<Target, ApplicationError> {
    Target::parse(id, csv_wkt).ok_or_else(|| {
        ApplicationError::InvalidRequest(format!(
            "unknown output format '{id}'; use geojson, gpx, kml, kmz or csv"
        ))
    })
}

fn extension_of(location: &Location) -> String {
    location
        .name()
        .ok()
        .and_then(|name| {
            name.rsplit_once('.')
                .map(|(_, extension)| extension.to_owned())
        })
        .unwrap_or_default()
}

fn stem_of(location: &Location) -> String {
    let name = location.name().unwrap_or_default();
    name.rsplit_once('.')
        .map_or(name.clone(), |(stem, _)| stem.to_owned())
}

/// Name of the converted file for `source`.
fn output_name(source: &Location, target: Target) -> String {
    format!("{}.{}", stem_of(source), target.extension())
}

async fn convert_location(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    target: Target,
    cancellation: &CancellationToken,
) -> Result<Converted, String> {
    let entry = entry_of(location);
    let size = provider
        .file_size(&entry, cancellation.clone())
        .await
        .map_err(|error| error.to_string())?;
    if size > MAX_GEO_FILE_BYTES {
        return Err(format!(
            "the file is {size} bytes; files above {MAX_GEO_FILE_BYTES} bytes are not converted"
        ));
    }
    let bytes = read_all(provider, &entry, cancellation)
        .await
        .map_err(|error| error.to_string())?;
    let extension = extension_of(location);
    tokio::task::spawn_blocking(move || {
        let document = read_document(&extension, &bytes, conversion_limits())
            .map_err(|failure| failure.message)?;
        convert(&document, target).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Reports what converting each file would lose, without writing anything.
pub(crate) async fn preview_conversion(
    providers: &ProviderRegistry,
    request: PreviewGeoConversionRequestDto,
) -> Result<PreviewGeoConversionResponseDto, ApplicationError> {
    if request.locations.is_empty() || request.locations.len() > MAX_SOURCES {
        return Err(ApplicationError::InvalidRequest(format!(
            "between 1 and {MAX_SOURCES} files can be converted at once"
        )));
    }
    let target = target_of(&request.target_format, request.csv_wkt)?;
    let mut reports = Vec::with_capacity(request.locations.len());
    for dto in request.locations {
        let location: Location = dto.clone().into();
        let provider = providers.resolve(&location)?;
        let outcome =
            convert_location(&provider, &location, target, &CancellationToken::new()).await;
        reports.push(match outcome {
            Ok(converted) => GeoConversionReportDto {
                location: dto,
                output_name: Some(output_name(&location, target)),
                notes: converted.notes,
                error: None,
            },
            Err(message) => GeoConversionReportDto {
                location: dto,
                output_name: None,
                notes: Vec::new(),
                error: Some(message),
            },
        });
    }
    Ok(PreviewGeoConversionResponseDto { reports })
}

/// Converts GeoJSON text (an editor's current content) into a text format.
pub(crate) async fn convert_text(
    request: ConvertGeoTextRequestDto,
) -> Result<ConvertGeoTextResponseDto, ApplicationError> {
    if request.text.len() > MAX_TEXT_BYTES {
        return Err(ApplicationError::InvalidRequest(
            "the text is too large to convert".into(),
        ));
    }
    let target = target_of(&request.target_format, request.csv_wkt)?;
    if target == Target::Kmz {
        return Err(ApplicationError::InvalidRequest(
            "KMZ is a binary format; save the file and convert it from the file menu".into(),
        ));
    }
    let converted = tokio::task::spawn_blocking(move || {
        read_document("geojson", request.text.as_bytes(), conversion_limits())
            .map_err(|failure| failure.message)
            .and_then(|document| convert(&document, target).map_err(|error| error.to_string()))
            .map(|converted| (converted, target.extension()))
    })
    .await
    .map_err(|_| ApplicationError::Internal)?
    .map_err(ApplicationError::InvalidRequest)?;
    let (converted, extension) = converted;
    Ok(ConvertGeoTextResponseDto {
        content: String::from_utf8(converted.bytes).map_err(|_| ApplicationError::Internal)?,
        extension: extension.to_owned(),
        notes: converted.notes,
    })
}

/// Builds the executor for a validated `transformGeo` request.
pub(crate) fn plan_transform_geo(
    providers: &ProviderRegistry,
    request: &StartOperationRequestDto,
) -> Result<Arc<dyn OperationExecutor>, ApplicationError> {
    let transform = request.geo_transform.as_ref().ok_or_else(|| {
        ApplicationError::InvalidRequest("transformGeo requires geoTransform".into())
    })?;
    if request.sources.is_empty() || request.sources.len() > MAX_SOURCES {
        return Err(ApplicationError::InvalidRequest(format!(
            "transformGeo needs between 1 and {MAX_SOURCES} files"
        )));
    }
    let target = target_of(&transform.target_format, transform.csv_wkt)?;
    let destination: Option<Location> = request.destination.clone().map(Into::into);
    let mut sources = Vec::with_capacity(request.sources.len());
    for source in &request.sources {
        let location: Location = source.clone().into();
        providers.resolve(&location)?;
        let folder =
            match &destination {
                Some(folder) => folder.clone(),
                None => location.parent().ok().flatten().ok_or_else(|| {
                    ApplicationError::InvalidRequest("a file needs a folder".into())
                })?,
            };
        providers
            .resolve(&folder)?
            .capabilities_for(&folder)?
            .require(ProviderCapabilities::WRITE)?;
        sources.push((location, folder));
    }
    Ok(Arc::new(TransformGeoExecutor {
        providers: providers.clone(),
        sources,
        target,
        created: Mutex::new(Vec::new()),
        replaced: Mutex::new(false),
        temporary: Mutex::new(None),
    }))
}

struct TransformGeoExecutor {
    providers: ProviderRegistry,
    /// Source file and the folder its output goes to.
    sources: Vec<(Location, Location)>,
    target: Target,
    /// Outputs written as new files, for undo.
    created: Mutex<Vec<Location>>,
    /// Whether an existing file was replaced, which undo cannot bring back.
    replaced: Mutex<bool>,
    temporary: Mutex<Option<Location>>,
}

#[async_trait]
impl OperationExecutor for TransformGeoExecutor {
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
            .ok_or_else(|| ExecutionError::Failed("transform plan entry is missing".into()))?;
        let source_provider = self.providers.resolve(&item.entry.location)?;
        let folder_provider = self.providers.resolve(&folder)?;
        let converted = convert_location(
            &source_provider,
            &item.entry.location,
            self.target,
            cancellation,
        )
        .await
        .map_err(ExecutionError::Failed)?;

        let mut name = output_name(&item.entry.location, self.target);
        let mut destination = folder
            .join(&name)
            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
        let mut overwrite = false;
        if let Ok(existing) = folder_provider
            .inspect(&entry_of(&destination), cancellation.clone())
            .await
        {
            let source = source_provider
                .inspect(&item.entry, cancellation.clone())
                .await?;
            match effective_resolution(operation.conflict_policy, resolution) {
                None => return Err(conflict_error(&source, &existing)),
                Some(ConflictResolution::Skip) => return Ok(ExecutionOutcome::Skipped),
                Some(ConflictResolution::Overwrite) => {
                    if existing.kind != EntryKind::File {
                        return Err(ExecutionError::Failed(
                            "a folder cannot be replaced by a converted file".into(),
                        ));
                    }
                    overwrite = true;
                }
                Some(ConflictResolution::RenameNew) => {
                    let mut suffix = 1;
                    loop {
                        name = copy_name(&output_name(&item.entry.location, self.target), suffix);
                        destination = folder
                            .join(&name)
                            .map_err(|error| ExecutionError::Failed(error.to_string()))?;
                        if folder_provider
                            .inspect(&entry_of(&destination), cancellation.clone())
                            .await
                            .is_err()
                        {
                            break;
                        }
                        suffix += 1;
                        if suffix > 999 {
                            return Err(ExecutionError::Failed(
                                "too many files with this name already exist".into(),
                            ));
                        }
                    }
                }
            }
        }

        let temporary = sibling(&destination, &format!(".fm-geo-{}.tmp", Uuid::new_v4()))?;
        *self
            .temporary
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(temporary.clone());
        let written = async {
            write_new(&folder_provider, &temporary, &converted.bytes, cancellation).await?;
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
            Ok::<(), ExecutionError>(())
        }
        .await;
        if written.is_err() {
            let _ = folder_provider
                .discard_copy(&temporary, CancellationToken::new())
                .await;
        }
        *self
            .temporary
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        written?;

        if overwrite {
            *self
                .replaced
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = true;
        } else {
            self.created
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(destination);
        }
        progress.report_bytes(item.bytes);
        if converted.notes.is_empty() {
            Ok(ExecutionOutcome::Completed)
        } else {
            Ok(ExecutionOutcome::CompletedWithWarnings(vec![
                OperationEntryError {
                    entry: item.entry.clone(),
                    message: format!(
                        "Written as {name}. Not represented: {}.",
                        converted.notes.join("; ")
                    ),
                },
            ]))
        }
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
        if *self
            .replaced
            .lock()
            .unwrap_or_else(|error| error.into_inner())
        {
            return Ok(OperationUndo::unavailable(
                "An existing file was replaced, which cannot be brought back.",
            ));
        }
        let created = self
            .created
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if created.is_empty() {
            return Ok(OperationUndo::unavailable("No file was created."));
        }
        let mut entries = Vec::with_capacity(created.len());
        for location in created {
            let provider = self.providers.resolve(&location)?;
            entries.push(fingerprint(&provider, &location, cancellation).await?);
        }
        Ok(OperationUndo::available(UndoPlan {
            actions: vec![UndoAction::RemoveCreated { entries }],
        }))
    }
}
