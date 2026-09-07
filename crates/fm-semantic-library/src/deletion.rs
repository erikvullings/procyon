use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hierarchy::is_same_or_descendant;
use crate::{
    ConversationPinId, DeletionPlanId, DerivedArtifactId, DocumentId, ExclusionCleanupStatus,
    ExclusionId, OccurrenceId, OccurrenceRecord, OccurrenceScope, PolicyError, RootId,
    SemanticCatalog, SemanticLibraryPolicy,
};

/// Saved-conversation evidence pin scoped to one authorized occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationEvidencePin {
    id: ConversationPinId,
    occurrence_id: OccurrenceId,
    scope: OccurrenceScope,
}

impl ConversationEvidencePin {
    /// Creates a scoped evidence pin.
    #[must_use]
    pub const fn new(
        id: ConversationPinId,
        occurrence_id: OccurrenceId,
        scope: OccurrenceScope,
    ) -> Self {
        Self {
            id,
            occurrence_id,
            scope,
        }
    }

    /// Returns the pin id.
    #[must_use]
    pub const fn id(&self) -> ConversationPinId {
        self.id
    }

    /// Returns the pinned occurrence.
    #[must_use]
    pub const fn occurrence_id(&self) -> OccurrenceId {
        self.occurrence_id
    }

    /// Returns the workspace/root scope that owns the pin.
    #[must_use]
    pub const fn scope(&self) -> OccurrenceScope {
        self.scope
    }

    pub(crate) fn relocate(&mut self, remapped: &BTreeMap<OccurrenceId, OccurrenceId>) {
        if let Some(occurrence_id) = remapped.get(&self.occurrence_id) {
            self.occurrence_id = *occurrence_id;
        }
    }
}

/// Mandatory, independently resumable deletion category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeletionCategory {
    /// Provider occurrence records in the excluded subtree.
    Occurrences,
    /// Retained normalized excerpts whose document becomes orphaned.
    ExtractedContent,
    /// Derived summaries whose document becomes orphaned.
    Summaries,
    /// Labels whose document becomes orphaned.
    Labels,
    /// Vectors no longer referenced by any occurrence.
    OrphanVectors,
    /// Saved-conversation evidence pins in the revoked scope.
    ConversationEvidencePins,
}

const DELETION_CATEGORIES: [DeletionCategory; 6] = [
    DeletionCategory::Occurrences,
    DeletionCategory::ExtractedContent,
    DeletionCategory::Summaries,
    DeletionCategory::Labels,
    DeletionCategory::OrphanVectors,
    DeletionCategory::ConversationEvidencePins,
];

impl DeletionCategory {
    /// Returns every mandatory category in stable processing order.
    #[must_use]
    pub const fn all() -> &'static [Self; 6] {
        &DELETION_CATEGORIES
    }
}

/// Immutable destructive inventory captured when exclusion becomes effective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionDeletionInventory {
    occurrences: BTreeSet<OccurrenceId>,
    orphan_documents: BTreeSet<DocumentId>,
    extracted_content: BTreeSet<DerivedArtifactId>,
    summaries: BTreeSet<DerivedArtifactId>,
    labels: BTreeSet<DerivedArtifactId>,
    orphan_vectors: BTreeSet<DerivedArtifactId>,
    conversation_evidence_pins: BTreeSet<ConversationPinId>,
}

impl ExclusionDeletionInventory {
    /// Returns occurrences inside the revoked subtree.
    #[must_use]
    pub const fn occurrences(&self) -> &BTreeSet<OccurrenceId> {
        &self.occurrences
    }

    /// Returns documents that had no occurrence outside the revoked scope
    /// when the plan was captured.
    ///
    /// Membership here is not authority to delete: cleanup rechecks live
    /// catalog references before every artifact mutation.
    #[must_use]
    pub const fn orphan_documents(&self) -> &BTreeSet<DocumentId> {
        &self.orphan_documents
    }

    /// Returns normalized excerpt records that become orphaned.
    #[must_use]
    pub const fn extracted_content(&self) -> &BTreeSet<DerivedArtifactId> {
        &self.extracted_content
    }

    /// Returns summaries that become orphaned.
    #[must_use]
    pub const fn summaries(&self) -> &BTreeSet<DerivedArtifactId> {
        &self.summaries
    }

    /// Returns labels that become orphaned.
    #[must_use]
    pub const fn labels(&self) -> &BTreeSet<DerivedArtifactId> {
        &self.labels
    }

