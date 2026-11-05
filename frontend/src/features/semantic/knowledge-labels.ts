import { t } from '../../i18n';
import type {
  KnowledgeNeed,
  KnowledgeQueryIntent,
  KnowledgeRetrievalMode,
  KnowledgeRouteFallbackReason,
  KnowledgeScopeKind,
  KnowledgeSearchPlan,
  KnowledgeSearchReason,
} from '../../models';

export function needLabel(need: KnowledgeNeed): string {
  switch (need) {
    case 'overview':
      return t('knowledgeSearch', 'needOverview');
    case 'definition':
      return t('knowledgeSearch', 'needDefinition');
    case 'procedure':
      return t('knowledgeSearch', 'needProcedure');
    case 'examples':
      return t('knowledgeSearch', 'needExamples');
    case 'evidence':
      return t('knowledgeSearch', 'needEvidence');
    case 'arguments':
      return t('knowledgeSearch', 'needArguments');
    case 'comparison':
      return t('knowledgeSearch', 'needComparison');
    case 'limitations':
      return t('knowledgeSearch', 'needLimitations');
    case 'references':
      return t('knowledgeSearch', 'needReferences');
  }
}

export function intentLabel(intent: KnowledgeQueryIntent): string {
  switch (intent) {
    case 'search':
      return t('knowledgeSearch', 'intentDocuments');
    case 'questionAnswering':
      return t('knowledgeSearch', 'intentQuestions');
    case 'codeRetrieval':
      return t('knowledgeSearch', 'intentCode');
  }
}

export function modeLabel(mode: KnowledgeRetrievalMode): string {
  switch (mode) {
    case 'hybrid':
      return t('knowledgeSearch', 'modeHybrid');
    case 'fullText':
      return t('knowledgeSearch', 'modeFullText');
    case 'semantic':
      return t('knowledgeSearch', 'modeSemantic');
  }
}

export function scopeKindLabel(kind: KnowledgeScopeKind): string {
  switch (kind) {
    case 'entireLibrary':
      return t('knowledgeSearch', 'scopeEntireLibrary');
    case 'enrolledRoots':
      return t('knowledgeSearch', 'scopeEnrolledRoots');
    case 'currentFolder':
      return t('knowledgeSearch', 'scopeCurrentFolder');
    case 'semanticResults':
      return t('knowledgeSearch', 'scopeSemanticResults');
  }
}

export function fallbackReasonLabel(reason: KnowledgeRouteFallbackReason): string {
  switch (reason) {
    case 'queryEmbeddingsUnavailable':
      return t('knowledgeSearch', 'fallbackQueryEmbeddingsUnavailable');
    case 'queryEmbeddingFailed':
      return t('knowledgeSearch', 'fallbackQueryEmbeddingFailed');
    case 'semanticQueryFailed':
      return t('knowledgeSearch', 'fallbackSemanticQueryFailed');
    case 'fullTextIndexUnavailable':
      return t('knowledgeSearch', 'fallbackFullTextIndexUnavailable');
    case 'fullTextQueryFailed':
      return t('knowledgeSearch', 'fallbackFullTextQueryFailed');
  }
}

export function reasonLabel(reason: KnowledgeSearchReason): string {
  switch (reason.kind) {
    case 'subject':
      return t('knowledgeSearch', 'reasonSubject');
    case 'need':
      return t('knowledgeSearch', 'reasonNeed', {
        need: reason.need == null ? '' : needLabel(reason.need),
      });
    case 'actionDefault':
      return t('knowledgeSearch', 'reasonActionDefault', {
        action: reason.action ?? '',
        need: reason.need == null ? '' : needLabel(reason.need),
      });
    case 'relatedTerm':
      return t('knowledgeSearch', 'reasonRelatedTerm');
  }
}

export function priorityLabel(
  priority: KnowledgeSearchPlan['searches'][number]['priority'],
): string {
  switch (priority) {
    case 'primary':
      return t('knowledgeSearch', 'planSearchPrimary');
    case 'secondary':
      return t('knowledgeSearch', 'planSearchSecondary');
    case 'related':
      return t('knowledgeSearch', 'planSearchRelated');
  }
}

/**
 * Whether a host refused to answer because the inspected evidence set is gone.
 *
 * Every host reports this the same way - an `ApiError`, a Tauri
 * `ApplicationErrorDto`, or the mock's `MockClientError` all carry the typed
 * `code` - so the dialog can tell the user to search again instead of guessing
 * from a message, and never re-searches on their behalf.
 */
export function isKnowledgeEvidenceRefreshRequired(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  return (error as { readonly code?: unknown }).code === 'knowledgeEvidenceRefreshRequired';
}

/** Maps transport-stable error codes to actionable, localized search guidance. */
export function knowledgeSearchErrorMessage(error: unknown): string {
  const code =
    typeof error === 'object' && error !== null
      ? (error as { readonly code?: unknown }).code
      : undefined;
  switch (code) {
    case 'notFound':
      return t('knowledgeSearch', 'searchNoSources');
    case 'providerUnavailable':
      return t('knowledgeSearch', 'searchUnavailable');
    case 'permissionDenied':
      return t('knowledgeSearch', 'searchDenied');
    case 'invalidRequest':
      return t('knowledgeSearch', 'searchInvalid');
    case 'operationCancelled':
      return t('knowledgeSearch', 'cancelled');
    default:
      return t('knowledgeSearch', 'searchFailed');
  }
}
