//! Directory entry summaries and detailed, lazily fetched metadata
//! (spec §5.2).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::EntryId;
use crate::location::Location;

/// The kind of a directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory (or provider-specific container, such as an archive
    /// browsed through the `archive://` scheme).
    Directory,
    /// A symbolic link or platform equivalent (junction, alias).
    Symlink,
}

/// An entry's git working-tree status (task 0135), local provider only.
///
/// For a directory, this is the aggregate of its descendants' statuses
/// (highest-priority state wins: [`Self::Modified`] > [`Self::Staged`] >
/// [`Self::Untracked`] > [`Self::Ignored`] > [`Self::Clean`]), matching
/// common IDE file-tree conventions ("contains changes").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitFileStatus {
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

/// One commit in a file's git history (task 0135's Alt+Space history section), local provider
/// only. Ordered newest-first, matching `git log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLogEntry {
    /// The commit's full SHA-1 (or SHA-256, for repositories using that object format).
    pub commit_id: String,
    /// The commit's abbreviated id, as `git log --oneline` would show it.
    pub short_id: String,
    /// The commit author's display name.
    pub author_name: String,
    /// The commit author's email address.
    pub author_email: String,
    /// When the commit was authored.
    pub committed_at: DateTime<Utc>,
    /// The commit message's first line.
    pub summary: String,
}

/// A compact summary of a directory entry, suitable for directory listings.
///
/// Expensive metadata (permissions, checksums, media info, ...) is
/// deliberately not part of this type — see [`EntryMetadata`], fetched
/// lazily through a separate request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    /// Stable identifier for this entry.
    pub id: EntryId,
    /// The entry's location.
    pub location: Location,
    /// The entry's display name (the last path segment).
    pub name: String,
    /// Whether this entry is a file, directory or symlink.
    pub kind: EntryKind,
    /// Size in bytes, when known and meaningful (absent for directories on
    /// providers that do not report it eagerly).
    pub size: Option<u64>,
    /// Last modification time, when reported by the provider.
    pub modified_at: Option<DateTime<Utc>>,
    /// Creation time, when reported by the provider.
    pub created_at: Option<DateTime<Utc>>,
    /// Whether the entry is hidden (dotfile, Windows hidden attribute, ...).
    pub hidden: bool,
    /// Whether the entry is read-only.
    pub read_only: bool,
    /// The file extension, without the leading dot, when applicable.
    pub extension: Option<String>,
    /// The detected MIME type, when known.
    pub mime_type: Option<String>,
    /// A key used to look up a display icon, when known.
    pub icon_key: Option<String>,
    /// Monotonic revision, incremented whenever this summary is refreshed;
    /// used to reject stale metadata responses.
    pub metadata_revision: u64,
    /// Git working-tree status, when this entry sits inside a local git
    /// working tree. `None` outside a working tree, or on non-local
    /// providers, which are out of scope for task 0135.
    pub git_status: Option<GitFileStatus>,
}

/// Filesystem permission information for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionsInfo {
    /// Whether the current user can read the entry.
    pub readable: bool,
    /// Whether the current user can write the entry.
    pub writable: bool,
    /// Whether the current user can execute the entry.
    pub executable: bool,
    /// The raw Unix permission bits, on platforms that have them.
    pub unix_mode: Option<u32>,
}

/// Ownership information for an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipInfo {
    /// The owning user name, when known.
    pub owner: Option<String>,
    /// The owning group name, when known.
    pub group: Option<String>,
}

/// Pixel dimensions of an image entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageDimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Media (audio/video) metadata for an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadata {
    /// Duration in seconds, when known.
    pub duration_seconds: Option<f64>,
    /// The media codec, when known.
    pub codec: Option<String>,
    /// Bitrate in bits per second, when known.
    pub bitrate_bps: Option<u64>,
}

/// Archive-specific metadata for an entry (for example a `.zip` file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveInfo {
    /// Number of entries contained in the archive, when known.
    pub entry_count: Option<u64>,
    /// Total uncompressed size in bytes, when known.
    pub uncompressed_size: Option<u64>,
    /// The entry's compressed size within its archive, in bytes, when known
    /// (per-entry, not the whole container).
    pub compressed_size: Option<u64>,
    /// The compression method used for this entry (for example `"Deflated"`
    /// or `"Stored"`), when known.
    pub compression_method: Option<String>,
}

