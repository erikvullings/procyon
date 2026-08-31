use std::path::PathBuf;

/// A mounted volume or drive reported by the operating system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedVolume {
    /// Human-readable volume/drive name.
    pub name: String,
    /// Filesystem path the volume is mounted at.
    pub mount_point: PathBuf,
}

/// Total and available capacity for the volume backing a filesystem path
/// (task 0096), used to render a Marta/Finder-style status bar segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeCapacity {
    /// Total capacity of the volume, in bytes.
    pub total_bytes: u64,
    /// Currently available (free) capacity of the volume, in bytes.
    pub available_bytes: u64,
}

/// Broad presentation category for an operating-system-managed location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemLocationKind {
    /// A filesystem location synchronized or mounted by a cloud provider.
    Cloud,
    /// A filesystem mounted from another computer through the operating system.
    Network,
}

/// A filesystem location discovered from operating-system conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemLocation {
    /// Stable human-readable label suitable for navigation UI.
    pub name: String,
    /// Native absolute path, later resolved through the existing local provider.
    pub path: PathBuf,
    /// Presentation category.
    pub kind: SystemLocationKind,
    /// Optional advisory vendor hint. File semantics never depend on this value.
    pub provider_hint: Option<String>,
    /// Optional lower-case mount protocol, for example `smb`.
    pub protocol: Option<String>,
    /// Optional remote server name supplied by the operating system.
    pub server: Option<String>,
    /// Optional remote share name supplied by the operating system.
    pub share: Option<String>,
    /// Whether the mounted filesystem is read-only, when detectable.
    pub read_only: Option<bool>,
}

/// One of Finder's seven label colors, or no color (task 0136).
///
/// A tagged file's `com.apple.metadata:_kMDItemUserTags` xattr is a binary
/// property list array of strings; a colored tag is encoded as
/// `"<name>\n<digit>"`, where `<digit>` is this color's index. These indices
/// are undocumented by Apple but long-stable and shared by every known
/// open-source reader/writer (e.g. the `tag` CLI, <https://github.com/jdberry/tag>,
/// and what `mdls`/`xattr -p` decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FinderTagColor {
    /// No label color.
    #[default]
    None,
    /// Finder's built-in "Gray" label color.
    Gray,
    /// Finder's built-in "Green" label color.
    Green,
    /// Finder's built-in "Purple" label color.
    Purple,
    /// Finder's built-in "Blue" label color.
    Blue,
    /// Finder's built-in "Yellow" label color.
    Yellow,
    /// Finder's built-in "Red" label color.
    Red,
    /// Finder's built-in "Orange" label color.
    Orange,
}

impl FinderTagColor {
    /// The color's index within the `\n<digit>` suffix.
    #[must_use]
    pub fn to_index(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Gray => 1,
            Self::Green => 2,
            Self::Purple => 3,
            Self::Blue => 4,
            Self::Yellow => 5,
            Self::Red => 6,
            Self::Orange => 7,
        }
    }

    /// Recovers a color from its `\n<digit>` suffix index. An index outside
    /// `0..=7` (never produced by Finder itself) is treated as no color
    /// rather than rejected, so a foreign or corrupted xattr degrades to an
    /// uncolored tag instead of failing to load every tag on the entry.
    #[must_use]
    pub fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Gray,
            2 => Self::Green,
            3 => Self::Purple,
            4 => Self::Blue,
            5 => Self::Yellow,
            6 => Self::Red,
            7 => Self::Orange,
            _ => Self::None,
        }
    }
}

/// A single Finder tag (task 0136): a name, and an optional label color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinderTag {
    /// The tag's display name, e.g. `"Work"` or one of Finder's seven
    /// built-in color names (`"Red"`, `"Orange"`, ...).
    pub name: String,
    /// The tag's label color, if any.
    pub color: FinderTagColor,
}

/// A file or folder discovered under one of task 0148's well-known macOS
/// locations that appears to belong to an application bundle being
/// uninstalled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallCandidate {
    /// Absolute native path of the discovered file or folder.
    pub path: PathBuf,
    /// Total size in bytes (recursive for a directory).
    pub size_bytes: u64,
    /// Whether this candidate can actually be moved to the Trash by this
    /// feature. `false` for matches under `/Library`, which require
    /// elevation this task deliberately does not implement (task 0148: "out
    /// of scope") - such candidates are still reported so the user can see
    /// them, but are never offered for removal.
    pub removable: bool,
}

