import { describe, expect, it } from 'vitest';
import {
  caretOnFirstLine,
  caretOnLastLine,
  completed,
  composerOpening,
  promptIsEmpty,
} from './question-composer';

describe('composerOpening', () => {
  it('opens on a typed #, with nothing after it', () => {
    expect(composerOpening('#')).toEqual({ text: '', submit: false });
  });

  it('takes the text of a pasted question, with or without bracketed-paste markers', () => {
    expect(composerOpening('# list files')).toEqual({ text: 'list files', submit: false });
    expect(composerOpening('\x1b[200~# two\rlines\x1b[201~')).toEqual({
      text: 'two\nlines',
      submit: false,
    });
  });

  it('sends a question typed together with Enter', () => {
    expect(composerOpening('#ls\r')).toEqual({ text: 'ls', submit: true });
  });

  it('leaves other input and editing keys to the shell', () => {
    expect(composerOpening('ls')).toBeUndefined();
    expect(composerOpening('\x1b[A')).toBeUndefined();
    expect(composerOpening('#\t')).toBeUndefined();
  });
});

describe('promptIsEmpty', () => {
  it('sees a prompt with nothing typed after it', () => {
    expect(promptIsEmpty('erik@pc deception % ')).toBe(true);
    expect(promptIsEmpty('root@host:~# ')).toBe(true);
    expect(promptIsEmpty('')).toBe(true);
  });

  it('sees a line with a command on it', () => {
    expect(promptIsEmpty('erik@pc deception % ls -la')).toBe(false);
  });
});

describe('composer editing helpers', () => {
  it('applies a completion at the end', () => {
    expect(completed('read @mai', { erase: 3, insert: 'src/main.rs ' })).toBe('read @src/main.rs ');
  });

  it('knows whether the caret is on the first or last line', () => {
    expect(caretOnFirstLine('a\nb', 1)).toBe(true);
    expect(caretOnFirstLine('a\nb', 2)).toBe(false);
    expect(caretOnLastLine('a\nb', 1)).toBe(false);
    expect(caretOnLastLine('a\nb', 3)).toBe(true);
  });
});
