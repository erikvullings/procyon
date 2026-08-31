import m from 'mithril';
import { ThemeManager, Toast } from 'mithril-materialized';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFileManagerClient } from '../api/client/create-client';
import { MockFileManagerClient } from '../api/client/mock-file-manager-client';
import { ApiError } from '../api/fetch-mutator';
import type { Location, Operation } from '../models';
import {
  AppShell,
  locationForPath,
  removeDiskUsageNodes,
  respectSystemLocationReadOnly,
} from './app-shell';

const DISMISSED_OPERATIONS_STORAGE_KEY = 'fm.dismissedOperationIds';

describe('respectSystemLocationReadOnly', () => {
  it('makes a detected read-only network mount non-writable', () => {
    const mount = { providerId: 'local', uri: 'file:///Volumes/Reference' } as const;
    const location = { providerId: 'local', uri: 'file:///Volumes/Reference/Manuals' } as const;
    expect(
      respectSystemLocationReadOnly(
        { state: { type: 'loaded' }, entries: [], location, writable: true, hasMore: false },
        [
          {
            name: 'Reference',
            kind: 'network',
            location: mount,
            protocol: 'smb',
            readOnly: true,
          },
        ],
      ).writable,
    ).toBe(false);
  });
});

describe('locationForPath', () => {
  const archive = {
    providerId: 'archive',
    uri: 'archive:///home/erik/My%20Comic.zip!/chapter',
  } as const;

  it('maps outer archive breadcrumbs back to local filesystem locations', () => {
    expect(locationForPath(archive, '/home/erik')).toEqual({
      providerId: 'local',
      uri: 'file:///home/erik',
    });
  });

  it('maps the archive and inner breadcrumbs to archive locations', () => {
    expect(locationForPath(archive, '/home/erik/My Comic.zip!')).toEqual({
      providerId: 'archive',
      uri: 'archive:///home/erik/My%20Comic.zip!/',
    });
    expect(locationForPath(archive, '/home/erik/My Comic.zip!/chapter')).toEqual({
      providerId: 'archive',
      uri: 'archive:///home/erik/My%20Comic.zip!/chapter',
    });
  });

  const local = { providerId: 'local', uri: 'file:///Users/erik/projects' } as const;

  it('expands a bare ~ to the home directory when known', () => {
    expect(locationForPath(local, '~', '/Users/erik')).toEqual({
      providerId: 'local',
      uri: 'file:///Users/erik',
    });
  });

  it('expands ~/... to a path under the home directory', () => {
    expect(locationForPath(local, '~/.codex', '/Users/erik')).toEqual({
      providerId: 'local',
      uri: 'file:///Users/erik/.codex',
    });
  });

  it('leaves a leading ~ untouched when the home directory is unknown', () => {
    expect(locationForPath(local, '~/.codex')).toEqual({
      providerId: 'local',
      uri: 'file:///~/.codex',
    });
  });

  it('does not expand ~ for a non-local provider, which may have its own convention', () => {
    const sftp = { providerId: 'sftp', uri: 'sftp://host/home/erik' } as const;
    expect(locationForPath(sftp, '~/uploads', '/Users/erik')).toEqual({
      providerId: 'sftp',
      uri: 'sftp://host/~/uploads',
    });
  });
});

let root: HTMLElement;

class TestEventSource extends EventTarget {
  close(): void {}
}

function mountShell(runtime: 'http' | 'tauri' | 'mock' = 'http'): void {
  m.mount(root, { view: () => m(AppShell, { runtime, client: createFileManagerClient(runtime) }) });
}

function directoryRowNamed(
  container: ParentNode | null | undefined,
  name: string,
): HTMLElement | undefined {
  if (container === null || container === undefined) return undefined;
  return [...container.querySelectorAll<HTMLElement>('.fm-directory-row')].find(
    (row) => row.querySelector('.fm-entry-name [title]')?.getAttribute('title') === name,
  );
}

/**
 * Selects a theme button by its `title` prefix rather than its text: the Auto
 * button renders a ligature icon, so its `textContent` is `brightness_autoAuto`.
 */
function themeButtonIn(container: HTMLElement, label: string): HTMLButtonElement {
  const match = container.querySelector<HTMLButtonElement>(
    `.theme-switcher button[title^="${label}"]`,
  );
  if (!match) {
    throw new Error(`no theme button titled "${label}" in: ${container.innerHTML}`);
  }
  return match;
}

function themeButton(label: string): HTMLButtonElement {
  return themeButtonIn(root, label);
}

/**
 * Opens the settings disclosure and waits for the (async) initial settings
 * load to complete, since the settings editor only renders once
 * `currentSettings` is available (§0083).
 */
async function openAppearanceSettings(container: HTMLElement = root): Promise<void> {
  container.querySelector<HTMLElement>('.fm-settings-button')?.click();
  m.redraw.sync();
  await vi.waitFor(() => expect(container.querySelector('.theme-switcher')).not.toBeNull());
}

/** Opens the workspace switcher disclosure in the toolbar (task 0084). */
async function openWorkspaceSwitcher(container: HTMLElement = root): Promise<void> {
  container.querySelector<HTMLElement>('.fm-workspace-switcher-button')?.click();
  m.redraw.sync();
  await vi.waitFor(() => expect(container.querySelector('.fm-workspace-switcher')).not.toBeNull());
}

async function openOperationCentre(container: HTMLElement = root): Promise<void> {
  await vi.waitFor(() =>
    expect(
      container.querySelector<HTMLButtonElement>('.fm-operation-centre-button')?.disabled,
    ).toBe(false),
  );
  container.querySelector<HTMLButtonElement>('.fm-operation-centre-button')?.click();
  m.redraw.sync();
  await vi.waitFor(() =>
    expect(
      container
        .querySelector<HTMLButtonElement>('.fm-operation-centre-button')
        ?.getAttribute('aria-pressed'),
    ).toBe('true'),
  );
}

beforeEach(() => {
  vi.stubGlobal('EventSource', TestEventSource);
  globalThis.localStorage?.removeItem(DISMISSED_OPERATIONS_STORAGE_KEY);
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  vi.unstubAllGlobals();
  m.mount(root, null);
  root.remove();
  globalThis.localStorage?.removeItem(DISMISSED_OPERATIONS_STORAGE_KEY);
  document.documentElement.removeAttribute('data-theme');
});

