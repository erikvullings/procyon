import { expect, it } from 'vitest';
import { MockFileManagerClient } from './mock-file-manager-client';

it('mock sync plans honour include and exclude patterns like the Rust host', async () => {
  const client = new MockFileManagerClient();
  const { comparisonId } = await client.startComparison({
    workspaceId: 'workspace-1',
    left: { providerId: 'mock', uri: 'mock:///Documents' },
    right: { providerId: 'mock', uri: 'mock:///Documents/Projects' },
    criteria: 'nameOnly',
  });
  const all = await client.generateSyncPlan(comparisonId, { mode: 'mirrorLeftToRight' });
  expect(all.items.length).toBeGreaterThan(1);
  const first = all.items[0]?.relativePath ?? '';

  const onlyFirst = await client.generateSyncPlan(comparisonId, {
    mode: 'mirrorLeftToRight',
    include: [first],
  });
  expect(onlyFirst.items.length).toBeGreaterThan(0);
  expect(onlyFirst.items.every((item) => item.relativePath.startsWith(first))).toBe(true);

  const withoutFirst = await client.generateSyncPlan(comparisonId, {
    mode: 'mirrorLeftToRight',
    exclude: [first],
  });
  // Excluding a folder removes it and everything below it.
  expect(withoutFirst.items.some((item) => item.relativePath.startsWith(first))).toBe(false);
  expect(withoutFirst.items.length).toBeLessThan(all.items.length);
});
