use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use fm_domain::Location;
use tauri::{AppHandle, Manager, Runtime};

use crate::{AppState, plugin_spa};

static STARTED: AtomicBool = AtomicBool::new(false);

pub(crate) fn stage(message: &str) {
    if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some() {
        eprintln!("native-spa-stage: {message}");
    }
}

pub(crate) fn start_once<R: Runtime>(app: AppHandle<R>) {
    let Some(file) = std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE") else {
        return;
    };
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    stage("trusted-page-loaded");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = open(&app, Path::new(&file)).await {
            stage(&format!("child-open-failed: {error}"));
        }
    });
}

async fn open<R: Runtime>(app: &AppHandle<R>, file: &Path) -> Result<(), String> {
    let source = app
        .windows()
        .into_values()
        .find(|window| plugin_spa::trusted_invoke_label(window.label()))
        .ok_or("no trusted window opened")?;
    let size = source.inner_size().map_err(|error| error.to_string())?;
    let scale = source.scale_factor().map_err(|error| error.to_string())?;
    let location = Location::from_native_path(file)
        .map_err(|error| error.to_string())?
        .into();
    plugin_spa::open_plugin_panel(
        app.clone(),
        source,
        app.state::<AppState>(),
        plugin_spa::OpenPanelRequest {
            plugin_id: "procyon.svgo".to_owned(),
            action_id: "procyon.svgo.open".to_owned(),
            location,
            bounds: plugin_spa::PanelBounds {
                x: 0.0,
                y: 0.0,
                width: f64::from(size.width) / scale / 2.0,
                height: f64::from(size.height) / scale / 2.0,
            },
            theme: plugin_spa::PanelTheme::Light,
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    stage("child-created");
    Ok(())
}
