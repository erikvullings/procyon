//! Worker-side representative preparation and generated-summary publication.

use std::fmt::Write;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::embedding::{EmbeddingCacheKey, EmbeddingError};
use crate::ingestion::{DerivedIndex, DerivedRecord, EmbeddingProvider};
use crate::representative_selection::{
    RepresentativeChunk, RepresentativeSelection, RepresentativeSelectionConfig,
    RepresentativeSelectionError, RepresentativeSelectionMode, SummarySectionRole,
    SummarySourceChunk, representative_selection_fingerprint, select_representative_chunks,
};
use crate::semantic_storage::{
    SemanticCatalog, StorageError, StoredDocumentSummary, SummaryPublication, SummarySourceSet,
};

/// Versioned prompt contract persisted beside every generated summary.
pub const SUMMARY_PROMPT_VERSION: &str = "document-summary/1";

/// One document whose representative evidence should be prepared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareDocumentSummary {
    /// Tenant boundary.
    pub tenant_id: String,
    /// Enrolled semantic library.
    pub library_id: String,
    /// Logical document identity.
    pub document_id: String,
    /// Maximum representative input tokens.
    pub input_token_budget: usize,
}

/// Prepared real source evidence returned to the host-side generator.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDocumentSummary {
    /// Original request.
    pub request: PrepareDocumentSummary,
    /// Current source generation.
    pub source_generation: u64,
    /// Current source hash.
    pub source_content_hash: String,
    /// Source occurrence retained for derived provenance.
    pub occurrence_id: String,
    /// Source location metadata needed only for local index fields.
    pub root_id: String,
    /// Optional workspace scope.
    pub workspace_id: Option<String>,
    /// Source media type.
    pub media_type: String,
    /// Source modification time.
    pub modified_at_ms: i64,
    /// Deterministic source selection.
    pub selection: RepresentativeSelection,
    /// True when a matching prior selection was reused.
    pub reused_selection: bool,
}

/// Host-generated prose accepted back into local semantic storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedSummary {
    /// Generation profile identity.
    pub profile_id: String,
    /// Exact remote or local generation model.
    pub model_id: String,
    /// Concise overview.
    pub brief_text: String,
    /// Full structured summary.
    pub full_text: String,
    /// Host creation timestamp in Unix milliseconds.
    pub created_at_ms: i64,
}

/// Coordinates local representative selection and derived-summary storage.
pub struct DocumentSummaryService {
    catalog: SemanticCatalog,
    embedder: Arc<dyn EmbeddingProvider>,
    index: Arc<dyn DerivedIndex>,
}

impl DocumentSummaryService {
    /// Creates a summary capability over the worker's existing catalog, embedder, and index.
    #[must_use]
    pub fn new(
        catalog: SemanticCatalog,
        embedder: Arc<dyn EmbeddingProvider>,
        index: Arc<dyn DerivedIndex>,
    ) -> Self {
        Self {
            catalog,
            embedder,
            index,
        }
    }

    /// Loads existing source vectors and selects bounded representative chunks.
    pub fn prepare(
        &self,
        request: PrepareDocumentSummary,
        cancellation: &CancellationToken,
    ) -> Result<PreparedDocumentSummary, DocumentSummaryError> {
        let source = self.catalog.summary_source_chunks(
            &request.tenant_id,
            &request.library_id,
            &request.document_id,
        )?;
        let chunks = source_chunks(&source);
        let config = RepresentativeSelectionConfig::for_budget(request.input_token_budget);
        let fingerprint = representative_selection_fingerprint(
            source.generation,
            &source.content_hash,
            &self.embedder.identity().model_revision,
            &chunks,
            config,
        );
        let previous = self.catalog.document_summary(
            &request.tenant_id,
            &request.library_id,
            &request.document_id,
        )?;
        let (selection, reused_selection) = previous
            .filter(|summary| summary.selection_fingerprint == fingerprint)
            .and_then(|summary| reuse_selection(&chunks, config, fingerprint.clone(), &summary))
            .map_or_else(
                || {
                    select_representative_chunks(
                        source.generation,
                        &source.content_hash,
                        &self.embedder.identity().model_revision,
                        &chunks,
                        config,
                        cancellation,
                    )
                    .map(|selection| (selection, false))
                },
                |selection| Ok((selection, true)),
            )?;
        Ok(PreparedDocumentSummary {
            request,
            source_generation: source.generation,
            source_content_hash: source.content_hash,
            occurrence_id: source.occurrence_id,
            root_id: source.root_id,
            workspace_id: source.workspace_id,
            media_type: source.media_type,
            modified_at_ms: source.modified_at_ms,
            selection,
            reused_selection,
        })
    }

