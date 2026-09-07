//! Bounded query planning and deterministic fusion for grounded Ask.

use fm_semantic_worker::rag_retrieval::{
    RagContext, RagContextChunk, RagRetrievalError, RagRetrievalPolicy,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(crate) const QUERY_PLANNER_VERSION: &str = "grounded-rag-query-planner/1";
pub(crate) const FUSION_VERSION: &str = "reciprocal-rank-fusion/1";
const MAX_PLANNED_QUERIES: usize = 3;
const MAX_QUERY_BYTES: usize = 8 * 1024;
const MAX_PLAN_BYTES: usize = 16 * 1024;

/// Retrieval strategy requested for one Ask turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RagRetrievalStrategy {
    /// Retrieve only with the exact user question.
    #[default]
    SingleQuery,
    /// Plan bounded rewrites and fuse their local retrieval results.
    MultiQuery,
}

/// Sanitized reason why an opted-in multi-query request used the control path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RagPlanningFallbackReason {
    /// Planner returned no queries.
    Empty,
    /// Planner returned only the unchanged question.
    DuplicateOnly,
    /// Planner explicitly declined the request.
    Refused,
    /// Planner output violated the bounded schema.
    Malformed,
    /// Planner exceeded the profile timeout.
    TimedOut,
    /// The configured planning model was unavailable.
    Unavailable,
    /// Planning failed for another sanitized reason.
    Failed,
}

/// Validated query plan used for retrieval and confirmation fingerprinting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RagQueryPlan {
    pub requested_strategy: RagRetrievalStrategy,
    pub applied_strategy: RagRetrievalStrategy,
    pub queries: Vec<String>,
    pub fallback_reason: Option<RagPlanningFallbackReason>,
}

impl RagQueryPlan {
    #[must_use]
    pub(crate) fn single(question: &str, requested_strategy: RagRetrievalStrategy) -> Self {
        Self {
            requested_strategy,
            applied_strategy: RagRetrievalStrategy::SingleQuery,
            queries: vec![question.to_owned()],
            fallback_reason: None,
        }
    }

    fn fallback(question: &str, reason: RagPlanningFallbackReason) -> Self {
        Self {
            fallback_reason: Some(reason),
            ..Self::single(question, RagRetrievalStrategy::MultiQuery)
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannerResponse {
    #[serde(default)]
    queries: Vec<String>,
    #[serde(default)]
    refused: bool,
}

pub(crate) fn parse_planner_response(question: &str, response: &str) -> RagQueryPlan {
    if response.trim().is_empty() {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Empty);
    }
    if response.len() > MAX_PLAN_BYTES {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Malformed);
    }
    let Ok(response) = serde_json::from_str::<PlannerResponse>(response) else {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Malformed);
    };
    if response.refused {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Refused);
    }
    if response.queries.is_empty() {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Empty);
    }

    let original_key = question.trim().to_lowercase();
    let mut seen = std::collections::BTreeSet::from([original_key]);
    let mut rewrites = Vec::new();
    for query in response.queries {
        let query = query.trim();
        if query.is_empty() || query.len() > MAX_QUERY_BYTES {
            return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Malformed);
        }
        if seen.insert(query.to_lowercase()) {
            rewrites.push(query.to_owned());
        }
    }
    if rewrites.is_empty() {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::DuplicateOnly);
    }
    if rewrites.len() > MAX_PLANNED_QUERIES
        || question
            .len()
            .saturating_add(rewrites.iter().map(String::len).sum::<usize>())
            > MAX_PLAN_BYTES
    {
        return RagQueryPlan::fallback(question, RagPlanningFallbackReason::Malformed);
    }
    let mut queries = Vec::with_capacity(rewrites.len() + 1);
    queries.push(question.to_owned());
    queries.extend(rewrites);
    RagQueryPlan {
        requested_strategy: RagRetrievalStrategy::MultiQuery,
        applied_strategy: RagRetrievalStrategy::MultiQuery,
        queries,
        fallback_reason: None,
    }
}

#[cfg(test)]
pub(crate) fn fuse_contexts(
    contexts: &[RagContext],
    policy: RagRetrievalPolicy,
) -> Result<RagContext, RagRetrievalError> {
    pack_ranked_chunks(fuse_ranked_chunks(contexts)?, policy)
}

