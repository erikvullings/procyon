//! Permission, ownership and attribute editor REST transport (task 0258). The change itself is a
//! `changeAttributes` operation started through the operation routes.

use axum::Json;
use axum::extract::{Extension, State};
use fm_transport_dto::{
    ApplicationErrorDto, InspectAttributesRequestDto, InspectAttributesResponseDto,
    PreviewAttributeChangeRequestDto, PreviewAttributeChangeResponseDto,
};
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Reads which attribute edits a selection supports and each entry's current state.
#[utoipa::path(
    post,
    path = "/api/v1/attributes/inspect",
    operation_id = "inspectAttributes",
    request_body = InspectAttributesRequestDto,
    responses(
        (status = 200, description = "Editable fields and current state", body = InspectAttributesResponseDto),
        (status = 403, description = "An entry is outside the accessible roots", body = ApplicationErrorDto),
        (status = 404, description = "An entry does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn inspect_attributes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<InspectAttributesRequestDto>,
) -> Result<Json<InspectAttributesResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    for source in &request.sources {
        crate::error::require_within_roots(source, &state.accessible_roots, request_id)?;
    }
    state
        .service
        .inspect_attributes(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Counts the entries an attribute change would touch before it is applied.
#[utoipa::path(
    post,
    path = "/api/v1/attributes/preview",
    operation_id = "previewAttributeChange",
    request_body = PreviewAttributeChangeRequestDto,
    responses(
        (status = 200, description = "Entries the change would touch", body = PreviewAttributeChangeResponseDto),
        (status = 403, description = "An entry is outside the accessible roots", body = ApplicationErrorDto),
        (status = 404, description = "An entry does not exist", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn preview_attribute_change(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<PreviewAttributeChangeRequestDto>,
) -> Result<Json<PreviewAttributeChangeResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    for source in &request.sources {
        crate::error::require_within_roots(source, &state.accessible_roots, request_id)?;
    }
    state
        .service
        .preview_attribute_change(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}
