//! Structured error responses shared by every handler (spec §8: every error
//! carries a request id).

use axum::Json;
use axum::extract::Extension;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use fm_application::ApplicationError;
use fm_transport_dto::LocationDto;
use std::path::PathBuf;
use tower_http::request_id::RequestId;
use uuid::Uuid;

/// Renders unmatched routes as a structured `ApplicationErrorDto`, tagged with
/// the request id set by the `tower_http` request-id middleware.
pub(crate) async fn not_found(Extension(request_id): Extension<RequestId>) -> impl IntoResponse {
    let id = extract_request_id(&request_id);

    (
        StatusCode::NOT_FOUND,
        Json(ApplicationError::NotFound.into_dto(id)),
    )
}

/// Extracts the correlation id set by the `tower_http` request-id
/// middleware, falling back to a fresh one if it is somehow missing or
/// unparseable.
pub(crate) fn extract_request_id(request_id: &RequestId) -> Uuid {
    request_id
        .header_value()
        .to_str()
        .ok()
        .and_then(|value| value.parse::<Uuid>().ok())
        .unwrap_or_else(Uuid::new_v4)
}

/// Wraps an [`ApplicationError`] with the request id that produced it, so
/// handlers can simply `?`-propagate service errors and have them rendered
/// as the correct HTTP status and body (spec §8).
pub(crate) struct ApiError {
    error: ApplicationError,
    request_id: Uuid,
}

impl ApiError {
    pub(crate) fn new(error: ApplicationError, request_id: Uuid) -> Self {
        Self { error, request_id }
    }
}

/// Maps an [`ApplicationError`] to the HTTP status code it should be
/// rendered with.
fn status_for(error: &ApplicationError) -> StatusCode {
    match error {
        ApplicationError::NotFound => StatusCode::NOT_FOUND,
        ApplicationError::PermissionDenied => StatusCode::FORBIDDEN,
        // The request is valid and may succeed once the holder releases the file.
        ApplicationError::FileLocked => StatusCode::CONFLICT,
        ApplicationError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        ApplicationError::DestinationAlreadyExists => StatusCode::CONFLICT,
        ApplicationError::ProviderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ApplicationError::OperationCancelled => StatusCode::CONFLICT,
        ApplicationError::CredentialRequired | ApplicationError::InvalidCredential => {
            StatusCode::BAD_REQUEST
        }
        ApplicationError::WorkspaceRevisionConflict { .. } => StatusCode::CONFLICT,
        ApplicationError::FileRevisionConflict { .. } => StatusCode::CONFLICT,
        ApplicationError::ActionNotFound(_) => StatusCode::NOT_FOUND,
        ApplicationError::ActionUnavailable(_) => StatusCode::CONFLICT,
        ApplicationError::PlatformOperationFailed(_) => StatusCode::BAD_GATEWAY,
        ApplicationError::HostKeyUnverified { .. } | ApplicationError::HostKeyMismatch { .. } => {
            StatusCode::CONFLICT
        }
        ApplicationError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Rejects a request whose [`LocationDto`] resolves outside the configured
/// accessible roots (spec §22, task 0064). A no-op when `roots` is empty
/// (unrestricted) or the location belongs to a non-local provider.
pub(crate) fn require_within_roots(
    location: &LocationDto,
    roots: &[PathBuf],
    request_id: Uuid,
) -> Result<(), ApiError> {
    crate::accessible_roots::validate_location(location, roots)
        .map_err(|_| ApiError::new(ApplicationError::PermissionDenied, request_id))
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = status_for(&self.error);
        (status, Json(self.error.into_dto(self.request_id))).into_response()
    }
}