describe('AppShell', () => {
  it('shows the directory table and loads the mock root directory', async () => {
    mountShell('mock');

    expect(root.textContent).not.toContain('Shell only');

    await vi.waitFor(() => {
      expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(2);
      expect(root.textContent).toContain('Documents');
      expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined();
    });
    expect(root.querySelector('.fm-pane-tabs')).not.toBeNull();
    expect(root.querySelector('.fm-breadcrumb')).not.toBeNull();
    expect(root.querySelector('.fm-pane-status')?.textContent).not.toBeNull();
  });

  it('selects a row and opens its directory with Enter', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const documents = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((row) =>
      row.textContent?.includes('Documents'),
    );
    documents?.click();
    m.redraw.sync();
    const activePane = documents?.closest<HTMLElement>('.fm-pane');
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() => expect(directoryRowNamed(activePane, 'report.pdf')).not.toBeUndefined());
    expect(activePane?.querySelector('.fm-cursor-row')?.textContent).toContain('Projects');
    // Landing on a freshly entered directory positions the keyboard cursor only - it must not
    // also select the entry (selecting is a deliberate user action, not a side effect of simply
    // arriving somewhere). Single-selection actions like F3 already act on the cursor entry
    // "regardless of the wider selection" (see the core.view case in global-keydown-handler.ts).
    expect(activePane?.querySelector('.fm-selected-row')).toBeNull();
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    m.redraw.sync();
    expect(directoryRowNamed(activePane, 'report.pdf')?.classList.contains('fm-cursor-row')).toBe(
      true,
    );
  });

  it('keeps the directory-tree sidebar in sync when the active pane navigates by other means (table Enter)', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', altKey: true, bubbles: true }));
    m.redraw.sync();
    await vi.waitFor(() => expect(root.querySelector('.fm-directory-tree')).not.toBeNull());

    const documents = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((rowEl) =>
      rowEl.textContent?.includes('Documents'),
    );
    documents?.click();
    m.redraw.sync();
    documents
      ?.closest<HTMLElement>('.fm-pane')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // The tree, driven purely by table navigation (not by clicking the tree itself), expands to
    // reveal and select the new active location without the user ever touching the sidebar.
    await vi.waitFor(() => {
      const selected = root.querySelector('.fm-directory-tree .fm-tree-row-selected');
      expect(selected?.textContent).toContain('Documents');
    });
  });

  it('Alt+F10 toggles the directory-tree sidebar, which navigates the active pane on activation', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    expect(root.querySelector('.fm-directory-tree')).toBeNull();
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', altKey: true, bubbles: true }));
    m.redraw.sync();

    await vi.waitFor(() => {
      expect(root.querySelector('.fm-directory-tree')).not.toBeNull();
    });
    // The active pane's current location (mock:///) is the tree's root itself here, so only the
    // root row is shown until it is expanded (lazy expansion) - it must be selected, though.
    expect(root.querySelector('.fm-directory-tree .fm-tree-row-selected')).not.toBeNull();
    // Opening the sidebar moves DOM focus straight into it (scheduled via `requestAnimationFrame`,
    // matching the terminal drawer's own focus-on-open pattern), so arrow-key navigation works
    // immediately without an extra click.
    const tree = root.querySelector<HTMLElement>('.fm-directory-tree');
    await vi.waitFor(() => expect(document.activeElement).toBe(tree));

    // The root row has no expand-toggle affordance (it saves horizontal space instead), so
    // expanding it goes through the keyboard, the same way any other node's ArrowRight would.
    tree?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    await vi.waitFor(() => {
      expect(root.querySelector('.fm-directory-tree')?.textContent).toContain('Documents');
    });

    const documentsRow = [...root.querySelectorAll<HTMLElement>('.fm-tree-row')].find((rowEl) =>
      rowEl.textContent?.includes('Documents'),
    );
    documentsRow?.click();
    m.redraw.sync();

    const activePane = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    await vi.waitFor(() => expect(activePane?.textContent).toContain('Projects'));

    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', altKey: true, bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-directory-tree')).toBeNull();
  });

  it('places the cursor on the ".." row when entering an empty directory', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.textContent).toContain('Empty'));
    const empty = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((row) =>
      row.textContent?.includes('Empty'),
    );
    empty?.click();
    m.redraw.sync();
    const activePane = empty?.closest<HTMLElement>('.fm-pane');
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // The directory itself has no entries, but the table still renders a synthetic ".." row
    // (there's nothing else to land the keyboard cursor on) - regression test for the cursor
    // silently disappearing instead of landing there.
    await vi.waitFor(() =>
      expect(activePane?.querySelector('.fm-cursor-row')?.textContent).toContain('..'),
    );
  });

  it('keeps keyboard focus and the active pane together after Tab', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(2));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    left?.focus();

    left?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    m.redraw.sync();

    expect(document.activeElement?.closest('[data-pane-id]')?.getAttribute('data-pane-id')).toBe(
      'right',
    );
    expect(root.querySelector('[data-pane-id="right"]')?.getAttribute('data-active')).toBe('true');
  });

  it('focuses the active pane when the workspace first appears', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(2));

    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    await vi.waitFor(() => expect(document.activeElement).toBe(activePane));
  });

  it('restores keyboard focus to the active pane when the window regains focus', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(2));
    const left = root.querySelector<HTMLElement>('[data-pane-id="left"] > .fm-pane');
    left?.focus();
    expect(document.activeElement).toBe(left);

    // Simulates alt-tabbing away and back: DOM focus is left wherever the OS/browser happened to
    // put it (here, nothing - the same as a real app losing then regaining window focus).
    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.activeElement).not.toBe(left);

    window.dispatchEvent(new Event('focus'));
    m.redraw.sync();

    expect(document.activeElement).toBe(left);
  });

  it('End loads every remaining page and moves the cursor to the true last entry', async () => {
    const client = new MockFileManagerClient({ pageSize: 100 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    if (activePane === null) throw new Error('no active pane');

    activePane
      .querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000000'));
    // Only the first page (100 of 1000) should be loaded before pressing End.
    expect(activePane.querySelectorAll('.fm-directory-row').length).toBeLessThan(200);

    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000999'));
    expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain('generated-0000999');

    // No entry follows the true last one, so ArrowDown must be a no-op.
    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    m.redraw.sync();
    expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain('generated-0000999');
  });

  it('does not snap the cursor back to the last entry if ArrowUp is pressed while End is still loading pages', async () => {
    // Regression test: End on a directory with unloaded pages kicks off a background
    // `loadAllPages` fetch and only lands the cursor on the true last entry once it resolves. If
    // the user presses ArrowUp in the meantime, that newer action must win — the background
    // resolution must not overwrite it and silently snap the cursor back to the last entry.
    const client = new MockFileManagerClient({ pageSize: 100, latencyMs: 20 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    if (activePane === null) throw new Error('no active pane');

    activePane
      .querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000000'));

    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    // The background load hasn't resolved yet (each page is delayed by `latencyMs`), so the
    // cursor is still wherever it was before End — pressing ArrowUp now moves it from there.
    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }));
    m.redraw.sync();
    const cursorAfterArrowUp = activePane.querySelector('.fm-cursor-row')?.textContent;
    expect(cursorAfterArrowUp).not.toContain('generated-0000999');

    // Give the background load every chance to resolve (10 pages * 20ms latency) and redraw.
    await new Promise((resolve) => setTimeout(resolve, 500));
    m.redraw.sync();

    expect(activePane.querySelector('.fm-cursor-row')?.textContent).toBe(cursorAfterArrowUp);
    expect(activePane.querySelector('.fm-cursor-row')?.textContent).not.toContain(
      'generated-0000999',
    );
  });

  it('End on a directory large enough to use the responsive background sort still highlights the true last entry', async () => {
    const client = new MockFileManagerClient({ pageSize: 1_000 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    if (activePane === null) throw new Error('no active pane');

    activePane
      .querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/10000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000000'));

    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0009999'), {
      timeout: 10_000,
    });
    await vi.waitFor(
      () =>
        expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain(
          'generated-0009999',
        ),
      { timeout: 10_000 },
    );

    // No entry follows the true last one, so ArrowDown must be a no-op.
    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    m.redraw.sync();
    expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain('generated-0009999');
  }, 15_000);

  it('type-to-select still background-loads the rest of the directory even when the prefix already matches a loaded entry', async () => {
    // Regression test for the reported bug: typing a prefix that already matches some entries
    // among the first page (so a selection happens immediately) must still keep searching the
    // rest of the directory in the background, instead of only ever considering the loaded page.
    const client = new MockFileManagerClient({ pageSize: 100 });
    const listDirectorySpy = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    if (activePane === null) throw new Error('no active pane');

    activePane
      .querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000000'));
    // Only the first of 10 pages (1000 entries / 100 per page) has been fetched so far.
    const callsAfterInitialLoad = listDirectorySpy.mock.calls.length;

    // "0" matches "generated-0000000" immediately, among the first (already loaded) page.
    activePane.dispatchEvent(new KeyboardEvent('keydown', { key: '0', bubbles: true }));
    m.redraw.sync();
    expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain('generated-0000000');

    // Every remaining page must still be fetched in the background, so entries further in
    // (matching this same prefix, or a later one) are eventually reachable too.
    await vi.waitFor(() =>
      expect(listDirectorySpy.mock.calls.length).toBeGreaterThan(callsAfterInitialLoad),
    );
  });

  it('type-to-select finds an entry only present on an unloaded page by background-loading the rest of the directory', async () => {
    // Regression test: type-to-select used to only search entries loaded so far. Typing a prefix
    // that matches nothing among the first page must background-load every remaining page and
    // retry, exactly like End does for the true last entry.
    const client = new MockFileManagerClient({ pageSize: 100 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    if (activePane === null) throw new Error('no active pane');

    activePane
      .querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000000'));
    // Only the first page (100 of 1000) should be loaded so far.
    expect(activePane.textContent).not.toContain('generated-0000999');

    for (const typed of '0000999') {
      activePane.dispatchEvent(new KeyboardEvent('keydown', { key: typed, bubbles: true }));
    }
    m.redraw.sync();

    await vi.waitFor(() => expect(activePane.textContent).toContain('generated-0000999'));
    await vi.waitFor(() =>
      expect(activePane.querySelector('.fm-cursor-row')?.textContent).toContain(
        'generated-0000999',
      ),
    );
  });

  it('keeps cursor and selection independent while using keyboard selection', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    activePane?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'a', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    const entryCount = activePane?.querySelectorAll('.fm-directory-row').length ?? 0;
    expect(activePane?.querySelectorAll('.fm-selected-row')).toHaveLength(entryCount);
    const cursorBefore = activePane?.querySelector('.fm-cursor-row')?.textContent;

    // Plain ArrowDown moves the cursor but, per the current selection model, deliberately keeps
    // an existing multi-selection intact rather than collapsing it to the new cursor row alone
    // (see the "Keep an existing multi-selection when navigating without Shift" comment in
    // reduceSelection's moveCursor case) - cursor and selection are independent axes.
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    m.redraw.sync();
    expect(activePane?.querySelectorAll('.fm-selected-row')).toHaveLength(entryCount);
    expect(activePane?.querySelector('.fm-cursor-row')?.textContent).not.toBe(cursorBefore);

    // Space toggles the entry under the cursor (and then advances the cursor, Total Commander
    // parity) - every rendered row stays selected except the one just toggled off. Re-querying
    // the rendered row count after the keypress (rather than reusing `entryCount`) keeps this
    // assertion robust to the cursor's advance changing which rows are virtualized into the DOM.
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    m.redraw.sync();
    const renderedAfterSpace = activePane?.querySelectorAll('.fm-directory-row').length ?? 0;
    expect(activePane?.querySelectorAll('.fm-selected-row')).toHaveLength(renderedAfterSpace - 1);
  });

  it('sorts the loaded page from a column header and reports the active direction', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const nameHeader = activePane?.querySelector<HTMLButtonElement>('[data-column-id="core.name"]');
    expect(nameHeader?.getAttribute('aria-sort')).toBe('ascending');

    nameHeader?.click();

    await vi.waitFor(() => expect(nameHeader?.getAttribute('aria-sort')).toBe('descending'));
  });

  it('shows a parent row outside the root and opens it with Enter', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const documents = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((row) =>
      row.textContent?.includes('Documents'),
    );
    documents?.click();
    m.redraw.sync();
    const activePane = documents?.closest<HTMLElement>('.fm-pane');
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() =>
      expect(activePane?.querySelector('.fm-directory-row')?.textContent).toContain('..'),
    );
    activePane?.querySelector<HTMLElement>('.fm-directory-row')?.click();
    m.redraw.sync();
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() => expect(directoryRowNamed(activePane, 'report.pdf')).toBeUndefined());
    expect(activePane?.textContent).toContain('Documents');
    expect(activePane?.querySelector('.fm-directory-row')?.textContent).not.toContain('..');
    // Navigating back up must position the keyboard cursor on the child directory just exited,
    // but not also select it - landing somewhere is not a selection action.
    expect(activePane?.querySelector('.fm-cursor-row')?.textContent).toContain('Documents');
    expect(activePane?.querySelector('.fm-selected-row')).toBeNull();
  });

  it('composes the complete main-window workspace regions', async () => {
    mountShell('mock');

    await vi.waitFor(() => expect(root.querySelectorAll('.fm-workspace-pane')).toHaveLength(2));
    expect(root.querySelector('.fm-app-bar')).toBeNull();
    expect(root.querySelector('.fm-workspace-toolbar')).not.toBeNull();
    expect(root.querySelector('.fm-navigation-controls')).not.toBeNull();
    expect(root.querySelector('.fm-operation-centre')).toBeNull();
    expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F5 Copy');
    expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F6 Move');
  });

  it('toggles and persists the hidden operation centre from the toolbar and Alt+Z', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const workspaceId = (await client.listWorkspaces())[0]?.id;
    if (workspaceId === undefined) throw new Error('no active workspace');

    expect(
      root
        .querySelector<HTMLButtonElement>('.fm-operation-centre-button')
        ?.getAttribute('aria-pressed'),
    ).toBe('false');

    await openOperationCentre();
    expect(root.querySelector('.fm-operation-centre')?.textContent).toContain(
      'No operations to show.',
    );
    await vi.waitFor(async () =>
      expect((await client.getWorkspace(workspaceId)).operationCentre.visible).toBe(true),
    );

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'z', altKey: true, bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() =>
      expect(
        root
          .querySelector<HTMLButtonElement>('.fm-operation-centre-button')
          ?.getAttribute('aria-pressed'),
      ).toBe('false'),
    );
    await vi.waitFor(async () =>
      expect((await client.getWorkspace(workspaceId)).operationCentre.visible).toBe(false),
    );
  });

  it('previews modified function-key commands while a modifier is held', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    vi.spyOn(client, 'listActions').mockImplementation(async () => {
      const actions = await new MockFileManagerClient().listActions();
      return [
        ...actions.filter((action) => action.id !== 'core.quickLook'),
        {
          id: 'core.quickLook',
          title: 'Quick Look',
          category: 'fileOperations',
          defaultShortcuts: [{ key: 'F3', shift: true }],
          contextRequirements: { featureAvailable: true, requiresSingleSelection: true },
          source: { kind: 'core' },
        },
      ];
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F3 View'),
    );
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Shift', shiftKey: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F3 Quick Look'),
    );

    document.dispatchEvent(new KeyboardEvent('keyup', { key: 'Shift' }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F3 View'),
    );

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Alt', altKey: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain(
        'F3 Open Externally',
      ),
    );
    const externalOpenKey = [...root.querySelectorAll<HTMLElement>('.fm-function-key')].find(
      (span) => span.textContent?.includes('F3 Open Externally'),
    );
    externalOpenKey?.click();
    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Shift', altKey: true, shiftKey: true }),
    );
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F4 External Edit'),
    );

    window.dispatchEvent(new Event('blur'));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-function-key-bar')?.textContent).toContain('F3 View'),
    );
  });

  it('opens the command palette with Ctrl+P, supports keyboard invocation, and restores focus', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    const trigger = root.querySelector<HTMLButtonElement>(
      '.fm-workspace-toolbar .fm-command-palette-trigger',
    );
    trigger?.focus();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    const input = root.querySelector<HTMLInputElement>('.fm-command-palette-input');
    expect(input).not.toBeNull();
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    // "Calculate Folder Size" (task 0071) sorts alphabetically first among the unfiltered,
    // always-available action list, so one ArrowDown from the palette's initial state now lands
    // there instead of "Clear selection".
    expect(invokeAction).toHaveBeenCalledWith(
      expect.objectContaining({ actionId: 'core.calculateFolderSize' }),
    );

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    root
      .querySelector('.fm-command-palette-input')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector('.fm-command-palette')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('toggles the directory-tree sidebar from the command palette (task 0139)', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'p', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    const input = root.querySelector<HTMLInputElement>('.fm-command-palette-input');
    expect(input).not.toBeNull();
    if (input !== null) {
      input.value = 'directory tree';
      input.dispatchEvent(new Event('input'));
    }
    m.redraw.sync();
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // A purely frontend UI toggle - never reaches the backend action-invoke endpoint.
    expect(invokeAction).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(root.querySelector('.fm-directory-tree')).not.toBeNull());
    expect(root.querySelector('.fm-command-palette')).toBeNull();
  });

  it('opens F7 validation and selects the directory after a reset refresh', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F7', bubbles: true }));
    m.redraw.sync();
    const input = document.querySelector<HTMLInputElement>('#create-directory-name');
    expect(document.activeElement).toBe(input);
    if (!input) throw new Error('create-directory input missing');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(startOperation).not.toHaveBeenCalled();
    input.value = 'New folder';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() => expect(root.textContent).toContain('Create archive'));
    [...root.querySelectorAll<HTMLButtonElement>('button')]
      .find((button) => button.textContent?.trim() === 'Create')
      ?.click();
    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    const request = startOperation.mock.calls[0]?.[0];
    expect(request).toMatchObject({
      type: 'createDirectory',
      name: 'New folder',
      createIntermediateDirectories: false,
    });
    const workspace = await client.getWorkspace((await client.listWorkspaces())[0]?.id ?? '');
    const paneId = workspace.activePaneId;
    const snapshot = await client.listDirectory({
      workspaceId: workspace.id,
      paneId,
      requestId: 'selection-test',
      location: request?.destination ?? { providerId: 'file', uri: 'mock:///' },
    });
    client.emit({
      eventId: 99,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'directory.delta',
        paneId,
        delta: {
          type: 'reset',
          snapshot: {
            ...snapshot,
            revision: snapshot.revision + 1,
            entries: [
              ...snapshot.entries,
              {
                id: 'created-folder',
                name: 'New folder',
                kind: 'directory',
                location: {
                  providerId: request?.destination?.providerId ?? 'file',
                  uri: `${request?.destination?.uri ?? 'mock://'}New%20folder`,
                },
                hidden: false,
                readOnly: false,
                metadataRevision: 0,
              },
            ],
          },
        },
      },
    });

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-selected-row')?.textContent).toContain('New folder'),
    );
  });

  it('moves the cursor to the preceding file after F6 removes its entry', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const rows = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])];
    const target = rows[2];
    const precedingName = rows[1]?.querySelector('[role="gridcell"]')?.textContent?.trim();
    const targetName = target?.querySelector('[role="gridcell"]')?.textContent?.trim();
    expect(precedingName).toBeTruthy();
    expect(targetName).toBeTruthy();
    target?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F6', bubbles: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    const request = startOperation.mock.calls[0]?.[0];
    if (request === undefined) throw new Error('move request missing');
    expect(request).toMatchObject({ type: 'move' });
    const movedUri = request?.type === 'move' ? request.sources[0]?.uri : undefined;
    const workspace = await client.getWorkspace((await client.listWorkspaces())[0]?.id ?? '');
    const paneId = workspace.activePaneId;
    const snapshot = await client.listDirectory({
      workspaceId: workspace.id,
      paneId,
      requestId: 'move-cursor-test',
      location: snapshotLocation(request),
    });
    client.emit({
      eventId: 100,
      timestamp: '2026-07-31T12:00:01Z',
      payload: {
        type: 'directory.delta',
        paneId,
        delta: {
          type: 'reset',
          snapshot: {
            ...snapshot,
            revision: snapshot.revision + 1,
            entries: snapshot.entries.filter((entry) => entry.location.uri !== movedUri),
          },
        },
      },
    });

    await vi.waitFor(() =>
      expect(
        activePane?.querySelector('.fm-cursor-row [role="gridcell"]')?.textContent?.trim(),
      ).toBe(precedingName),
    );
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    await vi.waitFor(() =>
      expect(
        activePane?.querySelector('.fm-cursor-row [role="gridcell"]')?.textContent?.trim(),
      ).toBe(precedingName),
    );
  });

  function snapshotLocation(request: Parameters<MockFileManagerClient['startOperation']>[0]) {
    if (request.type !== 'move') return { providerId: 'file', uri: 'mock:///' };
    const source = request.sources[0];
    if (source === undefined) return { providerId: 'file', uri: 'mock:///' };
    return {
      providerId: source.providerId,
      uri: source.uri.slice(0, Math.max(0, source.uri.lastIndexOf('/')) + 1),
    };
  }

  it('copies one selected file to the other pane with F5', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F5', bubbles: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'copy',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
  });

  it('packages the selected entries into a ZIP with Alt+F5', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F5', altKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(root.textContent).toContain('Create archive'));
    document
      .querySelector<HTMLInputElement>('#archive-create-name')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'createArchive',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///archive.zip' },
    });
  });

  it('moves the selected entries into a ZIP with Alt+Shift+F5', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F5', altKey: true, shiftKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(root.textContent).toContain('Move to archive'));
    document
      .querySelector<HTMLInputElement>('#archive-create-name')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'moveToArchive',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///archive.zip' },
    });
  });

  it('copies one selected file to the other pane by clicking the F5 footer hint (Tauri parity fix)', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    m.redraw.sync();
    const footerKey = [...root.querySelectorAll<HTMLElement>('.fm-function-key')].find((span) =>
      span.textContent?.includes('F5 Copy'),
    );
    expect(footerKey).not.toBeUndefined();
    footerKey?.click();

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'copy',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
  });

  it('trashes the selected file with F8 when core.trash owns the shortcut (task 0043)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.trash',
        title: 'Trash',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F8' }, { key: 'Delete' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.delete',
        title: 'Delete',
        category: 'fileOperations',
        defaultShortcuts: [
          { key: 'F8', shift: true },
          { key: 'Delete', shift: true },
        ],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ]);
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F8', bubbles: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'trash',
      sources: [{ uri: 'mock:///.env' }],
      conflictPolicy: 'ask',
    });
  });

  it('scans a .app cursor entry and opens the uninstall review checklist (task 0148)', async () => {
    const client = new MockFileManagerClient();
    const originalListActions = client.listActions.bind(client);
    vi.spyOn(client, 'listActions').mockImplementation(async (...args) => {
      const actions = await originalListActions(...args);
      // Only override this one action's shortcut/availability - replacing the whole list would
      // also drop `core.open`'s Enter shortcut, breaking this test's own directory navigation.
      return actions.map((action) =>
        action.id === 'core.uninstallApplication'
          ? {
              ...action,
              defaultShortcuts: [{ key: 'u', ctrl: true, alt: true }],
              contextRequirements: { requiresSingleSelection: true },
            }
          : action,
      );
    });
    const discover = vi.spyOn(client, 'discoverApplicationUninstallCandidates').mockResolvedValue({
      bundleIdentifier: 'com.example.Widget',
      productName: 'Widget',
      relatedFiles: [
        {
          location: { providerId: 'file', uri: 'mock:///Applications/Widget-support' },
          sizeBytes: 4096,
          removable: true,
        },
      ],
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    const applications = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((row) =>
      row.textContent?.includes('Applications'),
    );
    applications?.click();
    m.redraw.sync();
    const activePane = applications?.closest<HTMLElement>('.fm-pane');
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane?.textContent).toContain('Widget.app'));

    const widget = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('Widget.app'),
    );
    widget?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'u', ctrlKey: true, altKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(discover).toHaveBeenCalledOnce());
    expect(discover.mock.calls[0]?.[0]).toMatchObject({
      location: { uri: 'mock:///Applications/Widget.app' },
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Uninstall Widget'));
    expect(root.textContent).toContain('/Applications/Widget-support');

    // Confirm-then-trash flow (task 0148): confirming the checklist must delete nothing itself -
    // it reuses the exact same Trash-first `startOperation` path as `core.trash`, targeting the
    // bundle plus whatever related file the user left checked.
    const startOperation = vi.spyOn(client, 'startOperation');
    const removeDockIcon = vi.spyOn(client, 'removeApplicationDockIcon');
    const confirmButton = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent === 'Move to Trash',
    );
    confirmButton?.click();

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'trash',
      sources: [
        { uri: 'mock:///Applications/Widget.app' },
        { uri: 'mock:///Applications/Widget-support' },
      ],
    });

    // Best-effort Dock cleanup (task 0148 follow-up): fired alongside the Trash operation, not
    // gating it.
    await vi.waitFor(() => expect(removeDockIcon).toHaveBeenCalledOnce());
    expect(removeDockIcon.mock.calls[0]?.[0]).toMatchObject({
      location: { uri: 'mock:///Applications/Widget.app' },
    });
  });

  it('does nothing for the uninstall shortcut when the cursor entry is not a .app bundle', async () => {
    const client = new MockFileManagerClient();
    const originalListActions = client.listActions.bind(client);
    vi.spyOn(client, 'listActions').mockImplementation(async (...args) => {
      const actions = await originalListActions(...args);
      return actions.map((action) =>
        action.id === 'core.uninstallApplication'
          ? {
              ...action,
              defaultShortcuts: [{ key: 'u', ctrl: true, alt: true }],
              contextRequirements: { requiresSingleSelection: true },
            }
          : action,
      );
    });
    const discover = vi.spyOn(client, 'discoverApplicationUninstallCandidates');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'u', ctrlKey: true, altKey: true, bubbles: true }),
    );

    expect(discover).not.toHaveBeenCalled();
  });

  it('deletes the cursor entry with Shift+F8 even when no explicit selection remains', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.trash',
        title: 'Trash',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F8' }, { key: 'Delete' }],
        contextRequirements: {},
        source: { kind: 'core' },
      },
      {
        id: 'core.delete',
        title: 'Delete',
        category: 'fileOperations',
        defaultShortcuts: [
          { key: 'F8', shift: true },
          { key: 'Delete', shift: true },
        ],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ]);
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    if (file === undefined) throw new Error('fixture file row missing');
    file.click();
    activePane?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
    m.redraw.sync();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F8', shiftKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'delete',
      sources: [{ uri: 'mock:///.env' }],
      conflictPolicy: 'ask',
    });
  });

  it('invokes core.openWith on the selected file with Ctrl+Enter (Marta shortcut, task 0086/0087)', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction.mock.calls[0]?.[0]).toMatchObject({
      actionId: 'core.openWith',
      parameters: { uri: 'mock:///.env' },
    });
  });

  it('invokes the external core.edit action on the selected file with Shift+Alt+F4', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F4', shiftKey: true, altKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction.mock.calls[0]?.[0]).toMatchObject({
      actionId: 'core.edit',
      parameters: { uri: 'mock:///.env' },
    });
  });

  it('does not repurpose Ctrl+F4 as external edit', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F4', ctrlKey: true, bubbles: true }),
    );

    await Promise.resolve();
    expect(invokeAction).not.toHaveBeenCalled();
  });

  it('opens extensionless text files in the opposite pane with F4', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F4', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-file-editor')).not.toBeNull());
    expect(invokeAction).not.toHaveBeenCalled();
  });

  it('opens the Lister viewer in the opposite pane with F3 (task 0088)', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));

    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());
    // The viewer occupies a tab in the OPPOSITE pane, leaving the active pane's directory
    // listing (and its selection) untouched, and never falling back to the OS-open action.
    expect(root.querySelector('[data-active="true"] > .fm-pane .fm-file-viewer')).toBeNull();
    const inactivePane = root.querySelector('[data-active="false"] > .fm-pane');
    expect(inactivePane?.classList.contains('fm-pane-viewer')).toBe(true);
    expect(inactivePane?.querySelectorAll('.fm-pane-tab')).toHaveLength(2);
    expect(inactivePane?.querySelector('.fm-file-viewer')?.textContent).toContain('.env');
    expect(invokeAction).not.toHaveBeenCalled();
  });

  it('opens an external-only F3 video in the OS default player', async () => {
    const client = new MockFileManagerClient();
    const originalListDirectory = client.listDirectory.bind(client);
    vi.spyOn(client, 'listDirectory').mockImplementation(async (request, signal) => {
      const snapshot = await originalListDirectory(request, signal);
      return {
        ...snapshot,
        entries: snapshot.entries.map((entry) =>
          entry.name === '.env'
            ? {
                ...entry,
                id: 'entry-movie',
                name: 'movie.mkv',
                extension: 'mkv',
                size: 3,
                location: { ...entry.location, uri: 'mock:///movie.mkv' },
              }
            : entry,
        ),
      };
    });
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, 'movie.mkv')).not.toBeUndefined());
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = directoryRowNamed(activePane, 'movie.mkv');
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer-open-externally')).not.toBeNull(),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-open-externally')?.click();

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction.mock.calls[0]?.[0]).toMatchObject({
      actionId: 'core.open',
      parameters: { uri: 'mock:///movie.mkv' },
    });
  });

  it('toggles an existing Lister tab for the cursor file and ignores other selected files', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const rows = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])];
    const first = rows.find((row) => row.textContent?.includes('.env'));
    const second = directoryRowNamed(activePane, '日本語.txt');
    first?.click();
    second?.dispatchEvent(new MouseEvent('click', { ctrlKey: true, bubbles: true }));

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());
    expect(root.querySelector('.fm-file-viewer-title')?.textContent).toContain('日本語.txt');

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).toBeNull());
  });

  it('reuses the Lister tab for a different cursor file and toggles the new file closed', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const sourcePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const rows = [...(sourcePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])];
    const first = rows.find((row) => row.textContent?.includes('.env'));
    const second = directoryRowNamed(sourcePane, '日本語.txt');

    first?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer-title')?.textContent).toContain('.env'),
    );

    second?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer-title')?.textContent).toContain('日本語.txt'),
    );
    const viewerPane = root.querySelector<HTMLElement>('.fm-pane-viewer');
    expect(viewerPane?.querySelectorAll('.fm-pane-tab')).toHaveLength(2);

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).toBeNull());
  });

  it('pre-populates and highlights the content-search term in the viewer when F3-ing a content-search result (task 0089 follow-up)', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const filenameInput = root.querySelector<HTMLInputElement>('#find-files-query');
    const contentInput = [...root.querySelectorAll<HTMLInputElement>('input')].find(
      (input) => input.placeholder === 'Text or regex to find in files',
    );
    if (filenameInput === null || contentInput === undefined) {
      throw new Error('find files inputs missing');
    }
    filenameInput.value = '';
    filenameInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    contentInput.value = 'ERROR';
    contentInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    contentInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    // Wait for the search results tab (marked with the search icon, task 0089 follow-up) to
    // replace the dialog, confirming the async startSearch()/navigate() completed.
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-pane-tab-title')?.textContent).toBe('ERROR'),
    );
    expect(root.querySelector('.fm-pane-tab-content-search-icon')).not.toBeNull();
    const activePane = () => root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    expect(
      [...(activePane()?.querySelectorAll('.fm-breadcrumb-segment') ?? [])].map(
        (segment) => segment.textContent,
      ),
    ).toEqual(['/', 'search', 'local', 'content: ERROR']);
    // The search:// pane's first row is a synthetic ".." parent entry (for backing out of the
    // virtual search results location) - skip it to select an actual result.
    const resultRow = () =>
      [...(activePane()?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
        (row) => !row.textContent?.includes('..'),
      );
    await vi.waitFor(() => expect(resultRow()).not.toBeUndefined());
    resultRow()?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));

    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());
    // Assert the search bar is pre-populated with the content-search query
    const searchInput = root.querySelector<HTMLInputElement>('.fm-file-viewer input[type="text"]');
    expect(searchInput).not.toBeNull();
    expect(searchInput?.value).toBe('ERROR');
  });

  it('closes the Lister viewer via its close button', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());

    const viewerPane = root.querySelector<HTMLElement>('[data-active="false"] > .fm-pane');
    const tabs = viewerPane?.querySelectorAll<HTMLButtonElement>('.fm-pane-tab');
    expect(tabs).toHaveLength(2);
    expect(tabs?.[1]?.textContent).toContain('.env');

    tabs?.[0]?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).toBeNull());
    expect(viewerPane?.querySelector('.fm-directory-table')).not.toBeNull();
    viewerPane?.querySelectorAll<HTMLButtonElement>('.fm-pane-tab')[1]?.click();
    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).not.toBeNull());

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-close')?.click();

    await vi.waitFor(() => expect(root.querySelector('.fm-file-viewer')).toBeNull());
    expect(viewerPane?.classList.contains('fm-pane-viewer')).toBe(false);
    expect(viewerPane?.querySelectorAll('.fm-pane-tab')).toHaveLength(1);
  });

  it('re-fetches the listing when clicking a tab that is already active', async () => {
    // Regression test: clicking a tab that's already the active one used to be a pure no-op, so
    // an external filesystem change (e.g. a browser download landing in a folder the user is
    // already looking at) never showed up without a manual hard reload.
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const activeTab = activePane?.querySelector<HTMLButtonElement>(
      '.fm-pane-tab[aria-selected="true"]',
    );
    expect(activeTab).not.toBeUndefined();
    const callsBefore = listDirectory.mock.calls.length;

    activeTab?.click();

    await vi.waitFor(() => expect(listDirectory.mock.calls.length).toBeGreaterThan(callsBefore));
  });

  it('keeps the Lister viewer open with an external fallback when the content is unsupported', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'readFileRange').mockResolvedValue({
      data: [0, 1, 2],
      offset: 0,
      length: 3,
      eof: true,
      probablyBinary: true,
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F3', bubbles: true }));

    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer')?.textContent).toContain('Preview not available'),
    );
    expect(root.querySelector('.fm-file-viewer-open-externally')).not.toBeNull();
  });

  it('opens the OS default application instead of the Lister viewer with Alt+F3', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F3', altKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction.mock.calls[0]?.[0]).toMatchObject({
      actionId: 'core.view',
      parameters: { uri: 'mock:///.env' },
    });
    expect(root.querySelector('.fm-file-viewer')).toBeNull();
  });

  it('shows a brief toast instead of invoking a permanently browser-unavailable action from its shortcut', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.openWith',
        title: 'Open With…',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'Enter', ctrl: true }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'core' },
      },
    ]);
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true }),
    );

    try {
      await vi.waitFor(() => expect(document.querySelector('.toast')).not.toBeNull());
      expect(document.querySelector('.toast')?.textContent).toContain('Open With…');
      expect(invokeAction).not.toHaveBeenCalled();
    } finally {
      Toast.dismissAll();
      await vi.waitFor(() => expect(document.getElementById('toast-container')).toBeNull());
    }
  });

  it('keeps F3 View in the footer even when the OS-open fallback is unavailable in the browser (task 0088)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.view',
        title: 'View',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3' }],
        contextRequirements: { requiresSelection: true, requiresSingleSelection: true },
        source: { kind: 'core' },
      },
      {
        id: 'core.open',
        title: 'Open',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'Enter' }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'core' },
      },
    ]);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const footerKey = [...root.querySelectorAll<HTMLElement>('.fm-function-key')].find((span) =>
      span.textContent?.includes('F3 View'),
    );
    expect(footerKey).not.toBeUndefined();
  });

  it('keeps F4 Edit in the footer when only the OS-edit fallback is unavailable in the browser', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.edit',
        title: 'Edit',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F4' }],
        contextRequirements: {
          requiresSelection: true,
          requiresSingleSelection: true,
          featureAvailable: false,
        },
        source: { kind: 'core' },
      },
    ]);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const footerKey = [...root.querySelectorAll<HTMLElement>('.fm-function-key')].find((span) =>
      span.textContent?.includes('F4 Edit'),
    );
    expect(footerKey).not.toBeUndefined();
  });

  it('shows a friendly toast instead of an error for the Alt+F3 OS-open fallback when it is unavailable in the browser', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.view',
        title: 'View',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'F3' }],
        contextRequirements: { requiresSelection: true, requiresSingleSelection: true },
        source: { kind: 'core' },
      },
      {
        id: 'core.open',
        title: 'Open',
        category: 'fileOperations',
        defaultShortcuts: [{ key: 'Enter' }],
        contextRequirements: { featureAvailable: false },
        source: { kind: 'core' },
      },
    ]);
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(activePane?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (row) => row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F3', altKey: true, bubbles: true }),
    );

    try {
      await vi.waitFor(() => expect(document.querySelector('.toast')).not.toBeNull());
      expect(document.querySelector('.toast')?.textContent).toContain("isn't available");
      expect(invokeAction).not.toHaveBeenCalled();
    } finally {
      Toast.dismissAll();
      await vi.waitFor(() => expect(document.getElementById('toast-container')).toBeNull());
    }
  });

  it('cuts a selection, dims it, and pastes the move into the active pane', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));
    const left = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const file = [...(left?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find((row) =>
      row.textContent?.includes('.env'),
    );
    file?.click();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'x', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    expect(file?.classList.contains('fm-cut-entry')).toBe(true);

    root.querySelector<HTMLElement>('[data-pane-id="right"]')?.click();
    await vi.waitFor(() =>
      expect(root.querySelector('[data-pane-id="right"]')?.getAttribute('data-active')).toBe(
        'true',
      ),
    );
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'v', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'move',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
    await vi.waitFor(() => expect(file?.classList.contains('fm-cut-entry')).toBe(false));
  });

  it('drags a selection between panes through the operation engine', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const left = root.querySelector<HTMLElement>('[data-pane-id="left"]');
    const right = root.querySelector<HTMLElement>('[data-pane-id="right"]');
    const source = [...(left?.querySelectorAll<HTMLElement>('.fm-directory-row') ?? [])].find(
      (candidate) => candidate.textContent?.includes('.env'),
    );
    const target = directoryRowNamed(right, 'report.pdf');
    source?.click();
    source?.dispatchEvent(new Event('dragstart', { bubbles: true }));
    target?.dispatchEvent(new Event('dragover', { bubbles: true, cancelable: true }));
    target?.dispatchEvent(new Event('drop', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'move',
      sources: [{ uri: 'mock:///.env' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
  });

  it('hands a selection to the native drag host when that capability is available', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      ...(await client.getRuntimeCapabilities()),
      nativeDragOut: true,
      platform: 'macos',
      runtime: 'tauri',
    });
    const startNativeDrag = vi.spyOn(client, 'startNativeDrag');
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('.env'));

    const source = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((candidate) =>
      candidate.textContent?.includes('.env'),
    );
    source?.click();
    const dragStart = new Event('dragstart', { bubbles: true, cancelable: true });
    source?.dispatchEvent(dragStart);

    expect(dragStart.defaultPrevented).toBe(true);
    expect(startNativeDrag).toHaveBeenCalledWith([{ providerId: 'file', uri: 'mock:///.env' }]);
  });

  it('defaults an in-app drag to move even when routed through the native drag host', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      ...(await client.getRuntimeCapabilities()),
      nativeDragOut: true,
      platform: 'macos',
      runtime: 'tauri',
    });
    let nativeDropListener:
      | ((drop: {
          locations: readonly Location[];
          position: { readonly x: number; readonly y: number };
        }) => void)
      | undefined;
    vi.spyOn(client, 'subscribeNativeFileDrops').mockImplementation(async (listener) => {
      nativeDropListener = listener;
      return () => undefined;
    });
    vi.spyOn(client, 'startNativeDrag').mockResolvedValue(undefined);
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, 'report.pdf')).not.toBeUndefined());

    const source = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((candidate) =>
      candidate.textContent?.includes('.env'),
    );
    const target = directoryRowNamed(root, 'report.pdf');
    source?.click();
    source?.dispatchEvent(new Event('dragstart', { bubbles: true, cancelable: true }));

    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => target ?? null),
    });
    nativeDropListener?.({
      locations: [{ providerId: 'file', uri: 'mock:///.env' }],
      position: { x: 240, y: 120 },
    });

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'move',
      sources: [{ providerId: 'file', uri: 'mock:///.env' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
  });

  it('copies a native file drop through the operation engine', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      ...(await client.getRuntimeCapabilities()),
      nativeDragOut: true,
      platform: 'windows',
      runtime: 'tauri',
    });
    let nativeDropListener:
      | ((drop: {
          locations: readonly Location[];
          position: { readonly x: number; readonly y: number };
        }) => void)
      | undefined;
    vi.spyOn(client, 'subscribeNativeFileDrops').mockImplementation(async (listener) => {
      nativeDropListener = listener;
      return () => undefined;
    });
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, 'report.pdf')).not.toBeUndefined());

    const target = directoryRowNamed(root, 'report.pdf');
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: vi.fn(() => target ?? null),
    });
    nativeDropListener?.({
      locations: [{ providerId: 'local', uri: 'file:///Users/example/from-finder.txt' }],
      position: { x: 240, y: 120 },
    });

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    expect(startOperation.mock.calls[0]?.[0]).toMatchObject({
      type: 'copy',
      sources: [{ providerId: 'local', uri: 'file:///Users/example/from-finder.txt' }],
      destination: { uri: 'mock:///Documents' },
      conflictPolicy: 'ask',
    });
  });

  it('keeps runtime diagnostics out of the workspace chrome', () => {
    mountShell('mock');

    expect(root.textContent).not.toContain('File Manager');
    expect(root.textContent).not.toContain('Connection:');
    expect(root.textContent).not.toContain('mock');
  });

  it('does not render connection diagnostics in the workspace', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    expect(root.querySelector('.fm-connection-status')).toBeNull();
  });

  it('loads operations once then updates progress from events without polling', async () => {
    const client = new MockFileManagerClient();
    const operation = await client.startOperation({
      type: 'copy',
      sources: [{ providerId: 'file', uri: 'mock:///Documents/report.pdf' }],
      destination: { providerId: 'file', uri: 'mock:///Empty' },
      conflictPolicy: 'ask',
    });
    const listOperations = vi.spyOn(client, 'listOperations');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await openOperationCentre();
    await vi.waitFor(() => expect(root.textContent).toContain('Copy - Running'));
    client.emit({
      eventId: 11,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'operation.progress',
        operationId: operation.id,
        progress: { completedItems: 1, totalItems: 2, completedBytes: 512 },
      },
    });

    await vi.waitFor(() => expect(root.textContent).toContain('1 / 2 items'));
    expect(listOperations).toHaveBeenCalledTimes(1);
  });

  it('keeps manually dismissed failed operations hidden after remount', async () => {
    const client = new MockFileManagerClient();
    const failed: Operation = {
      id: 'failed-copy-1',
      kind: 'copy',
      state: 'failed',
      sources: [],
      progress: { completedItems: 0, completedBytes: 0 },
      conflictPolicy: 'ask',
      createdAt: '2026-08-10T12:00:00.000Z',
    };
    vi.spyOn(client, 'listOperations').mockResolvedValue([failed]);

    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openOperationCentre();
    await vi.waitFor(() => expect(root.textContent).toContain('Copy - Failed'));

    root
      .querySelector<HTMLButtonElement>(
        '[data-operation-id="failed-copy-1"] [data-action="dismiss"]',
      )
      ?.click();
    m.redraw.sync();
    expect(root.textContent).not.toContain('Copy - Failed');

    m.mount(root, null);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    expect(root.textContent).not.toContain('Copy - Failed');
  });

  it('restores completed undoable operations into the centre after restart', async () => {
    const client = new MockFileManagerClient();
    const undoableTrash: Operation = {
      id: 'undoable-trash-1',
      kind: 'trash',
      state: 'completed',
      sources: [],
      progress: { completedItems: 1, completedBytes: 0 },
      conflictPolicy: 'ask',
      createdAt: '2026-08-30T12:00:00.000Z',
      undo: { available: true },
    };
    vi.spyOn(client, 'listOperations').mockResolvedValue([undoableTrash]);

    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openOperationCentre();

    await vi.waitFor(() => expect(root.textContent).toContain('Trash - Completed'));
    expect(
      root.querySelector('[data-operation-id="undoable-trash-1"] [data-action="undo"]'),
    ).not.toBeNull();
  });

  it('replaces an undone operation with the undo job and updates its inline state', async () => {
    const client = new MockFileManagerClient();
    const undoableTrash: Operation = {
      id: 'undoable-trash-1',
      kind: 'trash',
      state: 'completed',
      sources: [
        {
          id: 'report',
          location: { providerId: 'local', uri: 'file:///Documents/report.pdf' },
        },
      ],
      progress: { completedItems: 1, completedBytes: 0 },
      conflictPolicy: 'ask',
      createdAt: '2026-08-30T12:00:00.000Z',
      completedAt: '2026-08-30T12:00:01.000Z',
      undo: { available: true },
    };
    const undo: Operation = {
      id: 'undo-trash-1',
      kind: 'undo',
      state: 'running',
      sources: undoableTrash.sources,
      progress: { completedItems: 0, totalItems: 1, completedBytes: 0 },
      conflictPolicy: 'ask',
      createdAt: '2026-08-30T12:01:00.000Z',
      startedAt: '2026-08-30T12:01:00.000Z',
      undo: { available: false, reason: 'Undo operations cannot themselves be undone.' },
      undoOf: undoableTrash.id,
    };
    vi.spyOn(client, 'listOperations').mockResolvedValue([undoableTrash]);
    vi.spyOn(client, 'undoOperation').mockResolvedValue(undo);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openOperationCentre();
    await vi.waitFor(() => expect(root.textContent).toContain('Trash - Completed'));

    root
      .querySelector<HTMLButtonElement>(
        `[data-operation-id="${undoableTrash.id}"] [data-action="undo"]`,
      )
      ?.click();

    await vi.waitFor(() => expect(root.textContent).toContain('Undo - Running'));
    expect(root.textContent).not.toContain('Trash - Completed');
    expect(root.textContent).not.toContain('Undo is in progress');
    expect(
      root.querySelector('[data-operation-id="undo-trash-1"] .fm-operation-undo-reason'),
    ).toBeNull();

    client.emit({
      eventId: 12,
      timestamp: '2026-08-30T12:01:01.000Z',
      payload: {
        type: 'operation.completed',
        operation: {
          ...undo,
          state: 'completed',
          progress: { completedItems: 1, totalItems: 1, completedBytes: 0 },
          completedAt: '2026-08-30T12:01:01.000Z',
        },
      },
    });

    await vi.waitFor(() => expect(root.textContent).toContain('Undo - Completed'));
    expect(root.textContent).not.toContain('Undo is in progress');
    expect(
      root.querySelector('[data-operation-id="undo-trash-1"] .fm-operation-undo-reason'),
    ).toBeNull();
  });

  it('acknowledges cancel immediately while the backend request is still pending', async () => {
    const client = new MockFileManagerClient();
    const operation = await client.startOperation({
      type: 'copy',
      sources: [{ providerId: 'file', uri: 'mock:///Documents/report.pdf' }],
      destination: { providerId: 'file', uri: 'mock:///Empty' },
      conflictPolicy: 'ask',
    });
    let acknowledgeCancel: (() => void) | undefined;
    const pendingCancel = new Promise<void>((resolve) => {
      acknowledgeCancel = resolve;
    });
    const cancelOperation = vi.spyOn(client, 'cancelOperation').mockReturnValue(pendingCancel);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openOperationCentre();
    await vi.waitFor(() => expect(root.textContent).toContain('Copy - Running'));

    root
      .querySelector<HTMLButtonElement>(
        `[data-operation-id="${operation.id}"] [data-action="cancel"]`,
      )
      ?.click();
    m.redraw.sync();

    expect(cancelOperation).toHaveBeenCalledWith(operation.id);
    expect(root.textContent).toContain('Copy - Cancelling');
    expect(
      root.querySelector(`[data-operation-id="${operation.id}"] [data-action="cancel"]`),
    ).toBeNull();
    acknowledgeCancel?.();
  });

  it('does not retain an unconfirmed permanent delete after cancellation', async () => {
    const client = new MockFileManagerClient();
    const operation: Operation = {
      id: 'pending-delete-1',
      kind: 'delete',
      state: 'waitingForConflictResolution',
      sources: [{ id: 'report.pdf', location: { providerId: 'file', uri: 'mock:///report.pdf' } }],
      progress: {
        completedItems: 0,
        totalItems: 1,
        completedBytes: 0,
        totalBytes: 1024,
      },
      conflictPolicy: 'ask',
      createdAt: '2026-08-30T12:00:00.000Z',
    };
    vi.spyOn(client, 'listOperations').mockResolvedValue([operation]);
    const cancelOperation = vi.spyOn(client, 'cancelOperation').mockResolvedValue();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() =>
      expect(
        root
          .querySelector('.fm-permanent-delete-modal')
          ?.closest('[role="dialog"]')
          ?.getAttribute('aria-hidden'),
      ).toBe('false'),
    );

    root.querySelector<HTMLButtonElement>('.fm-permanent-delete-cancel')?.click();
    m.redraw.sync();

    expect(cancelOperation).toHaveBeenCalledWith(operation.id);
    expect(root.querySelector(`[data-operation-id="${operation.id}"]`)).toBeNull();
    expect(root.textContent).not.toContain('Delete - Cancelled');
  });

  it('presents operation conflicts and submits the selected apply-to-all decision', async () => {
    const client = new MockFileManagerClient();
    const resolveConflict = vi.spyOn(client, 'resolveConflict').mockResolvedValue();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    client.emit({
      eventId: 12,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'operation.conflict',
        operationId: 'operation-1',
        conflictId: 'conflict-1',
        message: 'report.pdf already exists',
        source: { name: 'report.pdf', kind: 'file', size: 6 },
        destination: { name: 'report.pdf', kind: 'file', size: 8 },
      },
    });

    await vi.waitFor(() => expect(root.textContent).toContain('Resolve conflict'));
    root.querySelector<HTMLInputElement>('.fm-conflict-dialog input')?.click();
    const rename = [...root.querySelectorAll<HTMLButtonElement>('.fm-conflict-dialog button')].find(
      (button) => button.textContent === 'Rename new',
    );
    rename?.click();

    await vi.waitFor(() =>
      expect(resolveConflict).toHaveBeenCalledWith({
        operationId: 'operation-1',
        resolution: 'renameNew',
        applyToAllSimilar: true,
      }),
    );
    await vi.waitFor(() => expect(root.textContent).not.toContain('Resolve conflict'));
  });

  it('ignores old directory revisions and refetches pane snapshots after a replay gap', async () => {
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const summary = (await client.listWorkspaces())[0];
    if (summary === undefined) throw new Error('mock workspace fixture missing');
    const projection = await client.openWorkspace(summary.id);
    const paneId = projection.paneOrder[0];
    if (paneId === undefined) throw new Error('mock workspace pane missing');
    const initialCalls = listDirectory.mock.calls.length;
    const getWorkspace = vi.spyOn(client, 'getWorkspace');

    client.emit({
      eventId: 9,
      timestamp: '2026-07-31T12:00:00Z',
      workspaceId: projection.id,
      payload: { type: 'workspace.renamed', revision: projection.revision, name: 'Old name' },
    });
    await Promise.resolve();
    expect(getWorkspace).not.toHaveBeenCalled();

    client.emit({
      eventId: 10,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'directory.delta',
        paneId,
        delta: { type: 'entriesRemoved', revision: 0, entryIds: [] },
      },
    });
    await Promise.resolve();
    expect(listDirectory).toHaveBeenCalledTimes(initialCalls);

    client.emitResynchronise();
    await vi.waitFor(() => expect(listDirectory.mock.calls.length).toBeGreaterThan(initialCalls));
  });

  it('falls back to a full refetch when receiving a malformed entriesRemoved delta payload', async () => {
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const summary = (await client.listWorkspaces())[0];
    if (summary === undefined) throw new Error('mock workspace fixture missing');
    const projection = await client.openWorkspace(summary.id);
    const paneId = projection.paneOrder[0];
    if (paneId === undefined) throw new Error('mock workspace pane missing');
    const before = listDirectory.mock.calls.length;

    client.emit({
      eventId: 13,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'directory.delta',
        paneId,
        delta: {
          type: 'entriesRemoved',
          revision: 99,
          // Simulate malformed payload from a bad producer/version skew.
          entryIds: undefined,
        } as unknown as import('../models').DirectoryDelta,
      },
    });

    await vi.waitFor(() => expect(listDirectory.mock.calls.length).toBeGreaterThan(before));
  });

  it('refreshes pane snapshots when a mutating operation reaches failed state', async () => {
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const before = listDirectory.mock.calls.length;

    const op: Operation = {
      id: 'copy-with-warning-path',
      kind: 'copy',
      state: 'running',
      sources: [],
      progress: { completedItems: 1, completedBytes: 1_024 },
      conflictPolicy: 'ask',
      createdAt: '2026-08-10T12:00:00.000Z',
    };
    client.emit({
      eventId: 21,
      timestamp: '2026-08-10T12:00:00.000Z',
      payload: { type: 'operation.created', operation: op },
    });
    client.emit({
      eventId: 22,
      timestamp: '2026-08-10T12:00:00.100Z',
      payload: {
        type: 'operation.failed',
        operationId: op.id,
        code: 'destinationAlreadyExists',
        message: 'Destination already exists.',
      },
    });

    await vi.waitFor(() => expect(listDirectory.mock.calls.length).toBeGreaterThan(before));
  });

  it('switches to the other pane on Tab', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));

    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'setActivePane' }),
        undefined,
      ),
    );
  });

  it('activates a clicked pane locally before the workspace command completes', async () => {
    const client = new MockFileManagerClient({ latencyMs: 50 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    root.querySelector<HTMLElement>('[data-pane-id="right"]')?.click();
    m.redraw.sync();

    expect(root.querySelector('[data-pane-id="right"]')?.getAttribute('data-active')).toBe('true');
  });

  it('refetches panes after confirming a conflict resolution', async () => {
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    const resolveConflict = vi.spyOn(client, 'resolveConflict').mockResolvedValue();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const before = listDirectory.mock.calls.length;

    client.emit({
      eventId: 13,
      timestamp: '2026-08-10T12:00:00.000Z',
      payload: {
        type: 'operation.conflict',
        operationId: 'delete-conflict-op',
        conflictId: 'delete-conflict-1',
        message: 'Target already exists',
        source: { name: 'old-folder', kind: 'directory' },
        destination: { name: 'old-folder', kind: 'directory' },
      },
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Resolve conflict'));

    [...root.querySelectorAll<HTMLButtonElement>('.fm-conflict-dialog button')]
      .find((button) => button.textContent === 'Overwrite')
      ?.click();

    await vi.waitFor(() =>
      expect(resolveConflict).toHaveBeenCalledWith({
        operationId: 'delete-conflict-op',
        resolution: 'overwrite',
        applyToAllSimilar: false,
      }),
    );
    await vi.waitFor(() => expect(listDirectory.mock.calls.length).toBeGreaterThan(before));
  });

  it('does not expose the runtime in the workspace chrome', () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'tauri', client }) });

    expect(root.textContent).not.toContain('tauri');
    expect(root.textContent).not.toContain('mock');
  });

  it('initializes the theme manager from its oninit lifecycle hook', () => {
    const initialize = vi.spyOn(ThemeManager, 'initialize');
    const setUseLocalStorage = vi.spyOn(ThemeManager, 'setUseLocalStorage');

    mountShell();

    // Settings belong to the backend (§26), so browser-storage persistence is
    // switched off explicitly. `initialize` is asserted by argument rather than
    // call count, because ThemeSwitcher initializes itself as well.
    expect(setUseLocalStorage).toHaveBeenCalledExactlyOnceWith(false);
    expect(initialize).toHaveBeenCalledWith('auto');
  });

  it('loads and applies backend theme, dimensions, and entry formats at bootstrap', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSettings').mockResolvedValue({
      ...(await client.getSettings()),
      theme: 'dark',
      fontSize: 17,
      rowHeight: 39,
      dateFormat: 'iso',
      sizeFormat: 'bytes',
    });
    const setTheme = vi.spyOn(ThemeManager, 'setTheme');

    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await vi.waitFor(() => expect(setTheme).toHaveBeenCalledWith('dark'));
    expect(document.documentElement.style.getPropertyValue('--fm-font-size')).toBe('17px');
    expect(document.documentElement.style.getPropertyValue('--fm-row-height')).toBe('39px');
    await vi.waitFor(() => expect(root.textContent).toContain('8,192 B'));
  });

  it('renders the theme switcher inside the appearance settings editor', async () => {
    m.mount(root, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });

    expect(root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure')?.open).toBe(false);
    await openAppearanceSettings();
    expect(root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure')?.open).toBe(true);
    expect(root.querySelector('.fm-settings-editor')?.getAttribute('role')).toBe('dialog');
    expect(root.querySelector('.theme-switcher')).not.toBeNull();
    expect(themeButton('Light')).toBeInstanceOf(HTMLButtonElement);
    expect(themeButton('Dark')).toBeInstanceOf(HTMLButtonElement);
    expect(themeButton('Auto')).toBeInstanceOf(HTMLButtonElement);
  });

  it('renders settings content when the native disclosure state opens', async () => {
    m.mount(root, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const disclosure = root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure');
    expect(disclosure).not.toBeNull();

    if (disclosure === null) throw new Error('settings disclosure was not rendered');
    disclosure.open = true;
    disclosure.dispatchEvent(new Event('toggle'));
    m.redraw.sync();

    expect(disclosure.querySelector('.fm-settings-editor-body')).not.toBeNull();
    expect(disclosure.querySelector('.theme-switcher')).not.toBeNull();
    expect(disclosure.querySelector('.fm-settings-editor-actions')).not.toBeNull();
  });

  it('persists a changed setting and closes the dialog when Save is clicked', async () => {
    const client = new MockFileManagerClient();
    const updateSettings = vi.spyOn(client, 'updateSettings');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openAppearanceSettings();

    themeButton('Dark').click();
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    m.redraw.sync();

    await vi.waitFor(() => expect(updateSettings).toHaveBeenCalledTimes(1));
    expect(updateSettings.mock.calls[0]?.[0]?.theme).toBe('dark');
    expect(root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure')?.open).toBe(false);
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('applies a saved "show hidden files" change to every open tab and refetches immediately', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openAppearanceSettings();
    listDirectory.mockClear();

    const hiddenFilesLabel = [...root.querySelectorAll<HTMLElement>('label.switch-label')].find(
      (label) => label.textContent?.includes('Show hidden files'),
    );
    hiddenFilesLabel?.closest<HTMLElement>('.input-field')?.click();
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    m.redraw.sync();

    // Both panes' tabs start with `showHidden: false` (mock workspace fixture) and must each be
    // patched, since hidden-file filtering happens server-side - unlike sort/quick-filter, a
    // stale client-side view can't just be re-shown locally.
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'updateView',
          paneId: 'left',
          patch: { showHidden: true },
        }),
        undefined,
      ),
    );
    expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'updateView', paneId: 'right', patch: { showHidden: true } }),
      undefined,
    );
    await vi.waitFor(() =>
      expect(listDirectory).toHaveBeenCalledWith(
        expect.objectContaining({ showHidden: true }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('applies the persisted show-hidden setting to restored workspace tabs on startup', async () => {
    const client = new MockFileManagerClient();
    const savedSettings = await client.getSettings();
    vi.spyOn(client, 'getSettings').mockResolvedValue({
      ...savedSettings,
      showHiddenFiles: true,
    });
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    const listDirectory = vi.spyOn(client, 'listDirectory');

    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'updateView',
          paneId: 'left',
          patch: { showHidden: true },
        }),
        undefined,
      ),
    );
    expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'updateView', paneId: 'right', patch: { showHidden: true } }),
      undefined,
    );
    await vi.waitFor(() =>
      expect(listDirectory).toHaveBeenCalledWith(
        expect.objectContaining({ showHidden: true }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('applies the current "show hidden files" setting to a freshly opened tab', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openAppearanceSettings();

    const hiddenFilesLabel = [...root.querySelectorAll<HTMLElement>('label.switch-label')].find(
      (label) => label.textContent?.includes('Show hidden files'),
    );
    hiddenFilesLabel?.closest<HTMLElement>('.input-field')?.click();
    m.redraw.sync();
    root.querySelector<HTMLButtonElement>('.fm-settings-save')?.click();
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'updateView',
          paneId: 'left',
          patch: { showHidden: true },
        }),
        undefined,
      ),
    );
    listDirectory.mockClear();

    // New tabs are built from the pane's fixed `default_view` server-side, which never learns
    // about later `showHidden` patches to sibling tabs - without an explicit follow-up patch a
    // freshly opened tab would silently revert to hiding dotfiles.
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() =>
      expect(listDirectory).toHaveBeenCalledWith(
        expect.objectContaining({ showHidden: true }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('reverts a previewed setting and closes the dialog when Cancel is clicked', async () => {
    const client = new MockFileManagerClient();
    const updateSettings = vi.spyOn(client, 'updateSettings');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openAppearanceSettings();

    themeButton('Dark').click();
    m.redraw.sync();
    expect(document.documentElement.dataset.theme).toBe('dark');

    root.querySelector<HTMLButtonElement>('.fm-settings-cancel')?.click();
    m.redraw.sync();

    expect(updateSettings).not.toHaveBeenCalled();
    expect(root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure')?.open).toBe(false);
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
  });

  it('reverts a previewed setting when the dialog is dismissed via the close button', async () => {
    const client = new MockFileManagerClient();
    const updateSettings = vi.spyOn(client, 'updateSettings');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openAppearanceSettings();

    themeButton('Dark').click();
    m.redraw.sync();
    expect(document.documentElement.dataset.theme).toBe('dark');

    root.querySelector<HTMLButtonElement>('[aria-label="Close settings"]')?.click();
    m.redraw.sync();

    expect(updateSettings).not.toHaveBeenCalled();
    expect(root.querySelector<HTMLDetailsElement>('.fm-settings-disclosure')?.open).toBe(false);
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
  });

  it('lists discovered plugins inside the settings editor', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await openAppearanceSettings();

    await vi.waitFor(() => expect(root.querySelector('.fm-plugin-row')).not.toBeNull());
    expect(root.querySelector('.fm-plugin-row strong')?.textContent).toBe('Mock Archive');
  });

  it('applies a plugin.changed event to the plugins already listed', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await openAppearanceSettings();
    await vi.waitFor(() => expect(root.querySelector('.fm-plugin-row')).not.toBeNull());

    client.emit({
      eventId: 1,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'plugin.changed',
        plugin: { id: 'mock.archive', name: 'Mock Archive', version: '1.0.0', enabled: false },
      },
    });
    m.redraw.sync();

    const checkbox = root.querySelector<HTMLInputElement>('.fm-plugin-row input[type="checkbox"]');
    expect(checkbox?.checked).toBe(false);
  });

  it('refetches the full plugin list after a plugin.changed event, picking up fields the sparse event payload omits (e.g. iconTheme)', async () => {
    const client = new MockFileManagerClient();
    const listPlugins = vi.spyOn(client, 'listPlugins');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await openAppearanceSettings();
    await vi.waitFor(() => expect(root.querySelector('.fm-plugin-row')).not.toBeNull());
    const callsBeforeEvent = listPlugins.mock.calls.length;

    listPlugins.mockResolvedValueOnce([
      {
        id: 'mock.archive',
        name: 'Mock Archive',
        version: '1.0.0',
        description: 'Mock plugin with a sample column.',
        enabled: true,
        iconTheme: {
          iconDefinitions: { file: { iconPath: 'icons/file.svg' } },
          fileExtensions: {},
          fileNames: {},
          mimePrefixes: {},
        },
      },
    ]);
    client.emit({
      eventId: 1,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'plugin.changed',
        plugin: { id: 'mock.archive', name: 'Mock Archive', version: '1.0.0', enabled: true },
      },
    });

    await vi.waitFor(() => expect(listPlugins.mock.calls.length).toBeGreaterThan(callsBeforeEvent));
  });

  it('reinstalls a plugin icon theme after disable-then-reenable, instead of assuming it is still installed', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getSettings').mockResolvedValue({
      schemaVersion: 2,
      theme: 'auto',
      language: 'en',
      fontSize: 13,
      rowHeight: 20,
      dateFormat: 'medium',
      sizeFormat: 'binary',
      showHiddenFiles: false,
      confirmPermanentDelete: true,
      defaultConflictPolicy: 'ask',
      operationConcurrency: 2,
      defaultPaneLayout: 'dual',
      defaultColumns: ['core.name', 'core.size', 'core.modified'],
      columnWidths: {},
      keybindings: {},
      enabledPlugins: ['mock.archive'],
      pluginSettings: {},
      terminalCommand: null,
      editorCommand: null,
      defaultStartLocations: [],
      favouriteLocations: [],
      recentLocationsByWorkspace: {},
      multiRenamePresets: [],
      savedSearches: [],
      iconTheme: 'mock.archive',
    });
    const iconTheme = {
      iconDefinitions: { file: { iconPath: 'icons/file.svg' } },
      fileExtensions: {},
      fileNames: {},
      mimePrefixes: {},
    };
    const enabledDescriptor = {
      id: 'mock.archive',
      name: 'Mock Archive',
      version: '1.0.0',
      description: 'Mock plugin with a sample column.',
      enabled: true,
      iconTheme,
    };
    const disabledDescriptor = {
      id: 'mock.archive',
      name: 'Mock Archive',
      version: '1.0.0',
      description: 'Mock plugin with a sample column.',
      enabled: false,
      iconTheme,
    };
    const listPlugins = vi.spyOn(client, 'listPlugins').mockResolvedValue([enabledDescriptor]);
    const pluginIconTheme = await import('../themes/plugin-icon-theme');
    const install = vi.spyOn(pluginIconTheme, 'installPluginIconTheme').mockResolvedValue();
    const restore = vi
      .spyOn(pluginIconTheme, 'restoreDefaultIconTheme')
      .mockImplementation(() => {});

    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(install).toHaveBeenCalledTimes(1));
    const restoreCallsAfterBoot = restore.mock.calls.length;

    listPlugins.mockResolvedValueOnce([disabledDescriptor]);
    client.emit({
      eventId: 1,
      timestamp: '2026-07-31T12:00:00Z',
      payload: {
        type: 'plugin.changed',
        plugin: { id: 'mock.archive', name: 'Mock Archive', version: '1.0.0', enabled: false },
      },
    });
    await vi.waitFor(() =>
      expect(restore.mock.calls.length).toBeGreaterThan(restoreCallsAfterBoot),
    );

    listPlugins.mockResolvedValueOnce([enabledDescriptor]);
    client.emit({
      eventId: 2,
      timestamp: '2026-07-31T12:00:01Z',
      payload: {
        type: 'plugin.changed',
        plugin: { id: 'mock.archive', name: 'Mock Archive', version: '1.0.0', enabled: true },
      },
    });

    await vi.waitFor(() => expect(install).toHaveBeenCalledTimes(2));
  });

  it('applies a theme change and keeps the switcher selection in step', async () => {
    const setTheme = vi.spyOn(ThemeManager, 'setTheme');

    m.mount(root, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });
    await openAppearanceSettings();
    themeButton('Dark').click();
    m.redraw.sync();

    expect(setTheme).toHaveBeenCalledWith('dark');
    expect(themeButton('Dark').classList.contains('active')).toBe(true);
    expect(themeButton('Light').classList.contains('active')).toBe(false);
  });

  it('switches light, dark and follow-system themes without remounting', async () => {
    m.mount(root, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });
    await openAppearanceSettings();

    themeButton('Light').click();
    m.redraw.sync();
    expect(document.documentElement.dataset.theme).toBe('light');

    themeButton('Dark').click();
    m.redraw.sync();
    expect(document.documentElement.dataset.theme).toBe('dark');

    themeButton('Auto').click();
    m.redraw.sync();
    expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
    expect(root.querySelector('.fm-app-shell')).not.toBeNull();
  });

  it('keeps per-instance theme state in the factory closure', async () => {
    m.mount(root, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });
    await openAppearanceSettings();
    themeButton('Dark').click();
    m.redraw.sync();
    expect(themeButton('Dark').classList.contains('active')).toBe(true);

    // A fresh mount must not inherit the previous instance's closure state.
    const second = document.createElement('div');
    document.body.appendChild(second);
    m.mount(second, {
      view: () => m(AppShell, { runtime: 'mock', client: new MockFileManagerClient() }),
    });
    await openAppearanceSettings(second);

    expect(themeButtonIn(second, 'Auto').classList.contains('active')).toBe(true);
    expect(themeButtonIn(second, 'Dark').classList.contains('active')).toBe(false);

    m.mount(second, null);
    second.remove();
  });

  it('opens a file with core.open, passing its uri as a parameter (task 0061)', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined());

    const fileRow = directoryRowNamed(root, '日本語.txt');
    fileRow?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction).toHaveBeenCalledWith(
      expect.objectContaining({
        actionId: 'core.open',
        parameters: { uri: `mock:///${encodeURIComponent('日本語.txt')}` },
      }),
    );
  });

  it.each([
    ['cloud', 'OneDrive'],
    ['network', 'Mounted SMB'],
  ] as const)('navigates a discovered %s symlink inside the pane', async (kind, name) => {
    const client = new MockFileManagerClient();
    const location = { providerId: 'file', uri: 'mock:///documents-link' } as const;
    vi.spyOn(client, 'getSystemLocations').mockResolvedValue([{ name, kind, location }]);
    const navigatePane = vi.spyOn(client, 'navigatePane');
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });

    await vi.waitFor(() => expect(root.textContent).toContain('documents-link'));
    const linkRow = [...root.querySelectorAll<HTMLElement>('.fm-directory-row')].find((row) =>
      row.textContent?.includes('documents-link'),
    );
    linkRow?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({ location }),
        expect.anything(),
      ),
    );
    expect(invokeAction).not.toHaveBeenCalled();
  });

  it('loads discovered volumes on startup and shows them in the favourites dropdown (task 0144)', async () => {
    const client = new MockFileManagerClient();
    const location = { providerId: 'file', uri: 'mock:///' } as const;
    const getVolumes = vi
      .spyOn(client, 'getVolumes')
      .mockResolvedValue([{ name: 'Macintosh HD', location }]);
    const navigatePane = vi.spyOn(client, 'navigatePane');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(getVolumes).toHaveBeenCalled());

    root.querySelector<HTMLButtonElement>('.fm-pane-tab-favourites')?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-volumes-locations strong')?.textContent).toBe('Volumes');
    root.querySelector<HTMLButtonElement>('.fm-volumes-locations [role="menuitem"]')?.click();

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({ location }),
        expect.anything(),
      ),
    );
  });

  it('shows filename-search results as a virtual directory and opens a result in its folder', async () => {
    const client = new MockFileManagerClient();
    const navigatePane = vi.spyOn(client, 'navigatePane');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('#find-files-query');
    expect(input).not.toBeNull();
    if (input === null) return;
    input.value = 'report';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() => {
      expect(
        [...root.querySelectorAll('.fm-search-result-name')].map((name) => name.textContent),
      ).toContain('report');
      expect(
        [...root.querySelectorAll('.fm-search-result-parent')].map((path) => path.textContent),
      ).toContain('/Documents');
    });
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    expect(root.querySelector('.fm-find-files-body')?.textContent).toContain('Search in /');
    expect(root.querySelector('.fm-find-files-body')?.textContent).not.toContain('search://');
    root
      .querySelector<HTMLInputElement>('#find-files-query')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const rows = activePane?.querySelectorAll<HTMLElement>('.fm-directory-row');
    expect(rows?.item(0).textContent).toContain('..');
    rows
      ?.item(1)
      .dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 20, clientY: 20 }));
    m.redraw.sync();
    const searchResultActions = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu-item'),
    ];
    expect(searchResultActions.find((button) => button.textContent === 'Rename')?.disabled).toBe(
      false,
    );
    expect(searchResultActions.find((button) => button.textContent === 'Delete')?.disabled).toBe(
      false,
    );
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();
    rows?.item(1).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({ location: { providerId: 'file', uri: 'mock:///Documents' } }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('persists a search result favourite as a pinned saved search', async () => {
    const client = new MockFileManagerClient();
    const settings = await client.getSettings();
    await client.updateSettings({
      ...settings,
      favouriteLocations: [
        {
          label: 'Legacy search',
          location: { providerId: 'search', uri: 'search://local/expired' },
        },
      ],
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('#find-files-query');
    if (input === null) throw new Error('find files input missing');
    input.value = '*.md';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-pane-tab-title')?.textContent).toBe('*.md'),
    );

    root.querySelector<HTMLButtonElement>('.fm-pane-tab-favourites')?.click();
    m.redraw.sync();
    expect(root.textContent).not.toContain('Legacy search');
    root.querySelector<HTMLButtonElement>('.fm-favourites-add-button')?.click();

    await vi.waitFor(async () => {
      expect((await client.getSettings()).savedSearches).toEqual([
        expect.objectContaining({
          name: '*.md',
          pinned: true,
          query: expect.objectContaining({
            name: expect.objectContaining({ pattern: '*.md' }),
          }),
        }),
      ]);
    });
    expect((await client.getSettings()).favouriteLocations).toEqual([]);
    expect(root.querySelector('.fm-favourites-add')).toBeNull();
    expect(root.querySelector('.fm-icon-heart')).not.toBeNull();
    expect(root.querySelector('.fm-icon-heart-plus')).toBeNull();
  });

  it('shows the search term in the breadcrumb/tab title and focuses/cursors the first result so ArrowDown moves the cursor', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('#find-files-query');
    if (input === null) throw new Error('input missing');
    input.value = 'e';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() =>
      // The "search: " text prefix is replaced by a search icon in the tab strip (task 0089
      // follow-up) - only the bare query text remains in the tab title's textContent.
      expect(root.querySelector('.fm-pane-tab-title')?.textContent).toBe('e'),
    );
    expect(root.querySelector('.fm-pane-tab-filename-search-icon')?.getAttribute('width')).toBe(
      '12',
    );
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    expect(
      [...(activePane?.querySelectorAll('.fm-breadcrumb-segment') ?? [])].map(
        (segment) => segment.textContent,
      ),
    ).toEqual(['/', 'search', 'local', 'file: e']);

    // Focus lands in the pane (not e.g. document.body) so keyboard cursor
    // navigation works immediately, without an extra click. This happens after an
    // additional microtask (navigation.navigate() resolving), so wait for it rather
    // than asserting immediately after the tab-title waitFor above resolves.
    await vi.waitFor(() => expect(document.activeElement).toBe(activePane));
    const firstCursorRow = activePane?.querySelector('.fm-cursor-row')?.textContent;
    expect(firstCursorRow).not.toBeUndefined();
    // The first result is cursored, not selected - landing on results is not a selection action
    // (matches freshly entering a real directory - see the "selects a row... with Enter" test).
    expect(activePane?.querySelector('.fm-selected-row')).toBeNull();

    document.activeElement?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }),
    );
    m.redraw.sync();
    expect(activePane?.querySelector('.fm-cursor-row')?.textContent).not.toBe(firstCursorRow);
  });

  it('removes a trashed file from a search results list', async () => {
    const client = new MockFileManagerClient();
    const startOperation = vi.spyOn(client, 'startOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'F7', altKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const input = root.querySelector<HTMLInputElement>('#find-files-query');
    if (input === null) throw new Error('input missing');
    input.value = 'report';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    const resultRow = () =>
      root
        .querySelector<HTMLElement>('[data-active="true"] .fm-search-result-name')
        ?.closest<HTMLElement>('.fm-directory-row');
    await vi.waitFor(() => expect(resultRow()).not.toBeUndefined());
    resultRow()?.click();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'F8', bubbles: true }));

    await vi.waitFor(() => expect(startOperation).toHaveBeenCalledOnce());
    const operation = await startOperation.mock.results[0]?.value;
    if (operation === undefined) throw new Error('trash operation missing');
    client.emit({
      eventId: 10_000,
      timestamp: '2026-08-27T12:00:00Z',
      payload: {
        type: 'operation.completed',
        operation: { ...operation, state: 'completed' },
      },
    });
    await vi.waitFor(() => expect(resultRow()).toBeUndefined());
  });

  it('enters a local archive as a folder with Enter', async () => {
    const client = new MockFileManagerClient();
    const originalListDirectory = client.listDirectory.bind(client);
    vi.spyOn(client, 'listDirectory').mockImplementation(async (request, signal) => {
      const snapshot = await originalListDirectory(request, signal);
      if (request.location.uri !== 'mock:///') return snapshot;
      return {
        ...snapshot,
        entries: [
          ...snapshot.entries,
          {
            id: 'archive-file',
            location: { providerId: 'local', uri: 'file:///tmp/photos.zip' },
            name: 'photos.zip',
            kind: 'file',
            hidden: false,
            readOnly: false,
            metadataRevision: 0,
          },
        ],
        totalKnownEntries: (snapshot.totalKnownEntries ?? snapshot.entries.length) + 1,
      };
    });
    const navigatePane = vi.spyOn(client, 'navigatePane').mockImplementation(async (request) => ({
      paneId: request.paneId,
      requestId: request.requestId,
      revision: 1,
      location: request.location,
      writable: true,
      entries: [],
      totalKnownEntries: 0,
      hasMore: false,
      loadingState: { type: 'loaded' },
    }));
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, 'photos.zip')).toBeDefined());

    const archiveRow = directoryRowNamed(root, 'photos.zip');
    archiveRow?.click();
    m.redraw.sync();
    archiveRow
      ?.closest<HTMLElement>('.fm-pane')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({
          location: { providerId: 'archive', uri: 'archive:///tmp/photos.zip!/' },
        }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('enters an epub file as a folder-like archive on double-click, instead of opening it externally', async () => {
    const client = new MockFileManagerClient();
    const originalListDirectory = client.listDirectory.bind(client);
    vi.spyOn(client, 'listDirectory').mockImplementation(async (request, signal) => {
      const snapshot = await originalListDirectory(request, signal);
      if (request.location.uri !== 'mock:///') return snapshot;
      return {
        ...snapshot,
        entries: [
          ...snapshot.entries,
          {
            id: 'epub-file',
            location: { providerId: 'local', uri: 'file:///tmp/book.epub' },
            name: 'book.epub',
            kind: 'file',
            hidden: false,
            readOnly: false,
            metadataRevision: 0,
          },
        ],
        totalKnownEntries: (snapshot.totalKnownEntries ?? snapshot.entries.length) + 1,
      };
    });
    const navigatePane = vi.spyOn(client, 'navigatePane').mockImplementation(async (request) => ({
      paneId: request.paneId,
      requestId: request.requestId,
      revision: 1,
      location: request.location,
      writable: true,
      entries: [],
      totalKnownEntries: 0,
      hasMore: false,
      loadingState: { type: 'loaded' },
    }));
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, 'book.epub')).toBeDefined());

    const epubRow = directoryRowNamed(root, 'book.epub');
    epubRow?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({
          location: { providerId: 'archive', uri: 'archive:///tmp/book.epub!/' },
        }),
        expect.any(AbortSignal),
      ),
    );
    expect(invokeAction).not.toHaveBeenCalledWith(
      expect.objectContaining({ actionId: 'core.open' }),
    );
  });

  it('reveals the selected entry via the context menu, passing its uri as a parameter (task 0061)', async () => {
    const client = new MockFileManagerClient();
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined());

    const fileRow = directoryRowNamed(root, '日本語.txt');
    fileRow?.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, clientX: 20, clientY: 20 }),
    );
    m.redraw.sync();

    const revealButton = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu-item'),
    ].find((button) => button.textContent === 'Reveal in File Manager');
    expect(revealButton).not.toBeUndefined();
    revealButton?.click();
    m.redraw.sync();

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction).toHaveBeenCalledWith(
      expect.objectContaining({
        actionId: 'core.revealInSystemFileManager',
        parameters: { uri: `mock:///${encodeURIComponent('日本語.txt')}` },
      }),
    );
  });

  it('opens the macOS Services submenu from the existing selection context menu', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      clipboard: false,
      extendedAttributes: false,
      finderTags: false,
      nativeDragOut: false,
      nativeFileIcons: false,
      nativeMenus: true,
      nativeThumbnails: false,
      openTerminal: false,
      platform: 'macos',
      platformContextMenu: true,
      plugins: true,
      revealInSystemFileManager: false,
      runtime: 'tauri',
      serverAdministration: false,
      systemTrash: false,
    });
    const listDirectory = client.listDirectory.bind(client);
    vi.spyOn(client, 'listDirectory').mockImplementation(async (request, signal) => {
      const snapshot = await listDirectory(request, signal);
      return {
        ...snapshot,
        entries: snapshot.entries.map((entry) => ({
          ...entry,
          location: {
            providerId: 'local',
            uri: `file:///tmp/${encodeURIComponent(entry.name)}`,
          },
        })),
      };
    });
    const showPlatformContextMenu = vi.spyOn(client, 'showPlatformContextMenu');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined());

    const fileRow = directoryRowNamed(root, '日本語.txt');
    fileRow?.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, clientX: 20, clientY: 20 }),
    );
    m.redraw.sync();
    const servicesButton = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu-item'),
    ].find((button) => button.textContent?.includes('Services'));
    servicesButton?.click();

    await vi.waitFor(() => expect(showPlatformContextMenu).toHaveBeenCalledOnce());
    expect(showPlatformContextMenu).toHaveBeenCalledWith([
      {
        providerId: 'local',
        uri: `file:///tmp/${encodeURIComponent('日本語.txt')}`,
      },
    ]);
  });

  it('opens a terminal at the current directory via the context menu, passing its uri (task 0061)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      clipboard: false,
      extendedAttributes: false,
      finderTags: false,
      nativeDragOut: false,
      nativeFileIcons: false,
      nativeMenus: false,
      platformContextMenu: false,
      nativeThumbnails: false,
      openTerminal: true,
      platform: 'linux',
      plugins: true,
      revealInSystemFileManager: false,
      runtime: 'mock',
      serverAdministration: false,
      systemTrash: false,
    });
    vi.spyOn(client, 'listActions').mockResolvedValue([
      {
        id: 'core.openTerminal',
        title: 'Open Terminal Here',
        category: 'tools',
        defaultShortcuts: [],
        contextRequirements: {},
        source: { kind: 'core' },
      },
    ]);
    const invokeAction = vi.spyOn(client, 'invokeAction');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined());

    const table = root.querySelector<HTMLElement>('[role="grid"]');
    table?.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    m.redraw.sync();

    const terminalButton = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-context-menu-item'),
    ].find((button) => button.textContent === 'Open Terminal Here');
    expect(terminalButton).not.toBeUndefined();
    terminalButton?.click();
    m.redraw.sync();

    await vi.waitFor(() => expect(invokeAction).toHaveBeenCalledOnce());
    expect(invokeAction).toHaveBeenCalledWith(
      expect.objectContaining({
        actionId: 'core.openTerminal',
        parameters: { uri: 'mock:///' },
      }),
    );
  });

  it('surfaces a platform action failure as a brief toast, never a persistent banner (task 0061)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'invokeAction').mockRejectedValue(
      new Error('no default application is registered for this file type'),
    );
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(directoryRowNamed(root, '日本語.txt')).not.toBeUndefined());

    const fileRow = directoryRowNamed(root, '日本語.txt');
    fileRow?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();

    try {
      await vi.waitFor(() =>
        expect(document.querySelector('.toast')?.textContent).toContain(
          'no default application is registered for this file type',
        ),
      );
      expect(root.querySelector('.fm-command-palette-error')).toBeNull();
    } finally {
      Toast.dismissAll();
      await vi.waitFor(() => expect(document.getElementById('toast-container')).toBeNull());
    }
  });

  it('opens the quick filter with Ctrl+F, filters the active pane live, and closes with Escape (task 0067)', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    const totalRows = activePane?.querySelectorAll('.fm-directory-row').length ?? 0;
    expect(totalRows).toBeGreaterThan(0);

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    const filterInput = activePane?.querySelector<HTMLInputElement>('.fm-quick-filter-input');
    expect(filterInput).not.toBeNull();
    expect(document.activeElement).toBe(filterInput);
    if (!filterInput) throw new Error('quick filter input missing');

    filterInput.value = 'doc';
    filterInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();

    expect(activePane?.querySelectorAll('.fm-directory-row')).toHaveLength(2);
    expect(activePane?.querySelector('.fm-pane-status')?.textContent).toContain(
      `2 of ${totalRows} shown`,
    );

    filterInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();

    expect(activePane?.querySelector('.fm-quick-filter-input')).toBeNull();
    expect(activePane?.querySelectorAll('.fm-directory-row')).toHaveLength(totalRows);
  });

  it('does nothing harmful when Ctrl+F repeats or an editable target already has focus (task 0067)', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    expect(activePane?.querySelectorAll('.fm-quick-filter-input')).toHaveLength(1);

    // The quick filter occupies the breadcrumb bar's own slot, so close it first to reach the
    // breadcrumb (see the pane.ts merge — the two are mutually exclusive in the same row).
    activePane
      ?.querySelector<HTMLInputElement>('.fm-quick-filter-input')
      ?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();
    expect(activePane?.querySelector('.fm-quick-filter-input')).toBeNull();

    activePane
      ?.querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane?.querySelector<HTMLInputElement>('.fm-path-input');
    pathInput?.focus();
    pathInput?.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    expect(document.activeElement).toBe(pathInput);
    expect(activePane?.querySelector('.fm-quick-filter-input')).toBeNull();
  });

  it('replaces an in-progress path edit when the quick filter is invoked, and does not resurrect it once the filter closes (task 0067)', async () => {
    mountShell('mock');
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');

    activePane
      ?.querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    expect(activePane?.querySelector('.fm-path-input')).not.toBeNull();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    const filterInput = activePane?.querySelector<HTMLInputElement>('.fm-quick-filter-input');
    expect(filterInput).not.toBeNull();
    expect(activePane?.querySelector('.fm-path-input')).toBeNull();

    filterInput?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    m.redraw.sync();

    expect(activePane?.querySelector('.fm-quick-filter-input')).toBeNull();
    expect(activePane?.querySelector('.fm-path-input')).toBeNull();
    expect(activePane?.querySelector('.fm-breadcrumb-segments')).not.toBeNull();
  });

  it('persists the committed quick-filter query and restores it when the filter box reopens (task 0067)', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const activePane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'f', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    const filterInput = activePane?.querySelector<HTMLInputElement>('.fm-quick-filter-input');
    if (!filterInput) throw new Error('quick filter input missing');
    filterInput.value = 'doc';
    filterInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    filterInput.dispatchEvent(new FocusEvent('blur', { bubbles: true }));
    m.redraw.sync();

    const workspaceId = (await client.listWorkspaces())[0]?.id ?? '';
    await vi.waitFor(async () => {
      const workspace = await client.getWorkspace(workspaceId);
      const pane = workspace.panesById[workspace.activePaneId];
      const tab = pane?.tabsById[pane.activeTabId];
      expect(tab?.view.quickFilter).toEqual({ query: 'doc' });
    });

    m.mount(root, null);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const reopenedPane = root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
    expect(reopenedPane?.querySelectorAll('.fm-directory-row')).toHaveLength(2);
  });
});

