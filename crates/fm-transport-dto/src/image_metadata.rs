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
    /// Field ids this file's format can store and write; empty when it cannot be edited.
    #[serde(default)]
    pub editable_fields: Vec<String>,
}

/// A calendar date and time without a time zone, as stored by cameras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTimeDto {
    /// Four-digit year.
    pub year: u16,
    /// Month 1-12.
    pub month: u8,
    /// Day 1-31.
    pub day: u8,
    /// Hour 0-23.
    pub hour: u8,
    /// Minute 0-59.
    pub minute: u8,
    /// Second 0-59.
    pub second: u8,
}

/// Metadata changes applied to every file of an `editImageMetadata` operation (task 0274).
///
/// A field that is absent is left alone; a field listed in `clearFields` is removed. The ids are
/// the ones returned in `editableFields`: `dateTaken`, `gps`, `description`, `copyright`,
/// `author`, `orientation`, `title`, `keywords` and `rating`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadataEditDto {
    /// Remove EXIF, XMP and IPTC; the orientation is kept.
    #[serde(default)]
    pub remove_all: bool,
    /// Remove GPS coordinates only.
    #[serde(default)]
    pub remove_location: bool,
    /// Fields to remove.
    #[serde(default)]
    pub clear_fields: Vec<String>,
    /// Keep the original file as `<name>.orig` next to it; required for undo.
    #[serde(default)]
    pub backup_original: bool,
    /// New capture time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_taken: Option<CaptureTimeDto>,
    /// Shift each file's own capture time by this many seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift_date_seconds: Option<i64>,
    /// New GPS position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<GpsPositionDto>,
    /// New title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// New copyright notice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    /// New author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// New keywords.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    /// New rating 0-5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    /// New EXIF orientation 1-8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<u8>,
}

/// Reads the GPS position of many images cheaply (`POST /api/v1/files/image-gps`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadImageGpsRequestDto {
    /// Images to read, at most 100 per request.
    pub locations: Vec<LocationDto>,
}

/// GPS result for one image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageGpsEntryDto {
    /// The image.
    pub location: LocationDto,
    /// Position when the image is geotagged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gps: Option<GpsPositionDto>,
    /// Capture time as `YYYY-MM-DDTHH:MM:SS` (no time zone) when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_at: Option<String>,
    /// True when the file could not be read (missing, unreadable).
    #[serde(default)]
    pub unreadable: bool,
}

/// GPS results in request order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadImageGpsResponseDto {
    /// One entry per requested location.
    pub entries: Vec<ImageGpsEntryDto>,
}
