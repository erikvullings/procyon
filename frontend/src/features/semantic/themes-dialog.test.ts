import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { mockCorpusThemes } from '../../api/client/mock-corpus-themes';
import { MockFileManagerClient } from '../../api/client/mock-file-manager-client';
import { setLocale } from '../../i18n';
import type { RagScope } from '../../models';
import { ThemesDialog } from './themes-dialog';

let root: HTMLElement;

const scope = (kind: 'entireLibrary' | 'currentFolder', label: string): RagScope => ({
  kind,
  workspaceId: '22222222-2222-4222-8222-222222222222',
  label,
  selectedFiles: [],
  semanticSourceIds: [],
  enrolledRootIds: [],
});

beforeEach(() => {
  setLocale('en');
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function mount(
  client: MockFileManagerClient,
  overrides: Partial<Parameters<typeof ThemesDialog>[0]['attrs']> = {},
): { onOpenPassage: ReturnType<typeof vi.fn>; onClose: ReturnType<typeof vi.fn> } {
  const onOpenPassage = vi.fn();
  const onClose = vi.fn();
  m.mount(root, {
    view: () =>
      m(ThemesDialog, {
        open: true,
        client,
        scopes: [scope('entireLibrary', 'Entire library')],
        onOpenPassage,
        onClose,
        ...overrides,
      }),
  });
  return { onOpenPassage, onClose };
}

describe('ThemesDialog', () => {
  it('shows a busy skeleton, then an accessible theme list with keywords and shares', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    mount(client);
    m.redraw.sync();
    expect(root.textContent).toContain('Finding themes');
    await vi.waitFor(() => expect(analyze).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-list')).not.toBeNull());

    const rows = [...root.querySelectorAll('.fm-themes-row')];
    expect(rows.map((row) => row.querySelector('.fm-themes-row-label')?.textContent)).toEqual([
      'harbour, shipping',
      'mortgage, interest',
      'violin',
      'No theme',
    ]);
    expect(rows[0]?.textContent).toContain('10 · 42%');
    expect(root.querySelector('ul[aria-label="Themes"]')).not.toBeNull();
    // The canvas is decorative; the list carries the content.
    expect(root.querySelector('canvas')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('selecting a theme reveals its keywords, passages and strongest documents', async () => {
    const client = new MockFileManagerClient();
    mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.querySelector('.fm-themes-details')).toBeNull();

    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    const details = root.querySelector('.fm-themes-details');
    expect(details?.textContent).toContain('Keywords');
    expect(details?.textContent).toContain('shipping');
    expect(details?.textContent).toContain('Documents in this theme');
    expect(details?.textContent).toContain('Port report.txt');
    expect(root.querySelector('.fm-themes-row[aria-pressed="true"]')).not.toBeNull();

    // Selecting the same theme again clears the filter.
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-themes-details')).toBeNull();
  });

  it('opens a passage source through the host and reports a failure', async () => {
    const client = new MockFileManagerClient();
    const { onOpenPassage } = mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    (root.querySelector('.fm-themes-row') as HTMLButtonElement).click();
    m.redraw.sync();

    onOpenPassage.mockRejectedValueOnce(new Error('gone'));
    (root.querySelector('.fm-themes-open') as HTMLButtonElement).click();
    expect(onOpenPassage).toHaveBeenCalledWith(
      expect.objectContaining({ sourceId: 'mock-source-a', chunkId: 'chunk-a' }),
    );
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('.fm-themes-error')?.textContent).toBe('gone');
    });
  });

  it('flags a stale analysis when the index revision moves on and re-analyses on request', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    mount(client);
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.querySelector('.fm-themes-stale')).toBeNull();
    m.mount(root, null);
    vi.spyOn(client, 'getCorpusThemeStatus').mockResolvedValue({
      indexRevision: 'changed',
      documentCount: 3,
      chunkCount: 30,
      modelId: 'mock-embedding-model',
      profileTooWeak: false,
    });
    mount(client);
    await vi.waitFor(() => {
      m.redraw.sync();
      expect(root.querySelector('.fm-themes-stale')).not.toBeNull();
    });
    const before = analyze.mock.calls.length;
    (root.querySelector('.fm-themes-stale button') as HTMLButtonElement).click();
    await vi.waitFor(() => expect(analyze.mock.calls.length).toBe(before + 1));
  });

  it('explains a profile that is too weak instead of showing an empty map', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'analyzeCorpusThemes').mockResolvedValue({
      outcome: 'profileTooWeak',
      modelId: 'procyon.dev.hashing-embedding',
      themes: null,
    });
    mount(client);
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-themes-notice')?.textContent).toContain(
        'procyon.dev.hashing-embedding',
      ),
    );
    expect(root.querySelector('canvas')).toBeNull();
    expect(root.querySelector('.fm-themes-list')).toBeNull();
  });

  it('switches scope and shows a sampled-map disclosure', async () => {
    const client = new MockFileManagerClient();
    const analyze = vi.spyOn(client, 'analyzeCorpusThemes');
    analyze.mockResolvedValue({
      outcome: 'ready',
      modelId: 'mock-embedding-model',
      themes: { ...mockCorpusThemes(), mapSampled: true },
    });
    mount(client, {
      scopes: [scope('entireLibrary', 'Entire library'), scope('currentFolder', 'Current folder')],
    });
    await vi.waitFor(() => expect(root.querySelector('.fm-themes-row')).not.toBeNull());
    expect(root.textContent).toContain('The map shows a sample of 24 of 24 passages.');

    const buttons = [...root.querySelectorAll<HTMLButtonElement>('.fm-themes-scope')];
    expect(buttons.map((button) => button.textContent)).toEqual([
      'Entire library',
      'Current folder',
    ]);
    buttons[1]?.click();
    await vi.waitFor(() => expect(analyze).toHaveBeenCalledTimes(2));
    expect(analyze.mock.calls[1]?.[0].scope.kind).toBe('currentFolder');
  });
});
