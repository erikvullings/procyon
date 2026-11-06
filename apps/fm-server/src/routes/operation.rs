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
    ApplicationErrorDto, EntrySummaryDto, InterruptedTransferDto, InterruptedTransfersDto,
    LinkOptionsDto, LinkOptionsRequestDto, OperationDto, OperationPageDto,
    ResolveLinkTargetRequestDto, ResolveOperationConflictRequestDto, StartOperationRequestDto,
    TransferConflictsDto, TransferConflictsRequestDto,
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

/// Lists transfers the previous process left queued or running, with what is safe to do about
/// each (task 0300). In browser/server mode only transfers wholly inside the accessible roots
/// are listed.
#[utoipa::path(
    get,
    path = "/api/v1/operations/interrupted",
    operation_id = "listInterruptedTransfers",
    responses((status = 200, body = InterruptedTransfersDto))
)]
pub(crate) async fn list_interrupted_transfers(
    State(state): State<AppState>,
) -> Json<InterruptedTransfersDto> {
    let mut listed = state.service.list_interrupted_transfers().await;
    listed
        .transfers
        .retain(|transfer| interrupted_within_roots(transfer, &state.accessible_roots));
    Json(listed)
}

fn interrupted_within_roots(
    transfer: &InterruptedTransferDto,
    roots: &[std::path::PathBuf],
) -> bool {
    transfer
        .sources
        .iter()
        .chain(transfer.destination.iter())
        .all(|location| crate::accessible_roots::validate_location(location, roots).is_ok())
}

/// Rejects an action on an interrupted transfer that reaches outside the accessible roots.
/// Authorises from the stored intent, so it does not probe any provider.
fn require_interrupted_within_roots(
    state: &AppState,
    id: Uuid,
    correlation_id: Uuid,
) -> Result<(), ApiError> {
    let Some((sources, destination)) = state
        .service
        .interrupted_transfer_locations(&id.to_string())
    else {
        // Unknown ids fall through to the service's own 404.
        return Ok(());
    };
    for location in sources.iter().chain(destination.iter()) {
        crate::error::require_within_roots(location, &state.accessible_roots, correlation_id)?;
    }
    Ok(())
}

/// Runs an interrupted transfer's unfinished work again as a new operation (task 0300).
#[utoipa::path(
    post,
    path = "/api/v1/operations/interrupted/{operationId}/restart",
    operation_id = "restartInterruptedTransfer",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 201, body = OperationDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn restart_interrupted_transfer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<OperationDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    require_interrupted_within_roots(&state, id, correlation_id)?;
    state
        .service
        .restart_interrupted_transfer(&id.to_string())
        .await
        .map(|operation| (StatusCode::CREATED, Json(operation)))
        .map_err(|error| ApiError::new(error, correlation_id))
}

/// Acknowledges an interrupted transfer: removes its private partials and its record (task 0300).
#[utoipa::path(
    post,
    path = "/api/v1/operations/interrupted/{operationId}/discard",
    operation_id = "discardInterruptedTransfer",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 204),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn discard_interrupted_transfer(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    require_interrupted_within_roots(&state, id, correlation_id)?;
    state
        .service
        .discard_interrupted_transfer(&id.to_string())
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, correlation_id))
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

/// Resolves a symbolic link to the entry it points at, for read-only viewers.
#[utoipa::path(
    post,
    path = "/api/v1/link-target",
    operation_id = "resolveLinkTarget",
    request_body = ResolveLinkTargetRequestDto,
    responses(
        (status = 200, body = EntrySummaryDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn resolve_link_target(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ResolveLinkTargetRequestDto>,
) -> Result<Json<EntrySummaryDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, correlation_id)?;
    let resolved = state
        .service
        .resolve_link_target(request)
        .await
        .map_err(|error| ApiError::new(error, correlation_id))?;
    // A link must not expose an entry outside the server's accessible roots.
    crate::error::require_within_roots(
        &resolved.location,
        &state.accessible_roots,
        correlation_id,
    )?;
    Ok(Json(resolved))
}

/// Lists the sources of a planned copy/move whose names already exist in the destination, so
/// the confirmation can offer a conflict policy before the job starts (task 0062).
#[utoipa::path(
    post,
    path = "/api/v1/operations/transfer-conflicts",
    operation_id = "findTransferConflicts",
    request_body = TransferConflictsRequestDto,
    responses(
        (status = 200, body = TransferConflictsDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn find_transfer_conflicts(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<TransferConflictsRequestDto>,
) -> Result<Json<TransferConflictsDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    crate::error::require_within_roots(
        &request.destination,
        &state.accessible_roots,
        correlation_id,
    )?;
    for source in &request.sources {
        crate::error::require_within_roots(source, &state.accessible_roots, correlation_id)?;
    }
    state
        .service
        .transfer_conflicts(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, correlation_id))
}

/// Lists the link kinds that can point at a target from a destination directory (task 0168).
#[utoipa::path(
    post,
    path = "/api/v1/link-options",
    operation_id = "getLinkOptions",
    request_body = LinkOptionsRequestDto,
    responses(
        (status = 200, body = LinkOptionsDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn get_link_options(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<LinkOptionsRequestDto>,
) -> Result<Json<LinkOptionsDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.target, &state.accessible_roots, correlation_id)?;
    crate::error::require_within_roots(
        &request.destination,
        &state.accessible_roots,
        correlation_id,
    )?;
    state
        .service
        .link_options(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, correlation_id))
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
    path = "/api/v1/operations/{operationId}/undo",
    operation_id = "undoOperation",
    params(("operationId" = Uuid, Path)),
    responses(
        (status = 201, body = OperationDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 400, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn undo_operation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<OperationDto>), ApiError> {
    let correlation_id = extract_request_id(&request_id);
    state
        .service
        .undo_operation(OperationId::from(id))
        .map(|operation| (StatusCode::CREATED, Json(operation)))
        .map_err(|error| ApiError::new(error, correlation_id))
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
