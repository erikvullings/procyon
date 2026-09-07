use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};
use semver::{Version, VersionReq};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use crate::SemanticProfile;
use crate::installer::{InstallationConsent, WorkerPatchUpdate};

fn validate_opaque_identifier(value: String, field: &'static str) -> Result<String, CatalogError> {
    if value.is_empty()
        || matches!(value.as_str(), "." | "..")
        || value.ends_with('.')
        || is_windows_reserved_component(&value)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(CatalogError::InvalidIdentifier { field });
    }
    Ok(value)
}

pub(crate) fn is_windows_reserved_component(value: &str) -> bool {
    let stem = value
        .split_once('.')
        .map_or(value, |(stem, _extension)| stem)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn revalidate_opaque_identifier(value: &str, field: &'static str) -> Result<(), CatalogError> {
    validate_opaque_identifier(value.to_owned(), field).map(drop)
}

macro_rules! opaque_identifier {
    ($name:ident, $description:literal, $field:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated opaque identifier.
            ///
            /// # Errors
            ///
            /// Returns [`CatalogError::InvalidIdentifier`] for an empty value
            /// or one containing path separators or other unsafe characters.
            pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
                validate_opaque_identifier(value.into(), $field).map(Self)
            }

            /// Returns the uninterpreted identifier text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

opaque_identifier!(
    ComponentId,
    "An opaque logical worker, runtime, or model component identifier.",
    "component identifier"
);
opaque_identifier!(
    ArtifactId,
    "An opaque identifier used to request one signed catalog artifact.",
    "artifact identifier"
);
opaque_identifier!(ModelId, "An opaque model identifier.", "model identifier");
opaque_identifier!(
    ModelRevision,
    "An immutable opaque upstream model revision.",
    "model revision"
);
opaque_identifier!(
    TokenizerId,
    "An opaque tokenizer identity and revision.",
    "tokenizer identifier"
);

/// An immutable opaque revision of a curated catalog.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ManifestRevision(String);

impl ManifestRevision {
    /// Creates a non-empty opaque catalog revision.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidIdentifier`] for an empty value.
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
        validate_opaque_identifier(value.into(), "manifest revision").map(Self)
    }

    /// Returns the opaque revision text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ManifestRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A SHA-256 digest from a signed catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// Calculates the digest of an artifact's bytes.
    #[must_use]
    pub fn calculate(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Creates a digest from its exact bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// A catalog-owned download location, never accepted by [`crate::ArtifactSource`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ArtifactLocation(String);

impl ArtifactLocation {
    /// Creates an HTTPS artifact location for signed catalog data.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidArtifactLocation`] for a non-HTTPS URL.
    pub fn new(value: impl Into<String>) -> Result<Self, CatalogError> {
        let value = value.into();
        let parsed = Url::parse(&value).map_err(|_| CatalogError::InvalidArtifactLocation)?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(CatalogError::InvalidArtifactLocation);
        }

        Ok(Self(value))
    }

    /// Returns the verified catalog location for host adapter configuration.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ArtifactLocation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Operating-system and CPU-architecture compatibility for a bundle.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TargetTriple {
    operating_system: String,
    architecture: String,
}

impl TargetTriple {
    /// Creates a target from non-empty platform labels.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidIdentifier`] for an unsafe label.
    pub fn new(
        operating_system: impl Into<String>,
        architecture: impl Into<String>,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            operating_system: validate_opaque_identifier(
                operating_system.into(),
                "operating system",
            )?,
            architecture: validate_opaque_identifier(architecture.into(), "architecture")?,
        })
    }

    /// Returns the operating-system label.
    #[must_use]
    pub fn operating_system(&self) -> &str {
        &self.operating_system
    }

    /// Returns the CPU-architecture label.
    #[must_use]
    pub fn architecture(&self) -> &str {
        &self.architecture
    }

    fn validate(&self) -> Result<(), CatalogError> {
        revalidate_opaque_identifier(&self.operating_system, "operating system")?;
        revalidate_opaque_identifier(&self.architecture, "architecture")
    }
}

/// Inclusive semantic worker protocol compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolRange {
    minimum: u32,
    maximum: u32,
}

impl ProtocolRange {
    /// Creates an ordered non-zero protocol range.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidProtocolRange`] for zero or reversed bounds.
    pub fn new(minimum: u32, maximum: u32) -> Result<Self, CatalogError> {
        if minimum == 0 || minimum > maximum {
            return Err(CatalogError::InvalidProtocolRange { minimum, maximum });
        }
        Ok(Self { minimum, maximum })
    }

    /// Reports whether the range includes a protocol version.
    #[must_use]
    pub const fn contains(self, version: u32) -> bool {
        version >= self.minimum && version <= self.maximum
    }

    /// Returns the oldest compatible protocol version.
    #[must_use]
    pub const fn minimum(self) -> u32 {
        self.minimum
    }

    /// Returns the newest compatible protocol version.
    #[must_use]
    pub const fn maximum(self) -> u32 {
        self.maximum
    }

    fn validate(self) -> Result<(), CatalogError> {
        Self::new(self.minimum, self.maximum).map(drop)
    }
}

/// Download, installed-disk, and peak-memory estimates for a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentResources {
    download_bytes: u64,
    installed_bytes: u64,
    ram_bytes: u64,
}

impl ComponentResources {
    /// Creates non-zero resource estimates.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidResourceEstimate`] when any estimate is zero.
    pub fn new(
        download_bytes: u64,
        installed_bytes: u64,
        ram_bytes: u64,
    ) -> Result<Self, CatalogError> {
        if download_bytes == 0 || installed_bytes == 0 || ram_bytes == 0 {
            return Err(CatalogError::InvalidResourceEstimate);
        }
        Ok(Self {
            download_bytes,
            installed_bytes,
            ram_bytes,
        })
    }

    /// Returns the compressed download size.
    #[must_use]
    pub const fn download_bytes(self) -> u64 {
        self.download_bytes
    }

