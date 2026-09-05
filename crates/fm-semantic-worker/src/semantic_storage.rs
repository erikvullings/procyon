//! Authoritative SQLite lifecycle catalog for worker-owned semantic indexes.
//!
//! Zvec is a derived retrieval index. Visibility, generations, occurrence
//! ownership, jobs, component revisions, and vector references are decided
//! here and survive worker restarts.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::embedding::{EmbeddingCacheKey, VectorNormalization};

const CATALOG_SCHEMA_VERSION: i64 = 1;

/// Distance metric bound into one library's immutable index manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceMetric {
    /// Cosine similarity over consistently normalized FP32 vectors.
    Cosine,
}

/// Complete compatibility contract for one device-local semantic library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryIndexManifest {
    /// Procyon schema version for the derived Zvec collection.
    pub zvec_schema_version: u32,
    /// Fixed embedding dimensions.
    pub dimensions: usize,
    /// Fixed vector distance metric.
    pub distance_metric: DistanceMetric,
    /// Exact immutable model revision.
    pub model_revision: String,
    /// Exact immutable tokenizer identity and revision.
    pub tokenizer: String,
    /// Conversion contract version.
    pub converter_version: String,
    /// Structural chunking contract version.
    pub chunker_version: String,
    /// Vector normalization contract.
    pub normalization: VectorNormalization,
}

impl LibraryIndexManifest {
    fn validate(&self) -> Result<(), StorageError> {
        if self.zvec_schema_version == 0
            || self.dimensions == 0
            || [
                self.model_revision.as_str(),
                self.tokenizer.as_str(),
                self.converter_version.as_str(),
                self.chunker_version.as_str(),
            ]
            .iter()
            .any(|value| value.is_empty())
        {
            return Err(StorageError::InvalidManifest);
        }
        Ok(())
    }

    fn incompatible_fields(&self, requested: &Self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.zvec_schema_version != requested.zvec_schema_version {
            fields.push("zvec_schema_version");
        }
        if self.dimensions != requested.dimensions {
            fields.push("dimensions");
        }
        if self.distance_metric != requested.distance_metric {
            fields.push("distance_metric");
        }
        if self.model_revision != requested.model_revision {
            fields.push("model_revision");
        }
        if self.tokenizer != requested.tokenizer {
            fields.push("tokenizer");
        }
        if self.converter_version != requested.converter_version {
            fields.push("converter_version");
        }
        if self.chunker_version != requested.chunker_version {
            fields.push("chunker_version");
        }
        if self.normalization != requested.normalization {
            fields.push("normalization");
        }
        fields
    }
}

/// Measured derived-index strategy for a library size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIndexKind {
    /// Exact scan for small collections.
    Flat,
    /// Approximate HNSW for larger collections.
    Hnsw,
}

/// Chooses an index from an externally measured threshold.
///
/// Task 0188 owns the benchmark that selects the production threshold; this
/// function deliberately does not embed an unevaluated global default.
#[must_use]
pub const fn choose_index_kind(
    searchable_records: u64,
    measured_flat_max_records: u64,
) -> VectorIndexKind {
    if searchable_records <= measured_flat_max_records {
        VectorIndexKind::Flat
    } else {
        VectorIndexKind::Hnsw
    }
}

/// One source occurrence retaining access scope and provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// Stable occurrence identity.
    pub occurrence_id: String,
    /// Opaque source locator understood by the host.
    pub source_id: String,
    /// Enrolled root identity.
    pub root_id: String,
    /// Optional workspace scope.
    pub workspace_id: Option<String>,
    /// Structured content type.
    pub media_type: String,
    /// Source modification time in Unix milliseconds.
    pub modified_at_ms: i64,
    /// Whether the source is currently available.
    pub available: bool,
    /// Bounded serialized provenance without source excerpts.
    pub provenance: String,
}

/// A derived chunk or summary record staged for retrieval.
#[derive(Debug, Clone)]
pub struct StagedRecord {
    /// Stable derived record identity.
    pub record_id: String,
    /// Owning occurrence.
    pub occurrence_id: String,
    /// Shared deterministic embedding-cache key.
    pub cache_key: EmbeddingCacheKey,
    /// Normalized FP32 vector.
    pub vector: Vec<f32>,
    /// Structured record kind, such as `chunk` or `summary`.
    pub record_kind: String,
    /// Optional SKOS concept identity.
    pub concept_id: Option<String>,
}

/// One complete staged document generation.
#[derive(Debug, Clone)]
pub struct StagedGeneration {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled semantic library.
    pub library_id: String,
    /// Globally deduplicated content identity.
    pub document_id: String,
    /// Streamed source-content digest.
    pub content_hash: String,
    /// Monotonically increasing generation.
    pub generation: u64,
    /// Current source occurrences represented by this generation.
    pub occurrences: Vec<Occurrence>,
    /// Derived searchable records.
    pub records: Vec<StagedRecord>,
}

