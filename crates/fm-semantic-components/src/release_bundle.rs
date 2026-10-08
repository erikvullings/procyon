//! Deterministic packer for one target's production semantic release bundle.
//!
//! A production release bundle contains the semantic worker executable, the
//! native Zvec runtime, the pinned multilingual-E5 model package, and any
//! target-specific native inference loader required by that worker. An opt-in
//! builder can also append unprofiled original EmbeddingGemma 2 files. Unlike the
//! developer bundle, it never packs the deterministic token-hashing fixture
//! and every model pack member is verified against its exact known byte length
//! and SHA-256 before it is packed.
//!
//! The output is an *unsigned* [`ProductionCatalogManifest`] written next to
//! its artifacts as `catalog-input.json`; a separate, isolated signing step
//! (see [`crate::sign_production_catalog`]) turns that input into a signed,
//! trusted catalog. Keeping packing and signing apart means the release
//! signing key is never exposed to the (much larger, less audited) packaging
//! surface exercised here.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    ArtifactCompatibility, ArtifactId, ArtifactKind, ArtifactLocation, CatalogArtifact,
    CatalogError, CatalogManifest, ComponentId, ComponentResources, EmbeddingNormalization,
    LicenseInfo, ManifestRevision, ModelId, ModelIdentity, ModelManifest, ModelMetadata, ModelPack,
    ModelPackError, ModelPackKind, ModelPackSpec, ModelRevision, PRODUCTION_MODEL_COMPONENT_ID,
    PRODUCTION_MODEL_ID, PRODUCTION_MODEL_REVISION, PRODUCTION_ONNX_RUNTIME_COMPONENT_ID,
    PRODUCTION_TOKENIZER_ID, PRODUCTION_WORKER_COMPONENT_ID, PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID,
    ProductionArtifactProvenance, ProductionCatalogManifest, ProductionPipelineIdentity,
    ProtocolRange, RuntimeCompatibility, SemanticProfile, Sha256Digest, TargetTriple, TokenizerId,
    production_artifact_id, write_model_pack,
};

/// Exact pinned `zvec-rust` / native Zvec release packed as the runtime artifact.
const PRODUCTION_ZVEC_VERSION: &str = "0.7.0";
/// Credential-free public source of the pinned Zvec native runtime.
const PRODUCTION_ZVEC_SOURCE_URL: &str = "https://github.com/zvec-ai/zvec-rust";
/// Immutable upstream revision recorded for the pinned Zvec release.
const PRODUCTION_ZVEC_SOURCE_REVISION: &str = "733e0bc82e02a0c63202bff594a7f4530520dfd0";
/// Native Zvec revision pinned by the Rust SDK's v0.7.0 submodule.
const PRODUCTION_ZVEC_NATIVE_SOURCE_REVISION: &str = "8321c1314a559fd5f909e92498f43e5194bf9b99";
/// Microsoft ONNX Runtime release used by the Linux x86-64 dynamic loader.
const PRODUCTION_ONNX_RUNTIME_VERSION: &str = "1.28.0";
/// Credential-free public source of the pinned ONNX Runtime release.
const PRODUCTION_ONNX_RUNTIME_SOURCE_URL: &str = "https://github.com/microsoft/onnxruntime";
/// Immutable source revision recorded inside the official ONNX Runtime archive.
const PRODUCTION_ONNX_RUNTIME_SOURCE_REVISION: &str = "da9b5e364c465de65c49d91e696cd6485270757f";
/// Credential-free public source of the pinned multilingual-E5 model.
const PRODUCTION_MODEL_SOURCE_URL: &str = "https://huggingface.co/intfloat/multilingual-e5-small";
/// Upstream repository slug recorded in the packed model's provenance string.
const PRODUCTION_MODEL_SOURCE_REPOSITORY: &str = "intfloat/multilingual-e5-small";
/// Credential-free public source of the Procyon repository itself. Fixed
/// rather than caller-supplied: it is a release-wide fact, not a per-build
/// input, so it cannot silently drift between invocations of the packer.
const PRODUCTION_PROCYON_SOURCE_URL: &str = "https://github.com/erikvullings/procyon";

const PRODUCTION_MODEL_DIMENSIONS: u32 = 384;
const PRODUCTION_MODEL_MAX_INPUT_TOKENS: u32 = 512;
const PRODUCTION_MODEL_QUERY_PREFIX: &str = "query: ";
const PRODUCTION_MODEL_PASSAGE_PREFIX: &str = "passage: ";
/// Conservative peak resident bytes while loading and running the graph.
const PRODUCTION_MODEL_RAM_BYTES: u64 = 1_600 * 1024 * 1024;
/// Languages the upstream model card lists first; the pack covers many more.
const PRODUCTION_MODEL_LANGUAGES: [&str; 12] = [
    "ar", "de", "en", "es", "fr", "hi", "it", "ja", "nl", "pt", "ru", "zh",
];

/// Conservative peak resident bytes for the packaged worker executable.
const WORKER_RAM_BYTES: u64 = 64 * 1024 * 1024;
/// Conservative peak resident bytes for the packaged native Zvec runtime.
const RUNTIME_RAM_BYTES: u64 = 8 * 1024 * 1024;
/// Conservative mapped/runtime overhead for the optional ONNX Runtime loader.
const ONNX_RUNTIME_RAM_BYTES: u64 = 64 * 1024 * 1024;

const WORKER_PROTOCOL_VERSION: u32 = 1;
const INDEX_SCHEMA_VERSION: u32 = 2;

/// Target operating-system/architecture pairs with a qualified, verified
/// Zvec native runtime artifact (see `docs/architecture/zvec-rust-sdk.md`).
///
/// macOS x64 is deliberately absent: `zvec-rust` 0.7.0 has no macOS x64
/// prebuilt artifact, so Procyon must not advertise the component there.
const SUPPORTED_TARGETS: [(&str, &str); 4] = [
    ("macos", "aarch64"),
    ("windows", "x86_64"),
    ("linux", "x86_64"),
    ("linux", "aarch64"),
];

/// One pinned multilingual-E5 model-pack member's exact known byte length
/// and SHA-256, verified before packing so a corrupted or substituted cache
/// can never reach a signed release.
///
/// Every value here must stay in lockstep with `scripts/fetch-semantic-model.mjs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PinnedModelFile {
    name: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const PRODUCTION_MODEL_FILES: [PinnedModelFile; 5] = [
    PinnedModelFile {
        name: "model.onnx",
        bytes: 470_268_510,
        sha256: "ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665",
    },
    PinnedModelFile {
        name: "tokenizer.json",
        bytes: 17_082_730,
        sha256: "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
    },
    PinnedModelFile {
        name: "config.json",
        bytes: 655,
        sha256: "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
    },
    PinnedModelFile {
        name: "tokenizer_config.json",
        bytes: 443,
        sha256: "a1d6bc8734a6f635dc158508bef000f8e2e5a759c7d92f984b2c86e5ff53425b",
    },
    PinnedModelFile {
        name: "special_tokens_map.json",
        bytes: 167,
        sha256: "d05497f1da52c5e09554c0cd874037a083e1dc1b9cfd48034d1c717f1afc07a7",
    },
];

