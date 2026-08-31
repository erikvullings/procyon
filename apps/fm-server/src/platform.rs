//! Selects the concrete platform adapter for the machine hosting the server.
//!
//! Browser clients browse the server's filesystem, so OS-managed locations must
//! be discovered on that same machine just like the embedded desktop host.

use std::sync::Arc;

use fm_platform::PlatformAdapter;
use fm_search_acceleration::SearchAcceleration;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fm_search_acceleration::UnsupportedSearchAccelerator;

/// Builds the platform adapter for the current server build target.
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

/// Builds the optional native local-search adapter for the server host.
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

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use super::build_platform_adapter;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    use fm_platform::PlatformCapabilities;

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn native_server_build_uses_the_host_platform_adapter() {
        assert!(
            build_platform_adapter()
                .capabilities()
                .contains(PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION)
        );
    }
}
