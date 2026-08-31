//! The structured error DTO shared by every endpoint (spec §8).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A closed set of machine-readable error codes.
///
/// Codes are stable and additive: existing variants are never renamed or
/// removed, only appended to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ApplicationErrorCode {
    /// The requested resource does not exist.
    NotFound,
    /// The current user is not permitted to perform this action.
    PermissionDenied,
    /// The entry is held open by another program (task 0060, spec §8/§23).
    FileLocked,
    /// The request itself was malformed or failed validation.
    InvalidRequest,
    /// The operation's destination already exists.
    DestinationAlreadyExists,
    /// The location's provider is not available or not registered.
    ProviderUnavailable,
    /// The operation was cancelled by the caller.
    OperationCancelled,
    /// The archive is encrypted and needs a password for this backend session.
    CredentialRequired,
    /// The supplied archive password was rejected.
    InvalidCredential,
    /// A workspace mutation's `expected_revision` no longer matches the
    /// stored revision (spec §5.3.10).
    WorkspaceRevisionConflict,
    /// A file changed after its editable content was loaded.
    FileRevisionConflict,
    /// No action is registered with the requested id (spec §18).
    ActionNotFound,
    /// The action is registered but not currently invokable (spec §18).
    ActionUnavailable,
    /// A native platform operation (open/reveal/terminal) failed for a
    /// reason safe to show the user, e.g. no default application or the
    /// configured terminal was not found (spec §21, task 0061).
    PlatformOperationFailed,
    /// An SSH host key has never been verified before (task 0104, spec
    /// §6.4); the caller must explicitly accept it.
    HostKeyUnverified,
    /// An SSH host key changed since it was last accepted (task 0104, spec
    /// §6.4); never silently accepted.
    HostKeyMismatch,
    /// An unexpected, unclassified failure occurred.
    Internal,
}

/// A structured, user-facing error, never a raw OS error string (spec §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "code": "destinationAlreadyExists",
    "message": "A file named report.pdf already exists.",
    "requestId": "e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e",
    "details": {"destination": "file:///Users/erik/Documents/report.pdf"}
}))]
pub struct ApplicationErrorDto {
    /// A stable, machine-readable error code.
    pub code: ApplicationErrorCode,
    /// A user-readable description, never a raw OS error.
    pub message: String,
    /// Correlates this error with the request that produced it.
    pub request_id: Uuid,
    /// Additional, code-specific structured context.
    #[schema(value_type = Option<Object>)]
    pub details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ApplicationErrorDto {
        ApplicationErrorDto {
            code: ApplicationErrorCode::DestinationAlreadyExists,
            message: "A file named report.pdf already exists.".to_owned(),
            request_id: Uuid::new_v4(),
            details: Some(
                serde_json::json!({"destination": "file:///Users/erik/Documents/report.pdf"}),
            ),
        }
    }

    #[test]
    fn application_error_dto_round_trips_through_serde_json() {
        let dto = sample();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: ApplicationErrorDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn application_error_dto_matches_the_spec_example_shape() {
        let json = serde_json::to_string(&sample()).expect("serialization must succeed");
        assert!(json.contains("\"code\":\"destinationAlreadyExists\""));
        assert!(json.contains("\"requestId\""));
        assert!(json.contains("\"details\""));
    }

    #[test]
    fn application_error_dto_allows_details_to_be_absent() {
        let dto = ApplicationErrorDto {
            code: ApplicationErrorCode::NotFound,
            message: "Not found.".to_owned(),
            request_id: Uuid::new_v4(),
            details: None,
        };
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: ApplicationErrorDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn application_error_code_never_leaks_raw_os_error_text() {
        for code in [
            ApplicationErrorCode::NotFound,
            ApplicationErrorCode::PermissionDenied,
            ApplicationErrorCode::FileLocked,
            ApplicationErrorCode::InvalidRequest,
            ApplicationErrorCode::DestinationAlreadyExists,
            ApplicationErrorCode::ProviderUnavailable,
            ApplicationErrorCode::OperationCancelled,
            ApplicationErrorCode::WorkspaceRevisionConflict,
            ApplicationErrorCode::FileRevisionConflict,
            ApplicationErrorCode::ActionNotFound,
            ApplicationErrorCode::ActionUnavailable,
            ApplicationErrorCode::PlatformOperationFailed,
            ApplicationErrorCode::HostKeyUnverified,
            ApplicationErrorCode::HostKeyMismatch,
            ApplicationErrorCode::Internal,
        ] {
            let json = serde_json::to_string(&code).expect("serialization must succeed");
            assert!(json.starts_with('"') && json.ends_with('"'));
        }
    }

    #[test]
    fn workspace_revision_conflict_code_serializes_to_the_spec_string() {
        let json = serde_json::to_string(&ApplicationErrorCode::WorkspaceRevisionConflict)
            .expect("serialization must succeed");
        assert_eq!(json, "\"workspaceRevisionConflict\"");
    }

    #[test]
    fn action_error_codes_serialize_to_the_spec_18_strings() {
        assert_eq!(
            serde_json::to_string(&ApplicationErrorCode::ActionNotFound)
                .expect("serialization must succeed"),
            "\"actionNotFound\""
        );
        assert_eq!(
            serde_json::to_string(&ApplicationErrorCode::ActionUnavailable)
                .expect("serialization must succeed"),
            "\"actionUnavailable\""
        );
    }

    #[test]
    fn platform_operation_failed_code_serializes_to_the_spec_18_string() {
        assert_eq!(
            serde_json::to_string(&ApplicationErrorCode::PlatformOperationFailed)
                .expect("serialization must succeed"),
            "\"platformOperationFailed\""
        );
    }
}
