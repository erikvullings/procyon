//! Application integration coverage for explicit one-shot semantic indexing.

#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use fm_application::FileManagerService;
#[cfg(all(unix, feature = "gemma-native"))]
use fm_application::semantic::IpcSemanticCapability;
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
    DeviceLibraryIdentity, EligibilityOverride, EligibilityReason, EligibilityReasonCounts,
    GemmaMediaSelection, LibraryId, ModelIdentity, ResourceBudgets, ResourceProfile,
    ResourceProfileKind, RootId,
};
#[cfg(all(unix, feature = "gemma-native"))]
use fm_semantic_worker::gemma_native::{GemmaMedia, GemmaNativeFiles};
#[cfg(all(unix, feature = "gemma-native"))]
use fm_semantic_worker::{ManagedWorkerLaunch, ManagedWorkerResolver};
#[cfg(unix)]
use fm_transport_dto::ResolveRagCitationRequestDto;
use fm_transport_dto::RuntimeKindDto;
#[cfg(all(unix, feature = "gemma-native"))]
use fm_transport_dto::{
    SearchModeDto, SearchQueryDto, SearchScopeDto, SearchSemanticPredicateDto,
    SemanticSearchScopeDto, StartSearchRequestDto,
};
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
    ingested_titles: Mutex<Vec<String>>,
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
            ingested_titles: Mutex::new(Vec::new()),
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
        self.ingested_titles.lock().unwrap().push(
            ingestion
                .metadata
                .get("title")
                .expect("host supplies a real display title")
                .clone(),
        );
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
    let parent = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .unwrap()
}

fn library(directory: &TempDir) -> SemanticLibraryService {
    library_with_model(
        directory,
        ModelIdentity::new("fixture-model", "revision-1", 384, "fixture-space").unwrap(),
    )
}

fn library_with_model(directory: &TempDir, model: ModelIdentity) -> SemanticLibraryService {
    SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::new(
            directory.path().join("library-config"),
            directory.path().join("semantic-data"),
            DeviceLibraryIdentity::new(LibraryId::from_uuid(Uuid::from_u128(0x190)), model),
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

#[tokio::test]
async fn reconciliation_feeds_only_media_enabled_for_the_gemma_library() {
    let root = project_temp_dir("media-root-");
    for name in ["image.png", "sound.wav", "clip.mp4"] {
        std::fs::write(root.path().join(name), b"fixture-bytes").unwrap();
    }
    let state = project_temp_dir("media-state-");
    let library = library_with_model(
        &state,
        ModelIdentity::embeddinggemma_2(
            256,
            GemmaMediaSelection {
                images: true,
                audio: false,
                video: true,
            },
        )
        .unwrap(),
    );
    let root_id = enrol(
        &library,
        WorkspaceId::from(Uuid::from_u128(0x1910)),
        Location::from_native_path(root.path()).unwrap(),
    );
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));

    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.observed_files, 2);
    assert_eq!(report.ingested_occurrences, 2);
}

