import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OperationConflict, OperationId } from '../../models';
import { ConflictDialog, formatConflictMetadata } from './conflict-dialog';

let mountedRoot: HTMLElement | undefined;

describe('formatConflictMetadata', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    if (mountedRoot !== undefined) {
      m.mount(mountedRoot, null);
      mountedRoot.remove();
      mountedRoot = undefined;
    }
    document.querySelector('[data-test-conflict-trigger]')?.remove();
  });

  it('uses compact bytes and second-precision timestamps in the local time zone', () => {
    // 2026-07-30 is in EDT (UTC-4): the UTC instant below must render as 10:17:06 local,
    // not the underlying 14:17:06 UTC value — this is the regression check for the bug
    // where conflict metadata displayed raw UTC time instead of the viewer's local time.
    vi.stubEnv('TZ', 'America/New_York');
    expect(
      formatConflictMetadata({
        name: 'locations.md',
        size: 1648,
        modifiedAt: '2026-07-30T14:17:06.901716538Z',
        kind: 'file',
      }),
    ).toBe('locations.md · 1648b · 2026-07-30 10:17:06');
  });

  it('renders a different local time in a different time zone for the same instant', () => {
    vi.stubEnv('TZ', 'Asia/Tokyo');
    expect(
      formatConflictMetadata({
        name: 'locations.md',
        size: 1648,
        modifiedAt: '2026-07-30T14:17:06.901716538Z',
        kind: 'file',
      }),
    ).toBe('locations.md · 1648b · 2026-07-30 23:17:06');
  });

  it('reports missing size and modified time explicitly', () => {
    expect(
      formatConflictMetadata({
        name: 'untitled',
        kind: 'file',
      }),
    ).toBe('untitled · size unavailable · modified time unavailable');
  });

  it('cancels the pending operation when Escape closes the modal', async () => {
    let conflict: OperationConflict | undefined = {
      operationId: 'operation-1' as OperationId,
      conflictId: 'conflict-1',
      message: 'Destination exists.',
      source: { name: 'source.txt', kind: 'file' },
      destination: { name: 'source.txt', kind: 'file' },
    };
    const onResolve = vi.fn(() => {
      conflict = undefined;
    });
    const trigger = document.createElement('button');
    trigger.textContent = 'Start copy';
    trigger.dataset.testConflictTrigger = '';
    document.body.appendChild(trigger);
    trigger.focus();
    mountedRoot = document.createElement('div');
    document.body.appendChild(mountedRoot);
    m.mount(mountedRoot, {
      view: () =>
        m(ConflictDialog, {
          conflict,
          onResolve,
        }),
    });
    m.redraw.sync();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    expect(onResolve).toHaveBeenCalledWith('cancelOperation', false);
    expect(document.activeElement).toBe(trigger);
  });

  it('announces the problem and focuses the safest conflict resolution', async () => {
    mountedRoot = document.createElement('div');
    document.body.appendChild(mountedRoot);
    m.mount(mountedRoot, {
      view: () =>
        m(ConflictDialog, {
          conflict: {
            operationId: 'operation-1' as OperationId,
            conflictId: 'conflict-1',
            message: 'Destination exists.',
            source: { name: 'incoming-report-with-a-very-long-name.txt', kind: 'file' },
            destination: { name: 'existing-report.txt', kind: 'file' },
          },
          onResolve: vi.fn(),
        }),
    });
    m.redraw.sync();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    const dialog = mountedRoot.querySelector('[role="alertdialog"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.querySelector('.fm-conflict-dialog-problem')?.textContent).toBe(
      'Destination exists.',
    );
    expect(dialog?.textContent).toContain('Safest choice: rename the incoming item to keep both.');
    const recommended = dialog?.querySelector<HTMLButtonElement>('.fm-conflict-recommended');
    expect(recommended?.textContent).toBe('Rename new');
    expect(document.activeElement).toBe(recommended);
  });
});
