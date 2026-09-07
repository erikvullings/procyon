use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use semver::Version;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ArtifactId, CatalogError, ProductionCatalogManifest, Sha256Digest,
    SignedProductionCatalogManifest, TrustedCatalog,
};

/// Environment variable containing the path to the raw 32-byte release signing seed.
pub const PRODUCTION_SIGNING_KEY_FILE_ENV: &str = "PROCYON_SEMANTIC_CATALOG_SIGNING_KEY_FILE";
/// Compile-time variable containing the trusted public key as 64 lowercase hex digits.
pub const PRODUCTION_VERIFYING_KEY_HEX_ENV: &str = "PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX";
/// Converter pipeline compiled into the production semantic worker.
pub const PRODUCTION_CONVERTER_IDENTITY: &str = "docling-pdf/1036000+baseline/1";
/// Structural chunker compiled into the production semantic worker.
pub const PRODUCTION_CHUNKER_IDENTITY: &str = "structural/2";
/// Logical component identity of the production semantic worker.
pub const PRODUCTION_WORKER_COMPONENT_ID: &str = "procyon.semantic.worker";
/// Logical component identity of the production Zvec native runtime.
pub const PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID: &str = "procyon.semantic.zvec-runtime";
/// Logical component identity of the production multilingual model package.
pub const PRODUCTION_MODEL_COMPONENT_ID: &str = "procyon.semantic.model.multilingual-e5-small";
/// Exact upstream model identity selected for the first production profile.
pub const PRODUCTION_MODEL_ID: &str = "intfloat.multilingual-e5-small";
/// Immutable upstream multilingual-E5 revision.
pub const PRODUCTION_MODEL_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
/// Tokenizer identity tied to the pinned multilingual-E5 revision.
pub const PRODUCTION_TOKENIZER_ID: &str = "xlm-roberta-sentencepiece.614241f6";

/// Returns the public production key compiled into a release build.
///
/// The private key is intentionally not accepted by this API and is only read
/// by the isolated signing command.
///
/// # Errors
///
/// Returns a redaction-safe error when the release build did not provide a
/// valid 32-byte public key.
pub fn embedded_production_verifying_key() -> Result<VerifyingKey, ProductionCatalogError> {
    let value = option_env!("PROCYON_SEMANTIC_CATALOG_VERIFYING_KEY_HEX")
        .ok_or(ProductionCatalogError::VerifyingKeyUnavailable)?;
    parse_verifying_key_hex(value)
}

/// Returns the complete semantic pipeline identity trusted by this application build.
#[must_use]
pub fn production_pipeline_identity() -> crate::ProductionPipelineIdentity {
    crate::ProductionPipelineIdentity::new(
        1,
        1,
        PRODUCTION_CONVERTER_IDENTITY,
        PRODUCTION_CHUNKER_IDENTITY,
        crate::TokenizerId::new(PRODUCTION_TOKENIZER_ID)
            .expect("production tokenizer constant is valid"),
        crate::ModelIdentity::new(
            crate::ModelId::new(PRODUCTION_MODEL_ID).expect("production model constant is valid"),
            crate::ModelRevision::new(PRODUCTION_MODEL_REVISION)
                .expect("production model revision constant is valid"),
        ),
    )
    .expect("production pipeline constants are valid")
}

/// Derives an immutable content-addressed production artifact identifier.
///
/// # Errors
///
/// Returns a catalog identifier error if any supplied identity is unsafe.
pub fn production_artifact_id(
    component_id: &crate::ComponentId,
    target: Option<&crate::TargetTriple>,
    version: &Version,
    checksum: Sha256Digest,
) -> Result<ArtifactId, CatalogError> {
    let target = target.map_or_else(String::new, |target| {
        format!(".{}-{}", target.operating_system(), target.architecture())
    });
    let version = version.to_string().replace(['-', '+'], ".");
    let digest =
        checksum
            .as_bytes()
            .iter()
            .take(8)
            .fold(String::with_capacity(16), |mut output, byte| {
                use std::fmt::Write as _;
                let _ = write!(output, "{byte:02x}");
                output
            });
    ArtifactId::new(format!(
        "{}{target}.{version}.{digest}",
        component_id.as_str()
    ))
}

/// Loads the release signing key without exposing its path or contents in errors.
///
/// # Errors
///
/// Returns a redaction-safe error when the variable is absent, the file cannot
/// be read, or it does not contain exactly one Ed25519 signing seed.
pub fn load_production_signing_key() -> Result<SigningKey, ProductionCatalogError> {
    let path = std::env::var_os(PRODUCTION_SIGNING_KEY_FILE_ENV)
        .ok_or(ProductionCatalogError::SigningKeyUnavailable)?;
    let bytes = fs::read(path).map_err(|_| ProductionCatalogError::SigningKeyUnavailable)?;
    parse_signing_key(bytes)
}

