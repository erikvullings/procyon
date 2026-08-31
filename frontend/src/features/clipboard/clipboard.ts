import { t } from '../../i18n';
import type { ClipboardMode, ClipboardState, Location } from '../../models';

export type { ClipboardMode, ClipboardState };

/** Information already known for the visible destination pane. */
export interface PasteTarget {
  readonly location: Location;
  readonly writable: boolean;
  readonly loaded: boolean;
}

export type PasteTargetValidation =
  | { readonly ok: true }
  | { readonly ok: false; readonly message: string };

export const emptyClipboard: ClipboardState = { locations: [] };

function setClipboard(mode: ClipboardMode, locations: readonly Location[]): ClipboardState {
  return locations.length === 0 ? emptyClipboard : { mode, locations: [...locations] };
}

/** Replaces the clipboard with locations scheduled to be copied. */
export function copyToClipboard(
  _clipboard: ClipboardState,
  locations: readonly Location[],
): ClipboardState {
  return setClipboard('copy', locations);
}

/** Replaces the clipboard with locations scheduled to be moved. */
export function cutToClipboard(
  _clipboard: ClipboardState,
  locations: readonly Location[],
): ClipboardState {
  return setClipboard('move', locations);
}

/** Removes every in-application file reference. */
export function clearClipboard(_clipboard: ClipboardState): ClipboardState {
  return emptyClipboard;
}

/** True when a visible entry should be rendered as awaiting a move. */
export function isCutLocation(clipboard: ClipboardState, location: Location): boolean {
  return (
    clipboard.mode === 'move' &&
    clipboard.locations.some(
      (candidate) => candidate.providerId === location.providerId && candidate.uri === location.uri,
    )
  );
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

/** Validates a visible destination before submitting an operation to the engine. */
export function validatePasteTarget(
  clipboard: ClipboardState,
  target: PasteTarget | undefined,
): PasteTargetValidation {
  if (clipboard.mode === undefined || clipboard.locations.length === 0) {
    return { ok: false, message: t('clipboard', 'empty') };
  }
  if (target === undefined || !target.loaded) {
    return { ok: false, message: t('clipboard', 'destinationUnavailable') };
  }
  if (!target.writable) {
    return { ok: false, message: t('clipboard', 'destinationReadOnly') };
  }
  if (clipboard.locations.some((source) => isSameOrDescendant(source, target.location))) {
    return { ok: false, message: t('clipboard', 'recursivePaste') };
  }
  return { ok: true };
}
