use serde::{Deserialize, Serialize};

use crate::{
    CatalogError, ConsentState, DocumentId, LibraryId, OccurrenceId, SemanticCatalog,
    SemanticLibraryPolicy, SemanticLibraryState, TenantId,
};

/// Path-free action the host may send to the isolated semantic worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkerFeedAction {
    /// Host may stream bounded content for this opaque occurrence.
    Upsert,
}

/// Provider-neutral worker decision containing no filesystem location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerFeedDecision {
    tenant_id: TenantId,
    library_id: LibraryId,
    document_id: DocumentId,
    occurrence_id: OccurrenceId,
    action: WorkerFeedAction,
}

impl WorkerFeedDecision {
    /// Returns the opaque tenant id.
    #[must_use]
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the opaque library id.
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    /// Returns the opaque deduplicated document id.
    #[must_use]
    pub const fn document_id(&self) -> DocumentId {
        self.document_id
    }

    /// Returns the opaque occurrence id used by the host to stream bytes.
    #[must_use]
    pub const fn occurrence_id(&self) -> OccurrenceId {
        self.occurrence_id
    }

    /// Returns the feed action.
    #[must_use]
    pub const fn action(&self) -> WorkerFeedAction {
        self.action
    }
}

impl SemanticCatalog {
    /// Produces path-free decisions for currently consented occurrences.
    ///
    /// The caller retains locations and performs all VFS reads; the returned
    /// values contain only opaque worker identifiers.
    ///
    /// Nothing is fed while ingestion is paused, and an occurrence is only fed
    /// when at least one of its scopes is proven by an enrolled root that is
    /// currently available. Pause and unavailability therefore stop new work
    /// without discarding queryable generations.
    ///
    /// # Errors
    ///
    /// Rejects a policy or runtime state for another library, paused
    /// ingestion, and provider-location failures.
    pub fn worker_feed_decisions(
        &self,
        policy: &SemanticLibraryPolicy,
        state: &SemanticLibraryState,
        tenant_id: TenantId,
    ) -> Result<Vec<WorkerFeedDecision>, CatalogError> {
        self.ensure_library(policy, state)?;
        if state.is_paused() {
            return Err(CatalogError::IngestionPaused);
        }
        let mut decisions = Vec::new();
        for occurrence in self.occurrences() {
            if !matches!(
                policy.consent_state(occurrence.location()),
                Ok(ConsentState::IncludedHere { .. } | ConsentState::InheritedFromParent { .. })
            ) {
                continue;
            }
            if !self.feedable_scopes(policy, occurrence)? {
                continue;
            }
            decisions.push(WorkerFeedDecision {
                tenant_id: tenant_id.clone(),
                library_id: self.library_id(),
                document_id: occurrence.document_id(),
                occurrence_id: occurrence.id(),
                action: WorkerFeedAction::Upsert,
            });
        }
        Ok(decisions)
    }
}
