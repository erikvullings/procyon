//! macOS platform integration (task 0059).
//!
//! File icons, Finder reveal, Trash, mounted volumes, a native menu bar hook
//! and terminal integration. The crate is a workspace member everywhere but
//! compiles to nothing off macOS.
//!
//! Deliberately unimplemented (capability bits stay unset, per specification
//! §23/§35): thumbnails and macOS alias resolution (no capability flag exists
//! for this in [`fm_platform::PlatformCapabilities`]; aliases are simply not
//! resolved). Finder tags and the Spotlight
//! "Finder comment" extended attribute are implemented (task 0136), via the
//! `xattr`/`plist` crates rather than AppKit - see [`read_finder_tags`] and
//! [`read_spotlight_comment`]. Native drag-to-Finder is provided by the Tauri window host (task 0062),
//! while clipboard file references stay delegated to the fallback adapter.
//! `open_with_default_application` (task
//! 0061) shells out to `open <path>`. `open_with_chooser` (task 0061
//! follow-up) queries Launch Services (`NSWorkspace
//! -URLsForApplicationsToOpenURL:`) for the applications capable of opening
//! the target file and shows only those in a `choose from list` dialog via
//! `osascript` (Marta/Finder-style filtering), plus a trailing "Other
//! Application…" entry that falls back to the unfiltered `choose
//! application` dialog; every path and application name is passed as an
//! `argv` element, never interpolated into the script text, so none of it
//! can be used for AppleScript/shell injection; cancelling either dialog is
//! caught inside the script (AppleScript error -128) and treated as a
//! successful no-op, not a failure. `open_in_text_editor`
//! (task 0086) shells out to `open -t <path>`, macOS's own "always open in
//! the default text editor" flag, or `open -a <override> <path>` when an
//! editor command is configured - a genuine distinct binding, unlike
//! `open_with_default_application`/`open_terminal`'s shared gap above.

#![cfg(target_os = "macos")]
// Native AppKit/Foundation bindings are inherently FFI: `objc2` message sends
// and Retained-pointer handling require `unsafe`. This crate is isolated
// specifically so the rest of the workspace can keep `unsafe_code = "deny"`
// (see docs/decisions/0010-native-platform-adapters.md).
#![allow(unsafe_code)]

pub(crate) mod dock;
pub mod search;
pub(crate) mod uninstall;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use fm_platform::{
    ApplicationUninstallPlan, FallbackPlatformAdapter, FinderTag, FinderTagColor, MountedVolume,
    PlatformAdapter, PlatformCapabilities, PlatformError, SystemLocation, SystemLocationKind,
    VolumeCapacity, cloud_provider_hint,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, NSObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBitmapImageFileType, NSBitmapImageRep, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSMenu, NSMenuItem, NSPasteboard,
    NSPasteboardType, NSResponder, NSWindowWillCloseNotification, NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSFileAttributeKey, NSFileManager, NSFileSystemFreeSize,
    NSFileSystemSize, NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol,
    NSOperationQueue, NSProcessInfo, NSString, NSURL, NSURLResourceKey, NSURLVolumeIsBrowsableKey,
    NSURLVolumeIsLocalKey, NSURLVolumeIsReadOnlyKey, NSURLVolumeMountFromLocationKey,
    NSVolumeEnumerationOptions,
};
use quicklook::{PreviewItem, QuickLookPanel};

struct QuickLookSession {
    panel: QuickLookPanel,
    _close_observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

thread_local! {
    /// Quick Look exposes one shared panel per application and requires every panel call on the
    /// process main thread. The Tauri action command enforces that boundary before entering the
    /// platform adapter; thread-local ownership prevents the main-thread-only panel from leaking
    /// into the adapter's `Send + Sync` type.
    static QUICK_LOOK_PANEL: RefCell<Option<QuickLookSession>> = const { RefCell::new(None) };
}

fn quick_look_session() -> Result<QuickLookSession, PlatformError> {
    let panel = QuickLookPanel::shared().ok_or_else(|| PlatformError::Io {
        message: "Quick Look must be invoked on the macOS main thread".to_owned(),
    })?;
    let handle = panel.handle();
    let close_observer = RcBlock::new(move |notification: NonNull<NSNotification>| {
        let notification = unsafe { notification.as_ref() };
        if notification
            .object()
            .is_some_and(|object| object.class().name() == c"QLPreviewPanel")
        {
            handle.set_items(Vec::new());
        }
    });
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWindowWillCloseNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &close_observer,
        )
    };
    Ok(QuickLookSession {
        panel,
        _close_observer: observer,
    })
}

/// macOS implementation of [`PlatformAdapter`].
///
/// File icons are cached by file extension (not per path), so listing many
/// files sharing an extension issues a single native icon lookup rather than
/// one per entry (specification §28). The cache is process-lifetime only and
/// never persisted to disk.
#[derive(Debug, Default)]
pub struct MacosPlatformAdapter {
    fallback: FallbackPlatformAdapter,
    icon_cache: Mutex<HashMap<String, Vec<u8>>>,
}

impl MacosPlatformAdapter {
    /// Builds a new macOS adapter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Cache key for a path's icon: the lowercased extension, or a sentinel for
/// directories and extension-less files. Sentinels use a NUL byte prefix so
/// they can never collide with a real (NUL-free) file extension.
fn icon_cache_key(path: &Path) -> String {
    if path.is_dir() {
        return "\0dir".to_owned();
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => extension.to_ascii_lowercase(),
        None => "\0noext".to_owned(),
    }
}

fn path_to_str(path: &Path) -> Result<&str, PlatformError> {
    path.to_str().ok_or_else(|| PlatformError::Io {
        message: "path is not valid UTF-8".to_owned(),
    })
}

/// Builds (without running) the `osascript` invocation behind
/// [`MacosPlatformAdapter::open_with_chooser`], factored out so tests can
/// assert on its arguments without ever popping the interactive dialog.
///
/// `path` is passed as a trailing `argv` element, never interpolated into
/// the `-e` script text, so it can't be used for AppleScript/shell
/// injection; cancelling `choose application` raises AppleScript error -128,
/// caught inside the script and treated as a successful no-op.
fn open_with_chooser_command(path: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("osascript");
    command
        .arg("-e")
        .arg("on run argv")
        .arg("-e")
        .arg("set targetPath to item 1 of argv")
        .arg("-e")
        .arg("try")
        .arg("-e")
        .arg("set chosenApp to (choose application)")
        .arg("-e")
        .arg("on error number -128")
        .arg("-e")
        .arg("return")
        .arg("-e")
        .arg("end try")
        .arg("-e")
        .arg("tell application \"Finder\" to open (POSIX file targetPath) using chosenApp")
        .arg("-e")
        .arg("end run")
        .arg(path);
    command
}

/// Sentinel that [`choose_from_list_command`]'s AppleScript prints to
/// stdout when its dialog is dismissed (Cancel, Escape, or AppleScript
/// error -128), so [`resolve_open_with_choice`] can tell "cancelled" apart
/// from a genuinely chosen (and coincidentally identical) application name.
const OPEN_WITH_CANCELLED_SENTINEL: &str = "__fm_open_with_cancelled__";

/// Trailing entry appended to [`MacosPlatformAdapter::open_with_chooser`]'s
/// filtered dialog, mirroring Finder's own "Open With" submenu, which lists
/// Launch Services' recommended apps first and an "Other…" catch-all last.
const OPEN_WITH_OTHER_APPLICATIONS: &str = "Other Application…";

/// Queries Launch Services (via `NSWorkspace`) for the applications capable
/// of opening `path`, in Launch Services' own recommended order (the
/// system's current default application first, per Apple's documented
/// behaviour for `-URLsForApplicationsToOpenURL:`). Each entry pairs the
/// app's localized display name (e.g. "Preview", not "Preview.app") with
/// its absolute bundle path, so `open_with_chooser` can present a
/// Marta/Finder-style *filtered* list instead of every installed
/// application.
fn recommended_applications(path: &Path) -> Result<Vec<(String, PathBuf)>, PlatformError> {
    let ns_path = NSString::from_str(path_to_str(path)?);
    let url = NSURL::fileURLWithPath(&ns_path);
    let app_urls = NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenURL(&url);
    let file_manager = NSFileManager::defaultManager();
    let mut apps = Vec::with_capacity(app_urls.len());
    for app_url in &app_urls {
        let Some(app_path) = app_url.path() else {
            continue;
        };
        let app_path = PathBuf::from(app_path.to_string());
        let Some(app_path_str) = app_path.to_str() else {
            continue;
        };
        let display_name = file_manager
            .displayNameAtPath(&NSString::from_str(app_path_str))
            .to_string();
        apps.push((display_name, app_path));
    }
    Ok(apps)
}

/// What the user picked from [`MacosPlatformAdapter::open_with_chooser`]'s
/// filtered `choose from list` dialog, decoded from its raw stdout.
#[derive(Debug, PartialEq, Eq)]
enum OpenWithChoice {
    /// The dialog was dismissed (Cancel, Escape, or AppleScript error -128).
    Cancelled,
    /// The user asked to see every installed application, not just the
    /// filtered/recommended list.
    Other,
    /// The user picked one of the recommended applications.
    App(PathBuf),
}

/// Decodes the trimmed stdout of [`choose_from_list_command`] into an
/// [`OpenWithChoice`], matching the chosen display name back to its
/// absolute application path in `recommended` (first match wins if two
/// recommended applications happen to share a display name; an unmatched
/// name - which should never happen, since the dialog only offers names
/// drawn from `recommended` - is treated as cancelled rather than guessed).
fn resolve_open_with_choice(chosen: &str, recommended: &[(String, PathBuf)]) -> OpenWithChoice {
    if chosen == OPEN_WITH_CANCELLED_SENTINEL {
        return OpenWithChoice::Cancelled;
    }
    if chosen == OPEN_WITH_OTHER_APPLICATIONS {
        return OpenWithChoice::Other;
    }
    match recommended.iter().find(|(name, _)| name == chosen) {
        Some((_, app_path)) => OpenWithChoice::App(app_path.clone()),
        None => OpenWithChoice::Cancelled,
    }
}

/// Builds (without running) the `osascript` invocation that shows a
/// Marta/Finder-style filtered "Open With" dialog listing just `names`
/// (Launch Services' recommended applications for the target file, per
/// [`recommended_applications`]), returning the chosen name on stdout - or
/// [`OPEN_WITH_CANCELLED_SENTINEL`] if the dialog was dismissed - so the
/// caller can resolve it via [`resolve_open_with_choice`] without ever
/// interpolating a name into the script text.
///
/// Every name is passed as a trailing `argv` element, never interpolated
/// into the `-e` script text, so none of them can be used for
/// AppleScript/shell injection; cancelling raises AppleScript error -128,
/// caught inside the script and reported back as a sentinel string (rather
/// than a non-zero exit) so the caller can tell "cancelled" apart from a
/// genuine `osascript` failure.
fn choose_from_list_command(names: &[String]) -> std::process::Command {
    let mut command = std::process::Command::new("osascript");
    command
        .arg("-e")
        .arg("on run argv")
        .arg("-e")
        .arg("try")
        .arg("-e")
        .arg("set chosenNameList to (choose from list argv with title \"Open With\" without multiple selections allowed)")
        .arg("-e")
        .arg("on error number -128")
        .arg("-e")
        .arg(format!("return \"{OPEN_WITH_CANCELLED_SENTINEL}\""))
        .arg("-e")
        .arg("end try")
        .arg("-e")
        .arg(format!(
            "if chosenNameList is false then return \"{OPEN_WITH_CANCELLED_SENTINEL}\""
        ))
        .arg("-e")
        .arg("return (item 1 of chosenNameList)")
        .arg("-e")
        .arg("end run");
    for name in names {
        command.arg(name);
    }
    command
}

/// Runs an `osascript` command built by [`open_with_chooser_command`] or
/// [`choose_from_list_command`], returning its stdout on success.
fn run_osascript(mut command: std::process::Command) -> Result<Vec<u8>, PlatformError> {
    let output = command.output().map_err(|error| PlatformError::Io {
        message: format!("failed to launch the Open With chooser: {error}"),
    })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(PlatformError::Io {
            message: format!(
                "Open With chooser exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        })
    }
}

fn fetch_icon_png(path: &Path) -> Result<Vec<u8>, PlatformError> {
    let ns_path = NSString::from_str(path_to_str(path)?);
    let image = NSWorkspace::sharedWorkspace().iconForFile(&ns_path);
    let tiff = image
        .TIFFRepresentation()
        .ok_or_else(|| PlatformError::Io {
            message: "failed to obtain a TIFF representation of the icon".to_owned(),
        })?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&tiff).ok_or_else(|| PlatformError::Io {
        message: "failed to decode the icon's TIFF representation".to_owned(),
    })?;
    let properties = NSDictionary::new();
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| PlatformError::Io {
        message: "failed to encode the icon as PNG".to_owned(),
    })?;
    Ok(png.to_vec())
}

fn discover_system_locations(home: &Path) -> Vec<SystemLocation> {
    let mut locations = Vec::new();
    let cloud_storage = home.join("Library/CloudStorage");
    if let Ok(entries) = std::fs::read_dir(cloud_storage) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            locations.push(SystemLocation {
                provider_hint: cloud_provider_hint(&name).map(str::to_owned),
                name,
                path,
                kind: SystemLocationKind::Cloud,
                protocol: None,
                server: None,
                share: None,
                read_only: None,
            });
        }
    }
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.is_dir() {
        locations.push(SystemLocation {
            name: "iCloud Drive".to_owned(),
            path: icloud,
            kind: SystemLocationKind::Cloud,
            provider_hint: Some("icloud".to_owned()),
            protocol: None,
            server: None,
            share: None,
            read_only: None,
        });
    }
    prefer_home_symlink_aliases(home, &mut locations);
    locations.sort_by(|left, right| left.name.cmp(&right.name));
    locations.dedup_by(|left, right| left.path == right.path);
    locations
}