    /// Returns the estimated installed size.
    #[must_use]
    pub const fn installed_bytes(self) -> u64 {
        self.installed_bytes
    }

    /// Returns the estimated peak RAM use.
    #[must_use]
    pub const fn ram_bytes(self) -> u64 {
        self.ram_bytes
    }

    fn validate(self) -> Result<(), CatalogError> {
        Self::new(self.download_bytes, self.installed_bytes, self.ram_bytes).map(drop)
    }
}

/// The role of one catalog artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "model", rename_all = "camelCase")]
pub enum ArtifactKind {
    /// Isolated semantic worker executable.
    Worker,
    /// Native embedding runtime bundle.
    Runtime,
    /// Model package for one exact embedding space.
    Model(ModelIdentity),
}

/// Platform, protocol, runtime, and index-schema constraints for an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactCompatibility {
    target: Option<TargetTriple>,
    protocol: Option<ProtocolRange>,
    runtimes: Vec<RuntimeCompatibility>,
    index_schema_version: u32,
}

impl ArtifactCompatibility {
    /// Creates the compatibility contract signed with an artifact.
    #[must_use]
    pub const fn new(
        target: Option<TargetTriple>,
        protocol: Option<ProtocolRange>,
        runtimes: Vec<RuntimeCompatibility>,
        index_schema_version: u32,
    ) -> Self {
        Self {
            target,
            protocol,
            runtimes,
            index_schema_version,
        }
    }

    /// Returns a platform restriction, or `None` for portable data.
    #[must_use]
    pub const fn target(&self) -> Option<&TargetTriple> {
        self.target.as_ref()
    }

    /// Returns an optional worker protocol restriction.
    #[must_use]
    pub const fn protocol(&self) -> Option<ProtocolRange> {
        self.protocol
    }

    /// Returns required installed runtime versions.
    #[must_use]
    pub fn runtimes(&self) -> &[RuntimeCompatibility] {
        &self.runtimes
    }

    /// Returns the index schema understood by the component.
    #[must_use]
    pub const fn index_schema_version(&self) -> u32 {
        self.index_schema_version
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.index_schema_version == 0 {
            return Err(CatalogError::InvalidSchemaVersion);
        }
        if let Some(target) = &self.target {
            target.validate()?;
        }
        if let Some(protocol) = self.protocol {
            protocol.validate()?;
        }
        let mut runtime_components = BTreeSet::new();
        for runtime in &self.runtimes {
            runtime.validate()?;
            if !runtime_components.insert(runtime.component_id()) {
                return Err(CatalogError::DuplicateRuntimeRequirement {
                    component: runtime.component_id().clone(),
                });
            }
        }
        Ok(())
    }
}

/// One immutable, signed, checksummed component artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogArtifact {
    id: ArtifactId,
    component_id: ComponentId,
    kind: ArtifactKind,
    version: Version,
    location: ArtifactLocation,
    license: LicenseInfo,
    checksum: Sha256Digest,
    resources: ComponentResources,
    compatibility: ArtifactCompatibility,
}

impl CatalogArtifact {
    /// Creates a complete signed-artifact record.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidSchemaVersion`] for a zero index schema.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ArtifactId,
        component_id: ComponentId,
        kind: ArtifactKind,
        version: Version,
        location: ArtifactLocation,
        license: LicenseInfo,
        checksum: Sha256Digest,
        resources: ComponentResources,
        compatibility: ArtifactCompatibility,
    ) -> Result<Self, CatalogError> {
        if compatibility.index_schema_version == 0 {
            return Err(CatalogError::InvalidSchemaVersion);
        }
        Ok(Self {
            id,
            component_id,
            kind,
            version,
            location,
            license,
            checksum,
            resources,
            compatibility,
        })
    }

    /// Returns the catalog artifact identifier used for downloads.
    #[must_use]
    pub const fn id(&self) -> &ArtifactId {
        &self.id
    }

    /// Returns the logical component identifier shared by its versions.
    #[must_use]
    pub const fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the artifact role.
    #[must_use]
    pub const fn kind(&self) -> &ArtifactKind {
        &self.kind
    }

    /// Returns the immutable package version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns the catalog-verified source location.
    #[must_use]
    pub const fn location(&self) -> &ArtifactLocation {
        &self.location
    }

    /// Returns the component license.
    #[must_use]
    pub const fn license(&self) -> &LicenseInfo {
        &self.license
    }

    /// Returns the expected SHA-256 digest.
    #[must_use]
    pub const fn checksum(&self) -> Sha256Digest {
        self.checksum
    }

    /// Returns the resource estimates.
    #[must_use]
    pub const fn resources(&self) -> ComponentResources {
        self.resources
    }

    /// Returns the compatibility contract.
    #[must_use]
    pub const fn compatibility(&self) -> &ArtifactCompatibility {
        &self.compatibility
    }

    fn validate(&self) -> Result<(), CatalogError> {
        revalidate_opaque_identifier(self.id.as_str(), "artifact identifier")?;
        revalidate_opaque_identifier(self.component_id.as_str(), "component identifier")?;
        ArtifactLocation::new(self.location.as_str())?;
        self.license.validate()?;
        self.resources.validate()?;
        self.compatibility.validate()
    }
}

/// The immutable identity of a model and its upstream revision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelIdentity {
    model: ModelId,
    revision: ModelRevision,
}

impl ModelIdentity {
    /// Combines a model identifier with one exact immutable upstream revision.
    #[must_use]
    pub const fn new(model: ModelId, revision: ModelRevision) -> Self {
        Self { model, revision }
    }

    /// Returns the opaque model identifier.
    #[must_use]
    pub const fn model_id(&self) -> &ModelId {
        &self.model
    }

    /// Returns the exact immutable upstream revision.
    #[must_use]
    pub const fn revision(&self) -> &ModelRevision {
        &self.revision
    }
}

/// License information shown before installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseInfo {
    spdx: String,
    notice: String,
}

