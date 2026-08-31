//! Thumbnail REST transport (task 0134).

use axum::extract::{Extension, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use fm_transport_dto::ApplicationErrorDto;
use serde::Deserialize;
use tower_http::request_id::RequestId;
use utoipa::IntoParams;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// A concrete entry location and requested size for a downscaled preview.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct ThumbnailQuery {
    uri: String,
    /// One of `small`, `medium`, `large`.
    size: String,
}

/// Returns JPEG thumbnail bytes for an image or CBZ/CBR comic archive
/// entry, if supported.
#[utoipa::path(
    get,
    path = "/api/v1/thumbnails",
    operation_id = "getThumbnail",
    params(ThumbnailQuery),
    responses(
        (status = 200, description = "Downscaled JPEG preview", content_type = "image/jpeg"),
        (status = 400, description = "The location URI or size was invalid", body = ApplicationErrorDto),
        (status = 404, description = "No thumbnail is available for this entry", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_thumbnail(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<ThumbnailQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .thumbnail(&query.uri, &query.size)
        .await
        .map(|bytes| {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "image/jpeg")],
                bytes,
            )
        })
        .map_err(|error| ApiError::new(error, request_id))
}
