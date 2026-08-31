//! Thin REST adapter for the backend action registry (specification §8, §18).

use crate::{
    error::{ApiError, extract_request_id},
    state::AppState,
};
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::HeaderMap,
};
use fm_transport_dto::{
    ActionDescriptorDto, ActionResultDto, ApplicationErrorDto, InvokeActionRequestDto,
};
use tower_http::request_id::RequestId;

#[utoipa::path(
    get,
    path = "/api/v1/actions",
    operation_id = "listActions",
    responses((status = 200, body = Vec<ActionDescriptorDto>))
)]
pub(crate) async fn list_actions(State(state): State<AppState>) -> Json<Vec<ActionDescriptorDto>> {
    Json(state.service.list_actions())
}

#[utoipa::path(
    post,
    path = "/api/v1/actions/{actionId}/invoke",
    operation_id = "invokeAction",
    params(("actionId" = String, Path)),
    request_body = InvokeActionRequestDto,
    responses(
        (status = 200, body = ActionResultDto),
        (status = 400, body = ApplicationErrorDto),
        (status = 404, body = ApplicationErrorDto),
        (status = 409, body = ApplicationErrorDto)
    )
)]
pub(crate) async fn invoke_action(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(action_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<InvokeActionRequestDto>,
) -> Result<Json<ActionResultDto>, ApiError> {
    let correlation_id = extract_request_id(&request_id);
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    state
        .service
        .invoke_action(action_id, request, key)
        .map(Json)
        .map_err(|error| ApiError::new(error, correlation_id))
}
