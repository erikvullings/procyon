//! Explicitly opt-in, non-production semantic worker assembly for local development.
//!
//! The assembly loads whichever model the host installed and activated through
//! the signed developer catalog. Two shapes are supported: a deterministic
//! hashed bag of words that exercises the pipeline without any learned
//! parameters, and a real transformer graph evaluated offline with ONNX
//! Runtime. Neither path performs network access, and the bundle as a whole
//! remains a development-only artifact rather than a supported component pack.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fm_semantic_components::{ModelPack, ModelPackKind};
use fm_semantic_conversion::{ConversionBudgets, STRUCTURAL_CHUNKER_VERSION};
use fm_semantic_docling::{
    DEFAULT_CONVERTER_PIPELINE_VERSION, OcrMyPdfConfiguration, converter_with_optional_ocr,
};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::developer_onnx::OnnxEmbeddingBackend;
use crate::embedding::{
    CpuEmbeddingBackend, CpuEmbeddingLoader, CuratedModelPackage, EmbeddingError,
    EmbeddingModelIdentity, EmbeddingResourceProfile, LocalEmbeddingRuntime, VectorNormalization,
};
use crate::ingestion::{
    DerivedIndex, EmbeddingProvider, IngestionCoordinator, IngestionEventSink, IngestionProgress,
    InteractivePriority, PipelineIngestionBackend, ResourceProbe, ResourceState,
};
use crate::semantic_search::{
    DenseWorkerQueryBackend, SemanticCandidateIndex, SemanticSearchService,
};
use crate::semantic_storage::{
    DistanceMetric, LibraryIndexManifest, SemanticCatalog, VectorIndexKind,
};
use crate::zvec_storage::{ZVEC_SCHEMA_VERSION, ZvecStorage, ZvecStorageError};
use crate::{
    ServerError, WorkerConfig, WorkerIngestionBackend, WorkerQueryBackend, WorkerServer,
    run_desktop_worker_with_factory,
};

/// Fixed vector width for the non-production developer embedder.
pub const DEVELOPMENT_EMBEDDING_DIMENSIONS: usize = 384;
/// Fixed maximum number of Unicode word tokens accepted per input.
pub const DEVELOPMENT_MAX_INPUT_TOKENS: usize = 8_192;
/// Fixed maximum UTF-8 byte length accepted per input.
pub const DEVELOPMENT_MAX_INPUT_BYTES: usize = 256 * 1024;

const DEVELOPMENT_MODEL_ID: &str = "procyon.dev.hashing-embedding";
const DEVELOPMENT_MODEL_REVISION: &str = "sha256-token-hashing-v1";
const DEVELOPMENT_TOKENIZER: &str = "unicode-words-v1";
const MAX_TOKEN_BYTES: usize = 64;
const PROJECTIONS_PER_TOKEN: usize = 8;

/// Returns the immutable identity of the non-production developer embedder.
#[must_use]
pub fn development_embedding_identity() -> EmbeddingModelIdentity {
    EmbeddingModelIdentity {
        model_id: DEVELOPMENT_MODEL_ID.into(),
        model_revision: DEVELOPMENT_MODEL_REVISION.into(),
        tokenizer: DEVELOPMENT_TOKENIZER.into(),
        dimensions: DEVELOPMENT_EMBEDDING_DIMENSIONS,
        max_input_tokens: DEVELOPMENT_MAX_INPUT_TOKENS,
    }
}

struct DevelopmentEmbeddingLoader;

impl CpuEmbeddingLoader for DevelopmentEmbeddingLoader {
    fn load(
        &self,
        package: &CuratedModelPackage,
    ) -> Result<Box<dyn CpuEmbeddingBackend>, EmbeddingError> {
        if package.identity != development_embedding_identity() {
            return Err(EmbeddingError::ModelIdentityMismatch);
        }
        Ok(Box::new(DevelopmentEmbeddingBackend {
            identity: development_embedding_identity(),
        }))
    }
}

struct DevelopmentEmbeddingBackend {
    identity: EmbeddingModelIdentity,
}

impl CpuEmbeddingBackend for DevelopmentEmbeddingBackend {
    fn identity(&self) -> &EmbeddingModelIdentity {
        &self.identity
    }

    fn token_count(&self, input: &str) -> Result<usize, EmbeddingError> {
        validate_input_size(input)?;
        Ok(unicode_tokens(input, None)?.0)
    }

    fn embed_batch(
        &self,
        inputs: &[&str],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let mut vectors = Vec::with_capacity(inputs.len());
        for input in inputs {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            validate_input_size(input)?;
            let (token_count, terms) = unicode_tokens(input, Some(cancellation))?;
            if token_count > DEVELOPMENT_MAX_INPUT_TOKENS {
                return Err(EmbeddingError::Backend(format!(
                    "non-production developer input exceeds {DEVELOPMENT_MAX_INPUT_TOKENS} tokens"
                )));
            }
            vectors.push(project_terms(&terms, cancellation)?);
        }
        Ok(vectors)
    }
}

