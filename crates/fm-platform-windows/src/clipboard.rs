//! System clipboard file references (`CF_HDROP`), so Procyon copies paste in
//! Explorer and Explorer copies paste in Procyon.
//!
//! Written data always carries `Preferred DropEffect = DROPEFFECT_COPY`: a
//! Procyon cut is exposed to Explorer as a plain copy, and move semantics stay
//! inside Procyon's own clipboard so another application can never delete the
//! source files.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use fm_platform::PlatformError;
use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::{CF_HDROP, DROPEFFECT_COPY};
use windows_sys::Win32::UI::Shell::{DROPFILES, DragQueryFileW, HDROP};

/// Size of the `DROPFILES` header that precedes the path list.
const DROPFILES_HEADER_BYTES: usize = std::mem::size_of::<DROPFILES>();

/// Builds a `CF_HDROP` payload: a wide-character `DROPFILES` header followed by
/// NUL-terminated UTF-16 paths and a final NUL.
pub(crate) fn drop_files_payload(paths: &[PathBuf]) -> Result<Vec<u8>, PlatformError> {
    if paths.is_empty() {
        return Err(io("at least one file is required"));
    }
    let mut payload = Vec::with_capacity(DROPFILES_HEADER_BYTES);
    payload.extend_from_slice(&(DROPFILES_HEADER_BYTES as u32).to_le_bytes()); // pFiles
    payload.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    payload.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    payload.extend_from_slice(&0i32.to_le_bytes()); // fNC
    payload.extend_from_slice(&1i32.to_le_bytes()); // fWide
    debug_assert_eq!(payload.len(), DROPFILES_HEADER_BYTES);
    for path in paths {
        let wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if wide.is_empty() || wide.contains(&0) {
            return Err(io("file path is empty or contains a NUL character"));
        }
        for unit in wide.into_iter().chain([0]) {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
    }
    payload.extend_from_slice(&0u16.to_le_bytes());
    Ok(payload)
}

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

/// Whether the attributes mark a cloud placeholder whose content is not on the disk yet
/// (OneDrive "online-only" files and similar).
pub(crate) fn is_cloud_placeholder(attributes: u32) -> bool {
    attributes
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

/// Downloads the content of an online-only file by reading it once. Opening a placeholder makes
/// the cloud sync engine fetch it, so the bytes are on the disk when this returns.
fn hydrate_file(path: &Path) -> Result<(), PlatformError> {
    let mut file = File::open(path).map_err(|error| {
        io(&format!(
            "{} could not be downloaded: {error}",
            path.display()
        ))
    })?;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(error) => {
                return Err(io(&format!(
                    "{} could not be downloaded: {error}",
                    path.display()
                )));
            }
        }
    }
}

/// Makes every online-only file among `paths` (and inside copied folders) available locally.
///
/// Other applications read the clipboard's file paths themselves - often a remote-desktop or
/// virtual-machine bridge that does not trigger a cloud download - and would otherwise receive
/// empty files that still show their size. Explorer avoids this by streaming the content itself.
fn hydrate_placeholders(paths: &[PathBuf]) -> Result<(), PlatformError> {
    for path in paths {
        hydrate_tree(path)?;
    }
    Ok(())
}

fn hydrate_tree(path: &Path) -> Result<(), PlatformError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Ok(());
    }
    if file_type.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            hydrate_tree(&entry.path())?;
        }
        return Ok(());
    }
    if is_cloud_placeholder(metadata.file_attributes()) {
        hydrate_file(path)?;
    }
    Ok(())
}

/// Replaces the clipboard contents with `paths` as a copy-only `CF_HDROP`.
pub(crate) fn write_file_references(owner: HWND, paths: &[PathBuf]) -> Result<(), PlatformError> {
    let payload = drop_files_payload(paths)?;
    // Before the clipboard is opened: the download can take long and must not block other apps.
    hydrate_placeholders(paths)?;
    let drop_effect = DROPEFFECT_COPY.to_le_bytes();
    let preferred_drop_effect = preferred_drop_effect_format()?;
    let _clipboard = OpenedClipboard::open(owner)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err(io("the clipboard could not be emptied"));
    }
    set_clipboard_bytes(u32::from(CF_HDROP), &payload)?;
    set_clipboard_bytes(preferred_drop_effect, &drop_effect)
}

