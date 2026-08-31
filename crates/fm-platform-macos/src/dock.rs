//! Dock icon cleanup for the macOS application uninstaller (task 0148 follow-up).
//!
//! Uninstalling an app through this feature moves its `.app` bundle to the Trash, but a Dock icon
//! the user had pinned for it (`~/Library/Preferences/com.apple.dock.plist`'s `persistent-apps`
//! array) is otherwise left behind pointing at a now-missing bundle. This module removes that one
//! matching entry, if any, and restarts the Dock so the change is visible immediately - it never
//! touches any other `persistent-apps` entry.

use std::path::Path;

use fm_platform::PlatformError;

/// Removes `bundle_path`'s Dock icon, if the user had pinned one, and restarts the Dock so the
/// change takes effect immediately. Returns `false` (never an error) when there is no Dock
/// preferences file, it can't be parsed, or no entry matches - a missing or unpinned icon is an
/// expected, harmless outcome, not a failure of the uninstall itself.
pub(crate) fn remove_dock_icon(bundle_path: &Path) -> Result<bool, PlatformError> {
    let home = dirs::home_dir().ok_or_else(|| PlatformError::Io {
        message: "home directory is unavailable".to_owned(),
    })?;
    let dock_plist_path = home.join("Library/Preferences/com.apple.dock.plist");
    let Ok(value) = plist::Value::from_file(&dock_plist_path) else {
        return Ok(false);
    };
    let Ok(target_location) = fm_domain::Location::from_native_path(bundle_path) else {
        return Ok(false);
    };

    let (updated, removed) = remove_dock_entry(value, &target_location.uri);
    if !removed {
        return Ok(false);
    }
    updated
        .to_file_binary(&dock_plist_path)
        .map_err(|error| PlatformError::Io {
            message: format!("failed to update Dock preferences: {error}"),
        })?;
    restart_dock();
    Ok(true)
}

/// Restarts the Dock (macOS relaunches it automatically) so a preferences change is picked up
/// immediately. Best-effort: a failure here (e.g. the Dock isn't running, as in a test/CI
/// environment) must not fail the uninstall - the plist has already been updated correctly and
/// will simply take effect the next time the Dock does start.
fn restart_dock() {
    let _ = std::process::Command::new("killall").arg("Dock").status();
}

/// Removes any `persistent-apps` entry from `dock_plist` whose file URL matches `target_url`,
/// returning the (possibly updated) value and whether anything was removed. Pure and
/// filesystem-free, so it's testable without a real Dock preferences file.
fn remove_dock_entry(dock_plist: plist::Value, target_url: &str) -> (plist::Value, bool) {
    let plist::Value::Dictionary(mut dict) = dock_plist else {
        return (dock_plist, false);
    };
    let Some(plist::Value::Array(apps)) = dict.get("persistent-apps").cloned() else {
        return (plist::Value::Dictionary(dict), false);
    };
    let original_len = apps.len();
    let filtered: Vec<plist::Value> = apps
        .into_iter()
        .filter(|entry| !entry_matches_url(entry, target_url))
        .collect();
    let removed = filtered.len() != original_len;
    if removed {
        dict.insert("persistent-apps".to_owned(), plist::Value::Array(filtered));
    }
    (plist::Value::Dictionary(dict), removed)
}

/// `true` when a `persistent-apps` entry's `tile-data.file-data._CFURLString` identifies the same
/// bundle as `target_url` - macOS appends a trailing slash for a directory URL where this
/// application's own `file://` URIs never do, so only that difference is normalized away; nothing
/// else about the comparison is loosened; an unrecognized entry shape never matches instead of
/// panicking.
fn entry_matches_url(entry: &plist::Value, target_url: &str) -> bool {
    let Some(url) = entry
        .as_dictionary()
        .and_then(|dict| dict.get("tile-data"))
        .and_then(plist::Value::as_dictionary)
        .and_then(|tile_data| tile_data.get("file-data"))
        .and_then(plist::Value::as_dictionary)
        .and_then(|file_data| file_data.get("_CFURLString"))
        .and_then(plist::Value::as_string)
    else {
        return false;
    };
    url.trim_end_matches('/') == target_url.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Value};

    fn dock_entry(url: &str) -> Value {
        let mut file_data = Dictionary::new();
        file_data.insert("_CFURLString".to_owned(), Value::String(url.to_owned()));
        file_data.insert("_CFURLStringType".to_owned(), Value::Integer(0.into()));
        let mut tile_data = Dictionary::new();
        tile_data.insert("file-data".to_owned(), Value::Dictionary(file_data));
        let mut entry = Dictionary::new();
        entry.insert("tile-data".to_owned(), Value::Dictionary(tile_data));
        entry.insert(
            "tile-type".to_owned(),
            Value::String("file-tile".to_owned()),
        );
        Value::Dictionary(entry)
    }

    fn dock_plist(persistent_apps: Vec<Value>) -> Value {
        let mut dict = Dictionary::new();
        dict.insert("persistent-apps".to_owned(), Value::Array(persistent_apps));
        Value::Dictionary(dict)
    }

    fn persistent_app_urls(value: &Value) -> Vec<String> {
        value
            .as_dictionary()
            .and_then(|dict| dict.get("persistent-apps"))
            .and_then(Value::as_array)
            .map(|apps| {
                apps.iter()
                    .filter_map(|entry| {
                        entry
                            .as_dictionary()?
                            .get("tile-data")?
                            .as_dictionary()?
                            .get("file-data")?
                            .as_dictionary()?
                            .get("_CFURLString")?
                            .as_string()
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn remove_dock_entry_removes_only_the_matching_entry() {
        let plist = dock_plist(vec![
            dock_entry("file:///Applications/Widget.app/"),
            dock_entry("file:///Applications/Other.app/"),
        ]);

        let (updated, removed) = remove_dock_entry(plist, "file:///Applications/Widget.app");

        assert!(removed);
        assert_eq!(
            persistent_app_urls(&updated),
            vec!["file:///Applications/Other.app/".to_owned()]
        );
    }

    #[test]
    fn remove_dock_entry_matches_despite_the_trailing_slash_macos_adds_for_directories() {
        let plist = dock_plist(vec![dock_entry("file:///Applications/Widget.app/")]);

        let (_updated, removed) = remove_dock_entry(plist, "file:///Applications/Widget.app");

        assert!(removed);
    }

    #[test]
    fn remove_dock_entry_leaves_the_plist_unchanged_when_nothing_matches() {
        let plist = dock_plist(vec![dock_entry("file:///Applications/Other.app/")]);

        let (updated, removed) = remove_dock_entry(plist, "file:///Applications/Widget.app");

        assert!(!removed);
        assert_eq!(
            persistent_app_urls(&updated),
            vec!["file:///Applications/Other.app/".to_owned()]
        );
    }

    #[test]
    fn remove_dock_entry_handles_a_plist_with_no_persistent_apps_key() {
        let plist = Value::Dictionary(Dictionary::new());

        let (_updated, removed) = remove_dock_entry(plist, "file:///Applications/Widget.app");

        assert!(!removed);
    }

    #[test]
    fn entry_matches_url_never_panics_on_an_unrecognized_entry_shape() {
        let malformed = Value::Dictionary(Dictionary::new());

        assert!(!entry_matches_url(
            &malformed,
            "file:///Applications/Widget.app"
        ));
    }
}
