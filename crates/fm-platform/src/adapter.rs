use std::path::{Path, PathBuf};

use crate::{
    ApplicationUninstallPlan, FinderTag, MountedVolume, PlatformCapabilities, PlatformError,
    SystemLocation, VolumeCapacity,
};

/// Native OS integrations the application calls into: file icons,
/// thumbnails, revealing entries in the system file manager, trash, opening
/// with the default application, opening a terminal, system clipboard file
/// references, mounted volumes/drives and native menus (specification §23).
///
/// Every method has a default implementation reporting its capability as
/// unsupported, so a concrete adapter only needs to override the methods it
/// actually implements. [`PlatformAdapter::capabilities`] must stay in sync
/// with the overridden methods, so unsupported functions are always reported
/// as `false` and their UI affordances can be hidden or disabled rather than
/// left present-but-broken.
///
/// Methods are synchronous: native OS calls are blocking. Callers running
/// inside an async runtime must invoke them through `spawn_blocking` rather
/// than awaiting them directly, so a native call never blocks the Tauri UI
/// thread (specification §28).
pub trait PlatformAdapter: Send + Sync + std::any::Any {
    /// Discovers currently reachable OS-managed locations.
    fn system_locations(&self) -> Result<Vec<SystemLocation>, PlatformError> {
        Ok(Vec::new())
    }
    /// Reports which capabilities this adapter actually implements.
    fn capabilities(&self) -> PlatformCapabilities;

