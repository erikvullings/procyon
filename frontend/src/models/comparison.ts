import type { ComparisonEntryDto } from '../api/generated/models/comparisonEntryDto';
import type { ComparisonEntrySideDto } from '../api/generated/models/comparisonEntrySideDto';
import type { SyncPlanItemDto } from '../api/generated/models/syncPlanItemDto';
import type { EntryKind } from './entry';

/** How two directory trees are compared (spec §16 milestone 5, task 0075). */
export type ComparisonCriteria = 'nameOnly' | 'sizeAndTimestamp' | 'contentHash';

/** Per-entry comparison outcome. */
export type ComparisonStatus =
  | 'onlyLeft'
  | 'onlyRight'
  | 'newer'
  | 'older'
  | 'differentSize'
  | 'identical'
  | 'typeMismatch';

/** One side's metadata for a compared entry. */
export interface ComparisonEntrySide {
  kind: EntryKind;
  size?: number;
  modifiedAt?: string;
  contentHash?: string;
}

/** Converts the wire DTO into the frontend model, normalizing `null` to omitted. */
export function comparisonEntrySideFromDto(dto: ComparisonEntrySideDto): ComparisonEntrySide {
  return {
    kind: dto.kind,
    ...(dto.size == null ? {} : { size: dto.size }),
    ...(dto.modifiedAt == null ? {} : { modifiedAt: dto.modifiedAt }),
    ...(dto.contentHash == null ? {} : { contentHash: dto.contentHash }),
  };
}

/** One compared path, relative to both roots. */
export interface ComparisonEntry {
  relativePath: string;
  left?: ComparisonEntrySide;
  right?: ComparisonEntrySide;
  status: ComparisonStatus;
}

/** Converts the wire DTO into the frontend model, normalizing `null` to omitted. */
export function comparisonEntryFromDto(dto: ComparisonEntryDto): ComparisonEntry {
  return {
    relativePath: dto.relativePath,
    status: dto.status,
    ...(dto.left == null ? {} : { left: comparisonEntrySideFromDto(dto.left) }),
    ...(dto.right == null ? {} : { right: comparisonEntrySideFromDto(dto.right) }),
  };
}

/** Which side is authoritative when a sync plan proposes actions. */
export type SyncMode = 'mirrorLeftToRight' | 'mirrorRightToLeft' | 'twoWayUpdate';

/** A proposed (and, before applying, user-editable) action for one entry. */
export type SyncAction =
  | 'copyLeftToRight'
  | 'copyRightToLeft'
  | 'deleteLeft'
  | 'deleteRight'
  | 'skip';

/** One row of a sync plan. */
export interface SyncPlanItem {
  relativePath: string;
  status: ComparisonStatus;
  action: SyncAction;
  left?: ComparisonEntrySide;
  right?: ComparisonEntrySide;
}

/** Converts the wire DTO into the frontend model, normalizing `null` to omitted. */
export function syncPlanItemFromDto(dto: SyncPlanItemDto): SyncPlanItem {
  return {
    relativePath: dto.relativePath,
    status: dto.status,
    action: dto.action,
    ...(dto.left == null ? {} : { left: comparisonEntrySideFromDto(dto.left) }),
    ...(dto.right == null ? {} : { right: comparisonEntrySideFromDto(dto.right) }),
  };
}

/** Converts a sync plan item back to the wire DTO shape for `applySyncPlan`. */
export function syncPlanItemToDto(item: SyncPlanItem): SyncPlanItemDto {
  return {
    relativePath: item.relativePath,
    status: item.status,
    action: item.action,
    ...(item.left === undefined ? {} : { left: item.left }),
    ...(item.right === undefined ? {} : { right: item.right }),
  };
}
