import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setLocale } from '../../i18n';
import type { InterruptedTransfer } from '../../models';
import { InterruptedTransfers } from './interrupted-transfers';
import { OperationCentre } from './operation-centre';
import { createOperationsState } from './operation-state';

const transfer = (
  overrides: Partial<InterruptedTransfer> & Pick<InterruptedTransfer, 'operationId'>,
): InterruptedTransfer => ({
  kind: 'copy',
  createdAt: '2026-11-05T10:00:00Z',
  class: 'safelyRestartable',
  reason: 'partialUnfinished',
  sources: [{ providerId: 'local', uri: 'file:///Photos/a.jpg' }],
  destination: { providerId: 'local', uri: 'file:///Backup' },
  totalItems: 4,
  committedItems: 1,
  unconfirmedItems: 0,
  partialItems: 1,
  remainingItems: 3,
  ...overrides,
});

describe('InterruptedTransfers', () => {
  let root: HTMLElement;

  beforeEach(() => {
    setLocale('en');
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
  });

  function mount(
    transfers: readonly InterruptedTransfer[],
    extra: Partial<Parameters<typeof InterruptedTransfers.view>[0]['attrs']> = {},
  ) {
    const onRestart = vi.fn();
    const onDiscard = vi.fn();
    m.mount(root, {
      view: () => m(InterruptedTransfers, { transfers, onRestart, onDiscard, ...extra }),
    });
    return { onRestart, onDiscard };
  }

  it('renders nothing when nothing was interrupted', () => {
    mount([]);
    expect(root.querySelector('.fm-interrupted-transfers')).toBeNull();
  });

  it('says what happened and what is safe, using the reason and the counts', () => {
    mount([transfer({ operationId: 'one' })]);
    const text = root.textContent ?? '';
    expect(text).toContain('Interrupted transfers');
    expect(text).toContain('Safe to run again');
    expect(text).toContain('A file was half copied');
    expect(text).toContain('1 of 4 items finished, 3 still to do');
    expect(text).toContain('file:///Photos/a.jpg → file:///Backup');
  });

  it('runs nothing by itself and wires the two explicit actions to the right transfer', () => {
    const { onRestart, onDiscard } = mount([
      transfer({ operationId: 'first' }),
      transfer({ operationId: 'second', class: 'needsIntervention', reason: 'sourceChanged' }),
    ]);
    expect(onRestart).not.toHaveBeenCalled();
    const items = root.querySelectorAll('.fm-interrupted-item');
    const [first, second] = [items[0], items[1]];
    first?.querySelectorAll('button')[0]?.click();
    second?.querySelectorAll('button')[1]?.click();
    expect(onRestart).toHaveBeenCalledWith('first');
    expect(onDiscard).toHaveBeenCalledWith('second');
  });

  it('asks for an informed choice when a transfer needs a decision', () => {
    mount([transfer({ operationId: 'a', class: 'needsIntervention', reason: 'sourceMissing' })]);
    const buttons = [...root.querySelectorAll('button')].map((button) => button.textContent);
    expect(buttons).toEqual(['Run again anyway', 'Discard']);
    expect(root.textContent).toContain('Needs your decision');
  });

  it('offers to confirm instead of re-running a transfer that probably finished', () => {
    mount([
      transfer({
        operationId: 'b',
        class: 'completedUnconfirmed',
        reason: 'publishedUnconfirmed',
        committedItems: 0,
        unconfirmedItems: 4,
        remainingItems: 0,
      }),
    ]);
    const buttons = [...root.querySelectorAll('button')].map((button) => button.textContent);
    expect(buttons).toEqual(['Run again', 'Mark as done']);
    expect(root.textContent).toContain('4 of 4 items finished, 0 still to do');
  });

  it('disables only the transfer an action is running for and shows a failure', () => {
    mount([transfer({ operationId: 'busy' }), transfer({ operationId: 'idle' })], {
      busyId: 'busy',
      error: 'That action could not be completed. Try again.',
    });
    const items = root.querySelectorAll('.fm-interrupted-item');
    const busyButtons = [...(items[0]?.querySelectorAll('button') ?? [])];
    const idleButtons = [...(items[1]?.querySelectorAll('button') ?? [])];
    expect(busyButtons.every((button) => button.disabled)).toBe(true);
    expect(idleButtons.every((button) => !button.disabled)).toBe(true);
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('could not be completed');
  });

  it('shows in the operation centre ahead of the regular operations list', () => {
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
          interrupted: {
            transfers: [transfer({ operationId: 'centre' })],
            onRestart: vi.fn(),
            onDiscard: vi.fn(),
          },
        }),
    });
    const centre = root.querySelector('.fm-operation-centre');
    expect(centre?.firstElementChild?.classList.contains('fm-interrupted-transfers')).toBe(true);
  });
});
