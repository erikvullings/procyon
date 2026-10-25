//! Corpus themes: clustering of chunk embeddings with keyword labels.
//!
//! Chunks, not documents, are clustered: a document is multi-topic and a JSONL
//! feed holds thousands of unrelated records, so one document vector blurs
//! them. Each document instead gets a distribution over themes (its mixture)
//! and, separately, a persisted centroid for similar-document and large-corpus
//! use. Everything here is deterministic for the same ordered input, seed and
//! configuration, bounded in memory (the fit uses a stride sample above a size
//! threshold and every other chunk is assigned to the nearest fitted centroid),
//! cancellable, and free of any LLM.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::corpus_projection;
use crate::corpus_theme_storage::{
    CorpusChunk, CorpusDocument, CorpusScope, StoredDocumentCentroid,
};
use crate::embedding::{EmbeddingModelIdentity, VectorNormalization};
use crate::kmeans;
use crate::semantic_storage::{SemanticCatalog, StorageError};

/// Versioned clustering and labelling contract.
pub const ALGORITHM_VERSION: &str = "corpus-themes-kmeans/1";
/// Versioned document-centroid contract.
pub const CENTROID_ALGORITHM_VERSION: &str = "document-centroid-mean/1";
/// Model identity of the deterministic token-hashing development profile.
pub const TOKEN_HASHING_MODEL_ID: &str = "procyon.dev.hashing-embedding";

const MAX_EXCERPT_CHARS: usize = 240;
const MAP_EXCERPT_CHARS: usize = 120;
const MAX_TERMS_PER_CLASS: usize = 50_000;
const MAX_CONCEPTS_PER_CLASS: usize = 1_024;
const SILHOUETTE_SAMPLE: usize = 300;
const MIN_POPULATION_FOR_RADIUS: usize = 8;

/// Tunable, bounded clustering parameters. All fields feed the run fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusThemeConfig {
    /// Seed for the deterministic initialization.
    pub seed: u64,
    /// Smallest number of themes tried.
    pub min_themes: usize,
    /// Largest number of themes tried.
    pub max_themes: usize,
    /// Hard k-means convergence bound.
    pub max_iterations: usize,
    /// Fewer chunks than this cannot form themes.
    pub min_chunks: usize,
    /// Largest number of chunks held in memory for fitting; above this the fit
    /// uses a deterministic stride sample.
    pub max_fit_chunks: usize,
    /// Clusters smaller than this become "no theme".
    pub min_theme_chunks: usize,
    /// Distance deviation (in standard deviations) beyond which a chunk is
    /// "no theme" even inside a large cluster.
    pub outlier_sigma: f32,
    /// Keywords reported per theme.
    pub keywords_per_theme: usize,
    /// Representative passages reported per theme.
    pub passages_per_theme: usize,
    /// Document mixtures retained in the result.
    pub max_document_mixtures: usize,
    /// Chunks projected onto the 2D map (stride sample above this bound).
    pub max_map_points: usize,
    /// Largest fraction of chunks that may have changed since the previous run for
    /// its fitted themes to be kept (stable theme ids, no refit).
    pub incremental_max_change: f32,
}

impl Default for CorpusThemeConfig {
    fn default() -> Self {
        Self {
            seed: 0,
            min_themes: 2,
            max_themes: 12,
            max_iterations: 32,
            min_chunks: 6,
            max_fit_chunks: 20_000,
            min_theme_chunks: 3,
            outlier_sigma: 2.5,
            keywords_per_theme: 6,
            passages_per_theme: 3,
            max_document_mixtures: 5_000,
            max_map_points: 2_000,
            incremental_max_change: 0.2,
        }
    }
}

impl CorpusThemeConfig {
    fn validate(&self) -> Result<(), CorpusThemeError> {
        if self.min_themes == 0
            || self.min_themes > self.max_themes
            || self.max_themes > 64
            || self.max_iterations == 0
            || self.max_iterations > 100
            || self.min_chunks < 2
            || self.max_fit_chunks < self.min_chunks
            || self.min_theme_chunks == 0
            || !self.outlier_sigma.is_finite()
            || self.outlier_sigma <= 0.0
            || self.keywords_per_theme == 0
            || self.keywords_per_theme > 32
            || self.passages_per_theme == 0
            || self.passages_per_theme > 16
            || self.max_map_points == 0
            || self.max_map_points > 10_000
            || !self.incremental_max_change.is_finite()
            || !(0.0..=1.0).contains(&self.incremental_max_change)
        {
            return Err(CorpusThemeError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// One scored label term for a theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeKeyword {
    /// Lower-cased term.
    pub term: String,
    /// c-TF-IDF weight.
    pub score: f32,
}

/// A real chunk nearest a theme's centroid, with enough provenance to open it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemePassage {
    /// Chunk identity.
    pub chunk_id: String,
    /// Logical document identity.
    pub document_id: String,
    /// Authorized source identity.
    pub source_id: String,
    /// Position within the document.
    pub source_position: u32,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Serialized provenance; line numbers for JSONL records.
    pub provenance: String,
    /// Bounded, whitespace-collapsed opening of the chunk text.
    pub excerpt: String,
    /// Squared distance from the centroid (zero for "no theme").
    pub distance: f32,
}

/// A SKOS concept shared by many chunks of a theme (task 0242).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeConcept {
    /// Stable concept identity as indexed.
    pub concept_id: String,
    /// Chunks of the theme carrying it.
    pub chunk_count: u64,
}

/// One discovered theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Id within the analysis. A fresh fit numbers themes by size, largest first; a kept
    /// (incremental) fit keeps every theme's id while the list stays ordered by size.
    pub id: u32,
    /// Chunks assigned to the theme.
    pub chunk_count: u64,
    /// Documents with at least one chunk in the theme.
    pub document_count: u32,
    /// Share of all analysed chunks.
    pub share: f32,
    /// Top c-TF-IDF terms.
    pub keywords: Vec<ThemeKeyword>,
    /// Nearest-to-centroid chunks.
    pub passages: Vec<ThemePassage>,
    /// Most common SKOS concepts, when the library was labelled.
    pub concepts: Vec<ThemeConcept>,
}

