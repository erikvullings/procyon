import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { StartSearchResult } from '../../models';
import { exportSemanticEvaluationCases, recordSemanticFeedback } from './semantic-feedback';

const result: NonNullable<StartSearchResult['semanticResults']>[number] = {
  entryId: 'entry-1',
  location: { providerId: 'local', uri: 'file:///library/report.pdf' },
  score: 0.9,
  bestEvidence: {
    recordId: 'record-1',
    sourceId: 'source-1',
    score: 0.9,
    chunkKind: 'paragraph',
    excerpt: 'Grounded evidence',
    provenanceJson: '{"kind":"textLines","startLine":1,"endLine":2}',
    indexedContentHash: 'sha256:fixture',
    generation: 3,
    available: true,
    stale: false,
    generated: false,
    sourcePosition: 0,
  },
  additionalEvidence: [],
  additionalSourceIds: [],
};

describe('semantic feedback', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('keeps one local judgement per query and result', () => {
    recordSemanticFeedback('report', result, true);
    recordSemanticFeedback('report', result, false);

    const stored = JSON.parse(
      localStorage.getItem('procyon.semanticEvaluation.v1') ?? '[]',
    ) as unknown[];
    expect(stored).toHaveLength(1);
    expect(stored[0]).toMatchObject({
      query: 'report',
      entryId: 'entry-1',
      relevant: false,
      evidenceHash: 'sha256:fixture',
    });
  });

  it('exports only after an explicit action', () => {
    recordSemanticFeedback('report', result, true);
    const anchor = document.createElement('a');
    const click = vi.spyOn(anchor, 'click').mockImplementation(() => undefined);
    vi.spyOn(document, 'createElement').mockReturnValue(anchor);
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:evaluation');
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);

    exportSemanticEvaluationCases();

    expect(click).toHaveBeenCalledOnce();
  });
});
