use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ed25519_dalek::SigningKey;
use fm_application::semantic_components::{
    DesktopSemanticDistribution, ManagedSemanticComponentAdapters,
    ManagedSemanticComponentCapability, ManagedSemanticComponentConfiguration,
};
use fm_application::semantic_ocr::{
    DoclingOcrExecutableProbe, OcrExecutableProbe, OcrPolicyStore, SemanticOcrService,
};
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactKind, ArtifactRequest, ArtifactSource,
    ArtifactSourceError, CatalogArtifact, CatalogManifest, ComponentManager, ComponentQuiescer,
    DataCategory, FreeSpaceError, FreeSpaceProbe, IndexingController, IndexingPauseGuard,
    InstallEnvironment, ModelPack, PauseError, QuiesceError, SemanticDataRoot, SemanticState,
    SemanticStateStore, SignedCatalogManifest, TargetTriple, TrustedCatalog,
};
#[cfg(feature = "semantic-gemma")]
use fm_semantic_worker::gemma_native::GemmaNativeFiles;

const DEVELOPMENT_SIGNING_KEY: [u8; 32] = [0x19; 32];
/// Largest model pack member re-hashed during activation.
const SMALL_MEMBER_VERIFICATION_BYTES: u64 = 32 * 1024 * 1024;
/// Bytes returned per development artifact read.
const ARTIFACT_CHUNK_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) struct DeveloperSemanticBundle {
    pub(crate) components: Arc<ManagedSemanticComponentCapability>,
    pub(crate) installed_worker: PathBuf,
    pub(crate) runtime_directory: PathBuf,
    pub(crate) worker_data_directory: PathBuf,
    /// Durable marker used to resume an interrupted model-change reindex.
    pub(crate) reindex_pending_marker: PathBuf,
    pub(crate) native_library_directory: PathBuf,
    /// Resolves the currently activated model pack when the worker is launched.
    pub(crate) active_model_pack: fm_semantic_worker::DeveloperModelPackResolver,
    pub(crate) original_model: Option<fm_semantic_worker::DeveloperManagedWorkerResolver>,
    pub(crate) ocr: SemanticOcrService,
    pub(crate) ocr_executable: fm_semantic_worker::DeveloperOcrExecutableResolver,
}

