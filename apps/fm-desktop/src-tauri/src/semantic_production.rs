use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ed25519_dalek::VerifyingKey;
use fm_application::semantic_components::{
    DesktopSemanticDistribution, ManagedSemanticComponentAdapters,
    ManagedSemanticComponentCapability, ManagedSemanticComponentConfiguration,
};
use fm_application::semantic_ocr::{
    DoclingOcrExecutableProbe, OcrAvailability, OcrExecutableProbe, OcrPolicyStore,
    SemanticOcrService,
};
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactId, ArtifactKind, ArtifactRequest,
    ArtifactSource, ArtifactSourceError, CatalogArtifact, ComponentManager, ComponentQuiescer,
    DataCategory, FreeSpaceError, FreeSpaceProbe, IndexingController, IndexingPauseGuard,
    InstallEnvironment, ModelPack, PRODUCTION_ONNX_RUNTIME_COMPONENT_ID,
    PRODUCTION_WORKER_COMPONENT_ID, PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID, PauseError,
    ProductionCatalogManifest, QuiesceError, SemanticDataRoot, SemanticStateStore,
    SignedProductionCatalogManifest, TargetTriple, TrustedCatalog,
    embedded_production_verifying_key, production_pipeline_identity,
};

const ARTIFACT_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const SMALL_MEMBER_VERIFICATION_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) struct ProductionSemanticBundle {
    pub(crate) components: Arc<ManagedSemanticComponentCapability>,
    pub(crate) runtime_directory: PathBuf,
    pub(crate) reindex_pending_marker: PathBuf,
    pub(crate) worker: fm_semantic_worker::ManagedWorkerResolver,
    pub(crate) ocr: SemanticOcrService,
}

impl ProductionSemanticBundle {
    pub(crate) fn load(
        resource_directory: &Path,
        configuration_directory: &Path,
        app_data_directory: &Path,
    ) -> Result<Option<Self>, ProductionSemanticError> {
        let catalog_directory = resource_directory.join("semantic");
        let manifest_path = catalog_directory.join("catalog.json");
        let signature_path = catalog_directory.join("catalog.sig");
        if !manifest_path.exists() && !signature_path.exists() {
            return Ok(None);
        }
        if !manifest_path.is_file() || !signature_path.is_file() {
            return Err(ProductionSemanticError::IncompleteCatalog);
        }
        let verifying_key = embedded_production_verifying_key()?;
        let catalog = Self::load_verified_catalog(&catalog_directory, &verifying_key)?;
        if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            return Ok(None);
        }
        Self::from_catalog(catalog, configuration_directory, app_data_directory).map(Some)
    }

    fn load_verified_catalog(
        catalog_directory: &Path,
        verifying_key: &VerifyingKey,
    ) -> Result<Arc<TrustedCatalog>, ProductionSemanticError> {
        let manifest: ProductionCatalogManifest =
            serde_json::from_slice(&fs::read(catalog_directory.join("catalog.json"))?)?;
        let signature: [u8; 64] = fs::read(catalog_directory.join("catalog.sig"))?
            .try_into()
            .map_err(|_| ProductionSemanticError::InvalidSignatureFile)?;
        Ok(Arc::new(TrustedCatalog::verify_production_for_pipeline(
            SignedProductionCatalogManifest::new(manifest, signature),
            verifying_key,
            &production_pipeline_identity(),
        )?))
    }

    fn from_catalog(
        catalog: Arc<TrustedCatalog>,
        configuration_directory: &Path,
        app_data_directory: &Path,
    ) -> Result<Self, ProductionSemanticError> {
        let target = TargetTriple::new(std::env::consts::OS, std::env::consts::ARCH)?;
        let artifacts = target_artifacts(&catalog, &target)?;
        let mut runtime_and_worker_artifacts = vec![artifacts.zvec_runtime.id().clone()];
        if let Some(onnx_runtime) = artifacts.onnx_runtime {
            runtime_and_worker_artifacts.push(onnx_runtime.id().clone());
        }
        runtime_and_worker_artifacts.push(artifacts.worker.id().clone());
        let manager = ComponentManager::new(
            SemanticStateStore::new(configuration_directory.join("semantic-components")),
            app_data_directory,
        );
        let data_root = SemanticDataRoot::from_app_data(app_data_directory);
        let runtime_directory = data_root.path().join("worker-runtime");
        let native_library_directory = runtime_directory.join("native");
        let ocr_directory = configuration_directory.join("semantic-ocr");
        let ocr_policy = Arc::new(OcrPolicyStore::load(&ocr_directory));
        let ocr_probe: Arc<dyn OcrExecutableProbe> = Arc::new(DoclingOcrExecutableProbe);
        let resolver = managed_worker_resolver(
            manager.clone(),
            Arc::clone(&catalog),
            data_root.category_path(DataCategory::Zvec),
            native_library_directory,
            Arc::clone(&ocr_policy),
            Arc::clone(&ocr_probe),
        );
        let source = HttpArtifactSource::new(&catalog)?;
        let components = Arc::new(ManagedSemanticComponentCapability::new(
            manager,
            catalog,
            ManagedSemanticComponentConfiguration {
                runtime_and_worker_artifacts,
                environment: InstallEnvironment::new(target, 1, BTreeMap::new()),
                distribution: DesktopSemanticDistribution::Direct,
                minimum_free_space_reserve_bytes: 64 * 1024 * 1024,
            },
            ManagedSemanticComponentAdapters {
                artifact_source: Arc::new(source),
                free_space: Arc::new(FilesystemFreeSpace),
                activation: Arc::new(ProductionActivation),
                indexing: Arc::new(DesktopIndexing::default()),
                quiescer: Arc::new(DesktopQuiescer {
                    runtime_directory: runtime_directory.clone(),
                }),
                index_inventory: None,
                index_remover: None,
            },
        ));
        Ok(Self {
            components,
            runtime_directory,
            reindex_pending_marker: data_root
                .category_path(DataCategory::Zvec)
                .join("model-reindex-pending"),
            worker: resolver,
            ocr: SemanticOcrService::load(ocr_directory, ocr_policy, ocr_probe),
        })
    }
}

