//! Public application-layer tests for semantic library enrolment (task 0179).

#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use fm_application::FileManagerService;
use fm_application::semantic_components::{
    AdministratorProvisionedSemanticComponentCapability, InstalledSemanticComponent,
    InstalledSemanticComponentState, SemanticComponentCapabilities, SemanticComponentCapability,
    SemanticComponentError, SemanticComponentKind, SemanticComponentLifecycle,
    SemanticComponentStatus, SemanticDiskUse, SemanticEmbeddingNormalization, SemanticLicense,
    SemanticModelIdentity, SemanticModelMetadata, SemanticModelProfile, SemanticModelSelection,
    SemanticProfile,
};
use fm_application::semantic_library::{
    FixedSemanticEnrolmentEstimator, SemanticAccessContext, SemanticCleanupExecutor,
    SemanticCleanupFailure, SemanticCleanupRequest, SemanticCleanupState, SemanticCommitObserver,
    SemanticDeletionCategory, SemanticEnrolmentEstimate, SemanticEstimateCompleteness,
    SemanticFeedCandidate, SemanticFolderConsent, SemanticFolderContext, SemanticLibraryAuthority,
    SemanticLibraryConfiguration, SemanticLibraryError, SemanticLibraryOperation,
    SemanticLibraryService, SemanticServerIdentity, UnavailableSemanticEnrolmentEstimator,
};
use fm_domain::{EntryId, Location, WorkspaceId};
use fm_semantic_library::{
    CatalogObservation, CommitStep, ContentFingerprint, ConversationEvidencePin, ConversationPinId,
    DeletionPlanId, DerivedArtifactId, DeviceLibraryIdentity, DocumentArtifacts,
    DocumentMeasurement, EligibilityCandidate, EligibilityDecision, EligibilityEntryKind,
    EligibilityReason, EligibilityReasonCounts, EnrolledRoot, FilesystemIdentity, HardQuotas,
    LibraryId, LibraryOperation, ModelIdentity, ObservedRootIdentity, OccurrenceScope,
    ResourceBudgets, ResourceProfile, ResourceProfileKind, RootId, RootMoveResolution,
    RootUnavailabilityReason, SemanticCatalog, SemanticLibraryCoordinator, SemanticLibraryPolicy,
    SemanticLibraryState, ServerEnrolmentPolicy,
};
use fm_transport_dto::RuntimeKindDto;
use fm_transport_dto::{ConfirmSemanticEnrolmentRequestDto, PreviewSemanticEnrolmentRequestDto};
use tempfile::TempDir;
use uuid::Uuid;

const HOST: SemanticAccessContext = SemanticAccessContext::Host;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-library-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

fn location(uri: &str) -> Location {
    Location::parse(uri).unwrap()
}

fn workspace(value: u128) -> WorkspaceId {
    WorkspaceId::from(Uuid::from_u128(value))
}

fn library_identity() -> DeviceLibraryIdentity {
    DeviceLibraryIdentity::new(
        LibraryId::from_uuid(Uuid::from_u128(1)),
        ModelIdentity::new("fixture-model", "revision-1", 384, "fixture-space").unwrap(),
    )
}

fn configuration(directory: &TempDir) -> SemanticLibraryConfiguration {
    SemanticLibraryConfiguration::new(
        directory.path().join("config"),
        directory.path().join("semantic-data"),
        library_identity(),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
}

fn estimate() -> SemanticEnrolmentEstimate {
    SemanticEnrolmentEstimate {
        completeness: SemanticEstimateCompleteness::Partial,
        estimated_files: Some(42),
        estimated_source_bytes: Some(4_200),
        estimated_extracted_bytes: Some(1_200),
        estimated_vector_bytes: Some(800),
        skipped_reason_counts: EligibilityReasonCounts::from_decisions([]),
        missing_model_download_bytes: Some(500),
        unavailable_reason: None,
        filesystem_identity: Some(fm_application::semantic_library::SemanticRootIdentity::new(
            "volume", "folder",
        )),
    }
}

fn desktop(directory: &TempDir) -> SemanticLibraryService {
    SemanticLibraryService::desktop_managed(
        configuration(directory),
        Arc::new(FixedSemanticEnrolmentEstimator::new(estimate())),
    )
    .unwrap()
}

/// Enrols one root through the public preview/confirm protocol.
fn enrol(service: &SemanticLibraryService, context: &SemanticFolderContext) -> u64 {
    let preview = service
        .preview_enrolment(&HOST, context.clone(), true)
        .unwrap();
    service
        .confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            context,
        )
        .unwrap()
        .revision
}

/// Refuses one specific durable commit step, exactly as an interrupted process
/// or a failing filesystem would.
struct StepFailpoint {
    step: CommitStep,
    armed: Mutex<bool>,
}

impl StepFailpoint {
    fn new(step: CommitStep) -> Arc<Self> {
        Arc::new(Self {
            step,
            armed: Mutex::new(true),
        })
    }

    fn disarm(&self) {
        *self.armed.lock().unwrap() = false;
    }
}

impl SemanticCommitObserver for StepFailpoint {
    fn before_step(&self, step: CommitStep) -> Result<(), SemanticCleanupFailure> {
        if step == self.step && *self.armed.lock().unwrap() {
            return Err(SemanticCleanupFailure::new("injected commit failure"));
        }
        Ok(())
    }
}

/// Fails exactly one deletion category until it is disarmed.
struct FailingCleanupExecutor {
    category: SemanticDeletionCategory,
}

impl SemanticCleanupExecutor for FailingCleanupExecutor {
    fn execute(&self, request: &SemanticCleanupRequest) -> Result<(), SemanticCleanupFailure> {
        if request.category() == self.category {
            Err(SemanticCleanupFailure::new("injected cleanup failure"))
        } else {
            Ok(())
        }
    }
}

/// Records the categories a cleanup run actually executed.
struct RecordingCleanupExecutor {
    executed: Mutex<Vec<SemanticDeletionCategory>>,
    keys: Mutex<Vec<String>>,
}

impl RecordingCleanupExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            executed: Mutex::new(Vec::new()),
            keys: Mutex::new(Vec::new()),
        })
    }
}

impl SemanticCleanupExecutor for RecordingCleanupExecutor {
    fn execute(&self, request: &SemanticCleanupRequest) -> Result<(), SemanticCleanupFailure> {
        self.executed.lock().unwrap().push(request.category());
        self.keys
            .lock()
            .unwrap()
            .push(request.idempotency_key().to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn normal_service_constructor_keeps_semantic_library_inert() {
    let directory = project_temp_dir("inert-");
    let config = directory.path().join("config");
    let semantic_data = directory.path().join("semantic-data");
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        directory.path().join("workspaces"),
        &config,
    );

    assert_eq!(
        service
            .semantic_library_capabilities(&HOST)
            .await
            .authority(),
        SemanticLibraryAuthority::Unavailable
    );
    assert!(
        !service
            .semantic_library_status(&HOST)
            .await
            .unwrap()
            .available
    );
    assert!(!config.join("semantic-library-policy.json").exists());
    assert!(!semantic_data.exists());
}

#[test]
fn deterministic_mock_previews_confirms_and_inherits_without_claiming_exact_counts() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let preview = service
        .preview_enrolment(&HOST, context.clone(), true)
        .unwrap();

    assert_eq!(
        preview.estimate.completeness,
        SemanticEstimateCompleteness::Partial
    );
    assert!(preview.normalized_excerpts_retained_locally);
    assert_eq!(preview.estimate.estimated_files, Some(42));

    let enrolled = service
        .confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            &context,
        )
        .unwrap();
    assert_eq!(enrolled.roots.len(), 1);
    assert_eq!(
        service
            .folder_status(
                &HOST,
                &SemanticFolderContext::new(workspace(10), location("file:///docs/child"))
            )
            .unwrap()
            .consent,
        SemanticFolderConsent::InheritedFromParent
    );
}

#[test]
fn stale_confirmation_cannot_change_consent() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let preview = service
        .preview_enrolment(&HOST, context.clone(), true)
        .unwrap();
    service.pause(&HOST, preview.policy_revision).unwrap();

    assert!(matches!(
        service.confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            &context
        ),
        Err(SemanticLibraryError::StaleRevision { .. })
    ));
    assert_eq!(
        service.folder_status(&HOST, &context).unwrap().consent,
        SemanticFolderConsent::NotIncluded
    );
}

#[test]
fn exclusion_plan_is_authoritative_and_cleanup_covers_every_artifact_category() {
    let service = SemanticLibraryService::deterministic_mock();
    let root = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let revision = enrol(&service, &root);
    let child = SemanticFolderContext::new(workspace(10), location("file:///docs/private"));
    let plan = service
        .plan_exclusion(&HOST, child.clone(), revision)
        .unwrap();

    assert_eq!(plan.categories.len(), 6);
    assert!(
        plan.categories
            .iter()
            .any(|category| category.category
                == SemanticDeletionCategory::ConversationEvidencePins)
    );

    let status = service
        .confirm_exclusion(&HOST, &plan.confirmation_id, plan.policy_revision, &child)
        .unwrap();
    assert_eq!(
        service.folder_status(&HOST, &child).unwrap().consent,
        SemanticFolderConsent::Excluded
    );
    assert_eq!(
        status.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Complete
    );
}

