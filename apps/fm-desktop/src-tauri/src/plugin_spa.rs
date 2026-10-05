//! Isolated, package-only desktop WebViews for enabled plugin SPA panels.

use std::{
    collections::HashSet,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use fm_application::FileManagerService;
use fm_domain::{Location, ProviderId};
use fm_transport_dto::{LoadEditableFileRequestDto, LocationDto, SaveEditableFileRequestDto};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalSize, Rect, Runtime, State, Url,
    Webview, WebviewBuilder, WebviewUrl, Window,
    http::{Method, Response, StatusCode},
    webview::{NewWindowResponse, PageLoadEvent},
};
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::AppState;

const PANEL_SLOTS: usize = 16;
// Monaco's packaged TypeScript worker is ~5.8 MiB; larger assets are rejected.
const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BRIDGE_BYTES: usize = 1024 * 1024;
const BRIDGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const VISIBLE_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(20);
const HIDDEN_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(120);
const SVGO_PLUGIN_ID: &str = "procyon.svgo";

fn require_local_svgo(plugin_id: &str, location: &LocationDto) -> Result<(), PanelError> {
    if plugin_id != SVGO_PLUGIN_ID {
        return Ok(());
    }
    if location.provider_id != "local"
        || Location::try_new(ProviderId::new("local"), location.uri.clone())
            .and_then(|location| location.to_native_path())
            .is_err()
    {
        return Err(PanelError::Denied);
    }
    Ok(())
}

#[cfg(test)]
async fn svgo_file_lock(location: &LocationDto) -> Result<std::fs::File, PanelError> {
    use sha2::{Digest, Sha256};
    let path = Location::try_new(ProviderId::new("local"), location.uri.clone())
        .and_then(|location| location.to_native_path())
        .map_err(|_| PanelError::Denied)?;
    let parent = path.parent().ok_or(PanelError::Invalid)?.to_path_buf();
    let name = path.file_name().ok_or(PanelError::Invalid)?.to_os_string();
    tokio::task::spawn_blocking(move || {
        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| PanelError::File(error.to_string()))?;
        let target = canonical_parent
            .join(name)
            .canonicalize()
            .map_err(|error| PanelError::File(error.to_string()))?;
        let key = Sha256::digest(target.to_string_lossy().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let cache = dirs::cache_dir()
            .ok_or(PanelError::Unavailable)?
            .join("procyon")
            .join("editor-locks");
        std::fs::create_dir_all(&cache).map_err(|error| PanelError::File(error.to_string()))?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join(format!("{key}.lock")))
            .map_err(|error| PanelError::File(error.to_string()))?;
        fs2::FileExt::lock_exclusive(&file).map_err(|error| PanelError::File(error.to_string()))?;
        Ok(file)
    })
    .await
    .map_err(|error| PanelError::File(error.to_string()))?
}

fn panel_csp(origin: &str) -> String {
    #[cfg(feature = "native-spa-smoke")]
    if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some() {
        return format!(
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self' data:; manifest-src 'self'; connect-src {origin}/bridge {origin}/smoke; worker-src 'self' blob:; frame-src 'none'; child-src 'none'; object-src 'none'; form-action 'none'; navigate-to 'self'; base-uri 'none'"
        );
    }
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self' data:; manifest-src 'self'; connect-src {origin}/bridge; worker-src 'self' blob:; frame-src 'none'; child-src 'none'; object-src 'none'; form-action 'none'; navigate-to 'self'; base-uri 'none'"
    )
}