struct TargetArtifacts<'a> {
    worker: &'a CatalogArtifact,
    zvec_runtime: &'a CatalogArtifact,
    onnx_runtime: Option<&'a CatalogArtifact>,
}

fn one_target_component<'a>(
    catalog: &'a TrustedCatalog,
    target: &TargetTriple,
    component_id: &str,
    worker: bool,
    failure: &'static str,
) -> Result<&'a CatalogArtifact, ProductionSemanticError> {
    let artifacts = catalog
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.component_id().as_str() == component_id
                && artifact.compatibility().target() == Some(target)
                && if worker {
                    matches!(artifact.kind(), ArtifactKind::Worker)
                } else {
                    matches!(artifact.kind(), ArtifactKind::Runtime)
                }
        })
        .collect::<Vec<_>>();
    if artifacts.len() != 1 {
        return Err(ProductionSemanticError::ArtifactLayout(failure));
    }
    Ok(artifacts[0])
}

fn target_artifacts<'a>(
    catalog: &'a TrustedCatalog,
    target: &TargetTriple,
) -> Result<TargetArtifacts<'a>, ProductionSemanticError> {
    let worker = one_target_component(
        catalog,
        target,
        PRODUCTION_WORKER_COMPONENT_ID,
        true,
        "catalog must contain one worker for this desktop target",
    )?;
    let zvec_runtime = one_target_component(
        catalog,
        target,
        PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID,
        false,
        "catalog must contain one Zvec runtime for this desktop target",
    )?;
    let matching_onnx = catalog
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.component_id().as_str() == PRODUCTION_ONNX_RUNTIME_COMPONENT_ID
                && artifact.compatibility().target() == Some(target)
                && matches!(artifact.kind(), ArtifactKind::Runtime)
        })
        .collect::<Vec<_>>();
    let needs_onnx = target.operating_system() == "linux" && target.architecture() == "x86_64";
    let onnx_runtime = match (needs_onnx, matching_onnx.as_slice()) {
        (true, [runtime]) => Some(*runtime),
        (false, []) => None,
        (true, _) => {
            return Err(ProductionSemanticError::ArtifactLayout(
                "Linux x86-64 catalog must contain one ONNX Runtime",
            ));
        }
        (false, _) => {
            return Err(ProductionSemanticError::ArtifactLayout(
                "catalog contains an ONNX Runtime for a static-runtime target",
            ));
        }
    };
    Ok(TargetArtifacts {
        worker,
        zvec_runtime,
        onnx_runtime,
    })
}