#[cfg(all(unix, feature = "gemma-native"))]
#[tokio::test]
#[ignore = "requires a signed Gemma worker, runtime, and verified original model files"]
async fn enrolled_gemma_media_crosses_provider_worker_and_authorized_search() {
    use image::{ImageBuffer, ImageFormat, Rgb};

    let files: BTreeMap<String, std::path::PathBuf> = serde_json::from_str(
        &std::env::var("PROCYON_GEMMA_PACKAGED_FILES").expect("verified original-file paths"),
    )
    .unwrap();
    let model = GemmaNativeFiles::from_original_files(&files).unwrap();
    let root = project_temp_dir("gemma-host-media-");
    let state = project_temp_dir("gemma-host-state-");
    let runtime = project_temp_dir("gemma-host-runtime-");
    let image = ImageBuffer::from_fn(128, 96, |x, y| {
        Rgb([
            ((x * 2 + y) % 256) as u8,
            ((x + y * 2) % 256) as u8,
            ((x + y) % 256) as u8,
        ])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png).unwrap();
    for (name, bytes) in [
        ("photo.png", png.into_inner()),
        (
            "sound.mp3",
            include_bytes!("../../fm-semantic-worker/tests/fixtures/gemma-audio-440hz-44k.mp3")
                .to_vec(),
        ),
        (
            "clip.mp4",
            include_bytes!("../../fm-semantic-worker/tests/fixtures/gemma-video-2s.mp4").to_vec(),
        ),
    ] {
        std::fs::write(root.path().join(name), bytes).unwrap();
    }
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1911));
    let library = library_with_model(
        &state,
        ModelIdentity::embeddinggemma_2(
            128,
            GemmaMediaSelection {
                images: true,
                audio: true,
                video: true,
            },
        )
        .unwrap(),
    );
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let launch = ManagedWorkerLaunch::new_gemma(
        std::env::var("PROCYON_SEMANTIC_PRODUCTION_WORKER")
            .expect("signed packaged worker")
            .into(),
        state.path().join("worker-data"),
        std::env::var("PROCYON_SEMANTIC_PRODUCTION_NATIVE_DIRECTORY")
            .expect("verified native runtime directory")
            .into(),
        model,
        128,
        GemmaMedia {
            images: true,
            audio: true,
            video: true,
        },
    );
    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(launch.clone()));
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(IpcSemanticCapability::desktop_managed(
        runtime.path(),
        resolver,
    )));

    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.observed_files, 3);
    assert_eq!(report.ingested_occurrences, 3, "{report:?}");
    assert_eq!(report.failed_occurrences, 0, "{report:?}");
    assert_eq!(report.excluded_occurrences, 0, "{report:?}");
    let results = service
        .semantic_query(SemanticQuery {
            scope: SemanticScope::new(
                TenantId::new(workspace_id.to_string()),
                WorkerLibraryId::new(report.library_id.clone()),
            ),
            request_id: SemanticOperationId::new("media-host-query"),
            text: "search enrolled media".to_owned(),
            concept: None,
            maximum_results: 10,
        })
        .await
        .unwrap();
    let mut image_source_id = None;
    for (name, media_type) in [
        ("photo.png", "image/png"),
        ("sound.mp3", "audio/mpeg"),
        ("clip.mp4", "video/mp4"),
    ] {
        let result = results
            .iter()
            .find(|result| {
                result
                    .metadata
                    .get("media_type")
                    .is_some_and(|kind| kind == media_type)
            })
            .unwrap_or_else(|| panic!("enrolled {name} was not returned: {results:?}"));
        if name == "photo.png" {
            image_source_id = result.metadata.get("semantic.sourceId").cloned();
        }
        let resolved = service
            .resolve_rag_citation(
                &HOST,
                ResolveRagCitationRequestDto {
                    workspace_id: workspace_id.into_inner(),
                    source_id: result.metadata.get("semantic.sourceId").unwrap().clone(),
                },
            )
            .await
            .unwrap();
        let location: Location = resolved.location.into();
        assert!(location.uri.ends_with(name), "{location:?}");
        assert!(resolved.available);
    }
    let outside = service
        .semantic_query(SemanticQuery {
            scope: SemanticScope::new(
                TenantId::new("other-workspace"),
                WorkerLibraryId::new(report.library_id),
            ),
            request_id: SemanticOperationId::new("media-host-other-workspace"),
            text: "photo.png".to_owned(),
            concept: None,
            maximum_results: 10,
        })
        .await
        .unwrap();
    assert!(outside.is_empty(), "unenrolled workspace saw media");

    std::fs::remove_file(root.path().join("photo.png")).unwrap();
    let after_deletion = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(after_deletion.observed_files, 2);
    assert!(matches!(
        service
            .resolve_rag_citation(
                &HOST,
                ResolveRagCitationRequestDto {
                    workspace_id: workspace_id.into_inner(),
                    source_id: image_source_id.expect("indexed image source"),
                },
            )
            .await,
        Err(fm_application::ApplicationError::NotFound)
    ));
    let root_location: fm_transport_dto::LocationDto =
        Location::from_native_path(root.path()).unwrap().into();
    let remaining = service
        .start_search(StartSearchRequestDto {
            workspace_id: workspace_id.into_inner(),
            roots: vec![root_location.clone()],
            query: String::new(),
            content_query: None,
            content_regex: false,
            content_case_sensitive: false,
            content_whole_word: false,
            recurse: true,
            show_hidden: false,
            structured_query: Some(SearchQueryDto {
                schema_version: 2,
                mode: SearchModeDto::Semantic,
                scope: SearchScopeDto {
                    locations: vec![root_location],
                    recurse: true,
                    show_hidden: false,
                },
                name: None,
                entry_kinds: Vec::new(),
                mime_types: Vec::new(),
                min_size_bytes: None,
                max_size_bytes: None,
                modified_after: None,
                modified_before: None,
                content: None,
                semantic: Some(SearchSemanticPredicateDto {
                    query: "photo.png".to_owned(),
                    library_id: after_deletion.library_id,
                    scope: SemanticSearchScopeDto::CurrentFolder,
                    enrolled_root_ids: Vec::new(),
                }),
                concept: None,
                git_statuses: Vec::new(),
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            }),
        })
        .await
        .unwrap();
    assert_eq!(
        remaining.semantic_results.len(),
        2,
        "the audio and video should remain visible after image deletion: {remaining:?}"
    );
    assert!(
        remaining
            .semantic_results
            .iter()
            .all(|result| !result.location.uri.ends_with("photo.png")),
        "deleted image was still visible in file-primary search: {remaining:?}"
    );
    service
        .semantic_shutdown(Duration::from_secs(10))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(12), async {
        while runtime.path().join("worker.pid").exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("signed worker stopped before temporary index cleanup");
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
async fn gitignore_rules_filter_recursive_indexing_and_prune_previous_observations() {
    let root = project_temp_dir("gitignore-root-");
    let repo = git2::Repository::init(root.path()).unwrap();
    std::fs::create_dir(root.path().join("private")).unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    for path in [
        "tracked.txt",
        "ignored.txt",
        "visible.md",
        "private/inside.md",
        "private/tracked.md",
        "nested/ignored.txt",
        "nested/keep.txt",
    ] {
        std::fs::write(root.path().join(path), format!("semantic {path}")).unwrap();
    }
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    index.add_path(Path::new("private/tracked.md")).unwrap();
    index.write().unwrap();
    std::fs::write(root.path().join(".gitignore"), "*.txt\nprivate/\n").unwrap();
    std::fs::write(root.path().join("nested/.gitignore"), "!keep.txt\n").unwrap();
    assert!(
        repo.index()
            .unwrap()
            .get_path(Path::new("tracked.txt"), 0)
            .is_some()
    );

    let state = project_temp_dir("gitignore-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1991));
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

    let first = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        first.observed_files, 4,
        "tracked files and nested negation survive"
    );
    assert!(
        first
            .skipped_reason_counts
            .iter()
            .any(|count| { count.reason == EligibilityReason::GitIgnored && count.count >= 3 })
    );
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].indexed_occurrences,
        4
    );
    let results = service
        .semantic_query(SemanticQuery {
            scope: SemanticScope::new(
                TenantId::new(workspace_id.to_string()),
                WorkerLibraryId::new(first.library_id.clone()),
            ),
            request_id: SemanticOperationId::new("gitignore-before"),
            text: "semantic".into(),
            concept: None,
            maximum_results: 10,
        })
        .await
        .unwrap();
    let mut visible_source = None;
    for result in results {
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
        if resolved.location.uri.ends_with("/visible.md") {
            visible_source = Some(source_id);
        }
    }
    let visible_source = visible_source.expect("visible source was indexed");

    std::fs::write(
        root.path().join(".gitignore"),
        "*.txt\nprivate/\nvisible.md\n",
    )
    .unwrap();
    let second = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(second.observed_files, 3);
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].indexed_occurrences,
        3,
        "a newly ignored file must not remain searchable in the authoritative catalog"
    );
    assert!(matches!(
        service
            .resolve_rag_citation(
                &HOST,
                ResolveRagCitationRequestDto {
                    workspace_id: workspace_id.into_inner(),
                    source_id: visible_source,
                },
            )
            .await,
        Err(fm_application::ApplicationError::NotFound)
    ));
}

