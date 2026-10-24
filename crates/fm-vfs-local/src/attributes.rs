//! Permission, ownership and attribute editing for local entries (task 0258).
//!
//! Every function inspects the entry with `symlink_metadata` first and refuses symbolic links, so
//! a link can never redirect a change onto its target.

use std::path::Path;

use fm_vfs::{
    AttributeChange, AttributeSnapshot, AttributeSupport, ProviderCapabilities, VfsError,
};

#[cfg(unix)]
use fm_vfs::EDITABLE_MODE_MASK;

use crate::map_io_error;

pub(crate) const fn support() -> AttributeSupport {
    AttributeSupport {
        mode: cfg!(unix),
        ownership: cfg!(unix),
        windows_attributes: cfg!(windows),
    }
}

fn unsupported() -> VfsError {
    VfsError::UnsupportedCapability {
        capability: ProviderCapabilities::SET_PERMISSIONS,
    }
}

fn inspect(path: &Path, uri: &str) -> Result<std::fs::Metadata, VfsError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| map_io_error(error, uri))?;
    if metadata.file_type().is_symlink() {
        return Err(unsupported());
    }
    Ok(metadata)
}

pub(crate) fn read(path: &Path, uri: &str) -> Result<AttributeSnapshot, VfsError> {
    let metadata = inspect(path, uri)?;
    Ok(snapshot(&metadata))
}

pub(crate) fn apply(path: &Path, uri: &str, change: AttributeChange) -> Result<(), VfsError> {
    if change.exceeds(support()) {
        return Err(unsupported());
    }
    let metadata = inspect(path, uri)?;
    apply_platform(path, uri, change, &metadata)
}

#[cfg(unix)]
fn snapshot(metadata: &std::fs::Metadata) -> AttributeSnapshot {
    use std::os::unix::fs::MetadataExt;
    AttributeSnapshot {
        mode: Some(metadata.mode() & EDITABLE_MODE_MASK),
        uid: Some(metadata.uid()),
        gid: Some(metadata.gid()),
        ..AttributeSnapshot::default()
    }
}

#[cfg(unix)]
fn apply_platform(
    path: &Path,
    uri: &str,
    change: AttributeChange,
    _metadata: &std::fs::Metadata,
) -> Result<(), VfsError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, lchown};
    // Ownership first: changing the owner clears setuid/setgid, which the mode step then restores.
    if change.uid.is_some() || change.gid.is_some() {
        lchown(path, change.uid, change.gid).map_err(|error| map_io_error(error, uri))?;
    }
    if change.mode.is_some() {
        // Re-read so bits the kernel cleared while changing the owner are not silently restored.
        let current = std::fs::symlink_metadata(path)
            .map_err(|error| map_io_error(error, uri))?
            .mode();
        if let Some(mode) = change.resolve_mode(current) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .map_err(|error| map_io_error(error, uri))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn snapshot(metadata: &std::fs::Metadata) -> AttributeSnapshot {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY,
    };
    let attributes = metadata.file_attributes();
    AttributeSnapshot {
        read_only: Some(attributes & FILE_ATTRIBUTE_READONLY != 0),
        hidden: Some(attributes & FILE_ATTRIBUTE_HIDDEN != 0),
        archive: Some(attributes & FILE_ATTRIBUTE_ARCHIVE != 0),
        ..AttributeSnapshot::default()
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn apply_platform(
    path: &Path,
    uri: &str,
    change: AttributeChange,
    metadata: &std::fs::Metadata,
) -> Result<(), VfsError> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
    };
    let mut attributes = metadata.file_attributes();
    for (value, flag) in [
        (change.read_only, FILE_ATTRIBUTE_READONLY),
        (change.hidden, FILE_ATTRIBUTE_HIDDEN),
        (change.archive, FILE_ATTRIBUTE_ARCHIVE),
    ] {
        match value {
            Some(true) => attributes |= flag,
            Some(false) => attributes &= !flag,
            None => {}
        }
    }
    if attributes == 0 {
        attributes = FILE_ATTRIBUTE_NORMAL;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call.
    let ok = unsafe { SetFileAttributesW(wide.as_ptr(), attributes) };
    if ok == 0 {
        return Err(map_io_error(std::io::Error::last_os_error(), uri));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn snapshot(_metadata: &std::fs::Metadata) -> AttributeSnapshot {
    AttributeSnapshot::default()
}

#[cfg(not(any(unix, windows)))]
fn apply_platform(
    _path: &Path,
    _uri: &str,
    _change: AttributeChange,
    _metadata: &std::fs::Metadata,
) -> Result<(), VfsError> {
    Err(unsupported())
}
