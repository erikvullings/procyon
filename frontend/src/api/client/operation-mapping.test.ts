import { describe, expect, it } from 'vitest';
import type { OperationDto } from '../generated/models/operationDto';
import { operationFromDto } from './operation-mapping';

const entry = { id: 'e1', location: { providerId: 'local', uri: 'file:///a' } };

function dto(overrides: Partial<OperationDto>): OperationDto {
  return {
    id: 'op',
    type: 'delete',
    state: 'completedWithWarnings',
    sources: [],
    progress: { completedItems: 1, completedBytes: 0 },
    conflictPolicy: 'ask',
    createdAt: '2026-01-01T00:00:00Z',
    errors: [],
    undo: { available: false },
    ...overrides,
  } as OperationDto;
}

describe('operationFromDto', () => {
  it('carries stable codes and params for the result, entry errors and undo reason', () => {
    const operation = operationFromDto(
      dto({
        resultSummary: 'Completed with 1 warnings.',
        resultSummaryCode: 'completedWithWarnings',
        resultSummaryParams: { count: '1' },
        errors: [{ entry, message: 'gone', code: 'attributeEntryMissing', params: { name: 'a' } }],
        undo: { available: false, reason: 'No.', reasonCode: 'undoUnsafe' },
      }),
    );

    expect(operation.result).toEqual({
      message: 'Completed with 1 warnings.',
      code: 'completedWithWarnings',
      params: { count: '1' },
    });
    expect(operation.errors?.[0]).toMatchObject({
      code: 'attributeEntryMissing',
      params: { name: 'a' },
    });
    expect(operation.undo?.reasonCode).toBe('undoUnsafe');
  });

  it('omits codes the backend did not send', () => {
    const operation = operationFromDto(
      dto({ resultSummary: 'Operation failed.', errors: [{ entry, message: 'x' }] }),
    );
    expect(operation.result).toEqual({ message: 'Operation failed.' });
    expect(operation.errors?.[0]).toEqual({ entry, message: 'x' });
  });
});
