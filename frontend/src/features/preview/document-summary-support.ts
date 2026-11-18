import type { EntrySummary } from '../../models';
import { previewExtension } from './content-preview';

/**
 * Formats whose extracted text the summary can work from: office documents, PDFs and prose.
 * Data (JSON, CSV, XML) and code do not read as a summary, so they get no "Generate summary".
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

/** Whether "Generate summary" is offered for `entry`. */
export function supportsDocumentSummary(entry: EntrySummary): boolean {
  const extension = previewExtension(entry);
  return extension !== undefined && SUMMARISABLE_EXTENSIONS.has(extension);
}
