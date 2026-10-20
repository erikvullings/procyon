//! Reading geographic files for the map viewers (tasks 0277-0280).
//!
//! Parsing happens here, not in the web view: the browser and desktop hosts see identical results
//! and a large file never blocks the UI thread. Files are read through the VFS provider with a
//! size cap; format problems come back as a structured `error` so the viewer can still offer the
//! raw text.

use fm_domain::{EntryId, Location};
use fm_geo::{CsvError, CsvInfo, CsvOptions, GeoDocument, Limits, read_csv, read_document};
use fm_transport_dto::{
    CsvGeoInfoDto, CsvGeoOptionsDto, GeoDocumentDto, GeoErrorDto, GeoIssueDto, GeoOverlayDto,
    GeoProfilePointDto, GeoTrackDto, ParseGeoTextRequestDto, ReadGeoFileRequestDto,
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
        tracks: Vec::new(),
        resources: std::collections::BTreeMap::new(),
        overlays: Vec::new(),
        error: Some(error),
        csv: None,
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

fn track_dto(track: fm_geo::TrackSummary) -> GeoTrackDto {
    GeoTrackDto {
        name: track.name,
        feature_index: u32::try_from(track.feature_index).unwrap_or(u32::MAX),
        points: track.points as u64,
        distance_meters: track.distance_meters,
        elapsed_seconds: track.elapsed_seconds,
        moving_seconds: track.moving_seconds,
        ascent_meters: track.ascent_meters,
        descent_meters: track.descent_meters,
        min_elevation: track.min_elevation,
        max_elevation: track.max_elevation,
        average_speed: track.average_speed,
        max_speed: track.max_speed,
        start_time: track.start_time,
        end_time: track.end_time,
        profile: track
            .profile
            .into_iter()
            .map(|point| GeoProfilePointDto {
                distance_meters: point.distance_meters,
                longitude: point.longitude,
                latitude: point.latitude,
                elevation: point.elevation,
                speed_meters_per_second: point.speed_meters_per_second,
                heart_rate: point.heart_rate,
            })
            .collect(),
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
        tracks: document.tracks.into_iter().map(track_dto).collect(),
        resources: document.resources,
        overlays: document
            .overlays
            .into_iter()
            .map(|overlay| GeoOverlayDto {
                name: overlay.name,
                image: overlay.image,
                north: overlay.north,
                south: overlay.south,
                east: overlay.east,
                west: overlay.west,
                rotation: overlay.rotation,
            })
            .collect(),
        error: None,
        csv: None,
    }
}

fn csv_info_dto(info: CsvInfo) -> CsvGeoInfoDto {
    CsvGeoInfoDto {
        headers: info.headers,
        delimiter: info.delimiter.to_string(),
        x_column: info.x_column,
        y_column: info.y_column,
        wkt_column: info.wkt_column,
        crs: Some(info.crs),
        crs_name: Some(info.crs_name),
        crs_guessed: info.crs_guessed,
        columns_detected: info.columns_detected,
        rows: info.rows as u64,
        skipped_rows: info.skipped_rows as u64,
    }
}

fn csv_options(options: Option<CsvGeoOptionsDto>) -> CsvOptions {
    let options = options.unwrap_or_default();
    let text = |value: Option<String>| {
        value
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    CsvOptions {
        x_column: text(options.x_column),
        y_column: text(options.y_column),
        wkt_column: text(options.wkt_column),
        crs: text(options.crs),
    }
}

/// Reads delimited text. A file without coordinates still reports its columns, so the viewer can
/// offer them in the "columns and coordinate system" dialog.
fn parse_csv(bytes: &[u8], options: CsvOptions) -> GeoDocumentDto {
    match read_csv(bytes, Limits::default(), &options) {
        Ok((document, info)) => {
            let mut dto = document_dto(document);
            dto.csv = Some(csv_info_dto(info));
            dto
        }
        Err(error) => {
            let mut dto = empty("csv", plain_error(error.to_string()));
            if let CsvError::NoCoordinates { headers, .. } = error {
                dto.csv = Some(CsvGeoInfoDto {
                    headers,
                    delimiter: String::new(),
                    x_column: None,
                    y_column: None,
                    wkt_column: None,
                    crs: None,
                    crs_name: None,
                    crs_guessed: false,
                    columns_detected: false,
                    rows: 0,
                    skipped_rows: 0,
                });
            }
            dto
        }
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
    if matches!(extension.to_ascii_lowercase().as_str(), "csv" | "tsv") {
        let options = csv_options(request.csv);
        return tokio::task::spawn_blocking(move || parse_csv(&bytes, options))
            .await
            .map_err(|_| ApplicationError::Internal);
    }
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
    read_document(extension, bytes, Limits::default())
        .map(document_dto)
        .map_err(|failure| {
            (
                failure
                    .format
                    .map(|format| format.id().to_owned())
                    .unwrap_or_default(),
                GeoErrorDto {
                    message: failure.message,
                    line: failure.line.and_then(|line| u32::try_from(line).ok()),
                    column: failure.column.and_then(|column| u32::try_from(column).ok()),
                    path: failure.path,
                },
            )
        })
}
