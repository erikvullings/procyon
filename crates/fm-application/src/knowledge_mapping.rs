//! Transport mapping for Structured Knowledge Search.
//!
//! Retrieval fields and answer-only fields are mapped separately: nothing from
//! the answer half of a draft can reach a planned search.

use std::collections::HashMap;

use fm_semantic_worker::knowledge_retrieval::{
    KnowledgeEvidence, KnowledgeRoute, RankContribution, RetrievalTrace, RouteFallbackReason,
    RouteOutcome,
};
use fm_transport_dto::{
    KnowledgeActionDto, KnowledgeAnswerDepthDto, KnowledgeCapabilitiesDto, KnowledgeCoverageDto,
    KnowledgeDiagnosticCodeDto, KnowledgeDiagnosticDto, KnowledgeDiagnosticSeverityDto,
    KnowledgeEvidenceDto, KnowledgeExcludedFieldDto, KnowledgeNeedDto, KnowledgeOutputFormatDto,
    KnowledgeParseAmbiguityDto, KnowledgeParseConfidenceDto, KnowledgePlannedSearchDto,
    KnowledgeQueryDraftDto, KnowledgeQueryInterpretationDto, KnowledgeRankContributionDto,
    KnowledgeRetrievalModeDto, KnowledgeRouteDto, KnowledgeRouteFallbackReasonDto,
    KnowledgeRouteOutcomeDto, KnowledgeScopeDto, KnowledgeScopeSelectorDto,
    KnowledgeScopeSelectorKindDto, KnowledgeSearchOptionsDto, KnowledgeSearchPlanDto,
    KnowledgeSearchPriorityDto, KnowledgeSearchReasonDto, KnowledgeSearchReasonKindDto,
    KnowledgeSearchTraceDto, KnowledgeTracedQueryDto,
};

use crate::error::ApplicationError;
use crate::knowledge::{
    KnowledgeAction, KnowledgeAnswerDepth, KnowledgeAnswerRequest, KnowledgeCapabilities,
    KnowledgeNeed, KnowledgeOutputFormat, KnowledgeScopeSelector, KnowledgeSearchOptions,
    KnowledgeSearchPlan, KnowledgeSearchPriority, KnowledgeSearchReason, RetrievalMode,
};
use crate::knowledge_answer::{
    KnowledgeAnswer, KnowledgeAnswerError, KnowledgeAnswerEvidence, citation_label,
};
use crate::knowledge_dsl::{
    DiagnosticSeverity, KnowledgeDslDiagnostic, KnowledgeDslDiagnosticCode, KnowledgeDslParse,
    KnowledgeParseAmbiguity, KnowledgeParseConfidence, KnowledgeQueryDraft, format_compact,
    format_multiline,
};
use crate::knowledge_search::{
    KnowledgeEvidenceRanking, KnowledgeSearchCoverage, reasons_for_contributions,
};

pub(crate) const fn need_from_dto(need: KnowledgeNeedDto) -> KnowledgeNeed {
    match need {
        KnowledgeNeedDto::Overview => KnowledgeNeed::Overview,
        KnowledgeNeedDto::Definition => KnowledgeNeed::Definition,
        KnowledgeNeedDto::Procedure => KnowledgeNeed::Procedure,
        KnowledgeNeedDto::Examples => KnowledgeNeed::Examples,
        KnowledgeNeedDto::Evidence => KnowledgeNeed::Evidence,
        KnowledgeNeedDto::Arguments => KnowledgeNeed::Arguments,
        KnowledgeNeedDto::Comparison => KnowledgeNeed::Comparison,
        KnowledgeNeedDto::Limitations => KnowledgeNeed::Limitations,
        KnowledgeNeedDto::References => KnowledgeNeed::References,
    }
}

