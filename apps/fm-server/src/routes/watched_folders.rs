//! HTTP adapter for watched-folder settings and preview jobs.

use axum::{
    Json,
    extract::{Extension, State},
};
use fm_transport_dto::{
    ApplicationErrorDto,
    watched_folders::{OrganizerRequest, OrganizerResponse},
};
use tower_http::request_id::RequestId;

use crate::{
    error::{ApiError, extract_request_id, require_within_roots},
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/v1/watched-folders",
    operation_id = "manageWatchedFolders",
    request_body = OrganizerRequest,
    responses((status = 200, body = OrganizerResponse), (status = 400, body = ApplicationErrorDto), (status = 403, body = ApplicationErrorDto))
)]
pub(crate) async fn manage(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<OrganizerRequest>,
) -> Result<Json<OrganizerResponse>, ApiError> {
    let request_id = extract_request_id(&request_id);
    match &request {
        OrganizerRequest::Preview { root } | OrganizerRequest::Remove { root } => {
            require_within_roots(root, &state.accessible_roots, request_id)?;
        }
        OrganizerRequest::Save { root, approved, .. } => {
            require_within_roots(root, &state.accessible_roots, request_id)?;
            for item in approved {
                require_within_roots(&item.source, &state.accessible_roots, request_id)?;
            }
        }
        _ => {}
    }
    let mut response = state
        .service
        .watched_folder(request)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    response.folders.retain(|folder| {
        require_within_roots(&folder.root, &state.accessible_roots, request_id).is_ok()
    });
    Ok(Json(response))
}