impl LicenseInfo {
    /// Creates displayable license information.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::MissingModelMetadata`] when the SPDX expression
    /// is empty.
    pub fn new(spdx: impl Into<String>, notice: impl Into<String>) -> Result<Self, CatalogError> {
        let spdx = spdx.into();
        if spdx.trim().is_empty() {
            return Err(CatalogError::MissingModelMetadata {
                field: ModelField::License,
            });
        }
        Ok(Self {
            spdx,
            notice: notice.into(),
        })
    }

    /// Returns the SPDX license expression.
    #[must_use]
    pub fn spdx(&self) -> &str {
        &self.spdx
    }

    /// Returns the human-readable attribution or notice.
    #[must_use]
    pub fn notice(&self) -> &str {
        &self.notice
    }

    fn validate(&self) -> Result<(), CatalogError> {
        Self::new(self.spdx.clone(), self.notice.clone()).map(drop)
    }
}

/// Whether stored embeddings are normalized by the model contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmbeddingNormalization {
    /// Embeddings are stored with unit L2 length.
    UnitLength,
    /// Embeddings retain the runtime's unnormalized magnitudes.
    None,
}

/// Runtime component and version range required by a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCompatibility {
    component_id: ComponentId,
    version_requirement: VersionReq,
}

impl RuntimeCompatibility {
    /// Creates a runtime compatibility requirement.
    #[must_use]
    pub const fn new(component_id: ComponentId, version_requirement: VersionReq) -> Self {
        Self {
            component_id,
            version_requirement,
        }
    }

    /// Returns the required logical runtime component.
    #[must_use]
    pub const fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the accepted runtime versions.
    #[must_use]
    pub const fn version_requirement(&self) -> &VersionReq {
        &self.version_requirement
    }

    fn validate(&self) -> Result<(), CatalogError> {
        revalidate_opaque_identifier(self.component_id.as_str(), "component identifier")
    }
}

/// A required field in the immutable model contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelField {
    /// Immutable upstream revision.
    UpstreamRevision,
    /// Model license.
    License,
    /// Exact tokenizer identity.
    Tokenizer,
    /// Embedding vector dimensions.
    Dimensions,
    /// Embedding normalization convention.
    Normalization,
    /// Compatible embedding runtime.
    RuntimeCompatibility,
    /// Language coverage.
    LanguageCoverage,
    /// Estimated installed disk use.
    EstimatedDiskBytes,
    /// Estimated peak RAM use.
    EstimatedRamBytes,
}

/// Complete immutable embedding-space metadata for a curated or local model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMetadata {
    identity: ModelIdentity,
    license: LicenseInfo,
    tokenizer: TokenizerId,
    dimensions: u32,
    normalization: EmbeddingNormalization,
    runtime: RuntimeCompatibility,
    language_coverage: Vec<String>,
    estimated_disk_bytes: u64,
    estimated_ram_bytes: u64,
}

impl ModelMetadata {
    /// Creates and validates the required embedding-space metadata.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::MissingModelMetadata`] for an empty or zero
    /// required field.
    #[allow(clippy::too_many_arguments)]
    pub fn new<I, S>(
        identity: ModelIdentity,
        license: LicenseInfo,
        tokenizer: TokenizerId,
        dimensions: u32,
        normalization: EmbeddingNormalization,
        runtime: RuntimeCompatibility,
        language_coverage: I,
        estimated_disk_bytes: u64,
        estimated_ram_bytes: u64,
    ) -> Result<Self, CatalogError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if dimensions == 0 {
            return Err(CatalogError::MissingModelMetadata {
                field: ModelField::Dimensions,
            });
        }
        let mut language_coverage: Vec<String> =
            language_coverage.into_iter().map(Into::into).collect();
        if language_coverage.is_empty()
            || language_coverage
                .iter()
                .any(|language| language.trim().is_empty())
        {
            return Err(CatalogError::MissingModelMetadata {
                field: ModelField::LanguageCoverage,
            });
        }
        if estimated_disk_bytes == 0 {
            return Err(CatalogError::MissingModelMetadata {
                field: ModelField::EstimatedDiskBytes,
            });
        }
        if estimated_ram_bytes == 0 {
            return Err(CatalogError::MissingModelMetadata {
                field: ModelField::EstimatedRamBytes,
            });
        }
        language_coverage.sort();
        language_coverage.dedup();
        Ok(Self {
            identity,
            license,
            tokenizer,
            dimensions,
            normalization,
            runtime,
            language_coverage,
            estimated_disk_bytes,
            estimated_ram_bytes,
        })
    }

    /// Returns the exact model identity and upstream revision.
    #[must_use]
    pub const fn identity(&self) -> &ModelIdentity {
        &self.identity
    }

    /// Returns the model license.
    #[must_use]
    pub const fn license(&self) -> &LicenseInfo {
        &self.license
    }

    /// Returns the exact tokenizer identity.
    #[must_use]
    pub const fn tokenizer(&self) -> &TokenizerId {
        &self.tokenizer
    }

    /// Returns the embedding vector dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> u32 {
        self.dimensions
    }

    /// Returns the embedding normalization contract.
    #[must_use]
    pub const fn normalization(&self) -> EmbeddingNormalization {
        self.normalization
    }

    /// Returns the compatible runtime requirement.
    #[must_use]
    pub const fn runtime(&self) -> &RuntimeCompatibility {
        &self.runtime
    }

    /// Returns sorted, de-duplicated language tags.
    #[must_use]
    pub fn language_coverage(&self) -> &[String] {
        &self.language_coverage
    }

    /// Returns estimated installed model bytes.
    #[must_use]
    pub const fn estimated_disk_bytes(&self) -> u64 {
        self.estimated_disk_bytes
    }

    /// Returns estimated peak model RAM bytes.
    #[must_use]
    pub const fn estimated_ram_bytes(&self) -> u64 {
        self.estimated_ram_bytes
    }

    fn validate(&self) -> Result<(), CatalogError> {
        revalidate_opaque_identifier(self.identity.model_id().as_str(), "model identifier")?;
        revalidate_opaque_identifier(self.identity.revision().as_str(), "model revision")?;
        self.license.validate()?;
        revalidate_opaque_identifier(self.tokenizer.as_str(), "tokenizer identifier")?;
        self.runtime.validate()?;
        let normalized = Self::new(
            self.identity.clone(),
            self.license.clone(),
            self.tokenizer.clone(),
            self.dimensions,
            self.normalization,
            self.runtime.clone(),
            self.language_coverage.clone(),
            self.estimated_disk_bytes,
            self.estimated_ram_bytes,
        )?;
        if normalized.language_coverage != self.language_coverage {
            return Err(CatalogError::NonCanonicalModelMetadata);
        }
        for language in &self.language_coverage {
            revalidate_opaque_identifier(language, "language coverage")?;
        }
        Ok(())
    }
}

