//! Wire representation of [`fm_domain::EntrySummary`] and
//! [`fm_domain::EntryMetadata`] (spec §5.2).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use fm_domain::{
    ArchiveInfo, EntryId, EntryKind, EntryMetadata, EntrySummary, GitFileStatus, ImageDimensions,
    MediaMetadata, OwnershipInfo, PermissionsInfo,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::location::LocationDto;

/// The kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum EntryKindDto {
    /// A regular file.
    File,
    /// A directory (or provider-specific container).
    Directory,
    /// A symbolic link or platform equivalent.
    Symlink,
}

impl From<EntryKind> for EntryKindDto {
    fn from(kind: EntryKind) -> Self {
        match kind {
            EntryKind::File => Self::File,
            EntryKind::Directory => Self::Directory,
            EntryKind::Symlink => Self::Symlink,
        }
    }
}

impl From<EntryKindDto> for EntryKind {
    fn from(kind: EntryKindDto) -> Self {
        match kind {
            EntryKindDto::File => Self::File,
            EntryKindDto::Directory => Self::Directory,
            EntryKindDto::Symlink => Self::Symlink,
        }
    }
}

/// An entry's git working-tree status (task 0135).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum GitFileStatusDto {
    /// No uncommitted, staged, untracked or ignored changes.
    Clean,
    /// Tracked, with unstaged working-tree changes.
    Modified,
    /// Staged in the index, with no further unstaged working-tree changes.
    Staged,
    /// Not tracked by git and not ignored.
    Untracked,
    /// Excluded by `.gitignore` (or equivalent) rules.
    Ignored,
}

impl From<GitFileStatus> for GitFileStatusDto {
    fn from(status: GitFileStatus) -> Self {
        match status {
            GitFileStatus::Clean => Self::Clean,
            GitFileStatus::Modified => Self::Modified,
            GitFileStatus::Staged => Self::Staged,
            GitFileStatus::Untracked => Self::Untracked,
            GitFileStatus::Ignored => Self::Ignored,
        }
    }
}

impl From<GitFileStatusDto> for GitFileStatus {
    fn from(status: GitFileStatusDto) -> Self {
        match status {
            GitFileStatusDto::Clean => Self::Clean,
            GitFileStatusDto::Modified => Self::Modified,
            GitFileStatusDto::Staged => Self::Staged,
            GitFileStatusDto::Untracked => Self::Untracked,
            GitFileStatusDto::Ignored => Self::Ignored,
        }
    }
}

/// A compact summary of a directory entry, suitable for directory listings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "id": "5b1b6b1e-9b1b-4b1b-8b1b-1b1b1b1b1b1b",
    "location": {"providerId": "local", "uri": "file:///Users/erik/report.pdf"},
    "name": "report.pdf",
    "kind": "file",
    "size": 1024,
    "modifiedAt": "2026-07-29T12:00:00Z",
    "createdAt": null,
    "hidden": false,
    "readOnly": false,
    "extension": "pdf",
    "mimeType": "application/pdf",
    "iconKey": "pdf",
    "metadataRevision": 0
}))]
pub struct EntrySummaryDto {
    /// Stable identifier for this entry.
    pub id: Uuid,
    /// The entry's location.
    pub location: LocationDto,
    /// The entry's display name (the last path segment).
    pub name: String,
    /// Whether this entry is a file, directory or symlink.
    pub kind: EntryKindDto,
    /// Size in bytes, when known and meaningful.
    pub size: Option<u64>,
    /// Last modification time, when reported by the provider.
    pub modified_at: Option<DateTime<Utc>>,
    /// Creation time, when reported by the provider.
    pub created_at: Option<DateTime<Utc>>,
    /// Whether the entry is hidden.
    pub hidden: bool,
    /// Whether the entry is read-only.
    pub read_only: bool,
    /// The file extension, without the leading dot, when applicable.
    pub extension: Option<String>,
    /// The detected MIME type, when known.
    pub mime_type: Option<String>,
    /// A key used to look up a display icon, when known.
    pub icon_key: Option<String>,
    /// Monotonic revision, incremented whenever this summary is refreshed.
    pub metadata_revision: u64,
    /// Git working-tree status, when this entry sits inside a local git
    /// working tree (task 0135).
    pub git_status: Option<GitFileStatusDto>,
}

