import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BackendEvent, Operation } from '../../models';
import { OperationCentre } from './operation-centre';
import {
  createOperationsState,
  reduceOperationEvents,
  transitionOperationState,
} from './operation-state';

const operation = (state: Operation['state'], id: string = state): Operation => ({
  id,
  kind: 'copy',
  state,
  sources: [],
  destination: { providerId: 'local', uri: 'file:///Archive' },
  progress: {
    completedItems: 2,
    totalItems: 4,
    completedBytes: 1_024,
    totalBytes: 2_048,
    bytesPerSecond: 512,
    currentEntry: {
      id: 'report',
      location: { providerId: 'local', uri: 'file:///Documents/report.pdf' },
    },
  },
  conflictPolicy: 'ask',
  createdAt: '2026-07-31T10:00:00Z',
});

const event = (eventId: number, payload: BackendEvent['payload']): BackendEvent => ({
  eventId,
  timestamp: '2026-07-31T10:00:00Z',
  payload,
});

describe('operation progress reducer', () => {
  it('batches realistically interleaved operation events without assuming group order', () => {
    const initial = createOperationsState([operation('queued', 'a'), operation('running', 'b')]);
    const next = reduceOperationEvents(initial, [
      event(1, {
        type: 'operation.progress',
        operationId: 'b',
        progress: { completedItems: 3, completedBytes: 1_536, bytesPerSecond: 600 },
      }),
      event(2, { type: 'operation.stateChanged', operationId: 'a', state: 'running' }),
      event(3, {
        type: 'operation.progress',
        operationId: 'a',
        progress: { completedItems: 1, completedBytes: 256 },
      }),
      event(4, {
        type: 'operation.completed',
        operation: {
          ...operation('completed', 'b'),
          undo: { available: true },
        },
      }),
    ]);

    expect(next.byId.a).toMatchObject({ state: 'running', progress: { completedItems: 1 } });
    expect(next.byId.b).toMatchObject({
      state: 'completed',
      progress: { completedItems: 2 },
      undo: { available: true },
    });
  });

  it('retains failure details and ignores progress for unknown operations', () => {
    const next = reduceOperationEvents(createOperationsState([operation('running', 'a')]), [
      event(1, {
        type: 'operation.progress',
        operationId: 'missing',
        progress: { completedItems: 9, completedBytes: 9 },
      }),
      event(2, {
        type: 'operation.failed',
        operationId: 'a',
        code: 'permissionDenied',
        message: 'Could not copy report.pdf.',
        details: { reason: 'Permission denied' },
      }),
    ]);

    expect(next.byId.missing).toBeUndefined();
    expect(next.byId.a?.state).toBe('failed');
    expect(next.failuresById.a).toEqual({
      code: 'permissionDenied',
      message: 'Could not copy report.pdf.',
      details: { reason: 'Permission denied' },
    });
  });

  it('preserves planned totals across incremental progress and pause/resume transitions', () => {
    const initial = createOperationsState([operation('running', 'copy')]);
    const paused = reduceOperationEvents(initial, [
      event(1, {
        type: 'operation.progress',
        operationId: 'copy',
        progress: { completedItems: 3, completedBytes: 1_536 },
      }),
      event(2, { type: 'operation.stateChanged', operationId: 'copy', state: 'paused' }),
    ]);
    const resumed = reduceOperationEvents(paused, [
      event(3, { type: 'operation.stateChanged', operationId: 'copy', state: 'running' }),
    ]);

    expect(resumed.byId.copy).toMatchObject({
      state: 'running',
      progress: {
        completedItems: 3,
        totalItems: 4,
        completedBytes: 1_536,
        totalBytes: 2_048,
      },
    });
  });

  it('applies an immediate local state transition without changing progress', () => {
    const initial = createOperationsState([operation('running', 'copy')]);

    const cancelling = transitionOperationState(initial, 'copy', 'cancelling');

    expect(cancelling.byId.copy).toMatchObject({
      state: 'cancelling',
      progress: initial.byId.copy?.progress,
    });
  });
});

