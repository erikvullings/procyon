//! Dense-only semantic retrieval and deterministic file-primary grouping.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use fm_semantic_conversion::ChunkProvenance;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::embedding::EmbeddingError;
use crate::ingestion::EmbeddingProvider;
use crate::semantic_storage::{QueryFilters, SemanticCatalog, StorageError};
use crate::{SearchResult, WorkerQueryBackend, WorkerQueryInput};

const MAX_CANDIDATES: usize = 1_000;
const CANDIDATE_MULTIPLIER: usize = 4;

/// One scored result from the derived vector index.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredRecord {
    /// Stable record identity.
    pub record_id: String,
    /// Cosine similarity; larger values are better.
    pub score: f32,
}

/// Read-only derived-index query boundary.
pub trait SemanticCandidateIndex: Send + Sync {
    /// Retrieves dense-vector candidates under coarse structured filters.
    fn query(
        &self,
        vector: &[f32],
        limit: usize,
        filters: &QueryFilters,
    ) -> Result<Vec<ScoredRecord>, String>;
}

/// Honest aggregate coverage for a requested semantic scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCoverage {
    /// Eligible source count.
    pub eligible: u64,
    /// Sources with a complete visible generation.
    pub indexed: u64,
    /// Sources whose current hash differs from indexed evidence.
    pub stale: u64,
    /// Eligible sources not yet published.
    pub pending: u64,
    /// Explicitly excluded sources.
    pub excluded: u64,
    /// Sources skipped after a terminal ingestion failure.
    pub skipped: u64,
    /// Sources with terminal ingestion failures.
    pub failed: u64,
    /// Sources beneath unavailable roots.
    pub unavailable: u64,
}

impl SearchCoverage {
    /// Whether the requested scope is only partially represented.
    #[must_use]
    pub const fn partial(self) -> bool {
        self.indexed < self.eligible
            || self.stale != 0
            || self.pending != 0
            || self.excluded != 0
            || self.skipped != 0
            || self.failed != 0
            || self.unavailable != 0
    }
}

/// One bounded supporting section beneath a primary file result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticEvidence {
    /// Stable derived record identity.
    pub record_id: String,
    /// Source occurrence identity.
    pub occurrence_id: String,
    /// Opaque host source identity.
    pub source_id: String,
    /// Cosine similarity.
    pub score: f32,
    /// `chunk`, `summary`, or a future versioned kind.
    pub chunk_kind: String,
    /// Bounded display excerpt.
    pub excerpt: String,
    /// Structural heading hierarchy included in the embedding input.
    #[serde(default)]
    pub section_path: Vec<String>,
    /// Indexed IANA media type.
    pub media_type: Option<String>,
    /// Indexed source modification time.
    pub modified_at_ms: Option<i64>,
    /// Strongest real converter provenance.
    pub provenance: ChunkProvenance,
    /// Published source hash.
    pub indexed_content_hash: String,
    /// Published generation.
    pub generation: u64,
    /// Source is currently unavailable.
    pub unavailable: bool,
    /// Current known bytes differ from the indexed generation.
    pub stale: bool,
    /// Generated evidence is labelled and ranked behind extracted chunks.
    pub generated: bool,
    /// Document order.
    pub source_position: u32,
}

/// One actionable file-primary semantic result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticFileResult {
    /// Logical document identity.
    pub document_id: String,
    /// Deterministic aggregate score.
    pub score: f32,
    /// Best source occurrence for row activation.
    pub primary_source_id: String,
    /// Other exact occurrences represented by this row.
    pub additional_source_ids: Vec<String>,
    /// Best-first, bounded supporting evidence.
    pub evidence: Vec<SemanticEvidence>,
}

/// One page in the existing cancellable search lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchPage {
    /// Ranked file-primary rows.
    pub results: Vec<SemanticFileResult>,
    /// Offset for the next page.
    pub next_offset: Option<usize>,
    /// Honest requested-scope coverage.
    pub coverage: SearchCoverage,
}

