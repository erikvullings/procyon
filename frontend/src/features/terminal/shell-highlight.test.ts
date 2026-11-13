// biome-ignore-all lint/suspicious/noTemplateCurlyInString: shell text, not JS templates
import { describe, expect, it } from 'vitest';
import { highlightShell, type ShellToken } from './shell-highlight';

const kinds = (tokens: readonly ShellToken[]) =>
  tokens.filter((token) => token.kind !== 'plain').map((token) => [token.text, token.kind]);

describe('highlightShell', () => {
  it('keeps the text intact across tokens', () => {
    const line = `git log --oneline | grep "fix x" && echo $HOME; # done`;
    expect(
      highlightShell(line)
        .map((token) => token.text)
        .join(''),
    ).toBe(line);
  });

  it('marks the command, options, strings, variables, operators and comments', () => {
    expect(kinds(highlightShell(`ls -la "my dir" | wc -l $HOME # count`))).toEqual([
      ['ls', 'command'],
      ['-la', 'option'],
      ['"my dir"', 'string'],
      ['|', 'operator'],
      ['wc', 'command'],
      ['-l', 'option'],
      ['$HOME', 'variable'],
      ['# count', 'comment'],
    ]);
  });

  it('treats the word after a separator as a new command, but not after a redirection', () => {
    expect(kinds(highlightShell('make && make install'))).toEqual([
      ['make', 'command'],
      ['&&', 'operator'],
      ['make', 'command'],
    ]);
    expect(kinds(highlightShell('cat > out.txt'))).toEqual([
      ['cat', 'command'],
      ['>', 'operator'],
    ]);
  });

  it('keeps an escaped quote inside a double-quoted string and reads ${...} as a variable', () => {
    expect(kinds(highlightShell('echo "a\\"b" ${HOME}'))).toEqual([
      ['echo', 'command'],
      ['"a\\"b"', 'string'],
      ['${HOME}', 'variable'],
    ]);
  });

  it('does not start a comment inside a word', () => {
    expect(kinds(highlightShell('echo a#b'))).toEqual([['echo', 'command']]);
  });
});
