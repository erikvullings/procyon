//! Maps action ids to platform-adapter operations (task 0061), platform-adapter failures to
//! user-readable application errors, detects the host OS (spec §21), discovers OS-managed
//! locations, and derives the reported runtime capabilities.
//!
//! Split out of the `FileManagerService` facade (task 0119). `platform_action_kind`,
//! `map_platform_error`, `map_file_icon_error`, and `detect_platform` are pure functions with no
//! facade-state dependency; `discover_system_locations` and `runtime_capabilities_dto` take their
//! one or two dependencies (the platform adapter, the runtime kind) as parameters instead of
//! reading `&self`, for the same reason.

use std::path::PathBuf;
use std::sync::Arc;

use fm_domain::{ActionId, Location};
use fm_platform::{FinderTag, FinderTagColor, PlatformAdapter, PlatformCapabilities};
use fm_transport_dto::{
    FinderTagColorDto, FinderTagDto, FinderTagsDto, PlatformKindDto, RuntimeCapabilitiesDto,
    RuntimeKindDto, SpotlightCommentDto, SystemLocationDto, SystemLocationKindDto,
    VolumeCapacityDto, VolumeDto,
};

use crate::error::ApplicationError;

/// Removes adapter capabilities that require a specific host runtime.
pub(crate) fn action_capabilities_for_runtime(
    runtime: RuntimeKindDto,
    mut capabilities: PlatformCapabilities,
) -> PlatformCapabilities {
    if runtime != RuntimeKindDto::Tauri {
        capabilities.remove(PlatformCapabilities::QUICK_LOOK);
    }
    capabilities
}

/// Which platform adapter method an action dispatches to (task 0061).
/// `core.view` shares [`PlatformActionKind::Open`] with `core.open`: it is
/// an explicit stopgap for a real viewer (task 0088). `core.openWith`
/// dispatches to [`PlatformActionKind::OpenWithChooser`], a native "choose
/// application" dialog (see `core_actions`'s doc comment in `action.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformActionKind {
    Open,
    OpenWithChooser,
    QuickLook,
    Reveal,
    OpenTerminal,
    EditInTextEditor,
}

/// Maps an action id to the platform adapter operation it dispatches to
/// (task 0061), or `None` for actions with no platform-adapter effect.
pub(crate) fn platform_action_kind(id: &ActionId) -> Option<PlatformActionKind> {
    match id.as_str() {
        "core.open" | "core.view" => Some(PlatformActionKind::Open),
        "core.openWith" => Some(PlatformActionKind::OpenWithChooser),
        "core.quickLook" => Some(PlatformActionKind::QuickLook),
        "core.edit" => Some(PlatformActionKind::EditInTextEditor),
        "core.revealInSystemFileManager" => Some(PlatformActionKind::Reveal),
        "core.openTerminal" => Some(PlatformActionKind::OpenTerminal),
        _ => None,
    }
}

/// Maps a platform adapter failure to a user-readable application error
/// (task 0061). An adapter reporting [`fm_platform::PlatformError::Unsupported`]
/// despite the registry's capability check (e.g. a race between capability
/// detection and invocation) is reported the same way as any other
/// unavailable action; any other failure keeps its sanitized, user-readable
/// message rather than a silent no-op or a generic "internal error".
pub(crate) fn map_platform_error(
    action_id: &ActionId,
    error: fm_platform::PlatformError,
) -> ApplicationError {
    match error {
        fm_platform::PlatformError::Unsupported { .. } => {
            ApplicationError::ActionUnavailable(action_id.clone())
        }
        other => ApplicationError::PlatformOperationFailed(other.to_string()),
    }
}

/// Missing native icon support is an expected fallback condition rather than
/// a host failure; genuine platform errors remain visible as a 502/IPC error.
pub(crate) fn map_file_icon_error(error: fm_platform::PlatformError) -> ApplicationError {
    match error {
        fm_platform::PlatformError::Unsupported { .. } => ApplicationError::NotFound,
        other => ApplicationError::PlatformOperationFailed(other.to_string()),
    }
}