/// Structured query filters enforced by worker storage.
#[derive(Debug, Clone, Default)]
pub struct QueryFilters {
    /// Mandatory tenant boundary.
    pub tenant_id: String,
    /// Optional library.
    pub library_id: Option<String>,
    /// Optional enrolled root.
    pub root_id: Option<String>,
    /// Optional workspace.
    pub workspace_id: Option<String>,
    /// Optional media type.
    pub media_type: Option<String>,
    /// Optional minimum modification time.
    pub modified_from_ms: Option<i64>,
    /// Optional maximum modification time.
    pub modified_to_ms: Option<i64>,
    /// Optional SKOS concept.
    pub concept_id: Option<String>,
    /// Optional exact generation.
    pub generation: Option<u64>,
    /// Whether unavailable occurrences may be returned.
    pub include_unavailable: bool,
}

/// Occurrence-level evidence authorized by the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryEvidence {
    /// Derived record selected by the vector index.
    pub record_id: String,
    /// Owning semantic library.
    pub library_id: String,
    /// Content identity.
    pub document_id: String,
    /// Source occurrence.
    pub occurrence_id: String,
    /// Opaque host source identity.
    pub source_id: String,
    /// Bounded serialized provenance.
    pub provenance: String,
    /// Published generation.
    pub generation: u64,
}

/// Authoritative worker-side semantic catalog.
#[derive(Clone)]
pub struct SemanticCatalog {
    path: PathBuf,
    active_readers: Arc<AtomicUsize>,
}

