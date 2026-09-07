//! Application integration coverage for explicit one-shot semantic indexing.

#![allow(clippy::unwrap_used, missing_docs)]

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fm_application::FileManagerService;
use fm_application::semantic::{
    DocumentIngestion, FakeSemanticCapability, SemanticCapability, SemanticError, SemanticHealth,
    SemanticIngestionJob, SemanticIngestionState, SemanticJobId, SemanticOperationId,
    SemanticProgressEvent, SemanticQuery, SemanticScope, SemanticSearchResult,
};
#[cfg(unix)]
use fm_application::semantic::{LibraryId as WorkerLibraryId, TenantId};
use fm_application::semantic_indexing::SemanticIndexingError;
use fm_application::semantic_library::{
    FixedSemanticEnrolmentEstimator, SemanticAccessContext, SemanticEnrolmentEstimate,
    SemanticEstimateCompleteness, SemanticFolderContext, SemanticLibraryConfiguration,
    SemanticLibraryService, SemanticRootIdentity,
};
use fm_application::semantic_ocr::{
    OcrAvailability, OcrExecutableProbe, OcrPolicyStore, OcrRemediationScope, OcrRemediationState,
    OcrRemediationTarget, SemanticOcrError, SemanticOcrService,
};
use fm_domain::{Location, WorkspaceId};
use fm_semantic_library::{
    DeviceLibraryIdentity, EligibilityReason, EligibilityReasonCounts, LibraryId, ModelIdentity,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId,
};
#[cfg(unix)]
use fm_transport_dto::ResolveRagCitationRequestDto;
use fm_transport_dto::RuntimeKindDto;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const HOST: SemanticAccessContext = SemanticAccessContext::Host;

struct FailingDocumentCapability {
    inner: FakeSemanticCapability,
}

struct RemediatingDocumentCapability {
    inner: FakeSemanticCapability,
    ocr_enabled: AtomicBool,
    complete_after_restart: bool,
    restart_count: AtomicUsize,
    cancel_count: AtomicUsize,
    remediated_contents: Mutex<Vec<Vec<u8>>>,
}

impl RemediatingDocumentCapability {
    fn new() -> Self {
        Self {
            inner: FakeSemanticCapability::new(),
            ocr_enabled: AtomicBool::new(false),
            complete_after_restart: true,
            restart_count: AtomicUsize::new(0),
            cancel_count: AtomicUsize::new(0),
            remediated_contents: Mutex::new(Vec::new()),
        }
    }

    fn pending() -> Self {
        Self {
            complete_after_restart: false,
            ..Self::new()
        }
    }
}

#[async_trait]
impl SemanticCapability for RemediatingDocumentCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        self.inner.health().await
    }

    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        if ingestion.content.starts_with(b"scan-") && !self.ocr_enabled.load(Ordering::Acquire) {
            return Ok(SemanticJobId::new(format!(
                "ocr-required-{}",
                ingestion.document_id.as_str()
            )));
        }
        if ingestion.content.starts_with(b"scan-") {
            self.remediated_contents
                .lock()
                .unwrap()
                .push(ingestion.content.clone());
            if !self.complete_after_restart {
                return Ok(SemanticJobId::new(format!(
                    "pending-{}",
                    ingestion.document_id.as_str()
                )));
            }
        }
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
        if job_id.as_str().starts_with("ocr-required-") {
            return Ok(SemanticIngestionJob {
                document_id: fm_application::semantic::DocumentId::new(
                    job_id.as_str().trim_start_matches("ocr-required-"),
                ),
                job_id,
                state: SemanticIngestionState::Skipped,
                detail: Some(
                    "Add a searchable text layer with OCRmyPDF, then reindex the file.".into(),
                ),
            });
        }
        if job_id.as_str().starts_with("pending-") {
            return Ok(SemanticIngestionJob {
                document_id: fm_application::semantic::DocumentId::new(
                    job_id.as_str().trim_start_matches("pending-"),
                ),
                job_id,
                state: SemanticIngestionState::Running,
                detail: None,
            });
        }
        self.inner.ingestion_job(scope, job_id).await
    }

    async fn events(
        &self,
        scope: SemanticScope,
    ) -> Result<Vec<SemanticProgressEvent>, SemanticError> {
        self.inner.events(scope).await
    }

    async fn cancel(&self, operation_id: SemanticOperationId) -> Result<bool, SemanticError> {
        if self.ocr_enabled.load(Ordering::Acquire) {
            self.cancel_count.fetch_add(1, Ordering::AcqRel);
        }
        self.inner.cancel(operation_id).await
    }

    async fn shutdown(&self, grace: Duration) -> Result<(), SemanticError> {
        self.inner.shutdown(grace).await
    }

    async fn restart(&self, _grace: Duration) -> Result<(), SemanticError> {
        self.restart_count.fetch_add(1, Ordering::AcqRel);
        self.ocr_enabled.store(true, Ordering::Release);
        Ok(())
    }
}

