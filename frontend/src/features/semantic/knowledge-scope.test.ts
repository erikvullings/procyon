import { describe, expect, it } from 'vitest';
import {
  classifyKnowledgeScopeSelectors,
  emptyDraft,
  joinSubjects,
  scopeSelectorText,
  splitSubjects,
  splitValues,
} from './knowledge-scope';

describe('knowledge scope helpers', () => {
  it('keeps a quoted value containing a comma as one value', () => {
    expect(splitValues('"ACME, Inc.", beta')).toEqual(['ACME, Inc.', 'beta']);
  });

  it('round-trips subjects through the editor text', () => {
    const subjects = ['ACME, Inc.', 'plain', 'with "quote"'];
    expect(splitSubjects(joinSubjects(subjects))).toEqual(subjects);
  });

  it('serialises selectors like the backend DSL', () => {
    expect(scopeSelectorText({ kind: 'wholeLibrary' })).toBe('library');
    expect(scopeSelectorText({ kind: 'root', id: 'r1' })).toBe('root:r1');
    expect(scopeSelectorText({ kind: 'workspace', id: 'w1' })).toBe('workspace:w1');
  });

  it('classifies unknown roots and workspaces as issues, keeping known roots', () => {
    const result = classifyKnowledgeScopeSelectors(
      [
        { kind: 'root', id: 'known' },
        { kind: 'root', id: 'gone' },
        { kind: 'workspace', id: 'w' },
        { kind: 'wholeLibrary' },
      ],
      new Set(['known']),
    );
    expect(result.wholeLibrary).toBe(true);
    expect(result.rootIds).toEqual(['known']);
    expect(result.issues.map((issue) => issue.reason)).toEqual(['unknownRoot', 'unsupported']);
  });

  it('starts from an explicit empty draft', () => {
    expect(emptyDraft().about).toEqual([]);
  });
});
