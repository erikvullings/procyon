//! Catalog queries and derived state for corpus themes.
//!
//! Centroids and cached theme runs are derived, rebuildable rows. They are
//! tagged with the embedding model, normalization, algorithm and document
//! generation that produced them, and are removed whenever the source
//! occurrence disappears, so a revoked or excluded document never lingers in
//! derived theme state.

use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

use crate::embedding::VectorNormalization;
use crate::semantic_storage::{
    LibraryIndexManifest, SemanticCatalog, StorageError, decode_vector, encode_vector,
    validate_identifier,
};

/// Largest number of authorized sources accepted in one scope.
pub const MAX_SCOPE_SOURCES: usize = 250_000;
/// Longest chunk text returned for term statistics and excerpts.
const MAX_CHUNK_TEXT_CHARS: usize = 4_096;

/// Authorized sources of one corpus whose chunks may be analysed.
///
/// An empty source list authorizes nothing; it never means "everything".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CorpusScope {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled library.
    pub library_id: String,
    /// Exact authorized source identities.
    pub source_ids: Vec<String>,
}

impl CorpusScope {
    fn validate(&self) -> Result<(), StorageError> {
        validate_identifier(&self.tenant_id, "tenant_id")?;
        validate_identifier(&self.library_id, "library_id")?;
        if self.source_ids.len() > MAX_SCOPE_SOURCES {
            return Err(StorageError::TooManyCandidates {
                actual: self.source_ids.len(),
                maximum: MAX_SCOPE_SOURCES,
            });
        }
        Ok(())
    }

    /// Stable identity of the sorted source set within its library.
    #[must_use]
    pub fn scope_key(&self) -> String {
        let mut sources = self.source_ids.iter().collect::<Vec<_>>();
        sources.sort();
        sources.dedup();
        let mut digest = Sha256::new();
        for source in sources {
            digest.update(source.as_bytes());
            digest.update([0]);
        }
        hex(&digest.finalize())
    }
}

/// One visible document generation inside a scope.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CorpusDocument {
    /// Logical document identity.
    pub document_id: String,
    /// Visible source generation.
    pub generation: u64,
    /// Indexed content hash.
    pub content_hash: String,
    /// Number of eligible chunks.
    pub chunk_count: u32,
}

/// One extracted, embedded chunk streamed out of the catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct CorpusChunk {
    /// Stable chunk identity.
    pub chunk_id: String,
    /// Logical document identity.
    pub document_id: String,
    /// Authorized source identity.
    pub source_id: String,
    /// Visible document generation.
    pub generation: u64,
    /// Indexed content hash of the document.
    pub content_hash: String,
    /// Bounded chunk text.
    pub text: String,
    /// Normalized stored embedding.
    pub vector: Vec<f32>,
    /// Position within the document.
    pub source_position: u32,
    /// Structural heading hierarchy.
    pub section_path: Vec<String>,
    /// Serialized strongest source provenance (line numbers for JSONL).
    pub provenance: String,
    /// Optional SKOS concept label.
    pub concept_id: Option<String>,
}

/// Persisted mean of one document's chunk vectors.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredDocumentCentroid {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Library boundary.
    pub library_id: String,
    /// Logical document identity.
    pub document_id: String,
    /// Document generation the centroid was computed from.
    pub generation: u64,
    /// Content hash the centroid was computed from.
    pub content_hash: String,
    /// Embedding model and revision that produced the vectors.
    pub model_tag: String,
    /// Normalization applied to the stored vector.
    pub normalization: VectorNormalization,
    /// Versioned centroid algorithm.
    pub algorithm_version: String,
    /// Number of chunks averaged.
    pub chunk_count: u32,
    /// Re-normalized mean vector.
    pub vector: Vec<f32>,
}

