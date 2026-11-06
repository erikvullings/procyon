import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { setLocale } from '../../i18n';
import type { Location, Operation } from '../../models';
import { OperationCentre } from './operation-centre';
import { OperationConfirmationDialog } from './operation-confirmation-dialog';
import { createOperationsState } from './operation-state';
import { readVerifyTransfers, writeVerifyTransfers } from './verify-preference';

const source: Location = { providerId: 'local', uri: 'file:///a/file.txt' };
const destination: Location = { providerId: 'local', uri: 'file:///b' };

const completedCopy = (verification: Operation['verification']): Operation => ({
  id: 'copy-1',
  kind: 'copy',
  state: 'completed',
  sources: [],
  destination,
  progress: { completedItems: 3, totalItems: 3, completedBytes: 30, totalBytes: 30 },
  conflictPolicy: 'ask',
  createdAt: '2026-07-31T10:00:00Z',
  ...(verification === undefined ? {} : { verification }),
});

describe('verify copied files', () => {
  let root: HTMLElement;

  beforeEach(() => {
    setLocale('en');
    window.localStorage.clear();
    root = document.createElement('div');
    document.body.appendChild(root);
  });

  afterEach(() => {
    m.mount(root, null);
    root.remove();
    window.localStorage.clear();
  });

  it('is off until the user ticks it, and remembers the choice', () => {
    expect(readVerifyTransfers()).toBe(false);
    writeVerifyTransfers(true);
    expect(readVerifyTransfers()).toBe(true);
    writeVerifyTransfers(false);
    expect(readVerifyTransfers()).toBe(false);
  });

  it('survives storage that throws', () => {
    const broken = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(readVerifyTransfers()).toBe(false);
    broken.mockRestore();
  });

  it('offers the option for a copy confirmation only, and stores what the user picks', () => {
    const mountDialog = (kind: 'copy' | 'move') => {
      m.mount(root, {
        view: () =>
          m(OperationConfirmationDialog, {
            request: { kind, sources: [source], destination },
            connections: [],
            onConfirm: vi.fn(),
            onCancel: vi.fn(),
          }),
      });
      m.redraw.sync();
    };
    mountDialog('move');
    expect(document.querySelector('.fm-operation-confirmation-verify')).toBeNull();

    mountDialog('copy');
    const box = document.querySelector<HTMLInputElement>(
      '.fm-operation-confirmation-verify input[type="checkbox"]',
    );
    expect(box).not.toBeNull();
    expect(box?.checked).toBe(false);
    box?.click();
    expect(readVerifyTransfers()).toBe(true);
  });

  it('shows how verification went on the operation card', () => {
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([
            completedCopy({ verified: 2, mismatched: 1, notVerified: 0, skipped: 0 }),
          ]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });
    const line = root.querySelector('.fm-operation-verification');
    expect(line?.textContent).toBe(
      'Verification: 2 matched, 1 differed and were not saved, 0 could not be checked.',
    );
    expect(line?.classList.contains('fm-operation-verification-mismatch')).toBe(true);
  });

  it('stays quiet when verification was not requested', () => {
    m.mount(root, {
      view: () =>
        m(OperationCentre, {
          state: createOperationsState([completedCopy(undefined)]),
          onCancel: vi.fn(),
          onPause: vi.fn(),
          onResume: vi.fn(),
          onDismiss: vi.fn(),
        }),
    });
    expect(root.querySelector('.fm-operation-verification')).toBeNull();
  });
});