fn managed_worker_resolver(
    manager: ComponentManager,
    catalog: Arc<TrustedCatalog>,
    data_directory: PathBuf,
    native_library_directory: PathBuf,
    ocr_policy: Arc<OcrPolicyStore>,
    ocr_probe: Arc<dyn OcrExecutableProbe>,
) -> fm_semantic_worker::ManagedWorkerResolver {
    Arc::new(move || {
        let state = manager
            .state()
            .map_err(|error| format!("semantic component state could not be read: {error}"))?;
        let target = TargetTriple::new(std::env::consts::OS, std::env::consts::ARCH)
            .map_err(|error| error.to_string())?;
        let artifacts = target_artifacts(&catalog, &target).map_err(|error| error.to_string())?;
        let active_model = state
            .active_model()
            .ok_or_else(|| "no production semantic model is active".to_owned())?;
        let model = catalog
            .artifacts()
            .iter()
            .find(|artifact| {
                matches!(
                    artifact.kind(),
                    ArtifactKind::Model(identity) if identity == active_model.identity()
                )
            })
            .ok_or_else(|| "the active model is not present in the trusted catalog".to_owned())?;

        let executable = required_verified_payload(&manager, artifacts.worker, "worker")?;
        let zvec_runtime =
            required_verified_payload(&manager, artifacts.zvec_runtime, "Zvec runtime")?;
        let onnx_runtime = artifacts
            .onnx_runtime
            .map(|artifact| required_verified_payload(&manager, artifact, "ONNX Runtime"))
            .transpose()?;
        let model_pack = required_verified_payload(&manager, model, "model")?;
        let mut native_payloads = vec![(zvec_runtime.as_path(), zvec_library_file_name())];
        if let Some(onnx_runtime) = onnx_runtime.as_deref() {
            native_payloads.push((onnx_runtime, onnx_runtime_file_name()));
        }
        prepare_native_library_directory(&native_library_directory, &native_payloads)?;
        let ocrmypdf_executable =
            configured_ocrmypdf_executable(ocr_policy.as_ref(), ocr_probe.as_ref());
        Ok(fm_semantic_worker::ManagedWorkerLaunch::new(
            executable,
            data_directory.clone(),
            native_library_directory.clone(),
            model_pack,
        )
        .with_ocrmypdf_executable(ocrmypdf_executable))
    })
}

fn configured_ocrmypdf_executable(
    policy: &OcrPolicyStore,
    probe: &dyn OcrExecutableProbe,
) -> Option<PathBuf> {
    if !policy.enabled() {
        return None;
    }
    match probe.probe() {
        OcrAvailability::Available { executable, .. } => Some(executable),
        OcrAvailability::Unavailable { .. } => None,
    }
}

fn required_verified_payload(
    manager: &ComponentManager,
    artifact: &CatalogArtifact,
    label: &str,
) -> Result<PathBuf, String> {
    manager
        .verified_installed_payload(artifact)
        .map_err(|error| format!("installed {label} failed verification: {error}"))?
        .ok_or_else(|| format!("the trusted {label} generation is not installed"))
}

