//! Library surface for the Tauri desktop host (spec §11, task 0015).
//!
//! Mirrors `apps/fm-server`'s `lib.rs`/`main.rs` split: `run()` is the real
//! entry point `main.rs` calls, and is also what the mock-runtime smoke test
//! below builds, so both exercise the exact same `Builder`.

mod commands;
mod credentials;
mod event_stream;
mod native_menu;
mod platform;
mod terminal;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_transport_dto::RuntimeKindDto;
use tauri::Manager;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// State injected into every Tauri command (spec §7: commands only call the
/// service).
pub struct AppState {
    pub(crate) service: Arc<FileManagerService>,
}

/// True once the whole app has started quitting (`RunEvent::ExitRequested`/`Exit`), checked by
/// the `Destroyed` window-event handler below so it can tell "the user closed one window" (its
/// ephemeral workspace, if any, should be deleted) apart from "the app is quitting" (every window
/// fires `Destroyed` too, but ephemeral workspaces must survive on disk instead - `setup()`'s
/// `surviving_ephemeral_ids` restores one window per surviving one on the next launch, ephemeral
/// per-window workspaces spec follow-up, phase 2).
#[derive(Default)]
pub(crate) struct QuittingFlag(AtomicBool);

impl QuittingFlag {
    fn mark_quitting(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn is_quitting(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Generates the app's `tauri.conf.json`-derived context.
///
/// `tauri::generate_context!()` must be invoked exactly once per source
/// location in this crate — each textual invocation embeds a fixed-name
/// static (e.g. `_EMBED_INFO_PLIST` on macOS), so calling the macro directly
/// from both [`run`] and the test module would collide. Both call this
/// function instead.
fn build_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

/// Builds and runs the desktop application.
///
/// No Axum server is started in-process to reuse HTTP (spec §11) — the
/// Tauri commands in [`commands`] call `FileManagerService` directly.
pub fn run() {
    init_tracing();
    tauri::Builder::default()
        .setup(|app| {
            // Built here rather than eagerly via `.manage()` because bundled plugin discovery
            // needs `app.path().resource_dir()`, which only resolves once the app has finished
            // initializing - not from a plain expression evaluated while assembling the
            // `Builder` chain, and not from `env!("CARGO_MANIFEST_DIR")` (a compile-time path
            // baked into the binary, real only on whichever machine built it - not this host's
            // installed app bundle). `resource_dir()` failing (should not happen for a real
            // bundle; only plausible for an unbundled `cargo tauri dev`/test run) leaves the
            // compile-time-default bundled directory in place rather than panicking - one
            // missing plugin source is not worth aborting startup over.
            let mut service =
                FileManagerService::with_platform_adapter_and_credential_store_and_search_accelerator(
                RuntimeKindDto::Tauri,
                fm_application::workspace::JsonFileWorkspaceRepository::default_directory(),
                fm_application::workspace::JsonFileWorkspaceRepository::default_directory()
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(".fm-config/fm"))
                    .to_path_buf(),
                EventBus::default(),
                platform::build_platform_adapter(),
                credentials::build_credential_store(),
                platform::build_search_accelerator(),
            );
            if let Ok(resource_dir) = app.path().resource_dir() {
                service.set_bundled_plugins_directory(resource_dir.join("plugins"));
            }
            app.manage(AppState {
                service: Arc::new(service),
            });

            // Dock icon right/long-click "New Window" item, mirroring the File menu's own item
            // (task 0133) - sends the frontend's `NEW_WORKSPACE_WINDOW_MENU_ID` through the same
            // native-menu-action channel a click on that File menu item uses, so both paths land
            // on `openNewWorkspaceWindow()`. Not localized: this installs once, before the
            // frontend has loaded any translations.
            #[cfg(target_os = "macos")]
            fm_platform_macos::install_dock_menu(
                "Dock Menu",
                "New Window",
                "ui.newWorkspaceWindow",
            );

            // `tauri.conf.json`'s declared window carries `"create": false` so it is never
            // auto-built here: this app must instead check, at every launch, whether one or more
            // ephemeral (per-window) workspaces survived a previous *quit* (as opposed to the
            // user closing each window, which deletes its own ephemeral workspace - see
            // `QuittingFlag`/the `Destroyed` handler below) and restore one window per surviving
            // one, rather than always opening a single default window (ephemeral per-window
            // workspaces spec follow-up, phase 2). `block_on` is safe here: `setup()` runs once,
            // synchronously, before the event loop starts - there is no running async task for
            // this to deadlock against, unlike building a window from inside a Tauri command.
            tauri::async_runtime::block_on(commands::open_startup_windows(app.handle()))?;

            Ok(())
        })
        // Registered first (per the plugin's own docs) so a second launch of the app is caught
        // before any other setup runs: rather than starting a second OS process that would race
        // this one over the same on-disk workspace store (task 0143), the second process hands its
        // launch off to this callback and exits, and this instance just focuses one of its
        // windows instead. There is no longer always a `"main"`-labelled window to reach for
        // (restoring ephemeral workspaces at startup, phase 2, may open only `workspace-<id>`
        // windows) - any currently open window satisfies "the app came back to the foreground",
        // so this just focuses the first one Tauri hands back.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        // Persists and restores each window's frame (position, size, maximized state) keyed by
        // its label, using only public Tauri/monitor APIs (task 0143 sub-task (c)). `map_label`
        // reduces a per-workspace window's label (`open_workspace_window`, sub-task (b), gives
        // every window a unique `workspace-<uuid>_<nonce>` label so "Open in New Window" always
        // opens another window rather than deduplicating) back to the stable `workspace-<uuid>`
        // form, so every window ever opened for the same workspace shares one remembered frame
        // instead of each getting its own. `on_window_ready` fires for windows built later via
        // `WebviewWindowBuilder` just as much as the config-declared `"main"` one. Deliberately
        // does not restore which macOS Space/virtual-desktop a window was on: no public API
        // exposes that (see TASKS/0143's Context for why private `CGSSpace*` APIs are out of
        // scope here).
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .map_label(commands::canonical_workspace_window_label)
                .build(),
        )
        .manage(event_stream::EventSubscriptionRegistry::default())
        .manage(terminal::TerminalRegistry::default())
        .manage(native_menu::NativeMenuActionChannel::default())
        .manage(QuittingFlag::default())
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                window
                    .state::<event_stream::EventSubscriptionRegistry>()
                    .unsubscribe_window(window.label());

                // Only a genuine single-window close deletes its ephemeral workspace - not every
                // window's `Destroyed` fired as part of the whole app quitting (see
                // `QuittingFlag`'s doc comment). `"main"`/non-`workspace-<id>` labels have no id to
                // parse and are never ephemeral, so this is a no-op for them.
                if !window.state::<QuittingFlag>().is_quitting()
                    && let Some(workspace_id) = commands::workspace_id_from_label(window.label())
                {
                    let service = window.state::<AppState>().service.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Ok(workspace) = service.get_workspace(workspace_id).await
                            && workspace.ephemeral
                        {
                            let _ = service.delete_workspace(workspace_id, None).await;
                        }
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::subscribe_events,
            commands::unsubscribe_events,
            commands::get_runtime_capabilities,
            commands::get_system_locations,
            commands::get_volumes,
            commands::get_home_directory,
            commands::start_native_drag,
            commands::show_platform_context_menu,
            commands::native_drag_locations,
            commands::get_file_icon,
            commands::get_thumbnail,
            commands::get_finder_tags,
            commands::set_finder_tags,
            commands::get_spotlight_comment,
            commands::set_spotlight_comment,
            commands::get_settings,
            commands::update_settings,
            commands::list_directory,
            commands::list_directory_children,
            commands::refresh_directory,
            commands::navigate_pane,
            commands::get_entry_metadata,
            commands::set_pane_activity,
            commands::read_file_range,
            commands::open_docx_preview,
            commands::read_docx_preview_resource,
            commands::close_docx_preview,
            commands::open_pptx_preview,
            commands::read_pptx_preview_pdf,
            commands::close_pptx_preview,
            commands::open_structured_view,
            commands::structured_view_status,
            commands::update_structured_view,
            commands::read_structured_rows,
            commands::read_structured_json_window,
            commands::search_structured_rows,
            commands::close_structured_view,
            commands::load_editable_file,
            commands::save_editable_file,
            commands::search_in_file,
            commands::calculate_folder_size,
            commands::archive_summary,
            commands::scan_disk_usage,
            commands::cancel_disk_usage,
            commands::discover_application_uninstall_candidates,
            commands::remove_application_dock_icon,
            commands::get_file_git_history,
            commands::cache_archive_password,
            commands::list_workspaces,
            commands::start_workspace,
            commands::open_workspace_window,
            commands::resync_workspace,
            commands::create_workspace,
            commands::get_workspace,
            commands::delete_workspace,
            commands::open_workspace,
            commands::apply_workspace_command,
            commands::start_operation,
            commands::list_operations,
            commands::get_operation,
            commands::cancel_operation,
            commands::pause_operation,
            commands::resume_operation,
            commands::undo_operation,
            commands::resolve_operation_conflict,
            commands::list_actions,
            commands::invoke_action,
            commands::list_plugins,
            commands::enable_plugin,
            commands::disable_plugin,
            commands::get_plugin_logs,
            commands::get_plugin_icon_theme_asset,
            commands::start_search,
            commands::cancel_search,
            commands::start_comparison,
            commands::get_comparison,
            commands::cancel_comparison,
            commands::generate_sync_plan,
            commands::apply_sync_plan,
            commands::start_checksums,
            commands::get_checksums,
            commands::cancel_checksums,
            commands::render_checksum_file,
            commands::save_checksum_file,
            commands::verify_checksum_file,
            commands::start_duplicate_scan,
            commands::get_duplicate_scan,
            commands::cancel_duplicate_scan,
            commands::list_connections,
            commands::create_connection,
            commands::get_connection,
            commands::update_connection,
            commands::delete_connection,
            commands::connect_connection,
            commands::disconnect_connection,
            commands::test_connection,
            commands::probe_ssh_host_key,
            commands::accept_ssh_host_key,
            commands::begin_onedrive_authorization,
            commands::get_onedrive_authorization_attempt,
            commands::cancel_onedrive_authorization,
            commands::open_embedded_terminal,
            commands::write_embedded_terminal,
            commands::resize_embedded_terminal,
            commands::set_caption_colours,
            commands::set_window_decorations,
            commands::get_diagnostics,
            commands::subscribe_native_menu_actions,
            commands::initialize_window_handle,
            commands::set_native_menu,
        ])
        .build(build_context())
        .expect("error while building the Tauri application")
        .run(|app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                app_handle.state::<QuittingFlag>().mark_quitting();
            }
            // Fires when macOS reactivates the app (Dock icon, `open -a`) while it has no
            // visible windows - the ordinary state after closing the last window without
            // quitting (see `commands::open_startup_windows`'s doc comment for why this matters:
            // without this handler the app silently failed to reopen at all). `has_visible_windows
            // == true` needs no action - the system already brings existing windows forward.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
                && !has_visible_windows
            {
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = commands::open_startup_windows(&app_handle).await;
                });
            }
        });
}

