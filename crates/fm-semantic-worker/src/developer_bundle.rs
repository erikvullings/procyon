//! Explicitly opt-in, non-production semantic worker assembly for local development.
//!
//! The embedder in this module is a deterministic hashed bag of words. It is
//! useful for exercising ingestion and retrieval, but it is not a trained
//! model and must never be represented as production-quality semantic search.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::embedding::{
    CpuEmbeddingBackend, CpuEmbeddingLoader, CuratedModelPackage, EmbeddingError,
    EmbeddingModelIdentity, EmbeddingResourceProfile, LocalEmbeddingRuntime, VectorNormalization,
};
use crate::ingestion::{
    DerivedIndex, IngestionCoordinator, IngestionEventSink, IngestionProgress, InteractivePriority,
    PipelineIngestionBackend, ResourceProbe, ResourceState,
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

fn developer_manifest() -> LibraryIndexManifest {
    let identity = development_embedding_identity();
    LibraryIndexManifest {
        zvec_schema_version: ZVEC_SCHEMA_VERSION,
        dimensions: identity.dimensions,
        distance_metric: DistanceMetric::Cosine,
        model_revision: identity.model_revision,
        tokenizer: identity.tokenizer,
        converter_version: "baseline/1".into(),
        chunker_version: "structural/2".into(),
        normalization: VectorNormalization::L2,
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
    data_directory: PathBuf,
}

impl DeveloperWorker {
    fn open(data_directory: &Path) -> Result<Self, DeveloperBundleError> {
        if !data_directory.is_absolute() {
            return Err(DeveloperBundleError::RelativeDataDirectory);
        }
        std::fs::create_dir_all(data_directory)?;
        if !data_directory.is_dir() {
            return Err(DeveloperBundleError::InvalidDataDirectory(
                data_directory.to_owned(),
            ));
        }

        let model_directory = data_directory.join("development-embedder");
        std::fs::create_dir_all(&model_directory)?;
        let package = CuratedModelPackage {
            identity: development_embedding_identity(),
            directory: model_directory,
        };
        let embedder = Arc::new(LocalEmbeddingRuntime::load(
            &package,
            &DevelopmentEmbeddingLoader,
            EmbeddingResourceProfile::Balanced,
            VectorNormalization::L2,
        )?);
        let catalog = SemanticCatalog::open(data_directory.join("catalog.sqlite"))?;
        catalog.validate_registered_library_manifests(&developer_manifest())?;
        let index_directory = data_directory.join("zvec");
        let index = if index_directory.exists() {
            ZvecStorage::open(&index_directory, DEVELOPMENT_EMBEDDING_DIMENSIONS, false)?
        } else {
            ZvecStorage::create(
                &index_directory,
                DEVELOPMENT_EMBEDDING_DIMENSIONS,
                VectorIndexKind::Flat,
            )?
        };
        Ok(Self {
            catalog,
            embedder,
            index: Arc::new(index),
            data_directory: data_directory.to_owned(),
        })
    }

    fn backends(&self) -> (Arc<dyn WorkerIngestionBackend>, Arc<dyn WorkerQueryBackend>) {
        let embedding: Arc<dyn crate::ingestion::EmbeddingProvider> = self.embedder.clone();
        let derived_index: Arc<dyn DerivedIndex> = self.index.clone();
        let candidate_index: Arc<dyn SemanticCandidateIndex> = self.index.clone();
        let coordinator = Arc::new(IngestionCoordinator::new(
            self.catalog.clone(),
            embedding.clone(),
            derived_index,
            Arc::new(DeveloperResources {
                data_directory: self.data_directory.clone(),
            }),
            Arc::new(DiscardDeveloperEvents),
            InteractivePriority::default(),
        ));
        let ingestion: Arc<dyn WorkerIngestionBackend> = Arc::new(
            PipelineIngestionBackend::with_library_manifest(coordinator, developer_manifest()),
        );
        let query: Arc<dyn WorkerQueryBackend> = Arc::new(DenseWorkerQueryBackend::new(
            SemanticSearchService::new(self.catalog.clone(), embedding, candidate_index),
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
    idle_timeout: Duration,
) -> Result<(), ServerError> {
    eprintln!(
        "Procyon semantic developer bundle: non-production hashed retrieval; data={}",
        data_directory.display()
    );
    run_desktop_worker_with_factory(runtime_directory, idle_timeout, |config| {
        DeveloperWorker::open(data_directory)
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
        let worker = DeveloperWorker::open(&directory.0).expect("developer worker");
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

        let reopened = DeveloperWorker::open(&directory.0).expect("reopened developer worker");
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
        assert!(directory.0.join("catalog.sqlite").is_file());
        assert!(directory.0.join("zvec").is_dir());
    }

    #[test]
    fn reopen_rejects_a_catalog_with_a_different_development_manifest() {
        let directory = TestDirectory::new("manifest-mismatch");
        let worker = DeveloperWorker::open(&directory.0).expect("developer worker");
        drop(worker);
        let catalog =
            SemanticCatalog::open(directory.0.join("catalog.sqlite")).expect("developer catalog");
        let mut incompatible = developer_manifest();
        incompatible.model_revision = "different-development-revision".into();
        catalog
            .register_library("tenant-other", "library-other", &incompatible)
            .expect("incompatible fixture library");
        drop(catalog);

        assert!(matches!(
            DeveloperWorker::open(&directory.0),
            Err(DeveloperBundleError::Catalog(
                crate::semantic_storage::StorageError::MigrationRequired { .. }
            ))
        ));
    }
}
