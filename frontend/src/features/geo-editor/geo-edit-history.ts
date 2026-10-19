import type { GeoEditDocument } from './geo-edit-model';

/** Undo/redo over immutable documents; consecutive identical states are not recorded. */
export class GeoEditHistory {
  private readonly past: GeoEditDocument[] = [];
  private future: GeoEditDocument[] = [];

  constructor(
    private current: GeoEditDocument,
    private readonly limit = 200,
  ) {}

  get document(): GeoEditDocument {
    return this.current;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }

  /** Records `next` as the new state and drops any redo steps. */
  push(next: GeoEditDocument): GeoEditDocument {
    if (next === this.current) return this.current;
    this.past.push(this.current);
    if (this.past.length > this.limit) this.past.shift();
    this.current = next;
    this.future = [];
    return this.current;
  }

  undo(): GeoEditDocument {
    const previous = this.past.pop();
    if (previous !== undefined) {
      this.future.push(this.current);
      this.current = previous;
    }
    return this.current;
  }

  redo(): GeoEditDocument {
    const next = this.future.pop();
    if (next !== undefined) {
      this.past.push(this.current);
      this.current = next;
    }
    return this.current;
  }
}
