/**
 * A least-recently-used cache bounded by total weight (for example the characters of the data URIs
 * it holds) rather than by entry count. Reads count as use. Inserting past the budget evicts the
 * least recently used entries first, but never the entry just inserted.
 *
 * Used for caches that would otherwise grow for the whole life of a long-running window.
 */
export class WeightedLru<V> {
  private readonly entries = new Map<string, { readonly value: V; readonly weight: number }>();
  private total = 0;

  constructor(
    private readonly maxWeight: number,
    private readonly weigh: (value: V) => number,
  ) {}

  get size(): number {
    return this.entries.size;
  }

  /** Sum of the weights currently held. */
  get weight(): number {
    return this.total;
  }

  has(key: string): boolean {
    return this.entries.has(key);
  }

  get(key: string): V | undefined {
    const entry = this.entries.get(key);
    if (entry === undefined) return undefined;
    // Re-insert to make this the most recently used entry.
    this.entries.delete(key);
    this.entries.set(key, entry);
    return entry.value;
  }

  set(key: string, value: V): void {
    const previous = this.entries.get(key);
    if (previous !== undefined) {
      this.total -= previous.weight;
      this.entries.delete(key);
    }
    const weight = Math.max(1, this.weigh(value));
    this.entries.set(key, { value, weight });
    this.total += weight;
    for (const [oldest, entry] of this.entries) {
      if (this.total <= this.maxWeight || oldest === key) break;
      this.entries.delete(oldest);
      this.total -= entry.weight;
    }
  }

  values(): IterableIterator<V> {
    return Array.from(this.entries.values(), (entry) => entry.value)[Symbol.iterator]();
  }
}
