//! Desktop-only update channel status for the Settings update controls.
//!
//! The updater plugin's JavaScript `check()` returns nothing when the build is
//! current, so this command reads the same signed endpoint configured for this
//! build and reports the version it offers.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::UpdaterExt;

/// Returns the version offered by this build's configured update endpoint.
#[tauri::command]
pub(crate) async fn get_app_update_channel_version<R: Runtime>(
    app: AppHandle<R>,
) -> Result<String, String> {
    let offered = Arc::new(Mutex::new(None::<String>));
    let captured = Arc::clone(&offered);
    let updater = app
        .updater_builder()
        .version_comparator(move |current, release| {
            let newer = release.version > current;
            if let Ok(mut version) = captured.lock() {
                *version = Some(release.version.to_string());
            }
            newer
        })
        .build()
        .map_err(|error| error.to_string())?;
    updater.check().await.map_err(|error| error.to_string())?;
    let version = offered
        .lock()
        .map_err(|_| "update channel status is unavailable".to_owned())?
        .clone();
    version.ok_or_else(|| "the update channel did not report a version".to_owned())
}