fn parse_signing_key(bytes: Vec<u8>) -> Result<SigningKey, ProductionCatalogError> {
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ProductionCatalogError::InvalidSigningKey)?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn parse_verifying_key_hex(value: &str) -> Result<VerifyingKey, ProductionCatalogError> {
    if value.len() != 64 {
        return Err(ProductionCatalogError::InvalidVerifyingKey);
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let pair =
            std::str::from_utf8(pair).map_err(|_| ProductionCatalogError::InvalidVerifyingKey)?;
        bytes[index] = u8::from_str_radix(pair, 16)
            .map_err(|_| ProductionCatalogError::InvalidVerifyingKey)?;
    }
    VerifyingKey::from_bytes(&bytes).map_err(|_| ProductionCatalogError::InvalidVerifyingKey)
}

/// Verifies that the artifact directory exactly matches the signed catalog.
///
/// Payloads are named by their opaque artifact ID. The function rejects
/// missing, extra, non-file, truncated, oversized, or checksum-mismatched
/// payloads before a catalog can be signed.
///
/// # Errors
///
/// Returns a typed production catalog error for any mismatch.
pub fn verify_production_payloads(
    manifest: &ProductionCatalogManifest,
    artifacts_directory: &Path,
) -> Result<(), ProductionCatalogError> {
    manifest.validate()?;
    let expected: BTreeSet<&ArtifactId> = manifest
        .catalog()
        .artifacts()
        .iter()
        .map(crate::CatalogArtifact::id)
        .collect();
    let mut found = BTreeSet::new();
    let entries = fs::read_dir(artifacts_directory)
        .map_err(|_| ProductionCatalogError::ArtifactDirectoryUnavailable)?;
    for entry in entries {
        let entry = entry.map_err(|_| ProductionCatalogError::ArtifactDirectoryUnavailable)?;
        let name = entry.file_name().into_string().map_err(|_| {
            ProductionCatalogError::UnknownPayload {
                name: "<non-utf8>".to_owned(),
            }
        })?;
        let Some(artifact) = expected.iter().find(|artifact| artifact.as_str() == name) else {
            return Err(ProductionCatalogError::UnknownPayload { name });
        };
        let file_type = entry
            .file_type()
            .map_err(|_| ProductionCatalogError::ArtifactDirectoryUnavailable)?;
        if !file_type.is_file() {
            return Err(ProductionCatalogError::InvalidPayloadType {
                artifact: (*artifact).clone(),
            });
        }
        found.insert((*artifact).clone());
    }

    for artifact in manifest.catalog().artifacts() {
        if !found.contains(artifact.id()) {
            return Err(ProductionCatalogError::MissingPayload {
                artifact: artifact.id().clone(),
            });
        }
        let path = artifacts_directory.join(artifact.id().as_str());
        let metadata = fs::metadata(&path).map_err(|_| ProductionCatalogError::MissingPayload {
            artifact: artifact.id().clone(),
        })?;
        let expected_bytes = artifact.resources().download_bytes();
        if metadata.len() != expected_bytes {
            return Err(ProductionCatalogError::PayloadSizeMismatch {
                artifact: artifact.id().clone(),
                expected_bytes,
                actual_bytes: metadata.len(),
            });
        }
        if digest_file(&path)? != artifact.checksum() {
            return Err(ProductionCatalogError::PayloadChecksumMismatch {
                artifact: artifact.id().clone(),
            });
        }
    }
    Ok(())
}

/// Signs a validated production catalog using an externally supplied key.
///
/// # Errors
///
/// Returns a typed catalog error if signed data is internally inconsistent.
pub fn sign_production_catalog(
    manifest: ProductionCatalogManifest,
    signing_key: &SigningKey,
) -> Result<SignedProductionCatalogManifest, ProductionCatalogError> {
    manifest.validate()?;
    let signature = signing_key.sign(&manifest.canonical_bytes()?);
    Ok(SignedProductionCatalogManifest::new(
        manifest,
        signature.to_bytes(),
    ))
}

/// Writes deterministic catalog JSON and its raw detached signature.
///
/// # Errors
///
/// Returns a typed serialization or output error.
pub fn write_signed_production_catalog(
    signed: &SignedProductionCatalogManifest,
    output_directory: &Path,
) -> Result<(), ProductionCatalogError> {
    fs::create_dir_all(output_directory).map_err(|_| ProductionCatalogError::OutputUnavailable)?;
    let mut json = serde_json::to_vec_pretty(signed.manifest())?;
    json.push(b'\n');
    fs::write(output_directory.join("catalog.json"), json)
        .map_err(|_| ProductionCatalogError::OutputUnavailable)?;
    fs::write(output_directory.join("catalog.sig"), signed.signature())
        .map_err(|_| ProductionCatalogError::OutputUnavailable)?;
    Ok(())
}