    /// Returns vectors that have no remaining occurrence.
    #[must_use]
    pub const fn orphan_vectors(&self) -> &BTreeSet<DerivedArtifactId> {
        &self.orphan_vectors
    }

    /// Returns conversation evidence pins in the revoked scope.
    #[must_use]
    pub const fn conversation_evidence_pins(&self) -> &BTreeSet<ConversationPinId> {
        &self.conversation_evidence_pins
    }

    fn count(&self, category: DeletionCategory) -> u64 {
        let count = match category {
            DeletionCategory::Occurrences => self.occurrences.len(),
            DeletionCategory::ExtractedContent => self.extracted_content.len(),
            DeletionCategory::Summaries => self.summaries.len(),
            DeletionCategory::Labels => self.labels.len(),
            DeletionCategory::OrphanVectors => self.orphan_vectors.len(),
            DeletionCategory::ConversationEvidencePins => self.conversation_evidence_pins.len(),
        };
        u64::try_from(count).unwrap_or(u64::MAX)
    }
}

/// Durable progress for one deletion category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionCategoryProgress {
    total_items: u64,
    #[serde(default)]
    completed_items: u64,
    complete: bool,
    last_error: Option<String>,
}

impl DeletionCategoryProgress {
    /// Returns planned item count.
    #[must_use]
    pub const fn total_items(&self) -> u64 {
        self.total_items
    }

    /// Returns durably checkpointed item progress.
    #[must_use]
    pub const fn completed_items(&self) -> u64 {
        self.completed_items
    }

    /// Reports whether this category committed.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    /// Returns the last visible failure.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

/// Overall resumable deletion status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionPlanStatus {
    /// Cleanup has pending categories.
    Running,
    /// At least one category failed and is waiting for resume.
    Failed,
    /// Every mandatory category committed.
    Complete,
}

/// Durable explicit-exclusion deletion plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionDeletionPlan {
    id: DeletionPlanId,
    root_id: RootId,
    exclusion_id: ExclusionId,
    inventory: ExclusionDeletionInventory,
    progress: BTreeMap<DeletionCategory, DeletionCategoryProgress>,
}

impl ExclusionDeletionPlan {
    /// Returns the plan id.
    #[must_use]
    pub const fn id(&self) -> DeletionPlanId {
        self.id
    }

    /// Returns the immutable deletion inventory.
    #[must_use]
    pub const fn inventory(&self) -> &ExclusionDeletionInventory {
        &self.inventory
    }

    /// Returns one category's durable progress.
    #[must_use]
    pub fn progress(&self, category: DeletionCategory) -> &DeletionCategoryProgress {
        self.progress
            .get(&category)
            .expect("all mandatory deletion categories are initialized")
    }

    /// Derives overall state from durable category progress.
    #[must_use]
    pub fn status(&self) -> DeletionPlanStatus {
        if self
            .progress
            .values()
            .any(|progress| progress.last_error.is_some())
        {
            DeletionPlanStatus::Failed
        } else if self.progress.values().all(|progress| progress.complete) {
            DeletionPlanStatus::Complete
        } else {
            DeletionPlanStatus::Running
        }
    }

    pub(crate) fn is_structurally_valid(&self, key: DeletionPlanId) -> bool {
        key == self.id
            && self.progress.len() == DELETION_CATEGORIES.len()
            && DELETION_CATEGORIES.iter().all(|category| {
                self.progress.get(category).is_some_and(|progress| {
                    progress.completed_items <= progress.total_items
                        && (!progress.complete || progress.completed_items == progress.total_items)
                        && !(progress.complete && progress.last_error.is_some())
                })
            })
    }

    pub(crate) fn relocate(&mut self, remapped: &BTreeMap<OccurrenceId, OccurrenceId>) {
        self.inventory.occurrences = self
            .inventory
            .occurrences
            .iter()
            .map(|id| remapped.get(id).copied().unwrap_or(*id))
            .collect();
    }
}