const GEMMA_SOURCE_URL: &str = "https://huggingface.co/google/embeddinggemma-2";
const GEMMA_REVISION: &str = "914f7f89142e33e77833254d9c9b90c3cef7303b";
const GEMMA_MODEL_ID: &str = "google-embeddinggemma-2";
/// Original files consumed by the native loader, not Python SentenceTransformers wrappers.
const GEMMA_MODEL_FILES: [PinnedModelFile; 5] = [
    PinnedModelFile {
        name: "model.safetensors",
        bytes: 1_488_915_288,
        sha256: "197a32965d4b1105faf060417baa899e193fb73cd401f42ec9295234d5553d79",
    },
    PinnedModelFile {
        name: "tokenizer.json",
        bytes: 32_170_510,
        sha256: "4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4",
    },
    PinnedModelFile {
        name: "config.json",
        bytes: 4_455,
        sha256: "b8f1e9931b57fbc054acdb445c41765d55b0074c58d145fa82839941ad1b5bb3",
    },
    PinnedModelFile {
        name: "processor_config.json",
        bytes: 1_788,
        sha256: "168f6a08522f3ce5dea596d94d003af2fd691742d4f41fe1f9d8cce76bfbf69c",
    },
    PinnedModelFile {
        name: "preprocessor_config.json",
        bytes: 511,
        sha256: "ea2ae257e901064abdd98dceb19f2b0da06af600bed15e0f99f5c85c37ee9d78",
    },
];

/// One verified original EmbeddingGemma 2 file for local bundle assembly.
pub struct VerifiedGemmaOriginalFile {
    /// Upstream filename, used by the native loader.
    pub name: &'static str,
    /// Exact pinned byte length.
    pub bytes: u64,
    /// Exact pinned SHA-256 digest.
    pub checksum: Sha256Digest,
}

/// Verifies the five native Gemma inputs against the pinned upstream revision.
///
/// # Errors
///
/// Returns an error if a file is missing, changed, or not a regular file.
pub fn verify_gemma_original_files(
    directory: &Path,
) -> Result<Vec<VerifiedGemmaOriginalFile>, ProductionBundleError> {
    GEMMA_MODEL_FILES
        .iter()
        .map(|file| {
            verify_original_file(directory, file)?;
            Ok(VerifiedGemmaOriginalFile {
                name: file.name,
                bytes: file.bytes,
                checksum: digest_of(&directory.join(file.name))?,
            })
        })
        .collect()
}

/// Explicit inputs to one production semantic release bundle build.
///
/// The builder never accepts a payload size or checksum: every artifact's
/// resource estimate and integrity digest is derived from the exact bytes it
/// packs, so a signed catalog can never disagree with what actually ships.
/// The target is likewise an explicit field, never inferred from the host
/// building the release, so CI's platform intent stays auditable in the
/// invocation itself rather than in whatever machine happened to run it.
///
/// The release version and the Procyon repository's source URL are *not*
/// fields here: both are release-wide facts baked into the packer binary
/// itself (the compiled crate version, and a fixed public repository URL)
/// rather than per-invocation inputs, so they can never drift between two
/// builds of the same release.
#[derive(Debug, Clone)]
pub struct ProductionBundleSpec {
    /// Target operating-system label, e.g. `macos`, `windows`, `linux`.
    pub target_operating_system: String,
    /// Target CPU-architecture label, e.g. `aarch64`, `x86_64`.
    pub target_architecture: String,
    /// Exact Procyon git commit this release was built from.
    pub procyon_revision: String,
    /// Exact structural document-converter pipeline identity, supplied by
    /// the caller because it is owned by `fm-semantic-conversion`.
    pub converter_identity: String,
    /// Exact structural chunker pipeline identity, supplied by the caller
    /// because it is owned by `fm-semantic-conversion`.
    pub chunker_identity: String,
    /// Path to the built semantic worker executable for this target.
    pub worker_executable: PathBuf,
    /// Path to the native Zvec runtime library for this target.
    pub zvec_runtime_library: PathBuf,
    /// Path to the verified ONNX Runtime loader when this target requires one.
    pub onnx_runtime_library: Option<PathBuf>,
    /// Directory containing the verified pinned multilingual-E5 model cache.
    pub model_cache_directory: PathBuf,
    /// Trusted credential-free HTTPS base URL artifacts are distributed from.
    pub release_base_url: ArtifactLocation,
    /// Directory the bundle is written into, atomically and deterministically.
    pub output_directory: PathBuf,
}