impl DeveloperSemanticBundle {
    pub(crate) fn load(
        bundle_directory: &Path,
        configuration_directory: &Path,
        app_data_directory: &Path,
    ) -> Result<Self, DeveloperBundleError> {
        let bundle_directory = bundle_directory.canonicalize().map_err(|source| {
            DeveloperBundleError::BundleDirectory {
                path: bundle_directory.to_owned(),
                source,
            }
        })?;
        let manifest: CatalogManifest =
            serde_json::from_slice(&fs::read(bundle_directory.join("catalog.json"))?)
                .map_err(DeveloperBundleError::CatalogJson)?;
        let signature: [u8; 64] = fs::read(bundle_directory.join("catalog.sig"))?
            .try_into()
            .map_err(|_| DeveloperBundleError::InvalidSignatureFile)?;
        let signing_key = SigningKey::from_bytes(&DEVELOPMENT_SIGNING_KEY);
        let catalog = Arc::new(
            TrustedCatalog::verify(
                SignedCatalogManifest::new(manifest, signature),
                &signing_key.verifying_key(),
            )
            .map_err(DeveloperBundleError::Catalog)?,
        );

        let workers = catalog
            .artifacts()
            .iter()
            .filter(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .collect::<Vec<_>>();
        if workers.len() != 1 {
            return Err(DeveloperBundleError::ArtifactLayout(
                "catalog must contain exactly one development worker",
            ));
        }
        let runtimes = catalog
            .artifacts()
            .iter()
            .filter(|artifact| matches!(artifact.kind(), ArtifactKind::Runtime))
            .collect::<Vec<_>>();
        if runtimes.len() != 1 {
            return Err(DeveloperBundleError::ArtifactLayout(
                "catalog must contain exactly one development runtime",
            ));
        }
        let runtime_artifacts = catalog
            .artifacts()
            .iter()
            .filter(|artifact| {
                matches!(
                    artifact.kind(),
                    ArtifactKind::Worker | ArtifactKind::Runtime
                )
            })
            .map(|artifact| artifact.id().clone())
            .collect::<Vec<_>>();
        if runtime_artifacts.len() != 2 {
            return Err(DeveloperBundleError::ArtifactLayout(
                "catalog must contain one worker and one runtime",
            ));
        }
        let worker = workers[0];
        let runtime = runtimes[0];
        let data_root = SemanticDataRoot::from_app_data(app_data_directory);
        let catalog_worker_path = bundle_directory
            .join("artifacts")
            .join(worker.id().as_str());
        let catalog_runtime_path = bundle_directory
            .join("artifacts")
            .join(runtime.id().as_str());

        let manager = ComponentManager::new(
            SemanticStateStore::new(configuration_directory.join("semantic-components")),
            app_data_directory,
        );
        let state = manager.state()?;
        let installed_worker = active_component_payload(&state, worker, catalog_worker_path);
        let installed_runtime = active_component_payload(&state, runtime, catalog_runtime_path);
        let active_model_pack = active_model_pack_resolver(manager.clone());
        let ocr_directory = configuration_directory.join("semantic-ocr");
        let ocr_policy = Arc::new(OcrPolicyStore::load(&ocr_directory));
        let ocr_probe: Arc<dyn OcrExecutableProbe> = Arc::new(DoclingOcrExecutableProbe);
        let ocr_executable = {
            let policy = Arc::clone(&ocr_policy);
            let probe = Arc::clone(&ocr_probe);
            Arc::new(move || {
                super::semantic_production::configured_ocrmypdf_executable(
                    policy.as_ref(),
                    probe.as_ref(),
                )
            }) as fm_semantic_worker::DeveloperOcrExecutableResolver
        };
        #[cfg(feature = "semantic-gemma")]
        let original_model = Some(original_model_resolver(
            manager.clone(),
            Arc::clone(&catalog),
            configuration_directory.to_path_buf(),
            installed_worker.clone(),
            data_root.category_path(DataCategory::Zvec),
            installed_runtime
                .parent()
                .expect("an installed artifact payload always has a parent")
                .to_path_buf(),
            Arc::clone(&ocr_executable),
        ));
        #[cfg(not(feature = "semantic-gemma"))]
        let original_model = None;
        let components = Arc::new(ManagedSemanticComponentCapability::new(
            manager,
            catalog,
            ManagedSemanticComponentConfiguration {
                runtime_and_worker_artifacts: runtime_artifacts,
                environment: InstallEnvironment::new(
                    TargetTriple::new(std::env::consts::OS, std::env::consts::ARCH)
                        .map_err(DeveloperBundleError::Catalog)?,
                    1,
                    BTreeMap::new(),
                ),
                distribution: DesktopSemanticDistribution::Direct,
                minimum_free_space_reserve_bytes: 64 * 1024 * 1024,
                gemma_library_configuration_directory: configuration_directory.to_path_buf(),
            },
            ManagedSemanticComponentAdapters {
                artifact_source: Arc::new(BundleArtifactSource {
                    directory: bundle_directory.join("artifacts"),
                }),
                free_space: Arc::new(FilesystemFreeSpace),
                activation: Arc::new(DeveloperActivation),
                indexing: Arc::new(DeveloperIndexing::default()),
                quiescer: Arc::new(DeveloperQuiescer {
                    runtime_directory: data_root.path().join("worker-runtime"),
                }),
                index_inventory: None,
                index_remover: None,
            },
        ));

        Ok(Self {
            components,
            installed_worker,
            runtime_directory: data_root.path().join("worker-runtime"),
            worker_data_directory: data_root.category_path(DataCategory::Zvec),
            reindex_pending_marker: data_root
                .category_path(DataCategory::Zvec)
                .join("model-reindex-pending"),
            native_library_directory: installed_runtime
                .parent()
                .expect("an installed artifact payload always has a parent")
                .to_path_buf(),
            active_model_pack,
            original_model,
            ocr: SemanticOcrService::load(ocr_directory, ocr_policy, ocr_probe),
            ocr_executable,
        })
    }
}

fn active_component_payload(
    state: &SemanticState,
    catalog_artifact: &CatalogArtifact,
    catalog_path: PathBuf,
) -> PathBuf {
    state
        .installed_component(catalog_artifact.component_id())
        .filter(|component| {
            component.artifact_id() == catalog_artifact.id()
                && component.version() == catalog_artifact.version()
                && component.checksum() == catalog_artifact.checksum()
        })
        .map(|component| component.installed_path().to_owned())
        .unwrap_or(catalog_path)
}

/// Resolves the installed payload of the model the durable component state
/// currently marks active.
///
/// Installation happens after startup and behind explicit consent, so this is
/// evaluated each time the worker is launched. It reads only host-owned durable
/// state; no frontend request contributes a path.
fn active_model_pack_resolver(
    manager: ComponentManager,
) -> fm_semantic_worker::DeveloperModelPackResolver {
    Arc::new(move || {
        let state = manager.state().map_err(|error| {
            format!("durable semantic component state could not be read: {error}")
        })?;
        let Some(active) = state.active_model().map(|model| model.identity().clone()) else {
            return Ok(None);
        };
        let component = state.installed_model(&active).ok_or_else(|| {
            format!(
                "active model {}@{} has no installed component",
                active.model_id().as_str(),
                active.revision().as_str()
            )
        })?;
        let path = component.installed_path();
        if !path.is_file() {
            return Err(format!(
                "active model {}@{} is missing its installed artifact at {}",
                active.model_id().as_str(),
                active.revision().as_str(),
                path.display()
            ));
        }
        Ok(Some(path.to_owned()))
    })
}

#[cfg(feature = "semantic-gemma")]
fn original_model_resolver(
    manager: ComponentManager,
    catalog: Arc<TrustedCatalog>,
    configuration_directory: PathBuf,
    installed_worker: PathBuf,
    data_directory: PathBuf,
    native_library_directory: PathBuf,
    ocr_executable: fm_semantic_worker::DeveloperOcrExecutableResolver,
) -> fm_semantic_worker::DeveloperManagedWorkerResolver {
    Arc::new(move || {
        let state = manager.state().map_err(|error| error.to_string())?;
        let Some(identity) = state.active_model().map(|model| model.identity()) else {
            return Ok(None);
        };
        if identity.model_id().as_str() != "google-embeddinggemma-2" {
            return Ok(None);
        }
        if !catalog.artifacts().iter().any(|artifact| {
            matches!(artifact.kind(), ArtifactKind::OriginalModel(model) if model == identity)
        }) {
            return Err("active Gemma model is absent from the development catalog".into());
        }
        let (dimensions, media) =
            super::semantic_production::gemma_library_settings(&configuration_directory, identity)?;
        let original = manager
            .verified_original_model_files(&catalog, identity)
            .map_err(|error| format!("Gemma original files are invalid: {error}"))?
            .ok_or_else(|| "Gemma original files are incomplete".to_owned())?;
        let files =
            GemmaNativeFiles::from_original_files(&original).map_err(|error| error.to_string())?;
        let launch = fm_semantic_worker::ManagedWorkerLaunch::new_gemma(
            installed_worker.clone(),
            data_directory.clone(),
            native_library_directory.clone(),
            files,
            dimensions,
            media,
        )
        .with_ocrmypdf_executable(ocr_executable());
        Ok(Some(launch))
    })
}

struct BundleArtifactSource {
    directory: PathBuf,
}

impl ArtifactSource for BundleArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        let path = self.directory.join(request.artifact_id().as_str());
        let mut file = fs::File::open(&path).map_err(|error| {
            ArtifactSourceError::Unavailable(format!(
                "cannot read development artifact {}: {error}",
                request.artifact_id().as_str()
            ))
        })?;
        let length = file
            .metadata()
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?
            .len();
        if request.offset() > length {
            return Err(ArtifactSourceError::InvalidOffset {
                offset: request.offset(),
            });
        }
        file.seek(SeekFrom::Start(request.offset()))
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        // Bounded chunks keep installing a multi-hundred-megabyte model pack
        // from costing its full size in resident memory, and give the installer
        // real resume points.
        let remaining = length - request.offset();
        let wanted = remaining.min(ARTIFACT_CHUNK_BYTES);
        let mut bytes =
            vec![
                0_u8;
                usize::try_from(wanted).map_err(|_| ArtifactSourceError::Unavailable(
                    "development artifact chunk exceeds this platform's addressable size"
                        .to_owned()
                ))?
            ];
        file.read_exact(&mut bytes)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        Ok(ArtifactChunk::new(bytes, wanted == remaining))
    }
}

