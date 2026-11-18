import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models';
import {
  documentSummaryActionKey,
  documentSummaryKind,
  supportsDocumentSummary,
} from './document-summary-support';

function entry(name: string): EntrySummary {
  return { name, kind: 'file' } as EntrySummary;
}

describe('document summary support', () => {
  it('summarises documents and prose', () => {
    for (const name of ['report.docx', 'deck.pptx', 'paper.pdf', 'notes.md', 'todo.txt']) {
      expect(documentSummaryKind(entry(name))).toBe('document');
      expect(documentSummaryActionKey(entry(name))).toBe('generate');
    }
  });

  it('describes source code rather than summarising it', () => {
    expect(documentSummaryKind(entry('main.RS'))).toBe('sourceCode');
    expect(documentSummaryActionKey(entry('main.rs'))).toBe('describeCode');
    expect(supportsDocumentSummary(entry('run.py'))).toBe(true);
  });

  it('offers nothing for data files or files without an extension', () => {
    for (const name of ['hf_download_tasks.json', 'table.csv', 'config.yaml', 'Makefile']) {
      expect(supportsDocumentSummary(entry(name))).toBe(false);
    }
    expect(documentSummaryActionKey(undefined)).toBe('generate');
  });
});