/// Only the trusted app windows may call *any* app command, regardless of the
/// capabilities Tauri associates with a newly registered custom scheme.
pub(crate) fn trusted_invoke_label(label: &str) -> bool {
    label == "main"
        || label
            .strip_prefix("workspace-")
            .and_then(|rest| rest.split_once('_').map_or(Some(rest), |(id, _)| Some(id)))
            .is_some_and(|id| Uuid::parse_str(id).is_ok())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PanelError {
    #[error("plugin panel unavailable")]
    Unavailable,
    #[error("plugin panel permission denied")]
    Denied,
    #[error("plugin panel request is invalid")]
    Invalid,
    #[error("plugin panel resource limit exceeded")]
    TooLarge,
    #[error("too many plugin panels are open (limit: 16)")]
    Capacity,
    #[error("plugin panel operation timed out")]
    Timeout,
    #[error("plugin panel file operation failed: {0}")]
    File(String),
}

impl serde::Serialize for PanelError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

struct PanelSession {
    label: String,
    owner_window: String,
    plugin_id: String,
    action_id: String,
    directory: PathBuf,
    entrypoint: PathBuf,
    location: LocationDto,
    token: String,
    revision: AsyncMutex<String>,
    save_lock: Arc<AsyncMutex<()>>,
    close_lock: AsyncMutex<()>,
    settings_sequence: Mutex<u64>,
    flush_sender: Mutex<Option<oneshot::Sender<bool>>>,
    heartbeat: Mutex<Option<HeartbeatProbe>>,
    heartbeat_responded: AtomicBool,
    created: Instant,
    loaded: AtomicBool,
    visible: AtomicBool,
    shutdown: CancellationToken,
    #[cfg(target_os = "linux")]
    _context_directory: tempfile::TempDir,
}

struct HeartbeatProbe {
    challenge: String,
    sent: Instant,
    retried: bool,
}

impl PanelSession {
    fn next_heartbeat(&self, now: Instant) -> Result<Option<String>, ()> {
        if !self.loaded.load(Ordering::SeqCst) {
            return if now.duration_since(self.created) >= VISIBLE_HEARTBEAT_TIMEOUT {
                Err(())
            } else {
                Ok(None)
            };
        }
        let mut pending = self
            .heartbeat
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut retried = false;
        if let Some(probe) = pending.as_ref() {
            let timeout = if self.visible.load(Ordering::SeqCst) {
                VISIBLE_HEARTBEAT_TIMEOUT
            } else {
                HIDDEN_HEARTBEAT_TIMEOUT
            };
            if now.duration_since(probe.sent) < timeout {
                return Ok(None);
            }
            if probe.retried {
                return Err(());
            }
            retried = true;
        }
        let challenge = Uuid::new_v4().simple().to_string();
        *pending = Some(HeartbeatProbe {
            challenge: challenge.clone(),
            sent: now,
            retried,
        });
        Ok(Some(challenge))
    }

    fn acknowledge_heartbeat(&self, challenge: &str) -> Result<(), PanelError> {
        let mut pending = self
            .heartbeat
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending
            .as_ref()
            .is_none_or(|probe| probe.challenge != challenge)
        {
            return Err(PanelError::Invalid);
        }
        *pending = None;
        self.heartbeat_responded.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PanelTheme {
    Light,
    Dark,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpenPanelRequest {
    pub(crate) plugin_id: String,
    pub(crate) action_id: String,
    pub(crate) location: LocationDto,
    pub(crate) bounds: PanelBounds,
    pub(crate) theme: PanelTheme,
}

enum Slot {
    Reserved {
        label: String,
        location: LocationDto,
        save_lock: Arc<AsyncMutex<()>>,
    },
    Active(Arc<PanelSession>),
}

#[derive(Default)]
pub(crate) struct PanelRegistry {
    slots: Mutex<Vec<Option<Slot>>>,
    closing_windows: Mutex<HashSet<String>>,
}

impl PanelRegistry {
    #[cfg(feature = "native-spa-smoke")]
    pub(crate) fn heartbeat_responded(&self, label: &str) -> bool {
        self.sessions().into_iter().any(|session| {
            session.label == label && session.heartbeat_responded.load(Ordering::SeqCst)
        })
    }

    fn reserve(
        &self,
        label: String,
        location: &LocationDto,
    ) -> Result<(usize, Arc<AsyncMutex<()>>), PanelError> {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        if slots.len() < PANEL_SLOTS {
            slots.resize_with(PANEL_SLOTS, || None);
        }
        let save_lock = slots
            .iter()
            .find_map(|slot| match slot {
                Some(Slot::Reserved {
                    location: selected,
                    save_lock,
                    ..
                }) if selected == location => Some(Arc::clone(save_lock)),
                Some(Slot::Active(session)) if session.location == *location => {
                    Some(Arc::clone(&session.save_lock))
                }
                _ => None,
            })
            .unwrap_or_else(|| Arc::new(AsyncMutex::new(())));
        let (index, slot) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
            .ok_or(PanelError::Capacity)?;
        *slot = Some(Slot::Reserved {
            label,
            location: location.clone(),
            save_lock: Arc::clone(&save_lock),
        });
        Ok((index, save_lock))
    }

    fn activate(&self, index: usize, session: Arc<PanelSession>) -> Result<(), PanelError> {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        let valid = matches!(
            slots.get(index),
            Some(Some(Slot::Reserved { label, location, save_lock }))
                if label == &session.label
                    && location == &session.location
                    && Arc::ptr_eq(save_lock, &session.save_lock)
        );
        if !valid {
            return Err(PanelError::Unavailable);
        }
        slots[index] = Some(Slot::Active(session));
        Ok(())
    }

    fn session_for(&self, index: usize, label: &str) -> Option<Arc<PanelSession>> {
        let slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        match slots.get(index)? {
            Some(Slot::Active(session))
                if session.label == label && !session.shutdown.is_cancelled() =>
            {
                Some(Arc::clone(session))
            }
            _ => None,
        }
    }

    pub(crate) fn release(&self, label: &str) {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        for slot in slots.iter_mut() {
            let matches = match slot {
                Some(Slot::Reserved {
                    label: reserved, ..
                }) => reserved == label,
                Some(Slot::Active(session)) => session.label == label,
                None => false,
            };
            if matches {
                if let Some(Slot::Active(session)) = slot.take() {
                    session.shutdown.cancel();
                } else {
                    *slot = None;
                }
                break;
            }
        }
    }

    pub(crate) fn labels_for_plugin(&self, plugin_id: &str) -> Vec<String> {
        self.slots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(|slot| match slot {
                Some(Slot::Active(session)) if session.plugin_id == plugin_id => {
                    Some(session.label.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn owned_by(&self, label: &str, window: &str) -> bool {
        self.sessions()
            .iter()
            .any(|session| session.label == label && session.owner_window == window)
    }

    pub(crate) fn labels_for_window(&self, window: &str) -> Vec<String> {
        self.sessions()
            .iter()
            .filter(|session| session.owner_window == window)
            .map(|session| session.label.clone())
            .collect()
    }

    fn sessions(&self) -> Vec<Arc<PanelSession>> {
        self.slots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(|slot| match slot {
                Some(Slot::Active(session)) => Some(Arc::clone(session)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn begin_window_close(&self, owner: &str) -> bool {
        if self.labels_for_window(owner).is_empty() {
            return false;
        }
        self.closing_windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(owner.to_owned())
    }

    pub(crate) fn end_window_close(&self, owner: &str) {
        self.closing_windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(owner);
    }

    pub(crate) fn window_close_pending(&self, owner: &str) -> bool {
        self.closing_windows
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(owner)
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PanelBounds {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl PanelBounds {
    fn validate<R: Runtime>(self, window: &Window<R>) -> Result<Rect, PanelError> {
        let size = window.inner_size().map_err(|_| PanelError::Unavailable)?;
        let scale = window.scale_factor().map_err(|_| PanelError::Unavailable)?;
        self.validate_for_size(size, scale)
    }

    fn validate_for_size(self, size: PhysicalSize<u32>, scale: f64) -> Result<Rect, PanelError> {
        if ![self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            || !scale.is_finite()
            || scale <= 0.0
            || self.x < 0.0
            || self.y < 0.0
            || self.width < 1.0
            || self.height < 1.0
            || self.x + self.width > f64::from(size.width) / scale + 1.0
            || self.y + self.height > f64::from(size.height) / scale + 1.0
        {
            return Err(PanelError::Invalid);
        }
        Ok(Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.width, self.height).into(),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SvgoSettings {
    precision: u8,
    path_precision: u8,
    remove_tspan: bool,
    remove_styling: bool,
    trim_text: bool,
    auto_autocrop: bool,
    custom_width: u32,
    custom_height: u32,
    use_custom_dimensions: bool,
    remove_default_values: bool,
    remove_font_family: bool,
    remove_font_size: bool,
    convert_sodipodi_arcs: bool,
    group_similar_elements: bool,
    group_text_elements_at_end: bool,
}

impl SvgoSettings {
    fn validate(&self) -> Result<(), PanelError> {
        if self.precision > 5
            || self.path_precision > 5
            || !(1..=100_000).contains(&self.custom_width)
            || !(1..=100_000).contains(&self.custom_height)
        {
            return Err(PanelError::Invalid);
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
enum BridgeRequest {
    SaveSvg {
        version: u8,
        svg: String,
        #[serde(rename = "loadToken")]
        load_token: String,
    },
    SettingsChange {
        version: u8,
        settings: SvgoSettings,
        sequence: u64,
        #[serde(default)]
        flush: bool,
        #[serde(rename = "loadToken")]
        load_token: String,
    },
    Heartbeat {
        version: u8,
        challenge: String,
        #[serde(rename = "loadToken")]
        load_token: String,
    },
}

impl BridgeRequest {
    fn credentials(&self) -> (u8, &str) {
        match self {
            Self::SaveSvg {
                version,
                load_token,
                ..
            }
            | Self::SettingsChange {
                version,
                load_token,
                ..
            }
            | Self::Heartbeat {
                version,
                load_token,
                ..
            } => (*version, load_token),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveResult {
    #[serde(rename = "type")]
    kind: &'static str,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<BridgeError>,
    load_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    sequence: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum BridgeError {
    Unavailable,
    PermissionDenied,
    InvalidRequest,
    TooLarge,
    Capacity,
    Timeout,
    SaveFailed,
}

impl From<PanelError> for BridgeError {
    fn from(value: PanelError) -> Self {
        match value {
            PanelError::Unavailable => Self::Unavailable,
            PanelError::Denied => Self::PermissionDenied,
            PanelError::Invalid => Self::InvalidRequest,
            PanelError::TooLarge => Self::TooLarge,
            PanelError::Capacity => Self::Capacity,
            PanelError::Timeout => Self::Timeout,
            PanelError::File(_) => Self::SaveFailed,
        }
    }
}

fn parse_bridge(bytes: &[u8], token: &str) -> Result<BridgeRequest, PanelError> {
    if bytes.len() > MAX_BRIDGE_BYTES {
        return Err(PanelError::TooLarge);
    }
    let message: BridgeRequest = serde_json::from_slice(bytes).map_err(|_| PanelError::Invalid)?;
    let (version, received_token) = message.credentials();
    if version != 1 || received_token != token {
        return Err(PanelError::Denied);
    }
    if let BridgeRequest::SettingsChange {
        settings, sequence, ..
    } = &message
    {
        settings.validate()?;
        if *sequence == 0 {
            return Err(PanelError::Invalid);
        }
    }
    if let BridgeRequest::Heartbeat { challenge, .. } = &message
        && (challenge.len() != 32 || !challenge.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(PanelError::Invalid);
    }
    Ok(message)
}

fn scheme(index: usize) -> String {
    format!("procyonspa{index}")
}

fn panel_origin(index: usize) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("https://{}.localhost", scheme(index))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("{}://localhost", scheme(index))
    }
}

fn allows_navigation(url: &Url, origin: &str) -> bool {
    Url::parse(origin).is_ok_and(|expected| {
        url.scheme() == expected.scheme()
            && url.host_str() == expected.host_str()
            && url.port() == expected.port()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn panel_request_url(uri: &tauri::http::Uri, origin: &str, slot: usize) -> Option<Url> {
    let url = Url::parse(&uri.to_string()).ok()?;
    #[cfg(target_os = "windows")]
    let url = if url.scheme() == scheme(slot)
        && url.host_str() == Some("localhost")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
    {
        Url::parse(&format!("{origin}{}", uri.path_and_query()?.as_str())).ok()?
    } else {
        url
    };
    #[cfg(not(target_os = "windows"))]
    let _ = slot;
    allows_navigation(&url, origin).then_some(url)
}

fn safe_asset_path(path: &str) -> Option<PathBuf> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains('%') || path.contains('\\')
    {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in Path::new(path.trim_start_matches('/')).components() {
        match component {
            Component::Normal(name) if !name.to_string_lossy().starts_with('.') => {
                relative.push(name);
            }
            _ => return None,
        }
    }
    let extension = relative.extension()?.to_str()?;
    if !matches!(
        extension,
        "html"
            | "js"
            | "mjs"
            | "css"
            | "svg"
            | "json"
            | "wasm"
            | "png"
            | "ico"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "woff"
            | "woff2"
            | "ttf"
            | "webmanifest"
    ) {
        return None;
    }
    Some(relative)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

fn response(
    status: StatusCode,
    body: Vec<u8>,
    mime: &'static str,
    origin: &str,
) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Content-Security-Policy", panel_csp(origin))
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .body(body)
        .expect("static response headers are valid")
}

fn error_response(error: PanelError, origin: &str) -> Response<Vec<u8>> {
    let status = match error {
        PanelError::Invalid => StatusCode::BAD_REQUEST,
        PanelError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        PanelError::Capacity => StatusCode::TOO_MANY_REQUESTS,
        PanelError::Denied => StatusCode::FORBIDDEN,
        PanelError::Unavailable | PanelError::Timeout | PanelError::File(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
    };
    response(status, Vec::new(), "text/plain; charset=utf-8", origin)
}

fn checked_panel(
    service: &FileManagerService,
    session: &PanelSession,
) -> Result<fm_application::PluginPanel, PanelError> {
    require_local_svgo(&session.plugin_id, &session.location)?;
    let panel = service
        .plugin_panel(&session.plugin_id, &session.action_id, &session.location)
        .map_err(|_| PanelError::Unavailable)?;
    if !panel.can_read_selected
        || panel.plugin_id != session.plugin_id
        || panel.directory != session.directory
        || panel.entrypoint != session.entrypoint
    {
        return Err(PanelError::Unavailable);
    }
    Ok(panel)
}

fn read_package_asset(directory: &Path, path: &str) -> Result<(Vec<u8>, PathBuf), PanelError> {
    let path = safe_asset_path(path).ok_or(PanelError::Invalid)?;
    let root = directory
        .canonicalize()
        .map_err(|_| PanelError::Unavailable)?;
    let asset = root
        .join(&path)
        .canonicalize()
        .map_err(|_| PanelError::Unavailable)?;
    if !asset.starts_with(&root) || !asset.is_file() {
        return Err(PanelError::Denied);
    }
    let file = std::fs::File::open(&asset).map_err(|_| PanelError::Unavailable)?;
    if file.metadata().map_err(|_| PanelError::Unavailable)?.len() > MAX_ASSET_BYTES {
        return Err(PanelError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PanelError::Unavailable)?;
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err(PanelError::TooLarge);
    }
    Ok((bytes, asset))
}

fn serve_asset(
    service: &FileManagerService,
    session: &PanelSession,
    path: &str,
    origin: &str,
) -> Result<Response<Vec<u8>>, PanelError> {
    let panel = checked_panel(service, session)?;
    let (bytes, asset) = read_package_asset(&panel.directory, path)?;
    Ok(response(
        StatusCode::OK,
        bytes,
        content_type(&asset),
        origin,
    ))
}

async fn save_svg(
    service: Arc<FileManagerService>,
    session: Arc<PanelSession>,
    svg: String,
) -> Result<(), PanelError> {
    require_local_svgo(&session.plugin_id, &session.location)?;
    let panel = checked_panel(&service, &session)?;
    if !panel.can_write_selected {
        return Err(PanelError::Denied);
    }
    let _save_guard = session.save_lock.lock().await;
    let mut revision = session.revision.lock().await;
    let future = service.save_editable_file(SaveEditableFileRequestDto {
        location: session.location.clone(),
        destination: None,
        content: svg,
        expected_revision: revision.clone(),
        overwrite_conflict: false,
    });
    let result = tokio::select! {
        () = session.shutdown.cancelled() => return Err(PanelError::Unavailable),
        result = tokio::time::timeout(BRIDGE_TIMEOUT, future) => result.map_err(|_| PanelError::Timeout)?,
    }
    .map_err(|error| PanelError::File(error.to_string()))?;
    if session.shutdown.is_cancelled() {
        return Err(PanelError::Unavailable);
    }
    *revision = result.revision;
    Ok(())
}

fn save_settings(
    service: &FileManagerService,
    session: &PanelSession,
    settings: SvgoSettings,
    sequence: u64,
) -> Result<(), PanelError> {
    let panel = checked_panel(service, session)?;
    if !panel.can_store_settings || session.plugin_id != SVGO_PLUGIN_ID {
        return Err(PanelError::Denied);
    }
    if session.shutdown.is_cancelled() {
        return Err(PanelError::Unavailable);
    }
    let mut accepted = session
        .settings_sequence
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if sequence <= *accepted {
        return Ok(());
    }
    service
        .save_plugin_panel_settings(
            &session.plugin_id,
            serde_json::to_value(settings).expect("validated settings are serializable"),
        )
        .map_err(|error| {
            tracing::warn!(%error, "could not persist plugin panel settings");
            PanelError::Unavailable
        })?;
    *accepted = sequence;
    Ok(())
}

async fn bridge(
    service: Arc<FileManagerService>,
    session: Arc<PanelSession>,
    body: Vec<u8>,
    origin: String,
) -> Response<Vec<u8>> {
    let request = match parse_bridge(&body, &session.token) {
        Ok(request) => request,
        Err(error) => return bridge_result(Err(error), "save-result", "", None, &origin),
    };
    let (kind, sequence, result) = match request {
        BridgeRequest::SaveSvg { svg, .. } => {
            #[cfg(feature = "native-spa-smoke")]
            crate::native_spa_smoke::stage("bridge-save-received");
            let result = save_svg(service, Arc::clone(&session), svg).await;
            #[cfg(feature = "native-spa-smoke")]
            crate::native_spa_smoke::stage(if result.is_ok() {
                "bridge-save-succeeded"
            } else {
                "bridge-save-failed"
            });
            ("save-result", None, result)
        }
        BridgeRequest::SettingsChange {
            settings,
            sequence,
            flush,
            ..
        } => {
            let result = save_settings(&service, &session, settings, sequence);
            if flush
                && let Some(sender) = session
                    .flush_sender
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
            {
                let _ = sender.send(result.is_ok());
            }
            ("settings-result", Some(sequence), result)
        }
        BridgeRequest::Heartbeat { challenge, .. } => (
            "heartbeat-result",
            None,
            session.acknowledge_heartbeat(&challenge),
        ),
    };
    bridge_result(result, kind, &session.token, sequence, &origin)
}

fn bridge_result(
    result: Result<(), PanelError>,
    kind: &'static str,
    token: &str,
    sequence: Option<u64>,
    origin: &str,
) -> Response<Vec<u8>> {
    let status = match &result {
        Ok(()) => StatusCode::OK,
        Err(PanelError::Invalid) => StatusCode::BAD_REQUEST,
        Err(PanelError::TooLarge) => StatusCode::PAYLOAD_TOO_LARGE,
        Err(PanelError::Capacity) => StatusCode::TOO_MANY_REQUESTS,
        Err(PanelError::Denied) => StatusCode::FORBIDDEN,
        Err(PanelError::File(_)) => StatusCode::CONFLICT,
        Err(PanelError::Timeout) => StatusCode::GATEWAY_TIMEOUT,
        Err(PanelError::Unavailable) => StatusCode::SERVICE_UNAVAILABLE,
    };
    let response_body = SaveResult {
        kind,
        success: result.is_ok(),
        error: result.err().map(BridgeError::from),
        load_token: token.to_owned(),
        sequence,
    };
    response(
        status,
        serde_json::to_vec(&response_body).expect("typed bridge result is serializable"),
        "application/json",
        origin,
    )
}

fn bootstrap(token: &str, theme: PanelTheme, settings: Option<&SvgoSettings>) -> String {
    let token = serde_json::to_string(token).expect("hex token is serializable");
    let theme = serde_json::to_string(&theme).expect("panel theme is serializable");
    let settings = serde_json::to_string(&settings).expect("panel settings are serializable");
    format!(
        r#"(() => {{
          const loadToken = {token};
          Object.defineProperty(window, 'procyonPlugin', {{
            value: Object.freeze({{
              loadToken,
              theme: {theme},
              settings: {settings},
              postMessage: async (message) => {{
                let result;
                try {{
                  const response = await fetch('/bridge', {{
                    method: 'POST',
                    headers: {{ 'Content-Type': 'application/json' }},
                    body: JSON.stringify({{ ...message, version: 1, loadToken }})
                  }});
                  result = await response.json();
                }} catch (_) {{
                  result = {{ type: 'save-result', success: false, error: 'bridge unavailable', loadToken }};
                }}
                window.postMessage(result, '*');
              }}
            }}),
            writable: false, configurable: false
          }});
        }})();"#
    )
}

fn deliver_load(svg: &str, uri: &str, token: &str) -> String {
    let message = serde_json::json!({
        "type": "load-svg",
        "svg": svg,
        "uri": uri,
        "loadToken": token,
    });
    format!("window.postMessage({}, '*');", message)
}

fn first_panel_load(url: &Url, expected: &Url, loaded: &AtomicBool) -> bool {
    url == expected && !loaded.swap(true, Ordering::SeqCst)
}

pub(crate) fn register_schemes<R: Runtime>(
    mut builder: tauri::Builder<R>,
    registry: Arc<PanelRegistry>,
) -> tauri::Builder<R> {
    for index in 0..PANEL_SLOTS {
        let registry = Arc::clone(&registry);
        builder = builder.register_asynchronous_uri_scheme_protocol(
            scheme(index),
            move |ctx, request, responder| {
                let label = ctx.webview_label().to_owned();
                let app = ctx.app_handle().clone();
                let origin = panel_origin(index);
                let Some(session) = registry.session_for(index, &label) else {
                    responder.respond(error_response(PanelError::Denied, &origin));
                    return;
                };
                if request
                    .headers()
                    .get("Origin")
                    .is_some_and(|value| value.to_str().ok() != Some(origin.as_str()))
                {
                    responder.respond(error_response(PanelError::Denied, &origin));
                    return;
                }
                #[cfg(feature = "native-spa-smoke")]
                if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some()
                    && request.method() == Method::GET
                    && request.uri().path() == "/smoke"
                {
                    let valid = (request.uri().to_string().len() <= 1024)
                        .then(|| panel_request_url(request.uri(), &origin, index))
                        .flatten()
                        .and_then(|url| {
                            let mut pairs = url.query_pairs();
                            let token = pairs.next()?;
                            let stage = pairs.next()?;
                            let detail = pairs.next();
                            if pairs.next().is_some()
                                || token.0 != "token"
                                || token.1 != session.token
                                || stage.0 != "stage"
                                || detail.as_ref().is_some_and(|(key, value)| {
                                    key != "error"
                                        || value.len() > 256
                                        || stage.1 != "script-failed"
                                })
                                || !matches!(
                                    stage.1.as_ref(),
                                    "plugin-ui-ready"
                                        | "acl-denied"
                                        | "save-requested"
                                        | "script-failed"
                                )
                            {
                                return None;
                            }
                            Some(match detail {
                                Some((_, detail)) => {
                                    format!("script-failed: {}", detail.replace(['\r', '\n'], " "))
                                }
                                None => stage.1.into_owned(),
                            })
                        });
                    responder.respond(if let Some(stage) = valid {
                        crate::native_spa_smoke::stage(&stage);
                        response(StatusCode::OK, Vec::new(), "text/plain", &origin)
                    } else {
                        error_response(PanelError::Denied, &origin)
                    });
                    return;
                }
                let Some(url) = panel_request_url(request.uri(), &origin, index)
                    .filter(|url| url.query().is_none() && url.fragment().is_none())
                else {
                    responder.respond(error_response(PanelError::Denied, &origin));
                    return;
                };
                let service = Arc::clone(&app.state::<AppState>().service);
                if request.method() == Method::POST && url.path() == "/bridge" {
                    if request.body().len() > MAX_BRIDGE_BYTES {
                        responder.respond(bridge_result(
                            Err(PanelError::TooLarge),
                            "save-result",
                            "",
                            None,
                            &origin,
                        ));
                        return;
                    }
                    let body = request.into_body();
                    tauri::async_runtime::spawn(async move {
                        responder.respond(bridge(service, session, body, origin).await);
                    });
                } else if request.method() == Method::GET {
                    let result = serve_asset(&service, &session, url.path(), &origin)
                        .unwrap_or_else(|error| error_response(error, &origin));
                    responder.respond(result);
                } else {
                    responder.respond(error_response(PanelError::Denied, &origin));
                }
            },
        );
    }
    builder
}

/// A trusted app window places a bounded editable selection in a private,
/// command-less child WebView. The plugin cannot choose its own origin or file.
#[tauri::command]
pub(crate) async fn open_plugin_panel<R: Runtime>(
    app: AppHandle<R>,
    source: Window<R>,
    state: State<'_, AppState>,
    request: OpenPanelRequest,
) -> Result<String, PanelError> {
    if !trusted_invoke_label(source.label()) {
        return Err(PanelError::Denied);
    }
    let OpenPanelRequest {
        plugin_id,
        action_id,
        location,
        bounds,
        theme,
    } = request;
    require_local_svgo(&plugin_id, &location)?;
    let panel = state
        .service
        .plugin_panel(&plugin_id, &action_id, &location)
        .map_err(|_| PanelError::Unavailable)?;
    let bounds = bounds.validate(&source)?;
    if !panel.can_read_selected {
        return Err(PanelError::Denied);
    }
    let settings = if panel.can_store_settings && plugin_id == SVGO_PLUGIN_ID {
        state
            .service
            .plugin_panel_settings(&plugin_id)
            .map(|value| {
                let settings: SvgoSettings =
                    serde_json::from_value(value).map_err(|_| PanelError::Invalid)?;
                settings.validate()?;
                Ok(settings)
            })
            .transpose()?
    } else {
        None
    };
    let loaded = state
        .service
        .load_editable_file(LoadEditableFileRequestDto {
            location: location.clone(),
        })
        .await
        .map_err(|error| PanelError::File(error.to_string()))?;
    if loaded.content.len() + 256 > MAX_BRIDGE_BYTES {
        return Err(PanelError::TooLarge);
    }
    let entrypoint = panel
        .entrypoint
        .components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str().ok_or(PanelError::Invalid),
            _ => Err(PanelError::Invalid),
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    if safe_asset_path(&format!("/{entrypoint}")).is_none() {
        return Err(PanelError::Invalid);
    }
    let slot_url = |slot| {
        let origin = panel_origin(slot);
        Url::parse(&format!("{origin}/{entrypoint}"))
            .map(|url| (origin, url))
            .map_err(|_| PanelError::Invalid)
    };
    let label = format!("plugin-spa-{}", Uuid::new_v4().simple());
    let registry = app.state::<Arc<PanelRegistry>>();
    let (slot, save_lock) = registry.reserve(label.clone(), &location)?;
    let (origin, url) = slot_url(slot).inspect_err(|_| registry.release(&label))?;
    let token = Uuid::new_v4().simple().to_string();
    #[cfg(target_os = "linux")]
    let context_directory = tempfile::tempdir()
        .map_err(|_| PanelError::Unavailable)
        .inspect_err(|_| registry.release(&label))?;
    #[cfg(target_os = "linux")]
    let context_path = context_directory.path().to_path_buf();
    let session = Arc::new(PanelSession {
        label: label.clone(),
        owner_window: source.label().to_owned(),
        plugin_id,
        action_id,
        directory: panel.directory,
        entrypoint: panel.entrypoint,
        location: location.clone(),
        token: token.clone(),
        revision: AsyncMutex::new(loaded.revision),
        save_lock,
        close_lock: AsyncMutex::new(()),
        settings_sequence: Mutex::new(0),
        flush_sender: Mutex::new(None),
        heartbeat: Mutex::new(None),
        heartbeat_responded: AtomicBool::new(false),
        created: Instant::now(),
        loaded: AtomicBool::new(false),
        visible: AtomicBool::new(true),
        shutdown: CancellationToken::new(),
        #[cfg(target_os = "linux")]
        _context_directory: context_directory,
    });
    registry
        .activate(slot, Arc::clone(&session))
        .inspect_err(|_| registry.release(&label))?;
    let load_script = deliver_load(&loaded.content, &location.uri, &token);
    let loaded_once = AtomicBool::new(false);
    let expected_url = url.clone();
    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url));
    #[cfg(target_os = "linux")]
    let builder = builder.data_directory(context_path);
    // Linux must give each incognito child a distinct Tauri context key: Wry replaces
    // the context with an ephemeral one, which otherwise misses the shared schemes.
    let builder = builder
        .incognito(true)
        .use_https_scheme(cfg!(target_os = "windows"))
        .initialization_script(bootstrap(&token, theme, settings.as_ref()))
        .on_navigation({
            let origin = origin.clone();
            move |url| allows_navigation(url, &origin)
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished || payload.url().as_str() == "about:blank"
            {
                return;
            }
            if !first_panel_load(payload.url(), &expected_url, &loaded_once) {
                tracing::warn!("plugin panel navigation requires reopening the selected file");
                window
                    .app_handle()
                    .state::<Arc<PanelRegistry>>()
                    .release(window.label());
                if let Err(error) = window.close() {
                    tracing::warn!(%error, "could not close reloaded plugin panel");
                }
            } else {
                #[cfg(feature = "native-spa-smoke")]
                crate::native_spa_smoke::stage("child-page-loaded");
                if let Err(error) = window.eval(&load_script) {
                    #[cfg(feature = "native-spa-smoke")]
                    crate::native_spa_smoke::stage(&format!("child-load-failed: {error}"));
                    tracing::warn!(%error, "could not deliver selected SVG to plugin panel");
                    window
                        .app_handle()
                        .state::<Arc<PanelRegistry>>()
                        .release(window.label());
                    if let Err(error) = window.close() {
                        tracing::warn!(%error, "could not close failed plugin panel");
                    }
                } else {
                    session.loaded.store(true, Ordering::SeqCst);
                    #[cfg(feature = "native-spa-smoke")]
                    if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some()
                        && let Err(error) = window.eval(
                            include_str!("native_spa_smoke.js")
                                .replace("__PROCYON_SMOKE_TOKEN__", &token),
                        )
                    {
                        crate::native_spa_smoke::stage(&format!(
                            "script-injection-failed: {error}"
                        ));
                    }
                }
            }
        });
    let webview = source.add_child(builder, bounds.position, bounds.size);
    if webview.is_err() {
        registry.release(&label);
    }
    webview.map_err(|_| PanelError::Unavailable)?;
    Ok(label)
}

#[tauri::command]
pub(crate) fn update_plugin_panel_bounds<R: Runtime>(
    source: Window<R>,
    registry: State<'_, Arc<PanelRegistry>>,
    label: String,
    bounds: PanelBounds,
) -> Result<(), PanelError> {
    if !trusted_invoke_label(source.label()) || !registry.owned_by(&label, source.label()) {
        return Err(PanelError::Denied);
    }
    let bounds = bounds.validate(&source)?;
    source
        .get_webview(&label)
        .ok_or(PanelError::Unavailable)?
        .set_bounds(bounds)
        .map_err(|_| PanelError::Unavailable)
}

#[tauri::command]
pub(crate) fn set_plugin_panel_visible<R: Runtime>(
    source: Window<R>,
    registry: State<'_, Arc<PanelRegistry>>,
    label: String,
    visible: bool,
) -> Result<(), PanelError> {
    if !trusted_invoke_label(source.label()) || !registry.owned_by(&label, source.label()) {
        return Err(PanelError::Denied);
    }
    let webview = source.get_webview(&label).ok_or(PanelError::Unavailable)?;
    if visible {
        webview.show()
    } else {
        webview.hide()
    }
    .map_err(|_| PanelError::Unavailable)?;
    if let Some(session) = registry
        .sessions()
        .into_iter()
        .find(|session| session.label == label)
    {
        session.visible.store(visible, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn set_plugin_panel_theme<R: Runtime>(
    source: Window<R>,
    registry: State<'_, Arc<PanelRegistry>>,
    label: String,
    theme: PanelTheme,
) -> Result<(), PanelError> {
    if !trusted_invoke_label(source.label()) || !registry.owned_by(&label, source.label()) {
        return Err(PanelError::Denied);
    }
    let session = registry
        .sessions()
        .into_iter()
        .find(|session| session.label == label)
        .ok_or(PanelError::Unavailable)?;
    let message = serde_json::json!({
        "type": "theme-change",
        "theme": theme,
        "loadToken": &session.token,
    });
    source
        .get_webview(&label)
        .ok_or(PanelError::Unavailable)?
        .eval(format!(
            "window.postMessage({}, window.location.origin);",
            message
        ))
        .map_err(|_| PanelError::Unavailable)
}

async fn flush_panel_settings<R: Runtime>(webview: &Webview<R>, session: &PanelSession) -> bool {
    if session.plugin_id != SVGO_PLUGIN_ID || !session.loaded.load(Ordering::SeqCst) {
        return true;
    }
    let (sender, receiver) = oneshot::channel();
    {
        let mut pending = session
            .flush_sender
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.is_some() {
            return false;
        }
        *pending = Some(sender);
    }
    let event = serde_json::json!({
        "type": "flush-settings",
        "loadToken": &session.token,
    });
    let requested = webview
        .eval(format!(
            "window.postMessage({}, window.location.origin);",
            event
        ))
        .is_ok();
    let result = if requested {
        matches!(
            tokio::time::timeout(Duration::from_secs(2), receiver).await,
            Ok(Ok(true))
        )
    } else {
        false
    };
    session
        .flush_sender
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if !result {
        tracing::warn!(label = %session.label, "could not flush SVGO settings before closing plugin panel");
    }
    result
}

async fn close_panel<R: Runtime>(app: &AppHandle<R>, label: &str) -> Result<(), PanelError> {
    let registry = app.state::<Arc<PanelRegistry>>();
    let session = registry
        .sessions()
        .into_iter()
        .find(|session| session.label == label)
        .ok_or(PanelError::Unavailable)?;
    let _closing = session.close_lock.lock().await;
    if session.shutdown.is_cancelled() {
        return Err(PanelError::Unavailable);
    }
    let webview = match app.get_webview(label) {
        Some(webview) => webview,
        None => {
            registry.release(label);
            return Err(PanelError::Unavailable);
        }
    };
    if let Err(error) = webview.hide() {
        tracing::warn!(%error, "could not hide closing plugin panel");
    }
    let flush_result = flush_panel_settings(&webview, &session).await;
    // A cancelled flush must never retain the slot or a pending Save.
    registry.release(label);
    webview.close().map_err(|_| PanelError::Unavailable)?;
    if flush_result {
        Ok(())
    } else {
        Err(PanelError::Unavailable)
    }
}

#[tauri::command]
pub(crate) async fn close_plugin_panel<R: Runtime>(
    source: Window<R>,
    registry: State<'_, Arc<PanelRegistry>>,
    label: String,
) -> Result<(), PanelError> {
    if !trusted_invoke_label(source.label()) || !registry.owned_by(&label, source.label()) {
        return Err(PanelError::Denied);
    }
    close_panel(source.app_handle(), &label).await
}

pub(crate) async fn flush_plugin_panels<R: Runtime>(app: &AppHandle<R>, plugin_id: &str) {
    let registry = app.state::<Arc<PanelRegistry>>();
    for label in registry.labels_for_plugin(plugin_id) {
        if let Some(webview) = app.get_webview(&label)
            && let Some(session) = registry.sessions().into_iter().find(|s| s.label == label)
        {
            let _closing = session.close_lock.lock().await;
            if !session.shutdown.is_cancelled() {
                flush_panel_settings(&webview, &session).await;
            }
        }
    }
}

pub(crate) fn close_plugin_panels<R: Runtime>(app: &AppHandle<R>, plugin_id: &str) {
    let registry = app.state::<Arc<PanelRegistry>>();
    for label in registry.labels_for_plugin(plugin_id) {
        registry.release(&label);
        if let Some(webview) = app.get_webview(&label)
            && let Err(error) = webview.close()
        {
            tracing::warn!(%error, %label, "could not close disabled plugin panel");
        }
    }
}

pub(crate) async fn close_panels_for_window<R: Runtime>(app: &AppHandle<R>, labels: Vec<String>) {
    for label in labels {
        if let Err(error) = close_panel(app, &label).await {
            tracing::warn!(%error, %label, "could not cleanly close plugin panel for host window");
        }
    }
}

pub(crate) fn reconcile_panels<R: Runtime>(app: &AppHandle<R>) {
    let registry = app.state::<Arc<PanelRegistry>>();
    let service = &app.state::<AppState>().service;
    for session in registry.sessions() {
        if session
            .flush_sender
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
        {
            continue;
        }
        let invalid = checked_panel(service, &session).is_err();
        let challenge = if invalid {
            Ok(None)
        } else {
            session.next_heartbeat(Instant::now())
        };
        let responsive = match challenge {
            Ok(Some(challenge)) => app.get_webview(&session.label).is_some_and(|webview| {
                let event = serde_json::json!({
                    "type": "heartbeat",
                    "challenge": challenge,
                });
                webview
                    .eval(format!("window.procyonPlugin.postMessage({});", event))
                    .is_ok()
            }),
            Ok(None) => app.get_webview(&session.label).is_some(),
            Err(()) => false,
        };
        if invalid || !responsive {
            tracing::warn!(label = %session.label, "plugin panel unavailable or renderer heartbeat timed out");
            registry.release(&session.label);
            if let Some(webview) = app.get_webview(&session.label)
                && let Err(error) = webview.close()
            {
                tracing::warn!(%error, "could not close unresponsive plugin panel");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svgo_service(root: &Path) -> Arc<FileManagerService> {
        let service = Arc::new(FileManagerService::new(
            fm_transport_dto::RuntimeKindDto::Tauri,
            root,
            root.join("settings"),
        ));
        service
            .set_plugin_enabled(SVGO_PLUGIN_ID.to_owned(), true)
            .unwrap();
        service
    }

    fn svgo_session(
        service: &FileManagerService,
        location: LocationDto,
        revision: String,
    ) -> Arc<PanelSession> {
        let panel = service
            .plugin_panel(SVGO_PLUGIN_ID, "procyon.svgo.open", &location)
            .unwrap();
        Arc::new(PanelSession {
            label: "plugin-spa-test".into(),
            owner_window: "main".into(),
            plugin_id: SVGO_PLUGIN_ID.into(),
            action_id: "procyon.svgo.open".into(),
            directory: panel.directory,
            entrypoint: panel.entrypoint,
            location,
            token: "test-token".into(),
            revision: AsyncMutex::new(revision),
            save_lock: Arc::new(AsyncMutex::new(())),
            close_lock: AsyncMutex::new(()),
            settings_sequence: Mutex::new(0),
            flush_sender: Mutex::new(None),
            heartbeat: Mutex::new(None),
            heartbeat_responded: AtomicBool::new(false),
            created: Instant::now(),
            loaded: AtomicBool::new(true),
            visible: AtomicBool::new(true),
            shutdown: CancellationToken::new(),
            #[cfg(target_os = "linux")]
            _context_directory: tempfile::tempdir().unwrap(),
        })
    }

    #[tokio::test]
    async fn svgo_denies_remote_open_and_forged_bridge_save_but_local_save_works() {
        let root = tempfile::tempdir().unwrap();
        let service = svgo_service(root.path());
        let path = root.path().join("drawing.svg");
        std::fs::write(&path, "<svg/>").unwrap();
        let location: LocationDto = Location::from_native_path(&path).unwrap().into();
        for (provider_id, uri) in [
            ("sftp", "sftp://host/drawing.svg"),
            ("ftp", "ftp://host/drawing.svg"),
            ("webdav", "webdav://host/drawing.svg"),
        ] {
            assert!(matches!(
                require_local_svgo(
                    SVGO_PLUGIN_ID,
                    &LocationDto {
                        provider_id: provider_id.into(),
                        uri: uri.into(),
                    }
                ),
                Err(PanelError::Denied)
            ));
        }
        let remote = LocationDto {
            provider_id: "sftp".into(),
            uri: "sftp://host/drawing.svg".into(),
        };
        assert!(matches!(
            require_local_svgo(SVGO_PLUGIN_ID, &remote),
            Err(PanelError::Denied)
        ));
        let mut forged = svgo_session(&service, location.clone(), "revision".into());
        Arc::get_mut(&mut forged).unwrap().location = remote;
        let result = bridge(
            Arc::clone(&service),
            forged,
            br#"{"version":1,"type":"save-svg","svg":"<svg/>","loadToken":"test-token"}"#.to_vec(),
            panel_origin(0),
        )
        .await;
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
        let mismatched = LocationDto {
            provider_id: "local".into(),
            uri: "sftp://host/drawing.svg".into(),
        };
        assert!(matches!(
            require_local_svgo(SVGO_PLUGIN_ID, &mismatched),
            Err(PanelError::Denied)
        ));

        assert!(require_local_svgo(SVGO_PLUGIN_ID, &location).is_ok());
        let loaded = service
            .load_editable_file(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .unwrap();
        let session = svgo_session(&service, location, loaded.revision);
        let result = bridge(
            service,
            session,
            br#"{"version":1,"type":"save-svg","svg":"<svg id=\"saved\"/>","loadToken":"test-token"}"#.to_vec(),
            panel_origin(0),
        ).await;
        assert_eq!(result.status(), StatusCode::OK);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "<svg id=\"saved\"/>"
        );
    }

    #[test]
    fn svgo_cross_process_worker() {
        let Ok(root) = std::env::var("PROCYON_SVGO_LOCK_TEST_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let service = svgo_service(&root);
        let target = std::env::var("PROCYON_SVGO_LOCK_TEST_TARGET").unwrap();
        let location: LocationDto = Location::from_native_path(Path::new(&target))
            .unwrap()
            .into();
        let loaded = tauri::async_runtime::block_on(service.load_editable_file(
            LoadEditableFileRequestDto {
                location: location.clone(),
            },
        ))
        .unwrap();
        std::fs::write(root.join("worker-ready"), b"ready").unwrap();
        let session = svgo_session(&service, location, loaded.revision.clone());
        if std::env::var_os("PROCYON_SVGO_LOCK_TEST_GENERIC").is_some() {
            let result = tauri::async_runtime::block_on(service.save_editable_file(
                SaveEditableFileRequestDto {
                    location: session.location.clone(),
                    destination: None,
                    content: "<svg id=\"worker\"/>".into(),
                    expected_revision: loaded.revision,
                    overwrite_conflict: false,
                },
            ));
            if std::env::var_os("PROCYON_SVGO_LOCK_TEST_RACE").is_some() {
                let outcome = match result {
                    Ok(_) => "saved",
                    Err(fm_application::ApplicationError::FileRevisionConflict { .. }) => {
                        "conflict"
                    }
                    Err(error) => panic!("unexpected generic editor error: {error}"),
                };
                std::fs::write(root.join("worker-outcome"), outcome).unwrap();
                return;
            }
            assert!(
                matches!(
                    result,
                    Err(fm_application::ApplicationError::FileRevisionConflict { .. })
                ),
                "{result:?}"
            );
        } else {
            let result = tauri::async_runtime::block_on(save_svg(
                service,
                session,
                "<svg id=\"worker\"/>".into(),
            ));
            assert!(matches!(result, Err(PanelError::File(_))), "{result:?}");
        }
    }

    #[tokio::test]
    async fn svgo_cross_process_alias_saves_serialize_and_conflict() {
        cross_process_alias_save(false).await;
    }

    #[tokio::test]
    async fn generic_editor_and_svgo_alias_saves_serialize_and_conflict() {
        cross_process_alias_save(true).await;
    }

    async fn wait_for_worker(worker: &mut std::process::Child) {
        let finished = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            loop {
                if let Some(status) = worker.try_wait().unwrap() {
                    return status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await;
        match finished {
            Ok(status) => assert!(status.success(), "worker failed: {status}"),
            Err(_) => {
                worker.kill().unwrap();
                worker.wait().unwrap();
                panic!("worker did not finish after the lock was released");
            }
        }
    }

    #[tokio::test]
    async fn generic_editor_and_svgo_processes_compete_without_losing_an_edit() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let path = real.join("drawing.svg");
        std::fs::write(&path, "<svg/>").unwrap();
        #[cfg(unix)]
        let alias = {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&real, &alias).unwrap();
            alias.join("drawing.svg")
        };
        #[cfg(not(unix))]
        let alias = path.clone();
        let service = svgo_service(root.path());
        let location: LocationDto = Location::from_native_path(&path).unwrap().into();
        let loaded = service
            .load_editable_file(LoadEditableFileRequestDto {
                location: location.clone(),
            })
            .await
            .unwrap();
        let session = svgo_session(&service, location, loaded.revision);
        let mut worker = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("plugin_spa::tests::svgo_cross_process_worker")
            .arg("--nocapture")
            .env("PROCYON_SVGO_LOCK_TEST_ROOT", root.path())
            .env("PROCYON_SVGO_LOCK_TEST_TARGET", &alias)
            .env("PROCYON_SVGO_LOCK_TEST_GENERIC", "1")
            .env("PROCYON_SVGO_LOCK_TEST_RACE", "1")
            .spawn()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while !root.path().join("worker-ready").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let parent = save_svg(service, session, "<svg id=\"parent\"/>".into()).await;
        wait_for_worker(&mut worker).await;
        let child = std::fs::read_to_string(root.path().join("worker-outcome")).unwrap();
        match (parent, child.as_str()) {
            (Ok(()), "conflict") => {
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    "<svg id=\"parent\"/>"
                );
            }
            (Err(PanelError::File(_)), "saved") => {
                assert_eq!(
                    std::fs::read_to_string(&path).unwrap(),
                    "<svg id=\"worker\"/>"
                );
            }
            (parent, child) => panic!("exactly one save should succeed: {parent:?}, {child}"),
        }
    }

    async fn cross_process_alias_save(generic_worker: bool) {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let path = real.join("drawing.svg");
        std::fs::write(&path, "<svg/>").unwrap();
        #[cfg(unix)]
        let alias = {
            let alias = root.path().join("alias");
            std::os::unix::fs::symlink(&real, &alias).unwrap();
            alias.join("drawing.svg")
        };
        #[cfg(not(unix))]
        let alias = path.clone();
        let location: LocationDto = Location::from_native_path(&path).unwrap().into();
        let aliased: LocationDto = Location::from_native_path(&alias).unwrap().into();
        let held = svgo_file_lock(&location).await.unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut command = std::process::Command::new(executable);
        command
            .arg("--exact")
            .arg("plugin_spa::tests::svgo_cross_process_worker")
            .arg("--nocapture")
            .env("PROCYON_SVGO_LOCK_TEST_ROOT", root.path())
            .env("PROCYON_SVGO_LOCK_TEST_TARGET", &alias);
        if generic_worker {
            command.env("PROCYON_SVGO_LOCK_TEST_GENERIC", "1");
        }
        let mut worker = command.spawn().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while !root.path().join("worker-ready").exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert!(
            worker.try_wait().unwrap().is_none(),
            "worker must wait on the file lock"
        );
        // The child loaded the old revision, but cannot recheck or commit until this lock
        // is released. Change the fixture while the lock is held to force a conflict.
        std::fs::write(&path, "<svg id=\"parent\"/>").unwrap();
        drop(held);
        wait_for_worker(&mut worker).await;
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "<svg id=\"parent\"/>"
        );
        assert_eq!(
            std::fs::read_to_string(&alias).unwrap(),
            "<svg id=\"parent\"/>"
        );
        assert!(!real.read_dir().unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("lock")
        }));
        assert_eq!(
            svgo_file_lock(&aliased)
                .await
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn child_bounds_must_fit_the_owning_window() {
        let size = PhysicalSize::new(800, 600);
        let valid = PanelBounds {
            x: 250.0,
            y: 50.0,
            width: 550.0,
            height: 500.0,
        };
        assert!(valid.validate_for_size(size, 1.0).is_ok());
        assert!(valid.validate_for_size(size, 2.0).is_err());
        assert!(
            PanelBounds {
                x: f64::NAN,
                ..valid
            }
            .validate_for_size(size, 1.0)
            .is_err()
        );
        assert!(
            PanelBounds { x: -1.0, ..valid }
                .validate_for_size(size, 1.0)
                .is_err()
        );
    }

    #[test]
    fn csp_denies_ambient_browser_authority() {
        let origin = panel_origin(0);
        let csp = panel_csp(&origin);
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains(&format!("connect-src {origin}/bridge;")));
        assert!(!csp.contains("connect-src 'self'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("style-src 'self'"));
        assert!(csp.contains("img-src 'self' data: blob:"));
        assert!(csp.contains("worker-src 'self' blob:"));
        assert!(csp.contains("font-src 'self' data:"));
        assert!(csp.contains("manifest-src 'self'"));
        assert!(csp.contains("frame-src 'none'"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("form-action 'none'"));
        assert!(csp.contains("navigate-to 'self'"));
        assert!(!csp.contains("'unsafe-inline'"));
        assert!(!csp.contains("example.com"));
        let html = response(StatusCode::OK, b"<html/>".to_vec(), "text/html", &origin);
        assert_eq!(html.headers()["Content-Security-Policy"], csp);
    }

    #[test]
    fn panel_request_urls_accept_only_the_allocated_origin() {
        let origin = panel_origin(0);
        let expected: tauri::http::Uri = format!("{origin}/dist/index.html").parse().unwrap();
        assert_eq!(
            panel_request_url(&expected, &origin, 0).unwrap().as_str(),
            expected.to_string()
        );
        let other: tauri::http::Uri = "https://example.com/dist/index.html".parse().unwrap();
        assert!(panel_request_url(&other, &origin, 0).is_none());
        #[cfg(target_os = "windows")]
        {
            let synthetic: tauri::http::Uri =
                "procyonspa0://localhost/dist/index.html".parse().unwrap();
            assert_eq!(
                panel_request_url(&synthetic, &origin, 0).unwrap().as_str(),
                expected.to_string()
            );
            let smoke: tauri::http::Uri =
                "procyonspa0://localhost/smoke?token=abc&stage=plugin-ui-ready"
                    .parse()
                    .unwrap();
            assert_eq!(
                panel_request_url(&smoke, &origin, 0).unwrap().as_str(),
                format!("{origin}/smoke?token=abc&stage=plugin-ui-ready")
            );
            assert!(panel_request_url(&synthetic, &panel_origin(1), 1).is_none());
            let foreign: tauri::http::Uri =
                "procyonspa0://example.com/dist/index.html".parse().unwrap();
            assert!(panel_request_url(&foreign, &origin, 0).is_none());
        }
    }

    #[test]
    fn plugin_labels_never_receive_tauri_commands() {
        assert!(trusted_invoke_label("main"));
        assert!(trusted_invoke_label(&format!(
            "workspace-{}_nonce",
            Uuid::new_v4()
        )));
        assert!(!trusted_invoke_label("plugin-spa-123"));
        assert!(!trusted_invoke_label("main-impersonator"));
        assert!(!trusted_invoke_label("workspace-"));
    }

    #[test]
    fn native_plugin_permissions_are_scoped_to_trusted_webviews() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("valid desktop capability");
        assert_eq!(
            capability["webviews"],
            serde_json::json!(["main", "workspace-*"])
        );
        assert!(
            capability.get("windows").is_none(),
            "window-scoped permissions also authorize untrusted child WebViews"
        );
    }

    #[test]
    fn bridge_rejects_spoofed_or_malformed_requests() {
        let valid = br#"{"version":1,"type":"save-svg","svg":"<svg/>","loadToken":"abc"}"#;
        assert!(parse_bridge(valid, "abc").is_ok());
        assert!(parse_bridge(valid, "other").is_err());
        assert!(
            parse_bridge(
                br#"{"version":2,"type":"save-svg","svg":"","loadToken":"abc"}"#,
                "abc"
            )
            .is_err()
        );
        assert!(
            parse_bridge(
                br#"{"version":1,"type":"save-svg","svg":"","loadToken":"abc","pluginId":"other"}"#,
                "abc"
            )
            .is_err()
        );
        assert!(parse_bridge(&vec![b'a'; MAX_BRIDGE_BYTES + 1], "abc").is_err());
        assert_eq!(
            serde_json::to_string(&BridgeError::from(PanelError::Denied)).unwrap(),
            "\"permission-denied\""
        );
        let denied = bridge_result(
            Err(PanelError::Denied),
            "save-result",
            "abc",
            None,
            &panel_origin(0),
        );
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(denied.body()).unwrap()["error"],
            "permission-denied"
        );
    }

    #[test]
    fn settings_bridge_accepts_only_bounded_optimizer_preferences() {
        let settings = serde_json::json!({
            "precision": 1,
            "pathPrecision": 2,
            "removeTspan": true,
            "removeStyling": true,
            "trimText": true,
            "autoAutocrop": false,
            "customWidth": 100,
            "customHeight": 100,
            "useCustomDimensions": false,
            "removeDefaultValues": true,
            "removeFontFamily": false,
            "removeFontSize": false,
            "convertSodipodiArcs": true,
            "groupSimilarElements": true,
            "groupTextElementsAtEnd": false
        });
        let request = |settings: serde_json::Value, sequence| {
            serde_json::to_vec(&serde_json::json!({
                "type": "settings-change",
                "version": 1,
                "loadToken": "abc",
                "settings": settings,
                "sequence": sequence,
                "flush": true
            }))
            .unwrap()
        };
        assert!(matches!(
            parse_bridge(&request(settings.clone(), 1), "abc"),
            Ok(BridgeRequest::SettingsChange { flush: true, .. })
        ));
        let persisted: SvgoSettings = serde_json::from_value(settings).unwrap();
        let script = bootstrap("abc", PanelTheme::Dark, Some(&persisted));
        assert!(script.contains("theme: \"dark\""));
        assert!(script.contains("\"pathPrecision\":2"));
        assert!(!script.contains("sourceSvg"));
        let receipt = bridge_result(Ok(()), "settings-result", "abc", Some(7), &panel_origin(0));
        let response: serde_json::Value = serde_json::from_slice(receipt.body()).unwrap();
        assert_eq!(response["type"], "settings-result");
        assert_eq!(response["sequence"], 7);
        assert_eq!(response["success"], true);
        let settings = serde_json::to_value(persisted).unwrap();
        assert!(parse_bridge(&request(settings.clone(), 1), "wrong").is_err());
        assert!(parse_bridge(&request(settings.clone(), 0), "abc").is_err());
        for (key, value) in [
            ("sourceSvg", serde_json::json!("<svg/>")),
            ("viewMode", serde_json::json!("tree")),
            ("theme", serde_json::json!("dark")),
            ("customWidth", serde_json::json!(0)),
            ("precision", serde_json::json!(6)),
            ("pathPrecision", serde_json::json!(1.5)),
        ] {
            let mut invalid = settings.clone();
            invalid[key] = value;
            assert!(parse_bridge(&request(invalid, 1), "abc").is_err(), "{key}");
        }
    }

    #[test]
    fn settings_bridge_persists_latest_sequence_and_acknowledges_close_flush() {
        let root = tempfile::tempdir().unwrap();
        let service = Arc::new(FileManagerService::new(
            fm_transport_dto::RuntimeKindDto::Tauri,
            root.path(),
            root.path().join("settings"),
        ));
        service
            .set_plugin_enabled(SVGO_PLUGIN_ID.to_owned(), true)
            .unwrap();
        let location = LocationDto {
            provider_id: "local".to_owned(),
            uri: "file:///drawing.svg".to_owned(),
        };
        let panel = service
            .plugin_panel(SVGO_PLUGIN_ID, "procyon.svgo.open", &location)
            .unwrap();
        let session = Arc::new(PanelSession {
            label: "plugin-spa-test".to_owned(),
            owner_window: "main".to_owned(),
            plugin_id: SVGO_PLUGIN_ID.to_owned(),
            action_id: "procyon.svgo.open".to_owned(),
            directory: panel.directory,
            entrypoint: panel.entrypoint,
            location,
            token: "test-token".to_owned(),
            revision: AsyncMutex::new("rev".to_owned()),
            save_lock: Arc::new(AsyncMutex::new(())),
            close_lock: AsyncMutex::new(()),
            settings_sequence: Mutex::new(0),
            flush_sender: Mutex::new(None),
            heartbeat: Mutex::new(None),
            heartbeat_responded: AtomicBool::new(false),
            created: Instant::now(),
            loaded: AtomicBool::new(true),
            visible: AtomicBool::new(true),
            shutdown: CancellationToken::new(),
            #[cfg(target_os = "linux")]
            _context_directory: tempfile::tempdir().unwrap(),
        });
        let mut settings = serde_json::json!({
            "precision": 2, "pathPrecision": 2,
            "removeTspan": true, "removeStyling": true, "trimText": true,
            "autoAutocrop": false, "customWidth": 100, "customHeight": 100,
            "useCustomDimensions": false, "removeDefaultValues": true,
            "removeFontFamily": false, "removeFontSize": false,
            "convertSodipodiArcs": true, "groupSimilarElements": true,
            "groupTextElementsAtEnd": false
        });
        let send = |settings: &serde_json::Value, sequence, flush| {
            let body = serde_json::to_vec(&serde_json::json!({
                "type": "settings-change", "version": 1,
                "loadToken": "test-token", "settings": settings,
                "sequence": sequence, "flush": flush,
            }))
            .unwrap();
            tauri::async_runtime::block_on(bridge(
                Arc::clone(&service),
                Arc::clone(&session),
                body,
                panel_origin(0),
            ))
        };
        assert_eq!(send(&settings, 2, false).status(), StatusCode::OK);
        settings["precision"] = serde_json::json!(1);
        assert_eq!(send(&settings, 1, false).status(), StatusCode::OK);
        assert_eq!(
            service.plugin_panel_settings(SVGO_PLUGIN_ID).unwrap()["precision"],
            2
        );
        let (sender, receiver) = oneshot::channel();
        *session.flush_sender.lock().unwrap() = Some(sender);
        settings["precision"] = serde_json::json!(3);
        assert_eq!(send(&settings, 3, true).status(), StatusCode::OK);
        assert!(tauri::async_runtime::block_on(receiver).unwrap());
        assert_eq!(
            service.plugin_panel_settings(SVGO_PLUGIN_ID),
            Some(settings)
        );

        let now = Instant::now();
        let first = session.next_heartbeat(now).unwrap().unwrap();
        assert_eq!(
            session
                .next_heartbeat(now + Duration::from_secs(19))
                .unwrap(),
            None
        );
        assert_eq!(
            session
                .acknowledge_heartbeat("incorrect-challenge")
                .unwrap_err()
                .to_string(),
            PanelError::Invalid.to_string()
        );
        let retry = session
            .next_heartbeat(now + VISIBLE_HEARTBEAT_TIMEOUT)
            .unwrap()
            .unwrap();
        assert_ne!(first, retry);
        let heartbeat = |challenge: &str| {
            tauri::async_runtime::block_on(bridge(
                Arc::clone(&service),
                Arc::clone(&session),
                serde_json::to_vec(&serde_json::json!({
                    "type": "heartbeat", "version": 1,
                    "loadToken": "test-token", "challenge": challenge
                }))
                .unwrap(),
                panel_origin(0),
            ))
        };
        assert_eq!(heartbeat(&first).status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            heartbeat("not-a-hex-challenge").status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(heartbeat(&retry).status(), StatusCode::OK);
        assert!(
            session
                .next_heartbeat(now + Duration::from_secs(21))
                .unwrap()
                .is_some()
        );
        session.visible.store(false, Ordering::SeqCst);
        assert_eq!(
            session
                .next_heartbeat(now + Duration::from_secs(41))
                .unwrap(),
            None
        );
        assert!(
            session
                .next_heartbeat(now + Duration::from_secs(141))
                .unwrap()
                .is_some()
        );
        assert!(
            session
                .next_heartbeat(now + Duration::from_secs(262))
                .is_err()
        );
        session.loaded.store(false, Ordering::SeqCst);
        assert!(
            session
                .next_heartbeat(session.created + VISIBLE_HEARTBEAT_TIMEOUT)
                .is_err()
        );
    }

    #[test]
    fn path_and_navigation_are_confined() {
        assert!(safe_asset_path("/dist/editor.js").is_some());
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/.hidden",
            "/editor.exe",
            "/bridge",
            "//example",
        ] {
            assert!(safe_asset_path(path).is_none(), "{path}");
        }

        let origin = panel_origin(0);
        assert!(allows_navigation(
            &format!("{origin}/index.html").parse().unwrap(),
            &origin
        ));
        assert!(!allows_navigation(
            &"https://example.com/".parse().unwrap(),
            &origin
        ));
        assert!(!allows_navigation(
            &"file:///etc/passwd".parse().unwrap(),
            &origin
        ));
    }

    #[test]
    fn reloading_panel_never_replays_stale_selected_content() {
        let expected: Url = format!("{}/dist/index.html", panel_origin(0))
            .parse()
            .unwrap();
        let other: Url = format!("{}/dist/other.html", panel_origin(0))
            .parse()
            .unwrap();
        let loaded = AtomicBool::new(false);
        assert!(!first_panel_load(&other, &expected, &loaded));
        assert!(first_panel_load(&expected, &expected, &loaded));
        assert!(!first_panel_load(&expected, &expected, &loaded));
    }

    #[test]
    fn registry_rejects_label_spoofing_and_releases_slots() {
        let registry = PanelRegistry::default();
        let label = "plugin-spa-a".to_owned();
        let location = LocationDto {
            provider_id: "local".into(),
            uri: "file:///selected.svg".into(),
        };
        let (slot, save_lock) = registry.reserve(label.clone(), &location).unwrap();
        assert_eq!(slot, 0);
        assert!(registry.session_for(slot, "plugin-spa-other").is_none());
        let shutdown = CancellationToken::new();
        registry
            .activate(
                slot,
                Arc::new(PanelSession {
                    label: label.clone(),
                    owner_window: "main".into(),
                    plugin_id: "first".into(),
                    action_id: "first.edit".into(),
                    directory: PathBuf::from("package"),
                    entrypoint: PathBuf::from("index.html"),
                    location: location.clone(),
                    token: "token".into(),
                    revision: AsyncMutex::new("rev".into()),
                    save_lock: Arc::clone(&save_lock),
                    close_lock: AsyncMutex::new(()),
                    settings_sequence: Mutex::new(0),
                    flush_sender: Mutex::new(None),
                    heartbeat: Mutex::new(None),
                    heartbeat_responded: AtomicBool::new(false),
                    created: Instant::now(),
                    loaded: AtomicBool::new(true),
                    visible: AtomicBool::new(true),
                    shutdown: shutdown.clone(),
                    #[cfg(target_os = "linux")]
                    _context_directory: tempfile::tempdir().unwrap(),
                }),
            )
            .unwrap();
        assert_eq!(registry.labels_for_plugin("first"), vec![label.clone()]);
        assert!(registry.owned_by(&label, "main"));
        assert!(!registry.owned_by(&label, "another-window"));
        assert_eq!(registry.labels_for_window("main"), vec![label.clone()]);
        assert!(registry.begin_window_close("main"));
        assert!(!registry.begin_window_close("main"));
        assert!(registry.window_close_pending("main"));
        registry.end_window_close("main");
        assert!(!registry.window_close_pending("main"));
        assert!(registry.session_for(slot, "plugin-spa-other").is_none());
        assert!(registry.session_for(slot, &label).is_some());
        registry.release(&label);
        assert!(shutdown.is_cancelled());
        assert!(registry.session_for(slot, &label).is_none());
        let (reused_slot, reused_lock) =
            registry.reserve("plugin-spa-b".into(), &location).unwrap();
        assert_eq!(reused_slot, slot);
        assert!(!Arc::ptr_eq(&save_lock, &reused_lock));
        for index in 1..PANEL_SLOTS {
            registry
                .reserve(format!("plugin-spa-{index}"), &location)
                .unwrap();
        }
        assert!(matches!(
            registry.reserve("plugin-spa-over-limit".into(), &location),
            Err(PanelError::Capacity)
        ));
        assert_eq!(
            PanelError::Capacity.to_string(),
            "too many plugin panels are open (limit: 16)"
        );
    }

    #[test]
    fn panels_on_the_same_file_share_a_save_lock_even_while_reserving() {
        let registry = PanelRegistry::default();
        let location = LocationDto {
            provider_id: "local".into(),
            uri: "file:///shared.svg".into(),
        };
        let (_, first) = registry.reserve("first".into(), &location).unwrap();
        let (_, second) = registry.reserve("second".into(), &location).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let (_, unrelated) = registry
            .reserve(
                "third".into(),
                &LocationDto {
                    provider_id: "local".into(),
                    uri: "file:///different.svg".into(),
                },
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &unrelated));
    }

    #[cfg(unix)]
    #[test]
    fn asset_reader_rejects_symlinks_outside_package_and_oversize_files() {
        let base = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("plugin-spa-security-{}", Uuid::new_v4()));
        let package = base.join("package");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("index.html"), b"safe").unwrap();
        std::fs::write(base.join("private.js"), b"secret").unwrap();
        std::os::unix::fs::symlink(base.join("private.js"), package.join("escape.js")).unwrap();
        assert_eq!(
            read_package_asset(&package, "/index.html").unwrap().0,
            b"safe"
        );
        assert!(matches!(
            read_package_asset(&package, "/escape.js"),
            Err(PanelError::Denied)
        ));
        let large = std::fs::File::create(package.join("large.js")).unwrap();
        large.set_len(MAX_ASSET_BYTES + 1).unwrap();
        assert!(matches!(
            read_package_asset(&package, "/large.js"),
            Err(PanelError::TooLarge)
        ));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn bundled_tree_editor_has_no_monaco_and_is_within_asset_budget() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/svgo");
        assert!(!package.join("dist/monaco").exists());
        let entrypoint = std::fs::read_to_string(package.join("dist/index.html")).unwrap();
        let bundle = std::fs::read_dir(package.join("dist/assets"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .find(|name| name.starts_with("index-") && name.ends_with(".js"))
            .expect("packaged SPA entrypoint missing");
        assert!(entrypoint.contains(&bundle));
        for asset in ["/dist/favicon.ico", "/dist/site.webmanifest"] {
            let (bytes, _) = read_package_asset(&package, asset).unwrap();
            assert!(!bytes.is_empty(), "{asset}");
            assert!(bytes.len() as u64 <= MAX_ASSET_BYTES);
        }
        let (script, _) = read_package_asset(&package, &format!("/dist/assets/{bundle}")).unwrap();
        assert!(!script.is_empty());
        assert!(!String::from_utf8(script).unwrap().contains("/monaco/"));
        let mut pending = vec![package.join("dist")];
        let mut count = 0;
        let mut total_bytes = 0;
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(&package)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let requested = format!("/{relative}");
                let (bytes, _) = read_package_asset(&package, &requested)
                    .unwrap_or_else(|_| panic!("packaged asset unavailable: {requested}"));
                total_bytes += bytes.len();
                count += 1;
            }
        }
        assert!(count >= 5, "packaged SPA assets missing");
        assert!(
            total_bytes < 2 * 1024 * 1024,
            "Tree-only package grew beyond 2 MiB"
        );
    }
}
