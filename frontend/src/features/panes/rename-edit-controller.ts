import type { EntrySummary } from '../../models';
import { validateDirectoryName } from '../operations/create-directory-dialog';

export interface RenameEditingController {
  /** The entry currently being renamed, or undefined when no rename is in progress. */
  readonly entry: EntrySummary | undefined;
  /** The current draft rename value. */
  readonly value: string;
  /** Validation error for the current draft, or undefined when the value is valid. */
  readonly error: string | undefined;
  /** Opens inline rename for the given entry, pre-filling the draft with its name. */
  open(entry: EntrySummary): void;
  /** Updates the draft value and re-validates it. */
  updateValue(value: string): void;
  /** Cancels the active rename, clearing entry and error state. */
  cancel(): void;
  /**
   * Validates the current draft and, if valid, returns the committed entry and name then clears
   * state. Returns undefined (and sets an error) when validation fails.
   */
  commit(): { entry: EntrySummary; name: string } | undefined;
}

/** Creates a controller managing the lifecycle of a single-entry inline rename. */
export function createRenameEditingController(): RenameEditingController {
  let _entry: EntrySummary | undefined;
  let _value = '';
  let _error: string | undefined;

  return {
    get entry() {
      return _entry;
    },
    get value() {
      return _value;
    },
    get error() {
      return _error;
    },
    open(entry) {
      _entry = entry;
      _value = entry.name;
      _error = undefined;
    },
    updateValue(value) {
      _value = value;
      _error = validateDirectoryName(value);
    },
    cancel() {
      _entry = undefined;
      _error = undefined;
    },
    commit() {
      _error = validateDirectoryName(_value);
      if (_error !== undefined || _entry === undefined) return undefined;
      const entry = _entry;
      _entry = undefined;
      return { entry, name: _value };
    },
  };
}
