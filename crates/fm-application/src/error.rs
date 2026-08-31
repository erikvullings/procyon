//! Application-level errors shared by every host (specification §7, §8).

use fm_connections::ConnectionError;
use fm_credentials::CredentialError;
use fm_domain::ActionId;
use fm_transport_dto::{ApplicationErrorCode, ApplicationErrorDto};
use fm_vfs::VfsError;
use uuid::Uuid;

use crate::workspace::WorkspaceError;

/// Failure modes the application service layer can report to a host.
///
/// Hosts translate these into transport-specific responses. The service layer
/// itself never leaks raw OS or filesystem error details (spec §8); each
/// variant carries only a user-readable message.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApplicationError {
    /// The requested resource does not exist.
    #[error("resource not found")]
    NotFound,
    /// The current caller is not permitted to perform this action.
    #[error("permission denied")]
    PermissionDenied,
    /// The entry is held open by another program and cannot be read or
    /// modified until that program releases it (task 0060, spec §8/§23).
    #[error("The file is in use by another program.")]
    FileLocked,
    /// The request failed validation.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The operation's destination already exists.
    #[error("destination already exists")]
    DestinationAlreadyExists,
    /// The location's provider is unavailable or not registered.
    #[error("provider unavailable")]
    ProviderUnavailable,
    /// The operation was cancelled by the caller.
    #[error("operation cancelled")]
    OperationCancelled,
    /// The archive is encrypted and needs a password for this backend session.
    #[error("archive password required")]
    CredentialRequired,
    /// The supplied archive password was rejected.
    #[error("archive password is incorrect")]
    InvalidCredential,
    /// A workspace mutation's `expected_revision` no longer matches the
    /// stored revision (spec §5.3.10).
    #[error("The workspace changed after this view was loaded.")]
    WorkspaceRevisionConflict {
        /// The workspace whose revision changed.
        workspace_id: uuid::Uuid,
        /// The revision the caller last observed.
        expected_revision: u64,
        /// The revision actually stored.
        actual_revision: u64,
    },
    /// Editable file content changed after it was loaded.
    #[error("The file changed after it was loaded.")]
    FileRevisionConflict {
        /// Revision supplied by the editor session.
        expected_revision: String,
        /// Revision computed from the current file bytes.
        actual_revision: String,
    },
    /// No action is registered with this id (spec §18).
    #[error("unknown action {0:?}")]
    ActionNotFound(ActionId),
    /// The action is registered but not currently invokable, either because
    /// its feature is not implemented yet or its context requirements are
    /// not met by the caller's invocation context (spec §18).
    #[error("action {0:?} is not currently available")]
    ActionUnavailable(ActionId),
    /// A native platform operation (open/reveal/terminal) failed for a
    /// reason safe to show the user, e.g. no default application or the
    /// configured terminal was not found (spec §21, task 0061). Never wraps
    /// a raw OS error string directly; the message is already sanitized by
    /// [`fm_platform::PlatformError`].
    #[error("{0}")]
    PlatformOperationFailed(String),
    /// An SSH host key has never been verified before (task 0104, spec
    /// §6.4); the caller must explicitly accept it (see
    /// `FileManagerService::accept_ssh_host_key`) before a connect attempt
    /// can succeed.
    #[error("host key {fingerprint} has not been verified yet")]
    HostKeyUnverified {
        /// `SHA256:<base64>` fingerprint of the presented host key.
        fingerprint: String,
    },
    /// An SSH host key changed since it was last accepted (task 0104, spec
    /// §6.4); never silently accepted.
    #[error("host key {fingerprint} does not match the previously accepted {expected_fingerprint}")]
    HostKeyMismatch {
        /// `SHA256:<base64>` fingerprint the host presented this time.
        fingerprint: String,
        /// `SHA256:<base64>` fingerprint previously accepted and stored.
        expected_fingerprint: String,
    },
    /// An unexpected, unclassified failure occurred.
    #[error("internal error")]
    Internal,
}