#[test]
fn workspace_detach_preserves_global_root_consent() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///shared"));
    let revision = enrol(&service, &context);

    service.detach_workspace(workspace(10)).unwrap();

    let status = service.status(&HOST).unwrap();
    assert_eq!(status.roots.len(), 1);
    assert!(status.roots[0].workspace_references.is_empty());
    assert!(status.revision > revision);
}

#[test]
fn the_same_global_root_can_be_attached_to_overlapping_workspace_scopes_once() {
    let service = SemanticLibraryService::deterministic_mock();
    let first = SemanticFolderContext::new(workspace(10), location("file:///shared"));
    enrol(&service, &first);
    let first_status = service.status(&HOST).unwrap();
    let second = SemanticFolderContext::new(workspace(11), location("file:///shared"));
    assert!(
        !service
            .folder_status(&HOST, &second)
            .unwrap()
            .workspace_referenced
    );
    enrol(&service, &second);
    let second_status = service.status(&HOST).unwrap();

    assert_eq!(first_status.roots[0].id, second_status.roots[0].id);
    assert_eq!(second_status.roots.len(), 1);
    assert_eq!(
        second_status.roots[0].workspace_references,
        [Uuid::from(workspace(10)), Uuid::from(workspace(11))]
    );
}

#[test]
fn unavailable_root_retains_consent_and_reports_source_unavailable() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///removable"));
    enrol(&service, &context);
    let root_id: RootId = service.status(&HOST).unwrap().roots[0].id.parse().unwrap();

    service
        .mark_root_unavailable(&HOST, root_id, RootUnavailabilityReason::Missing)
        .unwrap();

    let folder = service.folder_status(&HOST, &context).unwrap();
    assert_eq!(folder.consent, SemanticFolderConsent::IncludedHere);
    assert!(!folder.source_available);
    assert!(folder.unavailable_reason.is_some());

    // Only a provider observation carrying the enrolled stable identity may
    // restore availability; a path that reads again proves nothing.
    service
        .observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///removable"),
                Some(FilesystemIdentity::new("mock-volume", "mock-folder").unwrap()),
            )],
        )
        .unwrap();
    assert!(
        service
            .folder_status(&HOST, &context)
            .unwrap()
            .source_available
    );
}

#[test]
fn service_follows_only_a_stable_same_volume_root_move() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///before"));
    enrol(&service, &context);
    let root_id: RootId = service.status(&HOST).unwrap().roots[0].id.parse().unwrap();

    let resolution = service
        .observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///after"),
                Some(FilesystemIdentity::new("mock-volume", "mock-folder").unwrap()),
            )],
        )
        .unwrap();

    assert!(matches!(resolution, RootMoveResolution::ProvenMove { .. }));
    assert_eq!(
        service.status(&HOST).unwrap().roots[0].location,
        location("file:///after")
    );
}

struct CountingEstimator {
    calls: AtomicUsize,
}

impl fm_application::semantic_library::SemanticEnrolmentEstimator for CountingEstimator {
    fn estimate(&self, _location: &Location, _recursive: bool) -> SemanticEnrolmentEstimate {
        self.calls.fetch_add(1, Ordering::SeqCst);
        estimate()
    }
}

#[test]
fn one_server_library_serves_its_administrator_and_denies_every_other_principal() {
    let directory = project_temp_dir("server-");
    let estimator = Arc::new(CountingEstimator {
        calls: AtomicUsize::new(0),
    });
    let config_root = directory.path().join("config");
    let data_root = directory.path().join("semantic-data");
    let service = SemanticLibraryService::server_single_private(
        configuration(&directory),
        SemanticServerIdentity::new("tenant-a", "admin-a").unwrap(),
        ServerEnrolmentPolicy::local_only(),
        HardQuotas::default(),
        estimator.clone(),
    )
    .unwrap();
    let administrator = SemanticAccessContext::server("tenant-a", "admin-a").unwrap();
    let other_user = SemanticAccessContext::server("tenant-a", "other-user").unwrap();
    let other_tenant = SemanticAccessContext::server("tenant-b", "admin-a").unwrap();

    // The same instance answers the administrator and denies everyone else:
    // authority is not baked into the service.
    assert!(service.status(&administrator).is_ok());
    assert!(matches!(
        service.status(&other_user),
        Err(SemanticLibraryError::AccessDenied)
    ));
    assert!(matches!(
        service.status(&other_tenant),
        Err(SemanticLibraryError::AccessDenied)
    ));
    // A desktop host context cannot borrow server authority either.
    assert!(matches!(
        service.status(&HOST),
        Err(SemanticLibraryError::AccessDenied)
    ));
    assert!(matches!(
        service.status(&SemanticAccessContext::Anonymous),
        Err(SemanticLibraryError::AccessDenied)
    ));

    // Capabilities are per caller: the administrator sees the read-only server
    // surface, and every denied principal is told nothing is available rather
    // than being shown operations it can never invoke.
    assert_eq!(
        service.capabilities(&administrator).operations(),
        &[
            SemanticLibraryOperation::ViewStatus,
            SemanticLibraryOperation::ViewFolderStatus,
        ]
    );
    for denied in [
        &other_user,
        &other_tenant,
        &HOST,
        &SemanticAccessContext::Anonymous,
    ] {
        let capabilities = service.capabilities(denied);
        assert!(
            capabilities.operations().is_empty(),
            "a denied principal must not be advertised administrator operations"
        );
        assert_eq!(
            capabilities.authority(),
            SemanticLibraryAuthority::AdministratorProvisioned
        );
        assert!(matches!(
            service.ensure_operation_allowed(denied, SemanticLibraryOperation::ViewStatus),
            Err(SemanticLibraryError::AuthorityDenied { .. })
        ));
    }
    assert!(matches!(
        service.preview_enrolment(
            &administrator,
            SemanticFolderContext::new(workspace(10), location("file:///never-read")),
            true
        ),
        Err(SemanticLibraryError::AuthorityDenied { .. })
    ));
    assert_eq!(estimator.calls.load(Ordering::SeqCst), 0);
    // An authorized read may materialise the cross-process lock — journal
    // recovery runs on the read path and must never race another process — but
    // nothing else: no policy, catalog, runtime state, or journal exists.
    assert!(!config_root.exists());
    assert_eq!(
        data_root_entries(&data_root),
        vec!["library.lock".to_owned()],
        "an authorized read may create only the cross-process lock"
    );
}