/// Missing Finder-tag/extended-attribute *read* support is an expected
/// fallback condition, exactly like [`map_file_icon_error`]: the frontend's
/// lazy per-entry loader treats a 404 as "no tags/no comment" and falls back
/// silently, not as a failure. A vanished entry ([`fm_platform::PlatformError::NotFound`])
/// gets the same harmless treatment - it may simply have been removed while
/// a listing was still on screen.
pub(crate) fn map_extended_attribute_read_error(
    error: fm_platform::PlatformError,
) -> ApplicationError {
    match error {
        fm_platform::PlatformError::Unsupported { .. }
        | fm_platform::PlatformError::NotFound { .. } => ApplicationError::NotFound,
        other => ApplicationError::PlatformOperationFailed(other.to_string()),
    }
}

/// A Finder-tag/extended-attribute *write* is only ever offered once the
/// frontend has already checked the capability, exactly like
/// [`map_native_menu_error`] - `Unsupported` here is a genuine, surfaceable
/// failure rather than a silent fallback. A vanished entry still gets its
/// own clean [`ApplicationError::NotFound`] rather than a vague 502.
pub(crate) fn map_extended_attribute_write_error(
    error: fm_platform::PlatformError,
) -> ApplicationError {
    match error {
        fm_platform::PlatformError::NotFound { .. } => ApplicationError::NotFound,
        other => ApplicationError::PlatformOperationFailed(other.to_string()),
    }
}

/// Converts a platform-adapter Finder tag to its wire DTO.
pub(crate) fn finder_tag_to_dto(tag: FinderTag) -> FinderTagDto {
    FinderTagDto {
        name: tag.name,
        color: finder_tag_color_to_dto(tag.color),
    }
}

/// Converts a wire Finder tag DTO to its platform-adapter type.
pub(crate) fn finder_tag_from_dto(dto: FinderTagDto) -> FinderTag {
    FinderTag {
        name: dto.name,
        color: finder_tag_color_from_dto(dto.color),
    }
}

fn finder_tag_color_to_dto(color: FinderTagColor) -> FinderTagColorDto {
    match color {
        FinderTagColor::None => FinderTagColorDto::None,
        FinderTagColor::Gray => FinderTagColorDto::Gray,
        FinderTagColor::Green => FinderTagColorDto::Green,
        FinderTagColor::Purple => FinderTagColorDto::Purple,
        FinderTagColor::Blue => FinderTagColorDto::Blue,
        FinderTagColor::Yellow => FinderTagColorDto::Yellow,
        FinderTagColor::Red => FinderTagColorDto::Red,
        FinderTagColor::Orange => FinderTagColorDto::Orange,
    }
}

fn finder_tag_color_from_dto(color: FinderTagColorDto) -> FinderTagColor {
    match color {
        FinderTagColorDto::None => FinderTagColor::None,
        FinderTagColorDto::Gray => FinderTagColor::Gray,
        FinderTagColorDto::Green => FinderTagColor::Green,
        FinderTagColorDto::Purple => FinderTagColor::Purple,
        FinderTagColorDto::Blue => FinderTagColor::Blue,
        FinderTagColorDto::Yellow => FinderTagColor::Yellow,
        FinderTagColorDto::Red => FinderTagColor::Red,
        FinderTagColorDto::Orange => FinderTagColor::Orange,
    }
}

/// Maps a native-menu install failure to a user-readable application error
/// (task 0133). Unlike [`map_file_icon_error`], `Unsupported` has no silent
/// fallback interpretation here - the frontend only calls `setNativeMenu`
/// after checking `runtimeCapabilities().nativeMenus`, so seeing it anyway
/// is a genuine, surfaceable failure rather than an expected gap.
pub(crate) fn map_native_menu_error(error: fm_platform::PlatformError) -> ApplicationError {
    ApplicationError::PlatformOperationFailed(error.to_string())
}