/// Chunks that fit no theme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoTheme {
    /// Chunks in the bucket.
    pub chunk_count: u64,
    /// Documents with at least one such chunk.
    pub document_count: u32,
    /// Share of all analysed chunks.
    pub share: f32,
    /// A few examples, in document order.
    pub passages: Vec<ThemePassage>,
}

/// One slice of a document's theme distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeShare {
    /// Theme id, or `None` for the "no theme" bucket.
    pub theme: Option<u32>,
    /// Chunks of the document in this bucket.
    pub chunk_count: u32,
    /// Fraction of the document's chunks.
    pub share: f32,
}

/// A document's distribution over themes, largest share first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentThemeMixture {
    /// Logical document identity.
    pub document_id: String,
    /// Authorized source identity.
    pub source_id: String,
    /// Analysed chunks of the document.
    pub chunk_count: u32,
    /// Buckets with at least one chunk.
    pub shares: Vec<ThemeShare>,
}

/// One chunk on the 2D theme map. Only projected coordinates are exposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPoint {
    /// Horizontal coordinate in `[-1, 1]`.
    pub x: f32,
    /// Vertical coordinate in `[-1, 1]`.
    pub y: f32,
    /// Theme id, or `None` for the "no theme" bucket.
    pub theme: Option<u32>,
    /// Chunk identity, source and provenance so the point can open its source.
    pub passage: ThemePassage,
}

/// Complete analysis of one scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusThemes {
    /// Algorithm identity.
    pub algorithm_version: String,
    /// Embedding model and revision that produced every analysed vector.
    pub model_tag: String,
    /// Digest of the documents and generations analysed.
    pub index_revision: String,
    /// Digest of everything that determines this result.
    pub fingerprint: String,
    /// Eligible chunks analysed.
    pub chunk_count: u64,
    /// Documents analysed.
    pub document_count: u32,
    /// Whether the fit used a stride sample of the chunks.
    pub sampled: bool,
    /// Chunks the fit actually clustered.
    pub fitted_chunks: u32,
    /// Themes, largest first.
    pub themes: Vec<Theme>,
    /// The "no theme" bucket.
    pub no_theme: NoTheme,
    /// Per-document distributions, bounded by the configuration.
    pub document_mixtures: Vec<DocumentThemeMixture>,
    /// Whether mixtures beyond the bound were left out.
    pub mixtures_truncated: bool,
    /// Deterministic PCA projection of a bounded sample of chunks.
    pub map_points: Vec<MapPoint>,
    /// Whether the map shows a sample rather than every chunk.
    pub map_sampled: bool,
    /// Centroids recomputed during this run.
    pub centroids_recomputed: u32,
    /// Centroids reused unchanged during this run.
    pub centroids_reused: u32,
    /// Whether the result was served from the persisted run.
    pub from_cache: bool,
    /// Whether the previous fit was kept because few chunks changed (stable theme ids).
    pub incremental: bool,
}

/// Sources contributing to one theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeSources {
    /// Authorized source identities, strongest contribution first.
    pub source_ids: Vec<String>,
    /// Whether weaker sources were left out to respect the limit.
    pub truncated: bool,
}

/// Cheap, vector-free facts about a scope, used for the stale-index indicator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusThemeStatus {
    /// Digest of the documents and generations currently visible.
    pub index_revision: String,
    /// Visible documents in the scope.
    pub document_count: u32,
    /// Eligible chunks in the scope.
    pub chunk_count: u64,
    /// Active embedding model identity.
    pub model_id: String,
    /// Whether the active profile is too weak to give meaningful themes.
    pub profile_too_weak: bool,
}

/// Why themes could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum CorpusThemeError {
    /// The compact token-hashing profile cannot separate topics meaningfully.
    #[error("the active embedding profile is too weak for meaningful themes")]
    ProfileTooWeak {
        /// Offending model identity.
        model_id: String,
    },
    /// Indexed vectors do not belong to the active embedding profile.
    #[error("indexed vectors do not match the active embedding profile")]
    ProfileMismatch,
    /// No authorized chunk exists in the scope.
    #[error("the scope has no analysable content")]
    EmptyScope,
    /// Too few chunks to form themes.
    #[error("the scope has too few chunks to form themes")]
    InsufficientContent {
        /// Chunks found.
        chunks: u64,
    },
    /// Configuration is outside its documented bounds.
    #[error("corpus theme configuration is invalid")]
    InvalidConfiguration,
    /// Vectors are non-finite or inconsistent.
    #[error("indexed embeddings are invalid")]
    InvalidEmbeddings,
    /// Cancellation was requested.
    #[error("corpus theme analysis was cancelled")]
    Cancelled,
    /// The themes the caller holds no longer describe the current index.
    #[error("the corpus themes are stale; analyse again")]
    ThemesStale,
    /// A persisted run could not be decoded.
    #[error("persisted theme run is corrupt")]
    CorruptRun,
    /// Catalog failure.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The worker reported a failure whose detail stays on the worker side.
    #[error("the semantic worker failed the corpus theme request")]
    Remote,
}

/// Whether `identity` is the deterministic token-hashing profile.
#[must_use]
pub fn is_compact_hashing_profile(identity: &EmbeddingModelIdentity) -> bool {
    identity.model_id == TOKEN_HASHING_MODEL_ID || identity.model_revision.contains("token-hashing")
}

/// Fitted cluster geometry. Never leaves the worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ThemeModel {
    centroids: Vec<Vec<f32>>,
    /// Largest squared distance still considered part of the cluster.
    radii: Vec<f32>,
    /// Final theme id per cluster, `None` when the cluster dissolved.
    cluster_theme: Vec<Option<u32>>,
}

impl ThemeModel {
    /// Final theme id of one vector, or `None` for "no theme".
    fn theme_of(&self, vector: &[f32]) -> Option<u32> {
        self.cluster_of(vector)
            .0
            .and_then(|cluster| self.cluster_theme[cluster])
    }

