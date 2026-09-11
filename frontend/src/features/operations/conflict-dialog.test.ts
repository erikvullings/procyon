import m from 'mithril';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { OperationId } from '../../models';
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

  it('cancels the pending operation when Escape closes the modal', () => {
    const onResolve = vi.fn();
    mountedRoot = document.createElement('div');
    document.body.appendChild(mountedRoot);
    m.mount(mountedRoot, {
      view: () =>
        m(ConflictDialog, {
          conflict: {
            operationId: 'operation-1' as OperationId,
            conflictId: 'conflict-1',
            message: 'Destination exists.',
            source: { name: 'source.txt', kind: 'file' },
            destination: { name: 'source.txt', kind: 'file' },
          },
          onResolve,
        }),
    });
    m.redraw.sync();

    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    );

    expect(onResolve).toHaveBeenCalledWith('cancelOperation', false);
  });
});