impl SemanticCatalog {
    /// Reads the registered manifest of one library.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn library_manifest(
        &self,
        tenant_id: &str,
        library_id: &str,
    ) -> Result<Option<LibraryIndexManifest>, StorageError> {
        validate_identifier(tenant_id, "tenant_id")?;
        validate_identifier(library_id, "library_id")?;
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT manifest_json FROM libraries
                 WHERE tenant_id = ?1 AND library_id = ?2",
                params![tenant_id, library_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|json| serde_json::from_str(&json).map_err(StorageError::from))
            .transpose()
    }

    /// Lists visible, available documents of a scope with their chunk counts.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn corpus_documents(
        &self,
        scope: &CorpusScope,
    ) -> Result<Vec<CorpusDocument>, StorageError> {
        scope.validate()?;
        if scope.source_ids.is_empty() {
            return Ok(Vec::new());
        }
        let sources = serde_json::to_string(&scope.source_ids)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT r.document_id, r.generation, d.content_hash, COUNT(*)
             {CORPUS_FROM}
             GROUP BY r.document_id, r.generation, d.content_hash
             ORDER BY r.document_id"
        ))?;
        let rows =
            statement.query_map(params![scope.tenant_id, scope.library_id, sources], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
        rows.map(|row| {
            let (document_id, generation, content_hash, chunks) = row?;
            Ok(CorpusDocument {
                document_id,
                generation: u64::try_from(generation).map_err(|_| StorageError::CorruptCatalog)?,
                content_hash,
                chunk_count: u32::try_from(chunks).map_err(|_| StorageError::CorruptCatalog)?,
            })
        })
        .collect()
    }

    /// Streams eligible chunks ordered by document, position and chunk id.
    ///
    /// The visitor returns `false` to stop early. Only complete generations
    /// of available occurrences inside the exact source scope are visited, and
    /// generated summary chunks are never included.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, database, or persisted-data error.
    pub fn visit_corpus_chunks(
        &self,
        scope: &CorpusScope,
        mut visitor: impl FnMut(CorpusChunk) -> bool,
    ) -> Result<(), StorageError> {
        scope.validate()?;
        if scope.source_ids.is_empty() {
            return Ok(());
        }
        let sources = serde_json::to_string(&scope.source_ids)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT r.record_id, r.document_id, o.source_id, r.generation, d.content_hash,
                    substr(r.content, 1, {MAX_CHUNK_TEXT_CHARS}), v.vector,
                    r.source_position, r.section_path_json, r.provenance, r.concept_id
             {CORPUS_FROM}
             ORDER BY r.document_id, r.source_position, r.record_id"
        ))?;
        let mut rows = statement.query(params![scope.tenant_id, scope.library_id, sources])?;
        while let Some(row) = rows.next()? {
            let section_path: String = row.get(8)?;
            let chunk = CorpusChunk {
                chunk_id: row.get(0)?,
                document_id: row.get(1)?,
                source_id: row.get(2)?,
                generation: u64::try_from(row.get::<_, i64>(3)?)
                    .map_err(|_| StorageError::CorruptCatalog)?,
                content_hash: row.get(4)?,
                text: row.get(5)?,
                vector: decode_vector(&row.get::<_, Vec<u8>>(6)?)?,
                source_position: u32::try_from(row.get::<_, i64>(7)?)
                    .map_err(|_| StorageError::CorruptCatalog)?,
                section_path: serde_json::from_str(&section_path)?,
                provenance: row.get(9)?,
                concept_id: row.get(10)?,
            };
            if !visitor(chunk) {
                break;
            }
        }
        Ok(())
    }

    /// Persists or replaces one document centroid.
    ///
    /// # Errors
    ///
    /// Rejects empty or non-finite vectors and reports database failures.
    pub fn upsert_document_centroid(
        &self,
        centroid: &StoredDocumentCentroid,
    ) -> Result<(), StorageError> {
        validate_identifier(&centroid.tenant_id, "tenant_id")?;
        validate_identifier(&centroid.library_id, "library_id")?;
        validate_identifier(&centroid.document_id, "document_id")?;
        if centroid.vector.is_empty() || centroid.vector.iter().any(|value| !value.is_finite()) {
            return Err(StorageError::NonFiniteVector);
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO document_centroids (
               tenant_id, library_id, document_id, generation, content_hash, model_tag,
               normalization, algorithm_version, dimensions, chunk_count, vector)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (tenant_id, library_id, document_id) DO UPDATE SET
               generation = excluded.generation,
               content_hash = excluded.content_hash,
               model_tag = excluded.model_tag,
               normalization = excluded.normalization,
               algorithm_version = excluded.algorithm_version,
               dimensions = excluded.dimensions,
               chunk_count = excluded.chunk_count,
               vector = excluded.vector",
            params![
                centroid.tenant_id,
                centroid.library_id,
                centroid.document_id,
                i64::try_from(centroid.generation).map_err(|_| StorageError::CorruptCatalog)?,
                centroid.content_hash,
                centroid.model_tag,
                normalization_label(centroid.normalization),
                centroid.algorithm_version,
                i64::try_from(centroid.vector.len()).map_err(|_| StorageError::CorruptCatalog)?,
                i64::from(centroid.chunk_count),
                encode_vector(&centroid.vector),
            ],
        )?;
        Ok(())
    }

    /// Reads a centroid only while it still describes the visible document.
    ///
    /// A row is returned only when its generation and content hash match the
    /// document's complete generation and an occurrence still exists. The
    /// caller compares model and algorithm tags before trusting the vector.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, database, or persisted-data error.
    pub fn document_centroid(
        &self,
        tenant_id: &str,
        library_id: &str,
        document_id: &str,
    ) -> Result<Option<StoredDocumentCentroid>, StorageError> {
        validate_identifier(tenant_id, "tenant_id")?;
        validate_identifier(library_id, "library_id")?;
        validate_identifier(document_id, "document_id")?;
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT c.generation, c.content_hash, c.model_tag, c.normalization,
                        c.algorithm_version, c.chunk_count, c.vector
                 FROM document_centroids c
                 JOIN generations g
                   ON g.tenant_id = c.tenant_id AND g.library_id = c.library_id
                  AND g.document_id = c.document_id AND g.generation = c.generation
                  AND g.state = 'complete'
                 JOIN documents d
                   ON d.document_id = c.document_id AND d.content_hash = c.content_hash
                 WHERE c.tenant_id = ?1 AND c.library_id = ?2 AND c.document_id = ?3
                   AND EXISTS (
                     SELECT 1 FROM occurrences o
                     WHERE o.tenant_id = c.tenant_id AND o.library_id = c.library_id
                       AND o.document_id = c.document_id AND o.generation = c.generation)",
                params![tenant_id, library_id, document_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, Vec<u8>>(6)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(generation, content_hash, model_tag, normalization, algorithm, chunks, vector)| {
                Ok(StoredDocumentCentroid {
                    tenant_id: tenant_id.to_owned(),
                    library_id: library_id.to_owned(),
                    document_id: document_id.to_owned(),
                    generation: u64::try_from(generation)
                        .map_err(|_| StorageError::CorruptCatalog)?,
                    content_hash,
                    model_tag,
                    normalization: parse_normalization(&normalization)?,
                    algorithm_version: algorithm,
                    chunk_count: u32::try_from(chunks).map_err(|_| StorageError::CorruptCatalog)?,
                    vector: decode_vector(&vector)?,
                })
            },
        )
        .transpose()
    }

    /// Reads the cached theme run for one scope, if any.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn corpus_theme_run(
        &self,
        scope: &CorpusScope,
    ) -> Result<Option<(String, String)>, StorageError> {
        scope.validate()?;
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT fingerprint, run_json FROM corpus_theme_runs
                 WHERE tenant_id = ?1 AND library_id = ?2 AND scope_key = ?3",
                params![scope.tenant_id, scope.library_id, scope.scope_key()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?)
    }

    /// Stores the latest theme run for one scope, replacing any prior one.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or database error.
    pub fn store_corpus_theme_run(
        &self,
        scope: &CorpusScope,
        fingerprint: &str,
        run_json: &str,
    ) -> Result<(), StorageError> {
        scope.validate()?;
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO corpus_theme_runs (tenant_id, library_id, scope_key, fingerprint, run_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (tenant_id, library_id, scope_key) DO UPDATE SET
               fingerprint = excluded.fingerprint, run_json = excluded.run_json",
            params![
                scope.tenant_id,
                scope.library_id,
                scope.scope_key(),
                fingerprint,
                run_json
            ],
        )?;
        Ok(())
    }
}

