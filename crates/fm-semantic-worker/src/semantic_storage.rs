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

const CATALOG_SCHEMA_VERSION: i64 = 5;

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
    /// Bounded display excerpt.
    pub excerpt: String,
    /// Complete structurally bounded source content used for downstream local selection.
    pub content: String,
    /// Conservative token estimate of `content`.
    pub token_count: u32,
    /// Serialized section hierarchy.
    pub section_path: Vec<String>,
    /// Structural role used by representative selection.
    pub structural_role: String,
    /// Serialized strongest available provenance.
    pub provenance: String,
    /// Document-order position used for deterministic evidence ordering.
    pub source_position: u32,
    /// Whether this is generated evidence such as a summary.
    pub generated: bool,
    /// Optional SKOS concept identity.
    pub concept_id: Option<String>,
}

/// Complete extracted chunks and embeddings for one visible document generation.
#[derive(Debug, Clone, PartialEq)]
pub struct SummarySourceSet {
    /// Indexed source-content hash.
    pub content_hash: String,
    /// Visible source generation.
    pub generation: u64,
    /// Source occurrence used for derived summary provenance.
    pub occurrence_id: String,
    /// Enrolled root of the source occurrence.
    pub root_id: String,
    /// Optional workspace scope.
    pub workspace_id: Option<String>,
    /// Source media type.
    pub media_type: String,
    /// Source modification time.
    pub modified_at_ms: i64,
    /// Extracted non-generated source chunks.
    pub chunks: Vec<StoredSourceChunk>,
}

/// Durable generated-summary metadata and provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredDocumentSummary {
    /// Stable generated record identity.
    pub record_id: String,
    /// Source generation represented by the prose.
    pub source_generation: u64,
    /// Source content hash represented by the prose.
    pub source_content_hash: String,
    /// Generation profile identity.
    pub profile_id: String,
    /// Exact generation model identity.
    pub model_id: String,
    /// Representative-selection version.
    pub algorithm_version: String,
    /// Prompt template version.
    pub prompt_version: String,
    /// Complete representative-selection fingerprint.
    pub selection_fingerprint: String,
    /// Supporting extracted chunk identities.
    pub supporting_chunk_ids: Vec<String>,
    /// Population weights aligned with `supporting_chunk_ids`.
    pub supporting_weights: Vec<f32>,
    /// Creation time in Unix milliseconds.
    pub created_at_ms: i64,
    /// Concise generated overview.
    pub brief_text: String,
    /// Full generated structured summary.
    pub full_text: String,
}

/// Complete summary publication using a local embedding.
#[derive(Debug, Clone)]
pub struct SummaryPublication {
    /// Durable metadata and generated text.
    pub summary: StoredDocumentSummary,
    /// Owning source occurrence in the current complete generation.
    pub occurrence_id: String,
    /// Local embedding cache identity.
    pub cache_key: EmbeddingCacheKey,
    /// Normalized local summary embedding.
    pub vector: Vec<f32>,
    /// Serialized strongest supporting source provenance.
    pub provenance: String,
}

/// One complete source chunk loaded for representative selection.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSourceChunk {
    /// Stable record identity.
    pub record_id: String,
    /// Complete structurally bounded content.
    pub content: String,
    /// Existing local embedding.
    pub vector: Vec<f32>,
    /// Conservative token estimate.
    pub token_count: u32,
    /// Original source order.
    pub source_position: u32,
    /// Section hierarchy.
    pub section_path: Vec<String>,
    /// Serialized strongest source provenance.
    pub provenance: String,
    /// Structural role.
    pub structural_role: String,
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

/// One replaceable concept label derived from an existing source chunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptAnnotation {
    /// Existing visible chunk record.
    pub record_id: String,
    /// Stable SKOS concept URI.
    pub concept_uri: String,
    /// Deterministic local similarity score.
    pub confidence: f32,
    /// Bounded supporting chunk identities for explanation.
    pub supporting_chunk_ids: Vec<String>,
}

/// One complete relabelling generation staged for atomic publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConceptAnnotationGeneration {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Semantic library.
    pub library_id: String,
    /// Attached vocabulary identity.
    pub vocabulary_id: String,
    /// Monotonically increasing relabelling generation.
    pub generation: u64,
    /// Complete set of active labels for this generation.
    pub annotations: Vec<ConceptAnnotation>,
}

/// One document returned by a concept virtual folder.
#[derive(Debug, Clone, PartialEq)]
pub struct ConceptFolderDocument {
    /// Content identity.
    pub document_id: String,
    /// Authorized source identity.
    pub source_id: String,
    /// Source availability.
    pub available: bool,
    /// Highest matching confidence across the selected concepts.
    pub confidence: f32,
    /// Bounded source chunks explaining the active labels.
    pub supporting_chunk_ids: Vec<String>,
    /// Published source generation.
    pub source_generation: u64,
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
    /// Semantic record kind (`chunk`, `summary`, or a future versioned kind).
    pub record_kind: String,
    /// Bounded display excerpt.
    pub excerpt: String,
    /// Complete structurally bounded source content.
    pub content: String,
    /// Token count from the active local tokenizer.
    pub token_count: usize,
    /// Bounded structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Position in source order.
    pub source_position: u32,
    /// Whether this evidence was generated rather than extracted.
    pub generated: bool,
    /// Hash of the indexed source bytes.
    pub content_hash: String,
    /// Current source availability recorded by reconciliation.
    pub available: bool,
    /// Source media type.
    pub media_type: String,
    /// Source modification time in Unix milliseconds.
    pub modified_at_ms: i64,
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

    /// Verifies that every registered library uses one exact manifest.
    ///
    /// This is used by fixed-manifest worker assemblies before reopening their
    /// shared derived index. Ordinary production registration remains
    /// library-specific and unchanged.
    ///
    /// # Errors
    ///
    /// Returns a migration requirement before the derived index is opened
    /// when any registered library has an incompatible manifest.
    pub fn validate_registered_library_manifests(
        &self,
        expected: &LibraryIndexManifest,
    ) -> Result<(), StorageError> {
        expected.validate()?;
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT manifest_json FROM libraries ORDER BY tenant_id, library_id")?;
        let manifests = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for manifest in manifests {
            let manifest: LibraryIndexManifest = serde_json::from_str(&manifest)?;
            manifest.validate()?;
            let incompatible_fields = manifest.incompatible_fields(expected);
            if !incompatible_fields.is_empty() {
                return Err(StorageError::MigrationRequired {
                    incompatible_fields,
                });
            }
        }
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
            "UPDATE records
             SET generation = ?4,
                 occurrence_id = (
                   SELECT MIN(occurrence_id) FROM occurrences
                   WHERE tenant_id = ?1 AND library_id = ?2
                     AND document_id = ?3 AND generation = ?4
                 )
             WHERE record_id = (
               SELECT record_id FROM document_summaries
               WHERE tenant_id = ?1 AND library_id = ?2 AND document_id = ?3
             )",
            params![tenant_id, library_id, document_id, generation],
        )?;
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