/// Detects the host operating system from the compiled target (spec §21).
pub(crate) fn detect_platform() -> PlatformKindDto {
    match std::env::consts::OS {
        "macos" => PlatformKindDto::Macos,
        "windows" => PlatformKindDto::Windows,
        "linux" => PlatformKindDto::Linux,
        _ => PlatformKindDto::Unknown,
    }
}

/// Discovers OS-managed locations and maps their native paths to the existing local provider.
pub(crate) async fn discover_system_locations(
    platform: Arc<dyn PlatformAdapter>,
) -> Result<Vec<SystemLocationDto>, ApplicationError> {
    let discovered = tokio::task::spawn_blocking(move || platform.system_locations())
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
    discovered
        .into_iter()
        .map(|location| {
            let local = Location::from_native_path(&location.path)
                .map_err(|_| ApplicationError::Internal)?;
            Ok(SystemLocationDto {
                name: location.name,
                kind: match location.kind {
                    fm_platform::SystemLocationKind::Cloud => SystemLocationKindDto::Cloud,
                    fm_platform::SystemLocationKind::Network => SystemLocationKindDto::Network,
                },
                location: local.into(),
                provider_hint: location.provider_hint,
                protocol: location.protocol,
                server: location.server,
                share: location.share,
                read_only: location.read_only,
            })
        })
        .collect()
}

/// Discovers currently mounted local/removable/disk-image volumes (task
/// 0144) and maps their native paths to the existing local provider,
/// mirroring [`discover_system_locations`]. Reuses
/// [`PlatformAdapter::mounted_volumes`] rather than adding a second
/// discovery path. Hosts/adapters without
/// [`PlatformCapabilities::MOUNTED_VOLUMES`] report an empty list, not an
/// error, exactly like [`runtime_capabilities_dto`] gates other optional
/// features.
pub(crate) async fn discover_volumes(
    platform: Arc<dyn PlatformAdapter>,
) -> Result<Vec<VolumeDto>, ApplicationError> {
    if !platform
        .capabilities()
        .contains(PlatformCapabilities::MOUNTED_VOLUMES)
    {
        return Ok(Vec::new());
    }
    let discovered = tokio::task::spawn_blocking(move || platform.mounted_volumes())
        .await
        .map_err(|_| ApplicationError::Internal)?
        .map_err(|error| ApplicationError::PlatformOperationFailed(error.to_string()))?;
    discovered
        .into_iter()
        .map(|volume| {
            let local = Location::from_native_path(&volume.mount_point)
                .map_err(|_| ApplicationError::Internal)?;
            Ok(VolumeDto {
                name: volume.name,
                location: local.into(),
            })
        })
        .collect()
}

/// Derives the reported runtime capabilities from the platform adapter's capability bitset
/// (spec §21).
pub(crate) fn runtime_capabilities_dto(
    runtime: RuntimeKindDto,
    capabilities: PlatformCapabilities,
) -> RuntimeCapabilitiesDto {
    RuntimeCapabilitiesDto {
        runtime,
        platform: detect_platform(),
        native_menus: capabilities.contains(PlatformCapabilities::NATIVE_MENUS),
        platform_context_menu: runtime == RuntimeKindDto::Tauri
            && capabilities.contains(PlatformCapabilities::PLATFORM_CONTEXT_MENU),
        native_file_icons: capabilities.contains(PlatformCapabilities::FILE_ICONS),
        native_thumbnails: capabilities.contains(PlatformCapabilities::THUMBNAILS),
        native_drag_out: capabilities.contains(PlatformCapabilities::NATIVE_DRAG_OUT),
        system_trash: capabilities.contains(PlatformCapabilities::TRASH),
        reveal_in_system_file_manager: capabilities
            .contains(PlatformCapabilities::REVEAL_IN_FILE_MANAGER),
        open_terminal: capabilities.contains(PlatformCapabilities::OPEN_TERMINAL),
        // Basic text/data clipboard access works through the browser
        // Clipboard API without any native bridge, on every host. This is
        // deliberately not derived from `PlatformCapabilities::
        // CLIPBOARD_FILE_REFERENCES`, which instead gates pasting actual
        // file path lists (e.g. from Finder/Explorer) - a capability
        // `RuntimeCapabilitiesDto` has no field for yet. A future task
        // adding file-reference paste support should add one rather than
        // overload this flag.
        clipboard: true,
        plugins: true,
        server_administration: false,
        extended_attributes: capabilities.contains(PlatformCapabilities::EXTENDED_ATTRIBUTES),
        finder_tags: capabilities.contains(PlatformCapabilities::FINDER_TAGS),
    }
}