/// Returns the file and directory names directly beneath a semantic-data root.
fn data_root_entries(data_root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(data_root) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_denied_server_user_cannot_reach_semantic_storage_through_workspace_deletion() {
    let directory = project_temp_dir("server-detach-");
    let config_root = directory.path().join("config");
    let data_root = directory.path().join("semantic-data");
    let service = SemanticLibraryService::server_single_private_read_only(
        configuration(&directory),
        SemanticServerIdentity::new("tenant-a", "admin-a").unwrap(),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();

    // Workspace deletion must still succeed against an administrator-owned
    // server library, and it must not touch a single byte beneath its roots.
    service.detach_workspace(workspace(10)).unwrap();

    assert!(!config_root.exists());
    assert!(!data_root.exists());
}

#[test]
fn unsafe_eligibility_overrides_are_rejected_without_changing_revision() {
    let service = SemanticLibraryService::deterministic_mock();
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let revision = enrol(&service, &context);
    let root_id: RootId = service.status(&HOST).unwrap().roots[0].id.parse().unwrap();

    assert!(matches!(
        service.update_eligibility_overrides(
            &HOST,
            root_id,
            workspace(10),
            revision,
            [(
                EligibilityReason::SymlinkOutsideRoot,
                fm_semantic_library::EligibilityOverride::Include
            )]
            .into()
        ),
        Err(SemanticLibraryError::UnsafeEligibilityOverride)
    ));
    assert_eq!(service.status(&HOST).unwrap().revision, revision);
}

#[test]
fn desktop_managed_storage_is_inert_until_consent_then_reloads_from_explicit_roots() {
    let directory = project_temp_dir("persistent-");
    let configuration = configuration(&directory);
    let policy_path = directory.path().join("config/semantic-library-policy.json");
    let data_root = directory.path().join("semantic-data");
    let service = SemanticLibraryService::desktop_managed(
        configuration.clone(),
        Arc::new(FixedSemanticEnrolmentEstimator::new(estimate())),
    )
    .unwrap();

    assert!(!policy_path.exists());
    assert!(!data_root.exists());
    assert!(service.status(&HOST).unwrap().available);

    let context = SemanticFolderContext::new(workspace(10), location("file:///persisted"));
    enrol(&service, &context);
    assert!(policy_path.exists());
    assert!(data_root.join("catalog/catalog.json").exists());
    assert!(data_root.join("state/library-state.json").exists());

    let reloaded = SemanticLibraryService::desktop_managed(
        configuration,
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    assert_eq!(reloaded.status(&HOST).unwrap().roots.len(), 1);
}

#[test]
fn unavailable_estimator_returns_an_explicit_non_numeric_preview() {
    let directory = project_temp_dir("estimate-unavailable-");
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let preview = service
        .preview_enrolment(
            &HOST,
            SemanticFolderContext::new(workspace(10), location("file:///docs")),
            true,
        )
        .unwrap();

    assert_eq!(
        preview.estimate.completeness,
        SemanticEstimateCompleteness::Unavailable
    );
    assert_eq!(preview.estimate.estimated_files, None);
    assert!(preview.estimate.unavailable_reason.is_some());
}

#[test]
fn a_failure_at_any_commit_step_discards_the_snapshot_and_reloads_durable_state() {
    for step in CommitStep::all() {
        let directory = project_temp_dir("failpoint-");
        let failpoint = StepFailpoint::new(*step);
        let service = SemanticLibraryService::desktop_managed(
            configuration(&directory),
            Arc::new(FixedSemanticEnrolmentEstimator::new(estimate())),
        )
        .unwrap()
        .with_commit_observer(failpoint.clone());
        let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
        let preview = service
            .preview_enrolment(&HOST, context.clone(), true)
            .unwrap();

        let outcome = service.confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            &context,
        );

        assert!(
            matches!(outcome, Err(SemanticLibraryError::Persistence)),
            "a refused {step:?} must surface as a persistence failure"
        );
        // Replaying the same confirmation must be impossible: the snapshot it
        // was minted against is gone.
        assert!(
            matches!(
                service.confirm_enrolment(
                    &HOST,
                    &preview.confirmation_id,
                    preview.policy_revision,
                    &context
                ),
                Err(SemanticLibraryError::StaleConfirmation)
                    | Err(SemanticLibraryError::StaleRevision { .. })
            ),
            "a consumed confirmation must never resurrect consent after {step:?}"
        );

        // Only a refusal strictly after the durable intent leaves a committed
        // record for recovery to replay.
        let committed = matches!(
            step,
            CommitStep::InstallCatalog
                | CommitStep::InstallPolicy
                | CommitStep::InstallState
                | CommitStep::ClearJournal
        );
        failpoint.disarm();
        for reader in [
            &service,
            &SemanticLibraryService::desktop_managed(
                configuration(&directory),
                Arc::new(UnavailableSemanticEnrolmentEstimator),
            )
            .unwrap(),
        ] {
            let status = reader.status(&HOST).unwrap();
            assert_eq!(
                status.roots.len(),
                usize::from(committed),
                "durable state after a refused {step:?} must decide consent"
            );
            assert_eq!(
                reader.folder_status(&HOST, &context).unwrap().consent,
                if committed {
                    SemanticFolderConsent::IncludedHere
                } else {
                    SemanticFolderConsent::NotIncluded
                }
            );
        }
    }
}

#[test]
fn pause_survives_a_restart_because_runtime_state_is_a_journal_participant() {
    let directory = project_temp_dir("pause-durable-");
    let service = desktop(&directory);
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let revision = enrol(&service, &context);

    let paused = service.pause(&HOST, revision).unwrap();
    assert!(paused.paused);

    let restarted = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let status = restarted.status(&HOST).unwrap();
    assert!(status.paused);
    assert_eq!(status.roots.len(), 1, "pause never revokes consent");

    let resumed = restarted.resume(&HOST, status.revision).unwrap();
    assert!(!resumed.paused);
    assert!(!service.status(&HOST).unwrap().paused);
}

#[test]
fn a_completed_reconciliation_records_a_crash_safe_indexed_generation() {
    let directory = project_temp_dir("generation-");
    let service = desktop(&directory);
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    enrol(&service, &context);
    let root_id: RootId = service.status(&HOST).unwrap().roots[0].id.parse().unwrap();

    let generation = service
        .complete_reconciliation(&HOST, root_id, &BTreeSet::new())
        .unwrap();

    assert_eq!(generation, 1);
    let restarted = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let status = restarted.status(&HOST).unwrap();
    assert_eq!(status.roots[0].indexed_generation, 1);
    assert_eq!(status.roots[0].reconciliation_generation, 1);
}

#[test]
fn a_second_service_over_the_same_roots_cannot_lose_the_first_services_update() {
    let directory = project_temp_dir("lost-update-");
    let first = desktop(&directory);
    let second = desktop(&directory);
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));

    let revision = enrol(&first, &context);
    // The second service has never seen that mutation in memory, but reads it
    // from disk under the lock.
    assert_eq!(second.status(&HOST).unwrap().revision, revision);

    let other = SemanticFolderContext::new(workspace(10), location("file:///other"));
    let after_second = enrol(&second, &other);
    assert!(after_second > revision);

    // The first service still believes it is at the older revision. Comparing
    // against the durable value turns the lost update into an explicit
    // conflict.
    let stale = first.pause(&HOST, revision);
    assert!(
        matches!(
            stale,
            Err(SemanticLibraryError::StaleRevision {
                expected,
                actual
            }) if expected == revision && actual == after_second
        ),
        "a process-local revision must never be the optimistic authority"
    );
    assert_eq!(first.status(&HOST).unwrap().roots.len(), 2);
}

#[test]
fn a_confirmation_minted_before_another_writer_committed_is_refused() {
    let directory = project_temp_dir("external-confirmation-");
    let first = desktop(&directory);
    let second = desktop(&directory);
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));

    let preview = first
        .preview_enrolment(&HOST, context.clone(), true)
        .unwrap();
    enrol(
        &second,
        &SemanticFolderContext::new(workspace(10), location("file:///other")),
    );

    assert!(matches!(
        first.confirm_enrolment(
            &HOST,
            &preview.confirmation_id,
            preview.policy_revision,
            &context
        ),
        Err(SemanticLibraryError::StaleRevision { .. })
    ));
    assert_eq!(
        first.folder_status(&HOST, &context).unwrap().consent,
        SemanticFolderConsent::NotIncluded
    );
}

#[test]
fn concurrent_services_serialize_through_the_exclusive_library_lock() {
    let directory = project_temp_dir("lock-");
    let roots = ["file:///a", "file:///b", "file:///c", "file:///d"];
    // The semantic-data root does not exist yet, which is exactly the window
    // in which a lock that only appears "once there is something to lock"
    // would let two services recover and commit over each other.
    assert!(!directory.path().join("semantic-data").exists());
    std::thread::scope(|scope| {
        for uri in roots {
            let configuration = configuration(&directory);
            scope.spawn(move || {
                let service = SemanticLibraryService::desktop_managed(
                    configuration,
                    Arc::new(FixedSemanticEnrolmentEstimator::new(estimate())),
                )
                .unwrap();
                let context = SemanticFolderContext::new(workspace(10), location(uri));
                // Retry only on an explicit optimistic conflict: a lost update
                // would show up as a missing root at the end instead.
                for _ in 0..64 {
                    let preview = service
                        .preview_enrolment(&HOST, context.clone(), true)
                        .unwrap();
                    match service.confirm_enrolment(
                        &HOST,
                        &preview.confirmation_id,
                        preview.policy_revision,
                        &context,
                    ) {
                        Ok(_) => return,
                        Err(SemanticLibraryError::StaleRevision { .. })
                        | Err(SemanticLibraryError::StaleConfirmation) => continue,
                        Err(error) => panic!("unexpected failure: {error:?}"),
                    }
                }
                panic!("enrolment never converged for {uri}");
            });
        }
    });

    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let status = service.status(&HOST).unwrap();
    assert_eq!(status.roots.len(), roots.len());
    assert_eq!(status.revision, 1 + roots.len() as u64);
}

/// Seeds a durable library with one enrolled root, two occurrences of one
/// document, derived artifacts, and a saved-conversation evidence pin.
fn seed_populated_library(directory: &TempDir) -> (RootId, SemanticFolderContext) {
    let mut policy = SemanticLibraryPolicy::new(
        library_identity(),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();
    let root_id = RootId::from_uuid(Uuid::from_u128(0x5eed));
    let mut root = EnrolledRoot::new(
        root_id,
        location("file:///docs"),
        Some(FilesystemIdentity::new("seed-volume", "seed-folder").unwrap()),
        true,
    );
    root.attach_workspace(workspace(10));
    policy.enrol_root(root).unwrap();

    let library_id = policy.library().id();
    let mut catalog = SemanticCatalog::new(library_id);
    let state = SemanticLibraryState::new(library_id);
    catalog.mark_root_available(root_id);
    let artifacts = DocumentArtifacts {
        extracted_content: [DerivedArtifactId::from_uuid(Uuid::from_u128(0xe1))].into(),
        summaries: [DerivedArtifactId::from_uuid(Uuid::from_u128(0xe2))].into(),
        labels: [DerivedArtifactId::from_uuid(Uuid::from_u128(0xe3))].into(),
        vectors: [DerivedArtifactId::from_uuid(Uuid::from_u128(0xe4))].into(),
    };
    catalog
        .upsert_observations(
            &policy,
            &state,
            [CatalogObservation::new(
                EntryId::from(Uuid::from_u128(100)),
                location("file:///docs/private/secret.txt"),
                ContentFingerprint::new("sha256:secret").unwrap(),
                OccurrenceScope::new(workspace(10), root_id),
                artifacts,
                DocumentMeasurement::new(64, 32, 16),
            )],
        )
        .unwrap();
    let occurrence_id = catalog
        .occurrences()
        .next()
        .expect("seeded occurrence")
        .id();
    catalog
        .add_conversation_pin(ConversationEvidencePin::new(
            ConversationPinId::from_uuid(Uuid::from_u128(0x9)),
            occurrence_id,
            OccurrenceScope::new(workspace(10), root_id),
        ))
        .unwrap();

    let coordinator = SemanticLibraryCoordinator::new(
        directory.path().join("config"),
        directory.path().join("semantic-data"),
    );
    coordinator
        .lock()
        .unwrap()
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            Some(&state),
        )
        .unwrap()
        .commit()
        .unwrap();
    (
        root_id,
        SemanticFolderContext::new(workspace(10), location("file:///docs/private")),
    )
}

