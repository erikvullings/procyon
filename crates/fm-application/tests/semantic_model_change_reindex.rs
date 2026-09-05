//! Application coverage for rebuilding enrolled indexes after a model change.

#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fm_application::FileManagerService;
use fm_application::semantic::{
    DocumentIngestion, FakeSemanticCapability, SemanticCapability, SemanticError, SemanticHealth,
    SemanticIngestionJob, SemanticJobId, SemanticOperationId, SemanticProgressEvent, SemanticQuery,
    SemanticScope, SemanticSearchResult,
};
use fm_application::semantic_library::{
    FixedSemanticEnrolmentEstimator, SemanticAccessContext, SemanticEnrolmentEstimate,
    SemanticEstimateCompleteness, SemanticFolderContext, SemanticLibraryConfiguration,
    SemanticLibraryService, SemanticRootIdentity,
};
use fm_domain::{Location, WorkspaceId};
use fm_semantic_library::{
    DeviceLibraryIdentity, EligibilityReasonCounts, LibraryId, ModelIdentity, ResourceBudgets,
    ResourceProfile, ResourceProfileKind, RootId,
};
use fm_transport_dto::RuntimeKindDto;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const HOST: SemanticAccessContext = SemanticAccessContext::Host;

/// Records the host orchestration a model change must perform, so a test can
/// assert that the previously active worker is stopped *before* any document is
/// fed to the newly active model.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Interaction {
    Restarted,
    Ingested(String),
}

struct RecordingCapability {
    inner: FakeSemanticCapability,
    interactions: Mutex<Vec<Interaction>>,
    restart_failure: Option<String>,
}

impl RecordingCapability {
    fn new(restart_failure: Option<&str>) -> Self {
        Self {
            inner: FakeSemanticCapability::new(),
            interactions: Mutex::new(Vec::new()),
            restart_failure: restart_failure.map(ToOwned::to_owned),
        }
    }

    fn interactions(&self) -> Vec<Interaction> {
        self.interactions.lock().unwrap().clone()
    }
}

#[async_trait]
impl SemanticCapability for RecordingCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        self.inner.health().await
    }

    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        self.interactions
            .lock()
            .unwrap()
            .push(Interaction::Ingested(
                ingestion.document_id.as_str().to_owned(),
            ));
        self.inner.ingest(ingestion).await
    }

    async fn query(
        &self,
        query: SemanticQuery,
    ) -> Result<Vec<SemanticSearchResult>, SemanticError> {
        self.inner.query(query).await
    }

    async fn ingestion_job(
        &self,
        scope: SemanticScope,
        job_id: SemanticJobId,
    ) -> Result<SemanticIngestionJob, SemanticError> {
        self.inner.ingestion_job(scope, job_id).await
    }

    async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        self.inner.events(scope).await
    }

    async fn cancel(&self, operation_id: SemanticOperationId) -> Result<bool, SemanticError> {
        self.inner.cancel(operation_id).await
    }

    async fn shutdown(&self, grace: Duration) -> Result<(), SemanticError> {
        self.inner.shutdown(grace).await
    }

    async fn restart(&self, _grace: Duration) -> Result<(), SemanticError> {
        self.interactions
            .lock()
            .unwrap()
            .push(Interaction::Restarted);
        match &self.restart_failure {
            Some(message) => Err(SemanticError::WorkerFailure(message.clone())),
            None => Ok(()),
        }
    }
}

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-model-change-tests");
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
                LibraryId::from_uuid(Uuid::from_u128(0x191)),
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
                estimated_files: Some(1),
                estimated_source_bytes: Some(64),
                estimated_extracted_bytes: Some(64),
                estimated_vector_bytes: Some(64),
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
    status
        .roots
        .iter()
        .find(|candidate| candidate.location == context.location)
        .unwrap()
        .id
        .parse()
        .unwrap()
}

fn service(
    state: &TempDir,
    library: SemanticLibraryService,
    capability: Arc<RecordingCapability>,
) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(capability)
}