/// Initialises structured tracing for the desktop host (spec §30).
///
/// - `RUST_LOG` controls level filter (default: `info`).
/// - `FM_LOG_FORMAT` controls output format: `compact` (default) or `pretty`.
/// - `FM_LOG_FILE` writes a rolling daily log to the given path prefix (desktop mode).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let format = std::env::var("FM_LOG_FORMAT").unwrap_or_default();
    let log_file = std::env::var("FM_LOG_FILE").ok().or_else(|| {
        // Default desktop log location: OS data dir / fm / fm-desktop.log
        dirs::data_dir().map(|d| {
            d.join("fm")
                .join("fm-desktop.log")
                .to_string_lossy()
                .into_owned()
        })
    });

    match log_file {
        Some(path) => {
            let dir = std::path::Path::new(&path)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            let prefix = std::path::Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("fm-desktop");
            let file_appender = tracing_appender::rolling::daily(dir, prefix);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            std::mem::forget(_guard);
            tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
                .init();
        }
        None => match format.as_str() {
            "pretty" => tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().pretty())
                .init(),
            _ => tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().compact())
                .init(),
        },
    }
}

#[cfg(test)]
mod tests {
    use tauri::Manager;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};
    use tauri::webview::InvokeRequest;

    use super::*;

    fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::App<R> {
        let workspace_directory =
            tempfile::tempdir().expect("must create a temp workspace directory");
        let settings_directory = workspace_directory.path().join("settings");
        let workspace_directory = workspace_directory.keep();
        builder
            .manage(AppState {
                service: Arc::new(
                    FileManagerService::with_platform_adapter_and_credential_store_and_search_accelerator(
                        RuntimeKindDto::Tauri,
                        workspace_directory,
                        settings_directory,
                        EventBus::default(),
                        platform::build_platform_adapter(),
                        credentials::build_credential_store(),
                        platform::build_search_accelerator(),
                    ),
                ),
            })
            .manage(event_stream::EventSubscriptionRegistry::default())
            .manage(native_menu::NativeMenuActionChannel::default())
            .invoke_handler(tauri::generate_handler![
                commands::subscribe_events,
                commands::unsubscribe_events,
                commands::get_runtime_capabilities,
                commands::get_system_locations,
                commands::get_volumes,
                commands::get_home_directory,
                commands::start_native_drag,
                commands::native_drag_locations,
                commands::get_file_icon,
                commands::get_thumbnail,
                commands::get_finder_tags,
                commands::set_finder_tags,
                commands::get_spotlight_comment,
                commands::set_spotlight_comment,
                commands::get_settings,
                commands::update_settings,
                commands::list_directory,
                commands::list_directory_children,
                commands::refresh_directory,
                commands::navigate_pane,
                commands::get_entry_metadata,
                commands::set_pane_activity,
                commands::read_file_range,
                commands::open_docx_preview,
                commands::read_docx_preview_resource,
                commands::close_docx_preview,
                commands::open_pptx_preview,
                commands::read_pptx_preview_pdf,
                commands::close_pptx_preview,
                commands::open_structured_view,
                commands::structured_view_status,
                commands::update_structured_view,
                commands::read_structured_rows,
                commands::read_structured_json_window,
                commands::search_structured_rows,
                commands::close_structured_view,
                commands::load_editable_file,
                commands::save_editable_file,
                commands::search_in_file,
                commands::calculate_folder_size,
                commands::archive_summary,
                commands::scan_disk_usage,
                commands::cancel_disk_usage,
                commands::discover_application_uninstall_candidates,
                commands::remove_application_dock_icon,
                commands::get_file_git_history,
                commands::cache_archive_password,
                commands::list_workspaces,
                commands::start_workspace,
                commands::resync_workspace,
                commands::create_workspace,
                commands::get_workspace,
                commands::delete_workspace,
                commands::open_workspace,
                commands::apply_workspace_command,
                commands::start_operation,
                commands::list_operations,
                commands::get_operation,
                commands::cancel_operation,
                commands::pause_operation,
                commands::resume_operation,
                commands::undo_operation,
                commands::resolve_operation_conflict,
                commands::list_actions,
                commands::invoke_action,
                commands::list_plugins,
                commands::enable_plugin,
                commands::disable_plugin,
                commands::get_plugin_logs,
                commands::get_plugin_icon_theme_asset,
                commands::start_search,
                commands::cancel_search,
                commands::start_comparison,
                commands::get_comparison,
                commands::cancel_comparison,
                commands::generate_sync_plan,
                commands::apply_sync_plan,
                commands::start_checksums,
                commands::get_checksums,
                commands::cancel_checksums,
                commands::render_checksum_file,
                commands::save_checksum_file,
                commands::verify_checksum_file,
                commands::start_duplicate_scan,
                commands::get_duplicate_scan,
                commands::cancel_duplicate_scan,
                commands::list_connections,
                commands::create_connection,
                commands::get_connection,
                commands::update_connection,
                commands::delete_connection,
                commands::connect_connection,
                commands::disconnect_connection,
                commands::test_connection,
                commands::probe_ssh_host_key,
                commands::accept_ssh_host_key,
                commands::begin_onedrive_authorization,
                commands::get_onedrive_authorization_attempt,
                commands::cancel_onedrive_authorization,
                commands::set_window_decorations,
                commands::get_diagnostics,
                commands::subscribe_native_menu_actions,
                commands::initialize_window_handle,
                commands::set_native_menu,
            ])
            // Uses the app's real `tauri.conf.json` config (same as `run()`)
            // rather than `mock_context(noop_assets())`'s empty default config,
            // so `is_local_url` resolves the same dev/prod URLs production does.
            .build(build_context())
            .expect("failed to build mock app")
    }

    /// The URL the webview really serves the frontend from, which is the only
    /// origin the app's ACL grants commands to. Windows and Android use the
    /// `http://tauri.localhost` workaround rather than the `tauri://` scheme
    /// (see `WebviewManager::tauri_protocol_url`).
    fn local_protocol_url() -> tauri::Url {
        let url = if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        };
        url.parse().expect("valid url")
    }

    /// Smoke test (task 0015's acceptance criteria): the app starts, on a
    /// headless `MockRuntime` (no real window), and `getRuntimeCapabilities`
    /// reports `runtime: "tauri"`.
    #[test]
    fn app_starts_and_reports_the_tauri_runtime() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let response = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_runtime_capabilities".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                // The app's ACL only grants commands to the platform's own local
                // protocol URL; anything else counts as remote and is rejected.
                url: local_protocol_url(),
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("command must succeed")
        .deserialize::<fm_transport_dto::RuntimeCapabilitiesDto>()
        .expect("response must deserialize");

        assert_eq!(response.runtime, RuntimeKindDto::Tauri);
        app.state::<AppState>();
    }

    #[test]
    fn file_icon_command_returns_a_typed_error_for_an_invalid_location() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_file_icon".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "uri": "not a location" })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("invalid location must reject the command");

        assert!(error.to_string().contains("invalidRequest"));
    }

    #[test]
    fn thumbnail_command_returns_a_typed_error_for_an_invalid_location() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_thumbnail".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(
                    serde_json::json!({ "uri": "not a location", "size": "small" }),
                ),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("invalid location must reject the command");

        assert!(error.to_string().contains("invalidRequest"));
    }

    #[tokio::test]
    async fn disk_usage_command_accepts_progress_correlation_fields() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "scan_disk_usage".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": {
                        "workspaceId": uuid::Uuid::new_v4(),
                        "scanId": uuid::Uuid::new_v4(),
                        "location": {
                            "providerId": "sftp",
                            "uri": "sftp://example.invalid/root"
                        },
                        "expandRoot": false
                    }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("event-driven disk usage must accept a correlated scan request");
    }

    #[test]
    fn cancel_disk_usage_command_is_idempotent_before_scan_registration() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "cancel_disk_usage".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "scanId": uuid::Uuid::new_v4(),
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("cancelling before registration must succeed");
    }

    #[test]
    fn finder_tags_command_returns_a_typed_error_for_an_invalid_location() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_finder_tags".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "uri": "not a location" })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("invalid location must reject the command");

        assert!(error.to_string().contains("invalidRequest"));
    }

    #[test]
    fn spotlight_comment_command_returns_a_typed_error_for_an_invalid_location() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_spotlight_comment".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "uri": "not a location" })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("invalid location must reject the command");

        assert!(error.to_string().contains("invalidRequest"));
    }

    #[tokio::test]
    async fn begin_onedrive_authorization_command_returns_not_found_for_an_unknown_connection() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "begin_onedrive_authorization".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "connectionId": uuid::Uuid::new_v4() })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("an unknown connection must reject the command");

        assert!(error.to_string().contains("notFound"));
    }

    /// Parity check (task 0110): the same create/begin/poll/cancel sequence
    /// the HTTP surface exposes (`apps/fm-server/tests/onedrive_authorization_routes.rs`)
    /// also works identically through the Tauri IPC surface, over the same
    /// `FileManagerService` methods.
    #[tokio::test]
    async fn onedrive_authorization_commands_round_trip_through_create_begin_poll_and_cancel() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let created = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "create_connection".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": {
                        "name": "My OneDrive",
                        "kind": "oneDrive",
                        "configuration": { "kind": "oneDrive" },
                        "secret": null,
                    }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("create_connection must succeed")
        .deserialize::<fm_transport_dto::ConnectionDto>()
        .expect("response must deserialize");
        assert!(!created.has_credential);

        let begin = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "begin_onedrive_authorization".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "connectionId": created.id })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("begin_onedrive_authorization must succeed")
        .deserialize::<fm_transport_dto::BeginOneDriveAuthorizationResponseDto>()
        .expect("response must deserialize");
        assert!(begin.authorization_url.contains("oauth2/v2.0/authorize"));
        assert!(!begin.authorization_url.contains("client_secret"));

        let pending = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_onedrive_authorization_attempt".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "attemptId": begin.attempt_id })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("get_onedrive_authorization_attempt must succeed")
        .deserialize::<fm_transport_dto::OneDriveAuthorizationAttemptDto>()
        .expect("response must deserialize");
        assert_eq!(
            pending.status,
            fm_transport_dto::OneDriveAuthorizationStatusDto::Pending
        );

        let cancelled = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "cancel_onedrive_authorization".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({ "attemptId": begin.attempt_id })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("cancel_onedrive_authorization must succeed")
        .deserialize::<fm_transport_dto::OneDriveAuthorizationAttemptDto>()
        .expect("response must deserialize");
        assert_eq!(cancelled.id, begin.attempt_id);
    }
}
