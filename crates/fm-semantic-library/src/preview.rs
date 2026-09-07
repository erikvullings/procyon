use std::collections::BTreeSet;

use thiserror::Error;

use crate::{
    EligibilityReasonCounts, EnrolledRoot, PolicyError, ResourceBudgets, RootId,
    SemanticLibraryPolicy,
};

/// Host-supplied enrolment estimate. The policy core performs no crawling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolmentEstimate {
    /// Estimated eligible file count.
    pub estimated_files: u64,
    /// Estimated source bytes inspected by the host.
    pub estimated_source_bytes: u64,
    /// Estimated normalized excerpts retained locally.
    pub estimated_extracted_bytes: u64,
    /// Estimated local vector storage.
    pub estimated_vector_bytes: u64,
    /// Unsupported and otherwise skipped entries by reason.
    pub skipped_reason_counts: EligibilityReasonCounts,
    /// Model bytes still requiring an explicit component download.
    pub missing_model_download_bytes: u64,
}

/// Injected provider-side estimation boundary.
pub trait EnrolmentEstimator {
    /// Estimates an enrolled root without giving a worker filesystem access.
    ///
    /// # Errors
    ///
    /// Returns a provider-specific estimate failure represented without paths
    /// or credentials.
    fn estimate(&self, root: &EnrolledRoot) -> Result<EnrolmentEstimate, EstimateError>;
}

/// Opaque estimator failure suitable for a user-visible preview error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("enrolment estimate failed: {message}")]
pub struct EstimateError {
    message: String,
}

impl EstimateError {
    /// Creates an estimator failure without retaining provider secrets.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Hard resource budget, used both by estimate-only previews and by
/// authoritative ingestion enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetKind {
    /// Document count.
    Documents,
    /// Source bytes of a single document.
    SourceBytesPerDocument,
    /// Cumulative source bytes.
    SourceBytes,
    /// Cumulative normalized excerpt bytes.
    ExtractedBytes,
    /// Cumulative vector bytes.
    VectorBytes,
}

/// Estimate-only budget result shown before consent is granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetAssessment {
    /// The estimate fits all hard limits.
    WithinBudget,
    /// One or more hard limits would be exceeded.
    ExceedsBudget(BTreeSet<BudgetKind>),
}

/// Fully validated enrolment preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolmentPreview {
    estimate: EnrolmentEstimate,
    estimated_additional_local_bytes: u64,
    budget_assessment: BudgetAssessment,
}

impl EnrolmentPreview {
    /// Returns estimated eligible files.
    #[must_use]
    pub const fn estimated_files(&self) -> u64 {
        self.estimate.estimated_files
    }

    /// Returns estimated source bytes.
    #[must_use]
    pub const fn estimated_source_bytes(&self) -> u64 {
        self.estimate.estimated_source_bytes
    }

    /// Returns estimated normalized excerpt bytes.
    #[must_use]
    pub const fn estimated_extracted_bytes(&self) -> u64 {
        self.estimate.estimated_extracted_bytes
    }

    /// Returns estimated vector bytes.
    #[must_use]
    pub const fn estimated_vector_bytes(&self) -> u64 {
        self.estimate.estimated_vector_bytes
    }

    /// Returns skipped and unsupported counts.
    #[must_use]
    pub const fn skipped_reason_counts(&self) -> &EligibilityReasonCounts {
        &self.estimate.skipped_reason_counts
    }

    /// Returns missing model download bytes.
    #[must_use]
    pub const fn missing_model_download_bytes(&self) -> u64 {
        self.estimate.missing_model_download_bytes
    }

    /// Returns extracted, vector, and missing-model local storage.
    #[must_use]
    pub const fn estimated_additional_local_bytes(&self) -> u64 {
        self.estimated_additional_local_bytes
    }

    /// Returns the estimate-only budget assessment shown before consent.
    ///
    /// The numbers come from an injected host estimator and are disclosure,
    /// not enforcement: ingestion recomputes every budget from stored catalog
    /// records and refuses observations that would exceed one.
    #[must_use]
    pub const fn estimated_budget_assessment(&self) -> &BudgetAssessment {
        &self.budget_assessment
    }

    /// Discloses that normalized excerpts are retained on this device.
    #[must_use]
    pub const fn normalized_excerpts_retained_locally(&self) -> bool {
        true
    }
}

pub(crate) fn preview(
    policy: &SemanticLibraryPolicy,
    root_id: RootId,
    estimator: &impl EnrolmentEstimator,
) -> Result<EnrolmentPreview, PreviewError> {
    let root = policy
        .root(root_id)
        .ok_or(PolicyError::UnknownRoot(root_id))?;
    let estimate = estimator.estimate(root)?;
    let local_bytes = estimate
        .estimated_extracted_bytes
        .checked_add(estimate.estimated_vector_bytes)
        .and_then(|bytes| bytes.checked_add(estimate.missing_model_download_bytes))
        .ok_or(PreviewError::EstimateOverflow)?;
    let budget_assessment = assess_budgets(&estimate, &policy.resource_profile().budgets);
    Ok(EnrolmentPreview {
        estimate,
        estimated_additional_local_bytes: local_bytes,
        budget_assessment,
    })
}

fn assess_budgets(estimate: &EnrolmentEstimate, budgets: &ResourceBudgets) -> BudgetAssessment {
    let mut exceeded = BTreeSet::new();
    if estimate.estimated_files > budgets.max_documents {
        exceeded.insert(BudgetKind::Documents);
    }
    if estimate.estimated_source_bytes > budgets.max_total_source_bytes {
        exceeded.insert(BudgetKind::SourceBytes);
    }
    if estimate.estimated_extracted_bytes > budgets.max_total_extracted_bytes {
        exceeded.insert(BudgetKind::ExtractedBytes);
    }
    if estimate.estimated_vector_bytes > budgets.max_total_vector_bytes {
        exceeded.insert(BudgetKind::VectorBytes);
    }
    if exceeded.is_empty() {
        BudgetAssessment::WithinBudget
    } else {
        BudgetAssessment::ExceedsBudget(exceeded)
    }
}

/// Enrolment preview failure.
#[derive(Debug, Error)]
pub enum PreviewError {
    /// Policy validation or root lookup failed.
    #[error(transparent)]
    Policy(#[from] PolicyError),
    /// Injected provider estimator failed.
    #[error(transparent)]
    Estimate(#[from] EstimateError),
    /// Estimated local storage overflowed `u64`.
    #[error("enrolment storage estimate overflowed")]
    EstimateOverflow,
}
