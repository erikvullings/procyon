//! Converting geographic files into other formats (tasks 0281, 0282).
//!
//! The conversion itself lives in `fm-geo`; this module gives it the operation lifecycle: it runs
//! as a cancellable job, writes a new file next to the source (or into a chosen folder) through a
//! private temporary file, resolves name conflicts like copy does, and can be undone because the
//! output is a new entry.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fm_domain::{EntryKind, Location};
use fm_geo::{MinifyOptions, Target, TransformSpec, Transformed, transform};
use fm_operations::{
    ConflictResolution, ExecutionError, ExecutionOutcome, Operation, OperationEntryError,
    OperationExecutor, OperationPlan, OperationProgressReporter, OperationUndo, PauseToken,
    PlanItem, UndoAction, UndoPlan,
};
use fm_transport_dto::{
    ConvertGeoTextRequestDto, ConvertGeoTextResponseDto, GeoConversionReportDto, GeoMinifyDto,
    PreviewGeoConversionRequestDto, PreviewGeoConversionResponseDto, PreviewGeoMinifyRequestDto,
    PreviewGeoMinifyResponseDto, StartOperationRequestDto,
};
use fm_vfs::{
    CopyCommitOptions, EntryRef, FileSystemProvider, ProviderCapabilities, ProviderRegistry,
    RemoveOptions,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::ApplicationError;
use crate::geo_files::MAX_GEO_FILE_BYTES;
use crate::image_metadata_edit::{
    entry_of, read_all, replace_content, sibling, write_backup, write_new,
};
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

/// Name of the output for `source`: the new extension, or `.min` before the old one.
fn output_name(source: &Location, spec: &TransformSpec) -> String {
    match spec.target {
        Some(target) => format!("{}.{}", stem_of(source), target.extension()),
        None => format!("{}.min.{}", stem_of(source), extension_of(source)),
    }
}

fn minify_options(dto: &GeoMinifyDto) -> MinifyOptions {
    MinifyOptions {
        precision: dto.precision,
        tolerance_meters: dto
            .tolerance_meters
            .filter(|value| value.is_finite() && *value > 0.0),
        drop_elevation: dto.drop_elevation,
        drop_time: dto.drop_time,
        drop_extensions: dto.drop_extensions,
        remove_duplicates: dto.remove_duplicates,
    }
}

async fn read_source(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, String> {
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
    read_all(provider, &entry, cancellation)
        .await
        .map_err(|error| error.to_string())
}

async fn transform_bytes(
    location: &Location,
    bytes: Vec<u8>,
    spec: TransformSpec,
) -> Result<Transformed, String> {
    let extension = extension_of(location);
    tokio::task::spawn_blocking(move || transform(&extension, &bytes, &spec))
        .await
        .map_err(|error| error.to_string())?
}

async fn transform_location(
    provider: &Arc<dyn FileSystemProvider>,
    location: &Location,
    spec: TransformSpec,
    cancellation: &CancellationToken,
) -> Result<Transformed, String> {
    let bytes = read_source(provider, location, cancellation).await?;
    transform_bytes(location, bytes, spec).await
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
        let spec = TransformSpec {
            target: Some(target),
            minify: None,
        };
        let outcome =
            transform_location(&provider, &location, spec, &CancellationToken::new()).await;
        reports.push(match outcome {
            Ok(converted) => GeoConversionReportDto {
                location: dto,
                output_name: Some(output_name(&location, &spec)),
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
        transform(
            "geojson",
            request.text.as_bytes(),
            &TransformSpec {
                target: Some(target),
                minify: None,
            },
        )
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

/// Features and positions the minify preview may send back for the before/after overlay.
const PREVIEW_MAX_FEATURES: usize = 20_000;

/// Reports the effect of a minification without writing anything.
pub(crate) async fn preview_minify(
    providers: &ProviderRegistry,
    request: PreviewGeoMinifyRequestDto,
) -> Result<PreviewGeoMinifyResponseDto, ApplicationError> {
    let location: Location = request.location.into();
    let provider = providers.resolve(&location)?;
    let cancellation = CancellationToken::new();
    let size = provider
        .file_size(&entry_of(&location), cancellation.clone())
        .await?;
    let spec = TransformSpec {
        target: None,
        minify: Some(minify_options(&request.minify)),
    };
    let failed = |message: String| PreviewGeoMinifyResponseDto {
        original_bytes: size,
        minified_bytes: size,
        features_before: 0,
        features_after: 0,
        positions_before: 0,
        positions_after: 0,
        distance_before_meters: 0.0,
        distance_after_meters: 0.0,
        notes: Vec::new(),
        collection: None,
        error: Some(message),
    };
    let result = match transform_location(&provider, &location, spec, &cancellation).await {
        Ok(result) => result,
        Err(message) => return Ok(failed(message)),
    };
    let stats = result.stats.unwrap_or_default();
    let collection = result
        .document
        .as_ref()
        .filter(|document| document.features.len() <= PREVIEW_MAX_FEATURES)
        .map(fm_geo::GeoDocument::feature_collection);
    Ok(PreviewGeoMinifyResponseDto {
        original_bytes: size,
        minified_bytes: result.bytes.len() as u64,
        features_before: stats.features_before as u64,
        features_after: stats.features_after as u64,
        positions_before: stats.positions_before as u64,
        positions_after: stats.positions_after as u64,
        distance_before_meters: stats.distance_before_meters,
        distance_after_meters: stats.distance_after_meters,
        notes: result.notes,
        collection,
        error: None,
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
    let target = transform
        .target_format
        .as_deref()
        .map(|id| target_of(id, transform.csv_wkt))
        .transpose()?;
    let minify = transform.minify.as_ref().map(minify_options);
    if target.is_none() && minify.is_none() {
        return Err(ApplicationError::InvalidRequest(
            "choose an output format or at least one reduction".into(),
        ));
    }
    if transform.in_place && (target.is_some() || minify.is_none()) {
        return Err(ApplicationError::InvalidRequest(
            "replacing a file in place is only possible for a reduction that keeps its format"
                .into(),
        ));
    }
    let destination: Option<Location> = request.destination.clone().map(Into::into);
    let mut sources = Vec::with_capacity(request.sources.len());
    for source in &request.sources {
        let location: Location = source.clone().into();
        let provider = providers.resolve(&location)?;
        if transform.in_place {
            provider
                .capabilities_for(&location)?
                .require(ProviderCapabilities::WRITE)?;
            let folder =
                location.parent().ok().flatten().ok_or_else(|| {
                    ApplicationError::InvalidRequest("a file needs a folder".into())
                })?;
            sources.push((location, folder));
            continue;
        }
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
        spec: TransformSpec { target, minify },
        in_place: transform.in_place,
        backup: transform.backup_original,
        created: Mutex::new(Vec::new()),
        edited: Mutex::new(Vec::new()),
        replaced: Mutex::new(false),
        temporary: Mutex::new(None),
    }))
}

struct TransformGeoExecutor {
    providers: ProviderRegistry,
    /// Source file and the folder its output goes to.
    sources: Vec<(Location, Location)>,
    spec: TransformSpec,
    /// Replace the source itself (reductions only).
    in_place: bool,
    /// With `in_place`: keep the original as a backup, which makes the job undoable.
    backup: bool,
    /// Outputs written as new files, for undo.
    created: Mutex<Vec<Location>>,
    /// Files rewritten in place with the backup that holds their old content.
    edited: Mutex<Vec<(Location, Option<Location>)>>,
    /// Whether an existing file was replaced, which undo cannot bring back.
    replaced: Mutex<bool>,
    temporary: Mutex<Option<Location>>,
}

fn size_note(before: usize, after: usize) -> Option<String> {
    (after >= before).then(|| {
        format!("The result ({after} bytes) is not smaller than the original ({before} bytes)")
    })
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
        let original = read_source(&source_provider, &item.entry.location, cancellation)
            .await
            .map_err(ExecutionError::Failed)?;
        let converted = transform_bytes(&item.entry.location, original.clone(), self.spec)
            .await
            .map_err(ExecutionError::Failed)?;
        let mut notes = converted.notes.clone();
        if self.spec.minify.is_some() {
            notes.extend(size_note(original.len(), converted.bytes.len()));
        }

        if self.in_place {
            if converted.bytes == original {
                progress.report_bytes(item.bytes);
                return Ok(ExecutionOutcome::Completed);
            }
            let backup = if self.backup {
                Some(
                    write_backup(
                        &source_provider,
                        &item.entry.location,
                        &original,
                        cancellation,
                    )
                    .await?,
                )
            } else {
                None
            };
            if let Err(error) = replace_content(
                &source_provider,
                &item.entry,
                &converted.bytes,
                &self.temporary,
                cancellation,
            )
            .await
            {
                if let Some(backup) = &backup {
                    let _ = source_provider
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
                .push((item.entry.location.clone(), backup));
            progress.report_bytes(item.bytes);
            return Ok(Self::outcome(
                item,
                &item.entry.location.name().unwrap_or_default(),
                &notes,
            ));
        }

        let folder_provider = self.providers.resolve(&folder)?;
        let mut name = output_name(&item.entry.location, &self.spec);
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
                        name = copy_name(&output_name(&item.entry.location, &self.spec), suffix);
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
        Ok(Self::outcome(item, &name, &notes))
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
        let edited = self
            .edited
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if !edited.is_empty() {
            let mut actions = Vec::with_capacity(edited.len());
            for (location, backup) in edited {
                let Some(backup) = backup else {
                    return Ok(OperationUndo::unavailable(
                        "Undo needs the backup copy; run the reduction with \"keep original\" enabled.",
                    ));
                };
                let provider = self.providers.resolve(&location)?;
                actions.push(UndoAction::RestoreContent {
                    current: fingerprint(&provider, &location, cancellation).await?,
                    backup,
                });
            }
            return Ok(OperationUndo::available(UndoPlan { actions }));
        }
        let created = self
            .created
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if created.is_empty() {
            return Ok(OperationUndo::unavailable("No file was changed."));
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

impl TransformGeoExecutor {
    fn outcome(item: &PlanItem, name: &str, notes: &[String]) -> ExecutionOutcome {
        if notes.is_empty() {
            ExecutionOutcome::Completed
        } else {
            ExecutionOutcome::CompletedWithWarnings(vec![
                OperationEntryError::new(
                    item.entry.clone(),
                    format!(
                        "Written as {name}. Not represented or removed: {}.",
                        notes.join("; ")
                    ),
                )
                .with_code(fm_operations::codes::entry_error::GEO_WRITTEN_WITH_LOSS)
                .with_param("name", name)
                .with_param("notes", notes.join("; ")),
            ])
        }
    }
}
