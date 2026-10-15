import type { Location, SearchExecutionMode, SearchQuery, StartSearchResult } from '../../models';
import type { FindFilesSearchParams } from './find-files-dialog';

export interface SearchPresentation {
  readonly kind: 'filename' | 'content' | 'semantic';
  readonly term: string;
  readonly label?: string;
  readonly headerTerm?: string;
  readonly executionMode: SearchExecutionMode;
  readonly semanticResults?: StartSearchResult['semanticResults'];
  readonly semanticCoverage?: StartSearchResult['semanticCoverage'];
  readonly conceptSearch?: boolean;
  readonly scope?:
    | { readonly kind: 'allEnrolled' }
    | { readonly kind: 'folder'; readonly location: Location };
}

/** Selects the primary user-facing term for a recursive search. */
export function searchPresentation(
  params: FindFilesSearchParams,
  executionMode: SearchExecutionMode,
  label?: string,
  semanticResults?: StartSearchResult['semanticResults'],
  semanticCoverage?: StartSearchResult['semanticCoverage'],
  query?: SearchQuery,
): SearchPresentation {
  const scope =
    query?.semantic?.scope === 'entireLibrary'
      ? { kind: 'allEnrolled' as const }
      : query?.scope.locations[0] === undefined
        ? undefined
        : { kind: 'folder' as const, location: query.scope.locations[0] };
  const mode = params.mode ?? (params.contentQuery === undefined ? 'name' : 'content');
  if (mode === 'semantic') {
    return {
      kind: 'semantic',
      term: params.semanticQuery?.trim() ?? '',
      ...(label === undefined ? {} : { label }),
      ...(params.conceptLabel === undefined ? {} : { headerTerm: params.conceptLabel }),
      executionMode,
      ...(scope === undefined ? {} : { scope }),
      ...(params.selectedConcept === undefined ? {} : { conceptSearch: true }),
      ...(semanticResults === undefined ? {} : { semanticResults }),
      ...(semanticCoverage === undefined ? {} : { semanticCoverage }),
    };
  }
  const filenameTerm = params.filenameQuery.trim();
  if (mode === 'name') {
    return {
      kind: 'filename',
      term: filenameTerm,
      ...(label === undefined ? {} : { label }),
      executionMode,
      ...(scope === undefined ? {} : { scope }),
    };
  }

  return {
    kind: 'content',
    term: params.contentQuery?.trim() ?? '',
    ...(label === undefined ? {} : { label }),
    executionMode,
    ...(scope === undefined ? {} : { scope }),
  };
}
