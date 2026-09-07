#![allow(clippy::unwrap_used, missing_docs)]

use fm_domain::Location;
use fm_semantic_library::{
    BudgetAssessment, DeviceLibraryIdentity, EligibilityDecision, EligibilityReason,
    EligibilityReasonCounts, EnrolledRoot, EnrolmentEstimate, EnrolmentEstimator, EstimateError,
    LibraryId, ModelIdentity, PreviewError, ResourceBudgets, ResourceProfile, ResourceProfileKind,
    RootId, SemanticLibraryPolicy,
};
use uuid::Uuid;

struct FixedEstimator(EnrolmentEstimate);

impl EnrolmentEstimator for FixedEstimator {
    fn estimate(&self, _root: &EnrolledRoot) -> Result<EnrolmentEstimate, EstimateError> {
        Ok(self.0.clone())
    }
}

fn policy_with_root() -> (SemanticLibraryPolicy, RootId) {
    let root_id = RootId::from_uuid(Uuid::from_u128(10));
    let mut policy = SemanticLibraryPolicy::new(
        DeviceLibraryIdentity::new(
            LibraryId::from_uuid(Uuid::from_u128(1)),
            ModelIdentity::new("model", "revision", 384, "space").unwrap(),
        ),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets {
                max_documents: 100,
                max_source_bytes_per_document: 1000,
                max_total_source_bytes: 10_000,
                max_total_extracted_bytes: 10_000,
                max_total_vector_bytes: 10_000,
            },
        },
    )
    .unwrap();
    policy
        .enrol_root(EnrolledRoot::new(
            root_id,
            Location::parse("file:///library").unwrap(),
            None,
            true,
        ))
        .unwrap();
    (policy, root_id)
}

#[test]
fn injected_estimate_produces_complete_local_retention_preview() {
    let (policy, root_id) = policy_with_root();
    let skipped = EligibilityReasonCounts::from_decisions([
        EligibilityDecision::Skipped([EligibilityReason::UnsupportedMime].into()),
        EligibilityDecision::Skipped([EligibilityReason::Hidden].into()),
        EligibilityDecision::Skipped([EligibilityReason::UnsupportedMime].into()),
    ]);
    let estimator = FixedEstimator(EnrolmentEstimate {
        estimated_files: 42,
        estimated_source_bytes: 100,
        estimated_extracted_bytes: 20,
        estimated_vector_bytes: 10,
        skipped_reason_counts: skipped,
        missing_model_download_bytes: 5,
    });

    let preview = policy
        .preview_enrolment(root_id, &estimator)
        .expect("preview");

    assert_eq!(preview.estimated_files(), 42);
    assert_eq!(preview.estimated_source_bytes(), 100);
    assert_eq!(preview.estimated_extracted_bytes(), 20);
    assert_eq!(preview.estimated_vector_bytes(), 10);
    assert_eq!(preview.missing_model_download_bytes(), 5);
    assert_eq!(preview.estimated_additional_local_bytes(), 35);
    assert_eq!(
        preview
            .skipped_reason_counts()
            .get(EligibilityReason::UnsupportedMime),
        2
    );
    assert_eq!(
        preview.estimated_budget_assessment(),
        &BudgetAssessment::WithinBudget
    );
    assert!(preview.normalized_excerpts_retained_locally());
}

#[test]
fn preview_reports_each_exceeded_hard_budget() {
    let (policy, root_id) = policy_with_root();
    let estimator = FixedEstimator(EnrolmentEstimate {
        estimated_files: 101,
        estimated_source_bytes: 10_001,
        estimated_extracted_bytes: 10_001,
        estimated_vector_bytes: 10_001,
        skipped_reason_counts: EligibilityReasonCounts::default(),
        missing_model_download_bytes: 0,
    });

    let preview = policy.preview_enrolment(root_id, &estimator).unwrap();

    assert_eq!(
        preview.estimated_budget_assessment(),
        &BudgetAssessment::ExceedsBudget(
            [
                fm_semantic_library::BudgetKind::Documents,
                fm_semantic_library::BudgetKind::SourceBytes,
                fm_semantic_library::BudgetKind::ExtractedBytes,
                fm_semantic_library::BudgetKind::VectorBytes,
            ]
            .into()
        )
    );
}

#[test]
fn preview_rejects_storage_estimate_overflow() {
    let (policy, root_id) = policy_with_root();
    let estimator = FixedEstimator(EnrolmentEstimate {
        estimated_files: 1,
        estimated_source_bytes: 1,
        estimated_extracted_bytes: u64::MAX,
        estimated_vector_bytes: 1,
        skipped_reason_counts: EligibilityReasonCounts::default(),
        missing_model_download_bytes: 0,
    });

    assert!(matches!(
        policy.preview_enrolment(root_id, &estimator),
        Err(PreviewError::EstimateOverflow)
    ));
}