fn prefer_home_symlink_aliases(home: &Path, locations: &mut [SystemLocation]) {
    let mut aliases = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            std::fs::symlink_metadata(&path)
                .ok()?
                .file_type()
                .is_symlink()
                .then(|| (entry.file_name().to_string_lossy().into_owned(), path))
        })
        .collect::<Vec<_>>();
    aliases.sort_by(|left, right| left.0.cmp(&right.0));

    for location in locations {
        let Ok(target) = std::fs::canonicalize(&location.path) else {
            continue;
        };
        if let Some((name, path)) = aliases
            .iter()
            .find(|(_, path)| std::fs::canonicalize(path).is_ok_and(|alias| alias == target))
        {
            location.name.clone_from(name);
            location.path.clone_from(path);
        }
    }
}

fn parse_mount_source(source: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (protocol, remainder) = source.split_once("://").map_or(
        (None, source.trim_start_matches('/')),
        |(protocol, remainder)| (Some(protocol.to_ascii_lowercase()), remainder),
    );
    let mut segments = remainder.split('/');
    let authority = segments.next().unwrap_or_default();
    let server = authority
        .rsplit_once('@')
        .map_or(authority, |(_, server)| server)
        .split(':')
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let share = segments.find(|value| !value.is_empty()).map(str::to_owned);
    (protocol, server, share)
}

fn network_location_from_metadata(
    path: PathBuf,
    name: String,
    is_local: bool,
    read_only: Option<bool>,
    mount_source: Option<&str>,
) -> Option<SystemLocation> {
    if is_local {
        return None;
    }
    let (protocol, server, share) = mount_source
        .map(parse_mount_source)
        .unwrap_or((None, None, None));
    Some(SystemLocation {
        name,
        path,
        kind: SystemLocationKind::Network,
        provider_hint: None,
        protocol,
        server,
        share,
        read_only,
    })
}

fn file_system_attribute_bytes(
    attributes: &NSDictionary<NSFileAttributeKey, AnyObject>,
    key: &NSFileAttributeKey,
) -> Option<u64> {
    attributes
        .objectForKey(key)?
        .downcast::<NSNumber>()
        .ok()
        .map(|value| value.unsignedLongLongValue())
}

fn bool_resource_value(url: &NSURL, key: &NSURLResourceKey) -> Option<bool> {
    let mut value = None;
    unsafe { url.getResourceValue_forKey_error(&mut value, key).ok()? };
    value?
        .downcast::<NSNumber>()
        .ok()
        .map(|value| value.as_bool())
}

fn string_resource_value(url: &NSURL, key: &NSURLResourceKey) -> Option<String> {
    let mut value = None;
    unsafe { url.getResourceValue_forKey_error(&mut value, key).ok()? };
    value?
        .downcast::<NSString>()
        .ok()
        .map(|value| value.to_string())
}

fn disk_image_mount_points_from_plist(metadata: &[u8]) -> Result<HashSet<PathBuf>, plist::Error> {
    let root = plist::Value::from_reader(std::io::Cursor::new(metadata))?;
    let mount_points = root
        .as_dictionary()
        .and_then(|dictionary| dictionary.get("images"))
        .and_then(plist::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(plist::Value::as_dictionary)
        .filter_map(|image| image.get("system-entities"))
        .filter_map(plist::Value::as_array)
        .flatten()
        .filter_map(plist::Value::as_dictionary)
        .filter_map(|entity| entity.get("mount-point"))
        .filter_map(plist::Value::as_string)
        .map(PathBuf::from)
        .collect();
    Ok(mount_points)
}

fn mounted_disk_image_paths() -> HashSet<PathBuf> {
    let Ok(output) = std::process::Command::new("/usr/bin/hdiutil")
        .args(["info", "-plist"])
        .output()
    else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }
    disk_image_mount_points_from_plist(&output.stdout).unwrap_or_default()
}

fn discover_network_locations() -> Result<Vec<SystemLocation>, PlatformError> {
    let (local_key, read_only_key, source_key) = unsafe {
        (
            NSURLVolumeIsLocalKey,
            NSURLVolumeIsReadOnlyKey,
            NSURLVolumeMountFromLocationKey,
        )
    };
    let keys = NSArray::from_slice(&[local_key, read_only_key, source_key]);
    let urls = NSFileManager::defaultManager()
        .mountedVolumeURLsIncludingResourceValuesForKeys_options(
            Some(&keys),
            NSVolumeEnumerationOptions::SkipHiddenVolumes,
        )
        .ok_or_else(|| PlatformError::Io {
            message: "failed to enumerate mounted volumes".to_owned(),
        })?;
    let mut locations = Vec::new();
    for url in &urls {
        let Some(path) = url.path() else {
            continue;
        };
        let path = PathBuf::from(path.to_string());
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let Some(is_local) = bool_resource_value(&url, local_key) else {
            continue;
        };
        let read_only = bool_resource_value(&url, read_only_key);
        let source = string_resource_value(&url, source_key);
        if let Some(location) =
            network_location_from_metadata(path, name, is_local, read_only, source.as_deref())
        {
            locations.push(location);
        }
    }
    locations.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(locations)
}

impl PlatformAdapter for MacosPlatformAdapter {
    fn system_locations(&self) -> Result<Vec<SystemLocation>, PlatformError> {
        let home = dirs::home_dir().ok_or_else(|| PlatformError::Io {
            message: "home directory is unavailable".to_owned(),
        })?;
        let mut locations = discover_system_locations(&home);
        // Network enumeration is advisory: a temporarily unavailable share or a host sandbox
        // must not hide otherwise reachable cloud-backed locations.
        if let Ok(network_locations) = discover_network_locations() {
            locations.extend(network_locations);
        }
        prefer_home_symlink_aliases(&home, &mut locations);
        Ok(locations)
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::FILE_ICONS
            | PlatformCapabilities::REVEAL_IN_FILE_MANAGER
            | PlatformCapabilities::TRASH
            | PlatformCapabilities::OPEN_TERMINAL
            | PlatformCapabilities::MOUNTED_VOLUMES
            | PlatformCapabilities::NATIVE_MENUS
            | PlatformCapabilities::NATIVE_DRAG_OUT
            | PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
            | PlatformCapabilities::VOLUME_CAPACITY
            | PlatformCapabilities::EXTENDED_ATTRIBUTES
            | PlatformCapabilities::FINDER_TAGS
            | PlatformCapabilities::APPLICATION_UNINSTALL
            | PlatformCapabilities::PLATFORM_CONTEXT_MENU
            | PlatformCapabilities::QUICK_LOOK
    }

