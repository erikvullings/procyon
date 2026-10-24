//! Wire types for the permissions and ownership editor (task 0258).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{EntryKindDto, LocationDto};

/// Which entries a (recursive) attribute change touches. Directories are always descended into;
/// the scope only decides which visited entries are changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AttributeScopeDto {
    /// Files and folders.
    #[default]
    Both,
    /// Files only.
    FilesOnly,
    /// Folders only.
    FoldersOnly,
}

/// Fields to change; an absent field is left untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeChangeDto {
    /// New Unix permission bits (0 through 0o7777: rwx, setuid, setgid, sticky).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    /// Which bits of `mode` to apply; the others keep each entry's own value. All twelve when
    /// absent. Lets a mixed selection change only the bits the user touched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode_mask: Option<u32>,
    /// New numeric owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// New numeric group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    /// Windows read-only attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// Windows hidden attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Windows archive attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<bool>,
}

/// The change carried by a `changeAttributes` operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAttributesDto {
    /// What to change.
    pub change: AttributeChangeDto,
    /// Descend into selected folders.
    #[serde(default)]
    pub recursive: bool,
    /// Which visited entries to change.
    #[serde(default)]
    pub scope: AttributeScopeDto,
}

/// Editable attribute state of one entry; an absent field is not exposed by the provider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSnapshotDto {
    /// Unix permission bits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    /// Numeric owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// Numeric group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    /// Windows read-only attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// Windows hidden attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Windows archive attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<bool>,
}

/// Which fields every selected entry's provider lets the user edit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeSupportDto {
    /// Unix permission bits.
    pub mode: bool,
    /// Numeric owner and group.
    pub ownership: bool,
    /// Windows read-only, hidden and archive attributes.
    pub windows_attributes: bool,
}

/// Why nothing can be edited for a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum AttributeUnsupportedReasonDto {
    /// The provider of at least one selected entry cannot change permissions.
    ProviderUnsupported,
    /// The selection spans providers with no editable field in common.
    MixedProviders,
    /// Symbolic links are never modified.
    SymbolicLink,
}

/// Reads the editable state of a selection (`POST /api/v1/attributes/inspect`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectAttributesRequestDto {
    /// Selected entries.
    pub sources: Vec<LocationDto>,
}

/// Current state of one selected entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AttributeEntryDto {
    /// The entry.
    pub location: LocationDto,
    /// File, folder or symbolic link.
    pub kind: EntryKindDto,
    /// Editable state; absent when the entry cannot be read or edited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<AttributeSnapshotDto>,
}

/// Result of inspecting a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InspectAttributesResponseDto {
    /// Fields editable for every selected entry.
    pub support: AttributeSupportDto,
    /// Why `support` is empty, when it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<AttributeUnsupportedReasonDto>,
    /// State of each selected entry in request order.
    pub entries: Vec<AttributeEntryDto>,
}

/// Counts what a `changeAttributes` operation would touch (`POST /api/v1/attributes/preview`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAttributeChangeRequestDto {
    /// Selected entries.
    pub sources: Vec<LocationDto>,
    /// Descend into selected folders.
    #[serde(default)]
    pub recursive: bool,
    /// Which visited entries would be changed.
    #[serde(default)]
    pub scope: AttributeScopeDto,
}

/// Number of entries a change would be applied to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAttributeChangeResponseDto {
    /// Entries that would be changed.
    pub total: u64,
    /// Of which files.
    pub files: u64,
    /// Of which folders.
    pub folders: u64,
    /// Symbolic links that would be left alone.
    pub skipped_symlinks: u64,
    /// The count stopped at the enumeration limit and is a lower bound.
    pub truncated: bool,
}