#[test]
fn a_failed_cleanup_category_revokes_consent_immediately_and_stays_resumable() {
    let directory = project_temp_dir("cleanup-failure-");
    let (root_id, excluded) = seed_populated_library(&directory);
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(FixedSemanticEnrolmentEstimator::new(estimate())),
    )
    .unwrap()
    .with_cleanup_executor(Arc::new(FailingCleanupExecutor {
        category: SemanticDeletionCategory::Occurrences,
    }));

    let revision = service.status(&HOST).unwrap().revision;
    let plan = service
        .plan_exclusion(&HOST, excluded.clone(), revision)
        .unwrap();
    assert!(
        plan.categories
            .iter()
            .any(|category| category.total_items > 0),
        "the authoritative plan must count real artifacts"
    );

    let status = service
        .confirm_exclusion(
            &HOST,
            &plan.confirmation_id,
            plan.policy_revision,
            &excluded,
        )
        .unwrap();
    let cleanup = &status.roots[0].exclusions[0].cleanup;

    assert_eq!(cleanup.status, SemanticCleanupState::Failed);
    assert!(cleanup.plan_id.is_some());
    assert!(
        cleanup
            .categories
            .iter()
            .any(|category| category.last_error.is_some())
    );
    // Consent is already revoked, so the feed refuses the scope even though no
    // artifact has been deleted yet.
    assert_eq!(
        service.folder_status(&HOST, &excluded).unwrap().consent,
        SemanticFolderConsent::Excluded
    );
    assert!(
        service
            .worker_feed_plan(&HOST, &[])
            .unwrap()
            .decisions
            .is_empty(),
        "a revoked scope must never be fed while cleanup is still pending"
    );

    // The failure is durable, not an in-memory state machine.
    let restarted = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let restarted_status = restarted.status(&HOST).unwrap();
    assert_eq!(
        restarted_status.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Failed
    );

    // Resuming with a working executor completes every mandatory category and
    // removes the retained evidence pin.
    let recording = RecordingCleanupExecutor::new();
    let resumed_service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_cleanup_executor(recording.clone());
    let plan_id: DeletionPlanId = restarted_status.roots[0].exclusions[0]
        .cleanup
        .plan_id
        .clone()
        .unwrap()
        .parse()
        .unwrap();
    let completed = resumed_service
        .resume_cleanup(&HOST, plan_id, restarted_status.revision)
        .unwrap();
    let cleanup = &completed.roots[0].exclusions[0].cleanup;

    assert_eq!(cleanup.status, SemanticCleanupState::Complete);
    assert_eq!(cleanup.categories.len(), 6);
    assert!(cleanup.categories.iter().all(|category| category.complete));
    assert_eq!(recording.executed.lock().unwrap().len(), 6);
    assert_eq!(
        resumed_service
            .worker_feed_plan(&HOST, &[])
            .unwrap()
            .decisions
            .len(),
        0
    );
    assert_eq!(root_id.to_string(), completed.roots[0].id);
}

#[test]
fn the_worker_feed_applies_the_curated_eligibility_policy_and_stops_while_paused() {
    let directory = project_temp_dir("feed-");
    let (root_id, _) = seed_populated_library(&directory);
    let service = desktop(&directory);

    let plan = service
        .worker_feed_plan(
            &HOST,
            &[
                SemanticFeedCandidate {
                    root_id,
                    candidate: candidate("file:///docs/report.md", Some("text/markdown"), false),
                },
                SemanticFeedCandidate {
                    root_id,
                    candidate: candidate("file:///docs/.hidden.md", Some("text/markdown"), true),
                },
                SemanticFeedCandidate {
                    root_id,
                    candidate: candidate(
                        "file:///docs/binary.bin",
                        Some("application/x-thing"),
                        false,
                    ),
                },
            ],
        )
        .unwrap();

    assert!(!plan.paused);
    assert_eq!(
        plan.eligible_locations,
        [location("file:///docs/report.md")]
    );
    assert_eq!(
        plan.skipped_reason_counts
            .iter()
            .find(|count| count.reason == EligibilityReason::Hidden)
            .map(|count| count.count),
        Some(1)
    );
    assert_eq!(
        plan.skipped_reason_counts
            .iter()
            .find(|count| count.reason == EligibilityReason::UnsupportedMime)
            .map(|count| count.count),
        Some(1)
    );
    assert_eq!(plan.decisions.len(), 1, "one consented occurrence is fed");

    let revision = service.status(&HOST).unwrap().revision;
    service.pause(&HOST, revision).unwrap();
    let paused = service.worker_feed_plan(&HOST, &[]).unwrap();

    assert!(paused.paused);
    assert!(
        paused.decisions.is_empty(),
        "pause stops new work without discarding indexed generations"
    );
}

fn candidate(uri: &str, mime: Option<&str>, hidden: bool) -> EligibilityCandidate {
    EligibilityCandidate {
        location: location(uri),
        kind: EligibilityEntryKind::File,
        hidden,
        system: false,
        application_or_package_bundle: false,
        git_ignored: false,
        mime_type: mime.map(str::to_owned),
        source_bytes: 1_024,
        estimated_extracted_bytes: 512,
        estimated_vector_bytes: 256,
        symlink_target: None,
    }
}

#[test]
fn an_unavailable_root_keeps_its_evidence_until_a_successful_reconciliation() {
    let directory = project_temp_dir("availability-");
    let (root_id, _) = seed_populated_library(&directory);
    let service = desktop(&directory);

    service
        .mark_root_unavailable(&HOST, root_id, RootUnavailabilityReason::Missing)
        .unwrap();

    // Absence is not deletion: a reconciliation cannot complete for a root that
    // is not currently observed.
    assert!(matches!(
        service.complete_reconciliation(&HOST, root_id, &BTreeSet::new()),
        Err(SemanticLibraryError::InvalidRequest)
    ));
    let status = service.status(&HOST).unwrap();
    assert!(matches!(
        status.roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::TemporarilyUnavailable { .. }
    ));

    service
        .observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                location("file:///docs"),
                Some(FilesystemIdentity::new("seed-volume", "seed-folder").unwrap()),
            )],
        )
        .unwrap();
    let generation = service
        .complete_reconciliation(&HOST, root_id, &BTreeSet::new())
        .unwrap();

    assert_eq!(generation, 1);
    assert!(
        service
            .worker_feed_plan(&HOST, &[])
            .unwrap()
            .decisions
            .is_empty(),
        "only a successful complete reconciliation removes missing documents"
    );
}

#[tokio::test]
async fn quarantined_roots_are_only_restored_by_a_proven_identity_observation() {
    let directory = project_temp_dir("watcher-seam-");
    // Deliberately outside the default workspace root, which is the home
    // directory: an unrelated location must not move any enrolled root.
    let unrelated = location("file:///procyon-0179-unrelated");
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    );
    let workspace_dto = service.start_workspace(None).await.unwrap();
    let pane = workspace_dto
        .panes
        .iter()
        .find(|pane| pane.id == workspace_dto.active_pane_id)
        .unwrap();
    let tab = pane
        .tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .unwrap();

    let preview = service
        .preview_semantic_enrolment(
            &HOST,
            PreviewSemanticEnrolmentRequestDto {
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
                recursive: true,
            },
        )
        .await
        .unwrap();
    service
        .confirm_semantic_enrolment(
            &HOST,
            ConfirmSemanticEnrolmentRequestDto {
                confirmation_id: preview.confirmation_id,
                policy_revision: preview.policy_revision,
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
            },
        )
        .await
        .unwrap();
    let enrolled_location = Location::from(tab.location.clone());

    // Quarantining needs no identity proof: it only ever reduces what the
    // library will do.
    service
        .semantic_library_quarantine_unreachable_root(&enrolled_location)
        .await;
    assert!(matches!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::TemporarilyUnavailable { .. }
    ));

    // Listing the very same path again — the strongest signal the directory
    // APIs can give — must not restore it, because the path may now name a
    // different directory entirely.
    service
        .list_directory(fm_transport_dto::ListDirectoryRequest {
            workspace_id: workspace_dto.id,
            pane_id: pane.id,
            request_id: Uuid::from_u128(0x179_0179),
            location: tab.location.clone(),
            continuation_token: None,
            sort: Vec::new(),
            show_hidden: false,
            folders_first: false,
            show_git_status: false,
        })
        .await
        .ok();
    assert!(
        matches!(
            service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
            fm_application::semantic_library::SemanticRootAvailability::TemporarilyUnavailable { .. }
        ),
        "a readable path is not proof of identity and must not restore a quarantined root"
    );

    // An observation whose identity does not match is equally powerless.
    let root_id: RootId = service.semantic_library_status(&HOST).await.unwrap().roots[0]
        .id
        .parse()
        .unwrap();
    let reused = service
        .semantic_library_observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                enrolled_location.clone(),
                Some(FilesystemIdentity::new("mock-volume", "another-folder").unwrap()),
            )],
        )
        .await
        .unwrap();
    assert_eq!(
        reused,
        RootMoveResolution::RetainedUnavailable {
            reason: RootUnavailabilityReason::PathReused
        }
    );
    assert!(matches!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::TemporarilyUnavailable { .. }
    ));

    // Only the enrolled stable identity restores it.
    service
        .semantic_library_observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                enrolled_location.clone(),
                Some(FilesystemIdentity::new("mock-volume", "mock-folder").unwrap()),
            )],
        )
        .await
        .unwrap();
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::Available
    );

    // An unrelated location must not touch any enrolled root.
    service
        .semantic_library_quarantine_unreachable_root(&unrelated)
        .await;
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::Available
    );

    // Neither may one unreadable folder inside the root: a permission-denied
    // subfolder must not stop ingestion for everything else beneath it.
    let descendant = enrolled_location.join("unreadable-subfolder").unwrap();
    service
        .semantic_library_quarantine_unreachable_root(&descendant)
        .await;
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::Available
    );
}