    /// Bucket of one vector: `Some(cluster)` or `None` for "no theme".
    fn cluster_of(&self, vector: &[f32]) -> (Option<usize>, f32) {
        let (cluster, distance) = kmeans::nearest_centroid(vector, &self.centroids);
        let member = self.cluster_theme[cluster].is_some() && distance <= self.radii[cluster];
        (member.then_some(cluster), distance)
    }
}

/// Everything persisted for an idempotent re-request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoredThemeRun {
    /// Digest of everything that determines the fitted clusters (not the index revision).
    pub(crate) fit_key: String,
    pub(crate) themes: CorpusThemes,
    pub(crate) model: ThemeModel,
    pub(crate) documents: Vec<CorpusDocument>,
}

#[derive(Debug, Clone)]
struct FitPoint {
    vector: Vec<f32>,
}

/// Document totals reported once all of a document's chunks were seen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DocumentTotals {
    pub(crate) document_id: String,
    pub(crate) generation: u64,
    pub(crate) content_hash: String,
    pub(crate) chunk_count: u32,
    pub(crate) centroid: Option<Vec<f32>>,
}

/// Worker-side corpus theme capability.
#[derive(Clone)]
pub struct CorpusThemeService {
    catalog: SemanticCatalog,
    identity: Arc<EmbeddingModelIdentity>,
}

impl CorpusThemeService {
    /// Creates the capability over the worker catalog and active embedding profile.
    #[must_use]
    pub fn new(catalog: SemanticCatalog, identity: EmbeddingModelIdentity) -> Self {
        Self {
            catalog,
            identity: Arc::new(identity),
        }
    }

    /// Reports the scope's current revision and whether themes are meaningful.
    ///
    /// # Errors
    ///
    /// Returns storage failures.
    pub fn status(&self, scope: &CorpusScope) -> Result<CorpusThemeStatus, CorpusThemeError> {
        let documents = self.catalog.corpus_documents(scope)?;
        Ok(CorpusThemeStatus {
            index_revision: revision_of(&documents),
            document_count: u32::try_from(documents.len()).unwrap_or(u32::MAX),
            chunk_count: documents
                .iter()
                .map(|document| u64::from(document.chunk_count))
                .sum(),
            model_id: self.identity.model_id.clone(),
            profile_too_weak: is_compact_hashing_profile(&self.identity),
        })
    }

    /// Embedding model tag bound into every centroid and run.
    #[must_use]
    pub fn model_tag(&self) -> String {
        model_tag(&self.identity)
    }

    /// Digest of the documents currently visible in `scope`, for stale-index checks.
    ///
    /// # Errors
    ///
    /// Returns storage failures.
    pub fn index_revision(&self, scope: &CorpusScope) -> Result<String, CorpusThemeError> {
        Ok(revision_of(&self.catalog.corpus_documents(scope)?))
    }

    /// Reads one document centroid only when it matches the active profile.
    ///
    /// # Errors
    ///
    /// Returns storage failures.
    pub fn document_centroid(
        &self,
        scope: &CorpusScope,
        document_id: &str,
    ) -> Result<Option<StoredDocumentCentroid>, CorpusThemeError> {
        Ok(self
            .catalog
            .document_centroid(&scope.tenant_id, &scope.library_id, document_id)?
            .filter(|stored| self.centroid_matches_profile(stored)))
    }

    /// Clusters the scope's chunk embeddings into labelled themes.
    ///
    /// # Errors
    ///
    /// Returns typed refusals for weak or mismatched profiles, empty or tiny
    /// scopes, invalid vectors, cancellation, and storage failures.
    pub fn analyze(
        &self,
        scope: &CorpusScope,
        config: &CorpusThemeConfig,
        cancellation: &CancellationToken,
    ) -> Result<CorpusThemes, CorpusThemeError> {
        config.validate()?;
        if is_compact_hashing_profile(&self.identity) {
            return Err(CorpusThemeError::ProfileTooWeak {
                model_id: self.identity.model_id.clone(),
            });
        }
        let manifest = self
            .catalog
            .library_manifest(&scope.tenant_id, &scope.library_id)?
            .ok_or(CorpusThemeError::Storage(
                StorageError::LibraryNotRegistered,
            ))?;
        if manifest.model_revision != self.identity.model_revision
            || manifest.dimensions != self.identity.dimensions
        {
            return Err(CorpusThemeError::ProfileMismatch);
        }
        let documents = self.catalog.corpus_documents(scope)?;
        let total_chunks = documents
            .iter()
            .map(|document| u64::from(document.chunk_count))
            .sum::<u64>();
        if total_chunks == 0 {
            return Err(CorpusThemeError::EmptyScope);
        }
        if total_chunks < config.min_chunks as u64 {
            return Err(CorpusThemeError::InsufficientContent {
                chunks: total_chunks,
            });
        }
        let index_revision = revision_of(&documents);
        let fingerprint = self.fingerprint(scope, config, &index_revision);
        let fit_key = self.fit_key(scope, config);
        let previous = self
            .catalog
            .corpus_theme_run(scope)?
            .and_then(|(stored, json)| {
                serde_json::from_str::<StoredThemeRun>(&json)
                    .ok()
                    .map(|run| (stored, run))
            });
        if let Some((stored, run)) = &previous
            && *stored == fingerprint
        {
            let mut themes = run.themes.clone();
            themes.from_cache = true;
            return Ok(themes);
        }

        // Few changed chunks keep the fitted themes, so ids stay stable and the
        // expensive fit is skipped; many changes refit from scratch.
        let kept = previous.as_ref().filter(|(_, run)| {
            run.fit_key == fit_key
                && !run.themes.themes.is_empty()
                && changed_fraction(&run.documents, &documents)
                    <= f64::from(config.incremental_max_change)
        });
        let (model, sampled, fitted_chunks, incremental) = if let Some((_, run)) = kept {
            (
                run.model.clone(),
                run.themes.sampled,
                run.themes.fitted_chunks,
                true,
            )
        } else {
            let stride = sample_stride(total_chunks, config.max_fit_chunks);
            let fit_points = self.collect_fit_points(scope, stride, cancellation)?;
            let model = fit_model(&fit_points, config, cancellation)?;
            let fitted = u32::try_from(fit_points.len()).unwrap_or(u32::MAX);
            (model, stride > 1, fitted, false)
        };

        let mut accumulator = ThemeAccumulator::new(
            config.clone(),
            model,
            self.identity.dimensions,
            total_chunks,
        );
        let mut failure: Option<CorpusThemeError> = None;
        let mut recomputed = 0u32;
        let mut reused = 0u32;
        let mut seen = 0u64;
        self.catalog.visit_corpus_chunks(scope, |chunk| {
            seen += 1;
            if seen.is_multiple_of(256) && cancellation.is_cancelled() {
                failure = Some(CorpusThemeError::Cancelled);
                return false;
            }
            if chunk.vector.len() != self.identity.dimensions
                || chunk.vector.iter().any(|value| !value.is_finite())
            {
                failure = Some(CorpusThemeError::InvalidEmbeddings);
                return false;
            }
            if let Some(totals) = accumulator.push(&chunk)
                && let Err(error) =
                    self.persist_centroid(scope, &totals, &mut recomputed, &mut reused)
            {
                failure = Some(error);
                return false;
            }
            true
        })?;
        if let Some(error) = failure {
            return Err(error);
        }
        if cancellation.is_cancelled() {
            return Err(CorpusThemeError::Cancelled);
        }
        let last = accumulator.flush_document();
        if let Some(totals) = last {
            self.persist_centroid(scope, &totals, &mut recomputed, &mut reused)?;
        }
        let (mut themes, model) = accumulator.finish(RunFacts {
            algorithm_version: ALGORITHM_VERSION,
            model_tag: self.model_tag(),
            index_revision,
            fingerprint: fingerprint.clone(),
            chunk_count: total_chunks,
            document_count: u32::try_from(documents.len()).unwrap_or(u32::MAX),
            sampled,
            fitted_chunks,
            incremental,
        });
        themes.centroids_recomputed = recomputed;
        themes.centroids_reused = reused;
        let run = StoredThemeRun {
            fit_key,
            themes: themes.clone(),
            model,
            documents,
        };
        self.catalog.store_corpus_theme_run(
            scope,
            &fingerprint,
            &serde_json::to_string(&run).map_err(|_| CorpusThemeError::CorruptRun)?,
        )?;
        Ok(themes)
    }

