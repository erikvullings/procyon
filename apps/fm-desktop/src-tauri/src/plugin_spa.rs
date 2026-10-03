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
    AppHandle, Manager, Runtime, State, Url, WebviewUrl, Window,
    http::{Method, Response, StatusCode},
    webview::{NewWindowResponse, PageLoadEvent, WebviewWindowBuilder},
};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::AppState;

const PANEL_SLOTS: usize = 16;
// Monaco's packaged TypeScript worker is ~5.8 MiB; larger assets are rejected.
const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BRIDGE_BYTES: usize = 1024 * 1024;
const BRIDGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
fn panel_csp(origin: &str) -> String {
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
    plugin_id: String,
    action_id: String,
    directory: PathBuf,
    entrypoint: PathBuf,
    location: LocationDto,
    token: String,
    revision: AsyncMutex<String>,
    save_lock: Arc<AsyncMutex<()>>,
    shutdown: CancellationToken,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveSvgRequest {
    version: u8,
    #[serde(rename = "type")]
    kind: String,
    svg: String,
    load_token: String,
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

fn parse_bridge(bytes: &[u8], token: &str) -> Result<SaveSvgRequest, PanelError> {
    if bytes.len() > MAX_BRIDGE_BYTES {
        return Err(PanelError::TooLarge);
    }
    let message: SaveSvgRequest = serde_json::from_slice(bytes).map_err(|_| PanelError::Invalid)?;
    if message.version != 1 || message.kind != "save-svg" || message.load_token != token {
        return Err(PanelError::Denied);
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
    request: SaveSvgRequest,
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
        content: request.svg,
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

async fn bridge(
    service: Arc<FileManagerService>,
    session: Arc<PanelSession>,
    body: Vec<u8>,
    origin: String,
) -> Response<Vec<u8>> {
    let result = match parse_bridge(&body, &session.token) {
        Ok(request) => save_svg(service, Arc::clone(&session), request).await,
        Err(error) => return bridge_result(Err(error), "", &origin),
    };
    bridge_result(result, &session.token, &origin)
}

fn bridge_result(result: Result<(), PanelError>, token: &str, origin: &str) -> Response<Vec<u8>> {
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
        kind: "save-result",
        success: result.is_ok(),
        error: result.err().map(BridgeError::from),
        load_token: token.to_owned(),
    };
    response(
        status,
        serde_json::to_vec(&response_body).expect("typed bridge result is serializable"),
        "application/json",
        origin,
    )
}

fn bootstrap(token: &str) -> String {
    let token = serde_json::to_string(token).expect("hex token is serializable");
    format!(
        r#"(() => {{
          const loadToken = {token};
          Object.defineProperty(window, 'procyonPlugin', {{
            value: Object.freeze({{
              loadToken,
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
                        responder.respond(bridge_result(Err(PanelError::TooLarge), "", &origin));
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

/// A trusted app window opens a bounded editable selection in a private,
/// command-less plugin window. The plugin cannot choose its own origin or file.
#[tauri::command]
pub(crate) async fn open_plugin_panel<R: Runtime>(
    app: AppHandle<R>,
    source: Window<R>,
    state: State<'_, AppState>,
    plugin_id: String,
    action_id: String,
    location: LocationDto,
) -> Result<String, PanelError> {
    if !trusted_invoke_label(source.label()) {
        return Err(PanelError::Denied);
    }
    let panel = state
        .service
        .plugin_panel(&plugin_id, &action_id, &location)
        .map_err(|_| PanelError::Unavailable)?;
    if !panel.can_read_selected {
        return Err(PanelError::Denied);
    }
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
        plugin_id,
        action_id,
        directory: panel.directory,
        entrypoint: panel.entrypoint,
        location: location.clone(),
        token: token.clone(),
        revision: AsyncMutex::new(loaded.revision),
        save_lock,
        shutdown: CancellationToken::new(),
    });
    registry
        .activate(slot, session)
        .inspect_err(|_| registry.release(&label))?;
    let load_script = deliver_load(&loaded.content, &location.uri, &token);
    let loaded_once = AtomicBool::new(false);
    let expected_url = url.clone();
    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(url))
        .title(panel.title)
        .inner_size(1000.0, 750.0)
        .incognito(true)
        .use_https_scheme(cfg!(target_os = "windows"))
        .initialization_script(bootstrap(&token))
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
            } else if let Err(error) = window.eval(&load_script) {
                tracing::warn!(%error, "could not deliver selected SVG to plugin panel");
                window
                    .app_handle()
                    .state::<Arc<PanelRegistry>>()
                    .release(window.label());
                if let Err(error) = window.close() {
                    tracing::warn!(%error, "could not close failed plugin panel");
                }
            }
        })
        .build();
    if window.is_err() {
        registry.release(&label);
    }
    window.map_err(|_| PanelError::Unavailable)?;
    Ok(label)
}

pub(crate) fn close_plugin_panels<R: Runtime>(app: &AppHandle<R>, plugin_id: &str) {
    let registry = app.state::<Arc<PanelRegistry>>();
    for label in registry.labels_for_plugin(plugin_id) {
        registry.release(&label);
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.close();
        }
    }
}

pub(crate) fn reconcile_panels<R: Runtime>(app: &AppHandle<R>) {
    let registry = app.state::<Arc<PanelRegistry>>();
    let service = &app.state::<AppState>().service;
    for session in registry.sessions() {
        if checked_panel(service, &session).is_err() {
            registry.release(&session.label);
            if let Some(window) = app.get_webview_window(&session.label) {
                let _ = window.close();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let denied = bridge_result(Err(PanelError::Denied), "abc", &panel_origin(0));
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(denied.body()).unwrap()["error"],
            "permission-denied"
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
                    plugin_id: "first".into(),
                    action_id: "first.edit".into(),
                    directory: PathBuf::from("package"),
                    entrypoint: PathBuf::from("index.html"),
                    location: location.clone(),
                    token: "token".into(),
                    revision: AsyncMutex::new("rev".into()),
                    save_lock: Arc::clone(&save_lock),
                    shutdown: shutdown.clone(),
                }),
            )
            .unwrap();
        assert_eq!(registry.labels_for_plugin("first"), vec![label.clone()]);
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
    fn bundled_monaco_editor_and_worker_are_within_asset_budget() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/svgo");
        for asset in [
            "/dist/monaco/vs/editor/editor.main.js",
            "/dist/monaco/vs/language/typescript/tsWorker.js",
            "/dist/assets/index-DkGjWShk.js",
            "/dist/monaco/vs/editor/editor.main.css",
            "/dist/monaco/vs/base/browser/ui/codicons/codicon/codicon.ttf",
            "/dist/favicon.ico",
            "/dist/site.webmanifest",
        ] {
            let (bytes, _) = read_package_asset(&package, asset).unwrap();
            assert!(!bytes.is_empty(), "{asset}");
            assert!(bytes.len() as u64 <= MAX_ASSET_BYTES);
        }
        let mut pending = vec![package.join("dist")];
        let mut count = 0;
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
                assert!(
                    read_package_asset(&package, &requested).is_ok(),
                    "packaged asset unavailable: {requested}"
                );
                count += 1;
            }
        }
        assert!(count >= 100, "Monaco worker assets were not bundled");
    }
}