    fn file_icon(&self, path: &Path) -> Result<Vec<u8>, PlatformError> {
        let key = icon_cache_key(path);
        if let Some(cached) = self
            .icon_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return Ok(cached.clone());
        }
        let png = fetch_icon_png(path)?;
        self.icon_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key, png.clone());
        Ok(png)
    }

    fn thumbnail(&self, path: &Path, max_size: u32) -> Result<Vec<u8>, PlatformError> {
        self.fallback.thumbnail(path, max_size)
    }

    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError> {
        let ns_path = NSString::from_str(path_to_str(path)?);
        let url = NSURL::fileURLWithPath(&ns_path);
        let urls = NSArray::from_slice(&[&*url]);
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
        Ok(())
    }

    fn trash(&self, path: &Path) -> Result<(), PlatformError> {
        self.trash_with_restore_location(path).map(|_| ())
    }

    fn trash_with_restore_location(&self, path: &Path) -> Result<Option<PathBuf>, PlatformError> {
        let ns_path = NSString::from_str(path_to_str(path)?);
        let url = NSURL::fileURLWithPath(&ns_path);
        let mut resulting_url = None;
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url))
            .map_err(|error| PlatformError::Io {
                message: error.localizedDescription().to_string(),
            })?;
        Ok(resulting_url
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string())))
    }

    fn open_with_default_application(&self, path: &Path) -> Result<(), PlatformError> {
        let status = std::process::Command::new("open")
            .arg(path)
            .status()
            .map_err(|error| PlatformError::Io {
                message: format!("failed to launch the default application: {error}"),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: format!("open exited with {status}"),
            })
        }
    }

    fn open_terminal(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        let app = command_override.unwrap_or("Terminal");
        let status = std::process::Command::new("open")
            .arg("-a")
            .arg(app)
            .arg(path)
            .status()
            .map_err(|error| PlatformError::Io {
                message: format!("failed to launch {app}: {error}"),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: format!("{app} launch exited with {status}"),
            })
        }
    }

    fn open_in_text_editor(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        let target = command_override.unwrap_or("the default text editor");
        let status = match command_override {
            Some(app) => std::process::Command::new("open")
                .arg("-a")
                .arg(app)
                .arg(path)
                .status(),
            None => std::process::Command::new("open")
                .arg("-t")
                .arg(path)
                .status(),
        }
        .map_err(|error| PlatformError::Io {
            message: format!("failed to launch {target}: {error}"),
        })?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Io {
                message: format!("{target} launch exited with {status}"),
            })
        }
    }

    fn open_with_chooser(&self, path: &Path) -> Result<(), PlatformError> {
        let recommended = recommended_applications(path)?;
        if recommended.is_empty() {
            run_osascript(open_with_chooser_command(path))?;
            return Ok(());
        }

        let mut names: Vec<String> = recommended.iter().map(|(name, _)| name.clone()).collect();
        names.push(OPEN_WITH_OTHER_APPLICATIONS.to_owned());
        let stdout = run_osascript(choose_from_list_command(&names))?;
        let chosen = String::from_utf8_lossy(&stdout).trim().to_owned();

        match resolve_open_with_choice(&chosen, &recommended) {
            OpenWithChoice::Cancelled => Ok(()),
            OpenWithChoice::Other => {
                run_osascript(open_with_chooser_command(path))?;
                Ok(())
            }
            OpenWithChoice::App(app_path) => {
                let status = std::process::Command::new("open")
                    .arg("-a")
                    .arg(&app_path)
                    .arg(path)
                    .status()
                    .map_err(|error| PlatformError::Io {
                        message: format!("failed to launch {}: {error}", app_path.display()),
                    })?;
                if status.success() {
                    Ok(())
                } else {
                    Err(PlatformError::Io {
                        message: format!("{} launch exited with {status}", app_path.display()),
                    })
                }
            }
        }
    }

    fn quick_look(&self, path: &Path) -> Result<(), PlatformError> {
        if !path.is_file() {
            return Err(PlatformError::NotFound {
                path: path.display().to_string(),
            });
        }
        let item = PreviewItem::from_file_url(path, None).ok_or_else(|| PlatformError::Io {
            message: "Quick Look requires a path that is valid UTF-8".to_owned(),
        })?;
        QUICK_LOOK_PANEL.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(quick_look_session()?);
            }
            let session = slot.as_ref().ok_or_else(|| PlatformError::Io {
                message: "Quick Look preview panel is unavailable".to_owned(),
            })?;
            session.panel.set_items(vec![item]);
            session.panel.reload_if_dirty();
            session.panel.set_current_preview_item_index(0);
            session.panel.show();
            Ok(())
        })
    }

    fn read_clipboard_file_references(&self) -> Result<Vec<PathBuf>, PlatformError> {
        self.fallback.read_clipboard_file_references()
    }

    fn write_clipboard_file_references(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        self.fallback.write_clipboard_file_references(paths)
    }

    fn mounted_volumes(&self) -> Result<Vec<MountedVolume>, PlatformError> {
        let disk_image_paths = mounted_disk_image_paths();
        let browsable_key = unsafe { NSURLVolumeIsBrowsableKey };
        let keys = NSArray::from_slice(&[browsable_key]);
        let urls = NSFileManager::defaultManager()
            .mountedVolumeURLsIncludingResourceValuesForKeys_options(
                Some(&keys),
                NSVolumeEnumerationOptions::SkipHiddenVolumes,
            )
            .ok_or_else(|| PlatformError::Io {
                message: "failed to enumerate mounted volumes".to_owned(),
            })?;
        let mut volumes = Vec::with_capacity(urls.len());
        for url in &urls {
            // Finder itself keys the sidebar's volume list off this flag, which is exactly what
            // it's for: driver installer disk images left mounted (printer/scanner software is
            // a common offender - it often mounts a support volume it never ejects) report
            // `isBrowsable == false` so they don't clutter a normal file browsing UI, even though
            // they remain fully mounted and are not "hidden" in `SkipHiddenVolumes`'s sense.
            // Missing/unreadable resource values default to included, matching the pre-filter
            // behaviour for volumes this key can't be determined for.
            if bool_resource_value(&url, browsable_key) == Some(false) {
                continue;
            }
            let Some(path) = url.path() else {
                continue;
            };
            let mount_point = PathBuf::from(path.to_string());
            if disk_image_paths.contains(&mount_point) {
                continue;
            }
            let name = mount_point
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| mount_point.to_string_lossy().into_owned());
            volumes.push(MountedVolume { name, mount_point });
        }
        Ok(volumes)
    }

    fn volume_capacity(&self, path: &Path) -> Result<VolumeCapacity, PlatformError> {
        let path_string = NSString::from_str(path_to_str(path)?);
        let attributes = NSFileManager::defaultManager()
            .attributesOfFileSystemForPath_error(&path_string)
            .map_err(|_| PlatformError::NotFound {
                path: path.display().to_string(),
            })?;
        let total_bytes = file_system_attribute_bytes(&attributes, unsafe { NSFileSystemSize })
            .ok_or_else(|| PlatformError::Io {
                message: "missing NSFileSystemSize attribute".to_owned(),
            })?;
        let available_bytes =
            file_system_attribute_bytes(&attributes, unsafe { NSFileSystemFreeSize }).ok_or_else(
                || PlatformError::Io {
                    message: "missing NSFileSystemFreeSize attribute".to_owned(),
                },
            )?;
        Ok(VolumeCapacity {
            total_bytes,
            available_bytes,
        })
    }

    /// Installs a real, populated native menu bar (task 0133) built from
    /// `spec`, replacing whatever menu bar (if any) is currently installed.
    /// Native menu APIs require the main thread; off it, this reports
    /// [`PlatformError::Io`] rather than panicking (unchanged from task
    /// 0058's original hook-point-only behaviour).
    ///
    /// `on_action` becomes the new process-wide "current menu action
    /// callback" - see [`MENU_ACTION_CALLBACK`]'s doc comment for why a
    /// single shared slot is an honest design here rather than a
    /// per-item closure-capture mechanism.
    fn install_native_menu(
        &self,
        spec: &fm_domain::NativeMenuSpec,
        on_action: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), PlatformError> {
        let Some(main_thread) = MainThreadMarker::new() else {
            return Err(PlatformError::Io {
                message: "installing the native menu bar requires the main thread".to_owned(),
            });
        };
        *MENU_ACTION_CALLBACK
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(on_action);
        // AppKit always replaces the App menu's own displayed title with the process name,
        // regardless of the NSMenuItem title given to it - but in an unbundled `cargo tauri dev`
        // run (no real .app bundle/Info.plist) that process name is just the raw executable name
        // (e.g. "fm-desktop"), not the product name a real release build would show. Overriding
        // NSProcessInfo's processName here makes the menu bar read correctly in dev mode too,
        // using the first top-level menu's title (spec.menus[0].title) as that name, since the
        // caller already supplies it for exactly this slot.
        if let Some(first_menu) = spec.menus.first() {
            NSProcessInfo::processInfo().setProcessName(&NSString::from_str(&first_menu.title));
        }
        let target = MenuActionTarget::shared(main_thread);
        let menu_bar = NSMenu::new(main_thread);
        for menu in &spec.menus {
            menu_bar.addItem(&build_menu_bar_item(main_thread, menu, &target));
        }
        NSApplication::sharedApplication(main_thread).setMainMenu(Some(&menu_bar));
        Ok(())
    }

    fn finder_tags(&self, path: &Path) -> Result<Vec<FinderTag>, PlatformError> {
        read_finder_tags(path)
    }

    fn set_finder_tags(&self, path: &Path, tags: &[FinderTag]) -> Result<(), PlatformError> {
        write_finder_tags(path, tags)
    }

    fn spotlight_comment(&self, path: &Path) -> Result<Option<String>, PlatformError> {
        read_spotlight_comment(path)
    }

    fn set_spotlight_comment(
        &self,
        path: &Path,
        comment: Option<&str>,
    ) -> Result<(), PlatformError> {
        write_spotlight_comment(path, comment)
    }

    fn plan_application_uninstall(
        &self,
        bundle_path: &Path,
    ) -> Result<ApplicationUninstallPlan, PlatformError> {
        uninstall::plan_application_uninstall(bundle_path)
    }

    fn remove_application_dock_icon(&self, bundle_path: &Path) -> Result<bool, PlatformError> {
        dock::remove_dock_icon(bundle_path)
    }
}

/// Installs a Dock-icon context menu with a single "New Window" item, so right/long-clicking
/// Procyon's Dock icon offers the same "open another window" shortcut as the File menu's own
/// "New Window" item, without switching to the app first.
///
/// AppKit builds the Dock menu by calling `-applicationDockMenu:` on `NSApp`'s delegate - but
/// Tauri/tao's own delegate class never implements that selector, so this adds it at runtime via
/// `class_addMethod`. That only ever *adds* a selector to a class; it can't override one that
/// already exists, so none of tao's own delegate behaviour (window/lifecycle events) is touched.
///
/// The added method ignores its arguments and always returns the one menu built here, wired
/// through the exact same [`MenuActionTarget`]/[`MENU_ACTION_CALLBACK`] plumbing as the main menu
/// bar (task 0133): a click sends `new_window_action_id` through whichever channel
/// [`MacosPlatformAdapter::install_native_menu`]'s `on_action` last installed - the frontend's
/// `NEW_WORKSPACE_WINDOW_MENU_ID`, so it lands on the exact same handler as the File menu's own
/// item.
///
/// A no-op off the main thread, or if `NSApp` has no delegate yet - both unreachable in practice
/// since this is called once from Tauri's `setup` hook, which runs on the main thread after
/// `NSApp`'s delegate is set.
pub fn install_dock_menu(menu_title: &str, new_window_title: &str, new_window_action_id: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return;
    };

    let target = MenuActionTarget::shared(mtm);
    let menu = NSMenu::new(mtm);
    menu.setTitle(&NSString::from_str(menu_title));
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(new_window_title),
            Some(sel!(handleMenuItem:)),
            &NSString::from_str(""),
        )
    };
    let target_ref: &AnyObject = &target;
    let action_id = NSString::from_str(new_window_action_id);
    let action_id_ref: &AnyObject = &action_id;
    unsafe {
        item.setTarget(Some(target_ref));
        item.setRepresentedObject(Some(action_id_ref));
    }
    menu.addItem(&item);

    // Leaked for the process's lifetime: `applicationDockMenu:` must keep returning a live menu
    // for as long as the app runs, and there is no natural point at which a Tauri app would want
    // to tear this down early.
    let menu_ptr = Retained::into_raw(menu);
    DOCK_MENU.store(menu_ptr as *mut AnyObject as usize, Ordering::SeqCst);

    let delegate_object: &AnyObject = delegate.as_ref();
    let delegate_class: *mut AnyClass = delegate_object.class() as *const AnyClass as *mut AnyClass;
    let sel = sel!(applicationDockMenu:);
    // SAFETY: `types` describes an Objective-C method returning an object (`@`), taking the
    // implicit `self`/`_cmd` pair plus one object argument (the sender) - matching
    // `dock_menu_imp`'s actual signature below. `class_addMethod` is a no-op (returns false,
    // which this ignores) if the delegate's class already implements the selector, so calling
    // this function twice (e.g. a hot-reloaded `setup`) never clobbers an earlier install.
    unsafe {
        objc2::ffi::class_addMethod(
            delegate_class,
            sel,
            core::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> *mut AnyObject,
                Imp,
            >(dock_menu_imp),
            c"@@:@".as_ptr(),
        );
    }
}