pub(crate) const fn need_to_dto(need: KnowledgeNeed) -> KnowledgeNeedDto {
    match need {
        KnowledgeNeed::Overview => KnowledgeNeedDto::Overview,
        KnowledgeNeed::Definition => KnowledgeNeedDto::Definition,
        KnowledgeNeed::Procedure => KnowledgeNeedDto::Procedure,
        KnowledgeNeed::Examples => KnowledgeNeedDto::Examples,
        KnowledgeNeed::Evidence => KnowledgeNeedDto::Evidence,
        KnowledgeNeed::Arguments => KnowledgeNeedDto::Arguments,
        KnowledgeNeed::Comparison => KnowledgeNeedDto::Comparison,
        KnowledgeNeed::Limitations => KnowledgeNeedDto::Limitations,
        KnowledgeNeed::References => KnowledgeNeedDto::References,
    }
}

pub(crate) const fn action_from_dto(action: KnowledgeActionDto) -> KnowledgeAction {
    match action {
        KnowledgeActionDto::Explain => KnowledgeAction::Explain,
        KnowledgeActionDto::Learn => KnowledgeAction::Learn,
        KnowledgeActionDto::Apply => KnowledgeAction::Apply,
        KnowledgeActionDto::Evaluate => KnowledgeAction::Evaluate,
        KnowledgeActionDto::Compare => KnowledgeAction::Compare,
        KnowledgeActionDto::Cite => KnowledgeAction::Cite,
    }
}

pub(crate) const fn action_to_dto(action: KnowledgeAction) -> KnowledgeActionDto {
    match action {
        KnowledgeAction::Explain => KnowledgeActionDto::Explain,
        KnowledgeAction::Learn => KnowledgeActionDto::Learn,
        KnowledgeAction::Apply => KnowledgeActionDto::Apply,
        KnowledgeAction::Evaluate => KnowledgeActionDto::Evaluate,
        KnowledgeAction::Compare => KnowledgeActionDto::Compare,
        KnowledgeAction::Cite => KnowledgeActionDto::Cite,
    }
}

const fn format_from_dto(format: KnowledgeOutputFormatDto) -> KnowledgeOutputFormat {
    match format {
        KnowledgeOutputFormatDto::Narrative => KnowledgeOutputFormat::Narrative,
        KnowledgeOutputFormatDto::Bullets => KnowledgeOutputFormat::Bullets,
        KnowledgeOutputFormatDto::Steps => KnowledgeOutputFormat::Steps,
        KnowledgeOutputFormatDto::Table => KnowledgeOutputFormat::Table,
    }
}

const fn format_to_dto(format: KnowledgeOutputFormat) -> KnowledgeOutputFormatDto {
    match format {
        KnowledgeOutputFormat::Narrative => KnowledgeOutputFormatDto::Narrative,
        KnowledgeOutputFormat::Bullets => KnowledgeOutputFormatDto::Bullets,
        KnowledgeOutputFormat::Steps => KnowledgeOutputFormatDto::Steps,
        KnowledgeOutputFormat::Table => KnowledgeOutputFormatDto::Table,
    }
}

const fn depth_from_dto(depth: KnowledgeAnswerDepthDto) -> KnowledgeAnswerDepth {
    match depth {
        KnowledgeAnswerDepthDto::Brief => KnowledgeAnswerDepth::Brief,
        KnowledgeAnswerDepthDto::Standard => KnowledgeAnswerDepth::Standard,
        KnowledgeAnswerDepthDto::Detailed => KnowledgeAnswerDepth::Detailed,
    }
}

const fn depth_to_dto(depth: KnowledgeAnswerDepth) -> KnowledgeAnswerDepthDto {
    match depth {
        KnowledgeAnswerDepth::Brief => KnowledgeAnswerDepthDto::Brief,
        KnowledgeAnswerDepth::Standard => KnowledgeAnswerDepthDto::Standard,
        KnowledgeAnswerDepth::Detailed => KnowledgeAnswerDepthDto::Detailed,
    }
}

pub(crate) const fn mode_from_dto(mode: KnowledgeRetrievalModeDto) -> RetrievalMode {
    match mode {
        KnowledgeRetrievalModeDto::Hybrid => RetrievalMode::Hybrid,
        KnowledgeRetrievalModeDto::FullText => RetrievalMode::FullText,
        KnowledgeRetrievalModeDto::Semantic => RetrievalMode::Semantic,
    }
}

