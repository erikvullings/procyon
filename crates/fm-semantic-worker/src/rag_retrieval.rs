//! Bounded local retrieval for grounded Ask conversations.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::embedding::EmbeddingError;
use crate::ingestion::EmbeddingProvider;
use crate::semantic_search::SemanticCandidateIndex;
use crate::semantic_storage::{QueryEvidence, QueryFilters, SemanticCatalog, StorageError};

const MAX_DOCUMENTS: usize = 32;
const MAX_CHUNKS_PER_DOCUMENT: usize = 4;
const MAX_CONTEXT_TOKENS: usize = 32_768;

/// Resource and diversity limits for one local RAG retrieval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RagRetrievalPolicy {
    /// Minimum dense similarity accepted as evidence.
    pub minimum_score: f32,
    /// Maximum score distance below the strongest in-scope candidate.
    pub maximum_score_drop: f32,
    /// Maximum distinct source documents.
    pub maximum_documents: usize,
    /// Maximum primary hits retained from one document.
    pub maximum_chunks_per_document: usize,
    /// Maximum complete-chunk tokens in the context.
    pub context_token_budget: usize,
    /// Number of adjacent structural chunks requested around primary hits.
    pub adjacent_chunk_radius: u32,
}

impl RagRetrievalPolicy {
    /// Returns conservative defaults for interactive Ask.
    #[must_use]
    pub const fn default_ask() -> Self {
        Self {
            minimum_score: 0.84,
            maximum_score_drop: 0.02,
            maximum_documents: 8,
            maximum_chunks_per_document: 2,
            context_token_budget: 8_192,
            adjacent_chunk_radius: 1,
        }
    }

    /// Combines the absolute floor with a bounded drop from the strongest candidate.
    #[must_use]
    pub fn effective_minimum_score(self, scores: impl IntoIterator<Item = f32>) -> f32 {
        let strongest = scores
            .into_iter()
            .filter(|score| score.is_finite())
            .max_by(f32::total_cmp);
        strongest.map_or(self.minimum_score, |score| {
            self.minimum_score.max(score - self.maximum_score_drop)
        })
    }

    fn valid(self) -> bool {
        self.minimum_score.is_finite()
            && (-1.0..=1.0).contains(&self.minimum_score)
            && self.maximum_score_drop.is_finite()
            && (0.0..=2.0).contains(&self.maximum_score_drop)
            && (1..=MAX_DOCUMENTS).contains(&self.maximum_documents)
            && (1..=MAX_CHUNKS_PER_DOCUMENT).contains(&self.maximum_chunks_per_document)
            && (1..=MAX_CONTEXT_TOKENS).contains(&self.context_token_budget)
            && self.adjacent_chunk_radius <= 4
    }
}

/// Optional exact occurrence restriction for selected-file and result-set scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RagSourceRestriction {
    /// Empty means all source occurrences already authorized by query filters.
    pub allowed_source_ids: BTreeSet<String>,
}

impl RagSourceRestriction {
    fn allows(&self, source_id: &str) -> bool {
        self.allowed_source_ids.is_empty() || self.allowed_source_ids.contains(source_id)
    }
}

/// One scored row supplied to the deterministic context planner.
#[derive(Debug, Clone, PartialEq)]
pub struct RagCandidate {
    /// Authoritatively visible structural evidence.
    pub evidence: QueryEvidence,
    /// Dense similarity assigned by the local index.
    pub score: f32,
}

/// One complete source chunk retained in the generation context.
#[derive(Debug, Clone, PartialEq)]
pub struct RagContextChunk {
    /// Opaque citation label assigned after final packing.
    pub label: String,
    /// Authoritatively visible structural evidence.
    pub evidence: QueryEvidence,
    /// Dense score of the primary hit that selected this chunk.
    pub score: f32,
    /// Whether this row was added as adjacent structural context.
    pub adjacent: bool,
    /// Original extracted records to which substantive claims must cite.
    pub source_citation_record_ids: Vec<String>,
    /// Whether the host reports newer source bytes than this indexed chunk.
    pub stale: bool,
}

/// Deterministic bounded retrieval output, inspectable before generation.
#[derive(Debug, Clone, PartialEq)]
pub struct RagContext {
    /// Packed source chunks in deterministic document/source order.
    pub chunks: Vec<RagContextChunk>,
    /// Complete packed token count.
    pub token_count: usize,
    /// Whether no qualifying complete chunk fit the policy.
    pub insufficient: bool,
}