    /// Embeds and atomically publishes host-generated prose as derived evidence.
    pub fn publish(
        &self,
        prepared: &PreparedDocumentSummary,
        generated: GeneratedSummary,
        cancellation: &CancellationToken,
    ) -> Result<StoredDocumentSummary, DocumentSummaryError> {
        if cancellation.is_cancelled() {
            return Err(DocumentSummaryError::Cancelled);
        }
        let vectors = self
            .embedder
            .embed(std::slice::from_ref(&generated.full_text), cancellation)?;
        let vector = vectors
            .into_iter()
            .next()
            .ok_or(DocumentSummaryError::MissingEmbedding)?;
        let identity = self.embedder.identity();
        let cache_key = EmbeddingCacheKey::calculate(
            &generated.full_text,
            identity,
            &identity.tokenizer,
            SUMMARY_PROMPT_VERSION,
        );
        let record_id = summary_record_id(
            &prepared.request,
            &prepared.selection.fingerprint,
            &generated.profile_id,
            &generated.model_id,
        );
        let summary = StoredDocumentSummary {
            record_id: record_id.clone(),
            source_generation: prepared.source_generation,
            source_content_hash: prepared.source_content_hash.clone(),
            profile_id: generated.profile_id,
            model_id: generated.model_id,
            algorithm_version: prepared.selection.algorithm_version.to_owned(),
            prompt_version: SUMMARY_PROMPT_VERSION.to_owned(),
            selection_fingerprint: prepared.selection.fingerprint.clone(),
            supporting_chunk_ids: prepared
                .selection
                .representatives
                .iter()
                .map(|item| item.source.chunk_id.clone())
                .collect(),
            supporting_weights: prepared
                .selection
                .representatives
                .iter()
                .map(|item| item.cluster_weight)
                .collect(),
            created_at_ms: generated.created_at_ms,
            brief_text: generated.brief_text,
            full_text: generated.full_text,
        };
        let provenance = prepared
            .selection
            .representatives
            .first()
            .map(|item| item.source.provenance.clone())
            .ok_or(DocumentSummaryError::NoRepresentatives)?;
        let derived = DerivedRecord {
            record_id: record_id.clone(),
            tenant_id: prepared.request.tenant_id.clone(),
            library_id: prepared.request.library_id.clone(),
            root_id: prepared.root_id.clone(),
            workspace_id: prepared.workspace_id.clone(),
            media_type: prepared.media_type.clone(),
            modified_at_ms: prepared.modified_at_ms,
            generation: prepared.source_generation,
            embedding: vector.clone(),
            content: summary.full_text.clone(),
            excerpt: summary.brief_text.clone(),
        };
        self.index
            .upsert(std::slice::from_ref(&derived))
            .map_err(DocumentSummaryError::Index)?;
        let old_record = self.catalog.publish_summary(
            &prepared.request.tenant_id,
            &prepared.request.library_id,
            &prepared.request.document_id,
            &SummaryPublication {
                summary: summary.clone(),
                occurrence_id: prepared.occurrence_id.clone(),
                cache_key,
                vector,
                provenance,
            },
        )?;
        if let Some(old_record) = old_record.filter(|old| old != &record_id) {
            self.index
                .delete(&[old_record])
                .map_err(DocumentSummaryError::Index)?;
        }
        Ok(summary)
    }

    /// Returns the current summary and whether its source generation is stale.
    pub fn current(
        &self,
        request: &PrepareDocumentSummary,
    ) -> Result<Option<(StoredDocumentSummary, bool)>, DocumentSummaryError> {
        let Some(summary) = self.catalog.document_summary(
            &request.tenant_id,
            &request.library_id,
            &request.document_id,
        )?
        else {
            return Ok(None);
        };
        let source = self.catalog.summary_source_chunks(
            &request.tenant_id,
            &request.library_id,
            &request.document_id,
        )?;
        let stale = summary.source_generation != source.generation
            || summary.source_content_hash != source.content_hash;
        Ok(Some((summary, stale)))
    }
}

fn source_chunks(source: &SummarySourceSet) -> Vec<SummarySourceChunk> {
    source
        .chunks
        .iter()
        .map(|chunk| SummarySourceChunk {
            chunk_id: chunk.record_id.clone(),
            text: chunk.content.clone(),
            embedding: chunk.vector.clone(),
            token_count: chunk.token_count as usize,
            source_position: chunk.source_position,
            section_path: chunk.section_path.clone(),
            provenance: chunk.provenance.clone(),
            role: match chunk.structural_role.as_str() {
                "title" => SummarySectionRole::Title,
                "introduction" => SummarySectionRole::Introduction,
                "conclusion" => SummarySectionRole::Conclusion,
                _ => SummarySectionRole::Body,
            },
            generated: false,
        })
        .collect()
}