/// One curated model tied to a checksummed artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifest {
    artifact_id: ArtifactId,
    metadata: ModelMetadata,
}

impl ModelManifest {
    /// Associates complete model metadata with its signed package.
    #[must_use]
    pub const fn new(artifact_id: ArtifactId, metadata: ModelMetadata) -> Self {
        Self {
            artifact_id,
            metadata,
        }
    }

    /// Returns the model package artifact.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    /// Returns the complete immutable embedding-space metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
}

/// Curated semantic component and model data covered by one signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogManifest {
    format_version: u32,
    revision: ManifestRevision,
    artifacts: Vec<CatalogArtifact>,
    models: Vec<ModelManifest>,
    profile_resolutions: BTreeMap<SemanticProfile, ModelIdentity>,
}

impl CatalogManifest {
    /// Creates a version-one catalog manifest.
    ///
    /// # Errors
    ///
    /// Returns a typed catalog validation failure when entries are invalid.
    pub fn new(
        revision: ManifestRevision,
        artifacts: Vec<CatalogArtifact>,
        models: Vec<ModelManifest>,
        profile_resolutions: BTreeMap<SemanticProfile, ModelIdentity>,
    ) -> Result<Self, CatalogError> {
        let manifest = Self {
            format_version: 1,
            revision,
            artifacts,
            models,
            profile_resolutions,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Returns the immutable catalog revision.
    #[must_use]
    pub const fn revision(&self) -> &ManifestRevision {
        &self.revision
    }

    /// Returns RFC 8785 JSON Canonicalization Scheme bytes covered by the signature.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the typed manifest cannot be encoded.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CatalogError> {
        serde_json_canonicalizer::to_vec(self).map_err(CatalogError::Serialize)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        if self.format_version != 1 {
            return Err(CatalogError::UnsupportedFormatVersion {
                version: self.format_version,
            });
        }
        revalidate_opaque_identifier(self.revision.as_str(), "manifest revision")?;
        let mut artifacts = BTreeMap::new();
        let mut portable_artifact_ids = BTreeMap::new();
        let mut portable_component_ids = BTreeMap::new();
        let mut binary_component_versions = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if artifacts.insert(&artifact.id, artifact).is_some() {
                return Err(CatalogError::DuplicateArtifact {
                    id: artifact.id.clone(),
                });
            }
            let portable_artifact_id = artifact.id.as_str().to_ascii_lowercase();
            if portable_artifact_ids
                .insert(portable_artifact_id, &artifact.id)
                .is_some()
            {
                return Err(CatalogError::FilesystemIdentifierCollision {
                    field: "artifact identifier",
                });
            }
            let portable_component_id = artifact.component_id.as_str().to_ascii_lowercase();
            if let Some(existing) =
                portable_component_ids.insert(portable_component_id, &artifact.component_id)
                && existing != &artifact.component_id
            {
                return Err(CatalogError::FilesystemIdentifierCollision {
                    field: "component identifier",
                });
            }
            if matches!(artifact.kind, ArtifactKind::Worker | ArtifactKind::Runtime)
                && !binary_component_versions.insert((
                    &artifact.component_id,
                    &artifact.version,
                    artifact.compatibility.target.as_ref(),
                ))
            {
                return Err(CatalogError::DuplicateComponentVersion {
                    component: artifact.component_id.clone(),
                    version: artifact.version.clone(),
                });
            }
            if matches!(artifact.kind, ArtifactKind::Worker | ArtifactKind::Runtime)
                && artifact.compatibility.target.is_none()
            {
                return Err(CatalogError::MissingPlatformTarget {
                    id: artifact.id.clone(),
                });
            }
            if matches!(artifact.kind, ArtifactKind::Worker)
                && artifact.compatibility.protocol.is_none()
            {
                return Err(CatalogError::MissingProtocolCompatibility {
                    id: artifact.id.clone(),
                });
            }
        }
        for artifact in &self.artifacts {
            for runtime in artifact.compatibility().runtimes() {
                let compatible_runtime = self.artifacts.iter().any(|candidate| {
                    matches!(candidate.kind(), ArtifactKind::Runtime)
                        && candidate.component_id() == runtime.component_id()
                        && runtime.version_requirement().matches(candidate.version())
                });
                if !compatible_runtime {
                    return Err(CatalogError::MissingCompatibleRuntime {
                        artifact: artifact.id().clone(),
                        component: runtime.component_id().clone(),
                    });
                }
            }
        }
        let mut models = BTreeMap::new();
        let mut portable_model_identities = BTreeSet::new();
        let mut model_artifacts = BTreeMap::new();
        for model in &self.models {
            model.metadata.validate()?;
            if models.insert(model.metadata.identity(), model).is_some() {
                return Err(CatalogError::DuplicateModel {
                    identity: model.metadata.identity().clone(),
                });
            }
            let portable_identity = (
                model
                    .metadata
                    .identity()
                    .model_id()
                    .as_str()
                    .to_ascii_lowercase(),
                model
                    .metadata
                    .identity()
                    .revision()
                    .as_str()
                    .to_ascii_lowercase(),
            );
            if !portable_model_identities.insert(portable_identity) {
                return Err(CatalogError::FilesystemIdentifierCollision {
                    field: "model identity",
                });
            }
            let artifact = artifacts.get(&model.artifact_id).ok_or_else(|| {
                CatalogError::MissingModelArtifact {
                    id: model.artifact_id.clone(),
                }
            })?;
            if !matches!(
                artifact.kind(),
                ArtifactKind::Model(identity) if identity == model.metadata.identity()
            ) || artifact.license() != model.metadata.license()
                || artifact.resources().installed_bytes() != model.metadata.estimated_disk_bytes()
                || artifact.resources().ram_bytes() != model.metadata.estimated_ram_bytes()
                || !artifact
                    .compatibility()
                    .runtimes()
                    .contains(model.metadata.runtime())
            {
                return Err(CatalogError::ModelArtifactMismatch {
                    id: model.artifact_id.clone(),
                });
            }
            model_artifacts.insert(&model.artifact_id, model);
        }
        for artifact in &self.artifacts {
            if matches!(artifact.kind(), ArtifactKind::Model(_))
                && !model_artifacts.contains_key(artifact.id())
            {
                return Err(CatalogError::MissingModelManifest {
                    id: artifact.id().clone(),
                });
            }
        }
        for identity in self.profile_resolutions.values() {
            revalidate_opaque_identifier(identity.model_id().as_str(), "model identifier")?;
            revalidate_opaque_identifier(identity.revision().as_str(), "model revision")?;
            if !models.contains_key(identity) {
                return Err(CatalogError::UnknownProfileModel {
                    identity: identity.clone(),
                });
            }
        }
        Ok(())
    }
}