/// Reports the backing volume's total/available capacity for a location
/// (task 0096), or `None` when the platform adapter doesn't support it,
/// the location isn't a local filesystem path, or the native call fails.
pub(crate) async fn volume_capacity(
    platform: &Arc<dyn PlatformAdapter>,
    location: &Location,
) -> Option<VolumeCapacityDto> {
    if !platform
        .capabilities()
        .contains(PlatformCapabilities::VOLUME_CAPACITY)
    {
        return None;
    }
    let path = location.to_native_path().ok()?;
    let platform = Arc::clone(platform);
    let capacity = tokio::task::spawn_blocking(move || platform.volume_capacity(&path))
        .await
        .ok()?
        .ok()?;
    Some(VolumeCapacityDto {
        total_bytes: capacity.total_bytes,
        available_bytes: capacity.available_bytes,
    })
}

/// Parses a wire location URI into a native filesystem path - shared by every
/// extended-attribute/icon accessor below, each of which takes a `uri` and
/// forwards a native [`std::path::Path`] straight to the platform adapter.
pub(crate) fn native_path_for(uri: &str) -> Result<PathBuf, ApplicationError> {
    Location::parse(uri)
        .and_then(|location| location.to_native_path())
        .map_err(|error| ApplicationError::InvalidRequest(format!("invalid `uri`: {error}")))
}

/// Returns the active platform adapter's PNG icon for one sample entry. The
/// adapter owns extension-level caching; this deliberately adds no second
/// cache layer (task 0091).
pub(crate) fn read_file_icon(
    platform: &Arc<dyn PlatformAdapter>,
    uri: &str,
) -> Result<Vec<u8>, ApplicationError> {
    let path = native_path_for(uri)?;
    platform.file_icon(&path).map_err(map_file_icon_error)
}

/// Reads an entry's Finder tags (task 0136). Missing capability support or a
/// vanished entry are both reported as [`ApplicationError::NotFound`] (see
/// [`map_extended_attribute_read_error`]) so a lazy per-entry frontend loader
/// can treat them as "no tags" rather than a failure.
pub(crate) fn read_finder_tags(
    platform: &Arc<dyn PlatformAdapter>,
    uri: &str,
) -> Result<FinderTagsDto, ApplicationError> {
    let path = native_path_for(uri)?;
    let tags = platform
        .finder_tags(&path)
        .map_err(map_extended_attribute_read_error)?;
    Ok(FinderTagsDto {
        tags: tags.into_iter().map(finder_tag_to_dto).collect(),
    })
}

/// Replaces an entry's complete set of Finder tags (task 0136), returning the
/// persisted set back (mirrors the settings service's get/put symmetry).
pub(crate) fn write_finder_tags(
    platform: &Arc<dyn PlatformAdapter>,
    uri: &str,
    request: FinderTagsDto,
) -> Result<FinderTagsDto, ApplicationError> {
    let path = native_path_for(uri)?;
    let tags: Vec<_> = request.tags.into_iter().map(finder_tag_from_dto).collect();
    platform
        .set_finder_tags(&path, &tags)
        .map_err(map_extended_attribute_write_error)?;
    Ok(FinderTagsDto {
        tags: tags.into_iter().map(finder_tag_to_dto).collect(),
    })
}

