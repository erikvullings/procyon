import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { EntrySummary } from '../../models';
import type { CodeMirrorEditorAttrs } from './code-mirror-editor';
import type { EditorLinkHandlers, ResolvedLink } from './link-actions';
import { LinkAwareEditor } from './link-aware-editor';

let captured: CodeMirrorEditorAttrs | undefined;
vi.mock('./code-mirror-editor', () => ({
  CodeMirrorEditor: {
    view: ({ attrs }: { attrs: CodeMirrorEditorAttrs }) => {
      captured = attrs;
      return m('.fake-editor');
    },
  },
}));

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.append(root);
  captured = undefined;
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const fileEntry = { id: 'f', name: 'a.txt', kind: 'file' } as EntrySummary;
const pathLink: ResolvedLink = {
  kind: 'path',
  location: { providerId: 'local', uri: 'file:///work/a.txt' },
  entry: fileEntry,
  line: 7,
  column: undefined,
};

function handlers(resolved: ResolvedLink | undefined): EditorLinkHandlers {
  return {
    base: { providerId: 'local', uri: 'file:///work/notes.md' },
    resolve: vi.fn().mockResolvedValue(resolved),
    openUrl: vi.fn(),
    openInPane: vi.fn(),
    reveal: vi.fn(),
    editAt: vi.fn(),
  };
}

async function rightClick(
  link: Parameters<NonNullable<CodeMirrorEditorAttrs['onLinkContextMenu']>>[0]['link'],
) {
  captured?.onLinkContextMenu?.({ link, x: 10, y: 20 });
  await Promise.resolve();
  await Promise.resolve();
  m.redraw.sync();
}

function menuLabels(): string[] {
  return Array.from(root.querySelectorAll('.fm-link-menu-item')).map(
    (item) => item.textContent ?? '',
  );
}

describe('LinkAwareEditor', () => {
  it('offers open, reveal and edit-at-line for an existing local file path', async () => {
    const actions = handlers(pathLink);
    m.mount(root, { view: () => m(LinkAwareEditor, { content: 'x', links: actions }) });
    m.redraw.sync();

    await rightClick({ kind: 'path', path: 'a.txt', line: 7, column: undefined, from: 0, to: 5 });

    expect(menuLabels()).toEqual(['Open in pane', 'Reveal in file manager', 'Edit file at line 7']);
    (root.querySelectorAll('.fm-link-menu-item')[2] as HTMLButtonElement).click();
    expect(actions.editAt).toHaveBeenCalledWith(pathLink);
    m.redraw.sync();
    expect(root.querySelector('.fm-link-menu')).toBeNull();
  });

  it('offers only Open URL for a web link and runs it', async () => {
    const actions = handlers({ kind: 'url', url: 'https://example.com' });
    m.mount(root, { view: () => m(LinkAwareEditor, { content: 'x', links: actions }) });
    m.redraw.sync();

    await rightClick({ kind: 'url', url: 'https://example.com', from: 0, to: 19 });

    expect(menuLabels()).toEqual(['Open URL']);
    (root.querySelector('.fm-link-menu-item') as HTMLButtonElement).click();
    expect(actions.openUrl).toHaveBeenCalledWith('https://example.com');
  });

  it('says so when the path does not exist and never offers actions', async () => {
    m.mount(root, {
      view: () => m(LinkAwareEditor, { content: 'x', links: handlers(undefined) }),
    });
    m.redraw.sync();

    await rightClick({
      kind: 'path',
      path: '/nope',
      line: undefined,
      column: undefined,
      from: 0,
      to: 5,
    });

    expect(menuLabels()).toEqual([]);
    expect(root.querySelector('.fm-link-menu-note')?.textContent).toBe('This path does not exist.');
  });

  it('adds no context-menu hook when no link handlers are provided', () => {
    m.mount(root, { view: () => m(LinkAwareEditor, { content: 'x' }) });
    m.redraw.sync();
    expect(captured?.onLinkContextMenu).toBeUndefined();
  });

  it('closes the menu from the backdrop', async () => {
    m.mount(root, {
      view: () => m(LinkAwareEditor, { content: 'x', links: handlers(pathLink) }),
    });
    m.redraw.sync();
    await rightClick({
      kind: 'path',
      path: 'a.txt',
      line: undefined,
      column: undefined,
      from: 0,
      to: 5,
    });
    (root.querySelector('.fm-link-menu-backdrop') as HTMLElement).click();
    m.redraw.sync();
    expect(root.querySelector('.fm-link-menu')).toBeNull();
  });
});