/// Shared `FROM`/`WHERE` for every eligible chunk of one scope.
///
/// Parameters: `?1` tenant, `?2` library, `?3` JSON array of source ids.
const CORPUS_FROM: &str = "FROM records r
     JOIN generations g
       ON g.tenant_id = r.tenant_id AND g.library_id = r.library_id
      AND g.document_id = r.document_id AND g.generation = r.generation
      AND g.state = 'complete'
     JOIN occurrences o
       ON o.occurrence_id = r.occurrence_id AND o.generation = r.generation
     JOIN documents d ON d.document_id = r.document_id
     JOIN vectors v ON v.cache_key = r.cache_key
     WHERE r.tenant_id = ?1 AND r.library_id = ?2
       AND r.record_kind = 'chunk' AND r.generated = 0 AND o.available = 1
       AND o.source_id IN (SELECT value FROM json_each(?3))";

/// Drops derived theme state for libraries that lost an occurrence.
///
/// Cached runs embed excerpts, so any run for the library is discarded; the
/// next request recomputes it from the surviving, still-authorized sources.
pub(crate) fn discard_derived_theme_state(
    transaction: &Transaction<'_>,
    owners: &[(String, String)],
) -> Result<(), StorageError> {
    for (tenant_id, library_id) in owners {
        transaction.execute(
            "DELETE FROM corpus_theme_runs WHERE tenant_id = ?1 AND library_id = ?2",
            params![tenant_id, library_id],
        )?;
    }
    discard_orphaned_centroids(transaction)
}

/// Deletes centroids whose document no longer has any occurrence.
pub(crate) fn discard_orphaned_centroids(
    transaction: &Transaction<'_>,
) -> Result<(), StorageError> {
    transaction.execute(
        "DELETE FROM document_centroids
         WHERE NOT EXISTS (
           SELECT 1 FROM occurrences o
           WHERE o.tenant_id = document_centroids.tenant_id
             AND o.library_id = document_centroids.library_id
             AND o.document_id = document_centroids.document_id)",
        [],
    )?;
    Ok(())
}

const fn normalization_label(normalization: VectorNormalization) -> &'static str {
    match normalization {
        VectorNormalization::L2 => "l2",
    }
}

fn parse_normalization(label: &str) -> Result<VectorNormalization, StorageError> {
    match label {
        "l2" => Ok(VectorNormalization::L2),
        _ => Err(StorageError::CorruptCatalog),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut text, "{byte:02x}").expect("writing to a string cannot fail");
    }
    text
}