impl SemanticCatalog {
    /// Opens or creates a catalog and applies its bounded schema migration.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the parent, database, or schema
    /// cannot be opened safely.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let catalog = Self {
            path,
            active_readers: Arc::new(AtomicUsize::new(0)),
        };
        let connection = catalog.connection()?;
        initialize_schema(&connection)?;
        Ok(catalog)
    }

    /// Returns the durable database path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Registers a new library or verifies exact manifest compatibility.
    ///
    /// # Errors
    ///
    /// Existing incompatible state returns [`StorageError::MigrationRequired`]
    /// before any derived index is opened.
    pub fn register_library(
        &self,
        tenant_id: &str,
        library_id: &str,
        manifest: &LibraryIndexManifest,
    ) -> Result<(), StorageError> {
        validate_identifier(tenant_id, "tenant_id")?;
        validate_identifier(library_id, "library_id")?;
        manifest.validate()?;
        let connection = self.connection()?;
        let existing = connection
            .query_row(
                "SELECT manifest_json FROM libraries
                 WHERE tenant_id = ?1 AND library_id = ?2",
                params![tenant_id, library_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let existing: LibraryIndexManifest = serde_json::from_str(&existing)?;
            let fields = existing.incompatible_fields(manifest);
            if !fields.is_empty() {
                return Err(StorageError::MigrationRequired {
                    incompatible_fields: fields,
                });
            }
            return Ok(());
        }
        connection.execute(
            "INSERT INTO libraries (tenant_id, library_id, manifest_json)
             VALUES (?1, ?2, ?3)",
            params![tenant_id, library_id, serde_json::to_string(manifest)?],
        )?;
        Ok(())
    }

    /// Stages one complete generation while leaving the published generation visible.
    ///
    /// # Errors
    ///
    /// Rejects missing libraries, dimensions that do not match the manifest,
    /// inconsistent cache entries, and invalid identities.
    pub fn stage_generation(&self, staged: &StagedGeneration) -> Result<(), StorageError> {
        validate_staged_generation(staged)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let manifest = load_manifest(&transaction, &staged.tenant_id, &staged.library_id)?;
        for record in &staged.records {
            if record.vector.len() != manifest.dimensions {
                return Err(StorageError::DimensionMismatch {
                    expected: manifest.dimensions,
                    actual: record.vector.len(),
                });
            }
            if record.vector.iter().any(|value| !value.is_finite()) {
                return Err(StorageError::NonFiniteVector);
            }
        }

        transaction.execute(
            "INSERT INTO documents (document_id, content_hash)
             VALUES (?1, ?2)
             ON CONFLICT(document_id) DO UPDATE SET content_hash = excluded.content_hash",
            params![staged.document_id, staged.content_hash],
        )?;
        transaction.execute(
            "INSERT INTO generations
             (tenant_id, library_id, document_id, generation, state)
             VALUES (?1, ?2, ?3, ?4, 'staging')
             ON CONFLICT(tenant_id, library_id, document_id, generation)
             DO NOTHING",
            params![
                staged.tenant_id,
                staged.library_id,
                staged.document_id,
                i64_generation(staged.generation)?
            ],
        )?;

        for occurrence in &staged.occurrences {
            insert_occurrence(&transaction, staged, occurrence)?;
        }
        for record in &staged.records {
            insert_record(&transaction, staged, record)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Atomically replaces the visible generation for one document.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::GenerationNotStaged`] unless the requested
    /// generation was durably staged first.
    pub fn publish_generation(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
        generation: u64,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = i64_generation(generation)?;
        let state = transaction
            .query_row(
                "SELECT state FROM generations
                 WHERE tenant_id = ?1 AND library_id = ?2
                   AND document_id = ?3 AND generation = ?4",
                params![tenant_id, library_id, document_id, generation],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if state.as_deref() != Some("staging") && state.as_deref() != Some("complete") {
            return Err(StorageError::GenerationNotStaged);
        }
        transaction.execute(
            "UPDATE generations SET state = 'superseded'
             WHERE tenant_id = ?1 AND library_id = ?2 AND document_id = ?3
               AND state = 'complete' AND generation <> ?4",
            params![tenant_id, library_id, document_id, generation],
        )?;
        transaction.execute(
            "UPDATE generations SET state = 'complete'
             WHERE tenant_id = ?1 AND library_id = ?2
               AND document_id = ?3 AND generation = ?4",
            params![tenant_id, library_id, document_id, generation],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Starts an in-process read lease that delays derived-record reclamation.
    #[must_use]
    pub fn begin_read(&self) -> CatalogReader {
        self.active_readers.fetch_add(1, Ordering::AcqRel);
        CatalogReader {
            catalog: self.clone(),
        }
    }

    /// Reclaims superseded records and unreferenced cached vectors.
    ///
    /// # Errors
    ///
    /// Reclamation is deferred while an in-flight reader may still refer to a
    /// superseded record.
    pub fn reclaim_superseded(&self) -> Result<ReclaimStats, StorageError> {
        if self.active_readers.load(Ordering::Acquire) != 0 {
            return Err(StorageError::ReadersActive);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut statement = transaction.prepare(
            "SELECT r.record_id, r.cache_key
             FROM records r
             JOIN generations g
               ON g.tenant_id = r.tenant_id
              AND g.library_id = r.library_id
              AND g.document_id = r.document_id
              AND g.generation = r.generation
             WHERE g.state = 'superseded'",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (record_id, cache_key) in &rows {
            transaction.execute("DELETE FROM records WHERE record_id = ?1", [record_id])?;
            transaction.execute(
                "UPDATE vectors SET reference_count = reference_count - 1
                 WHERE cache_key = ?1",
                [cache_key],
            )?;
        }
        let removed_vectors =
            transaction.execute("DELETE FROM vectors WHERE reference_count <= 0", [])?;
        transaction.execute(
            "DELETE FROM occurrences
             WHERE EXISTS (
               SELECT 1 FROM generations g
               WHERE g.state = 'superseded'
                 AND g.tenant_id = occurrences.tenant_id
                 AND g.library_id = occurrences.library_id
                 AND g.document_id = occurrences.document_id
                 AND g.generation = occurrences.generation
             )",
            [],
        )?;
        transaction.execute("DELETE FROM generations WHERE state = 'superseded'", [])?;
        transaction.commit()?;
        Ok(ReclaimStats {
            records: rows.len(),
            vectors: removed_vectors,
        })
    }

    /// Returns one cache key's authoritative reference count.
    ///
    /// # Errors
    ///
    /// Returns a database error when the catalog cannot be read.
    pub fn vector_reference_count(
        &self,
        cache_key: EmbeddingCacheKey,
    ) -> Result<Option<u64>, StorageError> {
        let connection = self.connection()?;
        let count = connection
            .query_row(
                "SELECT reference_count FROM vectors WHERE cache_key = ?1",
                [cache_key.as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        count
            .map(|value| u64::try_from(value).map_err(|_| StorageError::CorruptCatalog))
            .transpose()
    }

    /// Deletes one source occurrence and releases its cached-vector references.
    ///
    /// The returned record IDs let the ingestion coordinator remove matching
    /// Zvec rows idempotently. Catalog deletion is authoritative, so those
    /// derived rows stop producing evidence before cleanup finishes.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn delete_occurrence(
        &self,
        occurrence_id: &str,
    ) -> Result<DeletedOccurrence, StorageError> {
        validate_identifier(occurrence_id, "occurrence_id")?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut statement = transaction
            .prepare("SELECT record_id, cache_key FROM records WHERE occurrence_id = ?1")?;
        let records = statement
            .query_map([occurrence_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (record_id, cache_key) in &records {
            transaction.execute("DELETE FROM records WHERE record_id = ?1", [record_id])?;
            transaction.execute(
                "UPDATE vectors SET reference_count = reference_count - 1
                 WHERE cache_key = ?1",
                [cache_key],
            )?;
        }
        transaction.execute(
            "DELETE FROM occurrences WHERE occurrence_id = ?1",
            [occurrence_id],
        )?;
        transaction.execute("DELETE FROM vectors WHERE reference_count <= 0", [])?;
        transaction.commit()?;
        Ok(DeletedOccurrence {
            record_ids: records
                .into_iter()
                .map(|(record_id, _)| record_id)
                .collect(),
        })
    }

    /// Persists one ingestion/reconciliation job state.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn upsert_job(
        &self,
        job_id: &str,
        stage: &str,
        attempts: u32,
        detail: Option<&str>,
    ) -> Result<(), StorageError> {
        validate_identifier(job_id, "job_id")?;
        if stage.is_empty() {
            return Err(StorageError::InvalidIdentifier("stage"));
        }
        self.connection()?.execute(
            "INSERT INTO jobs (job_id, stage, attempts, detail)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(job_id) DO UPDATE SET
               stage = excluded.stage,
               attempts = excluded.attempts,
               detail = excluded.detail",
            params![job_id, stage, i64::from(attempts), detail],
        )?;
        Ok(())
    }

    /// Persists the exact installed worker/runtime/model component revision.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn upsert_component(&self, component_id: &str, revision: &str) -> Result<(), StorageError> {
        validate_identifier(component_id, "component_id")?;
        validate_identifier(revision, "component_revision")?;
        self.connection()?.execute(
            "INSERT INTO component_versions (component_id, revision)
             VALUES (?1, ?2)
             ON CONFLICT(component_id) DO UPDATE SET revision = excluded.revision",
            params![component_id, revision],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}

/// Read lease retaining superseded records until evidence materialization ends.
pub struct CatalogReader {
    catalog: SemanticCatalog,
}

impl CatalogReader {
    /// Filters vector-index candidates against an authoritative visibility snapshot.
    ///
    /// Candidate record IDs are untrusted derived-index output. Tenant and
    /// occurrence filters are reapplied here before evidence leaves storage.
    ///
    /// # Errors
    ///
    /// Rejects empty tenants, excessive candidate sets, and catalog failures.
    pub fn filter_visible_candidates(
        &self,
        candidate_record_ids: &[String],
        filters: &QueryFilters,
    ) -> Result<Vec<QueryEvidence>, StorageError> {
        validate_identifier(&filters.tenant_id, "tenant_id")?;
        if candidate_record_ids.len() > 4_096 {
            return Err(StorageError::TooManyCandidates {
                actual: candidate_record_ids.len(),
                maximum: 4_096,
            });
        }
        let mut connection = self.catalog.connection()?;
        let transaction = connection.transaction()?;
        let mut statement = transaction.prepare(
            "SELECT r.record_id, r.library_id, r.document_id, o.occurrence_id,
                    o.source_id, o.provenance, r.generation
             FROM records r
             JOIN generations g
               ON g.tenant_id = r.tenant_id
              AND g.library_id = r.library_id
              AND g.document_id = r.document_id
              AND g.generation = r.generation
             JOIN occurrences o
               ON o.occurrence_id = r.occurrence_id
              AND o.generation = r.generation
             WHERE r.record_id = ?1
               AND r.tenant_id = ?2
               AND g.state = 'complete'
               AND (?3 IS NULL OR r.library_id = ?3)
               AND (?4 IS NULL OR o.root_id = ?4)
               AND (?5 IS NULL OR o.workspace_id = ?5)
               AND (?6 IS NULL OR o.media_type = ?6)
               AND (?7 IS NULL OR o.modified_at_ms >= ?7)
               AND (?8 IS NULL OR o.modified_at_ms <= ?8)
               AND (?9 IS NULL OR r.concept_id = ?9)
               AND (?10 IS NULL OR r.generation = ?10)
               AND (?11 = 1 OR o.available = 1)",
        )?;
        let generation = filters.generation.map(i64_generation).transpose()?;
        let include_unavailable = i64::from(filters.include_unavailable);
        let mut evidence = Vec::new();
        for record_id in candidate_record_ids {
            let result = statement
                .query_row(
                    params![
                        record_id,
                        filters.tenant_id,
                        filters.library_id,
                        filters.root_id,
                        filters.workspace_id,
                        filters.media_type,
                        filters.modified_from_ms,
                        filters.modified_to_ms,
                        filters.concept_id,
                        generation,
                        include_unavailable,
                    ],
                    |row| {
                        let generation = row.get::<_, i64>(6)?;
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            generation,
                        ))
                    },
                )
                .optional()?;
            if let Some((
                record_id,
                library_id,
                document_id,
                occurrence_id,
                source_id,
                provenance,
                generation,
            )) = result
            {
                evidence.push(QueryEvidence {
                    record_id,
                    library_id,
                    document_id,
                    occurrence_id,
                    source_id,
                    provenance,
                    generation: u64::try_from(generation)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                });
            }
        }
        Ok(evidence)
    }
}

impl Drop for CatalogReader {
    fn drop(&mut self) {
        self.catalog.active_readers.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Number of derived rows reclaimed after readers drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReclaimStats {
    /// Removed occurrence-level records.
    pub records: usize,
    /// Removed globally cached vectors.
    pub vectors: usize,
}

/// Derived cleanup requested after authoritative occurrence deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedOccurrence {
    /// Zvec primary keys that may now be deleted.
    pub record_ids: Vec<String>,
}

/// Persistent semantic-storage failure.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// SQLite operation failed.
    #[error("semantic catalog database failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Filesystem setup failed.
    #[error("semantic catalog filesystem failed: {0}")]
    Io(#[from] std::io::Error),
    /// Durable manifest serialization failed.
    #[error("semantic manifest serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Identifier is missing or unsafe.
    #[error("invalid semantic storage identifier: {0}")]
    InvalidIdentifier(&'static str),
    /// Manifest contains a zero or empty compatibility field.
    #[error("invalid semantic library manifest")]
    InvalidManifest,
    /// Existing storage cannot be opened under a different contract.
    #[error("semantic library requires migration: {incompatible_fields:?}")]
    MigrationRequired {
        /// Exact compatibility fields that differ.
        incompatible_fields: Vec<&'static str>,
    },
    /// Library has not been registered with a manifest.
    #[error("semantic library is not registered")]
    LibraryNotRegistered,
    /// Stored or staged vector dimensions differ from the manifest.
    #[error("vector has {actual} dimensions, expected {expected}")]
    DimensionMismatch {
        /// Manifest dimensions.
        expected: usize,
        /// Actual dimensions.
        actual: usize,
    },
    /// Vector contains NaN or infinity.
    #[error("vector contains a non-finite value")]
    NonFiniteVector,
    /// Existing cache key resolves to different vector bytes.
    #[error("embedding cache key collision or incompatible cached vector")]
    CacheConflict,
    /// Publication requires a durable staging generation.
    #[error("document generation was not staged")]
    GenerationNotStaged,
    /// Reclamation must wait for active evidence readers.
    #[error("semantic records are still in use by active readers")]
    ReadersActive,
    /// Query candidate set exceeds the bounded worker limit.
    #[error("query supplied {actual} candidates, maximum is {maximum}")]
    TooManyCandidates {
        /// Candidate count.
        actual: usize,
        /// Maximum candidate count.
        maximum: usize,
    },
    /// Durable integer cannot be represented by the public type.
    #[error("semantic catalog contains invalid persisted data")]
    CorruptCatalog,
}

fn initialize_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.execute_batch(
        "BEGIN IMMEDIATE;
         CREATE TABLE IF NOT EXISTS catalog_meta (
           schema_version INTEGER NOT NULL
         );
         INSERT INTO catalog_meta (schema_version)
           SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM catalog_meta);
         CREATE TABLE IF NOT EXISTS libraries (
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           manifest_json TEXT NOT NULL,
           PRIMARY KEY (tenant_id, library_id)
         );
         CREATE TABLE IF NOT EXISTS documents (
           document_id TEXT PRIMARY KEY,
           content_hash TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS generations (
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           document_id TEXT NOT NULL,
           generation INTEGER NOT NULL,
           state TEXT NOT NULL CHECK (state IN ('staging', 'complete', 'superseded')),
           PRIMARY KEY (tenant_id, library_id, document_id, generation),
           FOREIGN KEY (tenant_id, library_id) REFERENCES libraries (tenant_id, library_id),
           FOREIGN KEY (document_id) REFERENCES documents (document_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_complete_generation
           ON generations (tenant_id, library_id, document_id)
           WHERE state = 'complete';
         CREATE TABLE IF NOT EXISTS occurrences (
           occurrence_id TEXT NOT NULL,
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           document_id TEXT NOT NULL,
           generation INTEGER NOT NULL,
           source_id TEXT NOT NULL,
           root_id TEXT NOT NULL,
           workspace_id TEXT,
           media_type TEXT NOT NULL,
           modified_at_ms INTEGER NOT NULL,
           available INTEGER NOT NULL,
           provenance TEXT NOT NULL,
           PRIMARY KEY (occurrence_id, generation),
           FOREIGN KEY (tenant_id, library_id) REFERENCES libraries (tenant_id, library_id),
           FOREIGN KEY (document_id) REFERENCES documents (document_id)
         );
         CREATE TABLE IF NOT EXISTS vectors (
           cache_key BLOB PRIMARY KEY,
           dimensions INTEGER NOT NULL,
           vector BLOB NOT NULL,
           reference_count INTEGER NOT NULL CHECK (reference_count >= 0)
         );
         CREATE TABLE IF NOT EXISTS records (
           record_id TEXT PRIMARY KEY,
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           document_id TEXT NOT NULL,
           occurrence_id TEXT NOT NULL,
           generation INTEGER NOT NULL,
           cache_key BLOB NOT NULL,
           record_kind TEXT NOT NULL,
           concept_id TEXT,
           FOREIGN KEY (occurrence_id, generation)
             REFERENCES occurrences (occurrence_id, generation),
           FOREIGN KEY (cache_key) REFERENCES vectors (cache_key)
         );
         CREATE INDEX IF NOT EXISTS record_scope
           ON records (tenant_id, library_id, document_id, generation);
         CREATE TABLE IF NOT EXISTS jobs (
           job_id TEXT PRIMARY KEY,
           stage TEXT NOT NULL,
           attempts INTEGER NOT NULL,
           detail TEXT
         );
         CREATE TABLE IF NOT EXISTS component_versions (
           component_id TEXT PRIMARY KEY,
           revision TEXT NOT NULL
         );
         COMMIT;",
    )?;
    let version = connection.query_row("SELECT schema_version FROM catalog_meta", [], |row| {
        row.get::<_, i64>(0)
    })?;
    if version != CATALOG_SCHEMA_VERSION {
        return Err(StorageError::MigrationRequired {
            incompatible_fields: vec!["catalog_schema_version"],
        });
    }
    Ok(())
}

fn load_manifest(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    library_id: &str,
) -> Result<LibraryIndexManifest, StorageError> {
    let manifest = transaction
        .query_row(
            "SELECT manifest_json FROM libraries
             WHERE tenant_id = ?1 AND library_id = ?2",
            params![tenant_id, library_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(StorageError::LibraryNotRegistered)?;
    Ok(serde_json::from_str(&manifest)?)
}

fn insert_occurrence(
    transaction: &Transaction<'_>,
    staged: &StagedGeneration,
    occurrence: &Occurrence,
) -> Result<(), StorageError> {
    for (value, field) in [
        (occurrence.occurrence_id.as_str(), "occurrence_id"),
        (occurrence.source_id.as_str(), "source_id"),
        (occurrence.root_id.as_str(), "root_id"),
        (occurrence.media_type.as_str(), "media_type"),
    ] {
        validate_identifier(value, field)?;
    }
    if let Some(workspace_id) = &occurrence.workspace_id {
        validate_identifier(workspace_id, "workspace_id")?;
    }
    transaction.execute(
        "INSERT INTO occurrences
         (occurrence_id, tenant_id, library_id, document_id, generation,
          source_id, root_id, workspace_id, media_type, modified_at_ms,
          available, provenance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(occurrence_id, generation) DO UPDATE SET
           tenant_id = excluded.tenant_id,
           library_id = excluded.library_id,
           document_id = excluded.document_id,
           source_id = excluded.source_id,
           root_id = excluded.root_id,
           workspace_id = excluded.workspace_id,
           media_type = excluded.media_type,
           modified_at_ms = excluded.modified_at_ms,
           available = excluded.available,
           provenance = excluded.provenance",
        params![
            occurrence.occurrence_id,
            staged.tenant_id,
            staged.library_id,
            staged.document_id,
            i64_generation(staged.generation)?,
            occurrence.source_id,
            occurrence.root_id,
            occurrence.workspace_id,
            occurrence.media_type,
            occurrence.modified_at_ms,
            i64::from(occurrence.available),
            occurrence.provenance
        ],
    )?;
    Ok(())
}

fn insert_record(
    transaction: &Transaction<'_>,
    staged: &StagedGeneration,
    record: &StagedRecord,
) -> Result<(), StorageError> {
    validate_identifier(&record.record_id, "record_id")?;
    validate_identifier(&record.occurrence_id, "occurrence_id")?;
    validate_identifier(&record.record_kind, "record_kind")?;
    if let Some(concept_id) = &record.concept_id {
        validate_identifier(concept_id, "concept_id")?;
    }
    if !staged
        .occurrences
        .iter()
        .any(|occurrence| occurrence.occurrence_id == record.occurrence_id)
    {
        return Err(StorageError::InvalidIdentifier("record occurrence"));
    }
    let existing_record_key = transaction
        .query_row(
            "SELECT cache_key FROM records WHERE record_id = ?1",
            [&record.record_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    if let Some(existing_record_key) = existing_record_key {
        if existing_record_key.as_slice() == record.cache_key.as_bytes() {
            return Ok(());
        }
        return Err(StorageError::CacheConflict);
    }
    let vector_bytes = encode_vector(&record.vector);
    let existing = transaction
        .query_row(
            "SELECT vector FROM vectors WHERE cache_key = ?1",
            [record.cache_key.as_bytes().as_slice()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    if existing
        .as_deref()
        .is_some_and(|value| value != vector_bytes)
    {
        return Err(StorageError::CacheConflict);
    }

    transaction.execute(
        "INSERT INTO vectors (cache_key, dimensions, vector, reference_count)
         VALUES (?1, ?2, ?3, 0)
         ON CONFLICT(cache_key) DO NOTHING",
        params![
            record.cache_key.as_bytes().as_slice(),
            i64::try_from(record.vector.len()).map_err(|_| StorageError::CorruptCatalog)?,
            vector_bytes,
        ],
    )?;
    let inserted = transaction.execute(
        "INSERT INTO records
         (record_id, tenant_id, library_id, document_id, occurrence_id,
          generation, cache_key, record_kind, concept_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(record_id) DO NOTHING",
        params![
            record.record_id,
            staged.tenant_id,
            staged.library_id,
            staged.document_id,
            record.occurrence_id,
            i64_generation(staged.generation)?,
            record.cache_key.as_bytes().as_slice(),
            record.record_kind,
            record.concept_id,
        ],
    )?;
    if inserted != 0 {
        transaction.execute(
            "UPDATE vectors SET reference_count = reference_count + 1
             WHERE cache_key = ?1",
            [record.cache_key.as_bytes().as_slice()],
        )?;
    }
    Ok(())
}

fn validate_staged_generation(staged: &StagedGeneration) -> Result<(), StorageError> {
    for (value, field) in [
        (staged.tenant_id.as_str(), "tenant_id"),
        (staged.library_id.as_str(), "library_id"),
        (staged.document_id.as_str(), "document_id"),
        (staged.content_hash.as_str(), "content_hash"),
    ] {
        validate_identifier(value, field)?;
    }
    if staged.generation == 0 {
        return Err(StorageError::InvalidIdentifier("generation"));
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), StorageError> {
    if value.is_empty()
        || value.len() > 1_024
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(StorageError::InvalidIdentifier(field));
    }
    Ok(())
}

fn i64_generation(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::CorruptCatalog)
}

fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(vector));
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::embedding::EmbeddingModelIdentity;

    fn manifest() -> LibraryIndexManifest {
        LibraryIndexManifest {
            zvec_schema_version: 1,
            dimensions: 3,
            distance_metric: DistanceMetric::Cosine,
            model_revision: "model-sha256-abc".into(),
            tokenizer: "tokenizer-r1".into(),
            converter_version: "plain/1".into(),
            chunker_version: "structural/2".into(),
            normalization: VectorNormalization::L2,
        }
    }

    fn key(input: &str) -> EmbeddingCacheKey {
        EmbeddingCacheKey::calculate(
            input,
            &EmbeddingModelIdentity {
                model_id: "model".into(),
                model_revision: "model-sha256-abc".into(),
                tokenizer: "tokenizer-r1".into(),
                dimensions: 3,
                max_input_tokens: 512,
            },
            "settings-r1",
            "structural/2",
        )
    }

    fn occurrence(id: &str, root: &str) -> Occurrence {
        Occurrence {
            occurrence_id: id.into(),
            source_id: format!("source-{id}"),
            root_id: root.into(),
            workspace_id: Some("workspace-a".into()),
            media_type: "text-plain".into(),
            modified_at_ms: 1_000,
            available: true,
            provenance: format!("page=1;occurrence={id}"),
        }
    }

    fn generation(
        tenant: &str,
        library: &str,
        document: &str,
        generation: u64,
        occurrences: Vec<Occurrence>,
        body: &str,
    ) -> StagedGeneration {
        let cache_key = key(body);
        let records = occurrences
            .iter()
            .map(|occurrence| StagedRecord {
                record_id: format!(
                    "{tenant}-{library}-{document}-{generation}-{}",
                    occurrence.occurrence_id
                ),
                occurrence_id: occurrence.occurrence_id.clone(),
                cache_key,
                vector: vec![0.6, 0.8, 0.0],
                record_kind: "chunk".into(),
                concept_id: Some("concept-a".into()),
            })
            .collect();
        StagedGeneration {
            tenant_id: tenant.into(),
            library_id: library.into(),
            document_id: document.into(),
            content_hash: format!("hash-{generation}"),
            generation,
            occurrences,
            records,
        }
    }

    fn catalog() -> (tempfile::TempDir, SemanticCatalog) {
        let directory = tempdir().expect("temp directory");
        let catalog =
            SemanticCatalog::open(directory.path().join("catalog.sqlite")).expect("catalog");
        (directory, catalog)
    }

    #[test]
    fn incompatible_manifest_requires_migration_before_open() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let mut changed = manifest();
        changed.model_revision = "model-sha256-new".into();
        changed.dimensions = 4;

        assert!(matches!(
            catalog.register_library("tenant-a", "library-a", &changed),
            Err(StorageError::MigrationRequired {
                incompatible_fields
            }) if incompatible_fields == vec!["dimensions", "model_revision"]
        ));
    }

    #[test]
    fn cache_is_shared_while_occurrences_remain_distinct() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let staged = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![
                occurrence("occurrence-a", "root-a"),
                occurrence("occurrence-b", "root-b"),
            ],
            "shared body",
        );
        let shared_key = staged.records[0].cache_key;
        catalog.stage_generation(&staged).expect("stage");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish");

        assert_eq!(
            catalog.vector_reference_count(shared_key).expect("count"),
            Some(2)
        );
        let candidates = staged
            .records
            .iter()
            .map(|record| record.record_id.clone())
            .collect::<Vec<_>>();
        let evidence = catalog
            .begin_read()
            .filter_visible_candidates(
                &candidates,
                &QueryFilters {
                    tenant_id: "tenant-a".into(),
                    ..QueryFilters::default()
                },
            )
            .expect("evidence");
        assert_eq!(evidence.len(), 2);
        assert_ne!(evidence[0].occurrence_id, evidence[1].occurrence_id);

        let deleted = catalog
            .delete_occurrence("occurrence-a")
            .expect("delete occurrence");
        assert_eq!(
            deleted.record_ids,
            vec!["tenant-a-library-a-document-a-1-occurrence-a"]
        );
        assert_eq!(
            catalog.vector_reference_count(shared_key).expect("count"),
            Some(1)
        );
        let remaining = catalog
            .begin_read()
            .filter_visible_candidates(
                &candidates,
                &QueryFilters {
                    tenant_id: "tenant-a".into(),
                    ..QueryFilters::default()
                },
            )
            .expect("remaining evidence");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].occurrence_id, "occurrence-b");
    }

    #[test]
    fn publication_is_old_or_new_and_reclamation_waits_for_readers() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let old = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "old body",
        );
        let new = generation(
            "tenant-a",
            "library-a",
            "document-a",
            2,
            vec![occurrence("occurrence-a", "root-a")],
            "new body",
        );
        catalog.stage_generation(&old).expect("stage old");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish old");
        catalog.stage_generation(&new).expect("stage new");
        let candidates = vec![
            old.records[0].record_id.clone(),
            new.records[0].record_id.clone(),
        ];
        let filters = QueryFilters {
            tenant_id: "tenant-a".into(),
            ..QueryFilters::default()
        };

        let before = catalog
            .begin_read()
            .filter_visible_candidates(&candidates, &filters)
            .expect("before");
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].generation, 1);

        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 2)
            .expect("publish new");
        let reader = catalog.begin_read();
        let after = reader
            .filter_visible_candidates(&candidates, &filters)
            .expect("after");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].generation, 2);
        assert!(matches!(
            catalog.reclaim_superseded(),
            Err(StorageError::ReadersActive)
        ));
        drop(reader);
        assert_eq!(
            catalog.reclaim_superseded().expect("reclaim"),
            ReclaimStats {
                records: 1,
                vectors: 1
            }
        );
    }

    #[test]
    fn structured_filters_and_tenant_boundary_are_authoritative() {
        let (_directory, catalog) = catalog();
        for tenant in ["tenant-a", "tenant-b"] {
            catalog
                .register_library(tenant, "library-a", &manifest())
                .expect("register");
        }
        let tenant_a = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "same body",
        );
        let tenant_b = generation(
            "tenant-b",
            "library-a",
            "document-b",
            1,
            vec![occurrence("occurrence-b", "root-b")],
            "same body",
        );
        for staged in [&tenant_a, &tenant_b] {
            catalog.stage_generation(staged).expect("stage");
            catalog
                .publish_generation(
                    &staged.tenant_id,
                    &staged.library_id,
                    &staged.document_id,
                    staged.generation,
                )
                .expect("publish");
        }
        let candidates = vec![
            tenant_a.records[0].record_id.clone(),
            tenant_b.records[0].record_id.clone(),
        ];
        let evidence = catalog
            .begin_read()
            .filter_visible_candidates(
                &candidates,
                &QueryFilters {
                    tenant_id: "tenant-a".into(),
                    root_id: Some("root-a".into()),
                    workspace_id: Some("workspace-a".into()),
                    media_type: Some("text-plain".into()),
                    modified_from_ms: Some(999),
                    modified_to_ms: Some(1_001),
                    concept_id: Some("concept-a".into()),
                    generation: Some(1),
                    ..QueryFilters::default()
                },
            )
            .expect("evidence");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].document_id, "document-a");
    }

    #[test]
    fn staging_and_jobs_survive_abnormal_worker_restart() {
        let (directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let staged = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "body",
        );
        catalog.stage_generation(&staged).expect("stage");
        catalog
            .upsert_job("job-a", "embedding", 2, Some("resume-token"))
            .expect("job");
        drop(catalog);

        let reopened =
            SemanticCatalog::open(directory.path().join("catalog.sqlite")).expect("reopen");
        let invisible = reopened
            .begin_read()
            .filter_visible_candidates(
                &[staged.records[0].record_id.clone()],
                &QueryFilters {
                    tenant_id: "tenant-a".into(),
                    ..QueryFilters::default()
                },
            )
            .expect("visibility");
        assert!(invisible.is_empty());
        reopened
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish recovered stage");
        assert_eq!(
            reopened
                .begin_read()
                .filter_visible_candidates(
                    &[staged.records[0].record_id.clone()],
                    &QueryFilters {
                        tenant_id: "tenant-a".into(),
                        ..QueryFilters::default()
                    },
                )
                .expect("visibility")
                .len(),
            1
        );
    }

    #[test]
    fn index_choice_uses_measured_threshold() {
        assert_eq!(choose_index_kind(10_000, 10_000), VectorIndexKind::Flat);
        assert_eq!(choose_index_kind(10_001, 10_000), VectorIndexKind::Hnsw);
    }
}