    fn collect_fit_points(
        &self,
        scope: &CorpusScope,
        stride: u64,
        cancellation: &CancellationToken,
    ) -> Result<Vec<FitPoint>, CorpusThemeError> {
        let mut points = Vec::new();
        let mut index = 0u64;
        let mut failure = None;
        self.catalog.visit_corpus_chunks(scope, |chunk| {
            if index.is_multiple_of(256) && cancellation.is_cancelled() {
                failure = Some(CorpusThemeError::Cancelled);
                return false;
            }
            if chunk.vector.len() != self.identity.dimensions
                || chunk.vector.iter().any(|value| !value.is_finite())
            {
                failure = Some(CorpusThemeError::InvalidEmbeddings);
                return false;
            }
            if index.is_multiple_of(stride) {
                points.push(FitPoint {
                    vector: chunk.vector,
                });
            }
            index += 1;
            true
        })?;
        failure.map_or(Ok(points), Err)
    }

    fn persist_centroid(
        &self,
        scope: &CorpusScope,
        totals: &DocumentTotals,
        recomputed: &mut u32,
        reused: &mut u32,
    ) -> Result<(), CorpusThemeError> {
        let Some(vector) = totals.centroid.clone() else {
            return Ok(());
        };
        let current = self.catalog.document_centroid(
            &scope.tenant_id,
            &scope.library_id,
            &totals.document_id,
        )?;
        if current.as_ref().is_some_and(|stored| {
            stored.generation == totals.generation
                && stored.content_hash == totals.content_hash
                && self.centroid_matches_profile(stored)
        }) {
            *reused += 1;
            return Ok(());
        }
        self.catalog
            .upsert_document_centroid(&StoredDocumentCentroid {
                tenant_id: scope.tenant_id.clone(),
                library_id: scope.library_id.clone(),
                document_id: totals.document_id.clone(),
                generation: totals.generation,
                content_hash: totals.content_hash.clone(),
                model_tag: self.model_tag(),
                normalization: VectorNormalization::L2,
                algorithm_version: CENTROID_ALGORITHM_VERSION.to_owned(),
                chunk_count: totals.chunk_count,
                vector,
            })?;
        *recomputed += 1;
        Ok(())
    }

    fn centroid_matches_profile(&self, stored: &StoredDocumentCentroid) -> bool {
        stored.model_tag == self.model_tag()
            && stored.algorithm_version == CENTROID_ALGORITHM_VERSION
            && stored.normalization == VectorNormalization::L2
            && stored.vector.len() == self.identity.dimensions
    }

    /// Digest of the inputs that determine the fitted clusters.
    fn fit_key(&self, scope: &CorpusScope, config: &CorpusThemeConfig) -> String {
        let mut digest = Sha256::new();
        digest.update(ALGORITHM_VERSION.as_bytes());
        digest.update([0]);
        digest.update(self.model_tag().as_bytes());
        digest.update([0]);
        digest.update(scope.scope_key().as_bytes());
        digest.update([0]);
        digest.update(
            serde_json::to_vec(&(
                config.seed,
                config.min_themes,
                config.max_themes,
                config.max_iterations,
                config.min_chunks,
                config.max_fit_chunks,
                config.min_theme_chunks,
                config.outlier_sigma.to_bits(),
            ))
            .unwrap_or_default(),
        );
        hex(&digest.finalize())
    }

