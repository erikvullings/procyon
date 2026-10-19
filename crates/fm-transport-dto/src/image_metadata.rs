//! Wire types for the full image metadata viewer (task 0273).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::LocationDto;

/// Reads all metadata tags of an image (`POST /api/v1/files/image-metadata`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadImageMetadataRequestDto {
    /// The image file.
    pub location: LocationDto,
}

/// One metadata tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageTagDto {
    /// Tag name, for example `Make` or `dc:title`.
    pub name: String,
    /// Human-readable value.
    pub value: String,
    /// The raw stored value when it differs from `value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}

/// Tags of one namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageTagGroupDto {
    /// `EXIF`, `GPS`, `IPTC`, `XMP`, `ICC`, `PNG` or `File`.
    pub namespace: String,
    /// The tags of the namespace.
    pub tags: Vec<ImageTagDto>,
}

/// A decimal GPS position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GpsPositionDto {
    /// Degrees north; negative is south.
    pub latitude: f64,
    /// Degrees east; negative is west.
    pub longitude: f64,
}

/// All metadata of an image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadataDto {
    /// Detected container format; absent when the file is not a supported image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// False when the format cannot be parsed, so the UI can say so instead of showing nothing.
    pub supported: bool,
    /// Non-empty tag groups.
    pub groups: Vec<ImageTagGroupDto>,
    /// GPS position when the EXIF GPS tags are complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<GpsPositionDto>,
}