#[tokio::test]
async fn non_repository_content_and_explicit_gitignore_include_remain_indexable() {
    let root = project_temp_dir("include-root-");
    let repo = git2::Repository::init(root.path()).unwrap();
    std::fs::create_dir(root.path().join("drafts")).unwrap();
    std::fs::write(root.path().join(".gitignore"), "drafts/\n").unwrap();
    std::fs::write(root.path().join("drafts/note.md"), "semantic draft").unwrap();
    std::fs::write(root.path().join(".secret.md"), "semantic secret").unwrap();
    let state = project_temp_dir("include-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1992));
    let library = library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(root.path()).unwrap(),
    );
    let revision = library.status(&HOST).unwrap().revision;
    library
        .update_eligibility_overrides(
            &HOST,
            root_id,
            workspace_id,
            revision,
            BTreeMap::from([(EligibilityReason::GitIgnored, EligibilityOverride::Include)]),
        )
        .unwrap();
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));
    assert_eq!(
        service
            .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
            .await
            .unwrap()
            .observed_files,
        1
    );
    drop(repo);

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("note.md"), "semantic non git").unwrap();
    std::fs::write(outside.path().join(".gitignore"), "*.md\n").unwrap();
    let subrepo = outside.path().join("subrepo");
    std::fs::create_dir(&subrepo).unwrap();
    git2::Repository::init(&subrepo).unwrap();
    std::fs::write(subrepo.join(".gitignore"), "*.md\n").unwrap();
    std::fs::write(subrepo.join("ignored.md"), "semantic subrepo").unwrap();
    let state = project_temp_dir("non-repo-state-");
    let library = self::library(&state);
    let root_id = enrol(
        &library,
        workspace_id,
        Location::from_native_path(outside.path()).unwrap(),
    );
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        state.path().join("workspaces"),
        state.path().join("settings"),
    )
    .with_semantic_library_service(library)
    .with_semantic_capability(Arc::new(FakeSemanticCapability::new()));
    let report = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(report.observed_files, 1);
    assert!(
        report
            .skipped_reason_counts
            .iter()
            .any(|count| count.reason == EligibilityReason::GitIgnored)
    );
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
                    .iter()
                    .any(|(key, value)| { key != "title" && value.contains("welcome.txt") })
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

