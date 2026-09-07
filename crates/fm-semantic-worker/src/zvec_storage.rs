//! Optional adapter for the official dynamically linked Zvec Rust SDK.
//!
//! This module is compiled only with the `zvec` feature. Normal Procyon
//! builds neither download nor link the optional native runtime.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::OnceLock;

use fs2::FileExt;
use zvec_rust::{
    Collection, CollectionOptions, CollectionSchema, DataType, Doc, FieldSchema, IndexParams,
    MetricType, SearchQuery,
};

use crate::ingestion::{DerivedIndex, DerivedRecord};
use crate::semantic_search::{ScoredRecord, SemanticCandidateIndex};
use crate::semantic_storage::{QueryFilters, VectorIndexKind};

/// Official Rust SDK version pinned by Procyon.
pub const ZVEC_RUST_VERSION: &str = "0.7.0";
/// Native C API version paired with the pinned Rust SDK.
pub const ZVEC_NATIVE_VERSION: &str = "0.7.0";
/// Procyon schema version implemented by this adapter.
pub const ZVEC_SCHEMA_VERSION: u32 = 1;
/// Maximum retrieval candidates accepted by the worker.
pub const MAX_TOP_K: usize = 1_000;
const MAX_WRITE_BATCH_DOCUMENTS: usize = 1_024;

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
    /// Normalized FP32 embedding.
    pub embedding: Vec<f32>,
}

/// Worker-owned Zvec derived-index handle.
pub struct ZvecStorage {
    collection: Collection,
    dimensions: usize,
    _writer_lock: Option<File>,
}

impl ZvecStorage {
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
            .add_vector_field("embedding", DataType::VectorFp32, dimensions, vector_index)
            .build()?;
        let collection = Collection::create_and_open(path_string(path)?, &schema, None)?;
        Ok(Self {
            collection,
            dimensions: dimensions as usize,
            _writer_lock: Some(writer_lock),
        })
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
        let schema = collection.schema()?;
        if !schema.has_field("embedding") || !schema.has_index("embedding") {
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
        if top_k == 0 || top_k > MAX_TOP_K {
            return Err(ZvecStorageError::InvalidTopK {
                requested: top_k,
                maximum: MAX_TOP_K,
            });
        }
        if vector.len() != self.dimensions {
            return Err(ZvecStorageError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        if filters.tenant_id.is_empty() {
            return Err(ZvecStorageError::MissingTenant);
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
        .open(path.with_extension("procyon-writer.lock"))?;
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
