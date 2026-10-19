import { describe, expect, it } from 'vitest';
import type { Operation } from '../../models';
import { requestSkippingSpaceCheck, spaceShortfallOf } from './insufficient-space';

const operation = {
  id: 'op',
  kind: 'copy',
  state: 'failed',
  sources: [{ id: 'e', location: { providerId: 'local', uri: 'file:///a' } }],
  destination: { providerId: 'local', uri: 'file:///dest' },
  conflictPolicy: 'ask',
} as unknown as Operation;

describe('spaceShortfallOf', () => {
  it('computes the shortfall of an insufficientSpace failure', () => {
    expect(
      spaceShortfallOf(operation, {
        code: 'insufficientSpace',
        message: 'x',
        details: { required: 200, available: 50 },
      }),
    ).toMatchObject({ required: 200, available: 50, shortfall: 150 });
  });

  it('ignores other failures, missing numbers and non-transfer operations', () => {
    expect(spaceShortfallOf(operation, { code: 'io', message: 'x' })).toBeUndefined();
    expect(
      spaceShortfallOf(operation, { code: 'insufficientSpace', message: 'x', details: {} }),
    ).toBeUndefined();
    expect(
      spaceShortfallOf({ ...operation, kind: 'delete' } as Operation, {
        code: 'insufficientSpace',
        message: 'x',
        details: { required: 2, available: 1 },
      }),
    ).toBeUndefined();
  });
});

describe('requestSkippingSpaceCheck', () => {
  it('repeats the operation with the check skipped', () => {
    expect(requestSkippingSpaceCheck(operation)).toEqual({
      type: 'copy',
      sources: [{ providerId: 'local', uri: 'file:///a' }],
      destination: { providerId: 'local', uri: 'file:///dest' },
      conflictPolicy: 'ask',
      skipSpaceCheck: true,
    });
  });
});
