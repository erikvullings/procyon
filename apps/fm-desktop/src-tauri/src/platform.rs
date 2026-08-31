//! Selects the concrete [`PlatformAdapter`] the desktop host injects into
//! [`fm_application::FileManagerService`] (task 0058).
//!
//! Adapter selection lives here, in the host binary, rather than in
//! `fm-application`: `fm-application` is target-agnostic and must not depend
//! on every OS-specific platform crate just to pick one at runtime (`fm
//! -platform-macos`/`fm-platform-windows` compile to nothing off their target
//! OS, so this file is the one place a `#[cfg(target_os = ...)]` branch is
//! needed).

use std::sync::Arc;

use fm_platform::PlatformAdapter;
use fm_search_acceleration::SearchAcceleration;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fm_search_acceleration::UnsupportedSearchAccelerator;

/// Builds the platform adapter for the current desktop build target.
///
/// macOS builds get the real [`fm_platform_macos::MacosPlatformAdapter`]
/// (task 0059); Windows builds still get a
/// [`fm_platform::FallbackPlatformAdapter`]-delegating adapter (task 0060 adds
/// real native integration there); any other target falls back directly,
/// since it has no native adapter crate at all.
#[must_use]
pub(crate) fn build_platform_adapter() -> Arc<dyn PlatformAdapter> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(fm_platform_macos::MacosPlatformAdapter::new())
    }

    #[cfg(target_os = "windows")]
    {
        Arc::new(fm_platform_windows::WindowsPlatformAdapter::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(fm_platform::FallbackPlatformAdapter)
    }
}

/// Builds the optional native local-search adapter for the desktop host.
#[must_use]
pub(crate) fn build_search_accelerator() -> Arc<dyn SearchAcceleration> {
    #[cfg(target_os = "macos")]
    {
        Arc::new(fm_platform_macos::search::MacosSpotlightSearchAccelerator::new())
    }
    #[cfg(target_os = "windows")]
    {
        Arc::new(fm_platform_windows::search::WindowsSearchAccelerator::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Arc::new(UnsupportedSearchAccelerator)
    }
}