describe('tabs per pane (task 0069)', () => {
  function activePane(): HTMLElement | null {
    return root.querySelector<HTMLElement>('[data-active="true"] > .fm-pane');
  }

  function closeLastTabDialog(): HTMLElement | undefined {
    return [...root.querySelectorAll<HTMLElement>('[role="dialog"]')].find((dialog) =>
      dialog.textContent?.includes('only tab'),
    );
  }

  it('opens a new tab in the active pane at its current location with Ctrl+T', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(1);

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(dispatchWorkspaceCommand).toHaveBeenCalledOnce());
    expect(dispatchWorkspaceCommand.mock.calls[0]?.[0]).toMatchObject({
      type: 'addTab',
      paneId: 'left',
      location: { uri: 'mock:///' },
    });
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
  });

  it('persists a tab move between panes', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    dispatchWorkspaceCommand.mockClear();
    const source = root.querySelector<HTMLElement>('[data-pane-id="left"] [role="tab"]');
    const target = root.querySelector<HTMLElement>('[data-pane-id="right"] [role="tab"]');

    source?.dispatchEvent(new MouseEvent('pointerdown', { clientX: 0, clientY: 0, bubbles: true }));
    Object.defineProperty(document, 'elementFromPoint', {
      configurable: true,
      value: () => target,
    });
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 20, clientY: 20 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 20, clientY: 20 }));

    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'moveTab',
          sourcePaneId: 'left',
          tabId: 'left-tab',
          targetPaneId: 'right',
          targetIndex: 0,
        }),
        undefined,
      ),
    );
    await vi.waitFor(() =>
      expect(root.querySelectorAll('[data-pane-id="right"] [role="tab"]')).toHaveLength(2),
    );
    expect(root.querySelectorAll('[data-pane-id="left"] [role="tab"]')).toHaveLength(1);
  });

  it('removes deleted nodes from disk usage and adjusts ancestor totals', () => {
    const rootNode = {
      name: 'home',
      location: { providerId: 'file', uri: 'file:///home' },
      kind: 'directory' as const,
      logicalBytes: 110,
      physicalBytes: 100,
      collapsed: false,
      children: [
        {
          name: '.olmx',
          location: { providerId: 'file', uri: 'file:///home/.olmx' },
          kind: 'directory' as const,
          logicalBytes: 90,
          physicalBytes: 80,
          collapsed: true,
          children: [],
        },
        {
          name: 'Documents',
          location: { providerId: 'file', uri: 'file:///home/Documents' },
          kind: 'directory' as const,
          logicalBytes: 10,
          physicalBytes: 10,
          collapsed: false,
          children: [],
        },
      ],
    };

    const next = removeDiskUsageNodes(rootNode, new Set(['file:///home/.olmx']));

    expect(next?.children.map((child) => child.name)).toEqual(['Documents']);
    expect(next?.logicalBytes).toBe(20);
    expect(next?.physicalBytes).toBe(20);
  });

  it('opens disk usage in a new tab and navigates a clicked folder in the opposite pane', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    const scanDiskUsage = vi.spyOn(client, 'scanDiskUsage');
    const navigatePane = vi.spyOn(client, 'navigatePane');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, shiftKey: true, bubbles: true }),
    );

    await vi.waitFor(() =>
      expect(scanDiskUsage).toHaveBeenCalledWith(
        {
          workspaceId: expect.any(String),
          scanId: expect.any(String),
          location: { providerId: 'file', uri: 'mock:///' },
          expandRoot: false,
        },
        expect.any(AbortSignal),
      ),
    );
    expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
      expect.objectContaining({ type: 'addTransientTab' }),
      undefined,
    );
    await vi.waitFor(() =>
      expect(activePane()?.querySelector('.fm-disk-usage-map')).not.toBeNull(),
    );
    expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2);

    activePane()
      ?.querySelector<SVGRectElement>('.fm-disk-usage-block[aria-label^="Documents,"]')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    await vi.waitFor(() =>
      expect(navigatePane).toHaveBeenCalledWith(
        expect.objectContaining({
          paneId: 'right',
          location: { providerId: 'file', uri: 'mock:///Documents' },
        }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('cancels backend work when a disk-usage scan is stopped', async () => {
    const client = new MockFileManagerClient();
    const scanDiskUsage = vi
      .spyOn(client, 'scanDiskUsage')
      .mockImplementation(() => new Promise(() => undefined));
    const cancelDiskUsage = vi.spyOn(client, 'cancelDiskUsage').mockResolvedValue(undefined);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(scanDiskUsage).toHaveBeenCalledOnce());
    const scanId = scanDiskUsage.mock.calls[0]?.[0].scanId;
    activePane()?.querySelector<HTMLButtonElement>('.fm-disk-usage-status button')?.click();

    await vi.waitFor(() => expect(cancelDiskUsage).toHaveBeenCalledWith(scanId));
    await vi.waitFor(() => expect(activePane()?.textContent).toContain('Scan of mock: stopped.'));
  });

  it('cancels backend work when an active disk-usage tab is closed', async () => {
    const client = new MockFileManagerClient();
    const scanDiskUsage = vi
      .spyOn(client, 'scanDiskUsage')
      .mockImplementation(() => new Promise(() => undefined));
    const cancelDiskUsage = vi.spyOn(client, 'cancelDiskUsage').mockResolvedValue(undefined);
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(scanDiskUsage).toHaveBeenCalledOnce());
    const scanId = scanDiskUsage.mock.calls[0]?.[0].scanId;
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(cancelDiskUsage).toHaveBeenCalledWith(scanId));
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(1));
  });

  it('keeps partial disk-usage results visible when the scan later fails', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'scanDiskUsage').mockImplementation((request) => {
      client.emit({
        eventId: 98,
        timestamp: '2026-08-28T09:00:00Z',
        workspaceId: request.workspaceId,
        payload: {
          type: 'diskUsage.progress',
          scanId: request.scanId,
          root: {
            name: '/',
            location: request.location,
            kind: 'directory',
            logicalBytes: 80,
            physicalBytes: 80,
            collapsed: false,
            children: [
              {
                name: '.olmx',
                location: { providerId: 'file', uri: 'mock:///.olmx' },
                kind: 'directory',
                logicalBytes: 80,
                physicalBytes: 80,
                collapsed: true,
                children: [],
              },
            ],
          },
          unreadableEntries: 0,
          unreadable: [],
          scannedEntries: 12,
          isComplete: false,
        },
      });
      client.emit({
        eventId: 99,
        timestamp: '2026-08-28T09:00:01Z',
        workspaceId: request.workspaceId,
        payload: {
          type: 'diskUsage.failed',
          scanId: request.scanId,
          code: 'internal',
          message: 'Scanner worker stopped unexpectedly',
        },
      });
      return Promise.resolve();
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, shiftKey: true, bubbles: true }),
    );

    await vi.waitFor(() =>
      expect(activePane()?.querySelector('.fm-disk-usage-failure')?.textContent).toContain(
        'Scanner worker stopped unexpectedly',
      ),
    );
    expect(activePane()?.querySelector('.fm-disk-usage-map')).not.toBeNull();
    expect(activePane()?.textContent).toContain('.olmx');
    expect(activePane()?.querySelector('.fm-disk-usage-progress')).toBeNull();
  });

  it('rescans a collapsed disk-usage block when its Expand action is used', async () => {
    const client = new MockFileManagerClient();
    const collapsedLocation = { providerId: 'file', uri: 'mock:///node_modules' };
    const collapsed = {
      name: 'node_modules',
      location: collapsedLocation,
      kind: 'directory' as const,
      logicalBytes: 80,
      physicalBytes: 80,
      collapsed: true,
      children: [],
    };
    const scanDiskUsage = vi
      .spyOn(client, 'scanDiskUsage')
      .mockImplementationOnce((request) => {
        client.emit({
          eventId: 97,
          timestamp: '2026-08-28T08:59:59Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'diskUsage.progress',
            scanId: request.scanId,
            root: {
              name: '/',
              location: { providerId: 'file', uri: 'mock:///' },
              kind: 'directory',
              logicalBytes: 80,
              physicalBytes: 80,
              collapsed: false,
              children: [collapsed],
            },
            unreadableEntries: 2,
            unreadable: [],
            scannedEntries: 10,
            isComplete: true,
          },
        });
        return Promise.resolve();
      })
      .mockImplementationOnce((request) => {
        const result = {
          root: {
            ...collapsed,
            logicalBytes: 100,
            physicalBytes: 100,
            collapsed: false,
          },
          unreadableEntries: 3,
        };
        client.emit({
          eventId: 99,
          timestamp: '2026-08-28T09:00:00Z',
          workspaceId: request.workspaceId,
          payload: {
            type: 'diskUsage.progress',
            scanId: request.scanId,
            root: result.root,
            unreadableEntries: 3,
            unreadable: [],
            scannedEntries: 12,
            isComplete: true,
          },
        });
        return Promise.reject(new Error('late transport failure'));
      });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'l', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(activePane()?.querySelector('.fm-disk-usage-map')).not.toBeNull(),
    );

    activePane()
      ?.querySelector<SVGRectElement>('.fm-disk-usage-block')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));

    await vi.waitFor(() => expect(scanDiskUsage).toHaveBeenCalledTimes(2));
    expect(scanDiskUsage.mock.calls[1]?.[0]).toMatchObject({
      location: collapsedLocation,
      expandRoot: true,
    });
    await vi.waitFor(() =>
      expect(activePane()?.querySelector('.fm-disk-usage-toolbar')?.textContent).toContain('100 B'),
    );
    const toolbar = activePane()?.querySelector('.fm-disk-usage-toolbar');
    expect(toolbar?.querySelector('strong')?.textContent).toBe('/');
    expect(toolbar?.textContent).toContain('100 B');
    expect(toolbar?.querySelector('.fm-disk-usage-warning')?.textContent).toContain('2');

    const expansionRequest = scanDiskUsage.mock.calls[1]?.[0];
    if (expansionRequest === undefined) throw new Error('missing expansion request');
    client.emit({
      eventId: 100,
      timestamp: '2026-08-28T09:00:01Z',
      workspaceId: expansionRequest.workspaceId,
      payload: {
        type: 'diskUsage.progress',
        scanId: expansionRequest.scanId,
        root: {
          ...collapsed,
          logicalBytes: 120,
          physicalBytes: 120,
          collapsed: false,
        },
        unreadableEntries: 4,
        unreadable: [],
        scannedEntries: 14,
        isComplete: false,
      },
    });
    m.redraw.sync();

    expect(toolbar?.textContent).toContain('100 B');
    expect(toolbar?.querySelector('.fm-disk-usage-progress')).toBeNull();
  });

  it('closes the active tab with Ctrl+W directly when another tab remains', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true }),
    );

    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(1));
    expect(closeLastTabDialog()?.getAttribute('aria-hidden')).toBe('true');
  });

  it('gates closing a pane down to zero tabs behind confirmation with Ctrl+W', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(1);

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();

    expect(closeLastTabDialog()?.getAttribute('aria-hidden')).toBe('false');
    expect(dispatchWorkspaceCommand).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: 'closeTab' }),
      undefined,
    );

    [...(closeLastTabDialog()?.querySelectorAll('button') ?? [])]
      .find((button) => button.textContent === 'Close tab')
      ?.click();

    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'closeTab', paneId: 'left' }),
        undefined,
      ),
    );
  });

  it('cancelling the close-last-tab dialog leaves the tab open', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true }),
    );
    m.redraw.sync();
    [...(closeLastTabDialog()?.querySelectorAll('button') ?? [])]
      .find((button) => button.textContent === 'Cancel')
      ?.click();
    m.redraw.sync();

    expect(closeLastTabDialog()?.getAttribute('aria-hidden')).toBe('true');
    expect(dispatchWorkspaceCommand).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: 'closeTab' }),
      undefined,
    );
  });

  it('cycles tabs with Ctrl+Tab / Ctrl+Shift+Tab and jumps to a tab with Ctrl+2', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    function selectedTabIndex(): number {
      return [...(activePane()?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].findIndex(
        (tab) => tab.getAttribute('aria-selected') === 'true',
      );
    }

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));
    dispatchWorkspaceCommand.mockClear();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'activateTab', paneId: 'left' }),
        undefined,
      ),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));

    dispatchWorkspaceCommand.mockClear();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'activateTab', paneId: 'left' }),
        undefined,
      ),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));

    dispatchWorkspaceCommand.mockClear();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: '1', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'activateTab', paneId: 'left' }),
        undefined,
      ),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));
  });

  it('refreshes the activated tab directory in the background when switching back with Ctrl+Tab', async () => {
    const client = new MockFileManagerClient();
    const listDirectory = vi.spyOn(client, 'listDirectory');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    function selectedTabIndex(): number {
      return [...(activePane()?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].findIndex(
        (tab) => tab.getAttribute('aria-selected') === 'true',
      );
    }

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));

    listDirectory.mockClear();
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));
    await vi.waitFor(() =>
      expect(listDirectory).toHaveBeenCalledWith(
        expect.objectContaining({ location: expect.objectContaining({ uri: 'mock:///' }) }),
        expect.any(AbortSignal),
      ),
    );
  });

  it('cycles tabs with literal Ctrl+Tab on macOS instead of switching panes (Cmd+Tab is OS-reserved)', async () => {
    const client = new MockFileManagerClient();
    vi.spyOn(client, 'getRuntimeCapabilities').mockResolvedValue({
      clipboard: false,
      extendedAttributes: false,
      finderTags: false,
      nativeDragOut: false,
      nativeFileIcons: false,
      nativeMenus: false,
      platformContextMenu: false,
      nativeThumbnails: false,
      openTerminal: false,
      platform: 'macos',
      plugins: true,
      revealInSystemFileManager: false,
      runtime: 'mock',
      serverAdministration: false,
      systemTrash: false,
    });
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', metaKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
    dispatchWorkspaceCommand.mockClear();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({ type: 'activateTab', paneId: 'left' }),
        undefined,
      ),
    );
    expect(dispatchWorkspaceCommand).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: 'setActivePane' }),
      undefined,
    );
  });

  it('keeps the pane populated after Ctrl+Tab away and back while End is still loading pages', async () => {
    // Regression test: pressing End on a directory with more pages than are loaded starts a
    // background `loadAllPages` fetch. If the user switches tabs (Ctrl+Tab) before it settles,
    // that background load must stay pinned to the tab it was started for — not silently follow
    // whichever tab becomes active — otherwise the original tab's entries and cursor end up
    // corrupted with data computed for the wrong tab once the fetch resolves.
    const client = new MockFileManagerClient({ pageSize: 100, latencyMs: 20 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    function selectedTabIndex(): number {
      return [...(activePane()?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].findIndex(
        (tab) => tab.getAttribute('aria-selected') === 'true',
      );
    }

    activePane()
      ?.querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane()?.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === undefined || pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane()?.textContent).toContain('generated-0000000'));

    // Open a second tab (Ctrl+Tab needs somewhere to switch to), then switch back to the first.
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));
    await vi.waitFor(() => expect(activePane()?.textContent).toContain('generated-0000000'));

    // Start loading every remaining page (each page fetch delayed by `latencyMs`), then switch
    // away and back before those background fetches settle.
    activePane()?.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));

    // The background load may or may not have finished loading every page (switching away stops
    // it, per the loop's own tab-pinning check), but tab 1's pane must never end up empty or
    // showing data computed for tab 2's location — that is the corruption bug this test guards.
    expect(activePane()?.querySelectorAll('.fm-directory-row').length).toBeGreaterThan(0);
    expect(activePane()?.textContent).toContain('generated-');
  });

  it('reuses a fully-loaded tab snapshot instead of truncating it back to the first page on Ctrl+Tab away and back', async () => {
    // Regression test: task 0069's acceptance criteria says switching tabs must reuse the
    // previous snapshot "if still valid", not unconditionally refetch. A prior bug had
    // `activateTab` always call `navigation.load()` on reactivation, which replaced an
    // already-fully-loaded large directory's entries with just a freshly refetched first page,
    // while leaving the cursor pointed at an entry (e.g. the true last entry, selected via End)
    // that no longer existed in the truncated array — visually emptying the pane even though the
    // footer still reported that entry as selected.
    const client = new MockFileManagerClient({ pageSize: 100, latencyMs: 5 });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    function selectedTabIndex(): number {
      return [...(activePane()?.querySelectorAll<HTMLElement>('[role="tab"]') ?? [])].findIndex(
        (tab) => tab.getAttribute('aria-selected') === 'true',
      );
    }

    activePane()
      ?.querySelector<HTMLElement>('.fm-breadcrumb-segments')
      ?.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    m.redraw.sync();
    const pathInput = activePane()?.querySelector<HTMLInputElement>('.fm-path-input');
    if (pathInput === undefined || pathInput === null) throw new Error('path input missing');
    pathInput.value = '/large/1000';
    pathInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    pathInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(activePane()?.textContent).toContain('generated-0000000'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));
    await vi.waitFor(() => expect(activePane()?.textContent).toContain('generated-0000000'));

    // Press End and wait for every page to actually finish loading this time.
    activePane()?.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
    await vi.waitFor(
      () =>
        expect(activePane()?.querySelector('.fm-cursor-row')?.textContent).toContain(
          'generated-0000999',
        ),
      { timeout: 10_000 },
    );
    const lastRowBefore = activePane()?.querySelector('.fm-cursor-row')?.textContent;
    expect(lastRowBefore).toContain('generated-0000999');

    // Switch away and back — the tab was already fully loaded, so this must reuse that snapshot.
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(1));
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Tab', ctrlKey: true, shiftKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(selectedTabIndex()).toBe(0));

    // The cursor row must still be visible on the true last entry — not silently truncated back
    // down to the first fetched page with an orphaned cursor and a blank-looking viewport.
    const cursorRow = activePane()?.querySelector('.fm-cursor-row');
    expect(cursorRow).not.toBeNull();
    expect(cursorRow?.textContent).toContain('generated-0000999');
    expect(activePane()?.querySelectorAll('.fm-directory-row').length).toBeGreaterThan(0);
  });

  it('reopens the most recently closed tab with Ctrl+Shift+T', async () => {
    const client = new MockFileManagerClient();
    const dispatchWorkspaceCommand = vi.spyOn(client, 'dispatchWorkspaceCommand');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'w', ctrlKey: true, bubbles: true }),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(1));
    dispatchWorkspaceCommand.mockClear();

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 't', ctrlKey: true, shiftKey: true, bubbles: true }),
    );

    await vi.waitFor(() =>
      expect(dispatchWorkspaceCommand).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'addTab',
          paneId: 'left',
          location: { providerId: 'file', uri: 'mock:///' },
        }),
        undefined,
      ),
    );
    await vi.waitFor(() => expect(activePane()?.querySelectorAll('[role="tab"]')).toHaveLength(2));
  });
});

