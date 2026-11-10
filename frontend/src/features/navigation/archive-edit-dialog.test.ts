import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ArchiveEditDialog } from './archive-edit-dialog';

let root: HTMLElement;

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

function mountDialog(overrides: { error?: string } = {}) {
  const handlers = { onSave: vi.fn(), onDiscard: vi.fn(), onLater: vi.fn() };
  m.mount(root, {
    view: () =>
      m(ArchiveEditDialog, { open: true, archiveName: 'inner.zip', ...overrides, ...handlers }),
  });
  m.redraw.sync();
  return handlers;
}

function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`button "${label}" missing`);
  return found;
}

describe('ArchiveEditDialog', () => {
  it('names the nested archive and warns that saving cannot be undone', () => {
    mountDialog();
    expect(document.body.textContent).toContain('Save changes to inner.zip?');
    expect(document.body.textContent).toContain('cannot be undone');
  });

  it('routes each button to its own handler, never saving implicitly', () => {
    const handlers = mountDialog();
    button('Not now').click();
    expect(handlers.onLater).toHaveBeenCalledTimes(1);
    expect(handlers.onSave).not.toHaveBeenCalled();
    button('Discard changes').click();
    expect(handlers.onDiscard).toHaveBeenCalledTimes(1);
    button('Save changes').click();
    expect(handlers.onSave).toHaveBeenCalledTimes(1);
  });

  it('shows why a save failed', () => {
    mountDialog({ error: 'the containing archive changed' });
    expect(document.body.textContent).toContain('the containing archive changed');
  });
});
