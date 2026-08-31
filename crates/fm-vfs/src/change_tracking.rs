use std::time::Duration;

/// How a provider's directory contents can be observed for external changes
/// (task 0109).
///
/// This is distinct from [`crate::ProviderCapabilities::WATCH`], which only
/// says whether [`crate::FileSystemProvider::watch`] can be called at all.
/// [`ChangeTracking`] tells a caller — chiefly `fm-application`'s directory
/// service — *how* to keep a listing fresh: consume
/// [`crate::FileSystemProvider::watch`]'s stream directly, fall back to
/// conservative polling, or give up on live tracking entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeTracking {
    /// [`crate::FileSystemProvider::watch`] streams real, low-latency
    /// notifications from the host OS (e.g. `inotify`/`FSEvents`/
    /// `ReadDirectoryChangesW`).
    NativeWatch,
    /// [`crate::FileSystemProvider::watch`] streams notifications derived
    /// from a remote delta/sync-token API rather than an OS-level
    /// filesystem event source (e.g. a future native OneDrive provider,
    /// task 0110).
    DeltaApi,
    /// No push notifications exist. [`crate::FileSystemProvider::watch`] is
    /// not implemented; a caller must instead poll
    /// [`crate::FileSystemProvider::list`] no more often than `interval`
    /// and diff the result itself.
    Poll {
        /// Conservative minimum time between polls.
        interval: Duration,
    },
    /// No change tracking is available at all; directories must be
    /// refreshed manually. [`crate::FileSystemProvider::watch`] is not
    /// implemented and must not be called.
    Unsupported,
}

/// A conservative default poll interval for remote providers with no native
/// change-notification API (e.g. SFTP, FTP/FTPS) — long enough to avoid
/// hammering a remote server on every tick, short enough that a manual
/// refresh rarely feels necessary.
pub const CONSERVATIVE_POLL_INTERVAL: Duration = Duration::from_secs(20);