pub(crate) fn fuse_ranked_chunks(
    contexts: &[RagContext],
) -> Result<Vec<RagContextChunk>, RagRetrievalError> {
    if contexts.len() > MAX_PLANNED_QUERIES + 1 {
        return Err(RagRetrievalError::InvalidPolicy);
    }

    struct FusedChunk {
        reciprocal_rank: f64,
        chunk: RagContextChunk,
    }

    let mut fused = BTreeMap::<String, FusedChunk>::new();
    for context in contexts {
        let mut ranked = context.chunks.clone();
        ranked.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.evidence.record_id.cmp(&right.evidence.record_id))
        });
        let mut seen_in_query = BTreeSet::new();
        for (rank, chunk) in ranked.into_iter().enumerate() {
            if !seen_in_query.insert(chunk.evidence.record_id.clone()) {
                continue;
            }
            let contribution = 1.0 / (60.0 + rank as f64 + 1.0);
            fused
                .entry(chunk.evidence.record_id.clone())
                .and_modify(|existing| {
                    existing.reciprocal_rank += contribution;
                    existing
                        .chunk
                        .source_citation_record_ids
                        .extend(chunk.source_citation_record_ids.clone());
                    existing.chunk.source_citation_record_ids.sort();
                    existing.chunk.source_citation_record_ids.dedup();
                    if chunk.score > existing.chunk.score {
                        existing.chunk.score = chunk.score;
                    }
                    existing.chunk.stale |= chunk.stale;
                    existing.chunk.evidence.available &= chunk.evidence.available;
                })
                .or_insert(FusedChunk {
                    reciprocal_rank: contribution,
                    chunk,
                });
        }
    }

    let mut ranked = fused.into_values().collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .reciprocal_rank
            .total_cmp(&left.reciprocal_rank)
            .then_with(|| {
                left.chunk
                    .evidence
                    .record_id
                    .cmp(&right.chunk.evidence.record_id)
            })
    });
    Ok(ranked.into_iter().map(|fused| fused.chunk).collect())
}

pub(crate) fn pack_ranked_chunks(
    ranked: Vec<RagContextChunk>,
    policy: RagRetrievalPolicy,
) -> Result<RagContext, RagRetrievalError> {
    let available = ranked.clone();
    let mut documents = BTreeSet::new();
    let mut chunks_per_document = HashMap::<String, usize>::new();
    let mut primaries = Vec::new();
    for chunk in ranked {
        let document_id = &chunk.evidence.document_id;
        let existing_for_document = chunks_per_document
            .get(document_id)
            .copied()
            .unwrap_or_default();
        if existing_for_document >= policy.maximum_chunks_per_document
            || (!documents.contains(document_id) && documents.len() >= policy.maximum_documents)
        {
            continue;
        }
        documents.insert(document_id.clone());
        chunks_per_document.insert(document_id.clone(), existing_for_document + 1);
        primaries.push(chunk);
    }

    let mut chunks = Vec::new();
    let mut seen = BTreeSet::new();
    let mut token_count = 0usize;
    for primary in primaries {
        let mut group = available
            .iter()
            .filter(|candidate| {
                candidate.evidence.source_id == primary.evidence.source_id
                    && candidate
                        .evidence
                        .source_position
                        .abs_diff(primary.evidence.source_position)
                        <= policy.adjacent_chunk_radius
            })
            .cloned()
            .collect::<Vec<_>>();
        if !group
            .iter()
            .any(|candidate| candidate.evidence.record_id == primary.evidence.record_id)
        {
            group.push(primary.clone());
        }
        group.sort_by(|left, right| {
            left.evidence
                .source_position
                .cmp(&right.evidence.source_position)
                .then_with(|| left.evidence.record_id.cmp(&right.evidence.record_id))
        });
        for mut chunk in group {
            if !seen.insert(chunk.evidence.record_id.clone()) {
                continue;
            }
            let Some(next_token_count) = token_count.checked_add(chunk.evidence.token_count) else {
                continue;
            };
            if next_token_count > policy.context_token_budget {
                continue;
            }
            token_count = next_token_count;
            chunk.adjacent = chunk.evidence.record_id != primary.evidence.record_id;
            if chunk.adjacent {
                chunk.score = primary.score;
            }
            chunk.label = format!("C{}", chunks.len() + 1);
            chunks.push(chunk);
        }
    }

    Ok(RagContext {
        insufficient: chunks.is_empty(),
        chunks,
        token_count,
    })
}

#[cfg(test)]
mod tests {
    use fm_semantic_worker::rag_retrieval::RagContextChunk;
    use fm_semantic_worker::semantic_storage::QueryEvidence;

    use super::*;

