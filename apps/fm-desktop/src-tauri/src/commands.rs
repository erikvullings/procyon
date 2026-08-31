//! Tauri commands: thin wrappers over `FileManagerService`, mirroring the
//! semantic REST API rather than reproducing HTTP concepts (spec §11).
//!
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::utils::config::WindowConfig;
use tauri::{AppHandle, Manager, Runtime, State, Window};
use uuid::Uuid;

use fm_domain::OperationId;
use fm_transport_dto::{
    AcceptSshHostKeyRequestDto, ActionDescriptorDto, ActionResultDto, ApplicationErrorDto,
    ApplySyncPlanRequestDto, ApplySyncPlanResponseDto, ArchiveCredentialRequestDto,
    ArchiveSummaryRequestDto, ArchiveSummaryResponseDto, BeginOneDriveAuthorizationResponseDto,
    CalculateFolderSizeRequestDto, CalculateFolderSizeResponseDto, ChecksumFileDto,
    ChecksumPageDto, ComparisonPageDto, ConnectionDto, ConnectionStateDto,
    CreateConnectionRequestDto, CreateWorkspaceRequestDto, DiagnosticErrorDto, DiagnosticsDto,
    DirectorySnapshotDto, DiscoverApplicationUninstallCandidatesRequestDto,
    DiscoverApplicationUninstallCandidatesResponseDto, DocxPreviewSessionRequestDto,
    DuplicatePageDto, EntryMetadataDto, EntryMetadataRequest, EntrySummaryDto, FinderTagsDto,
    GenerateSyncPlanRequestDto, GetFileGitHistoryRequestDto, GetFileGitHistoryResponseDto,
    HostKeyProbeDto, InvokeActionRequestDto, ListDirectoryChildrenRequest, ListDirectoryRequest,
    LocationDto, NavigateRequest, OneDriveAuthorizationAttemptDto, OpenDocxPreviewRequestDto,
    OpenDocxPreviewResponseDto, OpenStructuredViewRequestDto, OpenStructuredViewResponseDto,
    OperationDto, OperationQueueStatusDto, PluginDescriptorDto, PluginLogEntryDto, PluginStatusDto,
    ReadDocxPreviewResourceRequestDto, ReadDocxPreviewResourceResponseDto, ReadFileRangeRequestDto,
    ReadFileRangeResponseDto, ReadStructuredJsonWindowRequestDto,
    ReadStructuredJsonWindowResponseDto, ReadStructuredRowsRequestDto,
    ReadStructuredRowsResponseDto, RemoveApplicationDockIconRequestDto,
    RemoveApplicationDockIconResponseDto, RenderChecksumFileRequestDto,
    ResolveOperationConflictRequestDto, RuntimeCapabilitiesDto, SaveChecksumFileRequestDto,
    SaveChecksumFileResponseDto, ScanDiskUsageRequestDto, SearchInFileRequestDto,
    SearchInFileResponseDto, SearchStructuredRowsRequestDto, SearchStructuredRowsResponseDto,
    SetPaneActivityRequest, SettingsDto, SpotlightCommentDto, StartChecksumRequestDto,
    StartChecksumResponseDto, StartComparisonRequestDto, StartComparisonResponseDto,
    StartDuplicateScanRequestDto, StartDuplicateScanResponseDto, StartOperationRequestDto,
    StartSearchRequestDto, StartSearchResponseDto, StructuredViewSessionRequestDto,
    StructuredViewStatusDto, SyncPlanDto, UpdateConnectionRequestDto,
    UpdateStructuredViewRequestDto, VerificationReportDto, VerifyChecksumFileRequestDto,
    WorkspaceCommandDto, WorkspaceDto, WorkspaceSummaryDto,
};

#[cfg(target_os = "windows")]
use std::any::Any;

use crate::{
    AppState,
    event_stream::EventSubscriptionRegistry,
    native_menu::NativeMenuActionChannel,
    terminal::{TerminalError, TerminalEvent, TerminalRegistry},
};

/// Opens (or reuses) an embedded terminal session for `location` - a local
/// PTY for a `file:` location, or a remote PTY over SSH (task 0105) for a
/// `sftp:` one. A local location whose native path cannot be resolved falls
/// through to [`TerminalRegistry::open`]'s own `UnsupportedLocation` error
/// rather than being rejected here, so both schemes report failures the same
/// way.
#[tauri::command]
pub(crate) async fn open_embedded_terminal(
    state: State<'_, AppState>,
    registry: State<'_, TerminalRegistry>,
    location: LocationDto,
    columns: u16,
    rows: u16,
    channel: Channel<TerminalEvent>,
) -> Result<String, TerminalError> {
    let location_uri = location.uri.clone();
    let native_path = fm_domain::Location::from(location).to_native_path().ok();
    registry
        .open(
            &state.service,
            &location_uri,
            native_path.as_deref(),
            portable_pty::PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            },
            channel,
        )
        .await
}

#[tauri::command]
pub(crate) async fn write_embedded_terminal(
    registry: State<'_, TerminalRegistry>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), TerminalError> {
    registry.write(&session_id, &data).await
}

#[tauri::command]
pub(crate) async fn resize_embedded_terminal(
    registry: State<'_, TerminalRegistry>,
    session_id: String,
    columns: u16,
    rows: u16,
) -> Result<(), TerminalError> {
    registry
        .resize(
            &session_id,
            portable_pty::PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            },
        )
        .await
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum NativeDragError {
    #[error("native file dragging is unavailable on this platform")]
    Unsupported,
    #[error("at least one file is required to start a native drag")]
    EmptySelection,
    #[error("cannot drag `{uri}` as a native file: {reason}")]
    InvalidLocation { uri: String, reason: String },
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[error("failed to schedule native drag: {0}")]
    Schedule(String),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[error("failed to start native drag: {0}")]
    Start(String),
}

impl serde::Serialize for NativeDragError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformContextMenuError {
    #[error("the platform context menu is unavailable")]
    Unsupported,
    #[error("at least one file is required to open the platform context menu")]
    EmptySelection,
    #[error("cannot expose `{uri}` to the platform context menu: {reason}")]
    InvalidLocation { uri: String, reason: String },
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[error("failed to schedule the platform context menu: {0}")]
    Schedule(String),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[error("failed to open the platform context menu: {0}")]
    Open(String),
}

impl serde::Serialize for PlatformContextMenuError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

fn platform_context_menu_paths(
    locations: Vec<LocationDto>,
) -> Result<Vec<std::path::PathBuf>, PlatformContextMenuError> {
    if locations.is_empty() {
        return Err(PlatformContextMenuError::EmptySelection);
    }
    locations
        .into_iter()
        .map(|dto| {
            let uri = dto.uri.clone();
            fm_domain::Location::from(dto)
                .to_native_path()
                .map_err(|error| PlatformContextMenuError::InvalidLocation {
                    uri,
                    reason: error.to_string(),
                })
        })
        .collect()
}

fn native_drag_paths(
    locations: Vec<LocationDto>,
) -> Result<Vec<std::path::PathBuf>, NativeDragError> {
    if locations.is_empty() {
        return Err(NativeDragError::EmptySelection);
    }
    locations
        .into_iter()
        .map(|dto| {
            let uri = dto.uri.clone();
            fm_domain::Location::from(dto)
                .to_native_path()
                .map_err(|error| NativeDragError::InvalidLocation {
                    uri,
                    reason: error.to_string(),
                })
        })
        .collect()
}

/// Converts native paths supplied by Finder/Explorer into validated local locations.
#[tauri::command]
pub(crate) fn native_drag_locations(
    paths: Vec<std::path::PathBuf>,
) -> Result<Vec<LocationDto>, NativeDragError> {
    if paths.is_empty() {
        return Err(NativeDragError::EmptySelection);
    }
    paths
        .into_iter()
        .map(|path| {
            fm_domain::Location::from_native_path(&path)
                .map(Into::into)
                .map_err(|error| NativeDragError::InvalidLocation {
                    uri: path.display().to_string(),
                    reason: error.to_string(),
                })
        })
        .collect()
}