struct FilesystemFreeSpace;

impl FreeSpaceProbe for FilesystemFreeSpace {
    fn available_bytes(&self, path: &Path) -> Result<u64, FreeSpaceError> {
        let mut existing = path;
        while !existing.exists() {
            existing = existing.parent().ok_or_else(|| {
                FreeSpaceError::new("semantic data root has no existing filesystem ancestor")
            })?;
        }
        fs2::available_space(existing).map_err(|error| FreeSpaceError::new(error.to_string()))
    }
}

struct DeveloperActivation;

impl ActivationProbe for DeveloperActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        installed_path: &Path,
    ) -> Result<(), ActivationError> {
        let metadata = fs::metadata(installed_path)
            .map_err(|error| ActivationError::new(error.to_string()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(ActivationError::new(
                "development artifact is not a non-empty regular file",
            ));
        }
        match artifact.kind() {
            ArtifactKind::Worker => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mut permissions = metadata.permissions();
                    permissions.set_mode(0o700);
                    fs::set_permissions(installed_path, permissions)
                        .map_err(|error| ActivationError::new(error.to_string()))?;
                }
            }
            ArtifactKind::Runtime => {
                fs::copy(
                    installed_path,
                    installed_path.with_file_name(native_library_file_name()),
                )
                .map_err(|error| ActivationError::new(error.to_string()))?;
            }
            ArtifactKind::Model(identity) => {
                let pack = ModelPack::open(installed_path)
                    .map_err(|error| ActivationError::new(error.to_string()))?;
                let index = pack.index();
                if index.production {
                    return Err(ActivationError::new(
                        "development model pack is not explicitly non-production",
                    ));
                }
                if index.model_id != identity.model_id().as_str()
                    || index.model_revision != identity.revision().as_str()
                {
                    return Err(ActivationError::new(
                        "development model pack identity does not match the signed catalog",
                    ));
                }
                // `ModelPack::open` already validated the layout, and the
                // installer already verified the whole payload against the
                // signed catalog checksum. Re-hash only the small members so
                // activation stays fast for a multi-hundred-megabyte graph.
                for member in index
                    .files
                    .iter()
                    .filter(|member| member.length <= SMALL_MEMBER_VERIFICATION_BYTES)
                {
                    pack.read(&member.name)
                        .map_err(|error| ActivationError::new(error.to_string()))?;
                }
            }
            ArtifactKind::OriginalModel(identity) => {
                if !cfg!(feature = "semantic-gemma")
                    || identity.model_id().as_str() != "google-embeddinggemma-2"
                {
                    return Err(ActivationError::new(
                        "original-file model runtime is not configured for the developer worker",
                    ));
                }
            }
            ArtifactKind::ModelFile(_) => {}
        }
        Ok(())
    }
}