/// Dense semantic query request. Query vectors are never persisted.
#[derive(Debug, Clone)]
pub struct SemanticSearchRequest {
    /// User-entered query text embedded locally without rewriting.
    pub query: String,
    /// Authorized structured filters.
    pub filters: QueryFilters,
    /// Current source hashes known by the host, keyed by opaque source ID.
    pub current_hashes: HashMap<String, String>,
    /// Requested-scope coverage supplied by the authoritative host catalog.
    pub coverage: SearchCoverage,
    /// Zero-based file offset.
    pub offset: usize,
    /// Maximum file rows.
    pub limit: usize,
    /// Maximum supporting sections retained per file.
    pub evidence_limit: usize,
}

/// Local dense retrieval capability.
pub struct SemanticSearchService {
    catalog: SemanticCatalog,
    embedder: Arc<dyn EmbeddingProvider>,
    index: Arc<dyn SemanticCandidateIndex>,
}

/// IPC adapter that routes worker query frames through dense local retrieval.
pub struct DenseWorkerQueryBackend {
    service: SemanticSearchService,
}

impl DenseWorkerQueryBackend {
    /// Wraps a configured dense retrieval service.
    #[must_use]
    pub const fn new(service: SemanticSearchService) -> Self {
        Self { service }
    }
}

impl WorkerQueryBackend for DenseWorkerQueryBackend {
    fn query(
        &self,
        input: WorkerQueryInput,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, String> {
        if let Some(concept) = input.concept_query {
            if cancellation.is_cancelled() {
                return Err(SemanticSearchError::Cancelled.to_string());
            }
            let documents = self
                .service
                .catalog
                .concept_folder_documents(
                    &QueryFilters {
                        tenant_id: input.tenant_id,
                        library_id: Some(input.library_id),
                        root_id: concept.root_id,
                        workspace_id: concept.workspace_id,
                        include_unavailable: concept.include_unavailable,
                        ..QueryFilters::default()
                    },
                    &concept.vocabulary_id,
                    &concept.concept_uris,
                    usize::try_from(concept.offset).unwrap_or(usize::MAX),
                    usize::try_from(input.maximum_results)
                        .unwrap_or(200)
                        .min(200),
                )
                .map_err(|error| error.to_string())?;
            if cancellation.is_cancelled() {
                return Err(SemanticSearchError::Cancelled.to_string());
            }
            return documents
                .into_iter()
                .map(|document| {
                    Ok(SearchResult {
                        document_id: document.document_id,
                        score: f64::from(document.confidence),
                        metadata: BTreeMap::from([
                            ("semantic.sourceId".into(), document.source_id),
                            ("available".into(), document.available.to_string()),
                            (
                                "semantic.generation".into(),
                                document.source_generation.to_string(),
                            ),
                            ("semantic.concept".into(), concept.vocabulary_id.clone()),
                            (
                                "semantic.conceptEvidence".into(),
                                serde_json::to_string(&document.supporting_chunk_ids)
                                    .map_err(|error| error.to_string())?,
                            ),
                        ]),
                        excerpt: String::new(),
                    })
                })
                .collect();
        }
        let page = self
            .service
            .search(
                SemanticSearchRequest {
                    query: input.query,
                    filters: QueryFilters {
                        tenant_id: input.tenant_id,
                        library_id: Some(input.library_id),
                        ..QueryFilters::default()
                    },
                    current_hashes: HashMap::new(),
                    coverage: SearchCoverage::default(),
                    offset: 0,
                    limit: usize::try_from(input.maximum_results).unwrap_or(500),
                    evidence_limit: 4,
                },
                cancellation,
            )
            .map_err(|error| error.to_string())?;
        let coverage_json =
            serde_json::to_string(&page.coverage).map_err(|error| error.to_string())?;
        page.results
            .into_iter()
            .map(|result| worker_result(result, &coverage_json))
            .collect()
    }
}

fn worker_result(result: SemanticFileResult, coverage_json: &str) -> Result<SearchResult, String> {
    let Some(best) = result.evidence.first() else {
        return Err("semantic result has no supporting evidence".to_owned());
    };
    let mut metadata = BTreeMap::from([
        ("semantic.sourceId".to_owned(), result.primary_source_id),
        (
            "semantic.additionalSourceIds".to_owned(),
            serde_json::to_string(&result.additional_source_ids)
                .map_err(|error| error.to_string())?,
        ),
        ("semantic.recordId".to_owned(), best.record_id.clone()),
        ("semantic.chunkKind".to_owned(), best.chunk_kind.clone()),
        (
            "semantic.indexedContentHash".to_owned(),
            best.indexed_content_hash.clone(),
        ),
        (
            "semantic.generation".to_owned(),
            best.generation.to_string(),
        ),
        (
            "semantic.available".to_owned(),
            (!best.unavailable).to_string(),
        ),
        ("semantic.stale".to_owned(), best.stale.to_string()),
        ("semantic.generated".to_owned(), best.generated.to_string()),
        (
            "semantic.sourcePosition".to_owned(),
            best.source_position.to_string(),
        ),
        (
            "semantic.provenance".to_owned(),
            serde_json::to_string(&best.provenance).map_err(|error| error.to_string())?,
        ),
        (
            "semantic.evidence".to_owned(),
            serde_json::to_string(&result.evidence).map_err(|error| error.to_string())?,
        ),
        ("semantic.coverage".to_owned(), coverage_json.to_owned()),
    ]);
    if let Some(media_type) = &best.media_type {
        metadata.insert("media_type".to_owned(), media_type.clone());
    }
    if let Some(modified_at_ms) = best.modified_at_ms {
        metadata.insert("modified_at_ms".to_owned(), modified_at_ms.to_string());
    }
    Ok(SearchResult {
        document_id: result.document_id,
        score: f64::from(result.score),
        metadata,
        excerpt: best.excerpt.clone(),
    })
}

impl SemanticSearchService {
    /// Creates a dense-only search capability.
    #[must_use]
    pub fn new(
        catalog: SemanticCatalog,
        embedder: Arc<dyn EmbeddingProvider>,
        index: Arc<dyn SemanticCandidateIndex>,
    ) -> Self {
        Self {
            catalog,
            embedder,
            index,
        }
    }

