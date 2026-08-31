//! Multiplexed browser event stream (task 0032).

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response, Sse};
use fm_events::{
    BackendEventPayload, EventAudience, SessionId, SubscriptionEvent, serialize_event_envelope,
};
use futures::stream;
use serde::Deserialize;

use crate::state::AppState;

const DEVELOPMENT_SESSION_ID: &str = "local-development-session";
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EventStreamQuery {
    last_event_id: Option<u64>,
}

/// Streams every event visible to the explicit local development session.
#[utoipa::path(
    get,
    path = "/api/v1/events",
    operation_id = "subscribeEvents",
    responses((status = 200, description = "Multiplexed Server-Sent Events stream"))
)]
pub(crate) async fn get_events(
    State(state): State<AppState>,
    Query(query): Query<EventStreamQuery>,
    headers: HeaderMap,
) -> Response {
    if !origin_is_allowed(&state, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let last_event_id = match parse_last_event_id(&headers) {
        Ok(value) => value,
        Err(status) => return status.into_response(),
    }
    .or(query.last_event_id);
    let bus = state.service.event_bus();
    let session_id = SessionId::new(DEVELOPMENT_SESSION_ID);
    let subscription = bus.subscribe_all_workspaces(session_id.clone(), last_event_id);
    state.service.republish_pending_operation_conflicts();
    let session_end = state.session_end;
    bus.publish(
        EventAudience::Session(session_id),
        BackendEventPayload::RuntimeReady,
    );

    let keep_alive = tokio::time::interval_at(
        tokio::time::Instant::now() + KEEP_ALIVE_INTERVAL,
        KEEP_ALIVE_INTERVAL,
    );
    let connection_state = state.connection_state.clone();
    let events = stream::unfold(
        (subscription, session_end, keep_alive, connection_state),
        |(mut subscription, session_end, mut keep_alive, connection_state)| async move {
            let item = tokio::select! {
                () = session_end.cancelled() => return None,
                _ = keep_alive.tick() => Ok(keep_alive_event()),
                received = subscription.recv() => match received {
                    Ok(SubscriptionEvent::Event(envelope)) => {
                        connection_state.record_event();
                        let name = envelope.payload.event_name();
                        let id = envelope.event_id.to_string();
                        serialize_event_envelope(&envelope)
                            .map(|data| Event::default().event(name).id(id).data(data))
                    }
                    Ok(SubscriptionEvent::Gap {
                        last_event_id,
                        oldest_available_id,
                        newest_available_id,
                    }) => serde_json::to_string(&serde_json::json!({
                        "lastEventId": last_event_id,
                        "oldestAvailableId": oldest_available_id,
                        "newestAvailableId": newest_available_id,
                    }))
                    .map(|data| Event::default().event("resynchronise").data(data)),
                    Err(_) => return None,
                }
            };
            Some((
                item,
                (subscription, session_end, keep_alive, connection_state),
            ))
        },
    );
    Sse::new(events).into_response()
}

fn keep_alive_event() -> Event {
    Event::default().event("keep-alive").data("")
}

fn parse_last_event_id(headers: &HeaderMap) -> Result<Option<u64>, StatusCode> {
    headers.get("last-event-id").map_or(Ok(None), |value| {
        value
            .to_str()
            .ok()
            .and_then(|value| value.parse().ok())
            .map(Some)
            .ok_or(StatusCode::BAD_REQUEST)
    })
}

fn origin_is_allowed(state: &AppState, headers: &HeaderMap) -> bool {
    headers.get(header::ORIGIN).is_none_or(|origin| {
        origin.to_str().is_ok_and(|origin| {
            state
                .cors_allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        })
    })
}
