import { describe, expect, it } from 'vitest';

import type { EntrySummary } from '../../models';
import { commentStyleFor, insertCommentary } from './document-commentary';

function entry(name: string): EntrySummary {
  return { name, kind: 'file' } as EntrySummary;
}

describe('document commentary', () => {
  it('chooses a comment style per file type and none for unsupported files', () => {
    expect(commentStyleFor(entry('main.ts'))).toEqual({
      kind: 'block',
      open: '/*',
      close: ' */',
    });
    expect(commentStyleFor(entry('run.py'))).toEqual({
      kind: 'line',
      prefix: '# ',
      hashbang: true,
    });
    expect(commentStyleFor(entry('style.css'))?.kind).toBe('block');
    expect(commentStyleFor(entry('notes.md'))?.kind).toBe('html');
    expect(commentStyleFor(entry('notes.txt'))?.kind).toBe('plain');
    expect(commentStyleFor(entry('data.json'))).toBeUndefined();
    expect(commentStyleFor(entry('report.pdf'))).toBeUndefined();
  });

  it('writes TypeScript commentary as one /* */ block', () => {
    const style = commentStyleFor(entry('derive.ts'));
    if (style === undefined) throw new Error('typescript has a style');
    expect(insertCommentary('export {};\n', 'Brief\nRuns', style)).toBe(
      '/*\n * Brief\n * Runs\n */\n\nexport {};\n',
    );
  });

  it('wraps every commentary line to 80 columns, prefix included', () => {
    const long = 'word '.repeat(60).trim();
    for (const name of ['a.ts', 'a.py', 'a.md', 'a.txt', 'a.sql']) {
      const style = commentStyleFor(entry(name));
      if (style === undefined) throw new Error(`${name} has a style`);
      const lines = insertCommentary('', long, style).split('\n');
      expect(Math.max(...lines.map((line) => line.length))).toBeLessThanOrEqual(80);
      expect(lines.length).toBeGreaterThan(3);
    }
  });

  it('keeps a word longer than the column limit whole-character and under the limit', () => {
    const style = commentStyleFor(entry('a.py'));
    if (style === undefined) throw new Error('python has a style');
    const lines = insertCommentary('', 'x'.repeat(200), style).split('\n');
    expect(Math.max(...lines.map((line) => line.length))).toBeLessThanOrEqual(80);
    expect(lines.join('').replaceAll('# ', '').replaceAll('x', '')).toBe('');
  });

  it('prefixes every line of code and keeps a shebang first', () => {
    const style = commentStyleFor(entry('run.sh'));
    expect(style).toBeDefined();
    if (style === undefined) return;
    expect(insertCommentary('#!/bin/sh\necho hi\n', 'Brief\nRuns greeting', style)).toBe(
      '#!/bin/sh\n# Brief\n# Runs greeting\n\necho hi\n',
    );
  });

  it('places commentary after markdown front matter', () => {
    const style = commentStyleFor(entry('notes.md'));
    if (style === undefined) throw new Error('markdown has a style');
    expect(insertCommentary('---\ntitle: x\n---\n# Body\n', 'A -- B', style)).toBe(
      '---\ntitle: x\n---\n<!--\nA - - B\n-->\n\n# Body\n',
    );
  });

  it('cannot close a block comment early', () => {
    const style = commentStyleFor(entry('style.css'));
    if (style === undefined) throw new Error('css has a style');
    expect(insertCommentary('a{}', 'x */ y', style)).toBe('/*\n * x * / y\n */\n\na{}');
  });

  it('keeps CRLF line endings', () => {
    const style = commentStyleFor(entry('todo.txt'));
    if (style === undefined) throw new Error('text has a style');
    expect(insertCommentary('one\r\ntwo', 'Summary', style)).toBe('Summary\r\n\r\none\r\ntwo');
  });
});
