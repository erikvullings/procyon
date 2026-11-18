import type { EntrySummary } from '../../models';
import { previewExtension } from './content-preview';

/**
 * Formats whose extracted text the summary can work from: office documents, PDFs and prose.
 * Data (JSON, CSV, XML) does not read as a summary, so it gets no button at all.
 */
const SUMMARISABLE_EXTENSIONS: ReadonlySet<string> = new Set([
  'docx',
  'pptx',
  'pdf',
  'md',
  'markdown',
  'txt',
  'text',
  'log',
  'html',
  'htm',
  'xhtml',
]);

/** Source code is described (what it does), not summarised. Mirrors the backend's list. */
const DESCRIBED_CODE_EXTENSIONS: ReadonlySet<string> = new Set([
  'c',
  'cc',
  'cjs',
  'cpp',
  'cs',
  'css',
  'cxx',
  'go',
  'h',
  'hpp',
  'hs',
  'java',
  'js',
  'jsx',
  'kt',
  'lua',
  'm',
  'mjs',
  'php',
  'pl',
  'ps1',
  'py',
  'r',
  'rb',
  'rs',
  'scala',
  'scss',
  'sh',
  'sql',
  'swift',
  'ts',
  'tsx',
  'vue',
  'zsh',
]);

/** What the generated text is about; the backend keys its prompt on the same split. */
export type DocumentSummaryKind = 'document' | 'sourceCode';

/** The kind of generated text offered for `entry`, or `undefined` when none is offered. */
export function documentSummaryKind(entry: EntrySummary): DocumentSummaryKind | undefined {
  const extension = previewExtension(entry);
  if (extension === undefined) return undefined;
  if (DESCRIBED_CODE_EXTENSIONS.has(extension)) return 'sourceCode';
  return SUMMARISABLE_EXTENSIONS.has(extension) ? 'document' : undefined;
}

/** Whether "Generate summary" (or "Describe code") is offered for `entry`. */
export function supportsDocumentSummary(entry: EntrySummary): boolean {
  return documentSummaryKind(entry) !== undefined;
}

/** i18n key of the generate button: code is described, everything else is summarised. */
export function documentSummaryActionKey(
  entry: EntrySummary | undefined,
): 'generate' | 'describeCode' {
  return entry !== undefined && documentSummaryKind(entry) === 'sourceCode'
    ? 'describeCode'
    : 'generate';
}
