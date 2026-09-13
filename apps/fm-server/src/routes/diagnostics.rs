//! `GET /api/v1/diagnostics` - Diagnostics view for troubleshooting (spec §30).

use axum::{Json, extract::State, http::StatusCode};
use fm_transport_dto::{
    ConnectionStateDto, DiagnosticErrorDto, DiagnosticsDto, OperationQueueStatusDto,
    PluginStatusDto,
};

/// Provides comprehensive diagnostics information for troubleshooting and bug reports.
///
/// Includes version info, platform detection, runtime capabilities, and recent errors.
/// All paths and sensitive fields are redacted for safe sharing in bug reports.
#[utoipa::path(
    get,
    path = "/api/v1/diagnostics",
    operation_id = "getDiagnostics",
    responses((status = 200, description = "Diagnostics information", body = DiagnosticsDto))
)]
pub(crate) async fn get_diagnostics(
    State(state): State<crate::state::AppState>,
) -> Json<DiagnosticsDto> {
    // Get runtime capabilities from the service
    let runtime_capabilities = state
        .service
        .runtime_capabilities_with_semantic_components()
        .await;

    // Build plugin status
    let plugins = state.service.list_plugins();
    let loaded_plugins: Vec<PluginStatusDto> = plugins
        .iter()
        .map(|plugin| PluginStatusDto {
            plugin_id: plugin.id.clone(),
            name: plugin.name.clone(),
            enabled: plugin.enabled,
            version: plugin.version.clone(),
            error_count: 0, // TODO: get from plugin runtime error log
        })
        .collect();

    // Get operation queue status from operation page (placeholder for now)
    // TODO: Implement proper operation queue status tracking
    let operation_queue_status = OperationQueueStatusDto {
        queued_count: 0,
        running_count: 0,
        paused_count: 0,
        completed_count: 0,
        total_pending_size: 0,
    };

    // Get connection state from AppState
    let (connected, last_event_received, uptime_seconds, events_received) =
        state.connection_state.snapshot();
    let connection_state = ConnectionStateDto {
        connected,
        last_event_received,
        uptime_seconds,
        events_received,
        status_message: if connected {
            "Connected".to_string()
        } else {
            "Disconnected".to_string()
        },
    };

    // Get recent errors from error buffer
    let recent_errors = state.error_buffer.recent();

    // Convert platform to string representation
    let platform_str = match runtime_capabilities.platform {
        fm_transport_dto::PlatformKindDto::Macos => "macOS".to_string(),
        fm_transport_dto::PlatformKindDto::Windows => "Windows".to_string(),
        fm_transport_dto::PlatformKindDto::Linux => "Linux".to_string(),
        fm_transport_dto::PlatformKindDto::Unknown => "Unknown".to_string(),
    };

    Json(DiagnosticsDto {
        frontend_version: env!("CARGO_PKG_VERSION").to_string(),
        backend_version: env!("CARGO_PKG_VERSION").to_string(),
        tauri_version: None, // Set by Tauri host in desktop mode
        platform: platform_str,
        runtime_capabilities,
        connection_state,
        loaded_plugins,
        recent_errors,
        operation_queue_status,
    })
}

/// Retains one redacted frontend error for the diagnostics view and structured host log.
#[utoipa::path(
    post,
    path = "/api/v1/diagnostics/frontend-errors",
    operation_id = "recordFrontendDiagnostic",
    request_body = DiagnosticErrorDto,
    responses((status = 204, description = "Frontend diagnostic recorded"))
)]
pub(crate) async fn record_frontend_diagnostic(
    State(state): State<crate::state::AppState>,
    Json(error): Json<DiagnosticErrorDto>,
) -> StatusCode {
    let error = state.error_buffer.record_frontend(error);
    tracing::error!(
        target: "frontend",
        code = %error.code,
        context = error.context.as_deref().unwrap_or_default(),
        message = %error.message,
        "frontend diagnostic"
    );
    StatusCode::NO_CONTENT
}
