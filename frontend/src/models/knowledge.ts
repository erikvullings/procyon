export type {
  CancelKnowledgeSearchRequestDto as CancelKnowledgeSearchRequest,
  ExecuteKnowledgeSearchRequestDto as ExecuteKnowledgeSearchRequest,
  KnowledgeActionDto as KnowledgeAction,
  KnowledgeAnswerDepthDto as KnowledgeAnswerDepth,
  KnowledgeCapabilitiesDto as KnowledgeCapabilities,
  KnowledgeCoverageDto as KnowledgeCoverage,
  KnowledgeDiagnosticCodeDto as KnowledgeDiagnosticCode,
  KnowledgeDiagnosticDto as KnowledgeDiagnostic,
  KnowledgeDiagnosticSeverityDto as KnowledgeDiagnosticSeverity,
  KnowledgeEvidenceDto as KnowledgeEvidence,
  KnowledgeExcludedFieldDto as KnowledgeExcludedField,
  KnowledgeNeedDto as KnowledgeNeed,
  KnowledgeOutputFormatDto as KnowledgeOutputFormat,
  KnowledgeParseAmbiguityDto as KnowledgeParseAmbiguity,
  KnowledgeParseConfidenceDto as KnowledgeParseConfidence,
  KnowledgePlannedSearchDto as KnowledgePlannedSearch,
  KnowledgeQueryDraftDto as KnowledgeQueryDraft,
  KnowledgeQueryInterpretationDto as KnowledgeQueryInterpretation,
  KnowledgeRankContributionDto as KnowledgeRankContribution,
  KnowledgeRetrievalModeDto as KnowledgeRetrievalMode,
  KnowledgeRootDto as KnowledgeRoot,
  KnowledgeRouteDto as KnowledgeRoute,
  KnowledgeRouteFallbackReasonDto as KnowledgeRouteFallbackReason,
  KnowledgeRouteOutcomeDto as KnowledgeRouteOutcome,
  KnowledgeScopeDto as KnowledgeScope,
  KnowledgeScopeKindDto as KnowledgeScopeKind,
  KnowledgeScopeSelectorDto as KnowledgeScopeSelector,
  KnowledgeScopeSelectorKindDto as KnowledgeScopeSelectorKind,
  KnowledgeSearchOptionsDto as KnowledgeSearchOptions,
  KnowledgeSearchPlanDto as KnowledgeSearchPlan,
  KnowledgeSearchPriorityDto as KnowledgeSearchPriority,
  KnowledgeSearchReasonDto as KnowledgeSearchReason,
  KnowledgeSearchReasonKindDto as KnowledgeSearchReasonKind,
  KnowledgeSearchResultDto as KnowledgeSearchResult,
  KnowledgeSearchTraceDto as KnowledgeSearchTrace,
  KnowledgeSourceLocationDto as KnowledgeSourceLocation,
  KnowledgeTracedQueryDto as KnowledgeTracedQuery,
  ListKnowledgeRootsRequestDto as ListKnowledgeRootsRequest,
  ParseKnowledgeQueryRequestDto as ParseKnowledgeQueryRequest,
  PlanKnowledgeSearchRequestDto as PlanKnowledgeSearchRequest,
  ResolveKnowledgeSourceRequestDto as ResolveKnowledgeSourceRequest,
} from '../api/generated/models';

import type { KnowledgeSearchOptionsDto } from '../api/generated/models';

/**
 * Bounded retrieval defaults, mirroring `KnowledgeSearchOptions::default()` in
 * `fm-application`. Callers send these explicitly so a plan preview and the
 * search it previews always agree (task 0206).
 */
export function defaultKnowledgeSearchOptions(): KnowledgeSearchOptionsDto {
  return {
    maximumSearches: 8,
    candidateLimit: 64,
    resultLimit: 20,
    maximumResultsPerFile: 3,
    contextTokenBudget: 8_192,
    adjacentChunkRadius: 1,
    sectionBoundedContext: true,
    includeTrace: false,
  };
}