impl ApplicationError {
    /// The stable, machine-readable code for this error (spec §8).
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::NotFound => ApplicationErrorCode::NotFound,
            Self::PermissionDenied => ApplicationErrorCode::PermissionDenied,
            Self::FileLocked => ApplicationErrorCode::FileLocked,
            Self::InvalidRequest(_) => ApplicationErrorCode::InvalidRequest,
            Self::DestinationAlreadyExists => ApplicationErrorCode::DestinationAlreadyExists,
            Self::ProviderUnavailable => ApplicationErrorCode::ProviderUnavailable,
            Self::OperationCancelled => ApplicationErrorCode::OperationCancelled,
            Self::CredentialRequired => ApplicationErrorCode::CredentialRequired,
            Self::InvalidCredential => ApplicationErrorCode::InvalidCredential,
            Self::WorkspaceRevisionConflict { .. } => {
                ApplicationErrorCode::WorkspaceRevisionConflict
            }
            Self::FileRevisionConflict { .. } => ApplicationErrorCode::FileRevisionConflict,
            Self::ActionNotFound(_) => ApplicationErrorCode::ActionNotFound,
            Self::ActionUnavailable(_) => ApplicationErrorCode::ActionUnavailable,
            Self::PlatformOperationFailed(_) => ApplicationErrorCode::PlatformOperationFailed,
            Self::HostKeyUnverified { .. } => ApplicationErrorCode::HostKeyUnverified,
            Self::HostKeyMismatch { .. } => ApplicationErrorCode::HostKeyMismatch,
            Self::Internal => ApplicationErrorCode::Internal,
        }
    }

    /// Builds the transport DTO for this error, tagged with the request that
    /// produced it (spec §8).
    pub fn into_dto(self, request_id: Uuid) -> ApplicationErrorDto {
        let details = match &self {
            Self::WorkspaceRevisionConflict {
                workspace_id,
                expected_revision,
                actual_revision,
            } => Some(serde_json::json!({
                "workspaceId": workspace_id,
                "expectedRevision": expected_revision,
                "actualRevision": actual_revision,
            })),
            Self::FileRevisionConflict {
                expected_revision,
                actual_revision,
            } => Some(serde_json::json!({
                "expectedRevision": expected_revision,
                "actualRevision": actual_revision,
            })),
            Self::ActionNotFound(action_id) | Self::ActionUnavailable(action_id) => {
                Some(serde_json::json!({ "actionId": action_id.as_str() }))
            }
            Self::HostKeyUnverified { fingerprint } => Some(serde_json::json!({
                "fingerprint": fingerprint,
            })),
            Self::HostKeyMismatch {
                fingerprint,
                expected_fingerprint,
            } => Some(serde_json::json!({
                "fingerprint": fingerprint,
                "expectedFingerprint": expected_fingerprint,
            })),
            _ => None,
        };
        ApplicationErrorDto {
            code: self.code(),
            message: self.to_string(),
            request_id,
            details,
        }
    }
}

impl From<WorkspaceError> for ApplicationError {
    fn from(error: WorkspaceError) -> Self {
        match error {
            WorkspaceError::NotFound { .. } => Self::NotFound,
            WorkspaceError::RevisionConflict {
                id,
                expected,
                actual,
            } => Self::WorkspaceRevisionConflict {
                workspace_id: id.into(),
                expected_revision: expected.unwrap_or(0),
                actual_revision: actual,
            },
            WorkspaceError::Invalid(details) => {
                Self::InvalidRequest(format!("workspace failed validation: {details:?}"))
            }
            WorkspaceError::UnsupportedSchemaVersion { .. } => Self::Internal,
            WorkspaceError::Corrupt { .. } => Self::Internal,
            WorkspaceError::Io(_) => Self::Internal,
            WorkspaceError::Serialization(_) => Self::Internal,
            WorkspaceError::PaneNotFound { .. } => {
                Self::InvalidRequest("pane does not exist in this workspace".to_owned())
            }
            WorkspaceError::TabNotFound { .. } => {
                Self::InvalidRequest("tab does not exist in this pane".to_owned())
            }
            WorkspaceError::InvalidCommand(message) => Self::InvalidRequest(message),
            WorkspaceError::DuplicateName { name } => {
                Self::InvalidRequest(format!("a workspace named {name:?} already exists"))
            }
            WorkspaceError::NotEphemeral { id } => {
                Self::InvalidRequest(format!("workspace {id} is not an ephemeral workspace"))
            }
            WorkspaceError::TargetIsEphemeral { id } => Self::InvalidRequest(format!(
                "workspace {id} is ephemeral and cannot be a resync target"
            )),
        }
    }
}

