//! Optional adapter for the official dynamically linked Zvec Rust SDK.
//!
//! This module is compiled only with the `zvec` feature. Normal Procyon
//! builds neither download nor link the optional native runtime.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fs2::FileExt;
use zvec_rust::{
    Collection, CollectionOptions, CollectionSchema, DataType, Doc, FieldSchema, Fts, IndexParams,
    MetricType, SearchQuery,
};

use crate::ingestion::{DerivedIndex, DerivedRecord};
use crate::knowledge_retrieval::FullTextCandidateIndex;
use crate::semantic_search::{ScoredRecord, SemanticCandidateIndex};
use crate::semantic_storage::{DerivedIndexRecord, QueryFilters, VectorIndexKind};

/// Official Rust SDK version pinned by Procyon.
pub const ZVEC_RUST_VERSION: &str = "0.7.0";
/// Native C API version paired with the pinned Rust SDK.
pub const ZVEC_NATIVE_VERSION: &str = "0.7.0";
/// Procyon schema version implemented by this adapter.
pub const ZVEC_SCHEMA_VERSION: u32 = 2;
/// Maximum retrieval candidates accepted by the worker.
pub const MAX_TOP_K: usize = 1_000;
const MAX_WRITE_BATCH_DOCUMENTS: usize = 1_024;
const FTS_FIELD: &str = "content";

/// Schema shape detected in an existing Procyon Zvec collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZvecSchemaState {
    /// Schema v1 with vectors and filters but no searchable content.
    VectorOnly,
    /// Schema v2 with both dense vectors and native full-text search.
    FullText,
}

/// Result of ensuring that a collection has the current FTS schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZvecMigrationOutcome {
    /// The collection already used the current schema.
    AlreadyCurrent,
    /// A vector-only collection was rebuilt and atomically replaced.
    Rebuilt,
}

#[derive(Debug)]
struct MigrationPaths {
    staging: PathBuf,
    backup: PathBuf,
    ready: PathBuf,
}

/// Audited packaging facts for one Procyon target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZvecPackagingTarget {
    /// Rust target triple.
    pub target: &'static str,
    /// Whether an official v0.7.0 native artifact exists.
    pub official_artifact: bool,
    /// Compressed official artifact bytes, when published.
    pub compressed_bytes: Option<u64>,
    /// Extracted dynamic library bytes, when published.
    pub dynamic_library_bytes: Option<u64>,
    /// Runtime dynamic library filename.
    pub library_name: &'static str,
}

/// Audited v0.7.0 packaging matrix.
pub const ZVEC_PACKAGING_TARGETS: &[ZvecPackagingTarget] = &[
    ZvecPackagingTarget {
        target: "aarch64-apple-darwin",
        official_artifact: true,
        compressed_bytes: Some(8_135_328),
        dynamic_library_bytes: Some(23_146_352),
        library_name: "libzvec_c_api.dylib",
    },
    ZvecPackagingTarget {
        target: "x86_64-apple-darwin",
        official_artifact: false,
        compressed_bytes: None,
        dynamic_library_bytes: None,
        library_name: "libzvec_c_api.dylib",
    },
    ZvecPackagingTarget {
        target: "x86_64-pc-windows-msvc",
        official_artifact: true,
        compressed_bytes: Some(8_839_865),
        dynamic_library_bytes: Some(26_570_240),
        library_name: "zvec_c_api.dll",
    },
    ZvecPackagingTarget {
        target: "x86_64-unknown-linux-gnu",
        official_artifact: true,
        compressed_bytes: Some(13_331_422),
        dynamic_library_bytes: Some(36_854_864),
        library_name: "libzvec_c_api.so",
    },
    ZvecPackagingTarget {
        target: "aarch64-unknown-linux-gnu",
        official_artifact: true,
        compressed_bytes: Some(11_784_274),
        dynamic_library_bytes: Some(32_470_624),
        library_name: "libzvec_c_api.so",
    },
];

static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();

/// One structured occurrence-level record stored in the derived index.
#[derive(Debug, Clone)]
pub struct ZvecRecord {
    /// Stable record identity and Zvec primary key.
    pub record_id: String,
    /// Tenant filter field.
    pub tenant_id: String,
    /// Library filter field.
    pub library_id: String,
    /// Root filter field.
    pub root_id: String,
    /// Optional workspace filter field.
    pub workspace_id: Option<String>,
    /// Media-type filter field.
    pub media_type: String,
    /// Source modification time.
    pub modified_at_ms: i64,
    /// Optional concept filter field.
    pub concept_id: Option<String>,
    /// Published or staging generation.
    pub generation: u64,
    /// Complete structurally bounded text used by native lexical retrieval.
    pub content: String,
    /// Normalized FP32 embedding.
    pub embedding: Vec<f32>,
}

impl From<DerivedIndexRecord> for ZvecRecord {
    fn from(record: DerivedIndexRecord) -> Self {
        Self {
            record_id: record.record_id,
            tenant_id: record.tenant_id,
            library_id: record.library_id,
            root_id: record.root_id,
            workspace_id: record.workspace_id,
            media_type: record.media_type,
            modified_at_ms: record.modified_at_ms,
            concept_id: record.concept_id,
            generation: record.generation,
            content: record.content,
            embedding: record.vector,
        }
    }
}

/// Worker-owned Zvec derived-index handle.
pub struct ZvecStorage {
    collection: Collection,
    dimensions: usize,
    _writer_lock: Option<File>,
}

impl ZvecStorage {
    /// Returns whether an interrupted staged migration has recoverable artifacts.
    #[must_use]
    pub fn migration_pending(path: &Path) -> bool {
        let paths = migration_paths(path);
        paths.staging.exists() || paths.backup.exists() || paths.ready.exists()
    }