#[test]
fn a_stale_writer_cannot_install_over_a_commit_it_never_observed() {
    let directory = project_temp_dir("first-commit-race-");
    let first = desktop(&directory);
    let second = desktop(&directory);
    let first_context = SemanticFolderContext::new(workspace(10), location("file:///first"));
    let second_context = SemanticFolderContext::new(workspace(10), location("file:///second"));

    // Both services read the same empty durable state and mint a confirmation
    // before either has written anything: the semantic-data root does not exist
    // yet, so this is the one window in which no cross-process file lock is
    // held.
    let first_preview = first
        .preview_enrolment(&HOST, first_context.clone(), true)
        .unwrap();
    let second_preview = second
        .preview_enrolment(&HOST, second_context.clone(), true)
        .unwrap();
    assert_eq!(
        first_preview.policy_revision,
        second_preview.policy_revision
    );

    first
        .confirm_enrolment(
            &HOST,
            &first_preview.confirmation_id,
            first_preview.policy_revision,
            &first_context,
        )
        .unwrap();

    // The second service decided from a snapshot that is no longer durable.
    // Installing it would silently discard the first enrolment.
    assert!(
        matches!(
            second.confirm_enrolment(
                &HOST,
                &second_preview.confirmation_id,
                second_preview.policy_revision,
                &second_context,
            ),
            Err(SemanticLibraryError::StaleRevision { .. })
        ),
        "a first-ever commit must not be lost to a concurrent writer"
    );

    let status = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .status(&HOST)
    .unwrap();
    assert_eq!(status.roots.len(), 1);
    assert_eq!(status.roots[0].location, location("file:///first"));
}

#[tokio::test]
async fn facade_requires_the_active_folder_and_workspace_deletion_only_detaches_it() {
    let directory = project_temp_dir("facade-workspace-");
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    );
    let workspace_dto = service.start_workspace(None).await.unwrap();
    let pane = workspace_dto
        .panes
        .iter()
        .find(|pane| pane.id == workspace_dto.active_pane_id)
        .unwrap();
    let tab = pane
        .tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .unwrap();
    let preview = service
        .preview_semantic_enrolment(
            &HOST,
            PreviewSemanticEnrolmentRequestDto {
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
                recursive: true,
            },
        )
        .await
        .unwrap();
    let enrolled = service
        .confirm_semantic_enrolment(
            &HOST,
            ConfirmSemanticEnrolmentRequestDto {
                confirmation_id: preview.confirmation_id,
                policy_revision: preview.policy_revision,
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(enrolled.roots[0].workspace_references, [workspace_dto.id]);

    let mismatch = service
        .preview_semantic_enrolment(
            &HOST,
            PreviewSemanticEnrolmentRequestDto {
                workspace_id: workspace_dto.id,
                location: fm_transport_dto::LocationDto {
                    provider_id: "local".to_owned(),
                    uri: "file:///not-active".to_owned(),
                },
                recursive: true,
            },
        )
        .await;
    assert!(matches!(
        mismatch,
        Err(SemanticLibraryError::WorkspaceRequired)
    ));

    // A stale expected revision leaves the workspace alive, so its semantic
    // references must survive untouched: detaching them here would revoke the
    // scopes of a workspace the user still has.
    assert!(
        service
            .delete_workspace(workspace_dto.id, Some(workspace_dto.revision + 99))
            .await
            .is_err()
    );
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].workspace_references,
        [workspace_dto.id],
        "a failed workspace deletion must not detach a surviving workspace"
    );

    // Deleting a workspace that does not exist is equally powerless.
    assert!(
        service
            .delete_workspace(Uuid::from_u128(0x404), None)
            .await
            .is_err()
    );
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].workspace_references,
        [workspace_dto.id]
    );

    // A real deletion detaches it while keeping global root consent.
    service
        .delete_workspace(workspace_dto.id, None)
        .await
        .unwrap();
    let status = service.semantic_library_status(&HOST).await.unwrap();
    assert_eq!(status.roots.len(), 1);
    assert!(status.roots[0].workspace_references.is_empty());
}

#[tokio::test]
async fn workspace_deletion_succeeds_when_no_semantic_capability_is_configured() {
    let directory = project_temp_dir("delete-unavailable-");
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    );
    let workspace_dto = service.start_workspace(None).await.unwrap();

    service
        .delete_workspace(workspace_dto.id, Some(workspace_dto.revision))
        .await
        .unwrap();

    assert!(service.list_workspaces().await.unwrap().is_empty());
}

fn installed_component_status(data_root: &Path) -> SemanticComponentStatus {
    SemanticComponentStatus::new(
        SemanticComponentLifecycle::InstalledEnabled,
        Some(data_root.to_path_buf()),
        Some(SemanticModelSelection::new(
            SemanticProfile::CompactMultilingual,
            SemanticModelIdentity::new("installed-model", "installed-revision-1"),
        )),
        None,
        vec![InstalledSemanticComponent::new(
            "artifact-1",
            "component-1",
            SemanticComponentKind::Model,
            "1.0.0",
            InstalledSemanticComponentState::Active,
            1_024,
        )],
        SemanticDiskUse::empty(),
    )
}

fn installed_profiles() -> Vec<SemanticModelProfile> {
    vec![SemanticModelProfile {
        profile: SemanticProfile::CompactMultilingual,
        recommended: true,
        explanation: "fixture".to_owned(),
        resolved_model: SemanticModelIdentity::new("installed-model", "installed-revision-1"),
        metadata: SemanticModelMetadata {
            identity: SemanticModelIdentity::new("installed-model", "installed-revision-1"),
            license: SemanticLicense {
                spdx: "MIT".to_owned(),
                notice: "fixture".to_owned(),
            },
            tokenizer: "fixture-tokenizer".to_owned(),
            dimensions: 384,
            normalization: SemanticEmbeddingNormalization::UnitLength,
            runtime_component_id: "runtime".to_owned(),
            runtime_version_requirement: "^1".to_owned(),
            language_coverage: vec!["en".to_owned()],
            estimated_disk_bytes: 1_024,
            estimated_ram_bytes: 2_048,
        },
    }]
}

#[tokio::test]
async fn the_desktop_capability_tracks_managed_component_state() {
    let directory = project_temp_dir("composition-");
    let settings = directory.path().join("settings");
    let data_root = directory.path().join("semantic");
    let moved_root = directory.path().join("semantic-moved");
    let components = MutableComponentCapability::new(
        installed_component_status(&data_root),
        installed_profiles(),
    );
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        directory.path().join("workspaces"),
        &settings,
    )
    .with_semantic_component_capability(components.clone());

    // Constructing and resolving the capability writes nothing: no consent
    // policy, and no semantic-data root.
    assert!(!settings.join("semantic-library-policy.json").exists());
    assert!(!data_root.exists());

    assert_eq!(
        service
            .semantic_library_capabilities(&HOST)
            .await
            .authority(),
        SemanticLibraryAuthority::DesktopManaged
    );
    let status = service.semantic_library_status(&HOST).await.unwrap();
    assert!(status.available);
    let installed_library = status.library.expect("configured library");
    assert_eq!(installed_library.model.model_id, "installed-model");
    assert!(!settings.join("semantic-library-policy.json").exists());
    // Only the cross-process lock beneath the composed library root exists.
    assert_eq!(
        data_root_entries(&data_root.join("library")),
        vec!["library.lock".to_owned()]
    );

    // Pausing components keeps the same library: pause is explicitly not an
    // uninstall, and recomposing here would be pointless churn.
    components.set_status(SemanticComponentStatus::new(
        SemanticComponentLifecycle::Paused,
        Some(data_root.clone()),
        Some(SemanticModelSelection::new(
            SemanticProfile::CompactMultilingual,
            SemanticModelIdentity::new("installed-model", "installed-revision-1"),
        )),
        None,
        Vec::new(),
        SemanticDiskUse::empty(),
    ));
    assert_eq!(
        service
            .semantic_library_status(&HOST)
            .await
            .unwrap()
            .library
            .expect("paused components keep their library"),
        installed_library
    );

    // Uninstalling turns the capability off again instead of leaving a cached
    // service writing to a root that is no longer managed.
    components.set_status(SemanticComponentStatus::absent(None));
    assert_eq!(
        service
            .semantic_library_capabilities(&HOST)
            .await
            .authority(),
        SemanticLibraryAuthority::Unavailable
    );
    assert!(
        !service
            .semantic_library_status(&HOST)
            .await
            .unwrap()
            .available
    );

    // Reinstalling at a moved data root composes a library rooted there, and
    // nothing is written beneath the old root.
    components.set_status(installed_component_status(&moved_root));
    assert!(
        service
            .semantic_library_status(&HOST)
            .await
            .unwrap()
            .available
    );
    assert_eq!(
        data_root_entries(&moved_root.join("library")),
        vec!["library.lock".to_owned()],
        "the composed library must follow the authoritative data root"
    );

    // A model migration addresses a different device-local library, because
    // the library identity is derived from the immutable embedding identity.
    components.set_status(SemanticComponentStatus::new(
        SemanticComponentLifecycle::InstalledEnabled,
        Some(moved_root.clone()),
        Some(SemanticModelSelection::new(
            SemanticProfile::CompactMultilingual,
            SemanticModelIdentity::new("migrated-model", "migrated-revision-1"),
        )),
        None,
        Vec::new(),
        SemanticDiskUse::empty(),
    ));
    components.set_profiles(migrated_profiles());
    let migrated = service
        .semantic_library_status(&HOST)
        .await
        .unwrap()
        .library
        .expect("the migrated model composes its own library");

    assert_eq!(migrated.model.model_id, "migrated-model");
    assert_ne!(
        migrated.library_id, installed_library.library_id,
        "another embedding space must never reuse the previous library's records"
    );
}

