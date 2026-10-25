//! Desktop application updates for the Settings update controls.
//!
//! Stable builds poll the endpoint configured in `tauri.conf.json`. Builds that opted in to beta
//! releases poll the beta endpoint instead; its manifest is a superset of the stable one (every
//! stable release is published to both), so a beta user never lags behind the stable channel.
//! The check keeps the offered update here so the install step downloads exactly what the
//! user was shown, whichever channel offered it.

use crate::command_failure::CommandFailure;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_updater::{Update, UpdaterBuilder, UpdaterExt};

/// Signed update manifest that carries beta releases as well as stable ones.
const BETA_ENDPOINT: &str =
    "https://github.com/erikvullings/procyon/releases/download/updater-beta/latest.json";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// The update offered by the last check, awaiting installation.
#[derive(Default)]
pub(crate) struct PendingAppUpdate(Mutex<Option<Update>>);

/// What the Settings UI shows about an offered update.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateMetadata {
    current_version: String,
    version: String,
    body: Option<String>,
}

/// Download progress, shaped like the updater plugin's own events.
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub(crate) enum AppUpdateDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Finished,
}

fn updater_builder<R: Runtime>(
    app: &AppHandle<R>,
    include_beta: bool,
) -> Result<UpdaterBuilder, CommandFailure> {
    let mut builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    if include_beta {
        let endpoint = BETA_ENDPOINT
            .parse::<tauri::Url>()
            .map_err(CommandFailure::platform)?;
        builder = builder
            .endpoints(vec![endpoint])
            .map_err(CommandFailure::platform)?;
    }
    Ok(builder)
}

/// Checks the stable (or, when opted in, the beta) channel and remembers any newer release.
#[tauri::command]
pub(crate) async fn check_app_update<R: Runtime>(
    app: AppHandle<R>,
    pending: State<'_, PendingAppUpdate>,
    include_beta: bool,
) -> Result<Option<AppUpdateMetadata>, CommandFailure> {
    let update = updater_builder(&app, include_beta)?
        .build()
        .map_err(CommandFailure::platform)?
        .check()
        .await
        .map_err(CommandFailure::platform)?;
    let metadata = update.as_ref().map(|update| AppUpdateMetadata {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        body: update.body.clone(),
    });
    *pending.0.lock().map_err(|_| poisoned())? = update;
    Ok(metadata)
}

/// Downloads and installs the update the last check offered, reporting progress on `on_event`.
#[tauri::command]
pub(crate) async fn install_app_update(
    pending: State<'_, PendingAppUpdate>,
    on_event: Channel<AppUpdateDownloadEvent>,
) -> Result<(), CommandFailure> {
    let mut update = pending
        .0
        .lock()
        .map_err(|_| poisoned())?
        .take()
        .ok_or_else(|| {
            CommandFailure::new(
                "appUpdateUnavailable",
                "The selected application update is no longer available.",
            )
        })?;
    // The check's short timeout must not cut off a long download.
    update.timeout = None;
    let mut first_chunk = true;
    let finished = on_event.clone();
    update
        .download_and_install(
            |chunk_length, content_length| {
                if std::mem::take(&mut first_chunk) {
                    let _ = on_event.send(AppUpdateDownloadEvent::Started { content_length });
                }
                let _ = on_event.send(AppUpdateDownloadEvent::Progress { chunk_length });
            },
            || {
                let _ = finished.send(AppUpdateDownloadEvent::Finished);
            },
        )
        .await
        .map_err(CommandFailure::platform)
}

/// Returns the version offered by the channel, newer than this build or not.
#[tauri::command]
pub(crate) async fn get_app_update_channel_version<R: Runtime>(
    app: AppHandle<R>,
    include_beta: bool,
) -> Result<String, CommandFailure> {
    use std::sync::Arc;
    let offered = Arc::new(Mutex::new(None::<String>));
    let captured = Arc::clone(&offered);
    let mut builder = updater_builder(&app, include_beta)?;
    builder = builder.version_comparator(move |current, release| {
        let newer = release.version > current;
        if let Ok(mut version) = captured.lock() {
            *version = Some(release.version.to_string());
        }
        newer
    });
    builder
        .build()
        .map_err(CommandFailure::platform)?
        .check()
        .await
        .map_err(CommandFailure::platform)?;
    let version = offered.lock().map_err(|_| poisoned())?.clone();
    version.ok_or_else(|| {
        CommandFailure::new(
            "appUpdateChannelSilent",
            "the update channel did not report a version",
        )
    })
}

fn poisoned() -> CommandFailure {
    CommandFailure::new(
        "appUpdateStateUnavailable",
        "application update state is unavailable",
    )
}
