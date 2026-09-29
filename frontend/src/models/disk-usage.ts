import type { DiskUsageCleanupCandidateDto } from '../api/generated/models/diskUsageCleanupCandidateDto';
import type { DiskUsageNodeDto } from '../api/generated/models/diskUsageNodeDto';
import type { DiskUsageNodeKindDto } from '../api/generated/models/diskUsageNodeKindDto';
import type { DiskUsageTreeDto } from '../api/generated/models/diskUsageTreeDto';
import type { ScanDiskUsageRequestDto } from '../api/generated/models/scanDiskUsageRequestDto';
import type { ScanDiskUsageResponseDto } from '../api/generated/models/scanDiskUsageResponseDto';

export type DiskUsageNode = DiskUsageNodeDto;
export type ScanDiskUsageRequest = ScanDiskUsageRequestDto;
export type ScanDiskUsageResult = ScanDiskUsageResponseDto;
export type DiskUsageCleanupCandidate = DiskUsageCleanupCandidateDto;
/** Compact pre-order wire form of a disk-usage hierarchy (task 0233). */
export type DiskUsageTree = DiskUsageTreeDto;

const KIND_MASK = 0b11;
const COLLAPSED_FLAG = 0b100;
const KINDS: readonly DiskUsageNodeKindDto[] = ['directory', 'file', 'symlink'];

function joinUri(parentUri: string, name: string): string {
  return parentUri.endsWith('/') ? `${parentUri}${name}` : `${parentUri}/${name}`;
}

/** Rebuilds nested nodes from the flat wire form, or `undefined` when the arrays disagree. */
export function decodeDiskUsageTree(tree: DiskUsageTree): DiskUsageNode | undefined {
  const count = tree.names.length;
  if (
    count === 0 ||
    tree.parents.length !== count ||
    tree.flags.length !== count ||
    tree.logicalBytes.length !== count ||
    tree.physicalBytes.length !== count
  ) {
    return undefined;
  }
  const overrides = tree.uriOverrides ?? {};
  const nodes: DiskUsageNode[] = [];
  for (let index = 0; index < count; index += 1) {
    const flags = tree.flags[index] ?? 0;
    const kind = KINDS[flags & KIND_MASK];
    const name = tree.names[index] ?? '';
    const parentIndex = tree.parents[index] ?? 0;
    const parent = index === 0 ? undefined : nodes[parentIndex];
    if (kind === undefined || (index > 0 && (parentIndex >= index || parent === undefined))) {
      return undefined;
    }
    const uri =
      overrides[String(index)] ??
      (parent === undefined ? tree.rootUri : joinUri(parent.location.uri, name));
    const node: DiskUsageNode = {
      name,
      location: { providerId: tree.providerId, uri },
      kind,
      logicalBytes: tree.logicalBytes[index] ?? 0,
      physicalBytes: tree.physicalBytes[index] ?? 0,
      collapsed: (flags & COLLAPSED_FLAG) !== 0,
      children: [],
    };
    nodes.push(node);
    parent?.children.push(node);
  }
  return nodes[0];
}

/** Flattens nested nodes into the wire form; used by the in-process mock backend. */
export function encodeDiskUsageTree(root: DiskUsageNode): DiskUsageTree {
  const tree: DiskUsageTree = {
    providerId: root.location.providerId,
    rootUri: root.location.uri,
    parents: [],
    names: [],
    flags: [],
    logicalBytes: [],
    physicalBytes: [],
    uriOverrides: {},
  };
  const overrides: Record<string, string> = {};
  const stack: { node: DiskUsageNode; parent: number; parentUri?: string }[] = [
    { node: root, parent: 0 },
  ];
  for (let item = stack.pop(); item !== undefined; item = stack.pop()) {
    const { node, parent, parentUri } = item;
    const index = tree.names.length;
    if (parentUri !== undefined && joinUri(parentUri, node.name) !== node.location.uri) {
      overrides[String(index)] = node.location.uri;
    }
    tree.parents.push(parent);
    tree.names.push(node.name);
    tree.flags.push(KINDS.indexOf(node.kind) | (node.collapsed === true ? COLLAPSED_FLAG : 0));
    tree.logicalBytes.push(node.logicalBytes);
    tree.physicalBytes.push(node.physicalBytes);
    for (let child = node.children.length - 1; child >= 0; child -= 1) {
      const childNode = node.children[child];
      if (childNode !== undefined) {
        stack.push({ node: childNode, parent: index, parentUri: node.location.uri });
      }
    }
  }
  return { ...tree, uriOverrides: overrides };
}