fn prepare_native_library_directory(
    directory: &Path,
    payloads: &[(&Path, &str)],
) -> Result<(), String> {
    let staging = directory.with_extension("building");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let result = (|| {
        for (payload, loader_name) in payloads {
            let loader = staging.join(loader_name);
            fs::copy(payload, &loader).map_err(|error| error.to_string())?;
            if fs::read(payload).map_err(|error| error.to_string())?
                != fs::read(&loader).map_err(|error| error.to_string())?
            {
                return Err(format!(
                    "assembled native dependency {loader_name} differs from its verified payload"
                ));
            }
        }
        if directory.exists() {
            fs::remove_dir_all(directory).map_err(|error| error.to_string())?;
        }
        fs::rename(&staging, directory).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

struct HttpArtifact {
    location: String,
    bytes: u64,
}

struct HttpArtifactSource {
    client: reqwest::blocking::Client,
    artifacts: BTreeMap<ArtifactId, HttpArtifact>,
}

impl HttpArtifactSource {
    fn new(catalog: &TrustedCatalog) -> Result<Self, ProductionSemanticError> {
        let artifacts = catalog
            .artifacts()
            .iter()
            .map(|artifact| {
                (
                    artifact.id().clone(),
                    HttpArtifact {
                        location: artifact.location().as_str().to_owned(),
                        bytes: artifact.resources().download_bytes(),
                    },
                )
            })
            .collect();
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .https_only(true)
                .build()
                .map_err(ProductionSemanticError::HttpClient)?,
            artifacts,
        })
    }
}

impl ArtifactSource for HttpArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        let artifact = self.artifacts.get(request.artifact_id()).ok_or_else(|| {
            ArtifactSourceError::Unavailable("artifact is absent from the trusted catalog".into())
        })?;
        if request.offset() > artifact.bytes {
            return Err(ArtifactSourceError::InvalidOffset {
                offset: request.offset(),
            });
        }
        if request.offset() == artifact.bytes {
            return Ok(ArtifactChunk::new(Vec::new(), true));
        }
        let wanted = (artifact.bytes - request.offset()).min(ARTIFACT_CHUNK_BYTES);
        let end = request.offset() + wanted - 1;
        let response = self
            .client
            .get(&artifact.location)
            .header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", request.offset(), end),
            )
            .send()
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        if request.offset() > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(ArtifactSourceError::Unavailable(
                "artifact server did not honor the signed resumable byte range".into(),
            ));
        }
        if !response.status().is_success() {
            return Err(ArtifactSourceError::Unavailable(format!(
                "artifact server returned HTTP {}",
                response.status()
            )));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(wanted).map_err(|_| {
            ArtifactSourceError::Unavailable("artifact chunk exceeds addressable memory".into())
        })?);
        response
            .take(wanted + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        if bytes.len() != usize::try_from(wanted).unwrap_or(usize::MAX) {
            return Err(ArtifactSourceError::Unavailable(
                "artifact server returned an incomplete or oversized byte range".into(),
            ));
        }
        Ok(ArtifactChunk::new(bytes, end + 1 == artifact.bytes))
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

struct ProductionActivation;

impl ActivationProbe for ProductionActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        installed_path: &Path,
    ) -> Result<(), ActivationError> {
        let metadata = fs::metadata(installed_path)
            .map_err(|error| ActivationError::new(error.to_string()))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(ActivationError::new(
                "production artifact is not a non-empty regular file",
            ));
        }
        match artifact.kind() {
            ArtifactKind::Worker => {}
            ArtifactKind::Runtime => {
                let loader_name = runtime_loader_file_name(artifact.component_id().as_str())
                    .ok_or_else(|| {
                        ActivationError::new("production catalog contains an unknown runtime")
                    })?;
                fs::copy(installed_path, installed_path.with_file_name(loader_name))
                    .map_err(|error| ActivationError::new(error.to_string()))?;
            }
            ArtifactKind::Model(identity) => {
                let pack = ModelPack::open(installed_path)
                    .map_err(|error| ActivationError::new(error.to_string()))?;
                let index = pack.index();
                if !index.production {
                    return Err(ActivationError::new(
                        "managed desktop workers reject development model packs",
                    ));
                }
                if index.model_id != identity.model_id().as_str()
                    || index.model_revision != identity.revision().as_str()
                {
                    return Err(ActivationError::new(
                        "production model pack identity does not match the trusted catalog",
                    ));
                }
                for member in index
                    .files
                    .iter()
                    .filter(|member| member.length <= SMALL_MEMBER_VERIFICATION_BYTES)
                {
                    pack.read(&member.name)
                        .map_err(|error| ActivationError::new(error.to_string()))?;
                }
            }
        }
        Ok(())
    }
}

fn runtime_loader_file_name(component_id: &str) -> Option<&'static str> {
    if component_id == PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID {
        Some(zvec_library_file_name())
    } else if component_id == PRODUCTION_ONNX_RUNTIME_COMPONENT_ID
        && cfg!(all(target_os = "linux", target_arch = "x86_64"))
    {
        Some(onnx_runtime_file_name())
    } else {
        None
    }
}

fn zvec_library_file_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libzvec_c_api.dylib"
    } else if cfg!(target_os = "windows") {
        "zvec_c_api.dll"
    } else {
        "libzvec_c_api.so"
    }
}

fn onnx_runtime_file_name() -> &'static str {
    "libonnxruntime.so.1"
}

#[derive(Default)]
struct DesktopIndexing {
    paused: Arc<AtomicBool>,
}

impl IndexingController for DesktopIndexing {
    fn pause(&self) -> Result<Box<dyn IndexingPauseGuard>, PauseError> {
        self.paused.store(true, Ordering::Release);
        Ok(Box::new(DesktopPauseGuard {
            paused: Arc::clone(&self.paused),
        }))
    }
}

struct DesktopPauseGuard {
    paused: Arc<AtomicBool>,
}

impl IndexingPauseGuard for DesktopPauseGuard {}

impl Drop for DesktopPauseGuard {
    fn drop(&mut self) {
        self.paused.store(false, Ordering::Release);
    }
}

struct DesktopQuiescer {
    runtime_directory: PathBuf,
}