    /// Embeds the exact query locally, retrieves filtered candidates, and
    /// returns deterministic file-primary results.
    ///
    /// # Errors
    ///
    /// Returns typed validation, cancellation, embedding, index, storage, or
    /// provenance failures.
    pub fn search(
        &self,
        request: SemanticSearchRequest,
        cancellation: &CancellationToken,
    ) -> Result<SemanticSearchPage, SemanticSearchError> {
        if request.query.trim().is_empty() {
            return Err(SemanticSearchError::EmptyQuery);
        }
        if request.limit == 0 || request.evidence_limit == 0 {
            return Err(SemanticSearchError::InvalidLimit);
        }
        if cancellation.is_cancelled() {
            return Err(SemanticSearchError::Cancelled);
        }
        let query_vectors = self
            .embedder
            .embed(std::slice::from_ref(&request.query), cancellation)?;
        let query_vector = query_vectors
            .into_iter()
            .next()
            .ok_or(SemanticSearchError::MissingQueryVector)?;
        let needed_files = request
            .offset
            .checked_add(request.limit)
            .ok_or(SemanticSearchError::InvalidLimit)?;
        let candidate_limit = needed_files
            .saturating_mul(request.evidence_limit)
            .saturating_mul(CANDIDATE_MULTIPLIER)
            .clamp(1, MAX_CANDIDATES);
        let candidates = self
            .index
            .query(&query_vector, candidate_limit, &request.filters)
            .map_err(SemanticSearchError::Index)?;
        if cancellation.is_cancelled() {
            return Err(SemanticSearchError::Cancelled);
        }
        let scores = candidates
            .iter()
            .map(|candidate| (candidate.record_id.clone(), candidate.score))
            .collect::<HashMap<_, _>>();
        let candidate_ids = candidates
            .into_iter()
            .map(|candidate| candidate.record_id)
            .collect::<Vec<_>>();
        let evidence = self
            .catalog
            .begin_read()
            .filter_visible_candidates(&candidate_ids, &request.filters)?;
        let grouped = group_evidence(
            evidence
                .into_iter()
                .map(|item| {
                    let score = scores.get(&item.record_id).copied().unwrap_or_default();
                    let provenance = serde_json::from_str(&item.provenance)
                        .map_err(|_| SemanticSearchError::InvalidProvenance)?;
                    let stale = request
                        .current_hashes
                        .get(&item.source_id)
                        .is_some_and(|hash| hash != &item.content_hash);
                    Ok((
                        item.document_id,
                        SemanticEvidence {
                            record_id: item.record_id,
                            occurrence_id: item.occurrence_id,
                            source_id: item.source_id,
                            score,
                            chunk_kind: item.record_kind,
                            excerpt: item.excerpt,
                            section_path: item.section_path,
                            media_type: Some(item.media_type),
                            modified_at_ms: (item.modified_at_ms != 0)
                                .then_some(item.modified_at_ms),
                            provenance,
                            indexed_content_hash: item.content_hash,
                            generation: item.generation,
                            unavailable: !item.available,
                            stale,
                            generated: item.generated,
                            source_position: item.source_position,
                        },
                    ))
                })
                .collect::<Result<Vec<_>, SemanticSearchError>>()?,
            request.evidence_limit,
        );
        let end = request
            .offset
            .saturating_add(request.limit)
            .min(grouped.len());
        let results = grouped
            .get(request.offset..end)
            .unwrap_or_default()
            .to_vec();
        Ok(SemanticSearchPage {
            next_offset: (end < grouped.len()).then_some(end),
            results,
            coverage: request.coverage,
        })
    }
}

fn group_evidence(
    candidates: Vec<(String, SemanticEvidence)>,
    evidence_limit: usize,
) -> Vec<SemanticFileResult> {
    let mut by_document: HashMap<String, Vec<SemanticEvidence>> = HashMap::new();
    for (document_id, evidence) in candidates {
        by_document.entry(document_id).or_default().push(evidence);
    }
    let mut results = by_document
        .into_iter()
        .map(|(document_id, mut evidence)| {
            evidence.sort_by(|left, right| {
                left.generated
                    .cmp(&right.generated)
                    .then_with(|| right.score.total_cmp(&left.score))
                    .then_with(|| left.source_position.cmp(&right.source_position))
                    .then_with(|| left.record_id.cmp(&right.record_id))
            });
            let score = evidence
                .iter()
                .take(4)
                .enumerate()
                .map(|(index, evidence)| {
                    let diversity_weight = if index == 0 { 1.0 } else { 0.1 / index as f32 };
                    let generated_weight = if evidence.generated { 0.5 } else { 1.0 };
                    evidence.score * diversity_weight * generated_weight
                })
                .sum::<f32>();
            let primary_source_id = evidence[0].source_id.clone();
            let mut seen_sources = HashSet::from([primary_source_id.clone()]);
            let additional_source_ids = evidence
                .iter()
                .filter(|item| seen_sources.insert(item.source_id.clone()))
                .map(|item| item.source_id.clone())
                .collect();
            evidence.truncate(evidence_limit);
            SemanticFileResult {
                document_id,
                score,
                primary_source_id,
                additional_source_ids,
                evidence,
            }
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.document_id.cmp(&right.document_id))
            .then_with(|| left.primary_source_id.cmp(&right.primary_source_id))
    });
    results
}