fn validate_input_size(input: &str) -> Result<(), EmbeddingError> {
    if input.len() > DEVELOPMENT_MAX_INPUT_BYTES {
        return Err(EmbeddingError::Backend(format!(
            "non-production developer input exceeds {DEVELOPMENT_MAX_INPUT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn unicode_tokens(
    input: &str,
    cancellation: Option<&CancellationToken>,
) -> Result<(usize, BTreeMap<Vec<u8>, u32>), EmbeddingError> {
    let mut terms = BTreeMap::<Vec<u8>, u32>::new();
    let mut token = Vec::with_capacity(MAX_TOKEN_BYTES);
    let mut count = 0_usize;
    let finish_token =
        |token: &mut Vec<u8>, terms: &mut BTreeMap<Vec<u8>, u32>, count: &mut usize| {
            if !token.is_empty() {
                *count += 1;
                *terms.entry(std::mem::take(token)).or_default() += 1;
            }
        };

    for (index, character) in input.chars().enumerate() {
        if index % 256 == 0 && cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Err(EmbeddingError::Cancelled);
        }
        if character.is_alphanumeric() {
            for lowercase in character.to_lowercase() {
                let mut encoded = [0_u8; 4];
                let bytes = lowercase.encode_utf8(&mut encoded).as_bytes();
                if token.len() + bytes.len() <= MAX_TOKEN_BYTES {
                    token.extend_from_slice(bytes);
                }
            }
        } else {
            finish_token(&mut token, &mut terms, &mut count);
        }
    }
    finish_token(&mut token, &mut terms, &mut count);
    if terms.is_empty() {
        terms.insert(b"<no-unicode-words>".to_vec(), 1);
    }
    Ok((count, terms))
}

fn project_terms(
    terms: &BTreeMap<Vec<u8>, u32>,
    cancellation: &CancellationToken,
) -> Result<Vec<f32>, EmbeddingError> {
    let mut vector = vec![0.0_f32; DEVELOPMENT_EMBEDDING_DIMENSIONS];
    for (term, count) in terms {
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        let mut hasher = Sha256::new();
        hasher.update(b"procyon-non-production-developer-embedding-v1\0");
        hasher.update(term);
        let digest = hasher.finalize();
        let weight = 1.0 + (*count as f32).ln();
        for projection in 0..PROJECTIONS_PER_TOKEN {
            let offset = projection * 4;
            let index = usize::from(u16::from_le_bytes([digest[offset], digest[offset + 1]]))
                % DEVELOPMENT_EMBEDDING_DIMENSIONS;
            let sign = if digest[offset + 2] & 1 == 0 {
                1.0
            } else {
                -1.0
            };
            vector[index] += sign * weight;
        }
    }
    if vector.iter().all(|value| *value == 0.0) {
        vector[0] = 1.0;
    }
    Ok(vector)
}

fn developer_manifest(identity: &EmbeddingModelIdentity) -> LibraryIndexManifest {
    LibraryIndexManifest {
        zvec_schema_version: ZVEC_SCHEMA_VERSION,
        dimensions: identity.dimensions,
        distance_metric: DistanceMetric::Cosine,
        model_revision: identity.model_revision.clone(),
        tokenizer: identity.tokenizer.clone(),
        converter_version: DEFAULT_CONVERTER_PIPELINE_VERSION.into(),
        chunker_version: STRUCTURAL_CHUNKER_VERSION.to_string(),
        normalization: VectorNormalization::L2,
    }
}

/// The model the host installed, activated, and handed to this worker.
struct DeveloperModel {
    identity: EmbeddingModelIdentity,
    query_prefix: String,
    passage_prefix: String,
    loader: Box<dyn CpuEmbeddingLoader>,
    package_directory: PathBuf,
    description: String,
}

impl DeveloperModel {
    /// Resolves the deterministic fixture, or the installed pack the host
    /// selected. The pack path is always host-owned and never client supplied.
    fn resolve(
        model_pack: Option<&Path>,
        fixture_directory: &Path,
    ) -> Result<Self, DeveloperBundleError> {
        let Some(model_pack) = model_pack else {
            std::fs::create_dir_all(fixture_directory)?;
            return Ok(Self {
                identity: development_embedding_identity(),
                query_prefix: String::new(),
                passage_prefix: String::new(),
                loader: Box::new(DevelopmentEmbeddingLoader),
                package_directory: fixture_directory.to_owned(),
                description: "deterministic token-hashing fixture".to_owned(),
            });
        };
        if !model_pack.is_absolute() {
            return Err(DeveloperBundleError::RelativeModelPack);
        }
        let pack = ModelPack::open(model_pack)?;
        if pack.index().production {
            return Err(DeveloperBundleError::ProductionModelPack);
        }
        let identity = EmbeddingModelIdentity {
            model_id: pack.index().model_id.clone(),
            model_revision: pack.index().model_revision.clone(),
            tokenizer: pack.index().tokenizer.clone(),
            dimensions: usize::try_from(pack.index().dimensions)
                .map_err(|_| DeveloperBundleError::UnusableModelPack)?,
            max_input_tokens: usize::try_from(pack.index().max_input_tokens)
                .map_err(|_| DeveloperBundleError::UnusableModelPack)?,
        };
        let package_directory = model_pack
            .parent()
            .ok_or(DeveloperBundleError::UnusableModelPack)?
            .to_owned();
        let query_prefix = pack.index().query_prefix.clone();
        let passage_prefix = pack.index().passage_prefix.clone();
        let description = format!(
            "{} ({})",
            pack.index().source,
            match pack.index().kind {
                ModelPackKind::DeterministicTokenHashing => "deterministic token hashing",
                ModelPackKind::OnnxTransformerMeanPool => "ONNX transformer, mean pooled",
            }
        );
        let loader: Box<dyn CpuEmbeddingLoader> = match pack.index().kind {
            ModelPackKind::DeterministicTokenHashing => {
                if identity != development_embedding_identity() {
                    return Err(DeveloperBundleError::Embedding(
                        EmbeddingError::ModelIdentityMismatch,
                    ));
                }
                Box::new(DevelopmentEmbeddingLoader)
            }
            ModelPackKind::OnnxTransformerMeanPool => Box::new(OnnxPackLoader { pack }),
        };
        Ok(Self {
            identity,
            query_prefix,
            passage_prefix,
            loader,
            package_directory,
            description,
        })
    }

    /// Returns the per-model index directory beneath the host data root.
    fn index_directory(&self, data_directory: &Path) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(self.identity.model_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(self.identity.model_revision.as_bytes());
        let digest = hasher.finalize();
        let slug = self
            .identity
            .model_id
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '.' || character == '-' {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(48)
            .collect::<String>();
        data_directory.join("indexes").join(format!(
            "{slug}-{:016x}",
            u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
        ))
    }
}

/// Moves a pre-model-scoped developer index out of the way exactly once.
///
/// Task 0190 kept one `catalog.sqlite`/`zvec` pair directly beneath the data
/// root, implicitly owned by the only model that existed then. Those contents
/// belong to a different embedding space than any model selected now, so they
/// are retired to a clearly named sibling rather than silently reused or
/// deleted: a developer can inspect or remove them, and no query answers from
/// them by accident.
fn retire_superseded_flat_layout(data_directory: &Path) -> Result<(), DeveloperBundleError> {
    let catalog = data_directory.join("catalog.sqlite");
    let index = data_directory.join("zvec");
    if !catalog.exists() && !index.exists() {
        return Ok(());
    }
    let retired = data_directory.join("superseded-flat-index");
    std::fs::create_dir_all(&retired)?;
    for entry in std::fs::read_dir(data_directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "zvec" || name.starts_with("catalog.sqlite") {
            std::fs::rename(entry.path(), retired.join(entry.file_name()))?;
        }
    }
    eprintln!(
        "Procyon semantic developer bundle: retired the pre-model-scoped index to {}; \
it belongs to a superseded embedding space and is no longer queried",
        retired.display()
    );
    Ok(())
}

/// Selects the active model's index and clears stale incompatible contents.
///
/// The durable marker lives beside all model-scoped indexes. A normal worker
/// restart for the same model and converter pipeline preserves its index,
/// while switching either identity removes that target model's previous index
/// before it can answer queries. If the host exits after activation but before
/// launching a worker, the old marker remains and the next launch still
/// performs the reset.
fn prepare_active_model_index(
    data_directory: &Path,
    model: &DeveloperModel,
) -> Result<PathBuf, DeveloperBundleError> {
    const MARKER_NAME: &str = "active-model-index";
    const PENDING_MARKER_NAME: &str = "active-model-index.pending";
    const REINDEX_PENDING_NAME: &str = "model-reindex-pending";
    const REINDEX_RESET_NAME: &str = "model-reindex-reset";
    const REINDEX_RESET_PENDING_NAME: &str = "model-reindex-reset.pending";

    let model_directory = model.index_directory(data_directory);
    let identity = format!(
        "{}\n{}\n{}\n",
        model.identity.model_id, model.identity.model_revision, DEFAULT_CONVERTER_PIPELINE_VERSION
    );
    let marker = data_directory.join(MARKER_NAME);
    let reindex_pending = std::fs::read(data_directory.join(REINDEX_PENDING_NAME)).ok();
    let reindex_reset = data_directory.join(REINDEX_RESET_NAME);
    let reset_required = reindex_pending.as_ref().is_some_and(|pending| {
        std::fs::read(&reindex_reset)
            .map(|reset| reset != *pending)
            .unwrap_or(true)
    });
    let changed = reset_required
        || std::fs::read(&marker)
            .map(|current| current != identity.as_bytes())
            .unwrap_or(true);
    if !changed {
        return Ok(model_directory);
    }

    if model_directory.exists() {
        std::fs::remove_dir_all(&model_directory)?;
    }
    let pending = data_directory.join(PENDING_MARKER_NAME);
    std::fs::write(&pending, identity)?;
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }
    std::fs::rename(pending, marker)?;
    if let Some(reindex_generation) = reindex_pending {
        let reset_pending = data_directory.join(REINDEX_RESET_PENDING_NAME);
        std::fs::write(&reset_pending, reindex_generation)?;
        std::fs::rename(reset_pending, reindex_reset)?;
    }
    eprintln!("Procyon semantic developer bundle: prepared a clean index for the activated model");
    Ok(model_directory)
}

struct OnnxPackLoader {
    pack: ModelPack,
}

impl CpuEmbeddingLoader for OnnxPackLoader {
    fn load(
        &self,
        package: &CuratedModelPackage,
    ) -> Result<Box<dyn CpuEmbeddingBackend>, EmbeddingError> {
        Ok(Box::new(OnnxEmbeddingBackend::load(
            &self.pack,
            package.identity.clone(),
        )?))
    }
}

/// Applies the model's asymmetric input role prefix before embedding.
///
/// Instruction-tuned retrieval models such as E5 expect queries and indexed
/// passages to be marked differently. The prefix is data owned by the installed
/// model pack, so a model that needs none is passed through unchanged.
struct RolePrefixedEmbedder {
    inner: Arc<LocalEmbeddingRuntime>,
    prefix: String,
}

impl RolePrefixedEmbedder {
    fn wrap(inner: &Arc<LocalEmbeddingRuntime>, prefix: &str) -> Arc<dyn EmbeddingProvider> {
        if prefix.is_empty() {
            return Arc::clone(inner) as Arc<dyn EmbeddingProvider>;
        }
        Arc::new(Self {
            inner: Arc::clone(inner),
            prefix: prefix.to_owned(),
        })
    }
}

impl EmbeddingProvider for RolePrefixedEmbedder {
    fn identity(&self) -> &EmbeddingModelIdentity {
        self.inner.identity()
    }

    fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let prefixed = inputs
            .iter()
            .map(|input| format!("{}{input}", self.prefix))
            .collect::<Vec<_>>();
        self.inner.embed(&prefixed, cancellation)
    }
}

struct DeveloperResources {
    data_directory: PathBuf,
}

impl ResourceProbe for DeveloperResources {
    fn state(&self) -> ResourceState {
        ResourceState {
            available_disk_bytes: fs2::available_space(&self.data_directory).unwrap_or(0),
            on_battery: false,
            battery_percent: None,
            thermal_pressure: false,
        }
    }
}

struct DiscardDeveloperEvents;

impl IngestionEventSink for DiscardDeveloperEvents {
    fn publish(&self, _event: IngestionProgress) {}
}

struct DeveloperWorker {
    catalog: SemanticCatalog,
    embedder: Arc<LocalEmbeddingRuntime>,
    index: Arc<ZvecStorage>,
    manifest: LibraryIndexManifest,
    query_prefix: String,
    passage_prefix: String,
    data_directory: PathBuf,
    description: String,
}

impl DeveloperWorker {
    fn open(
        data_directory: &Path,
        model_pack: Option<&Path>,
    ) -> Result<Self, DeveloperBundleError> {
        if !data_directory.is_absolute() {
            return Err(DeveloperBundleError::RelativeDataDirectory);
        }
        std::fs::create_dir_all(data_directory)?;
        if !data_directory.is_dir() {
            return Err(DeveloperBundleError::InvalidDataDirectory(
                data_directory.to_owned(),
            ));
        }

        let model =
            DeveloperModel::resolve(model_pack, &data_directory.join("development-embedder"))?;
        let package = CuratedModelPackage {
            identity: model.identity.clone(),
            directory: model.package_directory.clone(),
        };
        let embedder = Arc::new(LocalEmbeddingRuntime::load(
            &package,
            model.loader.as_ref(),
            EmbeddingResourceProfile::Fast,
            VectorNormalization::L2,
        )?);

        retire_superseded_flat_layout(data_directory)?;
        // Each model owns its own catalog and vector index: an index built for
        // one embedding space is meaningless in another, so switching profiles
        // must start a new index rather than corrupt or reject the old one.
        let model_directory = prepare_active_model_index(data_directory, &model)?;
        std::fs::create_dir_all(&model_directory)?;
        let manifest = developer_manifest(&model.identity);
        let catalog = SemanticCatalog::open(model_directory.join("catalog.sqlite"))?;
        catalog.validate_registered_library_manifests(&manifest)?;
        let index_directory = model_directory.join("zvec");
        let index = if index_directory.exists() {
            ZvecStorage::open(&index_directory, model.identity.dimensions, false)?
        } else {
            ZvecStorage::create(
                &index_directory,
                model.identity.dimensions,
                VectorIndexKind::Flat,
            )?
        };
        Ok(Self {
            catalog,
            embedder,
            index: Arc::new(index),
            manifest,
            query_prefix: model.query_prefix,
            passage_prefix: model.passage_prefix,
            data_directory: data_directory.to_owned(),
            description: model.description,
        })
    }

    fn backends(&self) -> (Arc<dyn WorkerIngestionBackend>, Arc<dyn WorkerQueryBackend>) {
        let passages = RolePrefixedEmbedder::wrap(&self.embedder, &self.passage_prefix);
        let queries = RolePrefixedEmbedder::wrap(&self.embedder, &self.query_prefix);
        let derived_index: Arc<dyn DerivedIndex> = self.index.clone();
        let candidate_index: Arc<dyn SemanticCandidateIndex> = self.index.clone();
        let ocr_configuration = OcrMyPdfConfiguration::from_environment();
        let ocr_enabled = ocr_configuration.is_some();
        let mut coordinator = IngestionCoordinator::with_converter(
            self.catalog.clone(),
            converter_with_optional_ocr(ocr_configuration),
            passages,
            derived_index,
            Arc::new(DeveloperResources {
                data_directory: self.data_directory.clone(),
            }),
            Arc::new(DiscardDeveloperEvents),
            InteractivePriority::default(),
        );
        if ocr_enabled {
            coordinator = coordinator.with_conversion_budgets(ConversionBudgets {
                timeout: Duration::from_secs(4 * 60),
                ..ConversionBudgets::default()
            });
        }
        let coordinator = Arc::new(coordinator);
        let ingestion: Arc<dyn WorkerIngestionBackend> = Arc::new(
            PipelineIngestionBackend::with_library_manifest(coordinator, self.manifest.clone()),
        );
        let query: Arc<dyn WorkerQueryBackend> = Arc::new(DenseWorkerQueryBackend::new(
            SemanticSearchService::new(self.catalog.clone(), queries, candidate_index),
        ));
        (ingestion, query)
    }

    fn into_server(self, config: WorkerConfig) -> WorkerServer {
        let (ingestion, query) = self.backends();
        WorkerServer::with_backends(config, ingestion, query)
    }
}

/// Runs the normal authenticated local worker with durable development
/// ingestion and retrieval rooted under `data_directory`.
///
/// This path performs no network access and exists only when the crate was
/// built with `--features developer-bundle`.
///
/// # Errors
///
/// Returns setup, persistence, Zvec, embedding, lock, or IPC failures.
pub async fn run_developer_worker(
    runtime_directory: &Path,
    data_directory: &Path,
    model_pack: Option<&Path>,
    idle_timeout: Duration,
) -> Result<(), ServerError> {
    eprintln!(
        "Procyon semantic developer bundle: development-only retrieval; data={}",
        data_directory.display()
    );
    run_desktop_worker_with_factory(runtime_directory, idle_timeout, |config| {
        DeveloperWorker::open(data_directory, model_pack)
            .inspect(|worker| {
                eprintln!(
                    "Procyon semantic developer bundle: active model {}",
                    worker.description
                );
            })
            .map(|worker| worker.into_server(config))
            .map_err(|error| ServerError::Io(io::Error::other(error)))
    })
    .await
}

/// Failure to assemble the explicitly non-production developer worker.
#[derive(Debug, thiserror::Error)]
enum DeveloperBundleError {
    #[error("developer data directory must be an absolute host-provided path")]
    RelativeDataDirectory,
    #[error("developer model pack must be an absolute host-resolved path")]
    RelativeModelPack,
    #[error("developer bundles must not load a model pack marked as production")]
    ProductionModelPack,
    #[error("developer model pack declares unusable limits or location")]
    UnusableModelPack,
    #[error("developer model pack failed: {0}")]
    ModelPack(#[from] fm_semantic_components::ModelPackError),
    #[error("developer data path is not a directory: {0}")]
    InvalidDataDirectory(PathBuf),
    #[error("developer data directory failed: {0}")]
    Io(#[from] io::Error),
    #[error("developer catalog failed: {0}")]
    Catalog(#[from] crate::semantic_storage::StorageError),
    #[error("developer embedder failed: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("developer Zvec index failed: {0}")]
    Zvec(#[from] ZvecStorageError),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::{IngestionState, WorkerIngestionInput, WorkerQueryInput};

    use super::*;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    /// Pinned upstream revision the developer bundle packs for the quality profile.
    const MULTILINGUAL_TEST_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("developer-bundle-tests")
                .join(format!("{name}-{}-{sequence}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_index_directory(data_directory: &Path) -> PathBuf {
        DeveloperModel::resolve(None, &data_directory.join("development-embedder"))
            .expect("fixture model")
            .index_directory(data_directory)
    }

    fn embedding_runtime(directory: &Path) -> LocalEmbeddingRuntime {
        LocalEmbeddingRuntime::load(
            &CuratedModelPackage {
                identity: development_embedding_identity(),
                directory: directory.to_owned(),
            },
            &DevelopmentEmbeddingLoader,
            EmbeddingResourceProfile::Balanced,
            VectorNormalization::L2,
        )
        .expect("development embedding runtime")
    }

    #[test]
    fn development_embeddings_are_deterministic_normalized_and_overlap_sensitive() {
        let directory = TestDirectory::new("embedding");
        let runtime = embedding_runtime(&directory.0);
        let inputs = vec![
            "durable semantic retrieval index".to_owned(),
            "semantic index with durable storage".to_owned(),
            "painted wooden garden chair".to_owned(),
        ];

        let first = runtime
            .embed(&inputs, &CancellationToken::new())
            .expect("first embeddings");
        let second = runtime
            .embed(&inputs, &CancellationToken::new())
            .expect("second embeddings");
        let unicode_case_variants = runtime
            .embed(
                &["CAFÉ résumé".into(), "café RÉSUMÉ".into()],
                &CancellationToken::new(),
            )
            .expect("Unicode embeddings");

        assert_eq!(first, second);
        assert_eq!(unicode_case_variants[0], unicode_case_variants[1]);
        assert_eq!(
            development_embedding_identity(),
            EmbeddingModelIdentity {
                model_id: "procyon.dev.hashing-embedding".into(),
                model_revision: "sha256-token-hashing-v1".into(),
                tokenizer: "unicode-words-v1".into(),
                dimensions: 384,
                max_input_tokens: 8_192,
            }
        );
        for vector in &first {
            let norm = vector.iter().map(|value| value * value).sum::<f32>();
            assert!((norm - 1.0).abs() < 0.000_01);
        }
        let dot = |left: &[f32], right: &[f32]| {
            left.iter()
                .zip(right)
                .map(|(left, right)| left * right)
                .sum::<f32>()
        };
        assert!(dot(&first[0], &first[1]) > dot(&first[0], &first[2]));
    }

    #[test]
    fn development_embedding_honours_cancellation() {
        let directory = TestDirectory::new("cancellation");
        let runtime = embedding_runtime(&directory.0);
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        assert!(matches!(
            runtime.embed(&["semantic retrieval".into()], &cancellation),
            Err(EmbeddingError::Cancelled)
        ));
    }

    #[test]
    fn development_embedding_rejects_oversized_input_without_network_or_inference() {
        let directory = TestDirectory::new("bounded");
        let runtime = embedding_runtime(&directory.0);
        let oversized = "x".repeat(DEVELOPMENT_MAX_INPUT_BYTES + 1);
        let too_many_tokens = "x ".repeat(DEVELOPMENT_MAX_INPUT_TOKENS + 1);

        assert!(matches!(
            runtime.embed(&[oversized], &CancellationToken::new()),
            Err(EmbeddingError::Backend(message)) if message.contains("exceeds")
        ));
        assert!(matches!(
            runtime.embed(&[too_many_tokens], &CancellationToken::new()),
            Err(EmbeddingError::InputTooLarge {
                tokens: 8_193,
                maximum: 8_192,
                ..
            })
        ));
    }

    #[test]
    fn durable_pipeline_and_query_survive_catalog_and_index_reopen() {
        let directory = TestDirectory::new("reopen");
        let worker = DeveloperWorker::open(&directory.0, None).expect("developer worker");
        let (ingestion, query) = worker.backends();
        let job_id = ingestion
            .enqueue(
                WorkerIngestionInput {
                    job_id: "job-development-1".into(),
                    tenant_id: "tenant-development".into(),
                    library_id: "library-development".into(),
                    document_id: "document-development".into(),
                    media_type: "text/plain".into(),
                    metadata: BTreeMap::from([
                        ("occurrence_id".into(), "occurrence-development".into()),
                        ("source_id".into(), "source-development".into()),
                        ("root_id".into(), "root-development".into()),
                        ("modified_at_ms".into(), "1".into()),
                    ]),
                    content: b"Durable semantic indexes preserve searchable evidence.".to_vec(),
                },
                CancellationToken::new(),
            )
            .expect("enqueue");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let job = ingestion
                .job("tenant-development", "library-development", &job_id)
                .expect("job")
                .expect("registered job");
            if job.state == IngestionState::Completed {
                break;
            }
            assert_ne!(job.state, IngestionState::Failed, "{:?}", job.error);
            assert!(std::time::Instant::now() < deadline, "ingestion timed out");
            std::thread::sleep(Duration::from_millis(10));
        }

        let before = query
            .query(
                WorkerQueryInput {
                    tenant_id: "tenant-development".into(),
                    library_id: "library-development".into(),
                    query: "semantic evidence".into(),
                    concept_query: None,
                    maximum_results: 10,
                },
                &CancellationToken::new(),
            )
            .expect("query before reopen");
        assert_eq!(before[0].document_id, "document-development");

        drop(query);
        drop(ingestion);
        drop(worker);

        let reopened =
            DeveloperWorker::open(&directory.0, None).expect("reopened developer worker");
        let (_, query) = reopened.backends();
        let after = query
            .query(
                WorkerQueryInput {
                    tenant_id: "tenant-development".into(),
                    library_id: "library-development".into(),
                    query: "semantic evidence".into(),
                    concept_query: None,
                    maximum_results: 10,
                },
                &CancellationToken::new(),
            )
            .expect("query after reopen");
        assert_eq!(after[0].document_id, "document-development");
        let model_directory = fixture_index_directory(&directory.0);
        assert!(model_directory.join("catalog.sqlite").is_file());
        assert!(model_directory.join("zvec").is_dir());
    }

    /// Resolves the packed real model, failing loudly rather than passing
    /// vacuously when an explicitly requested ignored test has no bundle.
    fn required_model_pack() -> PathBuf {
        let pack = std::env::var_os("PROCYON_SEMANTIC_MODEL_PACK")
            .map(PathBuf::from)
            .expect(
                "set PROCYON_SEMANTIC_MODEL_PACK to the packed multilingual model from \
                 `pnpm semantic:bundle:dev`",
            );
        assert!(
            pack.is_file(),
            "PROCYON_SEMANTIC_MODEL_PACK does not point at a model pack file: {}",
            pack.display()
        );
        pack
    }

    fn write_pack(path: &Path, spec: &fm_semantic_components::ModelPackSpec) {
        std::fs::create_dir_all(path.parent().expect("pack parent")).expect("pack directory");
        fm_semantic_components::write_model_pack(path, spec).expect("model pack");
    }

    fn fixture_pack_spec() -> fm_semantic_components::ModelPackSpec {
        let identity = development_embedding_identity();
        fm_semantic_components::ModelPackSpec {
            kind: ModelPackKind::DeterministicTokenHashing,
            model_id: identity.model_id,
            model_revision: identity.model_revision,
            tokenizer: identity.tokenizer,
            dimensions: 384,
            max_input_tokens: 8_192,
            query_prefix: String::new(),
            passage_prefix: String::new(),
            production: false,
            source: "Procyon deterministic token-hashing fixture".into(),
            files: Vec::new(),
        }
    }

    #[test]
    fn installed_fixture_pack_loads_through_the_same_path_as_a_real_model() {
        let directory = TestDirectory::new("fixture-pack");
        let pack = directory.0.join("artifacts").join("fixture");
        write_pack(&pack, &fixture_pack_spec());

        let model = DeveloperModel::resolve(Some(&pack), &directory.0.join("unused"))
            .expect("fixture pack model");

        assert_eq!(model.identity, development_embedding_identity());
        assert!(model.query_prefix.is_empty());
        assert!(model.passage_prefix.is_empty());
        assert_eq!(model.package_directory, pack.parent().expect("parent"));
        let worker =
            DeveloperWorker::open(&directory.0, Some(&pack)).expect("worker from fixture pack");
        assert_eq!(worker.manifest.dimensions, 384);
    }

    #[test]
    fn rejects_relative_and_production_model_packs() {
        let directory = TestDirectory::new("pack-policy");
        assert!(matches!(
            DeveloperModel::resolve(Some(Path::new("relative/pack")), &directory.0),
            Err(DeveloperBundleError::RelativeModelPack)
        ));

        let mut production = fixture_pack_spec();
        production.production = true;
        let pack = directory.0.join("artifacts").join("production");
        write_pack(&pack, &production);
        assert!(matches!(
            DeveloperModel::resolve(Some(&pack), &directory.0),
            Err(DeveloperBundleError::ProductionModelPack)
        ));

        let mut mismatched = fixture_pack_spec();
        mismatched.model_revision = "not-the-fixture-revision".into();
        let pack = directory.0.join("artifacts").join("mismatched");
        write_pack(&pack, &mismatched);
        assert!(matches!(
            DeveloperModel::resolve(Some(&pack), &directory.0),
            Err(DeveloperBundleError::Embedding(
                EmbeddingError::ModelIdentityMismatch
            ))
        ));

        let foreign = directory.0.join("artifacts").join("foreign");
        std::fs::create_dir_all(foreign.parent().expect("parent")).expect("directory");
        std::fs::write(&foreign, b"{\"production\": false}").expect("foreign");
        assert!(matches!(
            DeveloperModel::resolve(Some(&foreign), &directory.0),
            Err(DeveloperBundleError::ModelPack(_))
        ));
    }

    #[test]
    fn distinct_models_never_share_one_index_directory() {
        let directory = TestDirectory::new("index-isolation");
        let fixture = DeveloperModel::resolve(None, &directory.0.join("development-embedder"))
            .expect("fixture");
        let mut other = fixture_pack_spec();
        other.kind = ModelPackKind::OnnxTransformerMeanPool;
        other.model_id = "example.other-model".into();
        other.model_revision = "revision-two".into();
        let pack = directory.0.join("artifacts").join("other");
        write_pack(&pack, &other);
        let other = DeveloperModel::resolve(Some(&pack), &directory.0).expect("other model");

        let first = fixture.index_directory(&directory.0);
        let second = other.index_directory(&directory.0);

        assert_ne!(first, second);
        assert!(first.starts_with(directory.0.join("indexes")));
        assert!(second.starts_with(directory.0.join("indexes")));
        assert_eq!(first, fixture.index_directory(&directory.0));
    }

    #[test]
    fn asymmetric_input_roles_are_applied_only_when_the_model_declares_them() {
        let directory = TestDirectory::new("role-prefix");
        let runtime = Arc::new(embedding_runtime(&directory.0));
        let cancellation = CancellationToken::new();

        let unprefixed = RolePrefixedEmbedder::wrap(&runtime, "");
        let prefixed = RolePrefixedEmbedder::wrap(&runtime, "query: ");
        let inputs = vec!["lekkende kraan".to_owned()];
        let baseline = runtime.embed(&inputs, &cancellation).expect("baseline");
        let explicit = runtime
            .embed(&["query: lekkende kraan".to_owned()], &cancellation)
            .expect("explicit");

        assert_eq!(
            unprefixed.embed(&inputs, &cancellation).expect("plain"),
            baseline
        );
        assert_eq!(
            prefixed.embed(&inputs, &cancellation).expect("prefixed"),
            explicit
        );
        assert_ne!(baseline, explicit);
        assert_eq!(prefixed.identity(), runtime.identity());
    }

    /// Runs the real pinned multilingual model. Ignored by default so no
    /// ordinary unit run depends on a multi-hundred-megabyte download; see
    /// `required_model_pack` for how to run it.
    #[test]
    #[ignore = "requires PROCYON_SEMANTIC_MODEL_PACK from a built developer bundle"]
    fn real_multilingual_model_embeds_and_ranks_across_languages() {
        let pack = required_model_pack();
        let directory = TestDirectory::new("multilingual");
        let model = DeveloperModel::resolve(Some(&pack), &directory.0).expect("packed model");
        assert_eq!(model.query_prefix, "query: ");
        assert_eq!(model.passage_prefix, "passage: ");
        assert_eq!(model.identity.dimensions, 384);
        assert_eq!(model.identity.max_input_tokens, 512);

        let runtime = Arc::new(
            LocalEmbeddingRuntime::load(
                &CuratedModelPackage {
                    identity: model.identity.clone(),
                    directory: model.package_directory.clone(),
                },
                model.loader.as_ref(),
                EmbeddingResourceProfile::Balanced,
                VectorNormalization::L2,
            )
            .expect("offline model load"),
        );
        let queries = RolePrefixedEmbedder::wrap(&runtime, &model.query_prefix);
        let passages = RolePrefixedEmbedder::wrap(&runtime, &model.passage_prefix);
        let cancellation = CancellationToken::new();

        let query = queries
            .embed(
                &["hoe herstel ik een lekkende kraan".to_owned()],
                &cancellation,
            )
            .expect("query embedding");
        let corpus = passages
            .embed(
                &[
                    "How to repair a dripping tap in your kitchen sink.".to_owned(),
                    "Recept voor een chocoladetaart met amandelen.".to_owned(),
                ],
                &cancellation,
            )
            .expect("passage embeddings");

        for vector in query.iter().chain(corpus.iter()) {
            assert_eq!(vector.len(), 384);
            assert!(vector.iter().all(|value| value.is_finite()));
            let norm = vector.iter().map(|value| value * value).sum::<f32>();
            assert!((norm - 1.0).abs() < 0.001, "vector is not unit length");
        }
        let similarity = |left: &[f32], right: &[f32]| {
            left.iter()
                .zip(right)
                .map(|(left, right)| left * right)
                .sum::<f32>()
        };
        // Cross-language relevance must beat a same-language irrelevant passage.
        assert!(
            similarity(&query[0], &corpus[0]) > similarity(&query[0], &corpus[1]),
            "cross-language retrieval did not outrank an unrelated passage"
        );

        // Truncation, not failure, bounds an input past the model's window.
        let long = "kraan reparatie ".repeat(4_000);
        let truncated = queries
            .embed(&[long], &cancellation)
            .expect("oversized input is truncated rather than rejected");
        assert_eq!(truncated[0].len(), 384);

        assert!(matches!(
            runtime.embed(&["kraan".to_owned()], &{
                let token = CancellationToken::new();
                token.cancel();
                token
            }),
            Err(EmbeddingError::Cancelled)
        ));
    }

    /// End-to-end proof that the installed real model, not the fixture, drives
    /// ingestion and search, with each side receiving its own input role.
    #[test]
    #[ignore = "requires PROCYON_SEMANTIC_MODEL_PACK from a built developer bundle"]
    fn real_multilingual_model_answers_a_cross_language_query_through_the_pipeline() {
        let pack = required_model_pack();
        let directory = TestDirectory::new("multilingual-pipeline");
        let worker =
            DeveloperWorker::open(&directory.0, Some(&pack)).expect("packed developer worker");
        assert_eq!(worker.manifest.model_revision, MULTILINGUAL_TEST_REVISION);
        let (ingestion, query) = worker.backends();

        for (document, content) in [
            (
                "document-tap",
                "Een lekkende keukenkraan repareer je door de kraan te sluiten, \
                 de knop los te draaien en de versleten rubberen ring te vervangen.",
            ),
            (
                "document-cake",
                "Voor een chocoladetaart met amandelen meng je bloem, cacao en \
                 gemalen amandelen en bak je het beslag drie kwartier.",
            ),
        ] {
            let job_id = ingestion
                .enqueue(
                    WorkerIngestionInput {
                        job_id: format!("job-{document}"),
                        tenant_id: "tenant-multilingual".into(),
                        library_id: "library-multilingual".into(),
                        document_id: document.into(),
                        media_type: "text/plain".into(),
                        metadata: BTreeMap::from([
                            ("occurrence_id".into(), format!("occurrence-{document}")),
                            ("source_id".into(), format!("source-{document}")),
                            ("root_id".into(), "root-multilingual".into()),
                            ("modified_at_ms".into(), "1".into()),
                        ]),
                        content: content.as_bytes().to_vec(),
                    },
                    CancellationToken::new(),
                )
                .expect("enqueue");
            let deadline = std::time::Instant::now() + Duration::from_secs(120);
            loop {
                let job = ingestion
                    .job("tenant-multilingual", "library-multilingual", &job_id)
                    .expect("job")
                    .expect("registered job");
                if job.state == IngestionState::Completed {
                    break;
                }
                assert_ne!(job.state, IngestionState::Failed, "{:?}", job.error);
                assert!(std::time::Instant::now() < deadline, "ingestion timed out");
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        let results = query
            .query(
                WorkerQueryInput {
                    tenant_id: "tenant-multilingual".into(),
                    library_id: "library-multilingual".into(),
                    query: "how do I fix a dripping kitchen tap".into(),
                    concept_query: None,
                    maximum_results: 10,
                },
                &CancellationToken::new(),
            )
            .expect("cross-language query");

        assert_eq!(
            results.first().map(|result| result.document_id.as_str()),
            Some("document-tap"),
            "an English query did not retrieve the relevant Dutch document"
        );
    }

    #[test]
    fn a_pre_model_scoped_index_is_retired_rather_than_reused() {
        let directory = TestDirectory::new("legacy-layout");
        let worker = DeveloperWorker::open(&directory.0, None).expect("developer worker");
        let model_directory = fixture_index_directory(&directory.0);
        drop(worker);

        // Recreate the task 0190 layout: one catalog and index directly beneath
        // the data root, with no model scoping.
        std::fs::rename(
            model_directory.join("catalog.sqlite"),
            directory.0.join("catalog.sqlite"),
        )
        .expect("legacy catalog");
        std::fs::rename(model_directory.join("zvec"), directory.0.join("zvec"))
            .expect("legacy index");
        std::fs::remove_dir_all(directory.0.join("indexes")).expect("model-scoped indexes");

        let worker = DeveloperWorker::open(&directory.0, None).expect("reopened developer worker");
        drop(worker);

        let retired = directory.0.join("superseded-flat-index");
        assert!(retired.join("catalog.sqlite").is_file());
        assert!(retired.join("zvec").is_dir());
        assert!(!directory.0.join("catalog.sqlite").exists());
        assert!(!directory.0.join("zvec").exists());
        assert!(
            fixture_index_directory(&directory.0)
                .join("catalog.sqlite")
                .is_file()
        );
    }

    #[test]
    fn model_migration_reset_removes_a_previously_used_models_stale_index() {
        let directory = TestDirectory::new("model-reset");
        let worker = DeveloperWorker::open(&directory.0, None).expect("developer worker");
        let model_directory = fixture_index_directory(&directory.0);
        drop(worker);
        std::fs::write(model_directory.join("stale-content"), b"revoked document")
            .expect("stale marker");

        let mut other_model =
            DeveloperModel::resolve(None, &directory.0.join("other-model")).expect("other model");
        other_model.identity.model_id = "procyon.dev.other-embedding".into();
        other_model.identity.model_revision = "other-revision".into();
        prepare_active_model_index(&directory.0, &other_model).expect("activate other model");
        drop(DeveloperWorker::open(&directory.0, None).expect("reactivated original model"));

        assert!(!model_directory.join("stale-content").exists());
        assert!(model_directory.join("catalog.sqlite").is_file());
        assert!(model_directory.join("zvec").is_dir());
    }

    #[test]
    fn legacy_converter_index_is_reset_once_then_rebuilt_content_is_preserved() {
        let directory = TestDirectory::new("converter-reset");
        drop(DeveloperWorker::open(&directory.0, None).expect("developer worker"));
        let model_directory = fixture_index_directory(&directory.0);
        std::fs::write(model_directory.join("baseline-content"), b"legacy chunks")
            .expect("legacy content");
        std::fs::write(
            directory.0.join("active-model-index"),
            format!("{DEVELOPMENT_MODEL_ID}\n{DEVELOPMENT_MODEL_REVISION}\n"),
        )
        .expect("legacy active-model marker");

        drop(DeveloperWorker::open(&directory.0, None).expect("migrated worker"));

        assert!(!model_directory.join("baseline-content").exists());
        std::fs::write(model_directory.join("docling-content"), b"rebuilt chunks")
            .expect("rebuilt content");
        drop(DeveloperWorker::open(&directory.0, None).expect("replacement worker"));

        assert!(model_directory.join("docling-content").is_file());
    }

    #[test]
    fn developer_manifest_identifies_the_docling_first_pipeline() {
        assert_eq!(
            developer_manifest(&development_embedding_identity()).converter_version,
            DEFAULT_CONVERTER_PIPELINE_VERSION
        );
    }

    #[test]
    fn pending_reindex_marker_restarts_an_interrupted_same_model_rebuild() {
        let directory = TestDirectory::new("pending-model-reset");
        drop(DeveloperWorker::open(&directory.0, None).expect("developer worker"));
        let model_directory = fixture_index_directory(&directory.0);
        std::fs::write(model_directory.join("partial-content"), b"partial rebuild")
            .expect("partial marker");
        std::fs::write(directory.0.join("model-reindex-pending"), b"pending\n")
            .expect("pending reindex");

        drop(DeveloperWorker::open(&directory.0, None).expect("retried worker"));

        assert!(!model_directory.join("partial-content").exists());
        assert!(model_directory.join("catalog.sqlite").is_file());
        assert!(model_directory.join("zvec").is_dir());
    }

    #[test]
    fn pending_reindex_generation_clears_only_the_first_worker_launch() {
        let directory = TestDirectory::new("pending-model-reset-once");
        drop(DeveloperWorker::open(&directory.0, None).expect("developer worker"));
        let model_directory = fixture_index_directory(&directory.0);
        std::fs::write(
            directory.0.join("model-reindex-pending"),
            b"migration-generation\n",
        )
        .expect("pending reindex");

        drop(DeveloperWorker::open(&directory.0, None).expect("first reindex worker"));
        std::fs::write(
            model_directory.join("completed-root"),
            b"content rebuilt before worker restart",
        )
        .expect("rebuilt content");
        drop(DeveloperWorker::open(&directory.0, None).expect("replacement reindex worker"));

        assert!(model_directory.join("completed-root").is_file());
    }

    #[test]
    fn reopen_rejects_a_catalog_with_a_different_development_manifest() {
        let directory = TestDirectory::new("manifest-mismatch");
        let worker = DeveloperWorker::open(&directory.0, None).expect("developer worker");
        let model_directory = fixture_index_directory(&directory.0);
        drop(worker);
        let catalog = SemanticCatalog::open(model_directory.join("catalog.sqlite"))
            .expect("developer catalog");
        let mut incompatible = developer_manifest(&development_embedding_identity());
        incompatible.model_revision = "different-development-revision".into();
        catalog
            .register_library("tenant-other", "library-other", &incompatible)
            .expect("incompatible fixture library");
        drop(catalog);

        assert!(matches!(
            DeveloperWorker::open(&directory.0, None),
            Err(DeveloperBundleError::Catalog(
                crate::semantic_storage::StorageError::MigrationRequired { .. }
            ))
        ));
    }
}