    /// Loads complete, non-generated chunks from the visible document generation.
    ///
    /// Existing vectors are returned with their source chunks so representative
    /// selection never performs a second embedding pass.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error for an unknown document or corrupt row.
    pub fn summary_source_chunks(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<SummarySourceSet, StorageError> {
        for (value, field) in [
            (tenant_id, "tenant_id"),
            (library_id, "library_id"),
            (document_id, "document_id"),
        ] {
            validate_identifier(value, field)?;
        }
        let connection = self.connection()?;
        let header = connection
            .query_row(
                "SELECT d.content_hash, g.generation, o.occurrence_id, o.root_id,
                        o.workspace_id, o.media_type, o.modified_at_ms
                 FROM generations g
                 JOIN documents d ON d.document_id = g.document_id
                 JOIN occurrences o
                   ON o.tenant_id = g.tenant_id
                  AND o.library_id = g.library_id
                  AND o.document_id = g.document_id
                  AND o.generation = g.generation
                 WHERE g.tenant_id = ?1 AND g.library_id = ?2
                   AND g.document_id = ?3 AND g.state = 'complete'
                 ORDER BY o.occurrence_id
                 LIMIT 1",
                params![tenant_id, library_id, document_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StorageError::DocumentNotFound)?;
        let mut statement = connection.prepare(
            "SELECT r.record_id, r.content, v.vector, r.token_count,
                    r.source_position, r.section_path_json, r.provenance,
                    r.structural_role
             FROM records r
             JOIN vectors v ON v.cache_key = r.cache_key
             WHERE r.tenant_id = ?1 AND r.library_id = ?2
               AND r.document_id = ?3 AND r.generation = ?4
               AND r.record_kind = 'chunk' AND r.generated = 0
             ORDER BY r.source_position, r.record_id",
        )?;
        let chunks = statement
            .query_map(
                params![tenant_id, library_id, document_id, header.1],
                |row| {
                    let vector = row.get::<_, Vec<u8>>(2)?;
                    let section_path = row.get::<_, String>(5)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        vector,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        section_path,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )?
            .map(|row| {
                let (
                    record_id,
                    content,
                    vector,
                    token_count,
                    source_position,
                    section_path,
                    provenance,
                    structural_role,
                ) = row?;
                Ok(StoredSourceChunk {
                    record_id,
                    content,
                    vector: decode_vector(&vector)?,
                    token_count: u32::try_from(token_count)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    source_position: u32::try_from(source_position)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    section_path: serde_json::from_str(&section_path)?,
                    provenance,
                    structural_role,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        Ok(SummarySourceSet {
            content_hash: header.0,
            generation: u64::try_from(header.1).map_err(|_| StorageError::CorruptCatalog)?,
            occurrence_id: header.2,
            root_id: header.3,
            workspace_id: header.4,
            media_type: header.5,
            modified_at_ms: header.6,
            chunks,
        })
    }

    /// Atomically replaces the visible generated summary metadata and record.
    ///
    /// The caller stages the derived vector first. If this transaction fails,
    /// that vector remains an unauthorized orphan and cannot become evidence.
    ///
    /// # Errors
    ///
    /// Rejects stale source metadata, unsupported evidence IDs, dimensions,
    /// and malformed generated output.
    pub fn publish_summary(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
        publication: &SummaryPublication,
    ) -> Result<Option<String>, StorageError> {
        for (value, field) in [
            (tenant_id, "tenant_id"),
            (library_id, "library_id"),
            (document_id, "document_id"),
            (publication.summary.record_id.as_str(), "record_id"),
            (publication.occurrence_id.as_str(), "occurrence_id"),
        ] {
            validate_identifier(value, field)?;
        }
        if publication.summary.brief_text.trim().is_empty()
            || publication.summary.full_text.trim().is_empty()
            || publication.summary.brief_text.len() > 16 * 1024
            || publication.summary.full_text.len() > 256 * 1024
            || publication.summary.supporting_chunk_ids.is_empty()
            || publication.summary.supporting_chunk_ids.len()
                != publication.summary.supporting_weights.len()
            || publication
                .summary
                .supporting_weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight < 0.0 || *weight > 1.0)
        {
            return Err(StorageError::InvalidSummary);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let manifest = load_manifest(&transaction, tenant_id, library_id)?;
        if publication.vector.len() != manifest.dimensions {
            return Err(StorageError::DimensionMismatch {
                expected: manifest.dimensions,
                actual: publication.vector.len(),
            });
        }
        let current = transaction
            .query_row(
                "SELECT d.content_hash, g.generation
                 FROM generations g
                 JOIN documents d ON d.document_id = g.document_id
                 WHERE g.tenant_id = ?1 AND g.library_id = ?2
                   AND g.document_id = ?3 AND g.state = 'complete'",
                params![tenant_id, library_id, document_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or(StorageError::DocumentNotFound)?;
        if current.0 != publication.summary.source_content_hash
            || u64::try_from(current.1).map_err(|_| StorageError::CorruptCatalog)?
                != publication.summary.source_generation
        {
            return Err(StorageError::StaleSummarySource);
        }
        for record_id in &publication.summary.supporting_chunk_ids {
            let exists = transaction.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM records
                   WHERE record_id = ?1 AND tenant_id = ?2 AND library_id = ?3
                     AND document_id = ?4 AND generation = ?5
                     AND record_kind = 'chunk' AND generated = 0
                 )",
                params![record_id, tenant_id, library_id, document_id, current.1],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(StorageError::InvalidSummary);
            }
        }
        let old = transaction
            .query_row(
                "SELECT record_id FROM document_summaries
                 WHERE tenant_id = ?1 AND library_id = ?2 AND document_id = ?3",
                params![tenant_id, library_id, document_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(old_record_id) = &old {
            let old_key = transaction
                .query_row(
                    "SELECT cache_key FROM records WHERE record_id = ?1",
                    [old_record_id],
                    |row| row.get::<_, Vec<u8>>(0),
                )
                .optional()?;
            transaction.execute(
                "DELETE FROM document_summaries WHERE record_id = ?1",
                [old_record_id],
            )?;
            transaction.execute("DELETE FROM records WHERE record_id = ?1", [old_record_id])?;
            if let Some(old_key) = old_key {
                transaction.execute(
                    "UPDATE vectors SET reference_count = reference_count - 1
                     WHERE cache_key = ?1",
                    [old_key],
                )?;
            }
        }

        let vector_bytes = encode_vector(&publication.vector);
        transaction.execute(
            "INSERT INTO vectors (cache_key, dimensions, vector, reference_count)
             VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(cache_key) DO NOTHING",
            params![
                publication.cache_key.as_bytes().as_slice(),
                i64::try_from(publication.vector.len())
                    .map_err(|_| StorageError::CorruptCatalog)?,
                vector_bytes,
            ],
        )?;
        transaction.execute(
            "INSERT INTO records
             (record_id, tenant_id, library_id, document_id, occurrence_id,
              generation, cache_key, record_kind, excerpt, content, token_count,
              section_path_json, structural_role, provenance, source_position,
              generated, concept_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'summary', ?8, ?9, 0,
                     '[]', 'body', ?10, 0, 1, NULL)",
            params![
                publication.summary.record_id,
                tenant_id,
                library_id,
                document_id,
                publication.occurrence_id,
                current.1,
                publication.cache_key.as_bytes().as_slice(),
                publication.summary.brief_text,
                publication.summary.full_text,
                publication.provenance,
            ],
        )?;
        transaction.execute(
            "UPDATE vectors SET reference_count = reference_count + 1
             WHERE cache_key = ?1",
            [publication.cache_key.as_bytes().as_slice()],
        )?;
        transaction.execute(
            "INSERT INTO document_summaries
             (tenant_id, library_id, document_id, record_id, source_generation,
              source_content_hash, profile_id, model_id, algorithm_version,
              prompt_version, selection_fingerprint, supporting_chunk_ids_json,
              supporting_weights_json, created_at_ms, brief_text, full_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, ?16)",
            params![
                tenant_id,
                library_id,
                document_id,
                publication.summary.record_id,
                i64_generation(publication.summary.source_generation)?,
                publication.summary.source_content_hash,
                publication.summary.profile_id,
                publication.summary.model_id,
                publication.summary.algorithm_version,
                publication.summary.prompt_version,
                publication.summary.selection_fingerprint,
                serde_json::to_string(&publication.summary.supporting_chunk_ids)?,
                serde_json::to_string(&publication.summary.supporting_weights)?,
                publication.summary.created_at_ms,
                publication.summary.brief_text,
                publication.summary.full_text,
            ],
        )?;
        transaction.execute("DELETE FROM vectors WHERE reference_count <= 0", [])?;
        transaction.commit()?;
        Ok(old)
    }

    /// Returns the current generated summary, including stale source identity.
    pub fn document_summary(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<Option<StoredDocumentSummary>, StorageError> {
        struct RawSummary {
            record_id: String,
            source_generation: i64,
            source_content_hash: String,
            profile_id: String,
            model_id: String,
            algorithm_version: String,
            prompt_version: String,
            selection_fingerprint: String,
            supporting_chunk_ids_json: String,
            supporting_weights_json: String,
            created_at_ms: i64,
            brief_text: String,
            full_text: String,
        }
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT record_id, source_generation, source_content_hash,
                        profile_id, model_id, algorithm_version, prompt_version,
                        selection_fingerprint, supporting_chunk_ids_json,
                        supporting_weights_json, created_at_ms, brief_text, full_text
                 FROM document_summaries
                 WHERE tenant_id = ?1 AND library_id = ?2 AND document_id = ?3",
                params![tenant_id, library_id, document_id],
                |row| {
                    Ok(RawSummary {
                        record_id: row.get(0)?,
                        source_generation: row.get(1)?,
                        source_content_hash: row.get(2)?,
                        profile_id: row.get(3)?,
                        model_id: row.get(4)?,
                        algorithm_version: row.get(5)?,
                        prompt_version: row.get(6)?,
                        selection_fingerprint: row.get(7)?,
                        supporting_chunk_ids_json: row.get(8)?,
                        supporting_weights_json: row.get(9)?,
                        created_at_ms: row.get(10)?,
                        brief_text: row.get(11)?,
                        full_text: row.get(12)?,
                    })
                },
            )
            .optional()?
            .map(|raw| {
                Ok(StoredDocumentSummary {
                    record_id: raw.record_id,
                    source_generation: u64::try_from(raw.source_generation)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    source_content_hash: raw.source_content_hash,
                    profile_id: raw.profile_id,
                    model_id: raw.model_id,
                    algorithm_version: raw.algorithm_version,
                    prompt_version: raw.prompt_version,
                    selection_fingerprint: raw.selection_fingerprint,
                    supporting_chunk_ids: serde_json::from_str(&raw.supporting_chunk_ids_json)?,
                    supporting_weights: serde_json::from_str(&raw.supporting_weights_json)?,
                    created_at_ms: raw.created_at_ms,
                    brief_text: raw.brief_text,
                    full_text: raw.full_text,
                })
            })
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

    /// Registers the stable scope and document identity of a durable job.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn register_job(
        &self,
        job_id: &str,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<(), StorageError> {
        validate_identifier(job_id, "job_id")?;
        validate_identifier(tenant_id, "tenant_id")?;
        validate_identifier(library_id, "library_id")?;
        validate_identifier(document_id, "document_id")?;
        self.connection()?.execute(
            "INSERT INTO jobs (
               job_id, tenant_id, library_id, document_id, stage, attempts, detail
             ) VALUES (?1, ?2, ?3, ?4, 'discovered', 0, NULL)
             ON CONFLICT(job_id) DO NOTHING",
            params![job_id, tenant_id, library_id, document_id],
        )?;
        Ok(())
    }

    /// Reads one persisted ingestion/reconciliation job.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn job(&self, job_id: &str) -> Result<Option<StoredJob>, StorageError> {
        validate_identifier(job_id, "job_id")?;
        self.connection()?
            .query_row(
                "SELECT tenant_id, library_id, document_id, stage, attempts, detail
                 FROM jobs WHERE job_id = ?1",
                [job_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?
            .map(
                |(tenant_id, library_id, document_id, stage, attempts, detail)| {
                    Ok(StoredJob {
                        job_id: job_id.to_owned(),
                        tenant_id,
                        library_id,
                        document_id,
                        stage,
                        attempts: u32::try_from(attempts)
                            .map_err(|_| StorageError::CorruptCatalog)?,
                        detail,
                    })
                },
            )
            .transpose()
    }

    /// Stages a complete replaceable concept-labelling generation.
    ///
    /// Existing published labels remain visible until [`Self::publish_concept_annotations`].
    pub fn stage_concept_annotations(
        &self,
        staged: &ConceptAnnotationGeneration,
    ) -> Result<(), StorageError> {
        validate_identifier(&staged.tenant_id, "tenant_id")?;
        validate_identifier(&staged.library_id, "library_id")?;
        validate_identifier(&staged.vocabulary_id, "vocabulary_id")?;
        if staged.generation == 0 {
            return Err(StorageError::InvalidIdentifier(
                "concept_annotation_generation",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        load_manifest(&transaction, &staged.tenant_id, &staged.library_id)?;
        transaction.execute(
            "INSERT INTO concept_annotation_generations
             (tenant_id, library_id, vocabulary_id, generation, state)
             VALUES (?1, ?2, ?3, ?4, 'staging')
             ON CONFLICT(tenant_id, library_id, vocabulary_id, generation)
             DO UPDATE SET state = 'staging'",
            params![
                staged.tenant_id,
                staged.library_id,
                staged.vocabulary_id,
                i64_generation(staged.generation)?
            ],
        )?;
        transaction.execute(
            "DELETE FROM concept_annotations
             WHERE tenant_id = ?1 AND library_id = ?2
               AND vocabulary_id = ?3 AND label_generation = ?4",
            params![
                staged.tenant_id,
                staged.library_id,
                staged.vocabulary_id,
                i64_generation(staged.generation)?
            ],
        )?;
        for annotation in &staged.annotations {
            validate_identifier(&annotation.record_id, "record_id")?;
            validate_identifier(&annotation.concept_uri, "concept_uri")?;
            if !annotation.confidence.is_finite()
                || !(0.0..=1.0).contains(&annotation.confidence)
                || annotation.supporting_chunk_ids.len() > 32
            {
                return Err(StorageError::InvalidConceptAnnotation);
            }
            let visible = transaction.query_row(
                "SELECT EXISTS (
                   SELECT 1 FROM records r
                   JOIN generations g
                     ON g.tenant_id = r.tenant_id AND g.library_id = r.library_id
                    AND g.document_id = r.document_id AND g.generation = r.generation
                   WHERE r.record_id = ?1 AND r.tenant_id = ?2 AND r.library_id = ?3
                     AND g.state = 'complete'
                 )",
                params![annotation.record_id, staged.tenant_id, staged.library_id],
                |row| row.get::<_, bool>(0),
            )?;
            if !visible {
                return Err(StorageError::ConceptAnnotationSourceNotFound);
            }
            for chunk_id in &annotation.supporting_chunk_ids {
                validate_identifier(chunk_id, "supporting_chunk_id")?;
            }
            transaction.execute(
                "INSERT INTO concept_annotations
                 (tenant_id, library_id, vocabulary_id, label_generation,
                  record_id, concept_uri, confidence, supporting_chunk_ids_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    staged.tenant_id,
                    staged.library_id,
                    staged.vocabulary_id,
                    i64_generation(staged.generation)?,
                    annotation.record_id,
                    annotation.concept_uri,
                    annotation.confidence,
                    serde_json::to_string(&annotation.supporting_chunk_ids)?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Atomically activates one complete concept-labelling generation.
    pub fn publish_concept_annotations(
        &self,
        tenant_id: &str,
        library_id: &str,
        vocabulary_id: &str,
        generation: u64,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let generation = i64_generation(generation)?;
        let staged = transaction.query_row(
            "SELECT EXISTS (
               SELECT 1 FROM concept_annotation_generations
               WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3
                 AND generation = ?4 AND state = 'staging'
             )",
            params![tenant_id, library_id, vocabulary_id, generation],
            |row| row.get::<_, bool>(0),
        )?;
        if !staged {
            return Err(StorageError::ConceptAnnotationGenerationNotStaged);
        }
        transaction.execute(
            "UPDATE concept_annotation_generations SET state = 'superseded'
             WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3
               AND state = 'complete'",
            params![tenant_id, library_id, vocabulary_id],
        )?;
        transaction.execute(
            "UPDATE concept_annotation_generations SET state = 'complete'
             WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3
               AND generation = ?4",
            params![tenant_id, library_id, vocabulary_id, generation],
        )?;
        transaction.execute(
            "DELETE FROM concept_annotations
             WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3
               AND label_generation <> ?4",
            params![tenant_id, library_id, vocabulary_id, generation],
        )?;
        transaction.execute(
            "DELETE FROM concept_annotation_generations
             WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3
               AND state = 'superseded'",
            params![tenant_id, library_id, vocabulary_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Removes all replaceable labels owned by a deleted vocabulary.
    pub fn remove_vocabulary_annotations(
        &self,
        tenant_id: &str,
        library_id: &str,
        vocabulary_id: &str,
    ) -> Result<(), StorageError> {
        self.connection()?.execute(
            "DELETE FROM concept_annotation_generations
             WHERE tenant_id = ?1 AND library_id = ?2 AND vocabulary_id = ?3",
            params![tenant_id, library_id, vocabulary_id],
        )?;
        Ok(())
    }

    /// Returns stable, paged concept-folder documents from active labels only.
    pub fn concept_folder_documents(
        &self,
        filters: &QueryFilters,
        vocabulary_id: &str,
        concept_uris: &[String],
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ConceptFolderDocument>, StorageError> {
        if concept_uris.is_empty() || concept_uris.len() > 256 || limit == 0 || limit > 200 {
            return Err(StorageError::InvalidConceptFolderQuery);
        }
        validate_identifier(&filters.tenant_id, "tenant_id")?;
        validate_identifier(vocabulary_id, "vocabulary_id")?;
        let concept_json = serde_json::to_string(concept_uris)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT r.document_id, MIN(o.source_id), MAX(o.available),
                    MAX(a.confidence), r.generation,
                    json_group_array(a.supporting_chunk_ids_json)
             FROM concept_annotations a
             JOIN concept_annotation_generations ag
               ON ag.tenant_id = a.tenant_id AND ag.library_id = a.library_id
              AND ag.vocabulary_id = a.vocabulary_id
              AND ag.generation = a.label_generation AND ag.state = 'complete'
             JOIN records r ON r.record_id = a.record_id
             JOIN generations g
               ON g.tenant_id = r.tenant_id AND g.library_id = r.library_id
              AND g.document_id = r.document_id AND g.generation = r.generation
              AND g.state = 'complete'
             JOIN occurrences o
               ON o.tenant_id = r.tenant_id AND o.library_id = r.library_id
              AND o.document_id = r.document_id AND o.generation = r.generation
             WHERE a.tenant_id = ?1 AND a.vocabulary_id = ?2
               AND (?3 IS NULL OR a.library_id = ?3)
               AND a.concept_uri IN (SELECT value FROM json_each(?4))
               AND (?5 IS NULL OR o.root_id = ?5)
               AND (?6 IS NULL OR o.workspace_id = ?6)
               AND (?7 OR o.available = 1)
             GROUP BY r.document_id, r.generation
             ORDER BY MAX(a.confidence) DESC, r.document_id
             LIMIT ?8 OFFSET ?9",
        )?;
        let rows = statement.query_map(
            params![
                filters.tenant_id,
                vocabulary_id,
                filters.library_id,
                concept_json,
                filters.root_id,
                filters.workspace_id,
                filters.include_unavailable,
                i64::try_from(limit).map_err(|_| StorageError::InvalidConceptFolderQuery)?,
                i64::try_from(offset).map_err(|_| StorageError::InvalidConceptFolderQuery)?
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, f32>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (document_id, source_id, available, confidence, generation, evidence_json) = row?;
            let mut supporting_chunk_ids = serde_json::from_str::<Vec<String>>(&evidence_json)?
                .into_iter()
                .map(|value| serde_json::from_str::<Vec<String>>(&value))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            supporting_chunk_ids.sort();
            supporting_chunk_ids.dedup();
            supporting_chunk_ids.truncate(32);
            Ok(ConceptFolderDocument {
                document_id,
                source_id,
                available,
                confidence,
                supporting_chunk_ids,
                source_generation: u64::try_from(generation)
                    .map_err(|_| StorageError::CorruptCatalog)?,
            })
        })
        .collect()
    }

    /// Returns the next generation for one logical document.
    ///
    /// # Errors
    ///
    /// Returns a typed database or persisted-data error.
    pub fn next_generation(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<u64, StorageError> {
        let maximum = self.connection()?.query_row(
            "SELECT COALESCE(MAX(generation), 0) FROM generations
             WHERE tenant_id = ?1 AND library_id = ?2 AND document_id = ?3",
            params![tenant_id, library_id, document_id],
            |row| row.get::<_, i64>(0),
        )?;
        u64::try_from(maximum)
            .map_err(|_| StorageError::CorruptCatalog)?
            .checked_add(1)
            .ok_or(StorageError::CorruptCatalog)
    }

    /// Reuses an interrupted staging generation or allocates the next one.
    ///
    /// # Errors
    ///
    /// Returns a typed database or persisted-data error.
    pub fn resume_or_next_generation(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<u64, StorageError> {
        let staging = self.connection()?.query_row(
            "SELECT MAX(generation) FROM generations
                 WHERE tenant_id = ?1 AND library_id = ?2
                   AND document_id = ?3 AND state = 'staging'",
            params![tenant_id, library_id, document_id],
            |row| row.get::<_, Option<i64>>(0),
        )?;
        if let Some(staging) = staging {
            return u64::try_from(staging).map_err(|_| StorageError::CorruptCatalog);
        }
        self.next_generation(tenant_id, library_id, document_id)
    }

    /// Reads a globally cached normalized vector.
    ///
    /// # Errors
    ///
    /// Returns a typed database or persisted-data error.
    pub fn cached_vector(
        &self,
        cache_key: EmbeddingCacheKey,
    ) -> Result<Option<Vec<f32>>, StorageError> {
        let bytes = self
            .connection()?
            .query_row(
                "SELECT vector FROM vectors WHERE cache_key = ?1",
                [cache_key.as_bytes().as_slice()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        bytes.map(|bytes| decode_vector(&bytes)).transpose()
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
                    o.source_id, r.provenance, r.generation, r.record_kind,
                    r.excerpt, r.content, r.token_count, r.section_path_json,
                    r.source_position, r.generated, d.content_hash,
                    o.available, o.media_type, o.modified_at_ms
             FROM records r
             JOIN generations g
               ON g.tenant_id = r.tenant_id
              AND g.library_id = r.library_id
              AND g.document_id = r.document_id
              AND g.generation = r.generation
             JOIN occurrences o
               ON o.occurrence_id = r.occurrence_id
              AND o.generation = r.generation
             JOIN documents d ON d.document_id = r.document_id
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
                            row.get::<_, String>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                            row.get::<_, i64>(10)?,
                            row.get::<_, String>(11)?,
                            row.get::<_, i64>(12)?,
                            row.get::<_, i64>(13)?,
                            row.get::<_, String>(14)?,
                            row.get::<_, i64>(15)?,
                            row.get::<_, String>(16)?,
                            row.get::<_, i64>(17)?,
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
                record_kind,
                excerpt,
                content,
                token_count,
                section_path_json,
                source_position,
                generated,
                content_hash,
                available,
                media_type,
                modified_at_ms,
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
                    record_kind,
                    excerpt,
                    content,
                    token_count: usize::try_from(token_count)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    section_path: serde_json::from_str(&section_path_json)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    source_position: u32::try_from(source_position)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    generated: generated != 0,
                    content_hash,
                    available: available != 0,
                    media_type,
                    modified_at_ms,
                });
            }
        }
        Ok(evidence)
    }

    /// Loads source chunks immediately surrounding an authorized evidence row.
    ///
    /// The returned rows are re-authorized through the same tenant and scope
    /// filters as vector candidates; generated records are never expanded.
    ///
    /// # Errors
    ///
    /// Returns a typed catalog failure or rejects an excessive radius.
    pub fn adjacent_source_chunks(
        &self,
        anchor: &QueryEvidence,
        radius: u32,
        filters: &QueryFilters,
    ) -> Result<Vec<QueryEvidence>, StorageError> {
        if radius > 4 {
            return Err(StorageError::InvalidIdentifier("adjacent_radius"));
        }
        let lower = anchor.source_position.saturating_sub(radius);
        let upper = anchor.source_position.saturating_add(radius);
        let connection = self.catalog.connection()?;
        let mut statement = connection.prepare(
            "SELECT record_id
             FROM records
             WHERE tenant_id = ?1
               AND library_id = ?2
               AND document_id = ?3
               AND occurrence_id = ?4
               AND generation = ?5
               AND generated = 0
               AND source_position BETWEEN ?6 AND ?7
             ORDER BY source_position, record_id",
        )?;
        let record_ids = statement
            .query_map(
                params![
                    filters.tenant_id,
                    anchor.library_id,
                    anchor.document_id,
                    anchor.occurrence_id,
                    i64_generation(anchor.generation)?,
                    i64::from(lower),
                    i64::from(upper),
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        self.filter_visible_candidates(&record_ids, filters)
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

/// Durable ingestion/reconciliation job snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredJob {
    /// Stable job identity.
    pub job_id: String,
    /// Tenant boundary, present for protocol-queued jobs.
    pub tenant_id: Option<String>,
    /// Library boundary, present for protocol-queued jobs.
    pub library_id: Option<String>,
    /// Opaque document identity, present for protocol-queued jobs.
    pub document_id: Option<String>,
    /// Current state-machine stage.
    pub stage: String,
    /// Number of bounded attempts.
    pub attempts: u32,
    /// Sanitized failure, pause, or resume detail.
    pub detail: Option<String>,
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
    /// Requested document has no complete visible generation.
    #[error("document has no complete semantic generation")]
    DocumentNotFound,
    /// Generated summary metadata or provenance is invalid.
    #[error("generated summary is invalid")]
    InvalidSummary,
    /// Source generation changed before summary publication.
    #[error("summary source generation is stale")]
    StaleSummarySource,
    /// A concept label points at a record outside the current visible generation.
    #[error("concept annotation source record is not visible")]
    ConceptAnnotationSourceNotFound,
    /// Concept score or evidence exceeds the bounded annotation contract.
    #[error("concept annotation is invalid")]
    InvalidConceptAnnotation,
    /// Publication requires a durable staging relabelling generation.
    #[error("concept annotation generation was not staged")]
    ConceptAnnotationGenerationNotStaged,
    /// Concept folder URI set or paging request exceeds bounded limits.
    #[error("concept folder query is invalid")]
    InvalidConceptFolderQuery,
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
           SELECT 5 WHERE NOT EXISTS (SELECT 1 FROM catalog_meta);
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
           excerpt TEXT NOT NULL,
           content TEXT NOT NULL,
           token_count INTEGER NOT NULL,
           section_path_json TEXT NOT NULL,
           structural_role TEXT NOT NULL,
           provenance TEXT NOT NULL,
           source_position INTEGER NOT NULL,
           generated INTEGER NOT NULL,
           concept_id TEXT,
           FOREIGN KEY (occurrence_id, generation)
             REFERENCES occurrences (occurrence_id, generation),
           FOREIGN KEY (cache_key) REFERENCES vectors (cache_key)
         );
         CREATE INDEX IF NOT EXISTS record_scope
           ON records (tenant_id, library_id, document_id, generation);
         CREATE TABLE IF NOT EXISTS document_summaries (
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           document_id TEXT NOT NULL,
           record_id TEXT NOT NULL UNIQUE,
           source_generation INTEGER NOT NULL,
           source_content_hash TEXT NOT NULL,
           profile_id TEXT NOT NULL,
           model_id TEXT NOT NULL,
           algorithm_version TEXT NOT NULL,
           prompt_version TEXT NOT NULL,
           selection_fingerprint TEXT NOT NULL,
           supporting_chunk_ids_json TEXT NOT NULL,
           supporting_weights_json TEXT NOT NULL,
           created_at_ms INTEGER NOT NULL,
           brief_text TEXT NOT NULL,
           full_text TEXT NOT NULL,
           PRIMARY KEY (tenant_id, library_id, document_id),
           FOREIGN KEY (record_id) REFERENCES records (record_id) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS jobs (
           job_id TEXT PRIMARY KEY,
           tenant_id TEXT,
           library_id TEXT,
           document_id TEXT,
           stage TEXT NOT NULL,
           attempts INTEGER NOT NULL,
           detail TEXT
         );
         CREATE TABLE IF NOT EXISTS component_versions (
           component_id TEXT PRIMARY KEY,
           revision TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS concept_annotation_generations (
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           vocabulary_id TEXT NOT NULL,
           generation INTEGER NOT NULL,
           state TEXT NOT NULL CHECK (state IN ('staging', 'complete', 'superseded')),
           PRIMARY KEY (tenant_id, library_id, vocabulary_id, generation),
           FOREIGN KEY (tenant_id, library_id)
             REFERENCES libraries (tenant_id, library_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS one_complete_concept_annotation_generation
           ON concept_annotation_generations (tenant_id, library_id, vocabulary_id)
           WHERE state = 'complete';
         CREATE TABLE IF NOT EXISTS concept_annotations (
           tenant_id TEXT NOT NULL,
           library_id TEXT NOT NULL,
           vocabulary_id TEXT NOT NULL,
           label_generation INTEGER NOT NULL,
           record_id TEXT NOT NULL,
           concept_uri TEXT NOT NULL,
           confidence REAL NOT NULL,
           supporting_chunk_ids_json TEXT NOT NULL,
           PRIMARY KEY (
             tenant_id, library_id, vocabulary_id, label_generation, record_id, concept_uri
           ),
           FOREIGN KEY (tenant_id, library_id, vocabulary_id, label_generation)
             REFERENCES concept_annotation_generations
               (tenant_id, library_id, vocabulary_id, generation)
             ON DELETE CASCADE,
           FOREIGN KEY (record_id) REFERENCES records (record_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS concept_annotation_lookup
           ON concept_annotations (tenant_id, vocabulary_id, concept_uri);
         COMMIT;",
    )?;
    let mut version =
        connection.query_row("SELECT schema_version FROM catalog_meta", [], |row| {
            row.get::<_, i64>(0)
        })?;
    if version == 3 {
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             ALTER TABLE records ADD COLUMN content TEXT NOT NULL DEFAULT '';
             ALTER TABLE records ADD COLUMN token_count INTEGER NOT NULL DEFAULT 0;
             ALTER TABLE records ADD COLUMN section_path_json TEXT NOT NULL DEFAULT '[]';
             ALTER TABLE records ADD COLUMN structural_role TEXT NOT NULL DEFAULT 'body';
             UPDATE catalog_meta SET schema_version = 4;
             COMMIT;",
        )?;
        version = 4;
    }
    if version == 4 {
        connection.execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS concept_annotation_generations (
               tenant_id TEXT NOT NULL,
               library_id TEXT NOT NULL,
               vocabulary_id TEXT NOT NULL,
               generation INTEGER NOT NULL,
               state TEXT NOT NULL CHECK (state IN ('staging', 'complete', 'superseded')),
               PRIMARY KEY (tenant_id, library_id, vocabulary_id, generation),
               FOREIGN KEY (tenant_id, library_id)
                 REFERENCES libraries (tenant_id, library_id)
             );
             CREATE UNIQUE INDEX IF NOT EXISTS one_complete_concept_annotation_generation
               ON concept_annotation_generations (tenant_id, library_id, vocabulary_id)
               WHERE state = 'complete';
             CREATE TABLE IF NOT EXISTS concept_annotations (
               tenant_id TEXT NOT NULL,
               library_id TEXT NOT NULL,
               vocabulary_id TEXT NOT NULL,
               label_generation INTEGER NOT NULL,
               record_id TEXT NOT NULL,
               concept_uri TEXT NOT NULL,
               confidence REAL NOT NULL,
               supporting_chunk_ids_json TEXT NOT NULL,
               PRIMARY KEY (
                 tenant_id, library_id, vocabulary_id, label_generation, record_id, concept_uri
               ),
               FOREIGN KEY (tenant_id, library_id, vocabulary_id, label_generation)
                 REFERENCES concept_annotation_generations
                   (tenant_id, library_id, vocabulary_id, generation)
                 ON DELETE CASCADE,
               FOREIGN KEY (record_id) REFERENCES records (record_id) ON DELETE CASCADE
             );
             CREATE INDEX IF NOT EXISTS concept_annotation_lookup
               ON concept_annotations (tenant_id, vocabulary_id, concept_uri);
             UPDATE catalog_meta SET schema_version = 5;
             COMMIT;",
        )?;
        version = 5;
    }
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
          generation, cache_key, record_kind, excerpt, content, token_count,
          section_path_json, structural_role, provenance, source_position,
          generated, concept_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                 ?14, ?15, ?16, ?17)
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
            record.excerpt,
            record.content,
            i64::from(record.token_count),
            serde_json::to_string(&record.section_path)?,
            record.structural_role,
            record.provenance,
            i64::from(record.source_position),
            i64::from(record.generated),
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

fn decode_vector(bytes: &[u8]) -> Result<Vec<f32>, StorageError> {
    if !bytes.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err(StorageError::CorruptCatalog);
    }
    Ok(bytes
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|chunk| {
            let mut encoded = [0_u8; std::mem::size_of::<f32>()];
            encoded.copy_from_slice(chunk);
            f32::from_le_bytes(encoded)
        })
        .collect())
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
                excerpt: "bounded evidence".into(),
                content: body.into(),
                token_count: 3,
                section_path: vec!["Section".into()],
                structural_role: "body".into(),
                provenance: r#"{"kind":"textLines","start_line":1,"end_line":2}"#.into(),
                source_position: 0,
                generated: false,
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
    fn version_three_catalog_adds_complete_structural_chunk_columns() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("catalog.sqlite");
        let connection = Connection::open(&path).expect("legacy catalog");
        connection
            .execute_batch(
                "CREATE TABLE catalog_meta (schema_version INTEGER NOT NULL);
                 INSERT INTO catalog_meta VALUES (3);
                 CREATE TABLE records (
                   record_id TEXT PRIMARY KEY,
                   tenant_id TEXT NOT NULL,
                   library_id TEXT NOT NULL,
                   document_id TEXT NOT NULL,
                   occurrence_id TEXT NOT NULL,
                   generation INTEGER NOT NULL,
                   cache_key BLOB NOT NULL,
                   record_kind TEXT NOT NULL,
                   excerpt TEXT NOT NULL,
                   provenance TEXT NOT NULL,
                   source_position INTEGER NOT NULL,
                   generated INTEGER NOT NULL,
                   concept_id TEXT
                 );",
            )
            .expect("version three schema");
        drop(connection);

        SemanticCatalog::open(&path).expect("migrated catalog");
        let connection = Connection::open(path).expect("migrated database");
        let version: i64 = connection
            .query_row("SELECT schema_version FROM catalog_meta", [], |row| {
                row.get(0)
            })
            .expect("schema version");
        assert_eq!(version, 5);
        let mut statement = connection
            .prepare("PRAGMA table_info(records)")
            .expect("record columns");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect columns");
        for required in [
            "content",
            "token_count",
            "section_path_json",
            "structural_role",
        ] {
            assert!(columns.iter().any(|column| column == required));
        }
    }

    #[test]
    fn concept_labels_publish_atomically_and_drive_scoped_virtual_folders() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let staged = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "machine learning",
        );
        let record_id = staged.records[0].record_id.clone();
        catalog.stage_generation(&staged).expect("stage source");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish source");

        let generation_one = ConceptAnnotationGeneration {
            tenant_id: "tenant-a".into(),
            library_id: "library-a".into(),
            vocabulary_id: "research-topics".into(),
            generation: 1,
            annotations: vec![ConceptAnnotation {
                record_id: record_id.clone(),
                concept_uri: "https://example.test/concepts/ml".into(),
                confidence: 0.97,
                supporting_chunk_ids: vec![record_id.clone()],
            }],
        };
        catalog
            .stage_concept_annotations(&generation_one)
            .expect("stage labels");
        let filters = QueryFilters {
            tenant_id: "tenant-a".into(),
            root_id: Some("root-a".into()),
            workspace_id: Some("workspace-a".into()),
            ..QueryFilters::default()
        };
        let selected = vec!["https://example.test/concepts/ml".into()];
        assert!(
            catalog
                .concept_folder_documents(&filters, "research-topics", &selected, 0, 50)
                .expect("query before publish")
                .is_empty()
        );
        catalog
            .publish_concept_annotations("tenant-a", "library-a", "research-topics", 1)
            .expect("publish labels");
        let visible = catalog
            .concept_folder_documents(&filters, "research-topics", &selected, 0, 50)
            .expect("query folder");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].document_id, "document-a");
        assert_eq!(visible[0].confidence, 0.97);
        assert_eq!(visible[0].supporting_chunk_ids, vec![record_id.clone()]);
        for unauthorized in [
            QueryFilters {
                tenant_id: "tenant-b".into(),
                ..filters.clone()
            },
            QueryFilters {
                root_id: Some("root-b".into()),
                ..filters.clone()
            },
            QueryFilters {
                workspace_id: Some("workspace-b".into()),
                ..filters.clone()
            },
        ] {
            assert!(
                catalog
                    .concept_folder_documents(&unauthorized, "research-topics", &selected, 0, 50)
                    .expect("isolated concept query")
                    .is_empty()
            );
        }

        let generation_two = ConceptAnnotationGeneration {
            generation: 2,
            annotations: Vec::new(),
            ..generation_one
        };
        catalog
            .stage_concept_annotations(&generation_two)
            .expect("stage replacement");
        assert_eq!(
            catalog
                .concept_folder_documents(&filters, "research-topics", &selected, 0, 50)
                .expect("old labels remain visible")
                .len(),
            1
        );
        catalog
            .publish_concept_annotations("tenant-a", "library-a", "research-topics", 2)
            .expect("publish replacement");
        assert!(
            catalog
                .concept_folder_documents(&filters, "research-topics", &selected, 0, 50)
                .expect("replaced labels")
                .is_empty()
        );

        catalog
            .stage_concept_annotations(&ConceptAnnotationGeneration {
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                vocabulary_id: "research-topics".into(),
                generation: 3,
                annotations: vec![ConceptAnnotation {
                    record_id: record_id.clone(),
                    concept_uri: "https://example.test/concepts/ml".into(),
                    confidence: 0.96,
                    supporting_chunk_ids: vec![record_id],
                }],
            })
            .expect("stage labels after threshold change");
        catalog
            .publish_concept_annotations("tenant-a", "library-a", "research-topics", 3)
            .expect("publish labels after threshold change");
        catalog
            .delete_occurrence("occurrence-a")
            .expect("exclude source occurrence");
        assert!(
            catalog
                .concept_folder_documents(&filters, "research-topics", &selected, 0, 50)
                .expect("query after source exclusion")
                .is_empty()
        );
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
    fn summary_selection_reads_complete_source_chunks_and_existing_vectors() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let staged = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "complete source content",
        );
        catalog.stage_generation(&staged).expect("stage");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish");

        let source = catalog
            .summary_source_chunks("tenant-a", "library-a", "document-a")
            .expect("summary source");

        assert_eq!(source.content_hash, "hash-1");
        assert_eq!(source.generation, 1);
        assert_eq!(source.occurrence_id, "occurrence-a");
        assert_eq!(source.chunks.len(), 1);
        assert_eq!(source.chunks[0].content, "complete source content");
        assert_eq!(source.chunks[0].vector, vec![0.6, 0.8, 0.0]);
        assert_eq!(source.chunks[0].section_path, ["Section"]);
    }

    #[test]
    fn generated_summary_retains_supporting_provenance_across_stale_rollover_and_deletion() {
        let (_directory, catalog) = catalog();
        catalog
            .register_library("tenant-a", "library-a", &manifest())
            .expect("register");
        let first = generation(
            "tenant-a",
            "library-a",
            "document-a",
            1,
            vec![occurrence("occurrence-a", "root-a")],
            "first body",
        );
        let supporting_id = first.records[0].record_id.clone();
        catalog.stage_generation(&first).expect("stage first");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .expect("publish first");
        let summary = StoredDocumentSummary {
            record_id: "summary-document-a-v1".into(),
            source_generation: 1,
            source_content_hash: "hash-1".into(),
            profile_id: "profile-a".into(),
            model_id: "generation-model-a".into(),
            algorithm_version: "representative-kmeans/1".into(),
            prompt_version: "document-summary/1".into(),
            selection_fingerprint: "selection-a".into(),
            supporting_chunk_ids: vec![supporting_id],
            supporting_weights: vec![1.0],
            created_at_ms: 1_000,
            brief_text: "Brief.".into(),
            full_text: "Full summary.".into(),
        };
        catalog
            .publish_summary(
                "tenant-a",
                "library-a",
                "document-a",
                &SummaryPublication {
                    summary: summary.clone(),
                    occurrence_id: "occurrence-a".into(),
                    cache_key: key("Full summary."),
                    vector: vec![0.0, 0.6, 0.8],
                    provenance: r#"{"kind":"textLines","start_line":1,"end_line":2}"#.into(),
                },
            )
            .expect("publish summary");
        assert_eq!(
            catalog
                .document_summary("tenant-a", "library-a", "document-a")
                .expect("summary"),
            Some(summary.clone())
        );

        let second = generation(
            "tenant-a",
            "library-a",
            "document-a",
            2,
            vec![occurrence("occurrence-a", "root-a")],
            "changed body",
        );
        catalog.stage_generation(&second).expect("stage second");
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 2)
            .expect("publish second");
        let visible = catalog
            .begin_read()
            .filter_visible_candidates(
                std::slice::from_ref(&summary.record_id),
                &QueryFilters {
                    tenant_id: "tenant-a".into(),
                    ..QueryFilters::default()
                },
            )
            .expect("summary remains visible");
        assert_eq!(visible.len(), 1);
        assert!(visible[0].generated);
        assert_eq!(
            catalog
                .document_summary("tenant-a", "library-a", "document-a")
                .unwrap()
                .unwrap()
                .source_generation,
            1
        );

        catalog
            .delete_occurrence("occurrence-a")
            .expect("delete source occurrence");
        assert!(
            catalog
                .document_summary("tenant-a", "library-a", "document-a")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn index_choice_uses_measured_threshold() {
        assert_eq!(choose_index_kind(10_000, 10_000), VectorIndexKind::Flat);
        assert_eq!(choose_index_kind(10_001, 10_000), VectorIndexKind::Hnsw);
    }
}
