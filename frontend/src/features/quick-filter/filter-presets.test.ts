import { describe, expect, it } from 'vitest';

import type { FilterPreset } from '../../models/settings';
import { EMPTY_ADVANCED_FILTER } from './advanced-filter';
import {
  deleteFilterPreset,
  findFilterPreset,
  renameFilterPreset,
  saveFilterPreset,
} from './filter-presets';

const docs: FilterPreset = { name: 'Docs', criteria: { ...EMPTY_ADVANCED_FILTER, name: '*.md' } };
const big: FilterPreset = { name: 'Big', criteria: { ...EMPTY_ADVANCED_FILTER, minSize: 1024 } };

describe('filter presets', () => {
  it('saves under a trimmed name and replaces a same-named preset case-insensitively', () => {
    const added = saveFilterPreset([docs], '  Big  ', big.criteria);
    expect(added).toEqual({ ok: true, name: 'Big', presets: [docs, big] });
    const replaced = saveFilterPreset([docs, big], 'docs', {
      ...EMPTY_ADVANCED_FILTER,
      kind: 'file',
    });
    expect(replaced.ok && replaced.presets.map((p) => p.name)).toEqual(['Docs', 'Big']);
    expect(replaced.ok && replaced.presets[0]?.criteria.kind).toBe('file');
  });

  it('copies criteria so later edits cannot mutate the saved preset', () => {
    const criteria = { ...EMPTY_ADVANCED_FILTER };
    const result = saveFilterPreset([], 'x', criteria);
    expect(result.ok && result.presets[0]?.criteria).not.toBe(criteria);
  });

  it('rejects blank names', () => {
    expect(saveFilterPreset([], '   ', big.criteria)).toEqual({ ok: false, reason: 'blank' });
    expect(renameFilterPreset([docs], 'Docs', ' ')).toEqual({ ok: false, reason: 'blank' });
  });

  it('renames while keeping criteria and order, rejecting collisions and unknown names', () => {
    const renamed = renameFilterPreset([docs, big], 'Docs', 'Markdown');
    expect(renamed.ok && renamed.presets.map((p) => p.name)).toEqual(['Markdown', 'Big']);
    expect(renameFilterPreset([docs, big], 'Docs', 'big')).toEqual({
      ok: false,
      reason: 'duplicate',
    });
    expect(renameFilterPreset([docs, big], 'Nope', 'Z')).toEqual({ ok: false, reason: 'missing' });
    expect(renameFilterPreset([docs, big], 'Docs', 'DOCS').ok).toBe(true);
  });

  it('deletes and finds by name', () => {
    expect(deleteFilterPreset([docs, big], 'docs')).toEqual([big]);
    expect(findFilterPreset([docs], 'DOCS')).toBe(docs);
  });
});
