//! CPU-first local embedding contracts and reproducibility metadata.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// Immutable identity of the exact embedding model and tokenizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingModelIdentity {
    /// Catalog-owned model identifier.
    pub model_id: String,
    /// Immutable upstream model revision.
    pub model_revision: String,
    /// Tokenizer identity including its immutable revision.
    pub tokenizer: String,
    /// Number of values in every produced vector.
    pub dimensions: usize,
    /// Maximum accepted tokens for one input.
    pub max_input_tokens: usize,
}

/// A locally installed, catalog-verified model package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedModelPackage {
    /// Exact immutable model identity.
    pub identity: EmbeddingModelIdentity,
    /// Local package directory. Loaders must not perform network access.
    pub directory: PathBuf,
}

/// Resource policy applied to CPU tokenization and inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingResourceProfile {
    /// Lowest background impact.
    Eco,
    /// Default balance between throughput and responsiveness.
    Balanced,
    /// Highest bounded local throughput.
    Fast,
}

impl EmbeddingResourceProfile {
    const fn limits(self) -> BatchLimits {
        match self {
            Self::Eco => BatchLimits {
                inputs: 2,
                tokens: 1_024,
            },
            Self::Balanced => BatchLimits {
                inputs: 8,
                tokens: 4_096,
            },
            Self::Fast => BatchLimits {
                inputs: 32,
                tokens: 16_384,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BatchLimits {
    inputs: usize,
    tokens: usize,
}

/// Normalization applied before vectors enter the cache or index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VectorNormalization {
    /// Unit-length L2 normalization.
    L2,
}

/// A CPU inference backend loaded only from an installed local package.
///
/// Implementations receive bounded batches and must not make remote requests.
pub trait CpuEmbeddingBackend: Send + Sync {
    /// Returns the exact model loaded by this backend.
    fn identity(&self) -> &EmbeddingModelIdentity;

    /// Counts tokens using the loaded immutable tokenizer.
    fn token_count(&self, input: &str) -> Result<usize, EmbeddingError>;

    /// Embeds one bounded batch using local CPU inference.
    fn embed_batch(
        &self,
        inputs: &[&str],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

/// Loads a CPU backend from a verified local package.
pub trait CpuEmbeddingLoader {
    /// Loads the package without consulting any network service.
    fn load(
        &self,
        package: &CuratedModelPackage,
    ) -> Result<Box<dyn CpuEmbeddingBackend>, EmbeddingError>;
}

/// Bounded local embedding runtime.
pub struct LocalEmbeddingRuntime {
    backend: Box<dyn CpuEmbeddingBackend>,
    profile: EmbeddingResourceProfile,
    normalization: VectorNormalization,
}

impl LocalEmbeddingRuntime {
    /// Loads an immutable local model package.
    ///
    /// # Errors
    ///
    /// Refuses missing package directories and loaders that return a different
    /// model revision than the catalog-selected package.
    pub fn load(
        package: &CuratedModelPackage,
        loader: &dyn CpuEmbeddingLoader,
        profile: EmbeddingResourceProfile,
        normalization: VectorNormalization,
    ) -> Result<Self, EmbeddingError> {
        if !package.directory.is_dir() {
            return Err(EmbeddingError::PackageUnavailable(
                package.directory.clone(),
            ));
        }
        let backend = loader.load(package)?;
        if backend.identity() != &package.identity {
            return Err(EmbeddingError::ModelIdentityMismatch);
        }
        if package.identity.dimensions == 0 || package.identity.max_input_tokens == 0 {
            return Err(EmbeddingError::InvalidModelLimits);
        }
        Ok(Self {
            backend,
            profile,
            normalization,
        })
    }

    /// Returns the exact loaded model identity.
    #[must_use]
    pub fn identity(&self) -> &EmbeddingModelIdentity {
        self.backend.identity()
    }

    /// Returns the deterministic output normalization.
    #[must_use]
    pub const fn normalization(&self) -> VectorNormalization {
        self.normalization
    }

    /// Tokenizes, batches, embeds, validates, and normalizes inputs.
    ///
    /// # Errors
    ///
    /// Returns a typed error for cancellation, input/model limits, backend
    /// failures, dimension mismatch, or non-finite/zero vectors.
    pub fn embed(
        &self,
        inputs: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if cancellation.is_cancelled() {
            return Err(EmbeddingError::Cancelled);
        }
        let limits = self.profile.limits();
        let mut token_counts = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let count = self.backend.token_count(input)?;
            if count > self.identity().max_input_tokens {
                return Err(EmbeddingError::InputTooLarge {
                    index,
                    tokens: count,
                    maximum: self.identity().max_input_tokens,
                });
            }
            token_counts.push(count);
        }

        let mut output = Vec::with_capacity(inputs.len());
        let mut start = 0;
        while start < inputs.len() {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            let mut end = start;
            let mut batch_tokens = 0_usize;
            while end < inputs.len()
                && end - start < limits.inputs
                && batch_tokens.saturating_add(token_counts[end]) <= limits.tokens
            {
                batch_tokens += token_counts[end];
                end += 1;
            }
            if end == start {
                end += 1;
            }
            let refs = inputs[start..end]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let vectors = self.backend.embed_batch(&refs, cancellation)?;
            if vectors.len() != refs.len() {
                return Err(EmbeddingError::OutputCountMismatch {
                    expected: refs.len(),
                    actual: vectors.len(),
                });
            }
            for vector in vectors {
                output.push(normalize_vector(
                    vector,
                    self.identity().dimensions,
                    self.normalization,
                )?);
            }
            start = end;
        }
        Ok(output)
    }
}

/// Reproducible key for one cached embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EmbeddingCacheKey([u8; 32]);

impl EmbeddingCacheKey {
    /// Hashes every input that can affect the output vector.
    #[must_use]
    pub fn calculate(
        normalized_input: &str,
        identity: &EmbeddingModelIdentity,
        tokenizer_settings: &str,
        chunker_version: &str,
    ) -> Self {
        let mut hasher = Sha256::new();
        for part in [
            normalized_input,
            identity.model_id.as_str(),
            identity.model_revision.as_str(),
            identity.tokenizer.as_str(),
            tokenizer_settings,
            chunker_version,
        ] {
            hasher.update((part.len() as u64).to_le_bytes());
            hasher.update(part.as_bytes());
        }
        Self(hasher.finalize().into())
    }

    /// Returns the stable binary key.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

fn normalize_vector(
    mut vector: Vec<f32>,
    dimensions: usize,
    normalization: VectorNormalization,
) -> Result<Vec<f32>, EmbeddingError> {
    if vector.len() != dimensions {
        return Err(EmbeddingError::DimensionMismatch {
            expected: dimensions,
            actual: vector.len(),
        });
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(EmbeddingError::NonFiniteVector);
    }
    match normalization {
        VectorNormalization::L2 => {
            let norm = vector
                .iter()
                .map(|value| f64::from(*value).powi(2))
                .sum::<f64>()
                .sqrt();
            if norm == 0.0 {
                return Err(EmbeddingError::ZeroVector);
            }
            for value in &mut vector {
                *value = (f64::from(*value) / norm) as f32;
            }
        }
    }
    Ok(vector)
}

/// Local embedding failure.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The installed model package cannot be read.
    #[error("model package is unavailable at {0}")]
    PackageUnavailable(PathBuf),
    /// The loaded backend does not match the selected immutable package.
    #[error("loaded model identity does not match the selected package")]
    ModelIdentityMismatch,
    /// Model dimensions or token limits are zero.
    #[error("model dimensions and token limit must be non-zero")]
    InvalidModelLimits,
    /// One input exceeds the immutable model limit.
    #[error("input {index} has {tokens} tokens, exceeding maximum {maximum}")]
    InputTooLarge {
        /// Input position.
        index: usize,
        /// Actual token count.
        tokens: usize,
        /// Accepted model limit.
        maximum: usize,
    },
    /// Inference was cancelled.
    #[error("embedding cancelled")]
    Cancelled,
    /// Backend returned a different number of vectors.
    #[error("backend returned {actual} vectors for {expected} inputs")]
    OutputCountMismatch {
        /// Expected vector count.
        expected: usize,
        /// Returned vector count.
        actual: usize,
    },
    /// Backend returned the wrong dimensions.
    #[error("embedding has {actual} dimensions, expected {expected}")]
    DimensionMismatch {
        /// Expected dimensions.
        expected: usize,
        /// Actual dimensions.
        actual: usize,
    },
    /// A vector contains NaN or infinity.
    #[error("embedding contains a non-finite value")]
    NonFiniteVector,
    /// A vector cannot be normalized.
    #[error("embedding is the zero vector")]
    ZeroVector,
    /// Backend-specific local failure.
    #[error("local embedding backend failed: {0}")]
    Backend(String),
}

/// Reports whether a path is local model-package input.
#[must_use]
pub fn is_local_package_path(path: &Path) -> bool {
    path.is_absolute() && path.components().next().is_some()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tempfile::tempdir;

    use super::*;

    struct FakeLoader {
        batches: std::sync::Arc<Mutex<Vec<usize>>>,
        returned_dimensions: usize,
    }

    struct FakeBackend {
        identity: EmbeddingModelIdentity,
        batches: std::sync::Arc<Mutex<Vec<usize>>>,
        returned_dimensions: usize,
    }

    impl CpuEmbeddingLoader for FakeLoader {
        fn load(
            &self,
            package: &CuratedModelPackage,
        ) -> Result<Box<dyn CpuEmbeddingBackend>, EmbeddingError> {
            Ok(Box::new(FakeBackend {
                identity: package.identity.clone(),
                batches: self.batches.clone(),
                returned_dimensions: self.returned_dimensions,
            }))
        }
    }

    impl CpuEmbeddingBackend for FakeBackend {
        fn identity(&self) -> &EmbeddingModelIdentity {
            &self.identity
        }

        fn token_count(&self, input: &str) -> Result<usize, EmbeddingError> {
            Ok(input.split_whitespace().count())
        }

        fn embed_batch(
            &self,
            inputs: &[&str],
            cancellation: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            self.batches.lock().expect("batch lock").push(inputs.len());
            Ok(inputs
                .iter()
                .map(|input| {
                    let seed = input.bytes().map(f32::from).sum::<f32>().max(1.0);
                    (0..self.returned_dimensions)
                        .map(|index| seed + index as f32)
                        .collect()
                })
                .collect())
        }
    }

    fn identity() -> EmbeddingModelIdentity {
        EmbeddingModelIdentity {
            model_id: "test-model".into(),
            model_revision: "sha256-abc".into(),
            tokenizer: "test-tokenizer-r1".into(),
            dimensions: 3,
            max_input_tokens: 512,
        }
    }

    #[test]
    fn embeddings_are_deterministic_normalized_and_bounded() {
        let directory = tempdir().expect("temp directory");
        let batches = std::sync::Arc::new(Mutex::new(Vec::new()));
        let runtime = LocalEmbeddingRuntime::load(
            &CuratedModelPackage {
                identity: identity(),
                directory: directory.path().into(),
            },
            &FakeLoader {
                batches: batches.clone(),
                returned_dimensions: 3,
            },
            EmbeddingResourceProfile::Eco,
            VectorNormalization::L2,
        )
        .expect("runtime");
        let inputs = vec!["one".into(), "two".into(), "three".into()];

        let first = runtime
            .embed(&inputs, &CancellationToken::new())
            .expect("embeddings");
        let second = runtime
            .embed(&inputs, &CancellationToken::new())
            .expect("embeddings");

        assert_eq!(first, second);
        assert_eq!(*batches.lock().expect("batch lock"), vec![2, 1, 2, 1]);
        for vector in first {
            let norm = vector.iter().map(|value| value * value).sum::<f32>();
            assert!((norm - 1.0).abs() < 0.000_01);
        }
    }

    #[test]
    fn dimension_mismatch_is_typed() {
        let directory = tempdir().expect("temp directory");
        let runtime = LocalEmbeddingRuntime::load(
            &CuratedModelPackage {
                identity: identity(),
                directory: directory.path().into(),
            },
            &FakeLoader {
                batches: Default::default(),
                returned_dimensions: 2,
            },
            EmbeddingResourceProfile::Balanced,
            VectorNormalization::L2,
        )
        .expect("runtime");

        assert!(matches!(
            runtime.embed(&["text".into()], &CancellationToken::new()),
            Err(EmbeddingError::DimensionMismatch {
                expected: 3,
                actual: 2
            })
        ));
    }

    #[test]
    fn cancellation_prevents_inference() {
        let directory = tempdir().expect("temp directory");
        let runtime = LocalEmbeddingRuntime::load(
            &CuratedModelPackage {
                identity: identity(),
                directory: directory.path().into(),
            },
            &FakeLoader {
                batches: Default::default(),
                returned_dimensions: 3,
            },
            EmbeddingResourceProfile::Balanced,
            VectorNormalization::L2,
        )
        .expect("runtime");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        assert!(matches!(
            runtime.embed(&["text".into()], &cancellation),
            Err(EmbeddingError::Cancelled)
        ));
    }

    #[test]
    fn cache_key_binds_every_embedding_input() {
        let identity = identity();
        let base = EmbeddingCacheKey::calculate("body", &identity, "lowercase", "structural/2");
        assert_eq!(
            base,
            EmbeddingCacheKey::calculate("body", &identity, "lowercase", "structural/2")
        );
        assert_ne!(
            base,
            EmbeddingCacheKey::calculate("body", &identity, "preserve-case", "structural/2")
        );
        assert_ne!(
            base,
            EmbeddingCacheKey::calculate("body", &identity, "lowercase", "structural/3")
        );
    }
}
