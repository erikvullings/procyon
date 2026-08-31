//! Public event-bus contract tests.

use fm_domain::{OperationId, WorkspaceId};
use fm_events::{
    BackendEventPayload, DirectoryDeltaPayload, EventAudience, EventBus, OperationProgressDetails,
    OperationProgressPayload, SessionId, SubscriptionEvent,
};

#[tokio::test]
async fn all_workspace_subscription_receives_interleaved_workspace_events() {
    let bus = EventBus::new(8);
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    let mut subscription = bus.subscribe_all_workspaces(SessionId::new("local-development"), None);

    let first = bus.publish(
        EventAudience::Workspace(workspace_b),
        BackendEventPayload::RuntimeReady,
    );
    let second = bus.publish(
        EventAudience::Workspace(workspace_a),
        BackendEventPayload::RuntimeReady,
    );

    let received_first = subscription.recv().await.expect("first event");
    let received_second = subscription.recv().await.expect("second event");
    assert!(
        matches!(received_first, SubscriptionEvent::Event(event) if event.event_id == first.event_id && event.workspace_id == Some(workspace_b))
    );
    assert!(
        matches!(received_second, SubscriptionEvent::Event(event) if event.event_id == second.event_id && event.workspace_id == Some(workspace_a))
    );
}

#[tokio::test]
async fn publishes_events_in_monotonic_order() {
    let bus = EventBus::new(8);
    let workspace_id = WorkspaceId::new();
    let mut subscription = bus.subscribe(SessionId::new("session-a"), [workspace_id], None);

    let first = bus.publish(
        EventAudience::Workspace(workspace_id),
        BackendEventPayload::RuntimeReady,
    );
    let second = bus.publish(
        EventAudience::Workspace(workspace_id),
        BackendEventPayload::RuntimeReady,
    );

    assert_eq!(first.event_id, 1);
    assert_eq!(second.event_id, 2);
    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(first))
    );
    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(second))
    );
}

#[tokio::test]
async fn filters_interleaved_events_by_session_and_workspace() {
    let bus = EventBus::new(16);
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    let session_a = SessionId::new("session-a");
    let session_b = SessionId::new("session-b");
    let mut subscriber_a = bus.subscribe(session_a.clone(), [workspace_a], None);
    let mut subscriber_b = bus.subscribe(session_b.clone(), [workspace_b], None);

    let global = bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);
    let only_b = bus.publish(
        EventAudience::Session(session_b),
        BackendEventPayload::RuntimeReady,
    );
    let workspace_a_event = bus.publish(
        EventAudience::Workspace(workspace_a),
        BackendEventPayload::RuntimeReady,
    );
    let only_a = bus.publish(
        EventAudience::Session(session_a),
        BackendEventPayload::RuntimeReady,
    );
    let workspace_b_event = bus.publish(
        EventAudience::Workspace(workspace_b),
        BackendEventPayload::RuntimeReady,
    );

    assert_eq!(
        subscriber_a.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(global.clone()))
    );
    assert_eq!(
        subscriber_a.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(workspace_a_event))
    );
    assert_eq!(
        subscriber_a.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(only_a))
    );
    assert_eq!(
        subscriber_b.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(global))
    );
    assert_eq!(
        subscriber_b.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(only_b))
    );
    assert_eq!(
        subscriber_b.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(workspace_b_event))
    );
}

#[tokio::test]
async fn replays_visible_events_after_last_event_id_before_live_delivery() {
    let bus = EventBus::new(8);
    let workspace_a = WorkspaceId::new();
    let workspace_b = WorkspaceId::new();
    bus.publish(
        EventAudience::Workspace(workspace_a),
        BackendEventPayload::RuntimeReady,
    );
    bus.publish(
        EventAudience::Workspace(workspace_b),
        BackendEventPayload::RuntimeReady,
    );
    let replayed = bus.publish(
        EventAudience::Workspace(workspace_a),
        BackendEventPayload::RuntimeReady,
    );
    bus.publish(
        EventAudience::Workspace(workspace_b),
        BackendEventPayload::RuntimeReady,
    );

    let mut subscription = bus.subscribe(SessionId::new("session-a"), [workspace_a], Some(2));
    let live = bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);

    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(replayed))
    );
    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(live))
    );
}

#[tokio::test]
async fn reports_a_gap_when_last_event_id_fell_out_of_replay() {
    let bus = EventBus::new(2);
    for _ in 0..4 {
        bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);
    }

    let mut subscription = bus.subscribe(SessionId::new("session-a"), std::iter::empty(), Some(1));
    let live = bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);

    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Gap {
            last_event_id: 1,
            oldest_available_id: 3,
            newest_available_id: 4,
        }
    );
    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(live))
    );
}

#[tokio::test]
async fn lagging_subscriber_gets_an_explicit_gap_without_blocking_publishers() {
    let bus = EventBus::new(2);
    let mut subscription = bus.subscribe(SessionId::new("slow-session"), std::iter::empty(), None);
    for _ in 0..4 {
        bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);
    }

    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Gap {
            last_event_id: 0,
            oldest_available_id: 3,
            newest_available_id: 4,
        }
    );

    let fresh = bus.publish(EventAudience::Global, BackendEventPayload::RuntimeReady);
    assert_eq!(
        subscription.recv().await.expect("bus remains open"),
        SubscriptionEvent::Event(Box::new(fresh))
    );
}

#[test]
fn identifies_high_frequency_payloads_as_coalescable() {
    let progress = BackendEventPayload::OperationProgress {
        progress: OperationProgressPayload {
            operation_id: OperationId::new(),
            progress: OperationProgressDetails {
                completed_items: 1,
                total_items: Some(10),
                completed_bytes: 100,
                total_bytes: Some(1_000),
                current_entry: None,
                bytes_per_second: Some(500),
            },
        },
    };
    let delta = BackendEventPayload::DirectoryDelta {
        pane_id: fm_domain::PaneId::new(),
        delta: DirectoryDeltaPayload::EntriesRemoved {
            revision: 2,
            entry_ids: Vec::new(),
        },
    };

    assert!(progress.should_coalesce());
    assert!(delta.should_coalesce());
    assert!(!BackendEventPayload::RuntimeReady.should_coalesce());
}
