use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use semver::VersionReq;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ComponentId, EmbeddingNormalization, LicenseInfo, ModelId, ModelIdentity, ModelMetadata,
    ModelRevision, RuntimeCompatibility, Sha256Digest, TokenizerId,
};

/// Raw expert-mode metadata supplied for a local model file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelImportRequest {
    /// Existing local model package.
    pub source_path: PathBuf,
    /// Opaque model identifier.
    pub model_id: String,
    /// Exact immutable upstream revision.
    pub upstream_revision: String,
    /// SPDX license expression.
    pub license_spdx: String,
    /// Human-readable license notice.
    pub license_notice: String,
    /// Exact tokenizer identifier.
    pub tokenizer: String,
    /// Embedding vector dimensions.
    pub dimensions: u32,
    /// Embedding normalization contract.
    pub normalization: Option<EmbeddingNormalization>,
    /// Required logical runtime component.
    pub runtime_component_id: String,
    /// Accepted runtime semantic versions.
    pub runtime_version_requirement: String,
    /// Declared language coverage.
    pub language_coverage: Vec<String>,
    /// Estimated installed disk bytes.
    pub estimated_disk_bytes: u64,
    /// Estimated peak RAM bytes.
    pub estimated_ram_bytes: u64,
}

/// Required expert import field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelImportField {
    /// Local package path.
    SourcePath,
    /// Model identifier.
    ModelId,
    /// Immutable upstream revision.
    UpstreamRevision,
    /// License expression.
    License,
    /// Tokenizer identity.
    Tokenizer,
    /// Embedding dimensions.
    Dimensions,
    /// Embedding normalization.
    Normalization,
    /// Runtime component and version requirement.
    RuntimeCompatibility,
    /// Language coverage.
    LanguageCoverage,
    /// Estimated installed disk bytes.
    EstimatedDiskBytes,
    /// Estimated peak RAM bytes.
    EstimatedRamBytes,
}

/// A validated local model package with no network location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalModelImport {
    source_path: PathBuf,
    metadata: ModelMetadata,
    checksum: Sha256Digest,
    package_bytes: u64,
}

impl LocalModelImport {
    /// Validates every required field and hashes the existing local package.
    ///
    /// # Errors
    ///
    /// Returns a field-specific metadata error or a local filesystem error.
    pub fn validate(request: LocalModelImportRequest) -> Result<Self, ModelImportError> {
        if request.source_path.as_os_str().is_empty() {
            return Err(ModelImportError::MissingMetadata {
                field: ModelImportField::SourcePath,
            });
        }
        require_text(&request.model_id, ModelImportField::ModelId)?;
        require_text(
            &request.upstream_revision,
            ModelImportField::UpstreamRevision,
        )?;
        require_text(&request.license_spdx, ModelImportField::License)?;
        require_text(&request.tokenizer, ModelImportField::Tokenizer)?;
        if request.dimensions == 0 {
            return Err(ModelImportError::MissingMetadata {
                field: ModelImportField::Dimensions,
            });
        }
        let normalization = request
            .normalization
            .ok_or(ModelImportError::MissingMetadata {
                field: ModelImportField::Normalization,
            })?;
        require_text(
            &request.runtime_component_id,
            ModelImportField::RuntimeCompatibility,
        )?;
        require_text(
            &request.runtime_version_requirement,
            ModelImportField::RuntimeCompatibility,
        )?;
        if request.language_coverage.is_empty()
            || request
                .language_coverage
                .iter()
                .any(|language| language.trim().is_empty())
        {
            return Err(ModelImportError::MissingMetadata {
                field: ModelImportField::LanguageCoverage,
            });
        }
        if request.estimated_disk_bytes == 0 {
            return Err(ModelImportError::MissingMetadata {
                field: ModelImportField::EstimatedDiskBytes,
            });
        }
        if request.estimated_ram_bytes == 0 {
            return Err(ModelImportError::MissingMetadata {
                field: ModelImportField::EstimatedRamBytes,
            });
        }

        let identity = ModelIdentity::new(
            ModelId::new(request.model_id).map_err(|_| ModelImportError::InvalidMetadata {
                field: ModelImportField::ModelId,
            })?,
            ModelRevision::new(request.upstream_revision).map_err(|_| {
                ModelImportError::InvalidMetadata {
                    field: ModelImportField::UpstreamRevision,
                }
            })?,
        );
        let license =
            LicenseInfo::new(request.license_spdx, request.license_notice).map_err(|_| {
                ModelImportError::InvalidMetadata {
                    field: ModelImportField::License,
                }
            })?;
        let tokenizer =
            TokenizerId::new(request.tokenizer).map_err(|_| ModelImportError::InvalidMetadata {
                field: ModelImportField::Tokenizer,
            })?;
        let runtime_component = ComponentId::new(request.runtime_component_id).map_err(|_| {
            ModelImportError::InvalidMetadata {
                field: ModelImportField::RuntimeCompatibility,
            }
        })?;
        let runtime_requirement =
            VersionReq::parse(&request.runtime_version_requirement).map_err(|_| {
                ModelImportError::InvalidMetadata {
                    field: ModelImportField::RuntimeCompatibility,
                }
            })?;
        let metadata = ModelMetadata::new(
            identity,
            license,
            tokenizer,
            request.dimensions,
            normalization,
            RuntimeCompatibility::new(runtime_component, runtime_requirement),
            request.language_coverage,
            request.estimated_disk_bytes,
            request.estimated_ram_bytes,
        )
        .map_err(|_| ModelImportError::InvalidMetadata {
            field: ModelImportField::LanguageCoverage,
        })?;
        let (checksum, package_bytes) = hash_file(&request.source_path)?;
        Ok(Self {
            source_path: request.source_path,
            metadata,
            checksum,
            package_bytes,
        })
    }

    /// Returns the local package path.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the validated complete model metadata.
    #[must_use]
    pub const fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }

    /// Returns the local package SHA-256 digest.
    #[must_use]
    pub const fn checksum(&self) -> Sha256Digest {
        self.checksum
    }

    /// Returns the local package size.
    #[must_use]
    pub const fn package_bytes(&self) -> u64 {
        self.package_bytes
    }
}

fn require_text(value: &str, field: ModelImportField) -> Result<(), ModelImportError> {
    if value.trim().is_empty() {
        Err(ModelImportError::MissingMetadata { field })
    } else {
        Ok(())
    }
}

fn hash_file(path: &Path) -> Result<(Sha256Digest, u64), ModelImportError> {
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(ModelImportError::InvalidSource);
    }
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes = bytes
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or(ModelImportError::SourceTooLarge)?;
    }
    Ok((Sha256Digest::from_bytes(digest.finalize().into()), bytes))
}

/// Expert local-model validation failure.
#[derive(Debug, Error)]
pub enum ModelImportError {
    /// A required field was empty or zero.
    #[error("local model metadata field {field:?} is required")]
    MissingMetadata {
        /// Missing field.
        field: ModelImportField,
    },
    /// A required field had invalid syntax.
    #[error("local model metadata field {field:?} is invalid")]
    InvalidMetadata {
        /// Invalid field.
        field: ModelImportField,
    },
    /// The selected source was not a regular file.
    #[error("local model source must be a regular file")]
    InvalidSource,
    /// The local package size overflowed its representation.
    #[error("local model package is too large")]
    SourceTooLarge,
    /// Local package access failed.
    #[error("local model filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