    /// Lists the sources contributing chunks to one theme, strongest first.
    ///
    /// `fingerprint` must be the one of the analysis the caller is looking at,
    /// and the index must not have changed since; otherwise the themes are
    /// stale and the caller has to analyse again. Only chunks of the
    /// authorized `scope` are assigned, so revoked sources never appear.
    ///
    /// # Errors
    ///
    /// Returns [`CorpusThemeError::ThemesStale`] for a mismatching analysis,
    /// plus cancellation and storage failures.
    pub fn theme_sources(
        &self,
        scope: &CorpusScope,
        theme: Option<u32>,
        fingerprint: &str,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<ThemeSources, CorpusThemeError> {
        if cancellation.is_cancelled() {
            return Err(CorpusThemeError::Cancelled);
        }
        let run = self
            .catalog
            .corpus_theme_run(scope)?
            .filter(|(stored, _)| stored == fingerprint)
            .and_then(|(_, json)| serde_json::from_str::<StoredThemeRun>(&json).ok())
            .ok_or(CorpusThemeError::ThemesStale)?;
        let current = revision_of(&self.catalog.corpus_documents(scope)?);
        if current != run.themes.index_revision {
            return Err(CorpusThemeError::ThemesStale);
        }
        let mut counts = std::collections::BTreeMap::<String, u64>::new();
        let mut failure = None;
        let mut seen = 0u64;
        self.catalog.visit_corpus_chunks(scope, |chunk| {
            seen += 1;
            if seen.is_multiple_of(256) && cancellation.is_cancelled() {
                failure = Some(CorpusThemeError::Cancelled);
                return false;
            }
            if chunk.vector.len() != self.identity.dimensions {
                failure = Some(CorpusThemeError::InvalidEmbeddings);
                return false;
            }
            if run.model.theme_of(&chunk.vector) == theme {
                *counts.entry(chunk.source_id).or_default() += 1;
            }
            true
        })?;
        if let Some(error) = failure {
            return Err(error);
        }
        if cancellation.is_cancelled() {
            return Err(CorpusThemeError::Cancelled);
        }
        let mut ranked = counts.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let truncated = ranked.len() > limit;
        ranked.truncate(limit);
        Ok(ThemeSources {
            source_ids: ranked.into_iter().map(|(source_id, _)| source_id).collect(),
            truncated,
        })
    }

    fn fingerprint(
        &self,
        scope: &CorpusScope,
        config: &CorpusThemeConfig,
        index_revision: &str,
    ) -> String {
        let mut digest = Sha256::new();
        digest.update(ALGORITHM_VERSION.as_bytes());
        digest.update([0]);
        digest.update(self.model_tag().as_bytes());
        digest.update([0]);
        digest.update(scope.scope_key().as_bytes());
        digest.update([0]);
        digest.update(index_revision.as_bytes());
        digest.update(serde_json::to_vec(config).unwrap_or_default());
        hex(&digest.finalize())
    }
}

/// Fraction of the current chunks living in new or changed documents, counting
/// removed documents against the larger of the two corpora.
fn changed_fraction(previous: &[CorpusDocument], current: &[CorpusDocument]) -> f64 {
    let before = previous
        .iter()
        .map(|document| (document.document_id.as_str(), document))
        .collect::<HashMap<_, _>>();
    let mut changed = 0u64;
    let mut total = 0u64;
    for document in current {
        total += u64::from(document.chunk_count);
        let unchanged = before
            .get(document.document_id.as_str())
            .is_some_and(|old| {
                old.generation == document.generation && old.content_hash == document.content_hash
            });
        if !unchanged {
            changed += u64::from(document.chunk_count);
        }
    }
    let kept = current
        .iter()
        .map(|document| document.document_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let removed = previous
        .iter()
        .filter(|document| !kept.contains(document.document_id.as_str()))
        .map(|document| u64::from(document.chunk_count))
        .sum::<u64>();
    let base = total.max(removed + changed).max(1);
    (changed + removed) as f64 / base as f64
}

/// Facts about one finished run, recorded in the result.
pub(crate) struct RunFacts {
    pub(crate) algorithm_version: &'static str,
    pub(crate) model_tag: String,
    pub(crate) index_revision: String,
    pub(crate) fingerprint: String,
    pub(crate) chunk_count: u64,
    pub(crate) document_count: u32,
    pub(crate) sampled: bool,
    pub(crate) fitted_chunks: u32,
    pub(crate) incremental: bool,
}

fn model_tag(identity: &EmbeddingModelIdentity) -> String {
    format!(
        "{}@{}#{}",
        identity.model_id, identity.model_revision, identity.dimensions
    )
}

fn revision_of(documents: &[CorpusDocument]) -> String {
    let mut digest = Sha256::new();
    for document in documents {
        digest.update(document.document_id.as_bytes());
        digest.update([0]);
        digest.update(document.generation.to_le_bytes());
        digest.update(document.content_hash.as_bytes());
        digest.update([0]);
        digest.update(document.chunk_count.to_le_bytes());
    }
    hex(&digest.finalize())
}

fn sample_stride(total: u64, max_fit: usize) -> u64 {
    total.div_ceil(max_fit as u64).max(1)
}

/// Fits k-means with automatic k chosen by silhouette over a bounded sample.
fn fit_model(
    points: &[FitPoint],
    config: &CorpusThemeConfig,
    cancellation: &CancellationToken,
) -> Result<ThemeModel, CorpusThemeError> {
    let count = points.len();
    let views = points
        .iter()
        .map(|point| point.vector.as_slice())
        .collect::<Vec<_>>();
    let maximum = config.max_themes.min(count / 2).max(1);
    let minimum = config.min_themes.min(maximum);
    let sample = silhouette_sample(count);
    let distances = pairwise_distances(&views, &sample);
    let mut best: Option<(f32, kmeans::KMeans)> = None;
    for clusters in minimum..=maximum {
        let seed = (usize::try_from(config.seed % count as u64).unwrap_or(0) + clusters) % count;
        let result = kmeans::run(&views, clusters, seed, config.max_iterations, cancellation)
            .map_err(|kmeans::Cancelled| CorpusThemeError::Cancelled)?;
        let score = silhouette(&sample, &distances, &result.assignments, clusters);
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score + 1e-6)
        {
            best = Some((score, result));
        }
    }
    let (_, result) = best.ok_or(CorpusThemeError::InvalidConfiguration)?;
    Ok(describe_clusters(&views, &result, config))
}

fn describe_clusters(
    views: &[&[f32]],
    result: &kmeans::KMeans,
    config: &CorpusThemeConfig,
) -> ThemeModel {
    let clusters = result.centroids.len();
    let mut distances = vec![Vec::<f32>::new(); clusters];
    for (point, assignment) in views.iter().zip(&result.assignments) {
        distances[*assignment]
            .push(kmeans::squared_distance(point, &result.centroids[*assignment]).sqrt());
    }
    let mut radii = Vec::with_capacity(clusters);
    let mut kept = Vec::with_capacity(clusters);
    for members in &distances {
        let dissolved = members.len() < config.min_theme_chunks;
        kept.push(!dissolved);
        if members.len() < MIN_POPULATION_FOR_RADIUS {
            radii.push(f32::MAX);
            continue;
        }
        let mean = members.iter().sum::<f32>() / members.len() as f32;
        let variance = members
            .iter()
            .map(|value| (value - mean) * (value - mean))
            .sum::<f32>()
            / members.len() as f32;
        let limit = mean + config.outlier_sigma * variance.sqrt();
        radii.push(limit * limit);
    }
    ThemeModel {
        centroids: result.centroids.clone(),
        radii,
        cluster_theme: kept.iter().map(|keep| keep.then_some(0)).collect(),
    }
}

fn silhouette_sample(count: usize) -> Vec<usize> {
    let size = count.min(SILHOUETTE_SAMPLE);
    (0..size).map(|index| index * count / size).collect()
}

fn pairwise_distances(views: &[&[f32]], sample: &[usize]) -> Vec<f32> {
    let size = sample.len();
    let mut matrix = vec![0.0; size * size];
    for left in 0..size {
        for right in (left + 1)..size {
            let distance =
                kmeans::squared_distance(views[sample[left]], views[sample[right]]).sqrt();
            matrix[left * size + right] = distance;
            matrix[right * size + left] = distance;
        }
    }
    matrix
}

fn silhouette(sample: &[usize], distances: &[f32], assignments: &[usize], clusters: usize) -> f32 {
    let size = sample.len();
    let labels = sample
        .iter()
        .map(|index| assignments[*index])
        .collect::<Vec<_>>();
    let mut populations = vec![0usize; clusters];
    for label in &labels {
        populations[*label] += 1;
    }
    let mut total = 0.0f32;
    for point in 0..size {
        let own = labels[point];
        if populations[own] < 2 {
            continue;
        }
        let mut sums = vec![0.0f32; clusters];
        for other in 0..size {
            if other != point {
                sums[labels[other]] += distances[point * size + other];
            }
        }
        let inside = sums[own] / (populations[own] - 1) as f32;
        let outside = (0..clusters)
            .filter(|cluster| *cluster != own && populations[*cluster] > 0)
            .map(|cluster| sums[cluster] / populations[cluster] as f32)
            .min_by(f32::total_cmp);
        if let Some(outside) = outside {
            let scale = inside.max(outside);
            if scale > 0.0 {
                total += (outside - inside) / scale;
            }
        }
    }
    total / size as f32
}

#[derive(Default)]
struct ClassStats {
    chunks: u64,
    documents: u32,
    last_document: Option<u32>,
    terms: HashMap<String, u32>,
    total_terms: u64,
    passages: Vec<ThemePassage>,
    concepts: HashMap<String, u64>,
}

struct DocumentState {
    document_id: String,
    source_id: String,
    ordinal: u32,
    generation: u64,
    content_hash: String,
    counts: Vec<u32>,
    sum: Vec<f64>,
    chunks: u32,
}

/// Streams chunks through a fitted model and gathers labels and mixtures.
pub(crate) struct ThemeAccumulator {
    config: CorpusThemeConfig,
    model: ThemeModel,
    classes: Vec<ClassStats>,
    mixtures: Vec<(String, String, u32, Vec<u32>)>,
    current: Option<DocumentState>,
    next_ordinal: u32,
    dimensions: usize,
    map_stride: u64,
    chunk_index: u64,
    map_candidates: Vec<MapCandidate>,
}

/// A chunk kept for the map, before theme ids are final.
struct MapCandidate {
    vector: Vec<f32>,
    bucket: usize,
    passage: ThemePassage,
}

impl ThemeAccumulator {
    pub(crate) fn new(
        config: CorpusThemeConfig,
        model: ThemeModel,
        dimensions: usize,
        total_chunks: u64,
    ) -> Self {
        let buckets = model.centroids.len() + 1;
        let map_stride = total_chunks.div_ceil(config.max_map_points as u64).max(1);
        Self {
            map_stride,
            chunk_index: 0,
            map_candidates: Vec::new(),
            config,
            model,
            classes: (0..buckets).map(|_| ClassStats::default()).collect(),
            mixtures: Vec::new(),
            current: None,
            next_ordinal: 0,
            dimensions,
        }
    }