impl ComponentQuiescer for DesktopQuiescer {
    fn quiesce(&self) -> Result<(), QuiesceError> {
        if self.runtime_directory.join("worker.pid").exists() {
            return Err(QuiesceError::new(
                "close and restart Procyon before uninstalling the active semantic worker",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProductionSemanticError {
    #[error("production semantic catalog resources are incomplete")]
    IncompleteCatalog,
    #[error("production semantic catalog signature must contain exactly 64 bytes")]
    InvalidSignatureFile,
    #[error("production semantic catalog JSON is invalid: {0}")]
    CatalogJson(#[from] serde_json::Error),
    #[error("production semantic catalog is invalid: {0}")]
    Catalog(#[from] fm_semantic_components::CatalogError),
    #[error("production semantic catalog trust is unavailable: {0}")]
    ProductionCatalog(#[from] fm_semantic_components::ProductionCatalogError),
    #[error("production semantic artifact layout is invalid: {0}")]
    ArtifactLayout(&'static str),
    #[error("production semantic catalog I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("production semantic HTTP client could not be created: {0}")]
    HttpClient(reqwest::Error),
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;

    struct FixedOcrProbe(PathBuf);

    impl OcrExecutableProbe for FixedOcrProbe {
        fn probe(&self) -> OcrAvailability {
            OcrAvailability::Available {
                executable: self.0.clone(),
                version: "16.10.4".to_owned(),
            }
        }
    }

    #[test]
    fn absent_catalog_keeps_desktop_semantics_inert() {
        let resources = tempfile::tempdir().expect("resources");
        let configuration = tempfile::tempdir().expect("configuration");
        let app_data = tempfile::tempdir().expect("app data");

        let bundle =
            ProductionSemanticBundle::load(resources.path(), configuration.path(), app_data.path())
                .expect("absent resources are valid");

        assert!(bundle.is_none());
        assert!(!configuration.path().join("semantic-components").exists());
        assert!(!app_data.path().join("semantic-data").exists());
    }

    #[test]
    fn incomplete_catalog_resources_fail_closed() {
        let resources = tempfile::tempdir().expect("resources");
        let semantic = resources.path().join("semantic");
        fs::create_dir(&semantic).expect("semantic resources");
        fs::write(semantic.join("catalog.json"), b"{}").expect("catalog");

        let error = match ProductionSemanticBundle::load(
            resources.path(),
            resources.path(),
            resources.path(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("missing signature must fail"),
        };

        assert!(matches!(error, ProductionSemanticError::IncompleteCatalog));
    }

    #[test]
    fn malformed_signed_catalog_fails_before_host_capabilities_are_created() {
        let root = tempfile::tempdir().expect("root");
        let catalog = root.path().join("semantic");
        fs::create_dir(&catalog).expect("catalog directory");
        fs::write(catalog.join("catalog.json"), b"not-json").expect("catalog");
        fs::write(catalog.join("catalog.sig"), [0_u8; 64]).expect("signature");
        let verifying_key = SigningKey::from_bytes(&[0x51; 32]).verifying_key();

        let error = match ProductionSemanticBundle::load_verified_catalog(&catalog, &verifying_key)
        {
            Err(error) => error,
            Ok(_) => panic!("malformed catalog must fail"),
        };

        assert!(matches!(error, ProductionSemanticError::CatalogJson(_)));
        assert!(!root.path().join("semantic-components").exists());
    }

    #[test]
    fn production_worker_receives_discovered_ocr_only_after_consent() {
        let root = tempfile::tempdir().expect("root");
        let executable = root.path().join("ocrmypdf");
        fs::write(&executable, b"fixture").expect("executable");
        let policy = OcrPolicyStore::load(root.path().join("ocr-policy"));
        let probe = FixedOcrProbe(executable.clone());

        assert!(configured_ocrmypdf_executable(&policy, &probe).is_none());
        policy.set_enabled(true).expect("enable OCR");
        assert_eq!(
            configured_ocrmypdf_executable(&policy, &probe),
            Some(executable)
        );
        policy.set_enabled(false).expect("disable OCR");
        assert!(configured_ocrmypdf_executable(&policy, &probe).is_none());
    }

    #[test]
    fn assembles_only_verified_native_payload_bytes_for_worker_launch() {
        let root = tempfile::tempdir().expect("root");
        let first = root.path().join("first");
        let second = root.path().join("second");
        let native = root.path().join("worker-native");
        fs::write(&first, b"zvec").expect("first payload");
        fs::write(&second, b"onnx").expect("second payload");
        fs::create_dir(&native).expect("stale native directory");
        fs::write(native.join("stale"), b"stale").expect("stale payload");

        prepare_native_library_directory(
            &native,
            &[
                (first.as_path(), "libzvec_c_api.so"),
                (second.as_path(), "libonnxruntime.so.1"),
            ],
        )
        .expect("native assembly");

        assert_eq!(fs::read(native.join("libzvec_c_api.so")).unwrap(), b"zvec");
        assert_eq!(
            fs::read(native.join("libonnxruntime.so.1")).unwrap(),
            b"onnx"
        );
        assert!(!native.join("stale").exists());
        assert!(!native.with_extension("building").exists());
    }
}