/// Typed local retrieval failures.
#[derive(Debug, thiserror::Error)]
pub enum RagRetrievalError {
    /// One or more resource limits are invalid.
    #[error("invalid RAG retrieval policy")]
    InvalidPolicy,
    /// The operation was cancelled.
    #[error("RAG retrieval was cancelled")]
    Cancelled,
    /// Query embedding failed locally.
    #[error(transparent)]
    Embedding(#[from] EmbeddingError),
    /// Derived index retrieval failed.
    #[error("RAG candidate retrieval failed: {0}")]
    Index(String),
    /// Authoritative catalog filtering failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// The local embedding backend omitted its output.
    #[error("RAG query embedding is missing")]
    MissingQueryVector,
}

/// One local dense-retrieval request.
#[derive(Debug, Clone)]
pub struct RagRetrievalRequest {
    /// Exact user question; no LLM query rewrite is performed.
    pub question: String,
    /// Authoritative tenant/library/root/workspace filters.
    pub filters: QueryFilters,
    /// Optional exact source-occurrence restriction.
    pub source_restriction: RagSourceRestriction,
    /// Current host source hashes for stale-state reporting.
    pub current_hashes: HashMap<String, String>,
    /// Retrieval and context limits.
    pub policy: RagRetrievalPolicy,
}

/// Local dense retrieval with authoritative catalog re-authorization.
pub struct RagRetrievalService {
    catalog: SemanticCatalog,
    embedder: Arc<dyn EmbeddingProvider>,
    index: Arc<dyn SemanticCandidateIndex>,
}

impl RagRetrievalService {
    /// Creates a retrieval service over one catalog, embedding runtime, and index.
    #[must_use]
    pub const fn new(
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

    /// Retrieves inspectable, complete structural evidence without contacting an LLM.
    ///
    /// # Errors
    ///
    /// Returns typed validation, cancellation, embedding, index, or catalog failures.
    pub fn retrieve(
        &self,
        request: RagRetrievalRequest,
        cancellation: &CancellationToken,
    ) -> Result<RagContext, RagRetrievalError> {
        if request.question.trim().is_empty() || !request.policy.valid() {
            return Err(RagRetrievalError::InvalidPolicy);
        }
        if cancellation.is_cancelled() {
            return Err(RagRetrievalError::Cancelled);
        }
        let vector = self
            .embedder
            .embed(std::slice::from_ref(&request.question), cancellation)?
            .into_iter()
            .next()
            .ok_or(RagRetrievalError::MissingQueryVector)?;
        let candidate_limit = request
            .policy
            .maximum_documents
            .saturating_mul(request.policy.maximum_chunks_per_document)
            .saturating_mul(8)
            .clamp(1, 512);
        let scored = self
            .index
            .query(&vector, candidate_limit, &request.filters)
            .map_err(RagRetrievalError::Index)?;
        if cancellation.is_cancelled() {
            return Err(RagRetrievalError::Cancelled);
        }
        let scores = scored
            .iter()
            .map(|item| (item.record_id.clone(), item.score))
            .collect::<HashMap<_, _>>();
        let ids = scored
            .into_iter()
            .map(|item| item.record_id)
            .collect::<Vec<_>>();
        let reader = self.catalog.begin_read();
        let visible = reader.filter_visible_candidates(&ids, &request.filters)?;
        let primaries = plan_primary_evidence(
            visible
                .into_iter()
                .map(|evidence| RagCandidate {
                    score: scores.get(&evidence.record_id).copied().unwrap_or_default(),
                    evidence,
                })
                .collect(),
            &request.source_restriction,
            request.policy,
        )?;
        let mut adjacent_by_primary = HashMap::new();
        for primary in &primaries {
            let adjacent = if primary.evidence.generated {
                vec![primary.evidence.clone()]
            } else {
                reader.adjacent_source_chunks(
                    &primary.evidence,
                    request.policy.adjacent_chunk_radius,
                    &request.filters,
                )?
            };
            adjacent_by_primary.insert(primary.evidence.record_id.clone(), adjacent);
        }
        let mut context = pack_context(&primaries, &adjacent_by_primary, request.policy)?;
        for chunk in &mut context.chunks {
            chunk.stale = request
                .current_hashes
                .get(&chunk.evidence.source_id)
                .is_some_and(|hash| hash != &chunk.evidence.content_hash);
            if chunk.evidence.generated
                && let Some(summary) = self.catalog.document_summary(
                    &request.filters.tenant_id,
                    &chunk.evidence.library_id,
                    &chunk.evidence.document_id,
                )?
                && summary.record_id == chunk.evidence.record_id
            {
                chunk.source_citation_record_ids = summary.supporting_chunk_ids;
            }
        }
        Ok(context)
    }
}

/// Plans diverse primary evidence before storage-backed adjacency expansion.
///
/// Input order is deliberately irrelevant; candidates are grouped by document,
/// score-sorted with generated summaries behind extracted chunks, and selected
/// round-robin so one document cannot consume the complete context.
pub fn plan_primary_evidence(
    candidates: Vec<RagCandidate>,
    restriction: &RagSourceRestriction,
    policy: RagRetrievalPolicy,
) -> Result<Vec<RagCandidate>, RagRetrievalError> {
    if !policy.valid() {
        return Err(RagRetrievalError::InvalidPolicy);
    }
    let candidates = candidates
        .into_iter()
        .filter(|candidate| restriction.allows(&candidate.evidence.source_id))
        .collect::<Vec<_>>();
    let has_extracted = candidates
        .iter()
        .any(|candidate| !candidate.evidence.generated);
    let minimum_score = policy.effective_minimum_score(
        candidates
            .iter()
            .filter(|candidate| !has_extracted || !candidate.evidence.generated)
            .map(|candidate| candidate.score),
    );
    let mut by_document = HashMap::<String, Vec<RagCandidate>>::new();
    for candidate in candidates {
        if candidate.score >= minimum_score {
            by_document
                .entry(candidate.evidence.document_id.clone())
                .or_default()
                .push(candidate);
        }
    }
    for candidates in by_document.values_mut() {
        candidates.sort_by(|left, right| {
            left.evidence
                .generated
                .cmp(&right.evidence.generated)
                .then_with(|| right.score.total_cmp(&left.score))
                .then_with(|| {
                    left.evidence
                        .source_position
                        .cmp(&right.evidence.source_position)
                })
                .then_with(|| left.evidence.record_id.cmp(&right.evidence.record_id))
        });
        candidates.truncate(policy.maximum_chunks_per_document);
    }
    let mut documents = by_document.into_iter().collect::<Vec<_>>();
    documents.sort_by(|(left_id, left), (right_id, right)| {
        right[0]
            .score
            .total_cmp(&left[0].score)
            .then_with(|| left_id.cmp(right_id))
    });
    documents.truncate(policy.maximum_documents);

    let mut selected = Vec::new();
    for round in 0..policy.maximum_chunks_per_document {
        for (_, candidates) in &documents {
            if let Some(candidate) = candidates.get(round) {
                selected.push(candidate.clone());
            }
        }
    }
    Ok(selected)
}

/// Packs primary and adjacent chunks intact under the token budget.
pub fn pack_context(
    primaries: &[RagCandidate],
    adjacent_by_primary: &HashMap<String, Vec<QueryEvidence>>,
    policy: RagRetrievalPolicy,
) -> Result<RagContext, RagRetrievalError> {
    if !policy.valid() {
        return Err(RagRetrievalError::InvalidPolicy);
    }
    let mut packed = Vec::<(QueryEvidence, f32, bool)>::new();
    let mut seen = HashSet::new();
    let mut token_count = 0usize;
    for primary in primaries {
        let mut group = adjacent_by_primary
            .get(&primary.evidence.record_id)
            .cloned()
            .unwrap_or_else(|| vec![primary.evidence.clone()]);
        if !group
            .iter()
            .any(|item| item.record_id == primary.evidence.record_id)
        {
            group.push(primary.evidence.clone());
        }
        group.sort_by(|left, right| {
            left.source_position
                .cmp(&right.source_position)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });
        for evidence in group {
            if !seen.insert(evidence.record_id.clone()) {
                continue;
            }
            let Some(next_tokens) = token_count.checked_add(evidence.token_count) else {
                continue;
            };
            if next_tokens > policy.context_token_budget {
                continue;
            }
            let adjacent = evidence.record_id != primary.evidence.record_id;
            token_count = next_tokens;
            packed.push((evidence, primary.score, adjacent));
        }
    }
    packed.sort_by(|(left, _, _), (right, _, _)| {
        left.document_id
            .cmp(&right.document_id)
            .then_with(|| left.source_position.cmp(&right.source_position))
            .then_with(|| left.record_id.cmp(&right.record_id))
    });
    let chunks = packed
        .into_iter()
        .enumerate()
        .map(|(index, (evidence, score, adjacent))| RagContextChunk {
            label: format!("C{}", index + 1),
            source_citation_record_ids: vec![evidence.record_id.clone()],
            evidence,
            score,
            adjacent,
            stale: false,
        })
        .collect::<Vec<_>>();
    Ok(RagContext {
        insufficient: chunks.is_empty(),
        chunks,
        token_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(
        record_id: &str,
        document_id: &str,
        source_id: &str,
        position: u32,
        tokens: usize,
        generated: bool,
    ) -> QueryEvidence {
        QueryEvidence {
            record_id: record_id.into(),
            library_id: "library-a".into(),
            document_id: document_id.into(),
            occurrence_id: format!("occurrence-{source_id}"),
            source_id: source_id.into(),
            provenance: r#"{"kind":"textLines","start_line":1,"end_line":2}"#.into(),
            generation: 1,
            record_kind: if generated { "summary" } else { "chunk" }.into(),
            excerpt: record_id.into(),
            content: format!("complete {record_id}"),
            token_count: tokens,
            section_path: vec!["Section".into()],
            source_position: position,
            generated,
            content_hash: "sha256:current".into(),
            available: true,
            media_type: "text/plain".into(),
            modified_at_ms: 1,
        }
    }

    #[test]
    fn interleaved_candidates_are_score_filtered_scope_limited_and_document_diverse() {
        let candidates = vec![
            RagCandidate {
                evidence: evidence("b2", "doc-b", "selected-b", 2, 5, false),
                score: 0.80,
            },
            RagCandidate {
                evidence: evidence("a-summary", "doc-a", "selected-a", 3, 5, true),
                score: 0.99,
            },
            RagCandidate {
                evidence: evidence("outside", "doc-c", "outside", 0, 5, false),
                score: 1.0,
            },
            RagCandidate {
                evidence: evidence("a1", "doc-a", "selected-a", 1, 5, false),
                score: 0.90,
            },
            RagCandidate {
                evidence: evidence("b1", "doc-b", "selected-b", 1, 5, false),
                score: 0.85,
            },
            RagCandidate {
                evidence: evidence("low", "doc-d", "selected-d", 0, 5, false),
                score: 0.10,
            },
        ];
        let restriction = RagSourceRestriction {
            allowed_source_ids: ["selected-a".into(), "selected-b".into()]
                .into_iter()
                .collect(),
        };
        let policy = RagRetrievalPolicy {
            minimum_score: 0.25,
            maximum_score_drop: 1.0,
            maximum_documents: 2,
            maximum_chunks_per_document: 2,
            context_token_budget: 100,
            adjacent_chunk_radius: 1,
        };

        let selected = plan_primary_evidence(candidates, &restriction, policy).unwrap();

        assert_eq!(
            selected
                .iter()
                .map(|item| item.evidence.record_id.as_str())
                .collect::<Vec<_>>(),
            ["a1", "b1", "a-summary", "b2"]
        );
    }

    #[test]
    fn relative_score_floor_rejects_distant_candidates() {
        let candidates = vec![
            RagCandidate {
                evidence: evidence("best", "doc-a", "source-a", 0, 5, false),
                score: 0.872,
            },
            RagCandidate {
                evidence: evidence("close", "doc-b", "source-b", 0, 5, false),
                score: 0.860,
            },
            RagCandidate {
                evidence: evidence("distant", "doc-c", "source-c", 0, 5, false),
                score: 0.849,
            },
        ];

        let selected = plan_primary_evidence(
            candidates,
            &RagSourceRestriction::default(),
            RagRetrievalPolicy::default_ask(),
        )
        .unwrap();

        assert_eq!(
            selected
                .iter()
                .map(|item| item.evidence.record_id.as_str())
                .collect::<Vec<_>>(),
            ["best", "close"]
        );
    }

    #[test]
    fn absolute_score_floor_rejects_an_unrelated_query_distribution() {
        let candidates = vec![
            RagCandidate {
                evidence: evidence("best", "doc-a", "source-a", 0, 5, false),
                score: 0.826,
            },
            RagCandidate {
                evidence: evidence("next", "doc-b", "source-b", 0, 5, false),
                score: 0.825,
            },
        ];

        let selected = plan_primary_evidence(
            candidates,
            &RagSourceRestriction::default(),
            RagRetrievalPolicy::default_ask(),
        )
        .unwrap();

        assert!(selected.is_empty());
    }

    #[test]
    fn adjacent_context_is_deduplicated_ordered_and_never_clipped_to_fit() {
        let policy = RagRetrievalPolicy {
            context_token_budget: 12,
            ..RagRetrievalPolicy::default_ask()
        };
        let primary = RagCandidate {
            evidence: evidence("middle", "doc-a", "source-a", 2, 5, false),
            score: 0.9,
        };
        let adjacent = HashMap::from([(
            "middle".into(),
            vec![
                evidence("after-too-large", "doc-a", "source-a", 3, 20, false),
                primary.evidence.clone(),
                evidence("before", "doc-a", "source-a", 1, 5, false),
            ],
        )]);

        let context = pack_context(&[primary], &adjacent, policy).unwrap();

        assert_eq!(context.token_count, 10);
        assert_eq!(
            context
                .chunks
                .iter()
                .map(|chunk| (chunk.label.as_str(), chunk.evidence.record_id.as_str()))
                .collect::<Vec<_>>(),
            [("C1", "before"), ("C2", "middle")]
        );
    }
}
