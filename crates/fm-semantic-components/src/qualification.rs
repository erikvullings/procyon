use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use thiserror::Error;

use crate::{
    ActivationError, ActivationProbe, ArtifactChunk, ArtifactKind, ArtifactRequest, ArtifactSource,
    ArtifactSourceError, CatalogArtifact, CatalogError, ComponentManager, FreeSpaceError,
    FreeSpaceProbe, InstallEnvironment, InstallError, ModelPack, PRODUCTION_MODEL_COMPONENT_ID,
    PRODUCTION_ONNX_RUNTIME_COMPONENT_ID, PRODUCTION_WORKER_COMPONENT_ID,
    PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID, ProductionCatalogError, ProductionCatalogManifest,
    SemanticDataRoot, SemanticProfile, SemanticStateStore, TargetTriple,
    production_pipeline_identity, verify_production_payloads,
};

const MARKER_FILE: &str = ".procyon-semantic-qualification-profile";
const MARKER_CONTENTS: &[u8] = b"procyon-semantic-qualification-profile-v1\n";
const APP_DATA_SUFFIX: &str = "Library/Application Support/fm";
const ARTIFACT_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const RESERVE_BYTES: u64 = 64 * 1024 * 1024;
const SMALL_MEMBER_VERIFICATION_BYTES: u64 = 32 * 1024 * 1024;

/// A marker-owned, disposable macOS profile used only for semantic qualification.
#[derive(Debug, Clone)]
pub struct QualificationProfile {
    root: PathBuf,
    application_data: PathBuf,
}

