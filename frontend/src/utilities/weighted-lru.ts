export interface WeightedLruOptions {
  /** Idle time after which an entry is dropped; omitted means entries never expire. */
  readonly maxAgeMs?: number;
  /** Clock in milliseconds; injectable so expiry is testable. */
  readonly now?: () => number;
}

/**
 * A least-recently-used cache bounded by total weight (for example the characters of the data URIs
 * it holds) rather than by entry count. Reads count as use. Inserting past the budget evicts the
 * least recently used entries first, but never the entry just inserted.
 *
 * Optionally an entry also expires after `maxAgeMs` without being read or written, for caches whose
 * values are large and only worth keeping while they are on screen. Expired entries behave as
 * absent and are freed by {@link WeightedLru.prune} (or lazily on access).
 *
 * Used for caches that would otherwise grow for the whole life of a long-running window.
 */
export class WeightedLru<V> {
  private readonly entries = new Map<
    string,
    { readonly value: V; readonly weight: number; readonly usedAt: number }
  >();
  private total = 0;
  private readonly maxAgeMs: number | undefined;
  private readonly now: () => number;

  constructor(
    private readonly maxWeight: number,
    private readonly weigh: (value: V) => number,
    options: WeightedLruOptions = {},
  ) {
    this.maxAgeMs = options.maxAgeMs;
    this.now = options.now ?? (() => Date.now());
  }

  get size(): number {
    return this.entries.size;
  }

  /** Sum of the weights currently held. */
  get weight(): number {
    return this.total;
  }

  has(key: string): boolean {
    this.dropIfExpired(key);
    return this.entries.has(key);
  }

  get(key: string): V | undefined {
    this.dropIfExpired(key);
    const entry = this.entries.get(key);
    if (entry === undefined) return undefined;
    // Re-insert to make this the most recently used entry.
    this.entries.delete(key);
    this.entries.set(key, { ...entry, usedAt: this.now() });
    return entry.value;
  }

  /** Frees every entry idle for longer than `maxAgeMs`. A no-op for a cache without expiry. */
  prune(): void {
    if (this.maxAgeMs === undefined) return;
    for (const key of [...this.entries.keys()]) this.dropIfExpired(key);
  }

  private dropIfExpired(key: string): void {
    if (this.maxAgeMs === undefined) return;
    const entry = this.entries.get(key);
    if (entry === undefined || this.now() - entry.usedAt <= this.maxAgeMs) return;
    this.entries.delete(key);
    this.total -= entry.weight;
  }

  set(key: string, value: V): void {
    const previous = this.entries.get(key);
    if (previous !== undefined) {
      this.total -= previous.weight;
      this.entries.delete(key);
    }
    const weight = Math.max(1, this.weigh(value));
    this.entries.set(key, { value, weight, usedAt: this.now() });
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
