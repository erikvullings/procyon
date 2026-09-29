import { describe, expect, it } from 'vitest';
import type { DiskUsageCleanupCandidate } from '../../models';
import { mergeCleanupCandidates } from './cleanup-candidates';

function candidate(uri: string, physicalBytes: number): DiskUsageCleanupCandidate {
  return {
    location: { providerId: 'local', uri },
    kind: 'nodeModules',
    logicalBytes: physicalBytes,
    physicalBytes,
  };
}

describe('mergeCleanupCandidates', () => {
  it('replaces candidates inside the expanded folder and keeps the rest sorted by size', () => {
    const merged = mergeCleanupCandidates(
      [
        candidate('file:///home/a/node_modules', 10),
        candidate('file:///home/big/x/node_modules', 50),
        candidate('file:///home/bigger/node_modules', 30),
      ],
      [candidate('file:///home/big/y/node_modules', 70)],
      { providerId: 'local', uri: 'file:///home/big' },
    );

    expect(merged.map((item) => item.location.uri)).toEqual([
      'file:///home/big/y/node_modules',
      'file:///home/bigger/node_modules',
      'file:///home/a/node_modules',
    ]);
  });
});