/// A production release bundle could not be packed.
#[derive(Debug, Error)]
pub enum ProductionBundleError {
    /// The requested target has no qualified, verified production runtime.
    #[error("production release target {operating_system}/{architecture} is not supported")]
    UnsupportedTarget {
        /// Requested operating-system label.
        operating_system: String,
        /// Requested CPU-architecture label.
        architecture: String,
    },
    /// The semantic worker executable path does not exist or is not a file.
    #[error("production semantic worker executable is unavailable")]
    WorkerUnavailable,
    /// The Zvec native runtime library path does not exist or is not a file.
    #[error("production Zvec native runtime library is unavailable")]
    RuntimeUnavailable,
    /// The runtime path does not use the target platform's loader filename.
    #[error("production Zvec runtime filename is `{actual}`; expected `{expected}`")]
    RuntimeFileNameMismatch {
        /// Exact platform loader filename required by the worker.
        expected: &'static str,
        /// Supplied path's filename.
        actual: String,
    },
    /// Linux x86-64 requires a separately verified ONNX Runtime loader.
    #[error("production ONNX Runtime loader is unavailable")]
    OnnxRuntimeUnavailable,
    /// Targets with a static ONNX Runtime must not receive a dynamic loader.
    #[error("production target does not accept a separate ONNX Runtime loader")]
    UnexpectedOnnxRuntime,
    /// The supplied ONNX Runtime path did not use the pinned source filename.
    #[error("production ONNX Runtime filename is `{actual}`; expected `{expected}`")]
    OnnxRuntimeFileNameMismatch {
        /// Exact filename in the pinned Microsoft release archive.
        expected: &'static str,
        /// Supplied path's filename.
        actual: String,
    },
    /// A pinned model-cache member was absent from the supplied directory.
    #[error("production model cache is missing the pinned member `{name}`")]
    ModelCacheMemberMissing {
        /// Missing pinned member name.
        name: &'static str,
    },
    /// A pinned model-cache member's bytes disagreed with its pinned revision.
    #[error("production model cache member `{name}` does not match its pinned revision")]
    ModelCacheMemberMismatch {
        /// Mismatched pinned member name.
        name: &'static str,
    },
    /// The assembled catalog manifest itself was invalid.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    /// The model package could not be written or re-verified.
    #[error(transparent)]
    ModelPack(#[from] ModelPackError),
    /// A release payload could not be read.
    #[error("production release payload could not be read")]
    PayloadReadFailed,
    /// The deterministic release output could not be written.
    #[error("production release output is unavailable")]
    OutputUnavailable,
}

impl From<io::Error> for ProductionBundleError {
    fn from(_value: io::Error) -> Self {
        Self::PayloadReadFailed
    }
}

/// Packs one target's production semantic release bundle.
///
/// Verifies the worker and Zvec runtime payloads exist, verifies every
/// pinned model-cache member against its exact known byte length and
/// SHA-256, packs the real multilingual-E5 model with its `production` flag
/// set, and writes a deterministic, atomically replaced bundle containing
/// `catalog-input.json` plus `artifacts/<artifact-id>`. The returned
/// manifest is unsigned; sign it with [`crate::sign_production_catalog`].
///
/// # Errors
///
/// Returns [`ProductionBundleError::UnsupportedTarget`] for a target with no
/// qualified production Zvec runtime, an unavailable-payload error when the
/// worker or runtime path does not exist, a model-cache error when a pinned
/// member is missing or does not match its pinned revision, or a typed
/// catalog/model-pack/IO error for any other packaging failure.
pub fn build_production_release_bundle(
    spec: &ProductionBundleSpec,
) -> Result<ProductionCatalogManifest, ProductionBundleError> {
    build_production_release_bundle_with_optional_gemma(spec, None)
}

/// Builds the E5 bundle with optional, unprofiled original Gemma files.
/// The Gemma records cannot change the E5 pipeline or any production profile.
///
/// # Errors
///
/// Returns a model-cache mismatch for any missing, unsafe, or unpinned
/// original file, or an ordinary production bundle error.
pub fn build_production_release_bundle_with_optional_gemma(
    spec: &ProductionBundleSpec,
    gemma_cache_directory: Option<&Path>,
) -> Result<ProductionCatalogManifest, ProductionBundleError> {
    build_bundle_with_optional_gemma(
        spec,
        &PRODUCTION_MODEL_FILES,
        gemma_cache_directory.map(|directory| (directory, GEMMA_MODEL_FILES.as_slice())),
    )
}

#[cfg(test)]
fn build_bundle(
    spec: &ProductionBundleSpec,
    pinned_files: &[PinnedModelFile],
) -> Result<ProductionCatalogManifest, ProductionBundleError> {
    build_bundle_with_optional_gemma(spec, pinned_files, None)
}

fn build_bundle_with_optional_gemma(
    spec: &ProductionBundleSpec,
    pinned_files: &[PinnedModelFile],
    gemma: Option<(&Path, &[PinnedModelFile])>,
) -> Result<ProductionCatalogManifest, ProductionBundleError> {
    let target = supported_target(&spec.target_operating_system, &spec.target_architecture)?;
    let target_label = format!("{}-{}", target.operating_system(), target.architecture());
    let release_version = production_release_version();

    if !spec.worker_executable.is_file() {
        return Err(ProductionBundleError::WorkerUnavailable);
    }
    if !spec.zvec_runtime_library.is_file() {
        return Err(ProductionBundleError::RuntimeUnavailable);
    }
    let expected_runtime_name =
        runtime_library_name(&spec.target_operating_system, &spec.target_architecture)?;
    let actual_runtime_name = spec
        .zvec_runtime_library
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if actual_runtime_name != expected_runtime_name {
        return Err(ProductionBundleError::RuntimeFileNameMismatch {
            expected: expected_runtime_name,
            actual: actual_runtime_name.to_owned(),
        });
    }
    let onnx_runtime_library = match (
        requires_external_onnx_runtime(&spec.target_operating_system, &spec.target_architecture),
        spec.onnx_runtime_library.as_ref(),
    ) {
        (true, Some(library)) if !library.is_file() => {
            return Err(ProductionBundleError::OnnxRuntimeUnavailable);
        }
        (true, Some(library)) => {
            let actual = library
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let expected = onnx_runtime_source_name();
            if actual != expected {
                return Err(ProductionBundleError::OnnxRuntimeFileNameMismatch {
                    expected,
                    actual: actual.to_owned(),
                });
            }
            Some(library)
        }
        (true, None) => return Err(ProductionBundleError::OnnxRuntimeUnavailable),
        (false, Some(_)) => return Err(ProductionBundleError::UnexpectedOnnxRuntime),
        (false, None) => None,
    };
    verify_model_cache(&spec.model_cache_directory, pinned_files)?;
    if let Some((directory, files)) = gemma {
        for file in files {
            verify_original_file(directory, file)?;
        }
    }

    let staging = spec.output_directory.with_extension("building");
    remove_existing(&staging)?;
    fs::create_dir_all(staging.join("artifacts"))?;

    let worker_component = ComponentId::new(PRODUCTION_WORKER_COMPONENT_ID)?;
    let worker_bytes = fs::read(&spec.worker_executable)?;
    let worker_checksum = Sha256Digest::calculate(&worker_bytes);
    let worker_id = production_artifact_id(
        &worker_component,
        Some(&target),
        &release_version,
        worker_checksum,
    )?;
    write_artifact(&staging, &worker_id, &worker_bytes)?;
    preserve_executable_permissions(
        &spec.worker_executable,
        &staging.join("artifacts").join(worker_id.as_str()),
    )?;

    let runtime_component = ComponentId::new(PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID)?;
    let runtime_version =
        Version::parse(PRODUCTION_ZVEC_VERSION).expect("pinned Zvec version literal is valid");
    let runtime_bytes = fs::read(&spec.zvec_runtime_library)?;
    let runtime_checksum = Sha256Digest::calculate(&runtime_bytes);
    let runtime_id = production_artifact_id(
        &runtime_component,
        Some(&target),
        &runtime_version,
        runtime_checksum,
    )?;
    write_artifact(&staging, &runtime_id, &runtime_bytes)?;

    let onnx_runtime = if let Some(library) = onnx_runtime_library {
        let component = ComponentId::new(PRODUCTION_ONNX_RUNTIME_COMPONENT_ID)?;
        let version = Version::parse(PRODUCTION_ONNX_RUNTIME_VERSION)
            .expect("pinned ONNX Runtime version literal is valid");
        let bytes = fs::read(library)?;
        let checksum = Sha256Digest::calculate(&bytes);
        let id = production_artifact_id(&component, Some(&target), &version, checksum)?;
        write_artifact(&staging, &id, &bytes)?;
        Some((component, version, bytes, checksum, id))
    } else {
        None
    };

    let model_component = ComponentId::new(PRODUCTION_MODEL_COMPONENT_ID)?;
    let model_identity = ModelIdentity::new(
        ModelId::new(PRODUCTION_MODEL_ID)?,
        ModelRevision::new(PRODUCTION_MODEL_REVISION)?,
    );
    let tokenizer = TokenizerId::new(PRODUCTION_TOKENIZER_ID)?;
    let staged_pack = staging.join("artifacts").join(".multilingual-model-pack");
    write_model_pack(
        &staged_pack,
        &ModelPackSpec {
            kind: ModelPackKind::OnnxTransformerMeanPool,
            model_id: PRODUCTION_MODEL_ID.into(),
            model_revision: PRODUCTION_MODEL_REVISION.into(),
            tokenizer: PRODUCTION_TOKENIZER_ID.into(),
            dimensions: PRODUCTION_MODEL_DIMENSIONS,
            max_input_tokens: PRODUCTION_MODEL_MAX_INPUT_TOKENS,
            query_prefix: PRODUCTION_MODEL_QUERY_PREFIX.into(),
            passage_prefix: PRODUCTION_MODEL_PASSAGE_PREFIX.into(),
            production: true,
            source: format!(
                "huggingface:{PRODUCTION_MODEL_SOURCE_REPOSITORY}@{PRODUCTION_MODEL_REVISION}"
            ),
            files: pinned_files
                .iter()
                .map(|file| {
                    (
                        file.name.to_owned(),
                        spec.model_cache_directory.join(file.name),
                    )
                })
                .collect(),
        },
    )?;
    verify_packed_model(&staged_pack, pinned_files)?;
    let model_bytes = fs::metadata(&staged_pack)?.len();
    let model_checksum = digest_of(&staged_pack)?;
    let model_id = production_artifact_id(
        &model_component,
        None,
        &Version::new(1, 0, 0),
        model_checksum,
    )?;
    let model_path = staging.join("artifacts").join(model_id.as_str());
    fs::rename(&staged_pack, &model_path)?;

    let model_metadata = ModelMetadata::new(
        model_identity.clone(),
        LicenseInfo::new(
            "MIT",
            "intfloat/multilingual-e5-small, MIT licensed, redistributed unmodified \
             at the pinned upstream revision.",
        )?,
        tokenizer.clone(),
        PRODUCTION_MODEL_DIMENSIONS,
        EmbeddingNormalization::UnitLength,
        RuntimeCompatibility::new(
            runtime_component.clone(),
            VersionReq::parse(&format!("={PRODUCTION_ZVEC_VERSION}"))
                .expect("pinned Zvec version literal is a valid requirement"),
        ),
        PRODUCTION_MODEL_LANGUAGES,
        model_bytes,
        PRODUCTION_MODEL_RAM_BYTES,
    )?;

    let mut worker_runtimes = vec![RuntimeCompatibility::new(
        runtime_component.clone(),
        VersionReq::parse(&format!("={PRODUCTION_ZVEC_VERSION}"))
            .expect("pinned Zvec version literal is a valid requirement"),
    )];
    if let Some((component, version, _, _, _)) = &onnx_runtime {
        worker_runtimes.push(RuntimeCompatibility::new(
            component.clone(),
            VersionReq::parse(&format!("={version}"))
                .expect("pinned ONNX Runtime version is a valid requirement"),
        ));
    }

    let worker_artifact = CatalogArtifact::new(
        worker_id.clone(),
        worker_component,
        ArtifactKind::Worker,
        release_version.clone(),
        distribution_location(&spec.release_base_url, &worker_id)?,
        LicenseInfo::new("MIT", "Procyon semantic worker.")?,
        worker_checksum,
        resources(worker_bytes.len(), WORKER_RAM_BYTES)?,
        ArtifactCompatibility::new(
            Some(target.clone()),
            Some(ProtocolRange::new(
                WORKER_PROTOCOL_VERSION,
                WORKER_PROTOCOL_VERSION,
            )?),
            worker_runtimes,
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let runtime_artifact = CatalogArtifact::new(
        runtime_id.clone(),
        runtime_component.clone(),
        ArtifactKind::Runtime,
        runtime_version,
        distribution_location(&spec.release_base_url, &runtime_id)?,
        LicenseInfo::new(
            "Apache-2.0",
            format!(
                "Zvec Rust SDK and native runtime at pinned commits \
                 {PRODUCTION_ZVEC_SOURCE_REVISION} and \
                 {PRODUCTION_ZVEC_NATIVE_SOURCE_REVISION}. Redistribution preserves the \
                 native NOTICE attributions for the Unicode Character Database and pyglass."
            ),
        )?,
        runtime_checksum,
        resources(runtime_bytes.len(), RUNTIME_RAM_BYTES)?,
        ArtifactCompatibility::new(Some(target.clone()), None, Vec::new(), INDEX_SCHEMA_VERSION),
    )?;
    let onnx_runtime_artifact = onnx_runtime
        .as_ref()
        .map(|(component, version, bytes, checksum, id)| {
            CatalogArtifact::new(
                id.clone(),
                component.clone(),
                ArtifactKind::Runtime,
                version.clone(),
                distribution_location(&spec.release_base_url, id)?,
                LicenseInfo::new(
                    "MIT",
                    format!(
                        "Microsoft ONNX Runtime CPU release {PRODUCTION_ONNX_RUNTIME_VERSION} at \
                         pinned commit {PRODUCTION_ONNX_RUNTIME_SOURCE_REVISION}; the official \
                         archive's ThirdPartyNotices.txt is retained in qualification evidence."
                    ),
                )?,
                *checksum,
                resources(bytes.len(), ONNX_RUNTIME_RAM_BYTES)?,
                ArtifactCompatibility::new(
                    Some(target.clone()),
                    None,
                    Vec::new(),
                    INDEX_SCHEMA_VERSION,
                ),
            )
        })
        .transpose()?;
    let model_artifact = CatalogArtifact::new(
        model_id.clone(),
        model_component,
        ArtifactKind::Model(model_identity.clone()),
        Version::new(1, 0, 0),
        distribution_location(&spec.release_base_url, &model_id)?,
        model_metadata.license().clone(),
        model_checksum,
        ComponentResources::new(model_bytes, model_bytes, PRODUCTION_MODEL_RAM_BYTES)?,
        ArtifactCompatibility::new(
            None,
            None,
            vec![model_metadata.runtime().clone()],
            INDEX_SCHEMA_VERSION,
        ),
    )?;
    let model_manifest = ModelManifest::new(model_id.clone(), model_metadata);
    let mut profiles: BTreeMap<_, _> = SemanticProfile::all()
        .iter()
        .filter(|profile| **profile != SemanticProfile::EmbeddingGemma2)
        .map(|profile| (*profile, model_identity.clone()))
        .collect();

    let mut revision_material = Vec::with_capacity(4 * 32);
    let mut revision_checksums = vec![worker_checksum, runtime_checksum];
    if let Some((_, _, _, checksum, _)) = &onnx_runtime {
        revision_checksums.push(*checksum);
    }
    revision_checksums.push(model_checksum);
    let mut gemma_records = None;
    if let Some((directory, files)) = gemma {
        let records = append_optional_gemma(
            &staging,
            &spec.release_base_url,
            &runtime_component,
            directory,
            files,
        )?;
        revision_checksums.extend(&records.3);
        gemma_records = Some(records);
    }
    for checksum in revision_checksums {
        revision_material.extend_from_slice(checksum.as_bytes());
    }
    let revision_checksum = Sha256Digest::calculate(&revision_material);
    let mut artifacts = vec![worker_artifact, runtime_artifact];
    if let Some(artifact) = onnx_runtime_artifact {
        artifacts.push(artifact);
    }
    artifacts.push(model_artifact);
    let mut models = vec![model_manifest];
    if let Some((extra_artifacts, extra_model, _, _)) = &gemma_records {
        profiles.insert(
            SemanticProfile::EmbeddingGemma2,
            extra_model.metadata().identity().clone(),
        );
        artifacts.extend(extra_artifacts.iter().cloned());
        models.push(extra_model.clone());
    }
    let catalog = CatalogManifest::new(
        ManifestRevision::new(format!(
            "procyon-{target_label}-{}-{}",
            release_version,
            digest_prefix(revision_checksum)
        ))?,
        artifacts,
        models,
        profiles,
    )?;

    let pipeline = ProductionPipelineIdentity::new(
        WORKER_PROTOCOL_VERSION,
        INDEX_SCHEMA_VERSION,
        spec.converter_identity.clone(),
        spec.chunker_identity.clone(),
        crate::PRODUCTION_EMBEDDING_PREPROCESSING_IDENTITY,
        tokenizer,
        model_identity,
    )?;
    let mut provenance = vec![
        ProductionArtifactProvenance::new(
            worker_id,
            ArtifactLocation::new(PRODUCTION_PROCYON_SOURCE_URL)?,
            ManifestRevision::new(spec.procyon_revision.clone())?,
        ),
        ProductionArtifactProvenance::new(
            runtime_id,
            ArtifactLocation::new(PRODUCTION_ZVEC_SOURCE_URL)?,
            ManifestRevision::new(PRODUCTION_ZVEC_SOURCE_REVISION)?,
        ),
    ];
    if let Some((_, _, _, _, id)) = onnx_runtime {
        provenance.push(ProductionArtifactProvenance::new(
            id,
            ArtifactLocation::new(PRODUCTION_ONNX_RUNTIME_SOURCE_URL)?,
            ManifestRevision::new(PRODUCTION_ONNX_RUNTIME_SOURCE_REVISION)?,
        ));
    }
    provenance.push(ProductionArtifactProvenance::new(
        model_id,
        ArtifactLocation::new(PRODUCTION_MODEL_SOURCE_URL)?,
        ManifestRevision::new(PRODUCTION_MODEL_REVISION)?,
    ));
    if let Some((_, _, extra_provenance, _)) = gemma_records {
        provenance.extend(extra_provenance);
    }
    let manifest = ProductionCatalogManifest::new(catalog, pipeline, provenance)?;

    fs::write(
        staging.join("catalog-input.json"),
        serde_json::to_vec_pretty(&manifest).map_err(CatalogError::Serialize)?,
    )?;

    remove_existing(&spec.output_directory)?;
    fs::rename(&staging, &spec.output_directory)?;
    Ok(manifest)
}

type OptionalModelRecords = (
    Vec<CatalogArtifact>,
    ModelManifest,
    Vec<ProductionArtifactProvenance>,
    Vec<Sha256Digest>,
);

fn verify_original_file(
    directory: &Path,
    file: &PinnedModelFile,
) -> Result<(), ProductionBundleError> {
    let path = directory.join(file.name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| ProductionBundleError::ModelCacheMemberMissing { name: file.name })?;
    if !metadata.file_type().is_file() || metadata.len() != file.bytes {
        return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
    }
    let expected = parse_sha256(file.sha256)
        .ok_or(ProductionBundleError::ModelCacheMemberMismatch { name: file.name })?;
    if digest_of(&path)?.as_bytes() != &expected {
        return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
    }
    Ok(())
}

fn append_optional_gemma(
    staging: &Path,
    base_url: &ArtifactLocation,
    runtime_component: &ComponentId,
    directory: &Path,
    files: &[PinnedModelFile],
) -> Result<OptionalModelRecords, ProductionBundleError> {
    let identity = ModelIdentity::new(
        ModelId::new(GEMMA_MODEL_ID)?,
        ModelRevision::new(GEMMA_REVISION)?,
    );
    let license = LicenseInfo::new(
        "Apache-2.0",
        "Original google/embeddinggemma-2 files, unmodified, at the pinned Hugging Face revision.",
    )?;
    let runtime = RuntimeCompatibility::new(
        runtime_component.clone(),
        VersionReq::parse(&format!("={PRODUCTION_ZVEC_VERSION}"))
            .expect("pinned Zvec version literal is a valid requirement"),
    );
    let version = Version::new(1, 0, 0);
    let source = ArtifactLocation::new(GEMMA_SOURCE_URL)?;
    let revision = ManifestRevision::new(GEMMA_REVISION)?;
    let mut artifacts = Vec::with_capacity(files.len());
    let mut provenance = Vec::with_capacity(files.len());
    let mut checksums = Vec::with_capacity(files.len());
    let mut additional = BTreeMap::new();
    let mut primary = None;
    let mut disk_bytes = 0_u64;
    // Provisional disclosure estimate, not CPU qualification.
    const GEMMA_RAM_ESTIMATE: u64 = 8 * 1024 * 1024 * 1024;
    for (index, file) in files.iter().enumerate() {
        let is_primary = file.name == "model.safetensors";
        let component = ComponentId::new(if is_primary {
            "embeddinggemma2-original".to_owned()
        } else {
            format!("embeddinggemma2-file-{index}")
        })?;
        let source_path = directory.join(file.name);
        let checksum = digest_of(&source_path)?;
        let expected = parse_sha256(file.sha256)
            .ok_or(ProductionBundleError::ModelCacheMemberMismatch { name: file.name })?;
        if fs::metadata(&source_path)?.len() != file.bytes || checksum.as_bytes() != &expected {
            return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
        }
        let id = production_artifact_id(&component, None, &version, checksum)?;
        let destination = staging.join("artifacts").join(id.as_str());
        fs::copy(&source_path, &destination)?;
        if fs::metadata(&destination)?.len() != file.bytes || digest_of(&destination)? != checksum {
            return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
        }
        let kind = if is_primary {
            primary = Some(id.clone());
            ArtifactKind::OriginalModel(identity.clone())
        } else {
            additional.insert(file.name.to_owned(), id.clone());
            ArtifactKind::ModelFile(identity.clone())
        };
        disk_bytes = disk_bytes
            .checked_add(file.bytes)
            .ok_or(CatalogError::InvalidResourceEstimate)?;
        artifacts.push(CatalogArtifact::new(
            id.clone(),
            component,
            kind,
            version.clone(),
            distribution_location(base_url, &id)?,
            license.clone(),
            checksum,
            ComponentResources::new(
                file.bytes,
                file.bytes,
                if is_primary { GEMMA_RAM_ESTIMATE } else { 1 },
            )?,
            ArtifactCompatibility::new(None, None, vec![runtime.clone()], INDEX_SCHEMA_VERSION),
        )?);
        provenance.push(ProductionArtifactProvenance::new(
            id,
            source.clone(),
            revision.clone(),
        ));
        checksums.push(checksum);
    }
    let primary = primary.ok_or(ProductionBundleError::ModelCacheMemberMissing {
        name: "model.safetensors",
    })?;
    let metadata = ModelMetadata::new(
        identity,
        license,
        TokenizerId::new("embeddinggemma-2-tokenizer")?,
        768,
        EmbeddingNormalization::UnitLength,
        runtime,
        PRODUCTION_MODEL_LANGUAGES,
        disk_bytes,
        GEMMA_RAM_ESTIMATE,
    )?;
    let model =
        ModelManifest::new(primary, metadata).with_original_files("model.safetensors", additional);
    Ok((artifacts, model, provenance, checksums))
}

/// Procyon's own release version: the packer binary's compiled-in
/// `[workspace.package].version`, not a caller-supplied value, so it can
/// never disagree with the workspace version the packer itself shipped from.
fn production_release_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("workspace package version is valid semver")
}

fn supported_target(
    operating_system: &str,
    architecture: &str,
) -> Result<TargetTriple, ProductionBundleError> {
    if !SUPPORTED_TARGETS
        .iter()
        .any(|(os, arch)| *os == operating_system && *arch == architecture)
    {
        return Err(ProductionBundleError::UnsupportedTarget {
            operating_system: operating_system.to_owned(),
            architecture: architecture.to_owned(),
        });
    }
    Ok(TargetTriple::new(operating_system, architecture)?)
}

fn runtime_library_name(
    operating_system: &str,
    architecture: &str,
) -> Result<&'static str, ProductionBundleError> {
    supported_target(operating_system, architecture)?;
    Ok(match operating_system {
        "macos" => "libzvec_c_api.dylib",
        "windows" => "zvec_c_api.dll",
        _ => "libzvec_c_api.so",
    })
}

fn requires_external_onnx_runtime(operating_system: &str, architecture: &str) -> bool {
    operating_system == "linux" && architecture == "x86_64"
}

fn onnx_runtime_source_name() -> &'static str {
    "libonnxruntime.so.1.28.0"
}

