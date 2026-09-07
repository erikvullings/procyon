import type { StartSearchResult } from '../../models';

const STORAGE_KEY = 'procyon.semanticEvaluation.v1';

export interface SemanticEvaluationCase {
  readonly query: string;
  readonly entryId: string;
  readonly relevant: boolean;
  readonly relevantFileIds: readonly string[];
  readonly relevantChunkIds: readonly string[];
  readonly recordedAt: string;
  readonly evidenceHash: string;
}

function readCases(): SemanticEvaluationCase[] {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === null) return [];
  const value: unknown = JSON.parse(stored);
  if (!Array.isArray(value) || !value.every(isEvaluationCase)) {
    throw new Error('Invalid local semantic evaluation data');
  }
  return value.map((item) => ({
    ...item,
    relevantFileIds: item.relevantFileIds ?? (item.relevant ? [item.entryId] : []),
    relevantChunkIds: item.relevantChunkIds ?? [],
  }));
}

type StoredEvaluationCase = Omit<SemanticEvaluationCase, 'relevantFileIds' | 'relevantChunkIds'> & {
  readonly relevantFileIds?: readonly string[];
  readonly relevantChunkIds?: readonly string[];
};

function isEvaluationCase(value: unknown): value is StoredEvaluationCase {
  if (typeof value !== 'object' || value === null) return false;
  const record = Object.fromEntries(Object.entries(value));
  return (
    typeof record.query === 'string' &&
    typeof record.entryId === 'string' &&
    typeof record.relevant === 'boolean' &&
    typeof record.recordedAt === 'string' &&
    typeof record.evidenceHash === 'string' &&
    (record.relevantFileIds === undefined ||
      (Array.isArray(record.relevantFileIds) &&
        record.relevantFileIds.every((id) => typeof id === 'string'))) &&
    (record.relevantChunkIds === undefined ||
      (Array.isArray(record.relevantChunkIds) &&
        record.relevantChunkIds.every((id) => typeof id === 'string')))
  );
}

/** Records an explicit local-only relevance judgement. */
export function recordSemanticFeedback(
  query: string,
  result: NonNullable<StartSearchResult['semanticResults']>[number],
  relevant: boolean,
): void {
  const cases = readCases().filter(
    (item) => !(item.query === query && item.entryId === result.entryId),
  );
  cases.push({
    query,
    entryId: result.entryId,
    relevant,
    relevantFileIds: relevant ? [result.entryId] : [],
    relevantChunkIds: relevant ? [result.bestEvidence.recordId] : [],
    recordedAt: new Date().toISOString(),
    evidenceHash: result.bestEvidence.indexedContentHash,
  });
  localStorage.setItem(STORAGE_KEY, JSON.stringify(cases));
}

/** Exports local evaluation cases only after an explicit user action. */
export function exportSemanticEvaluationCases(): void {
  const contents = JSON.stringify(readCases(), null, 2);
  const url = URL.createObjectURL(new Blob([contents], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = 'procyon-semantic-evaluation.json';
  link.click();
  URL.revokeObjectURL(url);
}