/// Verifies a detached production catalog signature with a public key only.
///
/// # Errors
///
/// Returns a typed catalog error for malformed JSON, a malformed signature,
/// an invalid signature, or invalid signed data.
pub fn verify_serialized_production_catalog(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    verifying_key: &VerifyingKey,
) -> Result<TrustedCatalog, ProductionCatalogError> {
    let manifest: ProductionCatalogManifest = serde_json::from_slice(manifest_bytes)?;
    let signature: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| ProductionCatalogError::InvalidSignatureEncoding)?;
    TrustedCatalog::verify_production(
        SignedProductionCatalogManifest::new(manifest, signature),
        verifying_key,
    )
    .map_err(Into::into)
}

fn digest_file(path: &Path) -> Result<Sha256Digest, ProductionCatalogError> {
    let mut file = fs::File::open(path).map_err(|_| ProductionCatalogError::PayloadReadFailed)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ProductionCatalogError::PayloadReadFailed)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(Sha256Digest::from_bytes(digest.finalize().into()))
}

/// Production catalog assembly, signing, or verification failure.
#[derive(Debug, Error)]
pub enum ProductionCatalogError {
    /// The catalog contract itself was invalid.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// The manifest was not valid JSON.
    #[error("production catalog JSON is invalid")]
    InvalidJson(#[from] serde_json::Error),
    /// The release signing credential was absent or unreadable.
    #[error("production catalog signing key is unavailable")]
    SigningKeyUnavailable,
    /// The release signing credential was not one raw Ed25519 seed.
    #[error("production catalog signing key must contain exactly 32 bytes")]
    InvalidSigningKey,
    /// No trusted public key was compiled into this build.
    #[error("production catalog verifying key is unavailable")]
    VerifyingKeyUnavailable,
    /// The configured public key was not a valid Ed25519 key.
    #[error("production catalog verifying key is invalid")]
    InvalidVerifyingKey,
    /// The detached signature had the wrong length.
    #[error("production catalog signature must contain exactly 64 bytes")]
    InvalidSignatureEncoding,
    /// The artifact directory could not be enumerated.
    #[error("production artifact directory is unavailable")]
    ArtifactDirectoryUnavailable,
    /// An artifact required by the catalog was absent.
    #[error("production artifact `{}` is missing", artifact.as_str())]
    MissingPayload {
        /// Missing artifact.
        artifact: ArtifactId,
    },
    /// The artifact directory contained a payload absent from the catalog.
    #[error("unknown production payload `{name}`")]
    UnknownPayload {
        /// Unknown file name.
        name: String,
    },
    /// A payload path was not a regular file.
    #[error("production artifact `{}` is not a regular file", artifact.as_str())]
    InvalidPayloadType {
        /// Invalid artifact.
        artifact: ArtifactId,
    },
    /// A payload length disagreed with the signed catalog.
    #[error(
        "production artifact `{}` has {actual_bytes} bytes; expected {expected_bytes}",
        artifact.as_str()
    )]
    PayloadSizeMismatch {
        /// Invalid artifact.
        artifact: ArtifactId,
        /// Signed exact length.
        expected_bytes: u64,
        /// Observed exact length.
        actual_bytes: u64,
    },
    /// A payload checksum disagreed with the signed catalog.
    #[error("production artifact `{}` failed SHA-256 verification", artifact.as_str())]
    PayloadChecksumMismatch {
        /// Invalid artifact.
        artifact: ArtifactId,
    },
    /// A payload could not be read.
    #[error("production payload could not be read")]
    PayloadReadFailed,
    /// Deterministic output could not be written.
    #[error("production catalog output is unavailable")]
    OutputUnavailable,
}

impl From<io::Error> for ProductionCatalogError {
    fn from(_value: io::Error) -> Self {
        Self::PayloadReadFailed
    }
}

#[cfg(test)]
mod tests {
    use super::{ProductionCatalogError, parse_signing_key, parse_verifying_key_hex};

    #[test]
    fn signing_key_parse_errors_never_echo_key_material() {
        let secret = b"release-key-material-that-must-not-appear".to_vec();
        let error = parse_signing_key(secret.clone()).expect_err("wrong length");
        let rendered = error.to_string();

        assert!(matches!(error, ProductionCatalogError::InvalidSigningKey));
        assert!(!rendered.contains(std::str::from_utf8(&secret).unwrap()));
    }

    #[test]
    fn verifying_key_parse_errors_never_echo_configured_material() {
        let configured = "not-a-public-key";
        let error = parse_verifying_key_hex(configured).expect_err("invalid public key");

        assert!(matches!(error, ProductionCatalogError::InvalidVerifyingKey));
        assert!(!error.to_string().contains(configured));
    }
}
