//! Seekable byte-range media streaming for `<audio>`/`<video>` elements (task 0260).
//!
//! A `GET` with the file location in the query string, because media elements cannot send a
//! body or an `Authorization` header (the session token travels as `?token=`, like SSE).

use axum::body::Body;
use axum::extract::{Extension, Query, State};
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use fm_application::ApplicationError;
use fm_application::media_stream::{MediaContent, MediaOutcome, MediaRangeRequest};
use fm_transport_dto::{ApplicationErrorDto, LocationDto};
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use tower_http::request_id::RequestId;
use utoipa::IntoParams;

use crate::error::{ApiError, extract_request_id};
use crate::state::AppState;

/// Query string of the media route.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct MediaQuery {
    /// JSON-encoded `LocationDto` of the file to stream.
    location: String,
    /// Session token, accepted by the auth middleware for requests that cannot set headers.
    #[serde(default)]
    #[allow(dead_code)]
    token: Option<String>,
}

/// Streams a file, honouring HTTP `Range` requests.
#[utoipa::path(
    get,
    path = "/api/v1/media",
    operation_id = "streamMedia",
    params(MediaQuery),
    responses(
        (status = 200, description = "The whole file", content_type = "application/octet-stream"),
        (status = 206, description = "The requested byte range", content_type = "application/octet-stream"),
        (status = 400, description = "The request was invalid", body = ApplicationErrorDto),
        (status = 403, description = "The file is unreadable", body = ApplicationErrorDto),
        (status = 404, description = "The file does not exist", body = ApplicationErrorDto),
        (status = 416, description = "The range is outside the file"),
    )
)]
pub(crate) async fn stream_media(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<MediaQuery>,
    headers: HeaderMap,
) -> Result<Response<Body>, ApiError> {
    let request_id = extract_request_id(&request_id);
    let location: LocationDto = serde_json::from_str(&query.location).map_err(|error| {
        ApiError::new(
            ApplicationError::InvalidRequest(format!("location must be JSON: {error}")),
            request_id,
        )
    })?;
    crate::error::require_within_roots(&location, &state.accessible_roots, request_id)?;
    let range_header = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let outcome = state
        .service
        .open_media_range(MediaRangeRequest {
            location: location.into(),
            range_header,
            max_response_bytes: None,
        })
        .await
        .map_err(|error| ApiError::new(error, request_id))?;
    Ok(match outcome {
        MediaOutcome::Unsatisfiable { total_size } => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{total_size}"))
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Body::empty())
            .unwrap_or_default(),
        MediaOutcome::Content(content) => content_response(content),
    })
}

/// Builds the 200/206 response; the body cancels the provider read when it is dropped.
fn content_response(content: MediaContent) -> Response<Body> {
    let MediaContent {
        total_size,
        span,
        content_length,
        content_type,
        reader,
        cancellation,
    } = content;
    let guard = cancellation.drop_guard();
    let stream = futures::StreamExt::map(ReaderStream::new(reader), move |chunk| {
        let _keep_alive = &guard;
        chunk
    });
    let mut builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
        .header(header::CONTENT_LENGTH, content_length);
    builder = match span {
        Some(span) => builder.status(StatusCode::PARTIAL_CONTENT).header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{total_size}", span.start, span.end),
        ),
        None => builder.status(StatusCode::OK),
    };
    builder.body(Body::from_stream(stream)).unwrap_or_default()
}
