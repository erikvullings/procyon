//! Thin REST adapter for backend-owned operations (specification §8).

use crate::{
    error::{ApiError, extract_request_id},
    state::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use fm_domain::OperationId;
use fm_transport_dto::{
    ApplicationErrorDto, OperationDto, OperationPageDto, ResolveOperationConflictRequestDto,
    StartOperationRequestDto,
};
use serde::Deserialize;
use std::time::Instant;
use tower_http::request_id::RequestId;
use tracing::{info, warn};
use uuid::Uuid;

#[utoipa::path(
    get,
    path = "/api/v1/operations",
    operation_id = "listOperations",
    params(OperationPageQuery),
    responses((status = 200, body = OperationPageDto))
)]
pub(crate) async fn list_operations(
    State(state): State<AppState>,
    Query(query): Query<OperationPageQuery>,
) -> Json<OperationPageDto> {
    Json(
        state
            .service
            .list_operation_page(query.offset.unwrap_or(0), query.limit.unwrap_or(50)),
    )
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationPageQuery {
    /// Zero-based entry offset.
    offset: Option<u64>,
    /// Page size, clamped to 1 through 100.
    limit: Option<u16>,
}

#[utoipa::path(
    post,
    path = "/api/v1/operations",
    operation_id = "startOperation",
    request_body = StartOperationRequestDto,
    responses(
        (status = 201, body = OperationDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn start_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    session_id: crate::audit::SessionIdHeader,
    Json(request): Json<StartOperationRequestDto>,
) -> Result<(StatusCode, Json<OperationDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    for source in &request.sources {
        crate::error::require_within_roots(source, &state.accessible_roots, correlation_id)?;
    }
    if let Some(destination) = &request.destination {
        crate::error::require_within_roots(destination, &state.accessible_roots, correlation_id)?;
    }
    for destination in &request.destinations {
        crate::error::require_within_roots(destination, &state.accessible_roots, correlation_id)?;
    }
    let started = Instant::now();
    let operation_kind = request.operation_type;
    let audit_operation = match operation_kind {
        fm_transport_dto::OperationKindDto::Delete => Some(crate::audit::AuditOperation::Delete),
        fm_transport_dto::OperationKindDto::Trash => Some(crate::audit::AuditOperation::Trash),
        _ if matches!(
            request.conflict_policy,
            fm_transport_dto::OperationConflictPolicyDto::Overwrite
        ) =>
        {
            Some(crate::audit::AuditOperation::Overwrite)
        }
        _ => None,
    };
    let audit_paths: Vec<String> = if audit_operation.is_some() {
        let mut paths: Vec<String> = request.sources.iter().map(|l| l.uri.clone()).collect();
        paths.extend(request.destination.iter().map(|l| l.uri.clone()));
        paths.extend(request.destinations.iter().map(|l| l.uri.clone()));
        paths
    } else {
        Vec::new()
    };
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    info!(
        request_id = %correlation_id,
        operation_kind = ?operation_kind,
        has_idempotency_key = key.is_some(),
        "start_operation received"
    );
    match state.service.start_operation(request, key) {
        Ok(operation) => {
            tracing::Span::current().record("operation_id", operation.id.to_string().as_str());
            info!(
                request_id = %correlation_id,
                operation_id = %operation.id,
                operation_kind = ?operation_kind,
                elapsed_ms = started.elapsed().as_millis(),
                "start_operation honored"
            );
            if let Some(audit_operation) = audit_operation {
                for path in audit_paths {
                    crate::audit::AuditEvent::new(audit_operation, path, session_id.0.clone())
                        .log();
                }
            }
            Ok((StatusCode::CREATED, Json(operation)))
        }
        Err(error) => {
            warn!(
                request_id = %correlation_id,
                operation_kind = ?operation_kind,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "start_operation failed"
            );
            Err(ApiError::new(error, correlation_id))
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/operations/{operationId}",
    operation_id = "getOperation",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 200, body = OperationDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<Json<OperationDto>, ApiError> {
    state
        .service
        .get_operation(OperationId::from(id))
        .map(Json)
        .map_err(|e| ApiError::new(e, extract_request_id(&request_id)))
}

#[utoipa::path(
    post,
    path = "/api/v1/operations/{operationId}/cancel",
    operation_id = "cancelOperation",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn cancel_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let started = Instant::now();
    let operation_id = OperationId::from(id);
    info!(request_id = %correlation_id, operation_id = %operation_id, "cancel_operation received");
    match state.service.cancel_operation(operation_id) {
        Ok(()) => {
            info!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                "cancel_operation honored"
            );
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => {
            warn!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "cancel_operation failed"
            );
            Err(ApiError::new(error, correlation_id))
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/operations/{operationId}/pause",
    operation_id = "pauseOperation",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn pause_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let started = Instant::now();
    let operation_id = OperationId::from(id);
    info!(request_id = %correlation_id, operation_id = %operation_id, "pause_operation received");
    match state.service.pause_operation(operation_id) {
        Ok(()) => {
            info!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                "pause_operation honored"
            );
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => {
            warn!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "pause_operation failed"
            );
            Err(ApiError::new(error, correlation_id))
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/operations/{operationId}/resume",
    operation_id = "resumeOperation",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn resume_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let started = Instant::now();
    let operation_id = OperationId::from(id);
    info!(request_id = %correlation_id, operation_id = %operation_id, "resume_operation received");
    match state.service.resume_operation(operation_id) {
        Ok(()) => {
            info!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                "resume_operation honored"
            );
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => {
            warn!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "resume_operation failed"
            );
            Err(ApiError::new(error, correlation_id))
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/operations/{operationId}/resolve-conflict",
    operation_id = "resolveOperationConflict",
    params(("operationId" = Uuid, Path)),
    request_body = ResolveOperationConflictRequestDto,
    responses(
        (status = 204),
        (status = 404, body = ApplicationErrorDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn resolve_operation_conflict(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
    session_id: crate::audit::SessionIdHeader,
    Json(request): Json<ResolveOperationConflictRequestDto>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let started = Instant::now();
    let operation_id = OperationId::from(id);
    let resolution = request.resolution;
    let audit_path = (resolution == fm_transport_dto::ConflictResolutionDto::Overwrite)
        .then(|| state.service.get_operation(operation_id).ok())
        .flatten()
        .and_then(|op| op.progress.current_entry)
        .map(|entry| entry.location.uri);
    info!(
        request_id = %correlation_id,
        operation_id = %operation_id,
        resolution = ?request.resolution,
        apply_to_all = request.apply_to_all_similar,
        "resolve_operation_conflict received"
    );
    match state
        .service
        .resolve_operation_conflict(operation_id, request)
    {
        Ok(()) => {
            info!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                "resolve_operation_conflict honored"
            );
            if let Some(path) = audit_path {
                crate::audit::AuditEvent::new(
                    crate::audit::AuditOperation::Overwrite,
                    path,
                    session_id.0,
                )
                .log();
            }
            Ok(StatusCode::NO_CONTENT)
        }
        Err(error) => {
            warn!(
                request_id = %correlation_id,
                operation_id = %operation_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "resolve_operation_conflict failed"
            );
            Err(ApiError::new(error, correlation_id))
        }
    }
}