/// The result of scanning for an application bundle's related files (task
/// 0148), returned for the user to review before anything is deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationUninstallPlan {
    /// The bundle's `CFBundleIdentifier`, when its `Info.plist` declared one.
    pub bundle_identifier: Option<String>,
    /// The bundle's product name, used for the fallback match when no
    /// identifier is available. Falls back to the bundle's file-stem when
    /// `Info.plist` has neither `CFBundleName` nor `CFBundleDisplayName`.
    pub product_name: String,
    /// Related files discovered outside the bundle itself.
    pub related_files: Vec<UninstallCandidate>,
}

/// Platform-facing discovery abstraction for OS-managed locations.
pub trait SystemLocationProvider: Send + Sync {
    /// Discovers currently reachable locations. Missing providers are omitted.
    fn system_locations(&self) -> Result<Vec<SystemLocation>, crate::PlatformError>;
}

impl<T: crate::PlatformAdapter + ?Sized> SystemLocationProvider for T {
    fn system_locations(&self) -> Result<Vec<SystemLocation>, crate::PlatformError> {
        crate::PlatformAdapter::system_locations(self)
    }
}

/// Classifies a cloud folder name without relying on a user-specific path.
#[must_use]
pub fn cloud_provider_hint(name: &str) -> Option<&'static str> {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("onedrive") {
        Some("onedrive")
    } else if normalized.contains("icloud") || normalized.contains("mobile documents") {
        Some("icloud")
    } else if normalized.contains("dropbox") {
        Some("dropbox")
    } else if normalized.contains("google drive") || normalized.starts_with("google-drive") {
        Some("google-drive")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{FinderTagColor, SystemLocation, SystemLocationKind, cloud_provider_hint};
    use std::path::PathBuf;

    #[test]
    fn finder_tag_color_round_trips_through_its_index_for_every_variant() {
        for color in [
            FinderTagColor::None,
            FinderTagColor::Gray,
            FinderTagColor::Green,
            FinderTagColor::Purple,
            FinderTagColor::Blue,
            FinderTagColor::Yellow,
            FinderTagColor::Red,
            FinderTagColor::Orange,
        ] {
            assert_eq!(FinderTagColor::from_index(color.to_index()), color);
        }
    }

    #[test]
    fn finder_tag_color_indices_match_the_stable_tag_cli_convention() {
        assert_eq!(FinderTagColor::None.to_index(), 0);
        assert_eq!(FinderTagColor::Gray.to_index(), 1);
        assert_eq!(FinderTagColor::Green.to_index(), 2);
        assert_eq!(FinderTagColor::Purple.to_index(), 3);
        assert_eq!(FinderTagColor::Blue.to_index(), 4);
        assert_eq!(FinderTagColor::Yellow.to_index(), 5);
        assert_eq!(FinderTagColor::Red.to_index(), 6);
        assert_eq!(FinderTagColor::Orange.to_index(), 7);
    }

    #[test]
    fn an_out_of_range_color_index_degrades_to_no_color_instead_of_panicking() {
        assert_eq!(FinderTagColor::from_index(8), FinderTagColor::None);
        assert_eq!(FinderTagColor::from_index(255), FinderTagColor::None);
    }

    #[test]
    fn classifies_common_cloud_folder_names_case_insensitively() {
        assert_eq!(
            cloud_provider_hint("OneDrive – Example Corp"),
            Some("onedrive")
        );
        assert_eq!(cloud_provider_hint("iCloud Drive"), Some("icloud"));
        assert_eq!(cloud_provider_hint("Dropbox"), Some("dropbox"));
        assert_eq!(cloud_provider_hint("Google Drive"), Some("google-drive"));
        assert_eq!(cloud_provider_hint("Projects"), None);
    }

    #[test]
    fn network_locations_carry_optional_mount_metadata() {
        let location = SystemLocation {
            name: "Team Files".to_owned(),
            path: PathBuf::from("/Volumes/Team Files"),
            kind: SystemLocationKind::Network,
            provider_hint: None,
            protocol: Some("smb".to_owned()),
            server: Some("files.example.test".to_owned()),
            share: Some("team".to_owned()),
            read_only: Some(true),
        };

        assert_eq!(location.kind, SystemLocationKind::Network);
        assert_eq!(location.protocol.as_deref(), Some("smb"));
        assert_eq!(location.server.as_deref(), Some("files.example.test"));
        assert_eq!(location.share.as_deref(), Some("team"));
        assert_eq!(location.read_only, Some(true));
    }
}
