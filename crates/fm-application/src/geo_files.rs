//! Reading geographic files for the map viewers (tasks 0277-0280).
//!
//! Parsing happens here, not in the web view: the browser and desktop hosts see identical results
//! and a large file never blocks the UI thread. Files are read through the VFS provider with a
//! size cap; format problems come back as a structured `error` so the viewer can still offer the
//! raw text.

use fm_domain::{EntryId, Location};
use fm_geo::{GeoDocument, GeoFormat, GeoJsonError, Limits, detect_format, parse_geojson};
use fm_transport_dto::{
    GeoDocumentDto, GeoErrorDto, GeoIssueDto, ParseGeoTextRequestDto, ReadGeoFileRequestDto,
};
use fm_vfs::{EntryRef, ProviderRegistry};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;

/// Largest geographic file parsed.
pub(crate) const MAX_GEO_FILE_BYTES: u64 = 64 * 1024 * 1024;

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

fn empty(format: &str, error: GeoErrorDto) -> GeoDocumentDto {
    GeoDocumentDto {
        format: format.to_owned(),
        collection: json!({ "type": "FeatureCollection", "features": [] }),
        total_features: 0,
        displayed_features: 0,
        truncated: false,
        bounds: None,
        warnings: Vec::new(),
        skipped: Vec::new(),
        error: Some(error),
    }
}

fn plain_error(message: String) -> GeoErrorDto {
    GeoErrorDto {
        message,
        line: None,
        column: None,
        path: None,
    }
}

fn document_dto(document: GeoDocument) -> GeoDocumentDto {
    GeoDocumentDto {
        format: document.format.id().to_owned(),
        collection: document.feature_collection(),
        total_features: document.total_features as u64,
        displayed_features: document.features.len() as u64,
        truncated: document.truncated,
        bounds: document.bounds,
        warnings: document
            .warnings
            .into_iter()
            .map(|issue| GeoIssueDto {
                path: issue.path,
                message: issue.message,
            })
            .collect(),
        skipped: document.skipped,
        error: None,
    }
}

pub(crate) async fn read_geo_file(
    providers: &ProviderRegistry,
    request: ReadGeoFileRequestDto,
) -> Result<GeoDocumentDto, ApplicationError> {
    let location: Location = request.location.into();
    let provider = providers.resolve(&location)?;
    let entry = EntryRef {
        id: EntryId::new(),
        location: location.clone(),
    };
    let size = provider.file_size(&entry, CancellationToken::new()).await?;
    if size > MAX_GEO_FILE_BYTES {
        return Ok(empty(
            "",
            plain_error(format!(
                "the file is {size} bytes; files above {MAX_GEO_FILE_BYTES} bytes are not opened on the map"
            )),
        ));
    }
    let mut bytes = Vec::new();
    provider
        .open_read(&entry, CancellationToken::new())
        .await?
        .take(MAX_GEO_FILE_BYTES)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| ApplicationError::InvalidRequest(error.to_string()))?;

    let extension = extension_of(&location);
    parse_off_thread(extension, bytes).await
}

/// Parses `text` as GeoJSON, for live validation while a document is being edited.
pub(crate) async fn parse_geo_text(
    request: ParseGeoTextRequestDto,
) -> Result<GeoDocumentDto, ApplicationError> {
    parse_off_thread("geojson".to_owned(), request.text.into_bytes()).await
}

async fn parse_off_thread(
    extension: String,
    bytes: Vec<u8>,
) -> Result<GeoDocumentDto, ApplicationError> {
    let parsed = tokio::task::spawn_blocking(move || parse_bytes(&extension, &bytes))
        .await
        .map_err(|_| ApplicationError::Internal)?;
    Ok(parsed.unwrap_or_else(|(format, error)| empty(&format, error)))
}

fn parse_bytes(extension: &str, bytes: &[u8]) -> Result<GeoDocumentDto, (String, GeoErrorDto)> {
    let Some(format) = detect_format(extension, bytes) else {
        return Err((
            String::new(),
            plain_error("this is not a recognised geographic file".into()),
        ));
    };
    match format {
        GeoFormat::GeoJson => parse_geojson(bytes, Limits::default())
            .map(document_dto)
            .map_err(|error| {
                (
                    format.id().to_owned(),
                    match error {
                        GeoJsonError::Syntax {
                            message,
                            line,
                            column,
                        } => GeoErrorDto {
                            message,
                            line: u32::try_from(line).ok(),
                            column: u32::try_from(column).ok(),
                            path: None,
                        },
                        GeoJsonError::Invalid(issue) => GeoErrorDto {
                            message: issue.message,
                            line: None,
                            column: None,
                            path: Some(issue.path),
                        },
                    },
                )
            }),
    }
}
