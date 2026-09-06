//! Public application-layer tests for managed semantic component lifecycle.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ed25519_dalek::{Signer, SigningKey};
use fm_application::FileManagerService;
use fm_application::semantic_components::{
    AdministratorProvisionedSemanticComponentCapability, DesktopSemanticDistribution,
    FakeSemanticComponentCapability, FakeSemanticComponentScenario,
    ManagedSemanticComponentAdapters, ManagedSemanticComponentCapability,
    ManagedSemanticComponentConfiguration, RemoveSemanticIndexRequest, RuntimeExecutableDownload,
    SemanticCategoryDiskUse, SemanticComponentAuthority, SemanticComponentError,
    SemanticComponentLifecycle, SemanticComponentOperation, SemanticComponentService,
    SemanticComponentStatus, SemanticDataCategory, SemanticDiskUse, SemanticEmbeddingNormalization,
    SemanticIndexInventory, SemanticIndexRecordCounts, SemanticIndexRemovalError,
    SemanticIndexRemover, SemanticIndexRetentionDecision, SemanticLocalModelImportRequest,
    SemanticModelImportField, SemanticModelMigrationCheckpoint, SemanticModelProfile,
    SemanticReindexEstimate, SemanticWorkerPatchRequest,
};
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactCompatibility, ArtifactId,
    ArtifactKind, ArtifactLocation, ArtifactRequest, ArtifactSource, ArtifactSourceError,
    CatalogArtifact, CatalogManifest, ComponentId, ComponentManager, ComponentQuiescer,
    ComponentResources, EmbeddingNormalization, EnrolmentDeletionCounts, EnrolmentDeletionPlan,
    FreeSpaceError, FreeSpaceProbe, IndexingController, IndexingPauseGuard, InstallEnvironment,
    LicenseInfo, ManifestRevision, ModelId, ModelIdentity, ModelManifest, ModelMetadata,
    ModelRevision, PauseError, ProtocolRange, QuiesceError, RuntimeCompatibility,
    SemanticStateStore, Sha256Digest, SignedCatalogManifest, TargetTriple, TrustedCatalog,
};
use fm_transport_dto::RuntimeKindDto;
use tempfile::TempDir;

fn project_temp_dir(prefix: &str) -> TempDir {
    let parent =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/application-component-tests");
    std::fs::create_dir_all(&parent).expect("create project-local test directory");
    let parent = parent
        .canonicalize()
        .expect("canonicalize project-local test directory");
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(parent)
        .expect("create project-local temporary directory")
}

#[tokio::test]
async fn default_service_has_an_inert_semantic_component_capability() {
    let root = project_temp_dir("default-");
    let workspace_directory = root.path().join("workspaces");
    let settings_directory = root.path().join("settings");
    let semantic_directory = settings_directory.join("semantic");

    let service = FileManagerService::new(
        RuntimeKindDto::Tauri,
        &workspace_directory,
        &settings_directory,
    );
    let capabilities = service.semantic_component_capabilities().await;
    let status = service
        .semantic_component_status()
        .await
        .expect("unavailable status remains reportable");

    assert_eq!(
        capabilities.authority(),
        SemanticComponentAuthority::Unavailable
    );
    assert!(capabilities.operations().is_empty());
    assert_eq!(status.lifecycle(), &SemanticComponentLifecycle::Unavailable);
    assert!(!semantic_directory.exists());
}

struct FixtureArtifactSource {
    artifacts: BTreeMap<ArtifactId, Vec<u8>>,
    calls: AtomicUsize,
}

impl ArtifactSource for FixtureArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let bytes = self.artifacts.get(request.artifact_id()).ok_or_else(|| {
            ArtifactSourceError::Unavailable("unknown fixture artifact".to_owned())
        })?;
        let offset =
            usize::try_from(request.offset()).map_err(|_| ArtifactSourceError::InvalidOffset {
                offset: request.offset(),
            })?;
        Ok(ArtifactChunk::new(bytes[offset..].to_vec(), true))
    }
}

struct FixedFreeSpace(u64);

impl FreeSpaceProbe for FixedFreeSpace {
    fn available_bytes(&self, _path: &Path) -> Result<u64, FreeSpaceError> {
        Ok(self.0)
    }
}

struct AcceptActivation;

impl ActivationProbe for AcceptActivation {
    fn validate(
        &self,
        _artifact: &CatalogArtifact,
        _installed_path: &Path,
    ) -> Result<(), ActivationError> {
        Ok(())
    }
}

struct PauseGuard;

impl IndexingPauseGuard for PauseGuard {}

struct FixtureIndexing;

impl IndexingController for FixtureIndexing {
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError> {
        Ok(Box::new(PauseGuard))
    }
}

struct FixtureQuiescer;

impl ComponentQuiescer for FixtureQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        Ok(())
    }
}

struct FixtureIndexRemover;

impl SemanticIndexRemover for FixtureIndexRemover {
    fn remove(
        &self,
        plan: &EnrolmentDeletionPlan,
    ) -> Result<EnrolmentDeletionCounts, SemanticIndexRemovalError> {
        Ok(plan.expected())
    }
}

