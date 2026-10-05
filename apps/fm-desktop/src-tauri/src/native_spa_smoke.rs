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
    let label = plugin_spa::open_plugin_panel(
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
    let service = &app.state::<AppState>().service;
    let registry = app.state::<std::sync::Arc<plugin_spa::PanelRegistry>>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let saved = std::fs::read_to_string(file)
            .is_ok_and(|svg| svg.contains("data-native-spa-smoke=\"acl-denied-and-saved\""));
        let settings_saved = service
            .plugin_panel_settings("procyon.svgo")
            .is_some_and(|settings| settings["precision"] == 4);
        if saved && settings_saved && registry.heartbeat_responded(&label) {
            plugin_spa::flush_plugin_panels(app, "procyon.svgo").await;
            service
                .set_plugin_enabled("procyon.svgo".to_owned(), false)
                .map_err(|error| error.to_string())?;
            plugin_spa::close_plugin_panels(app, "procyon.svgo");
            if !registry.labels_for_plugin("procyon.svgo").is_empty() {
                return Err("disabled panel retained an origin slot".to_owned());
            }
            if service
                .plugin_panel_settings("procyon.svgo")
                .is_none_or(|settings| settings["precision"] != 4)
            {
                return Err("host-driven teardown lost persisted settings".to_owned());
            }
            stage("heartbeat-and-disable-teardown-succeeded");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(
        "native SPA Save, settings persistence, or heartbeat did not complete in 30 seconds"
            .to_owned(),
    )
}
