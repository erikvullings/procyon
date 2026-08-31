//! OneDrive authorization REST surface (task 0110): thin handlers over
//! `FileManagerService::begin_onedrive_authorization`/
//! `onedrive_authorization_attempt`/`cancel_onedrive_authorization`, with no
//! validation/mutation logic of their own (spec §3 rule 2). The route
//! returns the Microsoft authorization URL as plain data; the frontend
//! adapter is responsible for opening it in the system browser - this
//! backend never opens one itself.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ApplicationErrorDto, BeginOneDriveAuthorizationResponseDto, OneDriveAuthorizationAttemptDto,
};
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Begins a OneDrive OAuth authorization attempt for a saved connection.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/onedrive/authorize",
    operation_id = "beginOneDriveAuthorization",
    params(("connectionId" = Uuid, Path, description = "The OneDrive connection to authorize")),
    responses(
        (status = 201, description = "The authorization attempt was started", body = BeginOneDriveAuthorizationResponseDto),
        (status = 400, description = "The connection is not a OneDrive connection, or an attempt is already in progress for it", body = ApplicationErrorDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn begin_onedrive_authorization(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<(StatusCode, Json<BeginOneDriveAuthorizationResponseDto>), ApiError> {
    let request_id = extract_request_id(&request_id);
    let response = state
        .service
        .begin_onedrive_authorization(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Polls a OneDrive authorization attempt's current status.
#[utoipa::path(
    get,
    path = "/api/v1/onedrive/authorizations/{attemptId}",
    operation_id = "getOneDriveAuthorizationAttempt",
    params(("attemptId" = Uuid, Path, description = "The authorization attempt to poll")),
    responses(
        (status = 200, description = "The attempt's current status", body = OneDriveAuthorizationAttemptDto),
        (status = 404, description = "No attempt exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_onedrive_authorization_attempt(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(attempt_id): Path<Uuid>,
) -> Result<Json<OneDriveAuthorizationAttemptDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let attempt = state
        .service
        .onedrive_authorization_attempt(attempt_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(attempt))
}

/// Cancels a pending OneDrive authorization attempt. Idempotent for an
/// attempt that has already reached a terminal state.
#[utoipa::path(
    post,
    path = "/api/v1/onedrive/authorizations/{attemptId}/cancel",
    operation_id = "cancelOneDriveAuthorization",
    params(("attemptId" = Uuid, Path, description = "The authorization attempt to cancel")),
    responses(
        (status = 200, description = "The attempt's status after cancellation", body = OneDriveAuthorizationAttemptDto),
        (status = 404, description = "No attempt exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn cancel_onedrive_authorization(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(attempt_id): Path<Uuid>,
) -> Result<Json<OneDriveAuthorizationAttemptDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let attempt = state
        .service
        .cancel_onedrive_authorization(attempt_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(attempt))
}
