//! Backend raster tile proxy for the map component (task 0275).
//!
//! The web view asks the backend for `{z}/{x}/{y}` and never contacts the tile host itself. That
//! keeps the renderer's network surface closed (no CSP exceptions per provider), lets the request
//! carry an identifying user agent as tile usage policies require, bounds concurrency and size,
//! and caches tiles in memory so panning back does not hit the provider again. Nothing is fetched
//! ahead of what the map asks for.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fm_settings::{MapBasemap, Settings};
use reqwest::header::CONTENT_TYPE;
use tokio::sync::Semaphore;

use crate::error::ApplicationError;

/// Most simultaneous requests to a tile host (OpenStreetMap's policy allows two).
const MAX_CONCURRENT_FETCHES: usize = 2;
const MAX_TILE_BYTES: usize = 1024 * 1024;
const MAX_CACHED_TILES: usize = 512;
const MAX_CACHED_BYTES: usize = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const HIGHEST_ZOOM: u8 = 24;

/// One fetched tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTile {
    /// Image bytes as served by the provider.
    pub bytes: Arc<Vec<u8>>,
    /// The provider's content type, always `image/*`.
    pub content_type: String,
}

#[derive(Default)]
struct TileCache {
    tiles: HashMap<String, MapTile>,
    order: VecDeque<String>,
    bytes: usize,
}

impl TileCache {
    fn get(&self, key: &str) -> Option<MapTile> {
        self.tiles.get(key).cloned()
    }

    fn insert(&mut self, key: String, tile: MapTile) {
        if self.tiles.contains_key(&key) {
            return;
        }
        self.bytes += tile.bytes.len();
        self.order.push_back(key.clone());
        self.tiles.insert(key, tile);
        while self.tiles.len() > MAX_CACHED_TILES || self.bytes > MAX_CACHED_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.tiles.remove(&oldest) {
                self.bytes -= removed.bytes.len();
            }
        }
    }
}

/// Fetches and caches tiles for the configured raster basemap.
pub(crate) struct MapTileService {
    settings: Arc<Mutex<Settings>>,
    client: reqwest::Client,
    permits: Semaphore,
    cache: Mutex<TileCache>,
}

