import { describe, expect, it, vi } from 'vitest';
import type { EntrySummary, Location } from '../../models';
import type { EditorLinkHandlers, ResolvedLink } from './link-actions';
import type { DetectedLink } from './link-detection';
import {
  followMarkdownLink,
  markdownLinkTarget,
  matchSibling,
  resolveMarkdownLink,
} from './markdown-links';
import { safeMarkdownHtml } from './markdown-preview';

function file(name: string, kind: EntrySummary['kind'] = 'file'): EntrySummary {
  return {
    id: name,
    location: { providerId: 'local', uri: `file:///tasks/${encodeURIComponent(name)}` },
    name,
    kind,
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

const SIBLINGS = [
  file('0033-old.md'),
  file('0336-terminal-assistant.md'),
  file('0336-folder', 'directory'),
  file('README.md'),
  file('My Note.md'),
  file('my note.txt'),
  file('diagram.png'),
];

function anchorFrom(html: string): Element {
  const host = document.createElement('div');
  host.innerHTML = html;
  const anchor = host.querySelector('a');
  if (anchor === null) throw new Error(`no anchor in ${html}`);
  return anchor;
}

type PathLink = Extract<ResolvedLink, { kind: 'path' }>;

/** Handlers whose `resolve` finds paths among `existing` (relative to `file:///tasks/`). */
function handlers(existing: readonly EntrySummary[]) {
  const base: Location = { providerId: 'local', uri: 'file:///tasks/0337.md' };
  const view = vi.fn<(link: PathLink) => void>();
  const openInPane = vi.fn<(link: PathLink) => void>();
  const links: EditorLinkHandlers = {
    base,
    resolve: async (link: DetectedLink): Promise<ResolvedLink | undefined> => {
      if (link.kind === 'url') return { kind: 'url', url: link.url };
      const entry = existing.find((candidate) => candidate.name === link.path);
      return entry === undefined
        ? undefined
        : { kind: 'path', location: entry.location, entry, line: undefined, column: undefined };
    },
    siblings: () => SIBLINGS,
    openUrl: vi.fn(),
    openInPane,
    reveal: vi.fn(),
    editAt: vi.fn(),
    view,
  };
  return { links, view, openInPane };
}

describe('markdownLinkTarget', () => {
  it('reads task, wiki, path and url anchors from rendered markdown', () => {
    const task = safeMarkdownHtml('# T\n\nStatus: open\nDepends on: 0336, 0033');
    expect(markdownLinkTarget(anchorFrom(task))).toEqual({ kind: 'task', id: '0336' });
    expect(markdownLinkTarget(anchorFrom(safeMarkdownHtml('[[My Note#Steps]]')))).toEqual({
      kind: 'wiki',
      name: 'My Note',
    });
    expect(markdownLinkTarget(anchorFrom(safeMarkdownHtml('[x](sub/My%20Note.md#a)')))).toEqual({
      kind: 'path',
      path: 'sub/My Note.md',
    });
    expect(markdownLinkTarget(anchorFrom('<a href="https://example.com">x</a>'))).toEqual({
      kind: 'url',
      url: 'https://example.com',
    });
    expect(markdownLinkTarget(anchorFrom('<a href="#top">x</a>'))).toBeUndefined();
  });
});

describe('matchSibling', () => {
  it('matches a task ID to its NNNN-title.md file, never a folder or a prefix of another ID', () => {
    expect(matchSibling({ kind: 'task', id: '0336' }, SIBLINGS)?.name).toBe(
      '0336-terminal-assistant.md',
    );
    expect(matchSibling({ kind: 'task', id: '0003' }, SIBLINGS)).toBeUndefined();
  });

  it('matches wiki names case-insensitively, preferring markdown, or exactly with an extension', () => {
    expect(matchSibling({ kind: 'wiki', name: 'my note' }, SIBLINGS)?.name).toBe('My Note.md');
    expect(matchSibling({ kind: 'wiki', name: 'readme' }, SIBLINGS)?.name).toBe('README.md');
    expect(matchSibling({ kind: 'wiki', name: 'diagram.png' }, SIBLINGS)?.name).toBe('diagram.png');
    expect(matchSibling({ kind: 'wiki', name: 'Missing' }, SIBLINGS)).toBeUndefined();
  });
});

describe('resolveMarkdownLink', () => {
  it('falls back to a NAME.md path for wikilinks outside the listed folder', async () => {
    const nested = file('sub/Deep.md');
    const resolved = await resolveMarkdownLink(
      { kind: 'wiki', name: 'sub/Deep' },
      handlers([nested]).links,
    );
    expect(resolved).toMatchObject({ kind: 'path', entry: { name: 'sub/Deep.md' } });
  });

  it('does not guess a path for an unlisted task ID', async () => {
    expect(
      await resolveMarkdownLink({ kind: 'task', id: '9999' }, handlers(SIBLINGS).links),
    ).toBeUndefined();
  });
});

describe('followMarkdownLink', () => {
  it('views files, opens folders in a pane and reports misses', async () => {
    const { links, view, openInPane } = handlers(SIBLINGS);
    const missed = vi.fn();
    const external = vi.fn();
    await followMarkdownLink({ kind: 'task', id: '0336' }, links, external, missed);
    expect(view).toHaveBeenCalledWith(
      expect.objectContaining({
        entry: expect.objectContaining({ name: '0336-terminal-assistant.md' }),
      }),
    );
    await followMarkdownLink({ kind: 'path', path: '0336-folder' }, links, external, missed);
    expect(openInPane).toHaveBeenCalledTimes(1);
    await followMarkdownLink({ kind: 'wiki', name: 'Nope' }, links, external, missed);
    await followMarkdownLink({ kind: 'url', url: 'javascript:alert(1)' }, links, external, missed);
    expect(missed).toHaveBeenCalledTimes(2);
    await followMarkdownLink({ kind: 'url', url: 'https://example.com' }, links, external, missed);
    expect(external).toHaveBeenCalledWith('https://example.com');
  });
});
