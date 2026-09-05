use std::collections::{BTreeMap, BTreeSet};

use fm_domain::{Location, LocationError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ResourceBudgets;
use crate::hierarchy::is_same_or_descendant;

/// Stable reason why an entry is skipped by semantic enrolment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EligibilityReason {
    /// Hidden entry.
    Hidden,
    /// Operating-system-managed entry.
    System,
    /// Application or package bundle.
    ApplicationOrPackageBundle,
    /// Dependency directory such as `node_modules`.
    DependencyDirectory,
    /// Generated build directory.
    BuildDirectory,
    /// Cache directory.
    CacheDirectory,
    /// Entry ignored by repository ignore rules.
    GitIgnored,
    /// No installed converter supports the MIME type.
    UnsupportedMime,
    /// One file exceeds the per-document size ceiling.
    Oversized,
    /// Adding the entry would exceed a library budget.
    OverBudget,
    /// A symbolic-link target is outside the enrolled root.
    SymlinkOutsideRoot,
    /// Explicit root policy excludes the entry.
    ExplicitlyExcluded,
}

impl EligibilityReason {
    pub(crate) const fn can_be_explicitly_included(self) -> bool {
        matches!(
            self,
            Self::Hidden
                | Self::System
                | Self::ApplicationOrPackageBundle
                | Self::DependencyDirectory
                | Self::BuildDirectory
                | Self::CacheDirectory
                | Self::GitIgnored
        )
    }
}

/// Per-root override of a curated eligibility reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EligibilityOverride {
    /// Admit entries skipped only for this reason.
    Include,
    /// Ensure entries matching this reason remain excluded.
    Exclude,
}

/// Provider-neutral entry kind considered by the enrolment policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EligibilityEntryKind {
    /// Regular file.
    File,
    /// Directory whose traversal may be skipped.
    Directory,
    /// Symbolic link, admitted only when its resolved target is confined.
    Symlink,
}

/// Metadata supplied by the host while enumerating through a VFS provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibilityCandidate {
    /// Provider-neutral entry location, retained by Procyon and never sent to the worker.
    pub location: Location,
    /// Entry kind.
    pub kind: EligibilityEntryKind,
    /// Provider-reported hidden state.
    pub hidden: bool,
    /// Provider/platform-reported system state.
    pub system: bool,
    /// Whether the host identified an application or package bundle.
    pub application_or_package_bundle: bool,
    /// Whether repository ignore rules matched.
    pub git_ignored: bool,
    /// Detected MIME type.
    pub mime_type: Option<String>,
    /// Source byte size.
    pub source_bytes: u64,
    /// Estimated normalized extracted bytes.
    pub estimated_extracted_bytes: u64,
    /// Estimated vector bytes.
    pub estimated_vector_bytes: u64,
    /// Resolved symbolic-link target, when this is a link.
    pub symlink_target: Option<Location>,
}

/// Resource consumption used to *predict* skip reasons during enumeration.
///
/// This is an estimate-only input: it decides which entries are worth
/// offering to ingestion and what to show the user. Hard budgets and hard
/// quotas are enforced separately by
/// [`SemanticCatalog::upsert_observations`](crate::SemanticCatalog::upsert_observations)
/// from stored records, which never trusts a caller-supplied total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceUsage {
    documents: u64,
    source_bytes: u64,
    extracted_bytes: u64,
    vector_bytes: u64,
}

impl ResourceUsage {
    /// Measures consumption from the authoritative catalog.
    #[must_use]
    pub fn measure(catalog: &crate::SemanticCatalog) -> Self {
        let usage = catalog.measured_usage();
        Self {
            documents: usage.documents(),
            source_bytes: usage.source_bytes(),
            extracted_bytes: usage.extracted_bytes(),
            vector_bytes: usage.vector_bytes(),
        }
    }

    /// Creates an explicitly estimated consumption, for previews and for
    /// accumulating a projection while enumerating.
    #[must_use]
    pub const fn estimated(
        documents: u64,
        source_bytes: u64,
        extracted_bytes: u64,
        vector_bytes: u64,
    ) -> Self {
        Self {
            documents,
            source_bytes,
            extracted_bytes,
            vector_bytes,
        }
    }

    /// Returns documents already catalogued or projected.
    #[must_use]
    pub const fn documents(self) -> u64 {
        self.documents
    }

    /// Returns represented source bytes.
    #[must_use]
    pub const fn source_bytes(self) -> u64 {
        self.source_bytes
    }

    /// Returns retained normalized-excerpt bytes.
    #[must_use]
    pub const fn extracted_bytes(self) -> u64 {
        self.extracted_bytes
    }

    /// Returns retained vector bytes.
    #[must_use]
    pub const fn vector_bytes(self) -> u64 {
        self.vector_bytes
    }
}

/// Result of applying curated defaults and explicit overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EligibilityDecision {
    /// Entry may be offered to the host-side ingestion stream.
    Eligible,
    /// Entry is skipped for all listed reasons.
    Skipped(BTreeSet<EligibilityReason>),
}

/// Aggregated skip reason counts for previews and status reporting.
///
/// The value is durable: the counts a user saw when granting consent are part
/// of that disclosure, so they are persisted with the library's runtime state
/// rather than recomputed — or silently lost — on restart.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EligibilityReasonCounts(BTreeMap<EligibilityReason, u64>);

impl EligibilityReasonCounts {
    /// Reports whether no reason was counted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Aggregates skipped reasons from decisions in arbitrary order.
    #[must_use]
    pub fn from_decisions(decisions: impl IntoIterator<Item = EligibilityDecision>) -> Self {
        let mut counts = BTreeMap::new();
        for decision in decisions {
            if let EligibilityDecision::Skipped(reasons) = decision {
                for reason in reasons {
                    *counts.entry(reason).or_insert(0) += 1;
                }
            }
        }
        Self(counts)
    }