const fn mode_to_dto(mode: RetrievalMode) -> KnowledgeRetrievalModeDto {
    match mode {
        RetrievalMode::Hybrid => KnowledgeRetrievalModeDto::Hybrid,
        RetrievalMode::FullText => KnowledgeRetrievalModeDto::FullText,
        RetrievalMode::Semantic => KnowledgeRetrievalModeDto::Semantic,
    }
}

const fn route_to_dto(route: KnowledgeRoute) -> KnowledgeRouteDto {
    match route {
        KnowledgeRoute::Hybrid => KnowledgeRouteDto::Hybrid,
        KnowledgeRoute::FullText => KnowledgeRouteDto::FullText,
        KnowledgeRoute::Semantic => KnowledgeRouteDto::Semantic,
    }
}

const fn fallback_to_dto(reason: RouteFallbackReason) -> KnowledgeRouteFallbackReasonDto {
    match reason {
        RouteFallbackReason::QueryEmbeddingsUnavailable => {
            KnowledgeRouteFallbackReasonDto::QueryEmbeddingsUnavailable
        }
        RouteFallbackReason::QueryEmbeddingFailed => {
            KnowledgeRouteFallbackReasonDto::QueryEmbeddingFailed
        }
        RouteFallbackReason::SemanticQueryFailed => {
            KnowledgeRouteFallbackReasonDto::SemanticQueryFailed
        }
        RouteFallbackReason::FullTextIndexUnavailable => {
            KnowledgeRouteFallbackReasonDto::FullTextIndexUnavailable
        }
        RouteFallbackReason::FullTextQueryFailed => {
            KnowledgeRouteFallbackReasonDto::FullTextQueryFailed
        }
    }
}

pub(crate) const fn route_outcome_to_dto(outcome: RouteOutcome) -> KnowledgeRouteOutcomeDto {
    KnowledgeRouteOutcomeDto {
        requested: route_to_dto(outcome.requested),
        applied: route_to_dto(outcome.applied),
        fallback_reason: match outcome.fallback_reason {
            Some(reason) => Some(fallback_to_dto(reason)),
            None => None,
        },
    }
}

pub(crate) const fn capabilities_to_dto(
    capabilities: KnowledgeCapabilities,
) -> KnowledgeCapabilitiesDto {
    KnowledgeCapabilitiesDto {
        full_text: capabilities.full_text,
        semantic: capabilities.semantic,
        answer_generation: capabilities.answer_generation,
    }
}

pub(crate) fn coverage_to_dto(coverage: KnowledgeSearchCoverage) -> KnowledgeCoverageDto {
    KnowledgeCoverageDto {
        eligible: coverage.eligible,
        indexed: coverage.indexed,
        fingerprinted: coverage.fingerprinted,
        unavailable: coverage.unavailable,
        stale_evidence: coverage.stale_evidence,
        unavailable_evidence: coverage.unavailable_evidence,
        unknown_freshness_evidence: coverage.unknown_freshness_evidence,
        scope_is_exact: coverage.scope_is_exact,
        partial: coverage.partial(),
    }
}

/// Maps one transport scope selector, failing closed on an incomplete one.
///
/// A `root` or `workspace` selector without an identity is a malformed request,
/// not a request for the whole library: silently widening it would search
/// everything the caller is enrolled in instead of the one scope they named.
pub(crate) fn selector_from_dto(
    selector: &KnowledgeScopeSelectorDto,
) -> Result<KnowledgeScopeSelector, ApplicationError> {
    let id = selector
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    match (selector.kind, id) {
        (KnowledgeScopeSelectorKindDto::WholeLibrary, _) => {
            Ok(KnowledgeScopeSelector::WholeLibrary)
        }
        (KnowledgeScopeSelectorKindDto::Root, Some(root_id)) => Ok(KnowledgeScopeSelector::Root {
            root_id: root_id.to_owned(),
        }),
        (KnowledgeScopeSelectorKindDto::Workspace, Some(workspace_id)) => {
            Ok(KnowledgeScopeSelector::Workspace {
                workspace_id: workspace_id.to_owned(),
            })
        }
        (KnowledgeScopeSelectorKindDto::Root, None) => Err(ApplicationError::InvalidRequest(
            "root knowledge scope selector requires a root id".into(),
        )),
        (KnowledgeScopeSelectorKindDto::Workspace, None) => Err(ApplicationError::InvalidRequest(
            "workspace knowledge scope selector requires a workspace id".into(),
        )),
    }
}

