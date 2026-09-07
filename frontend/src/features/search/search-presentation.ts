import type { SearchExecutionMode, StartSearchResult } from '../../models';
import type { FindFilesSearchParams } from './find-files-dialog';

export interface SearchPresentation {
  readonly kind: 'filename' | 'content' | 'semantic';
  readonly term: string;
  readonly label?: string;
  readonly executionMode: SearchExecutionMode;
  readonly semanticResults?: StartSearchResult['semanticResults'];
  readonly semanticCoverage?: StartSearchResult['semanticCoverage'];
}

/** Selects the primary user-facing term for a recursive search. */
export function searchPresentation(
  params: FindFilesSearchParams,
  executionMode: SearchExecutionMode,
  label?: string,
  semanticResults?: StartSearchResult['semanticResults'],
  semanticCoverage?: StartSearchResult['semanticCoverage'],
): SearchPresentation {
  const mode = params.mode ?? (params.contentQuery === undefined ? 'name' : 'content');
  if (mode === 'semantic') {
    return {
      kind: 'semantic',
      term: params.semanticQuery?.trim() ?? '',
      ...(label === undefined ? {} : { label }),
      executionMode,
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
    };
  }

  return {
    kind: 'content',
    term: params.contentQuery?.trim() ?? '',
    ...(label === undefined ? {} : { label }),
    executionMode,
  };
}