    /// Creates a collection and takes the Procyon single-writer lock.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unsupported dimensions, writer contention,
    /// filesystem failures, or SDK failures.
    pub fn create(
        path: &Path,
        dimensions: usize,
        index_kind: VectorIndexKind,
    ) -> Result<Self, ZvecStorageError> {
        ensure_initialized()?;
        let dimensions =
            u32::try_from(dimensions).map_err(|_| ZvecStorageError::InvalidDimensions)?;
        if dimensions == 0 {
            return Err(ZvecStorageError::InvalidDimensions);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let writer_lock = acquire_writer_lock(path)?;
        let vector_index = match index_kind {
            VectorIndexKind::Flat => IndexParams::flat(MetricType::Cosine)?,
            VectorIndexKind::Hnsw => IndexParams::hnsw(MetricType::Cosine, 16, 200)?,
        };
        let mut workspace_field = FieldSchema::new("workspace_id", DataType::String, true, 0)?;
        workspace_field.set_index_params(&IndexParams::invert(false, false)?)?;
        let mut concept_field = FieldSchema::new("concept_id", DataType::String, true, 0)?;
        concept_field.set_index_params(&IndexParams::invert(false, false)?)?;
        let schema = CollectionSchema::builder("procyon-semantic-records")
            .add_indexed_field(
                "tenant_id",
                DataType::String,
                IndexParams::invert(false, false)?,
            )
            .add_indexed_field(
                "library_id",
                DataType::String,
                IndexParams::invert(false, false)?,
            )
            .add_indexed_field(
                "root_id",
                DataType::String,
                IndexParams::invert(false, false)?,
            )
            .add_field(workspace_field)
            .add_indexed_field(
                "media_type",
                DataType::String,
                IndexParams::invert(false, false)?,
            )
            .add_indexed_field(
                "modified_at_ms",
                DataType::Int64,
                IndexParams::invert(true, false)?,
            )
            .add_field(concept_field)
            .add_indexed_field(
                "generation",
                DataType::Int64,
                IndexParams::invert(true, false)?,
            )
            .add_indexed_field(
                FTS_FIELD,
                DataType::String,
                IndexParams::fts(None, None, None)?,
            )
            .add_vector_field("embedding", DataType::VectorFp32, dimensions, vector_index)
            .build()?;
        let collection = Collection::create_and_open(path_string(path)?, &schema, None)?;
        Ok(Self {
            collection,
            dimensions: dimensions as usize,
            _writer_lock: Some(writer_lock),
        })
    }

    /// Inspects whether an existing collection is vector-only or FTS-capable.
    ///
    /// # Errors
    ///
    /// Returns [`ZvecStorageError::SchemaMismatch`] for partial or unknown schemas.
    pub fn inspect_schema(path: &Path) -> Result<ZvecSchemaState, ZvecStorageError> {
        ensure_initialized()?;
        let mut options = CollectionOptions::new()?;
        options.set_read_only(true)?;
        let collection = Collection::open(path_string(path)?, Some(&options))?;
        classify_collection_schema(&collection)
    }

    /// Rebuilds a vector-only collection into the current FTS schema.
    ///
    /// The caller supplies records recovered from authoritative SQLite state.
    /// The old collection remains available until the staging collection is
    /// flushed, optimized, and verified.
    ///
    /// # Errors
    ///
    /// Returns a typed filesystem, schema, verification, or SDK error. A failed
    /// publication restores the vector-only collection.
    pub fn migrate_vector_only(
        path: &Path,
        dimensions: usize,
        index_kind: VectorIndexKind,
        records: &[ZvecRecord],
    ) -> Result<ZvecMigrationOutcome, ZvecStorageError> {
        Self::migrate_vector_only_with(path, dimensions, index_kind, |staging| {
            staging.upsert(records)?;
            u64::try_from(records.len()).map_err(|_| ZvecStorageError::MigrationVerification)
        })
    }

    /// Rebuilds the collection while a caller streams authoritative batches.
    ///
    /// The callback must return the number of records it supplied. A generic
    /// error keeps catalog paging failures typed at the composition boundary.
    ///
    /// # Errors
    ///
    /// Returns the caller's error type for source reads and converted Zvec
    /// errors for staging, verification, or publication failures.
    pub fn migrate_vector_only_with<E, F>(
        path: &Path,
        dimensions: usize,
        index_kind: VectorIndexKind,
        populate: F,
    ) -> Result<ZvecMigrationOutcome, E>
    where
        E: From<ZvecStorageError>,
        F: FnOnce(&ZvecStorage) -> Result<u64, E>,
    {
        let _migration_lock = acquire_writer_lock(path).map_err(E::from)?;
        let paths = migration_paths(path);
        recover_interrupted_migration(path, &paths).map_err(E::from)?;
        let had_existing_collection = path.exists();
        if had_existing_collection
            && Self::inspect_schema(path).map_err(E::from)? == ZvecSchemaState::FullText
        {
            remove_directory_if_exists(&paths.staging)
                .map_err(ZvecStorageError::from)
                .map_err(E::from)?;
            remove_directory_if_exists(&paths.backup)
                .map_err(ZvecStorageError::from)
                .map_err(E::from)?;
            remove_file_if_exists(&paths.ready)
                .map_err(ZvecStorageError::from)
                .map_err(E::from)?;
            return Ok(ZvecMigrationOutcome::AlreadyCurrent);
        }

        remove_directory_if_exists(&paths.staging)
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        remove_directory_if_exists(&paths.backup)
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        remove_file_if_exists(&paths.ready)
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        let staging = Self::create(&paths.staging, dimensions, index_kind).map_err(E::from)?;
        let expected_count = populate(&staging)?;
        staging.flush().map_err(E::from)?;
        staging.optimize().map_err(E::from)?;
        let stats = staging
            .collection
            .stats()
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        let schema = staging
            .collection
            .schema()
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        if stats.doc_count != expected_count
            || !schema.has_field(FTS_FIELD)
            || !schema.has_index(FTS_FIELD)
        {
            return Err(E::from(ZvecStorageError::MigrationVerification));
        }
        drop(staging);
        std::fs::write(&paths.ready, b"ready\n")
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        if had_existing_collection {
            std::fs::rename(path, &paths.backup)
                .map_err(ZvecStorageError::from)
                .map_err(E::from)?;
        }
        if let Err(error) = std::fs::rename(&paths.staging, path) {
            if paths.backup.exists() {
                std::fs::rename(&paths.backup, path)
                    .map_err(ZvecStorageError::from)
                    .map_err(E::from)?;
            }
            return Err(E::from(ZvecStorageError::Io(error)));
        }
        remove_file_if_exists(&paths.ready)
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        remove_directory_if_exists(&paths.backup)
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        remove_file_if_exists(&append_suffix(&paths.staging, ".procyon-writer.lock"))
            .map_err(ZvecStorageError::from)
            .map_err(E::from)?;
        Ok(ZvecMigrationOutcome::Rebuilt)
    }

    /// Opens an existing collection for reading or writing.
    ///
    /// Manifest compatibility must be checked through
    /// [`crate::semantic_storage::SemanticCatalog::register_library`] before
    /// this function is called.
    ///
    /// # Errors
    ///
    /// Returns a writer-contention, path, schema, or SDK error.
    pub fn open(
        path: &Path,
        expected_dimensions: usize,
        read_only: bool,
    ) -> Result<Self, ZvecStorageError> {
        ensure_initialized()?;
        let writer_lock = if read_only {
            None
        } else {
            Some(acquire_writer_lock(path)?)
        };
        let mut options = CollectionOptions::new()?;
        options.set_read_only(read_only)?;
        let collection = Collection::open(path_string(path)?, Some(&options))?;
        if classify_collection_schema(&collection)? != ZvecSchemaState::FullText
            || vector_schema_probe(&collection, expected_dimensions).is_err()
        {
            return Err(ZvecStorageError::SchemaMismatch);
        }
        Ok(Self {
            collection,
            dimensions: expected_dimensions,
            _writer_lock: writer_lock,
        })
    }

    /// Inserts or replaces structured derived records.
    ///
    /// # Errors
    ///
    /// Rejects dimension mismatches and partial SDK writes.
    pub fn upsert(&self, records: &[ZvecRecord]) -> Result<(), ZvecStorageError> {
        for batch in records.chunks(MAX_WRITE_BATCH_DOCUMENTS) {
            let documents = self.documents(batch)?;
            let references = documents.iter().collect::<Vec<_>>();
            ensure_complete_write(self.collection.upsert(&references)?)?;
        }
        Ok(())
    }

    /// Inserts records that must not already exist.
    ///
    /// # Errors
    ///
    /// Rejects dimension mismatches and partial SDK writes.
    pub fn insert(&self, records: &[ZvecRecord]) -> Result<(), ZvecStorageError> {
        let documents = self.documents(records)?;
        let references = documents.iter().collect::<Vec<_>>();
        ensure_complete_write(self.collection.insert(&references)?)
    }

    /// Updates records that must already exist.
    ///
    /// # Errors
    ///
    /// Rejects dimension mismatches and partial SDK writes.
    pub fn update(&self, records: &[ZvecRecord]) -> Result<(), ZvecStorageError> {
        let documents = self.documents(records)?;
        let references = documents.iter().collect::<Vec<_>>();
        ensure_complete_write(self.collection.update(&references)?)
    }

    fn documents(&self, records: &[ZvecRecord]) -> Result<Vec<Doc>, ZvecStorageError> {
        let mut documents = Vec::with_capacity(records.len());
        for record in records {
            if record.embedding.len() != self.dimensions {
                return Err(ZvecStorageError::DimensionMismatch {
                    expected: self.dimensions,
                    actual: record.embedding.len(),
                });
            }
            let mut document = Doc::new()?;
            document.set_pk(&record.record_id);
            document.add_string("tenant_id", &record.tenant_id)?;
            document.add_string("library_id", &record.library_id)?;
            document.add_string("root_id", &record.root_id)?;
            if let Some(workspace_id) = &record.workspace_id {
                document.add_string("workspace_id", workspace_id)?;
            } else {
                document.set_field_null("workspace_id")?;
            }
            document.add_string("media_type", &record.media_type)?;
            document.add_i64("modified_at_ms", record.modified_at_ms)?;
            if let Some(concept_id) = &record.concept_id {
                document.add_string("concept_id", concept_id)?;
            } else {
                document.set_field_null("concept_id")?;
            }
            document.add_i64(
                "generation",
                i64::try_from(record.generation)
                    .map_err(|_| ZvecStorageError::InvalidGeneration)?,
            )?;
            document.add_string(FTS_FIELD, &record.content)?;
            document.add_vector_f32("embedding", &record.embedding)?;
            documents.push(document);
        }
        Ok(documents)
    }

    /// Deletes records by stable primary key.
    ///
    /// # Errors
    ///
    /// Returns a typed SDK or partial-write error.
    pub fn delete(&self, record_ids: &[&str]) -> Result<(), ZvecStorageError> {
        for batch in record_ids.chunks(MAX_WRITE_BATCH_DOCUMENTS) {
            let result = self.collection.delete(batch)?;
            if result.error_count != 0 {
                return Err(ZvecStorageError::PartialWrite {
                    succeeded: result.success_count,
                    failed: result.error_count,
                });
            }
        }
        Ok(())
    }

    /// Executes bounded vector retrieval using structured storage filters.
    ///
    /// The returned record IDs must still be authorized against the SQLite
    /// publication snapshot because Zvec is a derived index.
    ///
    /// # Errors
    ///
    /// Rejects invalid limits, dimensions, missing tenant scope, and SDK errors.
    pub fn query_record_ids(
        &self,
        vector: &[f32],
        top_k: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, ZvecStorageError> {
        self.query_scored_records(vector, top_k, filters)
            .map(|records| records.into_iter().map(|record| record.record_id).collect())
    }

    /// Executes bounded vector retrieval and preserves similarity scores.
    ///
    /// # Errors
    ///
    /// Applies the same validation and coarse filters as
    /// [`Self::query_record_ids`].
    pub fn query_scored_records(
        &self,
        vector: &[f32],
        top_k: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<ScoredRecord>, ZvecStorageError> {
        validate_query(top_k, filters)?;
        if vector.len() != self.dimensions {
            return Err(ZvecStorageError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        let filter = compile_filter(filters)?;
        let mut query = SearchQuery::new(
            "embedding",
            vector,
            i32::try_from(top_k).map_err(|_| ZvecStorageError::InvalidTopK {
                requested: top_k,
                maximum: MAX_TOP_K,
            })?,
        )?;
        query.set_filter(&filter)?;
        query.set_include_vector(false)?;
        query.set_output_fields(&[])?;
        self.collection
            .query(&query)?
            .into_iter()
            .map(|document| {
                let record_id = document
                    .get_pk()
                    .map(str::to_owned)
                    .ok_or(ZvecStorageError::MissingPrimaryKey)?;
                Ok(ScoredRecord {
                    record_id,
                    score: 1.0 - document.get_score(),
                })
            })
            .collect()
    }

    /// Executes bounded native full-text retrieval without requiring an embedding.
    ///
    /// Returned IDs remain candidates until reauthorized against SQLite.
    ///
    /// # Errors
    ///
    /// Rejects empty queries, invalid limits, missing tenant scope, and SDK errors.
    pub fn query_full_text_record_ids(
        &self,
        text: &str,
        top_k: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, ZvecStorageError> {
        validate_query(top_k, filters)?;
        if text.trim().is_empty() {
            return Err(ZvecStorageError::EmptyFullTextQuery);
        }
        let mut fts = Fts::new()?;
        fts.set_match_string(text)?;
        let mut query = SearchQuery::fts(
            FTS_FIELD,
            &fts,
            i32::try_from(top_k).map_err(|_| ZvecStorageError::InvalidTopK {
                requested: top_k,
                maximum: MAX_TOP_K,
            })?,
        )?;
        query.set_filter(&compile_filter(filters)?)?;
        query.set_include_vector(false)?;
        query.set_output_fields(&[])?;
        self.collection
            .query(&query)?
            .into_iter()
            .map(|document| {
                document
                    .get_pk()
                    .map(str::to_owned)
                    .ok_or(ZvecStorageError::MissingPrimaryKey)
            })
            .collect()
    }

    /// Iterates a stable snapshot of all record IDs.
    ///
    /// # Errors
    ///
    /// Returns a typed SDK error.
    pub fn record_ids(&self) -> Result<Vec<String>, ZvecStorageError> {
        self.collection
            .iter_with_options(Some(&[]), false)?
            .map(|result| {
                let document = result?;
                document
                    .get_pk()
                    .map(str::to_owned)
                    .ok_or(ZvecStorageError::MissingPrimaryKey)
            })
            .collect()
    }

    /// Flushes the current WAL/segments to durable storage.
    ///
    /// # Errors
    ///
    /// Returns a typed SDK error.
    pub fn flush(&self) -> Result<(), ZvecStorageError> {
        self.collection.flush()?;
        Ok(())
    }

    /// Runs Zvec's index rebuild and segment-merge operation.
    ///
    /// # Errors
    ///
    /// Returns a typed SDK error.
    pub fn optimize(&self) -> Result<(), ZvecStorageError> {
        self.collection.optimize()?;
        Ok(())
    }
}

/// Optional Zvec adapter failure.
#[derive(Debug, thiserror::Error)]
pub enum ZvecStorageError {
    /// Official SDK failure.
    #[error("Zvec SDK failed: {0}")]
    Sdk(#[from] zvec_rust::Error),
    /// Filesystem setup failed.
    #[error("Zvec storage filesystem failed: {0}")]
    Io(#[from] std::io::Error),
    /// Another writer owns this collection.
    #[error("another process owns the semantic index writer lock")]
    WriterAlreadyOpen,
    /// Dimensions are zero or cannot fit the SDK schema type.
    #[error("invalid Zvec dimensions")]
    InvalidDimensions,
    /// Persisted collection schema is incompatible with the manifest.
    #[error("Zvec collection schema does not match the library manifest")]
    SchemaMismatch,
    /// Record or query dimensions differ from the manifest.
    #[error("vector has {actual} dimensions, expected {expected}")]
    DimensionMismatch {
        /// Expected dimensions.
        expected: usize,
        /// Actual dimensions.
        actual: usize,
    },
    /// Generation cannot be represented by the pinned SDK.
    #[error("generation exceeds the Zvec scalar range")]
    InvalidGeneration,
    /// Zvec reported only a partial batch write.
    #[error("Zvec wrote {succeeded} records and rejected {failed}")]
    PartialWrite {
        /// Successful records.
        succeeded: u64,
        /// Failed records.
        failed: u64,
    },
    /// Retrieval limit is outside Procyon's bounded contract.
    #[error("top-k {requested} is invalid; maximum is {maximum}")]
    InvalidTopK {
        /// Requested count.
        requested: usize,
        /// Maximum accepted count.
        maximum: usize,
    },
    /// Every query requires tenant scope.
    #[error("tenant filter is required")]
    MissingTenant,
    /// Native full-text retrieval requires non-whitespace input.
    #[error("full-text query is empty")]
    EmptyFullTextQuery,
    /// A returned document did not contain its Zvec primary key.
    #[error("Zvec result omitted its primary key")]
    MissingPrimaryKey,
    /// Filter value is unsafe for the pinned Zvec expression grammar.
    #[error("invalid Zvec filter value")]
    InvalidFilterValue,
    /// Collection path is not representable by the SDK.
    #[error("Zvec collection path is not valid UTF-8")]
    InvalidPath,
    /// SDK global initialization failed previously.
    #[error("Zvec initialization failed: {0}")]
    Initialization(String),
    /// A staged FTS rebuild did not contain every record and a complete index.
    #[error("staged Zvec FTS migration failed verification")]
    MigrationVerification,
}

impl DerivedIndex for ZvecStorage {
    fn upsert(&self, records: &[DerivedRecord]) -> Result<(), String> {
        let records = records
            .iter()
            .map(|record| ZvecRecord {
                record_id: record.record_id.clone(),
                tenant_id: record.tenant_id.clone(),
                library_id: record.library_id.clone(),
                root_id: record.root_id.clone(),
                workspace_id: record.workspace_id.clone(),
                media_type: record.media_type.clone(),
                modified_at_ms: record.modified_at_ms,
                concept_id: None,
                generation: record.generation,
                content: record.content.clone(),
                embedding: record.embedding.clone(),
            })
            .collect::<Vec<_>>();
        self.upsert(&records).map_err(|error| error.to_string())
    }

    fn delete(&self, record_ids: &[String]) -> Result<(), String> {
        let record_ids = record_ids.iter().map(String::as_str).collect::<Vec<_>>();
        self.delete(&record_ids).map_err(|error| error.to_string())
    }
}

impl SemanticCandidateIndex for ZvecStorage {
    fn query(
        &self,
        vector: &[f32],
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<ScoredRecord>, String> {
        self.query_scored_records(vector, limit, filters)
            .map_err(|error| error.to_string())
    }
}

impl FullTextCandidateIndex for ZvecStorage {
    fn query_full_text(
        &self,
        text: &str,
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<String>, String> {
        self.query_full_text_record_ids(text, limit, filters)
            .map_err(|error| error.to_string())
    }
}

fn ensure_initialized() -> Result<(), ZvecStorageError> {
    let result =
        INITIALIZED.get_or_init(|| zvec_rust::initialize(None).map_err(|error| error.to_string()));
    result.clone().map_err(ZvecStorageError::Initialization)
}

fn ensure_complete_write(result: zvec_rust::WriteResult) -> Result<(), ZvecStorageError> {
    if result.error_count != 0 {
        return Err(ZvecStorageError::PartialWrite {
            succeeded: result.success_count,
            failed: result.error_count,
        });
    }
    Ok(())
}

fn acquire_writer_lock(path: &Path) -> Result<File, ZvecStorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(append_suffix(path, ".procyon-writer.lock"))?;
    lock.try_lock_exclusive()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::WouldBlock => ZvecStorageError::WriterAlreadyOpen,
            _ => ZvecStorageError::Io(error),
        })?;
    Ok(lock)
}

fn path_string(path: &Path) -> Result<&str, ZvecStorageError> {
    path.to_str().ok_or(ZvecStorageError::InvalidPath)
}

fn classify_collection_schema(
    collection: &Collection,
) -> Result<ZvecSchemaState, ZvecStorageError> {
    let schema = collection.schema()?;
    if !schema.has_field("embedding") || !schema.has_index("embedding") {
        return Err(ZvecStorageError::SchemaMismatch);
    }
    match (schema.has_field(FTS_FIELD), schema.has_index(FTS_FIELD)) {
        (false, false) => Ok(ZvecSchemaState::VectorOnly),
        (true, true) => {
            for field in [
                "tenant_id",
                "library_id",
                "root_id",
                "workspace_id",
                "media_type",
                "modified_at_ms",
                "concept_id",
                "generation",
            ] {
                if !schema.has_field(field) || !schema.has_index(field) {
                    return Err(ZvecStorageError::SchemaMismatch);
                }
            }
            full_text_schema_probe(collection)?;
            Ok(ZvecSchemaState::FullText)
        }
        _ => Err(ZvecStorageError::SchemaMismatch),
    }
}

fn full_text_schema_probe(collection: &Collection) -> Result<(), ZvecStorageError> {
    let mut fts = Fts::new()?;
    fts.set_match_string("__procyon_fts_schema_probe__")?;
    let mut query = SearchQuery::fts(FTS_FIELD, &fts, 1)?;
    query.set_filter("tenant_id = '__procyon_schema_probe__'")?;
    query.set_include_vector(false)?;
    query.set_output_fields(&[])?;
    collection.query(&query)?;
    Ok(())
}

fn vector_schema_probe(
    collection: &Collection,
    expected_dimensions: usize,
) -> Result<(), ZvecStorageError> {
    let vector = vec![0.0; expected_dimensions];
    let mut query = SearchQuery::new("embedding", &vector, 1)?;
    query.set_filter("tenant_id = '__procyon_schema_probe__'")?;
    query.set_include_vector(false)?;
    query.set_output_fields(&[])?;
    collection.query(&query)?;
    Ok(())
}

fn migration_paths(path: &Path) -> MigrationPaths {
    MigrationPaths {
        staging: path.with_extension("fts-v2-staging"),
        backup: path.with_extension("fts-v1-backup"),
        ready: path.with_extension("fts-v2-ready"),
    }
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn recover_interrupted_migration(
    path: &Path,
    paths: &MigrationPaths,
) -> Result<(), ZvecStorageError> {
    if path.exists() {
        return Ok(());
    }
    if paths.ready.exists() && paths.staging.exists() {
        std::fs::rename(&paths.staging, path)?;
        remove_directory_if_exists(&paths.backup)?;
        remove_file_if_exists(&paths.ready)?;
        return Ok(());
    }
    if paths.backup.exists() {
        remove_directory_if_exists(&paths.staging)?;
        remove_file_if_exists(&paths.ready)?;
        std::fs::rename(&paths.backup, path)?;
    }
    Ok(())
}

fn remove_directory_if_exists(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn remove_file_if_exists(path: &Path) -> Result<(), std::io::Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn compile_filter(filters: &QueryFilters) -> Result<String, ZvecStorageError> {
    let mut clauses = vec![format!(
        "tenant_id = '{}'",
        escape_filter_value(&filters.tenant_id)?
    )];
    for (field, value) in [
        ("library_id", filters.library_id.as_deref()),
        ("root_id", filters.root_id.as_deref()),
        ("workspace_id", filters.workspace_id.as_deref()),
        ("media_type", filters.media_type.as_deref()),
        ("concept_id", filters.concept_id.as_deref()),
    ] {
        if let Some(value) = value {
            clauses.push(format!("{field} = '{}'", escape_filter_value(value)?));
        }
    }
    if let Some(value) = filters.modified_from_ms {
        clauses.push(format!("modified_at_ms >= {value}"));
    }
    if let Some(value) = filters.modified_to_ms {
        clauses.push(format!("modified_at_ms <= {value}"));
    }
    if let Some(value) = filters.generation {
        let value = i64::try_from(value).map_err(|_| ZvecStorageError::InvalidGeneration)?;
        clauses.push(format!("generation = {value}"));
    }
    Ok(clauses.join(" AND "))
}

fn validate_query(top_k: usize, filters: &QueryFilters) -> Result<(), ZvecStorageError> {
    if top_k == 0 || top_k > MAX_TOP_K {
        return Err(ZvecStorageError::InvalidTopK {
            requested: top_k,
            maximum: MAX_TOP_K,
        });
    }
    if filters.tenant_id.is_empty() {
        return Err(ZvecStorageError::MissingTenant);
    }
    Ok(())
}

fn escape_filter_value(value: &str) -> Result<String, ZvecStorageError> {
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(ZvecStorageError::InvalidFilterValue);
    }
    Ok(value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::*;

    fn record(id: &str, tenant: &str, category: &str, vector: Vec<f32>) -> ZvecRecord {
        ZvecRecord {
            record_id: id.into(),
            tenant_id: tenant.into(),
            library_id: "library-a".into(),
            root_id: "root-a".into(),
            workspace_id: Some("workspace-a".into()),
            media_type: category.into(),
            modified_at_ms: 1_000,
            concept_id: Some("concept-a".into()),
            generation: 1,
            content: format!("content for {id}"),
            embedding: vector,
        }
    }

    fn tenant_filters(tenant_id: &str) -> QueryFilters {
        QueryFilters {
            tenant_id: tenant_id.into(),
            ..QueryFilters::default()
        }
    }

    #[test]
    fn packaging_matrix_covers_every_desktop_target() {
        assert_eq!(ZVEC_PACKAGING_TARGETS.len(), 5);
        assert!(
            ZVEC_PACKAGING_TARGETS
                .iter()
                .any(|target| target.target == "x86_64-apple-darwin" && !target.official_artifact)
        );
        assert!(
            ZVEC_PACKAGING_TARGETS
                .iter()
                .filter(|target| target.official_artifact)
                .all(|target| target.compressed_bytes.is_some()
                    && target.dynamic_library_bytes.is_some())
        );
    }

    #[test]
    fn real_sdk_covers_schema_crud_filters_limits_iteration_optimize_and_reopen() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        let storage =
            ZvecStorage::create(&path, 3, VectorIndexKind::Flat).expect("create collection");
        storage
            .insert(&[
                record("a", "tenant-a", "text-plain", vec![1.0, 0.0, 0.0]),
                record("b", "tenant-b", "text-plain", vec![1.0, 0.0, 0.0]),
            ])
            .expect("insert");
        storage
            .update(&[record(
                "a",
                "tenant-a",
                "text-markdown",
                vec![0.9, 0.1, 0.0],
            )])
            .expect("update");
        storage
            .upsert(&[record(
                "a",
                "tenant-a",
                "text-markdown",
                vec![0.8, 0.2, 0.0],
            )])
            .expect("upsert");
        let mut filters = tenant_filters("tenant-a");
        filters.media_type = Some("text-markdown".into());
        assert_eq!(
            storage
                .query_record_ids(&[1.0, 0.0, 0.0], 10, &filters)
                .expect("query"),
            vec!["a"]
        );
        assert!(matches!(
            storage.query_record_ids(&[1.0, 0.0, 0.0], 0, &filters),
            Err(ZvecStorageError::InvalidTopK { .. })
        ));
        assert!(matches!(
            storage.query_record_ids(&[1.0, 0.0, 0.0], MAX_TOP_K + 1, &filters),
            Err(ZvecStorageError::InvalidTopK { .. })
        ));
        assert_eq!(storage.record_ids().expect("iterate").len(), 2);
        storage.delete(&["b"]).expect("delete");
        storage.optimize().expect("optimize");
        storage.flush().expect("flush");
        drop(storage);

        let reopened = ZvecStorage::open(&path, 3, false).expect("reopen");
        assert_eq!(reopened.record_ids().expect("iterate"), vec!["a"]);
    }

    #[test]
    fn upsert_batches_documents_at_the_sdk_write_limit() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 1, VectorIndexKind::Flat)
            .expect("create collection");
        let records = (0..=MAX_WRITE_BATCH_DOCUMENTS)
            .map(|index| {
                record(
                    &format!("record-{index}"),
                    "tenant-a",
                    "text-plain",
                    vec![1.0],
                )
            })
            .collect::<Vec<_>>();

        storage.upsert(&records).expect("batched upsert");

        assert_eq!(
            storage.record_ids().expect("iterate records").len(),
            records.len()
        );
    }

    #[test]
    fn deleting_no_records_is_a_no_op() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 1, VectorIndexKind::Flat)
            .expect("create collection");

        storage.delete(&[]).expect("empty delete");

        assert!(storage.record_ids().expect("iterate records").is_empty());
    }

    #[test]
    fn delete_batches_documents_at_the_sdk_write_limit() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 1, VectorIndexKind::Flat)
            .expect("create collection");
        let records = (0..=MAX_WRITE_BATCH_DOCUMENTS)
            .map(|index| {
                record(
                    &format!("record-{index}"),
                    "tenant-a",
                    "text-plain",
                    vec![1.0],
                )
            })
            .collect::<Vec<_>>();
        storage.upsert(&records).expect("batched upsert");
        let record_ids = records
            .iter()
            .map(|record| record.record_id.as_str())
            .collect::<Vec<_>>();

        storage.delete(&record_ids).expect("batched delete");

        assert!(storage.record_ids().expect("iterate records").is_empty());
    }

    #[test]
    fn hnsw_and_concurrent_readers_use_the_real_sdk() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        let writer =
            ZvecStorage::create(&path, 3, VectorIndexKind::Hnsw).expect("create collection");
        writer
            .upsert(&[record("a", "tenant-a", "text-plain", vec![1.0, 0.0, 0.0])])
            .expect("insert");
        writer.flush().expect("flush");
        drop(writer);

        let path = Arc::new(path);
        let readers = (0..4)
            .map(|_| {
                let path = path.clone();
                thread::spawn(move || {
                    let reader = ZvecStorage::open(&path, 3, true).expect("reader");
                    reader
                        .query_record_ids(&[1.0, 0.0, 0.0], 1, &tenant_filters("tenant-a"))
                        .expect("query")
                })
            })
            .collect::<Vec<_>>();
        for reader in readers {
            assert_eq!(reader.join().expect("reader thread"), vec!["a"]);
        }
    }

    #[test]
    fn cosine_scores_are_larger_for_closer_vectors() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 3, VectorIndexKind::Flat)
            .expect("create collection");
        storage
            .insert(&[
                record("near", "tenant-a", "text-plain", vec![1.0, 0.0, 0.0]),
                record("far", "tenant-a", "text-plain", vec![0.0, 1.0, 0.0]),
            ])
            .expect("insert");