impl QualificationProfile {
    /// Creates a new qualification profile strictly beneath a dedicated test user's home.
    ///
    /// # Errors
    ///
    /// Rejects broad, normal application-data, pre-existing, escaping, or symlinked paths.
    pub fn create(
        dedicated_home: impl AsRef<Path>,
        profile_root: impl AsRef<Path>,
    ) -> Result<Self, QualificationProfileError> {
        let dedicated_home = canonical_directory(dedicated_home.as_ref())?;
        let profile_root = validate_profile_path(&dedicated_home, profile_root.as_ref())?;
        match fs::symlink_metadata(&profile_root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(QualificationProfileError::UnsafeProfilePath { path: profile_root });
            }
            Ok(_) => {
                return Err(QualificationProfileError::PreExistingUnownedProfile {
                    path: profile_root,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        ensure_safe_parents(&dedicated_home, &profile_root)?;
        fs::create_dir(&profile_root)?;
        let marker = profile_root.join(MARKER_FILE);
        let application_data = profile_root.join(APP_DATA_SUFFIX);
        if let Err(error) = (|| {
            fs::write(&marker, MARKER_CONTENTS)?;
            fs::create_dir_all(&application_data)
        })() {
            let _ = fs::remove_dir_all(&profile_root);
            return Err(error.into());
        }
        Ok(Self {
            root: profile_root,
            application_data,
        })
    }

    /// Opens an existing helper-owned qualification profile for cleanup.
    ///
    /// # Errors
    ///
    /// Rejects profiles without the exact ownership marker or with unsafe paths.
    pub fn open(
        dedicated_home: impl AsRef<Path>,
        profile_root: impl AsRef<Path>,
    ) -> Result<Self, QualificationProfileError> {
        let dedicated_home = canonical_directory(dedicated_home.as_ref())?;
        let root = validate_profile_path(&dedicated_home, profile_root.as_ref())?;
        ensure_safe_parents(&dedicated_home, &root)?;
        ensure_safe_existing_tree(&root)?;
        let marker = root.join(MARKER_FILE);
        if !marker_is_valid(&marker) {
            return Err(QualificationProfileError::PreExistingUnownedProfile { path: root });
        }
        Ok(Self {
            application_data: root.join(APP_DATA_SUFFIX),
            root,
        })
    }

    /// Returns the profile root that should be supplied as `HOME` when launching Procyon.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the release application's exact macOS application-data path inside this profile.
    #[must_use]
    pub fn application_data(&self) -> &Path {
        &self.application_data
    }

    /// Deletes only this exact marker-owned profile.
    ///
    /// # Errors
    ///
    /// Refuses cleanup after marker drift or if any symlink or special file appears in the tree.
    pub fn cleanup(self) -> Result<(), QualificationProfileError> {
        ensure_safe_existing_tree(&self.root)?;
        if !marker_is_valid(&self.root.join(MARKER_FILE)) {
            return Err(QualificationProfileError::PreExistingUnownedProfile { path: self.root });
        }
        fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn marker_is_valid(path: &Path) -> bool {
    fs::read(path).is_ok_and(|contents| contents == MARKER_CONTENTS)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, QualificationProfileError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(QualificationProfileError::UnsafeProfilePath {
            path: path.to_owned(),
        });
    }
    fs::canonicalize(path).map_err(Into::into)
}

fn validate_profile_path(
    dedicated_home: &Path,
    profile_root: &Path,
) -> Result<PathBuf, QualificationProfileError> {
    let relative = profile_root.strip_prefix(dedicated_home).ok();
    let qualification_namespace = relative.and_then(|relative| {
        let parts = relative.components().collect::<Vec<_>>();
        match parts.as_slice() {
            [Component::Normal(namespace), Component::Normal(name)]
                if *namespace == "qualification-profiles" && !name.is_empty() =>
            {
                Some(())
            }
            _ => None,
        }
    });
    if !profile_root.is_absolute()
        || profile_root
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || qualification_namespace.is_none()
    {
        return Err(QualificationProfileError::UnsafeProfilePath {
            path: profile_root.to_owned(),
        });
    }
    Ok(profile_root.to_owned())
}

fn ensure_safe_parents(home: &Path, profile: &Path) -> Result<(), QualificationProfileError> {
    let relative =
        profile
            .strip_prefix(home)
            .map_err(|_| QualificationProfileError::UnsafeProfilePath {
                path: profile.to_owned(),
            })?;
    let mut current = home.to_owned();
    let components = relative.components().collect::<Vec<_>>();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        let Component::Normal(name) = component else {
            return Err(QualificationProfileError::UnsafeProfilePath {
                path: profile.to_owned(),
            });
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(QualificationProfileError::UnsafeProfilePath { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_safe_existing_tree(path: &Path) -> Result<(), QualificationProfileError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(QualificationProfileError::UnsafeProfilePath {
            path: path.to_owned(),
        });
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(QualificationProfileError::UnsafeProfilePath { path: child });
        }
        if metadata.is_dir() {
            ensure_safe_existing_tree(&child)?;
        }
    }
    Ok(())
}

/// Result of installing the private production payloads into an isolated profile.
#[derive(Debug, Clone)]
pub struct QualificationInstallReceipt {
    application_data: PathBuf,
    installed_artifact_count: usize,
}

impl QualificationInstallReceipt {
    /// Returns the exact application-data directory consumed by the release app.
    #[must_use]
    pub fn application_data(&self) -> &Path {
        &self.application_data
    }

    /// Returns the number of signed artifacts installed through `ComponentManager`.
    #[must_use]
    pub const fn installed_artifact_count(&self) -> usize {
        self.installed_artifact_count
    }
}

/// Installs an exact private macOS arm64 production catalog into a disposable profile.
///
/// The detached signature, production pipeline, private qualification URLs, exact artifact
/// directory, target, checksums, and production activation checks are verified before durable
/// component state is committed.
///
/// # Errors
///
/// Returns a typed error and removes the newly created profile when any check or install fails.
pub fn install_macos_qualification(
    dedicated_home: &Path,
    profile_root: &Path,
    catalog_path: &Path,
    signature_path: &Path,
    artifact_directory: &Path,
    public_key_path: &Path,
) -> Result<QualificationInstallReceipt, QualificationInstallError> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err(QualificationInstallError::UnsupportedHost);
    }
    let profile = QualificationProfile::create(dedicated_home, profile_root)?;
    let result = install_into_profile(
        &profile,
        catalog_path,
        signature_path,
        artifact_directory,
        public_key_path,
    );
    if result.is_err() {
        profile.cleanup()?;
    }
    result
}

fn install_into_profile(
    profile: &QualificationProfile,
    catalog_path: &Path,
    signature_path: &Path,
    artifact_directory: &Path,
    public_key_path: &Path,
) -> Result<QualificationInstallReceipt, QualificationInstallError> {
    require_regular_file(catalog_path)?;
    require_regular_file(signature_path)?;
    require_regular_file(public_key_path)?;
    require_plain_directory(artifact_directory)?;
    let catalog_bytes = fs::read(catalog_path)?;
    let signature: [u8; 64] = fs::read(signature_path)?
        .try_into()
        .map_err(|_| ProductionCatalogError::InvalidSignatureEncoding)?;
    let public_key: [u8; 32] = fs::read(public_key_path)?
        .try_into()
        .map_err(|_| QualificationInstallError::InvalidPublicKey)?;
    let public_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| QualificationInstallError::InvalidPublicKey)?;
    let manifest: ProductionCatalogManifest = serde_json::from_slice(&catalog_bytes)?;
    let catalog = crate::TrustedCatalog::verify_production_for_pipeline(
        crate::SignedProductionCatalogManifest::new(manifest.clone(), signature),
        &public_key,
        &production_pipeline_identity(),
    )?;
    verify_production_payloads(&manifest, artifact_directory)?;
    verify_private_macos_catalog(&catalog)?;

    let target = TargetTriple::new("macos", "aarch64")?;
    let base_artifacts = catalog
        .artifacts()
        .iter()
        .filter(|artifact| !matches!(artifact.kind(), ArtifactKind::Model(_)))
        .map(|artifact| artifact.id().clone())
        .collect::<Vec<_>>();
    let selected =
        catalog.installation_artifacts(SemanticProfile::MultilingualQuality, &base_artifacts)?;
    let semantic_root = SemanticDataRoot::from_app_data(profile.application_data());
    let offer = catalog.installation_offer(
        SemanticProfile::MultilingualQuality,
        &selected,
        &target,
        manifest.pipeline().worker_protocol_version(),
        semantic_root.path(),
        RESERVE_BYTES,
    )?;
    let manager = ComponentManager::new(
        SemanticStateStore::new(profile.application_data().join("semantic-components")),
        profile.application_data(),
    );
    let receipt = manager.install(
        offer.consent(),
        &catalog,
        &InstallEnvironment::new(
            target,
            manifest.pipeline().worker_protocol_version(),
            BTreeMap::new(),
        ),
        &QualificationArtifactSource {
            root: artifact_directory.to_owned(),
        },
        &QualificationFreeSpace,
        &QualificationActivation,
    )?;
    Ok(QualificationInstallReceipt {
        application_data: profile.application_data().to_owned(),
        installed_artifact_count: receipt.installed_artifacts().len(),
    })
}

fn verify_private_macos_catalog(
    catalog: &crate::TrustedCatalog,
) -> Result<(), QualificationInstallError> {
    let target = TargetTriple::new("macos", "aarch64")?;
    let mut components = BTreeSet::new();
    for artifact in catalog.artifacts() {
        if artifact.location().as_str().split('/').nth(2) != Some("qualification.invalid") {
            return Err(QualificationInstallError::PublishedArtifactLocation);
        }
        if let Some(artifact_target) = artifact.compatibility().target()
            && artifact_target != &target
        {
            return Err(QualificationInstallError::WrongTarget);
        }
        let exact_component = match artifact.component_id().as_str() {
            PRODUCTION_WORKER_COMPONENT_ID => {
                matches!(artifact.kind(), ArtifactKind::Worker)
                    && artifact.compatibility().target() == Some(&target)
            }
            PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID => {
                matches!(artifact.kind(), ArtifactKind::Runtime)
                    && artifact.compatibility().target() == Some(&target)
            }
            PRODUCTION_MODEL_COMPONENT_ID => {
                matches!(
                    artifact.kind(),
                    ArtifactKind::Model(identity)
                        if identity == production_pipeline_identity().model()
                ) && artifact.compatibility().target().is_none()
            }
            _ => false,
        };
        if !exact_component {
            return Err(QualificationInstallError::UnexpectedArtifactSet);
        }
        components.insert(artifact.component_id().as_str());
    }
    let expected = BTreeSet::from([
        PRODUCTION_WORKER_COMPONENT_ID,
        PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID,
        PRODUCTION_MODEL_COMPONENT_ID,
    ]);
    if components != expected
        || catalog.artifacts().len() != 3
        || catalog.artifacts().iter().any(|artifact| {
            artifact.component_id().as_str() == PRODUCTION_ONNX_RUNTIME_COMPONENT_ID
        })
    {
        return Err(QualificationInstallError::UnexpectedArtifactSet);
    }
    Ok(())
}

fn require_regular_file(path: &Path) -> Result<(), QualificationInstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(QualificationInstallError::UnsafeInput {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn require_plain_directory(path: &Path) -> Result<(), QualificationInstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(QualificationInstallError::UnsafeInput {
            path: path.to_owned(),
        });
    }
    Ok(())
}

struct QualificationArtifactSource {
    root: PathBuf,
}

impl ArtifactSource for QualificationArtifactSource {
    fn read(&self, request: &ArtifactRequest) -> Result<ArtifactChunk, ArtifactSourceError> {
        let path = self.root.join(request.artifact_id().as_str());
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(path)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
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
        let wanted = (length - request.offset()).min(ARTIFACT_CHUNK_BYTES);
        let mut bytes = vec![
            0_u8;
            usize::try_from(wanted).map_err(|error| {
                ArtifactSourceError::Unavailable(error.to_string())
            })?
        ];
        file.read_exact(&mut bytes)
            .map_err(|error| ArtifactSourceError::Unavailable(error.to_string()))?;
        Ok(ArtifactChunk::new(
            bytes,
            request.offset() + wanted == length,
        ))
    }
}

struct QualificationFreeSpace;

impl FreeSpaceProbe for QualificationFreeSpace {
    fn available_bytes(&self, path: &Path) -> Result<u64, FreeSpaceError> {
        let existing = path
            .ancestors()
            .find(|candidate| candidate.exists())
            .ok_or_else(|| FreeSpaceError::new("qualification profile has no filesystem"))?;
        fs2::available_space(existing).map_err(|error| FreeSpaceError::new(error.to_string()))
    }
}

struct QualificationActivation;

impl ActivationProbe for QualificationActivation {
    fn validate(
        &self,
        artifact: &CatalogArtifact,
        installed_path: &Path,
    ) -> Result<(), ActivationError> {
        let metadata = fs::symlink_metadata(installed_path)
            .map_err(|error| ActivationError::new(error.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
            return Err(ActivationError::new(
                "production artifact is not a non-empty regular file",
            ));
        }
        match artifact.kind() {
            ArtifactKind::Worker => {}
            ArtifactKind::Runtime => {
                fs::copy(
                    installed_path,
                    installed_path.with_file_name("libzvec_c_api.dylib"),
                )
                .map_err(|error| ActivationError::new(error.to_string()))?;
            }
            ArtifactKind::Model(identity) => {
                let pack = ModelPack::open(installed_path)
                    .map_err(|error| ActivationError::new(error.to_string()))?;
                if !pack.index().production
                    || pack.index().model_id != identity.model_id().as_str()
                    || pack.index().model_revision != identity.revision().as_str()
                {
                    return Err(ActivationError::new(
                        "production model pack identity does not match the trusted catalog",
                    ));
                }
                for member in pack
                    .index()
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

/// Qualification profile creation or cleanup failure.
#[derive(Debug, Error)]
pub enum QualificationProfileError {
    /// The requested path was broad, escaped the home, or traversed an unsafe file type.
    #[error("unsafe semantic qualification profile path: {}", path.display())]
    UnsafeProfilePath {
        /// Rejected path.
        path: PathBuf,
    },
    /// An existing directory was not created and marked by this helper.
    #[error("semantic qualification profile is pre-existing or not helper-owned: {}", path.display())]
    PreExistingUnownedProfile {
        /// Unowned path.
        path: PathBuf,
    },
    /// Profile filesystem operation failed.
    #[error("semantic qualification profile filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Private qualification installation failure.
#[derive(Debug, Error)]
pub enum QualificationInstallError {
    /// The helper was not run by a native macOS arm64 binary.
    #[error("semantic qualification installation requires a native macOS arm64 host")]
    UnsupportedHost,
    /// The supplied raw Ed25519 public key was invalid.
    #[error("semantic qualification public key must contain exactly 32 valid bytes")]
    InvalidPublicKey,
    /// A signed artifact targeted another platform.
    #[error("semantic qualification catalog contains a non-macOS-arm64 artifact")]
    WrongTarget,
    /// A private kit may not consume release/publication URLs.
    #[error("semantic qualification catalog must use qualification.invalid artifact locations")]
    PublishedArtifactLocation,
    /// The signed catalog contained anything beyond the exact macOS production payload set.
    #[error("semantic qualification catalog has a missing or unexpected production component")]
    UnexpectedArtifactSet,
    /// An input path was a symlink or unexpected file type.
    #[error("unsafe semantic qualification input path: {}", path.display())]
    UnsafeInput {
        /// Rejected path.
        path: PathBuf,
    },
    /// Qualification profile validation failed.
    #[error(transparent)]
    Profile(#[from] QualificationProfileError),
    /// Production catalog verification failed.
    #[error(transparent)]
    Production(#[from] ProductionCatalogError),
    /// Signed catalog validation failed.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// Managed component installation failed.
    #[error(transparent)]
    Install(#[from] InstallError),
    /// Input JSON was malformed.
    #[error("semantic qualification catalog JSON is invalid")]
    Json(#[from] serde_json::Error),
    /// Qualification filesystem operation failed.
    #[error("semantic qualification filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