fn selector_to_dto(selector: &KnowledgeScopeSelector) -> KnowledgeScopeSelectorDto {
    match selector {
        KnowledgeScopeSelector::WholeLibrary => KnowledgeScopeSelectorDto {
            kind: KnowledgeScopeSelectorKindDto::WholeLibrary,
            id: None,
        },
        KnowledgeScopeSelector::Root { root_id } => KnowledgeScopeSelectorDto {
            kind: KnowledgeScopeSelectorKindDto::Root,
            id: Some(root_id.clone()),
        },
        KnowledgeScopeSelector::Workspace { workspace_id } => KnowledgeScopeSelectorDto {
            kind: KnowledgeScopeSelectorKindDto::Workspace,
            id: Some(workspace_id.clone()),
        },
    }
}

pub(crate) fn draft_from_dto(
    draft: &KnowledgeQueryDraftDto,
) -> Result<KnowledgeQueryDraft, ApplicationError> {
    Ok(KnowledgeQueryDraft {
        about: draft.about.clone(),
        needs: draft.needs.iter().copied().map(need_from_dto).collect(),
        related: draft.related.clone(),
        scopes: draft
            .scopes
            .iter()
            .map(selector_from_dto)
            .collect::<Result<Vec<_>, _>>()?,
        action: draft.action.map(action_from_dto),
        context: draft.context.clone(),
        constraints: draft.constraints.clone(),
        format: draft.format.map(format_from_dto),
        depth: draft.depth.map(depth_from_dto),
    })
}

pub(crate) fn draft_to_dto(draft: &KnowledgeQueryDraft) -> KnowledgeQueryDraftDto {
    KnowledgeQueryDraftDto {
        about: draft.about.clone(),
        needs: draft.needs.iter().copied().map(need_to_dto).collect(),
        related: draft.related.clone(),
        scopes: draft.scopes.iter().map(selector_to_dto).collect(),
        action: draft.action.map(action_to_dto),
        context: draft.context.clone(),
        constraints: draft.constraints.clone(),
        format: draft.format.map(format_to_dto),
        depth: draft.depth.map(depth_to_dto),
    }
}

/// Lists the answer-only fields a draft carries, which retrieval never reads.
pub(crate) fn excluded_fields(draft: &KnowledgeQueryDraft) -> Vec<KnowledgeExcludedFieldDto> {
    let mut fields = Vec::new();
    if let Some(action) = draft.action {
        fields.push(KnowledgeExcludedFieldDto {
            field: "do".to_owned(),
            value: format!("{action:?}").to_lowercase(),
        });
    }
    if let Some(context) = &draft.context {
        fields.push(KnowledgeExcludedFieldDto {
            field: "to".to_owned(),
            value: context.clone(),
        });
    }
    for constraint in &draft.constraints {
        fields.push(KnowledgeExcludedFieldDto {
            field: "constraint".to_owned(),
            value: constraint.clone(),
        });
    }
    if let Some(format) = draft.format {
        fields.push(KnowledgeExcludedFieldDto {
            field: "format".to_owned(),
            value: format!("{format:?}").to_lowercase(),
        });
    }
    if let Some(depth) = draft.depth {
        fields.push(KnowledgeExcludedFieldDto {
            field: "depth".to_owned(),
            value: format!("{depth:?}").to_lowercase(),
        });
    }
    fields
}

