import { describe, expect, it } from 'vitest';
import type { LinkProblem } from '../../models';
import {
  INITIAL_LINK_DOCTOR_STATE,
  missingTargetName,
  problemKey,
  sourceName,
  withoutProblem,
} from './link-doctor-state';

const problem: LinkProblem = {
  kind: 'missingDocumentLink',
  source: { providerId: 'local', uri: 'file:///notes/My%20Notes.md' },
  target: 'img/old%20pic.png',
  resolved: { providerId: 'local', uri: 'file:///notes/img/old%20pic.png' },
  line: 4,
};

describe('link doctor state helpers', () => {
  it('names the missing target from its resolved location, decoded', () => {
    expect(missingTargetName(problem)).toBe('old pic.png');
    expect(missingTargetName({ ...problem, resolved: null, target: '../a/b.txt' })).toBe('b.txt');
  });

  it('names the source file, decoded', () => {
    expect(sourceName(problem)).toBe('My Notes.md');
  });

  it('removes a repaired problem and its candidates', () => {
    const key = problemKey(problem);
    const state = {
      ...INITIAL_LINK_DOCTOR_STATE,
      problems: [problem],
      candidates: { [key]: { loading: false, candidates: [], truncated: false } },
    };
    const next = withoutProblem(state, key);
    expect(next.problems).toEqual([]);
    expect(next.candidates).toEqual({});
  });
});
