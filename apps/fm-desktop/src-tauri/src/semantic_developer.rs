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
use fm_semantic_components::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactKind, ArtifactRequest, ArtifactSource,
    ArtifactSourceError, CatalogArtifact, CatalogManifest, ComponentManager, ComponentQuiescer,
    DataCategory, FreeSpaceError, FreeSpaceProbe, IndexingController, IndexingPauseGuard,
    InstallEnvironment, PauseError, QuiesceError, SemanticDataRoot, SemanticStateStore,
    SignedCatalogManifest, TargetTriple, TrustedCatalog,
};

const DEVELOPMENT_SIGNING_KEY: [u8; 32] = [0x19; 32];

pub(crate) struct DeveloperSemanticBundle {
    pub(crate) components: Arc<ManagedSemanticComponentCapability>,
    pub(crate) installed_worker: PathBuf,
    pub(crate) runtime_directory: PathBuf,
    pub(crate) worker_data_directory: PathBuf,
    pub(crate) native_library_directory: PathBuf,
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
        let installed_worker = data_root
            .category_path(DataCategory::Workers)
            .join(worker.component_id().as_str())
            .join(worker.version().to_string())
            .join(worker.id().as_str())
            .join("payload");
        let installed_runtime = data_root
            .category_path(DataCategory::Workers)
            .join(runtime.component_id().as_str())
            .join(runtime.version().to_string())
            .join(runtime.id().as_str())
            .join("payload");

        let components = Arc::new(ManagedSemanticComponentCapability::new(
            ComponentManager::new(
                SemanticStateStore::new(configuration_directory.join("semantic-components")),
                app_data_directory,
            ),
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
            worker_data_directory: data_root.path().join("developer-worker-data"),
            native_library_directory: installed_runtime
                .parent()
                .expect("an installed artifact payload always has a parent")
                .to_path_buf(),
        })
    }
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
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        Ok(ArtifactChunk::new(bytes, true))
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
            ArtifactKind::Model(_) => {
                let value: serde_json::Value = serde_json::from_slice(
                    &fs::read(installed_path)
                        .map_err(|error| ActivationError::new(error.to_string()))?,
                )
                .map_err(|error| ActivationError::new(error.to_string()))?;
                if value.get("production").and_then(serde_json::Value::as_bool) != Some(false) {
                    return Err(ActivationError::new(
                        "development model metadata is not explicitly non-production",
                    ));
                }
            }
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
                "close and restart Procyon before uninstalling the active developer worker",
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
    #[error("semantic developer artifact layout is invalid: {0}")]
    ArtifactLayout(&'static str),
}