/// Component capability whose authoritative status and catalog can change, as
/// installing, pausing, moving, migrating, and uninstalling really do.
struct MutableComponentCapability {
    status: Mutex<SemanticComponentStatus>,
    profiles: Mutex<Vec<SemanticModelProfile>>,
}

impl MutableComponentCapability {
    fn new(status: SemanticComponentStatus, profiles: Vec<SemanticModelProfile>) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(status),
            profiles: Mutex::new(profiles),
        })
    }

    fn set_status(&self, status: SemanticComponentStatus) {
        *self.status.lock().unwrap() = status;
    }

    fn set_profiles(&self, profiles: Vec<SemanticModelProfile>) {
        *self.profiles.lock().unwrap() = profiles;
    }
}

#[async_trait::async_trait]
impl SemanticComponentCapability for MutableComponentCapability {
    async fn capabilities(&self) -> SemanticComponentCapabilities {
        SemanticComponentCapabilities::new(
            fm_application::semantic_components::SemanticComponentAuthority::DesktopManaged,
            Vec::new(),
            fm_application::semantic_components::RuntimeExecutableDownload::DirectDistribution,
        )
    }

    async fn status(&self) -> Result<SemanticComponentStatus, SemanticComponentError> {
        Ok(self.status.lock().unwrap().clone())
    }

    async fn catalog_profiles(&self) -> Result<Vec<SemanticModelProfile>, SemanticComponentError> {
        Ok(self.profiles.lock().unwrap().clone())
    }

    async fn installation_offer(
        &self,
        _profile: SemanticProfile,
    ) -> Result<
        fm_application::semantic_components::SemanticInstallationOffer,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn install_or_enable(
        &self,
        _consent: fm_application::semantic_components::SemanticInstallationConsent,
    ) -> Result<fm_application::semantic_components::SemanticInstallReceipt, SemanticComponentError>
    {
        Err(SemanticComponentError::Unavailable)
    }

    async fn install_compatible_worker_patch(
        &self,
        _request: fm_application::semantic_components::SemanticWorkerPatchRequest,
    ) -> Result<
        Option<fm_application::semantic_components::SemanticInstallReceipt>,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn pause_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn resume_indexing(&self) -> Result<(), SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }

    async fn remove_index(
        &self,
        _request: fm_application::semantic_components::RemoveSemanticIndexRequest,
    ) -> Result<
        fm_application::semantic_components::SemanticIndexRemovalReceipt,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn move_data(
        &self,
        _destination: std::path::PathBuf,
    ) -> Result<fm_application::semantic_components::SemanticDataMoveReceipt, SemanticComponentError>
    {
        Err(SemanticComponentError::Unavailable)
    }

    async fn uninstall_components(
        &self,
        _index_decision: fm_application::semantic_components::SemanticIndexRetentionDecision,
    ) -> Result<fm_application::semantic_components::SemanticUninstallReceipt, SemanticComponentError>
    {
        Err(SemanticComponentError::Unavailable)
    }

    async fn import_local_model(
        &self,
        _request: fm_application::semantic_components::SemanticLocalModelImportRequest,
        _profile: SemanticProfile,
        _estimate: fm_application::semantic_components::SemanticReindexEstimate,
    ) -> Result<
        fm_application::semantic_components::SemanticModelMigrationPlan,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn plan_model_migration(
        &self,
        _profile: SemanticProfile,
        _estimate: fm_application::semantic_components::SemanticReindexEstimate,
    ) -> Result<
        fm_application::semantic_components::SemanticModelMigrationPlan,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn confirm_model_migration(
        &self,
        _confirmation: fm_application::semantic_components::SemanticModelMigrationConfirmation,
    ) -> Result<
        fm_application::semantic_components::SemanticModelMigrationProgress,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn checkpoint_model_migration(
        &self,
        _checkpoint: fm_application::semantic_components::SemanticModelMigrationCheckpoint,
    ) -> Result<
        fm_application::semantic_components::SemanticModelMigrationProgress,
        SemanticComponentError,
    > {
        Err(SemanticComponentError::Unavailable)
    }

    async fn complete_model_migration(
        &self,
        _migration_id: fm_application::semantic_components::SemanticModelMigrationId,
    ) -> Result<SemanticModelSelection, SemanticComponentError> {
        Err(SemanticComponentError::Unavailable)
    }
}

/// Catalog metadata for the model a migration activates.
fn migrated_profiles() -> Vec<SemanticModelProfile> {
    let mut profiles = installed_profiles();
    profiles[0].resolved_model =
        SemanticModelIdentity::new("migrated-model", "migrated-revision-1");
    profiles[0].metadata.identity =
        SemanticModelIdentity::new("migrated-model", "migrated-revision-1");
    profiles
}

#[tokio::test]
async fn the_desktop_capability_stays_unavailable_until_components_report_a_model() {
    let directory = project_temp_dir("composition-absent-");
    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    )
    .with_semantic_component_capability(Arc::new(
        AdministratorProvisionedSemanticComponentCapability::new(
            SemanticComponentStatus::absent(Some(directory.path().join("semantic"))),
            installed_profiles(),
        ),
    ));

    assert_eq!(
        service
            .semantic_library_capabilities(&HOST)
            .await
            .authority(),
        SemanticLibraryAuthority::Unavailable
    );
    assert!(
        !service
            .semantic_library_status(&HOST)
            .await
            .unwrap()
            .available
    );
}

#[test]
fn overrides_are_persisted_and_reloaded_from_durable_policy() {
    let directory = project_temp_dir("overrides-");
    let service = desktop(&directory);
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    let revision = enrol(&service, &context);
    let root_id: RootId = service.status(&HOST).unwrap().roots[0].id.parse().unwrap();

    let overrides: BTreeMap<_, _> = [(
        EligibilityReason::Hidden,
        fm_semantic_library::EligibilityOverride::Include,
    )]
    .into();
    service
        .update_eligibility_overrides(&HOST, root_id, workspace(10), revision, overrides)
        .unwrap();

    let reloaded = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let status = reloaded.status(&HOST).unwrap();
    assert_eq!(status.roots[0].eligibility_overrides.len(), 1);
    assert_eq!(
        status.roots[0].eligibility_overrides[0].reason,
        EligibilityReason::Hidden
    );
}

#[tokio::test]
async fn the_reconciliation_facade_is_reachable_through_the_file_manager_service() {
    let directory = project_temp_dir("facade-reconciliation-");
    let service = FileManagerService::new(
        RuntimeKindDto::Mock,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    );
    let workspace_dto = service.start_workspace(None).await.unwrap();
    let pane = workspace_dto
        .panes
        .iter()
        .find(|pane| pane.id == workspace_dto.active_pane_id)
        .unwrap();
    let tab = pane
        .tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .unwrap();
    let preview = service
        .preview_semantic_enrolment(
            &HOST,
            PreviewSemanticEnrolmentRequestDto {
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
                recursive: true,
            },
        )
        .await
        .unwrap();
    let enrolled = service
        .confirm_semantic_enrolment(
            &HOST,
            ConfirmSemanticEnrolmentRequestDto {
                confirmation_id: preview.confirmation_id,
                policy_revision: preview.policy_revision,
                workspace_id: workspace_dto.id,
                location: tab.location.clone(),
            },
        )
        .await
        .unwrap();
    let root_id: RootId = enrolled.roots[0].id.parse().unwrap();

    // Availability, both directions.
    let unavailable = service
        .semantic_library_mark_root_unavailable(&HOST, root_id, RootUnavailabilityReason::Missing)
        .await
        .unwrap();
    assert!(matches!(
        unavailable.roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::TemporarilyUnavailable { .. }
    ));
    let available = service
        .semantic_library_observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                tab.location.clone().into(),
                Some(FilesystemIdentity::new("mock-volume", "mock-folder").unwrap()),
            )],
        )
        .await
        .unwrap();
    assert_eq!(available, RootMoveResolution::Unchanged);
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].availability,
        fm_application::semantic_library::SemanticRootAvailability::Available
    );

    // A move is only followed when stable identity proves it.
    let moved = location("file:///relocated-0179");
    let resolution = service
        .semantic_library_observe_root_identity(
            &HOST,
            root_id,
            &[ObservedRootIdentity::new(
                moved.clone(),
                Some(FilesystemIdentity::new("mock-volume", "mock-folder").unwrap()),
            )],
        )
        .await
        .unwrap();
    assert!(matches!(resolution, RootMoveResolution::ProvenMove { .. }));
    assert_eq!(
        service.semantic_library_status(&HOST).await.unwrap().roots[0].location,
        moved
    );

    // A successful complete reconciliation, and a provider-neutral feed plan
    // that runs the curated eligibility policy.
    let generation = service
        .semantic_library_complete_reconciliation(&HOST, root_id, &BTreeSet::new())
        .await
        .unwrap();
    assert_eq!(generation, 1);

    let plan = service
        .semantic_library_worker_feed_plan(
            &HOST,
            &[SemanticFeedCandidate {
                root_id,
                candidate: candidate(
                    "file:///relocated-0179/notes.md",
                    Some("text/markdown"),
                    false,
                ),
            }],
        )
        .await
        .unwrap();
    assert!(!plan.paused);
    assert_eq!(
        plan.eligible_locations,
        [location("file:///relocated-0179/notes.md")]
    );
}

