//! Owns the per-window IPC channels the frontend subscribes native menu action
//! clicks through (task 0133).
//!
//! There is one native menu bar for the whole process (macOS), but every
//! Procyon window runs its own frontend. A click must reach the window the user
//! is working in, so each window's subscription is kept under its label and the
//! channel is chosen when the click happens — not when the menu is installed —
//! from whichever window is focused. A single shared slot let the most recently
//! subscribed window receive every click, so Edit ▸ Select All (Cmd+A) and every
//! other menu item silently acted on the wrong window.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::ipc::Channel;

use crate::commands::NativeMenuActionEvent;

/// Native menu subscriptions by window label. A window reload re-subscribes under the same
/// label, replacing its previous channel; a destroyed window's entry is removed.
#[derive(Default)]
pub(crate) struct NativeMenuActionChannel {
    channels: Mutex<HashMap<String, Channel<NativeMenuActionEvent>>>,
}

impl NativeMenuActionChannel {
    /// Stores `channel` as `window`'s subscription, replacing any previous one.
    pub(crate) fn set(&self, window: &str, channel: Channel<NativeMenuActionEvent>) {
        self.channels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(window.to_owned(), channel);
    }

    /// Drops `window`'s subscription (the window was destroyed).
    pub(crate) fn remove(&self, window: &str) {
        self.channels
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(window);
    }

    /// The channel a menu click should go to: the focused window's, or — when no subscribed
    /// window reports focus — any subscribed one, so a single-window app keeps working.
    pub(crate) fn for_window(
        &self,
        focused: Option<&str>,
    ) -> Option<Channel<NativeMenuActionEvent>> {
        let channels = self
            .channels
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        focused
            .and_then(|label| channels.get(label))
            .or_else(|| channels.values().next())
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_id(channel: &Channel<NativeMenuActionEvent>) -> u32 {
        channel.id()
    }

    #[test]
    fn has_no_subscription_until_one_is_set() {
        let registry = NativeMenuActionChannel::default();
        assert!(registry.for_window(Some("main")).is_none());
        assert!(registry.for_window(None).is_none());
    }

    #[test]
    fn routes_to_the_focused_window_rather_than_the_latest_subscriber() {
        let registry = NativeMenuActionChannel::default();
        let first = Channel::new(|_| Ok(()));
        let second = Channel::new(|_| Ok(()));
        let first_id = channel_id(&first);
        let second_id = channel_id(&second);
        registry.set("workspace-a", first);
        registry.set("workspace-b", second);

        let routed = registry
            .for_window(Some("workspace-a"))
            .expect("subscribed");
        assert_eq!(channel_id(&routed), first_id);
        let routed = registry
            .for_window(Some("workspace-b"))
            .expect("subscribed");
        assert_eq!(channel_id(&routed), second_id);
    }

    #[test]
    fn a_reload_replaces_and_a_destroyed_window_is_removed() {
        let registry = NativeMenuActionChannel::default();
        registry.set("main", Channel::new(|_| Ok(())));
        let reloaded = Channel::new(|_| Ok(()));
        let reloaded_id = channel_id(&reloaded);
        registry.set("main", reloaded);
        let routed = registry.for_window(Some("main")).expect("subscribed");
        assert_eq!(channel_id(&routed), reloaded_id);

        registry.remove("main");
        assert!(registry.for_window(Some("main")).is_none());
    }

    #[test]
    fn falls_back_to_a_subscribed_window_when_none_is_focused() {
        let registry = NativeMenuActionChannel::default();
        registry.set("main", Channel::new(|_| Ok(())));
        assert!(registry.for_window(None).is_some());
        assert!(registry.for_window(Some("unsubscribed")).is_some());
    }
}
