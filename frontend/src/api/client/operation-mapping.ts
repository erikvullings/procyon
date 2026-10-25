import type { Operation } from '../../models';
import type { OperationDto } from '../generated/models/operationDto';

export function operationFromDto(dto: OperationDto): Operation {
  return {
    id: dto.id,
    kind: dto.type,
    state: dto.state,
    sources: dto.sources,
    ...(dto.destination == null ? {} : { destination: dto.destination }),
    progress: {
      completedItems: dto.progress.completedItems,
      completedBytes: dto.progress.completedBytes,
      ...(dto.progress.totalItems == null ? {} : { totalItems: dto.progress.totalItems }),
      ...(dto.progress.totalBytes == null ? {} : { totalBytes: dto.progress.totalBytes }),
      ...(dto.progress.currentEntry == null ? {} : { currentEntry: dto.progress.currentEntry }),
      ...(dto.progress.bytesPerSecond == null
        ? {}
        : { bytesPerSecond: dto.progress.bytesPerSecond }),
    },
    conflictPolicy: dto.conflictPolicy,
    createdAt: dto.createdAt,
    ...(dto.startedAt == null ? {} : { startedAt: dto.startedAt }),
    ...(dto.completedAt == null ? {} : { completedAt: dto.completedAt }),
    ...(dto.queuePosition == null ? {} : { queuePosition: dto.queuePosition }),
    ...(dto.resultSummary == null
      ? {}
      : {
          result: {
            message: dto.resultSummary,
            ...(dto.resultSummaryCode == null ? {} : { code: dto.resultSummaryCode }),
            ...(dto.resultSummaryParams == null ? {} : { params: dto.resultSummaryParams }),
          },
        }),
    ...(dto.errors.length === 0
      ? {}
      : {
          errors: dto.errors.map((error) => ({
            entry: error.entry,
            message: error.message,
            ...(error.code == null ? {} : { code: error.code }),
            ...(error.params == null ? {} : { params: error.params }),
          })),
        }),
    undo: {
      available: dto.undo.available,
      ...(dto.undo.reason == null ? {} : { reason: dto.undo.reason }),
      ...(dto.undo.reasonCode == null ? {} : { reasonCode: dto.undo.reasonCode }),
      ...(dto.undo.operationId == null ? {} : { operationId: dto.undo.operationId }),
    },
    ...(dto.undoOf == null ? {} : { undoOf: dto.undoOf }),
  };
}