    /// Adds one chunk; returns totals when this chunk starts a new document.
    pub(crate) fn push(&mut self, chunk: &CorpusChunk) -> Option<DocumentTotals> {
        let finished = if self
            .current
            .as_ref()
            .is_some_and(|state| state.document_id != chunk.document_id)
        {
            self.flush_document()
        } else {
            None
        };
        let buckets = self.classes.len();
        let dimensions = self.dimensions;
        let ordinal = &mut self.next_ordinal;
        let state = self.current.get_or_insert_with(|| {
            let state = DocumentState {
                document_id: chunk.document_id.clone(),
                source_id: chunk.source_id.clone(),
                ordinal: *ordinal,
                generation: chunk.generation,
                content_hash: chunk.content_hash.clone(),
                counts: vec![0; buckets],
                sum: vec![0.0; dimensions],
                chunks: 0,
            };
            *ordinal += 1;
            state
        });
        let (member, distance) = self.model.cluster_of(&chunk.vector);
        let bucket = member.unwrap_or(buckets - 1);
        state.counts[bucket] += 1;
        state.chunks += 1;
        for (sum, value) in state.sum.iter_mut().zip(&chunk.vector) {
            *sum += f64::from(*value);
        }
        let ordinal = state.ordinal;
        let outlier = member.is_none();
        let class = &mut self.classes[bucket];
        class.chunks += 1;
        if class.last_document != Some(ordinal) {
            class.last_document = Some(ordinal);
            class.documents += 1;
        }
        if !outlier {
            count_terms(class, &chunk.text);
            if let Some(concept) = &chunk.concept_id
                && (class.concepts.len() < MAX_CONCEPTS_PER_CLASS
                    || class.concepts.contains_key(concept))
            {
                *class.concepts.entry(concept.clone()).or_default() += 1;
            }
        }
        let passage = passage_of(chunk, if outlier { 0.0 } else { distance });
        if self.chunk_index.is_multiple_of(self.map_stride) {
            self.map_candidates.push(MapCandidate {
                vector: chunk.vector.clone(),
                bucket,
                passage: ThemePassage {
                    excerpt: passage.excerpt.chars().take(MAP_EXCERPT_CHARS).collect(),
                    ..passage.clone()
                },
            });
        }
        self.chunk_index += 1;
        if outlier {
            if class.passages.len() < self.config.passages_per_theme {
                class.passages.push(passage);
            }
        } else {
            keep_nearest(&mut class.passages, passage, self.config.passages_per_theme);
        }
        finished
    }