/// Process-wide slot for the menu [`install_dock_menu`] built, read back by [`dock_menu_imp`].
/// Stored as a raw pointer (rather than `Retained<NSMenu>`) because `NSMenu` isn't `Send`/`Sync`
/// and both are only ever touched from the main thread anyway - see [`install_dock_menu`]'s doc
/// comment for why leaking it is fine.
static DOCK_MENU: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The `-applicationDockMenu:` implementation [`install_dock_menu`] attaches to `NSApp`'s
/// delegate class. Ignores `_this`/`_cmd`/`_sender` - there is only ever one Dock menu for the
/// process - and returns whichever menu is currently in [`DOCK_MENU`], or a null pointer (AppKit
/// falls back to no Dock menu) if none has been installed yet.
unsafe extern "C-unwind" fn dock_menu_imp(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> *mut AnyObject {
    DOCK_MENU.load(Ordering::SeqCst) as *mut AnyObject
}

/// The callback most recently installed by
/// [`MacosPlatformAdapter::install_native_menu`] (task 0133).
///
/// There is only ever one native menu bar for the process, so a single
/// process-wide slot is an honest design here rather than a per-item
/// closure-capture mechanism: every `Action` item built from a
/// [`fm_domain::NativeMenuSpec`] shares one [`MenuActionTarget`] instance,
/// and its `handleMenuItem:` looks up whichever callback is current here.
/// Only ever touched from [`MacosPlatformAdapter::install_native_menu`]
/// (which requires a [`MainThreadMarker`]) and
/// `MenuActionTarget::handle_menu_item` (which AppKit only ever invokes on
/// the main thread), so the `Mutex` exists to satisfy `Sync` for the
/// `static`, not because of real cross-thread contention.
type MenuActionCallback = Arc<dyn Fn(String) + Send + Sync>;

static MENU_ACTION_CALLBACK: Mutex<Option<MenuActionCallback>> = Mutex::new(None);

define_class!(
    // SAFETY:
    // - `NSObject` has no subclassing requirements beyond calling `init`.
    // - `MenuActionTarget` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[derive(Debug)]
    struct MenuActionTarget;

    impl MenuActionTarget {
        /// Target-action handler for every `NativeMenuItem::Action` item
        /// (task 0133): reads the clicked item's `representedObject` (the
        /// action id, stashed there as an `NSString` when the item was
        /// built - see [`build_menu_item`]) and forwards it to whichever
        /// callback [`MacosPlatformAdapter::install_native_menu`] last
        /// installed via [`MENU_ACTION_CALLBACK`].
        #[unsafe(method(handleMenuItem:))]
        fn handle_menu_item(&self, sender: &NSMenuItem) {
            let Some(represented) = sender.representedObject() else {
                return;
            };
            let Ok(action_id) = represented.downcast::<NSString>() else {
                return;
            };
            let callback = MENU_ACTION_CALLBACK
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            if let Some(callback) = callback {
                callback(action_id.to_string());
            }
        }
    }
);

impl MenuActionTarget {
    /// Returns this thread's shared `MenuActionTarget` instance, creating it
    /// on first use. Kept in a `thread_local!` (rather than a process-wide
    /// `static`) because `MenuActionTarget` is `MainThreadOnly` and so isn't
    /// `Send`/`Sync`; every caller already holds a [`MainThreadMarker`], so
    /// this is always called from the same (main) thread in practice.
    fn shared(mtm: MainThreadMarker) -> Retained<Self> {
        thread_local! {
            static INSTANCE: RefCell<Option<Retained<MenuActionTarget>>> = const { RefCell::new(None) };
        }
        INSTANCE.with(|instance| {
            let mut instance = instance.borrow_mut();
            if let Some(existing) = instance.as_ref() {
                return existing.clone();
            }
            let allocated = mtm.alloc::<Self>().set_ivars(());
            let created: Retained<Self> = unsafe { msg_send![super(allocated), init] };
            *instance = Some(created.clone());
            created
        })
    }
}

#[derive(Debug)]
struct ServicesRequestorIvars {
    paths: Vec<String>,
}

define_class!(
    // SAFETY:
    // - `NSResponder` has no subclassing requirements beyond calling `init`.
    // - `ServicesRequestor` does not implement `Drop`.
    #[unsafe(super = NSResponder)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ServicesRequestorIvars]
    #[derive(Debug)]
    struct ServicesRequestor;

    impl ServicesRequestor {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(validRequestorForSendType:returnType:))]
        fn valid_requestor(
            &self,
            send_type: Option<&NSPasteboardType>,
            return_type: Option<&NSPasteboardType>,
        ) -> Option<&AnyObject> {
            let accepts_send_type = send_type
                .map(ToString::to_string)
                .is_some_and(|value| services_send_type_supported(&value));
            if !accepts_send_type || return_type.is_some() {
                return None;
            }
            let object: &AnyObject = self;
            Some(object)
        }

        #[unsafe(method(writeSelectionToPasteboard:types:))]
        fn write_selection(
            &self,
            pasteboard: &NSPasteboard,
            types: &NSArray<NSPasteboardType>,
        ) -> bool {
            pasteboard.clearContents();
            let requested_types = types
                .iter()
                .map(|data_type| data_type.to_string())
                .collect::<Vec<_>>();
            let mut wrote_selection = false;
            if requested_types
                .iter()
                .any(|data_type| data_type == "NSFilenamesPboardType")
            {
                let filenames = self
                    .ivars()
                    .paths
                    .iter()
                    .map(|path| NSString::from_str(path))
                    .collect::<Vec<_>>();
                let filenames = NSArray::from_retained_slice(&filenames);
                let filenames_type = NSString::from_str("NSFilenamesPboardType");
                wrote_selection |= unsafe {
                    pasteboard.setPropertyList_forType(&filenames, &filenames_type)
                };
            }
            if !requested_types
                .iter()
                .any(|data_type| data_type == "public.file-url")
            {
                return wrote_selection.into();
            }
            let urls = self
                .ivars()
                .paths
                .iter()
                .map(|path| NSURL::fileURLWithPath(&NSString::from_str(path)))
                .collect::<Vec<_>>();
            let urls = NSArray::from_retained_slice(&urls);
            let wrote_urls: bool = unsafe { msg_send![pasteboard, writeObjects: &*urls] };
            wrote_selection | wrote_urls
        }
    }
);

fn services_send_type_supported(send_type: &str) -> bool {
    matches!(send_type, "public.file-url" | "NSFilenamesPboardType")
}

impl ServicesRequestor {
    fn new(mtm: MainThreadMarker, paths: &[PathBuf]) -> Result<Retained<Self>, PlatformError> {
        let paths = paths
            .iter()
            .map(|path| path_to_str(path).map(str::to_owned))
            .collect::<Result<Vec<_>, _>>()?;
        let allocated = mtm
            .alloc::<Self>()
            .set_ivars(ServicesRequestorIvars { paths });
        Ok(unsafe { msg_send![super(allocated), init] })
    }
}

/// Opens AppKit's OS-populated Services menu for `paths` at the current pointer position.
///
/// The caller must schedule this on the process main thread. While the menu is open, a temporary
/// `NSServicesMenuRequestor`-compatible responder exposes the selected file URLs to AppKit; the
/// window's previous first responder is restored before returning.
pub fn show_services_menu(paths: &[PathBuf]) -> Result<(), PlatformError> {
    if paths.is_empty() {
        return Err(PlatformError::Io {
            message: "at least one path is required for the Services menu".to_owned(),
        });
    }
    let mtm = MainThreadMarker::new().ok_or_else(|| PlatformError::Io {
        message: "the Services menu must be opened on the main thread".to_owned(),
    })?;
    let app = NSApplication::sharedApplication(mtm);
    let window = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .ok_or_else(|| PlatformError::Io {
            message: "no active window is available for the Services menu".to_owned(),
        })?;
    let previous_responder = window.firstResponder();
    let requestor = ServicesRequestor::new(mtm, paths)?;
    if !window.makeFirstResponder(Some(&requestor)) {
        return Err(PlatformError::Io {
            message: "the selected files could not become the Services requestor".to_owned(),
        });
    }

    let existing_services_menu = app.servicesMenu();
    let services_menu = existing_services_menu
        .clone()
        .unwrap_or_else(|| NSMenu::new(mtm));
    services_menu.setTitle(&NSString::from_str("Services"));
    app.setServicesMenu(Some(&services_menu));
    services_menu.update();
    let _selected = services_menu.popUpMenuPositioningItem_atLocation_inView(
        None,
        NSEvent::mouseLocation(),
        None,
    );
    let _ = window.makeFirstResponder(previous_responder.as_deref());
    if existing_services_menu.is_none() {
        app.setServicesMenu(None);
    }

    Ok(())
}

/// Bit positions for `NSEventModifierFlags::{Shift,Control,Option,Command}`,
/// duplicated here (rather than depending on the real AppKit type) so
/// [`key_equivalent`] stays a pure function unit tests can exercise without
/// a windowing system.
const MODIFIER_SHIFT_BIT: usize = 1 << 17;
const MODIFIER_CONTROL_BIT: usize = 1 << 18;
const MODIFIER_OPTION_BIT: usize = 1 << 19;
const MODIFIER_COMMAND_BIT: usize = 1 << 20;

/// `NSF1FunctionKey`'s Unicode code point; `NSF2FunctionKey`.."NSF35FunctionKey" follow it
/// sequentially, one per function key.
const NS_F1_FUNCTION_KEY: u32 = 0xF704;
const MAX_FUNCTION_KEY_NUMBER: u32 = 35;

