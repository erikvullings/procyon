import type { SearchExecutionMode } from '../../models';
import type { FindFilesSearchParams } from './find-files-dialog';

export interface SearchPresentation {
  readonly kind: 'filename' | 'content';
  readonly term: string;
  readonly label?: string;
  readonly executionMode: SearchExecutionMode;
}

/** Selects the primary user-facing term for a recursive search. */
export function searchPresentation(
  params: FindFilesSearchParams,
  executionMode: SearchExecutionMode,
  label?: string,
): SearchPresentation {
  const filenameTerm = params.filenameQuery.trim();
  if (filenameTerm !== '') {
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
