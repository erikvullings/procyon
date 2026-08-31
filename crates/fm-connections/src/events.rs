//! Maps connection lifecycle transitions onto the `connection.*`
//! [`fm_events::BackendEventPayload`] variants (task 0103, spec §17).

use fm_events::{BackendEventPayload, ConnectionStatusPayload};

use crate::ids::ConnectionId;
use crate::status::ConnectionStatus;

fn status_payload(status: ConnectionStatus) -> ConnectionStatusPayload {
    match status {
        ConnectionStatus::Disconnected => ConnectionStatusPayload::Disconnected,
        ConnectionStatus::Connecting => ConnectionStatusPayload::Connecting,
        ConnectionStatus::Connected => ConnectionStatusPayload::Connected,
        ConnectionStatus::Reconnecting => ConnectionStatusPayload::Reconnecting,
        ConnectionStatus::AuthenticationRequired => ConnectionStatusPayload::AuthenticationRequired,
        ConnectionStatus::HostKeyUnverified => ConnectionStatusPayload::HostKeyUnverified,
        ConnectionStatus::HostKeyMismatch => ConnectionStatusPayload::HostKeyMismatch,
        ConnectionStatus::Failed => ConnectionStatusPayload::Failed,
    }
}

pub(crate) fn connection_created(id: ConnectionId) -> BackendEventPayload {
    BackendEventPayload::ConnectionCreated {
        connection_id: id.into_inner(),
    }
}

pub(crate) fn connection_updated(id: ConnectionId) -> BackendEventPayload {
    BackendEventPayload::ConnectionUpdated {
        connection_id: id.into_inner(),
    }
}

pub(crate) fn connection_deleted(id: ConnectionId) -> BackendEventPayload {
    BackendEventPayload::ConnectionDeleted {
        connection_id: id.into_inner(),
    }
}

pub(crate) fn connection_status_changed(
    id: ConnectionId,
    status: ConnectionStatus,
) -> BackendEventPayload {
    BackendEventPayload::ConnectionStatusChanged {
        connection_id: id.into_inner(),
        status: status_payload(status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_status_changed_carries_the_mapped_status() {
        let id = ConnectionId::new();
        let event = connection_status_changed(id, ConnectionStatus::Failed);
        assert_eq!(
            event,
            BackendEventPayload::ConnectionStatusChanged {
                connection_id: id.into_inner(),
                status: ConnectionStatusPayload::Failed,
            }
        );
    }
}
