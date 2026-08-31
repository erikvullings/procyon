use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fm_domain::WorkspaceId;
use tokio::sync::broadcast;

use crate::{BackendEventPayload, EventEnvelope};

/// Default number of live and replay events retained by the bus.
pub const DEFAULT_EVENT_BUS_CAPACITY: usize = 1_024;

/// Opaque identity used to scope events to one authenticated session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Creates a session identifier from a transport-owned value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

/// Selects which subscribers may receive an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventAudience {
    /// Every session receives the event.
    Global,
    /// Only the named session receives the event.
    Session(SessionId),
    /// Sessions authorized for this workspace receive the event.
    Workspace(WorkspaceId),
}

/// An item delivered to an event-bus subscriber.
#[derive(Debug, Clone, PartialEq)]
pub enum SubscriptionEvent {
    /// A typed backend event.
    Event(Box<EventEnvelope<BackendEventPayload>>),
    /// Requested history is no longer retained and a resynchronization is required.
    Gap {
        /// Last event confirmed by the reconnecting subscriber.
        last_event_id: u64,
        /// Oldest event still retained when the gap was detected.
        oldest_available_id: u64,
        /// Newest event retained when the gap was detected.
        newest_available_id: u64,
    },
}

/// Indicates that the bus was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionClosed;

impl fmt::Display for SubscriptionClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("event subscription closed")
    }
}

impl std::error::Error for SubscriptionClosed {}

#[derive(Debug, Clone)]
struct PublishedEvent {
    audience: EventAudience,
    envelope: EventEnvelope<BackendEventPayload>,
}

struct BusState {
    next_event_id: u64,
    replay: VecDeque<PublishedEvent>,
}

struct EventBusInner {
    capacity: usize,
    state: Mutex<BusState>,
    sender: broadcast::Sender<PublishedEvent>,
    subscriber_count: AtomicUsize,
}

/// Bounded, transport-neutral publisher for backend events.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

impl EventBus {
    /// Creates a bus whose live channel and replay buffer share one capacity.
    ///
    /// A minimum capacity of one is used when `capacity` is zero.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Self {
            inner: Arc::new(EventBusInner {
                capacity,
                state: Mutex::new(BusState {
                    next_event_id: 1,
                    replay: VecDeque::with_capacity(capacity),
                }),
                sender,
                subscriber_count: AtomicUsize::new(0),
            }),
        }
    }

    /// Publishes without waiting for subscribers.
    ///
    /// Allocation and broadcast occur under one short lock so concurrent
    /// publishers are observed in event-id order.
    pub fn publish(
        &self,
        audience: EventAudience,
        payload: BackendEventPayload,
    ) -> EventEnvelope<BackendEventPayload> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let event_id = state.next_event_id;
        state.next_event_id = state
            .next_event_id
            .checked_add(1)
            .expect("event id space exhausted");
        let envelope = EventEnvelope {
            event_id,
            timestamp: chrono::Utc::now(),
            workspace_id: match audience {
                EventAudience::Workspace(workspace_id) => Some(workspace_id),
                EventAudience::Global | EventAudience::Session(_) => None,
            },
            payload,
        };
        let published = PublishedEvent {
            audience,
            envelope: envelope.clone(),
        };
        if state.replay.len() == self.inner.capacity {
            state.replay.pop_front();
        }
        state.replay.push_back(published.clone());
        let _ = self.inner.sender.send(published);
        envelope
    }

    /// Creates a subscriber scoped to one session and its visible workspaces.
    pub fn subscribe(
        &self,
        session_id: SessionId,
        workspace_ids: impl IntoIterator<Item = WorkspaceId>,
        last_event_id: Option<u64>,
    ) -> EventSubscription {
        let workspace_ids = workspace_ids.into_iter().collect::<HashSet<_>>();
        self.subscribe_inner(session_id, workspace_ids, false, last_event_id)
    }

    /// Creates a subscriber that can see every workspace.
    ///
    /// This is reserved for a host-owned local development session. Authenticated
    /// multi-user hosts must use [`Self::subscribe`] with an explicit allow-list.
    pub fn subscribe_all_workspaces(
        &self,
        session_id: SessionId,
        last_event_id: Option<u64>,
    ) -> EventSubscription {
        self.subscribe_inner(session_id, HashSet::new(), true, last_event_id)
    }

    fn subscribe_inner(
        &self,
        session_id: SessionId,
        workspace_ids: HashSet<WorkspaceId>,
        all_workspaces: bool,
        last_event_id: Option<u64>,
    ) -> EventSubscription {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let receiver = self.inner.sender.subscribe();
        let replay_bounds = state
            .replay
            .front()
            .zip(state.replay.back())
            .map(|(oldest, newest)| (oldest.envelope.event_id, newest.envelope.event_id));
        let gap = last_event_id.and_then(|last_event_id| {
            replay_bounds
                .filter(|(oldest, _)| last_event_id.saturating_add(1) < *oldest)
                .map(
                    |(oldest_available_id, newest_available_id)| SubscriptionEvent::Gap {
                        last_event_id,
                        oldest_available_id,
                        newest_available_id,
                    },
                )
        });
        let replay = if gap.is_some() {
            VecDeque::new()
        } else {
            last_event_id.map_or_else(VecDeque::new, |last_event_id| {
                state
                    .replay
                    .iter()
                    .filter(|published| {
                        published.envelope.event_id > last_event_id
                            && can_receive(
                                &session_id,
                                &workspace_ids,
                                all_workspaces,
                                &published.audience,
                            )
                    })
                    .cloned()
                    .collect()
            })
        };
        drop(state);
        self.inner.subscriber_count.fetch_add(1, Ordering::Relaxed);
        EventSubscription {
            bus: Arc::clone(&self.inner),
            session_id,
            workspace_ids,
            all_workspaces,
            last_event_id: last_event_id.unwrap_or(0),
            gap,
            replay,
            receiver,
        }
    }

    /// Returns the number of currently live transport subscriptions.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.inner.subscriber_count.load(Ordering::Relaxed)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(DEFAULT_EVENT_BUS_CAPACITY)
    }
}

