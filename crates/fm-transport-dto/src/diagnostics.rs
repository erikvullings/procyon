//! Diagnostics data transfer objects (spec §30).
//!
//! Provides a comprehensive view of application state for bug reports and troubleshooting.
//! Includes version info, platform detection, runtime capabilities, and recent errors.

use crate::runtime::RuntimeCapabilitiesDto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Diagnostics information for troubleshooting and bug reports (spec §30).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDto {
    /// Frontend version from package.json
    pub frontend_version: String,
    /// Backend version from Cargo.toml
    pub backend_version: String,
    /// Tauri version if running in desktop mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tauri_version: Option<String>,
    /// Platform the backend is running on
    pub platform: String,
    /// Runtime capabilities indicating feature availability
    pub runtime_capabilities: RuntimeCapabilitiesDto,
    /// Current SSE/channel connection state
    pub connection_state: ConnectionStateDto,
    /// Loaded plugins and their status
    pub loaded_plugins: Vec<PluginStatusDto>,
    /// Recent non-sensitive errors (bounded buffer, most recent first)
    pub recent_errors: Vec<DiagnosticErrorDto>,
    /// Operation queue status
    pub operation_queue_status: OperationQueueStatusDto,
}

/// State of the SSE/event channel connection.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStateDto {
    /// Whether the connection is currently active
    pub connected: bool,
    /// Last timestamp an event was received (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_received: Option<String>,
    /// Connection uptime in seconds
    pub uptime_seconds: u64,
    /// Number of events received since last connection
    pub events_received: u64,
    /// Human-readable status message
    pub status_message: String,
}

/// Plugin status in the diagnostics view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatusDto {
    /// Plugin identifier
    pub plugin_id: String,
    /// Plugin display name
    pub name: String,
    /// Whether the plugin is currently enabled
    pub enabled: bool,
    /// Plugin version
    pub version: String,
    /// Number of errors in this plugin's diagnostic log (from fm-plugin-runtime)
    pub error_count: u32,
}

/// A single error entry from the recent errors buffer.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticErrorDto {
    /// ISO 8601 timestamp when the error occurred
    pub timestamp: String,
    /// Error message (redacted of sensitive data)
    pub message: String,
    /// Application code (e.g., "INVALID_PATH", "OPERATION_TIMEOUT")
    pub code: String,
    /// Optional context (e.g., operation ID, plugin ID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Operation queue status for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationQueueStatusDto {
    /// Number of queued operations waiting to run
    pub queued_count: u32,
    /// Number of currently running operations
    pub running_count: u32,
    /// Number of paused operations
    pub paused_count: u32,
    /// Total completed operations since app start
    pub completed_count: u64,
    /// Current total size of all pending operations (approximate bytes)
    pub total_pending_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostics_dto_serialization() {
        let diagnostics = DiagnosticsDto {
            frontend_version: "0.1.0".to_string(),
            backend_version: "0.1.0".to_string(),
            tauri_version: Some("2.0.0".to_string()),
            platform: "macOS".to_string(),
            runtime_capabilities: crate::runtime::RuntimeCapabilitiesDto {
                runtime: crate::runtime::RuntimeKindDto::Tauri,
                platform: crate::runtime::PlatformKindDto::Macos,
                native_menus: true,
                platform_context_menu: true,
                native_file_icons: true,
                native_thumbnails: true,
                native_drag_out: true,
                system_trash: true,
                reveal_in_system_file_manager: true,
                open_terminal: true,
                clipboard: true,
                plugins: true,
                server_administration: false,
                extended_attributes: true,
                finder_tags: true,
            },
            connection_state: ConnectionStateDto {
                connected: true,
                last_event_received: Some("2026-08-10T12:34:56Z".to_string()),
                uptime_seconds: 3600,
                events_received: 42,
                status_message: "Connected".to_string(),
            },
            loaded_plugins: vec![PluginStatusDto {
                plugin_id: "plugin-1".to_string(),
                name: "Test Plugin".to_string(),
                enabled: true,
                version: "1.0.0".to_string(),
                error_count: 0,
            }],
            recent_errors: vec![DiagnosticErrorDto {
                timestamp: "2026-08-10T12:34:50Z".to_string(),
                message: "Sample error message".to_string(),
                code: "TEST_ERROR".to_string(),
                context: Some("op-123".to_string()),
            }],
            operation_queue_status: OperationQueueStatusDto {
                queued_count: 1,
                running_count: 1,
                paused_count: 0,
                completed_count: 42,
                total_pending_size: 1024 * 1024,
            },
        };

        // Test serialization
        let json = serde_json::to_value(&diagnostics).expect("serialization failed");
        assert_eq!(json["frontendVersion"], "0.1.0");
        assert_eq!(json["backendVersion"], "0.1.0");
        assert_eq!(json["tauriVersion"], "2.0.0");
        assert_eq!(json["platform"], "macOS");

        // Test round-trip
        let _round_trip: DiagnosticsDto =
            serde_json::from_value(json).expect("deserialization failed");
    }

    #[test]
    fn test_connection_state_dto() {
        let state = ConnectionStateDto {
            connected: false,
            last_event_received: None,
            uptime_seconds: 0,
            events_received: 0,
            status_message: "Disconnected".to_string(),
        };

        let json = serde_json::to_value(&state).expect("serialization failed");
        assert!(!json["connected"].as_bool().unwrap());
        assert!(json.get("lastEventReceived").is_none());
    }

    #[test]
    fn test_diagnostic_error_dto_redaction() {
        let error = DiagnosticErrorDto {
            timestamp: "2026-08-10T12:34:50Z".to_string(),
            message: "Failed to process /Users/alice/file.txt".to_string(),
            code: "FILE_ERROR".to_string(),
            context: Some("op-456".to_string()),
        };

        let json = serde_json::to_value(&error).expect("serialization failed");
        let serialized = serde_json::to_string(&error).expect("string serialize");

        // Path should be visible in error (redaction happens at logging level, not DTO level)
        assert!(serialized.contains("file.txt"));
        assert_eq!(json["code"], "FILE_ERROR");
    }

    #[test]
    fn test_operation_queue_status_dto() {
        let status = OperationQueueStatusDto {
            queued_count: 5,
            running_count: 2,
            paused_count: 1,
            completed_count: 100,
            total_pending_size: 5 * 1024 * 1024,
        };

        let json = serde_json::to_value(&status).expect("serialization failed");
        assert_eq!(json["queuedCount"], 5);
        assert_eq!(json["runningCount"], 2);
        assert_eq!(json["pausedCount"], 1);
        assert_eq!(json["completedCount"], 100);
    }
}