#[tokio::test]
async fn a_model_change_stops_the_previous_worker_then_refeeds_every_enrolled_root() {
    let first_root = project_temp_dir("root-a-");
    let second_root = project_temp_dir("root-b-");
    std::fs::write(
        first_root.path().join("orchard.txt"),
        "The orchard contains semantic apples.",
    )
    .unwrap();
    std::fs::write(
        second_root.path().join("notes.md"),
        "A second semantic pear document.",
    )
    .unwrap();

    let state = project_temp_dir("state-");
    let library = library(&state);
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1910));
    enrol(
        &library,
        workspace_id,
        Location::from_native_path(first_root.path()).unwrap(),
    );
    enrol(
        &library,
        workspace_id,
        Location::from_native_path(second_root.path()).unwrap(),
    );
    let capability = Arc::new(RecordingCapability::new(None));
    let service = service(&state, library, Arc::clone(&capability));

    let report = service
        .semantic_reindex_after_model_change(
            &HOST,
            Duration::from_secs(1),
            CancellationToken::new(),
        )
        .await;

    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.reindexed_roots.len(), 2);
    assert!(report.unavailable_roots.is_empty());
    assert_eq!(report.ingested_occurrences, 2);

    let interactions = capability.interactions();
    assert_eq!(
        interactions.first(),
        Some(&Interaction::Restarted),
        "the previously active worker must be stopped before anything is ingested"
    );
    assert_eq!(
        interactions
            .iter()
            .filter(|interaction| matches!(interaction, Interaction::Restarted))
            .count(),
        1
    );
    let ingested = interactions
        .iter()
        .filter(|interaction| matches!(interaction, Interaction::Ingested(_)))
        .count();
    assert_eq!(ingested, 2, "both enrolled roots must reach the new model");
}

#[tokio::test]
async fn a_failed_worker_restart_is_reported_without_feeding_the_old_model() {
    let root = project_temp_dir("root-c-");
    std::fs::write(root.path().join("orchard.txt"), "Semantic apples again.").unwrap();

    let state = project_temp_dir("state-fail-");
    let library = library(&state);
    enrol(
        &library,
        WorkspaceId::from(Uuid::from_u128(0x1911)),
        Location::from_native_path(root.path()).unwrap(),
    );
    let capability = Arc::new(RecordingCapability::new(Some("worker refused to stop")));
    let service = service(&state, library, Arc::clone(&capability));

    let report = service
        .semantic_reindex_after_model_change(
            &HOST,
            Duration::from_secs(1),
            CancellationToken::new(),
        )
        .await;

    assert!(!report.is_complete());
    assert_eq!(
        report.restart_failure.as_deref(),
        Some("semantic worker failed: worker refused to stop")
    );
    // Activation remains durable, but feeding the still-running worker would
    // repopulate the superseded model's index rather than the new one.
    assert!(report.reindexed_roots.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(
        report.failures[0]
            .reason
            .contains("reindex was not started")
    );
    assert!(
        !capability
            .interactions()
            .iter()
            .any(|interaction| matches!(interaction, Interaction::Ingested(_)))
    );
}

#[tokio::test]
async fn a_cancelled_reindex_reports_every_root_it_did_not_rebuild() {
    let root = project_temp_dir("root-d-");
    std::fs::write(
        root.path().join("orchard.txt"),
        "Semantic apples once more.",
    )
    .unwrap();

    let state = project_temp_dir("state-cancel-");
    let library = library(&state);
    enrol(
        &library,
        WorkspaceId::from(Uuid::from_u128(0x1912)),
        Location::from_native_path(root.path()).unwrap(),
    );
    let capability = Arc::new(RecordingCapability::new(None));
    let service = service(&state, library, Arc::clone(&capability));

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let report = service
        .semantic_reindex_after_model_change(&HOST, Duration::from_secs(1), cancellation)
        .await;

    assert!(!report.is_complete());
    assert!(report.reindexed_roots.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].reason.contains("cancelled"));
    assert!(
        !capability
            .interactions()
            .iter()
            .any(|interaction| matches!(interaction, Interaction::Ingested(_)))
    );
}
