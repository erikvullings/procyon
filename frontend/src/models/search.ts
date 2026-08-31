import type { Location } from './location';

export type SearchNameMode = 'substring' | 'glob';
export type SearchEntryKind = 'file' | 'directory' | 'symlink';
export type SearchGitStatus = 'clean' | 'modified' | 'staged' | 'untracked' | 'ignored';
export type SearchPredicateKind =
  | 'name'
  | 'entryKind'
  | 'mimeType'
  | 'size'
  | 'modifiedTime'
  | 'content'
  | 'gitStatus'
  | 'tags'
  | 'metadata';

/** How a search result set is being populated by the backend. */
export type SearchExecutionMode = 'indexed' | 'liveRecursive' | 'mixed';

export interface SearchQuery {
  readonly schemaVersion: 1;
  readonly scope: {
    readonly locations: readonly Location[];
    readonly recurse: boolean;
    readonly showHidden: boolean;
  };
  readonly name?: {
    readonly pattern: string;
    readonly mode: SearchNameMode;
    readonly caseSensitive: boolean;
  };
  readonly entryKinds: readonly SearchEntryKind[];
  readonly mimeTypes: readonly string[];
  readonly minSizeBytes?: number;
  readonly maxSizeBytes?: number;
  readonly modifiedAfter?: string;
  readonly modifiedBefore?: string;
  readonly content?: {
    readonly query: string;
    readonly regex: boolean;
    readonly caseSensitive: boolean;
    readonly wholeWord: boolean;
  };
  readonly gitStatuses: readonly SearchGitStatus[];
  readonly tags: readonly string[];
  readonly metadata: Readonly<Record<string, string>>;
}

export interface SavedSearch {
  readonly id: string;
  readonly name: string;
  readonly pinned: boolean;
  readonly query: SearchQuery;
}

export interface SearchProviderLimitation {
  readonly providerId: string;
  readonly unevaluatedPredicates: readonly SearchPredicateKind[];
}