/// Records every idempotency key and optionally fails one specific batch.
struct BatchRecordingExecutor {
    keys: Mutex<Vec<String>>,
    fail_on_call: Option<usize>,
}

impl BatchRecordingExecutor {
    fn new(fail_on_call: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            keys: Mutex::new(Vec::new()),
            fail_on_call,
        })
    }

    fn keys(&self) -> Vec<String> {
        self.keys.lock().unwrap().clone()
    }
}

impl SemanticCleanupExecutor for BatchRecordingExecutor {
    fn execute(&self, request: &SemanticCleanupRequest) -> Result<(), SemanticCleanupFailure> {
        let mut keys = self.keys.lock().unwrap();
        keys.push(request.idempotency_key().to_owned());
        if self.fail_on_call == Some(keys.len()) {
            return Err(SemanticCleanupFailure::new("injected batch failure"));
        }
        Ok(())
    }
}

/// Refuses the `nth` occurrence of one commit step, leaving everything before
/// it durable exactly as a crash mid-run would.
struct NthStepFailpoint {
    step: CommitStep,
    nth: usize,
    seen: Mutex<usize>,
}

impl NthStepFailpoint {
    fn new(step: CommitStep, nth: usize) -> Arc<Self> {
        Arc::new(Self {
            step,
            nth,
            seen: Mutex::new(0),
        })
    }
}

impl SemanticCommitObserver for NthStepFailpoint {
    fn before_step(&self, step: CommitStep) -> Result<(), SemanticCleanupFailure> {
        if step != self.step {
            return Ok(());
        }
        let mut seen = self.seen.lock().unwrap();
        *seen += 1;
        if *seen == self.nth {
            return Err(SemanticCleanupFailure::new("injected commit interruption"));
        }
        Ok(())
    }
}

/// Seeds a durable library whose excluded subtree holds `count` occurrences, so
/// one deletion category spans more than a single durable batch.
fn seed_library_with_many_occurrences(
    directory: &TempDir,
    count: usize,
) -> (RootId, SemanticFolderContext) {
    let mut policy = SemanticLibraryPolicy::new(
        library_identity(),
        ResourceProfile {
            kind: ResourceProfileKind::Balanced,
            budgets: ResourceBudgets::default(),
        },
    )
    .unwrap();
    let root_id = RootId::from_uuid(Uuid::from_u128(0x5eed));
    let mut root = EnrolledRoot::new(
        root_id,
        location("file:///docs"),
        Some(FilesystemIdentity::new("seed-volume", "seed-folder").unwrap()),
        true,
    );
    root.attach_workspace(workspace(10));
    policy.enrol_root(root).unwrap();

    let library_id = policy.library().id();
    let mut catalog = SemanticCatalog::new(library_id);
    let state = SemanticLibraryState::new(library_id);
    catalog.mark_root_available(root_id);
    let observations: Vec<CatalogObservation> = (0..count)
        .map(|index| {
            CatalogObservation::new(
                EntryId::from(Uuid::from_u128(1_000 + index as u128)),
                location(&format!("file:///docs/private/file-{index}.txt")),
                ContentFingerprint::new(format!("sha256:secret-{index}")).unwrap(),
                OccurrenceScope::new(workspace(10), root_id),
                DocumentArtifacts::default(),
                DocumentMeasurement::new(64, 32, 16),
            )
        })
        .collect();
    catalog
        .upsert_observations(&policy, &state, observations)
        .unwrap();

    let coordinator = SemanticLibraryCoordinator::new(
        directory.path().join("config"),
        directory.path().join("semantic-data"),
    );
    coordinator
        .lock()
        .unwrap()
        .transaction(
            LibraryOperation::Enrolment,
            Some(&policy),
            Some(&catalog),
            Some(&state),
        )
        .unwrap()
        .commit()
        .unwrap();
    (
        root_id,
        SemanticFolderContext::new(workspace(10), location("file:///docs/private")),
    )
}

fn exclude(
    service: &SemanticLibraryService,
    context: &SemanticFolderContext,
) -> Result<fm_application::semantic_library::SemanticLibraryStatus, SemanticLibraryError> {
    let revision = service.status(&HOST).unwrap().revision;
    let plan = service
        .plan_exclusion(&HOST, context.clone(), revision)
        .unwrap();
    service.confirm_exclusion(&HOST, &plan.confirmation_id, plan.policy_revision, context)
}

fn cleanup_plan_id(
    status: &fm_application::semantic_library::SemanticLibraryStatus,
) -> DeletionPlanId {
    status.roots[0].exclusions[0]
        .cleanup
        .plan_id
        .clone()
        .expect("a confirmed exclusion always has a durable plan")
        .parse()
        .unwrap()
}

fn occurrence_progress(
    status: &fm_application::semantic_library::SemanticLibraryStatus,
) -> (u64, u64, bool) {
    let category = status.roots[0].exclusions[0]
        .cleanup
        .categories
        .iter()
        .find(|category| category.category == SemanticDeletionCategory::Occurrences)
        .expect("occurrences is a mandatory category");
    (
        category.completed_items,
        category.total_items,
        category.complete,
    )
}

#[test]
fn cleanup_checkpoints_every_batch_and_retries_with_the_same_idempotency_key() {
    let directory = project_temp_dir("cleanup-batches-");
    let (_root_id, excluded) = seed_library_with_many_occurrences(&directory, 70);
    // The second batch of the first category fails, after the first batch has
    // already been checkpointed durably.
    let failing = BatchRecordingExecutor::new(Some(2));
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_cleanup_executor(failing.clone());

    let status = exclude(&service, &excluded).unwrap();
    let (completed, total, complete) = occurrence_progress(&status);

    assert_eq!(total, 70);
    assert_eq!(
        completed, 64,
        "the batch that succeeded must be checkpointed durably, not repeated wholesale"
    );
    assert!(!complete);
    assert_eq!(
        status.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Failed
    );
    let attempted = failing.keys();
    assert_eq!(attempted.len(), 2);
    assert_ne!(attempted[0], attempted[1]);

    // A restart re-reads the durable checkpoint, so the retry of the failed
    // batch presents exactly the same opaque key: an external payload deletion
    // that already ran before the failure can recognise and ignore it.
    let restarted_status = {
        let restarted = SemanticLibraryService::desktop_managed(
            configuration(&directory),
            Arc::new(UnavailableSemanticEnrolmentEstimator),
        )
        .unwrap();
        restarted.status(&HOST).unwrap()
    };
    assert_eq!(occurrence_progress(&restarted_status).0, 64);

    let retrying = BatchRecordingExecutor::new(None);
    let resumed = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_cleanup_executor(retrying.clone());
    let completed_status = resumed
        .resume_cleanup(
            &HOST,
            cleanup_plan_id(&restarted_status),
            restarted_status.revision,
        )
        .unwrap();

    let retried = retrying.keys();
    assert_eq!(
        retried.first().map(String::as_str),
        Some(attempted[1].as_str()),
        "a retry must present the same idempotency key as the batch that failed"
    );
    assert_eq!(
        completed_status.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Complete
    );
    let categories = &completed_status.roots[0].exclusions[0].cleanup.categories;
    assert_eq!(categories.len(), 6, "all six mandatory categories survive");
    assert!(categories.iter().all(|category| category.complete));
    assert_eq!(occurrence_progress(&completed_status), (70, 70, true));
}

#[test]
fn a_cleanup_interrupted_mid_run_stays_running_and_resumes_after_a_restart() {
    let directory = project_temp_dir("cleanup-interrupted-");
    let (_root_id, excluded) = seed_library_with_many_occurrences(&directory, 70);
    let executor = BatchRecordingExecutor::new(None);
    // The revocation commits, the first batch runs, and the process then dies
    // before that batch's checkpoint becomes durable.
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_cleanup_executor(executor.clone())
    .with_commit_observer(NthStepFailpoint::new(CommitStep::RecordIntent, 2));

    assert!(matches!(
        exclude(&service, &excluded),
        Err(SemanticLibraryError::Persistence)
    ));
    let interrupted_key = executor.keys();
    assert_eq!(interrupted_key.len(), 1);

    // The durable plan is running — not failed — and consent is already
    // revoked, so a restart must be able to finish it.
    let restarted = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let status = restarted.status(&HOST).unwrap();
    assert_eq!(
        status.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Running
    );
    assert_eq!(occurrence_progress(&status), (0, 70, false));
    assert_eq!(
        restarted.folder_status(&HOST, &excluded).unwrap().consent,
        SemanticFolderConsent::Excluded
    );

    let resuming = BatchRecordingExecutor::new(None);
    let resumed = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_cleanup_executor(resuming.clone());
    let completed = resumed
        .resume_cleanup(&HOST, cleanup_plan_id(&status), status.revision)
        .unwrap();

    assert_eq!(
        resuming.keys().first().map(String::as_str),
        Some(interrupted_key[0].as_str()),
        "an interrupted batch must be retried under its original key"
    );
    assert_eq!(
        completed.roots[0].exclusions[0].cleanup.status,
        SemanticCleanupState::Complete
    );
    assert_eq!(occurrence_progress(&completed), (70, 70, true));
}