struct AvailableOcrProbe;

impl OcrExecutableProbe for AvailableOcrProbe {
    fn probe(&self) -> OcrAvailability {
        OcrAvailability::Available {
            executable: Path::new("/usr/local/bin/ocrmypdf").to_path_buf(),
            version: "16.10.4".to_owned(),
        }
    }
}

#[async_trait]
impl SemanticCapability for FailingDocumentCapability {
    async fn health(&self) -> Result<SemanticHealth, SemanticError> {
        self.inner.health().await
    }

    async fn ingest(&self, ingestion: DocumentIngestion) -> Result<SemanticJobId, SemanticError> {
        if ingestion.content == b"conversion fails" {
            return Ok(SemanticJobId::new(format!(
                "failed-{}",
                ingestion.document_id.as_str()
            )));
        }
        if ingestion.content == b"no text layer" {
            return Ok(SemanticJobId::new(format!(
                "skipped-{}",
                ingestion.document_id.as_str()
            )));
        }
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
        if job_id.as_str().starts_with("failed-") {
            return Ok(SemanticIngestionJob {
                document_id: fm_application::semantic::DocumentId::new(
                    job_id.as_str().trim_start_matches("failed-"),
                ),
                job_id,
                state: SemanticIngestionState::Failed,
                detail: None,
            });
        }
        if job_id.as_str().starts_with("skipped-") {
            return Ok(SemanticIngestionJob {
                document_id: fm_application::semantic::DocumentId::new(
                    job_id.as_str().trim_start_matches("skipped-"),
                ),
                job_id,
                state: SemanticIngestionState::Skipped,
                detail: Some(
                    "Add a searchable text layer with OCRmyPDF, then reindex the file.".into(),
                ),
            });
        }
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
}

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
    let context = SemanticFolderContext::new(workspace_id, root.clone());
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
        .find(|candidate| candidate.location == root)
        .expect("newly enrolled root")
        .id
        .parse()
        .unwrap()
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
    assert_eq!(
        report.tenant_ids,
        vec![workspace_id.to_string(), second_workspace_id.to_string()]
    );
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

