//! Connection REST surface (spec §16, task 0103): thin handlers that
//! translate a request into a `FileManagerService` call and back into a DTO,
//! with no validation/mutation logic of their own (spec §3 rule 2).

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    AcceptSshHostKeyRequestDto, ApplicationErrorDto, ConnectionDto, CreateConnectionRequestDto,
    HostKeyProbeDto, UpdateConnectionRequestDto,
};
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists every stored connection profile with its current runtime status.
#[utoipa::path(
    get,
    path = "/api/v1/connections",
    operation_id = "listConnections",
    responses((status = 200, description = "Every stored connection profile", body = Vec<ConnectionDto>))
)]
pub(crate) async fn list_connections(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<ConnectionDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connections = state
        .service
        .list_connections()
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connections))
}

/// Creates and persists a new connection profile.
#[utoipa::path(
    post,
    path = "/api/v1/connections",
    operation_id = "createConnection",
    request_body = CreateConnectionRequestDto,
    responses(
        (status = 201, description = "The created connection", body = ConnectionDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn create_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CreateConnectionRequestDto>,
) -> Result<(StatusCode, Json<ConnectionDto>), ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .create_connection(request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok((StatusCode::CREATED, Json(connection)))
}

/// Loads a single connection profile by id.
#[utoipa::path(
    get,
    path = "/api/v1/connections/{connectionId}",
    operation_id = "getConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to load")),
    responses(
        (status = 200, description = "The connection", body = ConnectionDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<ConnectionDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .get_connection(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connection))
}

/// Updates an existing connection profile.
#[utoipa::path(
    put,
    path = "/api/v1/connections/{connectionId}",
    operation_id = "updateConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to update")),
    request_body = UpdateConnectionRequestDto,
    responses(
        (status = 200, description = "The updated connection", body = ConnectionDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn update_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
    Json(request): Json<UpdateConnectionRequestDto>,
) -> Result<Json<ConnectionDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .update_connection(connection_id, request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connection))
}

/// Deletes a connection profile and its stored credential, if any.
#[utoipa::path(
    delete,
    path = "/api/v1/connections/{connectionId}",
    operation_id = "deleteConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to delete")),
    responses(
        (status = 204, description = "The connection was deleted"),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn delete_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .delete_connection(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Attempts to connect. See `fm_connections::ConnectionService`'s
/// documentation for the honest scope of this operation before task
/// 0104/0106 register a real protocol dialer.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/connect",
    operation_id = "connectConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to connect")),
    responses(
        (status = 200, description = "The connection's new status", body = ConnectionDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn connect_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<ConnectionDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .connect_connection(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connection))
}

/// Marks a connection as disconnected.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/disconnect",
    operation_id = "disconnectConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to disconnect")),
    responses(
        (status = 200, description = "The connection's new status", body = ConnectionDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn disconnect_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<ConnectionDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .disconnect_connection(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connection))
}

/// Checks whether a connection's configuration and credential are currently
/// usable, without changing its tracked status. See
/// `fm_connections::ConnectionService`'s documentation for the honest scope
/// of this operation before task 0104/0106 register a real protocol dialer.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/test",
    operation_id = "testConnection",
    params(("connectionId" = Uuid, Path, description = "The connection to test")),
    responses(
        (status = 200, description = "The connection's current status", body = ConnectionDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn test_connection(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<ConnectionDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let connection = state
        .service
        .test_connection(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(connection))
}

/// Probes an SSH connection's currently presented host key without
/// authenticating (task 0104, spec §6.4's mandatory explicit confirmation
/// flow) - lets a caller decide whether to accept a never-seen or changed
/// key before a `connect` attempt reports
/// [`fm_transport_dto::ApplicationErrorCode::HostKeyUnverified`]/
/// [`fm_transport_dto::ApplicationErrorCode::HostKeyMismatch`] via its
/// `status` field.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/hostKey/probe",
    operation_id = "probeSshHostKey",
    params(("connectionId" = Uuid, Path, description = "The SSH connection to probe")),
    responses(
        (status = 200, description = "The host key currently presented, and whether it is trusted", body = HostKeyProbeDto),
        (status = 400, description = "The connection is not an SSH connection", body = ApplicationErrorDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn probe_ssh_host_key(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
) -> Result<Json<HostKeyProbeDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let probe = state
        .service
        .probe_ssh_host_key(connection_id)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(Json(probe))
}

/// Accepts (persists) a host-key fingerprint for an SSH connection (task
/// 0104, spec §6.4) - the only endpoint that ever writes to the known-hosts
/// store, and only after the server re-probes to confirm the host is still
/// presenting exactly the fingerprint being accepted.
#[utoipa::path(
    post,
    path = "/api/v1/connections/{connectionId}/hostKey/accept",
    operation_id = "acceptSshHostKey",
    params(("connectionId" = Uuid, Path, description = "The SSH connection whose host key is being accepted")),
    request_body = AcceptSshHostKeyRequestDto,
    responses(
        (status = 204, description = "The host key was accepted and persisted"),
        (status = 400, description = "The fingerprint no longer matches what the host presents, the connection requires a pre-established known host, or it is not an SSH connection", body = ApplicationErrorDto),
        (status = 404, description = "No connection exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn accept_ssh_host_key(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(connection_id): Path<Uuid>,
    Json(request): Json<AcceptSshHostKeyRequestDto>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .accept_ssh_host_key(connection_id, request.fingerprint)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}
