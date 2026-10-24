//! Where Procyon keeps its per-user data, plus cleanup of the directories
//! earlier builds used under the former `fm` names.
//!
//! Everything lives under a directory named `procyon` inside the platform
//! config, data and cache directories (settings, workspaces, semantic index
//! data, logs and editor locks). The names below are the only ones allowed;
//! [`remove_legacy_data`] migrates and deletes the retired ones.

use std::io;
use std::path::{Path, PathBuf};

/// Directory name used inside each platform base directory.
pub const APP_DIR_NAME: &str = "procyon";

/// Names earlier builds stored data under. `fm` held what is now `procyon`;
/// the others were bundle-identifier or binary-name derived caches.
const LEGACY_DIR_NAMES: [&str; 3] = ["fm", "fm-desktop", "dev.fm.desktop"];

/// Extra macOS per-bundle-identifier locations (relative to `~/Library`) that
/// the legacy identifier left behind.
const LEGACY_MACOS_LIBRARY_ENTRIES: [&str; 5] = [
    "WebKit/dev.fm.desktop",
    "HTTPStorages/dev.fm.desktop",
    "Preferences/dev.fm.desktop.plist",
    "Saved Application State/dev.fm.desktop.savedState",
    "Application Support/dev.fm.desktop",
];

/// macOS per-bundle-identifier locations (relative to `~/Library`).
const IDENTIFIER_LIBRARY_ENTRIES: [&str; 6] = [
    "Application Support/nl.erikvullings.procyon",
    "Caches/nl.erikvullings.procyon",
    "HTTPStorages/nl.erikvullings.procyon",
    "Preferences/nl.erikvullings.procyon.plist",
    "Saved Application State/nl.erikvullings.procyon.savedState",
    "WebKit/nl.erikvullings.procyon",
];

/// Platform base directories that hold an app directory.
#[derive(Debug, Clone, Default)]
pub struct DataBases {
    /// Settings and workspaces (`dirs::config_dir`).
    pub config: Option<PathBuf>,
    /// Logs and other data (`dirs::data_dir`).
    pub data: Option<PathBuf>,
    /// Caches such as editor locks (`dirs::cache_dir`).
    pub cache: Option<PathBuf>,
    /// macOS `~/Library`, for per-identifier leftovers.
    pub macos_library: Option<PathBuf>,
}

impl DataBases {
    /// The current user's platform directories.
    #[must_use]
    pub fn platform() -> Self {
        Self {
            config: dirs::config_dir(),
            data: dirs::data_dir(),
            cache: dirs::cache_dir(),
            macos_library: if cfg!(target_os = "macos") {
                dirs::home_dir().map(|home| home.join("Library"))
            } else {
                None
            },
        }
    }

    fn all(&self) -> impl Iterator<Item = &PathBuf> {
        [&self.config, &self.data, &self.cache]
            .into_iter()
            .flatten()
    }

    /// Every `procyon` directory that may hold user data, deduplicated
    /// (config and data are the same directory on macOS).
    #[must_use]
    pub fn app_directories(&self) -> Vec<PathBuf> {
        let mut directories: Vec<PathBuf> = Vec::new();
        for base in self.all() {
            let directory = base.join(APP_DIR_NAME);
            if !directories.contains(&directory) {
                directories.push(directory);
            }
        }
        directories
    }
}

/// Moves data from the retired `fm` directory into `procyon` (never
/// overwriting existing files) and deletes every retired directory.
///
/// Best effort per entry: the first I/O error is returned after all entries
/// were attempted, so one locked file does not block the rest.
pub fn remove_legacy_data(bases: &DataBases) -> io::Result<()> {
    let mut first_error = None;
    let mut record = |result: io::Result<()>| {
        if let Err(error) = result {
            first_error.get_or_insert(error);
        }
    };
    for base in bases.all() {
        let legacy_fm = base.join(LEGACY_DIR_NAMES[0]);
        if legacy_fm.is_dir() {
            record(merge_without_overwrite(
                &legacy_fm,
                &base.join(APP_DIR_NAME),
            ));
        }
        for name in LEGACY_DIR_NAMES {
            record(remove_path(&base.join(name)));
        }
    }
    if let Some(library) = &bases.macos_library {
        for entry in LEGACY_MACOS_LIBRARY_ENTRIES {
            record(remove_path(&library.join(entry)));
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Everything the uninstall flow deletes: the `procyon` directories plus, on
/// macOS, the per-bundle-identifier entries the web view and OS create.
#[must_use]
pub fn uninstall_paths(bases: &DataBases) -> Vec<PathBuf> {
    let mut paths = bases.app_directories();
    if let Some(library) = &bases.macos_library {
        paths.extend(
            IDENTIFIER_LIBRARY_ENTRIES
                .iter()
                .map(|entry| library.join(entry)),
        );
    }
    paths
}

fn remove_path(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Moves entries of `from` into `to`, skipping names that already exist
/// there (recursing into directories present on both sides). Symlinks are
/// left behind and removed with the legacy directory, never followed.
fn merge_without_overwrite(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if !target.exists() {
            std::fs::rename(entry.path(), &target)?;
        } else if file_type.is_dir() && target.is_dir() {
            merge_without_overwrite(&entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases(root: &Path) -> DataBases {
        DataBases {
            config: Some(root.join("config")),
            data: Some(root.join("data")),
            cache: Some(root.join("cache")),
            macos_library: Some(root.join("Library")),
        }
    }

    #[test]
    fn legacy_fm_data_moves_into_procyon_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        std::fs::create_dir_all(config.join("fm/connections")).unwrap();
        std::fs::create_dir_all(config.join("procyon")).unwrap();
        std::fs::write(config.join("fm/settings.json"), "old").unwrap();
        std::fs::write(config.join("fm/connections/a.json"), "conn").unwrap();
        std::fs::write(config.join("procyon/settings.json"), "new").unwrap();

        remove_legacy_data(&bases(root.path())).unwrap();

        assert!(!config.join("fm").exists());
        assert_eq!(
            std::fs::read_to_string(config.join("procyon/settings.json")).unwrap(),
            "new"
        );
        assert_eq!(
            std::fs::read_to_string(config.join("procyon/connections/a.json")).unwrap(),
            "conn"
        );
    }

    #[test]
    fn retired_directories_are_removed_everywhere() {
        let root = tempfile::tempdir().unwrap();
        for dir in [
            "cache/fm-desktop",
            "data/dev.fm.desktop",
            "Library/WebKit/dev.fm.desktop",
        ] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        std::fs::create_dir_all(root.path().join("Library/Preferences")).unwrap();
        std::fs::write(
            root.path().join("Library/Preferences/dev.fm.desktop.plist"),
            "",
        )
        .unwrap();

        remove_legacy_data(&bases(root.path())).unwrap();

        for dir in [
            "cache/fm-desktop",
            "data/dev.fm.desktop",
            "Library/WebKit/dev.fm.desktop",
            "Library/Preferences/dev.fm.desktop.plist",
        ] {
            assert!(!root.path().join(dir).exists(), "{dir}");
        }
    }

    #[test]
    fn uninstall_paths_cover_procyon_directories_and_macos_identifier_entries() {
        let root = tempfile::tempdir().unwrap();
        let paths = uninstall_paths(&bases(root.path()));

        for expected in ["config/procyon", "data/procyon", "cache/procyon"] {
            assert!(paths.contains(&root.path().join(expected)), "{expected}");
        }
        assert!(paths.contains(&root.path().join("Library/WebKit/nl.erikvullings.procyon")));
    }
}