impl SemanticCatalog {
    /// Adds a saved-conversation evidence pin to an existing occurrence scope.
    ///
    /// # Errors
    ///
    /// Rejects duplicate pins or a scope not present on the occurrence.
    pub fn add_conversation_pin(
        &mut self,
        pin: ConversationEvidencePin,
    ) -> Result<(), DeletionError> {
        let occurrence = self
            .occurrences
            .get(&pin.occurrence_id)
            .ok_or(DeletionError::UnknownOccurrence)?;
        if !occurrence.scopes.contains(&pin.scope) {
            return Err(DeletionError::ScopeMismatch);
        }
        if self.conversation_pins.contains_key(&pin.id) {
            return Err(DeletionError::DuplicatePin);
        }
        self.conversation_pins.insert(pin.id, pin);
        Ok(())
    }

    /// Returns retained evidence pin count.
    #[must_use]
    pub fn conversation_pin_count(&self) -> usize {
        self.conversation_pins.len()
    }

    /// Plans every deletion category and attaches it to an immediately
    /// effective pending exclusion.
    ///
    /// # Errors
    ///
    /// Rejects unknown/complete exclusions, duplicate plans, and malformed
    /// provider locations.
    pub fn begin_exclusion_cleanup(
        &mut self,
        policy: &mut SemanticLibraryPolicy,
        root_id: RootId,
        exclusion_id: ExclusionId,
    ) -> Result<DeletionPlanId, DeletionError> {
        self.ensure_policy_library(policy)
            .map_err(|_| DeletionError::Policy(PolicyError::LibraryMismatch))?;
        let exclusion = policy
            .exclusion(root_id, exclusion_id)
            .ok_or(PolicyError::UnknownExclusion(exclusion_id))?;
        if exclusion.cleanup_status() != ExclusionCleanupStatus::Pending
            || exclusion.deletion_plan_id().is_some()
        {
            return Err(DeletionError::Policy(
                PolicyError::ExclusionCleanupAlreadyPlanned,
            ));
        }
        let excluded_location = exclusion.location().clone();
        let mut occurrences = BTreeSet::new();
        for (id, occurrence) in &self.occurrences {
            if is_same_or_descendant(&excluded_location, &occurrence.location)
                .map_err(PolicyError::InvalidLocation)?
            {
                occurrences.insert(*id);
            }
        }
        let affected_documents: BTreeSet<_> = occurrences
            .iter()
            .filter_map(|id| self.occurrences.get(id))
            .map(|occurrence| occurrence.document_id)
            .collect();
        let orphan_documents: BTreeSet<_> = affected_documents
            .into_iter()
            .filter(|document_id| {
                self.occurrences
                    .values()
                    .filter(|occurrence| occurrence.document_id == *document_id)
                    .all(|occurrence| occurrences.contains(&occurrence.id))
            })
            .collect();
        let mut extracted_content = BTreeSet::new();
        let mut summaries = BTreeSet::new();
        let mut labels = BTreeSet::new();
        let mut orphan_vectors = BTreeSet::new();
        for document_id in &orphan_documents {
            if let Some(document) = self.documents.get(document_id) {
                extracted_content.extend(document.artifacts.extracted_content.iter().copied());
                summaries.extend(document.artifacts.summaries.iter().copied());
                labels.extend(document.artifacts.labels.iter().copied());
                orphan_vectors.extend(document.artifacts.vectors.iter().copied());
            }
        }
        let conversation_evidence_pins = self
            .conversation_pins
            .values()
            .filter(|pin| occurrences.contains(&pin.occurrence_id))
            .map(|pin| pin.id)
            .collect();
        let inventory = ExclusionDeletionInventory {
            occurrences,
            orphan_documents,
            extracted_content,
            summaries,
            labels,
            orphan_vectors,
            conversation_evidence_pins,
        };
        let progress = DeletionCategory::all()
            .iter()
            .copied()
            .map(|category| {
                (
                    category,
                    DeletionCategoryProgress {
                        total_items: inventory.count(category),
                        completed_items: 0,
                        complete: false,
                        last_error: None,
                    },
                )
            })
            .collect();
        let plan_id = DeletionPlanId::new();
        policy
            .exclusion_mut(root_id, exclusion_id)?
            .attach_deletion_plan(plan_id)?;
        self.deletion_plans.insert(
            plan_id,
            ExclusionDeletionPlan {
                id: plan_id,
                root_id,
                exclusion_id,
                inventory,
                progress,
            },
        );
        Ok(plan_id)
    }

    /// Returns a durable deletion plan.
    #[must_use]
    pub fn deletion_plan(&self, plan_id: DeletionPlanId) -> Option<&ExclusionDeletionPlan> {
        self.deletion_plans.get(&plan_id)
    }

