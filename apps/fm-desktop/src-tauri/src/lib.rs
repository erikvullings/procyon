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
#[cfg(debug_assertions)]
mod semantic_developer;
mod semantic_production;
mod terminal;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_transport_dto::RuntimeKindDto;
use tauri::Manager;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// State injected into every Tauri command (spec §7: commands only call the
/// service).
pub struct AppState {
    pub(crate) service: Arc<FileManagerService>,
    pub(crate) rag_cancellations: Mutex<HashMap<uuid::Uuid, Option<CancellationToken>>>,
    pub(crate) semantic_managed_components: bool,
    pub(crate) semantic_reindex_pending_marker: Option<std::path::PathBuf>,
    pub(crate) semantic_ocr_shutdown: CancellationToken,
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
            let workspace_directory =
                fm_application::workspace::JsonFileWorkspaceRepository::default_directory();
            let app_data_directory = workspace_directory
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".fm-config/fm"))
                .to_path_buf();
            let resource_directory = app.path().resource_dir().ok();
            #[cfg(not(debug_assertions))]
            if std::env::var_os("PROCYON_SEMANTIC_DEVELOPER_BUNDLE").is_some() {
                return Err(std::io::Error::other(
                    "semantic developer bundles are rejected by release builds",
                )
                .into());
            }
            let mut service =
                FileManagerService::with_platform_adapter_and_credential_store_and_search_accelerator(
                RuntimeKindDto::Tauri,
                workspace_directory,
                app_data_directory.clone(),
                EventBus::default(),
                platform::build_platform_adapter(),
                credentials::build_credential_store(),
                platform::build_search_accelerator(),
            );
            let mut semantic_managed_components = false;
            let mut semantic_reindex_pending_marker = None;
            #[cfg(debug_assertions)]
            if let Some(bundle_directory) =
                std::env::var_os("PROCYON_SEMANTIC_DEVELOPER_BUNDLE")
            {
                semantic_managed_components = true;
                let bundle = semantic_developer::DeveloperSemanticBundle::load(
                    std::path::Path::new(&bundle_directory),
                    &app_data_directory,
                    &app_data_directory,
                )
                .map_err(|error| std::io::Error::other(error.to_string()))?;
                let semantic =
                    fm_application::semantic::IpcSemanticCapability::desktop_developer_bundle(
                    &bundle.runtime_directory,
                    &bundle.installed_worker,
                    &bundle.worker_data_directory,
                    &bundle.native_library_directory,
                    Some(Arc::clone(&bundle.active_model_pack)),
                );
                service = service
                    .with_semantic_component_capability(bundle.components)
                    .with_semantic_capability(Arc::new(semantic));
                semantic_reindex_pending_marker = Some(bundle.reindex_pending_marker);
            }
            if !semantic_managed_components
                && let Some(resource_directory) = resource_directory.as_deref()
                && let Some(bundle) = semantic_production::ProductionSemanticBundle::load(
                    resource_directory,
                    &app_data_directory,
                    &app_data_directory,
                )
                .map_err(|error| std::io::Error::other(error.to_string()))?
            {
                semantic_managed_components = true;
                let semantic = fm_application::semantic::IpcSemanticCapability::desktop_managed(
                    &bundle.runtime_directory,
                    Arc::clone(&bundle.worker),
                );
                service = service
                    .with_semantic_component_capability(bundle.components)
                    .with_semantic_capability(Arc::new(semantic))
                    .with_semantic_ocr_service(bundle.ocr);
                semantic_reindex_pending_marker = Some(bundle.reindex_pending_marker);
            }
            #[cfg(debug_assertions)]
            if !semantic_managed_components
                && std::env::var("PROCYON_SEMANTIC_COMPONENTS").as_deref() == Ok("mock")
            {
                service = service.with_semantic_component_capability(Arc::new(
                    fm_application::semantic_components::FakeSemanticComponentCapability::new(),
                ));
            }
            if let Some(resource_dir) = resource_directory {
                service.set_bundled_plugins_directory(resource_dir.join("plugins"));
            }
            let service = Arc::new(service);
            let semantic_ocr_shutdown = CancellationToken::new();
            app.manage(AppState {
                service: Arc::clone(&service),
                rag_cancellations: Mutex::new(HashMap::new()),
                semantic_managed_components,
                semantic_reindex_pending_marker: semantic_reindex_pending_marker.clone(),
                semantic_ocr_shutdown: semantic_ocr_shutdown.clone(),
            });
            if let Err(error) = service.recover_semantic_ocr_remediation_jobs() {
                tracing::error!(%error, "OCR remediation recovery failed");
            }
            tauri::async_runtime::spawn(
                Arc::clone(&service)
                    .run_semantic_ocr_remediation_jobs(semantic_ocr_shutdown),
            );
            if let Err(error) = tauri::async_runtime::block_on(service.semantic_library_status(
                &fm_application::semantic_library::SemanticAccessContext::Host,
            )) {
                tracing::warn!(%error, "semantic library startup reconciliation failed");
            }
            if semantic_managed_components {
                if let Some(marker) =
                    semantic_reindex_pending_marker.filter(|marker| marker.is_file())
                {
                    tauri::async_runtime::spawn(commands::resume_pending_semantic_model_reindex(
                        service, marker,
                    ));
                } else {
                    tauri::async_runtime::spawn(commands::reconcile_semantic_library_on_startup(
                        service,
                    ));
                }
            }

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
            commands::get_semantic_component_capabilities,
            commands::get_semantic_ocr_status,
            commands::set_semantic_ocr_consent,
            commands::start_semantic_ocr_remediation,
            commands::cancel_semantic_ocr_remediation,
            commands::get_semantic_component_status,
            commands::list_semantic_component_profiles,
            commands::create_semantic_component_installation_offer,
            commands::accept_semantic_component_installation_offer,
            commands::pause_semantic_component_indexing,
            commands::resume_semantic_component_indexing,
            commands::create_semantic_component_index_removal_plan,
            commands::confirm_semantic_component_index_removal,
            commands::move_semantic_component_data,
            commands::uninstall_semantic_components,
            commands::install_semantic_component_worker_patch,
            commands::import_semantic_component_local_model,
            commands::plan_semantic_component_model_migration,
            commands::confirm_semantic_component_model_migration,
            commands::checkpoint_semantic_component_model_migration,
            commands::complete_semantic_component_model_migration,
            commands::get_semantic_library_capabilities,
            commands::get_semantic_library_status,
            commands::list_semantic_vocabularies,
            commands::import_semantic_vocabulary,
            commands::export_semantic_vocabulary,
            commands::attach_semantic_vocabulary,
            commands::review_semantic_concept_candidate,
            commands::delete_semantic_vocabulary,
            commands::get_semantic_folder_status,
            commands::preview_semantic_enrolment,
            commands::confirm_semantic_enrolment,
            commands::plan_semantic_exclusion,
            commands::confirm_semantic_exclusion,
            commands::resume_semantic_cleanup,
            commands::pause_semantic_library,
            commands::resume_semantic_library,
            commands::update_semantic_eligibility_overrides,
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
            commands::list_llm_profile_presets,
            commands::list_llm_profiles,
            commands::create_llm_profile,
            commands::update_llm_profile,
            commands::delete_llm_profile,
            commands::clone_llm_profile,
            commands::export_llm_profile,
            commands::activate_llm_profile,
            commands::discover_llm_profile_models,
            commands::discover_llm_profile_draft_models,
            commands::test_llm_profile,
            commands::preview_document_summary,
            commands::generate_document_summary,
            commands::get_document_summary,
            commands::preview_rag,
            commands::generate_rag_answer,
            commands::cancel_rag,
            commands::save_rag_conversation,
            commands::list_saved_rag_conversations,
            commands::delete_rag_conversation,
            commands::resolve_rag_citation,
            commands::get_knowledge_capabilities,
            commands::list_knowledge_roots,
            commands::parse_knowledge_query,
            commands::plan_knowledge_search,
            commands::execute_knowledge_search,
            commands::cancel_knowledge_search,
            commands::resolve_knowledge_source,
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
                app_handle
                    .state::<AppState>()
                    .semantic_ocr_shutdown
                    .cancel();
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
        create_app_with_semantic_developer_bundle(builder, false)
    }

    fn create_app_with_semantic_developer_bundle<R: tauri::Runtime>(
        builder: tauri::Builder<R>,
        semantic_developer_bundle: bool,
    ) -> tauri::App<R> {
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
                    )
                    .with_semantic_component_capability(Arc::new(
                        fm_application::semantic_components::FakeSemanticComponentCapability::new(),
                    ))
                    .with_semantic_library_service(
                        fm_application::semantic_library::SemanticLibraryService::deterministic_mock(),
                    ),
                ),
                rag_cancellations: Mutex::new(HashMap::new()),
                semantic_managed_components: semantic_developer_bundle,
                semantic_reindex_pending_marker: None,
                semantic_ocr_shutdown: CancellationToken::new(),
            })
            .manage(event_stream::EventSubscriptionRegistry::default())
            .manage(native_menu::NativeMenuActionChannel::default())
            .invoke_handler(tauri::generate_handler![
                commands::subscribe_events,
                commands::unsubscribe_events,
                commands::get_runtime_capabilities,
                commands::get_semantic_component_capabilities,
                commands::get_semantic_ocr_status,
                commands::set_semantic_ocr_consent,
                commands::start_semantic_ocr_remediation,
                commands::cancel_semantic_ocr_remediation,
                commands::get_semantic_component_status,
                commands::list_semantic_component_profiles,
                commands::create_semantic_component_installation_offer,
                commands::accept_semantic_component_installation_offer,
                commands::pause_semantic_component_indexing,
                commands::resume_semantic_component_indexing,
                commands::create_semantic_component_index_removal_plan,
                commands::confirm_semantic_component_index_removal,
                commands::move_semantic_component_data,
                commands::uninstall_semantic_components,
                commands::install_semantic_component_worker_patch,
                commands::import_semantic_component_local_model,
                commands::plan_semantic_component_model_migration,
                commands::confirm_semantic_component_model_migration,
                commands::checkpoint_semantic_component_model_migration,
                commands::complete_semantic_component_model_migration,
                commands::get_semantic_library_capabilities,
                commands::get_semantic_library_status,
                commands::list_semantic_vocabularies,
                commands::import_semantic_vocabulary,
                commands::export_semantic_vocabulary,
                commands::attach_semantic_vocabulary,
                commands::review_semantic_concept_candidate,
                commands::delete_semantic_vocabulary,
                commands::get_semantic_folder_status,
                commands::preview_semantic_enrolment,
                commands::confirm_semantic_enrolment,
                commands::plan_semantic_exclusion,
                commands::confirm_semantic_exclusion,
                commands::resume_semantic_cleanup,
                commands::pause_semantic_library,
                commands::resume_semantic_library,
                commands::update_semantic_eligibility_overrides,
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
                commands::list_llm_profile_presets,
                commands::list_llm_profiles,
                commands::create_llm_profile,
                commands::update_llm_profile,
                commands::delete_llm_profile,
                commands::clone_llm_profile,
                commands::export_llm_profile,
                commands::activate_llm_profile,
                commands::discover_llm_profile_models,
                commands::discover_llm_profile_draft_models,
                commands::test_llm_profile,
                commands::preview_document_summary,
                commands::generate_document_summary,
                commands::get_document_summary,
                commands::preview_rag,
                commands::generate_rag_answer,
                commands::cancel_rag,
                commands::save_rag_conversation,
                commands::list_saved_rag_conversations,
                commands::delete_rag_conversation,
                commands::resolve_rag_citation,
                commands::get_knowledge_capabilities,
                commands::list_knowledge_roots,
                commands::parse_knowledge_query,
                commands::plan_knowledge_search,
                commands::execute_knowledge_search,
                commands::cancel_knowledge_search,
                commands::resolve_knowledge_source,
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

    /// Every knowledge command must be registered and usable on the desktop
    /// host without a generation profile, matching the browser host's routes.
    #[test]
    fn knowledge_commands_are_registered_and_work_without_a_generation_profile() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let capabilities = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_knowledge_capabilities".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("capability command must be registered")
        .deserialize::<fm_transport_dto::KnowledgeCapabilitiesDto>()
        .expect("response must deserialize");
        assert!(!capabilities.answer_generation);

        let interpretation = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "parse_knowledge_query".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": { "text": "about: wind turbines\nneed: definition\ndo: explain" }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("parse command must be registered")
        .deserialize::<fm_transport_dto::KnowledgeQueryInterpretationDto>()
        .expect("response must deserialize");
        assert_eq!(interpretation.draft.about, vec!["wind turbines".to_owned()]);
        assert!(
            interpretation
                .excluded_from_retrieval
                .iter()
                .any(|field| field.field == "do")
        );

        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "cancel_knowledge_search".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(
                    serde_json::json!({ "request": { "requestId": uuid::Uuid::new_v4() } }),
                ),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("cancel command must be registered");

        for command in [
            "list_knowledge_roots",
            "plan_knowledge_search",
            "execute_knowledge_search",
            "resolve_knowledge_source",
        ] {
            let error = get_ipc_response(
                &webview,
                InvokeRequest {
                    cmd: command.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: local_protocol_url(),
                    body: InvokeBody::Json(serde_json::json!({ "request": {} })),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.to_string(),
                },
            )
            .expect_err("an empty request must be rejected by the registered command");
            assert!(
                !error.to_string().contains("not found"),
                "{command} must be registered: {error}"
            );
        }
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

    #[tokio::test]
    async fn llm_profile_commands_round_trip_through_the_shared_service() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        macro_rules! invoke {
            ($command:literal, $body:expr) => {
                get_ipc_response(
                    &webview,
                    InvokeRequest {
                        cmd: $command.into(),
                        callback: CallbackFn(0),
                        error: CallbackFn(1),
                        url: local_protocol_url(),
                        body: InvokeBody::Json($body),
                        headers: Default::default(),
                        invoke_key: INVOKE_KEY.to_string(),
                    },
                )
            };
        }

        let presets = invoke!("list_llm_profile_presets", serde_json::json!({}))
            .expect("profile presets command must succeed")
            .deserialize::<Vec<fm_transport_dto::LlmProfilePresetDto>>()
            .expect("presets response must deserialize");
        assert_eq!(presets.len(), 7);

        let created = invoke!(
            "create_llm_profile",
            serde_json::json!({
                "request": {
                    "name": "Local generation",
                    "preset": "ollama",
                    "baseUrl": "http://127.0.0.1:1",
                    "deployment": null,
                    "apiVersion": null,
                    "model": "model-a",
                    "credential": null,
                    "advanced": {
                        "contextWindow": 8192,
                        "maximumAnswerTokens": 1024,
                        "temperature": 0.2,
                        "timeoutSeconds": 1,
                        "tlsPolicy": "requireValidCertificate",
                        "customHeaders": {}
                    },
                    "capabilities": ["chatCompletions"],
                    "redactFilenames": true
                }
            })
        )
        .expect("create profile command must succeed")
        .deserialize::<fm_transport_dto::LlmProfileDto>()
        .expect("profile response must deserialize");
        assert!(!created.has_credential);

        let listed = invoke!("list_llm_profiles", serde_json::json!({}))
            .expect("list profiles command must succeed")
            .deserialize::<Vec<fm_transport_dto::LlmProfileDto>>()
            .expect("profiles response must deserialize");
        assert_eq!(listed.len(), 1);

        let tested = invoke!(
            "test_llm_profile",
            serde_json::json!({ "profileId": created.id })
        )
        .expect("test profile command must return a normalized result")
        .deserialize::<fm_transport_dto::LlmProfileTestResultDto>()
        .expect("test response must deserialize");
        assert!(!tested.success);

        let cloned = invoke!(
            "clone_llm_profile",
            serde_json::json!({ "profileId": created.id })
        )
        .expect("clone profile command must succeed")
        .deserialize::<fm_transport_dto::LlmProfileDto>()
        .expect("clone response must deserialize");
        assert!(!cloned.has_credential);

        let exported = invoke!(
            "export_llm_profile",
            serde_json::json!({ "profileId": created.id })
        )
        .expect("export profile command must succeed")
        .deserialize::<serde_json::Value>()
        .expect("export response must deserialize");
        let exported = exported.to_string().to_ascii_lowercase();
        assert!(!exported.contains("credential"));
        assert!(!exported.contains("consent"));

        invoke!(
            "delete_llm_profile",
            serde_json::json!({
                "profileId": created.id,
                "request": { "credentialDisposition": "retain" }
            })
        )
        .expect("delete profile command must succeed");
    }

    #[tokio::test]
    async fn semantic_component_commands_round_trip_through_the_shared_service() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        macro_rules! invoke {
            ($command:literal, $body:expr) => {
                get_ipc_response(
                    &webview,
                    InvokeRequest {
                        cmd: $command.into(),
                        callback: CallbackFn(0),
                        error: CallbackFn(1),
                        url: local_protocol_url(),
                        body: InvokeBody::Json($body),
                        headers: Default::default(),
                        invoke_key: INVOKE_KEY.to_string(),
                    },
                )
            };
        }

        let capabilities = invoke!("get_semantic_component_capabilities", serde_json::json!({}))
            .expect("semantic capabilities command must succeed")
            .deserialize::<fm_transport_dto::SemanticComponentCapabilitiesDto>()
            .expect("capabilities response must deserialize");
        assert_eq!(
            capabilities.authority,
            fm_transport_dto::SemanticComponentAuthorityDto::DeterministicMock
        );
        assert_eq!(capabilities.operations.len(), 15);

        let status = invoke!("get_semantic_component_status", serde_json::json!({}))
            .expect("semantic status command must succeed")
            .deserialize::<fm_transport_dto::SemanticComponentStatusDto>()
            .expect("status response must deserialize");
        assert_eq!(
            status.lifecycle,
            fm_transport_dto::SemanticComponentLifecycleDto::Absent
        );

        let profiles = invoke!("list_semantic_component_profiles", serde_json::json!({}))
            .expect("semantic profiles command must succeed")
            .deserialize::<Vec<fm_transport_dto::SemanticModelProfileDto>>()
            .expect("profiles response must deserialize");
        assert_eq!(profiles.len(), 3);

        let offer = invoke!(
            "create_semantic_component_installation_offer",
            serde_json::json!({
                "request": { "profile": "compactMultilingual" }
            })
        )
        .expect("semantic installation offer command must succeed")
        .deserialize::<fm_transport_dto::SemanticInstallationOfferDto>()
        .expect("installation offer response must deserialize");
        assert!(offer.embeddings_stay_local);

        let install = invoke!(
            "accept_semantic_component_installation_offer",
            serde_json::json!({
                "request": { "offerId": offer.offer_id }
            })
        )
        .expect("semantic installation command must succeed")
        .deserialize::<fm_transport_dto::SemanticInstallReceiptDto>()
        .expect("installation response must deserialize");
        assert_eq!(install.installed_artifact_ids.len(), 3);

        invoke!("pause_semantic_component_indexing", serde_json::json!({}))
            .expect("pause semantic indexing command must succeed");
        invoke!("resume_semantic_component_indexing", serde_json::json!({}))
            .expect("resume semantic indexing command must succeed");

        let removal_plan = invoke!(
            "create_semantic_component_index_removal_plan",
            serde_json::json!({
                "request": {
                    "enrolmentId": "enrolment-1"
                }
            })
        )
        .expect("semantic index removal plan command must succeed")
        .deserialize::<fm_transport_dto::SemanticIndexRemovalPlanDto>()
        .expect("index removal plan response must deserialize");
        assert_eq!(removal_plan.expected.conversation_evidence, 2);

        let removed = invoke!(
            "confirm_semantic_component_index_removal",
            serde_json::json!({
                "request": {
                    "planId": removal_plan.plan_id
                }
            })
        )
        .expect("semantic index removal confirmation command must succeed")
        .deserialize::<fm_transport_dto::SemanticIndexRemovalReceiptDto>()
        .expect("index removal response must deserialize");
        assert!(removed.conversation_evidence_deleted);

        let moved = invoke!(
            "move_semantic_component_data",
            serde_json::json!({
                "request": { "destination": "mock/moved-semantic" }
            })
        )
        .expect("semantic data move command must succeed")
        .deserialize::<fm_transport_dto::SemanticDataMoveReceiptDto>()
        .expect("data move response must deserialize");
        assert_eq!(moved.destination, "mock/moved-semantic");

        let patch = invoke!(
            "install_semantic_component_worker_patch",
            serde_json::json!({
                "request": {
                    "componentId": "fake-worker"
                }
            })
        )
        .expect("semantic worker patch command must succeed")
        .deserialize::<fm_transport_dto::SemanticWorkerPatchResponseDto>()
        .expect("worker patch response must deserialize");
        assert!(patch.receipt.is_none());

        let imported = invoke!(
            "import_semantic_component_local_model",
            serde_json::json!({
                "request": {
                    "sourcePath": "mock/local-model",
                    "modelId": "expert.local",
                    "upstreamRevision": "revision-1",
                    "licenseSpdx": "Apache-2.0",
                    "licenseNotice": "Local model",
                    "tokenizer": "tokenizer-1",
                    "dimensions": 384,
                    "normalization": "unitLength",
                    "runtimeComponentId": "fake-runtime",
                    "runtimeVersionRequirement": "^1.0",
                    "languageCoverage": ["en"],
                    "estimatedDiskBytes": 100,
                    "estimatedRamBytes": 200,
                    "profile": "compactEnglish",
                    "estimate": { "documents": 1, "sourceBytes": 10 }
                }
            })
        )
        .expect("local semantic model import command must succeed")
        .deserialize::<fm_transport_dto::SemanticModelMigrationPlanDto>()
        .expect("local model import response must deserialize");
        assert!(imported.requires_confirmation);

        let plan = invoke!(
            "plan_semantic_component_model_migration",
            serde_json::json!({
                "request": {
                    "profile": "multilingualQuality",
                    "estimate": { "documents": 2, "sourceBytes": 20 }
                }
            })
        )
        .expect("semantic model migration plan command must succeed")
        .deserialize::<fm_transport_dto::SemanticModelMigrationPlanDto>()
        .expect("model migration plan response must deserialize");

        let progress = invoke!(
            "confirm_semantic_component_model_migration",
            serde_json::json!({
                "request": { "migrationId": plan.migration_id }
            })
        )
        .expect("semantic model migration confirmation command must succeed")
        .deserialize::<fm_transport_dto::SemanticModelMigrationProgressDto>()
        .expect("model migration confirmation response must deserialize");
        assert_eq!(progress.completed_documents, 0);

        let progress = invoke!(
            "checkpoint_semantic_component_model_migration",
            serde_json::json!({
                "request": {
                    "migrationId": progress.migration_id,
                    "completedDocuments": 2,
                    "resumeCursor": "cursor-2"
                }
            })
        )
        .expect("semantic model migration checkpoint command must succeed")
        .deserialize::<fm_transport_dto::SemanticModelMigrationProgressDto>()
        .expect("model migration checkpoint response must deserialize");
        assert_eq!(progress.completed_documents, 2);

        let selection = invoke!(
            "complete_semantic_component_model_migration",
            serde_json::json!({
                "request": { "migrationId": progress.migration_id }
            })
        )
        .expect("semantic model migration completion command must succeed")
        .deserialize::<fm_transport_dto::SemanticModelSelectionDto>()
        .expect("model migration completion response must deserialize");
        assert_eq!(
            selection.profile,
            fm_transport_dto::SemanticProfileDto::MultilingualQuality
        );

        let uninstall = invoke!(
            "uninstall_semantic_components",
            serde_json::json!({
                "request": { "indexDecision": "delete" }
            })
        )
        .expect("semantic component uninstall command must succeed")
        .deserialize::<fm_transport_dto::SemanticUninstallReceiptDto>()
        .expect("semantic component uninstall response must deserialize");
        assert_eq!(
            uninstall.index_decision,
            fm_transport_dto::SemanticIndexRetentionDecisionDto::Delete
        );
    }

    #[test]
    fn semantic_ocr_commands_are_registered_and_return_typed_unavailable_errors() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let status = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "get_semantic_ocr_status".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({})),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect("OCR status command must succeed")
        .deserialize::<fm_transport_dto::SemanticOcrStatusDto>()
        .expect("OCR status must deserialize");
        assert!(!status.enabled);
        assert!(matches!(
            status.availability,
            fm_transport_dto::SemanticOcrAvailabilityDto::Unavailable {
                reason: fm_transport_dto::SemanticOcrUnavailableReasonDto::HostUnavailable,
                ..
            }
        ));

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "set_semantic_ocr_consent".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": { "enabled": true }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("unavailable OCR consent must be rejected");
        assert!(error.to_string().contains("\"code\":\"unavailable\""));
        assert!(error.to_string().contains("\"code\":\"hostUnavailable\""));

        let start_error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "start_semantic_ocr_remediation".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": { "scope": "allReported" }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("disabled OCR remediation must reject a job");
        assert!(start_error.to_string().contains("\"code\":\"disabled\""));

        let cancel_error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "cancel_semantic_ocr_remediation".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": { "jobId": "not-a-job-id" }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("malformed OCR job id must reject cancellation");
        assert!(
            cancel_error
                .to_string()
                .contains("\"code\":\"invalidRequest\"")
        );
    }

    #[tokio::test]
    async fn semantic_library_commands_round_trip_through_the_shared_service() {
        let app = create_app_with_semantic_developer_bundle(mock_builder(), true);
        let workspace = app
            .state::<AppState>()
            .service
            .start_workspace(None)
            .await
            .expect("start workspace");
        let pane = workspace
            .panes
            .iter()
            .find(|pane| pane.id == workspace.active_pane_id)
            .expect("active pane");
        let tab = pane
            .tabs
            .iter()
            .find(|tab| tab.id == pane.active_tab_id)
            .expect("active tab");
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let invoke = |command: &'static str, request: serde_json::Value| {
            get_ipc_response(
                &webview,
                InvokeRequest {
                    cmd: command.into(),
                    callback: CallbackFn(0),
                    error: CallbackFn(1),
                    url: local_protocol_url(),
                    body: InvokeBody::Json(request),
                    headers: Default::default(),
                    invoke_key: INVOKE_KEY.to_string(),
                },
            )
        };

        let capabilities = invoke("get_semantic_library_capabilities", serde_json::json!({}))
            .expect("library capabilities")
            .deserialize::<fm_transport_dto::SemanticLibraryCapabilitiesDto>()
            .expect("capabilities DTO");
        assert_eq!(
            capabilities.authority,
            fm_transport_dto::SemanticLibraryAuthorityDto::DeterministicMock
        );

        let preview = invoke(
            "preview_semantic_enrolment",
            serde_json::json!({
                "request": {
                    "workspaceId": workspace.id,
                    "location": tab.location,
                    "recursive": true
                }
            }),
        )
        .expect("enrolment preview")
        .deserialize::<fm_transport_dto::SemanticEnrolmentPreviewDto>()
        .expect("preview DTO");
        assert!(preview.normalized_excerpts_retained_locally);

        let status = invoke(
            "confirm_semantic_enrolment",
            serde_json::json!({
                "request": {
                    "confirmationId": preview.confirmation_id,
                    "policyRevision": preview.policy_revision,
                    "workspaceId": workspace.id,
                    "location": tab.location
                }
            }),
        )
        .expect("confirm enrolment")
        .deserialize::<fm_transport_dto::SemanticLibraryStatusDto>()
        .expect("status DTO");
        assert_eq!(status.roots.len(), 1);
    }

    #[test]
    fn semantic_component_commands_return_the_shared_typed_error() {
        let app = create_app(mock_builder());
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("failed to build mock webview");

        let error = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: "accept_semantic_component_installation_offer".into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: local_protocol_url(),
                body: InvokeBody::Json(serde_json::json!({
                    "request": { "offerId": "unknown-offer" }
                })),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .expect_err("unknown offer must reject the command");

        assert!(error.to_string().contains("consentRequired"));
        assert!(error.to_string().contains("requestId"));
    }
}