    for workspace_id in [workspace_id, second_workspace_id] {
        let results = service
            .semantic_query(SemanticQuery {
                scope: SemanticScope::new(
                    TenantId::new(workspace_id.to_string()),
                    WorkerLibraryId::new(report.library_id.clone()),
                ),
                request_id: SemanticOperationId::new(format!("integration-query-{workspace_id}")),
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
                        source_id,
                    },
                )
                .await
                .unwrap();
            let location: Location = resolved.location.into();
            assert!(location.uri.ends_with("/welcome.txt") || location.uri.ends_with("/notes.md"));
            assert!(resolved.available);
        }
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

#[tokio::test]
async fn one_failed_document_does_not_abort_the_root_reconciliation() {
    let root = project_temp_dir("partial-root-");
    std::fs::write(root.path().join("good.txt"), "searchable semantic content").unwrap();
    std::fs::write(root.path().join("bad.txt"), "conversion fails").unwrap();
    std::fs::File::create(root.path().join("oversized.txt"))
        .unwrap()
        .set_len(65 * 1024 * 1024)
        .unwrap();
    let state = project_temp_dir("partial-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1903));
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
    .with_semantic_capability(Arc::new(FailingDocumentCapability {
        inner: FakeSemanticCapability::new(),
    }));

    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(report.observed_files, 2);
    assert_eq!(report.ingested_occurrences, 1);
    assert_eq!(report.failed_occurrences, 1);
    assert_eq!(
        report
            .skipped_reason_counts
            .iter()
            .find(|count| count.reason == EligibilityReason::Oversized)
            .map(|count| count.count),
        Some(1)
    );
    assert_eq!(report.reconciliation_generation, 1);
}

#[tokio::test]
async fn one_ocr_required_pdf_is_reported_without_aborting_reconciliation() {
    let root = project_temp_dir("ocr-required-root-");
    std::fs::write(
        root.path().join("searchable.txt"),
        "searchable semantic content",
    )
    .unwrap();
    std::fs::write(root.path().join("scanned.pdf"), "no text layer").unwrap();
    let state = project_temp_dir("ocr-required-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x192));
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
    .with_semantic_capability(Arc::new(FailingDocumentCapability {
        inner: FakeSemanticCapability::new(),
    }));

    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(report.ingested_occurrences, 1);
    assert_eq!(report.failed_occurrences, 0);
    assert_eq!(report.excluded_occurrences, 1);
    assert_eq!(
        report.exclusion_details,
        ["Add a searchable text layer with OCRmyPDF, then reindex the file."]
    );
    assert_eq!(
        report.ocr_required_files,
        [Location::from_native_path(&root.path().join("scanned.pdf")).unwrap()]
    );
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].ocr_required_files,
        report.ocr_required_files
    );
    assert_eq!(report.reconciliation_generation, 1);
}

#[tokio::test]
async fn ocr_scope_expansion_accepts_only_backend_reported_files() {
    let root = project_temp_dir("ocr-scopes-root-");
    let second_root = project_temp_dir("ocr-scopes-second-root-");
    for (name, content) in [
        ("one.pdf", "scan-one"),
        ("two.pdf", "scan-two"),
        ("three.pdf", "scan-three"),
    ] {
        std::fs::write(root.path().join(name), content).unwrap();
    }
    std::fs::write(second_root.path().join("four.pdf"), "scan-four").unwrap();
    let state = project_temp_dir("ocr-scopes-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1970));
    let library = library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let second_root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(second_root.path()).unwrap(),
    );
    let policy = Arc::new(OcrPolicyStore::load(state.path().join("ocr")));
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(RemediatingDocumentCapability::new()))
    .with_semantic_ocr_service(SemanticOcrService::load(
        state.path().join("ocr"),
        Arc::clone(&policy),
        Arc::new(AvailableOcrProbe),
    ));
    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    let second_report = service
        .semantic_reconcile_enrolled_root(&HOST, second_root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(second_report.ocr_required_files.len(), 1);
    service.set_semantic_ocr_consent(true).await.unwrap();
    let targets = report
        .ocr_required_files
        .iter()
        .cloned()
        .map(|location| OcrRemediationTarget { root_id, location })
        .collect::<Vec<_>>();

    let one = service
        .start_semantic_ocr_remediation(OcrRemediationScope::File(targets[0].clone()))
        .unwrap();
    assert_eq!(one.total_files, 1);
    service.cancel_semantic_ocr_remediation(&one.id).unwrap();

    let selected = service
        .start_semantic_ocr_remediation(OcrRemediationScope::SelectedFiles(targets[..2].to_vec()))
        .unwrap();
    assert_eq!(selected.total_files, 2);
    service
        .cancel_semantic_ocr_remediation(&selected.id)
        .unwrap();

    let enrolled_root = service
        .start_semantic_ocr_remediation(OcrRemediationScope::Root(root_id))
        .unwrap();
    assert_eq!(enrolled_root.total_files, 3);
    service
        .cancel_semantic_ocr_remediation(&enrolled_root.id)
        .unwrap();

    let all = service
        .start_semantic_ocr_remediation(OcrRemediationScope::AllRequired)
        .unwrap();
    assert_eq!(all.total_files, 4);
    service.cancel_semantic_ocr_remediation(&all.id).unwrap();

    let outside = OcrRemediationTarget {
        root_id,
        location: Location::from_native_path(&root.path().join("not-reported.pdf")).unwrap(),
    };
    assert!(matches!(
        service.start_semantic_ocr_remediation(OcrRemediationScope::File(outside)),
        Err(SemanticOcrError::UnreportedTarget)
    ));
}

#[tokio::test]
async fn successful_ocr_reingests_only_selected_files_and_updates_reported_state() {
    let root = project_temp_dir("ocr-remediation-root-");
    for (name, content) in [
        ("one.pdf", "scan-one"),
        ("two.pdf", "scan-two"),
        ("three.pdf", "scan-three"),
    ] {
        std::fs::write(root.path().join(name), content).unwrap();
    }

    let state = project_temp_dir("ocr-remediation-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1971));
    let library = library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let capability = Arc::new(RemediatingDocumentCapability::new());
    let policy = Arc::new(OcrPolicyStore::load(state.path().join("ocr")));
    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::Tauri,
            state.path().join("workspaces"),
            state.path().join("settings"),
        )
        .with_semantic_library_service(library)
        .with_semantic_capability(capability.clone())
        .with_semantic_ocr_service(SemanticOcrService::load(
            state.path().join("ocr"),
            policy,
            Arc::new(AvailableOcrProbe),
        )),
    );
    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.ocr_required_files.len(), 3);

    service.set_semantic_ocr_consent(true).await.unwrap();
    assert_eq!(capability.restart_count.load(Ordering::Acquire), 1);
    service.set_semantic_ocr_consent(true).await.unwrap();
    assert_eq!(capability.restart_count.load(Ordering::Acquire), 1);
    let selected = report
        .ocr_required_files
        .iter()
        .filter(|location| location.uri.ends_with("/one.pdf") || location.uri.ends_with("/two.pdf"))
        .cloned()
        .map(|location| OcrRemediationTarget { root_id, location })
        .collect::<Vec<_>>();
    let job = service
        .start_semantic_ocr_remediation(OcrRemediationScope::SelectedFiles(selected.clone()))
        .unwrap();

    let shutdown = CancellationToken::new();
    let runner =
        tokio::spawn(Arc::clone(&service).run_semantic_ocr_remediation_jobs(shutdown.clone()));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state = service
                .semantic_ocr_status()
                .jobs
                .into_iter()
                .find(|candidate| candidate.id == job.id)
                .unwrap()
                .state;
            if state == OcrRemediationState::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("remediation completed");
    shutdown.cancel();
    runner.await.unwrap();

    let remediated = capability.remediated_contents.lock().unwrap().clone();
    assert_eq!(remediated.len(), 2);
    assert!(remediated.contains(&b"scan-one".to_vec()));
    assert!(remediated.contains(&b"scan-two".to_vec()));
    assert!(!remediated.contains(&b"scan-three".to_vec()));
    assert_eq!(
        service.semantic_ocr_status().reported_files,
        [OcrRemediationTarget {
            root_id,
            location: Location::from_native_path(&root.path().join("three.pdf")).unwrap(),
        }]
    );

    service.set_semantic_ocr_consent(false).await.unwrap();
    assert_eq!(capability.restart_count.load(Ordering::Acquire), 2);
}

