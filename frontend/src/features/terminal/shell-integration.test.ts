import { describe, expect, it } from 'vitest';
import { parseShellMark } from './shell-integration';

describe('parseShellMark', () => {
  it('reads prompt, output and end marks', () => {
    expect(parseShellMark('A')).toEqual({ kind: 'prompt' });
    expect(parseShellMark('C')).toEqual({ kind: 'output' });
    expect(parseShellMark('D;2')).toEqual({ kind: 'end', exitCode: 2 });
    expect(parseShellMark('D')).toEqual({ kind: 'end', exitCode: 0 });
  });

  it('ignores marks the terminal does not use', () => {
    expect(parseShellMark('B')).toBeUndefined();
    expect(parseShellMark('nonsense')).toBeUndefined();
  });
});