const fn severity_to_dto(severity: DiagnosticSeverity) -> KnowledgeDiagnosticSeverityDto {
    match severity {
        DiagnosticSeverity::Error => KnowledgeDiagnosticSeverityDto::Error,
        DiagnosticSeverity::Warning => KnowledgeDiagnosticSeverityDto::Warning,
    }
}

const fn diagnostic_code_to_dto(code: KnowledgeDslDiagnosticCode) -> KnowledgeDiagnosticCodeDto {
    match code {
        KnowledgeDslDiagnosticCode::UnknownField => KnowledgeDiagnosticCodeDto::UnknownField,
        KnowledgeDslDiagnosticCode::DuplicateField => KnowledgeDiagnosticCodeDto::DuplicateField,
        KnowledgeDslDiagnosticCode::EmptyValue => KnowledgeDiagnosticCodeDto::EmptyValue,
        KnowledgeDslDiagnosticCode::InvalidNeedValue => {
            KnowledgeDiagnosticCodeDto::InvalidNeedValue
        }
        KnowledgeDslDiagnosticCode::InvalidScopeValue => {
            KnowledgeDiagnosticCodeDto::InvalidScopeValue
        }
        KnowledgeDslDiagnosticCode::InvalidActionValue => {
            KnowledgeDiagnosticCodeDto::InvalidActionValue
        }
        KnowledgeDslDiagnosticCode::InvalidFormatValue => {
            KnowledgeDiagnosticCodeDto::InvalidFormatValue
        }
        KnowledgeDslDiagnosticCode::TooManyValues => KnowledgeDiagnosticCodeDto::TooManyValues,
        KnowledgeDslDiagnosticCode::ValueTooLong => KnowledgeDiagnosticCodeDto::ValueTooLong,
        KnowledgeDslDiagnosticCode::UnterminatedQuote => {
            KnowledgeDiagnosticCodeDto::UnterminatedQuote
        }
        KnowledgeDslDiagnosticCode::InvalidDepthValue => {
            KnowledgeDiagnosticCodeDto::InvalidDepthValue
        }
    }
}

fn diagnostic_to_dto(diagnostic: &KnowledgeDslDiagnostic) -> KnowledgeDiagnosticDto {
    KnowledgeDiagnosticDto {
        severity: severity_to_dto(diagnostic.severity),
        code: diagnostic_code_to_dto(diagnostic.code),
        message: diagnostic.message.clone(),
        suggestion: diagnostic.suggestion.clone(),
        start: u32::try_from(diagnostic.span.start).unwrap_or(u32::MAX),
        end: u32::try_from(diagnostic.span.end).unwrap_or(u32::MAX),
    }
}

const fn confidence_to_dto(confidence: KnowledgeParseConfidence) -> KnowledgeParseConfidenceDto {
    match confidence {
        KnowledgeParseConfidence::Explicit => KnowledgeParseConfidenceDto::Explicit,
        KnowledgeParseConfidence::Deterministic => KnowledgeParseConfidenceDto::Deterministic,
        KnowledgeParseConfidence::Ambiguous => KnowledgeParseConfidenceDto::Ambiguous,
    }
}

fn ambiguity_to_dto(ambiguity: &KnowledgeParseAmbiguity) -> KnowledgeParseAmbiguityDto {
    KnowledgeParseAmbiguityDto {
        description: ambiguity.description.clone(),
        alternative_need: ambiguity.alternative_need.map(need_to_dto),
        alternative_action: ambiguity.alternative_action.map(action_to_dto),
    }
}

pub(crate) fn interpretation_to_dto(parse: &KnowledgeDslParse) -> KnowledgeQueryInterpretationDto {
    KnowledgeQueryInterpretationDto {
        draft: draft_to_dto(&parse.draft),
        confidence: confidence_to_dto(parse.confidence),
        ambiguities: parse.ambiguities.iter().map(ambiguity_to_dto).collect(),
        diagnostics: parse.diagnostics.iter().map(diagnostic_to_dto).collect(),
        dsl_multiline: format_multiline(&parse.draft),
        dsl_compact: format_compact(&parse.draft),
        excluded_from_retrieval: excluded_fields(&parse.draft),
    }
}

