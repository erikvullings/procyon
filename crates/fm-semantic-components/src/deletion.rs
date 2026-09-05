use thiserror::Error;

/// Opaque identity of an enrolled semantic source.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnrolmentId(String);

impl EnrolmentId {
    /// Creates a non-empty path-independent enrolment identifier.
    ///
    /// # Errors
    ///
    /// Returns [`EnrolmentDeletionError::InvalidEnrolmentId`] for an empty or
    /// unsafe value.
    pub fn new(value: impl Into<String>) -> Result<Self, EnrolmentDeletionError> {
        let value = value.into();
        if value.is_empty()
            || matches!(value.as_str(), "." | "..")
            || value.ends_with('.')
            || crate::catalog::is_windows_reserved_component(&value)
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(EnrolmentDeletionError::InvalidEnrolmentId);
        }
        Ok(Self(value))
    }

    /// Returns the opaque enrolment identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Mandatory category of enrolment-derived deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionTarget {
    /// Index records and enrolment metadata.
    Index,
    /// Extracted text/content.
    Extracted,
    /// Zvec vector records.
    Zvec,
    /// Reusable embedding-cache entries.
    EmbeddingCache,
    /// Saved-conversation evidence derived from the enrolment.
    ConversationEvidence,
}

const DELETION_TARGETS: [DeletionTarget; 5] = [
    DeletionTarget::Index,
    DeletionTarget::Extracted,
    DeletionTarget::Zvec,
    DeletionTarget::EmbeddingCache,
    DeletionTarget::ConversationEvidence,
];

/// Per-category enrolment-derived records expected or actually deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnrolmentDeletionCounts {
    /// Index records and enrolment metadata.
    pub index_records: u64,
    /// Extracted content files.
    pub extracted_files: u64,
    /// Zvec vectors.
    pub zvec_vectors: u64,
    /// Embedding-cache entries.
    pub cache_entries: u64,
    /// Saved-conversation evidence records.
    pub conversation_evidence: u64,
}

/// Immutable deletion plan with no option to preserve conversation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolmentDeletionPlan {
    enrolment_id: EnrolmentId,
    expected: EnrolmentDeletionCounts,
}

impl EnrolmentDeletionPlan {
    /// Creates a plan covering every enrolment-derived data category.
    #[must_use]
    pub const fn new(enrolment_id: EnrolmentId, expected: EnrolmentDeletionCounts) -> Self {
        Self {
            enrolment_id,
            expected,
        }
    }

    /// Returns the enrolment whose derived data must be removed.
    #[must_use]
    pub const fn enrolment_id(&self) -> &EnrolmentId {
        &self.enrolment_id
    }

    /// Returns all mandatory deletion targets.
    #[must_use]
    pub const fn targets(&self) -> &'static [DeletionTarget; 5] {
        &DELETION_TARGETS
    }

    /// Returns expected records by category.
    #[must_use]
    pub const fn expected(&self) -> EnrolmentDeletionCounts {
        self.expected
    }

    /// Reports the invariant that saved-conversation evidence cannot be retained.
    #[must_use]
    pub const fn requires_conversation_evidence_deletion(&self) -> bool {
        true
    }

    /// Completes the plan only when every expected category, including saved
    /// conversation evidence, was deleted.
    ///
    /// # Errors
    ///
    /// Returns [`EnrolmentDeletionError::Incomplete`] when any actual category
    /// differs from the authoritative plan.
    pub fn complete(
        &self,
        deleted: EnrolmentDeletionCounts,
    ) -> Result<EnrolmentDeletionResult, EnrolmentDeletionError> {
        if deleted != self.expected {
            return Err(EnrolmentDeletionError::Incomplete);
        }
        Ok(EnrolmentDeletionResult {
            enrolment_id: self.enrolment_id.clone(),
            deleted,
        })
    }
}

/// Verified result covering all enrolment-derived categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolmentDeletionResult {
    enrolment_id: EnrolmentId,
    deleted: EnrolmentDeletionCounts,
}

impl EnrolmentDeletionResult {
    /// Returns the deleted enrolment.
    #[must_use]
    pub const fn enrolment_id(&self) -> &EnrolmentId {
        &self.enrolment_id
    }

    /// Returns every category covered by this result.
    #[must_use]
    pub const fn deleted_targets(&self) -> &'static [DeletionTarget; 5] {
        &DELETION_TARGETS
    }

    /// Returns actual deletion counts.
    #[must_use]
    pub const fn deleted(&self) -> EnrolmentDeletionCounts {
        self.deleted
    }

    /// Reports that saved-conversation evidence was included in completion.
    #[must_use]
    pub const fn conversation_evidence_deleted(&self) -> bool {
        true
    }
}

/// Enrolment-derived deletion validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EnrolmentDeletionError {
    /// Enrolment identifier was empty or unsafe.
    #[error("enrolment identifier is invalid")]
    InvalidEnrolmentId,
    /// At least one mandatory category was not fully deleted.
    #[error("enrolment-derived deletion is incomplete")]
    Incomplete,
}
