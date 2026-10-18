//! Wire types for metadata rename tokens (task 0267).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::LocationDto;

/// Resolves metadata tokens for files (`POST /api/v1/files/metadata-tokens`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "locations": [{"providerId": "local", "uri": "file:///Users/erik/Music/a.mp3"}],
    "tokens": ["artist", "title", "exif:date-taken:yyyy-MM-dd"]
}))]
pub struct ResolveMetadataTokensRequestDto {
    /// Files to read, at most 500 per request.
    pub locations: Vec<LocationDto>,
    /// Token names without braces, for example `artist` or `exif:date-taken:yyyy-MM-dd`.
    pub tokens: Vec<String>,
}

/// Token values for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetadataTokenValuesDto {
    /// The file the values belong to.
    pub location: LocationDto,
    /// Resolved values by requested token. A token the file has no value for is absent.
    /// Values are already sanitised for use in a file name.
    pub values: BTreeMap<String, String>,
}

/// Resolved tokens for the requested files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveMetadataTokensResponseDto {
    /// One entry per requested location, in request order.
    pub entries: Vec<MetadataTokenValuesDto>,
    /// Requested tokens that are not supported.
    pub unknown_tokens: Vec<String>,
}
