//! NTFS directory junctions (mount-point reparse points).
//!
//! Junctions need no special privilege, unlike symbolic links, but only point at absolute local
//! directories. They are a distinct link kind and are never produced by the symlink path.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, Prefix};

use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_WRITE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
const REPARSE_HEADER_BYTES: usize = 8;
const MOUNT_POINT_HEADER_BYTES: usize = 8;
const MAX_REPARSE_BYTES: usize = 16 * 1024;

/// Why a junction could not be created.
#[derive(Debug)]
pub(crate) enum JunctionError {
    /// The target is not an absolute drive-letter path.
    InvalidTarget,
    /// The filesystem rejected the request.
    Io(io::Error),
}

/// Creates a junction at `link` pointing at the absolute local directory `target`.
///
/// The link directory is created exclusively, so an existing destination fails with
/// `AlreadyExists` and is never replaced. A half-created link directory is removed on failure.
pub(crate) fn create(target: &Path, link: &Path) -> Result<(), JunctionError> {
    let target = drive_path(target).ok_or(JunctionError::InvalidTarget)?;
    let substitute: Vec<u16> = OsStr::new("\\??\\")
        .encode_wide()
        .chain(target.iter().copied())
        .collect();
    let buffer = reparse_buffer(&substitute, &target).ok_or(JunctionError::InvalidTarget)?;
    std::fs::create_dir(link).map_err(JunctionError::Io)?;
    let result = set_reparse_point(link, &buffer);
    if result.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    result.map_err(JunctionError::Io)
}

/// Returns `C:\...` as UTF-16 for a verbatim or plain drive-letter path.
fn drive_path(target: &Path) -> Option<Vec<u16>> {
    let mut components = target.components();
    let letter = match components.next()? {
        Component::Prefix(prefix) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return None,
        },
        _ => return None,
    };
    if components.next()? != Component::RootDir {
        return None;
    }
    let mut path = format!("{}:\\", char::from(letter));
    let rest: Vec<String> = components
        .map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Option<_>>()?;
    path.push_str(&rest.join("\\"));
    Some(OsStr::new(&path).encode_wide().collect())
}

fn reparse_buffer(substitute: &[u16], print: &[u16]) -> Option<Vec<u8>> {
    let substitute_bytes = substitute.len() * 2;
    let print_bytes = print.len() * 2;
    let data_length = MOUNT_POINT_HEADER_BYTES + substitute_bytes + 2 + print_bytes + 2;
    if REPARSE_HEADER_BYTES + data_length > MAX_REPARSE_BYTES {
        return None;
    }
    let mut buffer = Vec::with_capacity(REPARSE_HEADER_BYTES + data_length);
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(data_length).ok()?.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&0u16.to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(substitute_bytes).ok()?.to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(substitute_bytes + 2).ok()?.to_le_bytes());
    buffer.extend_from_slice(&u16::try_from(print_bytes).ok()?.to_le_bytes());
    for unit in substitute.iter().chain(&[0]).chain(print).chain(&[0]) {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
    Some(buffer)
}

#[allow(unsafe_code)]
fn set_reparse_point(link: &Path, buffer: &[u8]) -> io::Result<()> {
    let wide: Vec<u16> = link.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; null security attributes and
    // template handle are permitted.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_WRITE,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let mut returned = 0u32;
    let length = u32::try_from(buffer.len()).map_err(|_| io::Error::other("reparse too large"))?;
    // SAFETY: `handle` is a valid open directory handle, `buffer` is a well-formed mount-point
    // reparse buffer of `length` bytes, and no output buffer or overlapped I/O is used.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            length,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    let error = (ok == 0).then(io::Error::last_os_error);
    // SAFETY: `handle` was returned by `CreateFileW` and is closed exactly once.
    unsafe { CloseHandle(handle) };
    error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_and_plain_drive_paths_normalise_to_the_same_print_name() {
        let plain = drive_path(Path::new(r"C:\Users\zoë")).expect("drive path");
        let verbatim = drive_path(Path::new(r"\\?\C:\Users\zoë")).expect("verbatim path");
        assert_eq!(plain, verbatim);
        assert_eq!(String::from_utf16(&plain).unwrap(), r"C:\Users\zoë");
    }

    #[test]
    fn network_and_relative_targets_are_rejected() {
        assert!(drive_path(Path::new(r"\\server\share\dir")).is_none());
        assert!(drive_path(Path::new(r"relative\dir")).is_none());
    }

    #[test]
    fn creates_a_junction_that_lists_the_target_directory() {
        let temp = tempfile::tempdir().expect("temp dir");
        let target = temp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("inside.txt"), b"x").unwrap();
        let link = temp.path().join("junction");
        create(&target, &link).expect("create junction");
        assert!(link.join("inside.txt").exists());
        std::fs::remove_dir(&link).expect("remove only the junction");
        assert!(target.join("inside.txt").exists());
    }
}