/// Reads an entry's Spotlight comment (task 0136). Same graceful
/// missing-capability/missing-entry handling as [`read_finder_tags`].
pub(crate) fn read_spotlight_comment(
    platform: &Arc<dyn PlatformAdapter>,
    uri: &str,
) -> Result<SpotlightCommentDto, ApplicationError> {
    let path = native_path_for(uri)?;
    let comment = platform
        .spotlight_comment(&path)
        .map_err(map_extended_attribute_read_error)?;
    Ok(SpotlightCommentDto { comment })
}

/// Sets or clears an entry's Spotlight comment (task 0136), returning the
/// persisted value back.
pub(crate) fn write_spotlight_comment(
    platform: &Arc<dyn PlatformAdapter>,
    uri: &str,
    request: SpotlightCommentDto,
) -> Result<SpotlightCommentDto, ApplicationError> {
    let path = native_path_for(uri)?;
    platform
        .set_spotlight_comment(&path, request.comment.as_deref())
        .map_err(map_extended_attribute_write_error)?;
    Ok(request)
}

/// Installs the native menu bar from `spec` (task 0133), a thin passthrough
/// to the platform adapter. `on_action` is invoked whenever the user clicks a
/// `NativeMenuItem::Action` item, with that item's action-registry id, so the
/// caller can dispatch it exactly like an `invoke_action` call from the
/// keyboard.
pub(crate) fn install_native_menu(
    platform: &Arc<dyn PlatformAdapter>,
    spec: &fm_domain::NativeMenuSpec,
    on_action: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<(), ApplicationError> {
    platform
        .install_native_menu(spec, on_action)
        .map_err(map_native_menu_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finder_tag_round_trips_through_its_dto_for_every_color() {
        for color in [
            FinderTagColor::None,
            FinderTagColor::Gray,
            FinderTagColor::Green,
            FinderTagColor::Purple,
            FinderTagColor::Blue,
            FinderTagColor::Yellow,
            FinderTagColor::Red,
            FinderTagColor::Orange,
        ] {
            let tag = FinderTag {
                name: "Work".to_owned(),
                color,
            };
            let round_tripped = finder_tag_from_dto(finder_tag_to_dto(tag.clone()));
            assert_eq!(round_tripped, tag);
        }
    }

    #[test]
    fn extended_attribute_read_error_treats_unsupported_and_not_found_as_not_found() {
        assert_eq!(
            map_extended_attribute_read_error(fm_platform::PlatformError::Unsupported {
                capability: PlatformCapabilities::FINDER_TAGS,
            }),
            ApplicationError::NotFound
        );
        assert_eq!(
            map_extended_attribute_read_error(fm_platform::PlatformError::NotFound {
                path: "/tmp/gone".to_owned(),
            }),
            ApplicationError::NotFound
        );
        assert_eq!(
            map_extended_attribute_read_error(fm_platform::PlatformError::Io {
                message: "boom".to_owned(),
            }),
            ApplicationError::PlatformOperationFailed("platform operation failed: boom".to_owned())
        );
    }

    #[test]
    fn extended_attribute_write_error_surfaces_unsupported_but_cleans_up_not_found() {
        assert!(matches!(
            map_extended_attribute_write_error(fm_platform::PlatformError::Unsupported {
                capability: PlatformCapabilities::FINDER_TAGS,
            }),
            ApplicationError::PlatformOperationFailed(_)
        ));
        assert_eq!(
            map_extended_attribute_write_error(fm_platform::PlatformError::NotFound {
                path: "/tmp/gone".to_owned(),
            }),
            ApplicationError::NotFound
        );
    }

    #[test]
    fn runtime_capabilities_dto_derives_extended_attribute_flags_from_the_bitset() {
        let dto = runtime_capabilities_dto(
            RuntimeKindDto::BrowserServer,
            PlatformCapabilities::FINDER_TAGS,
        );
        assert!(dto.finder_tags);
        assert!(!dto.extended_attributes);

        let dto = runtime_capabilities_dto(
            RuntimeKindDto::BrowserServer,
            PlatformCapabilities::EXTENDED_ATTRIBUTES,
        );
        assert!(dto.extended_attributes);
        assert!(!dto.finder_tags);
    }

    #[test]
    fn platform_context_menu_is_available_only_to_the_desktop_runtime() {
        let capability = PlatformCapabilities::PLATFORM_CONTEXT_MENU;

        let desktop = runtime_capabilities_dto(RuntimeKindDto::Tauri, capability);
        assert!(desktop.platform_context_menu);

        let browser = runtime_capabilities_dto(RuntimeKindDto::BrowserServer, capability);
        assert!(!browser.platform_context_menu);

        let mock = runtime_capabilities_dto(RuntimeKindDto::Mock, capability);
        assert!(!mock.platform_context_menu);
    }

    #[test]
    fn quick_look_is_available_only_to_the_desktop_runtime() {
        let capabilities = PlatformCapabilities::QUICK_LOOK;

        assert!(
            action_capabilities_for_runtime(RuntimeKindDto::Tauri, capabilities)
                .contains(PlatformCapabilities::QUICK_LOOK)
        );
        for runtime in [RuntimeKindDto::BrowserServer, RuntimeKindDto::Mock] {
            assert!(
                !action_capabilities_for_runtime(runtime, capabilities)
                    .contains(PlatformCapabilities::QUICK_LOOK)
            );
        }
    }

    fn fallback_platform() -> Arc<dyn PlatformAdapter> {
        Arc::new(fm_platform::FallbackPlatformAdapter)
    }

    #[test]
    fn native_path_for_rejects_an_unparsable_uri() {
        assert!(matches!(
            native_path_for("not a valid uri"),
            Err(ApplicationError::InvalidRequest(_))
        ));
    }

    #[test]
    fn read_file_icon_reports_not_found_when_the_adapter_has_no_icon_support() {
        let platform = fallback_platform();
        assert_eq!(
            read_file_icon(&platform, "file:///tmp/example.txt"),
            Err(ApplicationError::NotFound)
        );
    }

    #[test]
    fn finder_tag_accessors_report_not_found_when_the_adapter_lacks_support() {
        let platform = fallback_platform();
        assert_eq!(
            read_finder_tags(&platform, "file:///tmp/example.txt"),
            Err(ApplicationError::NotFound)
        );
        assert!(matches!(
            write_finder_tags(
                &platform,
                "file:///tmp/example.txt",
                FinderTagsDto { tags: Vec::new() },
            ),
            Err(ApplicationError::PlatformOperationFailed(_))
        ));
    }

    #[test]
    fn spotlight_comment_accessors_report_not_found_when_the_adapter_lacks_support() {
        let platform = fallback_platform();
        assert_eq!(
            read_spotlight_comment(&platform, "file:///tmp/example.txt"),
            Err(ApplicationError::NotFound)
        );
        assert!(matches!(
            write_spotlight_comment(
                &platform,
                "file:///tmp/example.txt",
                SpotlightCommentDto { comment: None },
            ),
            Err(ApplicationError::PlatformOperationFailed(_))
        ));
    }

    #[test]
    fn extended_attribute_accessors_reject_an_unparsable_uri_before_touching_the_adapter() {
        let platform = fallback_platform();
        assert!(matches!(
            read_finder_tags(&platform, "not a valid uri"),
            Err(ApplicationError::InvalidRequest(_))
        ));
        assert!(matches!(
            read_spotlight_comment(&platform, "not a valid uri"),
            Err(ApplicationError::InvalidRequest(_))
        ));
    }

    #[test]
    fn install_native_menu_surfaces_an_adapter_failure() {
        let platform = fallback_platform();
        let spec = fm_domain::NativeMenuSpec { menus: Vec::new() };
        assert!(matches!(
            install_native_menu(&platform, &spec, Arc::new(|_| {})),
            Err(ApplicationError::PlatformOperationFailed(_))
        ));
    }
}