pub(crate) fn options_from_dto(options: KnowledgeSearchOptionsDto) -> KnowledgeSearchOptions {
    KnowledgeSearchOptions {
        maximum_searches: options.maximum_searches as usize,
        candidate_limit: options.candidate_limit as usize,
        result_limit: options.result_limit as usize,
        maximum_results_per_file: options.maximum_results_per_file as usize,
        context_token_budget: options.context_token_budget as usize,
        adjacent_chunk_radius: options.adjacent_chunk_radius,
        section_bounded_context: options.section_bounded_context,
        include_trace: options.include_trace,
    }
}

pub(crate) fn options_to_dto(options: KnowledgeSearchOptions) -> KnowledgeSearchOptionsDto {
    KnowledgeSearchOptionsDto {
        maximum_searches: u32::try_from(options.maximum_searches).unwrap_or(u32::MAX),
        candidate_limit: u32::try_from(options.candidate_limit).unwrap_or(u32::MAX),
        result_limit: u32::try_from(options.result_limit).unwrap_or(u32::MAX),
        maximum_results_per_file: u32::try_from(options.maximum_results_per_file)
            .unwrap_or(u32::MAX),
        context_token_budget: u32::try_from(options.context_token_budget).unwrap_or(u32::MAX),
        adjacent_chunk_radius: options.adjacent_chunk_radius,
        section_bounded_context: options.section_bounded_context,
        include_trace: options.include_trace,
    }
}

const fn priority_to_dto(priority: KnowledgeSearchPriority) -> KnowledgeSearchPriorityDto {
    match priority {
        KnowledgeSearchPriority::Primary => KnowledgeSearchPriorityDto::Primary,
        KnowledgeSearchPriority::Secondary => KnowledgeSearchPriorityDto::Secondary,
        KnowledgeSearchPriority::Related => KnowledgeSearchPriorityDto::Related,
    }
}

fn reason_to_dto(reason: KnowledgeSearchReason) -> KnowledgeSearchReasonDto {
    match reason {
        KnowledgeSearchReason::Subject { subject_index } => KnowledgeSearchReasonDto {
            kind: KnowledgeSearchReasonKindDto::Subject,
            subject_index: Some(u32::try_from(subject_index).unwrap_or(u32::MAX)),
            need: None,
            action: None,
            related_term_index: None,
        },
        KnowledgeSearchReason::Need {
            subject_index,
            need,
        } => KnowledgeSearchReasonDto {
            kind: KnowledgeSearchReasonKindDto::Need,
            subject_index: Some(u32::try_from(subject_index).unwrap_or(u32::MAX)),
            need: Some(need_to_dto(need)),
            action: None,
            related_term_index: None,
        },
        KnowledgeSearchReason::ActionDefault {
            action,
            subject_index,
            need,
        } => KnowledgeSearchReasonDto {
            kind: KnowledgeSearchReasonKindDto::ActionDefault,
            subject_index: Some(u32::try_from(subject_index).unwrap_or(u32::MAX)),
            need: Some(need_to_dto(need)),
            action: Some(action_to_dto(action)),
            related_term_index: None,
        },
        KnowledgeSearchReason::RelatedTerm { related_term_index } => KnowledgeSearchReasonDto {
            kind: KnowledgeSearchReasonKindDto::RelatedTerm,
            subject_index: None,
            need: None,
            action: None,
            related_term_index: Some(u32::try_from(related_term_index).unwrap_or(u32::MAX)),
        },
    }
}

