//! Wire types for geographic files (tasks 0277-0282).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::LocationDto;

/// Reads a geographic file (`POST /api/v1/files/geo/read`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadGeoFileRequestDto {
    /// The GeoJSON, GPX, KML or KMZ file.
    pub location: LocationDto,
}

/// Parses GeoJSON text without a file (`POST /api/v1/geo/parse`), for live validation in editors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParseGeoTextRequestDto {
    /// The GeoJSON text, at most about 2 MiB.
    pub text: String,
}

/// A warning about data that is not represented faithfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoIssueDto {
    /// Where in the document, for example `/features/3/geometry`.
    pub path: String,
    /// What is wrong or left out.
    pub message: String,
}

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoErrorDto {
    /// Human-readable message.
    pub message: String,
    /// One-based line of a syntax error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// One-based column of a syntax error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    /// Location of a validation error inside the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One sample of a track's elevation, speed and heart-rate profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoProfilePointDto {
    /// Metres from the start of the track.
    pub distance_meters: f64,
    /// Position of the sample.
    pub longitude: f64,
    /// Position of the sample.
    pub latitude: f64,
    /// Elevation in metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<f64>,
    /// Speed over the last leg in metres per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_meters_per_second: Option<f64>,
    /// Heart rate in beats per minute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heart_rate: Option<f64>,
}

/// Statistics of one recorded GPX track; absent values were not in the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoTrackDto {
    /// Track name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Index of the track's feature in the collection.
    pub feature_index: u32,
    /// Recorded fixes.
    pub points: u64,
    /// Length in metres.
    pub distance_meters: f64,
    /// Last minus first timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_seconds: Option<f64>,
    /// Time spent faster than walking pace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moving_seconds: Option<f64>,
    /// Climbed metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ascent_meters: Option<f64>,
    /// Descended metres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descent_meters: Option<f64>,
    /// Lowest elevation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_elevation: Option<f64>,
    /// Highest elevation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_elevation: Option<f64>,
    /// Average moving speed in metres per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub average_speed: Option<f64>,
    /// Fastest sustained speed in metres per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<f64>,
    /// First timestamp, UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// Last timestamp, UTC.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
    /// Chart samples, at most about 500.
    pub profile: Vec<GeoProfilePointDto>,
}

/// An image draped over a rectangle of the map (KML ground overlay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoOverlayDto {
    /// Overlay name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Path of the image in `resources`.
    pub image: String,
    /// Northern edge in degrees.
    pub north: f64,
    /// Southern edge in degrees.
    pub south: f64,
    /// Eastern edge in degrees.
    pub east: f64,
    /// Western edge in degrees.
    pub west: f64,
    /// Counter-clockwise rotation in degrees around the centre.
    pub rotation: f64,
}

/// A geographic file read into GeoJSON features.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoDocumentDto {
    /// `geojson`, `gpx`, `kml` or `kmz`; empty when the file is not a geographic file.
    pub format: String,
    /// The displayed features as a GeoJSON `FeatureCollection`.
    #[schema(value_type = Object)]
    pub collection: Value,
    /// Features in the source file.
    pub total_features: u64,
    /// Features in `collection`; less than `totalFeatures` when limits applied.
    pub displayed_features: u64,
    /// Whether limits cut the document short.
    pub truncated: bool,
    /// West, south, east, north of the displayed features.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f64; 4]>,
    /// Non-fatal issues.
    pub warnings: Vec<GeoIssueDto>,
    /// Source elements without a 2D representation that were left out.
    pub skipped: Vec<String>,
    /// Statistics per recorded track (GPX).
    #[serde(default)]
    pub tracks: Vec<GeoTrackDto>,
    /// Images shipped inside a KMZ as `data:` URIs, by path; features reference them by `_icon`.
    #[serde(default)]
    pub resources: std::collections::BTreeMap<String, String>,
    /// Ground overlays whose image is in `resources`.
    #[serde(default)]
    pub overlays: Vec<GeoOverlayDto>,
    /// Set when the file could not be read; `collection` is then empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GeoErrorDto>,
}

