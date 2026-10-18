//! `procyon-media` custom URI scheme that feeds `<audio>`/`<video>` elements seekable byte
//! ranges of any VFS entry (task 0260). The Axum host exposes the same semantics at
//! `GET /api/v1/media`; both call `FileManagerService::open_media_range`.
//!
//! Tauri buffers a custom-protocol response, so every answer is capped at
//! [`MAX_RESPONSE_BYTES`]; media elements simply issue follow-up range requests.

use std::sync::Arc;

use fm_application::media_stream::{MediaContent, MediaOutcome, MediaRangeRequest};
use fm_application::{ApplicationError, FileManagerService};
use fm_domain::Location;
use fm_transport_dto::LocationDto;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime};
use tokio::io::AsyncReadExt;

use crate::AppState;

/// The URI scheme name; the frontend builds URLs for it with `convertFileSrc`.
pub(crate) const SCHEME: &str = "procyon-media";

/// Largest body returned by one request.
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;

/// Registers the scheme on the app builder.
pub(crate) fn register<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |ctx, request, responder| {
        if !crate::plugin_spa::trusted_invoke_label(ctx.webview_label()) {
            responder.respond(plain(StatusCode::FORBIDDEN));
            return;
        }
        let service = ctx
            .app_handle()
            .try_state::<AppState>()
            .map(|state| Arc::clone(&state.service));
        tauri::async_runtime::spawn(async move {
            let response = match service {
                Some(service) => handle(&service, &request).await,
                None => plain(StatusCode::SERVICE_UNAVAILABLE),
            };
            responder.respond(response);
        });
    })
}

/// Decodes the percent-encoded JSON `LocationDto` carried in the URI path.
fn location_from_path(path: &str) -> Option<Location> {
    let decoded = percent_encoding::percent_decode_str(path.trim_start_matches('/'))
        .decode_utf8()
        .ok()?;
    serde_json::from_str::<LocationDto>(&decoded)
        .ok()
        .map(Location::from)
}

async fn handle(service: &FileManagerService, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(location) = location_from_path(request.uri().path()) else {
        return plain(StatusCode::BAD_REQUEST);
    };
    let range_header = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let outcome = service
        .open_media_range(MediaRangeRequest {
            location,
            range_header,
            max_response_bytes: Some(MAX_RESPONSE_BYTES),
        })
        .await;
    match outcome {
        Ok(MediaOutcome::Unsatisfiable { total_size }) => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{total_size}"))
            .header(header::ACCEPT_RANGES, "bytes")
            .body(Vec::new())
            .unwrap_or_default(),
        Ok(MediaOutcome::Content(content)) => content_response(content).await,
        Err(error) => plain(status_for(&error)),
    }
}

async fn content_response(content: MediaContent) -> Response<Vec<u8>> {
    let MediaContent {
        total_size,
        span,
        content_length,
        content_type,
        mut reader,
        cancellation: _cancellation,
    } = content;
    let mut body = Vec::new();
    if reader.read_to_end(&mut body).await.is_err() || body.len() as u64 != content_length {
        return plain(StatusCode::INTERNAL_SERVER_ERROR);
    }
    let builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, content_length);
    match span {
        Some(span) => builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{total_size}", span.start, span.end),
            )
            .body(body),
        None => builder.status(StatusCode::OK).body(body),
    }
    .unwrap_or_default()
}

fn status_for(error: &ApplicationError) -> StatusCode {
    match error {
        ApplicationError::NotFound => StatusCode::NOT_FOUND,
        ApplicationError::PermissionDenied => StatusCode::FORBIDDEN,
        ApplicationError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn plain(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .body(Vec::new())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_round_trips_through_an_encoded_uri_path() {
        let json = r#"{"providerId":"local","uri":"file:///tmp/a b.mp3"}"#;
        let encoded =
            percent_encoding::utf8_percent_encode(json, percent_encoding::NON_ALPHANUMERIC);
        let location = location_from_path(&format!("/{encoded}")).expect("must decode");
        assert_eq!(location.provider_id.as_str(), "local");
        assert_eq!(location.uri, "file:///tmp/a b.mp3");
    }

    #[test]
    fn garbage_paths_are_rejected() {
        assert!(location_from_path("/not-json").is_none());
    }
}