impl From<EntrySummary> for EntrySummaryDto {
    fn from(entry: EntrySummary) -> Self {
        Self {
            id: entry.id.into(),
            location: entry.location.into(),
            name: entry.name,
            kind: entry.kind.into(),
            size: entry.size,
            modified_at: entry.modified_at,
            created_at: entry.created_at,
            hidden: entry.hidden,
            read_only: entry.read_only,
            extension: entry.extension,
            mime_type: entry.mime_type,
            icon_key: entry.icon_key,
            metadata_revision: entry.metadata_revision,
            git_status: entry.git_status.map(Into::into),
        }
    }
}

impl From<EntrySummaryDto> for EntrySummary {
    fn from(dto: EntrySummaryDto) -> Self {
        Self {
            id: EntryId::from(dto.id),
            location: dto.location.into(),
            name: dto.name,
            kind: dto.kind.into(),
            size: dto.size,
            modified_at: dto.modified_at,
            created_at: dto.created_at,
            hidden: dto.hidden,
            read_only: dto.read_only,
            extension: dto.extension,
            mime_type: dto.mime_type,
            icon_key: dto.icon_key,
            metadata_revision: dto.metadata_revision,
            git_status: dto.git_status.map(Into::into),
        }
    }
}

/// Filesystem permission information for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsInfoDto {
    /// Whether the current user can read the entry.
    pub readable: bool,
    /// Whether the current user can write the entry.
    pub writable: bool,
    /// Whether the current user can execute the entry.
    pub executable: bool,
    /// The raw Unix permission bits, on platforms that have them.
    pub unix_mode: Option<u32>,
}

impl From<PermissionsInfo> for PermissionsInfoDto {
    fn from(info: PermissionsInfo) -> Self {
        Self {
            readable: info.readable,
            writable: info.writable,
            executable: info.executable,
            unix_mode: info.unix_mode,
        }
    }
}

impl From<PermissionsInfoDto> for PermissionsInfo {
    fn from(dto: PermissionsInfoDto) -> Self {
        Self {
            readable: dto.readable,
            writable: dto.writable,
            executable: dto.executable,
            unix_mode: dto.unix_mode,
        }
    }
}

/// Ownership information for an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipInfoDto {
    /// The owning user name, when known.
    pub owner: Option<String>,
    /// The owning group name, when known.
    pub group: Option<String>,
}

impl From<OwnershipInfo> for OwnershipInfoDto {
    fn from(info: OwnershipInfo) -> Self {
        Self {
            owner: info.owner,
            group: info.group,
        }
    }
}

impl From<OwnershipInfoDto> for OwnershipInfo {
    fn from(dto: OwnershipInfoDto) -> Self {
        Self {
            owner: dto.owner,
            group: dto.group,
        }
    }
}

/// Pixel dimensions of an image entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageDimensionsDto {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl From<ImageDimensions> for ImageDimensionsDto {
    fn from(dimensions: ImageDimensions) -> Self {
        Self {
            width: dimensions.width,
            height: dimensions.height,
        }
    }
}

impl From<ImageDimensionsDto> for ImageDimensions {
    fn from(dto: ImageDimensionsDto) -> Self {
        Self {
            width: dto.width,
            height: dto.height,
        }
    }
}

/// Media (audio/video) metadata for an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetadataDto {
    /// Duration in seconds, when known.
    pub duration_seconds: Option<f64>,
    /// The media codec, when known.
    pub codec: Option<String>,
    /// Bitrate in bits per second, when known.
    pub bitrate_bps: Option<u64>,
}

impl From<MediaMetadata> for MediaMetadataDto {
    fn from(media: MediaMetadata) -> Self {
        Self {
            duration_seconds: media.duration_seconds,
            codec: media.codec,
            bitrate_bps: media.bitrate_bps,
        }
    }
}

