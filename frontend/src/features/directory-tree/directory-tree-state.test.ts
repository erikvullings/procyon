import { describe, expect, it } from 'vitest';

import type { EntrySummary, Location } from '../../models';
import {
  ancestorChain,
  createTreeChildrenState,
  flattenVisibleTree,
  withChildren,
  withError,
  withExpanded,
  withLoading,
} from './directory-tree-state';

function location(uri: string): Location {
  return { providerId: 'local', uri };
}

function directoryEntry(uri: string, name: string): EntrySummary {
  return {
    id: uri,
    location: location(uri),
    name,
    kind: 'directory',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
  };
}

describe('directory-tree-state', () => {
  it('starts with nothing expanded or cached', () => {
    const state = createTreeChildrenState();
    expect(state.expanded.size).toBe(0);
    expect(Object.keys(state.childrenByUri)).toEqual([]);
  });

  describe('flattenVisibleTree', () => {
    const root = { location: location('file:///'), name: 'root' };

    it('shows only the root when nothing is expanded', () => {
      const rows = flattenVisibleTree(root, createTreeChildrenState());
      expect(rows).toEqual([
        {
          location: location('file:///'),
          name: 'root',
          depth: 0,
          expanded: false,
          loading: false,
          error: undefined,
          hasChildren: undefined,
        },
      ]);
    });

    it('does not show a node’s children until its own children have been fetched (lazy expansion)', () => {
      // Expanded but never fetched: still just the root row, and the expand fetch has not
      // happened yet from the state's own perspective.
      const state = withExpanded(createTreeChildrenState(), 'file:///', true);
      const rows = flattenVisibleTree(root, state);
      expect(rows).toHaveLength(1);
      expect(rows[0]?.expanded).toBe(true);
      expect(rows[0]?.hasChildren).toBeUndefined();
    });

    it('shows cached children once fetched and the node is expanded', () => {
      const state = withChildren(createTreeChildrenState(), 'file:///', [
        directoryEntry('file:///alpha', 'alpha'),
        directoryEntry('file:///zeta', 'zeta'),
      ]);
      const rows = flattenVisibleTree(root, state);
      expect(rows.map((row) => ({ name: row.name, depth: row.depth }))).toEqual([
        { name: 'root', depth: 0 },
        { name: 'alpha', depth: 1 },
        { name: 'zeta', depth: 1 },
      ]);
      expect(rows[0]?.hasChildren).toBe(true);
    });

    it('recurses into a nested expanded-and-cached grandchild', () => {
      let state = withChildren(createTreeChildrenState(), 'file:///', [
        directoryEntry('file:///alpha', 'alpha'),
      ]);
      state = withChildren(state, 'file:///alpha', [directoryEntry('file:///alpha/beta', 'beta')]);

      const rows = flattenVisibleTree(root, state);
      expect(rows.map((row) => `${row.depth}:${row.name}`)).toEqual([
        '0:root',
        '1:alpha',
        '2:beta',
      ]);
    });

    it('collapsing a node hides its cached children again without discarding the cache', () => {
      let state = withChildren(createTreeChildrenState(), 'file:///', [
        directoryEntry('file:///alpha', 'alpha'),
      ]);
      state = withExpanded(state, 'file:///', false);

      const rows = flattenVisibleTree(root, state);
      expect(rows).toHaveLength(1);
      // Cache survives collapse, so re-expanding does not need to refetch.
      expect(state.childrenByUri['file:///']).toHaveLength(1);
    });

    it('marks an empty fetched directory as having no children', () => {
      const state = withChildren(createTreeChildrenState(), 'file:///', []);
      const rows = flattenVisibleTree(root, state);
      expect(rows[0]?.hasChildren).toBe(false);
    });

    it('reports the loading flag for a node whose fetch is in flight', () => {
      const state = withLoading(createTreeChildrenState(), 'file:///', true);
      const rows = flattenVisibleTree(root, state);
      expect(rows[0]?.loading).toBe(true);
    });

    it('reports a node error and leaves it collapsed', () => {
      let state = withLoading(createTreeChildrenState(), 'file:///', true);
      state = withError(state, 'file:///', 'Permission denied');
      const rows = flattenVisibleTree(root, state);
      expect(rows[0]?.error).toBe('Permission denied');
      expect(rows[0]?.loading).toBe(false);
      expect(rows[0]?.expanded).toBe(false);
    });
  });

  describe('ancestorChain', () => {
    it('is empty when the target is the root itself', () => {
      expect(ancestorChain(location('file:///'), location('file:///'))).toEqual([]);
    });

    it('returns the chain of directories to expand, root-first, excluding the target itself', () => {
      const chain = ancestorChain(location('file:///'), location('file:///a/b/c'));
      expect(chain.map((entry) => entry.uri)).toEqual(['file:///a', 'file:///a/b']);
    });

    it('returns a single-entry chain for a direct child of the root', () => {
      const chain = ancestorChain(location('file:///'), location('file:///a'));
      expect(chain).toEqual([]);
    });

    it('handles a target two levels below the root', () => {
      const chain = ancestorChain(location('file:///'), location('file:///a/b'));
      expect(chain.map((entry) => entry.uri)).toEqual(['file:///a']);
    });

    it('terminates at a provider root reached before the given root, rather than looping', () => {
      // `root` here is never actually reached by walking up from `target` (parentLocation's
      // own fixed point is `file:///`, not `file:///nonexistent-root`), so the walk stops at
      // that fixed point instead and includes it as the topmost known ancestor.
      const chain = ancestorChain(location('file:///nonexistent-root'), location('file:///a/b'));
      expect(chain.map((entry) => entry.uri)).toEqual(['file:///', 'file:///a']);
    });
  });
});
