//! Finder tags and Spotlight comment REST transport (task 0136).

use axum::Json;
use axum::extract::{Extension, Query, State};
use fm_transport_dto::{ApplicationErrorDto, FinderTagsDto, SpotlightCommentDto};
use serde::Deserialize;
use tower_http::request_id::RequestId;
use utoipa::IntoParams;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// One concrete entry location whose tags/comment are being read or written.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct EntryUriQuery {
    uri: String,
}

/// Returns an entry's Finder tags, or an empty list if unsupported/unset.
#[utoipa::path(
    get,
    path = "/api/v1/finder-tags",
    operation_id = "getFinderTags",
    params(EntryUriQuery),
    responses(
        (status = 200, description = "The entry's Finder tags", body = FinderTagsDto),
        (status = 400, description = "The location URI was invalid", body = ApplicationErrorDto),
        (status = 404, description = "The entry does not exist or tags are unsupported", body = ApplicationErrorDto),
        (status = 502, description = "The platform Finder-tag lookup failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_finder_tags(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<EntryUriQuery>,
) -> Result<Json<FinderTagsDto>, ApiError> {
    state
        .service
        .finder_tags(&query.uri)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// Replaces an entry's complete set of Finder tags, returning the persisted set back.
#[utoipa::path(
    put,
    path = "/api/v1/finder-tags",
    operation_id = "setFinderTags",
    params(EntryUriQuery),
    request_body = FinderTagsDto,
    responses(
        (status = 200, description = "The persisted Finder tags", body = FinderTagsDto),
        (status = 400, description = "The location URI was invalid", body = ApplicationErrorDto),
        (status = 404, description = "The entry does not exist", body = ApplicationErrorDto),
        (status = 502, description = "The platform Finder-tag write failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn set_finder_tags(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<EntryUriQuery>,
    Json(body): Json<FinderTagsDto>,
) -> Result<Json<FinderTagsDto>, ApiError> {
    state
        .service
        .set_finder_tags(&query.uri, body)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// Returns an entry's Spotlight comment, or `null` if unsupported/unset.
#[utoipa::path(
    get,
    path = "/api/v1/spotlight-comment",
    operation_id = "getSpotlightComment",
    params(EntryUriQuery),
    responses(
        (status = 200, description = "The entry's Spotlight comment", body = SpotlightCommentDto),
        (status = 400, description = "The location URI was invalid", body = ApplicationErrorDto),
        (status = 404, description = "The entry does not exist or comments are unsupported", body = ApplicationErrorDto),
        (status = 502, description = "The platform comment lookup failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn get_spotlight_comment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<EntryUriQuery>,
) -> Result<Json<SpotlightCommentDto>, ApiError> {
    state
        .service
        .spotlight_comment(&query.uri)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}

/// Sets or clears (`comment: null`) an entry's Spotlight comment, returning the persisted value back.
#[utoipa::path(
    put,
    path = "/api/v1/spotlight-comment",
    operation_id = "setSpotlightComment",
    params(EntryUriQuery),
    request_body = SpotlightCommentDto,
    responses(
        (status = 200, description = "The persisted Spotlight comment", body = SpotlightCommentDto),
        (status = 400, description = "The location URI was invalid", body = ApplicationErrorDto),
        (status = 404, description = "The entry does not exist", body = ApplicationErrorDto),
        (status = 502, description = "The platform comment write failed", body = ApplicationErrorDto),
    )
)]
pub(crate) async fn set_spotlight_comment(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<EntryUriQuery>,
    Json(body): Json<SpotlightCommentDto>,
) -> Result<Json<SpotlightCommentDto>, ApiError> {
    state
        .service
        .set_spotlight_comment(&query.uri, body)
        .map(Json)
        .map_err(|error| ApiError::new(error, extract_request_id(&request_id)))
}
