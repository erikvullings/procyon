import { describe, expect, it } from 'vitest';
import { parseShellMark, SHELL_INTEGRATION } from './shell-integration';

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

describe('SHELL_INTEGRATION', () => {
  it('emits the prompt, output and end marks in every supported shell', () => {
    for (const snippet of Object.values(SHELL_INTEGRATION)) {
      expect(snippet).toContain('133;A');
      expect(snippet).toContain('133;C');
      expect(snippet).toContain('133;D;');
    }
  });
});