struct FixtureIndexInventory;

impl SemanticIndexInventory for FixtureIndexInventory {
    fn counts(
        &self,
        _enrolment_id: &fm_semantic_components::EnrolmentId,
    ) -> Result<EnrolmentDeletionCounts, SemanticIndexRemovalError> {
        Ok(EnrolmentDeletionCounts {
            index_records: 5,
            extracted_files: 4,
            zvec_vectors: 5,
            cache_entries: 3,
            conversation_evidence: 2,
        })
    }
}

fn signed_catalog() -> TrustedCatalog {
    let target = TargetTriple::new("macos", "aarch64").unwrap();
    let runtime_compatibility = RuntimeCompatibility::new(
        ComponentId::new("fixture.runtime").unwrap(),
        ">=1.4.0, <2.0.0".parse().unwrap(),
    );
    let worker = CatalogArtifact::new(
        ArtifactId::new("fixture-worker").unwrap(),
        ComponentId::new("fixture.worker").unwrap(),
        ArtifactKind::Worker,
        "1.2.0".parse().unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/worker").unwrap(),
        LicenseInfo::new("MIT", "Fixture worker").unwrap(),
        Sha256Digest::calculate(b"worker"),
        ComponentResources::new(6, 60, 70).unwrap(),
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 1).unwrap()),
            Vec::new(),
            1,
        ),
    )
    .unwrap();
    let worker_patch = CatalogArtifact::new(
        ArtifactId::new("fixture-worker-patch").unwrap(),
        ComponentId::new("fixture.worker").unwrap(),
        ArtifactKind::Worker,
        "1.2.1".parse().unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/worker-patch").unwrap(),
        LicenseInfo::new("MIT", "Fixture worker patch").unwrap(),
        Sha256Digest::calculate(b"patch"),
        ComponentResources::new(5, 61, 70).unwrap(),
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(1, 1).unwrap()),
            Vec::new(),
            1,
        ),
    )
    .unwrap();
    let runtime = CatalogArtifact::new(
        ArtifactId::new("fixture-runtime").unwrap(),
        ComponentId::new("fixture.runtime").unwrap(),
        ArtifactKind::Runtime,
        "1.4.0".parse().unwrap(),
        ArtifactLocation::new("https://fixtures.invalid/runtime").unwrap(),
        LicenseInfo::new("MIT", "Fixture runtime").unwrap(),
        Sha256Digest::calculate(b"runtime"),
        ComponentResources::new(7, 70, 80).unwrap(),
        ArtifactCompatibility::new(Some(target), None, Vec::new(), 1),
    )
    .unwrap();
    let models = [
        (
            fm_semantic_components::SemanticProfile::CompactMultilingual,
            "fixture.multilingual",
            "upstream-deadbeef",
            "fixture-model",
            b"model".as_slice(),
            ["en", "nl"].as_slice(),
        ),
        (
            fm_semantic_components::SemanticProfile::CompactEnglish,
            "fixture.english",
            "upstream-english",
            "fixture-model-english",
            b"english".as_slice(),
            ["en"].as_slice(),
        ),
        (
            fm_semantic_components::SemanticProfile::MultilingualQuality,
            "fixture.quality",
            "upstream-quality",
            "fixture-model-quality",
            b"quality-model".as_slice(),
            ["en", "nl", "ja"].as_slice(),
        ),
    ];
    let mut artifacts = vec![worker, worker_patch, runtime];
    let mut manifests = Vec::new();
    let mut profiles = BTreeMap::new();
    for (profile, model_id, revision, artifact_id, payload, languages) in models {
        let identity = ModelIdentity::new(
            ModelId::new(model_id).unwrap(),
            ModelRevision::new(revision).unwrap(),
        );
        let model_metadata = ModelMetadata::new(
            identity.clone(),
            LicenseInfo::new("Apache-2.0", "Fixture model").unwrap(),
            fm_semantic_components::TokenizerId::new(format!("{model_id}-tokenizer")).unwrap(),
            384,
            EmbeddingNormalization::UnitLength,
            runtime_compatibility.clone(),
            languages.iter().copied(),
            u64::try_from(payload.len()).unwrap(),
            400,
        )
        .unwrap();
        let model_artifact_id = ArtifactId::new(artifact_id).unwrap();
        artifacts.push(
            CatalogArtifact::new(
                model_artifact_id.clone(),
                ComponentId::new(model_id).unwrap(),
                ArtifactKind::Model(identity.clone()),
                "1.0.0".parse().unwrap(),
                ArtifactLocation::new(format!("https://fixtures.invalid/{artifact_id}")).unwrap(),
                model_metadata.license().clone(),
                Sha256Digest::calculate(payload),
                ComponentResources::new(
                    u64::try_from(payload.len()).unwrap(),
                    u64::try_from(payload.len()).unwrap(),
                    400,
                )
                .unwrap(),
                ArtifactCompatibility::new(None, None, vec![runtime_compatibility.clone()], 1),
            )
            .unwrap(),
        );
        manifests.push(ModelManifest::new(model_artifact_id, model_metadata));
        profiles.insert(profile, identity);
    }
    let manifest = CatalogManifest::new(
        ManifestRevision::new("fixture-catalog").unwrap(),
        artifacts,
        manifests,
        profiles,
    )
    .unwrap();
    let signing_key = SigningKey::from_bytes(&[17_u8; 32]);
    let signature = signing_key.sign(&manifest.canonical_bytes().unwrap());
    TrustedCatalog::verify(
        SignedCatalogManifest::new(manifest, signature.to_bytes()),
        &signing_key.verifying_key(),
    )
    .unwrap()
}