/// A session-scoped event subscription.
pub struct EventSubscription {
    bus: Arc<EventBusInner>,
    session_id: SessionId,
    workspace_ids: HashSet<WorkspaceId>,
    all_workspaces: bool,
    last_event_id: u64,
    gap: Option<SubscriptionEvent>,
    replay: VecDeque<PublishedEvent>,
    receiver: broadcast::Receiver<PublishedEvent>,
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        self.bus.subscriber_count.fetch_sub(1, Ordering::Relaxed);
    }
}

impl EventSubscription {
    /// Receives the next event visible to this session.
    pub async fn recv(&mut self) -> Result<SubscriptionEvent, SubscriptionClosed> {
        if let Some(gap) = self.gap.take() {
            return Ok(gap);
        }
        if let Some(published) = self.replay.pop_front() {
            self.last_event_id = published.envelope.event_id;
            return Ok(SubscriptionEvent::Event(Box::new(published.envelope)));
        }
        loop {
            let published = match self.receiver.recv().await {
                Ok(published) => published,
                Err(broadcast::error::RecvError::Closed) => return Err(SubscriptionClosed),
                Err(broadcast::error::RecvError::Lagged(_)) => return Ok(self.live_gap()),
            };
            if self.can_receive(&published.audience) {
                self.last_event_id = published.envelope.event_id;
                return Ok(SubscriptionEvent::Event(Box::new(published.envelope)));
            }
        }
    }

    fn can_receive(&self, audience: &EventAudience) -> bool {
        can_receive(
            &self.session_id,
            &self.workspace_ids,
            self.all_workspaces,
            audience,
        )
    }

    fn live_gap(&mut self) -> SubscriptionEvent {
        let state = self
            .bus
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let oldest_available_id = state
            .replay
            .front()
            .map_or(self.last_event_id.saturating_add(1), |event| {
                event.envelope.event_id
            });
        let newest_available_id = state
            .replay
            .back()
            .map_or(self.last_event_id, |event| event.envelope.event_id);
        self.receiver = self.bus.sender.subscribe();
        SubscriptionEvent::Gap {
            last_event_id: self.last_event_id,
            oldest_available_id,
            newest_available_id,
        }
    }
}

fn can_receive(
    session_id: &SessionId,
    workspace_ids: &HashSet<WorkspaceId>,
    all_workspaces: bool,
    audience: &EventAudience,
) -> bool {
    match audience {
        EventAudience::Global => true,
        EventAudience::Session(target) => target == session_id,
        EventAudience::Workspace(workspace_id) => {
            all_workspaces || workspace_ids.contains(workspace_id)
        }
    }
}
