//! Owns the single IPC channel the frontend subscribes native menu action
//! clicks through (task 0133).
//!
//! Simpler than `event_stream::EventSubscriptionRegistry`: there is only
//! ever one native menu bar for the process (mirroring
//! `fm-platform-macos`'s own single process-wide callback slot), so this
//! only ever needs to remember at most one channel rather than a
//! subscription map.

use std::sync::Mutex;

use tauri::ipc::Channel;

use crate::commands::NativeMenuActionEvent;

/// Holds the channel the frontend subscribed via
/// [`crate::commands::subscribe_native_menu_actions`], if any. `None` until
/// the frontend subscribes, and again after a window reload replaces it -
/// `set_native_menu` treats "not subscribed yet" as a no-op callback rather
/// than an error, so installing a menu before the frontend subscribes still
/// succeeds.
#[derive(Default)]
pub(crate) struct NativeMenuActionChannel {
    channel: Mutex<Option<Channel<NativeMenuActionEvent>>>,
}

impl NativeMenuActionChannel {
    /// Stores `channel` as the current subscription, replacing any previous one.
    pub(crate) fn set(&self, channel: Channel<NativeMenuActionEvent>) {
        *self
            .channel
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(channel);
    }

    /// Returns the current subscription, if any.
    pub(crate) fn get(&self) -> Option<Channel<NativeMenuActionEvent>> {
        self.channel
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_no_subscription_until_one_is_set_then_returns_the_latest_one() {
        let registry = NativeMenuActionChannel::default();
        assert!(registry.get().is_none());

        registry.set(Channel::new(|_| Ok(())));
        assert!(registry.get().is_some());

        // A second `set` (a window reload re-subscribing) replaces, not adds to, the first.
        registry.set(Channel::new(|_| Ok(())));
        assert!(registry.get().is_some());
    }
}
