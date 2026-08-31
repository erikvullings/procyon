import type { EntrySummary } from '../../models';

export const GENERATED_DIRECTORY_SIZES = [1_000, 10_000, 100_000, 1_000_000] as const;

export type GeneratedDirectorySize = (typeof GENERATED_DIRECTORY_SIZES)[number];

export interface GeneratedDirectory {
  readonly totalEntries: number;
  page(offset: number, limit: number): EntrySummary[];
  entries(): IterableIterator<EntrySummary>;
}

function seededValue(seed: number, index: number): number {
  let value = (seed ^ Math.imul(index + 1, 0x9e_3779b1)) >>> 0;
  value = Math.imul(value ^ (value >>> 16), 0x21f0_aaad);
  value = Math.imul(value ^ (value >>> 15), 0x735a_2d97);
  return (value ^ (value >>> 15)) >>> 0;
}

function entryAt(seed: number, index: number): EntrySummary {
  const value = seededValue(seed, index);
  const paddedIndex = index.toString().padStart(7, '0');
  const name = `generated-${paddedIndex}-${value.toString(16).padStart(8, '0')}.dat`;

  return {
    id: `generated-${seed}-${index}`,
    location: { providerId: 'file', uri: `mock:///generated/${seed}/${name}` },
    name,
    kind: 'file',
    size: value,
    modifiedAt: new Date(Date.UTC(2025, 0, 1) + (value % 31_536_000) * 1_000).toISOString(),
    hidden: false,
    readOnly: false,
    extension: 'dat',
    mimeType: 'application/octet-stream',
    metadataRevision: 1,
  };
}

/**
 * Creates a random-access, seeded directory source. Only entries requested by
 * `page` or consumed from `entries` are allocated.
 */
export function createGeneratedDirectory(totalEntries: number, seed: number): GeneratedDirectory {
  if (!Number.isSafeInteger(totalEntries) || totalEntries < 0) {
    throw new RangeError('totalEntries must be a non-negative safe integer');
  }
  if (!Number.isSafeInteger(seed)) {
    throw new RangeError('seed must be a safe integer');
  }

  return {
    totalEntries,
    page(offset, limit) {
      if (!Number.isSafeInteger(offset) || offset < 0) {
        throw new RangeError('offset must be a non-negative safe integer');
      }
      if (!Number.isSafeInteger(limit) || limit < 0) {
        throw new RangeError('limit must be a non-negative safe integer');
      }

      const end = Math.min(totalEntries, offset + limit);
      return Array.from({ length: Math.max(0, end - offset) }, (_, relativeIndex) =>
        entryAt(seed, offset + relativeIndex),
      );
    },
    *entries() {
      for (let index = 0; index < totalEntries; index += 1) {
        yield entryAt(seed, index);
      }
    },
  };
}