fn distribution_location(
    base: &ArtifactLocation,
    id: &ArtifactId,
) -> Result<ArtifactLocation, CatalogError> {
    ArtifactLocation::new(format!(
        "{}/{}",
        base.as_str().trim_end_matches('/'),
        id.as_str()
    ))
}

fn resources(bytes_len: usize, ram_bytes: u64) -> Result<ComponentResources, CatalogError> {
    let bytes_len = u64::try_from(bytes_len).unwrap_or(u64::MAX);
    ComponentResources::new(bytes_len, bytes_len, ram_bytes)
}

fn write_artifact(root: &Path, id: &ArtifactId, bytes: &[u8]) -> io::Result<()> {
    fs::write(root.join("artifacts").join(id.as_str()), bytes)
}

fn verify_model_cache(
    directory: &Path,
    pinned_files: &[PinnedModelFile],
) -> Result<(), ProductionBundleError> {
    for file in pinned_files {
        verify_pinned_file(directory, file)?;
    }
    Ok(())
}

fn verify_pinned_file(
    directory: &Path,
    file: &PinnedModelFile,
) -> Result<(), ProductionBundleError> {
    let path = directory.join(file.name);
    let metadata = fs::metadata(&path)
        .map_err(|_| ProductionBundleError::ModelCacheMemberMissing { name: file.name })?;
    if metadata.len() != file.bytes {
        return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
    }
    let actual = digest_of(&path)?;
    let expected = parse_sha256(file.sha256)
        .ok_or(ProductionBundleError::ModelCacheMemberMismatch { name: file.name })?;
    if actual.as_bytes() != &expected {
        return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
    }
    Ok(())
}

