import type { ActionId, EntryId, EntrySummary } from '../../models';

/** Action identifiers reserved for selection before the configurable action registry lands. */
export const CORE_SELECTION_ACTION_IDS = {
  moveCursorUp: 'core.moveCursorUp',
  moveCursorDown: 'core.moveCursorDown',
  moveCursorPageUp: 'core.moveCursorPageUp',
  moveCursorPageDown: 'core.moveCursorPageDown',
  moveCursorFirst: 'core.moveCursorFirst',
  moveCursorLast: 'core.moveCursorLast',
  extendSelectionUp: 'core.extendSelectionUp',
  extendSelectionDown: 'core.extendSelectionDown',
  toggleSelection: 'core.toggleSelection',
  selectAll: 'core.selectAll',
  invertSelection: 'core.invertSelection',
  clearSelection: 'core.clearSelection',
} as const satisfies Record<string, ActionId>;

export type SelectionPlatform = 'macos' | 'windows' | 'linux' | 'unknown';

export interface SelectionKeyEvent {
  readonly key: string;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
}

export type SelectionKeyCommand =
  | { readonly type: 'moveCursor'; readonly offset: -1 | 1 }
  | { readonly type: 'moveCursorByPage'; readonly pages: -1 | 1 }
  | { readonly type: 'moveCursorTo'; readonly edge: 'first' | 'last' }
  | { readonly type: 'extendRange'; readonly offset: -1 | 1 }
  | { readonly type: 'toggleCursorSelection' }
  | { readonly type: 'selectAll' }
  | { readonly type: 'open' }
  | { readonly type: 'parent' }
  | { readonly type: 'switchPane'; readonly direction: -1 | 1 };

function primaryModifier(event: SelectionKeyEvent, platform: SelectionPlatform): boolean {
  if (platform === 'macos') {
    return event.metaKey && !event.ctrlKey;
  }
  if (platform === 'windows' || platform === 'linux') {
    return event.ctrlKey && !event.metaKey;
  }
  return event.ctrlKey !== event.metaKey;
}

/** Converts a keyboard event into a semantic directory command without touching the DOM. */
export function interpretSelectionKey(
  event: SelectionKeyEvent,
  platform: SelectionPlatform,
): SelectionKeyCommand | undefined {
  if (
    event.key.toLowerCase() === 'a' &&
    primaryModifier(event, platform) &&
    !event.shiftKey &&
    !event.altKey
  ) {
    return { type: 'selectAll' };
  }
  if (event.ctrlKey || event.metaKey || event.altKey) {
    return undefined;
  }
  if (event.shiftKey && event.key === 'ArrowUp') {
    return { type: 'extendRange', offset: -1 };
  }
  if (event.shiftKey && event.key === 'ArrowDown') {
    return { type: 'extendRange', offset: 1 };
  }
  if (event.key === 'Tab') {
    return { type: 'switchPane', direction: event.shiftKey ? -1 : 1 };
  }
  if (event.shiftKey) {
    return undefined;
  }
  switch (event.key) {
    case 'ArrowUp':
      return { type: 'moveCursor', offset: -1 };
    case 'ArrowDown':
      return { type: 'moveCursor', offset: 1 };
    case 'PageUp':
      return { type: 'moveCursorByPage', pages: -1 };
    case 'PageDown':
      return { type: 'moveCursorByPage', pages: 1 };
    case 'Home':
      return { type: 'moveCursorTo', edge: 'first' };
    case 'End':
      return { type: 'moveCursorTo', edge: 'last' };
    case 'Enter':
      return { type: 'open' };
    case 'Backspace':
      return { type: 'parent' };
    case ' ':
      return { type: 'toggleCursorSelection' };
    default:
      return undefined;
  }
}

export interface TypeaheadState {
  readonly prefix: string;
  readonly lastInputAt: number;
}

export interface TypeaheadResult {
  readonly state: TypeaheadState;
  readonly matchedEntryId?: EntryId;
}

export const TYPEAHEAD_TIMEOUT_MS = 700;

/** Extends or resets a typed prefix and returns the first matching entry. */
export function reduceTypeahead(
  state: TypeaheadState | undefined,
  input: string,
  entries: readonly EntrySummary[],
  now: number,
  timeoutMs = TYPEAHEAD_TIMEOUT_MS,
): TypeaheadResult {
  const normalizedInput = input.toLocaleLowerCase();
  const prefix =
    state === undefined || now - state.lastInputAt > timeoutMs
      ? normalizedInput
      : state.prefix + normalizedInput;
  const match = entries.find((entry) => entry.name.toLocaleLowerCase().includes(prefix));
  return {
    state: { prefix, lastInputAt: now },
    ...(match === undefined ? {} : { matchedEntryId: match.id }),
  };
}
