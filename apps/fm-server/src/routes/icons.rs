//! Native file icon REST transport (task 0091).

use axum::extract::{Extension, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use fm_transport_dto::ApplicationErrorDto;
use serde::Deserialize;
use tower_http::request_id::RequestId;
use utoipa::IntoParams;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// One concrete entry location whose extension/UTI identifies the native icon.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FileIconQuery {
    uri: String,
}

/// Returns PNG bytes from the active platform adapter, if supported.
#[utoipa::path(
    get,
    path = "/api/v1/icons",
    operation_id = "getFileIcon",
    params(FileIconQuery),
    responses(
        (status = 200, description = "Native file icon PNG", content_type = "image/png"),
        (status = 400, description = "The location URI was invalid", body = ApplicationErrorDto),
        (status = 404, description = "No native icon is available", body = ApplicationErrorDto),
        (status = 502, description = "The platform icon lookup failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_file_icon(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<FileIconQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let request_id = extract_request_id(&request_id);
    state
        .service
        .file_icon(&query.uri)
        .map(|bytes| (StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], bytes))
        .map_err(|error| ApiError::new(error, request_id))
}
