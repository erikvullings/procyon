//! Isolated, package-only desktop WebViews for enabled plugin SPA panels.

use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};

use fm_application::FileManagerService;
use fm_transport_dto::{LoadEditableFileRequestDto, LocationDto, SaveEditableFileRequestDto};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalSize, Rect, Runtime, State, Url,
    WebviewBuilder, WebviewUrl, Window,
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
const SVGO_PLUGIN_ID: &str = "procyon.svgo";
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
    settings_sequence: Mutex<u64>,
    flush_sender: Mutex<Option<oneshot::Sender<bool>>>,
    shutdown: CancellationToken,
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
pub(crate) struct PanelRegistry(Mutex<Vec<Option<Slot>>>);

impl PanelRegistry {
    fn reserve(
        &self,
        label: String,
        location: &LocationDto,
    ) -> Result<(usize, Arc<AsyncMutex<()>>), PanelError> {
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        let slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut slots = self.0.lock().unwrap_or_else(|error| error.into_inner());
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
        self.0
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
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .filter_map(|slot| match slot {
                Some(Slot::Active(session)) => Some(Arc::clone(session)),
                _ => None,
            })
            .collect()
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
                        .then(|| Url::parse(&request.uri().to_string()).ok())
                        .flatten()
                        .filter(|url| allows_navigation(url, &origin))
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
                                    "script-entered"
                                        | "plugin-ui-ready"
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
                let Some(url) = Url::parse(&request.uri().to_string()).ok().filter(|url| {
                    allows_navigation(url, &origin)
                        && url.query().is_none()
                        && url.fragment().is_none()
                }) else {
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
                    #[cfg(feature = "native-spa-smoke")]
                    if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some() {
                        crate::native_spa_smoke::stage(&format!(
                            "asset-response: {} {}",
                            result.status(),
                            url.path()
                        ));
                    }
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
        settings_sequence: Mutex::new(0),
        flush_sender: Mutex::new(None),
        shutdown: CancellationToken::new(),
    });
    registry
        .activate(slot, session)
        .inspect_err(|_| registry.release(&label))?;
    let load_script = deliver_load(&loaded.content, &location.uri, &token);
    let loaded_once = AtomicBool::new(false);
    let expected_url = url.clone();
    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
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
                    #[cfg(feature = "native-spa-smoke")]
                    if std::env::var_os("PROCYON_NATIVE_SPA_SMOKE_FILE").is_some()
                        && let Err(error) = window.eval(
                            &include_str!("native_spa_smoke.js")
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
    .map_err(|_| PanelError::Unavailable)
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

#[tauri::command]
pub(crate) async fn close_plugin_panel<R: Runtime>(
    source: Window<R>,
    registry: State<'_, Arc<PanelRegistry>>,
    label: String,
) -> Result<(), PanelError> {
    if !trusted_invoke_label(source.label()) || !registry.owned_by(&label, source.label()) {
        return Err(PanelError::Denied);
    }
    let session = registry
        .sessions()
        .into_iter()
        .find(|session| session.label == label)
        .ok_or(PanelError::Unavailable)?;
    let webview = match source.get_webview(&label) {
        Some(webview) => webview,
        None => {
            registry.release(&label);
            return Err(PanelError::Unavailable);
        }
    };
    if let Err(error) = webview.hide() {
        tracing::warn!(%error, "could not hide closing plugin panel");
    }
    let flush_result = if session.plugin_id == SVGO_PLUGIN_ID {
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = session
                .flush_sender
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if pending.is_some() {
                return Err(PanelError::Unavailable);
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
                tokio::time::timeout(std::time::Duration::from_secs(2), receiver).await,
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
            tracing::warn!("could not flush SVGO settings before closing plugin panel");
        }
        result
    } else {
        true
    };
    registry.release(&label);
    webview.close().map_err(|_| PanelError::Unavailable)?;
    if flush_result {
        Ok(())
    } else {
        Err(PanelError::Unavailable)
    }
}

pub(crate) fn close_plugin_panels<R: Runtime>(app: &AppHandle<R>, plugin_id: &str) {
    let registry = app.state::<Arc<PanelRegistry>>();
    for label in registry.labels_for_plugin(plugin_id) {
        registry.release(&label);
        if let Some(webview) = app.get_webview(&label) {
            let _ = webview.close();
        }
    }
}

pub(crate) fn close_panels_for_window<R: Runtime>(app: &AppHandle<R>, owner: &str) {
    let registry = app.state::<Arc<PanelRegistry>>();
    for label in registry.labels_for_window(owner) {
        registry.release(&label);
        if let Some(webview) = app.get_webview(&label)
            && let Err(error) = webview.close()
        {
            tracing::warn!(%error, "could not close plugin panel after host reload");
        }
    }
}

pub(crate) fn reconcile_panels<R: Runtime>(app: &AppHandle<R>) {
    let registry = app.state::<Arc<PanelRegistry>>();
    let service = &app.state::<AppState>().service;
    for session in registry.sessions() {
        if checked_panel(service, &session).is_err() {
            registry.release(&session.label);
            if let Some(webview) = app.get_webview(&session.label) {
                let _ = webview.close();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            settings_sequence: Mutex::new(0),
            flush_sender: Mutex::new(None),
            shutdown: CancellationToken::new(),
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
                    settings_sequence: Mutex::new(0),
                    flush_sender: Mutex::new(None),
                    shutdown: shutdown.clone(),
                }),
            )
            .unwrap();
        assert_eq!(registry.labels_for_plugin("first"), vec![label.clone()]);
        assert!(registry.owned_by(&label, "main"));
        assert!(!registry.owned_by(&label, "another-window"));
        assert_eq!(registry.labels_for_window("main"), vec![label.clone()]);
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