/// How to make a geographic file smaller (task 0282). Omitted options are not applied; with none
/// at all only whitespace is removed, which changes nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoMinifyDto {
    /// Round longitude and latitude to this many decimals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
    /// Remove line and area points closer than this many metres to the simplified shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance_meters: Option<f64>,
    /// Remove elevation values.
    #[serde(default)]
    pub drop_elevation: bool,
    /// Remove timestamps.
    #[serde(default)]
    pub drop_time: bool,
    /// Remove heart rates and custom properties.
    #[serde(default)]
    pub drop_extensions: bool,
    /// Remove repeated points, empty features and duplicate features.
    #[serde(default)]
    pub remove_duplicates: bool,
}

/// What a `transformGeo` operation does with every source file (tasks 0281, 0282).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoTransformDto {
    /// Output format: `geojson`, `gpx`, `kml`, `kmz` or `csv`; absent keeps each file's format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_format: Option<String>,
    /// CSV only: add a `wkt` column so lines and areas are kept.
    #[serde(default)]
    pub csv_wkt: bool,
    /// Size reductions to apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minify: Option<GeoMinifyDto>,
    /// Replace the source file instead of writing a new one; needs no `targetFormat`.
    #[serde(default)]
    pub in_place: bool,
    /// With `inPlace`: keep the original as `<name>.orig`, which also makes the job undoable.
    #[serde(default)]
    pub backup_original: bool,
}

/// Previews a minification (`POST /api/v1/geo/minify/preview`): sizes, statistics and the result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGeoMinifyRequestDto {
    /// The file.
    pub location: LocationDto,
    /// The reductions to preview.
    pub minify: GeoMinifyDto,
}

/// What a minification would do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGeoMinifyResponseDto {
    /// Size of the source file.
    pub original_bytes: u64,
    /// Size the result would have.
    pub minified_bytes: u64,
    /// Features before.
    pub features_before: u64,
    /// Features after.
    pub features_after: u64,
    /// Positions before.
    pub positions_before: u64,
    /// Positions after.
    pub positions_after: u64,
    /// Summed length of all lines before, metres.
    pub distance_before_meters: f64,
    /// Summed length of all lines after, metres.
    pub distance_after_meters: f64,
    /// What was removed or could not be represented.
    pub notes: Vec<String>,
    /// The minified features as a GeoJSON `FeatureCollection`, for the before/after overlay; absent
    /// when the result is too large to show or only whitespace changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub collection: Option<serde_json::Value>,
    /// Set when the file cannot be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Previews a conversion (`POST /api/v1/geo/convert/preview`): what would be lost, per file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGeoConversionRequestDto {
    /// Files to convert, at most 200.
    pub locations: Vec<LocationDto>,
    /// Output format.
    pub target_format: String,
    /// CSV only: add a `wkt` column.
    #[serde(default)]
    pub csv_wkt: bool,
}

/// The report for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeoConversionReportDto {
    /// The source file.
    pub location: LocationDto,
    /// Name the output would get.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_name: Option<String>,
    /// What the target format cannot represent.
    pub notes: Vec<String>,
    /// Set when this file cannot be converted at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Reports for all requested files, in request order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewGeoConversionResponseDto {
    /// One report per file.
    pub reports: Vec<GeoConversionReportDto>,
}

/// Converts text without a file (`POST /api/v1/geo/convert/text`), for editor export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertGeoTextRequestDto {
    /// GeoJSON text.
    pub text: String,
    /// Output format; `kmz` is not available for text.
    pub target_format: String,
    /// CSV only: add a `wkt` column.
    #[serde(default)]
    pub csv_wkt: bool,
}

/// The converted text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertGeoTextResponseDto {
    /// The converted content.
    pub content: String,
    /// File extension of the format, without a dot.
    pub extension: String,
    /// What the target format cannot represent.
    pub notes: Vec<String>,
}
