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
    /// Set when the file could not be read; `collection` is then empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GeoErrorDto>,
}
