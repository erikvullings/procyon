//! Thin REST adapter for the image pane's prompt history.

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use fm_transport_dto::ImagePromptDto;
use tower_http::request_id::RequestId;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Lists the remembered prompts, most recently used first.
#[utoipa::path(
    get,
    path = "/api/v1/images/prompts",
    operation_id = "listImagePrompts",
    responses((status = 200, description = "The remembered prompts", body = Vec<ImagePromptDto>))
)]
pub(crate) async fn list_image_prompts(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<Vec<ImagePromptDto>>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .list_image_prompts()
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Remembers a prompt with its settings. A prompt that matches an earlier one only refreshes it.
#[utoipa::path(
    post,
    path = "/api/v1/images/prompts",
    operation_id = "saveImagePrompt",
    request_body = ImagePromptDto,
    responses(
        (status = 200, description = "The stored prompt", body = ImagePromptDto),
        (status = 400, description = "The prompt was outside the bounds", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn save_image_prompt(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(prompt): Json<ImagePromptDto>,
) -> Result<Json<ImagePromptDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .save_image_prompt(prompt)
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Forgets one remembered prompt.
#[utoipa::path(
    delete,
    path = "/api/v1/images/prompts/{prompt_id}",
    operation_id = "deleteImagePrompt",
    params(("prompt_id" = Uuid, Path, description = "Prompt to forget")),
    responses(
        (status = 204, description = "The prompt was forgotten"),
        (status = 404, description = "No such prompt", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn delete_image_prompt(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path(prompt_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .delete_image_prompt(prompt_id)
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(StatusCode::NO_CONTENT)
}