fn verify_packed_model(
    path: &Path,
    pinned_files: &[PinnedModelFile],
) -> Result<(), ProductionBundleError> {
    let pack = ModelPack::open(path)?;
    for file in pinned_files {
        let member = pack
            .index()
            .file(file.name)
            .ok_or(ProductionBundleError::ModelCacheMemberMissing { name: file.name })?;
        let expected = parse_sha256(file.sha256)
            .ok_or(ProductionBundleError::ModelCacheMemberMismatch { name: file.name })?;
        if member.length != file.bytes || member.checksum.as_bytes() != &expected {
            return Err(ProductionBundleError::ModelCacheMemberMismatch { name: file.name });
        }
        pack.verify_member(file.name)?;
    }
    Ok(())
}

fn digest_prefix(checksum: Sha256Digest) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(32);
    for byte in &checksum.as_bytes()[..16] {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn digest_of(path: &Path) -> io::Result<Sha256Digest> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Sha256Digest::from_bytes(hasher.finalize().into()))
}

fn parse_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let pair = std::str::from_utf8(pair).ok()?;
        bytes[index] = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(bytes)
}

fn remove_existing(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn preserve_executable_permissions(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_GEMMA_FILES: [PinnedModelFile; 5] = [
        PinnedModelFile {
            name: "model.safetensors",
            bytes: 20,
            sha256: "884044ade81696294f79f7c74594fd8cf652973319506d867a2113a9f51e52ff",
        },
        PinnedModelFile {
            name: "tokenizer.json",
            bytes: 23,
            sha256: "6a48c29796788f90f7eb50a8c2dbb60de3a6f4aaa80accf580317b5973cf6749",
        },
        PinnedModelFile {
            name: "config.json",
            bytes: 6,
            sha256: "b79606fb3afea5bd1609ed40b622142f1c98125abcfe89a76a661b0e8e343910",
        },
        PinnedModelFile {
            name: "processor_config.json",
            bytes: 6,
            sha256: "b79606fb3afea5bd1609ed40b622142f1c98125abcfe89a76a661b0e8e343910",
        },
        PinnedModelFile {
            name: "preprocessor_config.json",
            bytes: 6,
            sha256: "b79606fb3afea5bd1609ed40b622142f1c98125abcfe89a76a661b0e8e343910",
        },
    ];

    fn fixture_gemma_cache(directory: &Path) -> PathBuf {
        let cache = directory.join("gemma-cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("model.safetensors"), b"fixture-onnx-bytes-A").unwrap();
        fs::write(cache.join("tokenizer.json"), b"fixture-tokenizer-bytes").unwrap();
        for name in [
            "config.json",
            "processor_config.json",
            "preprocessor_config.json",
        ] {
            fs::write(cache.join(name), b"config").unwrap();
        }
        cache
    }

    #[test]
    fn optional_gemma_preserves_e5_pipeline_and_emits_original_signed_files() {
        use ed25519_dalek::SigningKey;
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        let cache = fixture_gemma_cache(directory.path());
        let e5_only = build(&spec);
        let manifest = build_bundle_with_optional_gemma(
            &spec,
            &FIXTURE_MODEL_FILES,
            Some((&cache, &FIXTURE_GEMMA_FILES)),
        )
        .unwrap();
        assert_ne!(manifest.catalog().revision(), e5_only.catalog().revision());
        assert_eq!(
            manifest.pipeline().model().model_id().as_str(),
            PRODUCTION_MODEL_ID
        );
        let gemma = manifest
            .catalog()
            .models()
            .iter()
            .find(|model| model.primary_file_name() == Some("model.safetensors"))
            .unwrap();
        assert_eq!(gemma.files().len(), 4);
        assert_eq!(gemma.metadata().estimated_disk_bytes(), 20 + 23 + 6 * 3);
        for (name, id) in std::iter::once(("model.safetensors", gemma.artifact_id()))
            .chain(gemma.files().iter().map(|(name, id)| (name.as_str(), id)))
        {
            let artifact = manifest
                .catalog()
                .artifacts()
                .iter()
                .find(|artifact| artifact.id() == id)
                .unwrap();
            let bytes =
                fs::read(spec.output_directory.join("artifacts").join(id.as_str())).unwrap();
            assert_eq!(bytes, fs::read(cache.join(name)).unwrap());
            assert_eq!(artifact.checksum(), Sha256Digest::calculate(&bytes));
            assert_eq!(artifact.resources().download_bytes(), bytes.len() as u64);
            assert!(
                matches!(
                    artifact.kind(),
                    ArtifactKind::OriginalModel(_) if name == "model.safetensors"
                ) || matches!(
                    artifact.kind(),
                    ArtifactKind::ModelFile(_) if name != "model.safetensors"
                )
            );
            assert_eq!(
                manifest
                    .provenance()
                    .iter()
                    .find(|record| record.artifact_id() == id)
                    .unwrap()
                    .source_revision()
                    .as_str(),
                GEMMA_REVISION
            );
        }
        let key = SigningKey::from_bytes(&[0x42; 32]);
        let trusted = crate::TrustedCatalog::verify_production(
            crate::sign_production_catalog(manifest, &key).unwrap(),
            &key.verifying_key(),
        )
        .unwrap();
        assert_eq!(
            trusted
                .resolve_profile(SemanticProfile::EmbeddingGemma2)
                .unwrap()
                .model_id()
                .as_str(),
            GEMMA_MODEL_ID
        );
        for profile in SemanticProfile::all() {
            if *profile == SemanticProfile::EmbeddingGemma2 {
                continue;
            }
            assert_eq!(
                trusted
                    .resolve_profile(*profile)
                    .unwrap()
                    .model_id()
                    .as_str(),
                PRODUCTION_MODEL_ID
            );
        }
    }

    #[test]
    fn optional_gemma_requires_every_pinned_original_before_replacing_output() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        let cache = fixture_gemma_cache(directory.path());
        let original = build(&spec);
        let original_catalog = fs::read(spec.output_directory.join("catalog-input.json")).unwrap();

        fs::remove_file(cache.join("processor_config.json")).unwrap();
        assert!(matches!(
            build_bundle_with_optional_gemma(
                &spec,
                &FIXTURE_MODEL_FILES,
                Some((&cache, &FIXTURE_GEMMA_FILES))
            ),
            Err(ProductionBundleError::ModelCacheMemberMissing {
                name: "processor_config.json"
            })
        ));
        fs::write(cache.join("processor_config.json"), b"config").unwrap();
        fs::write(cache.join("model.safetensors"), b"fixture-onnx-bytes-B").unwrap();
        assert!(matches!(
            build_bundle_with_optional_gemma(
                &spec,
                &FIXTURE_MODEL_FILES,
                Some((&cache, &FIXTURE_GEMMA_FILES))
            ),
            Err(ProductionBundleError::ModelCacheMemberMismatch {
                name: "model.safetensors"
            })
        ));
        assert_eq!(
            fs::read(spec.output_directory.join("catalog-input.json")).unwrap(),
            original_catalog
        );
        assert_eq!(original.catalog().models().len(), 1);
    }

    #[test]
    fn original_model_file_symlink_is_not_a_release_source() {
        #[cfg(unix)]
        {
            let directory = tempfile::tempdir().unwrap();
            let spec = base_spec(directory.path());
            let cache = fixture_gemma_cache(directory.path());
            let original = cache.join("tokenizer.json");
            fs::rename(&original, cache.join("actual-tokenizer")).unwrap();
            std::os::unix::fs::symlink(cache.join("actual-tokenizer"), original).unwrap();
            assert!(matches!(
                build_bundle_with_optional_gemma(
                    &spec,
                    &FIXTURE_MODEL_FILES,
                    Some((&cache, &FIXTURE_GEMMA_FILES))
                ),
                Err(ProductionBundleError::ModelCacheMemberMismatch {
                    name: "tokenizer.json"
                })
            ));
            assert!(!spec.output_directory.exists());
        }
    }

    #[test]
    fn native_gemma_release_pins_only_required_flat_upstream_files() {
        assert_eq!(GEMMA_REVISION, "914f7f89142e33e77833254d9c9b90c3cef7303b");
        assert_eq!(
            GEMMA_MODEL_FILES.map(|file| (file.name, file.bytes, file.sha256)),
            [
                (
                    "model.safetensors",
                    1_488_915_288,
                    "197a32965d4b1105faf060417baa899e193fb73cd401f42ec9295234d5553d79"
                ),
                (
                    "tokenizer.json",
                    32_170_510,
                    "4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4"
                ),
                (
                    "config.json",
                    4_455,
                    "b8f1e9931b57fbc054acdb445c41765d55b0074c58d145fa82839941ad1b5bb3"
                ),
                (
                    "processor_config.json",
                    1_788,
                    "168f6a08522f3ce5dea596d94d003af2fd691742d4f41fe1f9d8cce76bfbf69c"
                ),
                (
                    "preprocessor_config.json",
                    511,
                    "ea2ae257e901064abdd98dceb19f2b0da06af600bed15e0f99f5c85c37ee9d78"
                ),
            ]
        );
    }

    /// Tiny stand-in pinned member descriptors so tests never touch the real
    /// 465 MiB multilingual model. Byte lengths and digests below are exact
    /// for the fixture content the tests themselves write.
    const FIXTURE_MODEL_FILES: [PinnedModelFile; 2] = [
        PinnedModelFile {
            name: "model.onnx",
            bytes: 20,
            sha256: "884044ade81696294f79f7c74594fd8cf652973319506d867a2113a9f51e52ff",
        },
        PinnedModelFile {
            name: "tokenizer.json",
            bytes: 23,
            sha256: "6a48c29796788f90f7eb50a8c2dbb60de3a6f4aaa80accf580317b5973cf6749",
        },
    ];

    fn write_fixture_model_cache(directory: &Path) -> PathBuf {
        let cache = directory.join("model-cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("model.onnx"), b"fixture-onnx-bytes-A").unwrap();
        fs::write(cache.join("tokenizer.json"), b"fixture-tokenizer-bytes").unwrap();
        cache
    }

    fn base_spec(directory: &Path) -> ProductionBundleSpec {
        let worker = directory.join("fm-semantic-worker");
        let runtime = directory.join("libzvec_c_api.so");
        let onnx_runtime = directory.join(onnx_runtime_source_name());
        fs::write(&worker, b"production worker payload").unwrap();
        fs::write(&runtime, b"production zvec runtime payload").unwrap();
        fs::write(&onnx_runtime, b"production onnx runtime payload").unwrap();
        ProductionBundleSpec {
            target_operating_system: "linux".into(),
            target_architecture: "x86_64".into(),
            procyon_revision: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
            converter_identity: "docling-pdf/1.36.0+baseline/1".into(),
            chunker_identity: "structural/2".into(),
            worker_executable: worker,
            zvec_runtime_library: runtime,
            onnx_runtime_library: Some(onnx_runtime),
            model_cache_directory: write_fixture_model_cache(directory),
            release_base_url: ArtifactLocation::new("https://cdn.example.com/procyon/releases")
                .unwrap(),
            output_directory: directory.join("bundle"),
        }
    }

    fn build(spec: &ProductionBundleSpec) -> ProductionCatalogManifest {
        build_bundle(spec, &FIXTURE_MODEL_FILES).expect("bundle build succeeds")
    }

    #[test]
    fn fixture_digests_match_fixture_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let cache = write_fixture_model_cache(directory.path());
        for file in FIXTURE_MODEL_FILES {
            let path = cache.join(file.name);
            assert_eq!(fs::metadata(&path).unwrap().len(), file.bytes);
            assert_eq!(
                digest_of(&path).unwrap().as_bytes(),
                &parse_sha256(file.sha256).unwrap()
            );
        }
    }

    #[test]
    fn builds_a_deterministic_byte_identical_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        build(&spec);
        let first_catalog = fs::read(spec.output_directory.join("catalog-input.json")).unwrap();
        let first_artifacts = list_artifact_bytes(&spec.output_directory);

        fs::remove_dir_all(&spec.output_directory).unwrap();
        build(&spec);
        let second_catalog = fs::read(spec.output_directory.join("catalog-input.json")).unwrap();
        let second_artifacts = list_artifact_bytes(&spec.output_directory);

        assert_eq!(first_catalog, second_catalog);
        assert_eq!(first_artifacts, second_artifacts);
    }

    fn list_artifact_bytes(output: &Path) -> Vec<(String, Vec<u8>)> {
        let mut entries: Vec<(String, Vec<u8>)> = fs::read_dir(output.join("artifacts"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let name = entry.file_name().into_string().unwrap();
                let bytes = fs::read(entry.path()).unwrap();
                (name, bytes)
            })
            .collect();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    #[test]
    fn artifact_ids_change_when_payload_bytes_change() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = base_spec(directory.path());
        let first = build(&spec);
        let first_worker_id = first
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .unwrap()
            .id()
            .clone();

        fs::write(
            &spec.worker_executable,
            b"a different production worker payload",
        )
        .unwrap();
        spec.output_directory = directory.path().join("bundle-2");
        let second = build(&spec);
        let second_worker_id = second
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .unwrap()
            .id()
            .clone();

        assert_ne!(first_worker_id, second_worker_id);
        assert_ne!(first.catalog().revision(), second.catalog().revision());
    }

    #[test]
    fn packs_the_model_with_the_production_marker_and_verified_member_checksums() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        let manifest = build(&spec);

        let model_artifact = manifest
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Model(_)))
            .unwrap();
        let pack_path = spec
            .output_directory
            .join("artifacts")
            .join(model_artifact.id().as_str());
        let pack = ModelPack::open(&pack_path).unwrap();
        assert!(pack.index().production);
        assert_eq!(pack.index().kind, ModelPackKind::OnnxTransformerMeanPool);
        assert_eq!(pack.index().model_id, PRODUCTION_MODEL_ID);
        assert_eq!(pack.index().model_revision, PRODUCTION_MODEL_REVISION);
        assert_eq!(pack.index().tokenizer, PRODUCTION_TOKENIZER_ID);
        for file in FIXTURE_MODEL_FILES {
            pack.verify_member(file.name)
                .expect("member checksum verifies");
        }
    }

    #[test]
    fn records_exact_pipeline_metadata_and_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        let manifest = build(&spec);

        assert_eq!(manifest.pipeline().worker_protocol_version(), 1);
        assert_eq!(manifest.pipeline().index_schema_version(), 2);
        assert_eq!(manifest.pipeline().converter(), spec.converter_identity);
        assert_eq!(manifest.pipeline().chunker(), spec.chunker_identity);
        assert_eq!(
            manifest.pipeline().tokenizer().as_str(),
            PRODUCTION_TOKENIZER_ID
        );
        assert_eq!(
            manifest.pipeline().model().model_id().as_str(),
            PRODUCTION_MODEL_ID
        );
        assert_eq!(
            manifest.pipeline().model().revision().as_str(),
            PRODUCTION_MODEL_REVISION
        );

        let provenance = manifest.provenance();
        assert_eq!(provenance.len(), 4);
        let worker_artifact = manifest
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
            .unwrap();
        let runtime_artifact = manifest
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| {
                artifact.component_id().as_str() == PRODUCTION_ZVEC_RUNTIME_COMPONENT_ID
            })
            .unwrap();
        let onnx_runtime_artifact = manifest
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| {
                artifact.component_id().as_str() == PRODUCTION_ONNX_RUNTIME_COMPONENT_ID
            })
            .unwrap();
        let model_artifact = manifest
            .catalog()
            .artifacts()
            .iter()
            .find(|artifact| matches!(artifact.kind(), ArtifactKind::Model(_)))
            .unwrap();

        let worker_provenance = provenance
            .iter()
            .find(|record| record.artifact_id() == worker_artifact.id())
            .unwrap();
        assert_eq!(
            worker_provenance.source().as_str(),
            PRODUCTION_PROCYON_SOURCE_URL
        );
        assert_eq!(
            worker_provenance.source_revision().as_str(),
            spec.procyon_revision
        );

        let runtime_provenance = provenance
            .iter()
            .find(|record| record.artifact_id() == runtime_artifact.id())
            .unwrap();
        assert_eq!(
            runtime_provenance.source().as_str(),
            PRODUCTION_ZVEC_SOURCE_URL
        );
        assert_eq!(
            runtime_provenance.source_revision().as_str(),
            PRODUCTION_ZVEC_SOURCE_REVISION
        );
        assert_eq!(
            runtime_artifact.version().to_string(),
            PRODUCTION_ZVEC_VERSION
        );
        assert!(
            runtime_artifact
                .license()
                .notice()
                .contains(PRODUCTION_ZVEC_NATIVE_SOURCE_REVISION)
        );

        let onnx_runtime_provenance = provenance
            .iter()
            .find(|record| record.artifact_id() == onnx_runtime_artifact.id())
            .unwrap();
        assert_eq!(
            onnx_runtime_provenance.source().as_str(),
            PRODUCTION_ONNX_RUNTIME_SOURCE_URL
        );
        assert_eq!(
            onnx_runtime_provenance.source_revision().as_str(),
            PRODUCTION_ONNX_RUNTIME_SOURCE_REVISION
        );
        assert_eq!(
            onnx_runtime_artifact.version().to_string(),
            PRODUCTION_ONNX_RUNTIME_VERSION
        );

        let model_provenance = provenance
            .iter()
            .find(|record| record.artifact_id() == model_artifact.id())
            .unwrap();
        assert_eq!(
            model_provenance.source().as_str(),
            PRODUCTION_MODEL_SOURCE_URL
        );
        assert_eq!(
            model_provenance.source_revision().as_str(),
            PRODUCTION_MODEL_REVISION
        );

        // Distribution locations are derived from the trusted release base
        // and artifact IDs, never a caller-supplied size or checksum.
        for artifact in manifest.catalog().artifacts() {
            assert!(
                artifact
                    .location()
                    .as_str()
                    .starts_with(spec.release_base_url.as_str())
            );
            assert!(
                artifact
                    .location()
                    .as_str()
                    .ends_with(artifact.id().as_str())
            );
            let bytes = fs::read(
                spec.output_directory
                    .join("artifacts")
                    .join(artifact.id().as_str()),
            )
            .unwrap();
            assert_eq!(Sha256Digest::calculate(&bytes), artifact.checksum());
            assert_eq!(artifact.resources().download_bytes(), bytes.len() as u64);
        }
    }

    #[test]
    fn e5_only_bundle_keeps_gemma_unavailable_once_signed() {
        use ed25519_dalek::SigningKey;

        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        let manifest = build(&spec);
        let expected = manifest.pipeline().model().clone();

        let signing_key = SigningKey::from_bytes(&[0x42; 32]);
        let signed = crate::sign_production_catalog(manifest, &signing_key).unwrap();
        let trusted =
            crate::TrustedCatalog::verify_production(signed, &signing_key.verifying_key()).unwrap();

        for profile in SemanticProfile::all() {
            assert_eq!(
                trusted.resolve_profile(*profile),
                (*profile != SemanticProfile::EmbeddingGemma2).then_some(&expected)
            );
        }
    }

    #[test]
    fn rejects_the_one_target_zvec_has_no_prebuilt_artifact_for() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = base_spec(directory.path());
        spec.target_operating_system = "macos".into();
        spec.target_architecture = "x86_64".into();

        let error = build_production_release_bundle(&spec).unwrap_err();
        assert!(matches!(
            error,
            ProductionBundleError::UnsupportedTarget { operating_system, architecture }
                if operating_system == "macos" && architecture == "x86_64"
        ));
    }

    #[test]
    fn accepts_every_documented_supported_target() {
        for (os, arch) in SUPPORTED_TARGETS {
            let directory = tempfile::tempdir().unwrap();
            let mut spec = base_spec(directory.path());
            spec.target_operating_system = os.into();
            spec.target_architecture = arch.into();
            let runtime = directory
                .path()
                .join(runtime_library_name(os, arch).unwrap());
            fs::write(&runtime, b"production zvec runtime payload").unwrap();
            spec.zvec_runtime_library = runtime;
            spec.onnx_runtime_library = if requires_external_onnx_runtime(os, arch) {
                let onnx_runtime = directory.path().join(onnx_runtime_source_name());
                fs::write(&onnx_runtime, b"production onnx runtime payload").unwrap();
                Some(onnx_runtime)
            } else {
                None
            };
            build(&spec);
        }
    }

    #[test]
    fn rejects_missing_or_wrong_linux_onnx_runtime_inputs() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = base_spec(directory.path());
        spec.onnx_runtime_library = None;
        assert!(matches!(
            build_bundle(&spec, &FIXTURE_MODEL_FILES),
            Err(ProductionBundleError::OnnxRuntimeUnavailable)
        ));

        let wrong = directory.path().join("libonnxruntime.so");
        fs::write(&wrong, b"production onnx runtime payload").unwrap();
        spec.onnx_runtime_library = Some(wrong);
        assert!(matches!(
            build_bundle(&spec, &FIXTURE_MODEL_FILES),
            Err(ProductionBundleError::OnnxRuntimeFileNameMismatch {
                expected: "libonnxruntime.so.1.28.0",
                ..
            })
        ));
    }

    #[test]
    fn rejects_a_runtime_without_the_platform_loader_filename() {
        let directory = tempfile::tempdir().unwrap();
        let mut spec = base_spec(directory.path());
        let alias = directory.path().join("libzvec_c_api.dll");
        fs::write(&alias, b"production zvec runtime payload").unwrap();
        spec.target_operating_system = "windows".into();
        spec.target_architecture = "x86_64".into();
        spec.zvec_runtime_library = alias;

        let error = build_bundle(&spec, &FIXTURE_MODEL_FILES).unwrap_err();
        assert!(matches!(
            error,
            ProductionBundleError::RuntimeFileNameMismatch {
                expected: "zvec_c_api.dll",
                actual,
            } if actual == "libzvec_c_api.dll"
        ));
    }

    #[test]
    fn rejects_a_model_cache_member_tampered_after_verification() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        fs::write(
            spec.model_cache_directory.join("model.onnx"),
            b"tampered-bytes-here!",
        )
        .unwrap();

        let error = build_bundle(&spec, &FIXTURE_MODEL_FILES).unwrap_err();
        assert!(matches!(
            error,
            ProductionBundleError::ModelCacheMemberMismatch { name: "model.onnx" }
        ));
        assert!(!spec.output_directory.exists());
    }

    #[test]
    fn rejects_a_missing_model_cache_member() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        fs::remove_file(spec.model_cache_directory.join("tokenizer.json")).unwrap();

        let error = build_bundle(&spec, &FIXTURE_MODEL_FILES).unwrap_err();
        assert!(matches!(
            error,
            ProductionBundleError::ModelCacheMemberMissing {
                name: "tokenizer.json"
            }
        ));
    }

    #[test]
    fn replaces_stale_output_contents_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let spec = base_spec(directory.path());
        build(&spec);
        fs::write(spec.output_directory.join("stale-leftover"), b"stale").unwrap();

        fs::write(
            &spec.worker_executable,
            b"a rebuilt production worker payload",
        )
        .unwrap();
        build(&spec);

        assert!(!spec.output_directory.join("stale-leftover").exists());
        assert!(spec.output_directory.join("catalog-input.json").exists());
        assert!(!spec.output_directory.with_extension("building").exists());
    }

    #[test]
    fn preserves_worker_executable_permissions_on_unix() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let directory = tempfile::tempdir().unwrap();
            let spec = base_spec(directory.path());
            fs::set_permissions(&spec.worker_executable, fs::Permissions::from_mode(0o755))
                .unwrap();
            let manifest = build(&spec);
            let worker_artifact = manifest
                .catalog()
                .artifacts()
                .iter()
                .find(|artifact| matches!(artifact.kind(), ArtifactKind::Worker))
                .unwrap();
            let packed = spec
                .output_directory
                .join("artifacts")
                .join(worker_artifact.id().as_str());
            let mode = fs::metadata(&packed).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }
}
