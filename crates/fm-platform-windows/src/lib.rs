//! Windows platform integration (task 0060).
//!
//! Explorer reveal, Recycle Bin, drive listing, opening with the default
//! application, terminal integration, and native menu bar hook point (task
//! 0131). The crate is a workspace member everywhere but compiles to nothing
//! off Windows. Native drag-to-Explorer is provided by the Tauri window host
//! (task 0062).
//!
//! Deliberately unimplemented (capability bits stay unset, per specification
//! §23/§35): shell thumbnails and clipboard file references.

#![cfg(target_os = "windows")]
#![allow(unsafe_code)]

pub mod search;

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fm_platform::{
    FallbackPlatformAdapter, MountedVolume, PlatformAdapter, PlatformCapabilities, PlatformError,
    SystemLocation, SystemLocationKind, VolumeCapacity,
};
use windows_sys::Win32::Foundation::{HWND, MAX_PATH, POINT};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits,
};
use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    FO_COPY, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
    FOLDERID_SendTo, KF_FLAG_DEFAULT, SHFILEINFOW, SHFILEOPSTRUCTW, SHFileOperationW, SHGFI_ICON,
    SHGFI_LARGEICON, SHGetFileInfoW, SHGetKnownFolderPath, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallWindowProcW, CreateMenu, CreatePopupMenu, DefWindowProcW, DestroyIcon,
    DestroyMenu, DrawMenuBar, GWLP_WNDPROC, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW,
    ICONINFO, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL,
    SetMenu, SetWindowLongPtrW, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_COMMAND,
    WNDPROC,
};

type NativeMenuAction = Arc<dyn Fn(String) + Send + Sync>;

struct WindowMenuState {
    previous_proc: WNDPROC,
    actions: HashMap<usize, String>,
    callback: NativeMenuAction,
}

static WINDOW_MENU_STATES: OnceLock<Mutex<HashMap<isize, WindowMenuState>>> = OnceLock::new();

fn window_menu_states() -> &'static Mutex<HashMap<isize, WindowMenuState>> {
    WINDOW_MENU_STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `GetDriveTypeW` return values (`winbase.h`), which `windows-sys` does not
/// re-export.
const DRIVE_UNKNOWN: u32 = 0;
const DRIVE_NO_ROOT_DIR: u32 = 1;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

/// Wrapper for HWND that implements Send + Sync. HWND is a window handle
/// (a pointer value), which is thread-safe to send between threads since
/// it's just an opaque identifier to the OS.
#[derive(Debug, Copy, Clone)]
struct SendSyncHwnd(HWND);

// SAFETY: HWND is a window handle, which is safe to send between threads.
// It's just an opaque identifier, not an actual pointer to thread-local data.
unsafe impl Send for SendSyncHwnd {}
unsafe impl Sync for SendSyncHwnd {}

fn shortcut_label(shortcut: Option<&fm_domain::KeyChord>) -> String {
    let Some(shortcut) = shortcut else {
        return String::new();
    };
    let mut modifiers = Vec::new();
    if shortcut.ctrl || shortcut.meta {
        modifiers.push("Ctrl");
    }
    if shortcut.alt {
        modifiers.push("Alt");
    }
    if shortcut.shift {
        modifiers.push("Shift");
    }
    if modifiers.is_empty() {
        shortcut.key.clone()
    } else {
        format!("{}+{}", modifiers.join("+"), shortcut.key)
    }
}

fn menu_label(title: &str, shortcut: Option<&fm_domain::KeyChord>) -> Vec<u16> {
    let shortcut = shortcut_label(shortcut);
    let label = if shortcut.is_empty() {
        title.to_owned()
    } else {
        format!("{title}\t{shortcut}")
    };
    label.encode_utf16().chain(Some(0)).collect()
}

fn role_label(role: fm_domain::NativeMenuRole) -> &'static str {
    use fm_domain::NativeMenuRole;
    match role {
        NativeMenuRole::About => "About",
        NativeMenuRole::Services => "Services",
        NativeMenuRole::HideApp => "Hide",
        NativeMenuRole::HideOthers => "Hide Others",
        NativeMenuRole::ShowAll => "Show All",
        NativeMenuRole::Quit => "Exit",
        NativeMenuRole::Minimize => "Minimize",
        NativeMenuRole::Zoom => "Maximize",
        NativeMenuRole::BringAllToFront => "Bring All to Front",
        NativeMenuRole::Copy => "Copy",
        NativeMenuRole::Paste => "Paste",
        NativeMenuRole::SelectAll => "Select All",
    }
}