/// A catalog plus its detached Ed25519 signature.
#[derive(Debug, Clone)]
pub struct SignedCatalogManifest {
    manifest: CatalogManifest,
    signature: [u8; 64],
}

impl SignedCatalogManifest {
    /// Associates canonical catalog data with its detached signature.
    #[must_use]
    pub const fn new(manifest: CatalogManifest, signature: [u8; 64]) -> Self {
        Self {
            manifest,
            signature,
        }
    }
}

/// A curated catalog whose signature and data validation have succeeded.
#[derive(Debug, Clone)]
pub struct TrustedCatalog {
    manifest: CatalogManifest,
}

impl TrustedCatalog {
    /// Verifies the Ed25519 signature before exposing catalog data.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::InvalidSignature`] when verification fails.
    pub fn verify(signed: SignedCatalogManifest, key: &VerifyingKey) -> Result<Self, CatalogError> {
        let bytes = signed.manifest.canonical_bytes()?;
        let signature = Signature::from_bytes(&signed.signature);
        key.verify_strict(&bytes, &signature)
            .map_err(|_| CatalogError::InvalidSignature)?;
        signed.manifest.validate()?;
        Ok(Self {
            manifest: signed.manifest,
        })
    }

    /// Returns the immutable signed catalog revision.
    #[must_use]
    pub const fn revision(&self) -> &ManifestRevision {
        self.manifest.revision()
    }

    /// Returns a verified artifact by its opaque catalog identifier.
    #[must_use]
    pub fn artifact(&self, id: &ArtifactId) -> Option<&CatalogArtifact> {
        self.manifest
            .artifacts
            .iter()
            .find(|artifact| artifact.id() == id)
    }

    /// Returns every verified artifact in deterministic catalog order.
    #[must_use]
    pub fn artifacts(&self) -> &[CatalogArtifact] {
        &self.manifest.artifacts
    }

    /// Returns complete model metadata for one exact identity.
    #[must_use]
    pub fn model(&self, identity: &ModelIdentity) -> Option<&ModelManifest> {
        self.manifest
            .models
            .iter()
            .find(|model| model.metadata().identity() == identity)
    }

    /// Resolves an abstract profile through this signed manifest.
    #[must_use]
    pub fn resolve_profile(&self, profile: SemanticProfile) -> Option<&ModelIdentity> {
        self.manifest.profile_resolutions.get(&profile)
    }

    /// Resolves one profile-specific model package alongside configured binary dependencies.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a configured base artifact is missing, is a
    /// model package, or duplicates another selected artifact.
    pub fn installation_artifacts(
        &self,
        profile: SemanticProfile,
        runtime_and_worker_artifacts: &[ArtifactId],
    ) -> Result<Vec<ArtifactId>, CatalogError> {
        let identity = self
            .resolve_profile(profile)
            .ok_or(CatalogError::UnresolvedProfile { profile })?;
        let model = self
            .model(identity)
            .ok_or_else(|| CatalogError::UnknownProfileModel {
                identity: identity.clone(),
            })?;
        let mut selected = Vec::with_capacity(runtime_and_worker_artifacts.len() + 1);
        let mut seen = BTreeSet::new();
        for id in runtime_and_worker_artifacts {
            let artifact = self
                .artifact(id)
                .ok_or_else(|| CatalogError::UnknownArtifact { id: id.clone() })?;
            if matches!(artifact.kind(), ArtifactKind::Model(_)) {
                return Err(CatalogError::ModelArtifactInBasePlan { id: id.clone() });
            }
            if !seen.insert(id) {
                return Err(CatalogError::DuplicateOfferedArtifact { id: id.clone() });
            }
            selected.push(id.clone());
        }
        if !seen.insert(model.artifact_id()) {
            return Err(CatalogError::DuplicateOfferedArtifact {
                id: model.artifact_id().clone(),
            });
        }
        selected.push(model.artifact_id().clone());
        Ok(selected)
    }

