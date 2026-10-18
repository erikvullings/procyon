//! Thin REST adapter for per-folder view memory (task 0270).

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use fm_transport_dto::{ApplicationErrorDto, ForgetFolderViewRequestDto};
use tower_http::request_id::RequestId;

use crate::{
    error::{ApiError, extract_request_id, require_within_roots},
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/v1/folder-views/forget",
    operation_id = "forgetFolderView",
    request_body = ForgetFolderViewRequestDto,
    responses(
        (status = 204, description = "The remembered view is gone (or there was none)"),
        (status = 403, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn forget_folder_view(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<ForgetFolderViewRequestDto>,
) -> Result<StatusCode, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    require_within_roots(&request.location, &state.accessible_roots, correlation_id)?;
    state
        .service
        .forget_folder_view(request.location)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|error| ApiError::new(error, correlation_id))
}