impl From<ConnectionError> for ApplicationError {
    fn from(error: ConnectionError) -> Self {
        match error {
            ConnectionError::NotFound { .. } => Self::NotFound,
            ConnectionError::Invalid(details) => {
                Self::InvalidRequest(format!("connection failed validation: {details:?}"))
            }
            ConnectionError::Io(_)
            | ConnectionError::Serialization(_)
            | ConnectionError::Corrupt { .. } => Self::Internal,
            ConnectionError::Credential(credential_error) => match credential_error {
                CredentialError::NotFound { .. } => Self::CredentialRequired,
                CredentialError::Unavailable(message) | CredentialError::Backend(message) => {
                    Self::PlatformOperationFailed(message)
                }
            },
            ConnectionError::HostKeyUnverified { fingerprint } => {
                Self::HostKeyUnverified { fingerprint }
            }
            ConnectionError::HostKeyMismatch {
                fingerprint,
                expected_fingerprint,
            } => Self::HostKeyMismatch {
                fingerprint,
                expected_fingerprint,
            },
            // In practice `ConnectionService::evaluate` always turns this
            // into `Ok(ConnectionStatus::Failed)` plus a stored
            // `last_error` message rather than letting it propagate as an
            // `Err` - this arm exists only for match exhaustiveness.
            ConnectionError::DialFailed(message) => Self::PlatformOperationFailed(message),
        }
    }
}

