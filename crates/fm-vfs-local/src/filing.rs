use fm_domain::{EntryId, Location};
use fm_vfs::{EntryRef, VfsError};

use crate::{invalid_location, map_io_error, stable_entry_id};

/// Renames an unchanged local regular file without overwriting an existing destination.
///
/// On Unix, the source and target directories are opened without following links and held
/// while the remaining checks and rename run relative to those directory descriptors.
pub fn rename_for_filing(
    source: &EntryRef,
    destination: &Location,
    directory_id: EntryId,
    size: Option<u64>,
    modified: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<EntryRef, VfsError> {
    let source_path = source
        .location
        .to_native_path()
        .map_err(|_| invalid_location(&source.location))?;
    let destination_path = destination
        .to_native_path()
        .map_err(|_| invalid_location(destination))?;
    let directory = destination
        .parent()
        .ok()
        .flatten()
        .ok_or_else(|| invalid_location(destination))?;
    let map_error = |error| map_io_error(error, &destination.uri);

    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags, RenameFlags, openat, renameat_with};
        let source_parent = open_parent(&source_path).map_err(map_error)?;
        let destination_parent = open_parent(&destination_path).map_err(map_error)?;
        let source_name = source_path
            .file_name()
            .ok_or_else(|| invalid_location(&source.location))?;
        let destination_name = destination_path
            .file_name()
            .ok_or_else(|| invalid_location(destination))?;
        let source_fd = openat(
            &source_parent,
            source_name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| map_error(error.into()))?;
        let source_metadata = std::fs::File::from(source_fd)
            .metadata()
            .map_err(map_error)?;
        let directory_metadata = destination_parent.metadata().map_err(map_error)?;
        if !matches_source(source, &source_metadata, size, modified)
            || stable_entry_id(&directory_metadata, &directory) != directory_id
        {
            return Err(invalid_location(&source.location));
        }
        renameat_with(
            &source_parent,
            source_name,
            &destination_parent,
            destination_name,
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| map_error(error.into()))?;
    }
    #[cfg(windows)]
    {
        let source_metadata = std::fs::symlink_metadata(&source_path).map_err(map_error)?;
        let parent = destination_path
            .parent()
            .ok_or_else(|| invalid_location(destination))?;
        let directory_metadata = std::fs::symlink_metadata(parent).map_err(map_error)?;
        if !matches_source(source, &source_metadata, size, modified)
            || !directory_metadata.is_dir()
            || crate::is_directory_link(&directory_metadata)
            || stable_entry_id(&directory_metadata, &directory) != directory_id
        {
            return Err(invalid_location(&source.location));
        }
        rename_no_replace(&source_path, &destination_path).map_err(map_error)?;
    }

    Ok(EntryRef {
        id: source.id,
        location: destination.clone(),
    })
}

fn matches_source(
    source: &EntryRef,
    metadata: &std::fs::Metadata,
    size: Option<u64>,
    modified: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    metadata.is_file()
        && stable_entry_id(metadata, &source.location) == source.id
        && size == Some(metadata.len())
        && modified == metadata.modified().ok().map(Into::into)
}

#[cfg(unix)]
fn open_parent(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::path::Component;
    let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open("/", flags, Mode::empty())?;
    for component in parent.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => directory = openat(&directory, name, flags, Mode::empty())?,
            _ => return Err(std::io::ErrorKind::InvalidInput.into()),
        }
    }
    Ok(directory.into())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn rename_no_replace(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileW;

    let source: Vec<_> = source.as_os_str().encode_wide().chain([0]).collect();
    let destination: Vec<_> = destination.as_os_str().encode_wide().chain([0]).collect();
    // The input vectors are valid, NUL-terminated path strings and live throughout the call.
    if unsafe { MoveFileW(source.as_ptr(), destination.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