    /// Closes the open document, if any.
    pub(crate) fn flush_document(&mut self) -> Option<DocumentTotals> {
        let state = self.current.take()?;
        if self.mixtures.len() < self.config.max_document_mixtures {
            self.mixtures.push((
                state.document_id.clone(),
                state.source_id.clone(),
                state.chunks,
                state.counts.clone(),
            ));
        }
        Some(DocumentTotals {
            centroid: mean_unit(&state.sum, state.chunks),
            document_id: state.document_id,
            generation: state.generation,
            content_hash: state.content_hash,
            chunk_count: state.chunks,
        })
    }

    pub(crate) fn finish(mut self, facts: RunFacts) -> (CorpusThemes, ThemeModel) {
        let RunFacts {
            algorithm_version,
            model_tag,
            index_revision,
            fingerprint,
            chunk_count,
            document_count,
            sampled,
            fitted_chunks,
            incremental,
        } = facts;
        let outlier_bucket = self.model.centroids.len();
        let mut order = (0..outlier_bucket)
            .filter(|bucket| self.classes[*bucket].chunks > 0)
            .collect::<Vec<_>>();
        order.sort_by(|left, right| {
            self.classes[*right]
                .chunks
                .cmp(&self.classes[*left].chunks)
                .then_with(|| left.cmp(right))
        });
        // A kept fit keeps its theme ids; a fresh fit numbers themes by size.
        let mut cluster_theme = vec![None; outlier_bucket];
        for (position, bucket) in order.iter().enumerate() {
            cluster_theme[*bucket] = if incremental {
                self.model.cluster_theme[*bucket]
            } else {
                Some(u32::try_from(position).unwrap_or(u32::MAX))
            };
        }
        let keywords = label_keywords(&self.classes, &order, self.config.keywords_per_theme);
        let total = chunk_count.max(1) as f32;
        let themes = order
            .iter()
            .zip(keywords)
            .map(|(bucket, keywords)| {
                let class = std::mem::take(&mut self.classes[*bucket]);
                Theme {
                    id: cluster_theme[*bucket].unwrap_or(u32::MAX),
                    chunk_count: class.chunks,
                    document_count: class.documents,
                    share: class.chunks as f32 / total,
                    keywords,
                    passages: class.passages,
                    concepts: top_concepts(class.concepts),
                }
            })
            .collect::<Vec<_>>();
        let outlier = std::mem::take(&mut self.classes[outlier_bucket]);
        let no_theme = NoTheme {
            chunk_count: outlier.chunks,
            document_count: outlier.documents,
            share: outlier.chunks as f32 / total,
            passages: outlier.passages,
        };
        let mixtures_truncated =
            u64::try_from(self.mixtures.len()).unwrap_or(u64::MAX) < u64::from(document_count);
        let document_mixtures = self
            .mixtures
            .iter()
            .map(|(document_id, source_id, chunks, counts)| {
                let mut shares = counts
                    .iter()
                    .enumerate()
                    .filter(|(_, count)| **count > 0)
                    .map(|(bucket, count)| ThemeShare {
                        theme: cluster_theme.get(bucket).copied().flatten(),
                        chunk_count: *count,
                        share: *count as f32 / *chunks as f32,
                    })
                    .collect::<Vec<_>>();
                shares.sort_by(|left, right| {
                    right
                        .chunk_count
                        .cmp(&left.chunk_count)
                        .then_with(|| left.theme.cmp(&right.theme))
                });
                DocumentThemeMixture {
                    document_id: document_id.clone(),
                    source_id: source_id.clone(),
                    chunk_count: *chunks,
                    shares,
                }
            })
            .collect();
        let coordinates = corpus_projection::project_2d(
            &self
                .map_candidates
                .iter()
                .map(|candidate| candidate.vector.as_slice())
                .collect::<Vec<_>>(),
        );
        let map_sampled = (self.map_candidates.len() as u64) < chunk_count;
        let map_points = self
            .map_candidates
            .into_iter()
            .zip(coordinates)
            .map(|(candidate, (x, y))| MapPoint {
                x,
                y,
                theme: cluster_theme.get(candidate.bucket).copied().flatten(),
                passage: candidate.passage,
            })
            .collect();
        let mut model = self.model;
        model.cluster_theme = cluster_theme;
        (
            CorpusThemes {
                algorithm_version: algorithm_version.to_owned(),
                model_tag,
                index_revision,
                fingerprint,
                chunk_count,
                document_count,
                sampled,
                fitted_chunks,
                themes,
                no_theme,
                document_mixtures,
                mixtures_truncated,
                map_points,
                map_sampled,
                centroids_recomputed: 0,
                centroids_reused: 0,
                from_cache: false,
                incremental,
            },
            model,
        )
    }
}

fn mean_unit(sum: &[f64], chunks: u32) -> Option<Vec<f32>> {
    if chunks == 0 {
        return None;
    }
    let mean = sum
        .iter()
        .map(|value| value / f64::from(chunks))
        .collect::<Vec<_>>();
    let norm = mean.iter().map(|value| value * value).sum::<f64>().sqrt();
    (norm > 1e-12 && norm.is_finite())
        .then(|| mean.iter().map(|value| (value / norm) as f32).collect())
}

fn passage_of(chunk: &CorpusChunk, distance: f32) -> ThemePassage {
    ThemePassage {
        chunk_id: chunk.chunk_id.clone(),
        document_id: chunk.document_id.clone(),
        source_id: chunk.source_id.clone(),
        source_position: chunk.source_position,
        section_path: chunk.section_path.clone(),
        provenance: chunk.provenance.clone(),
        excerpt: excerpt(&chunk.text),
        distance,
    }
}

fn excerpt(text: &str) -> String {
    let mut collapsed = String::new();
    for word in text.split_whitespace() {
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(word);
        if collapsed.chars().count() >= MAX_EXCERPT_CHARS {
            break;
        }
    }
    collapsed.chars().take(MAX_EXCERPT_CHARS).collect()
}

fn keep_nearest(passages: &mut Vec<ThemePassage>, passage: ThemePassage, limit: usize) {
    let order = |left: &ThemePassage, right: &ThemePassage| {
        left.distance
            .total_cmp(&right.distance)
            .then_with(|| left.document_id.cmp(&right.document_id))
            .then_with(|| left.source_position.cmp(&right.source_position))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    };
    let position = passages
        .binary_search_by(|existing| order(existing, &passage))
        .unwrap_or_else(|position| position);
    if position < limit {
        passages.insert(position, passage);
        passages.truncate(limit);
    }
}

fn top_concepts(concepts: HashMap<String, u64>) -> Vec<ThemeConcept> {
    let mut concepts = concepts
        .into_iter()
        .map(|(concept_id, chunk_count)| ThemeConcept {
            concept_id,
            chunk_count,
        })
        .collect::<Vec<_>>();
    concepts.sort_by(|left, right| {
        right
            .chunk_count
            .cmp(&left.chunk_count)
            .then_with(|| left.concept_id.cmp(&right.concept_id))
    });
    concepts.truncate(3);
    concepts
}

fn count_terms(class: &mut ClassStats, text: &str) {
    for term in terms(text) {
        class.total_terms += 1;
        if let Some(count) = class.terms.get_mut(&term) {
            *count += 1;
        } else if class.terms.len() < MAX_TERMS_PER_CLASS {
            class.terms.insert(term, 1);
        }
    }
}

fn terms(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| {
            let length = word.chars().count();
            (3..=24).contains(&length) && !word.chars().all(char::is_numeric)
        })
        .map(str::to_lowercase)
        .filter(|word| !STOPWORDS.contains(&word.as_str()))
}