/// Semantic retrieval failure.
#[derive(Debug, thiserror::Error)]
pub enum SemanticSearchError {
    /// Query text was blank.
    #[error("semantic query must not be empty")]
    EmptyQuery,
    /// Paging or evidence limit was zero or overflowed.
    #[error("semantic search limit is invalid")]
    InvalidLimit,
    /// Query was cancelled.
    #[error("semantic search cancelled")]
    Cancelled,
    /// Local embedding failed.
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    /// Local embedding returned no query vector.
    #[error("semantic embedding returned no query vector")]
    MissingQueryVector,
    /// Derived index failed.
    #[error("semantic vector query failed: {0}")]
    Index(String),
    /// Authoritative candidate filtering failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Persisted evidence provenance was malformed.
    #[error("semantic evidence provenance is invalid")]
    InvalidProvenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(
        document: &str,
        source: &str,
        record: &str,
        score: f32,
        position: u32,
        generated: bool,
    ) -> (String, SemanticEvidence) {
        (
            document.into(),
            SemanticEvidence {
                record_id: record.into(),
                occurrence_id: format!("occ-{record}"),
                source_id: source.into(),
                score,
                chunk_kind: if generated { "summary" } else { "chunk" }.into(),
                excerpt: format!("excerpt {record}"),
                section_path: vec!["Section".into()],
                media_type: Some("text/plain".into()),
                modified_at_ms: None,
                provenance: ChunkProvenance::Exact(fm_semantic_conversion::Provenance::TextLines {
                    start_line: position + 1,
                    end_line: position + 2,
                }),
                indexed_content_hash: "hash".into(),
                generation: 1,
                unavailable: false,
                stale: false,
                generated,
                source_position: position,
            },
        )
    }