fn native_library_file_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libzvec_c_api.dylib"
    } else if cfg!(target_os = "windows") {
        "zvec_c_api.dll"
    } else {
        "libzvec_c_api.so"
    }
}

#[derive(Default)]
struct DeveloperIndexing {
    paused: Arc<AtomicBool>,
}

impl IndexingController for DeveloperIndexing {
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError> {
        self.paused.store(true, Ordering::Release);
        Ok(Box::new(DeveloperPauseGuard {
            paused: Arc::clone(&self.paused),
        }))
    }
}

struct DeveloperPauseGuard {
    paused: Arc<AtomicBool>,
}

impl IndexingPauseGuard for DeveloperPauseGuard {}

impl Drop for DeveloperPauseGuard {
    fn drop(&mut self) {
        self.paused.store(false, Ordering::Release);
    }
}

struct DeveloperQuiescer {
    runtime_directory: PathBuf,
}

impl ComponentQuiescer for DeveloperQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        if self.runtime_directory.join("worker.pid").exists() {
            return Err(QuiesceError::new(
                "the developer worker remained active after bounded shutdown; close all Procyon processes and retry",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeveloperBundleError {
    #[error("semantic developer bundle directory is unavailable at {path}: {source}")]
    BundleDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("semantic developer bundle I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("semantic developer catalog JSON is invalid: {0}")]
    CatalogJson(serde_json::Error),
    #[error("semantic developer catalog signature file must contain exactly 64 bytes")]
    InvalidSignatureFile,
    #[error("semantic developer catalog is invalid: {0}")]
    Catalog(fm_semantic_components::CatalogError),
    #[error("semantic developer component state is invalid: {0}")]
    State(#[from] fm_semantic_components::InstallError),
    #[error("semantic developer artifact layout is invalid: {0}")]
    ArtifactLayout(&'static str),
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "semantic-gemma")]
    use fm_application::semantic_components::SemanticComponentCapability;
    #[cfg(feature = "semantic-gemma")]
    use fm_semantic_components::SemanticProfile;
    use fm_semantic_components::{
        ArtifactCompatibility, ArtifactId, ComponentResources, LicenseInfo, ModelId, ModelIdentity,
        ModelPackKind, ModelPackSpec, ModelRevision, Sha256Digest, write_model_pack,
    };

    use super::*;

    #[cfg(feature = "semantic-gemma")]
    #[tokio::test]
    #[ignore = "requires PROCYON_GEMMA_DEVELOPER_BUNDLE from the local Gemma builder"]
    async fn installed_development_gemma_resolves_verified_original_files() {
        use fm_application::semantic::{
            IpcSemanticCapability, LibraryId as WorkerLibraryId, SemanticCapability,
            SemanticOperationId, SemanticQuery, SemanticScope, TenantId,
        };
        use fm_semantic_library::{
            DeviceLibraryIdentity, GemmaMediaSelection, LibraryId,
            ModelIdentity as LibraryModelIdentity, ResourceBudgets, ResourceProfile,
            ResourceProfileKind, SemanticLibraryCoordinator, SemanticLibraryPolicy,
        };
        use fm_semantic_worker::semantic_storage::SemanticCatalog;
        use fm_semantic_worker::{
            IngestionScope, IngestionState, ManagedModel, ManagedWorkerLaunch,
            ManagedWorkerResolver, WorkerConnector,
        };

        let bundle_path = std::env::var_os("PROCYON_GEMMA_DEVELOPER_BUNDLE")
            .map(PathBuf::from)
            .expect("set PROCYON_GEMMA_DEVELOPER_BUNDLE to the built catalog");
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/semantic-developer-tests");
        fs::create_dir_all(&parent).expect("test parent");
        let data = tempfile::tempdir_in(parent.canonicalize().expect("canonical test parent"))
            .expect("isolated development data");
        let metal_integration = std::env::var_os("PROCYON_GEMMA_METAL_INTEGRATION").is_some();
        let media = GemmaMediaSelection {
            images: true,
            audio: !metal_integration,
            video: !metal_integration,
        };
        let model = LibraryModelIdentity::embeddinggemma_2(128, media).expect("Gemma model");
        let policy = SemanticLibraryPolicy::new(
            DeviceLibraryIdentity::new(LibraryId::new(), model),
            ResourceProfile {
                kind: ResourceProfileKind::Balanced,
                budgets: ResourceBudgets::default(),
            },
        )
        .expect("library policy");
        SemanticLibraryCoordinator::new(
            data.path().join("semantic-library-gemma"),
            data.path().join("semantic/library-gemma"),
        )
        .lock()
        .expect("lock Gemma library")
        .initialize(&policy)
        .expect("initialize fresh Gemma library");
        let bundle =
            DeveloperSemanticBundle::load(&bundle_path, data.path(), data.path()).expect("bundle");
        let profiles = bundle
            .components
            .catalog_profiles()
            .await
            .expect("profiles");
        assert!(
            profiles
                .iter()
                .any(|profile| profile.profile == SemanticProfile::EmbeddingGemma2)
        );
        let e5 = bundle
            .components
            .installation_offer(SemanticProfile::CompactMultilingual)
            .await
            .expect("signed E5 offer");
        bundle
            .components
            .install_or_enable(e5.consent())
            .await
            .expect("install E5 first");
        let offer = bundle
            .components
            .installation_offer(SemanticProfile::EmbeddingGemma2)
            .await
            .expect("signed development offer");
        bundle
            .components
            .install_or_enable(offer.consent())
            .await
            .expect("install original files");
        let status = bundle.components.status().await.expect("installed status");
        assert!(
            status.active_model().is_some(),
            "installed original Gemma files must resolve to the active embedding space",
        );
        let ocr_executable = Arc::clone(&bundle.ocr_executable);
        let service = fm_application::FileManagerService::new(
            fm_transport_dto::RuntimeKindDto::Tauri,
            data.path().join("workspaces"),
            data.path(),
        )
        .with_semantic_component_capability(bundle.components.clone())
        .with_semantic_ocr_service(bundle.ocr);
        let library = service
            .semantic_library_status(&fm_application::semantic_library::SemanticAccessContext::Host)
            .await
            .expect("semantic library status");
        assert!(
            library.available,
            "Gemma library must be available for enrolment"
        );
        assert_eq!(
            library
                .library
                .expect("initialized library")
                .model
                .dimensions,
            128
        );
        if !metal_integration {
            assert!(
                matches!(
                    service.semantic_ocr_status().availability,
                    fm_application::semantic_ocr::OcrAvailability::Available { .. }
                ),
                "installed OCRmyPDF must be discoverable in the Gemma development desktop",
            );
            assert!(ocr_executable().is_none(), "OCR requires explicit consent");
            service
                .set_semantic_ocr_consent(true)
                .await
                .expect("enable installed OCRmyPDF");
            assert!(ocr_executable().is_some(), "worker launch sees consent");
            service
                .set_semantic_ocr_consent(false)
                .await
                .expect("disable OCRmyPDF");
            assert!(ocr_executable().is_none(), "worker launch sees revocation");
        }
        let original_model = bundle.original_model.expect("Gemma resolver");
        let launch = original_model()
            .expect("verified development model")
            .expect("active Gemma launch");
        assert!(matches!(
            launch.model(),
            ManagedModel::Gemma {
                dimensions: 128,
                ..
            }
        ));
        let semantic = IpcSemanticCapability::desktop_developer_bundle(
            &bundle.runtime_directory,
            &bundle.installed_worker,
            &bundle.worker_data_directory,
            &bundle.native_library_directory,
            Some(bundle.active_model_pack),
            Some(original_model),
            bundle.ocr_executable,
        );
        let results = semantic
            .query(SemanticQuery {
                scope: SemanticScope::new(
                    TenantId::new("development-test"),
                    WorkerLibraryId::new("development-test"),
                ),
                request_id: SemanticOperationId::new("gemma-developer-query"),
                text: "A native Gemma worker starts from installed original files".into(),
                concept: None,
                maximum_results: 10,
            })
            .await
            .expect("installed Gemma worker query");
        assert!(results.is_empty());
        semantic
            .shutdown(std::time::Duration::from_secs(10))
            .await
            .expect("stop development worker");

        if metal_integration {
            use image::{ImageBuffer, ImageFormat, Rgb};

            let image = ImageBuffer::from_fn(128, 96, |x, y| {
                Rgb([
                    ((x * 2 + y) % 256) as u8,
                    ((x + y * 2) % 256) as u8,
                    ((x + y) % 256) as u8,
                ])
            });
            let mut png = std::io::Cursor::new(Vec::new());
            image.write_to(&mut png, ImageFormat::Png).unwrap();
            let png = png.into_inner();
            let reference: serde_json::Value = serde_json::from_str(include_str!(
                "../../../../crates/fm-semantic-worker/tests/embeddinggemma-image-reference-v1.json"
            ))
            .unwrap();
            assert_eq!(
                reference["revision"],
                "914f7f89142e33e77833254d9c9b90c3cef7303b"
            );
            let reference: Vec<f32> = serde_json::from_value(reference["vector"].clone()).unwrap();
            let ManagedModel::Gemma { files, media, .. } = launch.model() else {
                panic!("installed Gemma original files");
            };
            for dimensions in [128, 256, 512, 768] {
                let mut cpu_vector: Option<Vec<f32>> = None;
                for metal in [false, true] {
                    let label = format!("{dimensions}-{}", if metal { "metal" } else { "cpu" });
                    let runtime = data.path().join(format!("runtime-{label}"));
                    let index = data.path().join(format!("index-{label}"));
                    fs::create_dir_all(&runtime).unwrap();
                    fs::create_dir_all(&index).unwrap();
                    let mut selected = ManagedWorkerLaunch::new_gemma(
                        bundle.installed_worker.clone(),
                        index,
                        bundle.native_library_directory.clone(),
                        files.clone(),
                        dimensions,
                        *media,
                    );
                    if !metal {
                        selected = selected.with_development_cpu_images();
                    }
                    let resolver: ManagedWorkerResolver = Arc::new(move || Ok(selected.clone()));
                    let connector = WorkerConnector::desktop_managed_resolved(&runtime, resolver)
                        .with_startup_timeout(std::time::Duration::from_secs(120));
                    let peak = Arc::new(std::sync::atomic::AtomicU64::new(0));
                    let stop = Arc::new(AtomicBool::new(false));
                    let sample_peak = Arc::clone(&peak);
                    let sample_stop = Arc::clone(&stop);
                    let pid_path = runtime.join("worker.pid");
                    let sampler = tokio::spawn(async move {
                        while !sample_stop.load(Ordering::Relaxed) {
                            if let Ok(pid) = fs::read_to_string(&pid_path) {
                                let output = std::process::Command::new("ps")
                                    .args(["-o", "rss=", "-p", pid.trim()])
                                    .output()
                                    .expect("sample worker RSS");
                                if let Ok(kib) = String::from_utf8_lossy(&output.stdout)
                                    .trim()
                                    .parse::<u64>()
                                {
                                    sample_peak.fetch_max(kib * 1024, Ordering::Relaxed);
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        }
                    });
                    let start = std::time::Instant::now();
                    let client = connector.connect().await.expect("installed worker startup");
                    let startup_time = start.elapsed();
                    let backend = fs::read_to_string(runtime.join("gemma-backend"))
                        .expect("installed worker reports its backend");
                    if metal {
                        assert!(backend.contains("FP32 Metal images"), "{backend}");
                    } else {
                        assert!(backend.contains("development CPU baseline"), "{backend}");
                    }
                    let ingest_start = std::time::Instant::now();
                    let job = client
                        .ingest(
                            &label,
                            IngestionScope::new("metal-test", "metal-test"),
                            &label,
                            BTreeMap::from([
                                ("occurrence_id".into(), format!("occurrence-{label}")),
                                ("source_id".into(), format!("source-{label}")),
                                ("root_id".into(), "root".into()),
                                ("title".into(), "Gemma metal image".into()),
                            ]),
                            "image/png",
                            png.clone(),
                        )
                        .await
                        .expect("submit installed-worker image");
                    let state = loop {
                        let state = client
                            .ingestion_job("metal-test", "metal-test", &job)
                            .await
                            .expect("poll image ingestion")
                            .state;
                        if matches!(
                            state,
                            IngestionState::Completed
                                | IngestionState::Failed
                                | IngestionState::Cancelled
                                | IngestionState::Skipped
                        ) {
                            break state;
                        }
                        assert!(
                            ingest_start.elapsed().as_secs() < 120,
                            "image ingestion timed out"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    };
                    assert_eq!(state, IngestionState::Completed, "{label}");
                    let ingestion_time = ingest_start.elapsed();
                    let query_start = std::time::Instant::now();
                    let results = client
                        .query("metal-test", "metal-test", "Gemma metal image", 10)
                        .await
                        .expect("query installed-worker image");
                    let query_time = query_start.elapsed();
                    assert!(
                        results.iter().any(|result| result.document_id == label),
                        "{label}"
                    );
                    stop.store(true, Ordering::Relaxed);
                    sampler.await.unwrap();
                    assert!(peak.load(Ordering::Relaxed) > 0, "{label} RSS not sampled");
                    assert!(
                        peak.load(Ordering::Relaxed) < 4 * 1024 * 1024 * 1024,
                        "{label} exceeded the 4 GiB development-worker RSS bound"
                    );
                    eprintln!(
                        "installed Gemma image {label}: startup={startup_time:?} ingest={ingestion_time:?} query={query_time:?} sampled_peak_rss={} bytes",
                        peak.load(Ordering::Relaxed),
                    );
                    client
                        .shutdown(std::time::Duration::from_secs(10))
                        .await
                        .unwrap();
                    connector
                        .wait_until_stopped(std::time::Duration::from_secs(12))
                        .await
                        .unwrap();
                    let index = fs::read_dir(data.path().join(format!("index-{label}/indexes")))
                        .unwrap()
                        .map(|entry| entry.unwrap().path())
                        .collect::<Vec<_>>();
                    assert_eq!(index.len(), 1, "{label} has one embedding space");
                    let catalog = SemanticCatalog::open(index[0].join("catalog.sqlite")).unwrap();
                    let records = catalog.derived_index_record_batch(None, 100).unwrap();
                    let vector = records
                        .iter()
                        .find(|record| record.media_type == "image/png")
                        .unwrap_or_else(|| panic!("{label} has an image vector"));
                    assert_eq!(vector.vector.len(), dimensions);
                    let mut golden = reference[..dimensions].to_vec();
                    let norm = golden
                        .iter()
                        .map(|value| f64::from(*value).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    for value in &mut golden {
                        *value = (f64::from(*value) / norm) as f32;
                    }
                    let cosine = |a: &[f32], b: &[f32]| -> f64 {
                        a.iter()
                            .zip(b)
                            .map(|(a, b)| f64::from(*a) * f64::from(*b))
                            .sum()
                    };
                    let reference_cosine = cosine(&vector.vector, &golden);
                    assert!(
                        reference_cosine > 0.99999,
                        "{label} reference cosine {reference_cosine}"
                    );
                    if metal {
                        let parity = cosine(cpu_vector.as_ref().unwrap(), &vector.vector);
                        assert!(parity > 0.99999, "{label} CPU cosine {parity}");
                        eprintln!(
                            "installed Gemma image {label}: CPU={parity:.9} reference={reference_cosine:.9}"
                        );
                    } else {
                        cpu_vector = Some(vector.vector.clone());
                        eprintln!("installed Gemma image {label}: reference={reference_cosine:.9}");
                    }
                }
            }
        }
    }

    fn model_artifact(model_id: &str, revision: &str) -> CatalogArtifact {
        CatalogArtifact::new(
            ArtifactId::new("procyon.dev.model.test.v1").expect("artifact id"),
            fm_semantic_components::ComponentId::new("procyon.dev.model.test")
                .expect("component id"),
            ArtifactKind::Model(ModelIdentity::new(
                ModelId::new(model_id).expect("model id"),
                ModelRevision::new(revision).expect("revision"),
            )),
            semver::Version::new(1, 0, 0),
            fm_semantic_components::ArtifactLocation::new("https://developer.invalid/artifact")
                .expect("location"),
            LicenseInfo::new("MIT", "test fixture").expect("license"),
            Sha256Digest::calculate(b"unused"),
            ComponentResources::new(1, 1, 1).expect("resources"),
            ArtifactCompatibility::new(None, None, Vec::new(), 1),
        )
        .expect("artifact")
    }

    fn write_pack(path: &Path, model_id: &str, revision: &str, production: bool) {
        let members = path.with_extension("members");
        fs::create_dir_all(&members).expect("members");
        fs::write(members.join("config.json"), b"{}").expect("member");
        write_model_pack(
            path,
            &ModelPackSpec {
                kind: ModelPackKind::OnnxTransformerMeanPool,
                model_id: model_id.to_owned(),
                model_revision: revision.to_owned(),
                tokenizer: "test-tokenizer".into(),
                dimensions: 384,
                max_input_tokens: 512,
                query_prefix: "query: ".into(),
                passage_prefix: "passage: ".into(),
                production,
                source: "test fixture".into(),
                files: vec![("config.json".into(), members.join("config.json"))],
            },
        )
        .expect("pack");
    }

    #[test]
    fn activation_accepts_a_matching_non_production_model_pack() {
        let directory = tempfile::tempdir().expect("directory");
        let pack = directory.path().join("payload");
        write_pack(&pack, "example.model", "revision-one", false);

        DeveloperActivation
            .validate(&model_artifact("example.model", "revision-one"), &pack)
            .expect("activation");
    }

    #[test]
    fn original_file_model_is_not_activated_without_a_native_worker() {
        let directory = tempfile::tempdir().expect("directory");
        let weights = directory.path().join("model.safetensors");
        fs::write(&weights, b"verified original weights").expect("weights");
        let mut artifact = serde_json::to_value(model_artifact("example.model", "revision-one"))
            .expect("catalog artifact");
        artifact["kind"]["kind"] = "originalModel".into();
        let artifact: CatalogArtifact = serde_json::from_value(artifact).expect("original model");

        assert!(
            DeveloperActivation
                .validate(&artifact, &weights)
                .unwrap_err()
                .to_string()
                .contains("not configured")
        );
    }

    #[test]
    fn activation_rejects_production_foreign_and_mismatched_model_packs() {
        let directory = tempfile::tempdir().expect("directory");
        let artifact = model_artifact("example.model", "revision-one");

        let production = directory.path().join("production");
        write_pack(&production, "example.model", "revision-one", true);
        assert!(
            DeveloperActivation
                .validate(&artifact, &production)
                .unwrap_err()
                .to_string()
                .contains("non-production")
        );

        let mismatched = directory.path().join("mismatched");
        write_pack(&mismatched, "example.model", "revision-two", false);
        assert!(
            DeveloperActivation
                .validate(&artifact, &mismatched)
                .unwrap_err()
                .to_string()
                .contains("does not match the signed catalog")
        );

        let foreign = directory.path().join("foreign");
        fs::write(&foreign, br#"{"production": false}"#).expect("foreign");
        assert!(DeveloperActivation.validate(&artifact, &foreign).is_err());
    }

    #[test]
    fn large_artifacts_are_streamed_in_bounded_resumable_chunks() {
        let directory = tempfile::tempdir().expect("directory");
        let artifacts = directory.path().join("artifacts");
        fs::create_dir_all(&artifacts).expect("artifacts");
        let id = ArtifactId::new("procyon.dev.model.test.v1").expect("artifact id");
        let payload = (0..(ARTIFACT_CHUNK_BYTES * 2 + 1024))
            .map(|index| u8::try_from(index % 251).unwrap_or(0))
            .collect::<Vec<_>>();
        fs::write(artifacts.join(id.as_str()), &payload).expect("payload");
        let source = BundleArtifactSource {
            directory: artifacts,
        };

        let mut assembled = Vec::new();
        loop {
            let offset = u64::try_from(assembled.len()).expect("offset");
            let chunk = source
                .read(&ArtifactRequest::new(id.clone(), offset))
                .expect("chunk");
            assert!(u64::try_from(chunk.bytes().len()).expect("length") <= ARTIFACT_CHUNK_BYTES);
            assembled.extend_from_slice(chunk.bytes());
            if chunk.is_complete() {
                break;
            }
        }

        assert_eq!(assembled, payload);
        assert!(matches!(
            source.read(&ArtifactRequest::new(
                id,
                u64::try_from(payload.len()).expect("length") + 1
            )),
            Err(ArtifactSourceError::InvalidOffset { .. })
        ));
    }

    #[test]
    fn no_model_pack_is_resolved_before_anything_is_installed() {
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/semantic-developer-tests");
        fs::create_dir_all(&parent).expect("test parent");
        let parent = fs::canonicalize(parent).expect("canonical test parent");
        let directory = tempfile::tempdir_in(parent).expect("directory");
        let resolver = active_model_pack_resolver(ComponentManager::new(
            SemanticStateStore::new(directory.path().join("state")),
            directory.path(),
        ));

        assert_eq!(resolver(), Ok(None));
    }

    #[test]
    fn rebuilt_bundle_worker_wins_over_a_stale_installed_worker() {
        let directory = tempfile::tempdir().expect("directory");
        let installed = directory.path().join("installed-worker");
        fs::write(&installed, b"worker").expect("installed worker");
        let state: SemanticState = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "data_root": directory.path(),
            "active_model": null,
            "active_index_schema_version": null,
            "pending_model_migration": null,
            "installed_components": [{
                "artifact_id": "procyon.dev.worker.old",
                "component_id": "procyon.dev.worker",
                "kind": { "kind": "worker" },
                "version": "1.0.0",
                "checksum": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                             0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                "installed_path": installed
            }],
            "last_working_workers": [],
            "retained_models": []
        }))
        .expect("semantic state");
        let artifact = CatalogArtifact::new(
            ArtifactId::new("procyon.dev.worker.new").expect("artifact id"),
            fm_semantic_components::ComponentId::new("procyon.dev.worker").expect("component id"),
            ArtifactKind::Worker,
            semver::Version::new(1, 0, 0),
            fm_semantic_components::ArtifactLocation::new("https://developer.invalid/worker")
                .expect("location"),
            LicenseInfo::new("MIT", "test fixture").expect("license"),
            Sha256Digest::calculate(b"new worker"),
            ComponentResources::new(1, 1, 1).expect("resources"),
            ArtifactCompatibility::new(None, None, Vec::new(), 1),
        )
        .expect("artifact");

        let catalog_worker = directory.path().join("catalog-worker");
        assert_eq!(
            active_component_payload(&state, &artifact, catalog_worker.clone()),
            catalog_worker
        );
    }
}