#[cfg(unix)]
#[tokio::test]
async fn unreadable_entries_keep_their_index_without_aborting_the_root() {
    use std::os::unix::fs::PermissionsExt;

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    let root = project_temp_dir("unreadable-root-");
    std::fs::create_dir_all(root.path().join("synced")).unwrap();
    std::fs::write(root.path().join("good.txt"), "searchable semantic content").unwrap();
    std::fs::write(root.path().join("placeholder.txt"), "cloud placeholder").unwrap();
    std::fs::write(root.path().join("synced/inner.txt"), "synced semantic note").unwrap();
    let state = project_temp_dir("unreadable-state-");
    let workspace_id = WorkspaceId::from(Uuid::from_u128(0x1904));
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
    let indexed_occurrences = || async {
        service.semantic_library_status(&HOST).await.unwrap().roots[0].indexed_occurrences
    };

    let first = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first.observed_files, 3);
    assert_eq!(indexed_occurrences().await, 3);

    set_mode(&root.path().join("placeholder.txt"), 0o000);
    set_mode(&root.path().join("synced"), 0o000);
    let second = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await;
    set_mode(&root.path().join("placeholder.txt"), 0o644);
    set_mode(&root.path().join("synced"), 0o755);
    let second = second.unwrap();

    assert_eq!(second.observed_files, 1);
    assert_eq!(second.unreadable_files, 1);
    assert_eq!(second.unreadable_directories, 1);
    assert_eq!(second.reconciliation_generation, 2);
    assert_eq!(
        indexed_occurrences().await,
        3,
        "unreadable entries keep their index"
    );

    std::fs::remove_file(root.path().join("placeholder.txt")).unwrap();
    let third = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(third.unreadable_files, 0);
    assert_eq!(
        indexed_occurrences().await,
        2,
        "a readable pass still prunes deletions"
    );

    set_mode(root.path(), 0o000);
    let unlisted_root = service
        .semantic_reconcile_enrolled_root(&HOST, root_id, CancellationToken::new())
        .await;
    set_mode(root.path(), 0o755);
    assert!(matches!(
        unlisted_root.unwrap_err(),
        SemanticIndexingError::Provider(_)
    ));
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
    assert!(
        capability
            .ingested_titles
            .lock()
            .unwrap()
            .iter()
            .any(|title| title == "one.pdf")
    );
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