fn append_menu_items(
    menu: isize,
    items: &[fm_domain::NativeMenuItem],
    actions: &mut HashMap<usize, String>,
    next_id: &mut usize,
) -> Result<(), PlatformError> {
    for item in items {
        match item {
            fm_domain::NativeMenuItem::Separator => {
                let ok = unsafe { AppendMenuW(menu as _, MF_SEPARATOR, 0, std::ptr::null()) };
                if ok == 0 {
                    return Err(PlatformError::Io {
                        message: "failed to append native menu separator".to_owned(),
                    });
                }
            }
            fm_domain::NativeMenuItem::Action {
                id,
                title,
                shortcut,
                enabled,
                checked,
            } => {
                let command_id = *next_id;
                *next_id = next_id.saturating_add(1);
                actions.insert(command_id, id.clone());
                let mut flags = MF_STRING;
                if !enabled {
                    flags |= MF_DISABLED | MF_GRAYED;
                }
                if *checked {
                    flags |= MF_CHECKED;
                }
                let label = menu_label(title, shortcut.as_ref());
                let ok = unsafe { AppendMenuW(menu as _, flags, command_id, label.as_ptr()) };
                if ok == 0 {
                    return Err(PlatformError::Io {
                        message: "failed to append native menu action".to_owned(),
                    });
                }
            }
            fm_domain::NativeMenuItem::Submenu { title, items } => {
                let submenu = unsafe { CreatePopupMenu() };
                if submenu.is_null() {
                    return Err(PlatformError::Io {
                        message: "failed to create native submenu".to_owned(),
                    });
                }
                if let Err(error) = append_menu_items(submenu as isize, items, actions, next_id) {
                    unsafe { DestroyMenu(submenu) };
                    return Err(error);
                }
                let label = menu_label(title, None);
                let ok =
                    unsafe { AppendMenuW(menu as _, MF_POPUP, submenu as usize, label.as_ptr()) };
                if ok == 0 {
                    unsafe { DestroyMenu(submenu) };
                    return Err(PlatformError::Io {
                        message: "failed to append native submenu".to_owned(),
                    });
                }
            }
            fm_domain::NativeMenuItem::Role { role } => {
                let label = menu_label(role_label(*role), None);
                let ok = unsafe { AppendMenuW(menu as _, MF_STRING, 0, label.as_ptr()) };
                if ok == 0 {
                    return Err(PlatformError::Io {
                        message: "failed to append native menu role".to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}

unsafe extern "system" fn native_menu_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == WM_COMMAND && lparam == 0 {
        let command_id = wparam & 0xffff;
        let callback_and_action = window_menu_states()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(hwnd as isize))
            .and_then(|state| {
                state
                    .actions
                    .get(&command_id)
                    .cloned()
                    .map(|action| (Arc::clone(&state.callback), action))
            });
        if let Some((callback, action)) = callback_and_action {
            callback(action);
            return 0;
        }
    }

    let previous_proc = window_menu_states()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(hwnd as isize))
        .and_then(|state| state.previous_proc);
    previous_proc.map_or_else(
        || unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        |proc| unsafe { CallWindowProcW(Some(proc), hwnd, message, wparam, lparam) },
    )
}

/// Encodes a path as a NUL-terminated wide string for the Win32 API.
fn wide(value: &Path) -> Vec<u16> {
    value.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Strips the extended-length (`\\?\`) prefix the domain layer adds to long
/// paths: Explorer and `ShellExecute` reject that form even though the
/// filesystem APIs require it (specification §23).
fn shell_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SendToEntry {
    label: String,
    path: PathBuf,
    is_directory: bool,
}

fn discover_send_to_entries(directory: &Path) -> Result<Vec<SendToEntry>, PlatformError> {
    let candidates = std::fs::read_dir(directory).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            PlatformError::NotFound {
                path: directory.display().to_string(),
            }
        } else {
            PlatformError::Io {
                message: "could not enumerate the current user's Send To folder".to_owned(),
            }
        }
    })?;
    let mut entries = Vec::new();
    for candidate in candidates {
        let candidate = candidate.map_err(|_| PlatformError::Io {
            message: "could not read an entry in the current user's Send To folder".to_owned(),
        })?;
        let file_name = candidate.file_name();
        if file_name
            .to_string_lossy()
            .eq_ignore_ascii_case("desktop.ini")
        {
            continue;
        }
        let metadata = candidate.metadata().map_err(|_| PlatformError::Io {
            message: "could not read Send To entry metadata".to_owned(),
        })?;
        let attributes = metadata.file_attributes();
        if attributes
            & (windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN
                | windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_SYSTEM)
            != 0
        {
            continue;
        }
        let is_directory = metadata.is_dir();
        let label = if is_directory {
            file_name.to_string_lossy().into_owned()
        } else {
            Path::new(&file_name)
                .file_stem()
                .unwrap_or(file_name.as_os_str())
                .to_string_lossy()
                .into_owned()
        };
        if !label.is_empty() {
            entries.push(SendToEntry {
                label,
                path: candidate.path(),
                is_directory,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.label
            .to_lowercase()
            .cmp(&right.label.to_lowercase())
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

fn current_send_to_directory() -> Result<PathBuf, PlatformError> {
    let mut raw_path = std::ptr::null_mut();
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_SendTo,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut raw_path,
        )
    };
    if result < 0 {
        if !raw_path.is_null() {
            unsafe { CoTaskMemFree(raw_path.cast()) };
        }
        return Err(PlatformError::Io {
            message: "the current user's Send To folder is unavailable".to_owned(),
        });
    }
    if raw_path.is_null() {
        return Err(PlatformError::Io {
            message: "the current user's Send To folder is unavailable".to_owned(),
        });
    }

    let path = unsafe {
        let mut length = 0;
        while *raw_path.add(length) != 0 {
            length += 1;
        }
        PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(
            raw_path, length,
        )))
    };
    unsafe { CoTaskMemFree(raw_path.cast()) };
    Ok(path)
}

fn send_to_arguments(paths: &[PathBuf]) -> Vec<u16> {
    let mut arguments = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        if index > 0 {
            arguments.push(u16::from(b' '));
        }
        arguments.push(u16::from(b'"'));
        let mut backslashes = 0;
        for code_unit in path.as_os_str().encode_wide() {
            if code_unit == u16::from(b'\\') {
                backslashes += 1;
            } else {
                let copies = if code_unit == u16::from(b'"') {
                    backslashes * 2 + 1
                } else {
                    backslashes
                };
                arguments.extend(std::iter::repeat_n(u16::from(b'\\'), copies));
                arguments.push(code_unit);
                backslashes = 0;
            }
        }
        arguments.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
        arguments.push(u16::from(b'"'));
    }
    arguments
}

fn copy_to_send_to_directory(
    destination: &Path,
    paths: &[PathBuf],
    hwnd: HWND,
) -> Result<(), PlatformError> {
    let mut from = Vec::new();
    for path in paths {
        from.extend(wide(path));
    }
    from.push(0);
    let mut to = wide(destination);
    to.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        hwnd,
        wFunc: FO_COPY,
        pFrom: from.as_ptr(),
        pTo: to.as_ptr(),
        fFlags: 0,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result == 0 {
        Ok(())
    } else {
        Err(PlatformError::Io {
            message: "the Send To destination rejected the selected items".to_owned(),
        })
    }
}

fn execute_send_to_entry(
    entry: &SendToEntry,
    paths: &[PathBuf],
    hwnd: HWND,
) -> Result<(), PlatformError> {
    if entry.is_directory {
        return copy_to_send_to_directory(&entry.path, paths, hwnd);
    }

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let mut arguments = send_to_arguments(paths);
    arguments.push(0);
    let target = wide(&entry.path);
    let result = unsafe {
        ShellExecuteW(
            hwnd,
            verb.as_ptr(),
            target.as_ptr(),
            arguments.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(PlatformError::Io {
            message: "the selected Send To destination could not be opened".to_owned(),
        })
    }
}

/// Shows the current user's native Send To destinations at the pointer.
///
/// This performs modal Win32 menu tracking and must be called from the Windows
/// UI thread. A selected shortcut or registered SendTo handler receives every
/// selected path as a separately quoted argument; a directory receives a
/// shell copy operation, matching Explorer's direct-folder behavior.
pub fn show_send_to_menu(paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::Io {
            message: "Send To requires at least one selected item".to_owned(),
        });
    }
    let paths = paths
        .iter()
        .map(|path| require_existing(path))
        .collect::<Result<Vec<_>, _>>()?;
    let entries = discover_send_to_entries(&current_send_to_directory()?)?;
    if entries.is_empty() {
        return Ok(());
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return Err(PlatformError::Io {
            message: "Send To requires a foreground window".to_owned(),
        });
    }
    let mut pointer = POINT::default();
    if unsafe { GetCursorPos(&mut pointer) } == 0 {
        return Err(PlatformError::Io {
            message: "could not locate the pointer for the Send To menu".to_owned(),
        });
    }

    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return Err(PlatformError::Io {
            message: "failed to create the Send To menu".to_owned(),
        });
    }
    let selected = (|| {
        for (index, entry) in entries.iter().enumerate() {
            let command_id =
                u16::try_from(index.checked_add(1).ok_or_else(|| PlatformError::Io {
                    message: "the Send To folder contains too many entries".to_owned(),
                })?)
                .map_err(|_| PlatformError::Io {
                    message: "the Send To folder contains too many entries".to_owned(),
                })?;
            let label: Vec<u16> = entry
                .label
                .replace('&', "&&")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            if unsafe { AppendMenuW(menu, MF_STRING, usize::from(command_id), label.as_ptr()) } == 0
            {
                return Err(PlatformError::Io {
                    message: "failed to populate the Send To menu".to_owned(),
                });
            }
        }
        Ok(unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pointer.x,
                pointer.y,
                0,
                hwnd,
                std::ptr::null(),
            )
        })
    })();
    unsafe { DestroyMenu(menu) };

    let command_id = selected?;
    if command_id == 0 {
        return Ok(());
    }
    let Some(entry) = usize::try_from(command_id)
        .ok()
        .and_then(|command_id| command_id.checked_sub(1))
        .and_then(|index| entries.get(index))
    else {
        return Err(PlatformError::Io {
            message: "the Send To menu returned an unknown destination".to_owned(),
        });
    };
    execute_send_to_entry(entry, &paths, hwnd)
}

/// A generic label for a mountable drive type, or `None` for a letter that is
/// not currently mounted.
fn volume_label_kind(drive_type: u32) -> Option<&'static str> {
    match drive_type {
        DRIVE_REMOVABLE => Some("Removable Disk"),
        DRIVE_FIXED => Some("Local Disk"),
        DRIVE_REMOTE => Some("Network Drive"),
        DRIVE_CDROM => Some("CD Drive"),
        DRIVE_RAMDISK => Some("RAM Disk"),
        DRIVE_UNKNOWN | DRIVE_NO_ROOT_DIR => None,
        _ => None,
    }
}

/// Drive letters currently backed by a mounted volume, as `("C", root)` pairs.
fn mounted_drive_letters() -> Vec<(char, PathBuf, u32)> {
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for index in 0..26_u32 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let letter = char::from_u32(u32::from(b'A') + index).expect("drive letter");
        let root = PathBuf::from(format!("{letter}:\\"));
        let drive_type = unsafe { GetDriveTypeW(wide(&root).as_ptr()) };
        drives.push((letter, root, drive_type));
    }
    drives
}

/// The volume's own label, when the OS reports one.
fn volume_label(root: &Path) -> Option<String> {
    let mut label = [0_u16; MAX_PATH as usize + 1];
    let succeeded = unsafe {
        GetVolumeInformationW(
            wide(root).as_ptr(),
            label.as_mut_ptr(),
            u32::try_from(label.len()).expect("fixed buffer length fits u32"),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if succeeded == 0 {
        return None;
    }
    let end = label.iter().position(|value| *value == 0)?;
    let text = OsString::from_wide(&label[..end])
        .to_string_lossy()
        .into_owned();
    (!text.is_empty()).then_some(text)
}

/// Rejects a missing path before a native call, so callers get a typed
/// `NotFound` instead of an opaque shell failure.
fn require_existing(path: &Path) -> Result<PathBuf, PlatformError> {
    if path.exists() {
        Ok(shell_path(path))
    } else {
        Err(PlatformError::NotFound {
            path: path.to_string_lossy().into_owned(),
        })
    }
}

fn icon_cache_key(path: &Path) -> String {
    if path.is_dir() {
        return "\0dir".to_owned();
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => extension.to_ascii_lowercase(),
        None => "\0noext".to_owned(),
    }
}

fn png_chunk(name: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::with_capacity(12 + data.len());
    chunk
        .extend_from_slice(&(u32::try_from(data.len()).expect("PNG chunk fits u32")).to_be_bytes());
    chunk.extend_from_slice(name);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
    chunk
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut sum_a, mut sum_b) = (1_u32, 0_u32);
    for byte in bytes {
        sum_a = (sum_a + u32::from(*byte)) % 65_521;
        sum_b = (sum_b + sum_a) % 65_521;
    }
    (sum_b << 16) | sum_a
}

fn encode_png(width: i32, height: i32, bgra: &[u8]) -> Result<Vec<u8>, PlatformError> {
    if width <= 0 || height <= 0 {
        return Err(PlatformError::Io {
            message: "Windows returned an icon with invalid dimensions".to_owned(),
        });
    }
    let width = usize::try_from(width).expect("positive width fits usize");
    let height = usize::try_from(height).expect("positive height fits usize");
    let row_bytes = width.checked_mul(4).ok_or_else(|| PlatformError::Io {
        message: "Windows icon dimensions overflowed".to_owned(),
    })?;
    if bgra.len() != row_bytes * height {
        return Err(PlatformError::Io {
            message: "Windows returned incomplete icon pixels".to_owned(),
        });
    }

    let mut scanlines = Vec::with_capacity((row_bytes + 1) * height);
    for row in bgra.chunks_exact(row_bytes) {
        scanlines.push(0);
        for pixel in row.chunks_exact(4) {
            scanlines.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    let mut compressed = vec![0x78, 0x01];
    let blocks = scanlines.chunks(65_535).collect::<Vec<_>>();
    for (index, block) in blocks.iter().enumerate() {
        let final_block = index + 1 == blocks.len();
        compressed.push(if final_block { 1 } else { 0 });
        let length = u16::try_from(block.len()).expect("deflate block fits u16");
        compressed.extend_from_slice(&length.to_le_bytes());
        compressed.extend_from_slice(&(!length).to_le_bytes());
        compressed.extend_from_slice(block);
    }
    compressed.extend_from_slice(&adler32(&scanlines).to_be_bytes());

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&u32::try_from(width).expect("width fits u32").to_be_bytes());
    header.extend_from_slice(
        &u32::try_from(height)
            .expect("height fits u32")
            .to_be_bytes(),
    );
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend_from_slice(&png_chunk(b"IHDR", &header));
    png.extend_from_slice(&png_chunk(b"IDAT", &compressed));
    png.extend_from_slice(&png_chunk(b"IEND", &[]));
    Ok(png)
}

fn fetch_icon_png(path: &Path) -> Result<Vec<u8>, PlatformError> {
    let target = require_existing(path)?;
    let mut info = unsafe { std::mem::zeroed::<SHFILEINFOW>() };
    let result = unsafe {
        SHGetFileInfoW(
            wide(&target).as_ptr(),
            0,
            &mut info,
            u32::try_from(std::mem::size_of::<SHFILEINFOW>()).expect("SHFILEINFO size fits u32"),
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if result == 0 || info.hIcon.is_null() {
        return Err(PlatformError::Io {
            message: "Windows could not extract a shell icon".to_owned(),
        });
    }

    let mut icon_info = unsafe { std::mem::zeroed::<ICONINFO>() };
    let pixels = (|| {
        let succeeded = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetIconInfo(info.hIcon, &mut icon_info)
        };
        if succeeded == 0 || icon_info.hbmColor.is_null() {
            return Err(PlatformError::Io {
                message: "Windows returned an icon without a color bitmap".to_owned(),
            });
        }
        let mut header = unsafe { std::mem::zeroed::<BITMAPINFOHEADER>() };
        header.biSize =
            u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).expect("header size fits u32");
        header.biWidth = 0;
        header.biHeight = 0;
        let mut bitmap = unsafe { std::mem::zeroed::<BITMAPINFO>() };
        bitmap.bmiHeader = header;
        let dimensions = unsafe {
            windows_sys::Win32::Graphics::Gdi::GetObjectW(
                icon_info.hbmColor,
                std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAP>() as i32,
                std::ptr::null_mut(),
            )
        };
        if dimensions == 0 {
            return Err(PlatformError::Io {
                message: "Windows returned an icon without bitmap dimensions".to_owned(),
            });
        }
        let mut native_bitmap =
            unsafe { std::mem::zeroed::<windows_sys::Win32::Graphics::Gdi::BITMAP>() };
        unsafe {
            windows_sys::Win32::Graphics::Gdi::GetObjectW(
                icon_info.hbmColor,
                std::mem::size_of_val(&native_bitmap) as i32,
                std::ptr::addr_of_mut!(native_bitmap).cast(),
            )
        };
        let width = native_bitmap.bmWidth;
        let height = native_bitmap.bmHeight;
        bitmap.bmiHeader.biWidth = width;
        bitmap.bmiHeader.biHeight = -height;
        bitmap.bmiHeader.biPlanes = 1;
        bitmap.bmiHeader.biBitCount = 32;
        bitmap.bmiHeader.biCompression = BI_RGB;
        let mut bgra =
            vec![
                0_u8;
                usize::try_from(width).unwrap_or(0) * usize::try_from(height).unwrap_or(0) * 4
            ];
        let device_context = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        if device_context.is_null() {
            return Err(PlatformError::Io {
                message: "Windows could not create an icon device context".to_owned(),
            });
        }
        let rows = unsafe {
            GetDIBits(
                device_context,
                icon_info.hbmColor,
                0,
                u32::try_from(height).unwrap_or(0),
                bgra.as_mut_ptr().cast(),
                &mut bitmap,
                DIB_RGB_COLORS,
            )
        };
        unsafe {
            DeleteDC(device_context);
        }
        if rows == 0 {
            return Err(PlatformError::Io {
                message: "Windows could not read the shell icon pixels".to_owned(),
            });
        }
        encode_png(width, height, &bgra)
    })();
    unsafe {
        if !icon_info.hbmColor.is_null() {
            DeleteObject(icon_info.hbmColor.cast());
        }
        if !icon_info.hbmMask.is_null() {
            DeleteObject(icon_info.hbmMask.cast());
        }
        DestroyIcon(info.hIcon);
    }
    pixels
}

fn mapped_network_locations() -> Vec<SystemLocation> {
    let mut locations = Vec::new();
    for (letter, root, drive_type) in mounted_drive_letters() {
        if drive_type != DRIVE_REMOTE {
            continue;
        }
        let local = format!("{letter}:");
        let local_wide: Vec<u16> = local.encode_utf16().chain(Some(0)).collect();
        let mut remote = vec![0_u16; 32_768];
        let mut length = u32::try_from(remote.len()).expect("fixed buffer length fits u32");
        if unsafe { WNetGetConnectionW(local_wide.as_ptr(), remote.as_mut_ptr(), &mut length) } != 0
        {
            continue;
        }
        let end = remote
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(remote.len());
        let source = OsString::from_wide(&remote[..end])
            .to_string_lossy()
            .into_owned();
        let mut components = source.trim_start_matches('\\').split('\\');
        let server = components
            .next()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let share = components
            .next()
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        locations.push(SystemLocation {
            name: share.clone().unwrap_or(local),
            path: root,
            kind: SystemLocationKind::Network,
            provider_hint: None,
            protocol: Some("smb".to_owned()),
            server,
            share,
            read_only: None,
        });
    }
    locations
}

/// Paints a window's caption bar and title text, so the OS-drawn title bar can match the
/// application chrome instead of the system light/dark chrome colour.
///
/// `hwnd` is the raw window handle owned by the window host; colours are `COLORREF` values
/// (`0x00bbggrr`). Pre-22H2 Windows rejects the attributes, leaving the OS-themed caption in
/// place, which is why failures are ignored.
pub fn set_caption_colours(hwnd: isize, background: u32, foreground: u32) {
    let handle = hwnd as HWND;
    for (attribute, value) in [
        (DWMWA_CAPTION_COLOR, background),
        (DWMWA_TEXT_COLOR, foreground),
    ] {
        let size = u32::try_from(size_of::<u32>()).expect("u32 size fits u32");
        unsafe {
            DwmSetWindowAttribute(
                handle,
                attribute as u32,
                std::ptr::from_ref(&value).cast(),
                size,
            )
        };
    }
}

/// Windows implementation of [`PlatformAdapter`].
///
/// Native drag-to-Explorer is provided by the Tauri window host; native
/// menus are created from an HWND set by the desktop host after window
/// creation (task 0131). Icons, thumbnails, and clipboard file references
/// stay delegated to [`FallbackPlatformAdapter`] and their capability bits
/// stay unset.
#[derive(Debug, Default)]
pub struct WindowsPlatformAdapter {
    fallback: FallbackPlatformAdapter,
    icon_cache: Mutex<HashMap<String, Vec<u8>>>,
    /// Window handle for native menu bar installation (task 0131). Set by
    /// [`Self::set_window_handle`] after the Tauri window is created;
    /// `install_native_menu` uses it to attach the menu to the app window.
    window_handle: Mutex<Option<SendSyncHwnd>>,
}

impl WindowsPlatformAdapter {
    /// Builds a new Windows adapter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fallback: FallbackPlatformAdapter,
            icon_cache: Mutex::default(),
            window_handle: Mutex::default(),
        }
    }

    /// Sets the Tauri window handle for native menu bar installation
    /// (task 0131). Called by the desktop host (`fm-desktop`) after the
    /// window is created and ready to receive a menu bar.
    pub fn set_window_handle(&self, hwnd: HWND) {
        *self
            .window_handle
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(SendSyncHwnd(hwnd));
    }

    fn cached_file_icon<F>(&self, path: &Path, fetch: F) -> Result<Vec<u8>, PlatformError>
    where
        F: FnOnce(&Path) -> Result<Vec<u8>, PlatformError>,
    {
        let key = icon_cache_key(path);
        if let Some(icon) = self.icon_cache.lock().expect("icon cache lock").get(&key) {
            return Ok(icon.clone());
        }
        let png = fetch(path)?;
        self.icon_cache
            .lock()
            .expect("icon cache lock")
            .insert(key, png.clone());
        Ok(png)
    }
}

impl PlatformAdapter for WindowsPlatformAdapter {
    fn system_locations(&self) -> Result<Vec<SystemLocation>, PlatformError> {
        let candidates = [
            ("OneDrive", "onedrive"),
            ("OneDriveConsumer", "onedrive"),
            ("OneDriveCommercial", "onedrive"),
        ];
        let mut locations = Vec::new();
        for (variable, hint) in candidates {
            let Some(path) = std::env::var_os(variable).map(PathBuf::from) else {
                continue;
            };
            if !path.is_dir()
                || locations
                    .iter()
                    .any(|location: &SystemLocation| location.path == path)
            {
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "OneDrive".to_owned());
            locations.push(SystemLocation {
                name,
                path,
                kind: SystemLocationKind::Cloud,
                provider_hint: Some(hint.to_owned()),
                protocol: None,
                server: None,
                share: None,
                read_only: None,
            });
        }
        locations.extend(mapped_network_locations());
        Ok(locations)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::NATIVE_DRAG_OUT
            | PlatformCapabilities::REVEAL_IN_FILE_MANAGER
            | PlatformCapabilities::TRASH
            | PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
            | PlatformCapabilities::OPEN_TERMINAL
            | PlatformCapabilities::MOUNTED_VOLUMES
            | PlatformCapabilities::VOLUME_CAPACITY
            | PlatformCapabilities::FILE_ICONS
            | PlatformCapabilities::NATIVE_MENUS
            | PlatformCapabilities::PLATFORM_CONTEXT_MENU
    }

    fn file_icon(&self, path: &Path) -> Result<Vec<u8>, PlatformError> {
        self.cached_file_icon(path, fetch_icon_png)
    }

    fn thumbnail(&self, path: &Path, max_size: u32) -> Result<Vec<u8>, PlatformError> {
        self.fallback.thumbnail(path, max_size)
    }

    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError> {
        let target = require_existing(path)?;
        // `/select,` needs the argument unquoted and attached, so it is passed
        // as one raw argument rather than through `Command::arg` quoting.
        let status = std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", target.display()))
            .status()
            .map_err(|error| PlatformError::Io {
                message: format!("failed to launch Explorer: {error}"),
            })?;
        // Explorer reports success as exit code 1 when it reuses an existing
        // window, so the status is deliberately not treated as a failure.
        let _ = status;
        Ok(())
    }

    fn trash(&self, path: &Path) -> Result<(), PlatformError> {
        let target = require_existing(path)?;
        // SHFileOperation takes a double-NUL-terminated list of paths.
        let mut from: Vec<u16> = target.as_os_str().encode_wide().collect();
        from.push(0);
        from.push(0);
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI) as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result == 0 && operation.fAnyOperationsAborted == 0 {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: "the Recycle Bin rejected this item".to_owned(),
            })
        }
    }

    fn open_with_default_application(&self, path: &Path) -> Result<(), PlatformError> {
        let target = require_existing(path)?;
        // ShellExecute resolves `.lnk` files as Windows Shell links. Keeping
        // that resolution here means directory listing never follows targets.
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                wide(&target).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW returns a value greater than 32 on success.
        if result as isize > 32 {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: "no default application is associated with this file".to_owned(),
            })
        }
    }

    fn open_terminal(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        let directory = require_existing(path)?;
        let candidates: Vec<&str> = match command_override {
            Some(command) => vec![command],
            None => vec!["wt.exe", "powershell.exe"],
        };
        let mut last_error = None;
        for command in candidates {
            match std::process::Command::new(command)
                .current_dir(&directory)
                .spawn()
            {
                Ok(_) => return Ok(()),
                Err(error) => last_error = Some(format!("failed to launch {command}: {error}")),
            }
        }
        Err(PlatformError::Io {
            message: last_error.unwrap_or_else(|| "no terminal application was found".to_owned()),
        })
    }

    fn open_in_text_editor(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        match command_override {
            None => self.open_with_default_application(path),
            Some(command) => {
                let target = require_existing(path)?;
                std::process::Command::new(command)
                    .arg(&target)
                    .spawn()
                    .map(|_| ())
                    .map_err(|error| PlatformError::Io {
                        message: format!("failed to launch {command}: {error}"),
                    })
            }
        }
    }

    fn open_with_chooser(&self, path: &Path) -> Result<(), PlatformError> {
        let target = require_existing(path)?;
        let verb: Vec<u16> = "openas\0".encode_utf16().collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                wide(&target).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize > 32 {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: "the Open With dialog could not be shown".to_owned(),
            })
        }
    }

    fn read_clipboard_file_references(&self) -> Result<Vec<PathBuf>, PlatformError> {
        self.fallback.read_clipboard_file_references()
    }

    fn write_clipboard_file_references(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        self.fallback.write_clipboard_file_references(paths)
    }

    fn mounted_volumes(&self) -> Result<Vec<MountedVolume>, PlatformError> {
        let mut volumes = Vec::new();
        for (letter, root, drive_type) in mounted_drive_letters() {
            let Some(generic) = volume_label_kind(drive_type) else {
                continue;
            };
            // A removable or optical drive with no medium reports no volume
            // information; it still belongs in the list under its generic name.
            let label = volume_label(&root).unwrap_or_else(|| generic.to_owned());
            volumes.push(MountedVolume {
                name: format!("{label} ({letter}:)"),
                mount_point: root,
            });
        }
        Ok(volumes)
    }

    fn volume_capacity(&self, path: &Path) -> Result<VolumeCapacity, PlatformError> {
        let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut available_bytes: u64 = 0;
        let mut total_bytes: u64 = 0;
        let succeeded = unsafe {
            GetDiskFreeSpaceExW(
                wide_path.as_ptr(),
                &raw mut available_bytes,
                &raw mut total_bytes,
                std::ptr::null_mut(),
            )
        };
        if succeeded == 0 {
            return Err(PlatformError::NotFound {
                path: path.display().to_string(),
            });
        }
        Ok(VolumeCapacity {
            total_bytes,
            available_bytes,
        })
    }

    fn install_native_menu(
        &self,
        spec: &fm_domain::NativeMenuSpec,
        on_action: std::sync::Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), PlatformError> {
        let SendSyncHwnd(hwnd) = self
            .window_handle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .ok_or(PlatformError::Io {
                message: "native menu bar requires the window handle to be set first".to_owned(),
            })?;

        // SAFETY: CreateMenu is a safe Win32 call; returns NULL on failure.
        let menu = unsafe { CreateMenu() };
        if menu.is_null() {
            return Err(PlatformError::Io {
                message: "failed to create native menu bar (CreateMenu returned NULL)".to_owned(),
            });
        }

        let mut actions = HashMap::new();
        let mut next_id = 0x4000_usize;
        for top_level in &spec.menus {
            let submenu = unsafe { CreatePopupMenu() };
            if submenu.is_null() {
                unsafe { DestroyMenu(menu) };
                return Err(PlatformError::Io {
                    message: "failed to create native top-level submenu".to_owned(),
                });
            }
            if let Err(error) = append_menu_items(
                submenu as isize,
                &top_level.items,
                &mut actions,
                &mut next_id,
            ) {
                unsafe {
                    DestroyMenu(submenu);
                    DestroyMenu(menu);
                }
                return Err(error);
            }
            let label = menu_label(&top_level.title, None);
            let ok = unsafe { AppendMenuW(menu, MF_POPUP, submenu as usize, label.as_ptr()) };
            if ok == 0 {
                unsafe {
                    DestroyMenu(submenu);
                    DestroyMenu(menu);
                }
                return Err(PlatformError::Io {
                    message: "failed to append native top-level menu".to_owned(),
                });
            }
        }

        // Rebuilds are expected as action availability and workspace state change. Once this
        // window has been subclassed, `GetWindowLongPtrW` returns our own procedure; retaining
        // that value as the previous procedure would make every later message recurse forever.
        let previous_proc = window_menu_states()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(hwnd as isize))
            .and_then(|state| state.previous_proc)
            .or_else(|| {
                let current = unsafe { GetWindowLongPtrW(hwnd, GWLP_WNDPROC) };
                if current == 0 {
                    None
                } else {
                    Some(unsafe {
                        std::mem::transmute::<
                            isize,
                            unsafe extern "system" fn(HWND, u32, usize, isize) -> isize,
                        >(current)
                    })
                }
            });
        let Some(previous_proc) = previous_proc else {
            unsafe { DestroyMenu(menu) };
            return Err(PlatformError::Io {
                message: "failed to read the window procedure for native menu routing".to_owned(),
            });
        };
        let replaced = unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_WNDPROC,
                native_menu_window_proc as *const () as usize as isize,
            )
        };
        if replaced == 0 {
            unsafe { DestroyMenu(menu) };
            return Err(PlatformError::Io {
                message: "failed to install the native menu command handler".to_owned(),
            });
        }

        window_menu_states()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                hwnd as isize,
                WindowMenuState {
                    previous_proc: Some(previous_proc),
                    actions,
                    callback: on_action,
                },
            );

        // SAFETY: SetMenu attaches the menu to the window. Both HWND and HMENU
        // are valid at this point (hwnd came from `set_window_handle`, menu was
        // just created successfully).
        let success = unsafe { SetMenu(hwnd, menu) };
        if success == 0 {
            // SAFETY: menu was created successfully, so destroying it is safe.
            unsafe { DestroyMenu(menu) };
            window_menu_states()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&(hwnd as isize));
            unsafe {
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_WNDPROC,
                    previous_proc as *const () as usize as isize,
                );
            }
            return Err(PlatformError::Io {
                message: "failed to attach native menu bar to window (SetMenu failed)".to_owned(),
            });
        }

        unsafe { DrawMenuBar(hwnd) };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use super::*;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize, IPersistFile,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{HSTRING, Interface};

    struct ComApartment;

    impl ComApartment {
        fn initialize() -> Self {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
                .ok()
                .expect("initialize COM for shortcut fixture");
            Self
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    fn create_shortcut(shortcut: &Path, target: &Path, arguments: &str) {
        let _apartment = ComApartment::initialize();
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
            .expect("create ShellLink");
        unsafe {
            link.SetPath(&HSTRING::from(target.as_os_str()))
                .expect("set shortcut target");
            link.SetArguments(&HSTRING::from(arguments))
                .expect("set shortcut arguments");
        }
        let persist: IPersistFile = link.cast().expect("query IPersistFile");
        unsafe {
            persist
                .Save(&HSTRING::from(shortcut.as_os_str()), true)
                .expect("save shortcut");
        }
    }

    #[test]
    fn windows_menu_shortcuts_use_windows_modifier_labels() {
        let shortcut = fm_domain::KeyChord {
            key: "c".to_owned(),
            ctrl: true,
            shift: true,
            ..fm_domain::KeyChord::default()
        };
        assert_eq!(shortcut_label(Some(&shortcut)), "Ctrl+Shift+c");
        assert_eq!(
            String::from_utf16(&menu_label("Copy", Some(&shortcut))[..])
                .expect("menu label is valid UTF-16")
                .trim_end_matches('\0'),
            "Copy\tCtrl+Shift+c"
        );
    }

    #[test]
    fn windows_menu_roles_use_windows_conventions() {
        assert_eq!(role_label(fm_domain::NativeMenuRole::Quit), "Exit");
        assert_eq!(role_label(fm_domain::NativeMenuRole::Zoom), "Maximize");
        assert_eq!(role_label(fm_domain::NativeMenuRole::HideApp), "Hide");
    }

    #[test]
    fn send_to_entries_are_visible_labelled_and_sorted_like_a_menu() {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, SetFileAttributesW,
        };

        let root = tempfile::tempdir().expect("temporary SendTo root");
        let zulu = root.path().join("Zulu.lnk");
        let alpha = root.path().join("alpha.ZFSendToTarget");
        let dotted_folder = root.path().join("Bravo.folder");
        let hidden = root.path().join("Hidden target.lnk");
        let system = root.path().join("System target.lnk");
        std::fs::write(&zulu, b"shortcut").expect("write shortcut fixture");
        std::fs::write(&alpha, b"handler").expect("write handler fixture");
        std::fs::create_dir(&dotted_folder).expect("write directory fixture");
        std::fs::write(root.path().join("DeSkToP.InI"), b"metadata")
            .expect("write desktop metadata fixture");
        std::fs::write(&hidden, b"hidden").expect("write hidden fixture");
        std::fs::write(&system, b"system").expect("write system fixture");
        assert_ne!(
            unsafe { SetFileAttributesW(wide(&hidden).as_ptr(), FILE_ATTRIBUTE_HIDDEN) },
            0
        );
        assert_ne!(
            unsafe { SetFileAttributesW(wide(&system).as_ptr(), FILE_ATTRIBUTE_SYSTEM) },
            0
        );

        let entries = discover_send_to_entries(root.path()).expect("discover SendTo entries");
        let actual: Vec<_> = entries
            .into_iter()
            .map(|entry| (entry.label, entry.path))
            .collect();
        assert_eq!(
            actual,
            vec![
                ("alpha".to_owned(), alpha),
                ("Bravo.folder".to_owned(), dotted_folder),
                ("Zulu".to_owned(), zulu),
            ]
        );
    }

    #[test]
    fn a_missing_send_to_directory_is_reported_as_not_found() {
        let root = tempfile::tempdir().expect("temporary SendTo root");
        let missing = root.path().join("missing");
        assert!(matches!(
            discover_send_to_entries(&missing),
            Err(PlatformError::NotFound { path }) if path == missing.display().to_string()
        ));
    }

    #[test]
    fn send_to_arguments_preserve_spaces_quotes_and_trailing_backslashes() {
        let arguments = send_to_arguments(&[
            PathBuf::from(r"C:\one file.txt"),
            PathBuf::from("C:\\quoted\"name"),
            PathBuf::from(r"C:\folder\"),
        ]);
        assert_eq!(
            String::from_utf16(&arguments).expect("arguments are valid UTF-16"),
            "\"C:\\one file.txt\" \"C:\\quoted\\\"name\" \"C:\\folder\\\\\""
        );
    }

    #[test]
    fn volume_capacity_reports_plausible_totals_for_the_system_drive() {
        let capacity = WindowsPlatformAdapter::new()
            .volume_capacity(&std::env::temp_dir())
            .expect("query system drive capacity");
        assert!(capacity.total_bytes > 0);
        assert!(capacity.available_bytes <= capacity.total_bytes);
    }

    #[test]
    fn volume_capacity_reports_not_found_for_a_missing_drive() {
        let error = WindowsPlatformAdapter::new()
            .volume_capacity(Path::new("Z:\\no-such-fm-platform-windows-test-path"))
            .unwrap_err();
        assert!(matches!(error, PlatformError::NotFound { .. }));
    }

    #[test]
    fn native_icons_are_png_and_capability_is_reported() {
        let adapter = WindowsPlatformAdapter::new();
        let root = tempfile::tempdir().expect("temporary icon root");
        let text = root.path().join("readme.txt");
        std::fs::write(&text, b"icon").expect("write icon fixture");
        let extensionless = root.path().join("README");
        std::fs::write(&extensionless, b"icon").expect("write extensionless fixture");
        let png = adapter.file_icon(&text).expect("text icon");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png.len() > 100);
        assert!(adapter.file_icon(&extensionless).is_ok());
        assert!(adapter.file_icon(root.path()).is_ok());
        assert!(
            adapter
                .capabilities()
                .contains(PlatformCapabilities::FILE_ICONS)
        );
    }

    #[test]
    fn native_icon_cache_is_case_insensitive_by_extension() {
        let adapter = WindowsPlatformAdapter::new();
        let root = tempfile::tempdir().expect("temporary icon root");
        let first = root.path().join("first.TXT");
        let second = root.path().join("second.txt");
        std::fs::write(&first, b"icon").expect("write first fixture");
        std::fs::write(&second, b"icon").expect("write second fixture");
        let fetches = std::sync::atomic::AtomicUsize::new(0);
        let first_icon = adapter
            .cached_file_icon(&first, |_| {
                fetches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(b"test-png".to_vec())
            })
            .expect("first icon");
        let second_icon = adapter
            .cached_file_icon(&second, |_| {
                fetches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(b"unexpected-second-fetch".to_vec())
            })
            .expect("second icon");
        assert_eq!(first_icon, second_icon);
        assert_eq!(fetches.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn unimplemented_integrations_still_delegate_to_the_fallback_adapter() {
        let adapter = WindowsPlatformAdapter::new();
        let fallback = FallbackPlatformAdapter;
        let path = Path::new("C:\\fm-platform-windows-test.txt");
        assert_eq!(
            adapter.thumbnail(path, 64).unwrap_err().to_string(),
            fallback.thumbnail(path, 64).unwrap_err().to_string()
        );
        assert_eq!(
            adapter
                .read_clipboard_file_references()
                .unwrap_err()
                .to_string(),
            fallback
                .read_clipboard_file_references()
                .unwrap_err()
                .to_string()
        );
        assert_eq!(
            adapter
                .write_clipboard_file_references(&[path.to_path_buf()])
                .unwrap_err()
                .to_string(),
            fallback
                .write_clipboard_file_references(&[path.to_path_buf()])
                .unwrap_err()
                .to_string()
        );
        // Note: native menu installation requires the window handle to be set first
        // (task 0131). The adapter returns PlatformError::Io when HWND is not initialized.
        let spec = fm_domain::NativeMenuSpec::default();
        let adapter_result = adapter
            .install_native_menu(&spec, std::sync::Arc::new(|_id| {}))
            .unwrap_err()
            .to_string();
        assert!(
            adapter_result.contains("window handle") || adapter_result.contains("set first"),
            "Expected 'window handle' or 'set first' in error message, got: {}",
            adapter_result
        );
    }

    #[test]
    fn strips_extended_length_prefixes_before_handing_paths_to_the_shell() {
        assert_eq!(
            shell_path(Path::new(r"\\?\C:\Users\alice\file.txt")),
            PathBuf::from(r"C:\Users\alice\file.txt")
        );
        assert_eq!(
            shell_path(Path::new(r"\\?\UNC\server\share\file.txt")),
            PathBuf::from(r"\\server\share\file.txt")
        );
        assert_eq!(
            shell_path(Path::new(r"C:\plain\path")),
            PathBuf::from(r"C:\plain\path")
        );
    }

    #[test]
    fn every_mountable_drive_type_is_reported_and_unmounted_letters_are_skipped() {
        assert_eq!(volume_label_kind(DRIVE_REMOVABLE), Some("Removable Disk"));
        assert_eq!(volume_label_kind(DRIVE_FIXED), Some("Local Disk"));
        assert_eq!(volume_label_kind(DRIVE_REMOTE), Some("Network Drive"));
        assert_eq!(volume_label_kind(DRIVE_CDROM), Some("CD Drive"));
        assert_eq!(volume_label_kind(DRIVE_RAMDISK), Some("RAM Disk"));
        assert_eq!(volume_label_kind(DRIVE_UNKNOWN), None);
        assert_eq!(volume_label_kind(DRIVE_NO_ROOT_DIR), None);
    }

    #[test]
    fn mounted_volumes_are_reported_and_always_include_the_system_drive() {
        let volumes = WindowsPlatformAdapter::new().mounted_volumes().unwrap();
        assert!(
            volumes
                .iter()
                .any(|volume| volume.mount_point == Path::new(r"C:\")),
            "expected the system drive among {volumes:?}"
        );
        assert!(volumes.iter().all(|volume| !volume.name.is_empty()));
    }

    #[test]
    fn thumbnails_remain_an_explicitly_unsupported_capability() {
        let adapter = WindowsPlatformAdapter::new();
        assert!(
            !adapter
                .capabilities()
                .contains(PlatformCapabilities::THUMBNAILS)
        );
        assert!(matches!(
            adapter.thumbnail(Path::new(r"C:\any.png"), 64),
            Err(PlatformError::Unsupported { .. })
        ));
    }

    #[test]
    fn capabilities_cover_every_natively_implemented_operation() {
        let capabilities = WindowsPlatformAdapter::new().capabilities();
        for expected in [
            PlatformCapabilities::NATIVE_DRAG_OUT,
            PlatformCapabilities::REVEAL_IN_FILE_MANAGER,
            PlatformCapabilities::TRASH,
            PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION,
            PlatformCapabilities::OPEN_TERMINAL,
            PlatformCapabilities::MOUNTED_VOLUMES,
            PlatformCapabilities::PLATFORM_CONTEXT_MENU,
        ] {
            assert!(capabilities.contains(expected), "missing {expected:?}");
        }
    }

    #[test]
    fn finder_tags_and_extended_attributes_are_not_claimed_on_windows() {
        // Task 0136 acceptance criteria: Windows/Linux report these capabilities false rather
        // than half-implementing an equivalent (NTFS alternate data streams are a different
        // enough convention to warrant their own future task, not a shared abstraction here).
        let capabilities = WindowsPlatformAdapter::new().capabilities();
        assert!(!capabilities.contains(PlatformCapabilities::FINDER_TAGS));
        assert!(!capabilities.contains(PlatformCapabilities::EXTENDED_ATTRIBUTES));

        let path = Path::new(r"C:\fm-does-not-exist-0136\nothing.txt");
        let adapter = WindowsPlatformAdapter::new();
        assert!(matches!(
            adapter.finder_tags(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::FINDER_TAGS
            })
        ));
        assert!(matches!(
            adapter.spotlight_comment(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::EXTENDED_ATTRIBUTES
            })
        ));
    }

    #[test]
    fn native_operations_on_a_missing_path_report_not_found() {
        let adapter = WindowsPlatformAdapter::new();
        let missing = Path::new(r"C:\fm-does-not-exist-0060\nothing.txt");
        assert!(matches!(
            adapter.reveal_in_file_manager(missing),
            Err(PlatformError::NotFound { .. })
        ));
        assert!(matches!(
            adapter.trash(missing),
            Err(PlatformError::NotFound { .. })
        ));
        assert!(matches!(
            adapter.open_with_default_application(missing),
            Err(PlatformError::NotFound { .. })
        ));
    }

    #[test]
    fn explicit_open_resolves_and_launches_a_shortcut_target() {
        let root = tempfile::tempdir().expect("temporary shortcut root");
        let script = root.path().join("shortcut-target.cmd");
        let marker = root.path().join("opened.txt");
        let shortcut = root.path().join("target.lnk");
        std::fs::write(&script, "@echo resolved>\"%~1\"\r\n").expect("write shortcut target");
        create_shortcut(&shortcut, &script, &format!("\"{}\"", marker.display()));

        WindowsPlatformAdapter::new()
            .open_with_default_application(&shortcut)
            .expect("open shortcut through the Windows Shell");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut marker_contents = None;
        while Instant::now() < deadline {
            marker_contents = std::fs::read_to_string(&marker)
                .ok()
                .filter(|contents| contents == "resolved\r\n");
            if marker_contents.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(
            marker_contents.as_deref(),
            Some("resolved\r\n"),
            "shortcut target did not finish writing its marker"
        );
    }
}