        let results = storage
            .query_scored_records(&[1.0, 0.0, 0.0], 2, &tenant_filters("tenant-a"))
            .expect("query");

        assert_eq!(results[0].record_id, "near");
        assert!(results[0].score > results[1].score);
        assert!((results[0].score - 1.0).abs() < f32::EPSILON);
        assert!(results[1].score.abs() < f32::EPSILON);
    }

    #[test]
    fn full_text_search_handles_terms_phrases_case_unicode_updates_and_deletes() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 1, VectorIndexKind::Flat)
            .expect("create collection");
        let mut alpha = record("alpha", "tenant-a", "text-plain", vec![1.0]);
        alpha.content = "Fault Tree Analysis for naïve café systems".into();
        let mut beta = record("beta", "tenant-a", "text-plain", vec![1.0]);
        beta.content = "Ordinary maintenance procedure".into();
        storage.insert(&[alpha.clone(), beta]).expect("insert");

        assert_eq!(
            storage
                .query_full_text_record_ids("fault", 10, &tenant_filters("tenant-a"))
                .expect("term query"),
            vec!["alpha"]
        );
        assert_eq!(
            storage
                .query_full_text_record_ids(
                    "\"Fault Tree Analysis\"",
                    10,
                    &tenant_filters("tenant-a")
                )
                .expect("phrase query"),
            vec!["alpha"]
        );
        assert_eq!(
            storage
                .query_full_text_record_ids("NAÏVE", 10, &tenant_filters("tenant-a"))
                .expect("unicode query"),
            vec!["alpha"]
        );

        alpha.content = "Failure mode analysis".into();
        storage.update(&[alpha]).expect("update");
        assert!(
            storage
                .query_full_text_record_ids("fault", 10, &tenant_filters("tenant-a"))
                .expect("query removed term")
                .is_empty()
        );
        assert_eq!(
            storage
                .query_full_text_record_ids("failure", 10, &tenant_filters("tenant-a"))
                .expect("query updated term"),
            vec!["alpha"]
        );

        storage.delete(&["alpha"]).expect("delete");
        assert!(
            storage
                .query_full_text_record_ids("failure", 10, &tenant_filters("tenant-a"))
                .expect("query deleted term")
                .is_empty()
        );
    }

    #[test]
    fn full_text_search_preserves_tenant_scope_and_bounds() {
        let directory = tempdir().expect("temp directory");
        let storage = ZvecStorage::create(&directory.path().join("zvec"), 1, VectorIndexKind::Flat)
            .expect("create collection");
        let mut tenant_a = record("a", "tenant-a", "text-plain", vec![1.0]);
        tenant_a.content = "specialist identifier ABC-123".into();
        let mut tenant_b = record("b", "tenant-b", "text-plain", vec![1.0]);
        tenant_b.content = "specialist identifier ABC-123".into();
        storage.insert(&[tenant_a, tenant_b]).expect("insert");

        assert_eq!(
            storage
                .query_full_text_record_ids("ABC-123", 10, &tenant_filters("tenant-a"))
                .expect("scoped query"),
            vec!["a"]
        );
        assert!(matches!(
            storage.query_full_text_record_ids("ABC-123", 0, &tenant_filters("tenant-a")),
            Err(ZvecStorageError::InvalidTopK { .. })
        ));
        assert!(matches!(
            storage.query_full_text_record_ids("ABC-123", 10, &tenant_filters("")),
            Err(ZvecStorageError::MissingTenant)
        ));
    }

    #[test]
    fn migrates_a_vector_only_collection_without_losing_records() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        create_vector_only_collection(&path, 1);
        assert_eq!(
            ZvecStorage::inspect_schema(&path).expect("inspect v1"),
            ZvecSchemaState::VectorOnly
        );
        assert!(matches!(
            ZvecStorage::open(&path, 1, false),
            Err(ZvecStorageError::SchemaMismatch)
        ));
        let mut rebuilt = record("legacy", "tenant-a", "text-plain", vec![1.0]);
        rebuilt.content = "restored searchable content".into();

        assert_eq!(
            ZvecStorage::migrate_vector_only(
                &path,
                1,
                VectorIndexKind::Flat,
                std::slice::from_ref(&rebuilt),
            )
            .expect("migrate"),
            ZvecMigrationOutcome::Rebuilt
        );
        assert_eq!(
            ZvecStorage::inspect_schema(&path).expect("inspect v2"),
            ZvecSchemaState::FullText
        );
        let storage = ZvecStorage::open(&path, 1, false).expect("open migrated");
        assert_eq!(
            storage
                .query_full_text_record_ids("searchable", 10, &tenant_filters("tenant-a"),)
                .expect("query migrated"),
            vec!["legacy"]
        );
    }

    #[test]
    fn rebuilds_a_missing_derived_collection_from_authoritative_records() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        let mut rebuilt = record("restored", "tenant-a", "text-plain", vec![1.0]);
        rebuilt.content = "authoritative SQLite content".into();

        assert_eq!(
            ZvecStorage::migrate_vector_only(
                &path,
                1,
                VectorIndexKind::Flat,
                std::slice::from_ref(&rebuilt),
            )
            .expect("rebuild missing index"),
            ZvecMigrationOutcome::Rebuilt
        );
        let storage = ZvecStorage::open(&path, 1, false).expect("open rebuilt");
        assert_eq!(
            storage
                .query_full_text_record_ids("SQLite", 10, &tenant_filters("tenant-a"))
                .expect("query rebuilt"),
            vec!["restored"]
        );
    }

    #[test]
    fn rejects_a_content_field_with_the_wrong_index_type() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        let storage =
            ZvecStorage::create(&path, 1, VectorIndexKind::Flat).expect("create collection");
        storage
            .collection
            .drop_index(FTS_FIELD)
            .expect("drop FTS index");
        storage
            .collection
            .create_index(
                FTS_FIELD,
                &IndexParams::invert(false, false).expect("invert index"),
            )
            .expect("replace with wrong index");
        storage.flush().expect("flush schema");
        drop(storage);

        assert!(matches!(
            ZvecStorage::inspect_schema(&path),
            Err(ZvecStorageError::Sdk(_)) | Err(ZvecStorageError::SchemaMismatch)
        ));
    }

    #[test]
    fn migration_rolls_back_an_unpublished_staging_directory_after_restart() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        create_vector_only_collection(&path, 1);
        let paths = migration_paths(&path);
        std::fs::rename(&path, &paths.backup).expect("simulate retired v1");
        std::fs::create_dir_all(&paths.staging).expect("simulate partial staging");
        let mut invalid = record("legacy", "tenant-a", "text-plain", vec![1.0, 0.0]);
        invalid.content = "invalid migration attempt".into();
        assert!(matches!(
            ZvecStorage::migrate_vector_only(
                &path,
                1,
                VectorIndexKind::Flat,
                std::slice::from_ref(&invalid),
            ),
            Err(ZvecStorageError::DimensionMismatch { .. })
        ));
        assert_eq!(
            ZvecStorage::inspect_schema(&path).expect("restored v1"),
            ZvecSchemaState::VectorOnly
        );

        let mut rebuilt = record("legacy", "tenant-a", "text-plain", vec![1.0]);
        rebuilt.content = "restart-safe migration".into();

        ZvecStorage::migrate_vector_only(
            &path,
            1,
            VectorIndexKind::Flat,
            std::slice::from_ref(&rebuilt),
        )
        .expect("recover and migrate");

        assert!(!paths.backup.exists());
        assert!(!paths.staging.exists());
        let storage = ZvecStorage::open(&path, 1, false).expect("open recovered");
        assert_eq!(
            storage
                .query_full_text_record_ids("restart-safe", 10, &tenant_filters("tenant-a"))
                .expect("query recovered"),
            vec!["legacy"]
        );
    }

    #[test]
    fn migration_publishes_a_verified_ready_stage_after_restart() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        create_vector_only_collection(&path, 1);
        let paths = migration_paths(&path);
        let staging =
            ZvecStorage::create(&paths.staging, 1, VectorIndexKind::Flat).expect("staging index");
        let mut rebuilt = record("legacy", "tenant-a", "text-plain", vec![1.0]);
        rebuilt.content = "verified ready migration".into();
        staging
            .upsert(std::slice::from_ref(&rebuilt))
            .expect("stage record");
        staging.flush().expect("flush stage");
        staging.optimize().expect("optimize stage");
        drop(staging);
        std::fs::rename(&path, &paths.backup).expect("retire v1");
        std::fs::write(&paths.ready, b"ready\n").expect("ready marker");

        assert_eq!(
            ZvecStorage::migrate_vector_only(
                &path,
                1,
                VectorIndexKind::Flat,
                std::slice::from_ref(&rebuilt),
            )
            .expect("recover ready stage"),
            ZvecMigrationOutcome::AlreadyCurrent
        );
        assert!(!paths.backup.exists());
        assert!(!paths.ready.exists());
        let storage = ZvecStorage::open(&path, 1, false).expect("open published stage");
        assert_eq!(
            storage
                .query_full_text_record_ids("verified", 10, &tenant_filters("tenant-a"))
                .expect("query published stage"),
            vec!["legacy"]
        );
    }

    fn create_vector_only_collection(path: &Path, dimensions: u32) {
        ensure_initialized().expect("initialize");
        let schema = CollectionSchema::builder("procyon-semantic-records")
            .add_indexed_field(
                "tenant_id",
                DataType::String,
                IndexParams::invert(false, false).expect("tenant index"),
            )
            .add_vector_field(
                "embedding",
                DataType::VectorFp32,
                dimensions,
                IndexParams::flat(MetricType::Cosine).expect("vector index"),
            )
            .build()
            .expect("legacy schema");
        let collection =
            Collection::create_and_open(path_string(path).expect("path"), &schema, None)
                .expect("legacy collection");
        let mut document = Doc::new().expect("document");
        document.set_pk("legacy");
        document
            .add_string("tenant_id", "tenant-a")
            .expect("tenant");
        document
            .add_vector_f32("embedding", &[1.0])
            .expect("embedding");
        collection.insert(&[&document]).expect("insert legacy");
        collection.flush().expect("flush legacy");
    }

    #[test]
    fn adapter_enforces_single_writer() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec");
        let first = ZvecStorage::create(&path, 3, VectorIndexKind::Flat).expect("first writer");
        assert!(matches!(
            ZvecStorage::open(&path, 3, false),
            Err(ZvecStorageError::WriterAlreadyOpen)
        ));
        drop(first);
        ZvecStorage::open(&path, 3, false).expect("writer after release");
    }

    #[test]
    fn filter_values_are_escaped_inside_the_adapter() {
        let filters = QueryFilters {
            tenant_id: "tenant'quoted".into(),
            library_id: Some("library-a".into()),
            ..QueryFilters::default()
        };
        assert_eq!(
            compile_filter(&filters).expect("filter"),
            "tenant_id = 'tenant''quoted' AND library_id = 'library-a'"
        );
    }

    #[test]
    fn crash_writer_helper() {
        let Some(path) = std::env::var_os("PROCYON_ZVEC_CRASH_PATH") else {
            return;
        };
        let path = PathBuf::from(path);
        let storage =
            ZvecStorage::create(&path, 3, VectorIndexKind::Flat).expect("create crash collection");
        storage
            .insert(&[record(
                "crash-record",
                "tenant-a",
                "text-plain",
                vec![1.0, 0.0, 0.0],
            )])
            .expect("insert before crash");
        std::fs::write(path.with_extension("ready"), b"ready").expect("write ready marker");
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn abnormal_shutdown_recovers_committed_write() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("zvec-crash");
        let ready = path.with_extension("ready");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "zvec_storage::tests::crash_writer_helper",
                "--nocapture",
            ])
            .env("PROCYON_ZVEC_CRASH_PATH", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn crash writer");
        let deadline = Instant::now() + Duration::from_secs(15);
        while !ready.exists() && Instant::now() < deadline {
            if let Some(status) = child.try_wait().expect("child status") {
                panic!("crash writer exited before ready: {status}");
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "crash writer did not become ready");
        child.kill().expect("terminate crash writer");
        child.wait().expect("reap crash writer");

        let recovered = ZvecStorage::open(&path, 3, false).expect("reopen after crash");
        assert_eq!(
            recovered.record_ids().expect("recovered records"),
            vec!["crash-record"]
        );
        recovered.optimize().expect("optimize recovered collection");
        recovered.flush().expect("flush recovered collection");
    }
}