/// Opens the OS-populated Services (macOS) or Send To (Windows) submenu for a local selection.
#[tauri::command]
pub(crate) async fn show_platform_context_menu<R: Runtime>(
    state: State<'_, AppState>,
    app: AppHandle<R>,
    locations: Vec<LocationDto>,
) -> Result<(), PlatformContextMenuError> {
    if !state.service.runtime_capabilities().platform_context_menu {
        return Err(PlatformContextMenuError::Unsupported);
    }
    let paths = platform_context_menu_paths(locations)?;

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            #[cfg(target_os = "macos")]
            let result = fm_platform_macos::show_services_menu(&paths);
            #[cfg(target_os = "windows")]
            let result = fm_platform_windows::show_send_to_menu(&paths);
            let _ = sender
                .send(result.map_err(|error| PlatformContextMenuError::Open(error.to_string())));
        })
        .map_err(|error| PlatformContextMenuError::Schedule(error.to_string()))?;
        receiver
            .await
            .map_err(|error| PlatformContextMenuError::Schedule(error.to_string()))?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, paths);
        Err(PlatformContextMenuError::Unsupported)
    }
}

/// Starts a Finder/Explorer file-reference drag from the current desktop window.
#[tauri::command]
pub(crate) async fn start_native_drag<R: Runtime>(
    state: State<'_, AppState>,
    app: AppHandle<R>,
    window: Window<R>,
    locations: Vec<LocationDto>,
) -> Result<(), NativeDragError> {
    if !state.service.runtime_capabilities().native_drag_out {
        return Err(NativeDragError::Unsupported);
    }
    let paths = native_drag_paths(locations)?;

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = drag::start_drag(
                &window,
                drag::DragItem::Files(paths),
                drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec()),
                |_, _| {},
                drag::Options::default(),
            )
            .map_err(|error| NativeDragError::Start(error.to_string()));
            let _ = sender.send(result);
        })
        .map_err(|error| NativeDragError::Schedule(error.to_string()))?;
        receiver
            .await
            .map_err(|error| NativeDragError::Schedule(error.to_string()))?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (app, window, paths);
        Err(NativeDragError::Unsupported)
    }
}

/// Paints the native window caption to match the application chrome.
///
/// Windows draws the caption itself and only follows the light/dark system theme, so without
/// this the title bar sits at the OS chrome colour rather than the app's surface colour. macOS
/// draws its caption over our own reserved row already, and Linux caption colours are the
/// compositor's business, so both are deliberate no-ops.
#[tauri::command]
pub(crate) fn set_caption_colours<R: Runtime>(
    window: Window<R>,
    background: String,
    foreground: String,
) {
    #[cfg(target_os = "windows")]
    {
        let (Some(caption), Some(text)) = (colorref(&background), colorref(&foreground)) else {
            return;
        };
        let Ok(handle) = window.hwnd() else { return };
        fm_platform_windows::set_caption_colours(handle.0 as isize, caption, text);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, background, foreground);
    }
}

/// Switches Windows to the frontend-owned title bar, which lets the menu share the app's
/// themed surface instead of occupying a separate native `HMENU` band.
#[tauri::command]
pub(crate) fn set_window_decorations<R: Runtime>(window: Window<R>, decorations: bool) {
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_decorations(decorations);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, decorations);
    }
}

/// Returns the desktop diagnostics snapshot without requiring an HTTP server in the Tauri host.
#[tauri::command]
pub(crate) fn get_diagnostics(state: State<'_, AppState>) -> DiagnosticsDto {
    let plugins = state.service.list_plugins();
    let operations = state.service.list_operations();
    let mut queued_count = 0;
    let mut running_count = 0;
    let mut paused_count = 0;
    let mut completed_count = 0;
    for operation in operations {
        match operation.state {
            fm_transport_dto::OperationStateDto::Queued => queued_count += 1,
            fm_transport_dto::OperationStateDto::Running
            | fm_transport_dto::OperationStateDto::Planning
            | fm_transport_dto::OperationStateDto::Cancelling
            | fm_transport_dto::OperationStateDto::WaitingForConflictResolution => {
                running_count += 1
            }
            fm_transport_dto::OperationStateDto::Paused => paused_count += 1,
            fm_transport_dto::OperationStateDto::Completed
            | fm_transport_dto::OperationStateDto::CompletedWithWarnings => completed_count += 1,
            _ => {}
        }
    }
    DiagnosticsDto {
        frontend_version: env!("CARGO_PKG_VERSION").to_owned(),
        backend_version: env!("CARGO_PKG_VERSION").to_owned(),
        tauri_version: None,
        platform: "Windows".to_owned(),
        runtime_capabilities: state.service.runtime_capabilities(),
        connection_state: ConnectionStateDto {
            connected: true,
            last_event_received: None,
            uptime_seconds: 0,
            events_received: 0,
            status_message: "Desktop channel active".to_owned(),
        },
        loaded_plugins: plugins
            .into_iter()
            .map(|plugin| PluginStatusDto {
                plugin_id: plugin.id,
                name: plugin.name,
                enabled: plugin.enabled,
                version: plugin.version,
                error_count: 0,
            })
            .collect(),
        recent_errors: Vec::<DiagnosticErrorDto>::new(),
        operation_queue_status: OperationQueueStatusDto {
            queued_count,
            running_count,
            paused_count,
            completed_count,
            total_pending_size: 0,
        },
    }
}

/// Converts a `#rgb`/`#rrggbb` CSS colour into a Win32 `COLORREF` (`0x00bbggrr`).
#[cfg(target_os = "windows")]
fn colorref(css: &str) -> Option<u32> {
    let digits = css.trim().strip_prefix('#')?;
    let expanded = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => digits.to_owned(),
        _ => return None,
    };
    let red = u32::from_str_radix(&expanded[0..2], 16).ok()?;
    let green = u32::from_str_radix(&expanded[2..4], 16).ok()?;
    let blue = u32::from_str_radix(&expanded[4..6], 16).ok()?;
    Some((blue << 16) | (green << 8) | red)
}