#[test]
fn a_cached_service_sees_a_crashed_writers_committed_revocation_immediately() {
    let directory = project_temp_dir("pending-record-");
    let (_root_id, excluded) = seed_populated_library(&directory);
    // The reader caches a snapshot in which the subtree is still consented.
    let reader = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    assert_eq!(
        reader.folder_status(&HOST, &excluded).unwrap().consent,
        SemanticFolderConsent::InheritedFromParent
    );
    let cached_revision = reader.status(&HOST).unwrap().revision;

    // A second process revokes consent and dies after its intent became
    // durable but before the policy was installed, so the installed documents —
    // and therefore the durable revision — still describe the old consent.
    {
        let writer = SemanticLibraryService::desktop_managed(
            configuration(&directory),
            Arc::new(UnavailableSemanticEnrolmentEstimator),
        )
        .unwrap()
        .with_commit_observer(StepFailpoint::new(CommitStep::InstallPolicy));
        assert!(matches!(
            exclude(&writer, &excluded),
            Err(SemanticLibraryError::Persistence)
        ));
    }
    let durable_policy: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("config/semantic-library-policy.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        durable_policy["revision"],
        serde_json::json!(cached_revision),
        "the installed policy must still be the pre-commit one for this test to mean anything"
    );

    // The cached reader must not trust that unchanged revision: the committed
    // truth is in the journal, so it recovers first and reports the revocation.
    assert_eq!(
        reader.folder_status(&HOST, &excluded).unwrap().consent,
        SemanticFolderConsent::Excluded
    );
    assert!(
        reader.status(&HOST).unwrap().revision > cached_revision,
        "recovery installs the committed revision"
    );
    assert!(
        reader
            .worker_feed_plan(&HOST, &[])
            .unwrap()
            .decisions
            .is_empty(),
        "a revoked scope must never be fed after recovery"
    );
}

#[test]
fn durable_state_from_another_library_or_model_is_refused_without_touching_it() {
    let directory = project_temp_dir("identity-mismatch-");
    seed_populated_library(&directory);
    let policy_path = directory.path().join("config/semantic-library-policy.json");
    let before = std::fs::read(&policy_path).unwrap();

    // Same roots, different library id: a restored backup or a copied profile.
    let other_library = SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::new(
            directory.path().join("config"),
            directory.path().join("semantic-data"),
            DeviceLibraryIdentity::new(
                LibraryId::from_uuid(Uuid::from_u128(0xdead)),
                ModelIdentity::new("test-model", "test-revision-1", 384, "test-space").unwrap(),
            ),
            ResourceProfile {
                kind: ResourceProfileKind::Balanced,
                budgets: ResourceBudgets::default(),
            },
        ),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();

    assert!(matches!(
        other_library.status(&HOST),
        Err(SemanticLibraryError::IncompatibleLibraryIdentity)
    ));
    assert!(matches!(
        other_library.folder_status(
            &HOST,
            &SemanticFolderContext::new(workspace(10), location("file:///docs"))
        ),
        Err(SemanticLibraryError::IncompatibleLibraryIdentity)
    ));

    // Same library id, migrated embedding model: the records under the old
    // model are not this library's records.
    let other_model = SemanticLibraryService::desktop_managed(
        SemanticLibraryConfiguration::new(
            directory.path().join("config"),
            directory.path().join("semantic-data"),
            DeviceLibraryIdentity::new(
                library_identity().id(),
                ModelIdentity::new("test-model", "test-revision-2", 384, "test-space").unwrap(),
            ),
            ResourceProfile {
                kind: ResourceProfileKind::Balanced,
                budgets: ResourceBudgets::default(),
            },
        ),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();

    assert!(matches!(
        other_model.status(&HOST),
        Err(SemanticLibraryError::IncompatibleLibraryIdentity)
    ));
    assert!(matches!(
        other_model.pause(&HOST, 1),
        Err(SemanticLibraryError::IncompatibleLibraryIdentity)
    ));
    assert_eq!(
        std::fs::read(&policy_path).unwrap(),
        before,
        "a mismatched identity must never mutate the library it found"
    );

    // The correctly configured service still works on exactly the same roots.
    let matching = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    assert_eq!(matching.status(&HOST).unwrap().roots.len(), 1);
}

#[test]
fn eligibility_reason_counts_are_durable_across_a_restart() {
    let directory = project_temp_dir("reason-counts-");
    let disclosed = SemanticEnrolmentEstimate {
        skipped_reason_counts: EligibilityReasonCounts::from_decisions([
            EligibilityDecision::Skipped([EligibilityReason::Hidden].into()),
            EligibilityDecision::Skipped([EligibilityReason::Hidden].into()),
            EligibilityDecision::Skipped([EligibilityReason::UnsupportedMime].into()),
        ]),
        ..estimate()
    };
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(FixedSemanticEnrolmentEstimator::new(disclosed)),
    )
    .unwrap();
    let context = SemanticFolderContext::new(workspace(10), location("file:///docs"));
    enrol(&service, &context);

    let counts = &service.status(&HOST).unwrap().roots[0].eligibility_reason_counts;
    assert!(
        counts
            .iter()
            .any(|count| count.reason == EligibilityReason::Hidden && count.count > 0),
        "the disclosure the user consented to must be reported"
    );
    let before: Vec<_> = counts
        .iter()
        .map(|count| (count.reason, count.count))
        .collect();

    let restarted = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let after: Vec<_> = restarted.status(&HOST).unwrap().roots[0]
        .eligibility_reason_counts
        .iter()
        .map(|count| (count.reason, count.count))
        .collect();

    assert_eq!(
        after, before,
        "skip-reason counts are part of durable consent metadata, not an in-memory cache"
    );
}

#[test]
fn a_failed_workspace_detach_keeps_the_obligation_and_completes_later() {
    let directory = project_temp_dir("detach-retry-");
    let (_root_id, _excluded) = seed_populated_library(&directory);
    let failpoint = StepFailpoint::new(CommitStep::RecordIntent);
    let service = SemanticLibraryService::desktop_managed(
        configuration(&directory),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap()
    .with_commit_observer(failpoint.clone());

    assert!(matches!(
        service.detach_workspace(workspace(10)),
        Err(SemanticLibraryError::Persistence)
    ));
    assert!(
        service
            .pending_workspace_detachments()
            .contains(&workspace(10)),
        "a failed detach must stay owed instead of being silently dropped"
    );
    assert_eq!(
        service.status(&HOST).unwrap().roots[0].workspace_references,
        [Uuid::from(workspace(10))],
        "nothing may pretend the two stores committed atomically"
    );

    // The scopes are still there, but nothing can query them: every semantic
    // call validates the workspace, which the repository already deleted.
    failpoint.disarm();
    let status = service.status(&HOST).unwrap();

    assert!(
        status.roots[0].workspace_references.is_empty(),
        "the queued detachment must complete on the next locked operation"
    );
    assert!(service.pending_workspace_detachments().is_empty());
    assert_eq!(status.roots.len(), 1, "global root consent survives");
}

#[test]
fn the_worker_feed_stamps_the_tenant_of_the_authorized_caller() {
    let directory = project_temp_dir("feed-tenant-");
    let (root_id, _) = seed_populated_library(&directory);
    let service = SemanticLibraryService::server_single_private(
        configuration(&directory),
        SemanticServerIdentity::new("tenant-a", "admin-a").unwrap(),
        ServerEnrolmentPolicy::local_only(),
        HardQuotas::default(),
        Arc::new(UnavailableSemanticEnrolmentEstimator),
    )
    .unwrap();
    let administrator = SemanticAccessContext::server("tenant-a", "admin-a").unwrap();
    let other_tenant = SemanticAccessContext::server("tenant-b", "admin-a").unwrap();

    let plan = service.worker_feed_plan(&administrator, &[]).unwrap();

    assert!(!plan.decisions.is_empty());
    assert!(
        plan.decisions
            .iter()
            .all(|decision| decision.tenant_id().as_str() == "tenant-a"),
        "the tenant comes from the authorized caller, never from a separate argument"
    );
    assert!(matches!(
        service.worker_feed_plan(&other_tenant, &[]),
        Err(SemanticLibraryError::AccessDenied)
    ));
    assert!(matches!(
        service.worker_feed_plan(&SemanticAccessContext::Anonymous, &[]),
        Err(SemanticLibraryError::AccessDenied)
    ));
    assert_eq!(
        service
            .worker_feed_plan(&administrator, &[])
            .unwrap()
            .decisions
            .first()
            .map(fm_semantic_library::WorkerFeedDecision::library_id),
        Some(library_identity().id())
    );
    assert_eq!(
        root_id.to_string(),
        service.status(&administrator).unwrap().roots[0].id
    );
}