fn managed_capability(
    directory: &TempDir,
    distribution: DesktopSemanticDistribution,
    available_bytes: u64,
) -> (
    Arc<ManagedSemanticComponentCapability>,
    Arc<FixtureArtifactSource>,
) {
    managed_capability_with_inventory(directory, distribution, available_bytes, true)
}

fn managed_capability_with_inventory(
    directory: &TempDir,
    distribution: DesktopSemanticDistribution,
    available_bytes: u64,
    with_inventory: bool,
) -> (
    Arc<ManagedSemanticComponentCapability>,
    Arc<FixtureArtifactSource>,
) {
    let source = Arc::new(FixtureArtifactSource {
        artifacts: [
            (
                ArtifactId::new("fixture-worker").unwrap(),
                b"worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture-runtime").unwrap(),
                b"runtime".to_vec(),
            ),
            (ArtifactId::new("fixture-model").unwrap(), b"model".to_vec()),
            (
                ArtifactId::new("fixture-model-english").unwrap(),
                b"english".to_vec(),
            ),
            (
                ArtifactId::new("fixture-model-quality").unwrap(),
                b"quality-model".to_vec(),
            ),
            (
                ArtifactId::new("fixture-worker-patch").unwrap(),
                b"patch".to_vec(),
            ),
        ]
        .into(),
        calls: AtomicUsize::new(0),
    });
    let capability = ManagedSemanticComponentCapability::new(
        ComponentManager::new(
            SemanticStateStore::new(directory.path().join("config")),
            directory.path().join("app-data"),
        ),
        Arc::new(signed_catalog()),
        ManagedSemanticComponentConfiguration {
            runtime_and_worker_artifacts: vec![
                ArtifactId::new("fixture-worker").unwrap(),
                ArtifactId::new("fixture-runtime").unwrap(),
            ],
            environment: InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            distribution,
            minimum_free_space_reserve_bytes: 100,
        },
        ManagedSemanticComponentAdapters {
            artifact_source: source.clone(),
            free_space: Arc::new(FixedFreeSpace(available_bytes)),
            activation: Arc::new(AcceptActivation),
            indexing: Arc::new(FixtureIndexing),
            quiescer: Arc::new(FixtureQuiescer),
            index_inventory: with_inventory
                .then(|| Arc::new(FixtureIndexInventory) as Arc<dyn SemanticIndexInventory>),
            index_remover: with_inventory
                .then(|| Arc::new(FixtureIndexRemover) as Arc<dyn SemanticIndexRemover>),
        },
    );
    (Arc::new(capability), source)
}

#[tokio::test]
async fn managed_offer_selects_only_the_model_resolved_for_the_requested_profile() {
    let directory = project_temp_dir("profile-offers-");
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);

    for (profile, expected_artifact) in [
        (
            fm_semantic_components::SemanticProfile::CompactMultilingual,
            "fixture-model",
        ),
        (
            fm_semantic_components::SemanticProfile::CompactEnglish,
            "fixture-model-english",
        ),
        (
            fm_semantic_components::SemanticProfile::MultilingualQuality,
            "fixture-model-quality",
        ),
    ] {
        let offer = service.installation_offer(profile).await.unwrap();
        let offered_models: Vec<_> = offer
            .components
            .iter()
            .filter(|component| {
                component.kind == fm_application::semantic_components::SemanticComponentKind::Model
            })
            .map(|component| component.artifact_id.as_str())
            .collect();
        assert_eq!(offered_models, [expected_artifact]);
    }
}

#[tokio::test]
async fn managed_offer_maps_signed_catalog_and_low_disk_is_actionable() {
    let directory = project_temp_dir("managed-offer-");
    let (capability, source) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, 99);
    let service = SemanticComponentService::new(capability);

    let capabilities = service.capabilities().await;
    assert_eq!(
        capabilities.authority(),
        SemanticComponentAuthority::DesktopManaged
    );
    assert_eq!(
        capabilities.runtime_executable_download(),
        RuntimeExecutableDownload::DirectDistribution
    );
    assert!(capabilities.supports(SemanticComponentOperation::CheckpointModelMigration));
    assert!(capabilities.supports(SemanticComponentOperation::CompleteModelMigration));
    let profiles = service.catalog_profiles().await.unwrap();
    assert_eq!(profiles.len(), 3);
    assert!(profiles.iter().any(|profile| profile.recommended));
    assert_eq!(
        profiles
            .iter()
            .map(|profile| profile.resolved_model.revision())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );

    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    assert_eq!(offer.catalog_revision, "fixture-catalog");
    assert_eq!(offer.components.len(), 3);
    assert_eq!(
        offer
            .components
            .iter()
            .map(|component| component.component_id.as_str())
            .collect::<Vec<_>>(),
        ["fixture.worker", "fixture.runtime", "fixture.multilingual"]
    );
    assert_eq!(offer.data_root, directory.path().join("app-data/semantic"));
    assert_eq!(offer.minimum_free_space_reserve_bytes, 100);

    let error = service
        .install_or_enable(offer.consent())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        SemanticComponentError::InsufficientSpace {
            available_bytes: 99,
            required_bytes: 235,
            reserve_bytes: 100,
        }
    );
    assert_eq!(source.calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::LowDisk {
            available_bytes: 99,
            required_bytes: 235,
        }
    );
}

