//! Library surface for the Axum host (task 0008).
//!
//! Split out from `main.rs` so integration tests can build the exact router
//! `main` serves and drive it with `axum::serve` on an ephemeral port.

pub mod accessible_roots;
pub mod audit;
pub mod auth;
pub mod config;
mod credentials;
mod error;
pub mod openapi_export;
mod platform;
mod rate_limit;
mod routes;
mod state;

use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method};
use fm_application::FileManagerService;
use fm_events::EventBus;
use fm_transport_dto::RuntimeKindDto;
use tokio_util::sync::CancellationToken;
use tower::ServiceBuilder;
use tower_http::ServiceBuilderExt;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::request_id::{MakeRequestUuid, RequestId};
use tower_http::trace::TraceLayer;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use config::ServerConfig;
use state::AppState;

/// Registers every route once, shared by [`build_router`] (which serves the
/// resulting document at `/api/v1/openapi.json`) and [`openapi_document`]
/// (which the `export-openapi` CLI command uses). Sharing one builder means
/// the served and exported documents can never drift apart.
fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(routes::api_doc())
        .routes(utoipa_axum::routes!(routes::health::get_health))
        .routes(utoipa_axum::routes!(routes::diagnostics::get_diagnostics))
        .routes(utoipa_axum::routes!(routes::events::get_events))
        .routes(utoipa_axum::routes!(
            routes::runtime::get_runtime_capabilities
        ))
        .routes(utoipa_axum::routes!(routes::settings::get_settings))
        .routes(utoipa_axum::routes!(
            routes::system_location::get_system_locations
        ))
        .routes(utoipa_axum::routes!(routes::volume::get_volumes))
        .routes(utoipa_axum::routes!(routes::settings::update_settings))
        .routes(utoipa_axum::routes!(routes::directory::list_directory))
        .routes(utoipa_axum::routes!(routes::directory::refresh_directory))
        .routes(utoipa_axum::routes!(
            routes::directory::list_directory_children
        ))
        .routes(utoipa_axum::routes!(routes::directory::navigate_pane))
        .routes(utoipa_axum::routes!(routes::directory::get_entry_metadata))
        .routes(utoipa_axum::routes!(routes::directory::set_pane_activity))
        .routes(utoipa_axum::routes!(routes::files::read_file_range))
        .routes(utoipa_axum::routes!(routes::files::open_structured_view))
        .routes(utoipa_axum::routes!(routes::files::structured_view_status))
        .routes(utoipa_axum::routes!(routes::files::update_structured_view))
        .routes(utoipa_axum::routes!(routes::files::read_structured_rows))
        .routes(utoipa_axum::routes!(
            routes::files::read_structured_json_window
        ))
        .routes(utoipa_axum::routes!(routes::files::search_structured_rows))
        .routes(utoipa_axum::routes!(routes::files::close_structured_view))
        .routes(utoipa_axum::routes!(routes::files::load_editable_file))
        .routes(utoipa_axum::routes!(routes::files::save_editable_file))
        .routes(utoipa_axum::routes!(routes::files::search_in_file))
        .routes(utoipa_axum::routes!(routes::files::calculate_folder_size))
        .routes(utoipa_axum::routes!(routes::files::archive_summary))
        .routes(utoipa_axum::routes!(routes::files::scan_disk_usage))
        .routes(utoipa_axum::routes!(routes::files::cancel_disk_usage))
        .routes(utoipa_axum::routes!(routes::files::get_file_git_history))
        .routes(utoipa_axum::routes!(routes::files::cache_archive_password))
        .routes(utoipa_axum::routes!(
            routes::application_uninstall::discover_application_uninstall_candidates
        ))
        .routes(utoipa_axum::routes!(
            routes::application_uninstall::remove_application_dock_icon
        ))
        .routes(utoipa_axum::routes!(routes::icons::get_file_icon))
        .routes(utoipa_axum::routes!(routes::thumbnails::get_thumbnail))
        .routes(utoipa_axum::routes!(
            routes::extended_attributes::get_finder_tags
        ))
        .routes(utoipa_axum::routes!(
            routes::extended_attributes::set_finder_tags
        ))
        .routes(utoipa_axum::routes!(
            routes::extended_attributes::get_spotlight_comment
        ))
        .routes(utoipa_axum::routes!(
            routes::extended_attributes::set_spotlight_comment
        ))
        .routes(utoipa_axum::routes!(routes::search::start_search))
        .routes(utoipa_axum::routes!(routes::search::cancel_search))
        .routes(utoipa_axum::routes!(routes::comparison::start_comparison))
        .routes(utoipa_axum::routes!(routes::comparison::get_comparison))
        .routes(utoipa_axum::routes!(routes::comparison::cancel_comparison))
        .routes(utoipa_axum::routes!(routes::comparison::generate_sync_plan))
        .routes(utoipa_axum::routes!(routes::comparison::apply_sync_plan))
        .routes(utoipa_axum::routes!(routes::checksum::start_checksums))
        .routes(utoipa_axum::routes!(routes::checksum::get_checksums))
        .routes(utoipa_axum::routes!(routes::checksum::cancel_checksums))
        .routes(utoipa_axum::routes!(routes::checksum::render_checksum_file))
        .routes(utoipa_axum::routes!(routes::checksum::save_checksum_file))
        .routes(utoipa_axum::routes!(routes::checksum::verify_checksum_file))
        .routes(utoipa_axum::routes!(routes::checksum::start_duplicate_scan))
        .routes(utoipa_axum::routes!(routes::checksum::get_duplicate_scan))
        .routes(utoipa_axum::routes!(
            routes::checksum::cancel_duplicate_scan
        ))
        .routes(utoipa_axum::routes!(routes::workspace::list_workspaces))
        .routes(utoipa_axum::routes!(routes::workspace::start_workspace))
        .routes(utoipa_axum::routes!(routes::operation::list_operations))
        .routes(utoipa_axum::routes!(routes::operation::start_operation))
        .routes(utoipa_axum::routes!(routes::operation::get_operation))
        .routes(utoipa_axum::routes!(routes::operation::cancel_operation))
        .routes(utoipa_axum::routes!(routes::operation::undo_operation))
        .routes(utoipa_axum::routes!(routes::operation::pause_operation))
        .routes(utoipa_axum::routes!(routes::operation::resume_operation))
        .routes(utoipa_axum::routes!(
            routes::operation::resolve_operation_conflict
        ))
        .routes(utoipa_axum::routes!(routes::action::list_actions))
        .routes(utoipa_axum::routes!(routes::action::invoke_action))
        .routes(utoipa_axum::routes!(routes::plugin::list_plugins))
        .routes(utoipa_axum::routes!(routes::plugin::enable_plugin))
        .routes(utoipa_axum::routes!(routes::plugin::disable_plugin))
        .routes(utoipa_axum::routes!(routes::plugin::get_plugin_logs))
        .routes(utoipa_axum::routes!(
            routes::plugin::get_plugin_icon_theme_asset
        ))
        .routes(utoipa_axum::routes!(routes::workspace::create_workspace))
        .routes(utoipa_axum::routes!(routes::workspace::get_workspace))
        .routes(utoipa_axum::routes!(routes::workspace::delete_workspace))
        .routes(utoipa_axum::routes!(routes::workspace::open_workspace))
        .routes(utoipa_axum::routes!(
            routes::workspace::apply_workspace_command
        ))
        .routes(utoipa_axum::routes!(routes::connection::list_connections))
        .routes(utoipa_axum::routes!(routes::connection::create_connection))
        .routes(utoipa_axum::routes!(routes::connection::get_connection))
        .routes(utoipa_axum::routes!(routes::connection::update_connection))
        .routes(utoipa_axum::routes!(routes::connection::delete_connection))
        .routes(utoipa_axum::routes!(routes::connection::connect_connection))
        .routes(utoipa_axum::routes!(
            routes::connection::disconnect_connection
        ))
        .routes(utoipa_axum::routes!(routes::connection::test_connection))
        .routes(utoipa_axum::routes!(routes::connection::probe_ssh_host_key))
        .routes(utoipa_axum::routes!(
            routes::connection::accept_ssh_host_key
        ))
        .routes(utoipa_axum::routes!(
            routes::onedrive_authorization::begin_onedrive_authorization
        ))
        .routes(utoipa_axum::routes!(
            routes::onedrive_authorization::get_onedrive_authorization_attempt
        ))
        .routes(utoipa_axum::routes!(
            routes::onedrive_authorization::cancel_onedrive_authorization
        ))
}

