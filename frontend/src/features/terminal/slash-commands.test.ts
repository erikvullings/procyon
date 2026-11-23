import { describe, expect, it } from 'vitest';
import type { LlmProfile } from '../../models';
import {
  completeSlash,
  modelChoices,
  modelLines,
  parseSlash,
  pickModel,
  slashNames,
  typedSlash,
} from './slash-commands';

const profile = (id: string, name: string, model: string) =>
  ({ id, name, model }) as unknown as LlmProfile;

describe('slash commands (task 0343)', () => {
  it('tells commands, skills and ordinary questions apart', () => {
    expect(parseSlash('/new')).toEqual({ kind: 'command', name: 'new', argument: '' });
    expect(parseSlash('/MODEL 2')).toEqual({ kind: 'command', name: 'model', argument: '2' });
    expect(parseSlash('/release v1.2 now')).toEqual({
      kind: 'skill',
      name: 'release',
      rest: 'v1.2 now',
    });
    expect(parseSlash('/etc/hosts has what?')).toBeUndefined();
    expect(parseSlash('what is /new?')).toBeUndefined();
  });

  it('completes the name being typed after # /', () => {
    const names = slashNames(['release', 'review', 'new']);
    expect(names.filter((name) => name === 'new')).toHaveLength(1);
    expect(typedSlash('# /re')).toBe('re');
    expect(typedSlash('# /re now')).toBeUndefined();
    expect(completeSlash('# /rel', names)).toEqual({ erase: 3, insert: 'release ' });
    // `clear` and `compact` share only the typed `c`.
    expect(completeSlash('# /c', names)).toBeUndefined();
    expect(completeSlash('#/co', names)).toEqual({ erase: 2, insert: 'compact ' });
    expect(completeSlash('# /x', names)).toBeUndefined();
  });

  it('extends several matches to their common prefix', () => {
    expect(completeSlash('# /r', ['release', 'review'])).toEqual({ erase: 1, insert: 're' });
  });

  it('lists every profile model once, marks the chat binding, and picks by number or name', () => {
    const choices = modelChoices(
      [profile('p1', 'Local', 'qwen'), profile('p2', 'Cloud', 'big')],
      { profileId: 'p1', model: 'qwen', allowCloud: false },
      new Map([['p1', ['qwen', 'llama']]]),
    );
    expect(modelLines(choices)).toEqual([
      '* 1. qwen (Local)',
      '  2. llama (Local)',
      '  3. big (Cloud)',
    ]);
    expect(pickModel(choices, '2')?.model).toBe('llama');
    expect(pickModel(choices, 'BIG')?.profileId).toBe('p2');
    expect(pickModel(choices, 'cloud/big')?.model).toBe('big');
    expect(pickModel(choices, '9')).toBeUndefined();
    expect(pickModel(choices, '')).toBeUndefined();
  });
});
