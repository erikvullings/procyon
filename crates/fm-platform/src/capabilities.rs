bitflags::bitflags! {
    /// Native OS integrations a [`crate::PlatformAdapter`] can perform.
    ///
    /// Frontends respond to these flags rather than detecting the operating
    /// system directly (specification §21); an adapter that has not
    /// implemented an integration must leave its bit unset rather than
    /// reporting success and failing at call time (specification §23).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct PlatformCapabilities: u32 {
        /// Fetch a file's native icon.
        const FILE_ICONS = 1 << 0;
        /// Fetch a native thumbnail preview for a file.
        const THUMBNAILS = 1 << 1;
        /// Reveal an entry in the system file manager (Finder/Explorer/...).
        const REVEAL_IN_FILE_MANAGER = 1 << 2;
        /// Move an entry to the system trash/recycle bin.
        const TRASH = 1 << 3;
        /// Open an entry with the OS default application.
        const OPEN_WITH_DEFAULT_APPLICATION = 1 << 4;
        /// Open a terminal at a location.
        const OPEN_TERMINAL = 1 << 5;
        /// Read or write file path lists to the OS clipboard.
        const CLIPBOARD_FILE_REFERENCES = 1 << 6;
        /// List mounted volumes/drives.
        const MOUNTED_VOLUMES = 1 << 7;
        /// Install a native application menu bar.
        const NATIVE_MENUS = 1 << 8;
        /// Support dragging entries out to another application.
        const NATIVE_DRAG_OUT = 1 << 9;
        /// Report total/available capacity for the volume backing a path.
        const VOLUME_CAPACITY = 1 << 10;
        /// Read/write generic extended attributes (currently: the Spotlight
        /// "Finder comment", `kMDItemFinderComment`).
        const EXTENDED_ATTRIBUTES = 1 << 11;
        /// Read/write Finder tags (`_kMDItemUserTags`).
        const FINDER_TAGS = 1 << 12;
        /// Discover and remove an application bundle's related support files
        /// (task 0148: preferences, caches, application-support data, ...).
        const APPLICATION_UNINSTALL = 1 << 13;
        /// Expose the OS Services (macOS) or Send To (Windows) submenu for a selection.
        const PLATFORM_CONTEXT_MENU = 1 << 14;
        /// Present a local file through the OS Quick Look preview panel.
        const QUICK_LOOK = 1 << 15;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_capabilities_contain_nothing() {
        assert!(PlatformCapabilities::empty().is_empty());
    }

    #[test]
    fn capabilities_combine_with_bitwise_or() {
        let combined = PlatformCapabilities::TRASH | PlatformCapabilities::OPEN_TERMINAL;
        assert!(combined.contains(PlatformCapabilities::TRASH));
        assert!(combined.contains(PlatformCapabilities::OPEN_TERMINAL));
        assert!(!combined.contains(PlatformCapabilities::FILE_ICONS));
    }
}