    /// Builds the complete disclosure required before installation consent.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unknown or incompatible artifacts, a missing
    /// profile resolution, or a model package that does not match that resolution.
    pub fn installation_offer(
        &self,
        profile: SemanticProfile,
        artifact_ids: &[ArtifactId],
        target: &TargetTriple,
        protocol_version: u32,
        semantic_data_root: &Path,
        minimum_free_space_reserve_bytes: u64,
    ) -> Result<InstallationOffer, CatalogError> {
        let resolved_model = self
            .resolve_profile(profile)
            .cloned()
            .ok_or(CatalogError::UnresolvedProfile { profile })?;
        let mut artifacts = Vec::with_capacity(artifact_ids.len());
        let mut seen_artifacts = BTreeSet::new();
        for id in artifact_ids {
            if !seen_artifacts.insert(id) {
                return Err(CatalogError::DuplicateOfferedArtifact { id: id.clone() });
            }
            let artifact = self
                .artifact(id)
                .cloned()
                .ok_or_else(|| CatalogError::UnknownArtifact { id: id.clone() })?;
            ensure_platform_and_protocol(&artifact, target, protocol_version)?;
            artifacts.push(artifact);
        }

        let mut planned_versions = BTreeMap::new();
        for artifact in &artifacts {
            if planned_versions
                .insert(artifact.component_id(), artifact.version())
                .is_some()
            {
                return Err(CatalogError::AmbiguousComponentSelection {
                    component: artifact.component_id().clone(),
                });
            }
        }
        for artifact in &artifacts {
            for runtime in artifact.compatibility().runtimes() {
                let compatible = planned_versions
                    .get(runtime.component_id())
                    .is_some_and(|version| runtime.version_requirement().matches(version));
                if !compatible {
                    return Err(CatalogError::IncompatibleRuntime {
                        artifact: artifact.id().clone(),
                    });
                }
            }
        }
        let offered_models: Vec<_> = artifacts
            .iter()
            .filter(|artifact| matches!(artifact.kind(), ArtifactKind::Model(_)))
            .collect();
        if offered_models.len() != 1
            || !matches!(
                offered_models[0].kind(),
                ArtifactKind::Model(identity) if identity == &resolved_model
            )
        {
            return Err(CatalogError::ProfileModelNotOffered { profile });
        }
        let active_schema = offered_models[0].compatibility().index_schema_version();
        if let Some(incompatible) = artifacts
            .iter()
            .find(|artifact| artifact.compatibility().index_schema_version() != active_schema)
        {
            return Err(CatalogError::IncompatibleIndexSchema {
                artifact: incompatible.id().clone(),
                expected: active_schema,
                actual: incompatible.compatibility().index_schema_version(),
            });
        }

        let components = artifacts
            .into_iter()
            .map(ComponentDisclosure::from)
            .collect();
        Ok(InstallationOffer {
            catalog_revision: self.revision().clone(),
            profile,
            resolved_model,
            components,
            local_only_disclosure: LocalOnlyDisclosure,
            semantic_data_root: semantic_data_root.to_owned(),
            minimum_free_space_reserve_bytes,
        })
    }

    /// Selects the newest compatible patch update regardless of manifest order.
    ///
    /// Only stable worker releases with the same major/minor line and index
    /// schema are eligible for automatic update.
    #[must_use]
    pub fn select_worker_patch_update(
        &self,
        component_id: &ComponentId,
        current_version: &Version,
        current_index_schema_version: u32,
        target: &TargetTriple,
        protocol_version: u32,
        installed_runtimes: &BTreeMap<ComponentId, Version>,
    ) -> Option<&CatalogArtifact> {
        self.manifest
            .artifacts
            .iter()
            .filter(|artifact| {
                artifact.component_id() == component_id
                    && matches!(artifact.kind(), ArtifactKind::Worker)
                    && artifact.version() > current_version
                    && artifact.version().major == current_version.major
                    && artifact.version().minor == current_version.minor
                    && artifact.version().pre.is_empty()
                    && artifact.compatibility().index_schema_version()
                        == current_index_schema_version
                    && ensure_platform_and_protocol(artifact, target, protocol_version).is_ok()
                    && artifact.compatibility().runtimes().iter().all(|runtime| {
                        installed_runtimes
                            .get(runtime.component_id())
                            .is_some_and(|version| runtime.version_requirement().matches(version))
                    })
            })
            .max_by(|left, right| left.version().cmp(right.version()))
    }

    /// Authorizes an eligible automatic worker patch update from signed data.
    ///
    /// Returns `None` when no newer patch satisfies the platform, protocol,
    /// runtime, and index-schema constraints. Major, minor, pre-release,
    /// model, and schema-affecting changes are never authorized here.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn worker_patch_update(
        &self,
        component_id: &ComponentId,
        current_version: &Version,
        current_index_schema_version: u32,
        target: &TargetTriple,
        protocol_version: u32,
        installed_runtimes: &BTreeMap<ComponentId, Version>,
        minimum_free_space_reserve_bytes: u64,
    ) -> Option<WorkerPatchUpdate> {
        self.select_worker_patch_update(
            component_id,
            current_version,
            current_index_schema_version,
            target,
            protocol_version,
            installed_runtimes,
        )
        .map(|artifact| {
            WorkerPatchUpdate::new(
                self.revision().clone(),
                component_id.clone(),
                current_version.clone(),
                current_index_schema_version,
                artifact.id().clone(),
                minimum_free_space_reserve_bytes,
            )
        })
    }
}

fn ensure_platform_and_protocol(
    artifact: &CatalogArtifact,
    target: &TargetTriple,
    protocol_version: u32,
) -> Result<(), CatalogError> {
    if artifact
        .compatibility()
        .target()
        .is_some_and(|required| required != target)
    {
        return Err(CatalogError::IncompatibleTarget {
            artifact: artifact.id().clone(),
        });
    }

    if artifact
        .compatibility()
        .protocol()
        .is_some_and(|range| !range.contains(protocol_version))
    {
        return Err(CatalogError::IncompatibleProtocol {
            artifact: artifact.id().clone(),
            protocol_version,
        });
    }
    Ok(())
}

pub(crate) fn ensure_artifact_compatibility(
    artifact: &CatalogArtifact,
    target: &TargetTriple,
    protocol_version: u32,
    installed_runtimes: &BTreeMap<ComponentId, Version>,
) -> Result<(), CatalogError> {
    ensure_platform_and_protocol(artifact, target, protocol_version)?;
    for runtime in artifact.compatibility().runtimes() {
        if !installed_runtimes
            .get(runtime.component_id())
            .is_some_and(|version| runtime.version_requirement().matches(version))
        {
            return Err(CatalogError::IncompatibleRuntime {
                artifact: artifact.id().clone(),
            });
        }
    }
    Ok(())
}

