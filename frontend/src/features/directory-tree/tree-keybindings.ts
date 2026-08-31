/** Pure keyboard-event interpretation for the directory-tree sidebar (task 0139), mirroring
 * `selection/keybindings.ts`'s `interpretSelectionKey` shape: a DOM-free function mapping a raw
 * key event plus the focused row's own state to a semantic command, matching the standard
 * WAI-ARIA `role="tree"` keyboard pattern (arrow-right expands/descends, arrow-left
 * collapses/ascends). */
export interface TreeKeyEvent {
  readonly key: string;
  readonly shiftKey: boolean;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
}

/** The minimal state of the currently-focused row needed to resolve Arrow Left/Right, since
 * their meaning depends on whether the row is expanded and whether it is known to have
 * children. */
export interface TreeFocusedRowState {
  readonly expanded: boolean;
  /** `undefined` when the row's children have never been fetched. */
  readonly hasChildren: boolean | undefined;
  readonly depth: number;
}

/** Rows moved per Page Up/Down, matching the directory table's own fixed page size
 * (`pane.ts`'s `command.pages * 10`) rather than a viewport-height computation. */
const PAGE_SIZE = 10;

export type TreeKeyCommand =
  | { readonly type: 'moveFocus'; readonly offset: number }
  | { readonly type: 'moveFocusTo'; readonly edge: 'first' | 'last' }
  | { readonly type: 'expand' }
  | { readonly type: 'collapse' }
  | { readonly type: 'moveFocusToParent' }
  | { readonly type: 'moveFocusToFirstChild' }
  | { readonly type: 'activate' }
  /** Tab/Shift+Tab: leaves the tree entirely, e.g. to cycle into the next/previous pane. */
  | { readonly type: 'moveFocusOut'; readonly direction: -1 | 1 };

/** Converts a keyboard event into a semantic tree command without touching the DOM. */
export function interpretTreeKey(
  event: TreeKeyEvent,
  row: TreeFocusedRowState,
): TreeKeyCommand | undefined {
  // Tab/Shift+Tab is handled ahead of the general modifier guard below, since Shift is exactly
  // what distinguishes its two directions.
  if (event.key === 'Tab' && !event.ctrlKey && !event.metaKey && !event.altKey) {
    return { type: 'moveFocusOut', direction: event.shiftKey ? -1 : 1 };
  }
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) {
    return undefined;
  }
  switch (event.key) {
    case 'ArrowDown':
      return { type: 'moveFocus', offset: 1 };
    case 'ArrowUp':
      return { type: 'moveFocus', offset: -1 };
    case 'PageDown':
      return { type: 'moveFocus', offset: PAGE_SIZE };
    case 'PageUp':
      return { type: 'moveFocus', offset: -PAGE_SIZE };
    case 'Home':
      return { type: 'moveFocusTo', edge: 'first' };
    case 'End':
      return { type: 'moveFocusTo', edge: 'last' };
    case 'ArrowRight':
      if (row.hasChildren === false) return undefined;
      return row.expanded ? { type: 'moveFocusToFirstChild' } : { type: 'expand' };
    case 'ArrowLeft':
      if (row.expanded) return { type: 'collapse' };
      return row.depth > 0 ? { type: 'moveFocusToParent' } : undefined;
    case 'Enter':
    case ' ':
      return { type: 'activate' };
    default:
      return undefined;
  }
}
