import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { FindFilesSearchParams } from './find-files-dialog';
import { FindFilesDialog } from './find-files-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('FindFilesDialog', () => {
  it('is focused on open and submits the trimmed query with Enter', async () => {
    const onSearch = vi.fn();
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel,
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    expect(document.activeElement).toBe(input);
    if (!input) throw new Error('input missing');
    expect(input).toBeInstanceOf(HTMLInputElement);
    expect(input.type).toBe('text');
    expect(input.classList).toContain('browser-default');

    input.value = '  report  ';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).toHaveBeenCalledWith({
      filenameQuery: 'report',
      contentQuery: undefined,
      contentRegex: false,
      recurse: true,
    });
    expect(document.activeElement).not.toBe(input);

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('does not search on an empty/whitespace-only query', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });

    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');

    input.value = '   ';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).not.toHaveBeenCalled();
  });

  it('uses the filename query as the default saved-search name', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const query = document.querySelector<HTMLInputElement>('#find-files-query');
    const savedName = document.querySelector<HTMLInputElement>(
      'input[placeholder="Saved search name"]',
    );
    if (!query || !savedName) throw new Error('search inputs missing');

    query.value = '*.md, *.pdf';
    query.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(savedName.value).toBe('*.md, *.pdf');
  });

  it('leaves native text-editing shortcuts to the filename input', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');
    const documentKeydown = vi.fn();
    document.addEventListener('keydown', documentKeydown);

    for (const event of [
      new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'End', bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'a', metaKey: true, bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'c', metaKey: true, bubbles: true, cancelable: true }),
      new KeyboardEvent('keydown', { key: 'x', metaKey: true, bubbles: true, cancelable: true }),
    ]) {
      input.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }

    expect(documentKeydown).not.toHaveBeenCalled();
    document.removeEventListener('keydown', documentKeydown);
  });

  it('searches when only an advanced predicate is configured', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const minimum = document.querySelector<HTMLInputElement>('input[type="number"]');
    if (!minimum) throw new Error('minimum size input missing');
    minimum.value = '1024';
    minimum.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    const searchButton = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent?.trim() === 'Search',
    );
    expect(searchButton?.disabled).toBe(false);
    searchButton?.click();

    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({ filenameQuery: '', minSizeBytes: 1024 }),
    );
  });

  it('keeps results out of the modal because they render in the active pane', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(document.querySelector('.fm-find-files-results')).toBeNull();
    expect(document.querySelector('.fm-find-files-modal')?.id).toBe('find-files-dialog');
    expect(
      [...document.querySelectorAll('.fm-find-files-modal .modal-footer button')].every((button) =>
        button.classList.contains('btn-flat'),
      ),
    ).toBe(true);
  });

  it('blurs before cancel when Cancel is clicked', () => {
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel,
        }),
    });
    m.redraw.sync();
    const buttons = [...document.querySelectorAll('button')];
    const cancelButton = buttons.find((button) => button.textContent?.trim() === 'Cancel');
    if (!cancelButton) throw new Error('cancel button missing');

    cancelButton.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    expect(onCancel).toHaveBeenCalledOnce();
  });

  it('keeps the previous query on reopen, fully selected, so typing replaces it and Enter re-searches', () => {
    let open = true;
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!input) throw new Error('input missing');

    input.value = '*.svg';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const expected: FindFilesSearchParams = {
      filenameQuery: '*.svg',
      contentQuery: undefined,
      contentRegex: false,
      recurse: true,
    };
    expect(onSearch).toHaveBeenCalledWith(expected);

    // Simulate the parent closing the dialog after a successful search, then reopening it
    // for a second search (e.g. via Alt+F7 again).
    open = false;
    m.redraw.sync();
    open = true;
    m.redraw.sync();

    expect(input.value).toBe('*.svg');
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(0);
    expect(input.selectionEnd).toBe(input.value.length);

    // Pressing Enter immediately re-runs the same search.
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSearch).toHaveBeenCalledTimes(2);
    expect(onSearch).toHaveBeenLastCalledWith(expected);
  });

  it('passes content query and options when content search is used', () => {
    const onSearch = vi.fn();
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const contentInput = document.querySelectorAll<HTMLInputElement>(
      '.fm-find-files-body input',
    )[1];
    if (!contentInput) throw new Error('content input missing');
    contentInput.value = 'TODO';
    contentInput.dispatchEvent(new InputEvent('input', { bubbles: true }));

    // Trigger search from the filename input
    const files = document.querySelector<HTMLInputElement>('#find-files-query');
    if (!files) throw new Error('filename input missing');
    files.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    expect(onSearch).toHaveBeenCalledWith(
      expect.objectContaining({
        filenameQuery: '',
        contentQuery: 'TODO',
        contentRegex: false,
        recurse: true,
      }),
    );
  });

  it('shows recurse and regex toggles', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const options = document.querySelector('.fm-find-files-options');
    expect(options).not.toBeNull();
    expect(options?.textContent).toContain('Recurse subdirectories');
    expect(options?.textContent).toContain('Use regex');
  });

  it('opens a saved search in either pane or a new tab and exposes pin/delete controls', () => {
    const onOpenSaved = vi.fn();
    const onToggleSavedPin = vi.fn();
    const onDeleteSaved = vi.fn();
    const saved = {
      id: '11111111-1111-4111-8111-111111111111',
      name: 'Large videos',
      pinned: true,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Videos' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: ['video/*'],
        minSizeBytes: 1_000_000,
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    };
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [saved],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
          onOpenSaved,
          onToggleSavedPin,
          onDeleteSaved,
        }),
    });
    m.redraw.sync();

    const action = (label: string) =>
      document.querySelector<HTMLButtonElement>(`.fm-saved-search [aria-label="${label}"]`);
    action('Open in current pane')?.click();
    action('Open in other pane')?.click();
    action('Open in new tab')?.click();
    action('Remove from favourites')?.click();
    action('Delete saved search')?.click();

    expect(onOpenSaved.mock.calls.map((call) => call[1])).toEqual([
      'currentPane',
      'otherPane',
      'newTab',
    ]);
    expect(onToggleSavedPin).toHaveBeenCalledWith(saved.id);
    expect(onDeleteSaved).toHaveBeenCalledWith(saved.id);
    expect(document.querySelector('.fm-icon-star-filled')).not.toBeNull();
    expect(document.querySelectorAll('.fm-saved-search-actions button')).toHaveLength(6);
  });

  it('does not retain the edited search id after cancellation', () => {
    const onSave = vi.fn();
    const saved = {
      id: '11111111-1111-4111-8111-111111111111',
      name: 'Large videos',
      pinned: false,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Videos' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: ['video/*'],
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    };
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [saved],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
          onSave,
        }),
    });
    m.redraw.sync();
    const button = (label: string) =>
      [...document.querySelectorAll<HTMLButtonElement>('button')].find(
        (candidate) => candidate.textContent?.trim() === label,
      );
    document.querySelector<HTMLButtonElement>('[aria-label="Edit saved search"]')?.click();
    button('Cancel')?.click();

    const name = document.querySelector<HTMLInputElement>('input[placeholder="Saved search name"]');
    if (!name) throw new Error('saved search name input missing');
    name.value = 'Documents';
    name.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    button('Save search')?.click();

    expect(onSave).toHaveBeenCalledWith(
      'Documents',
      expect.objectContaining({ filenameQuery: '' }),
      undefined,
    );
  });

  it('keeps a long saved-search collection in a dedicated scroll region', () => {
    const savedSearches = Array.from({ length: 30 }, (_, index) => ({
      id: `11111111-1111-4111-8111-${index.toString().padStart(12, '0')}`,
      name: `Saved search ${index + 1}`,
      pinned: false,
      query: {
        schemaVersion: 1 as const,
        scope: {
          locations: [{ providerId: 'local', uri: 'file:///Documents' }],
          recurse: true,
          showHidden: false,
        },
        entryKinds: ['file' as const],
        mimeTypes: [],
        gitStatuses: [],
        tags: [],
        metadata: {},
      },
    }));
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches,
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const list = document.querySelector<HTMLElement>('.fm-saved-search-list');
    expect(list?.children).toHaveLength(30);
    expect(list?.classList.contains('fm-saved-search-list')).toBe(true);
  });

  it('uses the same flat-button treatment for disabled search and save actions', () => {
    m.mount(root, {
      view: () =>
        m(FindFilesDialog, {
          open: true,
          scopeLabel: 'file:///Documents',
          savedSearches: [],
          onSearch: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const save = document.querySelector<HTMLButtonElement>('.fm-save-search-button');
    const search = [...document.querySelectorAll<HTMLButtonElement>('.modal-footer button')].find(
      (button) => button.textContent?.trim() === 'Search',
    );
    expect(save?.disabled).toBe(true);
    expect(search?.disabled).toBe(true);
    expect(save?.classList.contains('btn-flat')).toBe(true);
    expect(search?.classList.contains('btn-flat')).toBe(true);
  });
});
