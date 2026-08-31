import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ApplicationUninstallCandidate } from '../../models';
import { ApplicationUninstallDialog } from './application-uninstall-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

const RELATED_FILES: readonly ApplicationUninstallCandidate[] = [
  {
    location: { providerId: 'local', uri: 'file:///Users/erik/Library/Caches/com.example.Widget' },
    sizeBytes: 2048,
    removable: true,
  },
  {
    location: { providerId: 'local', uri: 'file:///Library/LaunchAgents/com.example.Widget.plist' },
    sizeBytes: 512,
    removable: false,
  },
];

describe('ApplicationUninstallDialog', () => {
  it('renders one row per related file, with a checkbox only for removable candidates', () => {
    m.mount(root, {
      view: () =>
        m(ApplicationUninstallDialog, {
          open: true,
          productName: 'Widget',
          relatedFiles: RELATED_FILES,
          canTrash: true,
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const rows = root.querySelectorAll('.fm-application-uninstall-row');
    expect(rows).toHaveLength(2);
    expect(root.textContent).toContain('/Users/erik/Library/Caches/com.example.Widget');
    expect(root.textContent).toContain('/Library/LaunchAgents/com.example.Widget.plist');
    expect(root.querySelectorAll('input[type="checkbox"]')).toHaveLength(1);
    expect(root.textContent).toContain('administrator');
  });

  it('pre-checks removable candidates and omits unchecked ones from confirm', () => {
    const onConfirm = vi.fn();
    m.mount(root, {
      view: () =>
        m(ApplicationUninstallDialog, {
          open: true,
          productName: 'Widget',
          relatedFiles: RELATED_FILES,
          canTrash: true,
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const checkbox = root.querySelector<HTMLInputElement>('input[type="checkbox"]');
    expect(checkbox?.checked).toBe(true);

    checkbox?.dispatchEvent(new Event('change'));
    m.redraw.sync();

    const confirmButton = [...root.querySelectorAll('button')].find(
      (button) => button.textContent === 'Move to Trash',
    );
    confirmButton?.click();

    expect(onConfirm).toHaveBeenCalledWith([]);
  });

  it('reports the checked removable location when left checked', () => {
    const onConfirm = vi.fn();
    m.mount(root, {
      view: () =>
        m(ApplicationUninstallDialog, {
          open: true,
          productName: 'Widget',
          relatedFiles: RELATED_FILES,
          canTrash: true,
          onConfirm,
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    const confirmButton = [...root.querySelectorAll('button')].find(
      (button) => button.textContent === 'Move to Trash',
    );
    confirmButton?.click();

    expect(onConfirm).toHaveBeenCalledWith([RELATED_FILES[0]?.location]);
  });

  it('calls onCancel when Cancel is clicked', () => {
    const onCancel = vi.fn();
    m.mount(root, {
      view: () =>
        m(ApplicationUninstallDialog, {
          open: true,
          productName: 'Widget',
          relatedFiles: RELATED_FILES,
          canTrash: true,
          onConfirm: vi.fn(),
          onCancel,
        }),
    });
    m.redraw.sync();

    const cancelButton = [...root.querySelectorAll('button')].find(
      (button) => button.textContent === 'Cancel',
    );
    cancelButton?.click();

    expect(onCancel).toHaveBeenCalled();
  });

  it('hides the confirm button when trash is unavailable', () => {
    m.mount(root, {
      view: () =>
        m(ApplicationUninstallDialog, {
          open: true,
          productName: 'Widget',
          relatedFiles: RELATED_FILES,
          canTrash: false,
          onConfirm: vi.fn(),
          onCancel: vi.fn(),
        }),
    });
    m.redraw.sync();

    expect(
      [...root.querySelectorAll('button')].some((b) => b.textContent === 'Move to Trash'),
    ).toBe(false);
  });
});