/// One signed component's consent disclosure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDisclosure {
    artifact_id: ArtifactId,
    component_id: ComponentId,
    kind: ArtifactKind,
    version: Version,
    license: LicenseInfo,
    resources: ComponentResources,
}

impl From<CatalogArtifact> for ComponentDisclosure {
    fn from(artifact: CatalogArtifact) -> Self {
        Self {
            artifact_id: artifact.id,
            component_id: artifact.component_id,
            kind: artifact.kind,
            version: artifact.version,
            license: artifact.license,
            resources: artifact.resources,
        }
    }
}

impl ComponentDisclosure {
    /// Returns the opaque artifact identifier.
    #[must_use]
    pub const fn artifact_id(&self) -> &ArtifactId {
        &self.artifact_id
    }

    /// Returns the logical component identifier.
    #[must_use]
    pub const fn component_id(&self) -> &ComponentId {
        &self.component_id
    }

    /// Returns the component role.
    #[must_use]
    pub const fn kind(&self) -> &ArtifactKind {
        &self.kind
    }

    /// Returns the exact component version.
    #[must_use]
    pub const fn version(&self) -> &Version {
        &self.version
    }

    /// Returns the component license.
    #[must_use]
    pub const fn license(&self) -> &LicenseInfo {
        &self.license
    }

    /// Returns the compressed download size.
    #[must_use]
    pub const fn download_bytes(&self) -> u64 {
        self.resources.download_bytes()
    }

    /// Returns the estimated installed size.
    #[must_use]
    pub const fn estimated_installed_bytes(&self) -> u64 {
        self.resources.installed_bytes()
    }

    /// Returns the estimated peak RAM use.
    #[must_use]
    pub const fn estimated_ram_bytes(&self) -> u64 {
        self.resources.ram_bytes()
    }
}

/// Stable local-only privacy disclosure for semantic setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalOnlyDisclosure;

impl LocalOnlyDisclosure {
    /// Reports that embedding inference and semantic data remain local.
    #[must_use]
    pub const fn embeddings_stay_local(self) -> bool {
        true
    }

    /// Returns human-readable local-only disclosure text.
    #[must_use]
    pub const fn text(self) -> &'static str {
        "Embedding inference and semantic index data stay on this device."
    }
}

/// Complete, signed first-install disclosure awaiting explicit consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationOffer {
    catalog_revision: ManifestRevision,
    profile: SemanticProfile,
    resolved_model: ModelIdentity,
    components: Vec<ComponentDisclosure>,
    local_only_disclosure: LocalOnlyDisclosure,
    semantic_data_root: PathBuf,
    minimum_free_space_reserve_bytes: u64,
}

impl InstallationOffer {
    /// Returns the signed catalog revision behind the offer.
    #[must_use]
    pub const fn catalog_revision(&self) -> &ManifestRevision {
        &self.catalog_revision
    }

    /// Returns the user's abstract profile.
    #[must_use]
    pub const fn profile(&self) -> SemanticProfile {
        self.profile
    }

    /// Returns the exact immutable model selected by the signed catalog.
    #[must_use]
    pub const fn resolved_model(&self) -> &ModelIdentity {
        &self.resolved_model
    }

    /// Returns every component displayed for consent.
    #[must_use]
    pub fn components(&self) -> &[ComponentDisclosure] {
        &self.components
    }

    /// Returns the local-only privacy disclosure.
    #[must_use]
    pub const fn local_only_disclosure(&self) -> LocalOnlyDisclosure {
        self.local_only_disclosure
    }

    /// Returns the proposed semantic-data location.
    #[must_use]
    pub fn semantic_data_root(&self) -> &Path {
        &self.semantic_data_root
    }

    /// Returns bytes that must remain free in addition to installed-size estimates.
    #[must_use]
    pub const fn minimum_free_space_reserve_bytes(&self) -> u64 {
        self.minimum_free_space_reserve_bytes
    }

    /// Records explicit user consent for exactly this signed offer.
    #[must_use]
    pub fn consent(self) -> InstallationConsent {
        InstallationConsent::from_offer(self)
    }

    pub(crate) fn into_installation_parts(
        self,
    ) -> (
        ManifestRevision,
        SemanticProfile,
        ModelIdentity,
        Vec<ArtifactId>,
        PathBuf,
        u64,
    ) {
        (
            self.catalog_revision,
            self.profile,
            self.resolved_model,
            self.components
                .into_iter()
                .map(|component| component.artifact_id)
                .collect(),
            self.semantic_data_root,
            self.minimum_free_space_reserve_bytes,
        )
    }
}

