//! Wire types for splitting a file into parts and joining parts again (task 0257).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::LocationDto;

/// The options carried by a `splitFile` operation. Each source file is cut into
/// `<name>.001`, `<name>.002`, ... in the destination folder (next to the source when none is
/// given).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitFileDto {
    /// Size of every part except the last, in bytes (at least 1).
    pub part_size_bytes: u64,
    /// Also write `<name>.blake3`, listing the BLAKE3 hash of every part and of the whole file.
    #[serde(default)]
    pub write_manifest: bool,
}

/// Asks which split set a selection of part files belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectSplitSetRequestDto {
    /// One or more selected part files (`.001`, `.002`, ...), all in one folder.
    pub locations: Vec<LocationDto>,
}

/// One part of a detected split set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitPartDto {
    /// File name of the part.
    pub name: String,
    /// Size of the part in bytes.
    pub size: u64,
}

/// The split set a selection points at, and whether it can be joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectSplitSetResponseDto {
    /// Name of the file the parts join into.
    pub output_name: String,
    /// Parts found next to the selection, in order.
    pub parts: Vec<SplitPartDto>,
    /// Sum of the part sizes.
    pub total_size: u64,
    /// Whether a `<name>.blake3` manifest sits next to the parts and will be verified.
    pub has_manifest: bool,
    /// Why the set cannot be joined (gaps, damaged manifest, mixed selection); absent when ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Stable code of `error` that clients localise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Values the localised `error` interpolates.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub error_params: std::collections::BTreeMap<String, String>,
}
