//! Application integration coverage for explicit one-shot semantic indexing.

#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;
use std::sync::Arc;

use fm_application::FileManagerService;
use fm_application::semantic::{
    FakeSemanticCapability, LibraryId as WorkerLibraryId, SemanticOperationId, SemanticQuery,
    SemanticScope, TenantId,
};
use fm_application::semantic_indexing::SemanticIndexingError;
use fm_application::semantic_library::{
    FixedSemanticEnrolmentEstimator, SemanticAccessContext, SemanticEnrolmentEstimate,
    SemanticEstimateCompleteness, SemanticFolderContext, SemanticLibraryConfiguration,
    SemanticLibraryService, SemanticRootIdentity,
};
use fm_domain::{Location, WorkspaceId};
use fm_semantic_library::{
    DeviceLibraryIdentity, EligibilityReason, EligibilityReasonCounts, LibraryId, ModelIdentity,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId,
};
use fm_transport_dto::{ResolveRagCitationRequestDto, RuntimeKindDto};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const HOST: SemanticAccessContext = SemanticAccessContext::Host;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-indexing-tests");
    std::fs::create_dir_all(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .unwrap()
}

fn library(directory: &TempDir) -> SemanticLibraryService {
    SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::new(
            directory.path().join("library-config"),
            directory.path().join("semantic-data"),
            DeviceLibraryIdentity::new(
                LibraryId::from_uuid(Uuid::from_u128(0x190)),
                ModelIdentity::new("fixture-model", "revision-1", 384, "fixture-space").unwrap(),
            ),
            ResourceProfile {
                kind: ResourceProfileKind::Balanced,
                budgets: ResourceBudgets::default(),
            },
        ),
        Arc::new(FixedSemanticEnrolmentEstimator::new(
            SemanticEnrolmentEstimate {
                completeness: SemanticEstimateCompleteness::Estimated,
                estimated_files: Some(2),
                estimated_source_bytes: Some(128),
                estimated_extracted_bytes: Some(128),
                estimated_vector_bytes: Some(128),
                skipped_reason_counts: EligibilityReasonCounts::default(),
                missing_model_download_bytes: None,
                unavailable_reason: None,
                filesystem_identity: Some(SemanticRootIdentity::new("volume", "root")),
            },
        )),
    )
    .unwrap()
}

fn enrol(library: &SemanticLibraryService, workspace_id: WorkspaceId, root: Location) -> RootId {
    let context = SemanticFolderContext::new(workspace_id, root);
    let preview = library
        .preview_enrolment(&HOST, context.clone(), true)
        .unwrap();
    let status = library
        .confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            &context,
        )
        .unwrap();
    status.roots[0].id.parse().unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn recursive_indexing_filters_entries_and_resolves_search_evidence() {
    let root = project_temp_dir("root-");
    std::fs::create_dir_all(root.path().join("nested")).unwrap();
    std::fs::create_dir_all(root.path().join(".private")).unwrap();
    std::fs::create_dir_all(root.path().join("node_modules")).unwrap();
    std::fs::write(
        root.path().join("welcome.txt"),
        "The orchard contains semantic apples.",
    )
    .unwrap();
    std::fs::write(
        root.path().join("nested/notes.md"),
        "A nested semantic pear document.",
    )
    .unwrap();
    std::fs::write(root.path().join(".hidden.txt"), "semantic secret").unwrap();
    std::fs::write(root.path().join(".private/inside.txt"), "semantic private").unwrap();
    std::fs::write(
        root.path().join("node_modules/dependency.txt"),
        "semantic dependency",
    )
    .unwrap();
    std::fs::write(root.path().join("unsupported.bin"), b"\0\x01semantic").unwrap();
    std::os::unix::fs::symlink("welcome.txt", root.path().join("linked.txt")).unwrap();

    let state = project_temp_dir("state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1900));
    let second_workspace_id = WorkspaceId::from(Uuid::from_u128(0x1902));
    let root_location = Location::from_native_path(root.path()).unwrap();
    let library = library(&state);
    let root_id = enrol(&library, workspace_id, root_location.clone());
    assert_eq!(enrol(&library, second_workspace_id, root_location), root_id);
    let fake = Arc::new(FakeSemanticCapability::new());
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(fake);

    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(report.observed_files, 2);
    assert_eq!(report.ingested_occurrences, 4);
    assert_eq!(report.reconciliation_generation, 1);
    assert!(
        report
            .skipped_reason_counts
            .iter()
            .any(|count| count.reason == EligibilityReason::Hidden && count.count >= 2)
    );
    assert!(
        report
            .skipped_reason_counts
            .iter()
            .any(|count| count.reason == EligibilityReason::UnsupportedMime && count.count >= 1)
    );
    assert!(report.skipped_reason_counts.iter().any(|count| {
        count.reason == EligibilityReason::DependencyDirectory && count.count >= 1
    }));
    assert!(report.skipped_reason_counts.iter().any(|count| {
        count.reason == EligibilityReason::SymlinkOutsideRoot && count.count >= 1
    }));

    let results = service
        .semantic_query(SemanticQuery {
            scope: SemanticScope::new(
                TenantId::new(report.tenant_id),
                WorkerLibraryId::new(report.library_id),
            ),
            request_id: SemanticOperationId::new("integration-query"),
            text: "semantic".to_owned(),
            concept: None,
            maximum_results: 10,
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    for result in results {
        assert!(!result.metadata.contains_key("uri"));
        assert!(
            !result
                .metadata
                .values()
                .any(|value| value.contains("welcome.txt"))
        );
        let source_id = result.metadata.get("source_id").unwrap().clone();
        let resolved = service
            .resolve_rag_citation(
                &HOST,
                ResolveRagCitationRequestDto {
                    workspace_id: workspace_id.into_inner(),
                    source_id: source_id.clone(),
                },
            )
            .await
            .unwrap();
        let location: Location = resolved.location.into();
        assert!(location.uri.ends_with("/welcome.txt") || location.uri.ends_with("/notes.md"));
        assert!(resolved.available);
        service
            .resolve_rag_citation(
                &HOST,
                ResolveRagCitationRequestDto {
                    workspace_id: second_workspace_id.into_inner(),
                    source_id,
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn cancelled_indexing_never_commits_a_reconciliation() {
    let root = project_temp_dir("cancel-root-");
    std::fs::write(root.path().join("document.txt"), "semantic content").unwrap();
    let state = project_temp_dir("cancel-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1901));
    let library = library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, SemanticIndexingError::Cancelled));
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].reconciliation_generation,
        0
    );
}