#[tokio::test]
async fn managed_curated_model_migration_stages_target_before_activation() {
    let directory = project_temp_dir("managed-curated-migration-");
    let (capability, source) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();

    let plan = service
        .plan_model_migration(
            fm_semantic_components::SemanticProfile::MultilingualQuality,
            SemanticReindexEstimate::new(0, 0),
        )
        .await
        .unwrap();
    let migration_id = plan.id().clone();
    service
        .confirm_model_migration(plan.confirm())
        .await
        .unwrap();

    let migrating = service.status().await.unwrap();
    assert_eq!(
        migrating.active_model().unwrap().identity().revision(),
        "upstream-deadbeef"
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 4);

    service
        .checkpoint_model_migration(SemanticModelMigrationCheckpoint {
            migration_id: migration_id.clone(),
            completed_documents: 0,
            resume_cursor: None,
        })
        .await
        .unwrap();
    let selected = service
        .complete_model_migration(migration_id)
        .await
        .unwrap();
    assert_eq!(selected.identity().revision(), "upstream-quality");
}

#[tokio::test]
async fn mac_app_store_distribution_blocks_executable_component_offers() {
    let directory = project_temp_dir("app-store-");
    let (capability, source) = managed_capability(
        &directory,
        DesktopSemanticDistribution::MacAppStore,
        u64::MAX,
    );
    let service = SemanticComponentService::new(capability);

    assert_eq!(
        service.capabilities().await.runtime_executable_download(),
        RuntimeExecutableDownload::ProhibitedByMacAppStore
    );
    assert_eq!(
        service
            .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
            .await
            .unwrap_err(),
        SemanticComponentError::ExecutableDownloadProhibited
    );
    assert_eq!(source.calls.load(Ordering::Relaxed), 0);
}

fn local_model_request(source_path: PathBuf) -> SemanticLocalModelImportRequest {
    SemanticLocalModelImportRequest {
        source_path,
        model_id: "expert.local".to_owned(),
        upstream_revision: "local-revision-a".to_owned(),
        license_spdx: "Apache-2.0".to_owned(),
        license_notice: "Local model fixture".to_owned(),
        tokenizer: "local-tokenizer".to_owned(),
        dimensions: 768,
        normalization: Some(SemanticEmbeddingNormalization::UnitLength),
        runtime_component_id: "fixture.runtime".to_owned(),
        runtime_version_requirement: ">=1.4.0, <2.0.0".to_owned(),
        language_coverage: vec!["en".to_owned(), "nl".to_owned()],
        estimated_disk_bytes: 600,
        estimated_ram_bytes: 900,
    }
}