pub(crate) fn plan_to_dto(
    plan: &KnowledgeSearchPlan,
    scope: KnowledgeScopeDto,
    scope_label: String,
    authorized_sources: u64,
    scope_is_exact: bool,
    excluded_from_retrieval: Vec<KnowledgeExcludedFieldDto>,
) -> KnowledgeSearchPlanDto {
    KnowledgeSearchPlanDto {
        version: plan.version.clone(),
        subjects: plan
            .subjects
            .iter()
            .map(|subject| subject.text.clone())
            .collect(),
        mode: mode_to_dto(plan.mode),
        options: options_to_dto(plan.options),
        searches: plan
            .searches
            .iter()
            .map(|search| KnowledgePlannedSearchDto {
                text: search.text.clone(),
                priority: priority_to_dto(search.priority),
                reasons: search.reasons.iter().copied().map(reason_to_dto).collect(),
            })
            .collect(),
        omitted_searches: u32::try_from(plan.omitted_searches).unwrap_or(u32::MAX),
        scope,
        scope_label,
        authorized_sources,
        scope_is_exact,
        excluded_from_retrieval,
    }
}

fn contribution_to_dto(contribution: RankContribution) -> KnowledgeRankContributionDto {
    KnowledgeRankContributionDto {
        search_index: u32::try_from(contribution.query_index).unwrap_or(u32::MAX),
        route: route_to_dto(contribution.route),
        rank: u32::try_from(contribution.rank).unwrap_or(u32::MAX),
    }
}

pub(crate) fn evidence_to_dto(
    evidence: KnowledgeEvidence,
    plan: &KnowledgeSearchPlan,
    rankings: &HashMap<String, KnowledgeEvidenceRanking>,
    freshness: &HashMap<String, Option<bool>>,
    titles: &HashMap<String, String>,
) -> KnowledgeEvidenceDto {
    let ranking = rankings
        .get(&evidence.record_id)
        .cloned()
        .unwrap_or_default();
    let stale = freshness.get(&evidence.record_id).copied().flatten();
    let mut matched_search_indexes = ranking
        .contributions
        .iter()
        .map(|contribution| u32::try_from(contribution.query_index).unwrap_or(u32::MAX))
        .collect::<Vec<_>>();
    matched_search_indexes.sort_unstable();
    matched_search_indexes.dedup();
    KnowledgeEvidenceDto {
        title: titles
            .get(&evidence.source_id)
            .cloned()
            .unwrap_or_else(|| "Indexed document".to_owned()),
        record_id: evidence.record_id,
        source_id: evidence.source_id,
        duplicate_source_ids: evidence.duplicate_source_ids,
        document_id: evidence.document_id,
        chunk_kind: evidence.chunk_kind,
        excerpt: evidence.excerpt,
        content: evidence.content,
        token_count: u64::try_from(evidence.token_count).unwrap_or(u64::MAX),
        section_path: evidence.section_path,
        provenance: serde_json::to_string(&evidence.provenance).unwrap_or_default(),
        media_type: evidence.media_type,
        modified_at_ms: evidence.modified_at_ms,
        source_position: evidence.source_position,
        generated: evidence.generated,
        unavailable: evidence.unavailable,
        stale,
        adjacent: evidence.adjacent,
        final_rank: u32::try_from(evidence.final_rank).unwrap_or(u32::MAX),
        fused_score: ranking.fused_score,
        matched_search_indexes,
        reasons: reasons_for_contributions(plan, &ranking.contributions)
            .into_iter()
            .map(reason_to_dto)
            .collect(),
        rank_contributions: ranking
            .contributions
            .into_iter()
            .map(contribution_to_dto)
            .collect(),
    }
}

pub(crate) fn trace_to_dto(trace: &RetrievalTrace) -> KnowledgeSearchTraceDto {
    KnowledgeSearchTraceDto {
        rank_constant: trace.rank_constant,
        queries: trace
            .queries
            .iter()
            .map(|query| KnowledgeTracedQueryDto {
                text: query.text.clone(),
                semantic_candidates: u32::try_from(query.semantic_candidates).unwrap_or(u32::MAX),
                full_text_candidates: u32::try_from(query.full_text_candidates).unwrap_or(u32::MAX),
            })
            .collect(),
    }
}