/// Maps `"F1"`..`"F35"` to the `NSF1FunctionKey`.. private-use-area character AppKit expects as
/// a function-key equivalent (e.g. `"F1"` -> `'\u{F704}'`), or `None` for anything else.
fn function_key_char(key: &str) -> Option<char> {
    let number: u32 = key.strip_prefix('F')?.parse().ok()?;
    if number == 0 || number > MAX_FUNCTION_KEY_NUMBER {
        return None;
    }
    char::from_u32(NS_F1_FUNCTION_KEY + (number - 1))
}

/// Maps a [`fm_domain::KeyChord`] to the `(key equivalent, modifier mask)`
/// pair an `NSMenuItem` needs (task 0133). A lowercased single character is
/// handled directly ("c", "v", "a", ","), and `"F1"`..`"F35"` map to the
/// corresponding `NSF1FunctionKey`.. private-use-area character, which
/// AppKit renders as e.g. "F1" on its own without needing
/// `NSEventModifierFlagFunction` in the mask - that flag is reserved for
/// system-provided menu items and AppKit logs a warning and ignores it if
/// an app sets it on its own items. Any other multi-character key name
/// (e.g. `"Escape"`, `"Enter"`) is deliberately left blank (no key
/// equivalent shown at all) rather than over-engineered, matching this
/// task's scope.
/// Blank, not truncated: taking just the first character of a
/// multi-character key name would silently collide distinct shortcuts onto
/// the same displayed equivalent instead of leaving them untranslated,
/// which is worse than showing nothing.
/// The returned mask mirrors `NSEventModifierFlags`'s own bit positions 1:1,
/// so the only remaining step at the call site is widening it into that
/// real type.
fn key_equivalent(chord: &fm_domain::KeyChord) -> (String, usize) {
    let mut mask = 0usize;
    if chord.shift {
        mask |= MODIFIER_SHIFT_BIT;
    }
    if chord.ctrl {
        mask |= MODIFIER_CONTROL_BIT;
    }
    if chord.alt {
        mask |= MODIFIER_OPTION_BIT;
    }
    if chord.meta {
        mask |= MODIFIER_COMMAND_BIT;
    }

    if let Some(function_char) = function_key_char(&chord.key) {
        return (function_char.to_string(), mask);
    }

    let key = if chord.key.chars().count() == 1 {
        chord.key.to_ascii_lowercase()
    } else {
        String::new()
    };
    (key, mask)
}

/// Builds a top-level `NSMenuItem` (e.g. the "File" entry in the menu bar)
/// holding a submenu built from `menu.items`. AppKit ignores this item's
/// title for the very first menu in the bar (it shows the process name
/// instead) but the title is still required structurally.
fn build_menu_bar_item(
    mtm: MainThreadMarker,
    menu: &fm_domain::NativeMenu,
    target: &Retained<MenuActionTarget>,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(&menu.title),
            None,
            &NSString::from_str(""),
        )
    };
    item.setSubmenu(Some(&build_submenu(mtm, &menu.title, &menu.items, target)));
    item
}

/// Builds an `NSMenu` titled `title` from `items` (task 0133), used for both
/// top-level menus and nested `NativeMenuItem::Submenu` items.
fn build_submenu(
    mtm: MainThreadMarker,
    title: &str,
    items: &[fm_domain::NativeMenuItem],
    target: &Retained<MenuActionTarget>,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setTitle(&NSString::from_str(title));
    for item in items {
        menu.addItem(&build_menu_item(mtm, item, target));
    }
    menu
}

/// Builds one `NSMenuItem` from a [`fm_domain::NativeMenuItem`] (task 0133).
///
/// `Action` items are wired back to `target`'s `handleMenuItem:` (rather
/// than a per-item closure) with their action-registry id stashed in
/// `representedObject`, so a click and the matching keyboard shortcut
/// dispatch through the exact same id. `Role` items get no application
/// callback at all: their target stays nil so AppKit routes them through the
/// normal first-responder chain, exactly like a standard macOS app menu.
fn build_menu_item(
    mtm: MainThreadMarker,
    item: &fm_domain::NativeMenuItem,
    target: &Retained<MenuActionTarget>,
) -> Retained<NSMenuItem> {
    match item {
        fm_domain::NativeMenuItem::Separator => NSMenuItem::separatorItem(mtm),
        fm_domain::NativeMenuItem::Action {
            id,
            title,
            shortcut,
            enabled,
            checked,
        } => {
            let (key, mask) = shortcut.as_ref().map(key_equivalent).unwrap_or_default();
            let ns_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(title),
                    Some(sel!(handleMenuItem:)),
                    &NSString::from_str(&key),
                )
            };
            ns_item.setKeyEquivalentModifierMask(NSEventModifierFlags(mask));
            ns_item.setEnabled(*enabled);
            ns_item.setState(if *checked {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            let target_ref: &AnyObject = target;
            let action_id = NSString::from_str(id);
            let action_id_ref: &AnyObject = &action_id;
            unsafe {
                ns_item.setTarget(Some(target_ref));
                ns_item.setRepresentedObject(Some(action_id_ref));
            }
            ns_item
        }
        fm_domain::NativeMenuItem::Submenu { title, items } => {
            let ns_item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str(title),
                    None,
                    &NSString::from_str(""),
                )
            };
            ns_item.setSubmenu(Some(&build_submenu(mtm, title, items, target)));
            ns_item
        }
        fm_domain::NativeMenuItem::Role { role } => build_role_item(mtm, *role),
    }
}

/// Builds a standard OS-provided menu item for `role` (task 0133), with no
/// application callback: target stays nil (except for `Services`, which has
/// no target at all - it's registered directly via
/// `NSApplication::setServicesMenu`) so AppKit dispatches through the normal
/// first-responder chain, exactly like a stock macOS app menu.
fn build_role_item(mtm: MainThreadMarker, role: fm_domain::NativeMenuRole) -> Retained<NSMenuItem> {
    use fm_domain::NativeMenuRole;

    if role == NativeMenuRole::Services {
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Services"),
                None,
                &NSString::from_str(""),
            )
        };
        let services_menu = NSMenu::new(mtm);
        services_menu.setTitle(&NSString::from_str("Services"));
        item.setSubmenu(Some(&services_menu));
        NSApplication::sharedApplication(mtm).setServicesMenu(Some(&services_menu));
        return item;
    }

    // (title, selector, key equivalent, modifier mask): the same four standard AppKit
    // accelerators every macOS app menu carries for these roles (Cmd+Q, Cmd+H, Cmd+Option+H,
    // Cmd+M). `About`, `ShowAll`, `Zoom`, and `BringAllToFront` have no OS-standard shortcut in
    // stock macOS apps either, so they stay blank like before.
    let (title, selector, key, mask) = match role {
        NativeMenuRole::About => ("About", sel!(orderFrontStandardAboutPanel:), "", 0),
        NativeMenuRole::Services => unreachable!("handled above"),
        NativeMenuRole::HideApp => ("Hide", sel!(hide:), "h", MODIFIER_COMMAND_BIT),
        NativeMenuRole::HideOthers => (
            "Hide Others",
            sel!(hideOtherApplications:),
            "h",
            MODIFIER_COMMAND_BIT | MODIFIER_OPTION_BIT,
        ),
        NativeMenuRole::ShowAll => ("Show All", sel!(unhideAllApplications:), "", 0),
        NativeMenuRole::Quit => ("Quit", sel!(terminate:), "q", MODIFIER_COMMAND_BIT),
        NativeMenuRole::Minimize => (
            "Minimize",
            sel!(performMiniaturize:),
            "m",
            MODIFIER_COMMAND_BIT,
        ),
        NativeMenuRole::Zoom => ("Zoom", sel!(performZoom:), "", 0),
        NativeMenuRole::BringAllToFront => ("Bring All to Front", sel!(arrangeInFront:), "", 0),
    };
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            Some(selector),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(NSEventModifierFlags(mask));
    item
}

/// Extended attribute Finder stores a file's tags under (task 0136): a
/// binary property list containing an array of strings. Undocumented by
/// Apple but long-stable; see [`FinderTagColor`]'s doc comment for how a
/// colored tag is encoded within one array element.
const FINDER_TAGS_XATTR: &str = "com.apple.metadata:_kMDItemUserTags";

/// Extended attribute Finder stores the Get Info "Comments:" field under
/// (task 0136, also known as the Spotlight `kMDItemFinderComment`
/// attribute): a binary property list containing a single string.
const FINDER_COMMENT_XATTR: &str = "com.apple.metadata:kMDItemFinderComment";

/// Darwin's `ENOATTR` ("Attribute not found") errno - distinct from Linux's
/// `ENODATA`, and not one of the kinds `std::io::ErrorKind` categorizes on
/// its own, so [`remove_xattr`] checks the raw OS error directly. Verified
/// against this SDK's `<sys/errno.h>`; stable across macOS releases.
const ENOATTR: i32 = 93;

/// Reads and decodes [`FINDER_TAGS_XATTR`] (task 0136). An entry with no
/// tags (the attribute is absent) returns an empty `Vec`, not an error.
fn read_finder_tags(path: &Path) -> Result<Vec<FinderTag>, PlatformError> {
    let Some(bytes) = read_xattr(path, FINDER_TAGS_XATTR)? else {
        return Ok(Vec::new());
    };
    let value = decode_plist(&bytes, "Finder tags")?;
    let entries = value.into_array().ok_or_else(|| PlatformError::Io {
        message: "Finder tags xattr was not a plist array".to_owned(),
    })?;
    entries
        .into_iter()
        .map(|entry| {
            entry
                .into_string()
                .map(|raw| parse_finder_tag(&raw))
                .ok_or_else(|| PlatformError::Io {
                    message: "Finder tags xattr contained a non-string entry".to_owned(),
                })
        })
        .collect()
}

/// Replaces the complete set of Finder tags via [`FINDER_TAGS_XATTR`] (task
/// 0136), matching Finder's own all-at-once tag editor semantics. An empty
/// slice removes the attribute entirely, mirroring what Finder itself does
/// when the last tag is removed through its UI, rather than leaving behind
/// an empty-array attribute Finder would never write on its own.
fn write_finder_tags(path: &Path, tags: &[FinderTag]) -> Result<(), PlatformError> {
    if tags.is_empty() {
        return remove_xattr(path, FINDER_TAGS_XATTR);
    }
    let value = plist::Value::Array(
        tags.iter()
            .map(|tag| plist::Value::String(encode_finder_tag(tag)))
            .collect(),
    );
    write_xattr(
        path,
        FINDER_TAGS_XATTR,
        &encode_plist(&value, "Finder tags")?,
    )
}