#[tokio::test]
async fn managed_local_model_migration_does_not_activate_an_unstaged_package() {
    let directory = project_temp_dir("managed-migration-");
    let model_path = directory.path().join("local-model.bin");
    std::fs::write(&model_path, b"local model").unwrap();
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();

    let mut invalid = local_model_request(model_path.clone());
    invalid.tokenizer.clear();
    assert_eq!(
        service
            .import_local_model(
                invalid,
                fm_semantic_components::SemanticProfile::CompactEnglish,
                SemanticReindexEstimate::new(3, 30),
            )
            .await
            .unwrap_err(),
        SemanticComponentError::InvalidLocalModelMetadata {
            field: SemanticModelImportField::Tokenizer,
        }
    );

    let plan = service
        .import_local_model(
            local_model_request(model_path),
            fm_semantic_components::SemanticProfile::CompactEnglish,
            SemanticReindexEstimate::new(3, 30),
        )
        .await
        .unwrap();
    assert!(plan.requires_confirmation());
    assert!(plan.is_full_reindex());
    assert!(plan.is_resumable());
    assert_eq!(plan.target.identity().model_id(), "expert.local");
    assert_eq!(plan.target.identity().revision(), "local-revision-a");
    assert_eq!(
        service
            .status()
            .await
            .unwrap()
            .active_model()
            .unwrap()
            .identity()
            .revision(),
        "upstream-deadbeef"
    );

    let migration_id = plan.id().clone();
    let progress = service
        .confirm_model_migration(plan.confirm())
        .await
        .unwrap();
    assert_eq!(progress.completed_documents(), 0);
    service
        .checkpoint_model_migration(SemanticModelMigrationCheckpoint {
            migration_id: migration_id.clone(),
            completed_documents: 2,
            resume_cursor: Some("document-2".to_owned()),
        })
        .await
        .unwrap();
    assert!(
        service
            .complete_model_migration(migration_id.clone())
            .await
            .is_err()
    );
    assert!(matches!(
        service.status().await.unwrap().lifecycle(),
        SemanticComponentLifecycle::Migrating { .. }
    ));
    drop(service);

    let (resumed_capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(resumed_capability);
    let resumed = service.status().await.unwrap();
    assert_eq!(resumed.migration().unwrap().migration_id(), &migration_id);
    assert_eq!(
        resumed.migration().unwrap().resume_cursor(),
        Some("document-2")
    );
    assert_eq!(
        resumed.migration().unwrap().target().identity().revision(),
        "local-revision-a"
    );
    service
        .checkpoint_model_migration(SemanticModelMigrationCheckpoint {
            migration_id: migration_id.clone(),
            completed_documents: 3,
            resume_cursor: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        service.complete_model_migration(migration_id).await,
        Err(SemanticComponentError::State { .. })
    ));
    assert_eq!(resumed.migration().unwrap().estimate().source_bytes(), 30);
    assert_eq!(
        resumed.active_model().unwrap().identity().revision(),
        "upstream-deadbeef"
    );
}

#[tokio::test]
async fn managed_status_keeps_disk_categories_stable_after_interleaved_writes() {
    let directory = project_temp_dir("managed-status-");
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();
    let root = directory.path().join("app-data/semantic");
    for (directory_name, file_name, bytes) in [
        ("workers", "later.bin", b"workers".as_slice()),
        ("catalog", "first.bin", b"catalog".as_slice()),
        ("zvec", "middle.bin", b"zvec".as_slice()),
        ("extracted", "second.bin", b"extracted".as_slice()),
        ("models", "model.bin", b"models".as_slice()),
        ("embedding-cache", "cache.bin", b"cache".as_slice()),
    ] {
        std::fs::write(root.join(directory_name).join(file_name), bytes).unwrap();
    }

    let status = service.status().await.unwrap();
    assert_eq!(
        status
            .disk_use()
            .categories()
            .iter()
            .map(|usage| usage.category())
            .collect::<Vec<_>>(),
        SemanticDataCategory::all()
    );
    assert_eq!(
        status.disk_use().total_bytes(),
        status
            .disk_use()
            .categories()
            .iter()
            .map(|usage| usage.bytes())
            .sum::<u64>()
    );
    assert_eq!(status.components().len(), 3);
}

#[tokio::test]
async fn managed_lifecycle_uses_distinct_pause_remove_move_and_uninstall_operations() {
    let directory = project_temp_dir("managed-lifecycle-");
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    let consent = offer.consent();
    let duplicate = consent.clone();
    service.install_or_enable(consent).await.unwrap();
    assert_eq!(
        service.install_or_enable(duplicate).await.unwrap_err(),
        SemanticComponentError::ConsentRequired
    );

    service.pause_indexing().await.unwrap();
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::Paused
    );
    service.resume_indexing().await.unwrap();
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::InstalledEnabled
    );

    let expected = SemanticIndexRecordCounts {
        index_records: 5,
        extracted_files: 4,
        zvec_vectors: 5,
        cache_entries: 3,
        conversation_evidence: 2,
    };
    let removal_plan = service
        .plan_index_removal("managed-library".to_owned())
        .await
        .unwrap();
    assert!(!removal_plan.id().as_str().is_empty());
    assert_eq!(removal_plan.expected, expected);
    assert!(removal_plan.expected.conversation_evidence > 0);
    let confirmation = removal_plan.confirm();
    let replay = confirmation.clone();
    let removed = service.confirm_index_removal(confirmation).await.unwrap();
    assert_eq!(removed.deleted, expected);
    assert!(removed.conversation_evidence_deleted);
    assert!(service.confirm_index_removal(replay).await.is_err());

    let destination = directory.path().join("relocated-semantic");
    let moved = service.move_data(destination.clone()).await.unwrap();
    assert_eq!(moved.destination, destination);
    assert_eq!(
        service.status().await.unwrap().data_root(),
        Some(moved.destination.as_path())
    );

    let receipt = service
        .uninstall_components(SemanticIndexRetentionDecision::Retain)
        .await
        .unwrap();
    assert_eq!(
        receipt.index_decision,
        SemanticIndexRetentionDecision::Retain
    );
    assert_eq!(receipt.removed_component_count, 3);
}