    #[test]
    fn groups_unsorted_chunks_into_deterministic_diverse_file_results() {
        let grouped = group_evidence(
            vec![
                evidence("doc-b", "source-b", "b-2", 0.86, 8, false),
                evidence("doc-a", "source-a", "a-summary", 0.99, 0, true),
                evidence("doc-a", "source-copy", "a-2", 0.82, 9, false),
                evidence("doc-b", "source-b", "b-1", 0.88, 1, false),
                evidence("doc-a", "source-a", "a-1", 0.90, 1, false),
            ],
            2,
        );

        assert_eq!(
            grouped
                .iter()
                .map(|result| result.document_id.as_str())
                .collect::<Vec<_>>(),
            ["doc-a", "doc-b"]
        );
        assert_eq!(grouped[0].evidence[0].record_id, "a-1");
        assert_eq!(grouped[0].evidence.len(), 2);
        assert_eq!(grouped[0].additional_source_ids, ["source-copy"]);
        assert!(grouped[0].score > grouped[1].score);
    }

    #[test]
    fn generated_summary_cannot_displace_primary_extracted_evidence() {
        let grouped = group_evidence(
            vec![
                evidence("doc-a", "source-a", "summary", 1.0, 0, true),
                evidence("doc-a", "source-a", "chunk", 0.7, 2, false),
            ],
            2,
        );

        assert_eq!(grouped[0].evidence[0].record_id, "chunk");
        assert!(grouped[0].evidence[1].generated);
    }

    #[test]
    fn coverage_is_partial_for_every_non_ready_category() {
        for coverage in [
            SearchCoverage {
                eligible: 2,
                indexed: 1,
                ..SearchCoverage::default()
            },
            SearchCoverage {
                eligible: 1,
                indexed: 1,
                stale: 1,
                ..SearchCoverage::default()
            },
            SearchCoverage {
                eligible: 1,
                indexed: 1,
                unavailable: 1,
                ..SearchCoverage::default()
            },
        ] {
            assert!(coverage.partial());
        }
        assert!(
            !SearchCoverage {
                eligible: 1,
                indexed: 1,
                ..SearchCoverage::default()
            }
            .partial()
        );
    }
}
