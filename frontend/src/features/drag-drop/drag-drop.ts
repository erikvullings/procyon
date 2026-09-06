import { t } from '../../i18n';
import type { EntrySummary, Location, OperationKind, RuntimeCapabilities } from '../../models';

export interface DropModifiers {
  readonly altKey: boolean;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
}

export interface DropEventState extends DropModifiers {
  readonly dataTransfer?: DataTransfer | null;
}

export type DropValidation =
  | { readonly ok: true }
  | { readonly ok: false; readonly message: string };

/** A directory row receives the drop; files and empty space resolve to the pane directory. */
export function resolveDropTarget(paneLocation: Location, row: EntrySummary | undefined): Location {
  return row?.kind === 'directory' ? row.location : paneLocation;
}

function isSameOrDescendant(source: Location, destination: Location): boolean {
  if (source.providerId !== destination.providerId) return false;
  try {
    const sourceUrl = new URL(source.uri);
    const destinationUrl = new URL(destination.uri);
    if (sourceUrl.origin !== destinationUrl.origin) return false;
    const root = sourceUrl.pathname.replace(/\/+$/u, '');
    const target = destinationUrl.pathname.replace(/\/+$/u, '');
    return target === root || target.startsWith(`${root}/`);
  } catch {
    return source.uri === destination.uri || destination.uri.startsWith(`${source.uri}/`);
  }
}

function isCurrentParent(source: Location, destination: Location): boolean {
  if (source.providerId !== destination.providerId) return false;
  try {
    const sourceUrl = new URL(source.uri);
    const destinationUrl = new URL(destination.uri);
    if (sourceUrl.origin !== destinationUrl.origin) return false;
    const sourcePath = sourceUrl.pathname.replace(/\/+$/u, '');
    const separator = sourcePath.lastIndexOf('/');
    const parentPath = sourcePath.slice(0, Math.max(separator, 0)) || '/';
    return destinationUrl.pathname.replace(/\/+$/u, '') === parentPath.replace(/\/+$/u, '');
  } catch {
    const sourceUri = source.uri.replace(/\/+$/u, '');
    const separator = sourceUri.lastIndexOf('/');
    return destination.uri.replace(/\/+$/u, '') === sourceUri.slice(0, Math.max(separator, 0));
  }
}

/** Validates before `dragover` is accepted, so invalid targets never appear droppable. */
export function validateDropTarget(
  sources: readonly Location[],
  target: Location | undefined,
  writable: boolean,
): DropValidation {
  if (sources.length === 0) return { ok: false, message: t('clipboard', 'nothingDragged') };
  if (target === undefined) return { ok: false, message: t('clipboard', 'destinationUnavailable') };
  if (!writable) return { ok: false, message: t('clipboard', 'destinationReadOnly') };
  if (sources.some((source) => isCurrentParent(source, target))) {
    return { ok: false, message: t('clipboard', 'destinationUnchanged') };
  }
  if (sources.some((source) => isSameOrDescendant(source, target))) {
    return { ok: false, message: t('clipboard', 'recursiveDrop') };
  }
  return { ok: true };
}

/** Default is move; Command or Option on macOS and Control elsewhere requests a copy. */
export function operationForDrop(
  platform: RuntimeCapabilities['platform'],
  modifiers: DropModifiers,
): Extract<OperationKind, 'copy' | 'move'> {
  return platform === 'macos'
    ? modifiers.altKey || modifiers.metaKey
      ? 'copy'
      : 'move'
    : modifiers.ctrlKey
      ? 'copy'
      : 'move';
}