#[tokio::test]
async fn managed_index_removal_rejects_caller_counts_that_disagree_with_inventory() {
    let directory = project_temp_dir("managed-removal-counts-");
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);

    let result = service
        .remove_index(RemoveSemanticIndexRequest {
            enrolment_id: "managed-library".to_owned(),
            expected: SemanticIndexRecordCounts {
                index_records: 0,
                extracted_files: 0,
                zvec_vectors: 0,
                cache_entries: 0,
                conversation_evidence: 0,
            },
        })
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn managed_index_removal_is_unavailable_without_authoritative_inventory() {
    let directory = project_temp_dir("managed-removal-unavailable-");
    let (capability, _) = managed_capability_with_inventory(
        &directory,
        DesktopSemanticDistribution::Direct,
        u64::MAX,
        false,
    );
    let service = SemanticComponentService::new(capability);

    assert!(
        !service
            .capabilities()
            .await
            .supports(SemanticComponentOperation::RemoveIndex)
    );
    assert!(matches!(
        service
            .remove_index(RemoveSemanticIndexRequest {
                enrolment_id: "managed-library".to_owned(),
                expected: SemanticIndexRecordCounts {
                    index_records: 0,
                    extracted_files: 0,
                    zvec_vectors: 0,
                    cache_entries: 0,
                    conversation_evidence: 0,
                },
            })
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::DesktopManaged,
            operation: SemanticComponentOperation::RemoveIndex,
        })
    ));
}

#[tokio::test]
async fn managed_uninstall_quiesces_and_clears_an_existing_pause_guard() {
    let directory = project_temp_dir("managed-uninstall-paused-");
    let (capability, _) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();
    service.pause_indexing().await.unwrap();

    service
        .uninstall_components(SemanticIndexRetentionDecision::Retain)
        .await
        .unwrap();

    assert!(matches!(
        service.resume_indexing().await,
        Err(SemanticComponentError::InvalidLifecycle {
            operation: SemanticComponentOperation::ResumeIndexing,
            ..
        })
    ));
}

#[tokio::test]
async fn deterministic_fake_exposes_every_lifecycle_state() {
    let fake = Arc::new(FakeSemanticComponentCapability::new());
    let service = SemanticComponentService::new(fake.clone());
    let capabilities = service.capabilities().await;

    assert_eq!(
        capabilities.authority(),
        SemanticComponentAuthority::DeterministicMock
    );
    assert_eq!(
        capabilities.runtime_executable_download(),
        RuntimeExecutableDownload::Simulated
    );

    for scenario in FakeSemanticComponentScenario::all() {
        fake.set_scenario(*scenario);
        assert_eq!(
            service.status().await.unwrap().lifecycle(),
            &scenario.lifecycle()
        );
    }
}

#[tokio::test]
async fn fake_lifecycle_actions_are_distinct_and_install_requires_live_consent() {
    let fake = Arc::new(FakeSemanticComponentCapability::new());
    let service = SemanticComponentService::new(fake);
    let capabilities = service.capabilities().await;

    for operation in [
        SemanticComponentOperation::CreateInstallationOffer,
        SemanticComponentOperation::InstallOrEnable,
        SemanticComponentOperation::PauseIndexing,
        SemanticComponentOperation::ResumeIndexing,
        SemanticComponentOperation::RemoveIndex,
        SemanticComponentOperation::MoveData,
        SemanticComponentOperation::UninstallComponents,
    ] {
        assert!(capabilities.supports(operation));
    }

    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    assert_eq!(offer.components.len(), 3);
    assert!(
        offer
            .components
            .iter()
            .all(|component| component.download_bytes > 0
                && component.estimated_installed_bytes > 0
                && component.estimated_ram_bytes > 0
                && !component.version.is_empty()
                && !component.license.spdx.is_empty())
    );
    assert!(!offer.catalog_revision.is_empty());
    assert!(!offer.resolved_model.revision().is_empty());
    assert!(offer.embeddings_stay_local);
    assert!(offer.local_only_disclosure.contains("stay on this device"));
    assert!(offer.minimum_free_space_reserve_bytes > 0);
    let resolved_model = offer.resolved_model.clone();
    let consent = offer.consent();
    let duplicate = consent.clone();

    service.install_or_enable(consent).await.unwrap();
    let installed = service.status().await.unwrap();
    assert_eq!(
        installed.lifecycle(),
        &SemanticComponentLifecycle::InstalledEnabled
    );
    assert_eq!(
        installed.active_model().unwrap().identity(),
        &resolved_model
    );
    assert_eq!(installed.components().len(), 3);
    assert!(installed.disk_use().total_bytes() > 0);
    assert_eq!(
        service.install_or_enable(duplicate).await.unwrap_err(),
        SemanticComponentError::ConsentRequired
    );

    service.pause_indexing().await.unwrap();
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::Paused
    );
    service.resume_indexing().await.unwrap();

    let expected = SemanticIndexRecordCounts {
        index_records: 3,
        extracted_files: 2,
        zvec_vectors: 3,
        cache_entries: 1,
        conversation_evidence: 2,
    };
    let removed = service
        .remove_index(RemoveSemanticIndexRequest {
            enrolment_id: "library-a".to_owned(),
            expected,
        })
        .await
        .unwrap();
    assert_eq!(removed.deleted, expected);
    assert!(removed.conversation_evidence_deleted);

    let moved = service
        .move_data(PathBuf::from("mock/new-semantic-root"))
        .await
        .unwrap();
    assert_eq!(moved.destination, PathBuf::from("mock/new-semantic-root"));

    let uninstalled = service
        .uninstall_components(SemanticIndexRetentionDecision::Delete)
        .await
        .unwrap();
    assert_eq!(
        uninstalled.index_decision,
        SemanticIndexRetentionDecision::Delete
    );
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::Uninstalled {
            index_decision: SemanticIndexRetentionDecision::Delete,
        }
    );
}

