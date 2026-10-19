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
    /// Set when the file could not be read; `collection` is then empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<GeoErrorDto>,
}