    /// Records a visible category failure without losing completed progress.
    ///
    /// # Errors
    ///
    /// Rejects unknown plans or already-complete categories.
    pub fn fail_deletion_category(
        &mut self,
        plan_id: DeletionPlanId,
        category: DeletionCategory,
        message: impl Into<String>,
    ) -> Result<(), DeletionError> {
        let progress = self
            .deletion_plans
            .get_mut(&plan_id)
            .ok_or(DeletionError::UnknownPlan)?
            .progress
            .get_mut(&category)
            .ok_or(DeletionError::UnknownCategory)?;
        if progress.complete {
            return Err(DeletionError::CategoryAlreadyComplete);
        }
        progress.last_error = Some(message.into());
        Ok(())
    }

    /// Durably advances item-level progress without committing the category.
    ///
    /// # Errors
    ///
    /// Rejects unknown plans/categories, failed or complete categories, and
    /// progress that regresses or reaches beyond the planned inventory.
    pub fn checkpoint_deletion_category(
        &mut self,
        plan_id: DeletionPlanId,
        category: DeletionCategory,
        completed_items: u64,
    ) -> Result<(), DeletionError> {
        let plan = self
            .deletion_plans
            .get_mut(&plan_id)
            .ok_or(DeletionError::UnknownPlan)?;
        if plan.status() == DeletionPlanStatus::Failed {
            return Err(DeletionError::PlanFailed);
        }
        let progress = plan
            .progress
            .get_mut(&category)
            .ok_or(DeletionError::UnknownCategory)?;
        if progress.complete {
            return Err(DeletionError::CategoryAlreadyComplete);
        }
        if completed_items <= progress.completed_items || completed_items >= progress.total_items {
            return Err(DeletionError::InvalidProgress);
        }
        progress.completed_items = completed_items;
        Ok(())
    }

    /// Clears failures while preserving completed categories.
    ///
    /// # Errors
    ///
    /// Rejects unknown plans or a plan that is not failed.
    pub fn resume_deletion(&mut self, plan_id: DeletionPlanId) -> Result<(), DeletionError> {
        let plan = self
            .deletion_plans
            .get_mut(&plan_id)
            .ok_or(DeletionError::UnknownPlan)?;
        if plan.status() != DeletionPlanStatus::Failed {
            return Err(DeletionError::PlanNotFailed);
        }
        for progress in plan.progress.values_mut() {
            progress.last_error = None;
        }
        Ok(())
    }

    /// Commits one category and marks the exclusion cleanup complete only
    /// after all mandatory categories have committed.
    ///
    /// The plan's inventory is an upper bound, never a licence: live
    /// authoritative references are rechecked immediately before each
    /// category mutation, so a document, excerpt, summary, label, or vector
    /// that gained an occurrence outside the revoked scope after the plan was
    /// created is retained.
    ///
    /// # Errors
    ///
    /// Rejects a library mismatch and unknown, failed, or mismatched plans.
    pub fn complete_deletion_category(
        &mut self,
        policy: &mut SemanticLibraryPolicy,
        plan_id: DeletionPlanId,
        category: DeletionCategory,
    ) -> Result<(), DeletionError> {
        self.ensure_policy_library(policy)
            .map_err(|_| DeletionError::Policy(PolicyError::LibraryMismatch))?;
        let plan = self
            .deletion_plans
            .get(&plan_id)
            .ok_or(DeletionError::UnknownPlan)?;
        let exclusion = policy
            .exclusion(plan.root_id, plan.exclusion_id)
            .ok_or(PolicyError::UnknownExclusion(plan.exclusion_id))?;
        if exclusion.deletion_plan_id() != Some(plan_id)
            || exclusion.cleanup_status() != ExclusionCleanupStatus::Pending
        {
            return Err(DeletionError::Policy(PolicyError::ExclusionPlanMismatch));
        }
        if plan.status() == DeletionPlanStatus::Failed {
            return Err(DeletionError::PlanFailed);
        }
        if plan.progress(category).complete {
            return Ok(());
        }
        let inventory = plan.inventory.clone();
        apply_category(self, &inventory, category);
        let plan = self
            .deletion_plans
            .get_mut(&plan_id)
            .expect("plan existence checked above");
        let progress = plan
            .progress
            .get_mut(&category)
            .ok_or(DeletionError::UnknownCategory)?;
        progress.complete = true;
        progress.completed_items = progress.total_items;
        progress.last_error = None;
        if plan.status() == DeletionPlanStatus::Complete {
            let root_id = plan.root_id;
            let exclusion_id = plan.exclusion_id;
            self.prune_unreferenced_documents();
            policy
                .exclusion_mut(root_id, exclusion_id)?
                .complete_cleanup(plan_id)?;
        }
        Ok(())
    }
}