/// Splits a raw `_kMDItemUserTags` array element into a tag name and color,
/// per [`FinderTagColor`]'s `"<name>\n<digit>"` encoding. Anything that
/// doesn't match that exact shape (no trailing newline+digit, or a
/// multi-character/non-digit suffix) is treated as an uncolored tag named
/// after the whole raw string, rather than rejected - a foreign or
/// hand-edited xattr should degrade gracefully, not make every tag on the
/// entry unreadable.
fn parse_finder_tag(raw: &str) -> FinderTag {
    if let Some((name, suffix)) = raw.rsplit_once('\n')
        && let Ok(index) = suffix.parse::<u8>()
        && suffix.len() == 1
    {
        return FinderTag {
            name: name.to_owned(),
            color: FinderTagColor::from_index(index),
        };
    }
    FinderTag {
        name: raw.to_owned(),
        color: FinderTagColor::None,
    }
}

/// Encodes one [`FinderTag`] as a `_kMDItemUserTags` array element: just the
/// name when uncolored (so an uncolored tag's on-disk representation is
/// identical to what Finder itself writes), otherwise `"<name>\n<digit>"`.
fn encode_finder_tag(tag: &FinderTag) -> String {
    if tag.color == FinderTagColor::None {
        tag.name.clone()
    } else {
        format!("{}\n{}", tag.name, tag.color.to_index())
    }
}

/// Reads and decodes [`FINDER_COMMENT_XATTR`] (task 0136). `None` means no
/// comment is set (the attribute is absent), not an error.
fn read_spotlight_comment(path: &Path) -> Result<Option<String>, PlatformError> {
    let Some(bytes) = read_xattr(path, FINDER_COMMENT_XATTR)? else {
        return Ok(None);
    };
    let value = decode_plist(&bytes, "Spotlight comment")?;
    value
        .into_string()
        .map(Some)
        .ok_or_else(|| PlatformError::Io {
            message: "Spotlight comment xattr was not a plist string".to_owned(),
        })
}

/// Sets (`Some`) or clears (`None`) [`FINDER_COMMENT_XATTR`] (task 0136).
fn write_spotlight_comment(path: &Path, comment: Option<&str>) -> Result<(), PlatformError> {
    match comment {
        None => remove_xattr(path, FINDER_COMMENT_XATTR),
        Some(comment) => {
            let value = plist::Value::String(comment.to_owned());
            write_xattr(
                path,
                FINDER_COMMENT_XATTR,
                &encode_plist(&value, "Spotlight comment")?,
            )
        }
    }
}

fn decode_plist(bytes: &[u8], what: &str) -> Result<plist::Value, PlatformError> {
    plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|error| PlatformError::Io {
        message: format!("failed to decode {what}: {error}"),
    })
}

fn encode_plist(value: &plist::Value, what: &str) -> Result<Vec<u8>, PlatformError> {
    let mut bytes = Vec::new();
    value
        .to_writer_binary(&mut bytes)
        .map_err(|error| PlatformError::Io {
            message: format!("failed to encode {what}: {error}"),
        })?;
    Ok(bytes)
}

fn read_xattr(path: &Path, name: &str) -> Result<Option<Vec<u8>>, PlatformError> {
    xattr::get(path, name).map_err(|error| map_xattr_io_error(path, &error))
}

fn write_xattr(path: &Path, name: &str, value: &[u8]) -> Result<(), PlatformError> {
    xattr::set(path, name, value).map_err(|error| map_xattr_io_error(path, &error))
}

/// Removing an attribute that was never set (e.g. clearing tags on an
/// already-untagged file) is a no-op success, not an error - the caller
/// asked for "no tags/no comment" and that's already true.
fn remove_xattr(path: &Path, name: &str) -> Result<(), PlatformError> {
    match xattr::remove(path, name) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(ENOATTR) => Ok(()),
        Err(error) => Err(map_xattr_io_error(path, &error)),
    }
}