/// Reads the file paths held as `CF_HDROP`, in clipboard order. An empty
/// result means the clipboard holds no file references.
pub(crate) fn read_file_references(owner: HWND) -> Result<Vec<PathBuf>, PlatformError> {
    let _clipboard = OpenedClipboard::open(owner)?;
    let handle = unsafe { GetClipboardData(u32::from(CF_HDROP)) };
    if handle.is_null() {
        return Ok(Vec::new());
    }
    let drop = handle as HDROP;
    let count = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, std::ptr::null_mut(), 0) };
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), length + 1) };
        buffer.truncate(copied as usize);
        let path = PathBuf::from(OsString::from_wide(&buffer));
        if path.is_absolute() {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn preferred_drop_effect_format() -> Result<u32, PlatformError> {
    let name = "Preferred DropEffect\0".encode_utf16().collect::<Vec<_>>();
    match unsafe { RegisterClipboardFormatW(name.as_ptr()) } {
        0 => Err(io(
            "the Preferred DropEffect clipboard format is unavailable",
        )),
        format => Ok(format),
    }
}

/// Copies `bytes` into movable global memory and hands it to the clipboard,
/// which owns the memory once `SetClipboardData` succeeds.
fn set_clipboard_bytes(format: u32, bytes: &[u8]) -> Result<(), PlatformError> {
    let memory: HGLOBAL = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
    if memory.is_null() {
        return Err(io("clipboard memory could not be allocated"));
    }
    let target = unsafe { GlobalLock(memory) };
    if target.is_null() {
        unsafe { GlobalFree(memory) };
        return Err(io("clipboard memory could not be locked"));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        GlobalUnlock(memory);
    }
    if unsafe { SetClipboardData(format, memory) }.is_null() {
        unsafe { GlobalFree(memory) };
        return Err(io("the clipboard rejected the file references"));
    }
    Ok(())
}

/// An open clipboard, closed again on drop. Another process may hold the
/// clipboard briefly, so opening retries for a short while.
struct OpenedClipboard;

impl OpenedClipboard {
    fn open(owner: HWND) -> Result<Self, PlatformError> {
        for _ in 0..10 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Self);
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(io("the clipboard is in use by another application"))
    }
}

impl Drop for OpenedClipboard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn io(message: &str) -> PlatformError {
    PlatformError::Io {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(payload: &[u8]) -> Vec<String> {
        let offset = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
        let wide = u32::from_le_bytes(payload[16..20].try_into().unwrap());
        assert_eq!(wide, 1, "payload must be wide");
        let units = payload[offset..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        assert_eq!(&units[units.len() - 2..], &[0, 0], "double NUL terminator");
        units[..units.len() - 2]
            .split(|unit| *unit == 0)
            .map(String::from_utf16_lossy)
            .collect()
    }

    #[test]
    fn payload_lists_every_path_after_a_wide_header() {
        let payload = drop_files_payload(&[
            PathBuf::from(r"C:\Users\me\Ümlaut report.txt"),
            PathBuf::from(r"D:\Folder"),
        ])
        .expect("payload");

        assert_eq!(DROPFILES_HEADER_BYTES, 20);
        assert_eq!(
            parse(&payload),
            vec![
                r"C:\Users\me\Ümlaut report.txt".to_owned(),
                r"D:\Folder".to_owned()
            ]
        );
    }

    #[test]
    fn recognises_cloud_placeholders_by_their_attributes() {
        assert!(is_cloud_placeholder(
            FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | 0x20
        ));
        assert!(is_cloud_placeholder(FILE_ATTRIBUTE_RECALL_ON_OPEN));
        assert!(is_cloud_placeholder(FILE_ATTRIBUTE_OFFLINE));
        assert!(!is_cloud_placeholder(0x20)); // archive only
        assert!(!is_cloud_placeholder(0x10)); // directory
    }

    #[test]
    fn hydrating_ordinary_files_and_folders_is_a_no_op() {
        let dir = std::env::temp_dir().join(format!("procyon-hydrate-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).expect("dir");
        fs::write(dir.join("sub").join("a.txt"), b"data").expect("file");
        assert!(hydrate_placeholders(&[dir.clone(), dir.join("missing")]).is_ok());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn payload_rejects_an_empty_selection() {
        assert!(drop_files_payload(&[]).is_err());
    }
}