impl From<MediaMetadataDto> for MediaMetadata {
    fn from(dto: MediaMetadataDto) -> Self {
        Self {
            duration_seconds: dto.duration_seconds,
            codec: dto.codec,
            bitrate_bps: dto.bitrate_bps,
        }
    }
}

/// Archive-specific metadata for an entry (for example a `.zip` file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfoDto {
    /// Number of entries contained in the archive, when known.
    pub entry_count: Option<u64>,
    /// Total uncompressed size in bytes, when known.
    pub uncompressed_size: Option<u64>,
    /// The entry's compressed size within its archive, in bytes, when known.
    pub compressed_size: Option<u64>,
    /// The compression method used for this entry, when known.
    pub compression_method: Option<String>,
}

impl From<ArchiveInfo> for ArchiveInfoDto {
    fn from(archive: ArchiveInfo) -> Self {
        Self {
            entry_count: archive.entry_count,
            uncompressed_size: archive.uncompressed_size,
            compressed_size: archive.compressed_size,
            compression_method: archive.compression_method,
        }
    }
}

impl From<ArchiveInfoDto> for ArchiveInfo {
    fn from(dto: ArchiveInfoDto) -> Self {
        Self {
            entry_count: dto.entry_count,
            uncompressed_size: dto.uncompressed_size,
            compressed_size: dto.compressed_size,
            compression_method: dto.compression_method,
        }
    }
}

/// Detailed, non-eagerly-fetched metadata for a single entry (spec §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryMetadataDto {
    /// The entry this metadata describes.
    pub entry_id: Uuid,
    /// Filesystem permissions, when the provider exposes them.
    pub permissions: Option<PermissionsInfoDto>,
    /// Ownership information, when the provider exposes it.
    pub ownership: Option<OwnershipInfoDto>,
    /// Extended attributes, keyed by attribute name.
    pub extended_attributes: BTreeMap<String, String>,
    /// Checksums, keyed by algorithm name (for example `"sha256"`).
    pub checksums: BTreeMap<String, String>,
    /// Image pixel dimensions, when the entry is an image.
    pub image_dimensions: Option<ImageDimensionsDto>,
    /// Audio/video metadata, when the entry is a media file.
    pub media: Option<MediaMetadataDto>,
    /// Archive metadata, when the entry is a browsable archive.
    pub archive: Option<ArchiveInfoDto>,
    /// Plugin-provided fields, keyed by a plugin-namespaced field name.
    #[schema(value_type = Object)]
    pub plugin_fields: BTreeMap<String, serde_json::Value>,
}

impl From<EntryMetadata> for EntryMetadataDto {
    fn from(metadata: EntryMetadata) -> Self {
        Self {
            entry_id: metadata.entry_id.into(),
            permissions: metadata.permissions.map(Into::into),
            ownership: metadata.ownership.map(Into::into),
            extended_attributes: metadata.extended_attributes,
            checksums: metadata.checksums,
            image_dimensions: metadata.image_dimensions.map(Into::into),
            media: metadata.media.map(Into::into),
            archive: metadata.archive.map(Into::into),
            plugin_fields: metadata.plugin_fields,
        }
    }
}

