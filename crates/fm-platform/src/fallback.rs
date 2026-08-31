use crate::{PlatformAdapter, PlatformCapabilities};

/// No-op platform adapter used by browser/server mode and any platform
/// without native integration (specification §23): every operation reports
/// itself as unsupported so call sites never see an affordance that is
/// present but broken.
#[derive(Debug, Clone, Copy, Default)]
pub struct FallbackPlatformAdapter;

impl PlatformAdapter for FallbackPlatformAdapter {
    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::empty()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::PlatformError;

    #[test]
    fn reports_no_capabilities() {
        assert_eq!(
            FallbackPlatformAdapter.capabilities(),
            PlatformCapabilities::empty()
        );
    }

    #[test]
    fn every_operation_reports_unsupported() {
        let adapter = FallbackPlatformAdapter;
        let path = Path::new("/tmp/fm-platform-fallback-test.txt");

        assert!(matches!(
            adapter.file_icon(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::FILE_ICONS
            })
        ));
        assert!(matches!(
            adapter.thumbnail(path, 64),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::THUMBNAILS
            })
        ));
        assert!(matches!(
            adapter.reveal_in_file_manager(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::REVEAL_IN_FILE_MANAGER
            })
        ));
        assert!(matches!(
            adapter.trash(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::TRASH
            })
        ));
        assert!(matches!(
            adapter.open_with_default_application(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
            })
        ));
        assert!(matches!(
            adapter.open_terminal(path, None),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::OPEN_TERMINAL
            })
        ));
        assert!(matches!(
            adapter.open_in_text_editor(path, None),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
            })
        ));
        assert!(matches!(
            adapter.open_with_chooser(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION
            })
        ));
        assert!(matches!(
            adapter.quick_look(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::QUICK_LOOK
            })
        ));
        assert!(matches!(
            adapter.read_clipboard_file_references(),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::CLIPBOARD_FILE_REFERENCES
            })
        ));
        assert!(matches!(
            adapter.write_clipboard_file_references(&[path.to_path_buf()]),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::CLIPBOARD_FILE_REFERENCES
            })
        ));
        assert!(matches!(
            adapter.mounted_volumes(),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::MOUNTED_VOLUMES
            })
        ));
        assert!(matches!(
            adapter.volume_capacity(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::VOLUME_CAPACITY
            })
        ));
        assert!(matches!(
            adapter.finder_tags(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::FINDER_TAGS
            })
        ));
        assert!(matches!(
            adapter.set_finder_tags(path, &[]),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::FINDER_TAGS
            })
        ));
        assert!(matches!(
            adapter.spotlight_comment(path),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::EXTENDED_ATTRIBUTES
            })
        ));
        assert!(matches!(
            adapter.set_spotlight_comment(path, None),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::EXTENDED_ATTRIBUTES
            })
        ));
        assert!(matches!(
            adapter.install_native_menu(
                &fm_domain::NativeMenuSpec::default(),
                std::sync::Arc::new(|_id| {})
            ),
            Err(PlatformError::Unsupported {
                capability: PlatformCapabilities::NATIVE_MENUS
            })
        ));
    }

    #[test]
    fn capabilities_report_matches_the_methods_that_are_actually_overridden() {
        // The fallback overrides nothing beyond `capabilities`, so every
        // capability bit must be unset - if a future edit implements one of
        // the methods above without also flipping its bit here, this test
        // must be updated deliberately rather than silently drifting.
        let capabilities = FallbackPlatformAdapter.capabilities();
        for capability in [
            PlatformCapabilities::FILE_ICONS,
            PlatformCapabilities::THUMBNAILS,
            PlatformCapabilities::REVEAL_IN_FILE_MANAGER,
            PlatformCapabilities::TRASH,
            PlatformCapabilities::OPEN_WITH_DEFAULT_APPLICATION,
            PlatformCapabilities::OPEN_TERMINAL,
            PlatformCapabilities::CLIPBOARD_FILE_REFERENCES,
            PlatformCapabilities::MOUNTED_VOLUMES,
            PlatformCapabilities::NATIVE_MENUS,
            PlatformCapabilities::NATIVE_DRAG_OUT,
            PlatformCapabilities::VOLUME_CAPACITY,
            PlatformCapabilities::EXTENDED_ATTRIBUTES,
            PlatformCapabilities::FINDER_TAGS,
            PlatformCapabilities::PLATFORM_CONTEXT_MENU,
            PlatformCapabilities::QUICK_LOOK,
        ] {
            assert!(!capabilities.contains(capability), "{capability:?}");
        }
    }
}
