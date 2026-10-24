import {
  type AdvancedFilterCriteria,
  EMPTY_ADVANCED_FILTER,
  isAdvancedFilterActive,
} from './advanced-filter';

/**
 * Per-tab advanced-filter UI state: the criteria in force and whether the panel is expanded.
 * Keyed by the same tab key as the quick-filter query; criteria are immutable values so a
 * reference comparison tells caches whether they changed.
 */
export class AdvancedFilterStore {
  readonly #criteria = new Map<string, AdvancedFilterCriteria>();
  readonly #panelOpen = new Map<string, boolean>();

  criteriaFor(key: string): AdvancedFilterCriteria {
    return this.#criteria.get(key) ?? EMPTY_ADVANCED_FILTER;
  }

  isActive(key: string): boolean {
    return isAdvancedFilterActive(this.criteriaFor(key));
  }

  setCriteria(key: string, criteria: AdvancedFilterCriteria): void {
    if (isAdvancedFilterActive(criteria)) this.#criteria.set(key, criteria);
    else this.#criteria.delete(key);
  }

  panelOpen(key: string): boolean {
    return this.#panelOpen.get(key) === true;
  }

  setPanelOpen(key: string, open: boolean): void {
    if (open) this.#panelOpen.set(key, true);
    else this.#panelOpen.delete(key);
  }

  /** Clears the criteria and collapses the panel. */
  clear(key: string): void {
    this.#criteria.delete(key);
    this.#panelOpen.delete(key);
  }

  /** Moves a tab's state when the tab moves to another pane. */
  rekey(from: string, to: string): void {
    const criteria = this.#criteria.get(from);
    const open = this.#panelOpen.get(from);
    this.clear(from);
    if (criteria !== undefined) this.#criteria.set(to, criteria);
    if (open === true) this.#panelOpen.set(to, true);
  }
}
