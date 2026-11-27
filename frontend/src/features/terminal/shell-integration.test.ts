import { describe, expect, it } from 'vitest';
import { parseShellMark } from './shell-integration';

describe('parseShellMark', () => {
  it('reads prompt, output and end marks', () => {
    expect(parseShellMark('A')).toEqual({ kind: 'prompt' });
    expect(parseShellMark('C')).toEqual({ kind: 'output' });
    expect(parseShellMark('D;2')).toEqual({ kind: 'end', exitCode: 2 });
    expect(parseShellMark('D')).toEqual({ kind: 'end', exitCode: 0 });
  });

  it('reads shell-reported command text without losing the output mark on invalid metadata', () => {
    expect(parseShellMark('C;Command=echo one; echo two')).toEqual({
      kind: 'output',
      reportedCommand: 'echo one; echo two',
    });
    for (const command of ['', '  ', 'one\ntwo', 'hello\x07', 'a'.repeat(1025)]) {
      expect(parseShellMark(`C;Command=${command}`)).toEqual({ kind: 'output' });
    }
  });

  it('ignores marks the terminal does not use', () => {
    expect(parseShellMark('B')).toBeUndefined();
    expect(parseShellMark('nonsense')).toBeUndefined();
  });

  it('reads the working folder the shell reports', () => {
    expect(parseShellMark('P;Cwd=/Users/me/My Docs')).toEqual({
      kind: 'cwd',
      path: '/Users/me/My Docs',
    });
  });

  it('ignores a folder that is relative or holds control bytes', () => {
    expect(parseShellMark('P;Cwd=docs')).toBeUndefined();
    expect(parseShellMark('P;Cwd=/a\u0007b')).toBeUndefined();
    expect(parseShellMark('P;Cwd=')).toBeUndefined();
  });
});