impl From<VfsError> for ApplicationError {
    fn from(error: VfsError) -> Self {
        match error {
            VfsError::NotFound { .. } => Self::NotFound,
            VfsError::PermissionDenied { .. } => Self::PermissionDenied,
            VfsError::Locked { .. } => Self::FileLocked,
            VfsError::AlreadyExists { .. } => Self::DestinationAlreadyExists,
            VfsError::Cancelled => Self::OperationCancelled,
            VfsError::UnknownProvider { .. } => Self::ProviderUnavailable,
            VfsError::InvalidLocation { .. }
            | VfsError::EmptyName
            | VfsError::InvalidNameCharacters
            | VfsError::ReservedName
            | VfsError::PathTraversalName
            | VfsError::NotADirectory { .. }
            | VfsError::IsADirectory { .. }
            | VfsError::UnsupportedCapability { .. } => {
                Self::InvalidRequest("the requested filesystem operation is not valid".to_owned())
            }
            VfsError::CredentialRequired => Self::CredentialRequired,
            VfsError::InvalidCredential => Self::InvalidCredential,
            VfsError::Io { .. }
            | VfsError::UnsafeArchiveEntry
            | VfsError::ArchiveResourceLimit { .. } => Self::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_maps_to_its_matching_error_code() {
        let cases = [
            (ApplicationError::NotFound, ApplicationErrorCode::NotFound),
            (
                ApplicationError::PermissionDenied,
                ApplicationErrorCode::PermissionDenied,
            ),
            (
                ApplicationError::InvalidRequest("bad field".to_owned()),
                ApplicationErrorCode::InvalidRequest,
            ),
            (
                ApplicationError::DestinationAlreadyExists,
                ApplicationErrorCode::DestinationAlreadyExists,
            ),
            (
                ApplicationError::ProviderUnavailable,
                ApplicationErrorCode::ProviderUnavailable,
            ),
            (
                ApplicationError::OperationCancelled,
                ApplicationErrorCode::OperationCancelled,
            ),
            (
                ApplicationError::CredentialRequired,
                ApplicationErrorCode::CredentialRequired,
            ),
            (
                ApplicationError::InvalidCredential,
                ApplicationErrorCode::InvalidCredential,
            ),
            (
                ApplicationError::WorkspaceRevisionConflict {
                    workspace_id: uuid::Uuid::new_v4(),
                    expected_revision: 14,
                    actual_revision: 16,
                },
                ApplicationErrorCode::WorkspaceRevisionConflict,
            ),
            (
                ApplicationError::ActionNotFound(ActionId::new("core.missing")),
                ApplicationErrorCode::ActionNotFound,
            ),
            (
                ApplicationError::ActionUnavailable(ActionId::new("core.rename")),
                ApplicationErrorCode::ActionUnavailable,
            ),
            (
                ApplicationError::PlatformOperationFailed("no default application".to_owned()),
                ApplicationErrorCode::PlatformOperationFailed,
            ),
            (
                ApplicationError::HostKeyUnverified {
                    fingerprint: "SHA256:abc".to_owned(),
                },
                ApplicationErrorCode::HostKeyUnverified,
            ),
            (
                ApplicationError::HostKeyMismatch {
                    fingerprint: "SHA256:new".to_owned(),
                    expected_fingerprint: "SHA256:old".to_owned(),
                },
                ApplicationErrorCode::HostKeyMismatch,
            ),
            (ApplicationError::Internal, ApplicationErrorCode::Internal),
        ];

        for (error, expected_code) in cases {
            assert_eq!(error.code(), expected_code);
        }
    }

    #[test]
    fn platform_operation_failed_keeps_its_user_readable_message_instead_of_a_generic_string() {
        let request_id = Uuid::new_v4();
        let dto = ApplicationError::PlatformOperationFailed(
            "no default application is set for this file type".to_owned(),
        )
        .into_dto(request_id);

        assert_eq!(dto.code, ApplicationErrorCode::PlatformOperationFailed);
        assert_eq!(
            dto.message,
            "no default application is set for this file type"
        );
    }

    #[test]
    fn into_dto_carries_the_given_request_id_and_never_leaks_raw_errors() {
        let request_id = Uuid::new_v4();
        let dto = ApplicationError::Internal.into_dto(request_id);

        assert_eq!(dto.code, ApplicationErrorCode::Internal);
        assert_eq!(dto.request_id, request_id);
        assert_eq!(dto.message, "internal error");
        assert!(dto.details.is_none());
    }

    #[test]
    fn workspace_revision_conflict_into_dto_matches_the_spec_5_3_10_shape() {
        let workspace_id = uuid::Uuid::new_v4();
        let request_id = Uuid::new_v4();
        let dto = ApplicationError::WorkspaceRevisionConflict {
            workspace_id,
            expected_revision: 14,
            actual_revision: 16,
        }
        .into_dto(request_id);

        assert_eq!(dto.code, ApplicationErrorCode::WorkspaceRevisionConflict);
        assert_eq!(
            dto.message,
            "The workspace changed after this view was loaded."
        );
        let details = dto.details.expect("details must be present");
        assert_eq!(details["workspaceId"], workspace_id.to_string());
        assert_eq!(details["expectedRevision"], 14);
        assert_eq!(details["actualRevision"], 16);
    }

    #[test]
    fn action_not_found_into_dto_carries_the_action_id_in_details() {
        let dto = ApplicationError::ActionNotFound(ActionId::new("core.missing"))
            .into_dto(Uuid::new_v4());

        assert_eq!(dto.code, ApplicationErrorCode::ActionNotFound);
        let details = dto.details.expect("details must be present");
        assert_eq!(details["actionId"], "core.missing");
    }

    #[test]
    fn host_key_unverified_into_dto_carries_the_fingerprint_in_details() {
        let dto = ApplicationError::HostKeyUnverified {
            fingerprint: "SHA256:abc".to_owned(),
        }
        .into_dto(Uuid::new_v4());

        assert_eq!(dto.code, ApplicationErrorCode::HostKeyUnverified);
        let details = dto.details.expect("details must be present");
        assert_eq!(details["fingerprint"], "SHA256:abc");
    }

    #[test]
    fn host_key_mismatch_into_dto_carries_both_fingerprints_in_details() {
        let dto = ApplicationError::HostKeyMismatch {
            fingerprint: "SHA256:new".to_owned(),
            expected_fingerprint: "SHA256:old".to_owned(),
        }
        .into_dto(Uuid::new_v4());

        assert_eq!(dto.code, ApplicationErrorCode::HostKeyMismatch);
        let details = dto.details.expect("details must be present");
        assert_eq!(details["fingerprint"], "SHA256:new");
        assert_eq!(details["expectedFingerprint"], "SHA256:old");
    }

    #[test]
    fn from_connection_error_maps_host_key_states_distinctly() {
        assert_eq!(
            ApplicationError::from(ConnectionError::HostKeyUnverified {
                fingerprint: "SHA256:abc".to_owned()
            }),
            ApplicationError::HostKeyUnverified {
                fingerprint: "SHA256:abc".to_owned()
            }
        );
        assert_eq!(
            ApplicationError::from(ConnectionError::HostKeyMismatch {
                fingerprint: "SHA256:new".to_owned(),
                expected_fingerprint: "SHA256:old".to_owned(),
            }),
            ApplicationError::HostKeyMismatch {
                fingerprint: "SHA256:new".to_owned(),
                expected_fingerprint: "SHA256:old".to_owned(),
            }
        );
    }

    #[test]
    fn from_workspace_error_maps_revision_conflict_with_its_details() {
        let workspace_id = fm_domain::WorkspaceId::new();
        let error = WorkspaceError::RevisionConflict {
            id: workspace_id,
            expected: Some(14),
            actual: 16,
        };

        let mapped: ApplicationError = error.into();
        assert_eq!(
            mapped,
            ApplicationError::WorkspaceRevisionConflict {
                workspace_id: workspace_id.into(),
                expected_revision: 14,
                actual_revision: 16,
            }
        );
    }
}
