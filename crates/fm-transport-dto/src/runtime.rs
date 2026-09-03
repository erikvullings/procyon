//! Runtime capability negotiation (spec §21, `GET /api/v1/runtime`).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which host is serving the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeKindDto {
    /// The Axum host, reached over HTTP from a browser.
    BrowserServer,
    /// The Tauri desktop host.
    Tauri,
    /// An in-memory mock host used for frontend development and tests.
    Mock,
}

/// The host operating system, when known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlatformKindDto {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
    /// A platform the backend could not identify.
    Unknown,
}

/// Capabilities the current runtime and platform support, so the frontend can
/// respond to capabilities rather than detecting operating systems directly
/// (spec §21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[schema(example = json!({
    "runtime": "browserServer",
    "platform": "macos",
    "nativeMenus": false,
    "platformContextMenu": false,
    "nativeFileIcons": false,
    "nativeThumbnails": false,
    "nativeDragOut": false,
    "systemTrash": false,
    "revealInSystemFileManager": false,
    "openTerminal": false,
    "clipboard": true,
    "plugins": false,
    "serverAdministration": false,
    "extendedAttributes": false,
    "finderTags": false,
    "finderAliases": false
}))]
pub struct RuntimeCapabilitiesDto {
    /// Which host is serving the application.
    pub runtime: RuntimeKindDto,
    /// The host operating system, when known.
    pub platform: PlatformKindDto,
    /// Whether native OS menus are available.
    pub native_menus: bool,
    /// Whether the desktop host can expose the OS Services/Send To submenu.
    pub platform_context_menu: bool,
    /// Whether native file icons can be fetched.
    pub native_file_icons: bool,
    /// Whether native thumbnails can be fetched.
    pub native_thumbnails: bool,
    /// Whether dragging entries out to the OS is supported.
    pub native_drag_out: bool,
    /// Whether deleting sends to the system trash/recycle bin.
    pub system_trash: bool,
    /// Whether entries can be revealed in the system file manager.
    pub reveal_in_system_file_manager: bool,
    /// Whether opening a terminal at a location is supported.
    pub open_terminal: bool,
    /// Whether OS clipboard integration is available.
    pub clipboard: bool,
    /// Whether plugins can be loaded.
    pub plugins: bool,
    /// Whether server administration endpoints are available.
    pub server_administration: bool,
    /// Whether generic extended attributes (currently: the Spotlight
    /// "Finder comment") can be read/written (task 0136).
    pub extended_attributes: bool,
    /// Whether Finder tags can be read/written (task 0136).
    pub finder_tags: bool,
    /// Whether macOS Finder alias files can be resolved to their targets.
    pub finder_aliases: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RuntimeCapabilitiesDto {
        RuntimeCapabilitiesDto {
            runtime: RuntimeKindDto::BrowserServer,
            platform: PlatformKindDto::Macos,
            native_menus: false,
            platform_context_menu: false,
            native_file_icons: false,
            native_thumbnails: false,
            native_drag_out: false,
            system_trash: false,
            reveal_in_system_file_manager: false,
            open_terminal: false,
            clipboard: true,
            plugins: false,
            server_administration: false,
            extended_attributes: false,
            finder_tags: false,
            finder_aliases: false,
        }
    }

    #[test]
    fn runtime_capabilities_dto_round_trips_through_serde_json() {
        let dto = sample();
        let json = serde_json::to_string(&dto).expect("serialization must succeed");
        let parsed: RuntimeCapabilitiesDto =
            serde_json::from_str(&json).expect("deserialization must succeed");
        assert_eq!(dto, parsed);
    }

    #[test]
    fn runtime_capabilities_dto_uses_camel_case_field_names() {
        let json = serde_json::to_string(&sample()).expect("serialization must succeed");
        for field in [
            "\"nativeMenus\"",
            "\"platformContextMenu\"",
            "\"nativeFileIcons\"",
            "\"nativeThumbnails\"",
            "\"nativeDragOut\"",
            "\"systemTrash\"",
            "\"revealInSystemFileManager\"",
            "\"openTerminal\"",
            "\"serverAdministration\"",
            "\"extendedAttributes\"",
            "\"finderTags\"",
            "\"finderAliases\"",
        ] {
            assert!(json.contains(field), "expected {json} to contain {field}");
        }
    }

    #[test]
    fn runtime_kind_dto_matches_the_frontend_discriminators() {
        assert_eq!(
            serde_json::to_string(&RuntimeKindDto::BrowserServer).unwrap(),
            "\"browserServer\""
        );
        assert_eq!(
            serde_json::to_string(&RuntimeKindDto::Tauri).unwrap(),
            "\"tauri\""
        );
        assert_eq!(
            serde_json::to_string(&RuntimeKindDto::Mock).unwrap(),
            "\"mock\""
        );
    }

    #[test]
    fn platform_kind_dto_matches_the_frontend_discriminators() {
        assert_eq!(
            serde_json::to_string(&PlatformKindDto::Macos).unwrap(),
            "\"macos\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformKindDto::Windows).unwrap(),
            "\"windows\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformKindDto::Linux).unwrap(),
            "\"linux\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformKindDto::Unknown).unwrap(),
            "\"unknown\""
        );
    }
}