fn apply_category(
    catalog: &mut SemanticCatalog,
    inventory: &ExclusionDeletionInventory,
    category: DeletionCategory,
) {
    let planned = match category {
        DeletionCategory::Occurrences => {
            catalog
                .occurrences
                .retain(|id, _| !inventory.occurrences.contains(id));
            return;
        }
        DeletionCategory::ConversationEvidencePins => {
            catalog
                .conversation_pins
                .retain(|id, _| !inventory.conversation_evidence_pins.contains(id));
            return;
        }
        DeletionCategory::ExtractedContent => &inventory.extracted_content,
        DeletionCategory::Summaries => &inventory.summaries,
        DeletionCategory::Labels => &inventory.labels,
        DeletionCategory::OrphanVectors => &inventory.orphan_vectors,
    };
    let retained = retained_artifacts(catalog, inventory);
    let removable: BTreeSet<_> = planned.difference(&retained).copied().collect();
    for document in catalog.documents.values_mut() {
        let artifacts = match category {
            DeletionCategory::ExtractedContent => &mut document.artifacts.extracted_content,
            DeletionCategory::Summaries => &mut document.artifacts.summaries,
            DeletionCategory::Labels => &mut document.artifacts.labels,
            DeletionCategory::OrphanVectors => &mut document.artifacts.vectors,
            DeletionCategory::Occurrences | DeletionCategory::ConversationEvidencePins => continue,
        };
        artifacts.retain(|id| !removable.contains(id));
    }
}

/// Returns every derived artifact still reachable from a document that has an
/// occurrence outside the revoked scope.
///
/// The plan's inventory was captured when the exclusion became effective. A
/// document that has since been observed elsewhere is no longer orphaned, so
/// its excerpts, summaries, labels, and vectors must survive the cleanup even
/// though the plan listed them.
fn retained_artifacts(
    catalog: &SemanticCatalog,
    inventory: &ExclusionDeletionInventory,
) -> BTreeSet<DerivedArtifactId> {
    let referenced_outside: BTreeSet<DocumentId> = catalog
        .occurrences()
        .filter(|occurrence| !inventory.occurrences.contains(&occurrence.id()))
        .map(OccurrenceRecord::document_id)
        .collect();
    let mut retained = BTreeSet::new();
    for document_id in &referenced_outside {
        if let Some(document) = catalog.document(*document_id) {
            retained.extend(document.artifacts().extracted_content.iter().copied());
            retained.extend(document.artifacts().summaries.iter().copied());
            retained.extend(document.artifacts().labels.iter().copied());
            retained.extend(document.artifacts().vectors.iter().copied());
        }
    }
    retained
}

/// Explicit exclusion deletion failure.
#[derive(Debug, Error)]
pub enum DeletionError {
    /// Policy or exclusion validation failed.
    #[error(transparent)]
    Policy(#[from] PolicyError),
    /// Evidence pin references an unknown occurrence.
    #[error("conversation evidence pin references an unknown occurrence")]
    UnknownOccurrence,
    /// Evidence pin scope does not belong to its occurrence.
    #[error("conversation evidence pin scope does not match its occurrence")]
    ScopeMismatch,
    /// Evidence pin id already exists.
    #[error("conversation evidence pin already exists")]
    DuplicatePin,
    /// Deletion plan does not exist.
    #[error("exclusion deletion plan does not exist")]
    UnknownPlan,
    /// A persisted plan omitted a mandatory category.
    #[error("exclusion deletion plan omitted a mandatory category")]
    UnknownCategory,
    /// A completed category cannot subsequently fail.
    #[error("exclusion deletion category is already complete")]
    CategoryAlreadyComplete,
    /// Failed plan must be explicitly resumed.
    #[error("exclusion deletion plan is failed")]
    PlanFailed,
    /// Only failed plans can be resumed.
    #[error("exclusion deletion plan is not failed")]
    PlanNotFailed,
    /// Item-level progress regressed or exceeded the pending range.
    #[error("exclusion deletion category progress is invalid")]
    InvalidProgress,
}