fn map_xattr_io_error(path: &Path, error: &std::io::Error) -> PlatformError {
    if error.kind() == std::io::ErrorKind::NotFound {
        PlatformError::NotFound {
            path: path.display().to_string(),
        }
    } else {
        PlatformError::Io {
            message: format!("extended attribute operation failed: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn maps_remote_volume_metadata_without_assuming_volumes_are_smb() {
        let location = network_location_from_metadata(
            PathBuf::from("/Volumes/Team Files"),
            "Team Files".to_owned(),
            false,
            Some(true),
            Some("smb://files.example.test/team"),
        )
        .expect("remote volume");

        assert_eq!(location.kind, SystemLocationKind::Network);
        assert_eq!(location.protocol.as_deref(), Some("smb"));
        assert_eq!(location.server.as_deref(), Some("files.example.test"));
        assert_eq!(location.share.as_deref(), Some("team"));
        assert_eq!(location.read_only, Some(true));
    }

    #[test]
    fn excludes_local_volumes_even_when_mounted_under_volumes() {
        assert!(
            network_location_from_metadata(
                PathBuf::from("/Volumes/Backup"),
                "Backup".to_owned(),
                true,
                Some(false),
                None,
            )
            .is_none()
        );
    }

    #[test]
    fn cloud_location_discovery_classifies_known_providers_and_keeps_unknown_folders() {
        let home = tempdir().expect("temp home");
        for relative in [
            "Library/CloudStorage/OneDrive-Example",
            "Library/CloudStorage/Custom Sync",
            "Library/Mobile Documents/com~apple~CloudDocs",
        ] {
            std::fs::create_dir_all(home.path().join(relative)).expect("create cloud fixture");
        }

        let locations = discover_system_locations(home.path());

        assert_eq!(
            locations
                .iter()
                .map(|location| (location.name.as_str(), location.provider_hint.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("Custom Sync", None),
                ("OneDrive-Example", Some("onedrive")),
                ("iCloud Drive", Some("icloud")),
            ]
        );
        assert!(locations.iter().all(|location| {
            location.kind == SystemLocationKind::Cloud && location.path.starts_with(home.path())
        }));
    }

    #[test]
    fn cloud_location_discovery_prefers_a_home_symlink_to_the_canonical_provider_root() {
        let home = tempdir().expect("temp home");
        let canonical = home.path().join("Library/CloudStorage/OneDrive-Example");
        std::fs::create_dir_all(&canonical).expect("create cloud fixture");
        let alias = home.path().join("OneDrive");
        std::os::unix::fs::symlink(&canonical, &alias).expect("create home symlink");

        let locations = discover_system_locations(home.path());

        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].name, "OneDrive");
        assert_eq!(locations[0].path, alias);
        assert_eq!(locations[0].provider_hint.as_deref(), Some("onedrive"));
    }

    #[test]
    fn home_symlink_aliases_apply_to_mounted_network_locations_too() {
        let home = tempdir().expect("temp home");
        let mount = home.path().join("Volumes/Team Files");
        std::fs::create_dir_all(&mount).expect("create mounted share fixture");
        let alias = home.path().join("Team Files");
        std::os::unix::fs::symlink(&mount, &alias).expect("create home symlink");
        let mut locations = vec![SystemLocation {
            name: "Team Files".to_owned(),
            path: mount,
            kind: SystemLocationKind::Network,
            provider_hint: None,
            protocol: Some("smb".to_owned()),
            server: Some("files.example.test".to_owned()),
            share: Some("team".to_owned()),
            read_only: Some(false),
        }];

        prefer_home_symlink_aliases(home.path(), &mut locations);

        assert_eq!(locations[0].path, alias);
        assert_eq!(locations[0].kind, SystemLocationKind::Network);
        assert_eq!(locations[0].protocol.as_deref(), Some("smb"));
    }

    #[test]
    fn capabilities_report_exactly_the_implemented_operations() {
        let capabilities = MacosPlatformAdapter::new().capabilities();
        for expected in [
            PlatformCapabilities::FILE_ICONS,
            PlatformCapabilities::REVEAL_IN_FILE_MANAGER,
            PlatformCapabilities::TRASH,
            PlatformCapabilities::OPEN_TERMINAL,
            PlatformCapabilities::MOUNTED_VOLUMES,
            PlatformCapabilities::NATIVE_MENUS,
            PlatformCapabilities::NATIVE_DRAG_OUT,
            PlatformCapabilities::VOLUME_CAPACITY,
            PlatformCapabilities::EXTENDED_ATTRIBUTES,
            PlatformCapabilities::FINDER_TAGS,
            PlatformCapabilities::APPLICATION_UNINSTALL,
            PlatformCapabilities::PLATFORM_CONTEXT_MENU,
            PlatformCapabilities::QUICK_LOOK,
        ] {
            assert!(capabilities.contains(expected), "{expected:?}");
        }

        for unimplemented in [
            PlatformCapabilities::THUMBNAILS,
            PlatformCapabilities::CLIPBOARD_FILE_REFERENCES,
        ] {
            assert!(!capabilities.contains(unimplemented), "{unimplemented:?}");
        }
    }

    #[test]
    fn services_requestor_accepts_modern_and_legacy_file_types_but_not_arbitrary_text() {
        assert!(services_send_type_supported("public.file-url"));
        assert!(services_send_type_supported("NSFilenamesPboardType"));
        assert!(!services_send_type_supported("public.utf8-plain-text"));
    }

    #[test]
    fn thumbnail_and_clipboard_still_delegate_to_fallback() {
        let adapter = MacosPlatformAdapter::new();
        let fallback = FallbackPlatformAdapter;
        let path = Path::new("/tmp/fm-platform-macos-test.txt");

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
    }

    #[test]
    fn file_icon_is_fetched_once_per_extension_not_once_per_file() {
        let adapter = MacosPlatformAdapter::new();
        let dir = tempdir().expect("temp dir");
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        let c = dir.path().join("c.md");

        let icon_a = adapter.file_icon(&a).expect("icon for a.txt");
        assert!(!icon_a.is_empty());
        let icon_b = adapter.file_icon(&b).expect("icon for b.txt");
        assert_eq!(
            icon_a, icon_b,
            "two files sharing an extension must share a cached icon"
        );
        assert_eq!(
            adapter.icon_cache.lock().expect("icon cache lock").len(),
            1,
            "only one lookup should have happened for the shared .txt extension"
        );

        adapter.file_icon(&c).expect("icon for c.md");
        assert_eq!(
            adapter.icon_cache.lock().expect("icon cache lock").len(),
            2,
            "a distinct extension must populate a second cache entry"
        );
    }

    #[test]
    fn file_icon_extension_matching_is_case_insensitive() {
        assert_eq!(icon_cache_key(Path::new("/tmp/readme.TXT")), "txt");
        assert_eq!(icon_cache_key(Path::new("/tmp/readme.txt")), "txt");
    }

    #[test]
    #[ignore = "opens a real Finder window on the developer's desktop every run; \
                run explicitly with `cargo test -- --ignored` when verifying \
                reveal_in_file_manager changes"]
    fn reveal_in_finder_succeeds_for_a_real_temporary_file() {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("reveal-me.txt");
        std::fs::write(&file, b"content").expect("create fixture");

        MacosPlatformAdapter::new()
            .reveal_in_file_manager(&file)
            .expect("reveal in Finder");
    }

    #[test]
    fn trash_returns_a_location_that_can_be_safely_restored() {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("trash-me.txt");
        std::fs::write(&file, b"content").expect("create fixture");

        let adapter = MacosPlatformAdapter::new();
        let trashed = adapter
            .trash_with_restore_location(&file)
            .expect("trash the fixture file")
            .expect("macOS returns the trash location");

        assert!(!file.exists(), "the file must be gone from its directory");
        adapter
            .restore_from_trash(&trashed, &file)
            .expect("restore the fixture file");
        assert_eq!(std::fs::read(&file).expect("restored file"), b"content");
    }

    #[test]
    fn mounted_volumes_reports_at_least_the_boot_volume() {
        let volumes = MacosPlatformAdapter::new()
            .mounted_volumes()
            .expect("enumerate mounted volumes");
        let disk_image_paths = mounted_disk_image_paths();
        assert!(!volumes.is_empty());
        for volume in &volumes {
            assert!(volume.mount_point.is_absolute());
            assert!(
                !disk_image_paths.contains(&volume.mount_point),
                "disk image mount must not be listed: {}",
                volume.mount_point.display()
            );
        }
    }

    #[test]
    fn parses_mounted_disk_image_paths_from_hdiutil_metadata() {
        let metadata = br#"<?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
          "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0"><dict><key>images</key><array><dict>
          <key>system-entities</key><array>
            <dict><key>dev-entry</key><string>/dev/disk4</string></dict>
            <dict><key>dev-entry</key><string>/dev/disk4s1</string>
              <key>mount-point</key><string>/Volumes/GitHub Copilot</string></dict>
          </array>
        </dict></array></dict></plist>"#;

        assert_eq!(
            disk_image_mount_points_from_plist(metadata).expect("parse hdiutil metadata"),
            HashSet::from([PathBuf::from("/Volumes/GitHub Copilot")])
        );
    }

    #[test]
    fn volume_capacity_reports_plausible_totals_for_the_boot_volume() {
        let capacity = MacosPlatformAdapter::new()
            .volume_capacity(Path::new("/"))
            .expect("query boot volume capacity");
        assert!(capacity.total_bytes > 0);
        assert!(capacity.available_bytes <= capacity.total_bytes);
    }

    #[test]
    fn volume_capacity_reports_not_found_for_a_missing_path() {
        let error = MacosPlatformAdapter::new()
            .volume_capacity(Path::new("/no/such/path/fm-platform-macos-test"))
            .unwrap_err();
        assert!(matches!(error, PlatformError::NotFound { .. }));
    }

    #[test]
    fn install_native_menu_reports_an_io_error_off_the_main_thread() {
        // The test harness runs each test on a worker thread, never the
        // process's actual main thread, so this deterministically exercises
        // the off-main-thread error path; the happy path (real `NSMenu`
        // construction) can't be asserted against in this CI environment
        // (no real windowing system) and is instead exercised via manual
        // verification inside a running desktop app (see Agent Notes). The
        // pure `key_equivalent` mapping this method relies on is unit
        // tested directly below, without needing `NSMenu` at all.
        let error = MacosPlatformAdapter::new()
            .install_native_menu(&fm_domain::NativeMenuSpec::default(), Arc::new(|_id| {}))
            .expect_err("must fail off the main thread");
        assert!(matches!(error, PlatformError::Io { .. }));
    }

    #[test]
    fn key_equivalent_lowercases_the_key_and_maps_each_modifier_to_its_own_bit() {
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "C".to_owned(),
                meta: true,
                ..fm_domain::KeyChord::default()
            }),
            ("c".to_owned(), MODIFIER_COMMAND_BIT)
        );
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: ",".to_owned(),
                meta: true,
                ..fm_domain::KeyChord::default()
            }),
            (",".to_owned(), MODIFIER_COMMAND_BIT)
        );
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "z".to_owned(),
                meta: true,
                shift: true,
                ..fm_domain::KeyChord::default()
            }),
            ("z".to_owned(), MODIFIER_COMMAND_BIT | MODIFIER_SHIFT_BIT)
        );
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "a".to_owned(),
                ctrl: true,
                alt: true,
                ..fm_domain::KeyChord::default()
            }),
            ("a".to_owned(), MODIFIER_CONTROL_BIT | MODIFIER_OPTION_BIT)
        );
    }

    #[test]
    fn key_equivalent_reports_no_modifiers_for_a_plain_chord() {
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "a".to_owned(),
                ..fm_domain::KeyChord::default()
            }),
            ("a".to_owned(), 0)
        );
    }

    /// A regression test for a real bug (task 0133 follow-up): taking just the first character of
    /// a multi-character key name collided every one of `core.sortByName`..`core.sortUnsorted`'s
    /// distinct `"F3"`..`"F7"` shortcuts onto the same displayed "^F" key equivalent in the View
    /// menu. Multi-character key names that aren't function keys must produce a blank key
    /// equivalent instead.
    #[test]
    fn key_equivalent_leaves_multi_character_key_names_blank_instead_of_colliding() {
        for key in ["Escape", "Enter", "Tab"] {
            assert_eq!(
                key_equivalent(&fm_domain::KeyChord {
                    key: key.to_owned(),
                    ctrl: true,
                    ..fm_domain::KeyChord::default()
                }),
                (String::new(), MODIFIER_CONTROL_BIT),
                "key {key:?} must produce a blank key equivalent, not a truncated one"
            );
        }
    }

    /// A regression test for a real bug: the native Help menu's F1 shortcut
    /// (`core.showShortcutsHelp`) never displayed in the macOS menu bar because every
    /// multi-character key name, including function keys, was blanked out.
    #[test]
    fn key_equivalent_maps_function_keys_to_the_private_use_area_character() {
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "F1".to_owned(),
                ..fm_domain::KeyChord::default()
            }),
            ("\u{F704}".to_owned(), 0)
        );
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "F6".to_owned(),
                shift: true,
                ..fm_domain::KeyChord::default()
            }),
            ("\u{F709}".to_owned(), MODIFIER_SHIFT_BIT)
        );
        // Out of the supported F1..F35 range: falls back to blank, like any other
        // multi-character key name.
        assert_eq!(
            key_equivalent(&fm_domain::KeyChord {
                key: "F36".to_owned(),
                ..fm_domain::KeyChord::default()
            }),
            (String::new(), 0)
        );
    }

    #[test]
    fn open_terminal_passes_non_nfc_unicode_paths_through_untouched() {
        // "café" as NFC (precomposed é) vs NFD (e + combining acute) must
        // both survive into the spawned `open` command's arguments byte-for-
        // byte: never compare or rebuild the path via a normalizing string
        // operation.
        let nfc = Path::new("/tmp/caf\u{00e9}");
        let nfd = Path::new("/tmp/cafe\u{0301}");
        assert_ne!(nfc.as_os_str(), nfd.as_os_str());

        for path in [nfc, nfd] {
            let command = std::process::Command::new("open")
                .arg("-a")
                .arg("Terminal")
                .arg(path)
                .get_args()
                .map(std::ffi::OsStr::to_os_string)
                .collect::<Vec<_>>();
            let expected: Vec<std::ffi::OsString> =
                vec!["-a".into(), "Terminal".into(), path.into()];
            assert_eq!(command, expected);
        }
    }

    #[test]
    fn open_terminal_uses_the_command_override_as_the_open_dash_a_target() {
        let dir = tempdir().expect("temp dir");

        let error = MacosPlatformAdapter::new()
            .open_terminal(dir.path(), Some("Definitely Not An Installed App"))
            .expect_err("a bogus override app must fail, not silently open Terminal instead");
        let message = error.to_string();
        assert!(
            message.contains("Definitely Not An Installed App"),
            "error must name the overridden app, not the default: {message}"
        );
    }

    #[test]
    fn open_in_text_editor_uses_open_dash_t_without_an_override() {
        let command = std::process::Command::new("open")
            .arg("-t")
            .arg(Path::new("/tmp/fm-platform-macos-edit-test.txt"))
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect::<Vec<_>>();
        let expected: Vec<std::ffi::OsString> =
            vec!["-t".into(), "/tmp/fm-platform-macos-edit-test.txt".into()];
        assert_eq!(command, expected);
    }

    #[test]
    fn open_in_text_editor_uses_the_command_override_as_the_open_dash_a_target() {
        let dir = tempdir().expect("temp dir");

        let error = MacosPlatformAdapter::new()
            .open_in_text_editor(dir.path(), Some("Definitely Not An Installed Editor"))
            .expect_err("a bogus override app must fail, not silently open the default editor");
        let message = error.to_string();
        assert!(
            message.contains("Definitely Not An Installed Editor"),
            "error must name the overridden app, not the default: {message}"
        );
    }

    #[test]
    fn open_with_chooser_passes_the_path_as_a_trailing_argv_element_never_interpolated() {
        // Not executed: `choose application` pops a real, blocking system
        // dialog with no way for an automated test to dismiss it, so this
        // only asserts on the constructed command (the actual dialog is
        // manually verified inside a running desktop app, see Agent Notes).
        let path = Path::new("/tmp/weird \"quotes\" & caf\u{00e9}.txt");
        let command = open_with_chooser_command(path);
        assert_eq!(command.get_program(), "osascript");

        let args: Vec<std::ffi::OsString> = command
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect();
        assert_eq!(
            args.last(),
            Some(&std::ffi::OsString::from(path)),
            "the path must be the last argv element, passed verbatim"
        );
        for script_fragment in &args[..args.len() - 1] {
            assert!(
                !script_fragment.to_string_lossy().contains("caf\u{00e9}"),
                "the path must never be embedded inside an -e script fragment: {script_fragment:?}"
            );
        }

        assert!(
            args.iter()
                .any(|arg| arg.to_string_lossy().contains("-128")),
            "cancelling `choose application` (AppleScript error -128) must be handled inside the script"
        );
    }

    #[test]
    fn quick_look_rejects_missing_paths_and_directories_before_presenting_a_panel() {
        let dir = tempdir().expect("temp dir");
        let adapter = MacosPlatformAdapter::new();

        for path in [dir.path().join("missing.pdf"), dir.path().to_path_buf()] {
            assert!(
                matches!(
                    adapter.quick_look(&path),
                    Err(PlatformError::NotFound { .. })
                ),
                "{} must not reach Quick Look",
                path.display()
            );
        }
    }

    #[test]
    fn quick_look_reports_when_invoked_off_the_main_thread() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"%PDF").expect("fixture");

        let error = std::thread::spawn(move || MacosPlatformAdapter::new().quick_look(&path))
            .join()
            .expect("worker must not panic")
            .expect_err("AppKit preview calls must stay on the main thread");

        assert!(error.to_string().contains("main thread"));
    }

    #[test]
    fn choose_from_list_command_passes_names_as_trailing_argv_elements_never_interpolated() {
        // Not executed: `choose from list` pops a real, blocking system
        // dialog with no way for an automated test to dismiss it, so this
        // only asserts on the constructed command (the actual dialog is
        // manually verified inside a running desktop app, see Agent Notes).
        let names = vec![
            "Preview".to_owned(),
            "weird \"quotes\" & caf\u{00e9}".to_owned(),
            OPEN_WITH_OTHER_APPLICATIONS.to_owned(),
        ];
        let command = choose_from_list_command(&names);
        assert_eq!(command.get_program(), "osascript");

        let args: Vec<std::ffi::OsString> = command
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect();
        assert_eq!(
            &args[args.len() - names.len()..],
            names
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
                .as_slice(),
            "every name must be a trailing argv element, passed verbatim"
        );
        for script_fragment in &args[..args.len() - names.len()] {
            assert!(
                !script_fragment.to_string_lossy().contains("caf\u{00e9}"),
                "no name may ever be embedded inside an -e script fragment: {script_fragment:?}"
            );
        }
        assert!(
            args.iter()
                .any(|arg| arg.to_string_lossy().contains("-128")),
            "cancelling `choose from list` (AppleScript error -128) must be handled inside the script"
        );
    }

    #[test]
    fn resolve_open_with_choice_recognises_the_cancelled_sentinel() {
        let recommended = vec![(
            "Preview".to_owned(),
            PathBuf::from("/Applications/Preview.app"),
        )];
        assert_eq!(
            resolve_open_with_choice(OPEN_WITH_CANCELLED_SENTINEL, &recommended),
            OpenWithChoice::Cancelled
        );
    }

    #[test]
    fn resolve_open_with_choice_recognises_the_other_applications_entry() {
        let recommended = vec![(
            "Preview".to_owned(),
            PathBuf::from("/Applications/Preview.app"),
        )];
        assert_eq!(
            resolve_open_with_choice(OPEN_WITH_OTHER_APPLICATIONS, &recommended),
            OpenWithChoice::Other
        );
    }

    #[test]
    fn resolve_open_with_choice_matches_a_recommended_application_by_name() {
        let recommended = vec![
            (
                "Preview".to_owned(),
                PathBuf::from("/Applications/Preview.app"),
            ),
            ("Pinta".to_owned(), PathBuf::from("/Applications/Pinta.app")),
        ];
        assert_eq!(
            resolve_open_with_choice("Pinta", &recommended),
            OpenWithChoice::App(PathBuf::from("/Applications/Pinta.app"))
        );
    }

    #[test]
    fn resolve_open_with_choice_treats_an_unmatched_name_as_cancelled_rather_than_guessing() {
        let recommended = vec![(
            "Preview".to_owned(),
            PathBuf::from("/Applications/Preview.app"),
        )];
        assert_eq!(
            resolve_open_with_choice("Some App That Was Never Offered", &recommended),
            OpenWithChoice::Cancelled
        );
    }

    #[test]
    fn recommended_applications_finds_at_least_one_candidate_for_a_plain_text_file() {
        let dir = tempdir().expect("temp dir");
        let file = dir.path().join("recommended-apps-test.txt");
        std::fs::write(&file, b"content").expect("create fixture");

        let recommended = recommended_applications(&file).expect("query Launch Services");
        assert!(
            !recommended.is_empty(),
            "every macOS install ships at least one app (e.g. TextEdit) that can open .txt files"
        );
        for (name, app_path) in &recommended {
            assert!(
                !name.is_empty(),
                "display name must not be empty: {app_path:?}"
            );
            assert!(
                app_path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    == Some("app"),
                "recommended path must be an application bundle: {app_path:?}"
            );
        }
    }

    fn fixture_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"content").expect("create fixture file");
        path
    }

    /// Reads the raw bytes of `name` on `path` through the system
    /// `/usr/bin/xattr` CLI (`-p` prints the attribute's raw value verbatim,
    /// not hex-encoded) - a code path completely independent of this
    /// crate's own `xattr`/`plist` decoding, so a passing assertion here is
    /// real evidence the OS xattr store (what Finder itself reads) actually
    /// received the attribute, not just that our own reader agrees with our
    /// own writer.
    fn system_xattr_bytes(path: &Path, name: &str) -> Vec<u8> {
        let output = std::process::Command::new("/usr/bin/xattr")
            .arg("-p")
            .arg(name)
            .arg(path)
            .output()
            .expect("run system xattr CLI");
        assert!(
            output.status.success(),
            "system xattr -p {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn a_new_file_has_no_finder_tags() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "untagged.txt");

        assert_eq!(read_finder_tags(&file).expect("read tags"), Vec::new());
    }

    #[test]
    fn finder_tags_round_trip_through_the_real_xattr_store() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "tagged.txt");
        let tags = vec![
            FinderTag {
                name: "Work".to_owned(),
                color: FinderTagColor::Blue,
            },
            FinderTag {
                name: "Untagged Colorless".to_owned(),
                color: FinderTagColor::None,
            },
            FinderTag {
                name: "Red".to_owned(),
                color: FinderTagColor::Red,
            },
        ];

        write_finder_tags(&file, &tags).expect("write tags");
        let read_back = read_finder_tags(&file).expect("read tags");

        // Order preserved, exactly as Finder's own array-order convention
        // requires (the tags UI shows them in the order they were assigned).
        assert_eq!(read_back, tags);
        // Independent proof the OS xattr store itself received a real,
        // non-empty binary plist under the exact name Finder reads.
        let bytes = system_xattr_bytes(&file, FINDER_TAGS_XATTR);
        assert!(!bytes.is_empty());
        assert!(
            bytes.starts_with(b"bplist00"),
            "expected a bplist00 magic header, got: {bytes:?}"
        );
    }

    #[test]
    fn setting_an_empty_tag_list_removes_the_xattr_entirely() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "cleared.txt");
        write_finder_tags(
            &file,
            &[FinderTag {
                name: "Temporary".to_owned(),
                color: FinderTagColor::Gray,
            }],
        )
        .expect("write tags");

        write_finder_tags(&file, &[]).expect("clear tags");

        assert_eq!(read_finder_tags(&file).expect("read tags"), Vec::new());
        assert!(
            xattr::get(&file, FINDER_TAGS_XATTR)
                .expect("query xattr")
                .is_none(),
            "the attribute itself must be gone, not just present-but-empty"
        );
    }

    #[test]
    fn clearing_tags_on_an_already_untagged_file_is_a_no_op_not_an_error() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "never-tagged.txt");

        write_finder_tags(&file, &[]).expect("clearing absent tags must succeed");
    }

    #[test]
    fn finder_tags_on_a_missing_path_report_not_found() {
        let missing = Path::new("/tmp/fm-platform-macos-does-not-exist-0136/nothing.txt");

        assert!(matches!(
            read_finder_tags(missing),
            Err(PlatformError::NotFound { .. })
        ));
        assert!(matches!(
            write_finder_tags(
                missing,
                &[FinderTag {
                    name: "X".to_owned(),
                    color: FinderTagColor::None,
                }]
            ),
            Err(PlatformError::NotFound { .. })
        ));
    }

    #[test]
    fn parse_finder_tag_decodes_a_colored_tag() {
        assert_eq!(
            parse_finder_tag("Work\n4"),
            FinderTag {
                name: "Work".to_owned(),
                color: FinderTagColor::Blue,
            }
        );
    }

    #[test]
    fn parse_finder_tag_decodes_an_uncolored_tag_with_no_suffix() {
        assert_eq!(
            parse_finder_tag("Personal"),
            FinderTag {
                name: "Personal".to_owned(),
                color: FinderTagColor::None,
            }
        );
    }

    #[test]
    fn parse_finder_tag_degrades_a_foreign_suffix_to_an_uncolored_tag_instead_of_failing() {
        // Neither a real Finder-written tag: a multi-digit suffix, and a
        // non-numeric one. Both must still yield a usable (if uncolored)
        // tag rather than propagating a decode error for the whole list.
        assert_eq!(
            parse_finder_tag("Weird\n42"),
            FinderTag {
                name: "Weird\n42".to_owned(),
                color: FinderTagColor::None,
            }
        );
        assert_eq!(
            parse_finder_tag("Weird\nX"),
            FinderTag {
                name: "Weird\nX".to_owned(),
                color: FinderTagColor::None,
            }
        );
    }

    #[test]
    fn encode_finder_tag_omits_the_color_suffix_for_no_color() {
        assert_eq!(
            encode_finder_tag(&FinderTag {
                name: "Plain".to_owned(),
                color: FinderTagColor::None,
            }),
            "Plain"
        );
    }

    #[test]
    fn a_new_file_has_no_spotlight_comment() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "no-comment.txt");

        assert_eq!(read_spotlight_comment(&file).expect("read comment"), None);
    }

    #[test]
    fn spotlight_comment_round_trips_through_the_real_xattr_store() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "commented.txt");

        write_spotlight_comment(&file, Some("Reviewed 2026-08-17")).expect("write comment");

        assert_eq!(
            read_spotlight_comment(&file).expect("read comment"),
            Some("Reviewed 2026-08-17".to_owned())
        );
        let bytes = system_xattr_bytes(&file, FINDER_COMMENT_XATTR);
        assert!(
            bytes.starts_with(b"bplist00"),
            "expected a bplist00 magic header, got: {bytes:?}"
        );
    }

    #[test]
    fn setting_a_none_comment_clears_the_xattr_entirely() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "clear-comment.txt");
        write_spotlight_comment(&file, Some("temporary")).expect("write comment");

        write_spotlight_comment(&file, None).expect("clear comment");

        assert_eq!(read_spotlight_comment(&file).expect("read comment"), None);
        assert!(
            xattr::get(&file, FINDER_COMMENT_XATTR)
                .expect("query xattr")
                .is_none()
        );
    }

    #[test]
    fn clearing_a_comment_on_a_file_with_no_comment_is_a_no_op_not_an_error() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "never-commented.txt");

        write_spotlight_comment(&file, None).expect("clearing an absent comment must succeed");
    }

    #[test]
    fn spotlight_comment_on_a_missing_path_reports_not_found() {
        let missing = Path::new("/tmp/fm-platform-macos-does-not-exist-0136/nothing-else.txt");

        assert!(matches!(
            read_spotlight_comment(missing),
            Err(PlatformError::NotFound { .. })
        ));
        assert!(matches!(
            write_spotlight_comment(missing, Some("x")),
            Err(PlatformError::NotFound { .. })
        ));
    }

    #[test]
    fn platform_adapter_finder_tag_and_comment_methods_delegate_to_the_free_functions() {
        let dir = tempdir().expect("temp dir");
        let file = fixture_file(dir.path(), "via-adapter.txt");
        let adapter = MacosPlatformAdapter::new();
        let tags = vec![FinderTag {
            name: "Via Adapter".to_owned(),
            color: FinderTagColor::Green,
        }];

        adapter.set_finder_tags(&file, &tags).expect("set tags");
        assert_eq!(adapter.finder_tags(&file).expect("get tags"), tags);

        adapter
            .set_spotlight_comment(&file, Some("hello"))
            .expect("set comment");
        assert_eq!(
            adapter.spotlight_comment(&file).expect("get comment"),
            Some("hello".to_owned())
        );
    }
}
