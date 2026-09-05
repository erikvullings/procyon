use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{EligibilityReasonCounts, LibraryId, RootId};

/// Current durable semantic library runtime-state schema.
pub const CURRENT_LIBRARY_STATE_SCHEMA_VERSION: u32 = 2;

/// Durable pause state, last complete indexed generations, and the skip
/// reasons that were disclosed for each enrolled root.
///
/// Consent lives in [`crate::SemanticLibraryPolicy`], deliberately separate
/// from this operational state. Everything here is low-volume per-root
/// metadata, which is why it belongs in this small document beneath the
/// semantic-data root rather than in the high-volume catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticLibraryState {
    schema_version: u32,
    library_id: LibraryId,
    paused: bool,
    indexed_generations: BTreeMap<RootId, u64>,
    #[serde(default)]
    eligibility_reason_counts: BTreeMap<RootId, EligibilityReasonCounts>,
}

impl SemanticLibraryState {
    /// Creates active runtime state for a library.
    #[must_use]
    pub fn new(library_id: LibraryId) -> Self {
        Self {
            schema_version: CURRENT_LIBRARY_STATE_SCHEMA_VERSION,
            library_id,
            paused: false,
            indexed_generations: BTreeMap::new(),
            eligibility_reason_counts: BTreeMap::new(),
        }
    }

    /// Returns the owning library.
    #[must_use]
    pub const fn library_id(&self) -> LibraryId {
        self.library_id
    }

    /// Pauses ingestion without changing consent or indexed generations.
    pub const fn pause(&mut self) {
        self.paused = true;
    }

    /// Resumes ingestion.
    pub const fn resume(&mut self) {
        self.paused = false;
    }

    /// Reports whether ingestion is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Records a newly completed generation without permitting regression.
    ///
    /// # Errors
    ///
    /// Rejects zero, repeated, or decreasing generation numbers.
    pub fn record_indexed_generation(
        &mut self,
        root_id: RootId,
        generation: u64,
    ) -> Result<(), LibraryStateError> {
        if generation == 0
            || self
                .indexed_generations
                .get(&root_id)
                .is_some_and(|current| generation <= *current)
        {
            return Err(LibraryStateError::GenerationRegression);
        }
        self.indexed_generations.insert(root_id, generation);
        Ok(())
    }

    /// Returns the last complete indexed generation for a root.
    #[must_use]
    pub fn indexed_generation(&self, root_id: RootId) -> u64 {
        self.indexed_generations.get(&root_id).copied().unwrap_or(0)
    }

    /// Replaces the disclosed skip-reason counts of one root.
    ///
    /// The counts are written inside the same durable transaction as the
    /// consent change that produced them, so a restart shows what the user was
    /// actually told rather than an empty list.
    pub fn set_eligibility_reason_counts(
        &mut self,
        root_id: RootId,
        counts: EligibilityReasonCounts,
    ) {
        if counts.is_empty() {
            self.eligibility_reason_counts.remove(&root_id);
        } else {
            self.eligibility_reason_counts.insert(root_id, counts);
        }
    }

    /// Returns the disclosed skip-reason counts of one root.
    #[must_use]
    pub fn eligibility_reason_counts(&self, root_id: RootId) -> Option<&EligibilityReasonCounts> {
        self.eligibility_reason_counts.get(&root_id)
    }

    /// Drops per-root metadata for roots the policy no longer has.
    pub fn retain_roots(&mut self, retained: impl Fn(RootId) -> bool) {
        self.eligibility_reason_counts
            .retain(|root_id, _| retained(*root_id));
    }

    /// Migrates a stored runtime-state document to the current schema.
    ///
    /// Version one predates durable skip-reason counts, so it migrates to an
    /// explicitly empty map: the previous process kept those counts only in
    /// memory, and inventing values would be worse than reporting none.
    ///
    /// # Errors
    ///
    /// Rejects a document without a readable schema version, a version this
    /// build does not know, and a document that fails validation afterwards.
    pub(crate) fn migrate(mut value: Value) -> Result<Self, LibraryStateError> {
        let version = value
            .get("schemaVersion")
            .and_then(Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .ok_or(LibraryStateError::MalformedDocument)?;
        match version {
            1 => {
                value["eligibilityReasonCounts"] = Value::Object(serde_json::Map::new());
                value["schemaVersion"] = Value::from(CURRENT_LIBRARY_STATE_SCHEMA_VERSION);
            }
            CURRENT_LIBRARY_STATE_SCHEMA_VERSION => {}
            unsupported => return Err(LibraryStateError::UnsupportedSchema(unsupported)),
        }
        let state: Self =
            serde_json::from_value(value).map_err(|_| LibraryStateError::MalformedDocument)?;
        state.validate()?;
        Ok(state)
    }

    pub(crate) fn validate(&self) -> Result<(), LibraryStateError> {
        if self.schema_version != CURRENT_LIBRARY_STATE_SCHEMA_VERSION {
            return Err(LibraryStateError::UnsupportedSchema(self.schema_version));
        }
        if self
            .indexed_generations
            .values()
            .any(|generation| *generation == 0)
        {
            return Err(LibraryStateError::GenerationRegression);
        }
        Ok(())
    }
}

/// Durable runtime-state validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LibraryStateError {
    /// State schema is unsupported.
    #[error("unsupported semantic library state schema version {0}")]
    UnsupportedSchema(u32),
    /// The stored document could not be read as runtime state.
    #[error("semantic library state document is malformed")]
    MalformedDocument,
    /// A completed indexed generation was zero or regressed.
    #[error("semantic indexed generation must increase")]
    GenerationRegression,
}
