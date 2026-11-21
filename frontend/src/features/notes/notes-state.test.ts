import { describe, expect, it } from 'vitest';
import type { SemanticComponentStatus, SemanticLibraryCapabilities } from '../../models';
import {
  fileNameFor,
  lineDiff,
  parseTags,
  semanticIndexingAvailable,
  slugify,
  withNoteMeta,
} from './notes-state';

describe('slugify', () => {
  it('matches the backend: lowercase ASCII words joined by hyphens', () => {
    expect(slugify('Replace nginx TLS certificate')).toBe('replace-nginx-tls-certificate');
    expect(slugify('  CSR: create (openssl) -- v2!  ')).toBe('csr-create-openssl-v2');
    expect(slugify('Größe über Ärger, café')).toBe('grosse-uber-arger-cafe');
    expect(slugify('!!!')).toBe('note');
    const long = slugify('word '.repeat(40));
    expect(long.length).toBeLessThanOrEqual(60);
    expect(long.endsWith('-')).toBe(false);
    expect(fileNameFor('Create a CSR')).toBe('create-a-csr.md');
  });
});

describe('parseTags', () => {
  it('keeps clean, unique, lowercase tags', () => {
    expect(parseTags(' nginx, #TLS,, nginx , certbot ')).toEqual(['nginx', 'tls', 'certbot']);
    expect(parseTags('')).toEqual([]);
  });
});

describe('withNoteMeta', () => {
  it('replaces title and tags and keeps other keys and the body', () => {
    const note =
      '---\ntitle: Old\ncreated: 2026-11-20T14:03:00Z\ntags:\n  - a\n  - b\naliases: [x]\n---\n# Body\n';
    expect(withNoteMeta(note, 'New: title', ['tls', 'pki, x'])).toBe(
      '---\ntitle: "New: title"\ntags: [tls, "pki, x"]\ncreated: 2026-11-20T14:03:00Z\naliases: [x]\n---\n# Body\n',
    );
  });

  it('adds front matter to a note without it and leaves out empty tags', () => {
    expect(withNoteMeta('# Body\n', 'Plain', [])).toBe('---\ntitle: Plain\n---\n# Body\n');
  });
});

describe('lineDiff', () => {
  it('marks removed and added lines in order around unchanged ones', () => {
    expect(lineDiff('a\nb\nc', 'a\nB\nc\nd')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'removed', text: 'b' },
      { kind: 'added', text: 'B' },
      { kind: 'same', text: 'c' },
      { kind: 'added', text: 'd' },
    ]);
    expect(lineDiff('same', 'same')).toEqual([{ kind: 'same', text: 'same' }]);
  });
});

describe('semanticIndexingAvailable', () => {
  const capabilities = (operations: string[]) =>
    ({ authority: 'desktopManaged', operations }) as unknown as SemanticLibraryCapabilities;
  const status = (state: string) =>
    ({ lifecycle: { state } }) as unknown as SemanticComponentStatus;

  it('needs enrolment support and an installed, enabled worker', () => {
    const full = capabilities(['previewEnrolment', 'enrol']);
    expect(semanticIndexingAvailable(full, status('installedEnabled'))).toBe(true);
    expect(semanticIndexingAvailable(full, status('paused'))).toBe(true);
    expect(semanticIndexingAvailable(full, status('absent'))).toBe(false);
    expect(
      semanticIndexingAvailable(capabilities(['viewStatus']), status('installedEnabled')),
    ).toBe(false);
    expect(semanticIndexingAvailable(undefined, status('installedEnabled'))).toBe(false);
  });
});