/// c-TF-IDF: class term frequency weighted by `ln(1 + A / total frequency)`.
fn label_keywords(classes: &[ClassStats], order: &[usize], limit: usize) -> Vec<Vec<ThemeKeyword>> {
    let average = if order.is_empty() {
        1.0
    } else {
        order
            .iter()
            .map(|bucket| classes[*bucket].total_terms as f64)
            .sum::<f64>()
            / order.len() as f64
    }
    .max(1.0);
    order
        .iter()
        .map(|bucket| {
            let class = &classes[*bucket];
            let length = class.total_terms.max(1) as f64;
            let mut scored = class
                .terms
                .iter()
                .map(|(term, count)| {
                    let overall = order
                        .iter()
                        .map(|other| {
                            f64::from(classes[*other].terms.get(term).copied().unwrap_or(0))
                        })
                        .sum::<f64>();
                    let weight = f64::from(*count) / length * (1.0 + average / overall).ln();
                    (term, weight)
                })
                .collect::<Vec<_>>();
            scored.sort_by(|left, right| {
                right.1.total_cmp(&left.1).then_with(|| left.0.cmp(right.0))
            });
            scored
                .into_iter()
                .take(limit)
                .map(|(term, score)| ThemeKeyword {
                    term: term.clone(),
                    score: score as f32,
                })
                .collect()
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("writing to a string cannot fail");
    }
    text
}

/// Frequent function words in the languages the app ships, never useful as labels.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "that", "with", "this", "from", "are", "was", "were", "has", "have",
    "had", "not", "but", "you", "your", "can", "will", "all", "any", "its", "our", "their", "than",
    "then", "there", "which", "who", "whom", "what", "when", "where", "how", "also", "een", "het",
    "van", "dat", "die", "voor", "met", "zijn", "niet", "ook", "maar", "als", "naar", "bij",
    "deze", "dit", "kan", "worden", "wordt", "door", "over", "aan", "uit", "der", "die", "das",
    "und", "ist", "nicht", "mit", "von", "den", "dem", "ein", "eine", "auch", "auf", "sich", "für",
    "les", "des", "une", "est", "pas", "que", "qui", "dans", "pour", "sur", "par", "avec", "sont",
    "los", "las", "una", "por", "con", "para", "del", "como", "pero", "más", "que", "gli", "che",
    "non", "per", "con", "sono", "una", "uno", "dei", "nel", "nella", "como", "uma", "não", "mais",
    "são", "dos", "das", "nie", "jest", "się", "oraz", "ale", "dla", "przez", "czy",
];

#[cfg(test)]
#[path = "corpus_themes_tests.rs"]
mod tests;