    /// Returns the count for a reason.
    #[must_use]
    pub fn get(&self, reason: EligibilityReason) -> u64 {
        self.0.get(&reason).copied().unwrap_or(0)
    }

    /// Returns all non-zero reason counts.
    #[must_use]
    pub const fn as_map(&self) -> &BTreeMap<EligibilityReason, u64> {
        &self.0
    }
}

/// Curated, deterministic eligibility policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibilityPolicy {
    supported_mime_types: BTreeSet<String>,
}

impl EligibilityPolicy {
    /// Creates the bounded baseline converter MIME allow-list.
    #[must_use]
    pub fn curated_defaults() -> Self {
        Self {
            supported_mime_types: [
                "application/json",
                "application/pdf",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "application/xml",
                "text/css",
                "text/csv",
                "text/html",
                "text/javascript",
                "text/markdown",
                "text/plain",
                "text/xml",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }

    /// Evaluates one host-supplied entry without accessing its filesystem path.
    ///
    /// The decision is advisory: it produces user-visible skip reasons and
    /// keeps hopeless candidates out of the ingestion stream. Ingestion
    /// re-checks every hard budget against stored records.
    ///
    /// # Errors
    ///
    /// Returns a typed location error when the entry is outside the root or
    /// provider hierarchy cannot be evaluated.
    pub fn evaluate(
        &self,
        root: &Location,
        candidate: &EligibilityCandidate,
        budgets: &ResourceBudgets,
        usage: ResourceUsage,
        overrides: &BTreeMap<EligibilityReason, EligibilityOverride>,
    ) -> Result<EligibilityDecision, EligibilityError> {
        if !is_same_or_descendant(root, &candidate.location)? {
            return Err(EligibilityError::OutsideRoot);
        }
        let mut reasons = BTreeSet::new();
        if candidate.hidden {
            reasons.insert(EligibilityReason::Hidden);
        }
        if candidate.system {
            reasons.insert(EligibilityReason::System);
        }
        if candidate.application_or_package_bundle {
            reasons.insert(EligibilityReason::ApplicationOrPackageBundle);
        }
        if candidate.git_ignored {
            reasons.insert(EligibilityReason::GitIgnored);
        }
        for component in relative_component_names(root, &candidate.location)? {
            let component = component.to_ascii_lowercase();
            if matches!(component.as_str(), "node_modules" | "vendor" | ".venv") {
                reasons.insert(EligibilityReason::DependencyDirectory);
            }
            if matches!(component.as_str(), "target" | "build" | "dist" | "out") {
                reasons.insert(EligibilityReason::BuildDirectory);
            }
            if matches!(
                component.as_str(),
                ".cache" | "__pycache__" | ".gradle" | ".mypy_cache"
            ) {
                reasons.insert(EligibilityReason::CacheDirectory);
            }
        }
        if candidate.kind == EligibilityEntryKind::File
            && !candidate
                .mime_type
                .as_ref()
                .is_some_and(|mime| self.supported_mime_types.contains(mime))
        {
            reasons.insert(EligibilityReason::UnsupportedMime);
        }
        if candidate.source_bytes > budgets.max_source_bytes_per_document {
            reasons.insert(EligibilityReason::Oversized);
        }
        if exceeds_budgets(candidate, budgets, usage) {
            reasons.insert(EligibilityReason::OverBudget);
        }
        if candidate.kind == EligibilityEntryKind::Symlink
            && !candidate
                .symlink_target
                .as_ref()
                .is_some_and(|target| symlink_target_is_confined(root, target))
        {
            reasons.insert(EligibilityReason::SymlinkOutsideRoot);
        }
        for (reason, action) in overrides {
            if *action == EligibilityOverride::Include && reason.can_be_explicitly_included() {
                reasons.remove(reason);
            }
        }
        if reasons.is_empty() {
            Ok(EligibilityDecision::Eligible)
        } else {
            Ok(EligibilityDecision::Skipped(reasons))
        }
    }
}

fn exceeds_budgets(
    candidate: &EligibilityCandidate,
    budgets: &ResourceBudgets,
    usage: ResourceUsage,
) -> bool {
    usage
        .documents
        .checked_add(1)
        .is_none_or(|total| total > budgets.max_documents)
        || usage
            .source_bytes
            .checked_add(candidate.source_bytes)
            .is_none_or(|total| total > budgets.max_total_source_bytes)
        || usage
            .extracted_bytes
            .checked_add(candidate.estimated_extracted_bytes)
            .is_none_or(|total| total > budgets.max_total_extracted_bytes)
        || usage
            .vector_bytes
            .checked_add(candidate.estimated_vector_bytes)
            .is_none_or(|total| total > budgets.max_total_vector_bytes)
}

fn relative_component_names(
    root: &Location,
    candidate: &Location,
) -> Result<Vec<String>, EligibilityError> {
    let mut names = Vec::new();
    let mut current = candidate.clone();
    while current != *root {
        names.push(current.name()?);
        current = current.parent()?.ok_or(EligibilityError::OutsideRoot)?;
    }
    Ok(names)
}

/// Returns whether a resolved symlink target remains within the enrolled root.
#[must_use]
pub fn symlink_target_is_confined(root: &Location, target: &Location) -> bool {
    is_same_or_descendant(root, target).unwrap_or(false)
}

/// Eligibility location failure.
#[derive(Debug, Error)]
pub enum EligibilityError {
    /// Candidate does not belong to the supplied enrolled root.
    #[error("eligibility candidate is outside its enrolled root")]
    OutsideRoot,
    /// Provider-aware location traversal failed.
    #[error("eligibility location is invalid: {0}")]
    Location(#[from] LocationError),
}
