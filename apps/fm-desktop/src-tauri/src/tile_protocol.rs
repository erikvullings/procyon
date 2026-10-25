//! `procyon-tiles` custom URI scheme serving raster basemap tiles to the map component
//! (task 0275). The Axum host exposes the same data at `GET /api/v1/map/tiles/{z}/{x}/{y}`; both
//! call `FileManagerService::map_tile`, so the web view never contacts the tile provider.

use std::sync::Arc;

use fm_application::{ApplicationError, FileManagerService};
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, Runtime};

use crate::AppState;

/// The URI scheme name; the frontend builds the tile template with `convertFileSrc`.
pub(crate) const SCHEME: &str = "procyon-tiles";

/// Registers the scheme on the app builder.
pub(crate) fn register<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_asynchronous_uri_scheme_protocol(SCHEME, |ctx, request, responder| {
        if !crate::plugin_spa::trusted_invoke_label(ctx.webview_label()) {
            responder.respond(with_cors(plain(StatusCode::FORBIDDEN)));
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
            responder.respond(with_cors(response));
        });
    })
}

/// Reads `/{z}/{x}/{y}` (an optional file extension on `y` is ignored).
fn coordinates_from_path(path: &str) -> Option<(u8, u32, u32)> {
    let mut parts = path.trim_matches('/').split('/');
    let z = parts.next()?.parse().ok()?;
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.split('.').next()?.parse().ok()?;
    parts.next().is_none().then_some((z, x, y))
}

async fn handle(service: &FileManagerService, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some((z, x, y)) = coordinates_from_path(request.uri().path()) else {
        return plain(StatusCode::BAD_REQUEST);
    };
    match service.map_tile(z, x, y).await {
        Ok(tile) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, tile.content_type)
            .header(header::CACHE_CONTROL, "private, max-age=86400")
            .body(tile.bytes.as_ref().clone())
            .unwrap_or_default(),
        Err(error) => plain(status_for(&error)),
    }
}

fn status_for(error: &ApplicationError) -> StatusCode {
    match error {
        ApplicationError::NotFound => StatusCode::NOT_FOUND,
        ApplicationError::PermissionDenied => StatusCode::FORBIDDEN,
        ApplicationError::InvalidRequest(_) | ApplicationError::Rejected(_) => {
            StatusCode::BAD_REQUEST
        }
        ApplicationError::ProviderUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Lets the web view read the response from script (`fetch`): custom schemes get no CORS headers
/// from Tauri, and MapLibre loads tiles and PMTiles ranges with `fetch`.
fn with_cors(mut response: Response<Vec<u8>>) -> Response<Vec<u8>> {
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        header::HeaderValue::from_static("Content-Range, Accept-Ranges, Content-Length"),
    );
    response
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
    fn reads_tile_coordinates_from_the_uri_path() {
        assert_eq!(coordinates_from_path("/3/2/1"), Some((3, 2, 1)));
        assert_eq!(coordinates_from_path("/3/2/1.png"), Some((3, 2, 1)));
    }

    #[test]
    fn rejects_malformed_paths() {
        assert_eq!(coordinates_from_path("/3/2"), None);
        assert_eq!(coordinates_from_path("/3/2/1/0"), None);
        assert_eq!(coordinates_from_path("/a/b/c"), None);
        assert_eq!(coordinates_from_path("/300/1/1"), None);
    }
}