/// Detailed, non-eagerly-fetched metadata for a single entry (spec §5.2).
///
/// Retrieved through a dedicated metadata request, never as part of a
/// directory listing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryMetadata {
    /// The entry this metadata describes.
    pub entry_id: EntryId,
    /// Filesystem permissions, when the provider exposes them.
    pub permissions: Option<PermissionsInfo>,
    /// Ownership information, when the provider exposes it.
    pub ownership: Option<OwnershipInfo>,
    /// Extended attributes, keyed by attribute name.
    pub extended_attributes: BTreeMap<String, String>,
    /// Checksums, keyed by algorithm name (for example `"sha256"`).
    pub checksums: BTreeMap<String, String>,
    /// Image pixel dimensions, when the entry is an image.
    pub image_dimensions: Option<ImageDimensions>,
    /// Audio/video metadata, when the entry is a media file.
    pub media: Option<MediaMetadata>,
    /// Archive metadata, when the entry is a browsable archive.
    pub archive: Option<ArchiveInfo>,
    /// Plugin-provided fields, keyed by a plugin-namespaced field name.
    pub plugin_fields: BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::ids::ProviderId;

    fn sample_entry() -> EntrySummary {
        EntrySummary {
            id: EntryId::new(),
            location: Location::new(ProviderId::new("file"), "file:///Users/erik/report.pdf"),
            name: "report.pdf".to_owned(),
            kind: EntryKind::File,
            size: Some(1024),
            modified_at: Some(Utc.with_ymd_and_hms(2026, 7, 29, 12, 0, 0).unwrap()),
            created_at: None,
            hidden: false,
            read_only: false,
            extension: Some("pdf".to_owned()),
            mime_type: Some("application/pdf".to_owned()),
            icon_key: Some("pdf".to_owned()),
            metadata_revision: 0,
            git_status: Some(GitFileStatus::Modified),
        }
    }

    #[test]
    fn entry_summary_round_trips_through_serde_json() {
        let entry = sample_entry();
        let json = serde_json::to_string(&entry).expect("serialization must succeed");
        let parsed: EntrySummary =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(entry, parsed);
    }

    #[test]
    fn entry_summary_allows_unavailable_metadata_fields_to_be_absent() {
        let mut entry = sample_entry();
        entry.size = None;
        entry.modified_at = None;
        entry.created_at = None;
        entry.extension = None;
        entry.mime_type = None;
        entry.icon_key = None;

        let json = serde_json::to_string(&entry).expect("serialization must succeed");
        let parsed: EntrySummary =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(entry, parsed);
    }

    #[test]
    fn directory_entry_kind_round_trips_through_serde_json() {
        for kind in [EntryKind::File, EntryKind::Directory, EntryKind::Symlink] {
            let json = serde_json::to_string(&kind).expect("serialization must succeed");
            let parsed: EntryKind =
                serde_json::from_str(&json).expect("deserialization must succeed");
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn entry_metadata_round_trips_with_all_categories_absent() {
        let metadata = EntryMetadata {
            entry_id: EntryId::new(),
            permissions: None,
            ownership: None,
            extended_attributes: Default::default(),
            checksums: Default::default(),
            image_dimensions: None,
            media: None,
            archive: None,
            plugin_fields: Default::default(),
        };
        let json = serde_json::to_string(&metadata).expect("serialization must succeed");
        let parsed: EntryMetadata =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(metadata, parsed);
    }

    #[test]
    fn entry_metadata_round_trips_with_all_categories_populated() {
        let mut extended_attributes = std::collections::BTreeMap::new();
        extended_attributes.insert("com.apple.quarantine".to_owned(), "0081".to_owned());
        let mut checksums = std::collections::BTreeMap::new();
        checksums.insert("sha256".to_owned(), "deadbeef".to_owned());
        let mut plugin_fields = std::collections::BTreeMap::new();
        plugin_fields.insert("custom.rating".to_owned(), serde_json::json!(5));

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
            extended_attributes,
            checksums,
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
            plugin_fields,
        };

        let json = serde_json::to_string(&metadata).expect("serialization must succeed");
        let parsed: EntryMetadata =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(metadata, parsed);
    }
}
