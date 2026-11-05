import type {
  KnowledgeNeed,
  KnowledgeQueryIntent,
  KnowledgeRetrievalMode,
  KnowledgeScopeKind,
} from '../../models';

export const NEEDS: readonly KnowledgeNeed[] = [
  'overview',
  'definition',
  'procedure',
  'examples',
  'evidence',
  'arguments',
  'comparison',
  'limitations',
  'references',
];

export const MODES: readonly KnowledgeRetrievalMode[] = ['hybrid', 'fullText', 'semantic'];
export const INTENTS: readonly KnowledgeQueryIntent[] = [
  'search',
  'questionAnswering',
  'codeRetrieval',
];

export const SCOPE_KINDS: readonly KnowledgeScopeKind[] = [
  'entireLibrary',
  'enrolledRoots',
  'semanticResults',
];
export const PREFERENCES_STORAGE_KEY = 'procyon.knowledgeSearch.preferences.v1';

export function loadPreferredNeeds(): KnowledgeNeed[] {
  try {
    const stored = globalThis.localStorage?.getItem(PREFERENCES_STORAGE_KEY);
    if (stored == null) return [];
    const value: unknown = JSON.parse(stored);
    if (
      typeof value !== 'object' ||
      value === null ||
      !('needs' in value) ||
      !Array.isArray(value.needs)
    ) {
      return [];
    }
    const selected = new Set(
      value.needs.filter(
        (need): need is KnowledgeNeed =>
          typeof need === 'string' && NEEDS.includes(need as KnowledgeNeed),
      ),
    );
    return NEEDS.filter((need) => selected.has(need));
  } catch {
    return [];
  }
}

export function persistPreferredNeeds(needs: readonly KnowledgeNeed[]): void {
  try {
    globalThis.localStorage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify({ needs }));
  } catch {
    // Search remains usable when a host disables persistent browser storage.
  }
}
