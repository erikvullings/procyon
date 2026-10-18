import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import { isSafeExternalUrl, lineRange, resolveDetectedLink } from './link-actions';

const base = { providerId: 'local', uri: 'file:///work/notes/todo.md' };
const entry = { id: 'e', name: 'x', kind: 'file' } as EntrySummary;

function client(overrides: { exists?: boolean; home?: string | undefined } = {}) {
  return {
    getHomeDirectory: vi.fn().mockResolvedValue('home' in overrides ? overrides.home : '/Users/me'),
    resolveLinkTarget: vi.fn((location: { uri: string }) =>
      overrides.exists === false
        ? Promise.reject(new Error('not found'))
        : Promise.resolve({ ...entry, name: location.uri }),
    ),
  };
}

describe('isSafeExternalUrl', () => {
  it('allows only http and https', () => {
    expect(isSafeExternalUrl('https://example.com/a')).toBe(true);
    expect(isSafeExternalUrl('http://example.com')).toBe(true);
    for (const unsafe of ['javascript:alert(1)', 'file:///etc/passwd', 'ftp://x', 'not a url']) {
      expect(isSafeExternalUrl(unsafe)).toBe(false);
    }
  });
});

describe('resolveDetectedLink', () => {
  it('resolves relative paths against the edited file and keeps the line suffix', async () => {
    const result = await resolveDetectedLink(
      { kind: 'path', path: '../src/a b.ts', line: 12, column: 3, from: 0, to: 5 },
      base,
      client(),
    );
    expect(result).toMatchObject({
      kind: 'path',
      location: { uri: 'file:///work/src/a%20b.ts' },
      line: 12,
      column: 3,
    });
  });

  it('expands ~ with the home directory and refuses it when unknown', async () => {
    const link = {
      kind: 'path',
      path: '~/x.txt',
      line: undefined,
      column: undefined,
      from: 0,
      to: 1,
    } as const;
    expect(await resolveDetectedLink(link, base, client())).toMatchObject({
      location: { uri: 'file:///Users/me/x.txt' },
    });
    expect(await resolveDetectedLink(link, base, client({ home: undefined }))).toBeUndefined();
  });

  it('reports paths that do not exist as unresolved', async () => {
    const link = {
      kind: 'path',
      path: '/nope.txt',
      line: undefined,
      column: undefined,
      from: 0,
      to: 1,
    } as const;
    expect(await resolveDetectedLink(link, base, client({ exists: false }))).toBeUndefined();
  });

  it('passes only safe web URLs', async () => {
    expect(
      await resolveDetectedLink(
        { kind: 'url', url: 'https://a.test', from: 0, to: 1 },
        base,
        client(),
      ),
    ).toEqual({ kind: 'url', url: 'https://a.test' });
    expect(
      await resolveDetectedLink(
        { kind: 'url', url: 'javascript:1', from: 0, to: 1 },
        base,
        client(),
      ),
    ).toBeUndefined();
  });
});

describe('lineRange', () => {
  it('selects one-based lines and rejects lines past the end', () => {
    const text = 'one\ntwo\nthree';
    expect(lineRange(text, 1)).toEqual({ from: 0, to: 3 });
    expect(lineRange(text, 2)).toEqual({ from: 4, to: 7 });
    expect(lineRange(text, 3)).toEqual({ from: 8, to: 13 });
    expect(lineRange(text, 4)).toBeUndefined();
    expect(lineRange(text, 0)).toBeUndefined();
  });
});
