import type { EntryId, EntrySummary } from '../../models';
import { reduceTypeahead, type TypeaheadState } from '../selection/keybindings';
import type { SelectionAction } from '../selection/selection';

export interface TypeaheadController {
  /** The active prefix string, or undefined when no typeahead is in progress. */
  readonly prefix: string | undefined;
  /** True during the brief error-flash after a character finds no match. */
  readonly hasError: boolean;
  /** Resets all state (prefix, timer, error). */
  reset(): void;
  /** Cancels any pending error-fade timer without resetting the prefix. */
  clearTimer(): void;
  /**
   * Processes a single printable character. Returns a selection action when a matching entry is
   * found, or undefined when no match exists (and internally starts the error-flash timer).
   * `extend` (Shift+letter) selects the range from the current anchor through the match, instead
   * of replacing the selection with just the match.
   */
  handleChar(
    char: string,
    entries: readonly EntrySummary[],
    now: number,
    extend?: boolean,
  ): SelectionAction | undefined;
  /**
   * Handles a Backspace key against the active prefix. Returns true if the key was consumed
   * (typeahead was active), false if there was no active typeahead.
   */
  handleBackspace(): boolean;
  /**
   * Moves the cursor within the filtered match set. Returns a SelectionAction if typeahead is
   * active and should constrain movement, undefined if active but no action is needed (e.g. no
   * matches), or false if typeahead is not active (caller should use normal cursor movement).
   */
  moveWithinMatches(
    entries: readonly EntrySummary[],
    cursorEntry: EntrySummary | undefined,
    offset: number,
    edge: 'first' | 'last' | undefined,
    extend: boolean,
  ): SelectionAction | undefined | false;
}

/**
 * Creates a typeahead state machine that manages prefix accumulation, timer-based error
 * feedback, and match-constrained cursor movement.
 *
 * @param onRedraw - Invoked after the error-flash timer clears so the caller can trigger a redraw.
 */
export function createTypeaheadController(onRedraw: () => void): TypeaheadController {
  let _state: TypeaheadState | undefined;
  let _timer: ReturnType<typeof setTimeout> | undefined;
  let _hasError = false;

  function clearTimer(): void {
    if (_timer !== undefined) {
      clearTimeout(_timer);
      _timer = undefined;
    }
  }

  function flashError(): void {
    clearTimer();
    _hasError = true;
    _timer = setTimeout(() => {
      _hasError = false;
      _timer = undefined;
      onRedraw();
    }, 400);
  }

  return {
    get prefix() {
      return _state?.prefix;
    },
    get hasError() {
      return _hasError;
    },
    reset() {
      clearTimer();
      _state = undefined;
      _hasError = false;
    },
    clearTimer,
    handleChar(char, entries, now, extend) {
      const result = reduceTypeahead(_state, char, entries, now, Number.POSITIVE_INFINITY);
      _state = result.state;
      if (result.matchedEntryId !== undefined) {
        clearTimer();
        _hasError = false;
        return extend === true
          ? { type: 'extendRangeTo', entryId: result.matchedEntryId }
          : { type: 'selectOnly', entryId: result.matchedEntryId };
      }
      flashError();
      return undefined;
    },
    handleBackspace() {
      if (_state === undefined) return false;
      clearTimer();
      const prefix = _state.prefix.slice(0, -1);
      _state = prefix.length === 0 ? undefined : { prefix, lastInputAt: _state.lastInputAt };
      _hasError = false;
      return true;
    },
    moveWithinMatches(entries, cursorEntry, offset, edge, extend) {
      if (_state === undefined) return false;
      const prefix = _state.prefix;
      const matches = entries.filter((entry) => entry.name.toLocaleLowerCase().includes(prefix));
      if (matches.length === 0) return undefined;
      const currentMatchIndex = matches.findIndex((e) => e.id === cursorEntry?.id);
      const targetIndex =
        edge === 'first'
          ? 0
          : edge === 'last'
            ? matches.length - 1
            : Math.max(
                0,
                Math.min(
                  (currentMatchIndex < 0 ? (offset < 0 ? matches.length : -1) : currentMatchIndex) +
                    offset,
                  matches.length - 1,
                ),
              );
      const target = matches[targetIndex];
      if (target === undefined) return undefined;
      if (extend && cursorEntry !== undefined) {
        return {
          type: 'extendRangeWithin',
          orderedEntryIds: matches.map((entry) => entry.id),
          offset,
        };
      }
      return { type: 'setCursor', entryId: target.id as EntryId };
    },
  };
}