/// Builds the canonical answer-only request from its transport form.
///
/// Bounds are enforced by [`crate::knowledge::KnowledgeAnswerRequest::validate`]
/// downstream; this mapping never copies an answer field into retrieval.
pub(crate) fn answer_request_from_dto(
    request: &fm_transport_dto::GenerateKnowledgeAnswerRequestDto,
) -> KnowledgeAnswerRequest {
    KnowledgeAnswerRequest {
        evidence_fingerprint: request.evidence_fingerprint.clone(),
        action: request.action.map(action_from_dto),
        context: request.context.clone(),
        constraints: request.constraints.clone(),
        depth: request.depth.map(depth_from_dto),
        output: request.output.map(format_from_dto),
    }
}

/// Projects one displayed evidence row into the answer capability's input.
///
/// The label is derived from the displayed position rather than from anything
/// the model or the worker produced, so citations stay stable and always name
/// evidence the user already inspected.
pub(crate) fn answer_evidence_from_dto(
    index: usize,
    evidence: &KnowledgeEvidenceDto,
    indexed_content_hash: String,
) -> KnowledgeAnswerEvidence {
    KnowledgeAnswerEvidence {
        label: citation_label(index),
        record_id: evidence.record_id.clone(),
        source_id: evidence.source_id.clone(),
        title: evidence.title.clone(),
        content: evidence.content.clone(),
        section_path: evidence.section_path.clone(),
        provenance: evidence.provenance.clone(),
        indexed_content_hash,
        generated: evidence.generated,
        final_rank: evidence.final_rank,
        adjacent: evidence.adjacent,
        stale: evidence.stale,
        unavailable: evidence.unavailable,
    }
}

pub(crate) fn answer_to_dto(
    request_id: uuid::Uuid,
    answer: KnowledgeAnswer,
) -> fm_transport_dto::KnowledgeAnswerDto {
    fm_transport_dto::KnowledgeAnswerDto {
        request_id,
        evidence_fingerprint: answer.evidence_fingerprint,
        profile_id: answer.profile_id,
        profile_name: answer.profile_name,
        locality: crate::llm_profile_mapping::locality_to_dto(answer.locality),
        text: answer.text,
        citations: answer
            .citations
            .into_iter()
            .map(|citation| fm_transport_dto::KnowledgeAnswerCitationDto {
                label: citation.label,
                record_id: citation.record_id,
                source_id: citation.source_id,
                provenance: citation.provenance,
                section_path: citation.section_path,
                final_rank: citation.final_rank,
                unavailable: citation.unavailable,
                stale: citation.stale,
                generated: citation.generated,
            })
            .collect(),
        model_knowledge_allowed: answer.model_knowledge_allowed,
        insufficient: answer.insufficient,
        withheld_unauthorized: answer.withheld_unauthorized,
        stale_evidence: answer.stale_evidence,
        unavailable_evidence: answer.unavailable_evidence,
    }
}

/// Maps a sanitized answer failure onto the transport error contract.
///
/// A missing, evicted, or mismatched evidence set is reported as its own
/// machine-matchable code so a host can offer an explicit refresh instead of
/// guessing from a generic invalid-request failure.
pub(crate) fn answer_error_to_application(error: KnowledgeAnswerError) -> ApplicationError {
    match error {
        KnowledgeAnswerError::InvalidRequest(message) => ApplicationError::InvalidRequest(message),
        KnowledgeAnswerError::RefreshRequired {
            evidence_fingerprint,
        } => ApplicationError::KnowledgeEvidenceRefreshRequired {
            evidence_fingerprint,
        },
        KnowledgeAnswerError::AuthorizationUnavailable
        | KnowledgeAnswerError::ProfileUnavailable => ApplicationError::ProviderUnavailable,
        KnowledgeAnswerError::EvidenceRevoked | KnowledgeAnswerError::ConsentRequired => {
            ApplicationError::PermissionDenied
        }
        KnowledgeAnswerError::DuplicateRequest => {
            ApplicationError::InvalidRequest(error.to_string())
        }
        KnowledgeAnswerError::Cancelled => ApplicationError::OperationCancelled,
        KnowledgeAnswerError::GenerationFailed => ApplicationError::Internal,
    }
}