describe('workspace management (task 0084)', () => {
  function row(container: HTMLElement, workspaceId: string): HTMLElement | null {
    return container.querySelector<HTMLElement>(`[data-workspace-id="${workspaceId}"]`);
  }

  it('lists persisted workspaces in the switcher and switches the active one', async () => {
    const client = new MockFileManagerClient();
    const first = await client.createWorkspace({ name: 'Alpha' });
    const second = await client.createWorkspace({ name: 'Bravo' });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    await openWorkspaceSwitcher();

    expect(root.textContent).toContain('Alpha');
    expect(root.textContent).toContain('Bravo');
    expect(row(root, first.id)?.getAttribute('data-active')).toBe('true');

    row(root, second.id)?.querySelector<HTMLElement>('.fm-workspace-switcher-name')?.click();

    await vi.waitFor(() =>
      expect(
        root
          .querySelector('.fm-workspace-switcher-button')
          ?.closest('.fm-tooltip')
          ?.getAttribute('data-tooltip'),
      ).toBe('Workspace switcher, current workspace: Bravo'),
    );
    await vi.waitFor(() => expect(row(root, second.id)?.getAttribute('data-active')).toBe('true'));
  });

  it('closes the workspace switcher when clicking outside it', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    await openWorkspaceSwitcher();

    expect(root.querySelector<HTMLDetailsElement>('.fm-workspace-disclosure')?.open).toBe(true);
    root
      .querySelector<HTMLElement>('.fm-workspace-switcher-backdrop')
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector<HTMLDetailsElement>('.fm-workspace-disclosure')?.open).toBe(false);
  });

  it('creates a new workspace and activates it immediately', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    await openWorkspaceSwitcher();
    const before = await client.listWorkspaces();

    root.querySelector<HTMLButtonElement>('.fm-workspace-create-button')?.click();

    await vi.waitFor(async () =>
      expect((await client.listWorkspaces()).length).toBe(before.length + 1),
    );
    await vi.waitFor(() =>
      expect(
        root
          .querySelector('.fm-workspace-switcher-button')
          ?.closest('.fm-tooltip')
          ?.getAttribute('data-tooltip'),
      ).toBe('Workspace switcher, current workspace: Default'),
    );
  });

  it('renames the active workspace and updates the toolbar label', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const workspaceId = (await client.listWorkspaces())[0]?.id;
    if (workspaceId === undefined) throw new Error('no workspace to rename');
    await openWorkspaceSwitcher();

    row(root, workspaceId)
      ?.querySelector<HTMLButtonElement>('.fm-workspace-rename-button')
      ?.click();
    m.redraw.sync();
    const input = row(root, workspaceId)?.querySelector<HTMLInputElement>('input[type="text"]');
    if (input === null || input === undefined) throw new Error('rename input missing');
    input.value = 'Renamed workspace';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    row(root, workspaceId)
      ?.querySelector<HTMLFormElement>('.fm-workspace-rename-form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() =>
      expect(
        root
          .querySelector('.fm-workspace-switcher-button')
          ?.closest('.fm-tooltip')
          ?.getAttribute('data-tooltip'),
      ).toBe('Workspace switcher, current workspace: Renamed workspace'),
    );
  });

  it('deletes a workspace after confirmation and never strands the app without an active workspace', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const workspaceId = (await client.listWorkspaces())[0]?.id;
    if (workspaceId === undefined) throw new Error('no workspace to delete');
    await openWorkspaceSwitcher();

    row(root, workspaceId)
      ?.querySelector<HTMLButtonElement>('.fm-workspace-delete-button')
      ?.click();
    m.redraw.sync();
    [...root.querySelectorAll('button')].find((button) => button.textContent === 'Delete')?.click();

    await vi.waitFor(async () => {
      const summaries = await client.listWorkspaces();
      expect(summaries.find((summary) => summary.id === workspaceId)).toBeUndefined();
    });
    // Recovers by creating a fresh default workspace rather than stranding the app.
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    expect(root.querySelector('.fm-workspace-loading')).toBeNull();
  });

  it('leaves a running operation untouched when switching workspaces', async () => {
    const client = new MockFileManagerClient();
    await client.createWorkspace({ name: 'Alpha' });
    const second = await client.createWorkspace({ name: 'Bravo' });
    await client.startOperation({
      type: 'copy',
      sources: [{ providerId: 'file', uri: 'mock:///Documents/report.pdf' }],
      destination: { providerId: 'file', uri: 'mock:///Empty' },
      conflictPolicy: 'ask',
    });
    const cancelOperation = vi.spyOn(client, 'cancelOperation');
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await openOperationCentre();
    await vi.waitFor(() => expect(root.textContent).toContain('Copy - Running'));
    await openWorkspaceSwitcher();

    row(root, second.id)?.querySelector<HTMLElement>('.fm-workspace-switcher-name')?.click();

    await vi.waitFor(() =>
      expect(
        root
          .querySelector('.fm-workspace-switcher-button')
          ?.closest('.fm-tooltip')
          ?.getAttribute('data-tooltip'),
      ).toBe('Workspace switcher, current workspace: Bravo'),
    );
    await openOperationCentre();
    expect(root.textContent).toContain('Copy - Running');
    expect(cancelOperation).not.toHaveBeenCalled();
  });

  it('refreshes the switcher when another session creates, renames, and deletes a workspace', async () => {
    const client = new MockFileManagerClient();
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));

    const created = await client.createWorkspace({ name: 'Remote workspace' });
    client.emit({
      eventId: 101,
      timestamp: '2026-08-03T00:00:00Z',
      workspaceId: created.id,
      payload: { type: 'workspace.created', revision: created.revision },
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Remote workspace'));

    const renamed = await client.renameWorkspace(created.id, 'Renamed remotely', created.revision);
    client.emit({
      eventId: 102,
      timestamp: '2026-08-03T00:00:00Z',
      workspaceId: created.id,
      payload: { type: 'workspace.renamed', revision: renamed.revision, name: 'Renamed remotely' },
    });
    await vi.waitFor(() => expect(root.textContent).toContain('Renamed remotely'));

    await client.deleteWorkspace(created.id, renamed.revision);
    client.emit({
      eventId: 103,
      timestamp: '2026-08-03T00:00:00Z',
      workspaceId: created.id,
      payload: { type: 'workspace.deleted', revision: renamed.revision + 1 },
    });
    await vi.waitFor(() => expect(root.textContent).not.toContain('Renamed remotely'));
  });

  it('surfaces a rename revision conflict without silently discarding the edit', async () => {
    const client = new MockFileManagerClient({
      failures: {
        dispatchWorkspaceCommand: new ApiError(409, {
          code: 'workspaceRevisionConflict',
          message: 'stale workspace revision',
        }),
      },
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const workspaceId = (await client.listWorkspaces())[0]?.id;
    if (workspaceId === undefined) throw new Error('no workspace to rename');
    await openWorkspaceSwitcher();

    row(root, workspaceId)
      ?.querySelector<HTMLButtonElement>('.fm-workspace-rename-button')
      ?.click();
    m.redraw.sync();
    const input = row(root, workspaceId)?.querySelector<HTMLInputElement>('input[type="text"]');
    if (input === null || input === undefined) throw new Error('rename input missing');
    input.value = 'New name';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    row(root, workspaceId)
      ?.querySelector<HTMLFormElement>('.fm-workspace-rename-form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    await vi.waitFor(() => expect(root.textContent).toContain('changed elsewhere'));
    const unchanged = await client.getWorkspace(workspaceId);
    expect(unchanged.name).not.toBe('New name');
  });

  it('surfaces a delete revision conflict without deleting the workspace', async () => {
    const client = new MockFileManagerClient({
      failures: {
        deleteWorkspace: new ApiError(409, {
          code: 'workspaceRevisionConflict',
          message: 'stale workspace revision',
        }),
      },
    });
    m.mount(root, { view: () => m(AppShell, { runtime: 'mock', client }) });
    await vi.waitFor(() => expect(root.textContent).toContain('Documents'));
    const workspaceId = (await client.listWorkspaces())[0]?.id;
    if (workspaceId === undefined) throw new Error('no workspace to delete');
    await openWorkspaceSwitcher();

    row(root, workspaceId)
      ?.querySelector<HTMLButtonElement>('.fm-workspace-delete-button')
      ?.click();
    m.redraw.sync();
    [...root.querySelectorAll('button')].find((button) => button.textContent === 'Delete')?.click();

    await vi.waitFor(() => expect(root.textContent).toContain('changed elsewhere'));
    const summaries = await client.listWorkspaces();
    expect(summaries.find((summary) => summary.id === workspaceId)).toBeDefined();
  });
});