    fn chunk(record_id: &str, document_id: &str, score: f32, tokens: usize) -> RagContextChunk {
        RagContextChunk {
            label: String::new(),
            evidence: QueryEvidence {
                record_id: record_id.into(),
                library_id: "library-a".into(),
                document_id: document_id.into(),
                occurrence_id: format!("occurrence-{record_id}"),
                source_id: format!("source-{document_id}"),
                provenance: "{}".into(),
                generation: 1,
                record_kind: "chunk".into(),
                excerpt: record_id.into(),
                content: record_id.into(),
                token_count: tokens,
                section_path: Vec::new(),
                source_position: 0,
                generated: false,
                content_hash: format!("sha256:{record_id}"),
                available: true,
                media_type: "text/plain".into(),
                modified_at_ms: 1,
            },
            score,
            adjacent: false,
            source_citation_record_ids: vec![record_id.into()],
            stale: false,
        }
    }

    #[test]
    fn planner_keeps_the_original_and_accepts_at_most_three_unique_rewrites() {
        let plan = parse_planner_response(
            "How do alpha and beta differ?",
            r#"{"queries":["alpha design","beta design","alpha design","comparison criteria"]}"#,
        );

        assert_eq!(plan.applied_strategy, RagRetrievalStrategy::MultiQuery);
        assert_eq!(
            plan.queries,
            [
                "How do alpha and beta differ?",
                "alpha design",
                "beta design",
                "comparison criteria"
            ]
        );
        assert_eq!(plan.fallback_reason, None);
    }

    #[test]
    fn planner_fallbacks_are_typed_and_keep_the_exact_question() {
        let oversized = format!(r#"{{"queries":["{}"]}}"#, "x".repeat(MAX_QUERY_BYTES + 1));
        let cases = [
            ("", RagPlanningFallbackReason::Empty),
            (r#"{"queries":[]}"#, RagPlanningFallbackReason::Empty),
            (
                r#"{"queries":["  Original question  "]}"#,
                RagPlanningFallbackReason::DuplicateOnly,
            ),
            (
                r#"{"queries":["ignored"],"refused":true}"#,
                RagPlanningFallbackReason::Refused,
            ),
            ("not json", RagPlanningFallbackReason::Malformed),
            (&oversized, RagPlanningFallbackReason::Malformed),
        ];

        for (response, expected) in cases {
            let plan = parse_planner_response("Original question", response);
            assert_eq!(plan.applied_strategy, RagRetrievalStrategy::SingleQuery);
            assert_eq!(plan.queries, ["Original question"]);
            assert_eq!(plan.fallback_reason, Some(expected));
        }
    }

    #[test]
    fn reciprocal_rank_fusion_deduplicates_and_applies_final_diversity_and_budget() {
        let contexts = vec![
            RagContext {
                chunks: vec![
                    chunk("shared", "doc-a", 0.91, 4),
                    chunk("only-original", "doc-b", 0.88, 4),
                ],
                token_count: 8,
                insufficient: false,
            },
            RagContext {
                chunks: vec![
                    chunk("shared", "doc-a", 0.90, 4),
                    chunk("only-rewrite", "doc-c", 0.89, 4),
                ],
                token_count: 8,
                insufficient: false,
            },
        ];
        let policy = RagRetrievalPolicy {
            maximum_documents: 2,
            maximum_chunks_per_document: 1,
            context_token_budget: 8,
            ..RagRetrievalPolicy::default_ask()
        };

        let fused = fuse_contexts(&contexts, policy).expect("fused context");

        assert_eq!(
            fused
                .chunks
                .iter()
                .map(|chunk| (chunk.label.as_str(), chunk.evidence.record_id.as_str()))
                .collect::<Vec<_>>(),
            [("C1", "shared"), ("C2", "only-original")]
        );
        assert_eq!(fused.token_count, 8);
        assert!(!fused.insufficient);
    }

    #[test]
    fn final_document_selection_uses_fused_rank_instead_of_dense_score() {
        let contexts = vec![
            RagContext {
                chunks: vec![
                    chunk("dense-leader", "doc-a", 0.99, 4),
                    chunk("consensus", "doc-b", 0.90, 4),
                ],
                token_count: 8,
                insufficient: false,
            },
            RagContext {
                chunks: vec![chunk("consensus", "doc-b", 0.90, 4)],
                token_count: 4,
                insufficient: false,
            },
        ];
        let policy = RagRetrievalPolicy {
            maximum_documents: 1,
            maximum_chunks_per_document: 1,
            ..RagRetrievalPolicy::default_ask()
        };

        let fused = fuse_contexts(&contexts, policy).expect("fused context");

        assert_eq!(fused.chunks[0].evidence.record_id, "consensus");
    }
}