#[tokio::test]
async fn cancelling_running_ocr_stops_worker_ingestion_and_persists_cancelled_state() {
    let root = project_temp_dir("ocr-cancel-root-");
    std::fs::write(root.path().join("scan.pdf"), "scan-pending").unwrap();
    let state = project_temp_dir("ocr-cancel-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1972));
    let library = library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let capability = Arc::new(RemediatingDocumentCapability::pending());
    let policy = Arc::new(OcrPolicyStore::load(state.path().join("ocr")));
    let service = Arc::new(
        FileManagerService::new(
            RuntimeKindDto::Tauri,
            state.path().join("workspaces"),
            state.path().join("settings"),
        )
        .with_semantic_library_service(library)
        .with_semantic_capability(capability.clone())
        .with_semantic_ocr_service(SemanticOcrService::load(
            state.path().join("ocr"),
            policy,
            Arc::new(AvailableOcrProbe),
        )),
    );
    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    service.set_semantic_ocr_consent(true).await.unwrap();
    let job = service
        .start_semantic_ocr_remediation(OcrRemediationScope::File(OcrRemediationTarget {
            root_id,
            location: report.ocr_required_files[0].clone(),
        }))
        .unwrap();
    let shutdown = CancellationToken::new();
    let runner =
        tokio::spawn(Arc::clone(&service).run_semantic_ocr_remediation_jobs(shutdown.clone()));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if !capability.remediated_contents.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("job started");

    service.cancel_semantic_ocr_remediation(&job.id).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if capability.cancel_count.load(Ordering::Acquire) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker cancellation propagated");
    assert_eq!(
        service
            .semantic_ocr_status()
            .jobs
            .into_iter()
            .find(|candidate| candidate.id == job.id)
            .unwrap()
            .state,
        OcrRemediationState::Cancelled
    );
    shutdown.cancel();
    runner.await.unwrap();
}