/// Caches an archive password for the lifetime of this desktop backend session.
#[tauri::command]
pub(crate) fn cache_archive_password(
    state: State<'_, AppState>,
    request: ArchiveCredentialRequestDto,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cache_archive_password(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts one ordered EventBus-to-IPC channel subscription for this window.
#[tauri::command]
pub(crate) fn subscribe_events<R: Runtime>(
    state: State<'_, AppState>,
    subscriptions: State<'_, EventSubscriptionRegistry>,
    window: Window<R>,
    on_event: Channel<String>,
) -> Uuid {
    let id = subscriptions.subscribe(
        state.service.event_bus(),
        window.label().to_owned(),
        on_event,
    );
    state.service.republish_pending_operation_conflicts();
    id
}

/// Releases a desktop event subscription created by [`subscribe_events`].
#[tauri::command]
pub(crate) fn unsubscribe_events(
    subscriptions: State<'_, EventSubscriptionRegistry>,
    subscription_id: Uuid,
) {
    subscriptions.unsubscribe(subscription_id);
}

/// Reports the capabilities available for the current runtime and platform
/// (spec §21), identical in shape to `GET /api/v1/runtime`.
#[tauri::command]
pub(crate) fn get_runtime_capabilities(state: State<'_, AppState>) -> RuntimeCapabilitiesDto {
    state.service.runtime_capabilities()
}

/// Lists OS-managed filesystem locations through the shared application service.
#[tauri::command]
pub(crate) async fn get_system_locations(
    state: State<'_, AppState>,
) -> Result<Vec<fm_transport_dto::SystemLocationDto>, ApplicationErrorDto> {
    state
        .service
        .system_locations()
        .await
        .map_err(|error| error.into_dto(uuid::Uuid::new_v4()))
}

/// Lists currently mounted volumes through the shared application service.
#[tauri::command]
pub(crate) async fn get_volumes(
    state: State<'_, AppState>,
) -> Result<Vec<fm_transport_dto::VolumeDto>, ApplicationErrorDto> {
    state
        .service
        .volumes()
        .await
        .map_err(|error| error.into_dto(uuid::Uuid::new_v4()))
}

/// Returns the current user's home directory as a native path, for expanding a leading `~`
/// typed into an address bar.
#[tauri::command]
pub(crate) fn get_home_directory(state: State<'_, AppState>) -> Option<String> {
    state.service.home_directory()
}

/// Returns the same native PNG bytes as `GET /api/v1/icons`.
#[tauri::command]
pub(crate) fn get_file_icon(
    state: State<'_, AppState>,
    uri: String,
) -> Result<Vec<u8>, ApplicationErrorDto> {
    state
        .service
        .file_icon(&uri)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns the same JPEG thumbnail bytes as `GET /api/v1/thumbnails`
/// (task 0134). `size` must be `"small"`, `"medium"` or `"large"`.
#[tauri::command]
pub(crate) async fn get_thumbnail(
    state: State<'_, AppState>,
    uri: String,
    size: String,
) -> Result<Vec<u8>, ApplicationErrorDto> {
    state
        .service
        .thumbnail(&uri, &size)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns the same Finder tags as `GET /api/v1/finder-tags` (task 0136).
#[tauri::command]
pub(crate) fn get_finder_tags(
    state: State<'_, AppState>,
    uri: String,
) -> Result<FinderTagsDto, ApplicationErrorDto> {
    state
        .service
        .finder_tags(&uri)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Replaces Finder tags, same as `PUT /api/v1/finder-tags` (task 0136).
#[tauri::command]
pub(crate) fn set_finder_tags(
    state: State<'_, AppState>,
    uri: String,
    request: FinderTagsDto,
) -> Result<FinderTagsDto, ApplicationErrorDto> {
    state
        .service
        .set_finder_tags(&uri, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns the same Spotlight comment as `GET /api/v1/spotlight-comment`
/// (task 0136).
#[tauri::command]
pub(crate) fn get_spotlight_comment(
    state: State<'_, AppState>,
    uri: String,
) -> Result<SpotlightCommentDto, ApplicationErrorDto> {
    state
        .service
        .spotlight_comment(&uri)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Sets or clears the Spotlight comment, same as `PUT /api/v1/spotlight-comment`
/// (task 0136).
#[tauri::command]
pub(crate) fn set_spotlight_comment(
    state: State<'_, AppState>,
    uri: String,
    request: SpotlightCommentDto,
) -> Result<SpotlightCommentDto, ApplicationErrorDto> {
    state
        .service
        .set_spotlight_comment(&uri, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns the same settings document as `GET /api/v1/settings`.
#[tauri::command]
pub(crate) fn get_settings(state: State<'_, AppState>) -> SettingsDto {
    state.service.get_settings()
}

/// Atomically persists the same settings document as `PUT /api/v1/settings`.
#[tauri::command]
pub(crate) fn update_settings(
    state: State<'_, AppState>,
    settings: SettingsDto,
) -> Result<SettingsDto, ApplicationErrorDto> {
    state
        .service
        .update_settings(settings)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Lists a directory through the same application service as Axum.
#[tauri::command]
pub(crate) async fn list_directory(
    state: State<'_, AppState>,
    request: ListDirectoryRequest,
) -> Result<DirectorySnapshotDto, ApplicationErrorDto> {
    state
        .service
        .list_directory(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Refreshes a directory through the same application service as Axum.
#[tauri::command]
pub(crate) async fn refresh_directory(
    state: State<'_, AppState>,
    request: ListDirectoryRequest,
) -> Result<DirectorySnapshotDto, ApplicationErrorDto> {
    state
        .service
        .refresh_directory(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Lists the immediate child directories of a location through the same application service as
/// Axum, for the directory-tree sidebar (task 0139).
#[tauri::command]
pub(crate) async fn list_directory_children(
    state: State<'_, AppState>,
    request: ListDirectoryChildrenRequest,
) -> Result<Vec<EntrySummaryDto>, ApplicationErrorDto> {
    state
        .service
        .list_directory_children(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Navigates a pane through the same application service as Axum.
#[tauri::command]
pub(crate) async fn navigate_pane(
    state: State<'_, AppState>,
    request: NavigateRequest,
) -> Result<DirectorySnapshotDto, ApplicationErrorDto> {
    state
        .service
        .navigate_pane(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Fetches entry metadata through the same application service as Axum.
#[tauri::command]
pub(crate) async fn get_entry_metadata(
    state: State<'_, AppState>,
    request: EntryMetadataRequest,
) -> Result<EntryMetadataDto, ApplicationErrorDto> {
    state
        .service
        .get_entry_metadata(request)
        .await
        .map(Into::into)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Marks a pane's foreground/background state through the same application
/// service as Axum (task 0109).
#[tauri::command]
pub(crate) async fn set_pane_activity(
    state: State<'_, AppState>,
    request: SetPaneActivityRequest,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .set_pane_activity(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Reads one bounded byte range from a file through the same application
/// service as Axum, for the in-app large file viewer (task 0088).
#[tauri::command]
pub(crate) async fn read_file_range(
    state: State<'_, AppState>,
    request: ReadFileRangeRequestDto,
) -> Result<ReadFileRangeResponseDto, ApplicationErrorDto> {
    state
        .service
        .read_file_range(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn open_docx_preview(
    state: State<'_, AppState>,
    request: OpenDocxPreviewRequestDto,
) -> Result<OpenDocxPreviewResponseDto, ApplicationErrorDto> {
    state
        .service
        .open_docx_preview(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn read_docx_preview_resource(
    state: State<'_, AppState>,
    request: ReadDocxPreviewResourceRequestDto,
) -> Result<ReadDocxPreviewResourceResponseDto, ApplicationErrorDto> {
    state
        .service
        .read_docx_preview_resource(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn close_docx_preview(
    state: State<'_, AppState>,
    request: DocxPreviewSessionRequestDto,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .close_docx_preview(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn open_structured_view(
    state: State<'_, AppState>,
    request: OpenStructuredViewRequestDto,
) -> Result<OpenStructuredViewResponseDto, ApplicationErrorDto> {
    state
        .service
        .open_structured_view(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn structured_view_status(
    state: State<'_, AppState>,
    request: StructuredViewSessionRequestDto,
) -> Result<StructuredViewStatusDto, ApplicationErrorDto> {
    state
        .service
        .structured_view_status(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn update_structured_view(
    state: State<'_, AppState>,
    request: UpdateStructuredViewRequestDto,
) -> Result<OpenStructuredViewResponseDto, ApplicationErrorDto> {
    state
        .service
        .update_structured_view(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn read_structured_rows(
    state: State<'_, AppState>,
    request: ReadStructuredRowsRequestDto,
) -> Result<ReadStructuredRowsResponseDto, ApplicationErrorDto> {
    state
        .service
        .read_structured_rows(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn read_structured_json_window(
    state: State<'_, AppState>,
    request: ReadStructuredJsonWindowRequestDto,
) -> Result<ReadStructuredJsonWindowResponseDto, ApplicationErrorDto> {
    state
        .service
        .read_structured_json_window(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn search_structured_rows(
    state: State<'_, AppState>,
    request: SearchStructuredRowsRequestDto,
) -> Result<SearchStructuredRowsResponseDto, ApplicationErrorDto> {
    state
        .service
        .search_structured_rows(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn close_structured_view(
    state: State<'_, AppState>,
    request: StructuredViewSessionRequestDto,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .close_structured_view(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn load_editable_file(
    state: State<'_, AppState>,
    request: fm_transport_dto::LoadEditableFileRequestDto,
) -> Result<fm_transport_dto::LoadEditableFileResponseDto, ApplicationErrorDto> {
    state
        .service
        .load_editable_file(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[tauri::command]
pub(crate) async fn save_editable_file(
    state: State<'_, AppState>,
    request: fm_transport_dto::SaveEditableFileRequestDto,
) -> Result<fm_transport_dto::SaveEditableFileResponseDto, ApplicationErrorDto> {
    state
        .service
        .save_editable_file(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Searches a file's content through the same application service as Axum,
/// for the in-app large file viewer (task 0088).
#[tauri::command]
pub(crate) async fn search_in_file(
    state: State<'_, AppState>,
    request: SearchInFileRequestDto,
) -> Result<SearchInFileResponseDto, ApplicationErrorDto> {
    state
        .service
        .search_in_file(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Recursively sums a directory's total size through the same application service as Axum
/// (task 0071's Total Commander-style folder-size key).
#[tauri::command]
pub(crate) async fn calculate_folder_size(
    state: State<'_, AppState>,
    request: CalculateFolderSizeRequestDto,
) -> Result<CalculateFolderSizeResponseDto, ApplicationErrorDto> {
    state
        .service
        .calculate_folder_size(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Computes an archive summary through the same application service as Axum.
#[tauri::command]
pub(crate) async fn archive_summary(
    state: State<'_, AppState>,
    request: ArchiveSummaryRequestDto,
) -> Result<ArchiveSummaryResponseDto, ApplicationErrorDto> {
    state
        .service
        .archive_summary(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts a hierarchical disk-usage scan. Results and terminal failures arrive over the shared
/// event stream, so the IPC request does not remain open for the lifetime of a large scan.
fn dispatch_disk_usage_scan(
    service: Arc<fm_application::FileManagerService>,
    request: ScanDiskUsageRequestDto,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        service.run_disk_usage_job(request).await;
    })
}

#[tauri::command]
pub(crate) fn scan_disk_usage(state: State<'_, AppState>, request: ScanDiskUsageRequestDto) {
    std::mem::drop(dispatch_disk_usage_scan(
        Arc::clone(&state.service),
        request,
    ));
}

/// Cancels a running disk-usage scan (task 0118 follow-up) through the shared service, for
/// aborting/closing a scan's tab so the blocking traversal actually stops.
#[tauri::command]
pub(crate) fn cancel_disk_usage(
    state: State<'_, AppState>,
    scan_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cancel_disk_usage(scan_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Scans a `.app` bundle's well-known related-file locations through the same application
/// service as Axum, for the uninstall review checklist (task 0148). Read-only: nothing is
/// deleted by this call.
#[tauri::command]
pub(crate) async fn discover_application_uninstall_candidates(
    state: State<'_, AppState>,
    request: DiscoverApplicationUninstallCandidatesRequestDto,
) -> Result<DiscoverApplicationUninstallCandidatesResponseDto, ApplicationErrorDto> {
    state
        .service
        .discover_application_uninstall_candidates(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Removes a `.app` bundle's pinned Dock icon through the same application service as Axum, once
/// the user confirms an uninstall (task 0148 follow-up). Best-effort: no pinned icon is a normal
/// `removed: false`, not an error.
#[tauri::command]
pub(crate) async fn remove_application_dock_icon(
    state: State<'_, AppState>,
    request: RemoveApplicationDockIconRequestDto,
) -> Result<RemoveApplicationDockIconResponseDto, ApplicationErrorDto> {
    state
        .service
        .remove_application_dock_icon(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Fetches a file's git commit history through the same application service as Axum, for the
/// Alt+Space metadata panel's history section (task 0135).
#[tauri::command]
pub(crate) async fn get_file_git_history(
    state: State<'_, AppState>,
    request: GetFileGitHistoryRequestDto,
) -> Result<GetFileGitHistoryResponseDto, ApplicationErrorDto> {
    Ok(state.service.git_file_history(request).await)
}

/// A fresh, unique window label for a new window on `workspace_id` (task 0143 sub-task (b)).
/// Every call returns a different label - "Open in New Window" is meant to always open another
/// window, even for a workspace that already has one or more windows open, so labels can't be
/// reused as a dedup key the way an earlier version of this command did. The `workspace-<id>`
/// prefix is still what the capability glob (`capabilities/default.json`) and
/// [`canonical_workspace_window_label`] key off of; the `_<nonce>` suffix only exists to keep
/// labels distinct.
fn workspace_window_label(workspace_id: Uuid) -> String {
    format!("workspace-{workspace_id}_{}", Uuid::new_v4())
}

/// Reduces a window label back to its stable `workspace-<id>` form, stripping the uniquifying
/// nonce `workspace_window_label` appends. Passed to `tauri-plugin-window-state`'s `map_label` so
/// every window opened for the same workspace shares one remembered frame (position/size) instead
/// of each getting its own, now that windows for a workspace are no longer deduplicated by label.
/// Labels with no nonce (`"main"`) pass through unchanged.
pub(crate) fn canonical_workspace_window_label(label: &str) -> &str {
    label.split('_').next().unwrap_or(label)
}

/// The inverse of `workspace_window_label`: recovers the workspace id a window was opened for
/// from its label, or `None` for the config-declared `"main"` window (which carries no id in its
/// label) or any other non-`workspace-<id>` label. Used on window close to find the ephemeral
/// workspace (if any) that window's close should delete (ephemeral per-window workspaces spec
/// follow-up).
pub(crate) fn workspace_id_from_label(label: &str) -> Option<Uuid> {
    canonical_workspace_window_label(label)
        .strip_prefix("workspace-")
        .and_then(|id| Uuid::parse_str(id).ok())
}

/// Picks out the workspaces `lib.rs`'s `setup()` should restore a window for at startup - every
/// `ephemeral` (per-window) workspace still on disk, meaning it survived a previous app *quit*
/// rather than the user closing that window (which deletes its own ephemeral workspace - see
/// `QuittingFlag` in `lib.rs`). Named/template workspaces are never restored as their own window;
/// they only ever become a window's *source* via `open_workspace_window`'s fork
/// (ephemeral per-window workspaces spec follow-up, phase 2).
pub(crate) fn ephemeral_workspace_ids(summaries: &[WorkspaceSummaryDto]) -> Vec<Uuid> {
    summaries
        .iter()
        .filter(|summary| summary.ephemeral)
        .map(|summary| summary.id)
        .collect()
}

/// The app's single declared window entry from `tauri.conf.json`, used as a template for every
/// window this host builds - its `"create": false` there stops Tauri auto-building it at
/// startup, so `lib.rs`'s `setup()` and this module build every window explicitly instead
/// (ephemeral per-window workspaces spec follow-up, phase 2). Panics if the config declares no
/// window at all, which would mean `tauri.conf.json` itself is broken.
fn declared_window_config<R: Runtime>(app: &AppHandle<R>) -> WindowConfig {
    app.config()
        .app
        .windows
        .first()
        .cloned()
        .expect("tauri.conf.json must declare at least one window")
}

/// Builds and shows the app's single default window ("main"), using the declared config as-is -
/// the app's normal cold-start path when there is no surviving ephemeral workspace to restore.
/// Its frontend calls `start_workspace(None)`, resolving the last-active named workspace or
/// creating a default one (spec §5.3.7).
pub(crate) fn build_default_window<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<tauri::WebviewWindow<R>> {
    tauri::WebviewWindowBuilder::from_config(app, &declared_window_config(app))?.build()
}

/// Builds and shows a new OS window on `workspace_id`, reusing the declared window's config
/// (title, size, macOS title-bar style, etc.) but with its own label and a
/// `?workspaceId=<id>` URL, so its frontend calls `start_workspace` with that id explicitly on
/// boot instead of falling back to the last-active workspace (spec §5.3.7; see
/// `frontend/src/app/app-shell.ts`'s startup path). Shared by [`open_workspace_window`] (after
/// forking a brand-new ephemeral workspace) and `lib.rs`'s startup restore of ephemeral
/// workspaces that survived a previous quit (ephemeral per-window workspaces spec follow-up,
/// phase 2) - callers differ only in whether `workspace_id` was just created or already existed.
pub(crate) fn build_workspace_window<R: Runtime>(
    app: &AppHandle<R>,
    workspace_id: Uuid,
) -> tauri::Result<tauri::WebviewWindow<R>> {
    let mut config = declared_window_config(app);
    config.label = workspace_window_label(workspace_id);
    config.url = tauri::WebviewUrl::App(format!("index.html?workspaceId={workspace_id}").into());
    tauri::WebviewWindowBuilder::from_config(app, &config)?.build()
}

/// Opens one window per surviving ephemeral workspace, or a single default window if none
/// survived - the app's cold-start restore logic (`lib.rs`'s `setup()`), factored out so it can
/// also run when macOS reactivates the app with no visible windows open (`RunEvent::Reopen`).
///
/// Without this second call site, closing the app's last window (the ordinary macOS red-button
/// close, which does not quit the app - see `QuittingFlag`'s doc comment) left the process
/// running with zero windows and no way to bring one back: `tauri_plugin_single_instance`'s
/// callback only fires for a genuine new OS-level launch, not for the Dock icon reactivating an
/// already-running app, and nothing handled `RunEvent::Reopen` to create a window in that case
/// either. From the user's perspective this looked exactly like "quit and reopen lost my
/// workspace" - clicking the Dock icon silently did nothing, so recovering meant force-quitting
/// and relaunching a truly fresh process, by which point the ephemeral workspace behind the
/// closed window had already been deleted (correctly, per that window's own close).
pub(crate) async fn open_startup_windows<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let service = app.state::<AppState>().service.clone();
    let surviving_ephemeral_ids: Vec<Uuid> = service
        .list_workspaces()
        .await
        .map(|summaries| ephemeral_workspace_ids(&summaries))
        .unwrap_or_default();

    if surviving_ephemeral_ids.is_empty() {
        let _window = build_default_window(app)?;
        #[cfg(target_os = "windows")]
        let _ = _window.set_decorations(false);
    } else {
        for workspace_id in surviving_ephemeral_ids {
            let _window = build_workspace_window(app, workspace_id)?;
            #[cfg(target_os = "windows")]
            let _ = _window.set_decorations(false);
        }
    }
    Ok(())
}

/// Opens a new OS window on its own private, disposable ("ephemeral") workspace forked from
/// `source_workspace_id`'s current shape - or the hardcoded default shape if `None`, when there
/// is no named workspace to fork from yet (ephemeral per-window workspaces spec follow-up).
/// Windows are never made to share a live workspace id any more: each fork gets its own id, so
/// the existing per-id revision/event isolation keeps two windows from stomping each other's
/// tabs/panes/focus, with no cross-window merge logic required anywhere.
///
/// Deliberately does not reuse the `AppState`-managed `FileManagerService` through any
/// window-specific state: every window shares the same `Arc` already `.manage()`d once in
/// `run()`, so no new service wiring is needed here - only window lifecycle.
#[tauri::command]
pub(crate) async fn open_workspace_window(
    app: AppHandle,
    state: State<'_, AppState>,
    source_workspace_id: Option<Uuid>,
) -> Result<(), String> {
    let forked = state
        .service
        .fork_workspace(source_workspace_id)
        .await
        .map_err(|error| error.to_string())?;
    build_workspace_window(&app, forked.id).map_err(|error| error.to_string())?;
    Ok(())
}

/// Writes an ephemeral (per-window) workspace's current tabs/panes/layout back into
/// `target_workspace_id`, or - if omitted - the named workspace it was forked from, creating
/// one if it was seeded from the hardcoded default and has no source yet - identical in effect
/// to `WorkspaceService::resync` (ephemeral per-window workspaces spec follow-up). Returns the
/// target named workspace, not the ephemeral one.
#[tauri::command]
pub(crate) async fn resync_workspace(
    state: State<'_, AppState>,
    workspace_id: Uuid,
    target_workspace_id: Option<Uuid>,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .resync_workspace(workspace_id, target_workspace_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Lists every stored workspace as a lightweight summary, identical in shape
/// to `GET /api/v1/workspaces`.
#[tauri::command]
pub(crate) async fn list_workspaces(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceSummaryDto>, ApplicationErrorDto> {
    state
        .service
        .list_workspaces()
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Runs the workspace startup lifecycle, identical in shape to
/// `POST /api/v1/workspaces/start`: opens `workspace_id` if given, otherwise
/// the last-active workspace, otherwise creates a default.
#[tauri::command]
pub(crate) async fn start_workspace(
    state: State<'_, AppState>,
    workspace_id: Option<Uuid>,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .start_workspace(workspace_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Creates and persists a new workspace, identical in shape to
/// `POST /api/v1/workspaces`.
#[tauri::command]
pub(crate) async fn create_workspace(
    state: State<'_, AppState>,
    request: CreateWorkspaceRequestDto,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .create_workspace(request.name)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Loads a single workspace by id, identical in shape to
/// `GET /api/v1/workspaces/{workspaceId}`.
#[tauri::command]
pub(crate) async fn get_workspace(
    state: State<'_, AppState>,
    workspace_id: Uuid,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .get_workspace(workspace_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Deletes a workspace, identical in shape to
/// `DELETE /api/v1/workspaces/{workspaceId}`.
#[tauri::command]
pub(crate) async fn delete_workspace(
    state: State<'_, AppState>,
    workspace_id: Uuid,
    expected_revision: Option<u64>,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .delete_workspace(workspace_id, expected_revision)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Selects an existing workspace as the last-active workspace, identical in
/// shape to `POST /api/v1/workspaces/{workspaceId}/open`.
#[tauri::command]
pub(crate) async fn open_workspace(
    state: State<'_, AppState>,
    workspace_id: Uuid,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .open_workspace(workspace_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Applies a workspace command, identical in shape to
/// `POST /api/v1/workspaces/{workspaceId}/commands`.
#[tauri::command]
pub(crate) async fn apply_workspace_command(
    state: State<'_, AppState>,
    command: WorkspaceCommandDto,
) -> Result<WorkspaceDto, ApplicationErrorDto> {
    state
        .service
        .apply_workspace_command(command)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts an operation through the same service method as REST.
///
/// Must be `async` (not a plain blocking command): `Scheduler::submit` calls
/// `tokio::spawn` internally, which panics without a live Tokio reactor.
/// Tauri only guarantees that context for `async fn` commands.
#[tauri::command]
pub(crate) async fn start_operation(
    state: State<'_, AppState>,
    request: StartOperationRequestDto,
    idempotency_key: Option<String>,
) -> Result<OperationDto, ApplicationErrorDto> {
    state
        .service
        .start_operation(request, idempotency_key)
        .map_err(|e| e.into_dto(Uuid::new_v4()))
}
/// Lists operation snapshots through the shared service.
#[tauri::command]
pub(crate) fn list_operations(state: State<'_, AppState>) -> Vec<OperationDto> {
    state.service.list_operations()
}
/// Gets one operation through the shared service.
#[tauri::command]
pub(crate) fn get_operation(
    state: State<'_, AppState>,
    operation_id: Uuid,
) -> Result<OperationDto, ApplicationErrorDto> {
    state
        .service
        .get_operation(OperationId::from(operation_id))
        .map_err(|e| e.into_dto(Uuid::new_v4()))
}
macro_rules! operation_command {
    ($name:ident) => {
        #[doc = "Applies an operation lifecycle command through the shared service."]
        #[tauri::command]
        pub(crate) fn $name(
            state: State<'_, AppState>,
            operation_id: Uuid,
        ) -> Result<(), ApplicationErrorDto> {
            state
                .service
                .$name(OperationId::from(operation_id))
                .map_err(|e| e.into_dto(Uuid::new_v4()))
        }
    };
}
operation_command!(cancel_operation);
operation_command!(pause_operation);
operation_command!(resume_operation);
/// Starts an undo job through the same service method as REST.
#[tauri::command]
pub(crate) async fn undo_operation(
    state: State<'_, AppState>,
    operation_id: Uuid,
) -> Result<OperationDto, ApplicationErrorDto> {
    state
        .service
        .undo_operation(OperationId::from(operation_id))
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}
/// Resolves a pending operation conflict through the shared service.
#[tauri::command]
pub(crate) fn resolve_operation_conflict(
    state: State<'_, AppState>,
    operation_id: Uuid,
    request: ResolveOperationConflictRequestDto,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .resolve_operation_conflict(OperationId::from(operation_id), request)
        .map_err(|e| e.into_dto(Uuid::new_v4()))
}

/// Lists the registered actions through the same service method as REST.
#[tauri::command]
pub(crate) fn list_actions(state: State<'_, AppState>) -> Vec<ActionDescriptorDto> {
    state.service.list_actions()
}

/// Invokes a registered action through the same service method as REST.
///
/// Must be `async`: mutating actions delegate to `start_operation`, which
/// calls `Scheduler::submit` (`tokio::spawn`) and panics without a live
/// Tokio reactor outside an `async fn` command.
#[tauri::command]
pub(crate) async fn invoke_action<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    action_id: String,
    request: InvokeActionRequestDto,
    idempotency_key: Option<String>,
) -> Result<ActionResultDto, ApplicationErrorDto> {
    if action_id == "core.quickLook" {
        let service = Arc::clone(&state.service);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let result = service.invoke_action(action_id, request, idempotency_key);
            let _ = sender.send(result);
        })
        .map_err(|_| fm_application::ApplicationError::Internal.into_dto(Uuid::new_v4()))?;
        return receiver
            .await
            .map_err(|_| fm_application::ApplicationError::Internal.into_dto(Uuid::new_v4()))?
            .map_err(|error| error.into_dto(Uuid::new_v4()));
    }
    state
        .service
        .invoke_action(action_id, request, idempotency_key)
        .map_err(|e| e.into_dto(Uuid::new_v4()))
}

/// Lists plugins through the shared discovery service.
#[tauri::command]
pub(crate) fn list_plugins(state: State<'_, AppState>) -> Vec<PluginDescriptorDto> {
    state.service.list_plugins()
}

/// Persists plugin enablement through the shared service.
#[tauri::command]
pub(crate) fn enable_plugin(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .set_plugin_enabled(plugin_id, true)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Persists plugin disablement through the shared service.
#[tauri::command]
pub(crate) fn disable_plugin(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .set_plugin_enabled(plugin_id, false)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns one plugin's bounded diagnostic log through the shared service.
#[tauri::command]
pub(crate) fn get_plugin_logs(
    state: State<'_, AppState>,
    plugin_id: String,
) -> Result<Vec<PluginLogEntryDto>, ApplicationErrorDto> {
    state
        .service
        .plugin_logs(&plugin_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Serves one SVG asset from an enabled plugin's icon theme (task 0095), mirroring the HTTP
/// `GET /api/v1/plugins/{pluginId}/icon-theme/asset` route.
#[tauri::command]
pub(crate) fn get_plugin_icon_theme_asset(
    state: State<'_, AppState>,
    plugin_id: String,
    path: String,
) -> Result<String, ApplicationErrorDto> {
    state
        .service
        .plugin_icon_theme_asset(&plugin_id, &path)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts a cancellable recursive filename search through the same service
/// method as REST (task 0068).
///
/// Must be `async`: `SearchEngine::start` calls `tokio::task::spawn_blocking`
/// internally, which panics without a live Tokio reactor outside an
/// `async fn` command.
#[tauri::command]
pub(crate) async fn start_search(
    state: State<'_, AppState>,
    request: StartSearchRequestDto,
) -> Result<StartSearchResponseDto, ApplicationErrorDto> {
    state
        .service
        .start_search(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Cancels a running search through the shared service.
#[tauri::command]
pub(crate) fn cancel_search(
    state: State<'_, AppState>,
    search_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cancel_search(search_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts a cancellable directory comparison through the same service
/// method as REST (task 0075).
///
/// Must be `async`: `ComparisonEngine::start` calls `tokio::spawn`
/// internally, which panics without a live Tokio reactor outside an
/// `async fn` command.
#[tauri::command]
pub(crate) async fn start_comparison(
    state: State<'_, AppState>,
    request: StartComparisonRequestDto,
) -> Result<StartComparisonResponseDto, ApplicationErrorDto> {
    state
        .service
        .start_comparison(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns a bounded, optionally differences-only page of a comparison's
/// results, identical in shape to `GET /api/v1/comparisons/{comparisonId}`.
#[tauri::command]
pub(crate) fn get_comparison(
    state: State<'_, AppState>,
    comparison_id: Uuid,
    offset: Option<u64>,
    limit: Option<u16>,
    differences_only: Option<bool>,
) -> Result<ComparisonPageDto, ApplicationErrorDto> {
    state
        .service
        .get_comparison_page(
            comparison_id,
            offset.unwrap_or(0),
            limit.unwrap_or(200),
            differences_only.unwrap_or(false),
        )
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Cancels a running comparison through the shared service.
#[tauri::command]
pub(crate) fn cancel_comparison(
    state: State<'_, AppState>,
    comparison_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cancel_comparison(comparison_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Proposes a sync plan from a comparison's current results, identical in
/// shape to `POST /api/v1/comparisons/{comparisonId}/sync-plan`.
#[tauri::command]
pub(crate) fn generate_sync_plan(
    state: State<'_, AppState>,
    comparison_id: Uuid,
    request: GenerateSyncPlanRequestDto,
) -> Result<SyncPlanDto, ApplicationErrorDto> {
    state
        .service
        .generate_sync_plan(comparison_id, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Applies a (possibly user-edited) sync plan through the shared service,
/// identical in shape to
/// `POST /api/v1/comparisons/{comparisonId}/apply-sync-plan`.
#[tauri::command]
pub(crate) fn apply_sync_plan(
    state: State<'_, AppState>,
    comparison_id: Uuid,
    request: ApplySyncPlanRequestDto,
) -> Result<ApplySyncPlanResponseDto, ApplicationErrorDto> {
    state
        .service
        .apply_sync_plan(comparison_id, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts a cancellable checksum job through the same service method as
/// REST (task 0077).
///
/// Must be `async` for the same reason as [`start_comparison`]:
/// `ChecksumEngine::start_checksums` calls `tokio::spawn` internally.
#[tauri::command]
pub(crate) async fn start_checksums(
    state: State<'_, AppState>,
    request: StartChecksumRequestDto,
) -> Result<StartChecksumResponseDto, ApplicationErrorDto> {
    state
        .service
        .start_checksums(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns a bounded page of a checksum job's results, identical in shape to
/// `GET /api/v1/checksums/{jobId}`.
#[tauri::command]
pub(crate) fn get_checksums(
    state: State<'_, AppState>,
    job_id: Uuid,
    offset: Option<u64>,
    limit: Option<u16>,
) -> Result<ChecksumPageDto, ApplicationErrorDto> {
    state
        .service
        .get_checksum_page(job_id, offset.unwrap_or(0), limit.unwrap_or(200))
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Cancels a running checksum job through the shared service.
#[tauri::command]
pub(crate) fn cancel_checksums(
    state: State<'_, AppState>,
    job_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cancel_checksums(job_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Renders a job's results as checksum-file text, identical in shape to
/// `POST /api/v1/checksums/{jobId}/checksum-file`.
#[tauri::command]
pub(crate) fn render_checksum_file(
    state: State<'_, AppState>,
    job_id: Uuid,
    request: RenderChecksumFileRequestDto,
) -> Result<ChecksumFileDto, ApplicationErrorDto> {
    state
        .service
        .render_checksum_file(job_id, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Writes a job's results to a checksum file, identical in shape to
/// `POST /api/v1/checksums/{jobId}/save`.
///
/// Saving goes through the shared service and the provider's `WRITE` path
/// rather than a native Tauri save dialog, so the desktop and web hosts
/// create files by exactly the same audited route (spec §35, task 0077).
#[tauri::command]
pub(crate) async fn save_checksum_file(
    state: State<'_, AppState>,
    job_id: Uuid,
    request: SaveChecksumFileRequestDto,
) -> Result<SaveChecksumFileResponseDto, ApplicationErrorDto> {
    state
        .service
        .save_checksum_file(job_id, request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Verifies a job's digests against an existing checksum file, identical in
/// shape to `POST /api/v1/checksums/{jobId}/verify`.
#[tauri::command]
pub(crate) fn verify_checksum_file(
    state: State<'_, AppState>,
    job_id: Uuid,
    request: VerifyChecksumFileRequestDto,
) -> Result<VerificationReportDto, ApplicationErrorDto> {
    state
        .service
        .verify_checksum_file(job_id, request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Starts a cancellable duplicate scan through the shared service.
///
/// Must be `async` for the same reason as [`start_comparison`].
#[tauri::command]
pub(crate) async fn start_duplicate_scan(
    state: State<'_, AppState>,
    request: StartDuplicateScanRequestDto,
) -> Result<StartDuplicateScanResponseDto, ApplicationErrorDto> {
    state
        .service
        .start_duplicate_scan(request)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Returns a bounded page of a duplicate scan's grouped results, identical
/// in shape to `GET /api/v1/duplicate-scans/{scanId}`.
#[tauri::command]
pub(crate) fn get_duplicate_scan(
    state: State<'_, AppState>,
    scan_id: Uuid,
    offset: Option<u64>,
    limit: Option<u16>,
) -> Result<DuplicatePageDto, ApplicationErrorDto> {
    state
        .service
        .get_duplicate_page(scan_id, offset.unwrap_or(0), limit.unwrap_or(200))
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Cancels a running duplicate scan through the shared service.
#[tauri::command]
pub(crate) fn cancel_duplicate_scan(
    state: State<'_, AppState>,
    scan_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .cancel_duplicate_scan(scan_id)
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Lists every stored connection profile with its current runtime status,
/// identical in shape to `GET /api/v1/connections` (task 0103).
#[tauri::command]
pub(crate) async fn list_connections(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionDto>, ApplicationErrorDto> {
    state
        .service
        .list_connections()
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Creates and persists a new connection profile, identical in shape to
/// `POST /api/v1/connections`.
#[tauri::command]
pub(crate) async fn create_connection(
    state: State<'_, AppState>,
    request: CreateConnectionRequestDto,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .create_connection(request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Loads a single connection profile by id, identical in shape to
/// `GET /api/v1/connections/{connectionId}`.
#[tauri::command]
pub(crate) async fn get_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .get_connection(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Updates an existing connection profile, identical in shape to
/// `PUT /api/v1/connections/{connectionId}`.
#[tauri::command]
pub(crate) async fn update_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
    request: UpdateConnectionRequestDto,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .update_connection(connection_id, request)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Deletes a connection profile and its stored credential, if any, identical
/// in shape to `DELETE /api/v1/connections/{connectionId}`.
#[tauri::command]
pub(crate) async fn delete_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .delete_connection(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Attempts to connect, identical in shape to
/// `POST /api/v1/connections/{connectionId}/connect`. See
/// `fm_connections::ConnectionService`'s documentation for the honest scope
/// of this operation before task 0104/0106 register a real protocol dialer.
#[tauri::command]
pub(crate) async fn connect_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .connect_connection(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Marks a connection as disconnected, identical in shape to
/// `POST /api/v1/connections/{connectionId}/disconnect`.
#[tauri::command]
pub(crate) async fn disconnect_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .disconnect_connection(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Checks whether a connection's configuration and credential are currently
/// usable, without changing its tracked status, identical in shape to
/// `POST /api/v1/connections/{connectionId}/test`.
#[tauri::command]
pub(crate) async fn test_connection(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<ConnectionDto, ApplicationErrorDto> {
    state
        .service
        .test_connection(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Probes an SSH connection's currently presented host key without
/// authenticating, identical in shape to
/// `POST /api/v1/connections/{connectionId}/hostKey/probe` (task 0104, spec
/// §6.4).
#[tauri::command]
pub(crate) async fn probe_ssh_host_key(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<HostKeyProbeDto, ApplicationErrorDto> {
    state
        .service
        .probe_ssh_host_key(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Accepts (persists) a host-key fingerprint for an SSH connection,
/// identical in shape to
/// `POST /api/v1/connections/{connectionId}/hostKey/accept` (task 0104,
/// spec §6.4).
#[tauri::command]
pub(crate) async fn accept_ssh_host_key(
    state: State<'_, AppState>,
    connection_id: Uuid,
    request: AcceptSshHostKeyRequestDto,
) -> Result<(), ApplicationErrorDto> {
    state
        .service
        .accept_ssh_host_key(connection_id, request.fingerprint)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Begins a OneDrive OAuth authorization attempt for a saved connection,
/// identical in shape to
/// `POST /api/v1/connections/{connectionId}/onedrive/authorize` (task
/// 0110). Returns the Microsoft authorization URL for the frontend/host to
/// open in the system browser; this command never opens one itself.
#[tauri::command]
pub(crate) async fn begin_onedrive_authorization(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> Result<BeginOneDriveAuthorizationResponseDto, ApplicationErrorDto> {
    state
        .service
        .begin_onedrive_authorization(connection_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Polls a OneDrive authorization attempt's current status, identical in
/// shape to `GET /api/v1/onedrive/authorizations/{attemptId}` (task 0110).
#[tauri::command]
pub(crate) async fn get_onedrive_authorization_attempt(
    state: State<'_, AppState>,
    attempt_id: Uuid,
) -> Result<OneDriveAuthorizationAttemptDto, ApplicationErrorDto> {
    state
        .service
        .onedrive_authorization_attempt(attempt_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// Cancels a pending OneDrive authorization attempt, identical in shape to
/// `POST /api/v1/onedrive/authorizations/{attemptId}/cancel` (task 0110).
#[tauri::command]
pub(crate) async fn cancel_onedrive_authorization(
    state: State<'_, AppState>,
    attempt_id: Uuid,
) -> Result<OneDriveAuthorizationAttemptDto, ApplicationErrorDto> {
    state
        .service
        .cancel_onedrive_authorization(attempt_id)
        .await
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

/// One native menu bar click (task 0133), streamed to the frontend over
/// [`subscribe_native_menu_actions`]'s channel rather than the global
/// `emit`/`listen` event API, matching this app's existing IPC convention
/// (see `event_stream.rs`, `open_embedded_terminal`).
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeMenuActionEvent {
    id: String,
}

/// Subscribes the frontend to native menu action clicks (task 0133). Called
/// once at startup; a later call replaces the previous subscription.
#[tauri::command]
pub(crate) fn subscribe_native_menu_actions(
    registry: State<'_, NativeMenuActionChannel>,
    channel: Channel<NativeMenuActionEvent>,
) {
    registry.set(channel);
}

/// Builds the callback [`set_native_menu`] hands to
/// `FileManagerService::install_native_menu`: forwards each click's action
/// id over whichever channel is currently subscribed, or does nothing if
/// the frontend hasn't subscribed yet - installing a menu before that
/// happens still succeeds, it just has nowhere to report clicks to yet.
fn native_menu_action_callback(
    channel: Option<Channel<NativeMenuActionEvent>>,
) -> Arc<dyn Fn(String) + Send + Sync> {
    match channel {
        Some(channel) => Arc::new(move |id: String| {
            let _ = channel.send(NativeMenuActionEvent { id });
        }),
        None => Arc::new(|_id: String| {}),
    }
}

/// Initializes the platform-specific window handle for native integrations
/// (task 0131, Windows). Called by the frontend during app initialization
/// to set up the native menu bar hook point.
#[tauri::command]
#[cfg(target_os = "windows")]
pub(crate) fn initialize_window_handle<R: Runtime>(
    state: State<'_, AppState>,
    window: Window<R>,
) -> Result<(), ApplicationErrorDto> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::HWND;

    let adapter = state.service.platform_adapter();
    if let Ok(window_handle) = window.window_handle()
        && let RawWindowHandle::Win32(handle) = window_handle.as_raw()
    {
        // Downcast the adapter to Windows adapter using Any trait
        let adapter_any: &dyn Any = adapter.as_ref();
        if let Some(win_adapter) =
            adapter_any.downcast_ref::<fm_platform_windows::WindowsPlatformAdapter>()
        {
            // SAFETY: handle.hwnd is a valid window handle provided by Tauri.
            // Convert the opaque NonZeroIsize from raw_window_handle to windows-sys HWND.
            let hwnd = handle.hwnd.get() as HWND;
            win_adapter.set_window_handle(hwnd);
        }
    }
    Ok(())
}

/// No-op on non-Windows platforms.
#[tauri::command]
#[cfg(not(target_os = "windows"))]
pub(crate) fn initialize_window_handle<R: Runtime>(
    _state: State<'_, AppState>,
    _window: Window<R>,
) -> Result<(), ApplicationErrorDto> {
    Ok(())
}

/// Installs (or replaces) the native menu bar (task 0133) from `spec`, built
/// by the frontend from the action registry (task 0049). Native menu APIs
/// require the main thread, so this follows the same
/// `run_on_main_thread`-plus-`oneshot` pattern as [`start_native_drag`].
#[tauri::command]
pub(crate) async fn set_native_menu<R: Runtime>(
    state: State<'_, AppState>,
    registry: State<'_, NativeMenuActionChannel>,
    app: AppHandle<R>,
    spec: fm_domain::NativeMenuSpec,
) -> Result<(), ApplicationErrorDto> {
    let service = Arc::clone(&state.service);
    let on_action = native_menu_action_callback(registry.get());
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = service.install_native_menu(&spec, on_action);
        let _ = sender.send(result);
    })
    .map_err(|_| fm_application::ApplicationError::Internal.into_dto(Uuid::new_v4()))?;
    receiver
        .await
        .map_err(|_| fm_application::ApplicationError::Internal.into_dto(Uuid::new_v4()))?
        .map_err(|error| error.into_dto(Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tauri::ipc::InvokeResponseBody;

    use super::*;

    #[test]
    fn disk_usage_dispatch_does_not_require_an_entered_tokio_reactor() {
        let root = tempfile::tempdir().expect("create scan root");
        let service = Arc::new(fm_application::FileManagerService::new(
            fm_transport_dto::RuntimeKindDto::Tauri,
            root.path().join("workspaces"),
            root.path().join("settings"),
        ));
        let request = ScanDiskUsageRequestDto {
            workspace_id: Uuid::new_v4(),
            scan_id: Uuid::new_v4(),
            location: fm_domain::Location::from_native_path(root.path())
                .expect("map scan root")
                .into(),
            expand_root: false,
        };

        tauri::async_runtime::block_on(dispatch_disk_usage_scan(service, request))
            .expect("disk-usage task must complete");
    }

    #[test]
    fn workspace_window_label_is_unique_per_call_and_reduces_to_the_same_canonical_form() {
        let id = Uuid::new_v4();
        let first = workspace_window_label(id);
        let second = workspace_window_label(id);

        assert_ne!(
            first, second,
            "each call must open another window, never dedup"
        );
        assert_eq!(
            canonical_workspace_window_label(&first),
            canonical_workspace_window_label(&second),
            "every window opened for the same workspace must share one remembered frame"
        );
        assert_eq!(
            canonical_workspace_window_label(&first),
            format!("workspace-{id}"),
        );
    }

    #[test]
    fn canonical_workspace_window_label_passes_through_a_label_with_no_nonce() {
        assert_eq!(canonical_workspace_window_label("main"), "main");
    }

    fn summary(ephemeral: bool) -> WorkspaceSummaryDto {
        WorkspaceSummaryDto {
            id: Uuid::new_v4(),
            name: "Workspace".to_owned(),
            updated_at: chrono::Utc::now(),
            revision: 0,
            ephemeral,
        }
    }

    #[test]
    fn ephemeral_workspace_ids_keeps_only_ephemeral_summaries() {
        let named = summary(false);
        let ephemeral_one = summary(true);
        let ephemeral_two = summary(true);
        let summaries = vec![named.clone(), ephemeral_one.clone(), ephemeral_two.clone()];

        let restored = ephemeral_workspace_ids(&summaries);

        assert_eq!(restored, vec![ephemeral_one.id, ephemeral_two.id]);
        assert!(!restored.contains(&named.id));
    }

    #[test]
    fn ephemeral_workspace_ids_is_empty_when_every_workspace_is_named() {
        let summaries = vec![summary(false), summary(false)];
        assert!(ephemeral_workspace_ids(&summaries).is_empty());
    }

    #[test]
    fn native_menu_action_callback_forwards_the_id_over_the_subscribed_channel() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_by_channel = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            let json = match body {
                InvokeResponseBody::Json(json) => json,
                InvokeResponseBody::Raw(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            };
            received_by_channel
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(json);
            Ok(())
        });

        let callback = native_menu_action_callback(Some(channel));
        callback("core.preferences".to_owned());

        assert_eq!(
            received.lock().expect("channel lock").as_slice(),
            [r#"{"id":"core.preferences"}"#.to_owned()]
        );
    }

    #[test]
    fn native_menu_action_callback_is_a_no_op_without_a_subscription() {
        // Installing a menu before the frontend subscribes must still succeed silently rather
        // than panicking or erroring - there is simply nowhere to report clicks to yet.
        let callback = native_menu_action_callback(None);
        callback("core.copy".to_owned());
    }

    #[test]
    fn native_drag_paths_require_a_non_empty_selection() {
        assert!(matches!(
            native_drag_paths(Vec::new()),
            Err(NativeDragError::EmptySelection)
        ));
    }

    #[test]
    fn native_drag_paths_reject_non_local_locations() {
        let error = native_drag_paths(vec![LocationDto {
            provider_id: "archive".to_owned(),
            uri: "archive://local/example.zip!/report.txt".to_owned(),
        }])
        .expect_err("archive entries are not native OS files");

        assert!(matches!(error, NativeDragError::InvalidLocation { .. }));
    }

    #[test]
    fn platform_context_menu_paths_require_a_non_empty_local_selection() {
        assert!(matches!(
            platform_context_menu_paths(Vec::new()),
            Err(PlatformContextMenuError::EmptySelection)
        ));
        assert!(matches!(
            platform_context_menu_paths(vec![LocationDto {
                provider_id: "archive".to_owned(),
                uri: "archive://local/example.zip!/report.txt".to_owned(),
            }]),
            Err(PlatformContextMenuError::InvalidLocation { .. })
        ));
    }

    #[test]
    fn native_drag_paths_preserve_awkward_native_paths() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("quotes ' and café.txt");
        let location = fm_domain::Location::from_native_path(&path).expect("local location");

        assert_eq!(
            native_drag_paths(vec![location.into()]).expect("native path"),
            vec![path]
        );
    }

    #[test]
    fn native_drag_locations_preserve_awkward_native_paths() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("quotes ' and café.txt");

        let locations = native_drag_locations(vec![path.clone()]).expect("local location");
        let round_trip = fm_domain::Location::from(
            locations
                .into_iter()
                .next()
                .expect("one converted location"),
        )
        .to_native_path()
        .expect("native path");

        assert_eq!(round_trip, path);
    }
}
