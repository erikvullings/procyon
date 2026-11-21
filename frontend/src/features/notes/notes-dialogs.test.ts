import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { NotesController } from './notes-controller';
import { NotesDialogs } from './notes-dialogs';
import type { NotesState } from './notes-state';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
  document.body.innerHTML = '';
});

function controller(): NotesController {
  return {
    openSetup: vi.fn(async () => undefined),
    setSetupFolder: vi.fn(),
    setSetupIndex: vi.fn(),
    confirmSetup: vi.fn(async () => undefined),
    finishEnrolment: vi.fn(async () => undefined),
    saveTerminalSession: vi.fn(async () => undefined),
    showProposal: vi.fn(async () => undefined),
    editReview: vi.fn(),
    saveReview: vi.fn(async () => undefined),
    close: vi.fn(),
  };
}

function mount(state: NotesState, notes: NotesController): void {
  m.mount(root, {
    view: () =>
      m(NotesDialogs, {
        state,
        controller: notes,
        client: {} as never,
        workspaceId: 'ws-1',
      }),
  });
  m.redraw.sync();
}

const ROOT = { providerId: 'local', uri: 'file:///notes' };

describe('NotesDialogs', () => {
  it('renders nothing while closed', () => {
    mount({ phase: 'closed' }, controller());
    expect(document.querySelector('.fm-notes-modal')).toBeNull();
  });

  it('lets the folder be edited and the indexing choice be made in setup', () => {
    const notes = controller();
    mount(
      {
        phase: 'setup',
        folder: '/Users/me/Notes',
        index: true,
        semanticAvailable: true,
        busy: false,
      },
      notes,
    );
    const input = document.querySelector<HTMLInputElement>('#fm-notes-folder');
    expect(input?.value).toBe('/Users/me/Notes');
    if (!input) throw new Error('folder input missing');
    input.value = '/Users/me/Other';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    expect(notes.setSetupFolder).toHaveBeenCalledWith('/Users/me/Other');

    const check = document.querySelector<HTMLInputElement>('.fm-notes-check input');
    expect(check?.checked).toBe(true);
    check?.click();
    expect(notes.setSetupIndex).toHaveBeenCalledWith(false);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(notes.confirmSetup).toHaveBeenCalledOnce();
  });

  it('explains text search when semantic search is not installed', () => {
    mount(
      { phase: 'setup', folder: '', index: false, semanticAvailable: false, busy: false },
      controller(),
    );
    expect(document.querySelector('.fm-notes-check')).toBeNull();
    expect(document.body.textContent).toContain('searched by text');
  });

  it('warns about redactions and shows the changes of an update', () => {
    const notes = controller();
    mount(
      {
        phase: 'review',
        root: ROOT,
        title: 'Nginx',
        tags: 'nginx',
        fileName: 'ops/nginx.md',
        fileNameEdited: false,
        content: '# Nginx\nnew line\n',
        redactions: 2,
        view: 'changes',
        busy: false,
        replace: {
          path: 'ops/nginx.md',
          location: ROOT,
          revision: 'r1',
          original: '# Nginx\nold line\n',
        },
      },
      notes,
    );
    expect(document.querySelector('.fm-notes-warning')?.textContent).toContain('2 values');
    expect(document.querySelector('#fm-note-file')?.hasAttribute('disabled')).toBe(true);
    const removed = document.querySelector('.fm-notes-diff-line--removed')?.textContent;
    const added = document.querySelector('.fm-notes-diff-line--added')?.textContent;
    expect(removed).toContain('old line');
    expect(added).toContain('new line');

    const title = document.querySelector<HTMLInputElement>('#fm-note-title');
    if (!title) throw new Error('title input missing');
    title.value = 'Nginx TLS';
    title.dispatchEvent(new InputEvent('input', { bubbles: true }));
    expect(notes.editReview).toHaveBeenCalledWith({ title: 'Nginx TLS' });
  });

  it('previews Markdown without running scripts', () => {
    mount(
      {
        phase: 'review',
        root: ROOT,
        title: 'T',
        tags: '',
        fileName: 't.md',
        fileNameEdited: false,
        content: '# Heading\n<script>window.hacked = true</script>',
        redactions: 0,
        view: 'preview',
        busy: false,
      },
      controller(),
    );
    const preview = document.querySelector('.fm-notes-preview');
    expect(preview?.querySelector('h1')?.textContent).toBe('Heading');
    expect(preview?.querySelector('script')).toBeNull();
    expect(document.querySelector('.fm-notes-warning')).toBeNull();
  });
});