/// Signed catalog validation failure.
#[derive(Debug, Error)]
pub enum CatalogError {
    /// The signed manifest uses a format this build does not understand.
    #[error("unsupported semantic catalog format version {version}")]
    UnsupportedFormatVersion {
        /// Unsupported format number.
        version: u32,
    },
    /// An opaque identifier was empty or unsafe.
    #[error("{field} must be a portable opaque identifier")]
    InvalidIdentifier {
        /// Name of the invalid field.
        field: &'static str,
    },
    /// Distinct signed identifiers collapse to one path on common filesystems.
    #[error("{field} values collide after portable filesystem normalization")]
    FilesystemIdentifierCollision {
        /// Colliding identifier class.
        field: &'static str,
    },
    /// A signed artifact location was not HTTPS.
    #[error("artifact locations in curated manifests must use credential-free HTTPS")]
    InvalidArtifactLocation,
    /// A protocol compatibility range was zero or reversed.
    #[error("invalid protocol range {minimum}..={maximum}")]
    InvalidProtocolRange {
        /// Lower bound supplied.
        minimum: u32,
        /// Upper bound supplied.
        maximum: u32,
    },
    /// One or more artifact resource estimates were zero.
    #[error("component download, installed, and RAM estimates must be non-zero")]
    InvalidResourceEstimate,
    /// An index schema version was zero.
    #[error("index schema version must be non-zero")]
    InvalidSchemaVersion,
    /// Two runtime constraints targeted the same logical component.
    #[error("duplicate runtime requirement for `{}`", component.as_str())]
    DuplicateRuntimeRequirement {
        /// Duplicated runtime component.
        component: ComponentId,
    },
    /// Deserialized model metadata was not in constructor-normalized form.
    #[error("model metadata is not in canonical validated form")]
    NonCanonicalModelMetadata,
    /// A required immutable model metadata field was empty or zero.
    #[error("model metadata field {field:?} is required")]
    MissingModelMetadata {
        /// Missing or invalid field.
        field: ModelField,
    },
    /// Two artifacts shared one opaque identifier.
    #[error("duplicate catalog artifact `{}`", id.as_str())]
    DuplicateArtifact {
        /// Duplicated artifact identifier.
        id: ArtifactId,
    },
    /// Two artifacts ambiguously described one logical component release.
    #[error(
        "duplicate component version `{}` {version}",
        component.as_str()
    )]
    DuplicateComponentVersion {
        /// Ambiguous logical component.
        component: ComponentId,
        /// Ambiguous package version.
        version: Version,
    },
    /// A worker or runtime bundle omitted its platform and architecture.
    #[error("binary artifact `{}` has no platform target", id.as_str())]
    MissingPlatformTarget {
        /// Incomplete artifact.
        id: ArtifactId,
    },
    /// A worker bundle omitted its supported protocol range.
    #[error("worker artifact `{}` has no protocol compatibility", id.as_str())]
    MissingProtocolCompatibility {
        /// Incomplete worker artifact.
        id: ArtifactId,
    },
    /// A declared runtime requirement had no matching runtime artifact.
    #[error(
        "artifact `{}` requires unavailable runtime `{}`",
        artifact.as_str(),
        component.as_str()
    )]
    MissingCompatibleRuntime {
        /// Artifact with the unsatisfied requirement.
        artifact: ArtifactId,
        /// Missing runtime component.
        component: ComponentId,
    },
    /// Two model records shared one exact identity.
    #[error("duplicate model identity")]
    DuplicateModel {
        /// Duplicated model identity.
        identity: ModelIdentity,
    },
    /// Model metadata referred to a missing artifact.
    #[error("model artifact `{}` is absent", id.as_str())]
    MissingModelArtifact {
        /// Missing artifact identifier.
        id: ArtifactId,
    },
    /// A model artifact had no complete model metadata record.
    #[error("model artifact `{}` has no model manifest", id.as_str())]
    MissingModelManifest {
        /// Incomplete model artifact.
        id: ArtifactId,
    },
    /// A model record and its artifact described different embedding spaces.
    #[error("model artifact `{}` does not match its metadata", id.as_str())]
    ModelArtifactMismatch {
        /// Mismatched artifact identifier.
        id: ArtifactId,
    },
    /// A profile resolution referred to an absent model.
    #[error("profile resolves to a model absent from the catalog")]
    UnknownProfileModel {
        /// Missing exact identity.
        identity: ModelIdentity,
    },
    /// The requested abstract profile has no signed resolution.
    #[error("profile {profile:?} has no model resolution in this catalog")]
    UnresolvedProfile {
        /// Unresolved abstract profile.
        profile: SemanticProfile,
    },
    /// An artifact identifier was not present in the trusted catalog.
    #[error("unknown artifact `{}`", id.as_str())]
    UnknownArtifact {
        /// Unknown artifact identifier.
        id: ArtifactId,
    },
    /// A configured worker/runtime base plan tried to select a model directly.
    #[error("model artifact `{}` must be resolved from the selected profile", id.as_str())]
    ModelArtifactInBasePlan {
        /// Invalid configured base artifact.
        id: ArtifactId,
    },
    /// An installation plan selected one signed artifact more than once.
    #[error("installation plan contains duplicate artifact `{}`", id.as_str())]
    DuplicateOfferedArtifact {
        /// Duplicated artifact.
        id: ArtifactId,
    },
    /// An offer selected multiple releases for one logical component.
    #[error("installation plan ambiguously selects component `{}`", component.as_str())]
    AmbiguousComponentSelection {
        /// Ambiguous logical component.
        component: ComponentId,
    },
    /// A selected component does not support the profile model's index schema.
    #[error(
        "artifact `{}` uses index schema {actual}; expected {expected}",
        artifact.as_str()
    )]
    IncompatibleIndexSchema {
        /// Incompatible selected artifact.
        artifact: ArtifactId,
        /// Model/index schema.
        expected: u32,
        /// Artifact schema.
        actual: u32,
    },
    /// A binary artifact targets another platform or architecture.
    #[error("artifact `{}` is incompatible with this platform", artifact.as_str())]
    IncompatibleTarget {
        /// Incompatible artifact.
        artifact: ArtifactId,
    },
    /// A worker artifact cannot speak the host protocol.
    #[error(
        "artifact `{}` is incompatible with protocol version {protocol_version}",
        artifact.as_str()
    )]
    IncompatibleProtocol {
        /// Incompatible artifact.
        artifact: ArtifactId,
        /// Host protocol version.
        protocol_version: u32,
    },
    /// A model or component requires a runtime absent from the installation plan.
    #[error("artifact `{}` has no compatible runtime in the installation plan", artifact.as_str())]
    IncompatibleRuntime {
        /// Incompatible artifact.
        artifact: ArtifactId,
    },
    /// The offer omitted the exact model resolved for its abstract profile.
    #[error("installation offer omits the model resolved for {profile:?}")]
    ProfileModelNotOffered {
        /// Abstract profile whose model is absent.
        profile: SemanticProfile,
    },
    /// Canonical manifest serialization failed.
    #[error("catalog serialization failed: {0}")]
    Serialize(serde_json::Error),
    /// The detached Ed25519 signature did not cover the canonical manifest.
    #[error("catalog signature is invalid")]
    InvalidSignature,
}
