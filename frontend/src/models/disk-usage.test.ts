import { describe, expect, it } from 'vitest';
import { type DiskUsageNode, decodeDiskUsageTree, encodeDiskUsageTree } from './disk-usage';

function node(
  name: string,
  uri: string,
  kind: DiskUsageNode['kind'],
  children: DiskUsageNode[] = [],
  collapsed = false,
): DiskUsageNode {
  return {
    name,
    location: { providerId: 'local', uri },
    kind,
    logicalBytes: 10,
    physicalBytes: 12,
    collapsed,
    children,
  };
}

const sample = node('root', 'file:///root', 'directory', [
  node('app', 'file:///root/app', 'directory', [
    node('node_modules', 'file:///root/app/node_modules', 'directory', [], true),
    node('my file.txt', 'file:///root/app/my%20file.txt', 'file'),
  ]),
  node('link', 'file:///root/link', 'symlink'),
  node('Small files (3)', 'file:///root', 'file'),
]);

describe('disk-usage tree codec', () => {
  it('matches the backend wire layout and round-trips nested nodes', () => {
    const tree = encodeDiskUsageTree(sample);

    expect(tree.names).toEqual([
      'root',
      'app',
      'node_modules',
      'my file.txt',
      'link',
      'Small files (3)',
    ]);
    expect(tree.parents).toEqual([0, 0, 1, 1, 0, 0]);
    expect(tree.flags).toEqual([0, 0, 4, 1, 2, 1]);
    expect(tree.uriOverrides).toEqual({
      '3': 'file:///root/app/my%20file.txt',
      '5': 'file:///root',
    });
    expect(decodeDiskUsageTree(tree)).toEqual(sample);
  });

  it('joins names onto a root URI that already ends with a slash', () => {
    const root = node('/', 'file:///', 'directory', [node('Users', 'file:///Users', 'directory')]);

    const tree = encodeDiskUsageTree(root);

    expect(tree.uriOverrides).toEqual({});
    expect(decodeDiskUsageTree(tree)).toEqual(root);
  });

  it('rejects arrays that disagree or reference a later parent', () => {
    const tree = encodeDiskUsageTree(sample);

    expect(decodeDiskUsageTree({ ...tree, flags: tree.flags.slice(1) })).toBeUndefined();
    expect(decodeDiskUsageTree({ ...tree, parents: [0, 0, 4, 1, 0, 0] })).toBeUndefined();
    expect(decodeDiskUsageTree({ ...tree, flags: [3, 0, 4, 1, 2, 1] })).toBeUndefined();
  });
});