impl From<EntryMetadataDto> for EntryMetadata {
    fn from(dto: EntryMetadataDto) -> Self {
        Self {
            entry_id: EntryId::from(dto.entry_id),
            permissions: dto.permissions.map(Into::into),
            ownership: dto.ownership.map(Into::into),
            extended_attributes: dto.extended_attributes,
            checksums: dto.checksums,
            image_dimensions: dto.image_dimensions.map(Into::into),
            media: dto.media.map(Into::into),
            archive: dto.archive.map(Into::into),
            plugin_fields: dto.plugin_fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use fm_domain::{Location, ProviderId};

    use super::*;

    fn sample_entry_dto() -> EntrySummaryDto {
        EntrySummaryDto {
            id: Uuid::new_v4(),
            location: LocationDto {
                provider_id: "local".to_owned(),
                uri: "file:///Users/erik/report.pdf".to_owned(),
            },
            name: "report.pdf".to_owned(),
            kind: EntryKindDto::File,
            size: Some(1024),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: Some("pdf".to_owned()),
            mime_type: Some("application/pdf".to_owned()),
            icon_key: Some("pdf".to_owned()),
            metadata_revision: 0,
            git_status: Some(GitFileStatusDto::Modified),
        }
    }

    #[test]
    fn entry_summary_dto_round_trips_through_serde_json() {
        let dto = sample_entry_dto();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: EntrySummaryDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn entry_summary_dto_uses_camel_case_field_names() {
        let json = serde_json::to_string(&sample_entry_dto()).expect("serialization must succeed");
        for field in [
            "\"readOnly\"",
            "\"mimeType\"",
            "\"iconKey\"",
            "\"metadataRevision\"",
            "\"modifiedAt\"",
            "\"createdAt\"",
            "\"gitStatus\"",
        ] {
            assert!(json.contains(field), "expected {json} to contain {field}");
        }
    }

    #[test]
    fn entry_kind_dto_round_trips_for_every_variant() {
        for kind in [
            EntryKindDto::File,
            EntryKindDto::Directory,
            EntryKindDto::Symlink,
        ] {
            let json = serde_json::to_string(&kind).expect("serialization must succeed");
            let parsed: EntryKindDto =
                serde_json::from_str(&json).expect("deserialization must succeed");
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn entry_summary_dto_converts_to_and_from_the_domain_type() {
        let entry = EntrySummary {
            id: EntryId::new(),
            location: Location::new(ProviderId::new("local"), "file:///Users/erik/report.pdf"),
            name: "report.pdf".to_owned(),
            kind: EntryKind::File,
            size: Some(1024),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: Some("pdf".to_owned()),
            mime_type: Some("application/pdf".to_owned()),
            icon_key: Some("pdf".to_owned()),
            metadata_revision: 0,
            git_status: Some(GitFileStatus::Staged),
        };

        let dto: EntrySummaryDto = entry.clone().into();
        let round_tripped: EntrySummary = dto.into();
        assert_eq!(entry, round_tripped);
    }

    #[test]
    fn entry_metadata_dto_round_trips_through_serde_json_and_the_domain_type() {
        let metadata = EntryMetadata {
            entry_id: EntryId::new(),
            permissions: Some(PermissionsInfo {
                readable: true,
                writable: false,
                executable: false,
                unix_mode: Some(0o644),
            }),
            ownership: Some(OwnershipInfo {
                owner: Some("erik".to_owned()),
                group: Some("staff".to_owned()),
            }),
            extended_attributes: BTreeMap::from([(
                "com.apple.quarantine".to_owned(),
                "0081".to_owned(),
            )]),
            checksums: BTreeMap::from([("sha256".to_owned(), "deadbeef".to_owned())]),
            image_dimensions: Some(ImageDimensions {
                width: 1920,
                height: 1080,
            }),
            media: Some(MediaMetadata {
                duration_seconds: Some(125.5),
                codec: Some("h264".to_owned()),
                bitrate_bps: Some(8_000_000),
            }),
            archive: Some(ArchiveInfo {
                entry_count: Some(42),
                uncompressed_size: Some(1_048_576),
                compressed_size: Some(524_288),
                compression_method: Some("Deflated".to_owned()),
            }),
            plugin_fields: BTreeMap::from([("custom.rating".to_owned(), serde_json::json!(5))]),
        };

        let dto: EntryMetadataDto = metadata.clone().into();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        assert!(json.contains("\"entryId\""));
        assert!(json.contains("\"extendedAttributes\""));
        assert!(json.contains("\"imageDimensions\""));
        assert!(json.contains("\"pluginFields\""));

        let parsed: EntryMetadataDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);

        let round_tripped: EntryMetadata = parsed.into();
        assert_eq!(metadata, round_tripped);
    }
}