describe('OperationCentre states', () => {
  let root: HTMLElement;

  beforeEach(() => {
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  it('offers an accessible close control without affecting operation actions', () => {
    const onClose = vi.fn();

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
          onClose,
        }),
    });

    const close = root.querySelector<HTMLButtonElement>('.fm-operation-centre-close');
    expect(close?.getAttribute('aria-label')).toBe('Hide Operations Centre');
    close?.click();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it('shows queued, running, paused, completed, and failed states with appropriate controls', () => {
    const operations = [
      operation('queued'),
      operation('running'),
      operation('paused'),
      { ...operation('completed'), result: { message: 'Copied 4 items.' } },
      operation('failed'),
    ];
    const onCancel = vi.fn();
    const onPause = vi.fn();
    const onResume = vi.fn();
    const onDismiss = vi.fn();

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: {
            ...createOperationsState(operations),
            failuresById: {
              failed: {
                code: 'permissionDenied',
                message: 'Could not copy report.pdf.',
                details: { reason: 'Permission denied' },
              },
            },
          },
          onCancel,
          onPause,
          onResume,
          onDismiss,
        }),
    });

    expect(root.querySelectorAll('.fm-operation')).toHaveLength(5);
    expect(root.textContent).toContain('report.pdf');
    expect(root.textContent).toContain('512 B/s');
    expect(root.querySelector('[data-operation-id="completed"] .fm-operation-result')).toBeNull();
    expect(root.textContent).toContain('Could not copy report.pdf.');
    expect(root.querySelector('details')?.textContent).toContain('Permission denied');
    expect(
      root.querySelector('[data-operation-id="queued"] [data-action="cancel"]'),
    ).not.toBeNull();
    expect(
      root.querySelector('[data-operation-id="running"] [data-action="pause"]'),
    ).not.toBeNull();
    expect(
      root.querySelector('[data-operation-id="paused"] [data-action="resume"]'),
    ).not.toBeNull();
    expect(
      root.querySelector('[data-operation-id="completed"] [data-action="dismiss"]'),
    ).not.toBeNull();
    expect(
      root.querySelector('[data-operation-id="failed"] [data-action="dismiss"]'),
    ).not.toBeNull();
  });

  it('makes partial results explicit for a cancelled operation', () => {
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([operation('cancelled')]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    const result = root.querySelector('[data-operation-id="cancelled"] .fm-operation-result');
    expect(result?.textContent).toBe('Cancelled after 2 / 4 items (1 K / 2 K).');
  });

  it('uses a status icon and compact totals for completed operations', () => {
    const completed: Operation = {
      ...operation('completed'),
      progress: {
        ...operation('completed').progress,
        completedItems: 21,
        totalItems: 21,
        completedBytes: 36_400_000,
        totalBytes: 36_400_000,
      },
    };
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([completed]),
          formatSettings: { sizeFormat: 'decimal', dateFormat: 'medium', locale: 'en-US' },
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    const summary = root.querySelector('.fm-operation-summary');
    expect(summary?.textContent).toContain('Copy✓');
    expect(summary?.textContent).toContain('21 items');
    expect(summary?.textContent).toContain('36.4 M');
    expect(summary?.textContent).not.toContain('Completed');
    expect(summary?.textContent).not.toContain('21 / 21');
    expect(summary?.querySelector('.fm-operation-state')?.getAttribute('aria-label')).toBe(
      'Completed',
    );
  });

  it('renders an empty state when there are no operations', () => {
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    expect(root.querySelector('.fm-operation-centre')).not.toBeNull();
    expect(root.textContent).toBe('No operations to show.');
  });

  it('shows a match count instead of the current-entry filename for a running search', () => {
    const search: Operation = {
      ...operation('running', 'search'),
      kind: 'search',
      progress: {
        ...operation('running').progress,
        completedItems: 15,
      },
    };

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([search]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    const summary = root.querySelector('[data-operation-id="search"] .fm-operation-summary');
    expect(summary?.textContent).toContain('15 files found');
    // Neither the scanned-entry filename nor the generic "items"/bytes readout should show.
    expect(summary?.textContent).not.toContain('report.pdf');
    expect(summary?.textContent).not.toContain('items');
    expect(summary?.textContent).not.toContain('B');
  });

  it('reports a cancelled search in files, not bytes', () => {
    const search: Operation = { ...operation('cancelled', 'search'), kind: 'search' };

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([search]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    const result = root.querySelector('[data-operation-id="search"] .fm-operation-result');
    expect(result?.textContent).toBe('Cancelled after finding 2 / 4 files.');
  });

  it('shows entry-level warning details for completedWithWarnings operations', () => {
    const withWarnings: Operation = {
      ...operation('completedWithWarnings', 'warned-copy'),
      errors: [
        {
          entry: {
            id: 'existing-folder',
            location: { providerId: 'sftp', uri: 'sftp://server/home/demo/existing-folder' },
          },
          message: 'Skipped because destination already exists.',
        },
      ],
    };

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([withWarnings]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });

    expect(root.querySelector('[data-operation-id="warned-copy"] .fm-operation-result')).toBeNull();
    const warningText = root.querySelector(
      '[data-operation-id="warned-copy"] .fm-operation-warning',
    );
    expect(warningText?.textContent).toContain(
      'existing-folder: Skipped because destination already exists.',
    );
  });

  it('offers undo only when history marks it safe and explains unavailable entries', () => {
    const onUndo = vi.fn();
    const undoable: Operation = {
      ...operation('completed', 'undoable'),
      sources: [
        {
          id: 'report',
          location: { providerId: 'local', uri: 'file:///Documents/report.pdf' },
        },
        {
          id: 'notes',
          location: { providerId: 'local', uri: 'file:///Documents/notes.txt' },
        },
      ],
      completedAt: '2026-08-31T15:00:02Z',
      undo: { available: true },
    };
    const irreversible: Operation = {
      ...operation('completed', 'irreversible'),
      undo: { available: false, reason: 'Permanent delete cannot be undone.' },
    };

    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([undoable, irreversible]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onUndo,
          onDismiss: vi.fn(),
        }),
    });

    root
      .querySelector<HTMLButtonElement>('[data-operation-id="undoable"] [data-action="undo"]')
      ?.click();
    expect(onUndo).toHaveBeenCalledWith('undoable');
    expect(root.querySelector('[data-operation-id="undoable"] [data-action="dismiss"]')).toBeNull();
    expect(
      root.querySelector<HTMLTimeElement>('[data-operation-id="undoable"] time')?.dateTime,
    ).toBe('2026-08-31T15:00:02Z');
    const sources = root.querySelector('[data-operation-id="undoable"] .fm-operation-sources');
    expect(sources?.tagName).toBe('DETAILS');
    expect(sources?.hasAttribute('open')).toBe(false);
    expect(sources?.querySelector('.fm-operation-source-preview')?.textContent).toBe(
      'report.pdf, notes.txt',
    );
    expect(sources?.querySelector('summary')?.getAttribute('aria-label')).toBe(
      'Show or hide file list',
    );
    expect([...(sources?.querySelectorAll('li') ?? [])].map((item) => item.textContent)).toEqual([
      'report.pdf',
      'notes.txt',
    ]);
    expect(root.querySelector('[data-operation-id="undoable"] progress')).toBeNull();
    expect(root.querySelector('[data-operation-id="undoable"] .fm-operation-result')).toBeNull();
    expect(
      root.querySelector('[data-operation-id="irreversible"] [data-action="undo"]'),
    ).toBeNull();
    expect(
      root.querySelector('[data-operation-id="irreversible"] [data-action="dismiss"]'),
    ).not.toBeNull();
    expect(
      root.querySelector('[data-operation-id="irreversible"] .fm-operation-undo-reason')
        ?.textContent,
    ).toBe('Permanent delete cannot be undone.');
  });
});
