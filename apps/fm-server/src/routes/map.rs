//! Raster basemap tiles for the map component (task 0275).
//!
//! The browser asks this route instead of contacting the tile provider, so the page never needs
//! a network exception for the provider and requests carry the application's user agent. The
//! session token travels as `?token=` because map tile loaders cannot set headers.

use axum::body::Body;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use fm_transport_dto::ApplicationErrorDto;
use tower_http::request_id::RequestId;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Returns one tile of the configured raster basemap.
#[utoipa::path(
    get,
    path = "/api/v1/map/tiles/{z}/{x}/{y}",
    operation_id = "getMapTile",
    params(
        ("z" = u8, Path, description = "Zoom level"),
        ("x" = u32, Path, description = "Tile column"),
        ("y" = u32, Path, description = "Tile row"),
    ),
    responses(
        (status = 200, description = "The tile image", content_type = "image/*"),
        (status = 400, description = "Coordinates outside the world or the configured zoom range", body = ApplicationErrorDto),
        (status = 403, description = "Online tiles are disabled", body = ApplicationErrorDto),
        (status = 404, description = "The provider has no such tile", body = ApplicationErrorDto),
        (status = 503, description = "The tile provider is unavailable", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_map_tile(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Path((z, x, y)): Path<(u8, u32, u32)>,
) -> Result<Response<Body>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let tile = state
        .service
        .map_tile(z, x, y)
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    let content_type = HeaderValue::from_str(&tile.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("image/png"));
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, max-age=86400")
        .body(Body::from(tile.bytes.as_ref().clone()))
        .unwrap_or_default())
}