/// Builds the OpenAPI document without constructing application state or
/// binding a port (task 0009).
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    api_router().into_openapi()
}

/// Builds the Axum [`Router`] for the given configuration.
///
/// Wires tracing, request-size limits, CORS, request correlation ids and the
/// OpenAPI/Swagger routes (spec §8, §9, §22, §30). Handlers only translate a
/// request into a [`FileManagerService`] call and back into a DTO; no
/// filesystem logic lives in this crate (spec §3 rule 2).
pub fn build_router(config: &ServerConfig) -> Router {
    // Browser clients browse the server process's filesystem, so discovery and
    // platform actions describe that machine rather than the browser device.
    let service = Arc::new(
        FileManagerService::with_platform_adapter_and_credential_store_and_search_accelerator(
            RuntimeKindDto::BrowserServer,
            config.workspace_directory.clone(),
            config.settings_directory.clone(),
            EventBus::default(),
            platform::build_platform_adapter(),
            credentials::build_credential_store(),
            platform::build_search_accelerator(),
        ),
    );
    build_router_with_service(config, service)
}

/// Builds the Axum router around an existing application service.
///
/// Hosts and integration tests can use this to share one event bus with
/// non-HTTP adapters while keeping all application logic in `fm-application`.
pub fn build_router_with_service(
    config: &ServerConfig,
    service: Arc<FileManagerService>,
) -> Router {
    build_router_with_service_and_session(config, service, DevelopmentSession::new())
}

