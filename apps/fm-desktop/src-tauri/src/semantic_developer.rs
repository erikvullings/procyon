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
    InstallEnvironment, ModelPack, PauseError, QuiesceError, SemanticDataRoot, SemanticStateStore,
    SignedCatalogManifest, TargetTriple, TrustedCatalog,
};

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

        let manager = ComponentManager::new(
            SemanticStateStore::new(configuration_directory.join("semantic-components")),
            app_data_directory,
        );
        let active_model_pack = active_model_pack_resolver(manager.clone());
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
            reindex_pending_marker: data_root
                .path()
                .join("developer-worker-data/model-reindex-pending"),
            native_library_directory: installed_runtime
                .parent()
                .expect("an installed artifact payload always has a parent")
                .to_path_buf(),
            active_model_pack,
        })
    }
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
        let component = state.installed_components().iter().find(|component| {
            matches!(component.kind(), ArtifactKind::Model(identity) if *identity == active)
        }).ok_or_else(|| {
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

#[cfg(test)]
mod tests {
    use fm_semantic_components::{
        ArtifactCompatibility, ArtifactId, ComponentResources, LicenseInfo, ModelId, ModelIdentity,
        ModelPackKind, ModelPackSpec, ModelRevision, Sha256Digest, write_model_pack,
    };

    use super::*;

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
}