    /// Fetches a file's native icon, encoded as PNG bytes.
    fn file_icon(&self, path: &Path) -> Result<Vec<u8>, PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::FILE_ICONS,
        })
    }

    /// Fetches a native thumbnail preview, encoded as PNG bytes, no larger
    /// than `max_size` pixels on its longest side.
    fn thumbnail(&self, path: &Path, max_size: u32) -> Result<Vec<u8>, PlatformError> {
        let _ = (path, max_size);
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::THUMBNAILS,
        })
    }

    /// Reveals an entry in the system file manager (Finder/Explorer/...).
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::REVEAL_IN_FILE_MANAGER,
        })
    }

    /// Moves an entry to the system trash/recycle bin.
    fn trash(&self, path: &Path) -> Result<(), PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::TRASH,
        })
    }

    /// Moves an entry to trash and returns its restorable native location when the platform API
    /// exposes one.
    fn trash_with_restore_location(&self, path: &Path) -> Result<Option<PathBuf>, PlatformError> {
        self.trash(path)?;
        Ok(None)
    }

    /// Restores a previously trashed entry without replacing anything at its original path.
    fn restore_from_trash(
        &self,
        trashed_path: &Path,
        original_path: &Path,
    ) -> Result<(), PlatformError> {
        if original_path.exists() {
            return Err(PlatformError::Io {
                message: "the original path is occupied".into(),
            });
        }
        std::fs::rename(trashed_path, original_path).map_err(|_| PlatformError::Io {
            message: "the trash item could not be restored".into(),
        })
    }

    /// Opens an entry with the OS default application.
    fn open_with_default_application(&self, path: &Path) -> Result<(), PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION,
        })
    }

    /// Opens a terminal at a location.
    ///
    /// `command_override` is the configured terminal setting (specification
    /// §26), e.g. an application or executable name; `None` means use this
    /// adapter's sensible platform default (e.g. `Terminal` on macOS).
    fn open_terminal(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        let _ = (path, command_override);
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::OPEN_TERMINAL,
        })
    }

    /// Opens an entry in a text editor (task 0086), rather than its OS
    /// default application - e.g. opening a `.jpg` should still open a
    /// text/hex editor, not an image viewer.
    ///
    /// `command_override` is the configured editor setting (specification
    /// §26); `None` falls back to this adapter's default
    /// [`PlatformAdapter::open_with_default_application`] (a documented gap
    /// for adapters with no distinct text-editor association, not a silent
    /// over-claim - see `fm-application`'s `core_actions` doc comment).
    fn open_in_text_editor(
        &self,
        path: &Path,
        command_override: Option<&str>,
    ) -> Result<(), PlatformError> {
        let _ = command_override;
        self.open_with_default_application(path)
    }

    /// Shows the OS's native "Open With\u2026" application chooser for an
    /// entry (task 0061 follow-up), rather than silently opening it with the
    /// default application. Cancelling the chooser must be treated as a
    /// no-op, not an error.
    ///
    /// Falls back to [`PlatformAdapter::open_with_default_application`] for
    /// adapters with no native chooser (a documented gap, not a silent
    /// over-claim - see `fm-application`'s `core_actions` doc comment).
    fn open_with_chooser(&self, path: &Path) -> Result<(), PlatformError> {
        self.open_with_default_application(path)
    }

    /// Presents a local file through the OS Quick Look preview panel.
    fn quick_look(&self, path: &Path) -> Result<(), PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::QUICK_LOOK,
        })
    }

    /// Reads the file paths currently referenced on the OS clipboard.
    fn read_clipboard_file_references(&self) -> Result<Vec<PathBuf>, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::CLIPBOARD_FILE_REFERENCES,
        })
    }

    /// Writes file paths to the OS clipboard as file references.
    fn write_clipboard_file_references(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        let _ = paths;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::CLIPBOARD_FILE_REFERENCES,
        })
    }

    /// Lists currently mounted volumes/drives.
    fn mounted_volumes(&self) -> Result<Vec<MountedVolume>, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::MOUNTED_VOLUMES,
        })
    }

    /// Reports total/available capacity for the volume containing `path`
    /// (task 0096), used to render a Marta/Finder-style status bar segment.
    fn volume_capacity(&self, path: &Path) -> Result<VolumeCapacity, PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::VOLUME_CAPACITY,
        })
    }

    /// Reads an entry's Finder tags (task 0136), in the order Finder itself
    /// stores them. An entry with no tags returns an empty `Vec`, not an
    /// error.
    fn finder_tags(&self, path: &Path) -> Result<Vec<FinderTag>, PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::FINDER_TAGS,
        })
    }

    /// Replaces an entry's complete set of Finder tags (task 0136) - the
    /// same all-at-once semantics Finder's own tag editor uses, so a caller
    /// that wants to add or remove a single tag reads the current set with
    /// [`PlatformAdapter::finder_tags`] first. An empty slice removes every
    /// tag.
    fn set_finder_tags(&self, path: &Path, tags: &[FinderTag]) -> Result<(), PlatformError> {
        let _ = (path, tags);
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::FINDER_TAGS,
        })
    }

    /// Reads an entry's Spotlight comment (task 0136, `kMDItemFinderComment`).
    /// `None` means no comment is set, not an error.
    fn spotlight_comment(&self, path: &Path) -> Result<Option<String>, PlatformError> {
        let _ = path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::EXTENDED_ATTRIBUTES,
        })
    }

    /// Sets or clears (`None`) an entry's Spotlight comment (task 0136).
    fn set_spotlight_comment(
        &self,
        path: &Path,
        comment: Option<&str>,
    ) -> Result<(), PlatformError> {
        let _ = (path, comment);
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::EXTENDED_ATTRIBUTES,
        })
    }

    /// Installs the application's native menu bar (task 0133), replacing
    /// whatever menu is currently installed.
    ///
    /// `on_action` is invoked (on the main thread) whenever the user clicks
    /// an [`fm_domain::NativeMenuItem::Action`] item, with that item's
    /// action-registry id - the same id the caller would dispatch through
    /// `fm-application`'s action registry for a matching keyboard shortcut,
    /// so a menu click and its shortcut share one code path rather than
    /// diverging. [`fm_domain::NativeMenuItem::Role`] items have no
    /// application callback: the adapter wires them directly to the
    /// matching native OS selector instead.
    fn install_native_menu(
        &self,
        spec: &fm_domain::NativeMenuSpec,
        on_action: std::sync::Arc<dyn Fn(String) + Send + Sync>,
    ) -> Result<(), PlatformError> {
        let _ = (spec, on_action);
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::NATIVE_MENUS,
        })
    }

    /// Reads a `.app` bundle's identity and scans task 0148's well-known
    /// macOS locations for related files (preferences, caches,
    /// application-support data, saved application state, launch agents,
    /// logs), for the user to review before anything is deleted. Nothing
    /// outside those well-known locations is ever touched, and nothing is
    /// deleted by this call itself - it only plans.
    fn plan_application_uninstall(
        &self,
        bundle_path: &Path,
    ) -> Result<ApplicationUninstallPlan, PlatformError> {
        let _ = bundle_path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::APPLICATION_UNINSTALL,
        })
    }

    /// Removes `bundle_path`'s Dock icon, if the user had pinned one (task
    /// 0148 follow-up), so uninstalling an app doesn't leave a dangling icon
    /// pointing at a trashed bundle. Returns `false`, not an error, when
    /// there simply was no pinned icon to remove - only a genuine failure to
    /// read/write the platform's Dock state is an error.
    fn remove_application_dock_icon(&self, bundle_path: &Path) -> Result<bool, PlatformError> {
        let _ = bundle_path;
        Err(PlatformError::Unsupported {
            capability: PlatformCapabilities::APPLICATION_UNINSTALL,
        })
    }
}
