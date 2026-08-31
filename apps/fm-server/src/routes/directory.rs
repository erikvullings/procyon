//! Thin directory and metadata REST handlers (task 0019).

use axum::Json;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use fm_transport_dto::{
    ApplicationErrorDto, DirectorySnapshotDto, EntryMetadataDto, EntryMetadataRequest,
    EntrySummaryDto, ListDirectoryChildrenRequest, ListDirectoryRequest, NavigateRequest,
    SetPaneActivityRequest,
};
use std::time::Instant;
use tower_http::request_id::RequestId;
use tracing::{info, warn};

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists one directory page.
#[utoipa::path(
    post,
    path = "/api/v1/directories/list",
    operation_id = "listDirectory",
    request_body = ListDirectoryRequest,
    responses(
        (status = 200, description = "A directory snapshot", body = DirectorySnapshotDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn list_directory(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ListDirectoryRequest>,
) -> Result<Json<DirectorySnapshotDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    let started = Instant::now();
    tracing::Span::current()
        .record("workspace_id", request.workspace_id.to_string().as_str())
        .record("provider_id", request.location.provider_id.as_str());
    info!(
        request_id = %request_id,
        workspace_id = %request.workspace_id,
        pane_id = %request.pane_id,
        provider_id = %request.location.provider_id,
        uri = %request.location.uri,
        "list_directory received"
    );
    match state.service.list_directory(request).await {
        Ok(snapshot) => {
            info!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                "list_directory honored"
            );
            Ok(Json(snapshot))
        }
        Err(error) => {
            warn!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "list_directory failed"
            );
            Err(ApiError::new(error, request_id))
        }
    }
}

/// Refreshes one directory page.
#[utoipa::path(
    post,
    path = "/api/v1/directories/refresh",
    operation_id = "refreshDirectory",
    request_body = ListDirectoryRequest,
    responses(
        (status = 200, description = "A refreshed directory snapshot", body = DirectorySnapshotDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn refresh_directory(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ListDirectoryRequest>,
) -> Result<Json<DirectorySnapshotDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    let started = Instant::now();
    info!(
        request_id = %request_id,
        workspace_id = %request.workspace_id,
        pane_id = %request.pane_id,
        provider_id = %request.location.provider_id,
        uri = %request.location.uri,
        "refresh_directory received"
    );
    match state.service.refresh_directory(request).await {
        Ok(snapshot) => {
            info!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                "refresh_directory honored"
            );
            Ok(Json(snapshot))
        }
        Err(error) => {
            warn!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "refresh_directory failed"
            );
            Err(ApiError::new(error, request_id))
        }
    }
}

/// Lists the immediate child directories of a location, for the directory-tree sidebar (task
/// 0139). Not bound to a pane or workspace, unlike `list_directory`.
#[utoipa::path(
    post,
    path = "/api/v1/directories/children",
    operation_id = "listDirectoryChildren",
    request_body = ListDirectoryChildrenRequest,
    responses(
        (status = 200, description = "The location's immediate child directories", body = Vec<EntrySummaryDto>),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn list_directory_children(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ListDirectoryChildrenRequest>,
) -> Result<Json<Vec<EntrySummaryDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    let started = Instant::now();
    info!(
        request_id = %request_id,
        provider_id = %request.location.provider_id,
        uri = %request.location.uri,
        "list_directory_children received"
    );
    match state.service.list_directory_children(request).await {
        Ok(children) => {
            info!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                "list_directory_children honored"
            );
            Ok(Json(children))
        }
        Err(error) => {
            warn!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "list_directory_children failed"
            );
            Err(ApiError::new(error, request_id))
        }
    }
}

/// Navigates a pane and returns the destination's first page.
#[utoipa::path(
    post,
    path = "/api/v1/navigation/open",
    operation_id = "navigatePane",
    request_body = NavigateRequest,
    responses(
        (status = 200, description = "The destination directory snapshot", body = DirectorySnapshotDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn navigate_pane(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<NavigateRequest>,
) -> Result<Json<DirectorySnapshotDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    let started = Instant::now();
    info!(
        request_id = %request_id,
        workspace_id = %request.workspace_id,
        pane_id = %request.pane_id,
        provider_id = %request.location.provider_id,
        uri = %request.location.uri,
        "navigate_pane received"
    );
    match state.service.navigate_pane(request).await {
        Ok(snapshot) => {
            info!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                "navigate_pane honored"
            );
            Ok(Json(snapshot))
        }
        Err(error) => {
            warn!(
                request_id = %request_id,
                elapsed_ms = started.elapsed().as_millis(),
                error = ?error,
                "navigate_pane failed"
            );
            Err(ApiError::new(error, request_id))
        }
    }
}

/// Fetches detailed metadata for one entry.
#[utoipa::path(
    post,
    path = "/api/v1/entries/metadata",
    operation_id = "getEntryMetadata",
    request_body = EntryMetadataRequest,
    responses(
        (status = 200, description = "Detailed entry metadata", body = EntryMetadataDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The entry is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The entry does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_entry_metadata(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<EntryMetadataRequest>,
) -> Result<Json<EntryMetadataDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .get_entry_metadata(request)
        .await
        .map(EntryMetadataDto::from)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Marks whether a pane is currently in the foreground, so a poll-tracked
/// directory watch (SFTP, FTP, ...) can poll less often while backgrounded
/// (task 0109).
#[utoipa::path(
    post,
    path = "/api/v1/directories/activity",
    operation_id = "setPaneActivity",
    request_body = SetPaneActivityRequest,
    responses(
        (status = 204, description = "The pane's activity state was recorded"),
        (status = 404, description = "No pane exists with this id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn set_pane_activity(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SetPaneActivityRequest>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .set_pane_activity(request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}