fn reuse_selection(
    chunks: &[SummarySourceChunk],
    config: RepresentativeSelectionConfig,
    fingerprint: String,
    summary: &StoredDocumentSummary,
) -> Option<RepresentativeSelection> {
    let mut representatives = summary
        .supporting_chunk_ids
        .iter()
        .zip(&summary.supporting_weights)
        .map(|(record_id, weight)| {
            let source = chunks
                .iter()
                .find(|chunk| &chunk.chunk_id == record_id)?
                .clone();
            Some(RepresentativeChunk {
                structural_anchor: source.role != SummarySectionRole::Body,
                source,
                cluster_population: ((*weight * chunks.len() as f32).round() as usize).max(1),
                cluster_weight: *weight,
                centroid_distance: 0.0,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    representatives.sort_by_key(|item| item.source.source_position);
    let selected_tokens = representatives
        .iter()
        .map(|item| item.source.token_count)
        .sum();
    (selected_tokens <= config.input_token_budget).then_some(RepresentativeSelection {
        mode: if chunks.iter().map(|chunk| chunk.token_count).sum::<usize>()
            <= config.input_token_budget
        {
            RepresentativeSelectionMode::AllChunks
        } else {
            RepresentativeSelectionMode::ClusterMedoids
        },
        representatives,
        selected_tokens,
        fingerprint,
        algorithm_version: "representative-kmeans/1",
    })
}

fn summary_record_id(
    request: &PrepareDocumentSummary,
    selection_fingerprint: &str,
    profile_id: &str,
    model_id: &str,
) -> String {
    let mut digest = Sha256::new();
    for value in [
        request.tenant_id.as_str(),
        request.library_id.as_str(),
        request.document_id.as_str(),
        selection_fingerprint,
        profile_id,
        model_id,
    ] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    let mut record_id = String::from("summary-");
    for byte in digest.finalize() {
        write!(&mut record_id, "{byte:02x}").expect("writing to a string cannot fail");
    }
    record_id
}

/// Typed summary preparation and publication failures.
#[derive(Debug, thiserror::Error)]
pub enum DocumentSummaryError {
    /// Authoritative semantic storage rejected the operation.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Representative selection failed.
    #[error(transparent)]
    Selection(#[from] RepresentativeSelectionError),
    /// Local embedding failed.
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    /// Local embedding returned no vector.
    #[error("summary embedding output is missing")]
    MissingEmbedding,
    /// Selection unexpectedly contained no real source chunks.
    #[error("summary has no representative source chunks")]
    NoRepresentatives,
    /// Derived index update failed.
    #[error("derived summary index update failed: {0}")]
    Index(String),
    /// Operation was cancelled.
    #[error("summary operation was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tempfile::tempdir;

    use super::*;
    use crate::embedding::{EmbeddingModelIdentity, VectorNormalization};
    use crate::semantic_storage::{
        DistanceMetric, LibraryIndexManifest, Occurrence, StagedGeneration, StagedRecord,
    };

    struct FakeEmbedder {
        identity: EmbeddingModelIdentity,
        inputs: Mutex<Vec<String>>,
    }

    impl EmbeddingProvider for FakeEmbedder {
        fn identity(&self) -> &EmbeddingModelIdentity {
            &self.identity
        }

        fn embed(
            &self,
            inputs: &[String],
            cancellation: &CancellationToken,
        ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            if cancellation.is_cancelled() {
                return Err(EmbeddingError::Cancelled);
            }
            self.inputs.lock().unwrap().extend_from_slice(inputs);
            Ok(inputs.iter().map(|_| vec![0.0, 0.6, 0.8]).collect())
        }
    }

    #[derive(Default)]
    struct FakeIndex {
        records: Mutex<Vec<DerivedRecord>>,
        deleted: Mutex<Vec<String>>,
    }

    impl DerivedIndex for FakeIndex {
        fn upsert(&self, records: &[DerivedRecord]) -> Result<(), String> {
            self.records.lock().unwrap().extend_from_slice(records);
            Ok(())
        }

        fn delete(&self, record_ids: &[String]) -> Result<(), String> {
            self.deleted.lock().unwrap().extend_from_slice(record_ids);
            Ok(())
        }
    }

    fn summary_service() -> (
        DocumentSummaryService,
        SemanticCatalog,
        Arc<FakeEmbedder>,
        Arc<FakeIndex>,
    ) {
        let directory = tempdir().unwrap().keep();
        let catalog = SemanticCatalog::open(directory.join("catalog.sqlite")).unwrap();
        let identity = EmbeddingModelIdentity {
            model_id: "embed-model".into(),
            model_revision: "embed-revision".into(),
            tokenizer: "tokenizer-revision".into(),
            dimensions: 3,
            max_input_tokens: 8_192,
        };
        catalog
            .register_library(
                "tenant-a",
                "library-a",
                &LibraryIndexManifest {
                    zvec_schema_version: 1,
                    dimensions: 3,
                    distance_metric: DistanceMetric::Cosine,
                    model_revision: identity.model_revision.clone(),
                    tokenizer: identity.tokenizer.clone(),
                    normalization: VectorNormalization::L2,
                    chunker_version: "structural/2".into(),
                    converter_version: "text/1".into(),
                },
            )
            .unwrap();
        let occurrence = Occurrence {
            occurrence_id: "occurrence-a".into(),
            source_id: "source-a".into(),
            root_id: "root-a".into(),
            workspace_id: Some("workspace-a".into()),
            media_type: "text/plain".into(),
            modified_at_ms: 1_000,
            available: true,
            provenance: "source".into(),
        };
        let records = [
            ("title", 0, "title"),
            ("body", 1, "body"),
            ("conclusion", 2, "conclusion"),
        ]
        .into_iter()
        .map(|(id, position, role)| {
            let content = format!("{id} complete text");
            StagedRecord {
                record_id: id.into(),
                occurrence_id: occurrence.occurrence_id.clone(),
                cache_key: EmbeddingCacheKey::calculate(
                    &content,
                    &identity,
                    &identity.tokenizer,
                    "structural/2",
                ),
                vector: vec![position as f32, 0.6, 0.8],
                record_kind: "chunk".into(),
                excerpt: content.clone(),
                content,
                token_count: 10,
                section_path: vec![id.into()],
                structural_role: role.into(),
                provenance: format!(
                    "{{\"kind\":\"textLines\",\"start_line\":{},\"end_line\":{}}}",
                    position + 1,
                    position + 1
                ),
                source_position: position,
                generated: false,
                concept_id: None,
            }
        })
        .collect();
        catalog
            .stage_generation(&StagedGeneration {
                tenant_id: "tenant-a".into(),
                library_id: "library-a".into(),
                document_id: "document-a".into(),
                content_hash: "hash-a".into(),
                generation: 1,
                occurrences: vec![occurrence],
                records,
            })
            .unwrap();
        catalog
            .publish_generation("tenant-a", "library-a", "document-a", 1)
            .unwrap();
        let embedder = Arc::new(FakeEmbedder {
            identity,
            inputs: Mutex::new(Vec::new()),
        });
        let index = Arc::new(FakeIndex::default());
        (
            DocumentSummaryService::new(catalog.clone(), embedder.clone(), index.clone()),
            catalog,
            embedder,
            index,
        )
    }

    #[test]
    fn prepare_publish_and_regenerate_reuse_source_evidence() {
        let (service, catalog, embedder, index) = summary_service();
        let request = PrepareDocumentSummary {
            tenant_id: "tenant-a".into(),
            library_id: "library-a".into(),
            document_id: "document-a".into(),
            input_token_budget: 100,
        };
        let prepared = service
            .prepare(request.clone(), &CancellationToken::new())
            .unwrap();
        assert_eq!(prepared.selection.representatives.len(), 3);
        assert!(embedder.inputs.lock().unwrap().is_empty());

        let summary = service
            .publish(
                &prepared,
                GeneratedSummary {
                    profile_id: "profile-a".into(),
                    model_id: "generator-a".into(),
                    brief_text: "Brief overview.".into(),
                    full_text: "Full grounded summary.".into(),
                    created_at_ms: 2_000,
                },
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(
            summary.supporting_chunk_ids,
            ["title", "body", "conclusion"]
        );
        assert_eq!(
            embedder.inputs.lock().unwrap().as_slice(),
            ["Full grounded summary."]
        );
        let indexed = index.records.lock().unwrap();
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].record_id, summary.record_id);
        drop(indexed);
        assert_eq!(
            catalog
                .begin_read()
                .expect("reader")
                .filter_visible_candidates(
                    std::slice::from_ref(&summary.record_id),
                    &crate::semantic_storage::QueryFilters {
                        tenant_id: "tenant-a".into(),
                        ..crate::semantic_storage::QueryFilters::default()
                    },
                )
                .unwrap()
                .len(),
            1
        );

        let regenerated = service.prepare(request, &CancellationToken::new()).unwrap();
        assert!(regenerated.reused_selection);
        assert_eq!(
            regenerated.selection.fingerprint,
            prepared.selection.fingerprint
        );
        assert!(!service.current(&regenerated.request).unwrap().unwrap().1);
    }
}