#[tokio::test]
async fn administrator_provisioned_capability_reports_status_but_rejects_mutations() {
    let reported = SemanticComponentStatus::absent(Some(PathBuf::from("/srv/procyon/semantic")));
    let capability = Arc::new(AdministratorProvisionedSemanticComponentCapability::new(
        reported.clone(),
        Vec::<SemanticModelProfile>::new(),
    ));
    let service = SemanticComponentService::new(capability);

    let capabilities = service.capabilities().await;
    assert_eq!(
        capabilities.authority(),
        SemanticComponentAuthority::AdministratorProvisioned
    );
    assert_eq!(
        capabilities.operations(),
        &[
            SemanticComponentOperation::ViewStatus,
            SemanticComponentOperation::ViewCatalog,
        ]
    );
    assert_eq!(service.status().await.unwrap(), reported);

    let move_error = service
        .move_data(PathBuf::from("/browser/chosen/path"))
        .await
        .unwrap_err();
    assert_eq!(
        move_error,
        SemanticComponentError::AuthorityDenied {
            authority: SemanticComponentAuthority::AdministratorProvisioned,
            operation: SemanticComponentOperation::MoveData,
        }
    );
    assert!(matches!(
        service
            .uninstall_components(SemanticIndexRetentionDecision::Retain)
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::UninstallComponents,
            ..
        })
    ));
    assert!(matches!(
        service
            .import_local_model(
                SemanticLocalModelImportRequest {
                    source_path: PathBuf::new(),
                    model_id: String::new(),
                    upstream_revision: String::new(),
                    license_spdx: String::new(),
                    license_notice: String::new(),
                    tokenizer: String::new(),
                    dimensions: 0,
                    normalization: None,
                    runtime_component_id: String::new(),
                    runtime_version_requirement: String::new(),
                    language_coverage: Vec::new(),
                    estimated_disk_bytes: 0,
                    estimated_ram_bytes: 0,
                },
                fm_semantic_components::SemanticProfile::CompactEnglish,
                SemanticReindexEstimate::new(10, 100),
            )
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::ImportLocalModel,
            ..
        })
    ));
    assert!(matches!(
        service
            .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::CreateInstallationOffer,
            ..
        })
    ));
}

#[tokio::test]
async fn browser_service_cannot_gain_install_or_path_authority_from_an_injected_fake() {
    let directory = project_temp_dir("browser-authority-");
    let fake = Arc::new(FakeSemanticComponentCapability::new());
    let donor = SemanticComponentService::new(fake.clone());
    let consent = donor
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap()
        .consent();
    let service = FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        directory.path().join("workspaces"),
        directory.path().join("settings"),
    )
    .with_semantic_component_capability(fake);

    let capabilities = service.semantic_component_capabilities().await;
    assert_eq!(
        capabilities.authority(),
        SemanticComponentAuthority::AdministratorProvisioned
    );
    assert_eq!(
        capabilities.operations(),
        &[
            SemanticComponentOperation::ViewStatus,
            SemanticComponentOperation::ViewCatalog,
        ]
    );
    assert!(service.semantic_component_status().await.is_ok());

    assert!(matches!(
        service.semantic_component_install_or_enable(consent).await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::InstallOrEnable,
            ..
        })
    ));
    assert!(matches!(
        service
            .semantic_component_move_data(PathBuf::from("/browser/path"))
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::MoveData,
            ..
        })
    ));
    assert!(matches!(
        service
            .semantic_component_uninstall(SemanticIndexRetentionDecision::Delete)
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::UninstallComponents,
            ..
        })
    ));
    assert!(matches!(
        service
            .semantic_component_import_local_model(
                local_model_request(directory.path().join("never-read.bin")),
                fm_semantic_components::SemanticProfile::CompactEnglish,
                SemanticReindexEstimate::new(1, 1),
            )
            .await,
        Err(SemanticComponentError::AuthorityDenied {
            operation: SemanticComponentOperation::ImportLocalModel,
            ..
        })
    ));
    assert!(!directory.path().join("settings/semantic").exists());
}

