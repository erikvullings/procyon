#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::BTreeSet;

use fm_domain::Location;
use fm_semantic_library::{
    EligibilityCandidate, EligibilityDecision, EligibilityEntryKind, EligibilityOverride,
    EligibilityPolicy, EligibilityReason, EligibilityReasonCounts, ResourceBudgets, ResourceUsage,
    symlink_target_is_confined,
};

fn location(uri: &str) -> Location {
    Location::parse(uri).expect("valid location")
}

#[test]
fn curated_defaults_report_aggregatable_reasons_without_input_order_assumptions() {
    let policy = EligibilityPolicy::curated_defaults();
    let root = location("file:///repo");
    let budgets = ResourceBudgets {
        max_documents: 10,
        max_source_bytes_per_document: 100,
        max_total_source_bytes: 1000,
        max_total_extracted_bytes: 1000,
        max_total_vector_bytes: 1000,
    };
    let crowded = ResourceUsage::estimated(10, 950, 950, 950);
    let skipped = EligibilityCandidate {
        location: location("file:///repo/node_modules/.cache/blob.bin"),
        kind: EligibilityEntryKind::File,
        hidden: true,
        system: true,
        application_or_package_bundle: true,
        git_ignored: true,
        mime_type: Some("application/x-unknown".to_owned()),
        source_bytes: 200,
        estimated_extracted_bytes: 100,
        estimated_vector_bytes: 100,
        symlink_target: None,
    };
    let build = EligibilityCandidate {
        location: location("file:///repo/target/generated.txt"),
        kind: EligibilityEntryKind::File,
        hidden: false,
        system: false,
        application_or_package_bundle: false,
        git_ignored: false,
        mime_type: Some("text/plain".to_owned()),
        source_bytes: 1,
        estimated_extracted_bytes: 1,
        estimated_vector_bytes: 1,
        symlink_target: None,
    };

    let skipped_result = policy
        .evaluate(&root, &skipped, &budgets, crowded, &Default::default())
        .unwrap();
    let build_result = policy
        .evaluate(
            &root,
            &build,
            &budgets,
            ResourceUsage::default(),
            &Default::default(),
        )
        .unwrap();
    let counts =
        EligibilityReasonCounts::from_decisions([build_result.clone(), skipped_result.clone()]);

    let EligibilityDecision::Skipped(reasons) = skipped_result else {
        panic!("candidate should be skipped");
    };
    assert_eq!(
        reasons,
        BTreeSet::from([
            EligibilityReason::Hidden,
            EligibilityReason::System,
            EligibilityReason::ApplicationOrPackageBundle,
            EligibilityReason::DependencyDirectory,
            EligibilityReason::CacheDirectory,
            EligibilityReason::GitIgnored,
            EligibilityReason::UnsupportedMime,
            EligibilityReason::Oversized,
            EligibilityReason::OverBudget,
        ])
    );
    assert_eq!(counts.get(EligibilityReason::BuildDirectory), 1);
    assert_eq!(counts.get(EligibilityReason::OverBudget), 1);
}

#[test]
fn symlink_confinement_is_structural_and_cannot_be_overridden() {
    let policy = EligibilityPolicy::curated_defaults();
    let root = location("file:///repo");
    let candidate = EligibilityCandidate {
        location: location("file:///repo/link"),
        kind: EligibilityEntryKind::Symlink,
        hidden: false,
        system: false,
        application_or_package_bundle: false,
        git_ignored: false,
        mime_type: None,
        source_bytes: 0,
        estimated_extracted_bytes: 0,
        estimated_vector_bytes: 0,
        symlink_target: Some(location("file:///repo-secret/file.txt")),
    };
    let overrides = [(
        EligibilityReason::SymlinkOutsideRoot,
        EligibilityOverride::Include,
    )]
    .into();

    assert!(!symlink_target_is_confined(
        &root,
        candidate.symlink_target.as_ref().unwrap()
    ));
    assert!(symlink_target_is_confined(
        &root,
        &location("file:///repo/inside/file.txt")
    ));
    assert!(!symlink_target_is_confined(
        &root,
        &location("file:///repo/../outside.txt")
    ));
    assert_eq!(
        policy
            .evaluate(
                &root,
                &candidate,
                &ResourceBudgets::default(),
                ResourceUsage::default(),
                &overrides,
            )
            .unwrap(),
        EligibilityDecision::Skipped(BTreeSet::from([EligibilityReason::SymlinkOutsideRoot]))
    );
}

#[test]
fn explicit_root_override_can_include_curated_hidden_content() {
    let policy = EligibilityPolicy::curated_defaults();
    let root = location("file:///repo");
    let candidate = EligibilityCandidate {
        location: location("file:///repo/.notes.txt"),
        kind: EligibilityEntryKind::File,
        hidden: true,
        system: false,
        application_or_package_bundle: false,
        git_ignored: false,
        mime_type: Some("text/plain".to_owned()),
        source_bytes: 10,
        estimated_extracted_bytes: 10,
        estimated_vector_bytes: 10,
        symlink_target: None,
    };
    let overrides = [(EligibilityReason::Hidden, EligibilityOverride::Include)].into();

    assert_eq!(
        policy
            .evaluate(
                &root,
                &candidate,
                &ResourceBudgets::default(),
                ResourceUsage::default(),
                &overrides,
            )
            .unwrap(),
        EligibilityDecision::Eligible
    );
}