impl MapTileService {
    pub(crate) fn new(settings: Arc<Mutex<Settings>>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(format!(
                "Procyon/{} (file manager; https://github.com/erikvullings/procyon)",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .unwrap_or_default();
        Self {
            settings,
            client,
            permits: Semaphore::new(MAX_CONCURRENT_FETCHES),
            cache: Mutex::new(TileCache::default()),
        }
    }

    /// The tile URL for the current settings, or why none can be served.
    fn tile_url(&self, z: u8, x: u32, y: u32) -> Result<String, ApplicationError> {
        let settings = self
            .settings
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !settings.map.online_tiles {
            return Err(ApplicationError::PermissionDenied);
        }
        let MapBasemap::Raster {
            url_template,
            max_zoom,
            ..
        } = &settings.map.basemap
        else {
            return Err(ApplicationError::InvalidRequest(
                "the configured basemap is not a raster tile source".into(),
            ));
        };
        if z > (*max_zoom).min(HIGHEST_ZOOM) {
            return Err(ApplicationError::InvalidRequest(
                "zoom level outside the provider's range".into(),
            ));
        }
        let limit = 1_u64 << z;
        if u64::from(x) >= limit || u64::from(y) >= limit {
            return Err(ApplicationError::InvalidRequest(
                "tile coordinates outside the world".into(),
            ));
        }
        Ok(url_template
            .replace("{z}", &z.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string()))
    }

    /// Returns the tile at `z/x/y` from the cache or the configured provider.
    pub(crate) async fn tile(&self, z: u8, x: u32, y: u32) -> Result<MapTile, ApplicationError> {
        let url = self.tile_url(z, x, y)?;
        if let Some(hit) = self
            .cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&url)
        {
            return Ok(hit);
        }
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ApplicationError::Internal)?;
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|_| ApplicationError::ProviderUnavailable)?;
        match response.status().as_u16() {
            200 => {}
            404 => return Err(ApplicationError::NotFound),
            401 | 403 => return Err(ApplicationError::PermissionDenied),
            _ => return Err(ApplicationError::ProviderUnavailable),
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned())
            .filter(|value| value.starts_with("image/"))
            .ok_or(ApplicationError::ProviderUnavailable)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_TILE_BYTES as u64)
        {
            return Err(ApplicationError::ProviderUnavailable);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| ApplicationError::ProviderUnavailable)?;
        if bytes.len() > MAX_TILE_BYTES {
            return Err(ApplicationError::ProviderUnavailable);
        }
        let tile = MapTile {
            bytes: Arc::new(bytes.to_vec()),
            content_type,
        };
        self.cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(url, tile.clone());
        Ok(tile)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use fm_settings::MapSettings;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// A one-route HTTP server answering every request with `status`, `content_type` and `body`.
    async fn serve(
        status: &'static str,
        content_type: &'static str,
        body: &'static [u8],
    ) -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buffer = vec![0_u8; 4096];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                assert!(
                    String::from_utf8_lossy(&buffer[..read]).contains("Procyon/"),
                    "requests must identify the application"
                );
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(body).await;
                let _ = stream.shutdown().await;
            }
        });
        (port, hits)
    }

    fn service(port: u16, online: bool) -> MapTileService {
        let settings = Settings {
            map: MapSettings {
                online_tiles: online,
                basemap: MapBasemap::Raster {
                    url_template: format!("http://127.0.0.1:{port}/{{z}}/{{x}}/{{y}}.png"),
                    attribution: "test".into(),
                    max_zoom: 10,
                },
            },
            ..Settings::default()
        };
        MapTileService::new(Arc::new(Mutex::new(settings)))
    }

    #[tokio::test]
    async fn fetches_a_tile_once_and_serves_repeats_from_the_cache() {
        let (port, hits) = serve("200 OK", "image/png", b"PNGDATA").await;
        let tiles = service(port, true);
        let first = tiles.tile(3, 2, 1).await.expect("tile");
        assert_eq!(first.content_type, "image/png");
        assert_eq!(first.bytes.as_slice(), b"PNGDATA");
        tiles.tile(3, 2, 1).await.expect("cached tile");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        tiles.tile(3, 2, 2).await.expect("another tile");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn offline_mode_never_touches_the_network() {
        let (port, hits) = serve("200 OK", "image/png", b"x").await;
        let tiles = service(port, false);
        assert!(matches!(
            tiles.tile(1, 0, 0).await,
            Err(ApplicationError::PermissionDenied)
        ));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_coordinates_outside_the_world_and_the_zoom_range() {
        let (port, hits) = serve("200 OK", "image/png", b"x").await;
        let tiles = service(port, true);
        assert!(tiles.tile(2, 4, 0).await.is_err());
        assert!(tiles.tile(11, 0, 0).await.is_err());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn refuses_non_image_responses_and_provider_errors() {
        let (port, _) = serve("200 OK", "text/html", b"<html>").await;
        assert!(matches!(
            service(port, true).tile(1, 0, 0).await,
            Err(ApplicationError::ProviderUnavailable)
        ));
        let (port, _) = serve("404 Not Found", "text/plain", b"").await;
        assert!(matches!(
            service(port, true).tile(1, 0, 0).await,
            Err(ApplicationError::NotFound)
        ));
        let (port, _) = serve("429 Too Many Requests", "text/plain", b"").await;
        assert!(matches!(
            service(port, true).tile(1, 0, 0).await,
            Err(ApplicationError::ProviderUnavailable)
        ));
    }

    #[tokio::test]
    async fn other_basemap_kinds_are_not_served() {
        let settings = Settings {
            map: MapSettings {
                basemap: MapBasemap::Style {
                    url: "https://maps.example/style.json".into(),
                    attribution: String::new(),
                },
                ..MapSettings::default()
            },
            ..Settings::default()
        };
        let tiles = MapTileService::new(Arc::new(Mutex::new(settings)));
        assert!(matches!(
            tiles.tile(1, 0, 0).await,
            Err(ApplicationError::InvalidRequest(_))
        ));
    }
}
