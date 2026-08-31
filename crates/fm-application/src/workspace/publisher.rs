//! An injectable seam for publishing workspace mutation events (spec §5.3.9
//! step 7, §5.3.11).
//!
//! `WorkspaceService` constructs a focused [`fm_events::BackendEventPayload`]
//! after every lifecycle transition and successful
//! [`fm_domain::WorkspaceCommand`] and hands it to this seam (task 0081).
//! Publishing the payload onto a real shared event bus (task 0031) is left
//! to whichever host wires a concrete [`WorkspaceCommandPublisher`] in; this
//! seam only guarantees the payload is constructed correctly and handed off
//! exactly once per transition.

use fm_domain::WorkspaceId;
use fm_events::BackendEventPayload;

/// Notified after a workspace lifecycle transition or
/// [`fm_domain::WorkspaceCommand`] is applied and persisted.
pub trait WorkspaceCommandPublisher: Send + Sync {
    /// Called once, after the workspace change has been persisted, with the
    /// focused event payload describing what changed.
    fn publish(&self, workspace_id: WorkspaceId, payload: BackendEventPayload);
}

/// Default publisher used until a host wires in a real event bus (task
/// 0031).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopWorkspaceCommandPublisher;

impl WorkspaceCommandPublisher for NoopWorkspaceCommandPublisher {
    fn publish(&self, _workspace_id: WorkspaceId, _payload: BackendEventPayload) {}
}
