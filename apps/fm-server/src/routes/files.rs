//! Thin byte-range read and content-search REST handlers for the in-app
//! large file viewer (task 0088).

use axum::extract::{Extension, Path, State};
use axum::{Json, http::StatusCode};
use fm_transport_dto::{
    ApplicationErrorDto, ArchiveCredentialRequestDto, CalculateFolderSizeRequestDto,
    CalculateFolderSizeResponseDto, GetFileGitHistoryRequestDto, GetFileGitHistoryResponseDto,
    LoadEditableFileRequestDto, LoadEditableFileResponseDto, OpenStructuredViewRequestDto,
    OpenStructuredViewResponseDto, ReadFileRangeRequestDto, ReadFileRangeResponseDto,
    ReadStructuredJsonWindowRequestDto, ReadStructuredJsonWindowResponseDto,
    ReadStructuredRowsRequestDto, ReadStructuredRowsResponseDto, SaveEditableFileRequestDto,
    SaveEditableFileResponseDto, ScanDiskUsageRequestDto, SearchInFileRequestDto,
    SearchInFileResponseDto, SearchStructuredRowsRequestDto, SearchStructuredRowsResponseDto,
    StructuredViewSessionRequestDto, StructuredViewStatusDto, UpdateStructuredViewRequestDto,
};
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Caches an archive password for this backend session only.
#[utoipa::path(
    post,
    path = "/api/v1/archives/credential",
    operation_id = "cacheArchivePassword",
    request_body = ArchiveCredentialRequestDto,
    responses(
        (status = 204, description = "Credential cached for this backend session"),
        (status = 400, description = "The archive location was invalid", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn cache_archive_password(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ArchiveCredentialRequestDto>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .cache_archive_password(request)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Reads one bounded byte range from a single file.
#[utoipa::path(
    post,
    path = "/api/v1/files/range",
    operation_id = "readFileRange",
    request_body = ReadFileRangeRequestDto,
    responses(
        (status = 200, description = "The requested byte range", body = ReadFileRangeResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The file is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The file does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn read_file_range(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ReadFileRangeRequestDto>,
) -> Result<Json<ReadFileRangeResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .read_file_range(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Opens a provider-neutral structured-data viewer session.
#[utoipa::path(post, path = "/api/v1/files/structured/open", operation_id = "openStructuredView",
    request_body = OpenStructuredViewRequestDto,
    responses((status = 200, body = OpenStructuredViewResponseDto), (status = 400, body = ApplicationErrorDto), (status = 404, body = ApplicationErrorDto)))]
pub(crate) async fn open_structured_view(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<OpenStructuredViewRequestDto>,
) -> Result<Json<OpenStructuredViewResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .open_structured_view(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/status", operation_id = "getStructuredViewStatus",
    request_body = StructuredViewSessionRequestDto,
    responses((status = 200, body = StructuredViewStatusDto), (status = 404, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn structured_view_status(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StructuredViewSessionRequestDto>,
) -> Result<Json<StructuredViewStatusDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .structured_view_status(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/update", operation_id = "updateStructuredView",
    request_body = UpdateStructuredViewRequestDto,
    responses((status = 200, body = OpenStructuredViewResponseDto), (status = 400, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn update_structured_view(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<UpdateStructuredViewRequestDto>,
) -> Result<Json<OpenStructuredViewResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .update_structured_view(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/rows", operation_id = "readStructuredRows",
    request_body = ReadStructuredRowsRequestDto,
    responses((status = 200, body = ReadStructuredRowsResponseDto), (status = 400, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn read_structured_rows(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ReadStructuredRowsRequestDto>,
) -> Result<Json<ReadStructuredRowsResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .read_structured_rows(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/json-window", operation_id = "readStructuredJsonWindow",
    request_body = ReadStructuredJsonWindowRequestDto,
    responses((status = 200, body = ReadStructuredJsonWindowResponseDto), (status = 400, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn read_structured_json_window(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ReadStructuredJsonWindowRequestDto>,
) -> Result<Json<ReadStructuredJsonWindowResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .read_structured_json_window(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/search", operation_id = "searchStructuredRows",
    request_body = SearchStructuredRowsRequestDto,
    responses((status = 200, body = SearchStructuredRowsResponseDto), (status = 400, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn search_structured_rows(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SearchStructuredRowsRequestDto>,
) -> Result<Json<SearchStructuredRowsResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .search_structured_rows(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/structured/close", operation_id = "closeStructuredView",
    request_body = StructuredViewSessionRequestDto,
    responses((status = 204), (status = 404, body = ApplicationErrorDto)))]
pub(crate) async fn close_structured_view(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<StructuredViewSessionRequestDto>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .close_structured_view(request)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/editable/load", operation_id = "loadEditableFile",
    request_body = LoadEditableFileRequestDto,
    responses((status = 200, body = LoadEditableFileResponseDto), (status = 400, body = ApplicationErrorDto), (status = 404, body = ApplicationErrorDto)))]
pub(crate) async fn load_editable_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<LoadEditableFileRequestDto>,
) -> Result<Json<LoadEditableFileResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .load_editable_file(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

#[utoipa::path(post, path = "/api/v1/files/editable/save", operation_id = "saveEditableFile",
    request_body = SaveEditableFileRequestDto,
    responses((status = 200, body = SaveEditableFileResponseDto), (status = 400, body = ApplicationErrorDto), (status = 409, body = ApplicationErrorDto)))]
pub(crate) async fn save_editable_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    session_id: crate::audit::SessionIdHeader,
    Json(request): Json<SaveEditableFileRequestDto>,
) -> Result<Json<SaveEditableFileResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    if let Some(destination) = &request.destination {
        crate::error::require_within_roots(destination, &state.accessible_roots, request_id)?;
    }
    let audit_target = request
        .destination
        .as_ref()
        .unwrap_or(&request.location)
        .uri
        .clone();
    let result = state
        .service
        .save_editable_file(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id));
    if result.is_ok() {
        crate::audit::AuditEvent::new(
            crate::audit::AuditOperation::Overwrite,
            audit_target,
            session_id.0,
        )
        .log();
    }
    result
}

/// Searches a single file's content for a substring or regex.
#[utoipa::path(
    post,
    path = "/api/v1/files/search",
    operation_id = "searchInFile",
    request_body = SearchInFileRequestDto,
    responses(
        (status = 200, description = "Matches found in the file", body = SearchInFileResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The file is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The file does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn search_in_file(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SearchInFileRequestDto>,
) -> Result<Json<SearchInFileResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .search_in_file(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Recursively sums a directory's total size (task 0071's Total Commander-style folder-size key).
#[utoipa::path(
    post,
    path = "/api/v1/directories/size",
    operation_id = "calculateFolderSize",
    request_body = CalculateFolderSizeRequestDto,
    responses(
        (status = 200, description = "The directory's recursive total size", body = CalculateFolderSizeResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn calculate_folder_size(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<CalculateFolderSizeRequestDto>,
) -> Result<Json<CalculateFolderSizeResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    state
        .service
        .calculate_folder_size(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Starts a hierarchical logical/physical disk-usage scan for one local directory.
#[utoipa::path(
    post,
    path = "/api/v1/directories/disk-usage",
    operation_id = "scanDiskUsage",
    request_body = ScanDiskUsageRequestDto,
    responses(
        (status = 202, description = "The event-driven disk-usage scan was accepted"),
        (status = 400, description = "The location is not a local directory", body = ApplicationErrorDto),
        (status = 403, description = "The directory is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The directory does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn scan_disk_usage(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ScanDiskUsageRequestDto>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    let service = state.service.clone();
    let task = tokio::spawn(async move {
        service.run_disk_usage_job(request).await;
    });
    std::mem::drop(task);
    Ok(StatusCode::ACCEPTED)
}

/// Cancels a running disk-usage scan (task 0118 follow-up), for aborting/closing a scan's tab so
/// the blocking traversal actually stops instead of continuing unobserved.
#[utoipa::path(
    delete,
    path = "/api/v1/directories/disk-usage/{scanId}",
    operation_id = "cancelDiskUsage",
    params(("scanId" = Uuid, Path)),
    responses(
        (status = 204, description = "The scan was cancelled"),
        (status = 404, description = "No scan is running with that id", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn cancel_disk_usage(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(scan_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .service
        .cancel_disk_usage(scan_id)
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// Fetches a file's git commit history, for the Alt+Space metadata panel's history section
/// (task 0135). Local provider only; returns an empty commit list (never an error) when the
/// file is outside a git working tree, on a non-local provider, or not yet committed.
#[utoipa::path(
    post,
    path = "/api/v1/files/git-history",
    operation_id = "getFileGitHistory",
    request_body = GetFileGitHistoryRequestDto,
    responses(
        (status = 200, description = "Commits touching the file, newest first", body = GetFileGitHistoryResponseDto),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_file_git_history(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GetFileGitHistoryRequestDto>,
) -> Result<Json<GetFileGitHistoryResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.location, &state.accessible_roots, request_id)?;
    Ok(Json(state.service.git_file_history(request).await))
}
