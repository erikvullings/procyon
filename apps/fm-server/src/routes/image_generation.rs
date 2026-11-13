//! Thin REST adapter for the image generation pane (task 0325).
//!
//! Generation returns only a list of stored images; the bytes themselves come from the bytes route,
//! so no image travels as JSON. Saving is the only call that writes, and it goes through the same
//! accessible-root check as every other write.

use axum::Json;
use axum::body::Body;
use axum::extract::{Extension, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use fm_transport_dto::{
    CancelImageGenerationRequestDto, DiscardImageBatchRequestDto, GenerateImagesRequestDto,
    ImageBatchDto, SaveGeneratedImageRequestDto, SaveGeneratedImageResponseDto,
};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Query string of the bytes route.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ImageBytesQuery {
    /// Batch the image came from.
    generation_id: Uuid,
    /// Image to fetch.
    image_id: Uuid,
    /// Serve the stored original instead of the preview downscale.
    #[serde(default)]
    original: bool,
}

/// Asks the image role's endpoint for a bounded batch. Nothing is written to disk: the images are
/// held in a short-lived store until they are saved or discarded.
#[utoipa::path(
    post,
    path = "/api/v1/images/generate",
    operation_id = "generateImages",
    request_body = GenerateImagesRequestDto,
    responses(
        (status = 200, description = "The generated batch", body = ImageBatchDto),
        (status = 400, description = "The request was outside the bounds", body = fm_transport_dto::ApplicationErrorDto),
        (status = 403, description = "The image role needs consent or cloud permission", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn generate_images(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<GenerateImagesRequestDto>,
) -> Result<Json<ImageBatchDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .generate_images(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Stops one running generation.
#[utoipa::path(
    post,
    path = "/api/v1/images/cancel",
    operation_id = "cancelImageGeneration",
    request_body = CancelImageGenerationRequestDto,
    responses((status = 204))
)]
pub(crate) async fn cancel_image_generation(
    State(state): State<AppState>,
    Json(request): Json<CancelImageGenerationRequestDto>,
) -> StatusCode {
    state.service.cancel_image_generation(request);
    StatusCode::NO_CONTENT
}

/// Serves the bytes of one stored image: the preview downscale, or the original when asked for.
#[utoipa::path(
    get,
    path = "/api/v1/images/bytes",
    operation_id = "generatedImageBytes",
    params(ImageBytesQuery),
    responses(
        (status = 200, description = "The image bytes", content_type = "image/png"),
        (status = 404, description = "The batch is no longer held", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn generated_image_bytes(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<ImageBytesQuery>,
) -> Result<Response, ApiError> {
    let request_id = extract_request_id(&request_id);
    let image = state
        .service
        .generated_image_bytes(query.generation_id, query.image_id, query.original)
        .map_err(|error| ApiError::new(error, request_id))?;
    Response::builder()
        .header(header::CONTENT_TYPE, image.media_type)
        // The bytes live only as long as the batch does, and are the same for every viewer of it.
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .body(Body::from(image.bytes))
        .map_err(|_| ApiError::new(fm_application::ApplicationError::Internal, request_id))
}

/// Writes one stored image to the chosen folder and file name. An existing file is refused until the
/// person asks for it to be replaced.
#[utoipa::path(
    post,
    path = "/api/v1/images/save",
    operation_id = "saveGeneratedImage",
    request_body = SaveGeneratedImageRequestDto,
    responses(
        (status = 200, description = "The saved file", body = SaveGeneratedImageResponseDto),
        (status = 400, description = "The file name was refused", body = fm_transport_dto::ApplicationErrorDto),
        (status = 404, description = "The batch is no longer held", body = fm_transport_dto::ApplicationErrorDto),
        (status = 409, description = "The file already exists", body = fm_transport_dto::ApplicationErrorDto),
    )
)]
pub(crate) async fn save_generated_image(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(request): Json<SaveGeneratedImageRequestDto>,
) -> Result<Json<SaveGeneratedImageResponseDto>, ApiError> {
    let request_id = extract_request_id(&request_id);
    crate::error::require_within_roots(&request.destination, &state.accessible_roots, request_id)?;
    state
        .service
        .save_generated_image(request)
        .await
        .map(Json)
        .map_err(|error| ApiError::new(error, request_id))
}

/// Drops a batch from the store without saving any of its images.
#[utoipa::path(
    post,
    path = "/api/v1/images/discard",
    operation_id = "discardImageBatch",
    request_body = DiscardImageBatchRequestDto,
    responses((status = 204))
)]
pub(crate) async fn discard_image_batch(
    State(state): State<AppState>,
    Json(request): Json<DiscardImageBatchRequestDto>,
) -> StatusCode {
    state.service.discard_image_batch(request.generation_id);
    StatusCode::NO_CONTENT
}
