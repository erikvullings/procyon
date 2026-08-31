//! Tauri IPC adapter for the transport-neutral [`fm_events::EventBus`].

use std::collections::HashMap;
use std::sync::Mutex;

use fm_events::{EventBus, SessionId, SubscriptionEvent, serialize_event_envelope};
use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use uuid::Uuid;

const DESKTOP_SESSION_ID: &str = "local-desktop-session";

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum ControlMessage {
    Resynchronise {
        last_event_id: u64,
        oldest_available_id: u64,
        newest_available_id: u64,
    },
}

struct ActiveSubscription {
    window_label: String,
    task: JoinHandle<()>,
}

/// Owns desktop subscription tasks so commands and window lifecycle hooks only
/// install or release adapters; application and filesystem logic remains in
/// `fm-application`.
#[derive(Default)]
pub(crate) struct EventSubscriptionRegistry {
    subscriptions: Mutex<HashMap<Uuid, ActiveSubscription>>,
}

impl EventSubscriptionRegistry {
    pub(crate) fn subscribe(
        &self,
        event_bus: EventBus,
        window_label: String,
        on_event: Channel<String>,
    ) -> Uuid {
        let subscription_id = Uuid::new_v4();
        let task = tauri::async_runtime::spawn(forward_events(event_bus, on_event));
        self.subscriptions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(subscription_id, ActiveSubscription { window_label, task });
        subscription_id
    }

    pub(crate) fn unsubscribe(&self, subscription_id: Uuid) {
        if let Some(subscription) = self
            .subscriptions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&subscription_id)
        {
            subscription.task.abort();
        }
    }

    pub(crate) fn unsubscribe_window(&self, window_label: &str) {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let ids = subscriptions
            .iter()
            .filter_map(|(id, subscription)| {
                (subscription.window_label == window_label).then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(subscription) = subscriptions.remove(&id) {
                subscription.task.abort();
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.subscriptions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }
}

async fn forward_events(event_bus: EventBus, on_event: Channel<String>) {
    let mut subscription =
        event_bus.subscribe_all_workspaces(SessionId::new(DESKTOP_SESSION_ID), None);
    while let Ok(item) = subscription.recv().await {
        let json = match item {
            SubscriptionEvent::Event(envelope) => serialize_event_envelope(&envelope),
            SubscriptionEvent::Gap {
                last_event_id,
                oldest_available_id,
                newest_available_id,
            } => serde_json::to_string(&ControlMessage::Resynchronise {
                last_event_id,
                oldest_available_id,
                newest_available_id,
            }),
        };
        let Ok(json) = json else { continue };
        if on_event.send(json).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use fm_events::{BackendEventPayload, EventAudience};
    use tauri::ipc::InvokeResponseBody;

    use super::*;

    #[tokio::test]
    async fn forwards_the_same_envelope_json_used_by_sse_and_stops_when_cancelled() {
        let bus = EventBus::new(8);
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_by_channel = Arc::clone(&received);
        let channel = Channel::new(move |body| {
            let message = match body {
                InvokeResponseBody::Json(json) => serde_json::from_str::<String>(&json)?,
                InvokeResponseBody::Raw(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            };
            received_by_channel
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(message);
            Ok(())
        });
        let registry = EventSubscriptionRegistry::default();
        let id = registry.subscribe(bus.clone(), "main".into(), channel);
        while bus.subscriber_count() == 0 {
            tokio::task::yield_now().await;
        }
        let envelope = bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);
        for _ in 0..20 {
            if !received
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        let expected = serialize_event_envelope(&envelope).expect("fixture must serialize");
        assert_eq!(
            received.lock().expect("channel lock").as_slice(),
            [expected]
        );
        assert_eq!(bus.subscriber_count(), 1);

        registry.unsubscribe(id);
        assert_eq!(registry.len(), 0);
        // `abort()` only requests cancellation; the aborted task's subscriber-count-decrementing
        // drop runs whenever the runtime next polls it, which under load (e.g. the full workspace
        // suite running many test binaries concurrently) can take longer than a fixed short sleep.
        // Poll on a wall-clock budget instead, matching the sibling test below.
        for _ in 0..2_000 {
            if bus.subscriber_count() == 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn window_teardown_aborts_all_of_its_subscription_tasks_only() {
        let bus = EventBus::new(8);
        let registry = EventSubscriptionRegistry::default();
        for label in ["main", "main", "other"] {
            registry.subscribe(bus.clone(), label.into(), Channel::new(|_| Ok(())));
        }
        // A single `yield_now` only guarantees one scheduler round, not that all three spawned
        // subscription tasks have registered - under load (e.g. the full workspace suite running
        // many test binaries concurrently) that can flake, and so can a bounded count of
        // `yield_now`s if the runner's OS threads are themselves starved. Poll on a wall-clock
        // budget instead, matching the sibling test above.
        for _ in 0..2_000 {
            if bus.subscriber_count() == 3 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert_eq!(bus.subscriber_count(), 3);

        registry.unsubscribe_window("main");
        // Unsubscribing aborts the subscription tasks asynchronously, so poll rather than
        // sleep a fixed duration - the same flakiness the comment above describes.
        for _ in 0..2_000 {
            if bus.subscriber_count() == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }

        assert_eq!(registry.len(), 1);
        assert_eq!(bus.subscriber_count(), 1);
    }
}