#[tokio::test]
async fn administrator_status_normalizes_interleaved_disk_categories() {
    let disk_use = SemanticDiskUse::from_categories([
        SemanticCategoryDiskUse::new(SemanticDataCategory::Workers, 6),
        SemanticCategoryDiskUse::new(SemanticDataCategory::Catalog, 1),
        SemanticCategoryDiskUse::new(SemanticDataCategory::Models, 5),
        SemanticCategoryDiskUse::new(SemanticDataCategory::Extracted, 2),
        SemanticCategoryDiskUse::new(SemanticDataCategory::EmbeddingCache, 4),
        SemanticCategoryDiskUse::new(SemanticDataCategory::Zvec, 3),
    ])
    .unwrap();
    let reported = SemanticComponentStatus::new(
        SemanticComponentLifecycle::InstalledEnabled,
        Some(PathBuf::from("/srv/procyon/semantic")),
        None,
        None,
        Vec::new(),
        disk_use,
    );
    let service = SemanticComponentService::new(Arc::new(
        AdministratorProvisionedSemanticComponentCapability::new(reported, Vec::new()),
    ));

    let status = service.status().await.unwrap();
    assert_eq!(
        status
            .disk_use()
            .categories()
            .iter()
            .map(|usage| usage.category())
            .collect::<Vec<_>>(),
        SemanticDataCategory::all()
    );
    assert_eq!(status.disk_use().total_bytes(), 21);
}

struct RejectPatchActivation {
    reject_patch: AtomicBool,
}

impl ActivationProbe for RejectPatchActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        _installed_path: &Path,
    ) -> Result<(), ActivationError> {
        if artifact.id().as_str() == "fixture-worker-patch"
            && self.reject_patch.load(Ordering::Relaxed)
        {
            Err(ActivationError::new("fixture patch failed to start"))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn managed_worker_patch_failure_reports_rollback_to_the_working_version() {
    let directory = project_temp_dir("managed-patch-");
    let source = Arc::new(FixtureArtifactSource {
        artifacts: [
            (
                ArtifactId::new("fixture-worker").unwrap(),
                b"worker".to_vec(),
            ),
            (
                ArtifactId::new("fixture-runtime").unwrap(),
                b"runtime".to_vec(),
            ),
            (ArtifactId::new("fixture-model").unwrap(), b"model".to_vec()),
            (
                ArtifactId::new("fixture-worker-patch").unwrap(),
                b"patch".to_vec(),
            ),
        ]
        .into(),
        calls: AtomicUsize::new(0),
    });
    let activation = Arc::new(RejectPatchActivation {
        reject_patch: AtomicBool::new(false),
    });
    let capability = Arc::new(ManagedSemanticComponentCapability::new(
        ComponentManager::new(
            SemanticStateStore::new(directory.path().join("config")),
            directory.path().join("app-data"),
        ),
        Arc::new(signed_catalog()),
        ManagedSemanticComponentConfiguration {
            runtime_and_worker_artifacts: vec![
                ArtifactId::new("fixture-worker").unwrap(),
                ArtifactId::new("fixture-runtime").unwrap(),
            ],
            environment: InstallEnvironment::new(
                TargetTriple::new("macos", "aarch64").unwrap(),
                1,
                BTreeMap::new(),
            ),
            distribution: DesktopSemanticDistribution::Direct,
            minimum_free_space_reserve_bytes: 100,
        },
        ManagedSemanticComponentAdapters {
            artifact_source: source,
            free_space: Arc::new(FixedFreeSpace(u64::MAX)),
            activation: activation.clone(),
            indexing: Arc::new(FixtureIndexing),
            quiescer: Arc::new(FixtureQuiescer),
            index_inventory: Some(Arc::new(FixtureIndexInventory)),
            index_remover: Some(Arc::new(FixtureIndexRemover)),
        },
    ));
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();
    activation.reject_patch.store(true, Ordering::Relaxed);

    assert!(matches!(
        service
            .install_compatible_worker_patch(SemanticWorkerPatchRequest {
                component_id: "fixture.worker".to_owned(),
            })
            .await,
        Err(SemanticComponentError::ActivationFailed { .. })
    ));
    assert_eq!(
        service.status().await.unwrap().lifecycle(),
        &SemanticComponentLifecycle::UpdateFailedRolledBack {
            failed_version: "1.2.1".to_owned(),
            active_version: "1.2.0".to_owned(),
        }
    );
    assert!(
        service
            .status()
            .await
            .unwrap()
            .components()
            .iter()
            .any(|component| component.version() == "1.2.0")
    );
}

#[tokio::test]
async fn managed_worker_patch_does_not_auto_apply_without_a_durable_active_schema() {
    let directory = project_temp_dir("managed-patch-no-schema-");
    let (capability, source) =
        managed_capability(&directory, DesktopSemanticDistribution::Direct, u64::MAX);
    let service = SemanticComponentService::new(capability);
    let offer = service
        .installation_offer(fm_semantic_components::SemanticProfile::CompactMultilingual)
        .await
        .unwrap();
    service.install_or_enable(offer.consent()).await.unwrap();
    let calls_after_install = source.calls.load(Ordering::Relaxed);

    let state_path = directory.path().join("config/semantic-components.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    state
        .as_object_mut()
        .unwrap()
        .remove("active_index_schema_version");
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let result = service
        .install_compatible_worker_patch(SemanticWorkerPatchRequest {
            component_id: "fixture.worker".to_owned(),
        })
        .await
        .unwrap();

    assert!(result.is_none());
    assert_eq!(source.calls.load(Ordering::Relaxed), calls_after_install);
}
