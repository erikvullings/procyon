//! Protobuf mapping for the versioned knowledge-retrieval RPC.
//!
//! The wire model is a projection of [`crate::knowledge_retrieval`]: it carries
//! opaque identities, bounded content the host already admitted, and structural
//! provenance. Filesystem paths and host consent never cross this boundary.

use std::collections::HashMap;

use fm_semantic_conversion::ChunkProvenance;
use fm_semantic_protocol::v1;

use crate::knowledge_retrieval::{
    DEFAULT_RANK_CONSTANT, KnowledgeCapabilities, KnowledgeEvidence, KnowledgeQuery,
    KnowledgeRetrieval, KnowledgeRetrievalPolicy, KnowledgeRetrievalReason,
    KnowledgeRetrievalRequest, KnowledgeRetrievalScope, KnowledgeRoute, KnowledgeSourceRestriction,
    RankContribution, RetrievalTrace, RouteFallbackReason, RouteOutcome, TracedEvidence,
    TracedQuery,
};
use crate::semantic_search::SearchCoverage;
use crate::semantic_storage::QueryFilters;

/// Wire mapping failure; never carries host content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeWireError {
    /// A required wire field was absent.
    #[error("knowledge message is missing {0}")]
    Missing(&'static str),
    /// A wire enum value is not understood by this protocol version.
    #[error("knowledge message carries an unsupported {0}")]
    Unsupported(&'static str),
    /// Structural provenance could not be encoded or decoded.
    #[error("knowledge evidence provenance is invalid")]
    InvalidProvenance,
    /// The encoded response exceeds the negotiated message budget.
    #[error("knowledge response is {encoded} bytes; the maximum is {maximum}")]
    ResponseTooLarge {
        /// Exact encoded byte length.
        encoded: usize,
        /// Deterministic budget the response must fit in.
        maximum: usize,
    },
}

const fn route_to_wire(route: KnowledgeRoute) -> v1::KnowledgeRoute {
    match route {
        KnowledgeRoute::Hybrid => v1::KnowledgeRoute::Hybrid,
        KnowledgeRoute::FullText => v1::KnowledgeRoute::FullText,
        KnowledgeRoute::Semantic => v1::KnowledgeRoute::Semantic,
    }
}

fn route_from_wire(route: i32) -> Result<KnowledgeRoute, KnowledgeWireError> {
    match v1::KnowledgeRoute::try_from(route) {
        Ok(v1::KnowledgeRoute::Hybrid) => Ok(KnowledgeRoute::Hybrid),
        Ok(v1::KnowledgeRoute::FullText) => Ok(KnowledgeRoute::FullText),
        Ok(v1::KnowledgeRoute::Semantic) => Ok(KnowledgeRoute::Semantic),
        _ => Err(KnowledgeWireError::Unsupported("route")),
    }
}

const fn reason_to_wire(reason: KnowledgeRetrievalReason) -> v1::KnowledgeRetrievalReason {
    match reason {
        KnowledgeRetrievalReason::Subject => v1::KnowledgeRetrievalReason::Subject,
        KnowledgeRetrievalReason::Overview => v1::KnowledgeRetrievalReason::Overview,
        KnowledgeRetrievalReason::Definition => v1::KnowledgeRetrievalReason::Definition,
        KnowledgeRetrievalReason::Procedure => v1::KnowledgeRetrievalReason::Procedure,
        KnowledgeRetrievalReason::Examples => v1::KnowledgeRetrievalReason::Examples,
        KnowledgeRetrievalReason::Evidence => v1::KnowledgeRetrievalReason::Evidence,
        KnowledgeRetrievalReason::Arguments => v1::KnowledgeRetrievalReason::Arguments,
        KnowledgeRetrievalReason::Comparison => v1::KnowledgeRetrievalReason::Comparison,
        KnowledgeRetrievalReason::Limitations => v1::KnowledgeRetrievalReason::Limitations,
        KnowledgeRetrievalReason::References => v1::KnowledgeRetrievalReason::References,
        KnowledgeRetrievalReason::Related => v1::KnowledgeRetrievalReason::Related,
    }
}

fn reason_from_wire(reason: i32) -> Result<KnowledgeRetrievalReason, KnowledgeWireError> {
    match v1::KnowledgeRetrievalReason::try_from(reason) {
        Ok(v1::KnowledgeRetrievalReason::Subject) => Ok(KnowledgeRetrievalReason::Subject),
        Ok(v1::KnowledgeRetrievalReason::Overview) => Ok(KnowledgeRetrievalReason::Overview),
        Ok(v1::KnowledgeRetrievalReason::Definition) => Ok(KnowledgeRetrievalReason::Definition),
        Ok(v1::KnowledgeRetrievalReason::Procedure) => Ok(KnowledgeRetrievalReason::Procedure),
        Ok(v1::KnowledgeRetrievalReason::Examples) => Ok(KnowledgeRetrievalReason::Examples),
        Ok(v1::KnowledgeRetrievalReason::Evidence) => Ok(KnowledgeRetrievalReason::Evidence),
        Ok(v1::KnowledgeRetrievalReason::Arguments) => Ok(KnowledgeRetrievalReason::Arguments),
        Ok(v1::KnowledgeRetrievalReason::Comparison) => Ok(KnowledgeRetrievalReason::Comparison),
        Ok(v1::KnowledgeRetrievalReason::Limitations) => Ok(KnowledgeRetrievalReason::Limitations),
        Ok(v1::KnowledgeRetrievalReason::References) => Ok(KnowledgeRetrievalReason::References),
        Ok(v1::KnowledgeRetrievalReason::Related) => Ok(KnowledgeRetrievalReason::Related),
        _ => Err(KnowledgeWireError::Unsupported("retrieval reason")),
    }
}

const fn fallback_to_wire(reason: Option<RouteFallbackReason>) -> v1::KnowledgeRouteFallbackReason {
    match reason {
        None => v1::KnowledgeRouteFallbackReason::Unspecified,
        Some(RouteFallbackReason::QueryEmbeddingsUnavailable) => {
            v1::KnowledgeRouteFallbackReason::QueryEmbeddingsUnavailable
        }
        Some(RouteFallbackReason::QueryEmbeddingFailed) => {
            v1::KnowledgeRouteFallbackReason::QueryEmbeddingFailed
        }
        Some(RouteFallbackReason::SemanticQueryFailed) => {
            v1::KnowledgeRouteFallbackReason::SemanticQueryFailed
        }
        Some(RouteFallbackReason::FullTextIndexUnavailable) => {
            v1::KnowledgeRouteFallbackReason::FullTextIndexUnavailable
        }
        Some(RouteFallbackReason::FullTextQueryFailed) => {
            v1::KnowledgeRouteFallbackReason::FullTextQueryFailed
        }
    }
}

fn fallback_from_wire(reason: i32) -> Result<Option<RouteFallbackReason>, KnowledgeWireError> {
    match v1::KnowledgeRouteFallbackReason::try_from(reason) {
        Ok(v1::KnowledgeRouteFallbackReason::Unspecified) => Ok(None),
        Ok(v1::KnowledgeRouteFallbackReason::QueryEmbeddingsUnavailable) => {
            Ok(Some(RouteFallbackReason::QueryEmbeddingsUnavailable))
        }
        Ok(v1::KnowledgeRouteFallbackReason::QueryEmbeddingFailed) => {
            Ok(Some(RouteFallbackReason::QueryEmbeddingFailed))
        }
        Ok(v1::KnowledgeRouteFallbackReason::SemanticQueryFailed) => {
            Ok(Some(RouteFallbackReason::SemanticQueryFailed))
        }
        Ok(v1::KnowledgeRouteFallbackReason::FullTextIndexUnavailable) => {
            Ok(Some(RouteFallbackReason::FullTextIndexUnavailable))
        }
        Ok(v1::KnowledgeRouteFallbackReason::FullTextQueryFailed) => {
            Ok(Some(RouteFallbackReason::FullTextQueryFailed))
        }
        Err(_) => Err(KnowledgeWireError::Unsupported("route fallback reason")),
    }
}

const fn coverage_to_wire(coverage: SearchCoverage) -> v1::KnowledgeCoverage {
    v1::KnowledgeCoverage {
        eligible: coverage.eligible,
        indexed: coverage.indexed,
        stale: coverage.stale,
        pending: coverage.pending,
        excluded: coverage.excluded,
        skipped: coverage.skipped,
        failed: coverage.failed,
        unavailable: coverage.unavailable,
    }
}

const fn coverage_from_wire(coverage: &v1::KnowledgeCoverage) -> SearchCoverage {
    SearchCoverage {
        eligible: coverage.eligible,
        indexed: coverage.indexed,
        stale: coverage.stale,
        pending: coverage.pending,
        excluded: coverage.excluded,
        skipped: coverage.skipped,
        failed: coverage.failed,
        unavailable: coverage.unavailable,
    }
}

fn policy_to_wire(policy: KnowledgeRetrievalPolicy) -> v1::KnowledgeRetrievalPolicy {
    v1::KnowledgeRetrievalPolicy {
        candidate_limit: u32::try_from(policy.candidate_limit).unwrap_or(u32::MAX),
        result_limit: u32::try_from(policy.result_limit).unwrap_or(u32::MAX),
        maximum_results_per_file: u32::try_from(policy.maximum_results_per_file)
            .unwrap_or(u32::MAX),
        context_token_budget: u32::try_from(policy.context_token_budget).unwrap_or(u32::MAX),
        adjacent_chunk_radius: policy.adjacent_chunk_radius,
        section_bounded_context: policy.section_bounded_context,
        rank_constant: policy.rank_constant,
        include_trace: policy.include_trace,
    }
}

fn policy_from_wire(policy: &v1::KnowledgeRetrievalPolicy) -> KnowledgeRetrievalPolicy {
    KnowledgeRetrievalPolicy {
        candidate_limit: policy.candidate_limit as usize,
        result_limit: policy.result_limit as usize,
        maximum_results_per_file: policy.maximum_results_per_file as usize,
        context_token_budget: policy.context_token_budget as usize,
        adjacent_chunk_radius: policy.adjacent_chunk_radius,
        section_bounded_context: policy.section_bounded_context,
        rank_constant: if policy.rank_constant == 0 {
            DEFAULT_RANK_CONSTANT
        } else {
            policy.rank_constant
        },
        include_trace: policy.include_trace,
    }
}

/// Encodes one host retrieval request for the local worker transport.
///
/// Every exactly-scoped slice travels in one request: partitioning is a scope
/// representation detail, never a reason to run several independent searches
/// whose separate result budgets could not be reconciled afterwards.
#[must_use]
pub fn request_to_wire(
    session: v1::SessionContext,
    request_id: String,
    request: &KnowledgeRetrievalRequest,
) -> v1::KnowledgeSearchRequest {
    let tenant = request.scopes.first().map(|scope| &scope.filters);
    v1::KnowledgeSearchRequest {
        session: Some(session),
        scope: Some(v1::ResourceScope {
            tenant_id: tenant
                .map(|filters| filters.tenant_id.clone())
                .unwrap_or_default(),
            library_id: tenant
                .and_then(|filters| filters.library_id.clone())
                .unwrap_or_default(),
        }),
        request_id,
        queries: request
            .queries
            .iter()
            .map(|query| v1::KnowledgeQuery {
                text: query.text.clone(),
                reason: reason_to_wire(query.reason).into(),
            })
            .collect(),
        route: route_to_wire(request.route).into(),
        policy: Some(policy_to_wire(request.policy)),
        coverage: Some(coverage_to_wire(request.coverage)),
        partitions: request
            .scopes
            .iter()
            .map(|scope| v1::KnowledgeScopePartition {
                filters: Some(v1::KnowledgeFilters {
                    root_id: scope.filters.root_id.clone(),
                    workspace_id: scope.filters.workspace_id.clone(),
                    include_unavailable: scope.filters.include_unavailable,
                }),
                allowed_source_ids: scope
                    .source_restriction
                    .allowed_source_ids
                    .iter()
                    .cloned()
                    .collect(),
            })
            .collect(),
    }
}

/// Decodes one worker-side retrieval request.
///
/// # Errors
///
/// Returns a typed mapping failure for absent scope or unsupported enums.
pub fn request_from_wire(
    request: &v1::KnowledgeSearchRequest,
) -> Result<KnowledgeRetrievalRequest, KnowledgeWireError> {
    let scope = request
        .scope
        .as_ref()
        .ok_or(KnowledgeWireError::Missing("scope"))?;
    let policy = request
        .policy
        .as_ref()
        .ok_or(KnowledgeWireError::Missing("policy"))?;
    if request.partitions.is_empty() {
        return Err(KnowledgeWireError::Missing("scope partitions"));
    }
    let scopes = request
        .partitions
        .iter()
        .map(|partition| {
            let filters = partition
                .filters
                .as_ref()
                .ok_or(KnowledgeWireError::Missing("filters"))?;
            Ok(KnowledgeRetrievalScope {
                filters: QueryFilters {
                    tenant_id: scope.tenant_id.clone(),
                    library_id: Some(scope.library_id.clone()),
                    root_id: filters.root_id.clone(),
                    workspace_id: filters.workspace_id.clone(),
                    include_unavailable: filters.include_unavailable,
                    ..QueryFilters::default()
                },
                source_restriction: KnowledgeSourceRestriction {
                    allowed_source_ids: partition.allowed_source_ids.iter().cloned().collect(),
                },
            })
        })
        .collect::<Result<Vec<_>, KnowledgeWireError>>()?;
    Ok(KnowledgeRetrievalRequest {
        queries: request
            .queries
            .iter()
            .map(|query| {
                Ok(KnowledgeQuery {
                    text: query.text.clone(),
                    reason: reason_from_wire(query.reason)?,
                })
            })
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
        route: route_from_wire(request.route)?,
        scopes,
        current_hashes: HashMap::new(),
        coverage: request
            .coverage
            .as_ref()
            .map_or_else(SearchCoverage::default, coverage_from_wire),
        policy: policy_from_wire(policy),
    })
}

fn provenance_to_wire(provenance: &ChunkProvenance) -> Result<String, KnowledgeWireError> {
    serde_json::to_string(provenance).map_err(|_| KnowledgeWireError::InvalidProvenance)
}

fn provenance_from_wire(provenance: &str) -> Result<ChunkProvenance, KnowledgeWireError> {
    serde_json::from_str(provenance).map_err(|_| KnowledgeWireError::InvalidProvenance)
}

const fn route_outcome_to_wire(outcome: RouteOutcome) -> v1::KnowledgeRouteOutcome {
    v1::KnowledgeRouteOutcome {
        requested: route_to_wire(outcome.requested) as i32,
        applied: route_to_wire(outcome.applied) as i32,
        fallback_reason: fallback_to_wire(outcome.fallback_reason) as i32,
    }
}

fn route_outcome_from_wire(
    outcome: &v1::KnowledgeRouteOutcome,
) -> Result<RouteOutcome, KnowledgeWireError> {
    Ok(RouteOutcome {
        requested: route_from_wire(outcome.requested)?,
        applied: route_from_wire(outcome.applied)?,
        fallback_reason: fallback_from_wire(outcome.fallback_reason)?,
    })
}

fn evidence_to_wire(
    evidence: &KnowledgeEvidence,
) -> Result<v1::KnowledgeEvidence, KnowledgeWireError> {
    Ok(v1::KnowledgeEvidence {
        record_id: evidence.record_id.clone(),
        occurrence_id: evidence.occurrence_id.clone(),
        source_id: evidence.source_id.clone(),
        duplicate_source_ids: evidence.duplicate_source_ids.clone(),
        document_id: evidence.document_id.clone(),
        library_id: evidence.library_id.clone(),
        chunk_kind: evidence.chunk_kind.clone(),
        excerpt: evidence.excerpt.clone(),
        content: evidence.content.clone(),
        token_count: u64::try_from(evidence.token_count).unwrap_or(u64::MAX),
        section_path: evidence.section_path.clone(),
        provenance: provenance_to_wire(&evidence.provenance)?,
        media_type: evidence.media_type.clone(),
        modified_at_ms: evidence.modified_at_ms,
        indexed_content_hash: evidence.indexed_content_hash.clone(),
        generation: evidence.generation,
        source_position: evidence.source_position,
        generated: evidence.generated,
        unavailable: evidence.unavailable,
        stale: evidence.stale,
        adjacent: evidence.adjacent,
        final_rank: u32::try_from(evidence.final_rank).unwrap_or(u32::MAX),
    })
}

fn evidence_from_wire(
    evidence: &v1::KnowledgeEvidence,
) -> Result<KnowledgeEvidence, KnowledgeWireError> {
    Ok(KnowledgeEvidence {
        record_id: evidence.record_id.clone(),
        occurrence_id: evidence.occurrence_id.clone(),
        source_id: evidence.source_id.clone(),
        duplicate_source_ids: evidence.duplicate_source_ids.clone(),
        document_id: evidence.document_id.clone(),
        library_id: evidence.library_id.clone(),
        chunk_kind: evidence.chunk_kind.clone(),
        excerpt: evidence.excerpt.clone(),
        content: evidence.content.clone(),
        token_count: usize::try_from(evidence.token_count).unwrap_or(usize::MAX),
        section_path: evidence.section_path.clone(),
        provenance: provenance_from_wire(&evidence.provenance)?,
        media_type: evidence.media_type.clone(),
        modified_at_ms: evidence.modified_at_ms,
        indexed_content_hash: evidence.indexed_content_hash.clone(),
        generation: evidence.generation,
        source_position: evidence.source_position,
        generated: evidence.generated,
        unavailable: evidence.unavailable,
        stale: evidence.stale,
        adjacent: evidence.adjacent,
        final_rank: evidence.final_rank as usize,
    })
}

fn contribution_to_wire(contribution: RankContribution) -> v1::KnowledgeRankContribution {
    v1::KnowledgeRankContribution {
        query_index: u32::try_from(contribution.query_index).unwrap_or(u32::MAX),
        route: route_to_wire(contribution.route).into(),
        rank: u32::try_from(contribution.rank).unwrap_or(u32::MAX),
    }
}

fn contribution_from_wire(
    contribution: &v1::KnowledgeRankContribution,
) -> Result<RankContribution, KnowledgeWireError> {
    Ok(RankContribution {
        query_index: contribution.query_index as usize,
        route: route_from_wire(contribution.route)?,
        rank: contribution.rank as usize,
    })
}

fn trace_to_wire(
    trace: &RetrievalTrace,
) -> Result<v1::KnowledgeRetrievalTrace, KnowledgeWireError> {
    Ok(v1::KnowledgeRetrievalTrace {
        route: Some(route_outcome_to_wire(trace.route)),
        capabilities: Some(v1::KnowledgeCapabilities {
            full_text: trace.capabilities.full_text,
            query_embeddings: trace.capabilities.query_embeddings,
        }),
        rank_constant: trace.rank_constant,
        queries: trace
            .queries
            .iter()
            .map(|query| v1::KnowledgeTracedQuery {
                text: query.text.clone(),
                reason: reason_to_wire(query.reason).into(),
                semantic_candidates: u32::try_from(query.semantic_candidates).unwrap_or(u32::MAX),
                full_text_candidates: u32::try_from(query.full_text_candidates).unwrap_or(u32::MAX),
            })
            .collect(),
        entries: trace
            .entries
            .iter()
            .map(|entry| {
                Ok(v1::KnowledgeTracedEvidence {
                    record_id: entry.record_id.clone(),
                    occurrence_id: entry.occurrence_id.clone(),
                    source_id: entry.source_id.clone(),
                    document_id: entry.document_id.clone(),
                    contributions: entry
                        .contributions
                        .iter()
                        .copied()
                        .map(contribution_to_wire)
                        .collect(),
                    fused_score: entry.fused_score,
                    final_rank: u32::try_from(entry.final_rank).unwrap_or(u32::MAX),
                    provenance: provenance_to_wire(&entry.provenance)?,
                    adjacent: entry.adjacent,
                    generated: entry.generated,
                    unavailable: entry.unavailable,
                    stale: entry.stale,
                })
            })
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
    })
}

fn trace_from_wire(
    trace: &v1::KnowledgeRetrievalTrace,
) -> Result<RetrievalTrace, KnowledgeWireError> {
    let capabilities = trace
        .capabilities
        .as_ref()
        .ok_or(KnowledgeWireError::Missing("trace capabilities"))?;
    Ok(RetrievalTrace {
        route: route_outcome_from_wire(
            trace
                .route
                .as_ref()
                .ok_or(KnowledgeWireError::Missing("trace route"))?,
        )?,
        capabilities: KnowledgeCapabilities {
            full_text: capabilities.full_text,
            query_embeddings: capabilities.query_embeddings,
        },
        rank_constant: trace.rank_constant,
        queries: trace
            .queries
            .iter()
            .map(|query| {
                Ok(TracedQuery {
                    text: query.text.clone(),
                    reason: reason_from_wire(query.reason)?,
                    semantic_candidates: query.semantic_candidates as usize,
                    full_text_candidates: query.full_text_candidates as usize,
                })
            })
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
        entries: trace
            .entries
            .iter()
            .map(|entry| {
                Ok(TracedEvidence {
                    record_id: entry.record_id.clone(),
                    occurrence_id: entry.occurrence_id.clone(),
                    source_id: entry.source_id.clone(),
                    document_id: entry.document_id.clone(),
                    contributions: entry
                        .contributions
                        .iter()
                        .map(contribution_from_wire)
                        .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
                    fused_score: entry.fused_score,
                    final_rank: entry.final_rank as usize,
                    provenance: provenance_from_wire(&entry.provenance)?,
                    adjacent: entry.adjacent,
                    generated: entry.generated,
                    unavailable: entry.unavailable,
                    stale: entry.stale,
                })
            })
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
    })
}

/// Encodes one worker retrieval result.
///
/// # Errors
///
/// Returns a typed mapping failure when provenance cannot be encoded.
pub fn response_to_wire(
    retrieval: &KnowledgeRetrieval,
) -> Result<v1::KnowledgeSearchResponse, KnowledgeWireError> {
    Ok(v1::KnowledgeSearchResponse {
        route: Some(route_outcome_to_wire(retrieval.route)),
        capabilities: Some(v1::KnowledgeCapabilities {
            full_text: retrieval.capabilities.full_text,
            query_embeddings: retrieval.capabilities.query_embeddings,
        }),
        evidence: retrieval
            .evidence
            .iter()
            .map(evidence_to_wire)
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
        token_count: u64::try_from(retrieval.token_count).unwrap_or(u64::MAX),
        coverage: Some(coverage_to_wire(retrieval.coverage)),
        trace: retrieval.trace.as_ref().map(trace_to_wire).transpose()?,
    })
}

/// Encodes one worker retrieval result inside a deterministic byte budget.
///
/// A retrieval is bounded by tokens, not by encoded bytes: a low-token result
/// set can still carry large excerpts, section paths, duplicate identities, and
/// trace entries. Measuring the exact encoded length before the frame is
/// written keeps an otherwise valid response from exceeding the negotiated
/// message limit and dropping the connection. The response is never silently
/// truncated: an oversized result is reported as a typed limit failure.
///
/// # Errors
///
/// Returns [`KnowledgeWireError::ResponseTooLarge`] when the encoded response
/// exceeds `budget_bytes`, or a mapping failure for invalid provenance.
pub fn bounded_response_to_wire(
    retrieval: &KnowledgeRetrieval,
    budget_bytes: usize,
) -> Result<v1::KnowledgeSearchResponse, KnowledgeWireError> {
    let response = response_to_wire(retrieval)?;
    let encoded = prost::Message::encoded_len(&response);
    if encoded > budget_bytes {
        return Err(KnowledgeWireError::ResponseTooLarge {
            encoded,
            maximum: budget_bytes,
        });
    }
    Ok(response)
}

/// Decodes one worker retrieval result on the host side.
///
/// # Errors
///
/// Returns a typed mapping failure for absent fields or unsupported enums.
pub fn response_from_wire(
    response: &v1::KnowledgeSearchResponse,
) -> Result<KnowledgeRetrieval, KnowledgeWireError> {
    let capabilities = response
        .capabilities
        .as_ref()
        .ok_or(KnowledgeWireError::Missing("capabilities"))?;
    Ok(KnowledgeRetrieval {
        route: route_outcome_from_wire(
            response
                .route
                .as_ref()
                .ok_or(KnowledgeWireError::Missing("route"))?,
        )?,
        capabilities: KnowledgeCapabilities {
            full_text: capabilities.full_text,
            query_embeddings: capabilities.query_embeddings,
        },
        evidence: response
            .evidence
            .iter()
            .map(evidence_from_wire)
            .collect::<Result<Vec<_>, KnowledgeWireError>>()?,
        token_count: usize::try_from(response.token_count).unwrap_or(usize::MAX),
        coverage: response
            .coverage
            .as_ref()
            .map_or_else(SearchCoverage::default, coverage_from_wire),
        trace: response.trace.as_ref().map(trace_from_wire).transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use fm_semantic_conversion::Provenance;

    use super::*;

    fn evidence() -> KnowledgeEvidence {
        KnowledgeEvidence {
            record_id: "record".to_owned(),
            occurrence_id: "occurrence".to_owned(),
            source_id: "source".to_owned(),
            duplicate_source_ids: vec!["duplicate".to_owned()],
            document_id: "document".to_owned(),
            library_id: "library".to_owned(),
            chunk_kind: "chunk".to_owned(),
            excerpt: "excerpt".to_owned(),
            content: "content".to_owned(),
            token_count: 12,
            section_path: vec!["Section".to_owned()],
            provenance: ChunkProvenance::Exact(Provenance::TextLines {
                start_line: 4,
                end_line: 6,
            }),
            media_type: Some("text/plain".to_owned()),
            modified_at_ms: Some(99),
            indexed_content_hash: "sha256:hash".to_owned(),
            generation: 3,
            source_position: 7,
            generated: false,
            unavailable: false,
            stale: true,
            adjacent: false,
            final_rank: 1,
        }
    }

    fn retrieval() -> KnowledgeRetrieval {
        KnowledgeRetrieval {
            route: RouteOutcome {
                requested: KnowledgeRoute::Hybrid,
                applied: KnowledgeRoute::FullText,
                fallback_reason: Some(RouteFallbackReason::QueryEmbeddingsUnavailable),
            },
            capabilities: KnowledgeCapabilities {
                full_text: true,
                query_embeddings: false,
            },
            evidence: vec![evidence()],
            token_count: 12,
            coverage: SearchCoverage {
                eligible: 4,
                indexed: 3,
                unavailable: 1,
                ..SearchCoverage::default()
            },
            trace: Some(RetrievalTrace {
                route: RouteOutcome {
                    requested: KnowledgeRoute::Hybrid,
                    applied: KnowledgeRoute::FullText,
                    fallback_reason: Some(RouteFallbackReason::QueryEmbeddingsUnavailable),
                },
                capabilities: KnowledgeCapabilities {
                    full_text: true,
                    query_embeddings: false,
                },
                rank_constant: DEFAULT_RANK_CONSTANT,
                queries: vec![TracedQuery {
                    text: "wind turbines".to_owned(),
                    reason: KnowledgeRetrievalReason::Subject,
                    semantic_candidates: 0,
                    full_text_candidates: 5,
                }],
                entries: vec![TracedEvidence {
                    record_id: "record".to_owned(),
                    occurrence_id: "occurrence".to_owned(),
                    source_id: "source".to_owned(),
                    document_id: "document".to_owned(),
                    contributions: vec![RankContribution {
                        query_index: 0,
                        route: KnowledgeRoute::FullText,
                        rank: 1,
                    }],
                    fused_score: 0.0164,
                    final_rank: 1,
                    provenance: ChunkProvenance::Exact(Provenance::TextLines {
                        start_line: 4,
                        end_line: 6,
                    }),
                    adjacent: false,
                    generated: false,
                    unavailable: false,
                    stale: true,
                }],
            }),
        }
    }

    fn scope(root_id: &str, sources: &[&str]) -> KnowledgeRetrievalScope {
        KnowledgeRetrievalScope {
            filters: QueryFilters {
                tenant_id: "tenant".to_owned(),
                library_id: Some("library".to_owned()),
                root_id: Some(root_id.to_owned()),
                include_unavailable: true,
                ..QueryFilters::default()
            },
            source_restriction: KnowledgeSourceRestriction {
                allowed_source_ids: sources.iter().map(|source| (*source).to_owned()).collect(),
            },
        }
    }

    /// Every exactly-scoped slice must survive the wire: dropping one would
    /// silently narrow the search, and merging them would silently widen it.
    #[test]
    fn requests_round_trip_every_scope_partition_through_the_wire_model() {
        let request = KnowledgeRetrievalRequest {
            queries: vec![KnowledgeQuery {
                text: "wind turbines".to_owned(),
                reason: KnowledgeRetrievalReason::Subject,
            }],
            route: KnowledgeRoute::Hybrid,
            scopes: vec![
                scope("root-a", &["source-a", "source-b"]),
                scope("root-b", &[]),
            ],
            current_hashes: HashMap::new(),
            coverage: SearchCoverage {
                eligible: 2,
                indexed: 2,
                ..SearchCoverage::default()
            },
            policy: KnowledgeRetrievalPolicy::default_search(),
        };

        let wire = request_to_wire(
            v1::SessionContext {
                session_id: "session".to_owned(),
                session_token: vec![1, 2, 3],
            },
            "request".to_owned(),
            &request,
        );
        assert_eq!(wire.partitions.len(), 2);
        let decoded = request_from_wire(&wire).expect("decode");

        assert_eq!(decoded.queries, request.queries);
        assert_eq!(decoded.route, request.route);
        assert_eq!(decoded.scopes.len(), 2);
        for (decoded, expected) in decoded.scopes.iter().zip(&request.scopes) {
            assert_eq!(decoded.filters.tenant_id, expected.filters.tenant_id);
            assert_eq!(decoded.filters.library_id, expected.filters.library_id);
            assert_eq!(decoded.filters.root_id, expected.filters.root_id);
            assert_eq!(decoded.filters.workspace_id, expected.filters.workspace_id);
            assert_eq!(
                decoded.filters.include_unavailable,
                expected.filters.include_unavailable
            );
            assert_eq!(
                decoded.source_restriction.allowed_source_ids,
                expected.source_restriction.allowed_source_ids
            );
        }
        assert_eq!(decoded.policy, request.policy);
        assert_eq!(decoded.coverage, request.coverage);
    }

    /// A request that names no scope is rejected rather than decoded into an
    /// unscoped tenant-wide retrieval.
    #[test]
    fn a_request_without_any_scope_partition_is_rejected() {
        let mut wire = request_to_wire(
            v1::SessionContext {
                session_id: "session".to_owned(),
                session_token: vec![1, 2, 3],
            },
            "request".to_owned(),
            &KnowledgeRetrievalRequest {
                queries: vec![KnowledgeQuery {
                    text: "wind turbines".to_owned(),
                    reason: KnowledgeRetrievalReason::Subject,
                }],
                route: KnowledgeRoute::FullText,
                scopes: vec![scope("root-a", &[])],
                current_hashes: HashMap::new(),
                coverage: SearchCoverage::default(),
                policy: KnowledgeRetrievalPolicy::default_search(),
            },
        );
        wire.partitions.clear();

        assert_eq!(
            request_from_wire(&wire).err(),
            Some(KnowledgeWireError::Missing("scope partitions"))
        );
    }

    #[test]
    fn results_round_trip_with_provenance_ranks_and_fallback_metadata() {
        let retrieval = retrieval();

        let wire = response_to_wire(&retrieval).expect("encode");
        let decoded = response_from_wire(&wire).expect("decode");

        assert_eq!(decoded.route, retrieval.route);
        assert_eq!(decoded.capabilities, retrieval.capabilities);
        assert_eq!(decoded.evidence, retrieval.evidence);
        assert_eq!(decoded.coverage, retrieval.coverage);
        assert_eq!(decoded.trace, retrieval.trace);
    }

    #[test]
    fn the_wire_evidence_model_carries_no_filesystem_path_field() {
        let wire = response_to_wire(&retrieval()).expect("encode");
        let encoded = format!("{wire:?}");
        let provenance = &wire.evidence[0].provenance;

        assert!(!encoded.contains("file://"));
        assert!(!encoded.to_lowercase().contains("\"path\""));
        assert!(!encoded.to_lowercase().contains("\"uri\""));
        assert!(provenance.contains("textLine"), "{provenance}");
    }
}
