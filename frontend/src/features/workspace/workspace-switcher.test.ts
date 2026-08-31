import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { WorkspaceSummary } from '../../models';
import { WorkspaceSwitcher, type WorkspaceSwitcherAttrs } from './workspace-switcher';

let root: HTMLElement;

function summary(id: string, name: string, revision = 1, ephemeral = false): WorkspaceSummary {
  return { id, name, revision, ephemeral, updatedAt: '2026-01-01T00:00:00.000Z' };
}

function mount(attrs: Partial<WorkspaceSwitcherAttrs> = {}): void {
  const merged: WorkspaceSwitcherAttrs = {
    summaries: [summary('workspace-1', 'Alpha'), summary('workspace-2', 'Bravo')],
    activeWorkspaceId: 'workspace-1',
    error: undefined,
    onSwitch: vi.fn(),
    onCreate: vi.fn(),
    onRename: vi.fn(),
    onDelete: vi.fn(),
    ...attrs,
  };
  m.mount(root, { view: () => m(WorkspaceSwitcher, merged) });
  m.redraw.sync();
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function row(workspaceId: string): HTMLElement {
  const found = root.querySelector<HTMLElement>(`[data-workspace-id="${workspaceId}"]`);
  if (found === null) throw new Error(`row for ${workspaceId} not found`);
  return found;
}

describe('WorkspaceSwitcher', () => {
  it('lists every workspace summary and marks the active one', () => {
    mount();

    expect(root.textContent).toContain('Alpha');
    expect(root.textContent).toContain('Bravo');
    expect(row('workspace-1').dataset.active).toBe('true');
    expect(row('workspace-2').dataset.active).toBe('false');
  });

  it('switches to a workspace when its name is clicked', () => {
    const onSwitch = vi.fn();
    mount({ onSwitch });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-switcher-name')?.click();

    expect(onSwitch).toHaveBeenCalledWith('workspace-2');
  });

  it('does not switch when the already-active workspace is clicked', () => {
    const onSwitch = vi.fn();
    mount({ onSwitch });

    row('workspace-1').querySelector<HTMLButtonElement>('.fm-workspace-switcher-name')?.click();

    expect(onSwitch).not.toHaveBeenCalled();
  });

  it('creates a new workspace', () => {
    const onCreate = vi.fn();
    mount({ onCreate });

    [...root.querySelectorAll('button')].find((b) => b.textContent === 'New workspace')?.click();

    expect(onCreate).toHaveBeenCalledOnce();
  });

  it('renames a workspace through the inline form', () => {
    const onRename = vi.fn();
    mount({ onRename });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-rename-button')?.click();
    m.redraw.sync();
    const input = row('workspace-2').querySelector<HTMLInputElement>('input[type="text"]');
    if (input === null) throw new Error('rename input missing');
    input.value = 'Bravo renamed';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    row('workspace-2')
      .querySelector<HTMLFormElement>('.fm-workspace-rename-form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    expect(onRename).toHaveBeenCalledWith('workspace-2', 'Bravo renamed');
  });

  it('ignores a rename submission that trims to an empty name', () => {
    const onRename = vi.fn();
    mount({ onRename });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-rename-button')?.click();
    m.redraw.sync();
    const input = row('workspace-2').querySelector<HTMLInputElement>('input[type="text"]');
    if (input === null) throw new Error('rename input missing');
    input.value = '   ';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    root
      .querySelector<HTMLFormElement>('.fm-workspace-rename-form')
      ?.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));

    expect(onRename).not.toHaveBeenCalled();
  });

  it('cancels a rename in progress without invoking onRename', () => {
    const onRename = vi.fn();
    mount({ onRename });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-rename-button')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')?.click();
    m.redraw.sync();

    expect(root.querySelector('.fm-workspace-rename-form')).toBeNull();
    expect(onRename).not.toHaveBeenCalled();
  });

  it('requires confirmation before deleting a workspace', () => {
    const onDelete = vi.fn();
    mount({ onDelete });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-delete-button')?.click();
    m.redraw.sync();

    expect(root.textContent).toContain('Bravo');
    expect(onDelete).not.toHaveBeenCalled();
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Delete')?.click();

    expect(onDelete).toHaveBeenCalledWith('workspace-2');
  });

  it('cancelling the delete confirmation does not invoke onDelete', () => {
    const onDelete = vi.fn();
    mount({ onDelete });

    row('workspace-2').querySelector<HTMLButtonElement>('.fm-workspace-delete-button')?.click();
    m.redraw.sync();
    [...root.querySelectorAll('button')].find((b) => b.textContent === 'Cancel')?.click();

    expect(onDelete).not.toHaveBeenCalled();
  });

  it('surfaces an actionable error message', () => {
    mount({ error: 'The workspace changed elsewhere; refresh and try again.' });

    const alert = root.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain('changed elsewhere');
  });

  it('shows an empty state when there are no persisted workspaces', () => {
    mount({ summaries: [], activeWorkspaceId: undefined });

    expect(root.textContent).toContain('No workspaces yet');
  });
});