/// Handle for ending the explicit local development session.
#[derive(Clone)]
pub struct DevelopmentSession {
    cancellation: CancellationToken,
}

impl DevelopmentSession {
    /// Creates a live development session.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
        }
    }

    /// Ends the session and closes every attached event stream.
    pub fn end(&self) {
        self.cancellation.cancel();
    }
}

impl Default for DevelopmentSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds a router with a controllable local development session.
pub fn build_router_with_service_and_session(
    config: &ServerConfig,
    service: Arc<FileManagerService>,
    session: DevelopmentSession,
) -> Router {
    let state = AppState {
        service,
        cors_allowed_origins: Arc::from(config.cors_allowed_origins.clone()),
        session_end: session.cancellation,
        error_buffer: state::ErrorBuffer::new(),
        connection_state: state::ConnectionState::new(),
        session_manager: Arc::new(auth::SessionManager::new(
            config.session_secret.clone(),
            config.dev_mode_auth_disabled,
        )),
        accessible_roots: Arc::from(config.roots.clone()),
        mutation_limiter: rate_limit::build_limiter(config.max_mutations_per_second),
    };

    let (router, api) = api_router().split_for_parts();
    let router = router
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit::limit_mutations,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ))
        .with_state(state);

    // Each `Router::layer` call re-erases the response body back to
    // `axum::body::Body`, so layers that change the body type (like the
    // request-size limit) must be applied as separate calls rather than
    // composed into one `tower::ServiceBuilder` ahead of `CorsLayer`, which
    // requires a `Default` response body.
    let request_id_and_trace = ServiceBuilder::new()
        .set_x_request_id(MakeRequestUuid)
        .layer(TraceLayer::new_for_http().make_span_with(trace_span))
        .propagate_x_request_id();

    router
        .merge(SwaggerUi::new("/api/v1/docs").url("/api/v1/openapi.json", api))
        .fallback(error::not_found)
        .layer(RequestBodyLimitLayer::new(config.max_body_bytes))
        .layer(request_id_and_trace)
        .layer(cors_layer(config))
}

/// Builds a `tracing` span for each request, carrying the correlation id set
/// by [`MakeRequestUuid`] and the request duration/status recorded by
/// `TraceLayer`'s default callbacks (spec §30). Additional context fields
/// (operation_id, workspace_id, plugin_id, provider_id) may be recorded by
/// handlers as they become known during request processing.
fn trace_span(request: &axum::http::Request<axum::body::Body>) -> tracing::Span {
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("unknown");

    tracing::info_span!(
        "http_request",
        method = %request.method(),
        uri = %request.uri(),
        request_id,
        operation_id = tracing::field::Empty,
        workspace_id = tracing::field::Empty,
        plugin_id = tracing::field::Empty,
        provider_id = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
        result = tracing::field::Empty,
    )
}

/// Builds the CORS layer from the configured allow-list. An empty allow-list
/// blocks every cross-origin request; a wildcard origin is never used (spec
/// §22).
fn cors_layer(config: &ServerConfig) -> CorsLayer {
    let origins: Vec<HeaderValue> = config
        .cors_allowed_origins
        .iter()
        .filter_map(|origin| HeaderValue::from_str(origin).ok())
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            HeaderName::from_static("idempotency-key"),
        ])
}
